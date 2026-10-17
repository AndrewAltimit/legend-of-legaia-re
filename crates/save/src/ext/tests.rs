use super::*;
use crate::character::CharacterRecord;

fn minimal_save() -> SaveFile {
    SaveFile {
        party: Party {
            members: vec![CharacterRecord::zeroed()],
        },
        ext: SaveExt {
            story_flags: 0xDEAD_BEEF,
            money: 12345,
            inventory: vec![(1, 5), (7, 2), (255, 1)],
            story_flag_bits: Vec::new(),
            ..Default::default()
        },
        ext_v2: SaveExtV2::default(),
    }
}

#[test]
fn round_trip_v2_default_ext() {
    let sf = minimal_save();
    let bytes = sf.write();
    assert_eq!(&bytes[..4], b"LGSF");
    assert_eq!(bytes[4], SAVE_FILE_VERSION);
    let parsed = SaveFile::parse(&bytes).unwrap();
    assert_eq!(parsed, sf);
}

#[test]
fn empty_inventory_and_party() {
    let sf = SaveFile {
        party: Party { members: vec![] },
        ext: SaveExt {
            story_flags: 0,
            money: 0,
            inventory: vec![],
            story_flag_bits: Vec::new(),
            ..Default::default()
        },
        ext_v2: SaveExtV2::default(),
    };
    let bytes = sf.write();
    let parsed = SaveFile::parse(&bytes).unwrap();
    assert_eq!(parsed, sf);
}

#[test]
fn legacy_party_only_fallback() {
    // Old format: just raw party bytes with no magic header.
    let party = Party {
        members: vec![CharacterRecord::zeroed()],
    };
    let bytes = party.write();
    // Must NOT start with LGSF.
    assert_ne!(&bytes[..4], b"LGSF");
    let sf = SaveFile::parse(&bytes).unwrap();
    assert_eq!(sf.party.members.len(), 1);
    assert_eq!(sf.ext, SaveExt::default());
    assert_eq!(sf.ext_v2, SaveExtV2::default());
}

#[test]
fn multi_member_party() {
    let sf = SaveFile {
        party: Party {
            members: vec![CharacterRecord::zeroed(), CharacterRecord::zeroed()],
        },
        ext: SaveExt {
            story_flags: 1,
            money: 999,
            inventory: vec![(0, 3)],
            story_flag_bits: Vec::new(),
            ..Default::default()
        },
        ext_v2: SaveExtV2::default(),
    };
    let bytes = sf.write();
    let parsed = SaveFile::parse(&bytes).unwrap();
    assert_eq!(parsed.party.members.len(), 2);
    assert_eq!(parsed.ext.money, 999);
}

#[test]
fn round_trip_v2_full_ext() {
    let sf = SaveFile {
        party: Party {
            members: vec![CharacterRecord::zeroed()],
        },
        ext: SaveExt::default(),
        ext_v2: SaveExtV2 {
            play_time_seconds: 7200,
            field_position: None,
            audio_levels: None,
            options: None,
            active_party: vec![0, 1, 2],
            per_char: vec![
                (
                    0,
                    CharSaveExt {
                        learned_arts_mask: 0x0000_00FF,
                        spells: vec![0x10, 0x11, 0x20],
                        seru_captures: vec![(1, 50), (2, 100)],
                        active_chains: [[1, 2, 3, 4], [4, 4, 4, 0], [0, 0, 0, 0], [2, 1, 3, 4]],
                        shiny_spells: vec![0x11],
                    },
                ),
                (
                    1,
                    CharSaveExt {
                        learned_arts_mask: 0x0000_0007,
                        spells: vec![],
                        seru_captures: vec![(5, 25)],
                        active_chains: [[0; 4]; 4],
                        shiny_spells: vec![],
                    },
                ),
            ],
            saved_chains: vec![
                SavedChainRecord {
                    char_slot: 0,
                    name: "Combo A".into(),
                    sequence: vec![1, 2, 3, 4, 1],
                },
                SavedChainRecord {
                    char_slot: 2,
                    name: "Power".into(),
                    sequence: vec![4, 4, 4, 4],
                },
            ],
        },
    };
    let bytes = sf.write();
    let parsed = SaveFile::parse(&bytes).unwrap();
    assert_eq!(parsed, sf);
}

