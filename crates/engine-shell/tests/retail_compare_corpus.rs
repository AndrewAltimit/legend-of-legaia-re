//! The retail comparison corpus as a ratchet: seed the engine from every
//! seedable library state, score it per channel against what retail's RAM /
//! VRAM held, and fail if any state's channel drops below the committed
//! baseline (`scripts/ci/retail-compare-baseline.json`). A score may rise in
//! a reviewed edit (`scripts/ci/retail-compare.py --bless`); it may not fall.
//!
//! The image channel needs a display (the frame comes from `play-window`),
//! so it runs only with `LEGAIA_RETAIL_COMPARE_IMAGES=1`; without it the
//! baseline's image scores are skipped, never failed.
//!
//! Skips (passes) unless `LEGAIA_DISC_BIN` is set and the save library and
//! extracted disc are found (`LEGAIA_SAVES_LIBRARY` / `LEGAIA_EXTRACTED_DIR`
//! first, then repo-relative). See `docs/tooling/retail-compare.md`.

use std::path::{Path, PathBuf};

use legaia_engine_shell::retail_compare::{
    Baseline, RunOptions, SeedOrder, resolve_dirs, run_corpus, summarise,
};

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[test]
fn the_retail_comparison_corpus_holds_its_baseline() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let (_, library, extracted) = resolve_dirs();
    let (Some(library), Some(extracted)) = (library, extracted) else {
        eprintln!("[skip] save library or extracted disc not found");
        return;
    };
    let manifest_path = repo().join("scripts/scenarios.toml");
    let manifest =
        legaia_mednafen::ScenarioManifest::from_path(&manifest_path).expect("scenario manifest");
    let images = std::env::var_os("LEGAIA_RETAIL_COMPARE_IMAGES").is_some();
    let exe = PathBuf::from(env!("CARGO_BIN_EXE_legaia-engine"));
    let out = std::env::temp_dir().join("legaia-retail-compare-test");
    let reports = run_corpus(&RunOptions {
        extracted: &extracted,
        library: &library,
        manifest: &manifest,
        engine_exe: images.then_some(exe.as_path()),
        out_dir: Some(&out),
        filter: None,
        order: SeedOrder::Resume,
    })
    .expect("run corpus");
    let summary = summarise(&reports);
    eprintln!(
        "[retail-compare] {} states, {} seeded, {} seed failures, mean state score {:.3}",
        summary.states, summary.seeded, summary.seed_failed, summary.mean_state_score
    );
    for (ch, (mean, n)) in &summary.channels {
        eprintln!("[retail-compare]   {ch:<10} {mean:.3} over {n}");
    }
    assert!(
        summary.seeded > 0,
        "no state was seeded - the corpus is vacuous"
    );
    assert_eq!(summary.seed_failed, 0, "a seedable state failed to seed");

    let baseline_path = repo().join("scripts/ci/retail-compare-baseline.json");
    let baseline: Baseline = serde_json::from_str(
        &std::fs::read_to_string(&baseline_path).expect("read the committed baseline"),
    )
    .expect("parse the baseline");
    let allow: &[&str] = if images { &[] } else { &["image"] };
    let regressions = baseline.regressions(&reports, allow);
    for r in &regressions {
        eprintln!("[regression] {r}");
    }
    assert!(
        regressions.is_empty(),
        "{} per-state channel score(s) fell below the baseline",
        regressions.len()
    );
}
