//! Disc-gated: the engine builds **gold-shop stock from the disc** - per-scene
//! item lists decoded from the scene MAN ([`legaia_asset::shop_stock`]) and
//! priced from the SCUS item table ([`legaia_engine_core::shop_catalog`]).
//!
//! Anchored on the Rim Elm "Variety Store", whose 10 ids are pinned from a live
//! capture (shared ground truth with the randomizer's `shop_patch_real` test).
//! Also drives the live wiring: entering the scene that holds the shop populates
//! [`World::scene_shops`] with the priced inventory. Skips without
//! `LEGAIA_DISC_BIN` (CLAUDE.md convention).

use legaia_engine_core::Vfs;
use legaia_engine_core::scene::SceneHost;
use legaia_engine_core::shop_catalog::{self, ShopItemData};
use std::path::PathBuf;

/// Rim Elm Variety Store stock (order-independent), pinned from a live capture.
const VARIETY_STORE_ITEMS: &[u8] = &[0x22, 0x34, 0x59, 0xd6, 0x77, 0x7e, 0x88, 0x43, 0xc7, 0xc8];

fn disc_path() -> Option<PathBuf> {
    let p = PathBuf::from(std::env::var_os("LEGAIA_DISC_BIN")?);
    p.is_file().then_some(p)
}

fn read_scus(path: &std::path::Path) -> Option<Vec<u8>> {
    legaia_engine_core::DiscVfs::open(path)
        .ok()?
        .read("SCUS_942.54")
        .ok()
}

#[test]
fn gold_shops_decode_from_disc_with_real_prices() {
    let Some(path) = disc_path() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let host = SceneHost::open_disc(&path).expect("open disc");
    let scus = read_scus(&path).expect("SCUS_942.54 present on disc");
    let data = ShopItemData::from_scus(&scus).expect("SCUS item table parses");

    // Sweep every PROT entry for gold shops (the engine's per-scene
    // `scene_shops` builder over the whole corpus).
    let mut all = Vec::new();
    for idx in 0..host.index.entry_count() as u32 {
        if let Ok(bytes) = host.index.entry_bytes_extended(idx) {
            for shop in shop_catalog::scene_shops(&bytes, idx as usize, Some(&data)) {
                all.push((idx, shop));
            }
        }
    }
    assert!(!all.is_empty(), "the disc has gold shops");

    // Every located shop names real items at a real (non-zero) buy price - a
    // gold shop never stocks a quest / found-only (price-0) item.
    for (_, shop) in &all {
        assert!(shop.name.len() >= 2, "shop name too short: {:?}", shop.name);
        assert!(
            !shop.inventory.items.is_empty(),
            "shop {:?} sells nothing",
            shop.name
        );
        for item in &shop.inventory.items {
            assert!(
                item.price > 0,
                "shop {:?} item {:#04x} priced 0 (would be free)",
                shop.name,
                item.item_id
            );
        }
    }

    // The Rim Elm Variety Store is present with its pinned 10-item stock.
    let (variety_idx, variety) = all
        .iter()
        .find(|(_, s)| s.name.starts_with("Variety"))
        .expect("Rim Elm Variety Store present");
    let mut got: Vec<u8> = variety.inventory.items.iter().map(|i| i.item_id).collect();
    got.sort_unstable();
    let mut want = VARIETY_STORE_ITEMS.to_vec();
    want.sort_unstable();
    assert_eq!(got, want, "Variety Store sells its known 10 items");

    // Live wiring: entering the scene that holds the shop populates
    // World::scene_shops with the same priced inventory.
    let label = host
        .index
        .scene_for_index(*variety_idx)
        .expect("Variety Store entry resolves to a CDNAME scene")
        .to_string();
    let mut host = host;
    host.world.item_shop_data = Some(data);
    host.enter_field_scene(&label, 0)
        .unwrap_or_else(|e| panic!("enter field scene '{label}': {e:#}"));
    let live = host
        .world
        .scene_shops
        .iter()
        .find(|s| s.name.starts_with("Variety"))
        .expect("entering the scene populated World::scene_shops with the Variety Store");
    let mut live_ids: Vec<u8> = live.inventory.items.iter().map(|i| i.item_id).collect();
    live_ids.sort_unstable();
    assert_eq!(live_ids, want, "live scene_shops match the swept stock");
    assert!(
        live.inventory.items.iter().all(|i| i.price > 0),
        "live shop is priced"
    );
}
