//! The two carriers a save's *engine-only* state rides in, and the resume
//! point both hosts print and re-enter:
//!
//! - the LGSF `LGX5` trailer ([`SaveResume`]) - optional, so a save without
//!   a resume point is byte-identical to a v4 file;
//! - the `LGXE` blob a retail SC block keeps in the `0x1A18..0x1FFC` tail
//!   retail composes as zeros and never reads back, so the browser's card
//!   Save no longer drops the play clock / party composition / chain library
//!   on a round-trip;
//! - the retail scene-label + location fields the resume point maps onto,
//!   written the way retail's own composer fills them.
//!
//! No disc, no card: everything here is synthetic.

use legaia_save::card::{
    RETAIL_LIVE_STATE_SIZE, RETAIL_LOCATION_NAME_LEN, RETAIL_LOCATION_NAME_OFFSET,
    RETAIL_SCENE_LABEL_LEN, RETAIL_SCENE_LABEL_OFFSET,
};
use legaia_save::{
    BLOCK_SIZE, CharSaveExt, CharacterRecord, HpMpSp, Party, RETAIL_BLOCK_CHECKSUM_OFFSET,
    RETAIL_ENGINE_EXT_CAPACITY, RETAIL_ENGINE_EXT_MAGIC, RETAIL_ENGINE_EXT_OFFSET,
    SAVE_FILE_EXT5_MAGIC, SaveExt, SaveExtV2, SaveFile, SaveResume, SavedChainRecord,
    displayed_level, sc_block_checksum_valid,
};

fn a_record(name: &str, level: u8, xp: u32) -> CharacterRecord {
    let mut r = CharacterRecord::zeroed();
    r.set_name(name);
    r.set_magic_rank(level);
    r.set_cumulative_xp(xp);
    r.set_hp_mp_sp(HpMpSp {
        hp_cur: 123,
        hp_max: 456,
        mp_cur: 7,
        mp_max: 89,
        sp_cur: 0,
        sp_max: 0,
    });
    r
}

fn a_save() -> SaveFile {
    SaveFile {
        party: Party {
            members: vec![a_record("Vahn", 12, 5000), a_record("Noa", 11, 4000)],
        },
        ext: SaveExt {
            story_flags: 0x1234,
            story_flag_bits: vec![0xAB; 512],
            money: 4321,
            inventory: vec![(3, 2), (9, 1)],
        },
        ext_v2: SaveExtV2 {
            play_time_seconds: 3661,
            active_party: vec![1, 0],
            per_char: vec![
                (
                    0,
                    CharSaveExt {
                        learned_arts_mask: 0b1011,
                        spells: vec![0x81, 0x83],
                        seru_captures: vec![(5, 40)],
                        active_chains: [[1, 2, 3, 0], [4, 0, 0, 0], [0; 4], [0; 4]],
                        shiny_spells: vec![0x83],
                    },
                ),
                (1, CharSaveExt::default()),
            ],
            saved_chains: vec![SavedChainRecord {
                char_slot: 0,
                name: "Tornado Flame".into(),
                sequence: vec![1, 2, 3],
            }],
        },
    }
}

fn a_resume() -> SaveResume {
    SaveResume {
        scene: "town01".into(),
        location: "Rim Elm".into(),
    }
}

/// A block "retail" wrote: a live-state front, a zero tail, a valid sum.
fn retail_shaped_block() -> Vec<u8> {
    let mut block = vec![0u8; BLOCK_SIZE];
    for (i, b) in block[..RETAIL_LIVE_STATE_SIZE].iter_mut().enumerate() {
        *b = (i % 251) as u8;
    }
    legaia_save::restamp_sc_block_checksum(&mut block);
    block
}

// ---------------------------------------------------------------------------
// LGSF LGX5 trailer
// ---------------------------------------------------------------------------