#[test]
fn parse_v1_file_into_v2_struct() {
    // Hand-craft a v1 save: just the v1 fields with version=1.
    let mut v1 = Vec::new();
    v1.extend_from_slice(&SAVE_FILE_MAGIC);
    v1.push(SAVE_FILE_VERSION_V1);
    v1.extend_from_slice(&0u32.to_le_bytes()); // story_flags
    v1.extend_from_slice(&500i32.to_le_bytes()); // money
    v1.push(1); // inv_count
    v1.push(7); // item id
    v1.push(3); // item count
    v1.push(0); // party_count
    let parsed = SaveFile::parse(&v1).unwrap();
    assert_eq!(parsed.ext.money, 500);
    assert_eq!(parsed.ext.inventory, vec![(7, 3)]);
    // V2 ext should be default for v1 saves.
    assert_eq!(parsed.ext_v2, SaveExtV2::default());
}

#[test]
fn parse_unsupported_version_errors() {
    let mut bad = vec![b'L', b'G', b'S', b'F', 99]; // version 99
    bad.extend(std::iter::repeat_n(0u8, 20));
    let err = SaveFile::parse(&bad).unwrap_err();
    assert!(format!("{err:#}").contains("unsupported version"));
}

#[test]
fn ext_v2_default_writes_minimum_bytes() {
    let sf = SaveFile::default();
    let bytes = sf.write();
    // Find the LGX2 marker.
    let marker_pos = bytes.windows(4).position(|w| w == b"LGX2").unwrap();
    // After magic + size header (8 bytes): play_time(4) + active_len(1)
    // + per_char_count(1) + saved_chain_count(1) = 7 bytes minimum.
    let ext_size = u32::from_le_bytes(bytes[marker_pos + 4..marker_pos + 8].try_into().unwrap());
    assert_eq!(ext_size, 7);
}

#[test]
fn truncated_v2_ext_returns_error() {
    let mut bad = SaveFile::default().write();
    bad.truncate(bad.len() - 3);
    let err = SaveFile::parse(&bad).unwrap_err();
    assert!(format!("{err:#}").contains("ext"));
}

#[test]
fn round_trip_v3_story_flag_bits() {
    let mut bits = vec![0u8; 0x200];
    for (i, b) in bits.iter_mut().enumerate() {
        *b = (i as u8).wrapping_mul(0x33);
    }
    let sf = SaveFile {
        party: Party {
            members: vec![CharacterRecord::zeroed()],
        },
        ext: SaveExt {
            story_flags: 0xCAFE_F00D,
            money: 42,
            inventory: vec![(0x10, 7)],
            story_flag_bits: bits.clone(),
            ..Default::default()
        },
        ext_v2: SaveExtV2::default(),
    };
    let bytes = sf.write();
    assert_eq!(bytes[4], SAVE_FILE_VERSION);
    let parsed = SaveFile::parse(&bytes).unwrap();
    assert_eq!(parsed, sf);
    assert_eq!(parsed.ext.story_flag_bits, bits);
}

/// An over-full inventory (>255 distinct ids) must clamp, not corrupt: the
/// count byte is a single u8, so without the clamp 256 entries would write a
/// `0` count while still emitting 512 pair bytes, misaligning the party parse
/// and bricking the save. The writer caps the count AND the pairs at 255 so
/// the file stays self-consistent and the party survives.
#[test]
fn over_full_inventory_clamps_instead_of_corrupting_the_save() {
    let inventory: Vec<(u8, u8)> = (0u8..=255).map(|id| (id, 1)).collect();
    assert_eq!(inventory.len(), 256);
    let sf = SaveFile {
        party: Party {
            members: vec![CharacterRecord::zeroed()],
        },
        ext: SaveExt {
            story_flags: 0x1234_5678,
            money: 99,
            inventory,
            story_flag_bits: vec![],
            ..Default::default()
        },
        ext_v2: SaveExtV2::default(),
    };
    let bytes = sf.write();
    let parsed = SaveFile::parse(&bytes).expect("a clamped save still parses");
    // The party + money parsed at the right offsets (not misaligned) and the
    // inventory came back as the clamped 255 entries.
    assert_eq!(parsed.party.members.len(), 1, "party survived the clamp");
    assert_eq!(parsed.ext.money, 99);
    assert_eq!(parsed.ext.inventory.len(), 255, "inventory clamped to 255");
}

