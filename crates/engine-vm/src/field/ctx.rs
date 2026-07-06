//! Field VM per-script execution context ([`FieldCtx`]) and the [`StepResult`]
//! step outcome. Split out of `field.rs`.

/// Per-script execution context. One instance per running script.
///
/// Field naming follows the byte-offset convention from `docs/subsystems/script-vm.md`
/// to keep the link to the decompilation explicit. Each public field is a
/// distinct piece of state surfaced by at least one opcode handler.
#[derive(Debug, Clone, Default)]
pub struct FieldCtx {
    /// `+0x10` - context flag word. Bit `0x400` = halted (set by YIELD ops,
    /// checked by the dispatcher prelude). Bits `0x100`, `0x1000`, `0x20200`,
    /// `0x20000000`, `0x1000000`, `0x80000` carry per-feature semantics.
    pub flags: u32,
    /// `+0x14` - world X (units: `0.5` tile, formula `(b & 0x7F) * 0x80 + 0x40`).
    pub world_x: u16,
    /// `+0x16` - world Y (collision-derived).
    pub world_y: u16,
    /// `+0x18` - world Z.
    pub world_z: u16,
    /// `+0x26` - source value copied to [`saved_26`] by op 0x31 bit-8 path.
    pub field_26: u16,
    /// `+0x50` - script ID. `0xFB` = "system" channel.
    pub script_id: u16,
    /// `+0x54` - wait/timer accumulator. Cleared by YIELD; ticked by WAIT_FRAMES.
    pub wait_accum: i16,
    /// `+0x56` - move-table sub-state (op 0x22 sets to 5 if move==0, else 1).
    pub move_substate: u16,
    /// `+0x5A` - saved counterpart of [`field_26`] (op 0x31 bit-8 path).
    pub saved_26: u16,
    /// `+0x5C` - move-table index (op 0x22). Zeroed by the set-actor-model
    /// primitive (op 0x4C outer-nibble-5 sub-0, `FUN_80024e08`).
    pub move_id: u16,
    /// `+0x5E` - set to `0xFFFE` by op 0x22.
    pub field_5e: u16,
    /// `+0x60` - high-pool model-index mirror. Written by the set-actor-model
    /// primitive (op 0x4C outer-nibble-5 sub-0, `FUN_80024e08`) only when the
    /// high model pool is selected; read by the re-stage `FUN_80020F88`.
    pub model_id_high: u16,
    /// `+0x64` - active model index (into the global TMD pool). Written by the
    /// set-actor-model primitive (op 0x4C outer-nibble-5 sub-0, `FUN_80024e08`);
    /// consumed by the re-stage `FUN_80020F88` to resolve the actor's mesh.
    pub model_id: u16,
    /// `+0x62` - local flag bank (16 bits). Manipulated by 0x2B/0x2C/0x2D.
    pub local_flags: u16,
    /// `+0x6D` - face/body rotation index (op 0x43 sub-7).
    pub face_rotation: u8,
    /// `+0x72` - generic per-actor scalar slot. Written / ramped by
    /// op 0x4C outer-nibble-4 sub-0.
    pub field_72: u16,
    /// `+0x24` - generic per-actor scalar slot. Written / ramped by
    /// op 0x4C outer-nibble-4 sub-3 (ramp path); the immediate path is
    /// repurposed as an absolute jump and does not touch this field.
    pub field_24: i16,
    /// `+0x28` - generic per-actor scalar slot. Written by op 0x4C
    /// outer-nibble-4 sub-4 (immediate path); the ramp path is repurposed
    /// as an absolute jump and does not touch this field.
    pub field_28: i16,
    /// `+0x6A` - generic per-actor scalar slot. Written / ramped by op
    /// 0x4C outer-nibble-4 sub-1, which **halves the input** (`target >> 1`)
    /// and floors the result at `1` before applying.
    pub field_6a: i16,
    /// `+0x8E` - inverted-Y mirror slot. Written / ramped by op 0x4C
    /// outer-nibble-4 sub-2 (which also conditionally writes
    /// `world_y = -value` when `flags & 0x20000000` is set).
    pub field_8e: i16,
    /// `+0x8B` - cleared by op 0x23 NPC path.
    pub field_8b: u8,
    /// `+0x8C` - NPC X grid coordinate (op 0x23).
    pub npc_x: u8,
    /// `+0x8D` - NPC facing direction (op 0x23).
    pub npc_facing: u8,
    /// `+0x90` - opaque actor-handle field. Captured by op 0x49 sub-1 into the
    /// `_DAT_8007B44C` global (the runtime later restores it across the
    /// state-resume gate). Treated as opaque by the VM.
    pub field_90: u32,
    /// `+0x94` - saved PC (set by YIELD; the dispatcher reads this on resume).
    pub saved_pc: u32,
    /// `+0x42` - generic per-actor scalar slot. Written by op 0x4C
    /// outer-nibble-0xC sub-2 (`[4C, 0xC2, b1]` writes `b1` zero-extended).
    pub field_42: u16,
    /// `+0x58` - generic per-actor scalar slot. Written by op 0x4C
    /// outer-nibble-0xD sub-0xD (`[4C, 0xDD, b1]` writes `b1` zero-extended).
    pub field_58: u16,
    /// `+0x68` - local guard slot. Read by op 0x4C outer-nibble-8 sub-0xC
    /// to skip a forward jump when zero.
    pub field_68: i16,
    /// `+0x74` - composite control word. XOR-toggled by op 0x4C
    /// outer-nibble-0xC sub-8 (`[4C, 0xC8]` flips bit 0x10000000).
    pub field_74: u32,
}

impl FieldCtx {
    /// Has the YIELD bit (`flags & 0x400`) been set?
    pub fn is_halted(&self) -> bool {
        self.flags & 0x400 != 0
    }

    /// Set the halt bit. Called by the YIELD opcodes (0x37, 0x41, 0x47).
    pub fn halt(&mut self) {
        self.flags |= 0x400;
    }
}

/// Outcome of a single VM step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StepResult {
    /// Advance to a new PC offset. The next call to [`step`] should resume
    /// from `next_pc`.
    Advance { next_pc: usize },
    /// The script has yielded - caller should wait for the next host tick
    /// before resuming. The next PC was saved to `ctx.saved_pc`.
    Yield { resume_pc: usize },
    /// The script wants to halt and not resume on its own - typically because
    /// a flag-test failed and the conditional path is "halt".
    Halt { final_pc: usize },
    /// The opcode is recognized but not yet implemented in this port.
    /// Carries the opcode byte for diagnostics.
    Pending { opcode: u8, pc: usize },
    /// Unknown / out-of-range opcode (matches the original's "default" arm
    /// behaviour, which prints `"UNFIND INDICATION %d"` and returns).
    Unknown { opcode: u8, pc: usize },
}
