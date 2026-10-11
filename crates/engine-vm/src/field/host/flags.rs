//! The [`FieldFlagsHost`] supertrait of the field-VM host surface.

use crate::field::FieldCtx;

/// Flag banks, script registers and clocks: the words a script tests and writes.
///
/// One of the supertraits of [`FieldHost`](super::FieldHost), the surface the
/// field VM calls into the game through.
pub trait FieldFlagsHost {
    /// Read the global story-flag word at `_DAT_1F800394` (PSX scratchpad).
    /// Used by GFLAG_TST (0x30) and read/written by GFLAG_SET (0x2E) /
    /// GFLAG_CLR (0x2F).
    fn global_flags(&self) -> u32;

    /// Write the global story-flag word.
    fn set_global_flags(&mut self, value: u32);

    /// Frame delta tick. Mirrors `_DAT_1F800393` (PSX scratchpad byte read by
    /// op 0x4A WAIT_FRAMES on every step). Most engines return `1` when the
    /// frame budget allows the script to advance.
    fn frame_delta(&self) -> u16;

    /// Read the secondary global-flag bank `_DAT_8007B8F4` (op 0x42 mode 0).
    /// Distinct from the main `_DAT_1F800394` flag bank in scratchpad -
    /// `_DAT_8007B8F4` is the per-tile **region-type mask**: bit `n` set
    /// when the player tile sits inside a type-`n` region of the scene
    /// `.MAP` region table. Rebuilt by `FUN_800180EC` / `FUN_801DBA20`
    /// (ports: `legaia_engine_field::field_regions`).
    fn extra_flags(&self) -> u32 {
        0
    }

    /// Read the word op 0x42 mode 1 tests, `_DAT_8007B850` - the **held
    /// pad**, packed (d-pad in `0xF000`, Triangle / Circle / Cross / Square
    /// in `0x10` / `0x20` / `0x40` / `0x80`). The name is historical: the
    /// word is not a screen mode. Bits `0x10/0x20/0x40/0x80` are
    /// individually testable; bits `0xF000` are matched against the
    /// 8-entry compass table at `_DAT_801F28D0`.
    fn screen_mode(&self) -> u32 {
        0
    }

    /// Look up the 0x42-mode-1 8-entry table `_DAT_801F28D0` (when `op1 < 8`).
    /// Default returns `None` so the test falls through to the skip path.
    fn screen_mode_table(&self, index: u8) -> Option<u32> {
        let _ = index;
        None
    }

    /// Slot-table (scene-register) read at `0x801C6460 + slot * 2` (s16).
    ///
    /// The read side of op 0x4E sub-ops 5..=8 (`slot = sub_op - 5`, loader
    /// `0x801E0B0C`); the write side is op 0x4C outer-nibble-C sub-0xA
    /// (set) / sub-0xB (add) / sub-0xC (subtract) - see
    /// [`Self::op4c_n_c_sub_a_set_slot`]. Default returns 0.
    fn slot_table_read(&self, slot: u8) -> i16 {
        let _ = slot;
        0
    }

    /// Op 0x4E sub-op 4 - BIOS `Rand()` read.
    ///
    /// `FUN_80056798` is a 3-instruction BIOS thunk: `li t2,0xa0; jr t2;
    /// _li t1,0x2f` - the BIOS `Rand()` syscall. The loader at `0x801E0AFC`
    /// masks the result to `& 0xFF` and joins the shared compare
    /// continuation (`j 0x801e0b3c` staging the operand pointer), so
    /// sub-op 4 is a **random-chance branch**, not a jump-to-rand PC (the
    /// decompiled `return iVar18;` was the collapsed switch arm). The VM
    /// applies the `& 0xFF` mask itself; hosts return the raw PRNG value.
    /// Default returns 0.
    fn op4e_sub4_bios_rand(&mut self) -> i32 {
        0
    }

