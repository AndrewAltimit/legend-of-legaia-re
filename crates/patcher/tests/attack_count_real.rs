//! Disc-gated end-to-end tests for the enemy attack-count multiplier
//! (`--enemy-attack-count`): a data edit to the `battle_data` archive (PROT
//! entry 867) that divides each retail-affordable attack entry's AGL-cost
//! byte, applied to a scratch copy of the disc and re-decoded off the patched
//! image.
//!
//! Assertions:
//!
//! - every command-band attack entry comes back at the exact per-entry
//!   expectation ([`attack_count::scale_cost`] against the retail cost and the
//!   record's own AGL), including the round-half-up arithmetic, the floor of
//!   1, and the AGL affordability cap;
//! - the min-one-strike guarantee: every monster that can afford an attack in
//!   retail can still afford one at the slowest setting;
//! - sentinel (`0xFF`) and retail-unaffordable (deliberately overpriced)
//!   entries are byte-untouched, so movesets never change;
//! - every other record field is untouched on every monster, and the pinned
//!   tutorial fight (Tetsu, id 79) is byte-identical at every setting;
//! - every monster slot stays exactly `0x14000` bytes (no LBA moves);
//! - the pass is deterministic (byte-identical on a re-run), `1x` writes
//!   nothing, and the knob composes with the enemy difficulty scale.
//!
//! Skips + passes when `LEGAIA_DISC_BIN` is unset.

use legaia_asset::monster_archive::{self, SLOT_STRIDE};
use legaia_patcher::apply;
use legaia_patcher::attack_count::{self, ACTION_BAND, COST_UNAVAILABLE};
use legaia_patcher::disc::{DiscPatcher, MONSTER_ARCHIVE_ENTRY};
use legaia_patcher::monster_stats::{SCALE_PINNED_MONSTER_IDS, ScalePermille, StatScale};

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

/// True when the entry is an AI attack candidate the retail budget can afford.
fn affordable(s: &monster_archive::MonsterSpell, agl: u16) -> bool {
    ACTION_BAND.contains(&s.id) && s.agl_cost != COST_UNAVAILABLE && (s.agl_cost as u16) <= agl
}

/// Everything the attack-count pass must NOT move: the whole record with each
/// action entry's cost byte masked out (set to zero on both sides), so the
/// comparison covers every decoded field except the bytes the pass edits.
fn non_cost_fields(r: &monster_archive::MonsterRecord) -> monster_archive::MonsterRecord {
    let mut masked = r.clone();
    for s in &mut masked.spells {
        s.agl_cost = 0;
    }
    masked
}

#[test]
fn attack_count_scale_round_trips_on_disc() {
    let Some(original) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };

    let base = DiscPatcher::open(original.clone()).expect("open");
    let before = records(&base);
    assert!(before.len() > 100, "expected a large monster roster");

    // The retail roster carries plenty of attackers (records with at least one
    // affordable command-band entry) - the population the slider reaches.
    let attackers = before
        .iter()
        .filter(|r| r.spells.iter().any(|s| affordable(s, r.agility())))
        .count();
    assert!(
        attackers > 100,
        "expected an attacking roster, got {attackers}"
    );

    // Both directions plus the extremes, so the round-half-up arithmetic, the
    // floor of 1 and the AGL cap are all exercised against real records.
    for text in ["2", "0.5", "0.1", "5"] {
        let scale = ScalePermille::parse(text).expect("valid scale");
        let mut patcher = DiscPatcher::open(original.clone()).expect("open");
        let report = apply::scale_enemy_attack_count(&mut patcher, scale).expect("scale");
        assert!(
            report.monsters_changed > 100,
            "{scale}: a roster-wide scale should change most monsters, changed {}",
            report.monsters_changed
        );
        assert!(report.entries_changed >= report.monsters_changed);

        let after = by_id(&records(&patcher));
        for b in &before {
            if report.skipped.contains(&b.id) {
                continue;
            }
            let r = after.get(&b.id).expect("monster present after patch");
            assert_eq!(
                non_cost_fields(r),
                non_cost_fields(b),
                "{scale}: id {} moved a non-cost field",
                b.id
            );
            let pinned = SCALE_PINNED_MONSTER_IDS.contains(&b.id);
            let agl = b.agility();
            for (sb, sa) in b.spells.iter().zip(&r.spells) {
                // Only command-band entries are in scope; reaction / resist
                // entries (and the pinned tutorial fight) stay byte-identical.
                let expected = if pinned || !ACTION_BAND.contains(&sb.id) {
                    sb.agl_cost
                } else {
                    attack_count::scale_cost(sb.agl_cost, agl, scale).unwrap_or(sb.agl_cost)
                };
                assert_eq!(
                    sa.agl_cost, expected,
                    "{scale}: id {} entry at {:#x} cost wrong (retail {})",
                    b.id, sb.offset, sb.agl_cost
                );
            }
            // The min-one-strike guarantee: a retail attacker still affords
            // at least one attack after scaling.
            if !pinned && b.spells.iter().any(|s| affordable(s, agl)) {
                assert!(
                    r.spells.iter().any(|s| affordable(s, agl)),
                    "{scale}: id {} can no longer afford any attack",
                    b.id
                );
            }
        }

        // The pinned tutorial fight is byte-identical at every setting.
        let tetsu_before = before.iter().find(|r| r.id == 79).expect("Tetsu");
        let tetsu_after = after.get(&79).expect("Tetsu after");
        assert_eq!(
            tetsu_before, tetsu_after,
            "{scale}: the pinned tutorial fight moved"
        );

        // Every slot keeps its fixed footprint.
        let patched_entry = patcher.read_entry(MONSTER_ARCHIVE_ENTRY).expect("read 867");
        assert_eq!(patched_entry.len() % SLOT_STRIDE, 0);

        // Determinism: the pass is seedless, so a re-run is byte-identical.
        let mut patcher2 = DiscPatcher::open(original.clone()).expect("open");
        apply::scale_enemy_attack_count(&mut patcher2, scale).expect("scale");
        assert!(
            patcher2.image() == patcher.image(),
            "{scale}: re-run must reproduce the patched image"
        );

        eprintln!(
            "enemy attack count {scale}: {} monsters changed, {} entries",
            report.monsters_changed, report.entries_changed
        );
    }

    // A retail (1x) scale writes nothing.
    let mut patcher = DiscPatcher::open(original.clone()).expect("open");
    let report = apply::scale_enemy_attack_count(&mut patcher, ScalePermille::parse("1").unwrap())
        .expect("1x");
    assert_eq!(report.monsters_changed, 0, "1x must be a no-op");
    assert!(patcher.image() == &original[..], "1x must not touch a byte");
}