/// The scenario suite hashes `save_full().write()`: an empty resume point
/// must not change a single byte.
#[test]
fn empty_resume_leaves_the_v4_bytes_identical() {
    let sf = a_save();
    assert_eq!(sf.write(), sf.write_with_resume(&SaveResume::default()));
    assert!(!sf.write().windows(4).any(|w| w == SAVE_FILE_EXT5_MAGIC));
}

#[test]
fn resume_trailer_round_trips_and_is_appended_after_lgx4() {
    let sf = a_save();
    let plain = sf.write();
    let bytes = sf.write_with_resume(&a_resume());
    assert!(
        bytes.starts_with(&plain),
        "the trailer is appended, not spliced"
    );
    assert_eq!(&bytes[plain.len()..plain.len() + 4], &SAVE_FILE_EXT5_MAGIC);

    let (back, resume) = SaveFile::parse_with_resume(&bytes).unwrap();
    assert_eq!(back, sf);
    assert_eq!(resume, a_resume());
    // The plain parser still reads the same save and just skips the trailer.
    assert_eq!(SaveFile::parse(&bytes).unwrap(), sf);
}

/// A v4 file written before the trailer existed reads with an empty resume.
#[test]
fn a_file_without_the_trailer_reads_an_empty_resume() {
    let (_, resume) = SaveFile::parse_with_resume(&a_save().write()).unwrap();
    assert!(resume.is_empty());
}

#[test]
fn resume_strings_clamp_to_the_retail_field_widths() {
    let long = SaveResume {
        scene: "x".repeat(40),
        location: "y".repeat(100),
    };
    let (_, back) = SaveFile::parse_with_resume(&a_save().write_with_resume(&long)).unwrap();
    assert_eq!(back.scene.len(), RETAIL_SCENE_LABEL_LEN - 1);
    assert_eq!(back.location.len(), RETAIL_LOCATION_NAME_LEN - 1);
}

#[test]
fn a_truncated_trailer_is_an_error_not_a_silent_default() {
    let mut bytes = a_save().write_with_resume(&a_resume());
    bytes.truncate(bytes.len() - 3);
    assert!(SaveFile::parse_with_resume(&bytes).is_err());
}

// ---------------------------------------------------------------------------
// Retail SC block: the resume fields
// ---------------------------------------------------------------------------

#[test]
fn resume_writes_the_retail_scene_and_location_fields_nul_padded() {
    let mut block = retail_shaped_block();
    a_resume().write_into_retail_sc_block(&mut block).unwrap();
    let at = RETAIL_SCENE_LABEL_OFFSET;
    assert_eq!(&block[at..at + 6], b"town01");
    assert!(
        block[at + 6..at + RETAIL_SCENE_LABEL_LEN]
            .iter()
            .all(|&b| b == 0)
    );
    let at = RETAIL_LOCATION_NAME_OFFSET;
    assert_eq!(&block[at..at + 7], b"Rim Elm");
    assert!(
        block[at + 7..at + RETAIL_LOCATION_NAME_LEN]
            .iter()
            .all(|&b| b == 0)
    );
    assert!(
        sc_block_checksum_valid(&block),
        "the writer restamps the sum"
    );
    assert_eq!(SaveResume::from_retail_sc_block(&block), a_resume());
    // Nothing else moved: the previous-scene label right after the field is
    // whatever it was.
    let pristine = retail_shaped_block();
    let after = RETAIL_SCENE_LABEL_OFFSET + RETAIL_SCENE_LABEL_LEN;
    assert_eq!(&block[after..after + 0x10], &pristine[after..after + 0x10]);
}

/// A free block's leftover bytes are not a name: the reader stops at the
/// first non-printable byte, so a never-composed field reads as empty.
#[test]
fn unwritten_resume_fields_read_empty_not_garbage() {
    let mut block = retail_shaped_block();
    block[RETAIL_SCENE_LABEL_OFFSET] = 0x01;
    block[RETAIL_LOCATION_NAME_OFFSET] = 0xC7;
    assert!(SaveResume::from_retail_sc_block(&block).is_empty());
}

