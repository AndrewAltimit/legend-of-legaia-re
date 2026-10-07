use crate::world::{FIELD_OFFMAP_HIDE_XZ, World};

#[test]
fn restore_hidden_field_npcs_unparks_only_the_hide_box() {
    let mut w = World::default();
    // Two townsfolk parked off-map by the opening cutscene, one NPC left at
    // a real tile (e.g. a mid-scene walker), plus stale headings for all.
    let hide = FIELD_OFFMAP_HIDE_XZ;
    w.npcs.positions.insert(1, (hide, hide));
    w.npcs.positions.insert(2, (hide, hide));
    w.npcs.positions.insert(3, (2880, 5440));
    w.npcs.headings.insert(1, 0x800);
    w.npcs.headings.insert(2, 0x000);
    w.npcs.headings.insert(3, 0x400);

    w.restore_hidden_field_npcs();

    // The hide-box NPCs lose their overrides (render falls back to the MAN
    // spawn); the on-tile NPC and its heading are untouched.
    assert!(!w.npcs.positions.contains_key(&1));
    assert!(!w.npcs.positions.contains_key(&2));
    assert!(!w.npcs.headings.contains_key(&1));
    assert!(!w.npcs.headings.contains_key(&2));
    assert_eq!(w.npcs.positions.get(&3), Some(&(2880, 5440)));
    assert_eq!(w.npcs.headings.get(&3), Some(&0x400));
}

#[test]
fn restore_hidden_field_npcs_noop_when_none_parked() {
    let mut w = World::default();
    w.npcs.positions.insert(5, (1000, 2000));
    w.restore_hidden_field_npcs();
    assert_eq!(w.npcs.positions.get(&5), Some(&(1000, 2000)));
}

#[test]
fn scene_color_grade_only_on_the_prologue_cutscene() {
    let mut w = World::new();
    // No scene / arbitrary field scene -> ungraded natural colour.
    assert!(w.scene_color_grade().is_none());
    w.set_active_scene_label("town01");
    assert!(
        w.scene_color_grade().is_none(),
        "the Rim Elm hand-off renders in full colour"
    );
    // The opdeene prologue cutscene renders through the warm sepia grade.
    w.set_active_scene_label(legaia_asset::new_game::OPENING_CUTSCENE_SCENE);
    assert_eq!(
        w.scene_color_grade(),
        Some(crate::fade::ColorGrade::PROLOGUE_SEPIA)
    );
}

#[test]
fn scene_depth_cue_tracks_the_prologue_grade_gate() {
    let mut w = World::new();
    // Interactive scenes stage NO depth-cue ramp: the renderer's ramp-off
    // path is the pre-ramp identity, so town01 pixels are unchanged.
    assert!(w.scene_depth_cue().is_none());
    w.set_active_scene_label("town01");
    assert!(
        w.scene_depth_cue().is_none(),
        "interactive field renders without the far-colour pull"
    );
    // All three prologue legs pull toward the gold far colour.
    for scene in ["opdeene", "opstati", "opurud"] {
        w.set_active_scene_label(scene);
        assert_eq!(
            w.scene_depth_cue(),
            Some(crate::fade::DepthCueRamp::PROLOGUE_GOLD),
            "{scene} stages the prologue depth-cue ramp"
        );
    }
}

/// Build a minimal timeline that reaches a cross-context CFLAG_TST
/// (`B3 05 03` = op 0x33 extended, target channel id 5, completion bit 3),
/// then a WAIT_FRAMES so the timeline stays installed once it resumes past
/// the flag-test. A single spawned channel (script id 5) provides the
/// cross-context target whose flag the timeline waits on.
fn timeline_with_channel_wait(channel_flag_bit3: bool) -> World {
    use crate::cutscene_timeline::CutsceneTimeline;
    use crate::field_channels::FieldChannel;
    use legaia_engine_vm::field::FieldCtx;

    let mut w = World::new();
    // `B3 05 03` (3 bytes) then `4A FF 7F` (WAIT_FRAMES target 0x7FFF).
    let bc = vec![0xB3, 0x05, 0x03, 0x4A, 0xFF, 0x7F];
    w.cutscene.timeline = Some(CutsceneTimeline::new(bc, 0));
    let mut ctx = FieldCtx {
        script_id: 5,
        ..FieldCtx::default()
    };
    if channel_flag_bit3 {
        ctx.flags |= 1 << 3;
    }
    w.field_vm.channels = vec![FieldChannel {
        placement_index: 5,
        ctx,
        record_offset: 0,
        pc: 0,
        done: false,
        object_bind: false,
    }];
    w
}

/// Retail's op-`0x33` arm holds the PC while the target's bit is SET and
/// advances once it is clear (`0x801DEE2C..0x801DEE54`): the timeline
/// waits on a channel's busy bit dropping, not on a completion bit rising.
#[test]
fn cutscene_timeline_parks_on_channel_wait_until_flag_clears() {
    // The timeline reaches `B3 05 03` with the channel's bit 3 up: it
    // PARKS on the cross-context CFLAG_TST rather than stepping past.
    let mut w = timeline_with_channel_wait(true);
    w.step_cutscene_timeline();
    {
        let tl = w
            .cutscene
            .timeline
            .as_ref()
            .expect("timeline still installed");
        assert!(tl.channel_wait.is_some(), "parks on the channel handshake");
        assert_eq!(tl.pc, 0, "PC held on the flag-test op while parked");
        assert!(!tl.is_done());
    }
    for _ in 0..5 {
        w.step_cutscene_timeline();
    }
    {
        let tl = w.cutscene.timeline.as_ref().unwrap();
        assert!(
            tl.channel_wait.is_some(),
            "stays parked while the bit is up"
        );
        assert_eq!(tl.pc, 0);
    }
    // The awaited channel drops the bit: the very next step resolves the
    // park and resumes PAST the 3-byte flag-test op.
    w.field_vm.channels[0].ctx.flags &= !(1 << 3);
    w.step_cutscene_timeline();
    let tl = w
        .cutscene
        .timeline
        .as_ref()
        .expect("timeline still installed");
    assert!(tl.channel_wait.is_none(), "resumes once the bit is clear");
    assert_eq!(
        tl.pc, 3,
        "PC advanced past the CFLAG_TST op onto WAIT_FRAMES"
    );
}

/// A clear bit never parks: the arm takes the advanced PC at once.
#[test]
fn cutscene_timeline_channel_test_on_a_clear_bit_runs_straight_through() {
    let mut w = timeline_with_channel_wait(false);
    w.step_cutscene_timeline();
    let tl = w
        .cutscene
        .timeline
        .as_ref()
        .expect("timeline still installed");
    assert!(tl.channel_wait.is_none());
    assert_eq!(tl.pc, 3);
}

#[test]
fn cutscene_timeline_channel_wait_times_out_to_step_past() {
    // Safety net: a channel that never drops the bit must not stall the
    // timeline forever - after the park timeout it falls back to the
    // by-width step-past (the pre-handshake behaviour).
    let mut w = timeline_with_channel_wait(true);
    // First step parks; then it stays parked for CHANNEL_WAIT_PARK_TIMEOUT
    // frames, then steps past on the frame the budget is exhausted.
    let cap = crate::world::CHANNEL_WAIT_PARK_TIMEOUT;
    let mut resumed = false;
    for _ in 0..(cap + 4) {
        w.step_cutscene_timeline();
        if w.cutscene
            .timeline
            .as_ref()
            .is_some_and(|tl| tl.channel_wait.is_none() && tl.pc == 3)
        {
            resumed = true;
            break;
        }
    }
    assert!(
        resumed,
        "the park times out and the timeline steps past the flag-test"
    );
}

