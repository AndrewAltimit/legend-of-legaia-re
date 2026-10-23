//! The shops page's per-item records ([`legaia_web_viewer::shop_items`]):
//! the join of the SCUS item, equipment, effect and passive tables comes back
//! with each field from the right table - a heal leaf with its restore amount
//! and no equipment record, a weapon with its slot and equip mask and no
//! effect descriptor, and every sell price exactly half the buy price.
//!
//! Skipped (passes) when `LEGAIA_DISC_BIN` is unset.

#![cfg(not(target_arch = "wasm32"))]

use legaia_iso::image::extract_scus;
use legaia_web_viewer::shop_items::item_records_json;
use std::env;
use std::fs;

#[test]
fn item_records_join_the_scus_tables() {
    let Some(disc_path) = env::var_os("LEGAIA_DISC_BIN") else {
        eprintln!("LEGAIA_DISC_BIN unset; skipping shop item-record test");
        return;
    };
    let disc = fs::read(&disc_path).expect("disc image");
    let scus = extract_scus(&disc).expect("SCUS_942.54");
    let json = item_records_json(&scus).expect("item-name table");
    let rows: Vec<serde_json::Value> = serde_json::from_str(&json).expect("JSON array");
    let by_name = |n: &str| {
        rows.iter()
            .find(|r| r["name"].as_str() == Some(n))
            .unwrap_or_else(|| panic!("{n} missing"))
    };

    let leaf = by_name("Healing Leaf");
    assert_eq!(leaf["kind"], 2);
    assert!(leaf["equip"].is_null());
    assert_eq!(leaf["effect"]["category"], "heal_hp");
    assert_eq!(leaf["effect"]["restore"]["hp"], 200);

    // An accessory carries a passive, not a usable effect.
    let jewel = rows
        .iter()
        .find(|r| r["passive"]["name"].is_string() && r["equip"].is_null())
        .expect("an accessory with a passive");
    assert!(jewel["effect"].is_null(), "{}", jewel["name"]);

    let weapons: Vec<&serde_json::Value> = rows
        .iter()
        .filter(|r| r["equip"]["slot"] == "weapon")
        .collect();
    assert!(!weapons.is_empty(), "no weapon records");
    for w in &weapons {
        assert_eq!(w["kind"], 1, "{}", w["name"]);
        assert!(w["effect"].is_null(), "{}", w["name"]);
        let mask = w["equip"]["mask"].as_u64().unwrap();
        assert!(mask & 7 != 0, "{} equips nobody", w["name"]);
    }

    for r in &rows {
        if let (Some(p), Some(s)) = (r["price"].as_u64(), r["sell"].as_u64()) {
            assert_eq!(s, p / 2, "{}", r["name"]);
        }
    }
    assert!(
        rows.iter().any(|r| r["passive"]["name"].is_string()),
        "no item carries a passive"
    );
}
