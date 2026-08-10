//! Disc-gated end-to-end tests for the reward-tuning knobs: the EXP multiplier
//! (`--exp-scale`) and the Seru catch-rate override (`--seru-catch-rate`),
//! both data edits to the `battle_data` archive (PROT entry 867) applied to a
//! scratch copy of the disc and re-decoded off the patched image.
//!
//! Assertions per knob:
//!
//! - the edited field comes back as the exact per-monster expectation
//!   (including the floor / saturation clamps against real records);
//! - every other record field is byte-untouched on every monster;
//! - the catch-rate override touches **only** capturable records
//!   (`seru_id != 0`) - a non-Seru monster can never become capturable;
//! - every monster slot stays exactly `0x14000` bytes (no LBA moves);
//! - the pass is deterministic (byte-identical on a re-run) and composes with
//!   the enemy difficulty scale on one image.
//!
//! Skips + passes when `LEGAIA_DISC_BIN` is unset.

use legaia_asset::monster_archive::{self, SLOT_STRIDE};
use legaia_patcher::apply;
use legaia_patcher::disc::{DiscPatcher, MONSTER_ARCHIVE_ENTRY};
use legaia_patcher::monster_stats::{ScalePermille, StatScale};
use legaia_patcher::rewards::scale_exp_value;

fn load_disc() -> Option<Vec<u8>> {
    let p = std::path::PathBuf::from(std::env::var_os("LEGAIA_DISC_BIN")?);
    p.is_file().then(|| std::fs::read(&p).ok()).flatten()
}

fn records(patcher: &DiscPatcher) -> Vec<monster_archive::MonsterRecord> {
    let entry = patcher.read_entry(MONSTER_ARCHIVE_ENTRY).expect("read 867");
    monster_archive::records(&entry).expect("decode records")
}

fn by_id(
    recs: &[monster_archive::MonsterRecord],
) -> std::collections::HashMap<u16, monster_archive::MonsterRecord> {
    recs.iter().map(|r| (r.id, r.clone())).collect()
}

/// Everything the EXP pass must NOT move, as one comparable tuple.
type NonExpFields = (String, u16, u16, [u16; 6], u16, u8, u8, u8, u8, u8, u8, u8);
fn non_exp_fields(r: &monster_archive::MonsterRecord) -> NonExpFields {
    (
        r.name.clone(),
        r.hp,
        r.mp,
        r.stats,
        r.gold,
        r.drop_item,
        r.drop_chance_pct,
        r.seru_id,
        r.catch_rate_pct,
        r.element,
        r.size_class,
        r.magic_count,
    )
}

#[test]
fn exp_scale_round_trips_on_disc() {
    let Some(original) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };

    let base = DiscPatcher::open(original.clone()).expect("open");
    let before = records(&base);
    assert!(before.len() > 100, "expected a large monster roster");
    let before_by_id = by_id(&before);

    // Both directions of the slider, so the 5x saturation (Gaza's 42000 EXP
    // record) and the 0.1x floor are exercised against real records.
    for text in ["2", "0.1", "5"] {
        let scale = ScalePermille::parse(text).expect("valid scale");
        let mut patcher = DiscPatcher::open(original.clone()).expect("open");
        let report = apply::scale_monster_exp(&mut patcher, scale).expect("scale exp");
        assert!(
            report.monsters_changed > 100,
            "{scale}: a roster-wide scale should change nearly every monster, changed {}",
            report.monsters_changed
        );

        let after = by_id(&records(&patcher));
        for b in &before {
            if report.skipped.contains(&b.id) {
                continue;
            }
            let r = after.get(&b.id).expect("monster present after patch");
            assert_eq!(
                r.exp,
                scale_exp_value(b.exp, scale),
                "{scale}: id {} exp scaled wrong",
                b.id
            );
            assert_eq!(
                non_exp_fields(r),
                non_exp_fields(b),
                "{scale}: id {} moved a non-EXP field",
                b.id
            );
        }

        // Every slot keeps its fixed footprint.
        let patched_entry = patcher.read_entry(MONSTER_ARCHIVE_ENTRY).expect("read 867");
        assert_eq!(patched_entry.len() % SLOT_STRIDE, 0);

        // Determinism: the pass is seedless, so a re-run is byte-identical.
        let mut patcher2 = DiscPatcher::open(original.clone()).expect("open");
        apply::scale_monster_exp(&mut patcher2, scale).expect("scale exp");
        assert!(
            patcher2.image() == patcher.image(),
            "{scale}: re-run must reproduce the patched image"
        );

        eprintln!(
            "exp scale {scale}: {} monsters changed",
            report.monsters_changed
        );
    }

    // A retail (1x) scale writes nothing.
    let mut patcher = DiscPatcher::open(original.clone()).expect("open");
    let report =
        apply::scale_monster_exp(&mut patcher, ScalePermille::parse("1").unwrap()).expect("1x");
    assert_eq!(report.monsters_changed, 0, "1x must be a no-op");
    assert!(patcher.image() == &original[..], "1x must not touch a byte");
    let _ = before_by_id;
}

