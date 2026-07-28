//! A save the engine writes into a memory-card block must be one retail
//! would load back.
//!
//! Retail's loader re-sums the block's additive checksum word before it will
//! read the save (`docs/subsystems/save-screen.md`, "Save-block checksum");
//! a block whose word is stale captions as "Damaged data." and is refused.
//! `legaia_save` covers that discipline for its **targeted** patches (the
//! coin / gold writers restamp, and `emu.rs` pins that only the field and the
//! checksum word change), but the path both hosts' Save screens now take is
//! the *whole-block* composer `SaveFile::write_into_retail_sc_block`, and
//! nothing asserted the word it leaves behind.
//!
//! That is the gap this file closes. It is disc-free and card-free: the block
//! is a synthetic `0x2000` buffer, so it runs in CI.

use legaia_engine_core::world::World;
use legaia_save::card::{RETAIL_BLOCK_CHECKSUM_OFFSET, SAVE_BLOCK_MAGIC};
use legaia_save::{SaveFile, sc_block_checksum_valid};

/// One memory-card block. `legaia_save::card::CARD_BLOCK_BYTES`' value, taken
/// here as the buffer size a host hands the composer.
const BLOCK_BYTES: usize = 0x2000;

/// A block pre-filled with a pattern, so "the checksum happens to match"
/// cannot pass by accident and every stray write shows.
fn dirty_block() -> Vec<u8> {
    (0..BLOCK_BYTES)
        .map(|i| (i as u8).wrapping_mul(31) ^ 0x3C)
        .collect()
}

fn a_world() -> World {
    let mut world = World::new();
    world.roster = legaia_save::Party::zeroed(4);
    for (i, member) in world.roster.members.iter_mut().enumerate() {
        let mut hms = member.hp_mp_sp();
        hms.hp_cur = 40 + i as u16;
        hms.hp_max = 120;
        hms.mp_cur = 7;
        hms.mp_max = 33;
        member.set_hp_mp_sp(hms);
    }
    world.party_count = 4;
    world.money = 4321;
    world.inventory.insert(0x77, 5);
    world
}

/// The composer must leave a block retail accepts - magic, a checksum word
/// that re-sums, and a payload that parses straight back.
#[test]
fn a_composed_block_validates_against_the_retail_sum() {
    let mut block = dirty_block();
    assert!(
        !sc_block_checksum_valid(&block),
        "non-vacuous: the pattern fill starts with a stale word"
    );

    let sf = a_world().save_full();
    sf.write_into_retail_sc_block(&mut block).unwrap();

    assert_eq!(&block[..2], &SAVE_BLOCK_MAGIC, "SC magic");
    assert!(
        sc_block_checksum_valid(&block),
        "retail re-sums this word on load and refuses the block if it \
         disagrees - a Save that skips the restamp ships a block the game \
         captions \"Damaged data.\""
    );

    let back = SaveFile::from_retail_sc_block(&block, 4).unwrap();
    assert_eq!(back.party.members.len(), 4);
    assert_eq!(back.ext.money, 4321);
}

/// The word is genuinely load-bearing here, not incidentally right: flip one
/// payload byte after the composer ran and the block must stop validating.
#[test]
fn a_byte_flipped_after_composition_invalidates_the_block() {
    let mut block = dirty_block();
    a_world()
        .save_full()
        .write_into_retail_sc_block(&mut block)
        .unwrap();
    assert!(sc_block_checksum_valid(&block));

    // A byte well inside the payload and clear of the checksum word itself.
    let probe = RETAIL_BLOCK_CHECKSUM_OFFSET / 2;
    block[probe] ^= 0xFF;
    assert!(
        !sc_block_checksum_valid(&block),
        "the sum covers the payload, so an edit under it must show"
    );
}