/// Build a timeline whose record turns NPC channel 5 with a cross-context
/// op-0x38 facing op (`B8 05 <op0> <op1>`), then WAIT_FRAMES so the
/// timeline stays installed. The NPC starts posed at engine heading
/// `start` (slot 5 in the render-heading map).
fn timeline_with_npc_facing_op(op0: u8, op1: u8, start: i16) -> World {
    use crate::cutscene_timeline::CutsceneTimeline;
    use crate::field_channels::FieldChannel;
    use legaia_engine_vm::field::FieldCtx;

    let mut w = World::new();
    let bc = vec![0xB8, 0x05, op0, op1, 0x4A, 0xFF, 0x7F];
    w.cutscene.timeline = Some(CutsceneTimeline::new(bc, 0));
    w.field_vm.channels = vec![FieldChannel {
        placement_index: 5,
        ctx: FieldCtx {
            script_id: 5,
            ..FieldCtx::default()
        },
        record_offset: 0,
        pc: 0,
        done: false,
        object_bind: false,
    }];
    w.npcs.positions.insert(5, (1000, 1000));
    w.npcs.headings.insert(5, start);
    w
}

/// A budgeted cross-context `0x38` against an NPC channel plays out as
/// the retail rotate leg: a **linear ramp at the op's own `arc / budget`
/// rate**, holding raw pre-unwrap headings across the `0x1000` wrap, and
/// snapping exactly onto the compass entry on the terminal frame - one
/// tick per budget frame (the Mei dinner beat's `B8 46 <dir> <budget>`
/// turns, runtime-pinned frame-exact off the static recomp).
///
/// The record does **not** wait for it: the arm advances by 3 for every
/// target (`li s7,3` in the delay slot at `0x801DEEFC`) and parks the
/// caller only for the player, so the record is already on its next op
/// while the NPC turns.
#[test]
fn cutscene_timeline_npc_facing_ramp_plays_out_linearly() {
    // LUT index 6 (engine 0x400) over budget 0x12 = 18 frames, from
    // engine 0xE00: arc = 0x600, increasing, crossing the wrap.
    let mut w = timeline_with_npc_facing_op(0x06, 0x12, 0xE00);
    w.step_cutscene_timeline(); // reaches the op, arms the rotate leg
    {
        let tl = w.cutscene.timeline.as_ref().expect("timeline installed");
        assert!(
            tl.facing_wait.is_none(),
            "an NPC turn never parks the record"
        );
        assert_eq!(tl.npc_facings.len(), 1, "the turn runs as an NPC leg");
        assert_eq!(tl.pc, 4, "the record ran on to the op after the turn");
    }
    let mut headings = Vec::new();
    for _ in 0..18 {
        assert!(
            w.cutscene
                .timeline
                .as_ref()
                .is_some_and(|tl| !tl.npc_facings.is_empty()),
            "the leg runs for the op's whole frame budget"
        );
        w.step_cutscene_timeline();
        headings.push(*w.npcs.headings.get(&5).expect("heading written"));
    }
    // Linear at arc/budget = 0x600/18 = 85 units/frame (floor-divide
    // pattern 85 85 85 86 ... as the live arc feeds back), raw values
    // held past 0xFFF mid-ramp, exact compass snap on the last frame.
    assert_eq!(headings.first(), Some(&0x0E55), "first tick steps +85");
    assert!(
        headings.iter().any(|&h| !(0..=0xFFF).contains(&(h as i32))),
        "mid-ramp headings hold the raw pre-unwrap value across the wrap"
    );
    assert_eq!(
        headings.last(),
        Some(&0x0400),
        "terminal frame snaps exactly onto the compass entry"
    );
    let tl = w.cutscene.timeline.as_ref().expect("timeline installed");
    assert!(tl.npc_facings.is_empty(), "ramp done: leg released");
}

/// The simple path (`op1 & 0x7F == 0`) is retail's instant compass write
/// into the target's `+0x26` - no park, no ramp.
#[test]
fn cutscene_timeline_npc_facing_simple_path_snaps_instantly() {
    let mut w = timeline_with_npc_facing_op(0x02, 0x00, 0x000);
    w.step_cutscene_timeline();
    assert_eq!(
        w.npcs.headings.get(&5),
        Some(&0x0C00),
        "LUT index 2 (-X) written outright"
    );
    let tl = w.cutscene.timeline.as_ref().expect("timeline installed");
    assert!(tl.facing_wait.is_none(), "no park on the simple path");
}

/// A timeline whose record turns the **player** (`B8 F8 <op0> <op1>`,
/// `0xF8` resolving to the player object), then waits so it stays
/// installed. The player starts at engine heading `start`.
fn timeline_with_player_facing_op(op0: u8, op1: u8, start: i16) -> World {
    use crate::cutscene_timeline::CutsceneTimeline;
    let mut w = World {
        mode: crate::world::SceneMode::Field,
        ..World::default()
    };
    w.spawn_actor(0);
    w.player_actor_slot = Some(0);
    w.actors[0].move_state.render_26 = start;
    let bc = vec![0xB8, 0xF8, op0, op1, 0x4A, 0xFF, 0x7F];
    w.cutscene.timeline = Some(CutsceneTimeline::new(bc, 0));
    w
}

/// `B8 F8 <dir> 00` snaps the player to the compass entry - the story
/// beats' "Vahn turns to face ..." - rather than writing the timeline's
/// own context, where no renderer reads it.
#[test]
fn cutscene_timeline_player_facing_simple_path_snaps_the_player() {
    let mut w = timeline_with_player_facing_op(0x02, 0x00, 0x000);
    w.step_cutscene_timeline();
    assert_eq!(w.actors[0].move_state.render_26, 0x0C00, "LUT index 2 (-X)");
    let tl = w.cutscene.timeline.as_ref().expect("timeline installed");
    assert!(tl.facing_wait.is_none(), "no park on the simple path");
}

/// The budgeted player turn ramps on the same rotate leg an NPC's does,
/// one parked tick per budget frame, ending exactly on the compass entry.
#[test]
fn cutscene_timeline_player_facing_ramp_turns_the_player() {
    let mut w = timeline_with_player_facing_op(0x06, 0x12, 0xE00);
    w.step_cutscene_timeline();
    let parked = |w: &World| {
        w.cutscene
            .timeline
            .as_ref()
            .and_then(|tl| tl.facing_wait.as_ref())
            .map(|f| f.slot)
    };
    assert_eq!(parked(&w), Some(None), "parked on the player's rotate leg");
    for _ in 0..18 {
        w.step_cutscene_timeline();
    }
    assert_eq!(w.actors[0].move_state.render_26, 0x0400);
    let tl = w.cutscene.timeline.as_ref().expect("timeline installed");
    assert!(tl.facing_wait.is_none(), "ramp done: park released");
    assert_eq!(tl.pc, 4, "record resumed past the 4-byte yield op");
}

/// A halt-acquire of the player (`CC F8 85 <lo> <hi> <id>`) turns the
/// player toward the actor the bind names - the walk kernel's FaceTarget
/// leg - and parks the record until the turn's terminal frame. `jouine`
/// `P2[5]` turns Vahn toward Cort this way (`CC F8 85 0A 00 17`); the
/// retail capture holds the player's `+0x26` on `atan2` of the offset
/// plus the half-turn, engine `0x23B` for this geometry.
#[test]
fn cutscene_timeline_player_halt_acquire_turns_the_player_to_its_bind() {
    use crate::cutscene_timeline::CutsceneTimeline;
    use crate::field_channels::FieldChannel;
    use legaia_engine_vm::field::FieldCtx;
    let mut w = World {
        mode: crate::world::SceneMode::Field,
        ..World::default()
    };
    w.spawn_actor(0);
    w.player_actor_slot = Some(0);
    w.actors[0].move_state.world_x = 2368;
    w.actors[0].move_state.world_z = 2496;
    w.actors[0].move_state.render_26 = 0x800;
    w.field_vm.channels = vec![FieldChannel {
        placement_index: 4,
        ctx: FieldCtx {
            script_id: 0x17,
            ..FieldCtx::default()
        },
        record_offset: 0,
        pc: 0,
        done: false,
        object_bind: false,
    }];
    w.npcs.positions.insert(4, (3136, 3136));
    let bc = vec![0xCC, 0xF8, 0x85, 0x0A, 0x00, 0x17, 0x4A, 0xFF, 0x7F];
    w.cutscene.timeline = Some(CutsceneTimeline::new(bc, 0));
    w.step_cutscene_timeline();
    let parked = |w: &World| {
        w.cutscene
            .timeline
            .as_ref()
            .is_some_and(|tl| tl.player_face.is_some())
    };
    assert!(parked(&w), "the record waits on the turn");
    for _ in 0..12 {
        w.step_cutscene_timeline();
    }
    assert!(!parked(&w), "the turn's terminal frame released the park");
    assert_eq!(w.actors[0].move_state.render_26, 0x23B);
    let tl = w.cutscene.timeline.as_ref().expect("timeline installed");
    assert_eq!(tl.pc, 6, "record resumed past the 6-byte acquire");
}

