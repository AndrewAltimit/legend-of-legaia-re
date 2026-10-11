//! The 32-byte-block copy (`FUN_8003D26C`), the unrolled word loop the
//! battle-scene chunk walker uses to place a TMD body into its aligned buffer.
//!
//! Read out of the instruction stream, cross-checked against
//! `extracted/SCUS_942.54` at `0x800 + va - 0x80010000`; no `SCUS_942.54`
//! bytes live in this crate.
//!
//! REF: FUN_8001FE70
//!
//! REPLACED-BY: the borrow-in-place chunk walk in
//! `legaia_container_formats::parse_streaming_with`. That covers [`copy_blocks_32`]. Its retail
//! caller `FUN_8001FE70` **is** ported and live, as
//! `legaia_container_formats::parse_streaming_with`, but the copy itself is retail's way of
//! moving a chunk body into an aligned buffer and the Rust walker borrows the
//! chunk in place instead. There is no destination buffer to copy into, so
//! there is no call site - not a missing host. The kernel is kept for the two
//! edges the instruction stream settles and a `memcpy` would not: the
//! `count <= 0` case still copies one block, and the stride is fixed at `0x20`.

/// PORT: FUN_8003D26C
///
/// REPLACED-BY: the borrow-in-place chunk walk in
/// `legaia_container_formats::parse_streaming_with` - there is no destination buffer to
/// copy into, so there is no call site.
///
/// 32-byte-block copy - the unrolled `t0..t7` load/store loop the
/// battle-scene chunk walker uses to place a TMD body into its aligned
/// buffer (`FUN_8001FE70`, see `docs/subsystems/asset-loader.md`).
/// Copies `count` blocks of `0x20` bytes from `src` to `dst`.
///
/// Faithful edges, both off the instruction stream:
///
/// - **Do-while**: the count decrement + `bgtz` sit at the loop *tail*
///   (`0x8003D2AC..0x8003D2B4`), so `count <= 0` still copies exactly
///   one block.
/// - The retail body moves whole words (`lw`/`sw`), so its pointers must
///   be 4-byte aligned; the byte-slice port has no such constraint but
///   preserves the byte-for-byte result.
///
/// Both slices must hold `max(count, 1) * 32` bytes; the port panics
/// where retail would stride into unrelated memory.
pub fn copy_blocks_32(dst: &mut [u8], src: &[u8], count: i32) {
    let blocks = count.max(1) as usize;
    let len = blocks * 32;
    dst[..len].copy_from_slice(&src[..len]);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copy_blocks_32_copies_exactly_count_blocks() {
        let src: Vec<u8> = (0..96u8).collect();
        let mut dst = vec![0u8; 96];
        copy_blocks_32(&mut dst, &src, 2);
        assert_eq!(&dst[..64], &src[..64]);
        assert!(dst[64..].iter().all(|&b| b == 0), "third block untouched");
    }

    #[test]
    fn copy_blocks_32_do_while_copies_one_block_for_non_positive_counts() {
        // The decrement + bgtz live at the loop tail, so count <= 0 still
        // moves one 0x20-byte block.
        for count in [0, -1, -100] {
            let src = [0xABu8; 64];
            let mut dst = [0u8; 64];
            copy_blocks_32(&mut dst, &src, count);
            assert_eq!(&dst[..32], &src[..32], "count={count}");
            assert!(dst[32..].iter().all(|&b| b == 0), "count={count}");
        }
    }
}
