//! The player's `+0x72` word - the pad step's speed multiplier and the
//! animated renderer's scale, `0` = "do not draw" (`FUN_8001B964`) - reached
//! from any script that aims op `4C` nibble-4 sub-0 at the player (`0xF8`),
//! immediate or ramped, and the overworld walk's displacement over time.

use super::*;
use crate::world::vm_hosts::{FieldHostImpl, field_step_routed};

fn field_world() -> World {
    let mut w = World::new();
    w.mode = SceneMode::Field;
    w.install_field_player(0);
    w
}

/// Run `bc` from `pc` on a context that is NOT the player's - the way a
/// cutscene timeline, a placement channel or an inline talk issues the op.
fn run_on_foreign_ctx(w: &mut World, bc: &[u8]) -> FieldStepResult {
    let mut ctx = FieldCtx {
        script_id: 0x31,
        ..Default::default()
    };
    let mut host = FieldHostImpl { world: w };
    field_step_routed(&mut host, &mut ctx, bc, 0)
}

#[test]
fn a_player_aimed_scale_write_from_any_script_lands_on_the_player() {
    let mut w = field_world();
    assert_eq!(w.actors[0].move_state.field_72, 0x1000);
    // Cutscene hide: `CC F8 40 00 00 00 00`.
    let r = run_on_foreign_ctx(&mut w, &[0xCC, 0xF8, 0x40, 0x00, 0x00, 0x00, 0x00]);
    assert!(matches!(r, FieldStepResult::Advance { next_pc: 7 }));
    assert_eq!(w.actors[0].move_state.field_72, 0);
    assert!(w.player_hidden());
    assert_eq!(w.player_render_scale(), 0.0);
    // Restore: `CC F8 40 00 10 00 00`.
    run_on_foreign_ctx(&mut w, &[0xCC, 0xF8, 0x40, 0x00, 0x10, 0x00, 0x00]);
    assert_eq!(w.actors[0].move_state.field_72, 0x1000);
    assert!(!w.player_hidden());
    assert_eq!(w.player_render_scale(), 1.0);
}

#[test]
fn a_scale_write_aimed_elsewhere_leaves_the_player_alone() {
    let mut w = field_world();
    let mut ctx = FieldCtx::default();
    let mut host = FieldHostImpl { world: &mut w };
    field_step_routed(
        &mut host,
        &mut ctx,
        &[0xCC, 0x31, 0x40, 0x00, 0x00, 0x00, 0x00],
        0,
    );
    assert_eq!(ctx.field_72, 0, "the op ran on the caller's context");
    assert_eq!(w.actors[0].move_state.field_72, 0x1000);
}

#[test]
fn the_system_script_scale_write_lands_on_the_player() {
    let mut w = field_world();
    // The kingdom maps' entry script opener.
    w.load_field_script(vec![0xCC, 0xF8, 0x40, 0x00, 0x0C, 0x00, 0x00, 0x21]);
    w.step_field();
    assert_eq!(w.actors[0].move_state.field_72, 0x0C00);
    assert_eq!(w.player_render_scale(), 0.75);
    assert_eq!(w.field_ctx.field_72, 0, "the system context keeps its own");
}

#[test]
fn a_player_aimed_scale_ramp_lerps_the_player_each_frame() {
    let mut w = field_world();
    run_on_foreign_ctx(&mut w, &[0xCC, 0xF8, 0x40, 0x00, 0x00, 0x00, 0x00]);
    // `CC F8 40 00 10 04 00`: back to 0x1000 over 4 frames.
    run_on_foreign_ctx(&mut w, &[0xCC, 0xF8, 0x40, 0x00, 0x10, 0x04, 0x00]);
    assert_eq!(
        w.actors[0].move_state.field_72, 0,
        "a ramp writes nothing at install"
    );
    let mut seen = Vec::new();
    for _ in 0..5 {
        w.tick_player_scale_ramp();
        seen.push(w.actors[0].move_state.field_72);
    }
    // end + (start - end) * remaining / total, remaining 3, 2, 1, 0.
    assert_eq!(seen, vec![0x400, 0x800, 0xC00, 0x1000, 0x1000]);
    assert_eq!(w.locomotion.player_scale_ramps.active(), 0);
}

