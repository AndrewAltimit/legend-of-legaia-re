//! Cross-validate the curated `arts.toml` against the executable.
//!
//! The SCUS arts-name table (`DAT_80075EC4`, decoded by
//! `legaia_art::arts_table`) is the **ground truth** for each Tactical Art's
//! AP cost and directional command sequence. This test pins the curated
//! `data/gamedata/arts.toml` `ap` + `directions` columns against it: for every
//! art present in both, AP must match exactly and the directions must match
//! the executable's command sequence, except for a small documented set of
//! known walkthrough errors.
//!
//! Skips and passes when `extracted/SCUS_942.54` isn't on disk - same gating
//! pattern as the other disc-dependent tests, so CI needs no Sony bytes.

use legaia_art::ArtsOracle;
use legaia_gamedata::Database;
use std::path::PathBuf;

fn scus_path() -> Option<PathBuf> {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace = manifest.parent()?.parent()?;
    let p = workspace.join("extracted").join("SCUS_942.54");
    p.is_file().then_some(p)
}

/// Known divergences between the curated walkthrough data and the executable.
/// Each entry is `(art name, gamedata directions, SCUS commands)`. The
/// executable is authoritative; these document where the public walkthrough
/// the gamedata tables were mined from disagrees. The test fails if a *new*
/// undocumented divergence appears, or if a documented one silently changes.
const KNOWN_DIVERGENCES: &[(&str, &[u8], &[u8])] = &[
    // Vahn's Hyper Elbow: walkthrough lists the third input as "High" (Up=4),
    // but the executable's command glyph is Left (1). See
    // docs/formats/art-data.md#command-glyph-string.
    ("Hyper Elbow", &[1, 2, 4], &[1, 2, 1]),
];

fn divergence_for(name: &str) -> Option<&'static (&'static str, &'static [u8], &'static [u8])> {
    KNOWN_DIVERGENCES
        .iter()
        .find(|(n, _, _)| n.eq_ignore_ascii_case(name))
}

#[test]
fn arts_toml_ap_and_directions_match_scus_or_skips() {
    let Some(path) = scus_path() else {
        eprintln!("extracted/SCUS_942.54 not present - skipping");
        return;
    };
    let bytes = std::fs::read(&path).expect("read SCUS");
    let oracle = ArtsOracle::from_scus(&bytes).expect("build arts oracle");
    let gd = Database::load();

    let mut matched = 0usize;
    let mut divergences_seen = 0usize;

    for art in gd.arts() {
        let Some(entry) = oracle.by_name(&art.name) else {
            // Not every curated art has a row in this SCUS table (the table is
            // names + AP + command only); names that don't resolve are skipped.
            continue;
        };
        matched += 1;

        // AP is byte-exact ground truth.
        assert_eq!(
            entry.ap as u32, art.ap,
            "AP mismatch for {}: SCUS {} vs gamedata {}",
            art.name, entry.ap, art.ap
        );

        let scus_dirs: Vec<u8> = entry.commands.iter().map(|c| *c as u8).collect();
        if scus_dirs == art.directions {
            continue;
        }

        // A mismatch must be one we already understand.
        let div = divergence_for(&art.name).unwrap_or_else(|| {
            panic!(
                "undocumented arts.toml divergence for {}: gamedata {:?} vs SCUS {:?}",
                art.name, art.directions, scus_dirs
            )
        });
        assert_eq!(
            art.directions, div.1,
            "gamedata directions for {} changed; update KNOWN_DIVERGENCES",
            art.name
        );
        assert_eq!(
            scus_dirs, div.2,
            "SCUS commands for {} changed; update KNOWN_DIVERGENCES",
            art.name
        );
        divergences_seen += 1;
    }

    // The two tables overlap substantially; guard against a silent loader
    // regression that would make every name-lookup miss.
    assert!(
        matched >= 40,
        "expected to cross-check most arts, only matched {matched}"
    );
    // Every documented divergence we expect to still be present should have
    // fired (the test would otherwise be quietly stale).
    assert_eq!(
        divergences_seen,
        KNOWN_DIVERGENCES.len(),
        "a documented divergence no longer appears - update KNOWN_DIVERGENCES"
    );
}
