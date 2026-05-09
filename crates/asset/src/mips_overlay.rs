//! MIPS overlay-code detector.
//!
//! ### Provenance
//!
//! 22 PROT entries in the `0901_xxx_dat.BIN..0969_xxx_dat.BIN` cluster lead
//! with the canonical MIPS function prologue `addiu sp, sp, -X`. All 22 are
//! sized 14–37 KB (with one 163 KB outlier at `0969_xxx_dat.BIN`), match
//! `find-overlay`'s "MIPS-code-shape" heuristic, and were previously stuck
//! in `unknown_other` / `unknown_low_entropy` because the categorize pipeline
//! had no MIPS-aware detector.
//!
//! These are static disc copies of the runtime overlays that load into the
//! `0x801C0000+` overlay window — the same shape as the already-imported
//! `0896` / `0897` / `0898` / `0971` / `0978` overlays, but at smaller sizes
//! (each is a self-contained subsystem code blob, not a full scene overlay).
//!
//! ### Layout
//!
//! ```text
//! +0x00   u32  0x27BDFFXX        ; addiu sp, sp, -X (negative stack adjust)
//! +0x04   u32  prologue follow-up; sw ra/s* (0xAFB?_00XX), or another addiu
//!                                ; / lui / sw / R-type — the second instruction
//!                                ; of the entry function's prologue
//! +0x08   ...                    ; rest of the overlay code blob
//! ```
//!
//! ### Detection
//!
//! 1. `u32_le[0] & 0xFFFF_FF00 == 0x27BD_FF00` (the `addiu sp, sp, -X` opcode).
//! 2. `(u32_le[0] & 0xFF) ∈ [0x80, 0xF8]` — only accept reasonable stack
//!    adjustments (8 to 128 bytes, the common range for Legaia functions).
//! 3. `u32_le[1]`'s 6-bit MIPS opcode field is one of the common
//!    function-prologue follow-ups (`sw`, `addiu`, `lui`, `lw`, `R-type`).
//!
//! These three checks together produce **zero false positives** across the
//! 1232-entry PROT corpus. All 22 matches cluster in the `0901..=0969` PROT
//! range — the `xxx_dat` cluster that `find-overlay` already ranks as
//! MIPS-code-shape.
//!
//! ### Coverage impact
//!
//! Promotes 21 entries out of `unknown_other` (157 → 136) and 1 from
//! `unknown_low_entropy` (76 → 75). Coverage 888 / 1232 (72.0%) → 910 / 1232
//! (73.9%).
//!
//! ### Format meaning
//!
//! These entries are **MIPS code blobs** for runtime-loaded overlays. The
//! ones already imported (`0896`, `0897`, `0898`, `0971`, `0978`) cover
//! options-menu / town-field / battle / fishing / dance subsystems. The 22
//! new candidates are smaller subsystem blobs — likely cutscenes, world-map,
//! menu screens, mini-games, or per-scene specialised code.
//!
//! Each can be Ghidra-imported via `scripts/bulk-import-overlays.sh` once a
//! base address is determined (the overlay window is `0x801C0000`–`0x80200000`,
//! but each blob loads at a specific offset within that range — likely
//! reverse-engineerable from the PROT entry's reference in the asset chain).
//!
//! See `docs/formats/mips-overlay.md` for the spec.

use serde::Serialize;

/// MIPS `addiu sp, sp, immediate` opcode + register encoding (without the
/// signed 16-bit immediate). The full instruction word is
/// `0x27BD_FFXX` for stack-frame setup with `XX` = `(-frame_size)` low byte.
const ADDIU_SP_SP_NEG: u32 = 0x27BD_FF00;

/// Mask that isolates the high 24 bits of `addiu sp, sp, -X` so we ignore
/// the low byte of the immediate.
const ADDIU_SP_SP_NEG_MASK: u32 = 0xFFFF_FF00;

/// Minimum and maximum negative stack adjustment we'll accept. The signed
/// 16-bit immediate `-X` for `X` in `[0x08 .. 0x80]` decodes to low byte
/// `0xF8 .. 0x80`. Real Legaia entries cluster at `0x80..0xC0`.
const STACK_ADJ_LOW_MIN: u32 = 0x80;
const STACK_ADJ_LOW_MAX: u32 = 0xF8;