// ---------------------------------------------------------------------------
// Retail SC block: the engine-ext blob
// ---------------------------------------------------------------------------

#[test]
fn the_engine_ext_region_is_exactly_the_unread_tail() {
    assert_eq!(RETAIL_ENGINE_EXT_OFFSET, RETAIL_LIVE_STATE_SIZE);
    assert_eq!(
        RETAIL_ENGINE_EXT_OFFSET + RETAIL_ENGINE_EXT_CAPACITY,
        RETAIL_BLOCK_CHECKSUM_OFFSET
    );
    assert_eq!(RETAIL_ENGINE_EXT_CAPACITY, 0x5E4);
}

/// The whole point: a card Save keeps the play clock, party composition,
/// per-character ext and chain library across a card round-trip, and the
/// block still validates under retail's own checksum.
#[test]
fn engine_ext_round_trips_through_a_retail_block_and_keeps_it_valid() {
    let sf = a_save();
    let mut block = retail_shaped_block();
    sf.write_into_retail_sc_block(&mut block).unwrap();
    assert!(
        sf.write_engine_ext_into_retail_sc_block(&mut block)
            .unwrap()
    );
    assert_eq!(
        &block[RETAIL_ENGINE_EXT_OFFSET..RETAIL_ENGINE_EXT_OFFSET + 4],
        &RETAIL_ENGINE_EXT_MAGIC
    );
    assert!(sc_block_checksum_valid(&block));

    let back = SaveFile::from_retail_sc_block(&block, 4).unwrap();
    assert_eq!(back.ext_v2, sf.ext_v2);
    assert_eq!(back.ext_v2.play_time_seconds, 3661);
    // The fixture's live-state pattern makes slots 2..3 non-zero, so the
    // retail record walk reads four; the two composed ones are what matter.
    assert_eq!(back.party.members[0].name(), "Vahn");
    assert_eq!(back.party.members[1].name(), "Noa");
    assert_eq!(back.ext.money, 4321);
}

/// The blob writer is a sibling of the composer, not part of it: it touches
/// the tail and the checksum word and nothing retail reads.
#[test]
fn engine_ext_writer_touches_only_the_tail_and_the_checksum() {
    let pristine = retail_shaped_block();
    let mut block = pristine.clone();
    a_save()
        .write_engine_ext_into_retail_sc_block(&mut block)
        .unwrap();
    let touched: Vec<usize> = pristine
        .iter()
        .zip(block.iter())
        .enumerate()
        .filter(|(_, (a, b))| a != b)
        .map(|(i, _)| i)
        .collect();
    assert!(!touched.is_empty());
    assert!(
        touched.iter().all(|&i| i >= RETAIL_ENGINE_EXT_OFFSET),
        "wrote below the live-state boundary: {touched:?}"
    );
}

/// A block retail wrote has a zero tail, so the reader sees no magic and the
/// save reads exactly as it did before the blob existed.
#[test]
fn a_retail_block_without_the_blob_reads_the_default_ext() {
    let mut block = retail_shaped_block();
    a_save().write_into_retail_sc_block(&mut block).unwrap();
    let back = SaveFile::from_retail_sc_block(&block, 4).unwrap();
    assert_eq!(back.ext_v2, SaveExtV2::default());
    assert!(SaveFile::read_engine_ext_from_retail_sc_block(&block).is_none());
}

