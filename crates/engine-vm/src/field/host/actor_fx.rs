//! The [`FieldActorFxHost`] supertrait of the field-VM host surface.

use crate::field::FieldCtx;

/// Effects attached to an actor: lights, tint, reflections and model-anchored emitters.
///
/// One of the supertraits of [`FieldHost`](super::FieldHost), the surface the
/// field VM calls into the game through.
pub trait FieldActorFxHost {
    /// Op 0x34 sub-3 (3D model anim trigger). The original calls
    /// `func_0x800252ec(arg + 1, ctx + 0x14, ctx + 0x24)` where
    /// `arg = pbVar47[1]`. The host owns the actual animator; we pass the
    /// arg byte and the ctx mutably so it can access world-position
    /// fields if needed.
    fn effect_anim_trigger(&mut self, ctx: &mut FieldCtx, arg: u8) {
        let _ = (ctx, arg);
    }

    /// Op 0x34 sub-1: spawn an **attached light** on the target actor -
    /// retail `FUN_801E5668` (template `0x801F28B8`, ticked by
    /// `FUN_801E4470`), see `crate::field_actor_billboard`.
    ///
    /// The arm first walks the actor list at `_DAT_8007C354` for a light
    /// whose `+0x90` is already this actor (`FUN_8003CF04` keyed on the tick
    /// `0x801E4470`) and skips the spawn when it finds one. `ext` is the
    /// instruction's extended channel byte (`Some(0xF8)` = the player
    /// anchor); `script` is the bytecode after a following `0x40` block
    /// header - retail stores that pointer into `+0x94` - or `None` when no
    /// `0x40` follows.
    ///
    /// Returns `true` when a light was spawned: only then does the VM consume
    /// the `0x40` block as part of the instruction. Default: no light pool,
    /// the skip path.
    fn spawn_attached_light(
        &mut self,
        ctx: &FieldCtx,
        ext: Option<u8>,
        spawn: &crate::field_actor_billboard::AttachedSpriteSpawn,
        script: Option<&[u8]>,
    ) -> bool {
        let _ = (ctx, ext, spawn, script);
        false
    }

    /// Op 0x4C outer-nibble-8 sub-7 - **retire** every reflection
    /// controller.
    ///
    /// 2-byte instruction `[4C, 0x87]`, the install's teardown sibling. The
    /// arm at `0x801E2284` materialises `_DAT_8007C34C` and the handler VA
    /// `0x801E5154` and tail-jumps to the shared exit `0x801E2DC4`, which is
    /// `jal 0x8003CF40` with `addiu s8,s8,2` in its delay slot.
    /// `FUN_8003CF40` walks the list and ORs the kill bit into every node
    /// whose `+0x0C` matches - it **retires**, it does not register, it has
    /// no return value, and the arm does not halt.
    ///
    /// No shipped scene issues it; the four `4C 86` carriers drop their
    /// controllers on the scene boundary instead.
    fn retire_reflections(&mut self) {}

    /// Set the actor's draw **tint** (op 0x4C n8 sub-1, 9 bytes).
    ///
    /// `[4C, 0x81, r, g, b, blend_lo, blend_hi, ticks_lo, ticks_hi]`: a 24-bit
    /// colour (`FUN_8003CEB8` at operand `+1`), the `s16` blend and the `s16`
    /// tween length (`FUN_8003CE9C` at `+4` / `+6`). The arm
    /// (`0x801E1FC4..0x801E2068`, field overlay 0897) has three paths:
    /// - `ticks == 0`: `+0x74 = colour`, `+0x78 = blend`.
    /// - `+0x78 == 0`: `+0x74 = colour`, then a `FUN_8003C5F0` tween of
    ///   `+0x78` to `blend` (type 2).
    /// - otherwise: a type-3 tween of `+0x74` to `colour` (skipped when
    ///   `blend == 0`) and a tween of `+0x78` to `blend`.
    ///
    /// The draw stages `+0x74` / `+0x78` as the GTE far colour and `IR0`
    /// (`FUN_8001ADA4` -> `FUN_80043390`), so `(0, 0x1000)` pushes the actor
    /// fully to black. The earlier "model + animation frame" reading of this
    /// op named the operands for what they are not.
    ///
    /// `target` is the `0x80`-prefix target byte when the op carries one
    /// (`CC <id> 81 ..`): retail resolves it through `FUN_8003C83C` and runs
    /// the arm against **that** actor's words (`0xF8` = the player), so a
    /// host whose runner hands the op a stand-in context must land the tint
    /// on the named actor, not on `ctx`. The VM always advances PC by 9
    /// (10 with the prefix); the default impl is a no-op.
    fn set_actor_tint(
        &mut self,
        ctx: &mut FieldCtx,
        target: Option<u8>,
        colour: u32,
        blend: u16,
        ticks: u16,
    ) {
        let _ = (ctx, target, colour, blend, ticks);
    }

    /// Install the **reflection controller** (op 0x4C n8 sub-6, 15 bytes).
    ///
    /// `[4C, 0x86, w0_lo, w0_hi, .., w5_lo, w5_hi, source_id]`. The six
    /// `s16` are decoded through `FUN_8003CE9C` at operand `+1`, `+3`, `+5`,
    /// `+7`, `+9`, `+0xB`; the **last** byte is a cross-context actor id.
    ///
    /// The arm at `0x801E21E0` resolves that id through `FUN_8003C83C` and,
    /// on a hit, calls `FUN_801E573C(executing_ctx, resolved_actor, w0..w5)`,
    /// the spawner that allocates one controller off descriptor
    /// `0x801F2948` and writes `+0x90 = executing ctx`, `+0x94 = resolved
    /// actor`, `+0x54 = 0` and `+0x80 .. +0x8A = w0 .. w5`. Its per-frame
    /// body is `FUN_801E5154`
    /// ([`legaia_engine_field::field_actor_reflect::tick_reflection`]), which mirrors the
    /// `+0x94` end's pose onto the `+0x90` end while the `+0x94` end stands
    /// inside the tile rect `w2..w4` x `w3..w5`. So the six words are the
    /// controller's mirror line plus tracking rect, **not** a transform for
    /// the named actor, and the script's own actor is the image.
    ///
    /// This hook returns `true` when the host resolved the source; PC
    /// advances by 15 either way, because the arm's `addiu s8,s8,0xf` sits
    /// in the resolve call's delay slot and has already run when the miss
    /// branch is taken.
    ///
    /// The default impl returns `false` (no actor pool).
    fn install_reflection(&mut self, ctx: &mut FieldCtx, source_id: u8, words: [i16; 6]) -> bool {
        let _ = (ctx, source_id, words);
        false
    }

    /// Op 0x4C n6 sub-0x61 - scripted CLUT-cell effect behind the 16-byte
    /// halt-acquire arm (FUN_801E4C58 caller).
    ///
    /// `payload` is the 14-byte operand slice (bytes +1..+15 of the
    /// instruction, including the gating word at +0xD..=+0xE). The VM has
    /// already routed this through the standard halt-acquire predicate
    /// ([`super::FieldHaltHost::field_halt_acquire_predicate`] with `which = 0x61`) and
    /// performed the ctx mutation ((`saved_pc`, `wait_accum=0`,
    /// `flags |= 0x400`) plus optional system-channel mirror via
    /// [`super::FieldHaltHost::field_halt_acquire_apply`]).
    ///
    /// Default no-op. Hosts that model the emitter graph override.
    fn spawn_clut_cell_fx(&mut self, ctx: &mut FieldCtx, payload: [u8; 14]) {
        let _ = (ctx, payload);
    }
}
