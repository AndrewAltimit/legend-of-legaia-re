//! Disc-gated cross-validation of curated magic against the static spell table
//! in `SCUS_942.54` (`DAT_800754C8` stats / `DAT_800754D0` names, 12-byte
//! stride; parsed by `legaia_game_tables::spell_names`).
//!
//! The spell table is the single source the battle-action SM reads a cast's
//! on-screen name, MP cost (`stats +3`) and target shape (`stats +2`) from -
//! for both party and enemy casts. So it is authoritative ground truth for the
//! curated player-magic chart mined from walkthroughs.
//!
//! Joining the curated `magic.toml` to the disc table **by spell name** (the
//! player Seru-magic / Ra-Seru summons land in the named region of the table),
//! this asserts, for every curated spell that name-matches a disc id:
//!
//! * **MP cost** is byte-exact (`Spell::mp` == disc `stats +3`), and
//! * **target shape** agrees (`Spell::target` decodes to the same
//!   `SpellTargetShape` as the disc `+2` byte).
//!
//! Curated names that don't map to a disc id are tolerated (the curated chart
//! also lists higher-mist per-level variants the canonical table doesn't name);
//! only a present-but-different MP / target is a failure.
//!
//! **MP** is byte-exact for every name-join (this oracle pinned a curated
//! target error - Mushura / "Crazy Driver", whose disc `+2` is `0x44`
//! single-enemy, not all-enemies - which was corrected to the disc value).
//!
//! **Target shape** agrees for every name-join, including the revive Ra-Seru
//! summon **Horn / "Resurrector"** (id `0x9c`): its `+2` byte is `0x24`, which
//! the side bit both runtime pickers test (`0x40`, clear = the caster's own
//! side) reads as all allies. The `0x02`-bit reading once made it the lone
//! "enemy-side" exception; the test pins Horn explicitly so the case that
//! told the two readings apart cannot go vacuous.
//!
//! Skips silently when `extracted/SCUS_942.54` is missing.

use std::collections::HashMap;
use std::path::PathBuf;

use legaia_game_tables::spell_names::{SpellNameTable, SpellTargetShape};
use legaia_gamedata::Database;

fn scus_bytes() -> Option<Vec<u8>> {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace = manifest.parent()?.parent()?;
    std::fs::read(workspace.join("extracted").join("SCUS_942.54")).ok()
}

