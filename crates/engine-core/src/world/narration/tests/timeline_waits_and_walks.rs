//! Cutscene-timeline parks and moves: channel waits, the player channel door,
//! compass walks, the clip latch and the pad poll.

use crate::world::World;

/// Build a minimal timeline that reaches a cross-context CFLAG_TST
/// (`B3 05 03` = op 0x33 extended, target channel id 5, completion bit 3),
/// then a WAIT_FRAMES so the timeline stays installed once it resumes past
/// the flag-test. A single spawned channel (script id 5) provides the
/// cross-context target whose flag the timeline waits on.
fn timeline_with_channel_wait(channel_flag_bit3: bool) -> World {
    use legaia_engine_field::cutscene_timeline::CutsceneTimeline;
    use legaia_engine_field::field_channels::FieldChannel;
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

/// A player ExecMove queues the scene-record one-shot only when its pick
/// binds the scene bank. With the party-bank bit up the id strides into
/// the leader's own bank; a scene record there is another skeleton's
/// clip, and drawing it over the hero tore the body apart.
#[test]
fn a_player_exec_move_queues_a_scene_clip_only_off_the_party_bank() {
    use legaia_engine_field::cutscene_timeline::CutsceneTimeline;
    for (bit_op, queued) in [(0xB1u8, Vec::<u8>::new()), (0xB2, vec![5])] {
        let mut w = World {
            mode: legaia_engine_mode::mode::SceneMode::Field,
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
    use legaia_engine_field::cutscene_timeline::CutsceneTimeline;
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
        w.pending_field_events.iter().any(|e| matches!(
            e,
            legaia_engine_field::field_events::FieldEvent::ExecMove { move_id: 6 }
        )),
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
    use legaia_engine_field::cutscene_timeline::CutsceneTimeline;
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

/// A timeline carrying map01's cave-mouth walk-out shape: a player
/// compass walk (`B7 F8 00 81`), then a scene-bank clip poke with the
/// party-bank bit down (`B2 F8 18`, `A2 F8 03`), its end-latch spin
/// (`AC F8 08`, `AD F8 08`), and a `WaitFrames` so the timeline stays up.
fn timeline_with_player_walk_and_clip_wait(clip_ticks: u32) -> World {
    use legaia_engine_field::cutscene_timeline::CutsceneTimeline;
    let mut w = World {
        mode: legaia_engine_mode::mode::SceneMode::Field,
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
    use legaia_engine_field::cutscene_timeline::CutsceneTimeline;
    use legaia_engine_field::field_channels::FieldChannel;
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
    use legaia_engine_field::field_anim::CLIP_RATE;
    use legaia_engine_vm::actor_tick::{clip_end_ticks, clip_step};
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
    use legaia_engine_field::cutscene_timeline::CutsceneTimeline;
    let mut w = World {
        mode: legaia_engine_mode::mode::SceneMode::Field,
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
    w.set_pad(legaia_engine_system::input::PadButton::Cross.mask());
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
    use legaia_engine_field::cutscene_timeline::CutsceneTimeline;
    let mut w = World::default();
    let bc = vec![0xA2, 0xF8, 0x01, 0xAD, 0xF8, 0x08, 0x4A, 0xFF, 0x7F];
    w.cutscene.timeline = Some(CutsceneTimeline::new(bc, 0));
    w.step_cutscene_timeline();
    let tl = w.cutscene.timeline.as_ref().expect("installed");
    assert!(tl.player_clip_wait.is_none());
    assert_eq!(tl.pc, 6, "stepped past onto the wait");
}

/// Op `0x4A` credits the game frame's length on each game frame and passes
/// on the first one whose sum reaches the operand, the frame that reaches the
/// op included (`0x801E09CC..0x801E0A00`): at step `3` a `4A 1E 00` holds its
/// record for `3 * (ceil(30 / 3) - 1) = 27` vsyncs. The first wait here only
/// lands the record on the frame grid.
///
/// Every row is a hold a per-vsync capture of retail's opening chain reads
/// (`scripts/pcsx-redux/autorun_opdeene_pacing.lua` from `s1_newgame_field`),
/// at a constant step for the whole hold: step `3` in `opdeene`, step `2` in
/// `opstati` (`P2[0]`: `+0x2F7`, `+0x341`, `+0x34B`, `+0x34E`) and `opurud`
/// (`P2[9]` `+0x9D`), step `4` in `opurud` (`+0x17A`, `+0x228`, `+0x25D`,
/// `+0x277`, `+0x28D`). No field scene steps by `1`.
#[test]
fn a_wait_holds_for_whole_game_frames_less_the_one_that_reached_it() {
    use legaia_engine_field::cutscene_timeline::CutsceneTimeline;
    for (step, wait, held) in [
        // Step 3: operand 0, 1 and 2 past a multiple.
        (3u8, 30u8, 27u32),
        (3, 40, 39),
        (3, 5, 3),
        (3, 8, 6),
        // Step 2: even and odd operands.
        (2, 30, 28),
        (2, 32, 30),
        (2, 6, 4),
        (2, 90, 88),
        (2, 45, 44),
        // Step 4.
        (4, 90, 88),
        (4, 50, 48),
        (4, 45, 44),
        (4, 15, 12),
        (4, 5, 4),
    ] {
        let mut w = World {
            mode: legaia_engine_mode::mode::SceneMode::Field,
            ..World::default()
        };
        w.set_frame_step_floor(step);
        let bc = vec![
            0x4A, 0x01, 0x00, // lands on the next game frame
            0x4A, wait, 0x00, // the measured wait
            0x4A, 0xFF, 0x7F, // keeps the timeline up
        ];
        w.cutscene.timeline = Some(CutsceneTimeline::new(bc, 0));
        let mut on_wait = 0u32;
        for _ in 0..200 {
            w.tick();
            match w.cutscene.timeline.as_ref().map(|tl| tl.pc) {
                Some(3) => on_wait += 1,
                Some(6) => break,
                _ => {}
            }
        }
        assert_eq!(on_wait, held, "4A {wait} at step {step}");
    }
}

/// A screen-effect tween a record spawns is ticked in the frame that spawned
/// it, so its clock and the sum of the wait behind it move together:
/// `minigame_dance_pcsx` is parked on `koin3` `P2[6]`'s `4A 1E 00` behind
/// `34 01 FF FF FF 1E 00` with the walk-in's clock `+0xC8` and the wait's
/// `+0x54` both at `21`, on a step-`3` frame.
#[test]
fn a_spawned_screen_tween_keeps_step_with_the_wait_behind_it() {
    use legaia_engine_field::cutscene_timeline::CutsceneTimeline;
    for step in [2u8, 3] {
        let mut w = World {
            mode: legaia_engine_mode::mode::SceneMode::Field,
            ..World::default()
        };
        w.set_frame_step_floor(step);
        let bc = vec![
            0x4A, 0x01, 0x00, // lands on the next game frame
            0x34, 0x01, 0xFF, 0xFF, 0xFF, 0x1E, 0x00, // subtractive white walk-in
            0x4A, 0x1E, 0x00, // the wait the capture is parked on
            0x4A, 0xFF, 0x7F, // keeps the timeline up
        ];
        w.cutscene.timeline = Some(CutsceneTimeline::new(bc, 0));
        let mut seen = 0;
        for _ in 0..40 {
            w.tick();
            let Some(tl) = w.cutscene.timeline.as_ref() else {
                break;
            };
            if tl.pc != 10 {
                continue;
            }
            let clock = w
                .presentation
                .effect_tween_slot
                .and_then(|s| w.actors[s].colour_tween)
                .map(|t| t.clock);
            assert_eq!(
                clock,
                Some(tl.ctx.wait_accum as u16),
                "step {step}: the tween's clock is the wait's sum"
            );
            seen += 1;
        }
        assert!(seen >= 20, "step {step}: the record sat on the wait");
    }
}

/// A free-roam placement channel for the spawn-prologue tests below.
fn prologue_channel(
    slot: usize,
    id: u16,
    off: usize,
) -> legaia_engine_field::field_channels::FieldChannel {
    legaia_engine_field::field_channels::FieldChannel {
        placement_index: slot,
        ctx: legaia_engine_vm::field::FieldCtx {
            script_id: id,
            ..legaia_engine_vm::field::FieldCtx::default()
        },
        record_offset: off,
        pc: 0,
        done: false,
        object_bind: false,
    }
}

/// A placement whose spawn section seats it with `MOVE_TO` and then runs a
/// `41` compass walk on itself (`dolk` `P1[21]`: `23 64 4D`, `41 06 01`,
/// `38 80 00`) ends half a tile east of the `MOVE_TO` tile: the load slice
/// runs on past the yield (the `38` turns it), and the walk kernel plays the
/// `0x40 * 1` leg out over the next ticks, then clears the context's halt.
#[test]
fn a_spawn_prologue_compass_walk_moves_the_placement_after_the_seat() {
    let mut w = World {
        mode: legaia_engine_mode::mode::SceneMode::Field,
        ..World::default()
    };
    let man = vec![
        0x25, 0x23, 0x64, 0x4D, 0x41, 0x06, 0x01, 0x38, 0x80, 0x00, 0x21,
    ];
    w.field_vm.channels = vec![prologue_channel(21, 0x2F, 0)];
    w.field_vm.channels_man = Some(std::sync::Arc::new(man));
    w.npcs.positions.insert(21, (16320, 16320));
    w.pre_run_field_channel_prologues();
    // The seat lands in the load frame; the leg does not move it yet.
    assert_eq!(w.npcs.positions.get(&21), Some(&(12864, 9920)));
    assert!(w.npcs.glide_legs.contains_key(&21), "the leg is armed");
    assert!(w.field_vm.channels[0].ctx.is_halted());
    for _ in 0..8 {
        w.tick_field_npc_motions();
    }
    assert_eq!(w.npcs.positions.get(&21), Some(&(12928, 9920)));
    assert!(w.npcs.glide_legs.is_empty(), "the leg landed");
    assert!(
        !w.field_vm.channels[0].ctx.is_halted(),
        "landing clears 0x400"
    );
}

/// The cross-context form (`C1 <id> b0 b1`) in one placement's spawn
/// section walks the placement it names, not the poking record's own actor:
/// direction `1` is `-X -Z` in the axis table at `0x80073F14`.
#[test]
fn a_spawn_prologue_cross_context_compass_walk_moves_its_target() {
    let mut w = World {
        mode: legaia_engine_mode::mode::SceneMode::Field,
        ..World::default()
    };
    // Record 0 pokes channel `0x16`; record 1 (offset 6) does nothing.
    let man = vec![0x25, 0xC1, 0x16, 0x01, 0x01, 0x21, 0x25, 0x21];
    w.field_vm.channels = vec![prologue_channel(3, 0x10, 0), prologue_channel(9, 0x16, 6)];
    w.field_vm.channels_man = Some(std::sync::Arc::new(man));
    w.npcs.positions.insert(3, (1000, 1000));
    w.npcs.positions.insert(9, (2240, 2368));
    w.pre_run_field_channel_prologues();
    assert!(w.npcs.glide_legs.contains_key(&9));
    assert!(!w.npcs.glide_legs.contains_key(&3));
    for _ in 0..8 {
        w.tick_field_npc_motions();
    }
    assert_eq!(w.npcs.positions.get(&9), Some(&(2176, 2304)));
    assert_eq!(w.npcs.positions.get(&3), Some(&(1000, 1000)));
}
