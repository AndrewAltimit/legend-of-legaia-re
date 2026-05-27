//! Disc-gated: the inline scene-destination table decoded from a scene MAN's
//! `0x3F` named-scene-change ops.
//!
//! Field-VM opcode `0x3F` is a *named* scene-change: it carries the destination
//! scene name inline in the bytecode (`[i16 index][u8 len][name][entry_x]`
//! `[entry_z][dir]`) and hands it to the scene-change packet `FUN_8001FD44`. A
//! scene's controller script lists every place it can warp to as one such op, so
//! [`scene_destinations`] recovers the destinations straight from disc bytes —
//! the answer the old "map_id -> scene-name table lives in an uncaptured overlay"
//! note assumed was unreachable.
//!
//! This pins the Drake-kingdom overworld (`map01`): its controller lists the
//! towns / dungeons reachable from it (`town01/0b/0c`, `dolk`, `dolk2`,
//! `rikuroa`, `cave01`, `vell`, `vozz`, `suimon`, `keikoku`, `jou`), every
//! recovered name is a real CDNAME scene label, and the desync-phantom guard
//! keeps junk out. Skips when `extracted/PROT/` or `LEGAIA_DISC_BIN` is missing.

use std::path::PathBuf;

use legaia_asset::scene_asset_table;
use legaia_engine_core::man_field_scripts::scene_destinations;

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

fn extracted_root() -> Option<PathBuf> {
    for p in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(p);
        if d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

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
fn map01_controller_lists_its_overworld_destinations() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(prot) = extracted_prot() else {
        eprintln!("[skip] extracted/PROT/ missing — run `legaia-extract` first");
        return;
    };

    let bytes = std::fs::read(prot.join("0086_map01.BIN")).expect("0086_map01.BIN");
    let man = load_man_from_scene(&bytes).expect("MAN for map01");
    let mf = legaia_asset::man_section::parse(&man).expect("parse map01 MAN");

    let dests = scene_destinations(&mf, &man);
    let names: std::collections::BTreeSet<&str> =
        dests.iter().map(|d| d.scene_name.as_str()).collect();
    eprintln!("[map01] {} destinations: {names:?}", dests.len());

    // The Drake overworld reaches these scenes (decoded inline from its
    // controller's 0x3F ops). Pin a stable subset.
    for expected in [
        "town0c", "dolk", "dolk2", "rikuroa", "cave01", "vell", "vozz",
    ] {
        assert!(
            names.contains(expected),
            "map01 destination table is missing {expected:?}; got {names:?}"
        );
    }

    // Every recovered name must be a real CDNAME scene label (the clean-name
    // gate should never surface a string that isn't an actual scene).
    if let Some(root) = extracted_root() {
        let index =
            legaia_engine_core::scene::ProtIndex::open_extracted(&root).expect("prot index");
        let known: std::collections::BTreeSet<String> =
            index.cdname_scene_names().into_iter().collect();
        for d in &dests {
            assert!(
                known.contains(&d.scene_name),
                "recovered destination {:?} (index {}) is not a known CDNAME scene",
                d.scene_name,
                d.index,
            );
        }
    }

    // Indices are stable per destination (no duplicate name with conflicting
    // index slipped through the dedup).
    for d in &dests {
        let same: Vec<i16> = dests
            .iter()
            .filter(|o| o.scene_name == d.scene_name)
            .map(|o| o.index)
            .collect();
        assert!(
            same.windows(2).all(|w| w[0] == w[1]),
            "destination {:?} decoded with conflicting indices {same:?}",
            d.scene_name,
        );
    }
}
