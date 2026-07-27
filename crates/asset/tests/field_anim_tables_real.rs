//! Disc-gated: the two per-scene animation tables the field scenes carry in
//! their bundles.
//!
//! 1. **Walker tables** (type-6 slot, `legaia_asset::clut_walk`): the
//!    CLUT-cell `MoveImage` walker table is not kingdom-only - nine field
//!    scenes ship one too (their water / waterfall shimmer). This pins the
//!    full carrier set and the field tables' shapes.
//! 2. **VDF packs** (type-7 slot, `legaia_asset::scene_vdf`): the
//!    vertex-morph delta packs. Pins jou's 17-sub-entry pack and asserts
//!    every populated pack in the corpus parses with well-formed morph
//!    record streams.
//!
//! Skips and passes when `LEGAIA_DISC_BIN` / `extracted/PROT` are absent.

use legaia_asset::{clut_walk, scene_vdf};
use std::path::PathBuf;

fn prot_dir() -> Option<PathBuf> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    let ws = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()?
        .parent()
        .map(PathBuf::from)?;
    let prot = ws.join("extracted").join("PROT");
    if !prot.is_dir() {
        eprintln!("[skip] extracted/PROT not present");
        return None;
    }
    Some(prot)
}

/// Extraction files by 4-digit index prefix (labels are CDNAME hints, not
/// part of the coordinate).
fn bin_for(prot: &std::path::Path, entry: u32) -> Option<PathBuf> {
    let prefix = format!("{entry:04}_");
    std::fs::read_dir(prot).ok()?.flatten().find_map(|e| {
        let p = e.path();
        let name = p.file_name()?.to_str()?;
        (name.starts_with(&prefix) && name.ends_with(".BIN")).then_some(p)
    })
}

/// The nine field-scene walker-table carriers (extraction indices of the
/// scene bundles) with their expected `(entry_count, dest_cells)`.
/// `garmel` / `dohaty` share a single-entry table; the other seven share a
/// two-entry waterfall table.
const FIELD_WALKER_CARRIERS: [(u32, &[(u16, u16)]); 9] = [
    (95, &[(80, 506)]),             // garmel
    (166, &[(0, 505), (160, 506)]), // geremi
    (200, &[(0, 505), (160, 506)]), // rayman
    (218, &[(80, 506)]),            // dohaty
    (273, &[(0, 505), (160, 506)]), // tunnelb
    (310, &[(0, 505), (160, 506)]), // tunnelc
    (329, &[(0, 505), (160, 506)]), // rayman2
    (355, &[(0, 505), (160, 506)]), // son
    (821, &[(0, 505), (160, 506)]), // edson
];

/// jou's scene bundle (raw define 630 + 3 = raw 633 = extraction 631).
const JOU_BUNDLE: u32 = 631;

#[test]
fn field_walker_tables_parse_with_pinned_dest_cells_or_skip() {
    let Some(prot) = prot_dir() else { return };
    for (entry, cells) in FIELD_WALKER_CARRIERS {
        let path = bin_for(&prot, entry).unwrap_or_else(|| panic!("entry {entry} missing"));
        let buf = std::fs::read(&path).expect("read bundle");
        let table = clut_walk::from_scene_bundle(&buf)
            .unwrap_or_else(|e| panic!("entry {entry}: walker table: {e}"));
        assert_eq!(table.entries.len(), cells.len(), "entry {entry} count");
        for (k, e) in table.entries.iter().enumerate() {
            assert_eq!(e.kind, 1, "entry {entry} walker {k} kind");
            assert_eq!(
                (e.dest_x, e.dest_y),
                cells[k],
                "entry {entry} walker {k} dest"
            );
            assert!(!e.frames.is_empty(), "entry {entry} walker {k} frames");
            for f in &e.frames {
                assert!(f.hold_vsyncs > 0, "entry {entry} walker {k} hold");
                assert!(
                    (448..512).contains(&f.src_y),
                    "entry {entry} walker {k} src row {}",
                    f.src_y
                );
            }
        }
    }
    // The kingdom bundles resolve through the same by-type path.
    for entry in legaia_asset::kingdom_bundle::BUNDLE_ENTRIES {
        let path = bin_for(&prot, entry).expect("kingdom bundle");
        let buf = std::fs::read(&path).expect("read kingdom bundle");
        let table = clut_walk::from_scene_bundle(&buf).expect("kingdom walker table");
        assert_eq!(table.entries.len(), 8, "kingdom {entry} entry count");
    }
    // jou's slot is one of the 4-byte placeholders: no walker table.
    let jou = std::fs::read(bin_for(&prot, JOU_BUNDLE).expect("jou bundle")).unwrap();
    assert!(
        clut_walk::from_scene_bundle(&jou).is_err(),
        "jou type-6 slot is a placeholder"
    );
}

