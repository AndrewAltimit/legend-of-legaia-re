//! Decode the real item-effect descriptor table out of `extracted/SCUS_942.54`
//! if present. Skips and passes when the executable isn't on disk - same gating
//! pattern as the other disc-dependent tests so CI doesn't need Sony bytes.

use legaia_asset::item_effect::{
    ItemEffectCategory, ItemEffectTable, RestoreAmount, StatChange, StatItemEffect, StatTarget,
};
use std::path::PathBuf;

fn scus_path() -> Option<PathBuf> {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace = manifest.parent()?.parent()?;
    let p = workspace.join("extracted").join("SCUS_942.54");
    p.is_file().then_some(p)
}

#[test]
fn decodes_the_item_effect_table_or_skips() {
    let Some(path) = scus_path() else {
        eprintln!("extracted/SCUS_942.54 not present - skipping");
        return;
    };
    let bytes = std::fs::read(&path).expect("read SCUS");
    let table = ItemEffectTable::from_scus(&bytes).expect("parse item-effect table");

    // The table abuts the spell table; it is exactly 130 records (subtypes
    // 0x00..=0x81). This is a stable invariant of the executable's data layout.
    assert_eq!(table.record_count(), 130, "descriptor row count");

    // Every populated descriptor carries the 0x80 base flag, and the +3 byte
    // on consumable rows is the 0x41 no-passive sentinel (the same byte is a
    // live passive-effect index on accessory rows - see accessory_passive).
    for st in [0u8, 1, 2, 5, 8, 16, 19] {
        let d = table.descriptor(st).expect("descriptor in range");
        assert_eq!(d.flags & 0x80, 0x80, "subtype {st} base flag");
        assert_eq!(d.marker, 0x41, "subtype {st} no-passive sentinel");
    }

    // Byte-exact (class, tier, flags) for pinned consumables, keyed by their
    // REAL retail item ids. Each is validated against the on-disc description
    // string (see docs/formats/item-effect-table.md).
    let check = |id: u8, class: u8, tier: u8, cat: ItemEffectCategory| {
        let e = table
            .effect(id)
            .unwrap_or_else(|| panic!("effect for id {id:#x}"));
        assert_eq!(e.class, class, "id {id:#x} class");
        assert_eq!(e.tier, tier, "id {id:#x} tier");
        assert_eq!(e.category(), cat, "id {id:#x} category");
    };
    // Healing Leaf / Flower / Berry: heal-HP tiers 0/1/2.
    check(0x77, 0, 0, ItemEffectCategory::HealHp);
    check(0x78, 0, 1, ItemEffectCategory::HealHp);
    check(0x79, 0, 2, ItemEffectCategory::HealHp);
    // Healing Bloom: all-party HP heal.
    check(0x7A, 1, 0, ItemEffectCategory::HealHpAllParty);
    // Magic Leaf / Fruit: restore MP.
    check(0x7C, 2, 0, ItemEffectCategory::HealMp);
    check(0x7D, 2, 1, ItemEffectCategory::HealMp);
    // Medicine (cure-all), Antidote (cure single), Phoenix (revive).
    check(0x7F, 3, 0, ItemEffectCategory::CureAllStatus);
    check(0x7E, 8, 0, ItemEffectCategory::CureStatus);
    check(0x80, 4, 0, ItemEffectCategory::Revive);
    // Fury Boost (action-gauge), arts books, summon flute.
    check(0x81, 5, 0, ItemEffectCategory::ActionGaugeExtend);
    check(0x8F, 11, 3, ItemEffectCategory::ArtsBook);
    check(0x98, 126, 0, ItemEffectCategory::SummonFlute);
    // Field utilities: escape / warp / reduce-encounter.
    check(0x88, 128, 0, ItemEffectCategory::FieldEscapeDungeon);
    check(0x89, 129, 0, ItemEffectCategory::FieldWarpCity);
    check(0x8A, 130, 0, ItemEffectCategory::ReduceEncounter);

    // Usability flags: healers are usable in field AND battle; the field-only
    // warp items are field-usable but not battle-usable; the all-party heal
    // sets the all-party bit.
    let leaf = table.effect(0x77).unwrap();
    assert!(leaf.field_usable() && leaf.battle_usable());
    let warp = table.effect(0x89).unwrap();
    assert!(warp.field_usable() && !warp.battle_usable());
    assert!(table.effect(0x7A).unwrap().all_party());

    // Healing Shroom (0xA3) shares the item table's subtype 0 with Healing
    // Leaf (0x77), so it resolves to the SAME effect descriptor (HealHp tier 0)
    // - i.e. it heals 200 HP, not the 60 the curated gamedata listed (which had
    // conflated the 60-gold price with the amount).
    assert_eq!(
        table.subtype(0xA3),
        table.subtype(0x77),
        "Shroom shares Leaf subtype"
    );
    assert_eq!(table.subtype(0xA3), 0);
    assert_eq!(table.effect(0xA3), table.effect(0x77));

    // A key item (Swimsuit, id 0x58) funnels to class 0 but with neither
    // usability bit set, so it is NOT a usable consumable.
    let swimsuit = table.effect(0x58).expect("swimsuit effect");
    assert_eq!(swimsuit.class, 0);
    assert!(!swimsuit.is_usable_consumable());
}

