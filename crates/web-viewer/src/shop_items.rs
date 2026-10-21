//! Per-item stat records for the shops page, read from the visitor's own
//! `SCUS_942.54`.
//!
//! The shops page lists the curated game-data tables without a disc. With one
//! loaded, an expanded item adds what the executable itself says about it:
//! the item record (`kind`, price; sell is exactly half -
//! `docs/subsystems/shop.md`), the equipment stat-bonus record
//! ([`legaia_game_tables::equip_stats`]), the consumable effect descriptor and
//! its restore amount ([`legaia_game_tables::item_effect`]) and the passive an
//! equipped piece grants ([`legaia_game_tables::accessory_passive`]). Nothing
//! here is new decoding: every field is one of those parsers' accessors,
//! joined per item id.

use super::*;
use legaia_game_tables::accessory_passive::{self, AccessoryPassiveTable, BoostedStat};
use legaia_game_tables::equip_stats::{EquipSlot, EquipStatTable};
use legaia_game_tables::item_effect::{
    ItemEffectCategory, ItemEffectTable, RestoreAmount, StatItemEffect, StatTarget,
};
use legaia_game_tables::item_names::{self, ItemNameTable};

fn equip_slot_tag(s: EquipSlot) -> &'static str {
    match s {
        EquipSlot::Body => "body",
        EquipSlot::Head => "head",
        EquipSlot::Weapon => "weapon",
        EquipSlot::Footwear => "footwear",
    }
}

fn category_tag(c: ItemEffectCategory) -> String {
    match c {
        ItemEffectCategory::HealHp => "heal_hp".into(),
        ItemEffectCategory::HealHpAllParty => "heal_hp_all_party".into(),
        ItemEffectCategory::HealMp => "heal_mp".into(),
        ItemEffectCategory::CureAllStatus => "cure_all_status".into(),
        ItemEffectCategory::Revive => "revive".into(),
        ItemEffectCategory::ActionGaugeExtend => "action_gauge_extend".into(),
        ItemEffectCategory::StatUp => "stat_up".into(),
        ItemEffectCategory::StatBuffOneBattle => "stat_buff_one_battle".into(),
        ItemEffectCategory::CureStatus => "cure_status".into(),
        ItemEffectCategory::ArtsBook => "arts_book".into(),
        ItemEffectCategory::SummonFlute => "summon_flute".into(),
        ItemEffectCategory::FieldEscapeDungeon => "field_escape_dungeon".into(),
        ItemEffectCategory::FieldWarpCity => "field_warp_city".into(),
        ItemEffectCategory::ReduceEncounter => "reduce_encounter".into(),
        ItemEffectCategory::Other(n) => format!("other_{n}"),
    }
}

fn stat_tag(s: StatTarget) -> &'static str {
    match s {
        StatTarget::MaxHp => "MaxHP",
        StatTarget::MaxMp => "MaxMP",
        StatTarget::Agility => "AGL",
        StatTarget::Attack => "ATK",
        StatTarget::Defense => "DEF",
        StatTarget::Speed => "SPD",
        StatTarget::Intelligence => "INT",
    }
}

fn boosted_tag(s: BoostedStat) -> &'static str {
    match s {
        BoostedStat::MaxHp => "MaxHP",
        BoostedStat::MaxMp => "MaxMP",
        BoostedStat::Attack => "ATK",
        BoostedStat::DefenseUp => "UDF",
        BoostedStat::DefenseDown => "LDF",
        BoostedStat::Speed => "SPD",
        BoostedStat::Intelligence => "INT",
        BoostedStat::Agility => "AGL",
    }
}

