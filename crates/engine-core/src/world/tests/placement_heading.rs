//! Op `4C` nibble-4 sub-8 (`4C 48 <value> <ticks>`) on a placement: the
//! context's `+0x26` is the heading the actor draws at, so the write (or
//! the scheduled tween) lands in the placement's heading table. nilboa's
//! Delilas pair stand at `0x300` from their spawn prologue's
//! `4C 48 00 03 00 00`.

use super::*;
use crate::world::vm_hosts::{FieldHostImpl, field_step_routed};

fn world_with_placement() -> World {
    let mut w = World::new();
    w.mode = SceneMode::Field;
    w.install_field_player(0);
    w.field_vm.executing_channel = Some(4);
    w
}

#[test]
fn an_immediate_heading_write_turns_the_placement() {
    let mut w = world_with_placement();
    let mut ctx = FieldCtx {
        script_id: 0x1E,
        ..Default::default()
    };
    let mut host = FieldHostImpl { world: &mut w };
    let r = field_step_routed(
        &mut host,
        &mut ctx,
        &[0x4C, 0x48, 0x00, 0x03, 0x00, 0x00],
        0,
    );
    assert!(matches!(r, FieldStepResult::Advance { next_pc: 6 }));
    assert_eq!(ctx.field_26, 0x300);
    // Engine space is retail + 0x800.
    assert_eq!(w.npcs.headings.get(&4), Some(&0x0B00));
}

#[test]
fn a_ramped_heading_write_tweens_from_the_live_heading() {
    let mut w = world_with_placement();
    w.npcs.headings.insert(4, 0x800); // retail 0
    let mut ctx = FieldCtx {
        script_id: 0x1E,
        ..Default::default()
    };
    let mut host = FieldHostImpl { world: &mut w };
    // Retail 0 -> 0x1000 over 80 frames: one full turn.
    field_step_routed(
        &mut host,
        &mut ctx,
        &[0x4C, 0x48, 0x00, 0x10, 0x50, 0x00],
        0,
    );
    w.move_vm.ramp_ratio = 1;
    for _ in 0..40 {
        w.tick_npc_heading_ramps();
    }
    assert_eq!(w.npcs.headings.get(&4), Some(&(0x800 + 0x800)));
    for _ in 0..40 {
        w.tick_npc_heading_ramps();
    }
    assert_eq!(w.npcs.headings.get(&4), Some(&(0x1000 + 0x800)));
    assert_eq!(w.npcs.heading_ramps.active(), 0);
}
