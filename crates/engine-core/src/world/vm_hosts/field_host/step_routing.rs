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
    // A cross-context kill, from a runner standing in for the target: `cave01`
    // P1[2]'s hand-off talk retires placements `0x04..=0x08` with `B1 <id> 03`.
    // Stepped on the talker's context it would retire the talker.
    // REF: FUN_801DE840 (op 0x31, cross-context prefix), FUN_8003C83C
    if bytecode.get(pc) == Some(&0xB1)
        && bytecode.get(pc + 2).is_some_and(|b| b & 0x1F == 3)
        && let Some(&target) = bytecode.get(pc + 1)
        && target != legaia_engine_scene::field_env::PLAYER_ANCHOR_TARGET
        && target != 0xFB
    {
        host.world.field_vm.pending_retire_ids.push(target);
        return FieldStepResult::Advance { next_pc: pc + 3 };
    }
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
    // `CC F8 41 <lo> <hi> 00 00`: the player's clip rate `+0x6A`, which the
    // player's anim tick steps its cursor by. The arm halves the operand and
    // floors it at `1`; the port keeps the player's rate on
    // the locomotion state, which the clip tick reads. `bylon` `P1[47]`'s
    // talk slows Vahn's bow with `CC F8 41 20 ..` and restores it with
    // `CC F8 41 10 ..`; only the cutscene timeline carried it before.
    // REF: FUN_801DE840 (the nibble-4 sub-1 arm), FUN_8003C83C
    if bytecode.get(pc) == Some(&0xCC)
        && bytecode.get(pc + 1) == Some(&legaia_engine_scene::field_env::PLAYER_ANCHOR_TARGET)
        && bytecode.get(pc + 2) == Some(&0x41)
        && let Some(b) = bytecode.get(pc + 3..pc + 7)
        && b[2] == 0
        && b[3] == 0
    {
        let rate = i16::from_le_bytes([b[0], b[1]]);
        host.world.locomotion.player_clip_rate = (rate >> 1).max(1);
    }
    // `B4 F8 3x <arg>`: the ambient-tree install staged at `s5 + 0x14` /
    // `s5 + 0x24` (`0x801E00B0`), which `0xF8` makes the player's position
    // and rotation words. `bubu2`'s entry script installs one on Vahn; on the
    // issuing context it staged wherever that context stood, facing nowhere.
    // REF: FUN_801DE840 (0x801E00B0), FUN_800252EC, FUN_8003C83C
    if let Some(slot) = slot
        && bytecode.get(pc) == Some(&0xB4)
        && bytecode.get(pc + 1) == Some(&legaia_engine_scene::field_env::PLAYER_ANCHOR_TARGET)
        && bytecode.get(pc + 2).is_some_and(|s| s >> 4 == 3)
    {
        let ms = &host.world.actors[slot].move_state;
        let mut player_ctx = FieldCtx {
            script_id: u16::from(legaia_engine_scene::field_env::PLAYER_ANCHOR_TARGET),
            flags: ms.flags | 0x0100_0000,
            world_x: ms.world_x as u16,
            world_y: ms.world_y as u16,
            world_z: ms.world_z as u16,
            field_24: ms.render_24,
            // Retail's `+0x26`: the port holds the player's heading a
            // half-turn round (`render_26`).
            field_26: (ms.render_26 as u16).wrapping_sub(0x800) & 0x0FFF,
            field_28: ms.render_28,
            ..Default::default()
        };
        return vm::field::step(host, &mut player_ctx, bytecode, pc);
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

/// `true` when the op at `pc` is one whose dispatcher arm reads or writes the
/// words of the actor in `s5` - the context the prologue swapped in for an
/// extended target (`0x801DE894..0x801DE8B4`): the clip `22` (`+0x5C`, the
/// re-point), the clip-control word `2B` / `2C` / `2D` (`+0x62`), the morph
/// arm `4B` (`+0x6C`, the lanes), the ambient-tree install `34 3x`
/// (`s5 + 0x14` / `s5 + 0x24`, `0x801E00B0`) and the `4C` sub-ops of the
/// resolved-actor table ([`crate::world::prop_interact::is_resolved_actor_4c`]
/// plus the cursor test `8C`). Every runner that meets one of these aimed at
/// another actor steps it on that actor's context.
///
/// Excluded: the seat and run (`4C 51`, `23`), the walks and turns, the class
/// word `31` / `32` / `33` and the tint `4C 81` (which resolves its own
/// target byte), each of which the runners route on their own paths.
///
/// REF: FUN_801DE840 (0x801DE894..0x801DE8B4, 0x801E00B0, 0x801E23EC), FUN_8003C83C
pub(in crate::world) fn is_resolved_actor_word_op(bytecode: &[u8], pc: usize) -> bool {
    let Some(&b) = bytecode.get(pc) else {
        return false;
    };
    let sub_at = pc + if b & 0x80 != 0 { 2 } else { 1 };
    match b & 0x7F {
        0x22 | 0x2B..=0x2D | 0x4B => true,
        0x34 => bytecode.get(sub_at).is_some_and(|s| s >> 4 == 3),
        0x4C => bytecode.get(sub_at).is_some_and(|&s| {
            s == 0x8C
                || (s != 0x51 && s != 0x81 && crate::world::prop_interact::is_resolved_actor_4c(s))
        }),
        _ => false,
    }
}

/// Step one op on the context of channel `ci` - the actor an extended target
/// byte resolved to - as retail's dispatcher runs it on the actor in `s5`,
/// whichever record issued it (`caller_id` is the issuer's `+0x50`, which the
/// halted-target refusal reads; the refusal itself is the caller's business,
/// so a halt the channel carries is lifted for the op and put back after).
///
/// The channel is marked executing for the host hooks that address the actor
/// by it (the clip bind, the model set, the morph arm, the cursor test). A
/// placed object's copy of the clip-control word takes the bits its clip tick
/// owns first and hands the bits the op moved to the prop the object draws
/// after ([`World::poke_object_prop_words`], [`World::poke_object_prop_cflags`]);
/// a placement the op re-seated is drawn where its context now stands. An
/// ambient install (`34 3x`) reads the drawn placement's spot, which a leg
/// moves ahead of the context.
///
/// REF: FUN_801DE840 (0x801DE894..0x801DE8B4), FUN_8003C83C, FUN_800204F8
pub(in crate::world) fn step_on_resolved_channel(
    host: &mut FieldHostImpl<'_>,
    ci: usize,
    caller_id: u16,
    bytecode: &[u8],
    pc: usize,
) -> FieldStepResult {
    let world = &mut *host.world;
    let Some(ch) = world.field_vm.channels.get(ci) else {
        let width = legaia_scene_formats::field_disasm::decode(bytecode, pc).map_or(1, |i| i.size);
        return FieldStepResult::Advance {
            next_pc: pc + width,
        };
    };
    let (object, key) = (ch.object_bind, ch.placement_index);
    let mut ctx = ch.ctx.clone();
    let effect_op = bytecode.get(pc).is_some_and(|b| b & 0x7F == 0x34);
    let slot = (!object).then(|| u8::try_from(key).ok()).flatten();
    // An end-latch wait (`2D <id> <bit>`) on a placement the port binds no
    // clip cursor to: nothing would ever raise the bit, where retail's anim
    // tick always does once the clip runs out, so the wait is a clip of no
    // length (as `step_npc_word_op` takes it).
    if let Some(s) = slot
        && bytecode.get(pc).is_some_and(|b| b & 0x7F == 0x2D)
        && !world.npc_clip_cursor_bound(s)
        && !world.placement_morph_spin(s, bytecode.get(pc + 2).copied())
    {
        let width = legaia_scene_formats::field_disasm::decode(bytecode, pc).map_or(3, |i| i.size);
        return FieldStepResult::Advance {
            next_pc: pc + width,
        };
    }
    if object {
        world.mirror_object_tick_bits(key, &mut ctx.local_flags);
    } else if effect_op && let Some(&(x, z)) = slot.and_then(|s| world.npcs.positions.get(&s)) {
        ctx.world_x = x as u16;
        ctx.world_z = z as u16;
    }
    let words = (ctx.local_flags, ctx.field_6a);
    let class_before = ctx.flags;
    let seat_before = (ctx.world_x, ctx.world_z);
    let held = ctx.flags & 0x400;
    ctx.flags &= !0x400;
    let saved = (
        world.field_vm.executing_channel,
        world.field_vm.executing_object,
        world.dialog.stepping_inline_npc,
    );
    world.field_vm.executing_channel = slot;
    world.field_vm.executing_object = object.then_some(key as u16);
    world.dialog.stepping_inline_npc = None;
    let r = vm::field::step_from_caller(host, &mut ctx, caller_id, bytecode, pc);
    let world = &mut *host.world;
    (
        world.field_vm.executing_channel,
        world.field_vm.executing_object,
        world.dialog.stepping_inline_npc,
    ) = saved;
    ctx.flags |= held;
    let (flags_after, rate_after, class_after) = (ctx.local_flags, ctx.field_6a, ctx.flags);
    let seat_after = (ctx.world_x, ctx.world_z);
    if let Some(c) = world.field_vm.channels.get_mut(ci) {
        c.ctx = ctx;
    }
    if object {
        world.poke_object_prop_words(key, (words.0, flags_after), (words.1, rate_after));
        world.poke_object_prop_cflags(key, class_before, class_after);
    } else if let Some(s) = slot
        && !effect_op
        && seat_after != seat_before
    {
        world
            .npcs
            .positions
            .insert(s, (seat_after.0 as i16, seat_after.1 as i16));
        world.npcs.motions.remove(&s);
    }
    r
}
