//! Disc-gated end-to-end test for the monster combat-stat randomizer: shuffle
//! the per-monster HP / MP / ATK / DEF / AGL / SPD across the `battle_data`
//! archive (PROT entry 867) on a scratch copy of the disc, then re-decode the
//! patched archive straight off the patched image and confirm the edit is
//! faithful:
//!
//! - each stat column's multiset is preserved (a shuffle is a 1:1 reassignment);
//! - the un-randomized fields (spirit/SP, drop, exp, gold, name, element) are
//!   byte-untouched on every monster;
//! - every monster slot stays exactly `0x14000` bytes (so no LBA moves);
//! - a fixed seed reproduces the patched image byte-for-byte.
//!
//! Skips + passes when `LEGAIA_DISC_BIN` is unset.

use legaia_asset::monster_archive::{self, SLOT_STRIDE};
use legaia_rando::apply;
use legaia_rando::disc::{DiscPatcher, MONSTER_ARCHIVE_ENTRY};
use legaia_rando::drops::DropMode;
use legaia_rando::monster_stats::FIELD_COUNT;

fn load_disc() -> Option<Vec<u8>> {
    let p = std::path::PathBuf::from(std::env::var_os("LEGAIA_DISC_BIN")?);
    p.is_file().then(|| std::fs::read(&p).ok()).flatten()
}

/// Every populated monster's full record, keyed by id, for invariant checks.
fn records(patcher: &DiscPatcher) -> Vec<monster_archive::MonsterRecord> {
    let entry = patcher.read_entry(MONSTER_ARCHIVE_ENTRY).expect("read 867");
    monster_archive::records(&entry).expect("decode records")
}

/// Per-field sorted multiset across the roster (the invariant a shuffle keeps).
fn columns(recs: &[monster_archive::MonsterRecord]) -> [Vec<u16>; FIELD_COUNT] {
    let mut cols: [Vec<u16>; FIELD_COUNT] = Default::default();
    for r in recs {
        let vals = [
            r.hp,
            r.mp,
            r.attack(),
            r.defense_high(),
            r.defense_low(),
            r.agility(),
            r.speed(),
        ];
        for (c, v) in cols.iter_mut().zip(vals) {
            c.push(v);
        }
    }
    for c in &mut cols {
        c.sort_unstable();
    }
    cols
}

#[test]
fn shuffle_monster_stats_round_trips_on_disc() {
    let Some(original) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let seed = 0x5EA1_F00D_57A7_0001;

    let base = DiscPatcher::open(original.clone()).expect("open");
    let before = records(&base);
    assert!(
        before.len() > 100,
        "expected a large monster roster, found {}",
        before.len()
    );
    let before_cols = columns(&before);

    // Shuffle the stats on a scratch copy.
    let mut patcher = DiscPatcher::open(original.clone()).expect("open");
    let report =
        apply::randomize_monster_stats(&mut patcher, seed, DropMode::Shuffle).expect("randomize");
    assert!(
        report.monsters_changed > 50,
        "a roster-wide shuffle should change most monsters, changed {}",
        report.monsters_changed
    );

    // Re-decode the patched archive off the PATCHED image.
    let after = records(&patcher);
    let after_cols = columns(&after);

    // Each stat column's multiset is preserved by a shuffle.
    for f in 0..FIELD_COUNT {
        assert_eq!(
            after_cols[f], before_cols[f],
            "stat column {f} multiset must be preserved by a shuffle"
        );
    }

    // The un-randomized fields are byte-untouched, monster by monster.
    let by_id = |recs: &[monster_archive::MonsterRecord]| {
        recs.iter()
            .map(|r| (r.id, r.clone()))
            .collect::<std::collections::HashMap<_, _>>()
    };
    let a = by_id(&after);
    for b in &before {
        // A slot too tight to re-pack keeps its original stats; skip it here.
        if report.skipped.contains(&b.id) {
            continue;
        }
        let r = a.get(&b.id).expect("monster present after patch");
        assert_eq!(r.stats[0], b.stats[0], "id {}: spirit/SP changed", b.id);
        assert_eq!(r.drop_item, b.drop_item, "id {}: drop changed", b.id);
        assert_eq!(
            r.drop_chance_pct, b.drop_chance_pct,
            "id {}: drop% changed",
            b.id
        );
        assert_eq!(r.exp, b.exp, "id {}: exp changed", b.id);
        assert_eq!(r.gold, b.gold, "id {}: gold changed", b.id);
        assert_eq!(r.element, b.element, "id {}: element changed", b.id);
        assert_eq!(r.name, b.name, "id {}: name changed", b.id);
        assert_eq!(
            r.magic_count, b.magic_count,
            "id {}: spell count changed",
            b.id
        );
    }

    // Every slot stays its fixed footprint (no LBA moved).
    let patched_entry = patcher.read_entry(MONSTER_ARCHIVE_ENTRY).expect("read 867");
    assert_eq!(
        patched_entry.len() % SLOT_STRIDE,
        0,
        "archive size must stay a whole multiple of the slot stride"
    );

    // Determinism: same seed -> byte-identical patched image.
    let mut patcher2 = DiscPatcher::open(original).expect("open");
    let report2 =
        apply::randomize_monster_stats(&mut patcher2, seed, DropMode::Shuffle).expect("randomize");
    assert_eq!(report2.monsters_changed, report.monsters_changed);
    assert!(
        patcher2.image() == patcher.image(),
        "same seed must reproduce the patched image"
    );

    eprintln!(
        "monster-stats shuffle seed {seed:#x}: {} monsters, {} fields changed; all columns preserved",
        report.monsters_changed, report.fields_changed
    );
}
