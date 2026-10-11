//! The [`FieldHaltHost`] supertrait of the field-VM host surface.

use crate::field::FieldCtx;

/// The halt-acquire idiom: parking a script on an actor until a motion releases it.
///
/// One of the supertraits of [`FieldHost`](super::FieldHost), the surface the
/// field VM calls into the game through.
pub trait FieldHaltHost {
    /// Op 0x4C outer-nibble-8 sub-5 / sub-0xE / sub-0xF - halt-acquire idiom.
    ///
    /// All three sub-ops share the **same** inner-switch body in the dump
    /// (lines 6550-6570 of `overlay_0897_801de840.txt`): a conditional
    /// "halt-acquire" that, if the actor is acquireable, writes
    /// `ctx.saved_pc = current PC`, clears `ctx.wait_accum`, and sets the
    /// halt bit (`flags |= 0x400`). On `iVar18 == _DAT_8007c364` (system
    /// channel) the same fields are mirrored to the system context. Both
    /// the success and failure exits halt at PC (success returns
    /// `switchD_801e00f4::default()`; failure goes to `LAB_801dee50`).
    ///
    /// The default impl performs the standard ctx mutation (saved_pc +
    /// wait_accum + halt bit). Real hosts can override to also mirror to
    /// the system actor or to refuse the acquire.
    fn halt_acquire(&mut self, ctx: &mut FieldCtx, opcode_pc: u32) {
        ctx.saved_pc = opcode_pc;
        ctx.wait_accum = 0;
        ctx.flags |= 0x400;
    }

    /// Halt-acquire dispatcher used by op 0x38 (CAM_CFG yield path) and
    /// op 0x43 sub-0/1/A/B (actor-control halts).
    ///
    /// The original at `0x801df30c+` halts the active script (and the
    /// system-channel caller, when the active ctx is the player) by:
    /// - saving `pbVar43` (the opcode byte pointer) into `ctx.saved_pc`,
    /// - setting `ctx.flags |= 0x400` (HALT bit),
    /// - clearing `ctx.wait_accum`.
    ///
    /// For op 0x43 sub-0/1/A/B the resume PC is the fall-through past the
    /// 8-byte (sub-0/1) or 10-byte (sub-A/B) instruction; none of the operand
    /// halfwords is a jump target (they are the arc's apex, frame count and
    /// sub-A/B's negated Y).
    ///
    /// For op 0x38 the resume PC is `pc + 3` (post-instruction); there is no
    /// operand-encoded target.
    ///
    /// Acquisition succeeds only when:
    /// - `ctx.saved_pc != 0` OR `ctx == player_ctx`, AND
    /// - `ctx.flags & 0x400 == 0` OR the scene busy slot
    ///   `_DAT_801C6EA4 + 0x8` is non-zero.
    ///
    /// On success: VM emits `Yield { resume_pc: target_pc }` (the host's
    /// state-resume layer drives re-entry). On failure the VM `Halt`s at the
    /// current PC: for op 0x43 that is the arm's own `j 0x801DEE4C`
    /// (`move s8, s4` - retry next frame), for op 0x38 the original's
    /// `switchD_801e00f4::default()` fallthrough.
    ///
    /// `which` is the originating opcode/sub-op tag so hosts that need to
    /// distinguish the call site can - it's `0x38` for op 0x38, and the raw
    /// op-0x43 sub-op (`0`, `1`, `0xA`, `0xB`) for op 0x43.
    ///
    /// The default impl returns `true` (always acquire), which keeps the
    /// original's control-flow shape; hosts that don't model the player
    /// pointer can model their own predicate.
    fn field_halt_acquire_predicate(&self, ctx: &FieldCtx, which: u8) -> bool {
        let _ = (ctx, which);
        true
    }

    /// Op `0x43` sub-`0`/`1`/`0xA`/`0xB`: `true` when the arm's acquire must
    /// refuse because its target already carries the halt bit `0x400` and
    /// the scene word `*(_DAT_801C6EA4) + 8` is zero (`0x801DF3A4..0x801DF3CC`);
    /// in practice, an actor still in the air from an earlier arc. `ext`
    /// is the instruction's extended channel byte. A refused acquire leaves
    /// the PC on the instruction for a retry next frame. Default `false`.
    fn arc_target_halted(&self, ctx: &FieldCtx, ext: Option<u8>) -> bool {
        let _ = (ctx, ext);
        false
    }

    /// Side effect of [`field_halt_acquire_predicate`] returning `true`.
    ///
    /// The original also writes the same halt fields onto the system-channel
    /// (`param_3`) when `ctx == player_ctx`. Engines that maintain a separate
    /// caller channel use this hook; default no-op.
    ///
    /// `resume_pc` is the absolute PC the script will resume at; `coords` is
    /// the (x, y, z) position the original copies into a scratch local on the
    /// op-0x43 path. Op 0x38 passes `[0; 3]` since it has no coord operands.
    ///
    /// [`field_halt_acquire_predicate`]: Self::field_halt_acquire_predicate
    fn field_halt_acquire_apply(
        &mut self,
        ctx: &mut FieldCtx,
        which: u8,
        resume_pc: usize,
        coords: [i16; 3],
    ) {
        let _ = (ctx, which, resume_pc, coords);
    }
}
