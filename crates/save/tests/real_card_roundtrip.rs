//! Disc-gated test: walk a real PSX memory card image (mednafen `.mcr`),
//! locate the Legaia save block, and verify the card walker is correct
//! against the canonical 128 KB layout.
//!
//! Doesn't try to parse the per-block payload (the offset where the
//! character record region starts inside a Legaia save block is not yet
//! reverse-engineered). What it asserts:
//!
//! - `parse_card` finds an active save block
//! - The block's product code starts with `BASCUS-94254` (the NA Legaia
//!   product code prefix)
//! - The block bytes start with the `SC` magic
//! - Round-trip a synthesised character record through `parse → write`
//!
//! Looks for the card image at the mednafen default path:
//!   `~/.mednafen/sav/Legend of Legaia (USA).<hash>.0.mcr`
//!
//! Skips silently when the file isn't present. Doesn't gate on
//! `LEGAIA_DISC_BIN` - the memory card isn't disc data.

use std::path::PathBuf;

use legaia_save::{
    CHARACTER_RECORD_SIZE, CharacterRecord, EquipmentSlots, HpMpSp, SAVE_BLOCK_MAGIC, SpellList,
    parse_card, read_block,
};

/// Find a Legaia memory-card image that actually holds an active save block.
///
/// The mednafen `sav` directory accumulates one `.mcr` per disc fingerprint,
/// and emulator playtests routinely leave behind *empty* cards (a fresh card
/// written before any in-game save). Those parse fine but carry zero active
/// blocks, so picking the first name match would make the decode test panic on
/// a card that simply has nothing to decode. Scan all candidates (sorted for a
/// stable choice) and return the first one with at least one active block;
/// return `None` - so the tests skip - only when no usable card exists.
fn locate_card() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    let dir = PathBuf::from(home).join(".mednafen/sav");
    if !dir.exists() {
        return None;
    }
    let mut candidates: Vec<PathBuf> = std::fs::read_dir(&dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .map(|n| {
                    let name = n.to_string_lossy();
                    name.contains("Legaia") && name.ends_with(".0.mcr")
                })
                .unwrap_or(false)
        })
        .collect();
    candidates.sort();
    candidates.into_iter().find(|p| {
        std::fs::read(p)
            .ok()
            .and_then(|b| parse_card(&b).ok())
            .is_some_and(|saves| !saves.is_empty())
    })
}

#[test]
fn real_psx_memory_card_decodes_legaia_save_block() {
    let Some(card_path) = locate_card() else {
        eprintln!("[skip] no Legaia memory-card image at ~/.mednafen/sav/");
        return;
    };
    let bytes = std::fs::read(&card_path).expect("read memory card");
    eprintln!(
        "[real-card] {} ({} bytes)",
        card_path.display(),
        bytes.len()
    );

    let saves = parse_card(&bytes).expect("parse memory card");
    assert!(
        !saves.is_empty(),
        "no active save blocks found in {}",
        card_path.display()
    );

    let first = &saves[0];
    eprintln!(
        "[real-card] save block={} chain={:?} product={}",
        first.block, first.block_chain, first.product_code
    );
    assert!(
        first.product_code.starts_with("BASCUS-94254"),
        "first save's product code should start with BASCUS-94254 (NA Legaia); got {}",
        first.product_code
    );

    let block = read_block(&bytes, first.block).expect("read save block");
    assert_eq!(
        &block[..2],
        &SAVE_BLOCK_MAGIC,
        "save block must start with SC magic"
    );
    eprintln!(
        "[real-card] block 1 SC magic OK; block size = {}",
        block.len()
    );
}

#[test]
fn retail_sc_block_character_records_extract() {
    let Some(card_path) = locate_card() else {
        eprintln!("[skip] no Legaia memory-card image at ~/.mednafen/sav/");
        return;
    };
    let bytes = std::fs::read(&card_path).expect("read memory card");
    let saves = legaia_save::parse_card(&bytes).expect("parse card");
    if saves.is_empty() {
        eprintln!("[skip] no active save blocks");
        return;
    }
    let block = legaia_save::read_block(&bytes, saves[0].block).expect("read block");
    let records = legaia_save::card::read_retail_char_records(block, 4).expect("extract records");
    assert!(!records.is_empty(), "expected at least 1 character record");
    // Each record should be exactly 0x414 bytes.
    for (i, rec) in records.iter().enumerate() {
        assert_eq!(rec.len(), CHARACTER_RECORD_SIZE, "record {} wrong size", i);
    }
    eprintln!(
        "[real-card] extracted {} character records from retail save",
        records.len()
    );
}

