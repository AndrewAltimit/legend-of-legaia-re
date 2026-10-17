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
