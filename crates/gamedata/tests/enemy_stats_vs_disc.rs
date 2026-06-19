//! Disc-gated cross-validation of curated enemy stats against the static
//! monster-stat archive (`PROT 0867`, parsed by `legaia_asset::monster_archive`).
//!
//! Joining the curated `enemies.toml` to the disc records **by monster name**
//! reveals that the curated bestiary stats are not raw copies of the disc
//! record - they are **scaled derivations** by a small set of fixed factors
//! (the walkthrough the tables were mined from reports display/derived values):
//!
//! | curated field | disc source (`MonsterRecord`) | factor |
//! |---|---|---|
//! | `hp`   | `hp`             | ×1 (exact) |
//! | `spd`  | `speed()`        | ×1 (exact) |
//! | `agl`  | `spirit()`       | ×1 (exact) - the curated "agl" is the disc SP/spirit stat |
//! | `udf`  | `defense_high()` | ×2 (exact) |
//! | `ldf`  | `defense_low()`  | ×2 (exact) |
//! | `atk`  | `attack()`       | ×5/4 (±1 rounding) |
//! | `exp`  | `exp`            | ×3/4 (±1 rounding) |
//! | `intel`| `agility()`      | ×9/8 (±1 rounding) - the curated "int" is the disc AGL stat scaled |
//! | `gold` | `gold`           | ×5/16 (±1 rounding) |
//!
//! So the **disc is raw ground truth**; the curated `agl`/`intel` labels are in
//! fact the disc *spirit* and *(scaled) agility* stats. This both validates the
//! monster-archive parser (all nine stat fields relate by clean factors across
//! 120+ enemies) and documents the derivation.
//!
//! Only **unambiguously-named** enemies are cross-checked: a few multi-form
//! bosses (e.g. Gaza) share a name across several disc records, so a name join
//! is ambiguous for them and they are skipped.
//!
//! Skips silently when `extracted/PROT/0867_battle_data.BIN` is missing.

use std::collections::HashMap;
use std::path::PathBuf;

use legaia_asset::monster_archive::{self, MonsterRecord};
use legaia_gamedata::{Database, Enemy};

fn entry_867() -> Option<Vec<u8>> {
    for p in ["extracted/PROT", "../../extracted/PROT"] {
        let f = PathBuf::from(p).join("0867_battle_data.BIN");
        if let Ok(b) = std::fs::read(&f) {
            return Some(b);
        }
    }
    None
}

fn norm(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

/// `round(v * num / den)`.
fn scale(v: u16, num: u32, den: u32) -> u32 {
    (v as u32 * num + den / 2) / den
}

#[test]
fn curated_enemy_stats_are_scaled_disc_stats() {
    let Some(entry) = entry_867() else {
        eprintln!("[skip] extracted/PROT/0867_battle_data.BIN missing");
        return;
    };
    let recs = monster_archive::records(&entry).expect("archive walk");

    // Keep only names that map to exactly one disc record (drop multi-form
    // boss name collisions, which a name join can't disambiguate).
    let mut counts: HashMap<String, usize> = HashMap::new();
    for r in &recs {
        *counts.entry(norm(&r.name)).or_default() += 1;
    }
    let by_name: HashMap<String, &MonsterRecord> = recs
        .iter()
        .filter(|r| counts[&norm(&r.name)] == 1)
        .map(|r| (norm(&r.name), r))
        .collect();

    let db = Database::load();

    let mut checked = 0usize;
    let mut exact_fail: Vec<String> = Vec::new();
    let mut scaled_fail: Vec<String> = Vec::new();

    let exact = |fail: &mut Vec<String>, name: &str, label: &str, gd: Option<u32>, disc: u32| {
        if let Some(g) = gd
            && g != disc
        {
            fail.push(format!("{name} {label}: curated={g} disc={disc}"));
        }
    };
    let approx = |fail: &mut Vec<String>, name: &str, label: &str, gd: Option<u32>, pred: u32| {
        if let Some(g) = gd
            && (g as i64 - pred as i64).abs() > 1
        {
            fail.push(format!("{name} {label}: curated={g} pred={pred}"));
        }
    };

    for e in db.enemies() {
        let Some(r) = by_name.get(&norm(&e.name)) else {
            continue;
        };
        checked += 1;
        let e: &Enemy = e;
        // Exact-factor fields.
        exact(&mut exact_fail, &e.name, "hp", e.hp, r.hp as u32);
        exact(&mut exact_fail, &e.name, "spd", e.spd, r.speed() as u32);
        exact(
            &mut exact_fail,
            &e.name,
            "agl=spirit",
            e.agl,
            r.spirit() as u32,
        );
        exact(
            &mut exact_fail,
            &e.name,
            "udf*2",
            e.udf,
            r.defense_high() as u32 * 2,
        );
        exact(
            &mut exact_fail,
            &e.name,
            "ldf*2",
            e.ldf,
            r.defense_low() as u32 * 2,
        );
        // Fractional-factor fields (±1 rounding).
        approx(
            &mut scaled_fail,
            &e.name,
            "atk*5/4",
            e.atk,
            scale(r.attack(), 5, 4),
        );
        approx(
            &mut scaled_fail,
            &e.name,
            "exp*3/4",
            e.exp,
            scale(r.exp, 3, 4),
        );
        approx(
            &mut scaled_fail,
            &e.name,
            "int=agl*9/8",
            e.intel,
            scale(r.agility(), 9, 8),
        );
        approx(
            &mut scaled_fail,
            &e.name,
            "gold*5/16",
            e.gold,
            scale(r.gold, 5, 16),
        );
    }

    assert!(
        checked >= 100,
        "expected 100+ unambiguous enemy name-joins, got {checked}"
    );
    assert!(
        exact_fail.is_empty(),
        "exact-factor enemy stats disagree with the disc: {exact_fail:#?}"
    );
    assert!(
        scaled_fail.is_empty(),
        "scaled-factor enemy stats off by >1 from the disc derivation: {scaled_fail:#?}"
    );
}
