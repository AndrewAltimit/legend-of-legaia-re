//! Panic-hardening regression suite for the `legaia-asset` parser entry points.
//!
//! The web viewer and bulk scanners feed ARBITRARY PROT-entry / DATA_FIELD /
//! pack bytes into these parsers; a junk match must return `Err` / `None` /
//! a bounded `Ok`, never panic (OOB slice, capacity-overflow from an
//! unverified `with_capacity`, or arithmetic over/underflow).
//!
//! Every input here is SYNTHETIC (hand-constructed bytes); there is no real
//! game data, so the suite runs with `LEGAIA_DISC_BIN` unset.
//!
//! The assertions are deliberately weak ("did not panic"): the point is the
//! absence of a crash, not a particular classification. Where a function is
//! documented to always succeed (e.g. `categorize::classify`, `RecordTable`),
//! we additionally assert the call returns.

use legaia_asset::{self as asset, DecodeMode, Descriptor};

/// A spread of adversarial byte buffers: empty, 1-byte, truncated headers,
/// bogus huge counts/offsets, valid-magic-but-garbage-body, all-zero,
/// all-0xFF, and a pseudo-random stream.
fn adversarial_inputs() -> Vec<Vec<u8>> {
    // Degenerate sizes.
    let mut v: Vec<Vec<u8>> = vec![
        vec![],
        vec![0x00],
        vec![0xFF],
        vec![0x00, 0x00, 0x00],
        vec![0xFF; 3],
        vec![0x10, 0x00, 0x00, 0x00], // bare TIM magic, nothing else
    ];

    // Bogus huge u32 count/size leading words with a tiny tail. These are the
    // classic capacity-overflow trigger: a parser that does
    // `Vec::with_capacity(count)` before bounds-checking `count` aborts.
    for &lead in &[
        u32::MAX,
        0xFFFF_FFF0,
        0x7FFF_FFFF,
        0x0100_0000,
        0x0001_0000,
        0xDEAD_BEEF,
    ] {
        let mut b = lead.to_le_bytes().to_vec();
        b.extend_from_slice(&[0xAB; 12]);
        v.push(b);
    }

    // Valid-looking magic prefixes followed by garbage / truncation.
    let magics: [&[u8]; 5] = [
        b"pQES",                   // SEQ
        &[0x10, 0, 0, 0],          // TIM
        &[0x02, 0, 0, 0x80],       // Legaia TMD magic bytes (LE 0x80000002)
        b"VABp",                   // VAB
        &[0x84, 0x9B, 0x05, 0x01], // field-pack magic 0x01059B84 (LE)
    ];
    for m in magics {
        // magic + huge count + a little body
        let mut b = m.to_vec();
        b.extend_from_slice(&u32::MAX.to_le_bytes());
        b.extend_from_slice(&[0x55; 40]);
        v.push(b.clone());
        // magic alone, immediately truncated
        v.push(m.to_vec());
    }

    // Constant fills of various lengths (exercise the cheap-path detectors).
    for &n in &[31usize, 32, 64, 200, 600, 2048] {
        v.push(vec![0x00; n]);
        v.push(vec![0xAA; n]);
        v.push(vec![0xFF; n]);
    }

    // Pseudo-random streams of several lengths (deterministic LCG, no rand dep).
    for &n in &[37usize, 256, 1024, 4099, 0x10000 + 7] {
        let mut s: u32 = 0x1234_5678 ^ (n as u32);
        let buf: Vec<u8> = (0..n)
            .map(|_| {
                s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                (s >> 24) as u8
            })
            .collect();
        v.push(buf);
    }

    // A buffer that *almost* parses as a pack: count=1000 words but tiny body.
    {
        let mut b = 1000u32.to_le_bytes().to_vec();
        b.extend_from_slice(&[0u8; 8]);
        v.push(b);
    }

    // A "pochi"-prefixed buffer that is too short for the 0x786 EOF marker.
    v.push(b"pochipochi".to_vec());

    v
}