/// A halt-acquire of an **NPC** (`CC <id> 85 <lo> <hi> F8`) turns that
/// placement toward the bind - here the player, the cutscenes'
/// "Noa turns to Vahn" - over the op's budget, while the record runs on
/// past the op (only a player target halts the caller); the record's
/// next op on the NPC waits for the turn. The geometry is the jouine
/// capture's mirrored, so the NPC lands a half-turn from the player's
/// `0x23B`.
#[test]
fn cutscene_timeline_npc_halt_acquire_turns_the_npc_to_its_bind() {
    use crate::cutscene_timeline::CutsceneTimeline;
    use crate::field_channels::FieldChannel;
    use legaia_engine_vm::field::FieldCtx;
    let mut w = World {
        mode: crate::world::SceneMode::Field,
        ..World::default()
    };
    w.spawn_actor(0);
    w.player_actor_slot = Some(0);
    w.actors[0].move_state.world_x = 2368;
    w.actors[0].move_state.world_z = 2496;
    w.actors[0].move_state.render_26 = 0x800;
    w.field_vm.channels = vec![FieldChannel {
        placement_index: 4,
        ctx: FieldCtx {
            script_id: 0x26,
            ..FieldCtx::default()
        },
        record_offset: 0,
        pc: 0,
        done: false,
        object_bind: false,
    }];
    w.npcs.positions.insert(4, (3136, 3136));
    // Turn, then a second op on the same NPC (a `B1 26 18` bit clear),
    // then a long wait.
    let bc = vec![
        0xCC, 0x26, 0x85, 0x0A, 0x00, 0xF8, 0xB1, 0x26, 0x18, 0x4A, 0xFF, 0x7F,
    ];
    w.cutscene.timeline = Some(CutsceneTimeline::new(bc, 0));
    w.step_cutscene_timeline();
    {
        let tl = w.cutscene.timeline.as_ref().expect("timeline installed");
        assert_eq!(tl.npc_faces.len(), 1, "the turn runs as an NPC leg");
        assert_eq!(tl.pc, 6, "the record ran on to the next op, held on it");
        assert!(tl.player_face.is_none(), "the player is not turned");
    }
    assert_eq!(w.actors[0].move_state.render_26, 0x800, "player untouched");
    for _ in 0..12 {
        w.step_cutscene_timeline();
    }
    let tl = w.cutscene.timeline.as_ref().expect("timeline installed");
    assert!(
        tl.npc_faces.is_empty(),
        "the leg's terminal frame released it"
    );
    assert!(tl.pc > 6, "the held op ran once the turn landed");
    let h = i32::from(*w.npcs.headings.get(&4).expect("heading written"));
    let d = (h - (0x23B + 0x800)).rem_euclid(0x1000);
    assert!(d.min(0x1000 - d) <= 1, "NPC faces the player: {h:#x}");
}

/// `CC <dst> E3 F8` seats placement `<dst>` on the player's spot with
/// the player's heading (`stone` `P2[6]`'s `CC 09 E3 F8` / `CC 0A E3 F8`
/// bring Noa and Gala in on Vahn), and `CC F8 E3 <src>` the player on
/// another actor's.
#[test]
fn cutscene_timeline_position_copy_carries_the_heading() {
    use crate::cutscene_timeline::CutsceneTimeline;
    use crate::field_channels::FieldChannel;
    use legaia_engine_vm::field::FieldCtx;
    let mut w = World {
        mode: crate::world::SceneMode::Field,
        ..World::default()
    };
    w.spawn_actor(0);
    w.player_actor_slot = Some(0);
    w.actors[0].move_state.world_x = 2368;
    w.actors[0].move_state.world_z = 2496;
    w.actors[0].move_state.render_26 = 0x123;
    w.field_vm.channels = vec![
        FieldChannel {
            placement_index: 4,
            ctx: FieldCtx {
                script_id: 0x09,
                ..FieldCtx::default()
            },
            record_offset: 0,
            pc: 0,
            done: false,
            object_bind: false,
        },
        FieldChannel {
            placement_index: 5,
            ctx: FieldCtx {
                script_id: 0x0A,
                ..FieldCtx::default()
            },
            record_offset: 0,
            pc: 0,
            done: false,
            object_bind: false,
        },
    ];
    w.npcs.positions.insert(4, (100, 100));
    w.npcs.positions.insert(5, (3000, 3100));
    w.npcs.headings.insert(5, 0x456);
    let bc = vec![
        0xCC, 0x09, 0xE3, 0xF8, 0xCC, 0xF8, 0xE3, 0x0A, 0x4A, 0xFF, 0x7F,
    ];
    w.cutscene.timeline = Some(CutsceneTimeline::new(bc, 0));
    w.step_cutscene_timeline();
    assert_eq!(w.npcs.positions.get(&4), Some(&(2368, 2496)));
    assert_eq!(w.npcs.headings.get(&4), Some(&0x123));
    let p = &w.actors[0].move_state;
    assert_eq!((p.world_x, p.world_z, p.render_26), (3000, 3100, 0x456));
}

/// A placement's spawn section that turns its own actor toward another
/// (`4C 85 00 00 16` in `rikuroa` `P1[3]`, Noa facing the placement she
/// is seated beside) arms the walk kernel's FaceTarget leg on that
/// placement, which the next field tick lands.
#[test]
fn a_spawn_prologue_own_face_at_turns_the_placement() {
    use crate::field_channels::FieldChannel;
    use legaia_engine_vm::field::FieldCtx;
    let mut w = World {
        mode: crate::world::SceneMode::Field,
        ..World::default()
    };
    // Record 0 at offset 0: `25`, `4C 85 00 00 16`, `21`; record 1 at 7.
    let man = vec![0x25, 0x4C, 0x85, 0x00, 0x00, 0x16, 0x21, 0x25, 0x21];
    let ch = |slot: usize, id: u16, off: usize| FieldChannel {
        placement_index: slot,
        ctx: FieldCtx {
            script_id: id,
            ..FieldCtx::default()
        },
        record_offset: off,
        pc: 0,
        done: false,
        object_bind: false,
    };
    w.field_vm.channels = vec![ch(3, 0x10, 0), ch(9, 0x16, 7)];
    w.field_vm.channels_man = Some(std::sync::Arc::new(man));
    w.npcs.positions.insert(3, (8896, 9664));
    w.npcs.positions.insert(9, (8896, 10664));
    w.pre_run_field_channel_prologues();
    assert!(w.npcs.face_legs.contains_key(&3), "the leg is armed");
    w.tick_field_npc_motions();
    // Straight down +Z: engine heading 0.
    assert_eq!(w.npcs.headings.get(&3), Some(&0));
    assert!(w.npcs.face_legs.is_empty());
}

