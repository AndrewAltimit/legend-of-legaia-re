//! Disc-gated: the disc-accurate equipment modifier table (built from the real
//! `SCUS_942.54` stat-bonus table) keys by real item ids and carries the
//! byte-exact attack / defense bonuses - unlike the fabricated-id vanilla
//! catalog. Skips without `LEGAIA_DISC_BIN`.

use legaia_engine_core::Vfs;
use legaia_engine_core::equipment::equip_modifier_table_from_disc;
use std::path::PathBuf;

#[test]
fn disc_modifier_table_uses_real_ids_and_stats() {
    let Some(path) = std::env::var_os("LEGAIA_DISC_BIN").map(PathBuf::from) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    if !path.is_file() {
        eprintln!("[skip] LEGAIA_DISC_BIN is not a file");
        return;
    }
    let scus = legaia_engine_core::DiscVfs::open(&path)
        .expect("open disc")
        .read("SCUS_942.54")
        .expect("SCUS_942.54 present");
    let stats = legaia_asset::equip_stats::EquipStatTable::from_scus(&scus).expect("equip table");
    let table = equip_modifier_table_from_disc(&stats);

    // Chaos Breaker (id 0x27) is a weapon: attack 72, no defense / SPD / INT.
    let cb = table.get(0x27).expect("Chaos Breaker modifier");
    assert_eq!(cb.atk, 72);
    assert_eq!(cb.udf, 0);
    assert_eq!((cb.spd, cb.int), (0, 0), "weapons carry no SPD/INT");
    // Master Armor (id 0x47): def-up/def-down 45/45, no attack / SPD / INT.
    let armor = table.get(0x47).expect("Master Armor modifier");
    assert_eq!(armor.atk, 0);
    assert_eq!((armor.udf, armor.ldf), (45, 45));
    assert_eq!(
        (armor.spd, armor.int),
        (0, 0),
        "body armor carries no SPD/INT"
    );

    // Warrior Boots (id 0x59) are footwear: the +4 byte is the SPD bonus (4),
    // and footwear never carries INT.
    let boots = table.get(0x59).expect("Warrior Boots modifier");
    assert_eq!(boots.spd, 4, "Warrior Boots SPD bonus");
    assert_eq!(boots.int, 0, "footwear carries no INT");

    // The fabricated vanilla catalog keys 0x20 as "Bronze Sword" (atk 10), but
    // the real id 0x20 is the Mace (attack 38) - proving the disc table is the
    // real id space, not the fabricated one.
    let mace = table.get(0x20).expect("real id 0x20 = Mace");
    assert_eq!(
        mace.atk, 38,
        "real 0x20 is the Mace (atk 38), not Bronze Sword"
    );

    // A consumable id carries no equipment modifier.
    assert!(table.get(0x77).is_none(), "Healing Leaf is not equipment");
}

/// Disc-gated: the disc-pinned equip restrictions (character mask `+6` + slot
/// category `+7`) gate the equip session's per-character item list. A Vahn-only
/// weapon must not appear in Noa's weapon picker; an anyone-equippable weapon
/// must appear for every party member.
#[test]
fn disc_equip_restrictions_gate_equip_session_item_list() {
    use legaia_asset::equip_stats::EquipSlot as DiscSlot;
    use legaia_engine_core::battle_stats::{EquipmentTable, StatRecord, StatusModifiers};
    use legaia_engine_core::equip_session::EquipSession;
    use legaia_engine_core::equipment::DiscEquipInfo;
    use std::collections::HashMap;

    let Some(path) = std::env::var_os("LEGAIA_DISC_BIN").map(PathBuf::from) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    if !path.is_file() {
        eprintln!("[skip] LEGAIA_DISC_BIN is not a file");
        return;
    }
    let scus = legaia_engine_core::DiscVfs::open(&path)
        .expect("open disc")
        .read("SCUS_942.54")
        .expect("SCUS_942.54 present");
    let stats = legaia_asset::equip_stats::EquipStatTable::from_scus(&scus).expect("equip table");
    let info = DiscEquipInfo::from_disc(&stats);

    // Ground truth (cross-checked in crates/asset/tests/equip_stats_real.rs):
    //   Force Blade  0x25 = Vahn only  (mask 1)  weapon
    //   War God Band 0x41 = Gala only  (mask 4)  head
    //   Survival Knife 0x22 = anyone   (mask 7)  weapon
    assert_eq!(info.category(0x25), Some(DiscSlot::Weapon));
    assert!(info.can_equip(0x25, 0), "Vahn equips Force Blade");
    assert!(!info.can_equip(0x25, 1), "Noa cannot equip Force Blade");
    assert!(!info.can_equip(0x25, 2), "Gala cannot equip Force Blade");

    assert_eq!(info.category(0x41), Some(DiscSlot::Head));
    assert!(info.can_equip(0x41, 2), "Gala equips War God Band");
    assert!(!info.can_equip(0x41, 0), "Vahn cannot equip War God Band");

    assert!(
        (0..3).all(|s| info.can_equip(0x22, s)),
        "anyone equips Survival Knife"
    );

    // End-to-end through the equip session item filter. A weapon-slot picker
    // (UI slot 0) shows only weapons the active character may equip.
    let mut inv = HashMap::new();
    inv.insert(0x25, 1); // Force Blade (Vahn only)
    inv.insert(0x22, 1); // Survival Knife (anyone)

    let record = StatRecord {
        base_attack: 50,
        base_udf: 30,
        base_ldf: 25,
        base_accuracy: 80,
        base_evasion: 20,
        base_spd: 35,
        base_int: 18,
        equip: [0; 8],
    };

    let vahn = EquipSession::new_with_restrictions(
        record,
        inv.clone(),
        EquipmentTable::new(),
        StatusModifiers::default(),
        Vec::new(),
        info.clone(),
        0,
    );
    let mut v: Vec<u8> = vahn.items_for_slot(0).iter().map(|i| i.id).collect();
    v.sort();
    assert_eq!(v, vec![0x22, 0x25], "Vahn sees both weapons");

    let noa = EquipSession::new_with_restrictions(
        record,
        inv,
        EquipmentTable::new(),
        StatusModifiers::default(),
        Vec::new(),
        info,
        1,
    );
    let n: Vec<u8> = noa.items_for_slot(0).iter().map(|i| i.id).collect();
    assert_eq!(n, vec![0x22], "Noa only sees the anyone-equippable weapon");
}
