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

fn locate_card() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    let dir = PathBuf::from(home).join(".mednafen/sav");
    if !dir.exists() {
        return None;
    }
    let entries = std::fs::read_dir(&dir).ok()?;
    for e in entries.flatten() {
        let p = e.path();
        let name = p.file_name()?.to_string_lossy().to_string();
        if name.contains("Legaia") && name.ends_with(".0.mcr") {
            return Some(p);
        }
    }
    None
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
