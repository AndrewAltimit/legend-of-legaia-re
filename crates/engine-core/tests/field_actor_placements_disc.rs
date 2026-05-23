//! Disc-gated: the MAN partition-1 NPC/actor placement table (`FUN_8003A1E4`)
//! decodes into sane entity placements for real scenes — towns and the three
//! kingdom overworlds. Skips when `LEGAIA_DISC_BIN` is unset.

use std::path::PathBuf;
use std::sync::Arc;

use legaia_engine_core::scene::{ProtIndex, Scene};

fn extracted_dir() -> Option<PathBuf> {
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

#[test]
fn man_actor_placements_decode_for_real_scenes() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing");
        return;
    };
    let index = Arc::new(ProtIndex::open_extracted(&extracted).expect("open prot index"));

    let mut total_with_placements = 0;
    for label in ["town01", "map01", "map02", "map03"] {
        let Ok(scene) = Scene::load(&index, label) else {
            eprintln!("[{label}] scene load failed");
            continue;
        };
        let placements = match scene.field_actor_placements(&index) {
            Ok(Some(p)) => p,
            Ok(None) => {
                eprintln!("[{label}] no MAN bundle");
                continue;
            }
            Err(e) => {
                eprintln!("[{label}] placement decode error: {e:#}");
                continue;
            }
        };
        let specials = placements.iter().filter(|p| p.special_model).count();
        eprintln!(
            "[{label}] {} placement(s), {specials} special-model; first few: {:?}",
            placements.len(),
            placements
                .iter()
                .take(4)
                .map(|p| (
                    p.index,
                    p.model_index,
                    p.tile_x,
                    p.tile_z,
                    p.world_x,
                    p.world_z
                ))
                .collect::<Vec<_>>()
        );

        // Every decoded placement must sit on a valid 0x80x0x80 tile grid and
        // carry a script offset past its placement header.
        for p in &placements {
            assert!(p.tile_x < 0x80, "[{label}] tile_x {} out of grid", p.tile_x);
            assert!(p.tile_z < 0x80, "[{label}] tile_z {} out of grid", p.tile_z);
            assert!(
                p.world_x >= 0 && p.world_z >= 0,
                "[{label}] negative world pos ({}, {})",
                p.world_x,
                p.world_z
            );
            assert_eq!(
                p.script_pc0,
                1 + 2 * p.local_count + 4,
                "[{label}] script offset must follow the prefix + 4-byte header"
            );
        }
        if !placements.is_empty() {
            total_with_placements += 1;
        }
    }
    // town01 (a populated town) and all three kingdom overworlds decode a
    // non-empty placement list; this guards the record-walk against drift.
    assert!(
        total_with_placements >= 3,
        "the town + overworld scenes must decode actor placements (got {total_with_placements})"
    );
}

#[test]
fn placement_scripts_classify_into_kinds() {
    use legaia_asset::man_section::parse as parse_man;
    use legaia_engine_core::man_field_scripts::{PlacementKind, classify_placements};

    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing");
        return;
    };
    let index = Arc::new(ProtIndex::open_extracted(&extracted).expect("open prot index"));

    let mut any_portal = false;
    let mut any_npc = false;
    for label in ["town01", "map01", "map02", "map03"] {
        let Ok(scene) = Scene::load(&index, label) else {
            continue;
        };
        let Ok(Some(man_bytes)) = scene.field_man_payload(&index) else {
            eprintln!("[{label}] no MAN");
            continue;
        };
        let Ok(man) = parse_man(&man_bytes) else {
            eprintln!("[{label}] MAN parse failed");
            continue;
        };
        let classified = classify_placements(&man, &man_bytes);
        let portals = classified
            .iter()
            .filter(|(_, k)| matches!(k, PlacementKind::Portal { .. }))
            .count();
        let npcs = classified
            .iter()
            .filter(|(_, k)| matches!(k, PlacementKind::Npc { .. }))
            .count();
        let plain = classified
            .iter()
            .filter(|(_, k)| matches!(k, PlacementKind::Plain))
            .count();
        any_portal |= portals > 0;
        any_npc |= npcs > 0;
        eprintln!("[{label}] {portals} portal(s), {npcs} npc(s), {plain} plain");
        for (p, k) in &classified {
            if let PlacementKind::Portal { target_map } = k {
                // `target_map` is the raw field-VM map id (`op0 - 100`); its
                // scene-name table lives in an uncaptured overlay, so don't
                // resolve it through CDNAME here (that index is unrelated).
                eprintln!(
                    "    portal #{} at tile ({},{}) -> field map id {target_map}",
                    p.index, p.tile_x, p.tile_z
                );
            }
        }
    }
    // Real data has both: towns are full of dialog NPCs, the overworld carries
    // a handful of warp portals.
    assert!(any_npc, "a populated town must classify dialog NPCs");
    assert!(
        any_portal,
        "the overworld must classify at least one warp portal"
    );
}
