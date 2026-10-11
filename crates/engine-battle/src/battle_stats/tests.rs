use super::*;

fn record() -> StatRecord {
    StatRecord {
        base_attack: 100,
        base_udf: 50,
        base_ldf: 60,
        base_accuracy: 90,
        base_evasion: 30,
        base_spd: 40,
        base_int: 25,
        equip: [1, 2, 0, 0, 0, 0, 0, 0],
    }
}

fn weapon(atk: i16) -> ItemModifier {
    ItemModifier {
        atk,
        ..ItemModifier::default()
    }
}

fn armor(udf: i16, ldf: i16) -> ItemModifier {
    ItemModifier {
        udf,
        ldf,
        ..ItemModifier::default()
    }
}

#[test]
fn empty_equip_returns_base_stats() {
    let mut r = record();
    r.equip = [0; 8];
    let table = EquipmentTable::new();
    let s = compute_battle_stats_default(&r, &table, &[]);
    assert_eq!(s.atk, 100);
    assert_eq!(s.udf, 50);
    assert_eq!(s.ldf, 60);
    assert_eq!(s.acc, 90);
    assert_eq!(s.eva, 30);
    assert_eq!(s.spd, 40);
    assert_eq!(s.int, 25);
    assert!(!s.action_blocked);
    assert!(!s.magic_blocked);
}

#[test]
fn equipment_spd_and_int_add_but_not_acc_eva() {
    let mut t = EquipmentTable::new();
    // Footwear SPD bonus in slot id 1, head-gear INT bonus in slot id 2.
    t.set(
        1,
        ItemModifier {
            spd: 5,
            ..ItemModifier::default()
        },
    );
    t.set(
        2,
        ItemModifier {
            int: 8,
            ..ItemModifier::default()
        },
    );
    let s = compute_battle_stats_default(&record(), &t, &[]);
    assert_eq!(s.spd, 45); // 40 + 5
    assert_eq!(s.int, 33); // 25 + 8
    // Equipment never moves the derived accuracy / evasion lines.
    assert_eq!(s.acc, 90);
    assert_eq!(s.eva, 30);
}

#[test]
fn equipment_atk_adds() {
    let mut t = EquipmentTable::new();
    t.set(1, weapon(20));
    let s = compute_battle_stats_default(&record(), &t, &[]);
    assert_eq!(s.atk, 120);
}

#[test]
fn equipment_negative_modifier_clamps_at_zero() {
    let mut t = EquipmentTable::new();
    t.set(1, weapon(-200));
    let s = compute_battle_stats_default(&record(), &t, &[]);
    assert_eq!(s.atk, 0);
}

#[test]
fn ability_bits_or_into_mask() {
    let mut a = ItemModifier::default();
    a.ability_bits[0] = 0x05; // bits 0 and 2
    let mut b = ItemModifier::default();
    b.ability_bits[0] = 0x02; // bit 1
    b.ability_bits[3] = 0x80; // high bit of byte 3 = bit 31
    let mut t = EquipmentTable::new();
    t.set(1, a);
    t.set(2, b);
    let s = compute_battle_stats_default(&record(), &t, &[]);
    assert_eq!(s.abilities[0], 0x07);
    assert_eq!(s.abilities[3], 0x80);
    assert!(s.has_ability(0));
    assert!(s.has_ability(1));
    assert!(s.has_ability(2));
    assert!(s.has_ability(31));
    assert!(!s.has_ability(3));
}

#[test]
fn missing_equipment_id_is_silently_ignored() {
    let mut t = EquipmentTable::new();
    t.set(99, weapon(50));
    let s = compute_battle_stats_default(&record(), &t, &[]);
    assert_eq!(s.atk, 100);
}

#[test]
fn toxic_reduces_attack() {
    let s = compute_battle_stats_default(&record(), &EquipmentTable::new(), &[StatusKind::Toxic]);
    assert_eq!(s.atk, 70); // 100 * 0.7 (the FUN_801DD864 bit-2 scale)
}

