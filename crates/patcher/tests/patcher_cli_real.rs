//! Disc-gated end-to-end test for the top-level randomizer orchestration:
//! enumerate drops -> plan from a seed -> apply to a scratch disc -> diff into a
//! PPF -> apply the PPF to a fresh copy and confirm it reproduces the patched
//! image. Gates on `LEGAIA_DISC_BIN`; skips + passes when unset. Writes nothing
//! to disk - the patched image lives only in memory.

use legaia_patcher::apply::{self, DropApplyReport};
use legaia_patcher::disc::DiscPatcher;
use legaia_patcher::drops::{DropAssignment, DropMode};
use legaia_patcher::ppf;

fn load_disc() -> Option<Vec<u8>> {
    let p = std::path::PathBuf::from(std::env::var_os("LEGAIA_DISC_BIN")?);
    p.is_file().then(|| std::fs::read(&p).ok()).flatten()
}

fn run_shuffle(original: &[u8], seed: u64) -> (Vec<u8>, Vec<DropAssignment>, DropApplyReport) {
    let mut patcher = DiscPatcher::open(original.to_vec()).expect("open disc");
    let (plan, report) =
        apply::randomize_drops(&mut patcher, &[], seed, DropMode::Shuffle).expect("randomize");
    (patcher.into_image(), plan, report)
}

#[test]
fn shuffle_drops_ppf_round_trips_and_is_deterministic() {
    let Some(original) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };

    let seed = 0x1234_5678_9ABC_DEF0;
    let (patched, plan, report) = run_shuffle(&original, seed);

    // Same length (every edit is in-place / same-size).
    assert_eq!(patched.len(), original.len(), "patch must not resize image");
    assert!(
        report.changed > 0,
        "shuffle should change at least one slot"
    );

    // Drops actually moved: the monster archive decodes differently.
    let before = apply::current_drops(&DiscPatcher::open(original.clone()).unwrap()).unwrap();
    let after = apply::current_drops(&DiscPatcher::open(patched.clone()).unwrap()).unwrap();
    assert_eq!(before.len(), after.len());
    let moved = before
        .iter()
        .zip(&after)
        .filter(|(b, a)| (b.item, b.chance) != (a.item, a.chance))
        .count();
    assert!(moved > 0, "at least one monster's drop should change");

    // Each non-skipped monster reads back exactly its planned drop; each
    // skipped (too-full) monster keeps its original drop unchanged. This is the
    // precise invariant - under skips the global multiset isn't preserved (the
    // skipped slot rejects its assigned pair while donating its own twice).
    let plan_for = |id: u16| plan.iter().find(|a| a.monster_id == id).copied();
    for b in &before {
        let a = after.iter().find(|d| d.monster_id == b.monster_id).unwrap();
        match plan_for(b.monster_id) {
            Some(p) if !report.skipped.contains(&b.monster_id) => {
                assert_eq!(
                    (a.item, a.chance),
                    (p.item, p.chance),
                    "monster {} should read its planned drop",
                    b.monster_id
                );
            }
            // skipped, or no assignment (non-dropper): unchanged.
            _ => assert_eq!(
                (a.item, a.chance),
                (b.item, b.chance),
                "monster {} should be unchanged",
                b.monster_id
            ),
        }
    }

    // Every planned pair is drawn from the original dropper multiset (shuffle
    // only redistributes existing drops).
    let mut orig_pairs: Vec<(u8, u8)> = before
        .iter()
        .filter(|d| d.item != 0)
        .map(|d| (d.item, d.chance))
        .collect();
    let mut plan_pairs: Vec<(u8, u8)> = plan.iter().map(|a| (a.item, a.chance)).collect();
    orig_pairs.sort_unstable();
    plan_pairs.sort_unstable();
    assert_eq!(
        orig_pairs, plan_pairs,
        "shuffle plan must be a permutation of the original dropper pairs"
    );

    // PPF round-trip: diff -> write -> apply to a fresh original reproduces it.
    let runs = ppf::diff_runs(&original, &patched);
    assert!(!runs.is_empty());
    let ppf_bytes = ppf::write_ppf3("test", &runs);
    let mut rebuilt = original.clone();
    let applied = ppf::apply_ppf3(&mut rebuilt, &ppf_bytes).unwrap();
    assert_eq!(applied, runs.len());
    assert!(
        rebuilt == patched,
        "PPF apply must reproduce the patched image"
    );

    // Determinism: the same seed yields a byte-identical patched image.
    let (patched2, _plan2, report2) = run_shuffle(&original, seed);
    assert!(patched2 == patched, "same seed -> identical patched image");
    assert_eq!(report2.skipped, report.skipped);

    eprintln!(
        "shuffle seed {seed:#x}: {} slots changed, {} skipped, PPF {} records",
        report.changed,
        report.skipped.len(),
        runs.len()
    );
}
