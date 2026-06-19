//! Disc-gated: the player-summon → creature map
//! ([`legaia_asset::summon_creatures::SUMMON_CREATURES`]) is byte-validated
//! against the disc by mesh identity.
//!
//! For every base + evolved-Seru summon `0x81..=0x95`, the `summon.dat` group's
//! actor-record Legaia TMD is **byte-identical** to the mapped `battle_data`
//! creature's mesh (PROT 867), and the archive record carries the mapped name.
//! This pins the whole map - including the two evolved legs (`0x90` Kemaro,
//! `0x91` Spoon) that no mid-cast capture state covers - from disc bytes alone.
//!
//! The high block `0x99..=0xA0` is asserted to NOT byte-match any archive record
//! (only a short header prefix overlaps): those summons carry a bespoke mesh in
//! the group's raw part-pool slot, not a reused enemy body.
//!
//! Skips + passes when `LEGAIA_DISC_BIN` / `extracted/` is absent.

use std::path::{Path, PathBuf};

use legaia_asset::monster_archive;
use legaia_asset::summon_creatures::{SUMMON_CREATURES, actor_record_slot_index};
use legaia_asset::summon_readef::{self, SlotKind};

const SLOT_BYTES: usize = 0x10800;

fn root() -> Option<PathBuf> {
    std::env::var_os("LEGAIA_DISC_BIN")?;
    for p in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(p);
        if d.join("PROT.DAT").is_file() {
            return Some(d);
        }
    }
    None
}

fn entry_bytes(root: &Path, prefix: &str) -> Option<Vec<u8>> {
    let dir = root.join("PROT");
    let name = std::fs::read_dir(&dir)
        .ok()?
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .find(|n| n.starts_with(prefix))?;
    std::fs::read(dir.join(name)).ok()
}

fn lcp(a: &[u8], b: &[u8]) -> usize {
    a.iter().zip(b).take_while(|(x, y)| x == y).count()
}

/// The Legaia TMD of a summon spell's actor-record slot in `summon.dat`.
fn summon_creature_tmd(summon: &[u8], parsed: &summon_readef::SidebandFile, spell: u8) -> Vec<u8> {
    let rs = actor_record_slot_index(spell);
    let slot = parsed
        .slots
        .iter()
        .find(|s| s.index == rs)
        .unwrap_or_else(|| panic!("0x{spell:02X}: actor-record slot {rs} not parsed"));
    let SlotKind::ActorRecord(ar) = &slot.kind else {
        panic!(
            "0x{spell:02X}: slot {rs} is not an actor record ({:?})",
            slot.kind
        );
    };
    let slot_bytes = &summon[rs * SLOT_BYTES..(rs + 1) * SLOT_BYTES];
    slot_bytes[ar.tmd_offset..].to_vec()
}

#[test]
fn summon_creatures_byte_match_their_archive_mesh() {
    let Some(root) = root() else {
        eprintln!("[skip] LEGAIA_DISC_BIN / extracted absent");
        return;
    };
    let summon = entry_bytes(&root, "0893_").expect("summon.dat entry 893");
    let battle = entry_bytes(&root, "0867_").expect("battle_data entry 867");
    let parsed = summon_readef::parse(&summon).expect("parse summon.dat");

    for c in SUMMON_CREATURES {
        let rec_tmd = summon_creature_tmd(&summon, &parsed, c.spell_id);
        let mesh = monster_archive::mesh(&battle, c.creature_id)
            .ok()
            .flatten()
            .unwrap_or_else(|| {
                panic!(
                    "0x{:02X}: no archive mesh for id {}",
                    c.spell_id, c.creature_id
                )
            });
        let archive_tmd = mesh.tmd_bytes();

        // Both buffers carry trailing (non-TMD) data, so compare over the full
        // parsed TMD length: the whole creature mesh must be byte-identical.
        let tmd = legaia_tmd::parse(&rec_tmd).expect("parse summon.dat creature TMD");
        let tmd_len = tmd.stats().total_bytes_consumed;
        let n = lcp(&rec_tmd, archive_tmd);
        assert!(
            tmd_len > 0x400 && n >= tmd_len,
            "0x{:02X} {}: summon.dat TMD not byte-identical to archive id {} \
             (lcp {n} < TMD len {tmd_len})",
            c.spell_id,
            c.name,
            c.creature_id,
        );
        // ... and the archive record's mesh parses to the same object count.
        let archive_objs = legaia_tmd::parse(archive_tmd)
            .map(|t| t.objects.len())
            .unwrap_or(0);
        assert_eq!(
            tmd.objects.len(),
            archive_objs,
            "0x{:02X} {}: object-count mismatch vs archive id {}",
            c.spell_id,
            c.name,
            c.creature_id,
        );

        // The archive record carries the mapped name.
        let rec = monster_archive::record(&battle, c.creature_id)
            .ok()
            .flatten()
            .expect("archive record");
        assert_eq!(
            rec.name, c.name,
            "0x{:02X}: archive id {} is named {:?}, expected {:?}",
            c.spell_id, c.creature_id, rec.name, c.name,
        );

        eprintln!(
            "0x{:02X} {:10} -> battle_data id {:3} {:14} (TMD byte-identical, {} bytes)",
            c.spell_id, c.name, c.creature_id, rec.name, tmd_len,
        );
    }
}

#[test]
fn high_block_summons_are_bespoke_not_archive_meshes() {
    let Some(root) = root() else {
        eprintln!("[skip] LEGAIA_DISC_BIN / extracted absent");
        return;
    };
    let summon = entry_bytes(&root, "0893_").expect("summon.dat entry 893");
    let battle = entry_bytes(&root, "0867_").expect("battle_data entry 867");
    let parsed = summon_readef::parse(&summon).expect("parse summon.dat");

    // Pre-collect every archive mesh once.
    let slots = monster_archive::slot_count(&battle) as u16;
    let archive: Vec<Vec<u8>> = (1..=slots)
        .filter_map(|id| monster_archive::mesh(&battle, id).ok().flatten())
        .map(|m| m.tmd_bytes().to_vec())
        .collect();

    for spell in 0x99u8..=0xA0 {
        let rec_tmd = summon_creature_tmd(&summon, &parsed, spell);
        let best = archive.iter().map(|t| lcp(&rec_tmd, t)).max().unwrap_or(0);
        // No archive mesh shares more than a short header prefix: the summon
        // creature is a bespoke mesh, not a reused enemy body.
        assert!(
            best < 0x100,
            "0x{spell:02X}: a high-block summon unexpectedly byte-matches an \
             archive mesh ({best}-byte prefix) - revisit the bespoke-mesh reading",
        );
        eprintln!("0x{spell:02X}: bespoke summon mesh (best archive prefix {best} bytes)");
    }
}