#[test]
fn synthetic_character_record_round_trip_preserves_typed_fields() {
    // Build a record with every documented field set, then round-trip
    // and confirm typed views read the same values.
    let mut rec = CharacterRecord::zeroed();
    rec.set_hp_mp_sp(HpMpSp {
        hp_cur: 312,
        hp_max: 500,
        mp_cur: 80,
        mp_max: 120,
        sp_cur: 8,
        sp_max: 15,
    });
    rec.set_stat_cap(999);
    rec.set_ability_bits([0xAA; legaia_save::ABILITY_BITS_LEN]);
    let mut ids = [0u8; legaia_save::MAX_SPELLS];
    ids[0] = 0x10;
    ids[1] = 0x20;
    let mut levels = [0u8; legaia_save::MAX_SPELLS];
    levels[0] = 5;
    levels[1] = 3;
    rec.set_spell_list(SpellList {
        count: 2,
        ids,
        levels,
    });
    rec.set_equipment(EquipmentSlots {
        slots: [1, 2, 3, 4, 5, 6, 7, 8],
    });

    let bytes = rec.write();
    let parsed = CharacterRecord::parse(&bytes).expect("parse round-trip");
    assert_eq!(parsed, rec);
    let hms = parsed.hp_mp_sp();
    assert_eq!(hms.hp_cur, 312);
    assert_eq!(hms.mp_max, 120);
    assert_eq!(parsed.stat_cap(), 999);
    let snap = parsed.snapshot();
    assert_eq!(snap.spell_count, 2);
    assert_eq!(snap.spell_ids[0], 0x10);
    assert_eq!(snap.equipment_slots, vec![1, 2, 3, 4, 5, 6, 7, 8]);
}

/// Real-card **coin patch** oracle: the site's minigame flow imports a
/// player's retail card, banks won casino coins into the pinned coin slot
/// (`RETAIL_COINS_OFFSET`, RAM `0x800845A4`), and exports the card back.
/// The patch must:
///
/// - read back the new value through `read_retail_coins`,
/// - leave every other byte of the card untouched (a no-op patch is
///   byte-identical - the export-what-you-imported baseline),
/// - keep the block parsing as a valid retail save
///   (`SaveFile::from_retail_sc_block` yields the same party records),
/// - keep every directory-frame XOR checksum self-consistent (the only
///   checksum a card image carries; the retail SC payload itself is a
///   plain RAM memcpy with no checksum - see `docs/subsystems/save-screen.md`).
#[test]
fn real_card_coin_patch_round_trips_and_stays_valid() {
    let Some(card_path) = locate_card() else {
        eprintln!("[skip] no Legaia memory-card image at ~/.mednafen/sav/");
        return;
    };
    let bytes = std::fs::read(&card_path).expect("read memory card");
    let view = legaia_save::emu::detect(&bytes).expect("detect card container");
    assert_eq!(view.format, legaia_save::emu::Format::RawCard);
    let saves = view.saves(&bytes).expect("list saves");
    assert!(!saves.is_empty());
    let block_idx = saves[0].block;

    // Baseline: a detected view over unmodified bytes IS the original file.
    let block = view.sc_block(&bytes, block_idx).expect("sc block");
    let coins_before = legaia_save::read_retail_coins(block).expect("read coins");
    let gold_before = legaia_save::read_retail_gold(block).expect("read gold");
    let before_save =
        legaia_save::ext::SaveFile::from_retail_sc_block(block, 4).expect("block parses");
    eprintln!(
        "[real-card] block {block_idx}: gold={gold_before} coins={coins_before} party={}",
        before_save.party.members.len()
    );

    // No-op round-trip: nothing written -> byte-identical by construction
    // (the view borrows; there is no re-serialisation step to drift).
    let mut untouched = bytes.clone();
    let _ = view.sc_block_mut(&mut untouched, block_idx).unwrap();
    assert_eq!(untouched, bytes, "no-op export is byte-identical");

    // Patch: bank new coins.
    let new_coins = coins_before.wrapping_add(777).min(9_999_999);
    let mut patched = bytes.clone();
    let pblock = view.sc_block_mut(&mut patched, block_idx).unwrap();
    legaia_save::write_retail_coins(pblock, new_coins).expect("patch coins");

    // Re-walk the patched card from scratch, as a fresh import would.
    let view2 = legaia_save::emu::detect(&patched).expect("re-detect");
    let block2 = view2.sc_block(&patched, block_idx).expect("re-read block");
    assert_eq!(legaia_save::read_retail_coins(block2), Some(new_coins));
    assert_eq!(
        legaia_save::read_retail_gold(block2),
        Some(gold_before),
        "gold untouched by the coin patch"
    );
    let after_save =
        legaia_save::ext::SaveFile::from_retail_sc_block(block2, 4).expect("still a valid save");
    assert_eq!(
        after_save.party.members.len(),
        before_save.party.members.len()
    );
    for (a, b) in after_save
        .party
        .members
        .iter()
        .zip(before_save.party.members.iter())
    {
        assert_eq!(a.raw, b.raw, "party records untouched");
    }

    // Exactly 4 bytes changed, all inside the coin slot.
    let base = legaia_save::BLOCK_SIZE * block_idx as usize + legaia_save::RETAIL_COINS_OFFSET;
    let diff: Vec<usize> = bytes
        .iter()
        .zip(patched.iter())
        .enumerate()
        .filter(|(_, (a, b))| a != b)
        .map(|(i, _)| i)
        .collect();
    assert!(
        diff.iter().all(|&i| (base..base + 4).contains(&i)),
        "only the coin slot may change: {diff:?}"
    );

    // Directory-frame checksums stay self-consistent (we never touch frames).
    for i in 1..=legaia_save::DIR_FRAMES {
        let f = &patched[0x80 * i..0x80 * i + 0x80];
        let xor = f[..0x7F].iter().fold(0u8, |acc, &b| acc ^ b);
        assert_eq!(f[0x7F], xor, "directory frame {i} checksum intact");
    }
}
