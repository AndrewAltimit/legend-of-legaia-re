//! Disc-gated cross-validation of the equipment-slot model: the static
//! `SCUS_942.54` equip-stat table (`DAT_80074F68`, parsed by
//! `legaia_game_tables::equip_stats`) against the curated gamedata armor/weapon/
//! accessory tables.
//!
//! This adjudicates a long-standing residual that claimed the disc table's
//! `+7` slot byte (4 categories: weapon / body / head / footwear) "can't drive
//! an 8-slot UI because helmet vs. ring vs. accessory all read as head". That
//! premise is **false**, and this test pins why:
//!
//! 1. The 4 disc categories map to the four real equipment slots; the three
//!    armour categories match gamedata exactly (body 20 / head 15 / footwear
//!    16). Weapon (50) > curated weapons (27) only because the disc enumerates
//!    the upgradeable Ra-Seru weapon as ~24 per-tier entries - still all weapons.
//! 2. The disc "Head" category is **exactly the gamedata helmets** (Legaia's
//!    seals / clips / crowns / bands / earring / helmet / plume) - not a mix of
//!    helmet + accessory. Name-match: Body 20/20, Footwear 16/16, Head >=14/15
//!    (the lone gap is the "Power Earring(s)" singular/plural spelling).
//! 3. **None of the 77 gamedata accessories ("Goods") appear in the equip-stat
//!    table** - they are a separate system, so the `+7` byte was never meant to
//!    classify them.
//!
//! So the `+7` byte fully drives Legaia's four armor/weapon slots; there is no
//! helmet/accessory collision. The genuinely-separate open question is where
//! the accessory/Goods records live - a distinct thread, not a `+7`
//! disambiguation problem.
//!
//! Skips silently when `extracted/SCUS_942.54` is missing.

use std::path::PathBuf;

use legaia_game_tables::equip_stats::{EquipSlot, EquipStatTable};
use legaia_game_tables::item_names::ItemNameTable;
use legaia_gamedata::Database;

/// Disc equip-table item names bucketed by the four `+7` slot categories.
#[derive(Default)]
struct DiscEquip {
    weapon: Vec<String>,
    body: Vec<String>,
    head: Vec<String>,
    footwear: Vec<String>,
}

impl DiscEquip {
    fn all(&self) -> impl Iterator<Item = &String> {
        self.weapon
            .iter()
            .chain(&self.body)
            .chain(&self.head)
            .chain(&self.footwear)
    }
}

fn scus_bytes() -> Option<Vec<u8>> {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace = manifest.parent()?.parent()?;
    let p = workspace.join("extracted").join("SCUS_942.54");
    std::fs::read(&p).ok()
}