/// The literal restore *amounts* the apply handler `FUN_800402F4` reads from the
/// static heal-amount table at `0x8007655C`. Pins, on real disc bytes, that the
/// amounts are an on-disc table (NOT an overlay-resident immediate switch) and
/// that the engine's curated heal/MP figures match it byte-for-byte.
#[test]
fn decodes_the_heal_amount_table_or_skips() {
    let Some(path) = scus_path() else {
        eprintln!("extracted/SCUS_942.54 not present - skipping");
        return;
    };
    let bytes = std::fs::read(&path).expect("read SCUS");
    let table = ItemEffectTable::from_scus(&bytes).expect("parse item-effect table");

    // The tier-indexed sub-tables, byte-exact off the disc.
    let amts = table.heal_amounts();
    assert_eq!(amts.hp, [200, 800, 9999], "HP heal caps (tiers 0/1/2)");
    assert_eq!(amts.mp, [50, 200, 20], "MP heal caps (tiers 0/1/2)");

    // Per-item restore, resolved through each item's (class, tier) descriptor.
    // Healing Leaf / Flower / Berry climb the HP tiers; tier 2 is a full heal.
    assert_eq!(table.restore_amount(0x77), Some(RestoreAmount::Hp(200)));
    assert_eq!(table.restore_amount(0x78), Some(RestoreAmount::Hp(800)));
    assert_eq!(table.restore_amount(0x79), Some(RestoreAmount::Hp(9999)));
    // Healing Bloom (all-party HP, class 1) shares the HP table.
    assert_eq!(table.restore_amount(0x7A), Some(RestoreAmount::Hp(200)));
    // Magic Leaf / Fruit restore MP.
    assert_eq!(table.restore_amount(0x7C), Some(RestoreAmount::Mp(50)));
    assert_eq!(table.restore_amount(0x7D), Some(RestoreAmount::Mp(200)));
    // Healing Shroom shares Leaf's subtype 0, so it really heals 200 (not 60).
    assert_eq!(table.restore_amount(0xA3), Some(RestoreAmount::Hp(200)));
    // Revive (Phoenix) is character-relative (max_hp*0.4 + rand), not a flat
    // table amount; cures resolve to None (not a flat heal at all).
    assert_eq!(
        table.restore_amount(0x80),
        Some(RestoreAmount::CharRelative)
    );
    assert_eq!(
        table.restore_amount(0x7E),
        None,
        "Antidote is a cure, not a heal"
    );
}

/// The stat-affecting consumables (classes 5/6/7) the apply handler
/// `FUN_800402F4` writes, decoded off the disc by their real retail ids. Pins
/// that the *Water* line is permanent stat-up, the Elixirs are one-battle
/// buffs, and Fury Boost is the action-gauge extension — the items the engine
/// catalog historically omitted for lack of a buff taxonomy.
#[test]
fn decodes_the_stat_item_taxonomy_or_skips() {
    use StatTarget::*;
    let Some(path) = scus_path() else {
        eprintln!("extracted/SCUS_942.54 not present - skipping");
        return;
    };
    let bytes = std::fs::read(&path).expect("read SCUS");
    let table = ItemEffectTable::from_scus(&bytes).expect("parse item-effect table");
    let ch = |stat, delta, cap| StatChange { stat, delta, cap };

    // Class 5: Fury Boost (0x81) extends the action gauge.
    assert_eq!(table.stat_effect(0x81), Some(StatItemEffect::ActionGauge));

    // Class 6: the permanent stat-up *Water* line (field-use). Each Water item
    // raises one record stat; the amounts are the handler's inline immediates.
    assert_eq!(
        table.stat_effect(0x82), // Life Water
        Some(StatItemEffect::Permanent(vec![ch(MaxHp, 16, 9999)]))
    );
    assert_eq!(
        table.stat_effect(0x83), // Power Water
        Some(StatItemEffect::Permanent(vec![ch(Attack, 4, 999)]))
    );
    assert_eq!(
        table.stat_effect(0x84), // Guardian Water
        Some(StatItemEffect::Permanent(vec![ch(Defense, 4, 999)]))
    );
    assert_eq!(
        table.stat_effect(0x85), // Swift Water
        Some(StatItemEffect::Permanent(vec![ch(Speed, 4, 999)]))
    );
    assert_eq!(
        table.stat_effect(0x86), // Wisdom Water
        Some(StatItemEffect::Permanent(vec![ch(Intelligence, 4, 999)]))
    );
    assert_eq!(
        table.stat_effect(0x87), // Magic Water
        Some(StatItemEffect::Permanent(vec![ch(MaxMp, 8, 999)]))
    );
    // Honey (0x65) and Miracle Water (0x6D) share the tier-6 "all stats" subtype.
    for all_stats in [0x65u8, 0x6D] {
        let Some(StatItemEffect::Permanent(changes)) = table.stat_effect(all_stats) else {
            panic!("{all_stats:#04x} is a permanent all-stats item");
        };
        assert_eq!(changes.len(), 7, "{all_stats:#04x} raises all seven stats");
        assert!(changes.contains(&ch(Agility, 4, 280)));
        assert!(changes.contains(&ch(MaxHp, 4, 9999)));
    }

    // Class 7: the one-battle ×6/5 buff Elixirs (battle-use). param_2 = tier.
    assert_eq!(
        table.stat_effect(0x8D), // Speed Elixir
        Some(StatItemEffect::BuffOneBattle(vec![Speed]))
    );
    assert_eq!(
        table.stat_effect(0x8C), // Shield Elixir
        Some(StatItemEffect::BuffOneBattle(vec![Defense]))
    );
    assert_eq!(
        table.stat_effect(0x8B), // Power Elixir
        Some(StatItemEffect::BuffOneBattle(vec![Attack]))
    );
    assert_eq!(
        table.stat_effect(0x8E), // Wonder Elixir
        Some(StatItemEffect::BuffOneBattle(vec![
            Speed, Defense, Attack, Agility
        ]))
    );

    // A heal item is not a stat item.
    assert_eq!(table.stat_effect(0x77), None); // Healing Leaf
}