/// A player ExecMove queues the scene-record one-shot only when its pick
/// binds the scene bank. With the party-bank bit up the id strides into
/// the leader's own bank; a scene record there is another skeleton's
/// clip, and drawing it over the hero tore the body apart.
#[test]
fn a_player_exec_move_queues_a_scene_clip_only_off_the_party_bank() {
    use crate::cutscene_timeline::CutsceneTimeline;
    for (bit_op, queued) in [(0xB1u8, Vec::<u8>::new()), (0xB2, vec![5])] {
        let mut w = World {
            mode: crate::world::SceneMode::Field,
            ..World::default()
        };
        w.spawn_actor(0);
        w.player_actor_slot = Some(0);
        let bc = vec![bit_op, 0xF8, 0x18, 0xA2, 0xF8, 0x05, 0x4A, 0xFF, 0x7F];
        w.cutscene.timeline = Some(CutsceneTimeline::new(bc, 0));
        w.step_cutscene_timeline();
        assert_eq!(w.locomotion.player_move_cues, queued, "{bit_op:#X}");
    }
}

/// Build a timeline whose record drives the **player-anchor channel**
/// (`0xF8`) with the jou castle-door shape: `A2 F8 06` ExecMove, the
/// nine-byte `C3 F8 00 …` halt-acquire (its operand `s16`s are the walk
/// dispatcher's arguments, not a resume PC), then the trailing `0x3F`
/// scene change to `jouina` and the record's terminal backward-jump park.
fn timeline_with_player_channel_door() -> World {
    use crate::cutscene_timeline::CutsceneTimeline;
    let mut w = World::new();
    let mut bc = vec![
        0xA2, 0xF8, 0x06, // ExecMove move_id=6 against the player anchor
        0xC3, 0xF8, 0x00, 0x5E, 0xE2, 0x00, 0x00, 0x1E, 0x00, // halt-acquire sub-0 (9 bytes)
    ];
    // `0x3F` SceneChange -> "jouina", entry (0x84, 0x14), dir 0.
    bc.extend_from_slice(&[
        0x3F, 0x8F, 0x02, 0x06, b'j', b'o', b'u', b'i', b'n', b'a', 0x84, 0x14, 0x00,
    ]);
    bc.extend_from_slice(&[0x21, 0x26, 0xFE, 0xFF]); // Nop + JmpRel-to-self park
    w.cutscene.timeline = Some(CutsceneTimeline::new(bc, 0));
    w
}

#[test]
fn cutscene_timeline_player_channel_door_reaches_scene_change() {
    // The player-channel completion model: the `A2 F8` ExecMove arms the
    // in-flight countdown (emitting the move event), the `C3 F8`
    // halt-acquire PARKS instead of taking its backward resume yield,
    // and once the countdown drains the record runs its trailing `0x3F`.
    // Regression shape: the pre-model stepper took the backward yield
    // and spun `pc 0 -> 3` until the frame cap, never firing the scene
    // change.
    let mut w = timeline_with_player_channel_door();
    w.step_cutscene_timeline();
    {
        let tl = w
            .cutscene
            .timeline
            .as_ref()
            .expect("timeline still installed");
        assert!(
            tl.player_wait.is_some(),
            "parks at the player-channel halt-acquire"
        );
        assert_eq!(tl.pc, 3, "PC held on the halt-acquire op while parked");
        assert_eq!(
            tl.player_move_frames,
            crate::world::CHANNEL_WAIT_PARK_TIMEOUT,
            "the ExecMove armed the in-flight countdown"
        );
    }
    assert!(
        w.pending_field_events
            .iter()
            .any(|e| matches!(e, crate::field_events::FieldEvent::ExecMove { move_id: 6 })),
        "the player-channel ExecMove emits the move event"
    );
    // The park drains over the countdown, then the trailing `0x3F` fires
    // and the record's backward-jump park completes the timeline - well
    // inside the frame cap.
    let cap = crate::world::CHANNEL_WAIT_PARK_TIMEOUT;
    let mut ticks = 0;
    while w.cutscene.timeline.is_some() && ticks < cap + 8 {
        w.step_cutscene_timeline();
        ticks += 1;
    }
    assert_eq!(
        w.pending_named_scene_transition
            .as_ref()
            .map(|(n, ..)| n.as_str()),
        Some("jouina"),
        "the trailing 0x3F scene change fired"
    );
    assert!(
        w.cutscene.timeline.is_none(),
        "the timeline completed without hitting the frame cap"
    );
    assert!(
        ticks <= cap + 4,
        "completion took {ticks} ticks - the park drained, not the frame cap"
    );
}

#[test]
fn cutscene_timeline_player_halt_acquire_without_move_steps_past() {
    // A player-channel halt-acquire with NO move in flight completes
    // immediately: no park, PC steps past by the encoded width onto the
    // next op.
    use crate::cutscene_timeline::CutsceneTimeline;
    let mut w = World::new();
    // Nine bytes: extended header (2) + sub + tile x + tile z + two `s16`
    // walk-dispatcher arguments - retail `overlay_0897` `0x801DF5B8`
    // (`addiu s8,s8,8`) over an `s8` already advanced past the channel
    // byte at `0x801DE948`.
    let bc = vec![
        0xC3, 0xF8, 0x00, 0x5E, 0xE2, 0x00, 0x00, 0x00, 0x00, // halt-acquire sub-0
        0x4A, 0xFF, 0x7F, // WAIT_FRAMES target 0x7FFF (keeps it installed)
    ];
    w.cutscene.timeline = Some(CutsceneTimeline::new(bc, 0));
    w.step_cutscene_timeline();
    let tl = w
        .cutscene
        .timeline
        .as_ref()
        .expect("timeline still installed");
    assert!(tl.player_wait.is_none(), "no park without a move in flight");
    assert_eq!(
        tl.pc, 9,
        "stepped past the 9-byte halt-acquire onto WAIT_FRAMES"
    );
}

/// Build a minimal MAN whose partition 2 carries the given record
/// bodies (each already in the named-record shape from [`p2_record`]).
/// `n0` / `n1` fill the partition-0/1 counts so the op-`0x44` global
/// re-base (`global - N0 - N1`) is exercised.
fn man_with_p2_records(
    records: &[Vec<u8>],
    n0: i16,
    n1: i16,
) -> (legaia_asset::man_section::ManFile, Vec<u8>) {
    use legaia_asset::man_section::{ManFile, ManHeader, SectionRef};
    let data_region_offset = 0x40usize;
    let mut man = vec![0u8; data_region_offset];
    let mut offsets = Vec::new();
    for body in records {
        offsets.push((man.len() - data_region_offset) as u32);
        man.extend_from_slice(body);
    }
    let header = ManHeader {
        status_flags: 0,
        low_flag: false,
        depth_lut: [0; 16],
        partition_counts: [n0, n1, records.len() as i16],
        u24_at_28: 0,
    };
    let man_file = ManFile {
        header,
        partitions: [vec![], vec![], offsets],
        data_region_offset,
        sections: std::array::from_fn(|_| SectionRef {
            offset: man.len(),
            length: 0,
        }),
    };
    (man_file, man)
}

/// A partition-2 named-record body: 1-char name, empty C0, the given C1
/// story-flag OR-gate, empty C2, then `script`.
fn p2_record(c1: &[u16], script: &[u8]) -> Vec<u8> {
    let mut r = vec![1u8, 0x41, 0x00]; // name_len=1 + 2 SJIS name bytes
    r.push(0); // C0 empty
    r.push(c1.len() as u8);
    for f in c1 {
        r.extend_from_slice(&f.to_le_bytes());
    }
    r.push(0); // C2 empty
    r.extend_from_slice(script);
    r
}