/// MIPS opcode field is the high 6 bits of the instruction word.
const MIPS_OP_SHIFT: u32 = 26;
const MIPS_OP_MASK: u32 = 0x3F;

/// Whitelist of MIPS opcode-field values that are plausible second
/// instructions of a function prologue. These are the common opcodes that
/// follow `addiu sp, sp, -X`:
/// - `0x00` — R-type (special; e.g. `move`, `or`, `addu`).
/// - `0x09` — `addiu`.
/// - `0x0F` — `lui`.
/// - `0x23` — `lw`.
/// - `0x2B` — `sw` (most common — saves `ra` / `s*` / `gp`).
/// - `0x2C..=0x33`, `0x35`, `0x37..=0x3F` — load/store (less common but valid).
const PROLOGUE_FOLLOWUP_OPS: &[u32] = &[
    0x00, 0x09, 0x0F, 0x23, 0x2B, 0x2C, 0x2D, 0x2E, 0x2F, 0x30, 0x31, 0x32, 0x33, 0x35, 0x37, 0x38,
    0x39, 0x3D, 0x3F,
];

/// Detection result.
#[derive(Debug, Clone, Serialize)]
pub struct MipsOverlay {
    /// Negative stack-adjust value (in bytes — already the magnitude, not the
    /// signed-16 raw form). E.g. `addiu sp, sp, -0x80` → `stack_frame_bytes = 0x80`.
    pub stack_frame_bytes: u32,
    /// Raw second-instruction word (for reference).
    pub second_instruction: u32,
    /// MIPS opcode-field of the second instruction (for classification).
    pub second_op: u8,
}

/// Try to detect a MIPS overlay-code blob. Returns `None` when the buffer
/// doesn't lead with a recognisable function prologue.
pub fn detect(buf: &[u8]) -> Option<MipsOverlay> {
    if buf.len() < 8 {
        return None;
    }
    let u32_0 = read_u32_le(buf, 0)?;
    let u32_1 = read_u32_le(buf, 4)?;

    // (1) addiu sp, sp, -X — high 24 bits must match the prologue pattern.
    if u32_0 & ADDIU_SP_SP_NEG_MASK != ADDIU_SP_SP_NEG {
        return None;
    }
    let stack_low = u32_0 & 0xFF;
    if !(STACK_ADJ_LOW_MIN..=STACK_ADJ_LOW_MAX).contains(&stack_low) {
        return None;
    }

    // (2) Second instruction's 6-bit opcode field must be a plausible
    //     prologue continuation.
    let second_op = (u32_1 >> MIPS_OP_SHIFT) & MIPS_OP_MASK;
    if !PROLOGUE_FOLLOWUP_OPS.contains(&second_op) {
        return None;
    }

    // Stack frame size is the magnitude of the negative immediate, which for
    // `addiu sp, sp, -X` with `X` in `[0x08, 0x80]` decodes to `0x100 - low_byte`.
    let stack_frame_bytes = 0x100 - stack_low;

    Some(MipsOverlay {
        stack_frame_bytes,
        second_instruction: u32_1,
        second_op: second_op as u8,
    })
}

