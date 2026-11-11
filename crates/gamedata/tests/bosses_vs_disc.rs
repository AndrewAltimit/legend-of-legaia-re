//! Disc-gated cross-validation of the curated `bosses.toml` against the
//! monster records (PROT 0867) and the `SCUS_942.54` spell table.
//!
//! Each boss row joins its monster record - by name, with a trailing `xN`
//! read as a fight against N of them and a `(Expert)` / `(Master)` suffix as
//! the Muscle Dome round of that monster; the multi-record bosses are pinned
//! by id in [`ALIASES`]. Every row must then agree with the record:
//!
//! - `hp_min` = `hp_max` = the record HP, and `mp` the record MP;
//! - `item_reward` is the record's drop when it drops at 100% (`+0x48` /
//!   `+0x49`), and absent otherwise;
//! - `gold_reward` is what the spoils routine `FUN_8004E568` pays without the
//!   Golden Book: `acc - (acc >> 1)` over `acc = N * (gold >> 1)`;
//! - each attack's `mp` is the MP of the spell-table id of that name (the
//!   table the action SM reads an enemy cast's cost from).
//!
//! `exp_reward` is the walkthrough's per-member share, which depends on the
//! party size, so it is not pinned.
//!
//! Skips silently when the extracted entries are missing.

use std::collections::HashMap;
use std::path::PathBuf;

use legaia_actor_formats::monster_archive::{self, MonsterRecord};
use legaia_game_tables::item_names::ItemNameTable;
use legaia_game_tables::spell_names::SpellNameTable;
use legaia_gamedata::Database;

/// Boss rows whose name the archive carries on several records.
const ALIASES: &[(&str, u16)] = &[
    ("Cort", 180),
    ("Cort (Juggernaut)", 181),
    ("Gaza", 165),
    ("Gaza (Sim-Seru)", 166),
    ("Songi (1st)", 76),
    ("Songi (2nd)", 136),
    ("Songi (3rd)", 179),
    ("Tetsu (Rematch)", 175),
];

/// Rewards a story script hands out after the fight rather than the battle's
/// spoils, so the monster record does not carry them. No stock table holds
/// them, so they stay the walkthrough's and are only held in place here.
const SCRIPTED_ITEMS: &[(&str, &str)] = &[
    ("Gaza (Sim-Seru)", "astral_sword"),
    ("Tetsu (Rematch)", "mettle_gem"),
];
/// The same for gold. Cort's Juggernaut form pays nothing: the ending runs
/// instead of the spoils.
const SCRIPTED_GOLD: &[(&str, u32)] = &[("Tetsu (Rematch)", 12500), ("Cort (Juggernaut)", 0)];

fn norm(s: &str) -> String {
    s.replace('$', "Lv")
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

/// `(record name, how many are fought)` for a boss row name.
fn split_name(name: &str) -> (String, u16) {
    let base = name
        .trim_end_matches(" (Expert)")
        .trim_end_matches(" (Master)");
    match base.rsplit_once('x') {
        Some((b, n)) if !b.is_empty() && n.chars().all(|c| c.is_ascii_digit()) && !n.is_empty() => {
            (b.to_string(), n.parse().unwrap())
        }
        _ => (base.to_string(), 1),
    }
}

#[test]
fn curated_bosses_match_the_disc() {
    let ws = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let (Ok(archive), Ok(scus)) = (
        std::fs::read(ws.join("extracted/PROT/0867_battle_data.BIN")),
        std::fs::read(ws.join("extracted/SCUS_942.54")),
    ) else {
        eprintln!("[skip] extracted PROT 0867 / SCUS_942.54 missing");
        return;
    };
    let recs = monster_archive::records(&archive).expect("archive walk");
    let by_id: HashMap<u16, &MonsterRecord> = recs.iter().map(|r| (r.id, r)).collect();
    let mut by_name: HashMap<String, Vec<&MonsterRecord>> = HashMap::new();
    for r in &recs {
        by_name.entry(norm(&r.name)).or_default().push(r);
    }
    let names = ItemNameTable::from_scus(&scus).expect("item-name table");
    let spells = SpellNameTable::from_scus(&scus).expect("spell table");
    let spell_mp = |name: &str| -> Option<u8> {
        (0u8..=u8::MAX)
            .find(|&id| spells.name(id).is_some_and(|n| norm(n) == norm(name)))
            .and_then(|id| spells.mp(id))
    };
    let db = Database::load();

    let mut fail: Vec<String> = Vec::new();
    for b in db.bosses() {
        let (base, count) = split_name(&b.name);
        let r = match ALIASES.iter().find(|(n, _)| *n == b.name) {
            Some(&(_, id)) => by_id[&id],
            None => match by_name.get(&norm(&base)).map(Vec::as_slice) {
                Some([r]) => r,
                other => {
                    fail.push(format!(
                        "{}: joins {} disc records",
                        b.name,
                        other.map_or(0, <[_]>::len)
                    ));
                    continue;
                }
            },
        };
        let hp = u32::from(r.hp);
        if (b.hp_min, b.hp_max) != (hp, hp) {
            fail.push(format!(
                "{} hp: curated {}..={} disc {hp}",
                b.name, b.hp_min, b.hp_max
            ));
        }
        if b.mp.is_some_and(|mp| mp != u32::from(r.mp)) {
            fail.push(format!("{} mp: curated {:?} disc {}", b.name, b.mp, r.mp));
        }
        if b.meth_id.is_some() {
            let drop = (r.drop_item != 0 && r.drop_chance_pct == 100)
                .then(|| names.name(r.drop_item).unwrap_or("").to_string());
            match SCRIPTED_ITEMS.iter().find(|(n, _)| *n == b.name) {
                Some(&(_, key)) => assert_eq!(
                    b.item_reward.as_deref(),
                    Some(key),
                    "{}: scripted item changed; update SCRIPTED_ITEMS",
                    b.name
                ),
                None => {
                    let curated = b
                        .item_reward
                        .as_ref()
                        .map(|k| db.resolve_key(k).expect("item key").name.to_string());
                    if curated != drop {
                        fail.push(format!(
                            "{} item_reward: curated {curated:?} disc 100% drop {drop:?}",
                            b.name
                        ));
                    }
                }
            }
            let acc = u32::from(count) * u32::from(r.gold >> 1);
            let gold = SCRIPTED_GOLD
                .iter()
                .find(|(n, _)| *n == b.name)
                .map_or(acc - (acc >> 1), |&(_, g)| g);
            if b.gold_reward.is_some_and(|g| g != gold) {
                fail.push(format!(
                    "{} gold_reward: curated {:?} disc {gold}",
                    b.name, b.gold_reward
                ));
            }
        }
        for a in &b.attacks {
            match spell_mp(&a.name) {
                Some(mp) if u32::from(mp) == a.mp => {}
                other => fail.push(format!(
                    "{} attack {}: curated mp {} disc {other:?}",
                    b.name, a.name, a.mp
                )),
            }
        }
    }
    assert!(
        fail.is_empty(),
        "curated bosses disagree with the disc (disc is authoritative): {fail:#?}"
    );
}
