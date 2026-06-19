//! Disc-gated: every MAN partition-1 actor the placement classifier marks a
//! [`PlacementKind::Portal`] across the whole PROT corpus carries a **valid**
//! door-warp map id (`0..=6`), and the entries that used to emit phantom portals
//! no longer do.
//!
//! Background: [`classify_placements`] walks each actor's interaction script
//! with an over-approximating linear disassembly that desyncs inside embedded
//! message / SJIS text. Before the genuine-warp gate, a desynced read could land
//! on a `0x3E` whose next byte happened to be `>= 100` and report a phantom
//! `scene_transition` - e.g. `geremi` (`op0=200`) and the leftover-JP `other7`
//! (`op0=175/179`), both riding the `0x80` cross-context prefix, classified as
//! portals to non-existent maps 75 / 79 / 86 / 100. The overworld portal-spawn
//! path consumes these, so a phantom became a phantom on-map portal.
//!
//! This pins the corpus invariant: a Portal's `target_map` is always in the
//! 7-id door-warp range, and `geremi` / `other7` carry zero portals. Skips when
//! `extracted/PROT/` or `LEGAIA_DISC_BIN` is missing.

use std::path::PathBuf;

use legaia_asset::scene_asset_table;
use legaia_engine_core::man_field_scripts::{PlacementKind, classify_placements};

fn extracted_prot() -> Option<PathBuf> {
    for p in [
        "extracted/PROT",
        "../extracted/PROT",
        "../../extracted/PROT",
    ] {
        let d = PathBuf::from(p);
        if d.is_dir() {
            return Some(d);
        }
    }
    None
}

/// Extract + LZS-decode the type-`0x03` MAN payload from a scene bundle.
fn load_man_from_scene(bytes: &[u8]) -> Option<Vec<u8>> {
    let table = scene_asset_table::detect(bytes)?;
    let man = table
        .descriptors
        .iter()
        .find(|d| d.type_byte == 0x03)
        .copied()?;
    let start = man.data_offset as usize;
    if start >= bytes.len() {
        return None;
    }
    let (decoded, _) = legaia_lzs::decompress_tracked(&bytes[start..], man.size as usize).ok()?;
    (decoded.len() == man.size as usize).then_some(decoded)
}

#[test]
fn classified_portals_carry_valid_map_ids_across_the_corpus() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(prot) = extracted_prot() else {
        eprintln!("[skip] extracted/PROT/ missing - run `legaia-extract` first");
        return;
    };

    let mut files: Vec<PathBuf> = std::fs::read_dir(&prot)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("BIN"))
        .collect();
    files.sort();

    let mut portals = 0usize;
    let mut scenes_scanned = 0usize;
    // Entries that produced phantom portals before the genuine-warp gate.
    let mut geremi_portals = 0usize;
    let mut other7_portals = 0usize;

    for path in &files {
        let Ok(bytes) = std::fs::read(path) else {
            continue;
        };
        let Some(man) = load_man_from_scene(&bytes) else {
            continue;
        };
        let Ok(mf) = legaia_asset::man_section::parse(&man) else {
            continue;
        };
        scenes_scanned += 1;
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("?");

        for (i, (_, kind)) in classify_placements(&mf, &man).into_iter().enumerate() {
            if let PlacementKind::Portal { target_map } = kind {
                portals += 1;
                assert!(
                    target_map <= 6,
                    "{stem} placement[{i}]: portal target_map={target_map} is outside the \
                     7-id door-warp range (0..=6) - a text-desync phantom slipped the gate"
                );
                if stem.contains("geremi") {
                    geremi_portals += 1;
                }
                if stem.contains("other7") {
                    other7_portals += 1;
                }
            }
        }
    }

    assert!(scenes_scanned > 0, "no MAN-bearing scene bundles scanned");
    assert!(
        portals > 0,
        "expected at least one genuine door-warp portal across the corpus"
    );
    assert_eq!(
        geremi_portals, 0,
        "geremi's extended-prefix op0=200 pseudo-warp must no longer classify as a portal"
    );
    assert_eq!(
        other7_portals, 0,
        "other7's SJIS-text pseudo-warps (op0=175/179) must no longer classify as portals"
    );

    eprintln!(
        "[ok] scanned {scenes_scanned} MAN scenes; {portals} genuine portals, all map_id 0..=6; \
         geremi/other7 phantoms eliminated"
    );
}
