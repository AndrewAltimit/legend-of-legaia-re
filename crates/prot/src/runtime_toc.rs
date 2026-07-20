//! Queries against the in-RAM PROT TOC the boot loader installs at
//! `0x801C70F0`.
//!
//! PORT: FUN_8003E68C
//!
//! `FUN_8003E4E8` copies the first three sectors of `PROT.DAT` verbatim
//! into `0x801C70F0`, header words included, so the in-RAM table is the
//! file's word array with no transform. The resolver `FUN_8003E8A8` reads
//! an entry's start LBA from `TABLE[ram_index + 2]`; the routine ported
//! here is its sibling and returns the entry's **sector span**.
//!
//! See [`docs/formats/prot.md`](../../../docs/formats/prot.md) for the TOC
//! index spaces and [`docs/subsystems/boot.md`](../../../docs/subsystems/boot.md)
//! for the loader.
//!
//! ## Clean-room boundary
//!
//! No `SCUS_942.54` bytes live in this crate. The reference dump
//! `ghidra/scripts/funcs/8003e68c.txt` is the *spec*, cross-checked
//! against `extracted/SCUS_942.54` at file offset `0x2EE8C`.
//!
//! REF: FUN_8003E4E8, FUN_8003E8A8

/// Retail VA of the in-RAM PROT TOC.
pub const RAM_TOC_VA: u32 = 0x801C_70F0;

/// Word index skew between the in-RAM TOC and the word array
/// [`Archive::toc`](crate::archive::Archive::toc) holds.
///
/// The crate reads the TOC from `header_offset + 8`, i.e. it drops the
/// archive's two header words, so `Archive::toc[i]` is RAM word `i + 2`.
/// That is the same `+2` skew the CDNAME numbering space carries.
pub const RAM_WORD_SKEW: usize = 2;

/// Sector span of the entry at RAM TOC index `ram_index` -
/// `FUN_8003E68C`.
///
/// PORT: FUN_8003E68C
///
/// The twelve-instruction body is a pure leaf:
///
/// ```text
/// lui   v1, 0x801c
/// addiu v1, v1, 0x70f0        ; v1 = RAM_TOC_VA
/// addiu v0, a0, 3
/// sll   v0, v0, 2
/// addu  v0, v0, v1            ; &TABLE[a0 + 3]
/// addiu a0, a0, 2
/// sll   a0, a0, 2
/// addu  a0, a0, v1            ; &TABLE[a0 + 2]
/// lw    v1, 0(v0)
/// lw    v0, 0(a0)
/// jr    ra
/// subu  v0, v1, v0            ; TABLE[i+3] - TABLE[i+2]
/// ```
///
/// `TABLE[i + 2]` is the entry's start LBA (`FUN_8003E8A8` resolves the
/// same word) and `TABLE[i + 3]` is the *next* entry's start LBA, so the
/// difference is the entry's on-disc footprint in sectors - the same
/// `next_start - start_lba` quantity
/// [`Archive`](crate::archive::Archive) computes when it extends an entry
/// over a trailing gap.
///
/// This is **not** the TOC-indexed payload size `toc[p+5] - toc[p+3] + 4`.
/// The two agree only for entries with no trailing gap; where they differ,
/// this routine reports the larger footprint. Nothing in the body clamps
/// or checks, so a non-monotonic TOC row yields a wrapped result - hence
/// the `wrapping_sub` here and the `None` on a short table.
pub fn entry_sector_span(ram_toc: &[u32], ram_index: usize) -> Option<u32> {
    let start = *ram_toc.get(ram_index + 2)?;
    let next = *ram_toc.get(ram_index + 3)?;
    Some(next.wrapping_sub(start))
}

/// [`entry_sector_span`] against the crate's header-stripped TOC word
/// array, indexed by **extraction** entry index.
///
/// The two skews cancel, and the cancellation is worth spelling out
/// because the resulting expression is deceptively identical to the RAM
/// one. `Archive::toc[j]` is RAM word `j + `[`RAM_WORD_SKEW`], and
/// extraction entry `p` is RAM index `p + `[`RAM_WORD_SKEW`] (the same
/// `+2` the CDNAME numbering space carries). Substituting both gives
/// `toc[p + 2]` / `toc[p + 3]` - so this delegates unchanged, on a
/// different array in a different index space.
pub fn entry_sector_span_from_archive_toc(toc: &[u32], entry_index: usize) -> Option<u32> {
    entry_sector_span(toc, entry_index)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A synthetic TOC: two header words, then start LBAs 100, 140, 200.
    fn synthetic() -> Vec<u32> {
        vec![0xDEAD, 0xBEEF, 100, 140, 200, 260]
    }

    #[test]
    fn span_is_the_difference_of_consecutive_start_lbas() {
        let toc = synthetic();
        // RAM index 0 -> TABLE[2]=100, TABLE[3]=140.
        assert_eq!(entry_sector_span(&toc, 0), Some(40));
        assert_eq!(entry_sector_span(&toc, 1), Some(60));
        assert_eq!(entry_sector_span(&toc, 2), Some(60));
    }

    #[test]
    fn short_table_yields_none_rather_than_reading_past_the_end() {
        let toc = synthetic();
        assert_eq!(entry_sector_span(&toc, 3), None);
        assert_eq!(entry_sector_span(&[], 0), None);
    }

    #[test]
    fn non_monotonic_rows_wrap_exactly_as_subu_does() {
        // subu does not trap; a descending pair yields the two's
        // complement wrap, which is why Archive rejects the value
        // instead of trusting it.
        let toc = vec![0, 0, 500, 100];
        assert_eq!(entry_sector_span(&toc, 0), Some(0u32.wrapping_sub(400)));
    }

    #[test]
    fn ram_and_archive_index_spaces_land_on_the_same_words() {
        let toc = synthetic();
        for i in 0..3 {
            assert_eq!(
                entry_sector_span(&toc, i),
                entry_sector_span_from_archive_toc(&toc, i)
            );
        }
        assert_eq!(RAM_WORD_SKEW, 2);
        assert_eq!(RAM_TOC_VA, 0x801C70F0);
    }
}
