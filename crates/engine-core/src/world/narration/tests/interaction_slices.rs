//! Interaction timelines and the system script: touch resumes, player holds,
//! seats and walks within a slice, and helper contexts parked on text.

use crate::world::World;

/// A timeline that is a placement's own touch-resumed context
/// (`interaction_slot`) over `bc`, with that placement's channel sharing
/// the bytes at PC 0.
fn interaction_timeline(bc: Vec<u8>, slot: u8) -> World {
    use legaia_engine_field::cutscene_timeline::CutsceneTimeline;
    use legaia_engine_field::field_channels::FieldChannel;
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
    use legaia_engine_field::cutscene_timeline::CutsceneTimeline;
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
    use legaia_engine_field::cutscene_timeline::CutsceneTimeline;
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
    use legaia_engine_field::cutscene_timeline::CutsceneTimeline;
    use legaia_engine_field::field_channels::FieldChannel;
    use legaia_engine_vm::field::FieldCtx;
    // `CC 40 51 10 10 00 00` seats channel 0x40 at tile (16,16), then
    // `C7 40 12 10 33` walks it to (18,16) - the shape of every cutscene
    // that places an actor parked at the hide box and walks it in.
    let bc = vec![
        0xCC, 0x40, 0x51, 0x10, 0x10, 0x00, 0x00, 0xC7, 0x40, 0x12, 0x10, 0x33, 0x4A, 0x40, 0x00,
    ];
    let mut w = World::new();
    w.cutscene.timeline = Some(CutsceneTimeline::new(bc, 0));
    let hide = legaia_engine_field::man_field_scripts::FIELD_OFFMAP_HIDE_XZ;
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
    use legaia_engine_field::cutscene_timeline::CutsceneTimeline;
    use legaia_engine_field::field_channels::FieldChannel;
    use legaia_engine_mode::mode::SceneMode;
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
    use legaia_engine_field::cutscene_timeline::CutsceneTimeline;
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

/// A talker whose own context carries the party-bank class bit (`31 18`)
/// seats ANOTHER actor with a cross-context MOVE_TO: the player stays where
/// it stands. Retail's player arm is an identity compare of the resolved
/// context against the player (`0x801DEC7C`), not the class bit. `vozz`
/// P1[1] parks its helper actors at the hide tile this way.
#[test]
fn a_talk_seating_another_actor_leaves_the_player_where_it_stands() {
    let mut w = World::new();
    w.install_field_player(0);
    w.actors[0].move_state.world_x = 4162;
    w.actors[0].move_state.world_z = 9284;
    // `31 18` (the class bit), `A3 06 7F 7F` (park actor 6), end.
    w.start_inline_dialogue(vec![0x31, 0x18, 0xA3, 0x06, 0x7F, 0x7F, 0x21]);
    for _ in 0..4 {
        w.step_inline_dialogue(false, false, false);
    }
    let ms = &w.actors[0].move_state;
    assert_eq!(
        (ms.world_x, ms.world_z),
        (4162, 9284),
        "the seat names actor 6, not the player"
    );
}

/// A talk whose lead plays the talker's own clip and waits on its end latch
/// (`2C 08` / `2D 08`) runs the ops behind the wait even when the port holds
/// no clip cursor for the talker: the latch is the anim tick's
/// (`FUN_800204F8`), it always lands, and the record goes on from the op
/// after it. Falling back to the first text segment instead skipped the
/// grant and the flag between them - `vozz` P1[1] gives item `0xAA` and
/// raises `0x079` there.
#[test]
fn an_own_clip_wait_with_no_cursor_runs_the_ops_behind_it() {
    let mut w = World::new();
    w.install_field_player(0);
    // Clear the latch, wait on it, raise `0x079`, one text segment, end.
    w.start_inline_dialogue_with_prologue(
        vec![0x2C, 0x08, 0x2D, 0x08, 0x50, 0x79, 0x1F, b'A', 0x00, 0x21],
        0,
        6,
    );
    for _ in 0..4 {
        w.step_inline_dialogue(false, false, false);
    }
    assert!(
        w.system_flag_test(0x079),
        "the flag behind the wait is raised before the box opens"
    );
}

#[test]
fn a_copy_from_player_then_walk_starts_at_the_player() {
    use legaia_engine_field::cutscene_timeline::CutsceneTimeline;
    use legaia_engine_field::field_channels::FieldChannel;
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
    let hide = legaia_engine_field::man_field_scripts::FIELD_OFFMAP_HIDE_XZ;
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
    use legaia_engine_field::cutscene_timeline::CutsceneTimeline;
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
            legaia_engine_system::input::PadButton::Cross.mask()
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
    // The dismissing press's own pager call still draws the box (state `9`
    // for a byte outside the dispatch table, `0x801D8FE0..0x801D8FF8`);
    // it is gone by the next.
    assert!(
        w.script_dialog_panel()
            .is_some_and(|p| !p.is_waiting_for_input())
    );
    w.set_pad(0);
    w.step_helper_contexts();
    assert!(w.script_dialog_panel().is_none());
}

#[test]
fn a_parked_box_is_not_drawn_over_a_battle() {
    use legaia_engine_field::cutscene_timeline::CutsceneTimeline;
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
    w.mode = legaia_engine_mode::mode::SceneMode::Battle;
    assert!(w.script_dialog_panel().is_none(), "no box over the fight");
    w.mode = legaia_engine_mode::mode::SceneMode::Field;
    assert!(
        w.script_dialog_panel().is_some(),
        "the park survives the fight"
    );
}

#[test]
fn a_helper_off_text_keeps_its_slice_while_the_box_is_taken() {
    use legaia_engine_field::cutscene_timeline::CutsceneTimeline;
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
            legaia_engine_system::input::PadButton::Cross.mask()
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
            legaia_engine_system::input::PadButton::Cross.mask()
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
    use legaia_engine_field::cutscene_timeline::CutsceneTimeline;
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