/// A blob that cannot fit is not written - the tail is zeroed, the retail
/// regions untouched, the block still valid - and the reader falls back.
#[test]
fn an_oversized_blob_is_withheld_and_the_block_stays_loadable() {
    let mut sf = a_save();
    sf.ext_v2.saved_chains = (0..200)
        .map(|i| SavedChainRecord {
            char_slot: 0,
            name: format!("chain number {i} with a long enough name"),
            sequence: vec![1; 40],
        })
        .collect();
    let mut block = retail_shaped_block();
    sf.write_into_retail_sc_block(&mut block).unwrap();
    assert!(
        !sf.write_engine_ext_into_retail_sc_block(&mut block)
            .unwrap()
    );
    assert!(
        block[RETAIL_ENGINE_EXT_OFFSET..RETAIL_BLOCK_CHECKSUM_OFFSET]
            .iter()
            .all(|&b| b == 0)
    );
    assert!(sc_block_checksum_valid(&block));
    let back = SaveFile::from_retail_sc_block(&block, 4).unwrap();
    assert_eq!(back.ext_v2, SaveExtV2::default());
    assert_eq!(back.party.members[1].name(), "Noa");
}

/// A damaged blob costs the ext, never the save.
#[test]
fn a_corrupt_blob_falls_back_to_the_default_ext() {
    let sf = a_save();
    let mut block = retail_shaped_block();
    sf.write_into_retail_sc_block(&mut block).unwrap();
    sf.write_engine_ext_into_retail_sc_block(&mut block)
        .unwrap();
    // Lie about the v2 body length so the parse walks off the end.
    block[RETAIL_ENGINE_EXT_OFFSET + 4] = 0xFF;
    block[RETAIL_ENGINE_EXT_OFFSET + 5] = 0xFF;
    let back = SaveFile::from_retail_sc_block(&block, 4).unwrap();
    assert_eq!(back.ext_v2, SaveExtV2::default());
}

/// Re-writing a smaller blob over a larger one leaves no stale bytes.
#[test]
fn rewriting_the_blob_clears_the_previous_tail() {
    let big = a_save();
    let mut small = a_save();
    small.ext_v2.saved_chains.clear();
    small.ext_v2.per_char.clear();
    let mut block = retail_shaped_block();
    big.write_engine_ext_into_retail_sc_block(&mut block)
        .unwrap();
    small
        .write_engine_ext_into_retail_sc_block(&mut block)
        .unwrap();
    let back = SaveFile::read_engine_ext_from_retail_sc_block(&block).unwrap();
    assert_eq!(back, small.ext_v2);
    let mut fresh = retail_shaped_block();
    small
        .write_engine_ext_into_retail_sc_block(&mut fresh)
        .unwrap();
    assert_eq!(
        &block[RETAIL_ENGINE_EXT_OFFSET..],
        &fresh[RETAIL_ENGINE_EXT_OFFSET..],
        "the tail is a pure function of the save"
    );
}

// ---------------------------------------------------------------------------
// The info-panel law both hosts share
// ---------------------------------------------------------------------------

/// Retail's displayed-level byte wins when it is a level; a record retail
/// never displayed (zero byte) falls back to what its XP implies, so a save
/// never prints as level 0.
#[test]
fn displayed_level_prefers_the_record_byte_and_falls_back_to_xp() {
    assert_eq!(displayed_level(&a_record("Vahn", 12, 5000)), 12);
    // 5000 XP is well past the L2 threshold (121) - the fallback is not 1.
    let by_xp = displayed_level(&a_record("Vahn", 0, 5000));
    assert!(by_xp > 1 && by_xp < 99, "xp fallback gave {by_xp}");
    assert_eq!(displayed_level(&CharacterRecord::zeroed()), 1);
    assert_eq!(displayed_level(&a_record("Vahn", 200, 0)), 1);
}

#[test]
fn leader_summary_reads_the_record_not_a_placeholder() {
    let s = a_save().leader_summary().unwrap();
    assert_eq!(s.name, "Vahn");
    assert_eq!(s.level, 12);
    assert_eq!(s.hp, (123, 456));
    assert_eq!(s.mp, (7, 89));
    assert_eq!(s.char_id, 0);
    assert!(SaveFile::default().leader_summary().is_none());
}