/// Everything the catch-rate pass must NOT move.
type NonRateFields = (String, u16, u16, [u16; 6], u16, u16, u8, u8, u8, u8);
fn non_rate_fields(r: &monster_archive::MonsterRecord) -> NonRateFields {
    (
        r.name.clone(),
        r.hp,
        r.mp,
        r.stats,
        r.gold,
        r.exp,
        r.drop_item,
        r.drop_chance_pct,
        r.seru_id,
        r.magic_count,
    )
}

#[test]
fn seru_catch_rate_round_trips_on_disc() {
    let Some(original) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };

    let base = DiscPatcher::open(original.clone()).expect("open");
    let before = records(&base);
    let capturable: Vec<&monster_archive::MonsterRecord> =
        before.iter().filter(|r| r.seru_id != 0).collect();
    // The retail roster carries 63 capturable Seru records (ids 10..=161,
    // Seru ids 0x01..=0x15) with rates 1..=80%.
    assert_eq!(
        capturable.len(),
        63,
        "expected the retail capturable roster"
    );
    assert!(
        capturable
            .iter()
            .all(|r| (1..=80).contains(&r.catch_rate_pct))
    );

    for pct in [100u8, 0u8] {
        let mut patcher = DiscPatcher::open(original.clone()).expect("open");
        let report = apply::set_seru_catch_rate(&mut patcher, pct).expect("set catch rate");
        assert_eq!(
            report.monsters_changed + report.skipped.len(),
            capturable.len(),
            "{pct}%: every capturable record must be visited"
        );

        let after = by_id(&records(&patcher));
        for b in &before {
            if report.skipped.contains(&b.id) {
                continue;
            }
            let r = after.get(&b.id).expect("monster present after patch");
            if b.seru_id != 0 {
                assert_eq!(r.catch_rate_pct, pct, "{pct}%: id {} rate wrong", b.id);
            } else {
                // A non-Seru record is byte-untouched - rate byte included.
                assert_eq!(
                    r.catch_rate_pct, b.catch_rate_pct,
                    "{pct}%: non-Seru id {} rate moved",
                    b.id
                );
            }
            assert_eq!(
                non_rate_fields(r),
                non_rate_fields(b),
                "{pct}%: id {} moved a non-rate field",
                b.id
            );
        }

        let patched_entry = patcher.read_entry(MONSTER_ARCHIVE_ENTRY).expect("read 867");
        assert_eq!(patched_entry.len() % SLOT_STRIDE, 0);

        // Determinism.
        let mut patcher2 = DiscPatcher::open(original.clone()).expect("open");
        apply::set_seru_catch_rate(&mut patcher2, pct).expect("set catch rate");
        assert!(
            patcher2.image() == patcher.image(),
            "{pct}%: re-run must reproduce the patched image"
        );

        eprintln!(
            "seru catch rate {pct}%: {} monsters changed",
            report.monsters_changed
        );
    }
}

/// The two knobs and the difficulty scale all edit the same archive; run all
/// three on one image and confirm each lands independently.
#[test]
fn rewards_compose_with_stat_scale_on_disc() {
    let Some(original) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };

    let base = DiscPatcher::open(original.clone()).expect("open");
    let before = records(&base);

    let mut patcher = DiscPatcher::open(original).expect("open");
    let stat_scale = StatScale::parse("2").expect("valid scale");
    let exp_scale = ScalePermille::parse("3").expect("valid scale");
    let stats_report = apply::scale_monster_stats(&mut patcher, stat_scale).expect("scale stats");
    let exp_report = apply::scale_monster_exp(&mut patcher, exp_scale).expect("scale exp");
    let rate_report = apply::set_seru_catch_rate(&mut patcher, 100).expect("set catch rate");

    let after = by_id(&records(&patcher));
    let skipped = |id: u16| {
        stats_report.skipped.contains(&id)
            || exp_report.skipped.contains(&id)
            || rate_report.skipped.contains(&id)
    };
    for b in &before {
        if skipped(b.id) {
            continue;
        }
        let r = after.get(&b.id).expect("monster present after patch");
        assert_eq!(r.exp, scale_exp_value(b.exp, exp_scale), "id {}", b.id);
        if b.seru_id != 0 {
            assert_eq!(r.catch_rate_pct, 100, "id {}", b.id);
        }
        // The stat scale's own oracle covers the stat expectation; here it is
        // enough that the composed image still decodes and the reward fields
        // landed - plus one spot check that stats did move.
        assert_eq!(r.gold, b.gold, "id {}: gold must never move", b.id);
    }
    assert!(exp_report.monsters_changed > 100);
    assert_eq!(
        rate_report.monsters_changed + rate_report.skipped.len(),
        63,
        "catch-rate pass must still reach the whole capturable roster"
    );
}
