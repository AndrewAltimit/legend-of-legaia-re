//! Disc-gated end-to-end test for the monster combat-stat randomizer: shuffle
//! the per-monster HP / MP / ATK / DEF / INT / SPD across the `battle_data`
//! archive (PROT entry 867) on a scratch copy of the disc, then re-decode the
//! patched archive straight off the patched image and confirm the edit is
//! faithful:
//!
//! - each stat column's multiset is preserved (a shuffle is a 1:1 reassignment);
//! - the un-randomized fields (AGL gauge, drop, exp, gold, name, element) are
//!   byte-untouched on every monster;
//! - every monster slot stays exactly `0x14000` bytes (so no LBA moves);
//! - a fixed seed reproduces the patched image byte-for-byte.
//!
//! The second test covers the sibling **difficulty scale** (`--enemy-stat-scale`):
//! one global multiplier over the same halfwords, checked against the exact
//! per-monster expectation rather than against a multiset.
//!
//! Skips + passes when `LEGAIA_DISC_BIN` is unset.

use legaia_asset::monster_archive::{self, SLOT_STRIDE};
use legaia_patcher::apply;
use legaia_patcher::disc::{DiscPatcher, MONSTER_ARCHIVE_ENTRY};
use legaia_patcher::drops::DropMode;
use legaia_patcher::monster_stats::{
    self, FIELD_COUNT, SCALE_PINNED_MONSTER_IDS, StatScale, scale_stats,
};

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
            r.intelligence(),
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
        assert_eq!(r.stats[0], b.stats[0], "id {}: AGL gauge changed", b.id);
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

    // The protected scripted-fight monster(s) keep their disc stats verbatim, so
    // the unwinnable-by-design tutorial battle can't be made lethal.
    let stat_vec = |r: &monster_archive::MonsterRecord| {
        [
            r.hp,
            r.mp,
            r.attack(),
            r.defense_high(),
            r.defense_low(),
            r.intelligence(),
            r.speed(),
        ]
    };
    let before_by_id = by_id(&before);
    for &pid in legaia_patcher::monster_stats::PROTECTED_MONSTER_IDS {
        let (Some(b), Some(r)) = (before_by_id.get(&pid), a.get(&pid)) else {
            continue; // id not populated on this disc - nothing to pin
        };
        assert_eq!(
            stat_vec(r),
            stat_vec(b),
            "protected monster {pid}: combat stats must be unchanged"
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

/// The seven scaled halfwords, in `STAT_FIELDS` order.
fn stat_vec(r: &monster_archive::MonsterRecord) -> [u16; FIELD_COUNT] {
    [
        r.hp,
        r.mp,
        r.attack(),
        r.defense_high(),
        r.defense_low(),
        r.intelligence(),
        r.speed(),
    ]
}

/// The difficulty scale is exact, roster-wide, and reward-neutral: every
/// populated monster's stats come back off the patched image as its own disc
/// values times the multiplier, story bosses included, with only the scripted
/// tutorial fight pinned and EXP / gold / drops / AGL untouched.
///
/// Both spellings of the knob run through the identical assertions - a uniform
/// multiplier and a per-stat list are one code path, so the per-field mode gets
/// the same disc oracle rather than a weaker one of its own.
#[test]
fn enemy_stat_scale_round_trips_on_disc() {
    let Some(original) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };

    let base = DiscPatcher::open(original.clone()).expect("open");
    let before = records(&base);
    assert!(before.len() > 100, "expected a large monster roster");
    let by_id = |recs: &[monster_archive::MonsterRecord]| {
        recs.iter()
            .map(|r| (r.id, r.clone()))
            .collect::<std::collections::HashMap<_, _>>()
    };
    let before_by_id = by_id(&before);

    // Both directions of the slider, so the floor / saturation clamps are
    // exercised against real records rather than only synthetic ones - then the
    // advanced mode's per-stat spellings, including one that scales two stats in
    // opposite directions at once.
    for text in ["2", "0.5", "hp=2", "attack=2,defense=0.5"] {
        let scale = StatScale::parse(text).expect("valid scale");
        let mut patcher = DiscPatcher::open(original.clone()).expect("open");
        let report = apply::scale_monster_stats(&mut patcher, scale).expect("scale");
        assert!(
            report.monsters_changed > 100,
            "{scale}: a roster-wide scale should change nearly every monster, changed {}",
            report.monsters_changed
        );

        // Re-decode off the PATCHED image and check each monster individually.
        let after = by_id(&records(&patcher));
        let mut bosses_scaled = 0usize;
        for b in &before {
            // A slot too tight to re-pack keeps its original stats.
            if report.skipped.contains(&b.id) {
                continue;
            }
            let r = after.get(&b.id).expect("monster present after patch");
            let expected = if SCALE_PINNED_MONSTER_IDS.contains(&b.id) {
                stat_vec(b)
            } else {
                scale_stats(&stat_vec(b), scale)
            };
            assert_eq!(stat_vec(r), expected, "{scale}: id {} scaled wrong", b.id);

            // Independent of `scale_stats`: any stat left at 1x must come back
            // byte-identical. `expected` above is built with the very kernel the
            // patcher ran, so on its own it could not catch a kernel that scaled
            // a field nobody asked it to - this compares against the disc.
            if !SCALE_PINNED_MONSTER_IDS.contains(&b.id) {
                for (f, mult) in scale.fields().iter().enumerate() {
                    if mult.is_retail() {
                        assert_eq!(
                            stat_vec(r)[f],
                            stat_vec(b)[f],
                            "{scale}: id {} stat {} was left at 1x but moved",
                            b.id,
                            monster_stats::STAT_FIELDS[f].0
                        );
                    }
                }
            }

            // Nothing outside the seven stat halfwords moves - a hard run is
            // harder, not richer, and the AI's action economy is unchanged.
            assert_eq!(r.stats[0], b.stats[0], "id {}: AGL gauge changed", b.id);
            assert_eq!(r.exp, b.exp, "id {}: exp changed", b.id);
            assert_eq!(r.gold, b.gold, "id {}: gold changed", b.id);
            assert_eq!(r.drop_item, b.drop_item, "id {}: drop changed", b.id);
            assert_eq!(
                r.drop_chance_pct, b.drop_chance_pct,
                "id {}: drop% changed",
                b.id
            );
            assert_eq!(r.element, b.element, "id {}: element changed", b.id);
            assert_eq!(r.name, b.name, "id {}: name changed", b.id);

            // Unlike the shuffle, this pass deliberately reaches the bosses.
            if monster_stats::PROTECTED_MONSTER_IDS.contains(&b.id)
                && !SCALE_PINNED_MONSTER_IDS.contains(&b.id)
                && stat_vec(r) != stat_vec(b)
            {
                bosses_scaled += 1;
            }
        }
        assert!(
            bosses_scaled > 5,
            "{scale}: story bosses must be scaled too, only {bosses_scaled} moved"
        );

        // The pinned tutorial fight is byte-identical in both directions.
        for &pid in SCALE_PINNED_MONSTER_IDS {
            let (Some(b), Some(r)) = (before_by_id.get(&pid), after.get(&pid)) else {
                continue; // id not populated on this disc
            };
            assert_eq!(
                stat_vec(r),
                stat_vec(b),
                "{scale}: pinned monster {pid} must keep its disc stats"
            );
        }

        // Every slot keeps its fixed footprint (no LBA moved).
        let patched_entry = patcher.read_entry(MONSTER_ARCHIVE_ENTRY).expect("read 867");
        assert_eq!(
            patched_entry.len() % SLOT_STRIDE,
            0,
            "archive size must stay a whole multiple of the slot stride"
        );

        // Seedless determinism: the same multiplier reproduces the image.
        let mut again = DiscPatcher::open(original.clone()).expect("open");
        apply::scale_monster_stats(&mut again, scale).expect("scale");
        assert!(
            again.image() == patcher.image(),
            "{scale}: the same multiplier must reproduce the patched image"
        );

        eprintln!(
            "enemy-stat-scale {scale}: {} monsters, {} stats changed ({} bosses); {} slot(s) skipped",
            report.monsters_changed,
            report.fields_changed,
            bosses_scaled,
            report.skipped.len()
        );
    }

    // 1x is the identity: nothing is written at all. A per-stat list that names
    // only 1x multipliers is the same identity, so the advanced mode cannot
    // rewrite the disc while asking for nothing.
    for text in ["1", "hp=1,attack=1"] {
        let mut retail = DiscPatcher::open(original.clone()).expect("open");
        let report = apply::scale_monster_stats(&mut retail, StatScale::parse(text).unwrap())
            .expect("scale 1x");
        assert_eq!(report.monsters_changed, 0, "{text}: 1x must write nothing");
        assert!(
            retail.image() == &original[..],
            "{text}: 1x must leave the disc alone"
        );
    }

    // Composition: the scale multiplies whatever the stat randomizer dealt out,
    // because both read the roster back off the disc.
    let scale = StatScale::parse("2").unwrap();
    let mut combo = DiscPatcher::open(original.clone()).expect("open");
    apply::randomize_monster_stats(&mut combo, 0x5EA1_F00D_57A7_0002, DropMode::Shuffle)
        .expect("shuffle");
    let shuffled = by_id(&records(&combo));
    let scale_report = apply::scale_monster_stats(&mut combo, scale).expect("scale");
    let scaled = by_id(&records(&combo));
    for (id, s) in &shuffled {
        if scale_report.skipped.contains(id) || SCALE_PINNED_MONSTER_IDS.contains(id) {
            continue;
        }
        let r = scaled.get(id).expect("monster present after both passes");
        assert_eq!(
            stat_vec(r),
            scale_stats(&stat_vec(s), scale),
            "id {id}: the scale must multiply the shuffled values"
        );
    }
    eprintln!(
        "enemy-stat-scale composes with a stat shuffle across {} monsters",
        shuffled.len()
    );
}