#[test]
fn helper_record_installs_and_executes_without_modal_attributes() {
    // A mid-play spawned record (GFLAG_SET 26 then run-off-end) installs
    // as a concurrent helper context: it executes its script by the next
    // frame slice and never touches the modal timeline slot.
    let (mf, man) = man_with_p2_records(&[p2_record(&[], &[0x2E, 0x1A])], 0, 0);
    let mut w = World::new();
    assert!(w.install_helper_record(&mf, &man, 0));
    assert_eq!(w.field_vm.helper_contexts.len(), 1);
    assert!(
        !w.cutscene_timeline_active(),
        "a helper spawn never installs the modal cutscene timeline"
    );
    w.step_helper_contexts();
    assert_ne!(
        w.flags.story_flags & crate::world::PROLOGUE_HANDOFF_FLAG,
        0,
        "the helper record's GFLAG_SET executed"
    );
    assert!(
        w.field_vm.helper_contexts.is_empty(),
        "a completed helper context is dropped from the table"
    );
}

#[test]
fn helper_record_honors_c1_one_shot_gate() {
    // C1 blocks the spawn when ANY listed flag is set (the one-shot
    // latch) - the same FUN_8003BDE0 gate walk as the modal install.
    let (mf, man) = man_with_p2_records(&[p2_record(&[0x0193], &[0x21])], 0, 0);
    let mut w = World::new();
    assert!(w.install_helper_record(&mf, &man, 0), "clear flag: spawns");
    w.field_vm.helper_contexts.clear();
    w.system_flag_set(0x0193);
    assert!(
        !w.install_helper_record(&mf, &man, 0),
        "latched C1 flag blocks the spawn"
    );
    assert!(w.field_vm.helper_contexts.is_empty());
}

#[test]
fn helper_spawn_while_timeline_active_is_not_dropped() {
    // A second spawned record while a modal timeline (or another helper)
    // executes must not be dropped: both helper contexts coexist with the
    // active timeline in the bounded context table.
    use crate::cutscene_timeline::CutsceneTimeline;
    let long_wait = &[0x4A, 0xFF, 0x7F]; // WAIT_FRAMES 0x7FFF: stays live
    let (mf, man) = man_with_p2_records(
        &[p2_record(&[], long_wait), p2_record(&[], long_wait)],
        0,
        0,
    );
    let mut w = World::new();
    w.cutscene.timeline = Some(CutsceneTimeline::new(long_wait.to_vec(), 0));
    assert!(w.cutscene_timeline_active());
    assert!(w.install_helper_record(&mf, &man, 0));
    assert!(w.install_helper_record(&mf, &man, 1));
    assert_eq!(
        w.field_vm.helper_contexts.len(),
        2,
        "concurrent spawns coexist with the modal timeline"
    );
    w.step_helper_contexts();
    assert_eq!(
        w.field_vm.helper_contexts.len(),
        2,
        "waiting helper contexts stay installed across a frame"
    );
    assert!(w.cutscene_timeline_active(), "the modal slot is untouched");
}

#[test]
fn helper_context_table_is_bounded() {
    let (mf, man) = man_with_p2_records(&[p2_record(&[], &[0x4A, 0xFF, 0x7F])], 0, 0);
    let mut w = World::new();
    for _ in 0..crate::world::SPAWNED_CONTEXT_SLOTS {
        assert!(w.install_helper_record(&mf, &man, 0));
    }
    assert!(
        !w.install_helper_record(&mf, &man, 0),
        "a full context table refuses further spawns"
    );
    assert_eq!(
        w.field_vm.helper_contexts.len(),
        crate::world::SPAWNED_CONTEXT_SLOTS
    );
}

#[test]
fn spawned_helper_record_rebases_global_index() {
    // op-0x44 carries a GLOBAL record index; the install re-bases it into
    // partition 2 (`global - N0 - N1`, retail FUN_8003BDE0).
    let (mf, man) = man_with_p2_records(&[p2_record(&[], &[0x2E, 0x1A])], 3, 4);
    let mut w = World::new();
    assert!(
        !w.install_spawned_helper_record(&mf, &man, 2),
        "a global index below N0+N1 cannot re-base"
    );
    assert!(w.install_spawned_helper_record(&mf, &man, 7));
    assert_eq!(w.field_vm.helper_contexts.len(), 1);
}

/// A picker entry mid-opening tears the chain down - flag, timeline,
/// narration - and reports that something was live; a second call is a
/// quiet no-op.
#[test]
fn abandon_opening_chain_tears_down_and_reports() {
    let mut w = World::default();
    w.cutscene.opening_chain_active = true;
    w.cutscene.timeline = Some(crate::cutscene_timeline::CutsceneTimeline::new(vec![0], 0));
    assert!(w.cutscene_timeline_active());
    assert!(w.abandon_opening_chain());
    assert!(!w.cutscene.opening_chain_active);
    assert!(!w.cutscene_timeline_active());
    assert!(w.cutscene.narration.is_none() && w.cutscene.card.is_none());
    assert!(!w.cutscene.entering_town01_opening);
    assert!(!w.abandon_opening_chain(), "nothing live the second time");
}

/// A timeline carrying map01's cave-mouth walk-out shape: a player
/// compass walk (`B7 F8 00 81`), then a scene-bank clip poke with the
/// party-bank bit down (`B2 F8 18`, `A2 F8 03`), its end-latch spin
/// (`AC F8 08`, `AD F8 08`), and a `WaitFrames` so the timeline stays up.
fn timeline_with_player_walk_and_clip_wait(clip_ticks: u32) -> World {
    use crate::cutscene_timeline::CutsceneTimeline;
    let mut w = World {
        mode: crate::world::SceneMode::Field,
        ..World::default()
    };
    w.spawn_actor(0);
    w.player_actor_slot = Some(0);
    w.actors[0].move_state.world_x = 0x1040;
    w.actors[0].move_state.world_z = 0x2040;
    w.locomotion.scene_clip_ticks = vec![2, 2, clip_ticks];
    let bc = vec![
        0xB7, 0xF8, 0x00, 0x81, // compass walk: dir 0 (-Z), 1 x div 16
        0xB2, 0xF8, 0x18, // party-bank bit down
        0xA2, 0xF8, 0x03, // ExecMove 3 -> scene record 2
        0xAC, 0xF8, 0x08, // clear the end latch
        0xAD, 0xF8, 0x08, // spin on it
        0x4A, 0xFF, 0x7F, // WaitFrames (keeps the timeline installed)
    ];
    w.cutscene.timeline = Some(CutsceneTimeline::new(bc, 0));
    w
}

/// `B7 F8 00 81` walks the player one tile along `-Z` over sixteen
/// ticks - one speed unit per vsync, `FUN_8003774C`'s `0x37` arm, the
/// first spent on the arming frame. The record runs on past the op at
/// once and waits at its next op on the player (`B2 F8 18`) until the
/// frame after the leg lands.
#[test]
fn cutscene_timeline_player_compass_walk_runs_its_budget_while_the_record_waits() {
    let mut w = timeline_with_player_walk_and_clip_wait(10);
    let mut ticks = 0;
    loop {
        w.step_cutscene_timeline();
        ticks += 1;
        let tl = w.cutscene.timeline.as_ref().unwrap();
        assert_eq!(tl.pc, 4, "past the walk, held on the player op");
        if tl.player_glide.is_none() {
            break;
        }
        assert!(ticks < 64, "the leg lands");
    }
    assert_eq!(ticks, 16, "sixteen speed units at one per tick");
    assert_eq!(w.actors[0].move_state.world_z, 0x2040 - 128);
    assert_eq!(w.actors[0].move_state.world_x, 0x1040);
    w.step_cutscene_timeline();
    assert!(
        w.cutscene.timeline.as_ref().unwrap().pc > 4,
        "the next frame runs the player op"
    );
}

