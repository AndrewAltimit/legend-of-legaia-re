//! Disc-gated sweep of [`battle_char_assembly::equip_item`] over **every**
//! weapon / Ra-Seru record on the disc (81 of them), not a sample.
//!
//! The claim under test is that a held item is an exact primitive subset of
//! the bone object, selected by palette column - so the sweep asserts the
//! properties that would break if the cut were a heuristic over geometry:
//! every record separates, no primitive is claimed twice, the limb keeps
//! geometry of its own, and the three classes land where the disc puts them.
//!
//! Skips + passes when `LEGAIA_DISC_BIN` is unset.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use legaia_asset::battle_char_assembly::equip_item::{self, ItemClass};
use legaia_asset::{battle_char_assembly as bca, battle_data_pack};

fn extracted_prot_dir() -> Option<PathBuf> {
    [
        PathBuf::from("extracted/PROT"),
        PathBuf::from("../../extracted/PROT"),
    ]
    .into_iter()
    .find(|p| p.is_dir())
}

/// The equipment ids each of the five sections offers, in table order.
fn section_ids(pack: &battle_data_pack::BattleDataPack) -> Vec<Vec<u32>> {
    let mut out: Vec<Vec<u32>> = vec![Vec::new(); bca::SECTION_COUNT];
    let mut slot = 0usize;
    for r in &pack.records {
        if slot >= bca::SECTION_COUNT {
            break;
        }
        if r.id == 0 {
            slot += 1;
        } else {
            out[slot].push(r.id);
        }
    }
    out
}

fn load(dir: &Path, file: &str) -> Option<(Vec<u8>, battle_data_pack::BattleDataPack)> {
    let path = dir.join(file);
    if !path.exists() {
        eprintln!("[skip] {} missing", path.display());
        return None;
    }
    let raw = std::fs::read(&path).ok()?;
    let pack = battle_data_pack::parse(&raw).ok()?;
    Some((raw, pack))
}

/// Every section-2 / section-3 record on the disc separates into an item and
/// a limb, and the partition is a genuine partition of the object's
/// primitives.
#[test]
fn every_weapon_record_separates() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    }
    let Some(dir) = extracted_prot_dir() else {
        eprintln!("[skip] extracted/PROT missing");
        return;
    };
    let mut total = 0usize;
    let mut by_class: BTreeMap<&str, usize> = BTreeMap::new();
    let mut unseparated: Vec<String> = Vec::new();
    for (file, who) in [
        ("0863_edstati3.BIN", "Vahn"),
        ("0864_edstati3.BIN", "Noa"),
        ("0865_battle_data.BIN", "Gala"),
    ] {
        let Some((raw, pack)) = load(&dir, file) else {
            return;
        };
        let ids = section_ids(&pack);
        let bare = bca::assemble_character(&raw, &pack, &[0; bca::SECTION_COUNT])
            .unwrap_or_else(|e| panic!("{who}: bare assembly: {e:#}"));
        let bare_tmd = legaia_tmd::parse(&bare.tmd).expect("bare TMD");
        for section in equip_item::ITEM_SECTIONS {
            for &id in &ids[section] {
                total += 1;
                let mut load = [0u8; bca::SECTION_COUNT];
                load[section] = id as u8;
                let eq = bca::assemble_character(&raw, &pack, &load)
                    .unwrap_or_else(|e| panic!("{who} {id:#x}: assembly: {e:#}"));
                let eq_tmd = legaia_tmd::parse(&eq.tmd).expect("equipped TMD");
                let Some(p) = equip_item::item_partition(section, &bare, &bare_tmd, &eq, &eq_tmd)
                else {
                    unseparated.push(format!("{who} s{section} {id:#04x}"));
                    continue;
                };
                *by_class.entry(p.class.tag()).or_default() += 1;

                assert!(
                    p.item_primitives > 0,
                    "{who} {id:#x}: an empty item is not a separation"
                );
                // Each object appears once; a `whole_object` part claims
                // everything and must not also list columns.
                let mut seen: BTreeSet<usize> = BTreeSet::new();
                for part in &p.parts {
                    assert!(
                        seen.insert(part.object),
                        "{who} {id:#x}: object {} claimed twice",
                        part.object
                    );
                    assert!(
                        !part.whole_object || part.columns.is_empty(),
                        "{who} {id:#x}: whole-object part also names columns"
                    );
                    assert!(
                        part.whole_object || !part.columns.is_empty(),
                        "{who} {id:#x}: column part claims nothing"
                    );
                }
                // A cut that took the whole object is not a cut. (Except for
                // class A, where retail itself shipped the item standalone.)
                if p.class != ItemClass::OwnObject {
                    assert!(
                        p.limb_primitives > 0,
                        "{who} {id:#x}: the cut swallowed the limb"
                    );
                }
                // The seam is the class: zero shared vertices means the item
                // came away whole.
                match p.class {
                    ItemClass::WeldedSubset => assert!(p.seam_vertices > 0),
                    _ => assert_eq!(p.seam_vertices, 0, "{who} {id:#x}"),
                }
                assert_eq!(
                    p.class.is_complete(),
                    p.seam_vertices == 0,
                    "{who} {id:#x}: completeness must follow the seam"
                );
            }
        }
    }
    assert_eq!(total, 81, "weapon + Ra-Seru records on the disc");
    // The one record with nothing to cut: Noa's first Ra-Seru armband draws
    // its whole object from a single palette column, so there is no material
    // boundary to separate on. Asserted exact so a change is visible.
    assert_eq!(
        unseparated,
        vec!["Noa s2 0x0a".to_string()],
        "records with no separable item"
    );
    // All three classes occur, and none dominates by accident.
    for class in ["own-object", "separate", "welded"] {
        assert!(
            by_class.get(class).copied().unwrap_or(0) > 0,
            "class {class} never occurred: {by_class:?}"
        );
    }
    assert_eq!(by_class.values().sum::<usize>(), 80, "{by_class:?}");
}

