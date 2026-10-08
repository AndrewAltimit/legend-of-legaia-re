//! Writing a live session into a mounted memory-card image - the one kernel
//! both play hosts' **card Save** commit.
//!
//! The browser rack wrote cards from the start; the native window refused a
//! Save into its `--card` port because the write lived in the page crate.
//! Everything that decides the bytes lives here now, so a block written by
//! either host is the same block:
//!
//! * [`card_save_index`] - which save number the block is filed under (its
//!   own filename's number on an overwrite, else the lowest number the
//!   card's directory does not already file - retail's own classification
//!   `classify_card_directory` answers "taken").
//! * [`retail_block_icon`] - the slot's portrait, off the disc's save-icon
//!   sheet.
//! * [`write_save_into_card`] - the SC payload, the engine ext tail, the
//!   resume point, the block identity, and the directory claim for a block
//!   that was free.
//!
//! What stays with each host is only where the card's bytes go afterwards:
//! the page marks the card dirty for the player to export, the native window
//! writes the image back to the file it mounted.

use legaia_save::card::{self, RetailBlockIcon};
use legaia_save::emu::MountedCard;
use legaia_save::{SaveFile, SaveResume};

use crate::save_select::{
    CARD_SLOT_CLASSES, SlotContent, card_dir_entries, card_dir_slot_of, card_directory_scan,
    card_free_blocks, classify_card_directory,
};

/// The save number a rack *prefers* for a block it claims.
///
/// Retail's number comes from the save-select list position
/// (`_DAT_801F0210`), which is independent of the block the BIOS happens to
/// place the file in - a real card can hold `-01` in block 1 and `-00` in
/// block 2. A rack addressing blocks has no such list, so it starts from the
/// block; [`card_save_index`] reconciles that against the card.
pub fn preferred_slot_for_block(block: u8) -> u32 {
    u32::from(block.saturating_sub(1))
}

/// The save number to stamp into `block` of `card`.
///
/// * **The block already carries a Legaia save.** Its number is the one in
///   its own filename, because an overwrite does not re-claim the directory
///   frame - a different number would leave the block's title digits and its
///   filename disagreeing about which save it is.
/// * **The block is free.** Take [`preferred_slot_for_block`] unless the card
///   already files a save under it, in which case take the lowest number it
///   does not. Filenames on a card must be unique - the BIOS directory is
///   keyed by them.
///
/// Which numbers are taken is answered by retail's own walk:
/// `classify_card_directory` (`FUN_801E1208`) stamps
/// [`SlotContent::LegaiaSave`] on every save number a directory names, over
/// the free-block budget the preview grid prices.
pub fn card_save_index(card: &MountedCard, block: u8) -> u32 {
    let existing = card
        .block_is_save_start(block)
        .then(|| card.dir_frame(block))
        .flatten()
        .and_then(|f| card_dir_slot_of(f.get(0x0A..)?));
    if let Some(index) = existing {
        return index as u32;
    }
    let entries = card_dir_entries(card);
    let (dir_table, dir_count) = card_directory_scan(&entries);
    let free = card_free_blocks(&dir_table, dir_count).max(0) as u32;
    let names: Vec<&[u8]> = entries.iter().map(|e| e.name.as_slice()).collect();
    let classes = classify_card_directory(&names, free);
    let taken = |n: u32| {
        classes
            .get(n as usize)
            .is_some_and(|c| *c == SlotContent::LegaiaSave)
    };
    let preferred = preferred_slot_for_block(block);
    if !taken(preferred) {
        return preferred;
    }
    (0..CARD_SLOT_CLASSES as u32)
        .find(|n| !taken(*n))
        // A full class array means every number is spoken for; keeping the
        // preference leaves the collision visible rather than silently
        // renumbering to a wrong block.
        .unwrap_or(preferred)
}

/// The memory-card portrait for save number `slot`, read off the disc's
/// portrait sheet (`legaia_asset::save_icon`, PROT 899).
///
/// `None` when the sheet cannot be read or the slot is one the sheet does not
/// cover - the block-identity write then leaves the icon region as found
/// rather than stamping a wrong one.
pub fn retail_block_icon(index: &crate::scene::ProtIndex, slot: u32) -> Option<RetailBlockIcon> {
    let entry = index
        .entry_bytes(legaia_asset::save_icon::PROT_ENTRY as u32)
        .ok()?;
    let sheet = legaia_asset::save_icon::parse_entry(&entry).ok()?;
    // The slot -> tile mapping is retail's own (`0x3C0 + slot * 4`
    // halfwords), so it goes through the port rather than being open-coded
    // even though the map is the identity.
    let tile = legaia_asset::save_icon::tile_for_slot(slot as usize);
    if tile >= legaia_asset::save_icon::USABLE_TILE_COUNT {
        return None;
    }
    Some(RetailBlockIcon {
        clut: sheet.tile_clut_bytes(tile).ok()?,
        pixels: sheet.tile_block_pixels(tile).ok()?,
    })
}

/// What [`write_save_into_card`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CardWrite {
    /// The save number the block was filed under.
    pub save_slot: u32,
    /// `false` when the engine-only ext blob was too large for the block's
    /// unread tail and was withheld (the retail payload is written either
    /// way).
    pub ext_written: bool,
    /// `true` when the block was free and its directory frame was claimed.
    pub claimed: bool,
}

