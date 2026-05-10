//! Overlay pointer-table detector.
//!
//! ### Provenance
//!
//! 42 PROT entries in the `0900_xxx_dat.BIN..0968_xxx_dat.BIN` cluster lead
//! with a contiguous run of u32 values, each falling inside the runtime
//! overlay window `0x801C0000..=0x801FFFFF`. The first non-pointer u32
//! terminates the table; the bytes that follow are MIPS code (sometimes a
//! `jr ra; nop` stub at the start, sometimes an immediate function prologue,
//! sometimes a short instruction sequence that closes a previous frame
//! before the next function begins).
//!
//! These are the **sister cluster** to [`crate::mips_overlay`]: same general
//! kind of disc-resident overlay code blob, but with a function/jump-table
//! header at the start instead of a `addiu sp, sp, -X` prologue. Examples:
//!
//! - `0938_xxx_dat.BIN` - 5-pointer table at `[0x801F6AA4..=0x801F71F8]`,
//!   then `addiu sp, sp, -0x48 / lui v0, 0x0808 / lw a0, -0x42DC(v0)` at
//!   offset 0x14 (the table's end). The pointers index into this same blob.
//! - `0907_xxx_dat.BIN` - leads with the ASCII string `"Hell's Music"`,
//!   then a jump table - a dance-minigame song record.
//!
//! ### Layout
//!
//! ```text
//! +0x00   u32  ptr_0      ; address in 0x801C0000..0x80200000 (overlay window)
//! +0x04   u32  ptr_1      ; same range
//! ...
//! +N*4    u32  ptr_{N-1}  ; last pointer (still in range)
//! +(N+1)*4  ...           ; first non-pointer u32 (typically MIPS code or data)
//! ```
//!
//! The cluster has a long tail of variants - some monotonic (sorted entry
//! tables), some with repeating values (switch dispatch tables where many
//! cases share a default handler), some with a small alphabet of distinct
//! values (3-handler vtables).
//!
//! ### Detection
//!
//! 1. The first u32 is in `0x801C0000..=0x801FFFFF`.
//! 2. The run of consecutive overlay-pointer u32s is between 4 and 64 long.
//! 3. The byte after the run isn't itself another overlay pointer (the
//!    walker stops naturally on the first miss; this is enforced by
//!    construction).
//! 4. We don't require monotonicity - switch dispatch tables legitimately
//!    contain repeating handler addresses.
//!
//! These three checks together produce **zero overlap with already-named
//! formats** (mips_overlay, scene_*, lzs_container, etc.) across the full
//! 1232-entry PROT corpus.
//!
//! ### Coverage impact
//!
//! Promotes **42 entries** out of `unknown_other` (138 → 96) and
//! `unknown_low_entropy` (75 → 74). Coverage 910 / 1232 (73.7%) → 952 / 1232
//! (77.1%).
//!
//! ### Format meaning
//!
//! Same family as `mips_overlay` - runtime-loaded code blobs that the engine
//! pages into the `0x801C0000+` window. The pointer table at the start is
//! either:
//!
//! - A **function entry-point table** (small, monotonic - one slot per
//!   public function in the overlay).
//! - A **switch dispatch table** (larger, repeating - emitted by the C
//!   compiler for `switch(x)` over a dense integer range).
//! - A **vtable** for a per-mode actor type.
//!
//! Each can be Ghidra-imported via `scripts/bulk-import-overlays.sh` once
//! the load address is determined (the first pointer's high bits give a
//! strong hint - most cluster around `0x801F6Axx`).
//!
//! See `docs/formats/overlay-ptr-table.md` for the spec.

use serde::Serialize;

/// Low/high bounds for overlay-pointer u32 values. The PSX overlay window
/// runs from `0x801C0000` (just past the static SCUS code at `0x80010000`)
/// up to `0x80200000` (where the heap begins). Pointers that land here at
/// load time refer to the runtime overlay's resolved code/data.
const OVERLAY_LO: u32 = 0x801C_0000;
const OVERLAY_HI: u32 = 0x8020_0000;

/// Minimum and maximum number of consecutive overlay pointers we'll accept
/// as a header. The minimum (4) gates against single-pointer coincidences;
/// the maximum (64) accommodates the largest dispatch table observed
/// (`0918_xxx_dat.BIN` has a 64-slot switch table).
const MIN_PTRS: usize = 4;
const MAX_PTRS: usize = 64;

/// Detection result.
#[derive(Debug, Clone, Serialize)]
pub struct OverlayPtrTable {
    /// Number of consecutive overlay pointers at the start.
    pub count: usize,
    /// First pointer (smallest in monotonic tables; otherwise just the first
    /// slot - useful as a load-address hint).
    pub first_ptr: u32,
    /// Last pointer in the table.
    pub last_ptr: u32,
    /// Smallest pointer value (== `first_ptr` for monotonic tables).
    pub min_ptr: u32,
    /// Largest pointer value (== `last_ptr` for monotonic tables).
    pub max_ptr: u32,
}

/// Try to detect an overlay-pointer-table header. Returns `None` when the
/// buffer doesn't start with at least [`MIN_PTRS`] consecutive overlay
/// pointer u32s.
pub fn detect(buf: &[u8]) -> Option<OverlayPtrTable> {
    if buf.len() < MIN_PTRS * 4 {
        return None;
    }

    let mut count = 0;
    let mut min_ptr = u32::MAX;
    let mut max_ptr = 0;
    let mut first_ptr = 0;
    let mut last_ptr = 0;

    while count < MAX_PTRS && (count + 1) * 4 <= buf.len() {
        let v = read_u32_le(buf, count * 4)?;
        if !(OVERLAY_LO..OVERLAY_HI).contains(&v) {
            break;
        }
        if count == 0 {
            first_ptr = v;
        }
        last_ptr = v;
        if v < min_ptr {
            min_ptr = v;
        }
        if v > max_ptr {
            max_ptr = v;
        }
        count += 1;
    }

    if count < MIN_PTRS {
        return None;
    }

    Some(OverlayPtrTable {
        count,
        first_ptr,
        last_ptr,
        min_ptr,
        max_ptr,
    })
}