/// Every named item id's record as a JSON array (see
/// [`LegaiaViewer::shop_item_records_json`] for the shape). `None` when the
/// item-name table is not found in `scus` (a build with a different table
/// address); the other tables are optional and their fields come back `null`.
pub fn item_records_json(scus: &[u8]) -> Option<String> {
    let names = ItemNameTable::from_scus(scus)?.drawn(legaia_font::cell_text::level_mark(None));
    let effects = ItemEffectTable::from_scus(scus);
    let equips = EquipStatTable::from_scus(scus);
    let passives = AccessoryPassiveTable::from_scus(scus);
    let mut rows = Vec::new();
    for id in 1..=255u8 {
        let Some(name) = names.name(id) else {
            continue;
        };
        let price = item_names::item_price(scus, id);
        let kind = effects.as_ref().map(|t| t.kind(id));
        let equip = equips
            .as_ref()
            .filter(|_| kind == Some(1))
            .and_then(|t| t.bonus(id))
            .map(|b| {
                serde_json::json!({
                    "slot": equip_slot_tag(b.slot()),
                    "int": b.int_up(),
                    "atk": b.attack(),
                    "udf": b.def_up(),
                    "ldf": b.def_down(),
                    "spd": b.spd_up(),
                    "mask": b.equip_mask(),
                    "ra_seru": b.is_ra_seru(),
                })
            });
        let effect = effects.as_ref().filter(|_| kind == Some(2)).and_then(|t| {
            // An accessory is a kind-2 item too, but its descriptor only
            // carries the passive byte (+3): a descriptor neither menu
            // can use is not an effect.
            let e = t.effect(id).filter(|e| e.is_usable_consumable())?;
            let restore = t.restore_amount(id).map(|r| match r {
                RestoreAmount::Hp(n) => serde_json::json!({ "hp": n }),
                RestoreAmount::Mp(n) => serde_json::json!({ "mp": n }),
                RestoreAmount::CharRelative => serde_json::json!("max"),
            });
            let stat = t.stat_effect(id).map(|s| match s {
                StatItemEffect::ActionGauge => serde_json::json!({ "action_gauge": true }),
                StatItemEffect::Permanent(v) => serde_json::json!({
                    "permanent": v.iter().map(|c| serde_json::json!({
                        "stat": stat_tag(c.stat), "delta": c.delta, "cap": c.cap,
                    })).collect::<Vec<_>>(),
                }),
                StatItemEffect::BuffOneBattle(v) => serde_json::json!({
                    "buff_one_battle": v.iter().map(|s| stat_tag(*s)).collect::<Vec<_>>(),
                }),
            });
            Some(serde_json::json!({
                "class": e.class,
                "tier": e.tier,
                "category": category_tag(e.category()),
                "field": e.field_usable(),
                "battle": e.battle_usable(),
                "all_party": e.all_party(),
                "restore": restore,
                "stat": stat,
            }))
        });
        let passive = passives.as_ref().and_then(|t| {
            let (idx, rec) = t.passive(id)?;
            let boosts: Vec<serde_json::Value> = accessory_passive::stat_boosts(idx)
                .iter()
                .map(|(s, div)| serde_json::json!({ "stat": boosted_tag(*s), "pct": 100 / div }))
                .collect();
            Some(serde_json::json!({
                "index": idx,
                "name": rec.name,
                "description": rec.description,
                "party_wide": rec.party_wide(),
                "boosts": boosts,
            }))
        });
        rows.push(serde_json::json!({
            "id": id,
            "name": name,
            "desc": names.desc(id),
            "kind": kind,
            "price": price,
            "sell": price.map(|p| p / 2),
            "equip": equip,
            "effect": effect,
            "passive": passive,
        }));
    }
    Some(serde_json::Value::Array(rows).to_string())
}

#[wasm_bindgen]
impl LegaiaViewer {
    /// Every named item's record from the loaded disc's executable, for the
    /// shops page's expanded item. JSON array, `"[]"` on a raw PROT.DAT load
    /// (no SCUS):
    ///
    /// ```text
    /// [ { "id": 34, "name": "Healing Leaf", "desc": "...",
    ///     "kind": 2,                 // item record +0: 1 equipment, 2 item
    ///     "price": 100, "sell": 50,  // record +2; sell is exactly half
    ///     "equip": null | { "slot": "weapon", "int", "atk", "udf", "ldf",
    ///                       "spd", "mask": 7, "ra_seru": false },
    ///     "effect": null | { "class", "tier", "category": "heal_hp",
    ///                        "field", "battle", "all_party",
    ///                        "restore": {"hp": 200} | {"mp": n} | "max" | null,
    ///                        "stat": null | {...} },
    ///     "passive": null | { "index", "name", "description",
    ///                         "party_wide", "boosts": [{"stat","pct"}] } } ]
    /// ```
    pub fn shop_item_records_json(&self) -> String {
        self.shop_item_records
            .clone()
            .unwrap_or_else(|| "[]".to_string())
    }
}