/// The composer is an **in-place patch**, not a block rewrite: it stamps the
/// magic and four regions and leaves the rest of the block alone. Composing
/// one save into two differently-filled blocks therefore does NOT produce the
/// same bytes, and both results validate - the checksum is over whatever the
/// block ends up holding.
///
/// That is the documented contract (`SaveFile::write_into_retail_sc_block`,
/// "in place"), and it is right for editing an existing save. It has a sharp
/// edge for a *new* one: `RETAIL_GAME_DATA_OFFSET..` carries the location
/// name (`0x200`), the scene label (`0x408`) and the coin bank (`0x464`)
/// beside the gold slot, and only gold is composed. A host that claims a
/// previously-free card block and composes into it (the browser rack's
/// `write_session_into_card` -> `claim_block`, which writes the directory
/// frame and does not clear the data block) therefore ships a save whose
/// location and scene label are whatever the card held there.
///
/// This pins the region list rather than the leftovers, so extending the
/// composer fails here and the reader updates one list instead of
/// discovering the omission from a garbled info panel.
#[test]
fn composition_is_an_in_place_patch_over_a_named_region_list() {
    let sf = a_world().save_full();
    let mut patched = dirty_block();
    sf.write_into_retail_sc_block(&mut patched).unwrap();
    let pristine = dirty_block();

    let touched: Vec<usize> = pristine
        .iter()
        .zip(patched.iter())
        .enumerate()
        .filter(|(_, (a, b))| a != b)
        .map(|(i, _)| i)
        .collect();
    assert!(!touched.is_empty());

    // The regions the composer owns today, in block coordinates. The record
    // array is **four** slots wide, not `RETAIL_MAX_CHAR_RECORDS`: slot 3
    // (Terra)'s tail deliberately aliases the story-flag bitmap, and the
    // composer's write order - records, then flags, then inventory - is what
    // makes that benign (see `RETAIL_MAX_CHAR_RECORDS`' own note).
    let owned = |i: usize| {
        use legaia_save::card::{
            RETAIL_CHAR_RECORD_HEADER_SIZE, RETAIL_CHAR_RECORD_STRIDE, RETAIL_GAME_DATA_OFFSET,
            RETAIL_GOLD_OFFSET, RETAIL_INVENTORY_OFFSET, RETAIL_INVENTORY_SIZE,
            RETAIL_STORY_FLAGS_OFFSET, RETAIL_STORY_FLAGS_SIZE,
        };
        let records = RETAIL_GAME_DATA_OFFSET + RETAIL_CHAR_RECORD_HEADER_SIZE;
        i < 2
            || (records..records + 4 * RETAIL_CHAR_RECORD_STRIDE).contains(&i)
            || (RETAIL_STORY_FLAGS_OFFSET..RETAIL_STORY_FLAGS_OFFSET + RETAIL_STORY_FLAGS_SIZE)
                .contains(&i)
            || (RETAIL_INVENTORY_OFFSET..RETAIL_INVENTORY_OFFSET + RETAIL_INVENTORY_SIZE)
                .contains(&i)
            || (RETAIL_GOLD_OFFSET..RETAIL_GOLD_OFFSET + 4).contains(&i)
            || (RETAIL_BLOCK_CHECKSUM_OFFSET..RETAIL_BLOCK_CHECKSUM_OFFSET + 4).contains(&i)
    };
    let stray: Vec<usize> = touched.into_iter().filter(|&i| !owned(i)).collect();
    assert!(
        stray.is_empty(),
        "the composer wrote outside its region list at {stray:?} - if that is \
         intended, widen the list here"
    );

    // And the un-composed fields really are untouched: this is the edge
    // above, stated as a fact about the composer rather than about any
    // particular leftover value.
    for off in [
        legaia_save::card::RETAIL_LOCATION_NAME_OFFSET,
        legaia_save::card::RETAIL_SCENE_LABEL_OFFSET,
        legaia_save::card::RETAIL_COINS_OFFSET,
    ] {
        assert_eq!(
            patched[off], pristine[off],
            "offset {off:#x} is not part of the composed set"
        );
    }
}

/// The block's first 128 bytes are the PSX **title frame**, the only part a
/// real card's Load screen shows: `SC` at `+0`, the icon-frame descriptor at
/// `+2` and the block count at `+3` (`card_flow::SAVE_HEADER_MAGIC`), the
/// title from `+4`, the icon CLUT at `+0x60` and the 16x16 4bpp icon at
/// `+0x80`.
///
/// The composer stamps the two-byte `SC` and nothing else of it, so a save
/// written into a previously-free card block carries a valid payload and a
/// valid checksum behind a header the BIOS browser reads as junk. Retail's
/// own rule is ported and sitting right there unused - `SAVE_HEADER_MAGIC`
/// is the full four bytes and `save_title_digits` is the slot's two
/// full-width numerals.
///
/// This is a characterisation of today's composer, deliberately written so
/// that **fixing it fails here**: the assertion is "the composer does not
/// write the title frame", and whoever makes it write one deletes this test
/// and pins the bytes instead.
#[test]
fn the_composer_does_not_write_the_psx_title_frame() {
    use legaia_engine_core::card_flow::{SAVE_HEADER_MAGIC, save_title_digits};

    let mut block = dirty_block();
    let pristine = dirty_block();
    a_world()
        .save_full()
        .write_into_retail_sc_block(&mut block)
        .unwrap();

    assert_eq!(
        &block[..2],
        &SAVE_HEADER_MAGIC[..2],
        "the `SC` half is written"
    );
    assert_eq!(
        &block[2..4],
        &pristine[2..4],
        "the icon-frame descriptor and block count are not - retail's          `SAVE_HEADER_MAGIC` carries {:?} there",
        &SAVE_HEADER_MAGIC[2..4]
    );

    // Title (`+4..`), icon CLUT (`+0x60`) and icon bitmap (`+0x80`) likewise.
    for off in [0x04usize, 0x60, 0x80] {
        assert_eq!(
            block[off], pristine[off],
            "title-frame byte {off:#x} is not part of the composed set"
        );
    }

    // The rule for the title is ported and correct - it is only uncalled.
    // Slot 0 shows as "01", biased into the Shift-JIS full-width range.
    assert_eq!(save_title_digits(0), [0x4F, 0x50]);
    assert_eq!(save_title_digits(11), [0x50, 0x51]);
}
