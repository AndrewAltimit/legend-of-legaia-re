//! Disc-gated cross-validation of the accessory passive-effect indexing: the
//! static `SCUS_942.54` descriptor-`+3` passive index (parsed by
//! `legaia_asset::accessory_passive`) against the curated gamedata accessory
//! table (`data/gamedata/accessories.toml`).
//!
//! Every curated accessory with a structured `effect_class` is name-matched
//! to its disc item id and its decoded passive index is asserted against the
//! class (+ value / status / element where the class is parameterised). This
//! pins the whole 64-slot index space semantics byte-for-byte against the
//! published effects.
//!
//! Skips silently when `extracted/SCUS_942.54` is missing.

use std::collections::HashMap;
use std::path::PathBuf;

use legaia_asset::accessory_passive::AccessoryPassiveTable;
use legaia_asset::item_names::ItemNameTable;
use legaia_gamedata::{Accessory, Database};

fn scus_bytes() -> Option<Vec<u8>> {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace = manifest.parent()?.parent()?;
    let p = workspace.join("extracted").join("SCUS_942.54");
    std::fs::read(&p).ok()
}

/// Normalise a name for case-insensitive comparison (lowercase, collapse to
/// alphanumerics).
fn norm(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

/// The passive index a curated accessory's structured effect class predicts,
/// or `None` when the class doesn't map 1:1 to a single index (then the test
/// checks membership instead).
fn expected_index(a: &Accessory) -> Option<u8> {
    let class = a.effect_class.as_deref()?;
    let value = a.effect_value;
    Some(match (class, value) {
        ("hp_max_pct", Some(10)) => 0x00,
        ("hp_max_pct", Some(25)) => 0x01,
        ("mp_max_pct", Some(10)) => 0x02,
        ("mp_max_pct", Some(25)) => 0x03,
        ("mp_cost_pct", Some(-25)) => 0x04,
        ("mp_cost_pct", Some(-50)) => 0x05,
        ("attack_pct", _) => 0x06,
        ("udf_pct", _) => 0x07,
        ("ldf_pct", _) => 0x08,
        ("defense_pct", _) => 0x09,
        ("speed_pct", _) => 0x0A,
        ("intelligence_pct", _) => 0x0B,
        ("agility_pct", _) => 0x0C,
        ("double_attack", _) => 0x0D,
        ("ignore_defense", _) => 0x0E,
        ("counter_chance", _) => 0x0F,
        ("first_attack", _) => 0x11,
        ("last_attack", _) => 0x12,
        ("protect_status", _) => match a.status.as_deref() {
            Some("venom") => 0x16,
            Some("venom_toxic") => 0x17,
            Some("rot") => 0x18,
            Some("curse") => 0x19,
            Some("petrify") => 0x1A,
            Some("numb") => 0x1B,
            other => panic!("unmapped protect_status {other:?} for {}", a.name),
        },
        ("all_status_def", _) => 0x1C,
        ("elemental_def", _) => match a.element.as_deref() {
            Some("earth") => 0x1D,
            Some("water") => 0x1E,
            Some("fire") => 0x1F,
            Some("wind") => 0x20,
            Some("thunder") => 0x21,
            Some("light") => 0x22,
            Some("dark") => 0x23,
            other => panic!("unmapped element {other:?} for {}", a.name),
        },
        ("all_elemental_def", _) => 0x24,
        ("hp_per_turn", _) => 0x25,
        ("mp_per_turn", _) => 0x26,
        ("revive_once", _) => 0x27,
        ("ap_accrual_pct", Some(10)) => 0x28,
        ("ap_accrual_pct", Some(25)) => 0x29,
        ("ap_freeze_100", _) => 0x2A,
        ("ap_cost_pct", _) => 0x2B,
        ("art_power_pct", _) => 0x2C,
        ("berserk", _) => 0x2D,
        ("seru_absorb_chance", _) => 0x2E,
        ("xp_pct", _) => 0x2F,
        ("gold_pct", _) => 0x30,
        ("item_drop_chance", _) => 0x31,
        ("ambush_off_pct", _) => 0x32,
        ("ambush_def_pct", _) => 0x33,
        // Chicken Heart = base escape boost; Chicken King's guaranteed
        // escape ("value 100") is the stronger Great Escape passive.
        ("escape_chance", Some(100)) => 0x37,
        ("escape_chance", _) => 0x34,
        ("escape_def_pct", _) => 0x35,
        ("escape_block", _) => 0x36,
        ("hp_per_step", _) => 0x38,
        ("mp_per_step", _) => 0x39,
        ("ap_per_step", _) => 0x3A,
        ("encounter_pct", Some(v)) if v > 0 => 0x3B,
        ("encounter_pct", Some(_)) => 0x3C,
        // The five summon Talismans carry the matching guard passive
        // (elemental defense / encounter-down); the summon itself is the
        // separate spell-grant arm of FUN_80042558. Checked by membership.
        ("summon_seru", _) => return None,
        other => panic!("unmapped effect class {other:?} for {}", a.name),
    })
}

#[test]
fn accessory_passive_indices_match_curated_effects() {
    let Some(scus) = scus_bytes() else {
        eprintln!("extracted/SCUS_942.54 not present - skipping");
        return;
    };
    let passives = AccessoryPassiveTable::from_scus(&scus).expect("parse accessory passives");
    let names = ItemNameTable::from_scus(&scus).expect("parse item names");
    let db = Database::load();

    // Disc item id by normalised name.
    let mut by_name: HashMap<String, u8> = HashMap::new();
    for id in 0u8..=u8::MAX {
        if let Some(n) = names.name(id) {
            by_name.entry(norm(n)).or_insert(id);
        }
    }

    let mut matched = 0usize;
    for acc in db.accessories() {
        let Some(&id) = by_name.get(&norm(&acc.name)) else {
            // A few curated names differ from the disc spelling (e.g.
            // "Light Ra-Seru Egg" vs disc "Light Egg") - skip, the count
            // floor below keeps this honest.
            continue;
        };
        if let Some(expect) = expected_index(acc) {
            let got = passives
                .passive_index(id)
                .unwrap_or_else(|| panic!("{} ({id:#04x}) grants a passive", acc.name));
            assert_eq!(
                got, expect,
                "{} ({id:#04x}): passive index vs curated {:?}",
                acc.name, acc.effect_class
            );
            matched += 1;
        } else if acc.effect_class.as_deref() == Some("summon_seru") {
            // Talismans: guard passive of the matching element / encounter.
            let got = passives.passive_index(id).expect("talisman passive");
            assert!(
                matches!(got, 0x1D | 0x1E | 0x22 | 0x23 | 0x3C),
                "{} ({id:#04x}): talisman guard passive, got {got:#04x}",
                acc.name
            );
            matched += 1;
        }
    }

    // Keep the cross-check non-vacuous: nearly every curated accessory with a
    // structured class must have matched (74 at the time of pinning; allow
    // headroom only for curated-vs-disc spelling gaps).
    assert!(matched >= 70, "only {matched} accessories cross-checked");
}