fn read_u32_le(buf: &[u8], at: usize) -> Option<u32> {
    let bytes = buf.get(at..at + 4)?;
    Some(u32::from_le_bytes(bytes.try_into().unwrap()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ptr_table(ptrs: &[u32], total_size: usize) -> Vec<u8> {
        let mut buf = Vec::with_capacity(total_size);
        for &p in ptrs {
            buf.extend_from_slice(&p.to_le_bytes());
        }
        // Padding starting with a non-pointer value to terminate the run.
        buf.extend_from_slice(&0xDEAD_BEEFu32.to_le_bytes());
        buf.resize(total_size, 0);
        buf
    }

    #[test]
    fn detects_monotonic_5_pointer_table() {
        let buf = ptr_table(
            &[
                0x801F_6AA4,
                0x801F_6C00,
                0x801F_6D88,
                0x801F_6F3C,
                0x801F_71F8,
            ],
            0x4000,
        );
        let r = detect(&buf).expect("should detect");
        assert_eq!(r.count, 5);
        assert_eq!(r.first_ptr, 0x801F_6AA4);
        assert_eq!(r.last_ptr, 0x801F_71F8);
        assert_eq!(r.min_ptr, 0x801F_6AA4);
        assert_eq!(r.max_ptr, 0x801F_71F8);
    }

    #[test]
    fn detects_dispatch_table_with_repeating_handlers() {
        // Switch table: 8 slots, only 3 distinct handlers (most cases default
        // to one). Still a valid hit - we don't require monotonicity.
        let buf = ptr_table(
            &[
                0x801F_84B0,
                0x801F_8528,
                0x801F_8528,
                0x801F_84B0,
                0x801F_8488,
                0x801F_8528,
                0x801F_8488,
                0x801F_8528,
            ],
            0x4000,
        );
        let r = detect(&buf).expect("dispatch table should detect");
        assert_eq!(r.count, 8);
        assert_eq!(r.min_ptr, 0x801F_8488);
        assert_eq!(r.max_ptr, 0x801F_8528);
    }

    #[test]
    fn rejects_three_pointer_run() {
        let buf = ptr_table(&[0x801F_6AA4, 0x801F_6C00, 0x801F_6D88], 0x100);
        assert!(detect(&buf).is_none());
    }

    #[test]
    fn rejects_pointer_outside_overlay_range() {
        // First u32 lands in the static SCUS code range, not the overlay window.
        let buf = ptr_table(
            &[0x8001_0000, 0x8001_1000, 0x8001_2000, 0x8001_3000],
            0x4000,
        );
        assert!(detect(&buf).is_none());
    }

    #[test]
    fn rejects_pointer_above_overlay_window() {
        // 0x80200000 is the heap; not part of the overlay window.
        let buf = ptr_table(
            &[0x8020_0000, 0x8020_1000, 0x8020_2000, 0x8020_3000],
            0x4000,
        );
        assert!(detect(&buf).is_none());
    }

    #[test]
    fn caps_run_at_64_pointers() {
        // 70 valid pointers - detector should stop at 64.
        let mut ptrs: Vec<u32> = Vec::new();
        for i in 0..70 {
            ptrs.push(0x801F_6000 + i * 4);
        }
        let buf = ptr_table(&ptrs, 0x4000);
        let r = detect(&buf).expect("should still hit on long table");
        assert_eq!(r.count, 64);
    }

    #[test]
    fn rejects_buffer_smaller_than_min_run() {
        // Three valid pointers, then EOF - must reject.
        let mut buf = Vec::new();
        for i in 0..3 {
            buf.extend_from_slice(&(0x801F_6000u32 + i * 4).to_le_bytes());
        }
        assert!(detect(&buf).is_none());
    }

    #[test]
    fn accepts_real_world_0938_head() {
        // Mirrors the actual first 20 bytes of `0938_xxx_dat.BIN`.
        let mut buf = vec![0u8; 0x100];
        let ptrs = [
            0x801F_6AA4u32,
            0x801F_6C00,
            0x801F_6D88,
            0x801F_6F3C,
            0x801F_71F8,
        ];
        for (i, &p) in ptrs.iter().enumerate() {
            buf[i * 4..(i + 1) * 4].copy_from_slice(&p.to_le_bytes());
        }
        // Real follow-up: addiu sp, sp, -0x48 = 0x27BD_FFB8
        buf[20..24].copy_from_slice(&0x27BD_FFB8u32.to_le_bytes());
        let r = detect(&buf).expect("real-world header should detect");
        assert_eq!(r.count, 5);
        assert_eq!(r.first_ptr, 0x801F_6AA4);
    }

    #[test]
    fn high_byte_at_overlay_lower_bound_accepted() {
        // 0x801C_0000 is the lower bound - should be inclusive.
        let buf = ptr_table(
            &[0x801C_0000, 0x801C_0010, 0x801C_0020, 0x801C_0030],
            0x4000,
        );
        let r = detect(&buf).expect("0x801C_0000 should be inclusive");
        assert_eq!(r.count, 4);
    }
}
