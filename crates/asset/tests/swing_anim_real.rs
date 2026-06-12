//! Disc-gated verification of the battle party's **weapon-swing animation
//! records** and the per-character **art-animation bank** + `"ME"` stream
//! archives (see `docs/formats/battle-data-pack.md` § Battle animations /
//! § Art-animation bank):
//!
//! - every equippable id's section-2/3/4 payload carries a swing
//!   action-entry at `+0x04` (and a second at `+0x08` for section 4) whose
//!   keyframe stream decodes with a part count matching the character's
//!   skeleton (equipment extras may add up to two channels) and fits the
//!   section footprint; sections 0/1 carry none;
//! - the art bank ( record[0] `+0x58`: `[u32 count]` + `0xD0`-stride
//!   records) parses for all four characters with printable names, valid
//!   combo bytes, and every record's stream resolving through the
//!   character's `readef.DAT` `"ME"` archive (all-compressed; the
//!   `FUN_8002A9CC` channel-delta codec) to a keyframe stream whose part
//!   count equals the skeleton bone count exactly;
//! - the `+0x5C` "sibling pointer" is pinned: it equals `clut_a_off - 4`
//!   (a zero word before record[0]'s first image block) in all four files -
//!   refuting the "it points at the art ME archive" hypothesis (the
//!   archives live in `readef.DAT` slots `3*char + 1` / `3*char + 2`).
//!
//! Skips silently when `LEGAIA_DISC_BIN` is unset.

use std::path::PathBuf;

use legaia_asset::{battle_char_assembly, battle_data_pack, me_archive, summon_readef};

fn extracted_prot_dir() -> Option<PathBuf> {
    let cands = [
        PathBuf::from("extracted/PROT"),
        PathBuf::from("../../extracted/PROT"),
    ];
    cands.into_iter().find(|p| p.is_dir())
}

fn gated_inputs() -> Option<(PathBuf, Vec<u8>)> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return None;
    }
    let Some(prot_dir) = extracted_prot_dir() else {
        eprintln!("[skip] extracted/PROT missing");
        return None;
    };
    let readef = prot_dir.join("0894_card_data.BIN");
    let Ok(readef) = std::fs::read(&readef) else {
        eprintln!("[skip] {} missing", readef.display());
        return None;
    };
    Some((prot_dir, readef))
}

/// (name, extraction file, char index, skeleton bone count).
const PLAYER_FILES: [(&str, &str, usize, usize); 4] = [
    ("Vahn", "0863_edstati3.BIN", 0, 15),
    ("Noa", "0864_edstati3.BIN", 1, 16),
    ("Gala", "0865_battle_data.BIN", 2, 15),
    ("Terra", "0866_battle_data.BIN", 3, 17),
];

fn u32_at(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes(b[o..o + 4].try_into().unwrap())
}

#[test]
fn every_equippable_section_carries_decodable_swing_records() {
    let Some((prot_dir, _)) = gated_inputs() else {
        return;
    };
    for (name, file, _, bones) in PLAYER_FILES {
        let raw = std::fs::read(prot_dir.join(file)).expect("read player file");
        let pack = battle_data_pack::parse(&raw).expect("descriptor table");
        // Walk EVERY descriptor entry, tracking which equipment section it
        // belongs to (id = 0 entries terminate a section).
        let mut section = 0usize;
        let mut swings = 0usize;
        for rec in &pack.records {
            let entry =
                battle_data_pack::decode_record(&raw, &pack, rec.index).expect("decode slot");
            let d = &entry.bytes;
            let footprint = u32_at(d, 0xC) as usize;
            let w04 = u32_at(d, 4) as usize;
            let w08 = u32_at(d, 8) as usize;
            if section < 2 {
                assert_eq!(
                    (w04, w08),
                    (0, 0),
                    "{name} sec{section} id {:#x}: sections 0/1 carry no swing records",
                    rec.id
                );
            } else {
                let mut offsets = vec![w04];
                if section == 4 {
                    offsets.push(w08);
                    assert_ne!(w08, 0, "{name} sec4 id {:#x}: second swing record", rec.id);
                } else {
                    assert_eq!(
                        w08, 0,
                        "{name} sec{section} id {:#x}: +0x08 only used by section 4",
                        rec.id
                    );
                }
                for off in offsets {
                    assert!(
                        off != 0 && off < d.len(),
                        "{name} sec{section} id {:#x}: swing offset {off:#x} in range",
                        rec.id
                    );
                    let parts = d[off + 0xAC] as usize;
                    let frames = d[off + 0xAD] as usize;
                    assert!(
                        (bones..=bones + 2).contains(&parts),
                        "{name} sec{section} id {:#x}: swing parts {parts} vs {bones} bones",
                        rec.id
                    );
                    assert!(
                        (5..=120).contains(&frames),
                        "{name} sec{section} id {:#x}: swing frames {frames}",
                        rec.id
                    );
                    let end = off + 0xAE + parts * frames * 9;
                    assert!(
                        end <= footprint,
                        "{name} sec{section} id {:#x}: stream end {end:#x} inside the \
                         section footprint {footprint:#x}",
                        rec.id
                    );
                    let rate = d[off + 0x78];
                    assert!(
                        (1..=7).contains(&rate),
                        "{name} sec{section} id {:#x}: rate byte {rate}",
                        rec.id
                    );
                    swings += 1;
                }
            }
            if rec.id == 0 {
                section += 1;
            }
        }
        assert_eq!(section, 5, "{name}: five sections walked");
        assert!(swings >= 4, "{name}: at least one swing per slot 0xC..0xF");
        eprintln!("[ok] {name}: {swings} swing records across sections 2..4");
    }
}