    /// Set a bit in the **system flag bank** (the 4th of the four flag banks
    /// the field VM exposes). Wired by the high-byte default-route opcode
    /// `0x5x` (SET) at the dispatcher's default arm. Mirrors `func_0x8003CE08`
    /// which sets `(&DAT_80085758)[idx >> 3] |= (0x80 >> (idx & 7))`.
    ///
    /// `idx` is a 16-bit value computed by the VM as
    /// `((opcode_byte & 0x8F) << 8) | operand_byte` - bits 0..=7 from the
    /// operand byte, bits 8..=11 from the low nibble of the raw opcode byte,
    /// bit 15 from the extended-prefix bit (0x80) of the raw opcode byte.
    ///
    /// The bitfield's exact size and per-script-context partitioning aren't
    /// nailed down - the original SCUS dispatchers do **no bounds checking**
    /// past `idx >> 3`, so on the original a 16-bit `idx` could index past
    /// the documented 256-bit array. Hosts pick whatever representation
    /// works for them; the default impl is a no-op.
    fn system_flag_set(&mut self, idx: u16) {
        let _ = idx;
    }

    /// Clear a bit in the system flag bank. High-byte default-route opcode
    /// `0x6x`. Mirrors `func_0x8003CE34`. See [`system_flag_set`] for the
    /// `idx` encoding.
    ///
    /// [`system_flag_set`]: Self::system_flag_set
    fn system_flag_clear(&mut self, idx: u16) {
        let _ = idx;
    }

    /// Test a bit in the system flag bank. High-byte default-route opcode
    /// `0x7x`. Mirrors `func_0x8003CE64` (returns 0xFF if set, 0 if clear -
    /// the Rust port collapses both into a `bool`).
    ///
    /// On the original, when the bit is set the dispatcher takes the post-test
    /// branch (jumps to `pc + header_size + 1 + LE_u16(operand[1..3])`); when
    /// the bit is clear, the dispatcher falls through past the 4-byte
    /// instruction. The default impl returns `false`, which keeps the VM
    /// advancing past the instruction.
    fn system_flag_test(&self, idx: u16) -> bool {
        let _ = idx;
        false
    }

    /// Op 0x4C outer-nibble-A - conditional jump on a flag bit.
    ///
    /// 5-byte instruction `[4C, 0xAN, bit, lo, hi]` where `N` selects the
    /// bank:
    /// - `0`: `ctx.flags & (1 << bit)` (per-actor flag word, ctx[+0x10]).
    /// - `1`: `ctx.local_flags & (1 << bit)` (16-bit local flags, ctx[+0x62]).
    /// - `2`: `_DAT_1F800394 & (1 << bit)` (global story flag word).
    /// - `3..=0xF`: no bank check; helper returns `false`. The original at
    ///   line 6702 of `overlay_0897_801de840.txt` falls through every check
    ///   and skips 5 bytes (`return param_2 + 5`).
    ///
    /// When the bit is **set**, the original takes the absolute jump
    /// (`func_0x8003CE9C(operand + 2)` - signed 16-bit absolute PC). When
    /// clear, PC advances by 5.
    ///
    /// (Round 11 corrected the take/skip direction here: prior implementations
    /// were inverted because Ghidra's C output for case 10 hides the `bne a1,
    /// zero, 0x801e258c` dispatch at 0x801e2568. The asm flow is "if sub != 0,
    /// skip ctx[+0x10] check; per-sub-op bit SET → branch to the take-jump
    /// label", not the other way around.)
    ///
    /// `bit` is the operand byte; the VM masks the low 5 bits to match
    /// MIPS `sllv` shift semantics.
    fn op4c_n_a_flag_set(&self, ctx: &FieldCtx, bank: u8, bit: u8) -> bool {
        match bank {
            0 => ctx.flags & (1u32 << (bit & 0x1F)) != 0,
            1 => ctx.local_flags & (1u16 << (bit & 0x1F)) != 0,
            2 => self.global_flags() & (1u32 << (bit & 0x1F)) != 0,
            _ => false,
        }
    }

    /// Op 0x4C outer-nibble-C sub-0xA - write u16 at scratchpad slot.
    ///
    /// 5-byte instruction `[4C, 0xCA, slot, lo, hi]`. Writes
    /// `*(short *)(&DAT_801C6460 + slot * 2) = signed_16(operand[2..4])`.
    /// The slot table at `0x801C6460` is a 64-entry scratchpad zone shared
    /// between scripts; hosts can model it as their own slot array. PC += 5.
    fn op4c_n_c_sub_a_set_slot(&mut self, slot: u8, value: i16) {
        let _ = (slot, value);
    }

