//! Disc-gated cross-validation of the curated Muscle Dome roster in
//! `casino.toml` (`[[muscle_dome_course]]`, `[[muscle_dome_round]]`,
//! `[[muscle_dome_boss]]`) against the arena overlay's ladder (PROT 0977) and
//! the monster records it fights (PROT 0867).
//!
//! The ladder (`docs/subsystems/minigame-muscle-dome.md` "Course ladder") is
//! three course descriptors `{ i32 round_count; ptr first_round }` at
//! `0x801D1A08` over a 29-record roster `{ u32 name_ptr; u32 monster_id }` at
//! `0x801D1920`. Each course's curated enemy line-up and each round row's
//! boss + Seru level must be the roster's monster, in order. Every boss stat
//! block (and each Seru level block) must then be its monster record:
//!
//! - `hp` / `mp` / `spd` / `agl` as recorded;
//! - `atk` / `udf` / `ldf` / `intelligence` through the scripted-fight boost
//!   ([`MonsterRecord::battle_stats`]) - a dome leg is a scripted fight;
//! - `gold` = `h - (h >> 1)`, `h = gold >> 1` (the spoils routine
//!   `FUN_8004E568` without the Golden Book);
//! - `element`, `steal` / `steal_chance` (SCUS steal table), `drop` /
//!   `drop_chance` (record `+0x48` / `+0x49`, a `0%` drop is none) and a Seru
//!   level's `absorb_pct` (record catch rate `+0x3F`).
//!
//! `exp` is not pinned: the curated figure is a per-member share, which
//! depends on the party size.
//!
//! Skips silently when the extracted entries are missing.

use std::collections::HashMap;
use std::path::PathBuf;

use legaia_actor_formats::monster_archive::{self, MonsterRecord};
use legaia_game_tables::item_names::ItemNameTable;
use legaia_game_tables::steal_table::StealTable;
use toml::Value;

const CASINO_TOML: &str = include_str!("../../../data/gamedata/casino.toml");

/// Runtime VA the arena overlay (PROT 0977) loads at, from the roster's VA and
/// file offset (`0x801D1920` at `+0x3108`).
const OVERLAY_BASE_VA: u32 = 0x801D_1920 - 0x3108;
/// The course descriptor table (`0x801D1A08`).
const COURSE_TABLE_VA: u32 = 0x801D_1A08;
const COURSES: [&str; 3] = ["beginner", "expert", "master"];

/// Record `+0x1D` element ids (`legaia_game_tables::element_affinity`).
const ELEMENTS: [Option<&str>; 8] = [
    Some("earth"),
    Some("water"),
    Some("fire"),
    Some("wind"),
    Some("thunder"),
    Some("light"),
    Some("dark"),
    None,
];

fn workspace() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn u32_at(b: &[u8], va: u32) -> u32 {
    let o = (va - OVERLAY_BASE_VA) as usize;
    u32::from_le_bytes(b[o..o + 4].try_into().unwrap())
}

/// The archive name with its `$N` level suffix read as the curated `LvN`.
fn curated_name(r: &MonsterRecord) -> String {
    r.name.replace(" $", " Lv")
}