#[test]
fn swing_battle_animations_fills_slots_0xc_to_0xf_for_default_equipment() {
    let Some((prot_dir, _)) = gated_inputs() else {
        return;
    };
    for (name, file, _, bones) in PLAYER_FILES {
        let raw = std::fs::read(prot_dir.join(file)).expect("read player file");
        let pack = battle_data_pack::parse(&raw).expect("descriptor table");
        // Default (id = 0) equipment in every slot.
        let swings = battle_char_assembly::swing_battle_animations(&raw, &pack, &[0; 5])
            .expect("swing animations");
        let slots: Vec<u8> = swings.iter().map(|s| s.slot).collect();
        assert_eq!(slots, vec![0xC, 0xD, 0xE, 0xF], "{name}: runtime slots");
        for s in &swings {
            assert_eq!(s.anim.action_id, s.slot, "{name}: action_id = slot");
            assert!(
                (bones..=bones + 2).contains(&s.anim.part_count),
                "{name} slot {:#x}: parts {}",
                s.slot,
                s.anim.part_count
            );
            assert_eq!(
                s.anim.frames.len(),
                s.anim.frame_count,
                "{name} slot {:#x}: frame vector",
                s.slot
            );
        }
        // Section -> slot mapping: 2 -> 0xC, 3 -> 0xD, 4 -> 0xE + 0xF.
        assert_eq!(swings[0].section, 2, "{name}");
        assert_eq!(swings[1].section, 3, "{name}");
        assert_eq!(swings[2].section, 4, "{name}");
        assert_eq!(swings[3].section, 4, "{name}");
    }
}

