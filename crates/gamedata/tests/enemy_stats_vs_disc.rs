//! Disc-gated cross-validation of the curated `enemies.toml` against the
//! monster-stat archive (`PROT 0867`, parsed by
//! `legaia_actor_formats::monster_archive`) and the `SCUS_942.54` steal table
//! (`DAT_80077828`, `legaia_game_tables::steal_table`).
//!
//! The curated bestiary stats are not raw copies of the disc record - they
//! are the **in-battle stats**, the raw record run through the battle
//! loader's scripted-fight stat boost (`FUN_80054cb0`;
//! [`MonsterRecord::battle_stats`]) - and the rewards are what the victory
//! spoils routine `FUN_8004E568` pays for one enemy of the kind:
//!
//! | curated field | disc source | relation |
//! |---|---|---|
//! | `hp` / `mp` | record `hp` / `mp` | exact |
//! | `spd` / `agl` | `speed()` / `agility()` | exact |
//! | `atk` / `int` | `battle_stats()` ATK / INT | exact (`+= atk>>2`, `+= int>>3`) |
//! | `udf` / `ldf` | `defense_high()` / `defense_low()` | exact, x2 |
//! | `exp` | record `exp` | exact, `exp - (exp >> 2)` (`0x8004F0B8`) |
//! | `gold` | record `gold` | exact, `h - (h >> 1)`, `h = gold >> 1` (`0x8004EFBC` / `0x8004F0DC`, no Golden Book) |
//! | `element` | record `+0x1D` | exact (`7` = none) |
//! | `drop` | record `+0x48` item, `+0x49` chance | item name; a `0%` drop is no drop |
//! | `steal` / `steal_chance` | steal table `[chance, item]` | item name + chance |
//!
//! Curated names join the disc by name with the archive's `$2` / `$3` level
//! suffix read as the curated `Lv2` / `Lv3`. The boss names a plain name join
//! cannot place (one name across several records) are pinned to their record
//! id in [`ALIASES`]. Every curated enemy must join.
//!
//! Skips silently when `extracted/PROT/0867_battle_data.BIN` or
//! `extracted/SCUS_942.54` is missing.

use std::collections::HashMap;
use std::path::PathBuf;

use legaia_actor_formats::monster_archive::{self, MonsterRecord};
use legaia_game_tables::item_names::ItemNameTable;
use legaia_game_tables::steal_table::StealTable;
use legaia_gamedata::Database;

/// Curated boss rows whose name the archive carries on several records,
/// pinned to the record the walkthrough's numbers belong to. The five `Cort`
/// records `180, 183..=186` are identical; `181` is the Juggernaut form.
const ALIASES: &[(&str, u16)] = &[
    ("Cort", 180),
    ("Cort (Juggernaut)", 181),
    ("Gaza", 165),
    ("Gaza (Sim-Seru)", 166),
    ("Songi (1st)", 76),
    ("Songi (2nd)", 136),
    ("Songi (3rd)", 179),
    ("Tetsu (Rematch)", 175),
    ("Tetsu (Training)", 79),
];

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

/// Lowercase alphanumerics, with the archive's `$N` level suffix spelled as
/// the curated `LvN`.
fn norm(s: &str) -> String {
    s.replace('$', "Lv")
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

#[test]
fn curated_enemies_match_the_disc() {
    let ws = workspace();
    let Ok(entry) = std::fs::read(ws.join("extracted/PROT/0867_battle_data.BIN")) else {
        eprintln!("[skip] extracted/PROT/0867_battle_data.BIN missing");
        return;
    };
    let Ok(scus) = std::fs::read(ws.join("extracted/SCUS_942.54")) else {
        eprintln!("[skip] extracted/SCUS_942.54 missing");
        return;
    };
    let recs = monster_archive::records(&entry).expect("archive walk");
    let names = ItemNameTable::from_scus(&scus).expect("item-name table");
    let steals = StealTable::from_scus(&scus).expect("steal table");

    let mut by_name: HashMap<String, Vec<&MonsterRecord>> = HashMap::new();
    for r in &recs {
        by_name.entry(norm(&r.name)).or_default().push(r);
    }
    let by_id: HashMap<u16, &MonsterRecord> = recs.iter().map(|r| (r.id, r)).collect();

    let db = Database::load();
    let item_name = |key: &Option<String>| -> Option<String> {
        key.as_ref().map(|k| {
            db.resolve_key(k)
                .unwrap_or_else(|| panic!("unresolved item key {k}"))
                .name
                .to_string()
        })
    };

    let mut fail: Vec<String> = Vec::new();
    let mut checked = 0usize;
    for e in db.enemies() {
        let r = match ALIASES.iter().find(|(n, _)| *n == e.name) {
            Some(&(_, id)) => by_id[&id],
            None => match by_name.get(&norm(&e.name)).map(Vec::as_slice) {
                Some([r]) => r,
                other => {
                    fail.push(format!(
                        "{}: joins {} disc records",
                        e.name,
                        other.map_or(0, <[_]>::len)
                    ));
                    continue;
                }
            },
        };
        checked += 1;
        let bs = r.battle_stats();
        let h = u32::from(r.gold >> 1);
        let exp = u32::from(r.exp - (r.exp >> 2));
        let want: [(&str, Option<u32>, u32); 10] = [
            ("hp", e.hp, r.hp.into()),
            ("mp", e.mp, r.mp.into()),
            ("spd", e.spd, r.speed().into()),
            ("agl", e.agl, r.agility().into()),
            ("atk", e.atk, bs[1].into()),
            ("int", e.intel, bs[4].into()),
            ("udf", e.udf, u32::from(r.defense_high()) * 2),
            ("ldf", e.ldf, u32::from(r.defense_low()) * 2),
            ("exp", e.exp, exp),
            ("gold", e.gold, h - (h >> 1)),
        ];
        for (label, curated, disc) in want {
            if curated != Some(disc) {
                fail.push(format!(
                    "{} {label}: curated={curated:?} disc={disc}",
                    e.name
                ));
            }
        }
        let element = ELEMENTS[usize::from(r.element.min(7))];
        if e.element.as_deref() != element {
            fail.push(format!(
                "{} element: curated={:?} disc={element:?}",
                e.name, e.element
            ));
        }
        let drop = (r.drop_item != 0 && r.drop_chance_pct != 0)
            .then(|| names.name(r.drop_item).unwrap_or("").to_string());
        if item_name(&e.drop) != drop {
            fail.push(format!(
                "{} drop: curated={:?} disc={drop:?}",
                e.name, e.drop
            ));
        }
        let steal = steals.entry(r.id).filter(|s| s.is_stealable());
        let disc_steal = steal.map(|s| names.name(s.item_id).unwrap_or("").to_string());
        if item_name(&e.steal) != disc_steal {
            fail.push(format!(
                "{} steal: curated={:?} disc={disc_steal:?}",
                e.name, e.steal
            ));
        }
        let disc_chance = steal.map(|s| u32::from(s.chance_pct));
        if e.steal_chance != disc_chance {
            fail.push(format!(
                "{} steal_chance: curated={:?} disc={disc_chance:?}",
                e.name, e.steal_chance
            ));
        }
    }

    assert!(
        fail.is_empty(),
        "curated enemies disagree with the disc (disc is authoritative): {fail:#?}"
    );
    assert!(checked >= 170, "expected 170+ enemy joins, got {checked}");
}