/// Walk one VDF sub-entry's morph records; panics on a malformed stream.
/// Returns `(record_count, delta_total)`.
fn walk_morph_records(entry: &[u8], label: &str) -> (usize, usize) {
    assert!(entry.len() >= 4, "{label}: truncated sub-entry");
    let count = u32::from_le_bytes(entry[0..4].try_into().unwrap()) as usize;
    let mut off = 4usize;
    let mut deltas = 0usize;
    for r in 0..count {
        assert!(off + 12 <= entry.len(), "{label}: record {r} header");
        let delta_count = u32::from_le_bytes(entry[off + 8..off + 12].try_into().unwrap()) as usize;
        off += 12 + delta_count * 8;
        assert!(off <= entry.len(), "{label}: record {r} deltas");
        deltas += delta_count;
    }
    (count, deltas)
}

#[test]
fn scene_vdf_packs_parse_across_the_corpus_or_skip() {
    let Some(prot) = prot_dir() else { return };
    // jou: the flagship animated scene - 17 sub-entries of vertex deltas.
    let jou = std::fs::read(bin_for(&prot, JOU_BUNDLE).expect("jou bundle")).unwrap();
    let vdf = scene_vdf::from_scene_bundle(&jou)
        .expect("jou has a type-7 slot")
        .expect("jou VDF parses");
    assert_eq!(vdf.len(), 17, "jou VDF sub-entry count");
    let mut total_deltas = 0usize;
    for i in 0..vdf.len() {
        let sub = vdf.sub_entry(i).expect("sub-entry bytes");
        let (records, deltas) = walk_morph_records(sub, &format!("jou sub {i}"));
        assert!(records > 0, "jou sub {i} has records");
        total_deltas += deltas;
    }
    assert!(total_deltas > 100, "jou VDF carries real delta payloads");

    // Corpus: every populated type-7 slot parses as a VDF pack whose
    // sub-entries walk cleanly as morph-record streams.
    let mut populated = 0usize;
    for e in std::fs::read_dir(&prot).expect("read PROT dir").flatten() {
        let p = e.path();
        if p.extension().and_then(|x| x.to_str()) != Some("BIN") {
            continue;
        }
        let Ok(buf) = std::fs::read(&p) else { continue };
        let Some(res) = scene_vdf::from_scene_bundle(&buf) else {
            continue;
        };
        let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("?");
        match res {
            Ok(vdf) => {
                populated += 1;
                for i in 0..vdf.len() {
                    let sub = vdf.sub_entry(i).expect("sub-entry bytes");
                    walk_morph_records(sub, &format!("{name} sub {i}"));
                }
            }
            // 4-byte placeholders (`count == 0`) and the odd slot whose
            // stream a wider container owns are fine; a populated pack
            // failing structural parse is not.
            Err(err) => assert!(
                err.contains("count 0")
                    || err.contains("implausible")
                    || err.contains("lzs")
                    || err.contains("size 0"),
                "{name}: unexpected VDF parse failure: {err}"
            ),
        }
    }
    assert!(
        populated >= 50,
        "expected the populated-VDF family (61 bundles), found {populated}"
    );
}