#[test]
fn confuse_halves_accuracy() {
    let s = compute_battle_stats_default(&record(), &EquipmentTable::new(), &[StatusKind::Confuse]);
    assert_eq!(s.acc, 45);
}

#[test]
fn sleep_zeros_evasion_and_blocks_actions() {
    let s = compute_battle_stats_default(&record(), &EquipmentTable::new(), &[StatusKind::Sleep]);
    assert_eq!(s.eva, 0);
    assert!(s.action_blocked);
}

#[test]
fn silence_blocks_magic_only() {
    let s = compute_battle_stats_default(&record(), &EquipmentTable::new(), &[StatusKind::Curse]);
    assert!(s.magic_blocked);
    assert!(!s.action_blocked);
}

#[test]
fn petrify_blocks_both_magic_and_actions() {
    let s = compute_battle_stats_default(&record(), &EquipmentTable::new(), &[StatusKind::Faint]);
    assert!(s.magic_blocked);
    assert!(s.action_blocked);
}

#[test]
fn equipment_and_status_compose() {
    let mut t = EquipmentTable::new();
    t.set(1, weapon(40));
    t.set(2, armor(20, 25));
    let s = compute_battle_stats_default(&record(), &t, &[StatusKind::Toxic]);
    // Atk: 100 + 40 = 140; Toxic -> 140 * 0.7 = 98 (the FUN_801DD864
    // bit-2 roll scale mirrored at the stat line).
    assert_eq!(s.atk, 98);
    // Toxic also drops defense by 0.7:
    // UDF: (50 + 20) * 0.7 = 49; LDF: (60 + 25) * 0.7 = 59.5 -> 60 (rounded).
    assert_eq!(s.udf, 49);
    assert_eq!(s.ldf, 60);
}

#[test]
fn custom_modifiers_let_engines_tune_severity() {
    let mods = StatusModifiers {
        toxic_atk_mult: 0.5, // Brutal toxic
        ..StatusModifiers::default()
    };
    let s = compute_battle_stats(
        &record(),
        &EquipmentTable::new(),
        &[StatusKind::Toxic],
        &mods,
    );
    assert_eq!(s.atk, 50);
}

#[test]
fn action_blocked_takes_priority_when_multi_status() {
    let s = compute_battle_stats_default(
        &record(),
        &EquipmentTable::new(),
        &[StatusKind::Sleep, StatusKind::Toxic, StatusKind::Confuse],
    );
    assert!(s.action_blocked);
    assert_eq!(s.eva, 0);
    // Atk + accuracy still applied.
    assert_eq!(s.atk, 70);
    assert_eq!(s.acc, 45);
}

#[test]
fn passive_boost_is_percent_of_base_and_composes_with_equipment() {
    use crate::accessory_passives::AccessoryPassives;
    // Item 0xC6 grants passive 0x06 (ATK +20%) and also has a flat
    // equipment bonus; the percent reads the BASE attack, not the
    // post-equipment value.
    let passives = AccessoryPassives::from_entries([(0xC6, 0x06)], []);
    let mut t = EquipmentTable::new();
    t.set(0xC6, weapon(40));
    let mut r = record();
    r.base_attack = 103; // 103 / 5 = 20 (truncating)
    r.equip = [0xC6, 0, 0, 0, 0, 0, 0, 0];
    let s = compute_battle_stats_with_passives(&r, &t, &passives, &[], &StatusModifiers::default());
    // 103 (base) + 40 (flat equipment) + 103/5 = 163, NOT (143)/5.
    assert_eq!(s.atk, 163);
    // The passive bit lands in the resolved ability mask (index 0x06).
    assert!(s.has_ability(6));
}

#[test]
fn passive_division_truncates_like_retail() {
    use crate::accessory_passives::AccessoryPassives;
    // Index 0x07 = UDF +20% (`/5`), index 0x01 = max HP +25% (`>>2`,
    // no UDF effect). 49 / 5 = 9 truncating (not 9.8 rounded).
    let passives = AccessoryPassives::from_entries([(0xC9, 0x07)], []);
    let mut r = record();
    r.base_udf = 49;
    r.equip = [0xC9, 0, 0, 0, 0, 0, 0, 0];
    let s = compute_battle_stats_with_passives(
        &r,
        &EquipmentTable::new(),
        &passives,
        &[],
        &StatusModifiers::default(),
    );
    assert_eq!(s.udf, 49 + 9);
}

