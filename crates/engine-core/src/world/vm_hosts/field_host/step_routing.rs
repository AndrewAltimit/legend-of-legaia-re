//! Routed field-VM stepping: which context an opcode's player-facing writes
//! land on.

use super::*;

/// `true` when the op at `pc` is `CC F8 40`: op `4C` nibble-4 sub-0 (the
/// `+0x72` write-or-ramp) aimed at the player anchor `0xF8`.
pub(in crate::world) fn is_player_scale_op(bytecode: &[u8], pc: usize) -> bool {
    bytecode.get(pc) == Some(&0xCC)
        && bytecode.get(pc + 1) == Some(&legaia_engine_scene::field_env::PLAYER_ANCHOR_TARGET)
        && bytecode.get(pc + 2) == Some(&0x40)
}

/// `true` when the op at `pc` is `CC F8 C2`: op `4C` nibble-C sub-2 (the
/// `+0x42` byte write, `0x801E26F0..0x801E26FC`) aimed at the player anchor.
/// Five shipped sites raise or lower the player's object-effect gate this way.
pub(in crate::world) fn is_player_effect_gate_op(bytecode: &[u8], pc: usize) -> bool {
    bytecode.get(pc) == Some(&0xCC)
        && bytecode.get(pc + 1) == Some(&legaia_engine_scene::field_env::PLAYER_ANCHOR_TARGET)
        && matches!(bytecode.get(pc + 2), Some(&(0xC2 | 0x45)))
}

/// `true` when the op at `pc` is a player-aimed height or tilt write:
/// `CC F8 42` / `43` / `44` (nibble-4 sub-2..4, `+0x8E` / `+0x24` / `+0x28`)
/// or `CC F8 D9` / `DA` (the nibble-D height-law raise and drop,
/// `0x801E2E28..0x801E2E94`).
pub(in crate::world) fn is_player_height_op(bytecode: &[u8], pc: usize) -> bool {
    bytecode.get(pc) == Some(&0xCC)
        && bytecode.get(pc + 1) == Some(&legaia_engine_scene::field_env::PLAYER_ANCHOR_TARGET)
        && matches!(bytecode.get(pc + 2), Some(&(0x42..=0x44 | 0xD9 | 0xDA)))
}

/// Step one field-VM op, landing a player-aimed `+0x72` write on the player.
///
/// `FUN_8003C83C` resolves the extended target `0xF8` to the live player
/// object (`_DAT_8007C364`), so `CC F8 40 lo hi tlo thi` writes - or, with a
/// non-zero tick count, ramps - the **player's** `+0x72`: the pad step's speed
/// multiplier and `FUN_8001B964`'s render scale, where `0` is "do not draw".
/// Every runner that can meet the op (the system script, the cutscene
/// timeline, the placement channels, an inline talk, a prop run) calls this in
/// place of [`vm::field::step`], so the op runs on a stand-in player context
/// seeded from the live word and the result is written back, whichever
/// script issued it. Cutscenes hide the player this way (`CC F8 40 00 00 ..`
/// before a stand-in walks) and restore it with `CC F8 40 00 10 ..`.
///
/// Every other op passes straight through to [`vm::field::step`].
///
/// REF: FUN_8003C83C, FUN_801DE840 (the nibble-4 sub-0 arm `0x801E1174..0x801E11A8`)
pub(in crate::world) fn field_step_routed(
    host: &mut FieldHostImpl<'_>,
    ctx: &mut FieldCtx,
    bytecode: &[u8],
    pc: usize,
) -> vm::field::StepResult {
    // `CC F8 C2 <b>`: the player's object-effect gate `+0x42`, on the same
    // stand-in context the scale op uses, seeded from and written back to the
    // world's player word. `CC F8 45 ..` (the player's look rotation) runs on
    // the same stand-in, which `look_key` reads as the player.
    if is_player_effect_gate_op(bytecode, pc) {
        let mut player_ctx = FieldCtx {
            script_id: u16::from(legaia_engine_scene::field_env::PLAYER_ANCHOR_TARGET),
            flags: 0x0100_0000,
            field_42: host.world.field_vm.player_field_42,
            ..Default::default()
        };
        let r = vm::field::step(host, &mut player_ctx, bytecode, pc);
        host.world.field_vm.player_field_42 = player_ctx.field_42;
        return r;
    }
    let slot = host
        .world
        .player_actor_slot
        .map(usize::from)
        .filter(|&s| s < host.world.actors.len());
    // `CC F8 42..44` / `CC F8 D9|DA`: the player's `+0x8E` / `+0x24` /
    // `+0x28` and its `0x20000000` height law, on a stand-in seeded from the
    // live words and written back. A tween is scheduled by
    // `ramp_ctx_slot` onto the player's slot pool; `4C DA` drops the law and
    // snaps the player back onto the floor under it.
    if let Some(slot) = slot.filter(|_| is_player_height_op(bytecode, pc)) {
        let ms = &host.world.actors[slot].move_state;
        let mut player_ctx = FieldCtx {
            script_id: u16::from(legaia_engine_scene::field_env::PLAYER_ANCHOR_TARGET),
            flags: ms.flags | 0x0100_0000,
            field_8e: host.world.locomotion.player_8e,
            field_24: ms.render_24,
            field_28: ms.render_28,
            world_y: ms.world_y as u16,
            ..Default::default()
        };
        let r = vm::field::step(host, &mut player_ctx, bytecode, pc);
        let law = crate::world::field_loop::PLAYER_Y_FROM_8E;
        host.world.locomotion.player_8e = player_ctx.field_8e;
        let dropped = bytecode.get(pc + 2) == Some(&0xDA);
        if let Some(a) = host.world.actors.get_mut(slot) {
            let ms = &mut a.move_state;
            ms.flags = (ms.flags & !law) | (player_ctx.flags & law);
            ms.render_24 = player_ctx.field_24;
            ms.render_28 = player_ctx.field_28;
            if !dropped {
                ms.world_y = player_ctx.world_y as i16;
            }
        }
        if dropped {
            let (x, z) = {
                let ms = &host.world.actors[slot].move_state;
                (i32::from(ms.world_x), i32::from(ms.world_z))
            };
            let floor = host.world.sample_field_floor_height(x, z);
            host.world.actors[slot].move_state.world_y =
                floor.clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16;
        }
        return r;
    }
    let Some(slot) = slot.filter(|_| is_player_scale_op(bytecode, pc)) else {
        return vm::field::step(host, ctx, bytecode, pc);
    };
    let mut player_ctx = FieldCtx {
        script_id: u16::from(legaia_engine_scene::field_env::PLAYER_ANCHOR_TARGET),
        flags: 0x0100_0000,
        field_72: host.world.actors[slot].move_state.field_72,
        ..Default::default()
    };
    let r = vm::field::step(host, &mut player_ctx, bytecode, pc);
    if let Some(a) = host.world.actors.get_mut(slot) {
        a.move_state.field_72 = player_ctx.field_72;
    }
    r
}
