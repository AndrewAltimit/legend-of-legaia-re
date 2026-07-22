//! Monster-record edits for PROT entry 867 (the `battle_data` archive).
//!
//! Each monster occupies a fixed `0x14000`-byte slot at `(id-1) * 0x14000`,
//! laid out as `[u32 decompressed_size][Legaia LZS stream]` (see
//! [`legaia_asset::monster_archive`]). To change a value in the decoded record
//! - e.g. the item drop the randomizer shuffles - we decompress the slot, edit
//!   the decoded block in place, recompress with [`legaia_lzs::compress`], and
//!   rebuild `[u32 size][stream]` zero-padded back to the original slot size.
//!
//! The decoded length is unchanged, so the slot size and therefore every other
//! monster's slot offset stay fixed: a drop edit is a same-size, in-place byte
//! overwrite with no PROT TOC reshuffle. Our re-packer isn't byte-identical to
//! Sony's, but it packs tightly (lazy matching) and monster records compress far
//! enough that the re-packed stream fits the slot's slack (the [`repack_slot`]
//! guard rejects the rare case where it would not).

use anyhow::{Result, bail};
use legaia_asset::monster_archive::SLOT_STRIDE;

/// Decoded-record byte offset of the drop item id (`0` = no drop).
pub const DROP_ITEM_OFFSET: usize = 0x48;
/// Decoded-record byte offset of the drop chance, in percent (`rand()%100 < n`).
pub const DROP_CHANCE_OFFSET: usize = 0x49;

/// Decompress a monster slot (`[u32 size][LZS]`), hand the decoded record block
/// to `mutate` as a mutable slice, then re-pack into a fresh slot of exactly
/// [`SLOT_STRIDE`] bytes.
///
/// `mutate` edits the block in place; because it receives a `&mut [u8]` the
/// decoded length cannot change, so the re-packed slot stays the same size.
/// Errors if the slot is empty/filler (declared size `0`), the LZS decode
/// fails, or the recompressed stream plus its 4-byte header would overflow the
/// fixed slot.
pub fn repack_slot(slot_bytes: &[u8], mutate: impl FnOnce(&mut [u8])) -> Result<Vec<u8>> {
    if slot_bytes.len() < 4 {
        bail!("monster slot too small ({} bytes)", slot_bytes.len());
    }
    let declared = u32::from_le_bytes(slot_bytes[0..4].try_into().unwrap()) as usize;
    if declared == 0 {
        bail!("empty/filler monster slot (declared size 0)");
    }
    let mut block = legaia_lzs::decompress(&slot_bytes[4..], declared)?;
    mutate(&mut block);
    let stream = legaia_lzs::compress(&block);
    if 4 + stream.len() > SLOT_STRIDE {
        bail!(
            "re-packed stream does not fit slot: 4 + {} > {}",
            stream.len(),
            SLOT_STRIDE
        );
    }
    let mut out = Vec::with_capacity(SLOT_STRIDE);
    out.extend_from_slice(&(declared as u32).to_le_bytes());
    out.extend_from_slice(&stream);
    out.resize(SLOT_STRIDE, 0);
    Ok(out)
}

/// Set a monster slot's drop item id and drop chance (percent), returning the
/// re-packed slot bytes. Convenience wrapper over [`repack_slot`].
pub fn set_drop(slot_bytes: &[u8], drop_item: u8, drop_chance: u8) -> Result<Vec<u8>> {
    repack_slot(slot_bytes, |block| {
        block[DROP_ITEM_OFFSET] = drop_item;
        block[DROP_CHANCE_OFFSET] = drop_chance;
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a synthetic monster slot from a decoded block: `[u32 size][LZS]`
    /// padded to `SLOT_STRIDE`.
    fn fake_slot(block: &[u8]) -> Vec<u8> {
        let stream = legaia_lzs::compress(block);
        assert!(
            4 + stream.len() <= SLOT_STRIDE,
            "test block too large for a slot"
        );
        let mut slot = Vec::with_capacity(SLOT_STRIDE);
        slot.extend_from_slice(&(block.len() as u32).to_le_bytes());
        slot.extend_from_slice(&stream);
        slot.resize(SLOT_STRIDE, 0);
        slot
    }

    fn decode_slot(slot: &[u8]) -> Vec<u8> {
        let declared = u32::from_le_bytes(slot[0..4].try_into().unwrap()) as usize;
        legaia_lzs::decompress(&slot[4..], declared).unwrap()
    }

    #[test]
    fn set_drop_changes_only_drop_fields() {
        // A block with recognisable, non-zero content at every offset.
        let mut block: Vec<u8> = (0..512u32).map(|i| (i * 7 + 1) as u8).collect();
        block[DROP_ITEM_OFFSET] = 0x11;
        block[DROP_CHANCE_OFFSET] = 0x22;
        let slot = fake_slot(&block);

        let patched = set_drop(&slot, 0xAB, 50).unwrap();
        assert_eq!(patched.len(), SLOT_STRIDE, "slot size preserved");

        let out = decode_slot(&patched);
        assert_eq!(out.len(), block.len(), "decoded length preserved");

        let mut expected = block.clone();
        expected[DROP_ITEM_OFFSET] = 0xAB;
        expected[DROP_CHANCE_OFFSET] = 50;
        assert_eq!(out, expected, "only the two drop bytes changed");
    }

    #[test]
    fn empty_slot_is_rejected() {
        let slot = vec![0u8; SLOT_STRIDE]; // declared size 0
        assert!(set_drop(&slot, 1, 1).is_err());
    }

    #[test]
    fn too_small_slot_is_rejected() {
        assert!(set_drop(&[0u8, 1], 1, 1).is_err());
    }
}