    /// Op 0x4C outer-nibble-C sub-0xB / sub-0xC - add or subtract on
    /// scratchpad slot.
    ///
    /// 5-byte instruction `[4C, 0xCN, slot, lo, hi]` where `N ∈ {0xB, 0xC}`.
    /// Reads `value = signed_16(operand[2..4])`; if `value == 0xFFFF` the VM
    /// substitutes `_DAT_1F800393` (the per-frame tick byte, surfaced via
    /// [`frame_delta`]).
    /// - sub-0xB: `slot += value`
    /// - sub-0xC: `slot -= value`
    ///
    /// PC += 5.
    ///
    /// [`frame_delta`]: Self::frame_delta
    fn op4c_n_c_sub_bc_adjust_slot(&mut self, slot: u8, delta: i16, subtract: bool) {
        let _ = (slot, delta, subtract);
    }

    /// Op 0x4C outer-nibble-D sub-3 - party state setup + FUN_801D596C.
    ///
    /// 14-byte instruction `[4C, 0xD3, lo_a, hi_a, lo_b, hi_b, lo_c, hi_c,
    /// lo_d, hi_d, lo_e, hi_e, lo_f, hi_f]`. Reads two signed 16-bit values
    /// (a, b), two LE u32 values (c-d, e-f via `func_0x8003CED8`), then writes:
    /// - `_DAT_800845C0 = (a << 16) | b` (high/low halves)
    /// - `_DAT_800845B8 = packed_cd`
    /// - `_DAT_800845BC = packed_ef`
    /// - `_DAT_800845A0 = packed_cd` (mirror)
    /// - `_DAT_80073ED4 = _DAT_80084570` (snapshot)
    ///
    /// Then calls `FUN_801D596C()`, the scripted countdown actor's
    /// install / remove: it finds the live actor whose handler is
    /// `0x801D2EBC` (the escape-timer HUD) through `FUN_8003CF04`; with
    /// `_DAT_800845B8 == 0` it retires it, otherwise (when none is live) it
    /// spawns descriptor `0x801F22DC` at `(+0x14, +0x16) = (0xC0, 0x10)`.
    /// PC += 14.
    fn op4c_n_d_sub3_party_setup(&mut self, ab: u32, cd: u32, ef: u32) {
        let _ = (ab, cd, ef);
    }

    /// Op 0x4C outer-nibble-E sub-0xE - snapshot `_DAT_80084570`.
    ///
    /// 2-byte instruction `[4C, 0xEE]`. Writes
    /// `_DAT_800845DC = _DAT_80084570` - a single-direction snapshot of the
    /// party-leader/scene state. PC += 2.
    fn op4c_n_e_sub_e_snapshot_84570(&mut self) {}

    /// Read 16-bit value used by op 0x4C nC sub-5/6 - the operand-stream
    /// equivalent of `func_0x8003CE9C` followed by [`party_flag_test`] over
    /// the trigger-flag bank.
    ///
    /// Both sub-5 (jump-if-zero) and sub-6 (jump-if-nonzero) read a 16-bit
    /// value from `bytecode[operand+1..=operand+2]` and test whether the
    /// corresponding bit is set in the trigger-flag array. The dispatcher
    /// computes the index via [`field_helpers::load_u16_le`] and asks the
    /// host whether that bit is set.
    ///
    /// Returns `true` when the bit is set (= the original's `0xFF` saturation
    /// from `FUN_8003CE64`), `false` otherwise. Default impl returns `false`
    /// - engines without a flag bank treat all bits as clear.
    ///
    /// [`field_helpers::load_u16_le`]: crate::field::dispatch_leaves::load_u16_le
    /// [`party_flag_test`]: crate::field::dispatch_leaves::party_flag_test
    fn op4c_n_c_party_flag_test(&self, flag_idx: u16) -> bool {
        let _ = flag_idx;
        false
    }
}
