//! Cutscene-timeline facing ops: NPC and player facing ramps and snaps, halt-
//! acquire turns, the position copy and the spawn-prologue face-at.

use crate::world::World;

/// Build a timeline whose record turns NPC channel 5 with a cross-context
/// op-0x38 facing op (`B8 05 <op0> <op1>`), then WAIT_FRAMES so the
/// timeline stays installed. The NPC starts posed at engine heading
/// `start` (slot 5 in the render-heading map).
fn timeline_with_npc_facing_op(op0: u8, op1: u8, start: i16) -> World {
    use legaia_engine_field::cutscene_timeline::CutsceneTimeline;
    use legaia_engine_field::field_channels::FieldChannel;
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
    use legaia_engine_field::cutscene_timeline::CutsceneTimeline;
    let mut w = World {
        mode: legaia_engine_mode::mode::SceneMode::Field,
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
    use legaia_engine_field::cutscene_timeline::CutsceneTimeline;
    use legaia_engine_field::field_channels::FieldChannel;
    use legaia_engine_vm::field::FieldCtx;
    let mut w = World {
        mode: legaia_engine_mode::mode::SceneMode::Field,
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
    use legaia_engine_field::cutscene_timeline::CutsceneTimeline;
    use legaia_engine_field::field_channels::FieldChannel;
    use legaia_engine_vm::field::FieldCtx;
    let mut w = World {
        mode: legaia_engine_mode::mode::SceneMode::Field,
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
    use legaia_engine_field::cutscene_timeline::CutsceneTimeline;
    use legaia_engine_field::field_channels::FieldChannel;
    use legaia_engine_vm::field::FieldCtx;
    let mut w = World {
        mode: legaia_engine_mode::mode::SceneMode::Field,
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
    use legaia_engine_field::field_channels::FieldChannel;
    use legaia_engine_vm::field::FieldCtx;
    let mut w = World {
        mode: legaia_engine_mode::mode::SceneMode::Field,
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

/// A turn holds its NPC for the record's next op on it: the halt-acquire arm
/// raises the target's `0x400` for the ramp, and the dispatcher prologue
/// returns any later cross-context op on that actor at its own PC until the
/// leg snaps. `edteien` P2[1] runs `B8 0A 82 0C` then `C1 0A 02 CB` in one
/// tick; a walk that started in the same slice ran twelve vsyncs ahead of
/// retail's (`ending_vignette_fullscreen`, flat 10).
#[test]
fn a_walk_on_a_turning_npc_waits_for_the_turn() {
    use legaia_engine_field::cutscene_timeline::CutsceneTimeline;
    // `B8 05 82 06` (turn over 6 frames) then `C1 05 02 C1` (walk), then
    // a long wait that keeps the timeline installed.
    let mut w = timeline_with_npc_facing_op(0x82, 0x06, 0x000);
    w.cutscene.timeline = Some(CutsceneTimeline::new(
        vec![
            0xB8, 0x05, 0x82, 0x06, 0xC1, 0x05, 0x02, 0xC1, 0x4A, 0xFF, 0x7F,
        ],
        0,
    ));
    w.step_cutscene_timeline();
    let tl = w.cutscene.timeline.as_ref().expect("timeline installed");
    assert_eq!(tl.pc, 4, "the record waits on the walk while the NPC turns");
    assert!(tl.npc_glides.is_empty(), "no walk armed during the turn");
    let mut armed_at = None;
    for t in 1..=12 {
        w.step_cutscene_timeline();
        let tl = w.cutscene.timeline.as_ref().expect("timeline installed");
        if !tl.npc_glides.is_empty() {
            armed_at = Some(t);
            break;
        }
    }
    let t = armed_at.expect("the walk starts once the turn has landed");
    assert!(
        t >= 5,
        "the walk waited out the turn's budget (armed at {t})"
    );
}

/// On the game-frame grid an NPC compass walk spends the frame step once a
/// game frame and nothing between, and the op after it on the same actor
/// waits through the rest of the frame the leg lands on. `C1 05 02 41` is
/// eight units (`div 8`, rate `0x40`: eight world units a unit) at step 2:
/// four game frames, the first being the arming one.
#[test]
fn an_npc_walk_advances_on_the_game_frame_grid() {
    use legaia_engine_field::cutscene_timeline::CutsceneTimeline;
    let mut w = timeline_with_npc_facing_op(0x82, 0x00, 0x000);
    // `C1 05 02 41` (walk -X), `B8 05 84 00` (instant turn), long wait.
    w.cutscene.timeline = Some(CutsceneTimeline::new(
        vec![
            0xC1, 0x05, 0x02, 0x41, 0xB8, 0x05, 0x84, 0x00, 0x4A, 0xFF, 0x7F,
        ],
        0,
    ));
    w.clock.game_tick_live = true;
    w.clock.frame_step = 2;
    let mut xs = Vec::new();
    let mut freed_at = None;
    for vsync in 0..12 {
        w.clock.game_tick_fired = vsync % 2 == 0;
        w.step_cutscene_timeline();
        xs.push(w.npcs.positions.get(&5).map(|p| p.0));
        let tl = w.cutscene.timeline.as_ref().expect("timeline installed");
        if freed_at.is_none() && tl.pc > 4 {
            freed_at = Some(vsync);
        }
    }
    // 16 world units a game frame, held on the vsync between.
    assert_eq!(
        &xs[..8],
        &[
            Some(984),
            Some(984),
            Some(968),
            Some(968),
            Some(952),
            Some(952),
            Some(936),
            Some(936)
        ]
    );
    // Landed on the game frame at vsync 6; the turn runs on the next one.
    assert_eq!(freed_at, Some(8));
}