#[test]
fn parse_v2_file_into_v3_struct_keeps_bits_empty() {
    // Hand-craft a v2 save (no LGX3 trailer).
    let mut v2 = Vec::new();
    v2.extend_from_slice(&SAVE_FILE_MAGIC);
    v2.push(SAVE_FILE_VERSION_V2);
    v2.extend_from_slice(&0u32.to_le_bytes()); // story_flags
    v2.extend_from_slice(&500i32.to_le_bytes()); // money
    v2.push(1); // inv_count
    v2.push(7);
    v2.push(3); // (id, count)
    v2.push(0); // party_count
    v2.extend_from_slice(&SAVE_FILE_EXT_MAGIC);
    // 7 bytes minimum LGX2 body (play_time(4) + active_len(1) + per_char_count(1) + saved_chain_count(1)).
    v2.extend_from_slice(&7u32.to_le_bytes());
    v2.extend_from_slice(&0u32.to_le_bytes()); // play_time
    v2.push(0); // active_party len
    v2.push(0); // per_char count
    v2.push(0); // saved_chain count
    let parsed = SaveFile::parse(&v2).unwrap();
    assert_eq!(parsed.ext.money, 500);
    assert!(parsed.ext.story_flag_bits.is_empty());
}

#[test]
fn truncated_v3_ext_returns_error() {
    let mut bits = vec![0u8; 0x200];
    bits[0] = 0xAB;
    let sf = SaveFile {
        party: Party::default(),
        ext: SaveExt {
            story_flags: 0,
            money: 0,
            inventory: vec![],
            story_flag_bits: bits,
            ..Default::default()
        },
        ext_v2: SaveExtV2::default(),
    };
    let mut bytes = sf.write();
    // Lop off the last 16 bytes - well inside the 0x200-byte LGX3 body.
    bytes.truncate(bytes.len() - 16);
    let err = SaveFile::parse(&bytes).unwrap_err();
    assert!(
        format!("{err:#}").to_lowercase().contains("lgx3"),
        "{err:#}"
    );
}

#[test]
fn save_file_round_trips_through_retail_sc_block() {
    use crate::card::{BLOCK_SIZE, SAVE_BLOCK_MAGIC};

    let mut bits = vec![0u8; crate::card::RETAIL_STORY_FLAGS_SIZE];
    for (i, b) in bits.iter_mut().enumerate() {
        *b = (i as u8).wrapping_add(1);
    }
    // Two records with a recognisable pattern in the first byte so the
    // reader doesn't stop at slot 0 thinking it's empty.
    let mut rec0 = CharacterRecord::zeroed();
    rec0.raw[0] = 0x11;
    let mut rec1 = CharacterRecord::zeroed();
    rec1.raw[0] = 0x22;
    let original = SaveFile {
        party: Party {
            members: vec![rec0, rec1],
        },
        ext: SaveExt {
            story_flags: 0,
            money: 0,
            inventory: vec![(0x05, 9), (0x10, 1), (0x33, 64)],
            story_flag_bits: bits.clone(),
            ..Default::default()
        },
        ext_v2: SaveExtV2::default(),
    };

    let mut sc_block = vec![0u8; BLOCK_SIZE];
    original.write_into_retail_sc_block(&mut sc_block).unwrap();
    assert_eq!(&sc_block[..2], &SAVE_BLOCK_MAGIC);

    let parsed = SaveFile::from_retail_sc_block(&sc_block, 4).unwrap();
    assert_eq!(parsed.party.members.len(), 2);
    assert_eq!(parsed.party.members[0].raw[0], 0x11);
    assert_eq!(parsed.party.members[1].raw[0], 0x22);
    assert_eq!(parsed.ext.story_flag_bits, bits);
    assert_eq!(parsed.ext.inventory, vec![(0x05, 9), (0x10, 1), (0x33, 64)]);
    // story_flags scratchpad u32 mirrors the first 4 bits-bytes LE.
    assert_eq!(
        parsed.ext.story_flags,
        u32::from_le_bytes([bits[0], bits[1], bits[2], bits[3]])
    );
}

// --- Panic-hardening / adversarial-input regression tests -------------