#[test]
fn curated_muscle_dome_roster_matches_the_disc() {
    let ws = workspace();
    let (Ok(overlay), Ok(archive), Ok(scus)) = (
        std::fs::read(ws.join("extracted/PROT/0977_other_game.BIN")),
        std::fs::read(ws.join("extracted/PROT/0867_battle_data.BIN")),
        std::fs::read(ws.join("extracted/SCUS_942.54")),
    ) else {
        eprintln!("[skip] extracted PROT 0977 / 0867 / SCUS_942.54 missing");
        return;
    };
    let recs = monster_archive::records(&archive).expect("archive walk");
    let by_id: HashMap<u16, &MonsterRecord> = recs.iter().map(|r| (r.id, r)).collect();
    let by_name: HashMap<String, &MonsterRecord> =
        recs.iter().map(|r| (curated_name(r), r)).collect();
    let names = ItemNameTable::from_scus(&scus).expect("item-name table");
    let steals = StealTable::from_scus(&scus).expect("steal table");
    let casino: toml::Table = toml::from_str(CASINO_TOML).expect("casino.toml parses");
    let table = |k: &str| {
        casino
            .get(k)
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
    };

    // The disc ladder, as monster ids per course.
    let ladder: Vec<Vec<u16>> = (0..3u32)
        .map(|c| {
            let count = u32_at(&overlay, COURSE_TABLE_VA + c * 8);
            let first = u32_at(&overlay, COURSE_TABLE_VA + c * 8 + 4);
            (0..count)
                .map(|i| u32_at(&overlay, first + i * 8 + 4) as u16)
                .collect()
        })
        .collect();
    assert_eq!(
        ladder.iter().map(Vec::len).collect::<Vec<_>>(),
        [8, 8, 13],
        "the ladder decodes"
    );

    let mut fail: Vec<String> = Vec::new();

    // Course line-ups.
    for course in table("muscle_dome_course") {
        let key = course["key"].as_str().unwrap();
        let c = COURSES.iter().position(|k| *k == key).expect("course key");
        let curated: Vec<&str> = course["enemies"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        let disc: Vec<String> = ladder[c].iter().map(|id| curated_name(by_id[id])).collect();
        if curated != disc {
            fail.push(format!("{key} line-up: curated {curated:?} disc {disc:?}"));
        }
    }

    // Boss rows by key, for the round table.
    let bosses = table("muscle_dome_boss");
    let boss = |key: &str| {
        bosses
            .iter()
            .find(|b| b["key"].as_str() == Some(key))
            .cloned()
    };
    let mut rounds = 0usize;
    for r in table("muscle_dome_round") {
        let key = r["course_key"].as_str().unwrap();
        let c = COURSES.iter().position(|k| *k == key).expect("course key");
        let round = r["round"].as_integer().unwrap() as usize;
        let b = boss(r["boss_key"].as_str().unwrap()).expect("round boss resolves");
        let level = r.get("seru_level").and_then(Value::as_integer).unwrap_or(1);
        let mut name = b["name"].as_str().unwrap().to_string();
        if level > 1 {
            name = format!("{name} Lv{level}");
        }
        let disc = curated_name(by_id[&ladder[c][round - 1]]);
        if name != disc {
            fail.push(format!("{key} round {round}: curated {name} disc {disc}"));
        }
        rounds += 1;
    }
    assert_eq!(rounds, 29, "one round row per roster record");

    // Stat blocks.
    let int = |v: &Value, k: &str| v.get(k).and_then(Value::as_integer);
    let string = |v: &Value, k: &str| v.get(k).and_then(Value::as_str).map(str::to_string);
    let mut blocks = 0usize;
    for b in &bosses {
        let base = b["name"].as_str().unwrap();
        let disc_el = ELEMENTS[usize::from(by_name[base].element.min(7))];
        if string(b, "element").as_deref() != disc_el {
            fail.push(format!(
                "{base} element: curated {:?} disc {disc_el:?}",
                string(b, "element")
            ));
        }
        let mut stat_blocks: Vec<(String, Value)> = Vec::new();
        if let Some(levels) = b.get("seru_level").and_then(Value::as_array) {
            for l in levels {
                let n = int(l, "level").unwrap();
                let name = if n == 1 {
                    base.to_string()
                } else {
                    format!("{base} Lv{n}")
                };
                stat_blocks.push((name, l.clone()));
            }
        } else {
            stat_blocks.push((base.to_string(), b.clone()));
        }
        for (name, v) in stat_blocks {
            let r = by_name
                .get(&name)
                .unwrap_or_else(|| panic!("{name} has no monster record"));
            blocks += 1;
            let bs = r.battle_stats();
            let h = i64::from(r.gold >> 1);
            let want: [(&str, i64); 9] = [
                ("hp", r.hp.into()),
                ("mp", r.mp.into()),
                ("spd", r.speed().into()),
                ("agl", r.agility().into()),
                ("atk", bs[1].into()),
                ("udf", bs[2].into()),
                ("ldf", bs[3].into()),
                ("intelligence", bs[4].into()),
                ("gold", h - (h >> 1)),
            ];
            for (k, disc) in want {
                if int(&v, k) != Some(disc) {
                    fail.push(format!("{name} {k}: curated {:?} disc {disc}", int(&v, k)));
                }
            }
            let steal = steals.entry(r.id).filter(|s| s.is_stealable());
            let disc_steal = steal.map(|s| names.name(s.item_id).unwrap_or("").to_string());
            let disc_steal_pct = steal.map(|s| i64::from(s.chance_pct));
            if (string(&v, "steal"), int(&v, "steal_chance"))
                != (disc_steal.clone(), disc_steal_pct)
            {
                fail.push(format!(
                    "{name} steal: curated {:?}@{:?} disc {disc_steal:?}@{disc_steal_pct:?}",
                    string(&v, "steal"),
                    int(&v, "steal_chance")
                ));
            }
            let dropped = r.drop_item != 0 && r.drop_chance_pct != 0;
            let disc_drop = dropped.then(|| names.name(r.drop_item).unwrap_or("").to_string());
            let disc_drop_pct = dropped.then(|| i64::from(r.drop_chance_pct));
            if (string(&v, "drop"), int(&v, "drop_chance")) != (disc_drop.clone(), disc_drop_pct) {
                fail.push(format!(
                    "{name} drop: curated {:?}@{:?} disc {disc_drop:?}@{disc_drop_pct:?}",
                    string(&v, "drop"),
                    int(&v, "drop_chance")
                ));
            }
            if b.get("seru_level").is_some()
                && int(&v, "absorb_pct") != Some(i64::from(r.catch_rate_pct))
            {
                fail.push(format!(
                    "{name} absorb_pct: curated {:?} disc {}",
                    int(&v, "absorb_pct"),
                    r.catch_rate_pct
                ));
            }
        }
    }
    assert!(blocks >= 25, "only {blocks} stat blocks checked");
    assert!(
        fail.is_empty(),
        "curated Muscle Dome roster disagrees with the disc (disc is authoritative): {fail:#?}"
    );
}