/// Armour sections are refused rather than answered. There is no
/// body-without-armour to subtract: sections 0 / 1 / 4 carry no surplus
/// object at all, and their palette buckets split body from trim.
#[test]
fn armour_sections_never_yield_an_item() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    }
    let Some(dir) = extracted_prot_dir() else {
        eprintln!("[skip] extracted/PROT missing");
        return;
    };
    let mut checked = 0usize;
    for file in [
        "0863_edstati3.BIN",
        "0864_edstati3.BIN",
        "0865_battle_data.BIN",
    ] {
        let Some((raw, pack)) = load(&dir, file) else {
            return;
        };
        let ids = section_ids(&pack);
        let bare = bca::assemble_character(&raw, &pack, &[0; bca::SECTION_COUNT]).unwrap();
        let bare_tmd = legaia_tmd::parse(&bare.tmd).unwrap();
        for section in [0usize, 1, 4] {
            for &id in &ids[section] {
                checked += 1;
                let mut load = [0u8; bca::SECTION_COUNT];
                load[section] = id as u8;
                let eq = bca::assemble_character(&raw, &pack, &load).unwrap();
                let eq_tmd = legaia_tmd::parse(&eq.tmd).unwrap();
                assert!(
                    equip_item::item_partition(section, &bare, &bare_tmd, &eq, &eq_tmd).is_none(),
                    "section {section} id {id:#x} offered an item"
                );
            }
        }
    }
    assert_eq!(checked, 51, "body / head / feet records swept");
}

/// The `200+` surplus is usually a byte-copy of its attach bone - but not
/// always, and a viewer that assumes it always is drops real geometry.
#[test]
fn the_duplicate_surplus_is_measured_not_assumed() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    }
    let Some(dir) = extracted_prot_dir() else {
        eprintln!("[skip] extracted/PROT missing");
        return;
    };
    let mut copies = 0usize;
    let mut originals = 0usize;
    for file in [
        "0863_edstati3.BIN",
        "0864_edstati3.BIN",
        "0865_battle_data.BIN",
        "0866_battle_data.BIN",
    ] {
        let Some((raw, pack)) = load(&dir, file) else {
            return;
        };
        let ids = section_ids(&pack);
        let mut loads = vec![[0u8; bca::SECTION_COUNT]];
        for (s, section) in ids.iter().enumerate() {
            for &id in section {
                let mut l = [0u8; bca::SECTION_COUNT];
                l[s] = id as u8;
                loads.push(l);
            }
        }
        for load in loads {
            let asm = bca::assemble_character(&raw, &pack, &load).unwrap();
            let tmd = legaia_tmd::parse(&asm.tmd).unwrap();
            let dup = asm.duplicate_objects(&tmd);
            assert_eq!(dup.len(), asm.bone_tags.len());
            for (i, &tag) in asm.bone_tags.iter().enumerate() {
                if tag < 200 {
                    assert!(!dup[i], "a skeleton bone was called a duplicate");
                    continue;
                }
                if dup[i] { copies += 1 } else { originals += 1 }
            }
        }
    }
    assert!(copies > 0, "no `200+` surplus was a copy");
    assert!(
        originals > 0,
        "every `200+` surplus was a copy - then the tag would be enough, \
         and the measurement this test exists for would be dead weight"
    );
}