/// Normalise a name for case-insensitive comparison (lowercase, collapse to
/// alphanumerics so "Fighter's Band" == "fighters band").
fn norm(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

/// Collect the disc equip-table item names bucketed by slot category.
fn disc_equip_by_slot(scus: &[u8]) -> DiscEquip {
    let names = ItemNameTable::from_scus(scus).expect("parse item-name table");
    let equips = EquipStatTable::from_scus(scus).expect("parse equip-stat table");
    let mut out = DiscEquip::default();
    for id in 0u8..=u8::MAX {
        let Some(name) = names.name(id) else { continue };
        if name.is_empty() {
            continue;
        }
        if let Some(b) = equips.bonus(id) {
            let bucket = match b.slot() {
                EquipSlot::Weapon => &mut out.weapon,
                EquipSlot::Body => &mut out.body,
                EquipSlot::Head => &mut out.head,
                EquipSlot::Footwear => &mut out.footwear,
            };
            bucket.push(name.to_string());
        }
    }
    out
}

#[test]
fn disc_equip_slots_map_one_to_one_to_gamedata() {
    let Some(scus) = scus_bytes() else {
        eprintln!("[skip] extracted/SCUS_942.54 missing");
        return;
    };
    let by_slot = disc_equip_by_slot(&scus);

    let weapon = by_slot.weapon.len();
    let body = by_slot.body.len();
    let head = by_slot.head.len();
    let foot = by_slot.footwear.len();

    // The three armor categories map to the curated gamedata slots *exactly*
    // (armor has no tier system). Weapons do NOT, because the disc enumerates
    // the upgradeable Ra-Seru weapon as ~24 separate per-tier entries (Meta
    // $1..$9, Terra $1..$8, Ozma $1..$7) that gamedata collapses to one each -
    // so disc Weapon (51) > curated weapons (27). That's expected, not a slot
    // problem: every disc Weapon-slot item is still a weapon.
    let db = Database::load();
    let gd_armor = db.armor().iter().filter(|a| a.slot == "armor").count();
    let gd_helmet = db.armor().iter().filter(|a| a.slot == "helmet").count();
    let gd_shoes = db.armor().iter().filter(|a| a.slot == "shoes").count();

    assert_eq!(body, gd_armor, "disc Body count vs gamedata armor");
    assert_eq!(head, gd_helmet, "disc Head count vs gamedata helmets");
    assert_eq!(foot, gd_shoes, "disc Footwear count vs gamedata shoes");
    assert!(
        weapon >= db.weapons().len(),
        "disc Weapon ({weapon}) should cover at least the curated weapons ({})",
        db.weapons().len()
    );

    // Pin the absolute disc numbers, so a regression in the parser is caught.
    assert_eq!((weapon, body, head, foot), (51, 20, 15, 16));
}

#[test]
fn disc_head_is_helmets_not_accessories() {
    let Some(scus) = scus_bytes() else {
        eprintln!("[skip] extracted/SCUS_942.54 missing");
        return;
    };
    let by_slot = disc_equip_by_slot(&scus);
    let db = Database::load();

    let helmet: Vec<String> = db
        .armor()
        .iter()
        .filter(|a| a.slot == "helmet")
        .map(|a| norm(&a.name))
        .collect();
    let armor: Vec<String> = db
        .armor()
        .iter()
        .filter(|a| a.slot == "armor")
        .map(|a| norm(&a.name))
        .collect();
    let shoes: Vec<String> = db
        .armor()
        .iter()
        .filter(|a| a.slot == "shoes")
        .map(|a| norm(&a.name))
        .collect();
    let accessories: Vec<String> = db.accessories().iter().map(|a| norm(&a.name)).collect();

    let disc_head: Vec<String> = by_slot.head.iter().map(|n| norm(n)).collect();
    let disc_body: Vec<String> = by_slot.body.iter().map(|n| norm(n)).collect();
    let disc_foot: Vec<String> = by_slot.footwear.iter().map(|n| norm(n)).collect();

    // Fraction of each gamedata slot's names that appear in the matching disc
    // bucket. Body / Footwear are exact; Head is 14/15 (the "Power Earring(s)"
    // spelling), so allow one miss there.
    let found = |gd: &[String], disc: &[String]| gd.iter().filter(|n| disc.contains(n)).count();
    assert_eq!(
        found(&armor, &disc_body),
        armor.len(),
        "every armor in disc Body"
    );
    assert_eq!(
        found(&shoes, &disc_foot),
        shoes.len(),
        "every shoe in disc Footwear"
    );
    assert!(
        found(&helmet, &disc_head) >= helmet.len() - 1,
        "disc Head should be the gamedata helmets ({}/{} matched)",
        found(&helmet, &disc_head),
        helmet.len()
    );

    // Decisive: NOT ONE of the 77 accessories ("Goods") is in the equip-stat
    // table, in any slot. So "head" never lumps helmet with accessories.
    let all_disc: Vec<String> = by_slot.all().map(|n| norm(n)).collect();
    let acc_in_equip: Vec<&String> = accessories
        .iter()
        .filter(|a| all_disc.contains(a))
        .collect();
    assert!(
        acc_in_equip.is_empty(),
        "accessories must not appear in the equip-stat table, found: {acc_in_equip:?}"
    );
}

/// Equip-mask bit of a curated character name (record `+6`).
fn mask_bit(c: &str) -> u8 {
    match c {
        "Vahn" => 1,
        "Noa" => 2,
        "Gala" => 4,
        other => panic!("unknown character {other:?}"),
    }
}

/// Every curated weapon and armour piece carries the disc equipment record's
/// stat bonuses (`+0..+4`: INT, ATK, UDF, LDF, SPD), its wearer mask (`+6`)
/// and its slot (`+7`), byte-exact. Every curated piece must join a disc id.
#[test]
fn curated_equipment_stats_and_wearers_match_the_disc() {
    let Some(scus) = scus_bytes() else {
        eprintln!("[skip] extracted/SCUS_942.54 missing");
        return;
    };
    let names = ItemNameTable::from_scus(&scus).expect("parse item-name table");
    let equips = EquipStatTable::from_scus(&scus).expect("parse equip-stat table");
    let mut by_name = std::collections::HashMap::new();
    for id in 0u8..=u8::MAX {
        if let (Some(name), Some(b)) = (names.name(id), equips.bonus(id))
            && !name.is_empty()
        {
            by_name.insert(norm(name), b);
        }
    }
    let db = Database::load();
    let mut fail: Vec<String> = Vec::new();
    let mut checked = 0usize;
    for w in db.weapons() {
        let Some(b) = by_name.get(&norm(&w.name)) else {
            fail.push(format!("weapon {} has no disc equipment record", w.name));
            continue;
        };
        checked += 1;
        let mask = w
            .equip_others
            .iter()
            .fold(mask_bit(&w.equip_best), |m, c| m | mask_bit(c));
        let want = [b.int_up(), b.attack(), b.def_up(), b.def_down(), b.spd_up()].map(u16::from);
        let got = [0, w.attack, 0, 0, 0];
        if want != got {
            fail.push(format!("weapon {}: curated {got:?} disc {want:?}", w.name));
        }
        if b.equip_mask() != mask {
            fail.push(format!(
                "weapon {}: curated wearers {mask:#x} disc {:#x}",
                w.name,
                b.equip_mask()
            ));
        }
        if b.slot() != EquipSlot::Weapon {
            fail.push(format!("weapon {}: disc slot {:?}", w.name, b.slot()));
        }
    }
    for a in db.armor() {
        let Some(b) = by_name.get(&norm(&a.name)) else {
            fail.push(format!("armor {} has no disc equipment record", a.name));
            continue;
        };
        checked += 1;
        let want = [b.int_up(), b.attack(), b.def_up(), b.def_down(), b.spd_up()].map(u16::from);
        let got = [a.int, a.atk, a.udf, a.ldf, a.spd];
        if want != got {
            fail.push(format!(
                "armor {}: curated [int, atk, udf, ldf, spd] {got:?} disc {want:?}",
                a.name
            ));
        }
        if b.equip_mask() != mask_bit(&a.equip) {
            fail.push(format!(
                "armor {}: curated wearer {} disc mask {:#x}",
                a.name,
                a.equip,
                b.equip_mask()
            ));
        }
        let slot = match a.slot.as_str() {
            "armor" => EquipSlot::Body,
            "helmet" => EquipSlot::Head,
            _ => EquipSlot::Footwear,
        };
        if b.slot() != slot {
            fail.push(format!("armor {}: disc slot {:?}", a.name, b.slot()));
        }
    }
    assert!(
        fail.is_empty(),
        "curated equipment disagrees with the disc (disc is authoritative): {fail:#?}"
    );
    assert_eq!(
        checked,
        db.weapons().len() + db.armor().len(),
        "every piece checked"
    );
}