#[test]
fn passive_dual_defense_boost_applies_both_lines() {
    use crate::accessory_passives::AccessoryPassives;
    // Index 0x09 = UDF & LDF +20% each (Guardian Ring family).
    let passives = AccessoryPassives::from_entries([(0xC9, 0x09)], []);
    let mut r = record();
    r.base_udf = 50;
    r.base_ldf = 60;
    r.equip = [0, 0, 0, 0, 0, 0, 0, 0xC9];
    let s = compute_battle_stats_with_passives(
        &r,
        &EquipmentTable::new(),
        &passives,
        &[],
        &StatusModifiers::default(),
    );
    assert_eq!(s.udf, 50 + 10);
    assert_eq!(s.ldf, 60 + 12);
}

#[test]
fn agility_passive_feeds_both_acc_and_eva_with_retail_cap() {
    use crate::accessory_passives::AccessoryPassives;
    // Index 0x0C = AGL +20%; the AGL-derived lines cap at 0x118 = 280.
    let passives = AccessoryPassives::from_entries([(0xCC, 0x0C)], []);
    let mut r = record();
    r.base_accuracy = 250;
    r.base_evasion = 250;
    r.equip = [0xCC, 0, 0, 0, 0, 0, 0, 0];
    let s = compute_battle_stats_with_passives(
        &r,
        &EquipmentTable::new(),
        &passives,
        &[],
        &StatusModifiers::default(),
    );
    // 250 + 250/5 = 300 -> capped at 280.
    assert_eq!(s.acc, 280);
    assert_eq!(s.eva, 280);
}

#[test]
fn passive_boost_caps_stat_lines_at_999() {
    use crate::accessory_passives::AccessoryPassives;
    let passives = AccessoryPassives::from_entries([(0xC6, 0x06)], []);
    let mut r = record();
    r.base_attack = 990;
    r.equip = [0xC6, 0, 0, 0, 0, 0, 0, 0];
    let s = compute_battle_stats_with_passives(
        &r,
        &EquipmentTable::new(),
        &passives,
        &[],
        &StatusModifiers::default(),
    );
    // 990 + 990/5 = 1188 -> retail cap 999.
    assert_eq!(s.atk, 999);
}

#[test]
fn mp_saver_passive_sets_ability_bit_without_stat_change() {
    use crate::accessory_passives::AccessoryPassives;
    // Index 0x05 = "MP Used Down 2" (the Half-cost bit 0x20): a
    // point-of-use flag, no aggregator stat change.
    let passives = AccessoryPassives::from_entries([(0xC5, 0x05)], []);
    let mut r = record();
    r.equip = [0, 0, 0, 0, 0, 0, 0, 0xC5];
    let s = compute_battle_stats_with_passives(
        &r,
        &EquipmentTable::new(),
        &passives,
        &[],
        &StatusModifiers::default(),
    );
    assert!(s.has_ability(5));
    assert_eq!(s.abilities[0], 0x20);
    assert_eq!(s.atk, 100);
    assert_eq!(s.udf, 50);
}

#[test]
fn empty_passives_keep_legacy_behaviour() {
    use crate::accessory_passives::AccessoryPassives;
    let passives = AccessoryPassives::default();
    let s = compute_battle_stats_with_passives(
        &record(),
        &EquipmentTable::new(),
        &passives,
        &[],
        &StatusModifiers::default(),
    );
    let legacy = compute_battle_stats_default(&record(), &EquipmentTable::new(), &[]);
    assert_eq!(s, legacy);
}

#[test]
fn equipment_table_len_and_is_empty_track() {
    let mut t = EquipmentTable::new();
    assert!(t.is_empty());
    assert_eq!(t.len(), 0);
    t.set(1, weapon(10));
    assert_eq!(t.len(), 1);
    assert!(!t.is_empty());
}