/// Write `sf` + `resume` into `block` of `card`, in place.
///
/// Every byte outside the block (other saves, the container header) is
/// preserved, so the result is still the player's own card. In order:
///
/// 1. the SC payload, rebuilt from the save ([`SaveFile::write_into_retail_sc_block`]);
/// 2. the engine-only half - play clock, party composition, per-character
///    ext, chain library - into the block's unread tail (`0x1A18..0x1FFC`,
///    zero on every retail card and never copied back by retail's loader),
///    withheld rather than failing when it does not fit;
/// 3. the resume point into retail's own fields (scene label `+0x408`,
///    banner name `+0x200`);
/// 4. the block identity - the save number in the title and the slot's
///    portrait - which the payload writer cannot derive;
/// 5. the directory claim, for a block that was free.
///
/// `icon` is the caller's [`retail_block_icon`] for the save number this
/// resolves; pass `index` to have it looked up here.
pub fn write_save_into_card(
    card: &mut MountedCard,
    block: u8,
    sf: &SaveFile,
    resume: &SaveResume,
    index: Option<&crate::scene::ProtIndex>,
) -> Result<CardWrite, String> {
    let save_slot = card_save_index(card, block);
    let icon = index.and_then(|i| retail_block_icon(i, save_slot));
    let view = card.view;
    let was_active = view.block_is_save_start(&card.bytes, block);
    let sc = view
        .sc_block_mut(&mut card.bytes, block)
        .ok_or_else(|| format!("card has no block {block}"))?;
    sf.write_into_retail_sc_block(sc)
        .map_err(|e| format!("save: {e}"))?;
    let ext_written = sf
        .write_engine_ext_into_retail_sc_block(sc)
        .map_err(|e| format!("save: {e}"))?;
    resume
        .write_into_retail_sc_block(sc)
        .map_err(|e| format!("save: {e}"))?;
    card::write_retail_block_identity(sc, save_slot, icon.as_ref())
        .map_err(|e| format!("save: {e}"))?;
    if !was_active {
        view.claim_block(
            &mut card.bytes,
            block,
            &card::legaia_save_filename(save_slot),
        )
        .map_err(|e| format!("{e}"))?;
    }
    card.dirty = true;
    Ok(CardWrite {
        save_slot,
        ext_written,
        claimed: !was_active,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use legaia_save::card::{BLOCK_SIZE, CARD_MAGIC, CARD_SIZE, DIR_FRAME_SIZE, DIR_FRAMES, state};

    fn blank_card() -> MountedCard {
        let mut buf = vec![0u8; CARD_SIZE];
        buf[..2].copy_from_slice(&CARD_MAGIC);
        for i in 1..=DIR_FRAMES {
            let off = DIR_FRAME_SIZE * i;
            buf[off..off + 4].copy_from_slice(&state::FREE.to_le_bytes());
        }
        MountedCard::from_bytes(buf, "blank").unwrap()
    }

    fn a_save() -> SaveFile {
        let mut w = crate::world::World::new();
        w.party.money = 1234;
        w.save_full()
    }

    /// A Save into a free block of a blank card claims the block, files it
    /// under the block's preferred number, and reads back through the same
    /// `save_at` both hosts' Load uses - resume point included.
    #[test]
    fn a_free_block_write_round_trips_through_the_load_path() {
        let mut card = blank_card();
        let resume = SaveResume {
            scene: "town01".into(),
            location: "Rim Elm".into(),
        };
        let before = card.bytes.clone();
        let w = write_save_into_card(&mut card, 2, &a_save(), &resume, None).unwrap();
        assert_eq!(w.save_slot, 1, "block 2 prefers save number 1");
        assert!(w.claimed, "a free block's directory frame is claimed");
        assert!(card.dirty);
        let (sf, back) = card.save_at(1).expect("cell 1 = block 2 holds the save");
        assert_eq!(sf.ext.money, 1234);
        assert_eq!(back.scene, "town01");
        // Nothing outside block 2 and its directory frame moved.
        let blk = BLOCK_SIZE * 2;
        let frame = DIR_FRAME_SIZE * 2;
        for (i, (a, b)) in before.iter().zip(card.bytes.iter()).enumerate() {
            let inside = (blk..blk + BLOCK_SIZE).contains(&i)
                || (frame..frame + DIR_FRAME_SIZE).contains(&i);
            if !inside {
                assert_eq!(a, b, "byte {i:#x} outside the written block changed");
            }
        }
    }

    /// The slot portrait and title digits land inside the summed range, so a
    /// written block must still pass the load's checksum verify with an icon
    /// in hand - the path both hosts take whenever they hold the disc.
    /// Writing the identity after the last restamp left every such save
    /// "Damaged data." on the next load.
    #[test]
    fn a_written_block_passes_the_load_checksum_with_its_portrait() {
        let mut card = blank_card();
        let resume = SaveResume {
            scene: "town01".into(),
            location: "Rim Elm".into(),
        };
        let icon = card::RetailBlockIcon {
            clut: [0x5A; card::RETAIL_ICON_CLUT_BYTES],
            pixels: [0xC3; card::RETAIL_ICON_FRAME_BYTES],
        };
        let view = card.view;
        for _ in 0..2 {
            let w = write_save_into_card(&mut card, 2, &a_save(), &resume, None).unwrap();
            let sc = view.sc_block_mut(&mut card.bytes, 2).unwrap();
            card::write_retail_block_identity(sc, w.save_slot, Some(&icon)).unwrap();
            assert!(
                card::sc_block_checksum_valid(view.sc_block(&card.bytes, 2).unwrap()),
                "a block written with its identity must verify on load"
            );
        }
    }

    /// An overwrite keeps the block's own number and claims nothing.
    #[test]
    fn an_overwrite_keeps_the_number_and_claims_nothing() {
        let mut card = blank_card();
        let resume = SaveResume::default();
        write_save_into_card(&mut card, 3, &a_save(), &resume, None).unwrap();
        let w = write_save_into_card(&mut card, 3, &a_save(), &resume, None).unwrap();
        assert_eq!(w.save_slot, 2);
        assert!(!w.claimed);
    }
}
