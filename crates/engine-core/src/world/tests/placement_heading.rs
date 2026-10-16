//! Op `4C` nibble-4 sub-8 (`4C 48 <value> <ticks>`) on a placement: the
//! context's `+0x26` is the heading the actor draws at, so the write (or
//! the scheduled tween) lands in the placement's heading table. nilboa's
//! Delilas pair stand at `0x300` from their spawn prologue's
//! `4C 48 00 03 00 00`.

use super::*;
use crate::world::vm_hosts::{FieldHostImpl, field_step_routed};
use legaia_engine_mode::mode::SceneMode;

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

/// Op `43 07` on a placement (`rugi`'s `C3 <id> 07 01 07 ..` shape, here with
/// the shrink flag and an 8-frame ramp): the row is written, `+0x7A` ramps
/// `0 -> 0x1000`, and the NPC's draw becomes a shatter over the row with it
/// (the pose itself is untouched); `43 08` puts the body back.
#[test]
fn op_43_07_ramps_the_face_deformation_into_the_draw_pose() {
    let mut w = world_with_placement();
    w.npcs.positions.insert(4, (1000, 2000));
    let mut ctx = FieldCtx {
        script_id: 0x1E,
        ..Default::default()
    };
    let mut host = FieldHostImpl { world: &mut w };
    field_step_routed(
        &mut host,
        &mut ctx,
        &[
            0x43, 0x07, 0x01, 0x04, 0x00, 0x00, 0x00, 0x00, 0x00, 0xE0, 0xFF, 0x40, 0x00, 0x00,
            0x00, 0x08, 0x00,
        ],
        0,
    );
    assert_eq!(w.npcs.face_rows[&1].offset, [0, -32, 64, 0]);
    assert_eq!(w.npcs.face_rows[&1].flags, 4);
    w.move_vm.ramp_ratio = 1;
    for _ in 0..4 {
        w.tick_npc_heading_ramps();
    }
    assert_eq!(w.npcs.faces[&4], (1, 0x800));
    let pose = w.field_npc_draw_pose(4, (0, 0)).expect("drawn");
    assert_eq!(pose.scale, 1.0, "the pose is not folded");
    let s = w.field_npc_shatter(4).expect("half way: shattering");
    assert_eq!(s.word, 0x0004_0800, "shrink flag over +0x7A");
    assert_eq!(s.row.off, [0, -32, 64]);
    let mut host = FieldHostImpl { world: &mut w };
    field_step_routed(&mut host, &mut ctx, &[0x43, 0x08], 0);
    assert_eq!(w.npcs.faces[&4], (0, 0));
    assert!(w.field_npc_shatter(4).is_none(), "43 08: whole again");
}
