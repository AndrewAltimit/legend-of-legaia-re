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

/// Op `4C 42 <value> <ticks>` on a placement (`rayman` `P2[7]`'s
/// `CC 54 42 00 FD F0 00`, a bridge plank): the tween lands in the
/// placement context's `+0x8E` on retail's ramp law
/// (`end + (start - end) * remaining / total`, `FUN_80036D80`), and the
/// height arm mirrors it into the drawn Y - the plank falls, it does not
/// hold at the ledge.
#[test]
fn a_ramped_height_write_drops_a_held_height_placement() {
    let mut w = world_with_placement();
    let mut c = legaia_engine_field::field_channels::FieldChannel {
        placement_index: 4,
        ctx: FieldCtx {
            script_id: 0x1E,
            flags: 0x2000_0000,
            field_8e: 320,
            world_x: 6 * 128 + 64,
            world_z: 2 * 128 + 64,
            ..Default::default()
        },
        record_offset: 0,
        pc: 0,
        done: false,
        object_bind: false,
    };
    let mut host = FieldHostImpl { world: &mut w };
    // 320 -> -768 over 240 frames.
    let r = field_step_routed(
        &mut host,
        &mut c.ctx,
        &[0x4C, 0x42, 0x00, 0xFD, 0xF0, 0x00],
        0,
    );
    assert!(matches!(r, FieldStepResult::Advance { next_pc: 6 }));
    assert_eq!(c.ctx.field_8e, 320, "the ramp arm writes nothing at once");
    w.field_vm.channels.push(c);
    w.move_vm.ramp_ratio = 1;
    let y = |w: &World| w.field_npc_render_y(4, 6 * 128 + 64, 2 * 128 + 64);
    w.tick_field_npc_heights();
    assert_eq!(y(&w), -320);
    for _ in 0..120 {
        w.tick_npc_heading_ramps();
    }
    w.tick_field_npc_heights();
    // Halfway: -768 + (320 + 768) * 120 / 240 = -224.
    assert_eq!(w.field_vm.channels[0].ctx.field_8e, -224);
    assert_eq!(y(&w), 224);
    for _ in 0..120 {
        w.tick_npc_heading_ramps();
    }
    w.tick_field_npc_heights();
    assert_eq!(y(&w), 768);
    assert_eq!(w.npcs.slot_ramps.active(), 0);
}

fn plank_channel(placement_index: usize) -> legaia_engine_field::field_channels::FieldChannel {
    legaia_engine_field::field_channels::FieldChannel {
        placement_index,
        ctx: FieldCtx {
            script_id: 0x1E,
            ..Default::default()
        },
        record_offset: 0,
        pc: 0,
        done: false,
        object_bind: false,
    }
}

/// Op `4C 43 <value> <ticks>` on a placement: the X-angle tween
/// (`0x801E124C..0x801E1268`) lands in the placement context's `+0x24` and
/// draws as the NPC's tilt; `4C 44` does the same for `+0x28`.
#[test]
fn a_ramped_pitch_write_tilts_the_placement() {
    let mut w = world_with_placement();
    let mut c = plank_channel(4);
    let mut host = FieldHostImpl { world: &mut w };
    // `+0x24`: 0 -> 0x400 over 40 frames; `+0x28`: 0 -> -0x200 at once.
    field_step_routed(
        &mut host,
        &mut c.ctx,
        &[0x4C, 0x43, 0x00, 0x04, 0x28, 0x00],
        0,
    );
    field_step_routed(
        &mut host,
        &mut c.ctx,
        &[0x4C, 0x44, 0x00, 0xFE, 0x00, 0x00],
        0,
    );
    assert_eq!(c.ctx.field_24, 0, "the ramp arm writes nothing at once");
    assert_eq!(c.ctx.field_28, -0x200);
    w.field_vm.channels.push(c);
    w.move_vm.ramp_ratio = 1;
    for _ in 0..20 {
        w.tick_npc_heading_ramps();
    }
    assert_eq!(w.field_npc_tilt(4), Some((0x200, -0x200)));
    for _ in 0..20 {
        w.tick_npc_heading_ramps();
    }
    assert_eq!(w.field_npc_tilt(4), Some((0x400, -0x200)));
}

/// The height arm's mirror lands in the placement's own `+0x16`: a context
/// reader sees the held height (`kor5` `P2[5]` raises `0x20000000` after
/// writing `+0x8E = 480`, so only the arm puts the figure at `Y = -480`).
#[test]
fn the_held_height_reaches_the_placement_context() {
    let mut w = world_with_placement();
    let mut c = plank_channel(4);
    c.ctx.flags = 0x2000_0000;
    c.ctx.field_8e = 480;
    w.field_vm.channels.push(c);
    w.tick_field_npc_heights();
    assert_eq!(w.field_vm.channels[0].ctx.world_y as i16, -480);
}