/// `B7 40 00 84` against an NPC channel walks that placement 512 units
/// along `-Z` (64 speed units, `div 16`, one per tick, rate `0x80`) - the
/// arm bylon's Maya meeting opens on. The record runs on past the op and
/// holds at its next op on the same actor (`B2 40 18`) until the frame
/// after the leg lands, exactly as the player arm does.
#[test]
fn cutscene_timeline_npc_compass_walk_moves_the_placement_while_the_record_waits() {
    use crate::cutscene_timeline::CutsceneTimeline;
    use crate::field_channels::FieldChannel;
    use legaia_engine_vm::field::FieldCtx;
    let bc = vec![
        0xB7, 0x40, 0x00, 0x84, // compass walk: dir 0 (-Z), 4 x div 16
        0xB2, 0x40, 0x18, // next op on the same actor
        0x4A, 0xFF, 0x7F, // WaitFrames (keeps the timeline installed)
    ];
    let mut w = World::new();
    w.cutscene.timeline = Some(CutsceneTimeline::new(bc, 0));
    w.field_vm.channels = vec![FieldChannel {
        placement_index: 4,
        ctx: FieldCtx {
            script_id: 0x40,
            ..FieldCtx::default()
        },
        record_offset: 0,
        pc: 0,
        done: false,
        object_bind: false,
    }];
    w.npcs.positions.insert(4, (0x29C0, 0x1940));
    let mut ticks = 0;
    loop {
        w.step_cutscene_timeline();
        ticks += 1;
        let tl = w.cutscene.timeline.as_ref().unwrap();
        assert_eq!(tl.pc, 4, "past the walk, held on the actor's next op");
        if tl.npc_glides.is_empty() {
            break;
        }
        assert!(ticks < 200, "the leg lands");
    }
    assert_eq!(ticks, 64, "sixty-four speed units at one per tick");
    assert_eq!(w.npcs.positions.get(&4), Some(&(0x29C0, 0x1940 - 512)));
    w.step_cutscene_timeline();
    assert!(
        w.cutscene.timeline.as_ref().unwrap().pc > 4,
        "the next frame runs the actor op"
    );
}

/// The `AD F8 08` spin after a scene-bank clip poke holds for the clip's
/// end-latch length (the `FUN_800204F8` latch): two ticks a frame for an
/// ungated record, four for a gated divisor-4 one, then steps past.
#[test]
fn cutscene_timeline_player_clip_latch_spin_holds_for_the_clip() {
    use crate::field_anim::{CLIP_RATE, clip_end_ticks, clip_step};
    let frames = 10u16;
    for (gated, div, per_frame) in [(false, 0u8, 2usize), (true, 4, 4)] {
        let ticks = clip_end_ticks(frames, clip_step(CLIP_RATE, gated, div));
        let mut w = timeline_with_player_walk_and_clip_wait(ticks);
        let mut spin_ticks = 0;
        let mut saw_spin = false;
        for _ in 0..400 {
            w.step_cutscene_timeline();
            let tl = w.cutscene.timeline.as_ref().expect("installed");
            if tl.player_clip_wait.is_some() {
                saw_spin = true;
                spin_ticks += 1;
            } else if saw_spin {
                break;
            }
        }
        assert!(saw_spin, "the latch spin parks while the clip plays");
        // The poke and the spin land in one slice; the record then sits on
        // the spin for the whole latch length, counting that one.
        assert_eq!(spin_ticks, usize::from(frames) * per_frame, "gated={gated}");
        let tl = w.cutscene.timeline.as_ref().expect("installed");
        assert_eq!(tl.pc, 16, "resumed past the 3-byte spin onto the wait");
    }
}

/// A record that loops on a held-pad poll (`edlast`'s closing
/// `4A 08 00` / `42 01 09` / `26` back) is waiting for the player, not
/// wrapped: it stays installed under a released pad and moves on the
/// press.
#[test]
fn a_pad_poll_loop_waits_for_the_press() {
    use crate::cutscene_timeline::CutsceneTimeline;
    let mut w = World {
        mode: crate::world::SceneMode::Field,
        ..World::default()
    };
    let bc = vec![
        0x4A, 0x02, 0x00, // WaitFrames 2
        0x42, 0x01, 0x09, 0x05, 0x00, // Cross held -> +3+5 = 11
        0x26, 0xF7, 0xFF, // back to 0
        0x4A, 0xFF, 0x7F, // WaitFrames (keeps the timeline up)
    ];
    w.cutscene.timeline = Some(CutsceneTimeline::new(bc, 0));
    w.set_pad(0);
    for _ in 0..60 {
        w.step_cutscene_timeline();
    }
    let tl = w
        .cutscene
        .timeline
        .as_ref()
        .expect("still waiting on the pad");
    assert!(tl.pc < 11, "never passed the poll, pc={:#x}", tl.pc);
    w.set_pad(crate::input::PadButton::Cross.mask());
    for _ in 0..4 {
        w.step_cutscene_timeline();
    }
    let tl = w.cutscene.timeline.as_ref().expect("installed");
    assert_eq!(tl.pc, 11, "the press takes the jump");
}

/// A party-bank clip (the locomotion loops) has no timed end latch in
/// the port: the spin steps past as before rather than parking forever.
#[test]
fn cutscene_timeline_party_bank_clip_does_not_park_the_latch_spin() {
    use crate::cutscene_timeline::CutsceneTimeline;
    let mut w = World::default();
    let bc = vec![0xA2, 0xF8, 0x01, 0xAD, 0xF8, 0x08, 0x4A, 0xFF, 0x7F];
    w.cutscene.timeline = Some(CutsceneTimeline::new(bc, 0));
    w.step_cutscene_timeline();
    let tl = w.cutscene.timeline.as_ref().expect("installed");
    assert!(tl.player_clip_wait.is_none());
    assert_eq!(tl.pc, 6, "stepped past onto the wait");
}

/// A timeline that is a placement's own touch-resumed context
/// (`interaction_slot`) over `bc`, with that placement's channel sharing
/// the bytes at PC 0.
fn interaction_timeline(bc: Vec<u8>, slot: u8) -> World {
    use crate::cutscene_timeline::CutsceneTimeline;
    use crate::field_channels::FieldChannel;
    use legaia_engine_vm::field::FieldCtx;

    let mut w = World::new();
    let mut tl = CutsceneTimeline::new(bc.clone(), 0);
    tl.interaction_slot = Some(slot);
    w.cutscene.timeline = Some(tl);
    w.field_vm.channels_man = Some(std::sync::Arc::new(bc));
    w.field_vm.channels = vec![FieldChannel {
        placement_index: usize::from(slot),
        ctx: FieldCtx {
            script_id: 0x40,
            ..FieldCtx::default()
        },
        record_offset: 0,
        pc: 0,
        done: false,
        object_bind: false,
    }];
    w
}

#[test]
fn a_touch_resumed_context_ends_at_its_nop_and_hands_the_pc_back() {
    // `SET 5`, `21`, `SET 6`, `21`: the interaction ends on the first
    // executed `0x21` (`FUN_80039B7C` `0x80039E20` / `0x80039E68`), so
    // flag 6 - the bytes the NEXT touch runs - must not execute, and the
    // placement channel resumes after the `21`.
    let mut w = interaction_timeline(vec![0x50, 0x05, 0x21, 0x50, 0x06, 0x21], 3);
    w.step_cutscene_timeline();
    assert!(w.cutscene.timeline.is_none(), "the interaction ended");
    assert!(w.system_flag_test(5));
    assert!(!w.system_flag_test(6), "nothing past the `21` ran");
    assert_eq!(
        w.field_vm.channels[0].pc, 3,
        "the channel resumes past the `21`"
    );
}

#[test]
fn the_system_script_starts_no_pass_while_another_context_holds_the_player() {
    use crate::cutscene_timeline::CutsceneTimeline;
    // Two passes: `SET 7`, `21`, then `SET 8`, `21`.
    let mut w = World::new();
    w.load_field_script(vec![0x50, 0x07, 0x21, 0x50, 0x08, 0x21]);
    w.step_field_frame_slice();
    assert!(w.system_flag_test(7), "the install pass runs");
    assert!(!w.field_vm.system_pass_open, "the `21` closed the pass");
    // A stepped helper context holds the player (`+0x10 & 0x80000`):
    // `FUN_801DA51C` starts no new pass (`0x801DA794..0x801DA7AC`).
    let mut helper = CutsceneTimeline::new(vec![0x4A, 0x40, 0x00], 0);
    helper.stepped = true;
    w.field_vm.helper_contexts.push(helper);
    assert!(w.step_field_frame_slice().is_none());
    assert!(!w.system_flag_test(8), "no pass while the player is held");
    // Released: the next pass runs.
    w.field_vm.helper_contexts.clear();
    w.step_field_frame_slice();
    assert!(w.system_flag_test(8));
}