#[test]
fn scene_entry_drops_a_running_player_scale_ramp() {
    let mut w = field_world();
    run_on_foreign_ctx(&mut w, &[0xCC, 0xF8, 0x40, 0x00, 0x00, 0x64, 0x00]);
    assert_eq!(w.locomotion.player_scale_ramps.active(), 1);
    w.install_field_player(0);
    assert_eq!(w.locomotion.player_scale_ramps.active(), 0);
    assert_eq!(w.actors[0].move_state.field_72, 0x1000);
}

/// The overworld walk lands retail's `dt = 3` displacement over time: the
/// slow step `(5 * 0xC00) >> 12 = 3`, times the frame step, rounded up to
/// `10` by the 2-unit stepper, every three vsyncs - `130` units per `39`
/// (the captured map01 tile-crossing rate), not `4` every vsync.
#[test]
fn overworld_walk_moves_ten_units_every_three_vsyncs() {
    let mut w = World::new();
    w.install_field_player(0);
    w.enter_world_map();
    w.party.scene_save_allowed = true;
    w.actors[0].move_state.field_72 = 0x0C00;
    w.actors[0].move_state.world_x = 2000;
    w.actors[0].move_state.world_z = 2000;
    if let Some(c) = w.world_map.ctrl.as_mut() {
        c.view_mode = 0;
    }
    let z0 = i32::from(w.actors[0].move_state.world_z);
    let x0 = i32::from(w.actors[0].move_state.world_x);
    let mut per_tick = Vec::new();
    let mut last = 0;
    for _ in 0..39 {
        w.set_pad(input::PadButton::Up.mask());
        let _ = w.tick();
        let ms = &w.actors[0].move_state;
        let d = (i32::from(ms.world_z) - z0).abs() + (i32::from(ms.world_x) - x0).abs();
        per_tick.push(d - last);
        last = d;
    }
    assert_eq!(last, 130, "per-tick steps {per_tick:?}");
    for chunk in per_tick.chunks(3) {
        assert_eq!(chunk.iter().sum::<i32>(), 10, "{per_tick:?}");
    }
    // Releasing the pad drops the carry: a fresh press starts from zero.
    w.set_pad(0);
    let _ = w.tick();
    assert_eq!(w.world_map.walk_carry, 0);
}

/// A talk the runner ends before it reaches its own `CC F8 40 00 10` restore
/// does not leave the player hidden (undrawn and, through the same word,
/// unable to walk): the restore the record still owes lands at teardown.
#[test]
fn a_talk_cut_before_its_player_restore_still_shows_the_player() {
    let mut w = field_world();
    // Hide, a terminator the runner stops on, then the restore it never ran.
    w.start_inline_dialogue(vec![
        0xCC, 0xF8, 0x40, 0x00, 0x00, 0x00, 0x00, //
        0x00, //
        0xCC, 0xF8, 0x40, 0x00, 0x10, 0x00, 0x00,
    ]);
    w.step_inline_dialogue(false, false, false);
    assert!(w.dialog.inline.as_ref().is_some_and(|id| id.done));
    assert_eq!(w.actors[0].move_state.field_72, 0x1000);
    assert!(!w.player_hidden());
}

/// The rescue never invents a restore: a record that hides the player and
/// carries no later restore leaves it hidden, as retail would.
#[test]
fn a_talk_with_no_pending_restore_leaves_the_player_hidden() {
    let mut w = field_world();
    w.start_inline_dialogue(vec![0xCC, 0xF8, 0x40, 0x00, 0x00, 0x00, 0x00, 0x00]);
    w.step_inline_dialogue(false, false, false);
    assert!(w.dialog.inline.as_ref().is_some_and(|id| id.done));
    assert!(w.player_hidden());
}
