//! The disc-patching foundation every patcher feature writes through:
//! `disc::DiscPatcher` (same-size in-place PROT-entry edits over
//! `legaia_iso::write`'s Mode 2/2352 sector write-back with EDC/ECC
//! re-encoded), the PPF 3.0 patch writer / reader, the free-space ledger
//! of the SCUS and overlay regions the code mods claim, and the MAN
//! re-pack budget helpers.
//!
//! `legaia-patcher` re-exports every module here at its old path. Doc links
//! that pointed back up at it are plain code spans, since rustdoc cannot
//! resolve a link into a dependent crate.

#![forbid(unsafe_code)]

pub mod disc;
pub mod ppf;
pub mod rng;
pub mod space_ledger;

/// Compressed-stream budget for a scene bundle's MAN: the space its LZS stream
/// may occupy without overflowing into the next asset, i.e. the distance from
/// the MAN's `data_offset` to the **next descriptor's** `data_offset` (or the
/// entry end if the MAN is last).
///
/// This is the *original, stable* footprint - it does not depend on the current
/// stream length. That matters when several passes (encounter / chest / shop)
/// each decompress → edit → recompress the **same** MAN: our LZS re-packer is
/// often a touch tighter than Sony's, so reading the budget back from the
/// just-written (shorter) stream would shrink it on every pass and make a later
/// pass overflow + skip a scene it should have edited (the bug where Biron
/// Monastery's shop stayed vanilla after encounters/chests ran first). Reading
/// the budget from the descriptor boundary keeps every pass on the same, full
/// budget. The descriptors' `data_offset`s never move (all edits are same-size
/// in place), so the boundary is constant across passes.
pub fn man_compressed_budget(
    table: &legaia_asset::scene_asset_table::SceneAssetTable,
    man_data_offset: usize,
    entry_len: usize,
) -> usize {
    table
        .used()
        .iter()
        .map(|d| d.data_offset as usize)
        .filter(|&o| o > man_data_offset)
        .min()
        .unwrap_or(entry_len)
        .saturating_sub(man_data_offset)
}

/// Recompress a decoded MAN into `budget` bytes: greedy first, then the
/// optimal packer when greedy misses. `None` if even optimal overflows.
///
/// Every MAN re-pack site must use this rather than bare
/// [`legaia_lzs::compress`]: a growing pass (the Delilas Challenge, a grown
/// translation) can leave a MAN that only the optimal packer fits back into
/// its zero-slack footprint, and a later same-size pass (Earth Egg price,
/// chests, shops, doors) re-packs the whole stream - with greedy alone that
/// later pass would overflow and skip (or fail) a scene the first pass proved
/// fits.
pub fn compress_within(decoded: &[u8], budget: usize) -> Option<Vec<u8>> {
    let stream = legaia_lzs::compress(decoded);
    if stream.len() <= budget {
        return Some(stream);
    }
    let stream = legaia_lzs::compress_optimal(decoded);
    (stream.len() <= budget).then_some(stream)
}