fn norm(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

/// Map a curated `target` string to the disc target-shape, where one exists.
/// `self`-targeting curated entries have no four-way shape and return `None`.
fn curated_shape(target: &str) -> Option<SpellTargetShape> {
    match target {
        "single_enemy" => Some(SpellTargetShape::OneEnemy),
        "all_enemies" => Some(SpellTargetShape::AllEnemies),
        "single_ally" => Some(SpellTargetShape::OneAlly),
        "all_allies" => Some(SpellTargetShape::AllAllies),
        _ => None,
    }
}

#[test]
fn curated_magic_mp_and_target_match_the_disc() {
    let Some(scus) = scus_bytes() else {
        eprintln!("[skip] extracted/SCUS_942.54 missing");
        return;
    };
    let table = SpellNameTable::from_scus(&scus).expect("parse spell table");

    // norm(name) -> (id, mp, target shape), over every named id. A name that
    // maps to more than one id is dropped (ambiguous join).
    let mut counts: HashMap<String, usize> = HashMap::new();
    for id in 0u8..=u8::MAX {
        if let Some(name) = table.name(id) {
            *counts.entry(norm(name)).or_default() += 1;
        }
    }
    let mut by_name: HashMap<String, (u8, u8, SpellTargetShape)> = HashMap::new();
    for id in 0u8..=u8::MAX {
        if let Some(name) = table.name(id) {
            let n = norm(name);
            if counts[&n] != 1 {
                continue;
            }
            let e = table.entry(id).unwrap();
            by_name.insert(n, (id, e.mp, e.target_shape()));
        }
    }

    let db = Database::load();

    // The revive Ra-Seru summon Horn / "Resurrector": its `0x24` byte is the
    // case that separates the side bit `0x40` from the `0x02` correlation
    // (see the module doc).
    let revive_summon = norm("Horn");

    let mut checked = 0usize;
    let mut horn_seen = false;
    let mut mp_fail: Vec<String> = Vec::new();
    let mut target_fail: Vec<String> = Vec::new();

    for s in db.spells() {
        let key = norm(&s.name);
        let Some(&(id, disc_mp, disc_shape)) = by_name.get(&key) else {
            continue;
        };
        checked += 1;
        if s.mp != disc_mp as u16 {
            mp_fail.push(format!(
                "{} (id {id:#04x}): curated mp={} disc mp={disc_mp}",
                s.name, s.mp
            ));
        }

        if key == revive_summon {
            horn_seen = true;
            assert_eq!(
                disc_shape,
                SpellTargetShape::AllAllies,
                "Horn's disc `+2` byte no longer reads all allies under the `0x40` side bit"
            );
        }

        if let Some(want) = curated_shape(&s.target)
            && want != disc_shape
        {
            target_fail.push(format!(
                "{} (id {id:#04x}): curated target={} ({want:?}) disc={disc_shape:?}",
                s.name, s.target
            ));
        }
    }

    assert!(
        checked >= 18,
        "expected 18+ curated spells to name-join the disc table, got {checked}"
    );
    assert!(
        horn_seen,
        "Horn / Resurrector did not name-join; its side-bit pin went vacuous"
    );
    assert!(
        mp_fail.is_empty(),
        "curated MP costs disagree with the disc spell table (disc is authoritative): {mp_fail:#?}"
    );
    assert!(
        target_fail.is_empty(),
        "curated target shapes disagree with the disc spell table: {target_fail:#?}"
    );
}

/// Element plates of the spell-name icon escape, in the dialog font's order
/// (`docs/formats/dialog-font.md`): `0x14..=0x1A` for a Seru spell, the
/// winged `0x1D..=0x23` for a Ra-Seru summon.
const ICON_ELEMENTS: [&str; 7] = ["fire", "thunder", "wind", "water", "earth", "light", "dark"];

/// The one curated summon whose disc name differs: the walkthrough's
/// Juggernaut is spell `0x99`, "Evil Seru Magic", which carries no element
/// plate (its curated element is "evil").
const JUGGERNAUT: u8 = 0x99;

/// Every curated spell joins its disc id, and its MP, element (the name's
/// element plate) and - for a Seru spell - its Lv1 / Lv2 / Lv3 absorb
/// chances (the catch rate `+0x3F` of the monster records whose Seru id
/// `+0x3E` is the spell's, `spell = seru_id + 0x80`) are the disc's.
#[test]
fn every_curated_spell_joins_with_mp_element_and_absorb_rates() {
    let Some(scus) = scus_bytes() else {
        eprintln!("[skip] extracted/SCUS_942.54 missing");
        return;
    };
    let ws = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let Ok(archive) = std::fs::read(ws.join("extracted/PROT/0867_battle_data.BIN")) else {
        eprintln!("[skip] extracted/PROT/0867_battle_data.BIN missing");
        return;
    };
    let table = SpellNameTable::from_scus(&scus).expect("parse spell table");
    let monsters = legaia_actor_formats::monster_archive::records(&archive).expect("archive");
    let db = Database::load();
    let mut fail: Vec<String> = Vec::new();
    for s in db.spells() {
        let id = if s.name == "Juggernaut" {
            Some(JUGGERNAUT)
        } else {
            (0u8..=u8::MAX).find(|&id| table.name(id).is_some_and(|n| norm(n) == norm(&s.name)))
        };
        let Some(id) = id else {
            fail.push(format!("{}: no disc spell of that name", s.name));
            continue;
        };
        let e = table.entry(id).unwrap();
        if s.mp != u16::from(e.mp) {
            fail.push(format!("{}: mp curated={} disc={}", s.name, s.mp, e.mp));
        }
        let element = match e.icon {
            Some(i @ 0x14..=0x1A) => ICON_ELEMENTS[usize::from(i - 0x14)],
            Some(i @ 0x1D..=0x23) => ICON_ELEMENTS[usize::from(i - 0x1D)],
            None if id == JUGGERNAUT => "evil",
            other => panic!("{} ({id:#04x}): unexpected name icon {other:?}", s.name),
        };
        if s.element != element {
            fail.push(format!(
                "{}: element curated={} disc={element}",
                s.name, s.element
            ));
        }
        if s.family == legaia_gamedata::SpellFamily::Seru {
            // The level forms are named `X`, `X $2`, `X $3`, which sorts them.
            let mut rates: Vec<(&str, u8)> = monsters
                .iter()
                .filter(|m| m.seru_id != 0 && u16::from(m.seru_id) + 0x80 == u16::from(id))
                .map(|m| (m.name.as_str(), m.catch_rate_pct))
                .collect();
            rates.sort();
            let disc: Vec<Option<u8>> = rates.iter().map(|&(_, r)| Some(r)).collect();
            let curated = vec![s.absorb_lv1, s.absorb_lv2, s.absorb_lv3];
            if disc != curated {
                fail.push(format!(
                    "{}: absorb curated={curated:?} disc={disc:?}",
                    s.name
                ));
            }
        }
    }
    assert!(
        fail.is_empty(),
        "curated magic disagrees with the disc (disc is authoritative): {fail:#?}"
    );
}