/// The player's held height: `B1 F8 1D` raises its `0x20000000`, and a
/// `CC F8 42 <value> <ticks>` tween of its `+0x8E` then carries its Y on the
/// scheduler law; `CC F8 DA` drops the law and snaps it back to the floor.
#[test]
fn a_player_height_tween_lifts_the_player() {
    let mut w = world_with_placement();
    w.field_vm.executing_channel = None;
    let slot = w.player_actor_slot.unwrap() as usize;
    let mut caller = FieldCtx::default();
    let mut host = FieldHostImpl { world: &mut w };
    field_step_routed(&mut host, &mut caller, &[0xB1, 0xF8, 0x1D], 0);
    // 0 -> 400 over 50 frames.
    field_step_routed(
        &mut host,
        &mut caller,
        &[0xCC, 0xF8, 0x42, 0x90, 0x01, 0x32, 0x00],
        0,
    );
    assert_eq!(caller.flags, 0, "the caller's own word stays untouched");
    assert_ne!(w.actors[slot].move_state.flags & 0x2000_0000, 0);
    w.move_vm.ramp_ratio = 1;
    for _ in 0..25 {
        w.tick_player_scale_ramp();
    }
    assert_eq!(w.locomotion.player_8e, 200);
    assert_eq!(w.field_actor_mirrored_y(slot), Some(-200));
    for _ in 0..25 {
        w.tick_player_scale_ramp();
    }
    assert_eq!(w.field_actor_mirrored_y(slot), Some(-400));
    let mut host = FieldHostImpl { world: &mut w };
    field_step_routed(&mut host, &mut caller, &[0xCC, 0xF8, 0xDA], 0);
    assert_eq!(w.field_actor_mirrored_y(slot), None);
    assert_eq!(w.actors[slot].move_state.world_y, 0, "back on the floor");
}

/// `4C 44 <value> <ticks>` on a placed object's actor: the `+0x28` tween
/// turns the object's draw (`object_draw_turns`), as `4C 42` lowers it.
#[test]
fn a_ramped_roll_write_turns_a_placed_object() {
    let mut w = world_with_placement();
    w.field_vm.executing_channel = None;
    w.field_vm.executing_object = Some(0x12);
    let mut c = plank_channel(9);
    c.object_bind = true;
    c.ctx.script_id = 0x12;
    w.field_vm.object_channel_rots.insert(9, [0, 0, 0]);
    let mut host = FieldHostImpl { world: &mut w };
    // `+0x28`: 0 -> 0x300 over 3 actor ticks.
    field_step_routed(
        &mut host,
        &mut c.ctx,
        &[0x4C, 0x44, 0x00, 0x03, 0x03, 0x00],
        0,
    );
    w.field_vm.executing_object = None;
    w.field_vm.channels.push(c);
    w.tick_object_actor_heights();
    assert_eq!(w.object_draw_turns().get(&9), Some(&[0, 0, 0x100]));
    w.tick_object_actor_heights();
    w.tick_object_actor_heights();
    assert_eq!(w.object_draw_turns().get(&9), Some(&[0, 0, 0x300]));
}

/// `CC F8 43 <value> <ticks>` / `CC F8 44 <value> <ticks>` on the player:
/// the `+0x24` tween and the `+0x28` write land on the player's move state
/// and come back out as its draw tilt (`World::player_draw_tilt`, which both
/// play hosts compose into the player's matrix); a scene entry clears them.
#[test]
fn a_player_pitch_tween_and_roll_write_tilt_the_player_draw() {
    let mut w = world_with_placement();
    w.field_vm.executing_channel = None;
    let slot = w.player_actor_slot.unwrap();
    let mut caller = FieldCtx::default();
    let mut host = FieldHostImpl { world: &mut w };
    // `+0x24`: 0 -> 0x200 over 32 frames; `+0x28`: -0x100 at once.
    field_step_routed(
        &mut host,
        &mut caller,
        &[0xCC, 0xF8, 0x43, 0x00, 0x02, 0x20, 0x00],
        0,
    );
    field_step_routed(
        &mut host,
        &mut caller,
        &[0xCC, 0xF8, 0x44, 0x00, 0xFF, 0x00, 0x00],
        0,
    );
    assert_eq!(
        (caller.field_24, caller.field_28),
        (0, 0),
        "the caller stays untilted"
    );
    assert_eq!(w.player_draw_tilt(), (0, 0xF00));
    w.move_vm.ramp_ratio = 1;
    for _ in 0..16 {
        w.tick_player_scale_ramp();
    }
    assert_eq!(w.player_draw_tilt(), (0x100, 0xF00));
    for _ in 0..16 {
        w.tick_player_scale_ramp();
    }
    assert_eq!(w.player_draw_tilt(), (0x200, 0xF00));
    w.install_field_player(slot);
    assert_eq!(w.player_draw_tilt(), (0, 0));
}