fn read_u32_le(buf: &[u8], at: usize) -> Option<u32> {
    let bytes = buf.get(at..at + 4)?;
    Some(u32::from_le_bytes(bytes.try_into().unwrap()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a minimal valid MIPS overlay header: `addiu sp, sp, -X` followed
    /// by `sw ra, 0xXX(sp)`.
    fn synth(stack_frame: u32, total_size: usize) -> Vec<u8> {
        assert!((0x08..=0x80).contains(&stack_frame));
        let stack_low = (0x100 - stack_frame) & 0xFF;
        let prologue = ADDIU_SP_SP_NEG | stack_low;
        // sw ra, (stack_frame - 4)(sp) = 0xAFBF_00XX
        let sw_ra = 0xAFBF_0000u32 | (stack_frame - 4);
        let mut buf = Vec::with_capacity(total_size);
        buf.extend_from_slice(&prologue.to_le_bytes());
        buf.extend_from_slice(&sw_ra.to_le_bytes());
        buf.resize(total_size, 0);
        buf
    }

    #[test]
    fn detects_canonical_prologue() {
        let buf = synth(0x80, 0x4000);
        let m = detect(&buf).expect("should detect");
        assert_eq!(m.stack_frame_bytes, 0x80);
        // Second op: `sw` = 0x2B.
        assert_eq!(m.second_op, 0x2B);
    }

    #[test]
    fn detects_small_stack_frame() {
        let buf = synth(0x10, 0x4000);
        assert!(detect(&buf).is_some());
    }

    #[test]
    fn rejects_non_mips_buffer() {
        // Random bytes leading with neither an addiu nor any prologue.
        let buf: Vec<u8> = (0..=255u8).cycle().take(0x4000).collect();
        assert!(detect(&buf).is_none());
    }

    #[test]
    fn rejects_buffer_smaller_than_two_instructions() {
        assert!(detect(&[0u8; 4]).is_none());
        assert!(detect(&[0u8; 7]).is_none());
    }

    #[test]
    fn rejects_addiu_sp_with_implausible_stack_adjust() {
        // 0x27BD_FF7F is `addiu sp, sp, -0x81` — outside our [0x80, 0xF8] range.
        let mut buf = vec![0; 0x100];
        buf[0..4].copy_from_slice(&0x27BD_FF7Fu32.to_le_bytes());
        buf[4..8].copy_from_slice(&0xAFBF_0000u32.to_le_bytes());
        assert!(detect(&buf).is_none());
    }

    #[test]
    fn rejects_addiu_sp_followed_by_garbage_opcode() {
        // First word valid, second word's opcode field is 0x10 (cop0) which
        // isn't in our prologue-followup whitelist.
        let mut buf = vec![0; 0x100];
        buf[0..4].copy_from_slice(&0x27BD_FF80u32.to_le_bytes());
        buf[4..8].copy_from_slice(&0x40080000u32.to_le_bytes());
        assert!(detect(&buf).is_none());
    }

    #[test]
    fn accepts_addiu_followed_by_lui() {
        // Second instruction = `lui v0, 0xXXXX` = 0x3C02_XXXX (op=0x0F).
        let mut buf = vec![0; 0x100];
        buf[0..4].copy_from_slice(&0x27BD_FF90u32.to_le_bytes());
        buf[4..8].copy_from_slice(&0x3C02_8007u32.to_le_bytes());
        let m = detect(&buf).expect("lui follow-up should accept");
        assert_eq!(m.stack_frame_bytes, 0x70);
        assert_eq!(m.second_op, 0x0F);
    }

    #[test]
    fn accepts_addiu_followed_by_addiu() {
        // Second instruction = `addiu s0, s0, 0xXXXX` = 0x2610_XXXX (op=0x09).
        let mut buf = vec![0; 0x100];
        buf[0..4].copy_from_slice(&0x27BD_FFA0u32.to_le_bytes());
        buf[4..8].copy_from_slice(&0x2610_0001u32.to_le_bytes());
        let m = detect(&buf).expect("addiu follow-up should accept");
        assert_eq!(m.stack_frame_bytes, 0x60);
        assert_eq!(m.second_op, 0x09);
    }

    #[test]
    fn accepts_real_world_0901_xxx_dat_head() {
        // 0901_xxx_dat.BIN actually starts with `27 BD FF 90 ...`.
        let mut buf = vec![0u8; 0x100];
        buf[0..4].copy_from_slice(&[0x90, 0xFF, 0xBD, 0x27]);
        // Second instruction: real `0x901_xxx_dat` has `sw ra, 0x6c(sp)` = 0xAFBF_006C.
        buf[4..8].copy_from_slice(&0xAFBF_006Cu32.to_le_bytes());
        let m = detect(&buf).expect("real-world prologue should detect");
        assert_eq!(m.stack_frame_bytes, 0x70);
    }
}
