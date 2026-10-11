//! The two field-VM system-flag ops the script-editing mods emit: the
//! persistent flag **SET** (`0x5x`) and the **TEST**-and-skip (`0x7x`).
//!
//! A mod that splices new script into a scene MAN (the starting-bag grant,
//! the Delilas Challenge's enrollment branch) guards and latches it with
//! these, so the encoders sit with the MAN re-pack helpers every such mod
//! already takes from this crate.

/// Base opcode of the persistent system-flag **SET** op (`0x5x`). The flag index
/// is split across the opcode's low bits and the operand byte:
/// `idx = ((op & 0x8F) << 8) | operand` (see `field_disasm` `0x50..=0x77`).
pub const SYSFLAG_SET_BASE: u8 = 0x50;

/// Base opcode of the persistent system-flag **TEST** op (`0x7x`). When the bit is
/// set the VM jumps by the trailing `u16` delta (`[0x70, operand, dlo, dhi]`,
/// target `= pc + 2 + delta`); when clear it falls through.
pub const SYSFLAG_TEST_BASE: u8 = 0x70;

/// Encode the system-flag **SET** op for `bit` (`[op, operand]`, 2 bytes).
pub fn sysflag_set(bit: u16) -> [u8; 2] {
    let op = SYSFLAG_SET_BASE | ((bit >> 8) as u8 & 0x0F);
    [op, (bit & 0xFF) as u8]
}

/// Encode the system-flag **TEST**-and-skip op for `bit` (`[op, operand, dlo, dhi]`,
/// 4 bytes). `delta` is the relative jump applied when the bit is set; the VM
/// computes the target as `pc + 2 + delta` (`pc + header_size(1) + 1 + delta`).
pub fn sysflag_test(bit: u16, delta: u16) -> [u8; 4] {
    let op = SYSFLAG_TEST_BASE | ((bit >> 8) as u8 & 0x0F);
    let d = delta.to_le_bytes();
    [op, (bit & 0xFF) as u8, d[0], d[1]]
}