fn save_with_saved_chain() -> SaveFile {
    SaveFile {
        party: Party {
            members: vec![CharacterRecord::zeroed()],
        },
        ext: SaveExt {
            story_flags: 0,
            money: 0,
            inventory: vec![],
            story_flag_bits: Vec::new(),
            ..Default::default()
        },
        ext_v2: SaveExtV2 {
            play_time_seconds: 42,
            field_position: None,
            audio_levels: None,
            options: None,
            active_party: vec![0, 1],
            per_char: vec![(
                0,
                CharSaveExt {
                    learned_arts_mask: 0x12345678,
                    spells: vec![1, 2, 3],
                    seru_captures: vec![(0x81, 100), (0x82, 50)],
                    active_chains: [[1, 2, 3, 4]; 4],
                    shiny_spells: vec![2],
                },
            )],
            saved_chains: vec![SavedChainRecord {
                char_slot: 0,
                name: "Combo".to_string(),
                sequence: vec![0x10, 0x20, 0x30],
            }],
        },
    }
}

/// Truncating a well-formed LGSF v3 buffer at *every* prefix length must
/// only ever return `Err` (or `Ok` for the few prefixes that happen to
/// form a self-consistent shorter save) - never panic. This is the
/// blanket prefix-fuzz that surfaced the `parse_ext_v2` saved-chain
/// out-of-bounds read (a 1-byte guard before a 2-byte read).
#[test]
fn truncated_prefixes_never_panic() {
    let full = save_with_saved_chain().write();
    for len in 0..=full.len() {
        // Must not panic; result value is unimportant.
        let _ = SaveFile::parse(&full[..len]);
    }
}

/// Same prefix-fuzz, but tampering the declared ext-block size to point
/// the inner cursor at arbitrary boundaries. The historical bug was a
/// saved-chain `char_slot` landing as the final byte of the ext slice;
/// this drives the ext slice to end at every byte offset.
#[test]
fn tampered_ext_total_size_never_panics() {
    let full = save_with_saved_chain().write();
    let marker = full
        .windows(4)
        .position(|w| w == SAVE_FILE_EXT_MAGIC)
        .expect("ext magic present");
    let size_off = marker + 4;
    for claimed in 0u32..=200 {
        let mut bytes = full.clone();
        bytes[size_off..size_off + 4].copy_from_slice(&claimed.to_le_bytes());
        // Any claimed ext size, valid or not, must not panic the parser.
        let _ = SaveFile::parse(&bytes);
    }
}

/// Directed regression for the saved-chain OOB: an ext block whose final
/// byte is a saved-chain `char_slot` (so the `name_len` byte read would
/// run off the end) must return `Err`, not panic.
#[test]
fn saved_chain_char_slot_at_buffer_end_is_err_not_panic() {
    // Hand-build a minimal v2 ext block:
    //   play_time(4) active_len(=0) pc_count(=0) ch_count(=1) char_slot(1 byte, EOF)
    let ext_block: Vec<u8> = vec![
        0, 0, 0, 0, // play_time
        0, // active_len
        0, // pc_count
        1, // ch_count
        0, // saved_chain[0].char_slot  <- last byte; name_len missing
    ];
    let res = parse_ext_v2(&ext_block);
    assert!(res.is_err(), "expected Err on truncated saved-chain");
}

/// A full save round-trips with a saved chain present (guards against the
/// fix accidentally rejecting valid data).
#[test]
fn saved_chain_round_trips() {
    let sf = save_with_saved_chain();
    let bytes = sf.write();
    let parsed = SaveFile::parse(&bytes).unwrap();
    assert_eq!(parsed, sf);
    assert_eq!(parsed.ext_v2.saved_chains.len(), 1);
    assert_eq!(parsed.ext_v2.saved_chains[0].name, "Combo");
}

/// Random byte soup that happens to start with the LGSF magic must never
/// panic the versioned parser regardless of the trailing junk.
#[test]
fn lgsf_magic_with_junk_body_never_panics() {
    for seed in 0u64..512 {
        let mut x = seed.wrapping_mul(0x9E3779B97F4A7C15);
        let mut bytes = SAVE_FILE_MAGIC.to_vec();
        bytes.push(SAVE_FILE_VERSION);
        let n = (seed % 300) as usize;
        for _ in 0..n {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            bytes.push(x as u8);
        }
        let _ = SaveFile::parse(&bytes);
    }
}
