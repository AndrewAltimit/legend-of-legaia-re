//! Disc-gated integration test that drives `scripts/engine/scenarios.toml`
//! through `BootSession::open` and asserts the recorded SHA-256 of every
//! scenario's `SaveFile` byte stream still matches.
//!
//! Skips silently when `extracted/` or `LEGAIA_DISC_BIN` is missing -
//! same convention as the rest of the disc-gated suite. Drift is a hard
//! failure once an `extracted/` tree is present.
//!
//! Unblessed scenarios (empty `expected_save_sha256`) print the observed
//! hash and skip the assertion - the runner / bin enforces blessing
//! at PR time, not in the test loop.

use std::path::PathBuf;

use legaia_engine_shell::scenarios::{ScenariosManifest, run_all};

fn extracted_dir() -> Option<PathBuf> {
    let candidates = ["extracted", "../extracted", "../../extracted"];
    for c in candidates {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

fn manifest_path() -> Option<PathBuf> {
    let candidates = [
        "scripts/engine/scenarios.toml",
        "../scripts/engine/scenarios.toml",
        "../../scripts/engine/scenarios.toml",
    ];
    for c in candidates {
        let p = PathBuf::from(c);
        if p.exists() {
            return Some(p);
        }
    }
    None
}

#[test]
fn engine_scenarios_match_manifest() {
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing");
        return;
    };
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (matches disc-gated convention)");
        return;
    }
    let Some(manifest_path) = manifest_path() else {
        eprintln!("[skip] scripts/engine/scenarios.toml not found");
        return;
    };

    let manifest =
        ScenariosManifest::from_toml_path(&manifest_path).expect("parse engine scenarios manifest");
    if manifest.scenarios.is_empty() {
        eprintln!("[skip] manifest has zero scenarios");
        return;
    }

    let results = run_all(&manifest, &extracted).expect("run scenarios");
    let mut drifted = Vec::new();
    let mut unblessed = Vec::new();
    for r in &results {
        match (&r.expected_sha256, r.passed()) {
            (None, _) => {
                eprintln!(
                    "[unblessed] {:<32} scene={:<8} frames={:>3} observed={}",
                    r.name, r.scene, r.frames, r.observed_sha256
                );
                unblessed.push(r.name.clone());
            }
            (Some(_), true) => {
                eprintln!(
                    "[ok]        {:<32} scene={:<8} frames={:>3} hash={}",
                    r.name, r.scene, r.frames, r.observed_sha256
                );
            }
            (Some(exp), false) => {
                eprintln!(
                    "[DRIFT]     {} scene={} frames={}",
                    r.name, r.scene, r.frames
                );
                eprintln!("            expected: {exp}");
                eprintln!("            observed: {}", r.observed_sha256);
                drifted.push(r.name.clone());
            }
        }
    }
    assert!(
        drifted.is_empty(),
        "engine scenarios drifted: {:?}",
        drifted
    );
    if !unblessed.is_empty() {
        eprintln!(
            "warn: {} unblessed scenario(s); run `legaia-engine scenarios --bless` and review",
            unblessed.len()
        );
    }
}