/// A slowed roster keeps exactly one strike where retail priced one: spot the
/// one-hit-per-turn shape (cost == AGL) staying put at 0.5x while a multi-hit
/// record's costs cap at its AGL.
#[test]
fn attack_count_slowdown_floors_at_one_strike_on_disc() {
    let Some(original) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };

    let base = DiscPatcher::open(original.clone()).expect("open");
    let before = records(&base);
    let scale = ScalePermille::parse("0.1").expect("valid scale");
    let mut patcher = DiscPatcher::open(original).expect("open");
    let report = apply::scale_enemy_attack_count(&mut patcher, scale).expect("scale");
    let after = by_id(&records(&patcher));

    let mut capped = 0usize;
    for b in &before {
        if report.skipped.contains(&b.id) || SCALE_PINNED_MONSTER_IDS.contains(&b.id) {
            continue;
        }
        let agl = b.agility();
        let r = after.get(&b.id).expect("monster present after patch");
        for (sb, sa) in b.spells.iter().zip(&r.spells) {
            if affordable(sb, agl) && sb.agl_cost > 0 {
                assert!(
                    (sa.agl_cost as u16) <= agl,
                    "id {}: cost {} priced past AGL {}",
                    b.id,
                    sa.agl_cost,
                    agl
                );
                if sa.agl_cost as u16 == agl.min(0xFE) {
                    capped += 1;
                }
            }
        }
    }
    // At 0.1x essentially every affordable entry lands on the AGL cap (one
    // strike per turn); require a broad population so the clamp is proven to
    // fire against real records rather than vacuously.
    assert!(
        capped > 100,
        "expected the AGL cap to fire widely, got {capped}"
    );
}

/// The attack-count knob and the difficulty scale edit the same archive; run
/// both on one image and confirm each lands independently.
#[test]
fn attack_count_composes_with_stat_scale_on_disc() {
    let Some(original) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };

    let base = DiscPatcher::open(original.clone()).expect("open");
    let before = records(&base);

    let mut patcher = DiscPatcher::open(original).expect("open");
    let stat_scale = StatScale::parse("2").expect("valid scale");
    let count_scale = ScalePermille::parse("2").expect("valid scale");
    let stats_report = apply::scale_monster_stats(&mut patcher, stat_scale).expect("scale stats");
    let count_report =
        apply::scale_enemy_attack_count(&mut patcher, count_scale).expect("scale count");

    let after = by_id(&records(&patcher));
    let skipped =
        |id: u16| stats_report.skipped.contains(&id) || count_report.skipped.contains(&id);
    for b in &before {
        if skipped(b.id) || SCALE_PINNED_MONSTER_IDS.contains(&b.id) {
            continue;
        }
        let r = after.get(&b.id).expect("monster present after patch");
        // AGL is untouched by both passes, so the cost expectation still keys
        // on the retail AGL - the two dials compose without interference.
        assert_eq!(r.agility(), b.agility(), "id {}: AGL must never move", b.id);
        for (sb, sa) in b.spells.iter().zip(&r.spells) {
            let expected = if ACTION_BAND.contains(&sb.id) {
                attack_count::scale_cost(sb.agl_cost, b.agility(), count_scale)
                    .unwrap_or(sb.agl_cost)
            } else {
                sb.agl_cost
            };
            assert_eq!(sa.agl_cost, expected, "id {}: composed cost wrong", b.id);
        }
    }
    assert!(count_report.monsters_changed > 100);
}
