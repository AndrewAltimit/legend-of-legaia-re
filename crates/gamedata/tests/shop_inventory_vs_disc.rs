//! Disc-gated cross-validation of curated shop inventories against the real
//! gold-shop stock records embedded in each scene's MAN
//! (`legaia_asset::shop_stock`, op `0x49` sub-op `0`).
//!
//! A town merchant's stock is defined inline in the scene field-VM script, not
//! in a static table, so it is the authoritative ground truth for the curated
//! `shops.toml` mined from walkthroughs. This scans every PROT entry for shop
//! records, decodes each record's **sellable** item ids to names through the
//! SCUS item table, and joins disc shops to curated shops by the **item-name
//! set** (order-independent - the item set is far more distinctive than the
//! on-screen shop title, which repeats across towns as "Arms Shop" / "Items
//! Shop").
//!
//! Every disc shop's sellable stock matches a curated shop's resolved inventory
//! as an exact set, with one documented disc-only exception: **Soru's Bakery**
//! (Sol / Koin), which sells only the novelty item "Soru Bread" and has no
//! curated shops-table counterpart. The exception is asserted explicitly so it
//! can't go vacuous.
//!
//! This oracle pinned a curated item-name error - the Gala helmet the disc
//! names "Power Earring" (singular) was curated as "Power Earrings", breaking
//! the Wind Cave and Biron-after-mist set joins until corrected to the disc
//! spelling. (The price oracle missed it: a name-mismatch there is a tolerated
//! miss, not a failure.)
//!
//! Skips silently when `extracted/SCUS_942.54` or `extracted/PROT` is missing.

use std::collections::BTreeSet;
use std::path::PathBuf;

use legaia_asset::item_names::{self, ItemNameTable};
use legaia_asset::shop_stock;
use legaia_gamedata::Database;

fn workspace() -> Option<PathBuf> {
    Some(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()?
            .parent()?
            .to_path_buf(),
    )
}

fn norm(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

#[test]
fn curated_shop_inventories_match_the_disc() {
    let Some(ws) = workspace() else { return };
    let Ok(scus) = std::fs::read(ws.join("extracted").join("SCUS_942.54")) else {
        eprintln!("[skip] extracted/SCUS_942.54 missing");
        return;
    };
    let prot_dir = ws.join("extracted").join("PROT");
    let Ok(entries) = std::fs::read_dir(&prot_dir) else {
        eprintln!("[skip] extracted/PROT missing");
        return;
    };

    let names = ItemNameTable::from_scus(&scus).expect("parse item-name table");
    // "id names a sellable item" mask: a > 0 SCUS shop price. Used both to
    // validate shop records and to trim the unsellable template-id padding.
    let mut valid = [false; 256];
    for id in 0u8..=u8::MAX {
        if item_names::item_price(&scus, id).is_some_and(|p| p > 0) {
            valid[id as usize] = true;
        }
    }

    // Scan every PROT entry; dedup disc shops by (name, item-name set) - the
    // same scene MAN is reachable from duplicate PROT entries and a shop op can
    // appear more than once in one MAN.
    let mut paths: Vec<PathBuf> = entries
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().map(|x| x == "BIN").unwrap_or(false))
        .collect();
    paths.sort();

    let mut disc: Vec<(String, BTreeSet<String>)> = Vec::new();
    for p in &paths {
        let Ok(bytes) = std::fs::read(p) else {
            continue;
        };
        let Some(loc) = shop_stock::locate(&bytes, Some(&valid)) else {
            continue;
        };
        let prot = p.file_name().unwrap().to_string_lossy().to_string();
        for r in &loc.records {
            let set: BTreeSet<String> = r
                .id_offsets
                .iter()
                .map(|&o| norm(names.name(loc.decoded[o]).unwrap_or("")))
                .collect();
            let label = format!("{prot} {}", r.name);
            if !disc.iter().any(|(_, s)| *s == set) {
                disc.push((label, set));
            }
        }
    }

    let db = Database::load();
    let curated: Vec<BTreeSet<String>> = db
        .shops()
        .iter()
        .map(|s| {
            db.resolve_inventory(&s.inventory)
                .iter()
                .map(|e| norm(e.name))
                .collect()
        })
        .collect();

    // The one disc-only novelty shop (sells only "Soru Bread").
    let soru: BTreeSet<String> = std::iter::once(norm("Soru Bread")).collect();

    let mut matched = 0usize;
    let mut soru_seen = false;
    let mut unmatched: Vec<String> = Vec::new();
    for (label, set) in &disc {
        if curated.iter().any(|cs| cs == set) {
            matched += 1;
        } else if *set == soru {
            soru_seen = true;
        } else {
            unmatched.push(format!("{label}: {set:?}"));
        }
    }

    assert!(
        disc.len() >= 20,
        "expected 20+ unique disc shops located, got {}",
        disc.len()
    );
    assert!(
        unmatched.is_empty(),
        "disc shop stock with no matching curated inventory (disc is authoritative): {unmatched:#?}"
    );
    assert!(
        soru_seen,
        "Soru's Bakery did not appear; the disc-only exception went vacuous"
    );
    assert!(
        matched >= 20,
        "expected 20+ disc↔curated set matches, got {matched}"
    );
}
