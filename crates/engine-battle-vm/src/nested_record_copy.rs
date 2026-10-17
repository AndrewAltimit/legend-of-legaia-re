//! The two-level variable-length word-record copier of the `SCUS_942.54`
//! battle band ([`copy_nested_records`], `FUN_80055854`).
//!
//! Read out of the instruction stream in the reference dump
//! (`ghidra/scripts/funcs/80055854.txt`), not the decompiled C. No
//! `SCUS_942.54` bytes live in this crate.
//!
//! # Why no host calls it
//!
//! [`copy_nested_records`] stages a nested record block between two raw `u32`
//! buffers. Its one retail caller `FUN_80052FA0` (call sites `80053438`,
//! `800534F4`) is ported as `actor-formats::battle_char_palette`, over typed
//! engine state: animation and keyframe data arrive as parsed
//! `legaia_actor_formats` types and are handed to the renderer as typed clips,
//! never staged word-wise, so no caller holds a `&mut [u32]` destination for
//! it to advance. A wire would have to re-introduce the retail representation
//! purely so the kernel had something to chew.
//!
//! It is ported because its edge case (the do-while floor) is observable and
//! testable on its own.

/// Copy a two-level, variable-length record structure of 32-bit words -
/// `FUN_80055854`.
///
/// PORT: FUN_80055854
/// REPLACED-BY: `legaia_actor_formats::battle_char_palette`, the port of this
/// routine's one retail caller `FUN_80052FA0`, which parses the nested block
/// into typed records instead of staging it word-wise between two raw `u32`
/// buffers. Nothing in the engine holds a `&mut [u32]` destination for this
/// to advance, because the destination it would fill does not exist as a
/// representation - the same substitution the rest of this module's
/// "Port boundary" note describes, and a call site would have to
/// re-introduce retail's staging buffer purely so the kernel had something
/// to chew.
///
/// The retail routine takes a source and destination `u32*` and walks a
/// nested table, returning the advanced destination pointer (i.e. the
/// number of words written). The layout, straight off the loads/stores:
///
/// - **Outer header**: three words. Words 0 and 1 are copied verbatim;
///   word 2 is the outer record count (`t0`). If it is `<= 0` the routine
///   stops after the header (`blez t0` at `0x80055884`).
/// - For each of `count` outer records:
///   - **Inner header**: three words. Words 0 and 1 verbatim; word 2 is
///     the inner element count (`v0`).
///   - **Inner body**: `inner_count * 2` words (`sll a2, v0, 1` at
///     `0x800558bc`), copied verbatim. A non-positive doubled count skips
///     the body (`blez a2`).
///
/// Both count tests are signed (`blez` / `slt`), and both loops are
/// `while`-shaped (the guard precedes the body), so a zero count copies
/// only the header - unlike the `do-while` copier in
/// [`block_copy_32::copy_blocks_32`](legaia_engine_vm::block_copy_32::copy_blocks_32).
///
/// This port reads whole words from `src` and writes them into `dst`,
/// returning the count of words written. It stops early - returning what
/// it has copied - if either slice runs short, where retail (which does no
/// bounds check) would stride into adjacent memory.
pub fn copy_nested_records(src: &[u32], dst: &mut [u32]) -> usize {
    let mut si = 0usize;
    let mut di = 0usize;

    // Helper: copy one word src[si] -> dst[di], advancing both. Returns
    // false when either side is exhausted.
    macro_rules! copy_one {
        () => {{
            match (src.get(si), dst.get_mut(di)) {
                (Some(&w), Some(slot)) => {
                    *slot = w;
                    si += 1;
                    di += 1;
                    true
                }
                _ => return di,
            }
        }};
    }

    // Outer header: word0, word1, then the outer count word.
    if !copy_one!() {
        return di;
    }
    if !copy_one!() {
        return di;
    }
    let outer_count = match src.get(si) {
        Some(&w) => w as i32,
        None => return di,
    };
    if !copy_one!() {
        return di;
    }

    if outer_count <= 0 {
        return di;
    }

    for _ in 0..outer_count {
        // Inner header: word0, word1, then the inner count word.
        if !copy_one!() {
            return di;
        }
        if !copy_one!() {
            return di;
        }
        let inner_count = match src.get(si) {
            Some(&w) => w as i32,
            None => return di,
        };
        if !copy_one!() {
            return di;
        }

        // Body: inner_count * 2 words.
        let body = inner_count.wrapping_mul(2);
        if body > 0 {
            for _ in 0..body {
                if !copy_one!() {
                    return di;
                }
            }
        }
    }

    di
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nested_copy_header_only_when_outer_count_is_zero() {
        // Outer header [w0, w1, 0] with a non-zero count word absent.
        let src = [0xAAAA_AAAA, 0xBBBB_BBBB, 0, 0xDEAD_BEEF];
        let mut dst = [0u32; 8];
        let written = copy_nested_records(&src, &mut dst);
        assert_eq!(written, 3, "blez outer count stops after the header");
        assert_eq!(&dst[..3], &[0xAAAA_AAAA, 0xBBBB_BBBB, 0]);
        assert_eq!(dst[3], 0, "trailing word untouched");
    }

    #[test]
    fn nested_copy_one_outer_one_inner_pair() {
        // Outer: [h0, h1, count=1]
        //   Inner: [i0, i1, inner=2] then 2*2 = 4 body words.
        let src = [
            0x0000_00A0, // h0
            0x0000_00A1, // h1
            1,           // outer count
            0x0000_00B0, // inner h0
            0x0000_00B1, // inner h1
            2,           // inner count
            0x10,
            0x11,
            0x12,
            0x13, // 4 body words
        ];
        let mut dst = [0u32; 16];
        let written = copy_nested_records(&src, &mut dst);
        assert_eq!(written, src.len());
        assert_eq!(&dst[..src.len()], &src[..]);
    }

    #[test]
    fn nested_copy_inner_body_is_double_the_inner_count() {
        // inner count 3 -> 6 body words.
        let mut src = vec![0xF0, 0xF1, 1, 0xE0, 0xE1, 3];
        src.extend([0x20, 0x21, 0x22, 0x23, 0x24, 0x25]);
        let mut dst = [0u32; 16];
        let written = copy_nested_records(&src, &mut dst);
        assert_eq!(written, 12);
        assert_eq!(&dst[..12], &src[..]);
    }

    #[test]
    fn nested_copy_zero_inner_count_copies_only_inner_header() {
        let src = [0xA0, 0xA1, 1, 0xB0, 0xB1, 0, 0x99];
        let mut dst = [0u32; 8];
        let written = copy_nested_records(&src, &mut dst);
        assert_eq!(written, 6, "blez a2 skips the body");
        assert_eq!(&dst[..6], &src[..6]);
        assert_eq!(dst[6], 0);
    }

    #[test]
    fn nested_copy_stops_when_destination_is_short() {
        let src = [0xA0, 0xA1, 1, 0xB0, 0xB1, 1, 0x30, 0x31];
        let mut dst = [0u32; 4];
        let written = copy_nested_records(&src, &mut dst);
        assert_eq!(written, 4, "no overrun past dst");
        assert_eq!(&dst, &[0xA0, 0xA1, 1, 0xB0]);
    }
}