#[test]
fn an_open_system_pass_continues_under_a_held_player() {
    use crate::cutscene_timeline::CutsceneTimeline;
    // `WaitFrames 2` inside the pass, then `SET 9`, `21`: a pass already
    // open when the player gets held runs on (`0x801DA78C`).
    let mut w = World::new();
    w.load_field_script(vec![0x4A, 0x02, 0x00, 0x50, 0x09, 0x21]);
    w.step_field_frame_slice();
    assert!(w.field_vm.system_pass_open, "parked inside the pass");
    let mut helper = CutsceneTimeline::new(vec![0x4A, 0x40, 0x00], 0);
    helper.stepped = true;
    w.field_vm.helper_contexts.push(helper);
    for _ in 0..4 {
        w.step_field_frame_slice();
    }
    assert!(w.system_flag_test(9), "the open pass ran to its `21`");
}

#[test]
fn a_committed_battle_holds_the_system_script() {
    // The entity SM runs the system script only at state 0
    // (`FUN_801DA51C` `0x801DA750`); a latched scripted fight holds it
    // even inside an open pass.
    let mut w = World::new();
    w.load_field_script(vec![0x50, 0x0A, 0x21]);
    w.carriers.pending_battle = Some(1);
    assert!(w.field_scripts_held_for_battle());
    assert!(w.step_field_frame_slice().is_none());
    assert!(!w.system_flag_test(0x0A));
}

#[test]
fn a_seat_then_walk_in_one_slice_walks_from_the_seat() {
    use crate::cutscene_timeline::CutsceneTimeline;
    use crate::field_channels::FieldChannel;
    use legaia_engine_vm::field::FieldCtx;
    // `CC 40 51 10 10 00 00` seats channel 0x40 at tile (16,16), then
    // `C7 40 12 10 33` walks it to (18,16) - the shape of every cutscene
    // that places an actor parked at the hide box and walks it in.
    let bc = vec![
        0xCC, 0x40, 0x51, 0x10, 0x10, 0x00, 0x00, 0xC7, 0x40, 0x12, 0x10, 0x33, 0x4A, 0x40, 0x00,
    ];
    let mut w = World::new();
    w.cutscene.timeline = Some(CutsceneTimeline::new(bc, 0));
    let hide = crate::world::FIELD_OFFMAP_HIDE_XZ;
    w.field_vm.channels = vec![FieldChannel {
        placement_index: 4,
        ctx: FieldCtx {
            script_id: 0x40,
            world_x: hide as u16,
            world_z: hide as u16,
            ..FieldCtx::default()
        },
        record_offset: 0,
        pc: 0,
        done: false,
        object_bind: false,
    }];
    w.npcs.positions.insert(4, (hide, hide));
    w.step_cutscene_timeline();
    let leg = w.npcs.motions.get(&4).expect("the walk leg started");
    let (x, z) = (leg.state.world_x, leg.state.world_z);
    assert_eq!(
        (x, z),
        (0x840, 0x840),
        "the walk starts at the seat, not the hide box"
    );
    assert_eq!(leg.target, (0x940, 0x840));
}

#[test]
fn a_poked_placement_does_not_run_its_talk_body() {
    use crate::cutscene_timeline::CutsceneTimeline;
    use crate::field_channels::FieldChannel;
    use crate::world::SceneMode;
    use legaia_engine_vm::field::FieldCtx;
    // A placement whose talk body is `SET 0x20`. The timeline pokes it
    // (`B1 40 01`, a cross-context CFLAG_SET), then waits. Retail runs a
    // placement context only while `+0x10 & 0x100` is up, which a poke
    // does not raise (`FUN_8003BC08` -> `FUN_80039B7C`), so the talk body
    // stays asleep however many frames pass - `dolk2`'s Noa once set
    // `0x2FE` this way.
    let man = vec![0x50, 0x20, 0x21];
    let mut w = World::new();
    w.mode = SceneMode::Cutscene;
    w.cutscene.timeline = Some(CutsceneTimeline::new(
        vec![0xB1, 0x40, 0x01, 0x4A, 0x40, 0x00],
        0,
    ));
    w.field_vm.channels_man = Some(std::sync::Arc::new(man));
    w.field_vm.channels = vec![FieldChannel {
        placement_index: 1,
        ctx: FieldCtx {
            script_id: 0x40,
            ..FieldCtx::default()
        },
        record_offset: 0,
        pc: 0,
        done: false,
        object_bind: false,
    }];
    w.step_cutscene_timeline();
    assert_eq!(w.field_vm.channels[0].ctx.flags & 2, 2, "the poke landed");
    for _ in 0..8 {
        w.tick();
    }
    assert!(!w.system_flag_test(0x20), "the talk body stays asleep");
    assert_eq!(w.field_vm.channels[0].pc, 0);
}

#[test]
fn a_player_targeted_move_to_in_a_timeline_seats_the_player() {
    use crate::cutscene_timeline::CutsceneTimeline;
    // `A3 F8 60 0D` (MOVE_TO against the player anchor), then a wait.
    let mut w = World::new();
    w.install_field_player(0);
    let slot = 0u8;
    w.cutscene.timeline = Some(CutsceneTimeline::new(
        vec![0xA3, 0xF8, 0x60, 0x0D, 0x4A, 0x40, 0x00],
        0,
    ));
    w.step_cutscene_timeline();
    let ms = &w.actors[slot as usize].move_state;
    assert_eq!(
        (ms.world_x, ms.world_z),
        (0x60 * 0x80 + 0x40, 0x0D * 0x80 + 0x40),
        "retail resolves 0xF8 to the player and takes the player arm"
    );
    assert_eq!(w.cutscene.timeline.as_ref().unwrap().pc, 4);
}

#[test]
fn a_copy_from_player_then_walk_starts_at_the_player() {
    use crate::cutscene_timeline::CutsceneTimeline;
    use crate::field_channels::FieldChannel;
    use legaia_engine_vm::field::FieldCtx;
    // `CC 40 37` (4C nibble-3 sub-7: copy the player's position onto
    // channel 0x40), then `C7 40 12 10 33` walks it off. The channel
    // carries the party-bank bit a `4C 50 F1` model select raises, which
    // must not read as "this is the player".
    let bc = vec![
        0xCC, 0x40, 0x37, 0xC7, 0x40, 0x12, 0x10, 0x33, 0x4A, 0x40, 0x00,
    ];
    let mut w = World::new();
    w.install_field_player(0);
    w.actors[0].move_state.world_x = 0x900;
    w.actors[0].move_state.world_z = 0x700;
    w.cutscene.timeline = Some(CutsceneTimeline::new(bc, 0));
    let hide = crate::world::FIELD_OFFMAP_HIDE_XZ;
    w.field_vm.channels = vec![FieldChannel {
        placement_index: 4,
        ctx: FieldCtx {
            script_id: 0x40,
            world_x: hide as u16,
            world_z: hide as u16,
            flags: 0x0100_0000,
            ..FieldCtx::default()
        },
        record_offset: 0,
        pc: 0,
        done: false,
        object_bind: false,
    }];
    w.npcs.positions.insert(4, (hide, hide));
    w.step_cutscene_timeline();
    let leg = w.npcs.motions.get(&4).expect("the walk leg started");
    assert_eq!(
        (leg.state.world_x, leg.state.world_z),
        (0x900, 0x700),
        "the walk starts at the player the actor was copied onto"
    );
}

