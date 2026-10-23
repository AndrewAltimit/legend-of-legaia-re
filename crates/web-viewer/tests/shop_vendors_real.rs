//! Every shop town's gold shops resolve to the placed actor that opens them
//! ([`legaia_web_viewer::shop_vendors`]): the record holding the op-`0x49`
//! shop site is a partition-1 placement, and that placement is in the NPC
//! catalog the shops page draws the vendor model from.
//!
//! Skipped (passes) when `LEGAIA_DISC_BIN` is unset.

#![cfg(not(target_arch = "wasm32"))]

use legaia_engine_scene::scene::ProtIndex;
use legaia_iso::image::{extract_cdname_txt, extract_prot_dat};
use legaia_web_viewer::field_npc::build_npc_catalog;
use legaia_web_viewer::field_scene::build_field_scene;
use legaia_web_viewer::shop_vendors::scene_shop_vendors;
use std::env;
use std::fs;

/// Every scene the disc-wide `asset shop-stock` scan finds a gold shop in
/// (`site/_gen.py`'s `TOWN_SHOP_SCENES`).
const SCENES: &[&str] = &[
    "town0b", "town0c", "town0d", "izumi", "bylon", "jiji", "retock", "geremi", "balden", "conc",
    "conc3", "ropeway", "rayman2", "son", "bubu1", "koin2", "koin4", "station3",
];

#[test]
fn shop_sites_resolve_to_catalogued_vendors() {
    let Some(disc_path) = env::var_os("LEGAIA_DISC_BIN") else {
        eprintln!("LEGAIA_DISC_BIN unset; skipping shop-vendor test");
        return;
    };
    let disc = fs::read(&disc_path).expect("disc image");
    let prot = extract_prot_dat(&disc).expect("PROT.DAT extraction");
    let cdname = extract_cdname_txt(&disc).expect("CDNAME.TXT extraction");
    let index = ProtIndex::from_bytes(prot, Some(&cdname)).expect("ProtIndex");

    let (mut total, mut tied) = (0usize, 0usize);
    let (mut placeable, mut seated) = (0usize, 0usize);
    for &name in SCENES {
        let pack = build_field_scene(&index, name).unwrap_or_else(|e| panic!("{name}: {e}"));
        let npcs = build_npc_catalog(&index, name, &pack).unwrap_or_else(|e| panic!("{name}: {e}"));
        let vendors =
            scene_shop_vendors(&index, name, None).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert!(!vendors.is_empty(), "{name}: no shop records");
        for v in &vendors {
            total += 1;
            let npc = match v.owner {
                Some((1, slot)) if slot > 0 => {
                    npcs.entries.iter().position(|e| e.placement.index == slot)
                }
                _ => None,
            };
            if npc.is_some() {
                tied += 1;
            }
            // Every shop a placed actor opens has a station on the map: the
            // seat the shops page marks it at (`vendor_seat`).
            if matches!(v.owner, Some((1, slot)) if slot > 0) {
                placeable += 1;
                if let Some((x, z)) = v.seat {
                    seated += 1;
                    assert!(
                        (0..16320).contains(&x) && (0..16320).contains(&z),
                        "{name} {:?}: seat ({x}, {z}) off the grid",
                        v.name
                    );
                }
            }
            eprintln!(
                "{name}: {:?} owner {:?} npc {:?} model {:?} stock {}",
                v.name,
                v.owner,
                npc,
                npc.map(|i| (
                    npcs.entries[i].placement.model_index,
                    npcs.entries[i].nobj,
                    npcs.entries[i].placement.anim_id
                )),
                v.stock.len()
            );
        }
    }
    eprintln!("{tied}/{total} shops tied to a catalogued actor");
    eprintln!("{seated}/{placeable} placed-actor shops have a seat");
    assert_eq!(seated, placeable, "a placed-actor shop has no seat");
    assert!(
        tied * 10 >= total * 8,
        "only {tied}/{total} shops tie to an actor"
    );
}
