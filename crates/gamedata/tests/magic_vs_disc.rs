//! Disc-gated cross-validation of curated magic against the static spell table
//! in `SCUS_942.54` (`DAT_800754C8` stats / `DAT_800754D0` names, 12-byte
//! stride; parsed by `legaia_asset::spell_names`).
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
//! **Target shape** agrees for every name-join except the one revive Ra-Seru
//! summon, **Horn / "Resurrector"** (id `0x9c`): its `+2` byte is `0x24`
//! (enemy-side) even though the effect revives all allies - the summon's
//! projection plays toward the enemy field while the revive is special-cased by
//! id. The other six Ra-Seru summons (offensive, all-enemies) carry the correct
//! enemy-side byte, so this is a per-spell encoding nuance, not a model gap.
//! The test verifies this exception explicitly so a change on either side
//! re-trips it.
//!
//! Skips silently when `extracted/SCUS_942.54` is missing.

use std::collections::HashMap;
use std::path::PathBuf;

use legaia_asset::spell_names::{SpellNameTable, SpellTargetShape};
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

    // The revive Ra-Seru summon Horn / "Resurrector": its `+2` byte is
    // enemy-side though the effect revives all allies (see the module doc).
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
            // Verify the documented exception still holds: curated effect is
            // ally-side (all allies) but the disc `+2` byte reads enemy-side.
            horn_seen = true;
            assert_eq!(
                curated_shape(&s.target),
                Some(SpellTargetShape::AllAllies),
                "Horn curated target changed; re-confirm the Resurrector exception"
            );
            assert_eq!(
                disc_shape,
                SpellTargetShape::AllEnemies,
                "Horn disc target byte changed; re-confirm the Resurrector exception"
            );
            continue;
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
        "Horn / Resurrector did not name-join; the exception assertion went vacuous"
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