#[test]
fn art_bank_resolves_every_stream_through_the_readef_me_archives() {
    let Some((prot_dir, readef)) = gated_inputs() else {
        return;
    };
    for (name, file, char_index, bones) in PLAYER_FILES {
        let raw = std::fs::read(prot_dir.join(file)).expect("read player file");
        let record0 = battle_char_assembly::decode_record0(&raw).expect("record[0]");
        let bank = battle_char_assembly::art_animation_bank(&record0).expect("art bank");
        assert!(
            (9..=64).contains(&bank.len()),
            "{name}: bank count {}",
            bank.len()
        );

        let main = battle_char_assembly::art_me_archive(&readef, char_index, false)
            .expect("main ME archive");
        let base = battle_char_assembly::art_me_archive(&readef, char_index, true)
            .expect("base ME archive");
        assert_eq!(base.len(), 8, "{name}: base archive is 8 entries");
        // Every retail art stream is compressed - the FUN_8002A9CC codec is
        // the exercised path (no raw entry ships in the art corpus).
        for (lbl, a) in [("main", &main), ("base", &base)] {
            for n in 0..a.len() {
                assert_eq!(
                    a.is_compressed(n),
                    Some(true),
                    "{name} {lbl} entry {n}: compressed on disc"
                );
            }
        }

        // The exact-cover pin behind `uses_base_archive`: the 0xFF records'
        // sources span the base archive exactly; the rest span the main one.
        let max_src = |base_arch: bool| {
            bank.iter()
                .filter(|r| r.uses_base_archive() == base_arch)
                .map(|r| r.stream_source as usize)
                .max()
                .unwrap()
        };
        assert_eq!(max_src(true), base.len() - 1, "{name}: base exact cover");
        assert_eq!(max_src(false), main.len() - 1, "{name}: main exact cover");
        assert_eq!(
            bank.iter().filter(|r| r.uses_base_archive()).count(),
            8,
            "{name}: eight base records"
        );

        for rec in &bank {
            assert_eq!(
                rec.anim_id,
                0x10 + rec.index as u8,
                "{name} record {}: staged id space",
                rec.index
            );
            // Names are printable ASCII (enforced by the parser); combo
            // bytes are direction commands 1..=4 (likewise enforced).
            assert!(rec.name.len() <= 20, "{name} record {}: name", rec.index);
            let archive = if rec.uses_base_archive() {
                &base
            } else {
                &main
            };
            let anim = battle_char_assembly::art_animation(rec, archive)
                .unwrap_or_else(|e| panic!("{name} record {} ({:?}): {e:#}", rec.index, rec.name));
            assert_eq!(
                anim.part_count, bones,
                "{name} record {} ({:?}): parts == skeleton bones",
                rec.index, rec.name
            );
            assert!(
                (1..=120).contains(&anim.frame_count),
                "{name} record {} ({:?}): frames {}",
                rec.index,
                rec.name,
                anim.frame_count
            );
            // The decoded stream is length-exact: 2 + parts*frames*9.
            let stream = archive.entry(rec.stream_source as usize).expect("stream");
            assert_eq!(
                stream.len(),
                2 + anim.part_count * anim.frame_count * 9,
                "{name} record {}: decoded stream length-exact",
                rec.index
            );
        }
        // The HUD-named band (ids > 0x1A -> records 11+) is present in the
        // three arts-fighting characters (Terra's bank has no named band).
        if bank.len() > 11 {
            assert!(
                bank[11..].iter().any(|r| !r.name.is_empty()),
                "{name}: named art records past index 11"
            );
        }
        eprintln!(
            "[ok] {name}: {} art records ({} named) through ME archives {}+{}",
            bank.len(),
            bank.iter().filter(|r| !r.name.is_empty()).count(),
            main.len(),
            base.len()
        );
    }
}

#[test]
fn record0_word_0x5c_is_the_zero_word_before_the_first_image_block() {
    let Some((prot_dir, _)) = gated_inputs() else {
        return;
    };
    for (name, file, _, _) in PLAYER_FILES {
        let raw = std::fs::read(prot_dir.join(file)).expect("read player file");
        let clut_a = u32_at(&raw, 4) as usize;
        let record0 = battle_char_assembly::decode_record0(&raw).expect("record[0]");
        let w58 = u32_at(&record0, 0x58) as usize;
        let w5c = u32_at(&record0, 0x5C) as usize;
        assert_eq!(w5c, clut_a - 4, "{name}: +0x5C = clut_a_off - 4");
        assert_eq!(u32_at(&record0, w5c), 0, "{name}: the +0x5C word is zero");
        // The art bank ([u32 count] + 0xD0-stride records) sits between
        // +0x58's target and +0x5C's.
        let count = u32_at(&record0, w58) as usize;
        assert!(
            w58 + 4 + count * battle_char_assembly::ART_RECORD_STRIDE <= w5c,
            "{name}: bank extent before the +0x5C word"
        );
    }
}

#[test]
fn readef_art_slots_classify_as_me_archives() {
    let Some((_, readef)) = gated_inputs() else {
        return;
    };
    let parsed = summon_readef::parse(&readef).expect("parse readef");
    for (name, _, char_index, _) in PLAYER_FILES {
        for base in [false, true] {
            let slot = battle_char_assembly::art_me_slot(char_index, base);
            let summon_readef::SlotKind::MeArchive { count, compressed } = parsed.slots[slot].kind
            else {
                panic!("{name}: readef slot {slot} should classify as an ME archive");
            };
            assert_eq!(count, compressed, "{name} slot {slot}: all compressed");
            // Cross-check against the direct parse.
            let direct = me_archive::parse(
                &readef[slot * summon_readef::SLOT_BYTES..(slot + 1) * summon_readef::SLOT_BYTES],
            )
            .expect("direct parse");
            assert_eq!(direct.len(), count, "{name} slot {slot}");
        }
        // The group's first slot (3*char) is NOT an ME archive.
        let tex_slot = char_index * 3;
        assert!(
            !matches!(
                parsed.slots[tex_slot].kind,
                summon_readef::SlotKind::MeArchive { .. }
            ),
            "{name}: slot {tex_slot} is the non-ME slot of the group"
        );
    }
}