#[test]
fn a_helper_context_parks_on_its_text_and_runs_on_after_the_box() {
    use crate::cutscene_timeline::CutsceneTimeline;
    // `1F 'H' 'i' 00` (one text segment), `SET 0x0C`, then a long wait.
    // Retail's runner parks any engaged context on its text segment and
    // hands it to the shared box; a helper used to complete there and
    // drop the rest of the record.
    let bc = vec![0x1F, b'H', b'i', 0x00, 0x50, 0x0C, 0x4A, 0x40, 0x00];
    let mut w = World::new();
    w.field_vm
        .helper_contexts
        .push(CutsceneTimeline::new(bc, 0));
    w.step_helper_contexts();
    assert_eq!(w.field_vm.helper_contexts.len(), 1, "parked, not dropped");
    assert!(
        w.script_dialog_panel().is_some(),
        "the helper shows its box"
    );
    assert!(!w.system_flag_test(0x0C));
    for i in 0..200 {
        w.set_pad(if i % 2 == 0 {
            crate::input::PadButton::Cross.mask()
        } else {
            0
        });
        w.step_helper_contexts();
        if w.system_flag_test(0x0C) {
            break;
        }
    }
    assert!(
        w.system_flag_test(0x0C),
        "dismissing the box resumes the helper past the segment"
    );
    assert!(w.script_dialog_panel().is_none());
}

#[test]
fn a_parked_box_is_not_drawn_over_a_battle() {
    use crate::cutscene_timeline::CutsceneTimeline;
    // The field pager lives in the field overlay, which the battle
    // overlay replaces: a context parked on its text across a forced
    // fight keeps its park, but no box is drawn until the field returns.
    let bc = vec![0x1F, b'H', b'i', 0x00, 0x4A, 0x40, 0x00];
    let mut w = World::new();
    w.field_vm
        .helper_contexts
        .push(CutsceneTimeline::new(bc, 0));
    w.step_helper_contexts();
    assert!(w.script_dialog_panel().is_some());
    w.mode = crate::world::SceneMode::Battle;
    assert!(w.script_dialog_panel().is_none(), "no box over the fight");
    w.mode = crate::world::SceneMode::Field;
    assert!(
        w.script_dialog_panel().is_some(),
        "the park survives the fight"
    );
}

#[test]
fn a_helper_off_text_keeps_its_slice_while_the_box_is_taken() {
    use crate::cutscene_timeline::CutsceneTimeline;
    // Retail's runner `FUN_80039B7C` gates a context on its own `+0x9C`,
    // never on the box: while one context's text is up, a context that is
    // not on text runs every frame, and a second context that reaches
    // text waits for the box without typing it.
    let text = vec![0x1F, b'H', b'i', 0x00, 0x50, 0x0C, 0x4A, 0x40, 0x00];
    let first = vec![0x1F, b'Y', b'o', 0x00, 0x50, 0x0D, 0x4A, 0x40, 0x00];
    // WAIT 16, SET 0x0E, then a long wait. `frames` is no witness here:
    // a `WaitFrames` park is kept off the anti-hang cap, so a context
    // that sits in a wait reads 0 frames however many slices it took.
    let off_text = vec![0x4A, 0x10, 0x00, 0x50, 0x0E, 0x4A, 0xFF, 0x7F];
    let mut w = World::new();
    w.field_vm
        .helper_contexts
        .push(CutsceneTimeline::new(first, 0));
    w.step_helper_contexts();
    assert!(
        w.script_dialog_panel().is_some(),
        "the first helper holds the box"
    );
    // Two more contexts: one that never reaches text, one that does - and
    // sits AHEAD of the owner in the table, so table order cannot pick it.
    w.field_vm
        .helper_contexts
        .insert(0, CutsceneTimeline::new(text, 0));
    w.field_vm
        .helper_contexts
        .push(CutsceneTimeline::new(off_text, 0));
    for _ in 0..120 {
        w.step_helper_contexts();
    }
    let ctxs = &w.field_vm.helper_contexts;
    assert_eq!(ctxs.len(), 3);
    assert!(
        w.system_flag_test(0x0E),
        "the off-text helper ran through its wait while the box was up"
    );
    let waiting = ctxs[0]
        .dialog
        .as_ref()
        .expect("the second texter is parked on its text");
    let owner = ctxs[1]
        .dialog
        .as_ref()
        .expect("the owner still holds the box");
    assert_eq!(owner.page_glyphs().len(), 2, "the owner's page typed out");
    assert!(
        waiting.page_glyphs().is_empty(),
        "the waiting context's box has not typed while the box is taken"
    );
    assert!(
        std::ptr::eq(w.script_dialog_panel().unwrap(), owner),
        "the box shows the first claimant, not table order"
    );
    // Dismiss the owner's box: it runs on (SET 0x0D) and the waiting
    // context gets the box.
    for i in 0..200 {
        w.set_pad(if i % 2 == 0 {
            crate::input::PadButton::Cross.mask()
        } else {
            0
        });
        w.step_helper_contexts();
        if w.system_flag_test(0x0D) {
            break;
        }
    }
    assert!(w.system_flag_test(0x0D), "the owner resumed past its text");
    assert!(
        !w.system_flag_test(0x0C),
        "the waiting context has not run past its text"
    );
    for i in 0..400 {
        w.set_pad(if i % 2 == 0 {
            crate::input::PadButton::Cross.mask()
        } else {
            0
        });
        w.step_helper_contexts();
        if w.system_flag_test(0x0C) {
            break;
        }
    }
    assert!(
        w.system_flag_test(0x0C),
        "the waiting context claims the freed box and runs on"
    );
}

#[test]
fn a_second_player_walk_waits_for_the_first_to_land() {
    use crate::cutscene_timeline::CutsceneTimeline;
    // Two contexts each walk the player (`C7 F8 tx tz mode`) then SET a
    // flag. Retail's walk park leaves the player's halt bit `0x400` set
    // until the kernel lands it, and the dispatcher refuses the second
    // context's cross-context op on a halted target
    // (`0x801DE90C..0x801DE944`) - the walks run one after the other,
    // never against each other.
    let first = vec![0xC7, 0xF8, 0x12, 0x10, 0x33, 0x50, 0x0C, 0x4A, 0xFF, 0x7F];
    let second = vec![0xC7, 0xF8, 0x10, 0x10, 0x33, 0x50, 0x0D, 0x4A, 0xFF, 0x7F];
    let mut w = World::default();
    w.actors[0].active = true;
    w.player_actor_slot = Some(0);
    w.actors[0].move_state.world_x = 0x10 * 0x80 + 0x40;
    w.actors[0].move_state.world_z = 0x10 * 0x80 + 0x40;
    w.field_vm
        .helper_contexts
        .push(CutsceneTimeline::new(first, 0));
    w.field_vm
        .helper_contexts
        .push(CutsceneTimeline::new(second, 0));
    let target_first = (0x12 * 0x80 + 0x40, 0x10 * 0x80 + 0x40);
    let mut first_x = Vec::new();
    for _ in 0..400 {
        w.step_helper_contexts();
        first_x.push(w.actors[0].move_state.world_x);
        if w.system_flag_test(0x0D) {
            break;
        }
    }
    assert!(w.system_flag_test(0x0C), "the first walk landed");
    assert!(w.system_flag_test(0x0D), "the second walk ran after it");
    assert!(
        first_x.contains(&target_first.0),
        "the player reached the first walk's tile before turning back"
    );
    // Monotone out, then monotone back: no frame-by-frame tug-of-war.
    let peak = first_x.iter().position(|&x| x == target_first.0).unwrap();
    assert!(first_x[..=peak].windows(2).all(|p| p[1] >= p[0]));
    assert!(first_x[peak..].windows(2).all(|p| p[1] <= p[0]));
    assert_eq!(
        w.actors[0].move_state.world_x,
        0x10 * 0x80 + 0x40,
        "the player ends on the second walk's tile"
    );
}