#[test]
fn parse_pack_never_panics() {
    for buf in adversarial_inputs() {
        let _ = asset::pack::parse_pack(&buf);
        let _ = asset::pack::extract_pack(&buf);
    }
}

#[test]
fn parse_streaming_never_panics() {
    for buf in adversarial_inputs() {
        // max_chunks bounds the walk; result is always Ok by contract.
        let r = asset::parse_streaming(&buf, 4096).expect("parse_streaming is infallible");
        // Sanity: it never reports more chunks than it could fit.
        assert!(r.chunks.len() <= 4096);
    }
}

#[test]
fn parse_player_lzs_never_panics() {
    for buf in adversarial_inputs() {
        for &count in &[0usize, 1, 2, 3, 4, 8, 16, 64] {
            let _ = asset::parse_player_lzs(&buf, count);
        }
    }
}

#[test]
fn validate_never_panics() {
    for buf in adversarial_inputs() {
        for &count in &[1usize, 3, 8] {
            let _ = asset::validate(&buf, count);
        }
    }
}

#[test]
fn decode_never_panics() {
    for buf in adversarial_inputs() {
        // Adversarial descriptors: huge offset, huge size, both modes.
        for &(size, off) in &[
            (0u32, 0u32),
            (0x00FF_FFFF, 0), // max 24-bit size field
            (0x100, 0xFFFF_FFFF),
            (0x40, 8),
        ] {
            let d = Descriptor {
                type_byte: 0x02,
                size,
                data_offset: off,
            };
            let _ = asset::decode(&buf, &d, DecodeMode::Lzs);
            let _ = asset::decode(&buf, &d, DecodeMode::Raw);
        }
    }
}

#[test]
fn categorize_classify_never_panics() {
    for buf in adversarial_inputs() {
        // `classify` is documented to always return a `FileReport`.
        let r = asset::categorize::classify(&buf);
        assert_eq!(r.size, buf.len());
    }
}

#[test]
fn detector_modules_never_panic() {
    for buf in adversarial_inputs() {
        let _ = asset::anm_detect::detect(&buf);
        let _ = asset::data_field_truncated::detect(&buf);
        let _ = asset::mips_overlay::detect(&buf);
        let _ = asset::overlay_ptr_table::detect(&buf);
        let _ = asset::scene_scripted_asset_table::detect(&buf);
        let _ = asset::scene_scripted_asset_table::record_ranges(&buf);
        let _ = asset::scene_asset_table::detect(&buf);
        let _ = asset::scene_event_scripts::detect(&buf);
        let _ = asset::scene_event_scripts::record_ranges(&buf);
        let _ = asset::scene_v12_table::detect(&buf);
        let _ = asset::field_pack::detect(&buf);
        let _ = asset::effect_bundle::detect(&buf);
        let _ = asset::scene_tmd_stream::detect(&buf);
        let _ = asset::tmd_size_prefix::detect(&buf);
        let _ = asset::scene_vab_stream::detect(&buf);
        let _ = asset::vab_multi_bank::detect(&buf);
        let _ = asset::battle_data_pack::detect(&buf);
        let _ = asset::battle_data_pack::parse(&buf);
        let _ = asset::stage_geom::scan(&buf);
    }
}

#[test]
fn man_section_never_panics() {
    for buf in adversarial_inputs() {
        if let Ok(man) = asset::man_section::parse(&buf) {
            // If a junk buffer somehow parsed, walking its sections must also
            // be panic-free.
            for s in man.sections {
                if let Some(body) = s.body(&buf) {
                    let _ = asset::man_section::parse_encounter_section(body);
                }
            }
        }
        let _ = asset::man_section::parse_encounter_section(&buf);
        let _ = asset::man_section::FormationRecord::parse(&buf);
        let _ = asset::man_section::RegionRecord::parse(&buf);
    }
}

#[test]
fn monster_archive_never_panics() {
    for buf in adversarial_inputs() {
        let _ = asset::monster_archive::record(&buf, 1);
        let _ = asset::monster_archive::record(&buf, u16::MAX);
        let _ = asset::monster_archive::records(&buf);
    }
}
