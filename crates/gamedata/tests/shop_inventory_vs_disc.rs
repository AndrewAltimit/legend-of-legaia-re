//! Disc-gated cross-validation of curated shop inventories against the real
//! gold-shop stock records embedded in each scene's MAN
//! (`legaia_scene_formats::shop_stock`, op `0x49` sub-op `0`).
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
//! The scan covers both homes a stock record has: the LZS-packed MAN of a
//! scene bundle and the raw field-script carrier entries (Drake Castle's
//! shops in `dolk2`, Ashif at Usha). Every curated shop must match a disc
//! record (except [`NOT_ON_DISC`]), and in **on-screen order**: the buy list
//! builder `FUN_80030628` (case `0x0B`) hoists the record's last three slots -
//! the Platinum Card stock - to the top, which is the walkthrough's order, and
//! those hoisted rows are exactly the curated `featured` list.
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

use legaia_game_tables::item_names::{self, ItemNameTable};
use legaia_gamedata::Database;
use legaia_scene_formats::shop_stock;

/// A curated shop no op-`0x49` stock record carries: Karisto Station's
/// unmanned "Honest Person's Store".
const NOT_ON_DISC: &[&str] = &["Honest Person's Store"];

/// The tail slots the buy list hoists (`shop_buy_row_order` in
/// `engine-menus::menu_list_rows`).
const SHOP_TAIL_ROWS: usize = 3;

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

/// One disc stock record: where it is, its declared count, and its sellable
/// item names in record order.
struct DiscShop {
    label: String,
    count: usize,
    items: Vec<String>,
}

impl DiscShop {
    fn set(&self) -> BTreeSet<String> {
        self.items.iter().cloned().collect()
    }

    /// The buy list's on-screen order: the rows at or past
    /// `count - SHOP_TAIL_ROWS` first, then the rest (`FUN_80030628`).
    fn screen_order(&self) -> (Vec<String>, Vec<String>) {
        let split = self.count.saturating_sub(SHOP_TAIL_ROWS);
        let tail: Vec<String> = self.items.iter().skip(split).cloned().collect();
        let mut order = tail.clone();
        order.extend(self.items.iter().take(split).cloned());
        (order, tail)
    }
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

    let mut paths: Vec<PathBuf> = entries
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().map(|x| x == "BIN").unwrap_or(false))
        .collect();
    paths.sort();

    // Disc shops, deduplicated by (count, ordered stock): the same scene is
    // reachable from duplicate PROT entries and a shop op can repeat in one
    // script.
    let mut disc: Vec<DiscShop> = Vec::new();
    for p in &paths {
        let Ok(bytes) = std::fs::read(p) else {
            continue;
        };
        // A scene bundle's packed MAN, else the entry itself as a raw script.
        let (buf, records) = match shop_stock::locate(&bytes, Some(&valid)) {
            Some(loc) => (loc.decoded, loc.records),
            None => {
                let records = shop_stock::scan(&bytes, Some(&valid));
                (bytes, records)
            }
        };
        let prot = p.file_name().unwrap().to_string_lossy().to_string();
        for r in &records {
            let items: Vec<String> = r
                .id_offsets
                .iter()
                .map(|&o| norm(names.name(buf[o]).unwrap_or("")))
                .collect();
            let count = usize::from(buf[r.count_off]);
            if !disc.iter().any(|d| d.count == count && d.items == items) {
                disc.push(DiscShop {
                    label: format!("{prot} {}", r.name),
                    count,
                    items,
                });
            }
        }
    }

    let db = Database::load();
    let key_name = |k: &String| norm(db.resolve_key(k).expect("shop key resolves").name);

    // Forward: every curated shop is a disc record, in on-screen order, with
    // its `featured` rows the hoisted tail.
    let mut fail: Vec<String> = Vec::new();
    let mut matched = 0usize;
    for s in db.shops() {
        let label = format!(
            "{} {}",
            s.town,
            s.name.as_deref().or(s.merchant.as_deref()).unwrap_or("")
        );
        let inventory: Vec<String> = s.inventory.iter().map(key_name).collect();
        let featured: Vec<String> = s.featured.iter().map(key_name).collect();
        let set: BTreeSet<String> = inventory.iter().cloned().collect();
        if s.name.as_deref().is_some_and(|n| NOT_ON_DISC.contains(&n)) {
            assert!(
                !disc.iter().any(|d| d.set() == set),
                "{label} now has a disc record; drop it from NOT_ON_DISC"
            );
            continue;
        }
        let candidates: Vec<&DiscShop> = disc.iter().filter(|d| d.set() == set).collect();
        if candidates.is_empty() {
            fail.push(format!("{label}: no disc stock record with {set:?}"));
            continue;
        }
        if !candidates
            .iter()
            .any(|d| d.screen_order() == (inventory.clone(), featured.clone()))
        {
            let d = candidates[0];
            fail.push(format!(
                "{label}: curated order {inventory:?} featured {featured:?}, disc ({}) screen order {:?}",
                d.label,
                d.screen_order()
            ));
            continue;
        }
        matched += 1;
    }

    // Reverse: every disc shop is curated, except the one-item novelty and
    // stub records (Soru's Bakery sells only Soru Bread; a lone-item record
    // is not a town shop).
    let curated: Vec<BTreeSet<String>> = db
        .shops()
        .iter()
        .map(|s| s.inventory.iter().map(key_name).collect())
        .collect();
    let soru: BTreeSet<String> = std::iter::once(norm("Soru Bread")).collect();
    let mut soru_seen = false;
    for d in &disc {
        let set = d.set();
        if set == soru {
            soru_seen = true;
        } else if set.len() > 1 && !curated.contains(&set) {
            fail.push(format!(
                "disc shop {} has no curated shop: {set:?}",
                d.label
            ));
        }
    }

    assert!(
        fail.is_empty(),
        "curated shops disagree with the disc stock records (disc is authoritative): {fail:#?}"
    );
    assert!(
        soru_seen,
        "Soru's Bakery did not appear; the disc-only exception went vacuous"
    );
    assert_eq!(
        matched,
        db.shops().len() - NOT_ON_DISC.len(),
        "every curated shop matched"
    );
}
