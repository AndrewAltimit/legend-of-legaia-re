//! Disc-gated reproducibility for the static overlay-extraction pipeline.
//!
//! For every overlay in the committed map (`crates/asset/data/static-overlays.toml`),
//! re-extract its as-loaded bytes from the user's `PROT.DAT` and assert:
//!
//! 1. The bytes hash to the committed `fingerprint_sha256` (the extraction is
//!    bit-reproducible from any copy of the disc -- no Sony bytes committed,
//!    just the hash).
//! 2. The base statically recovered from the overlay's own internal `jal` call
//!    graph matches the committed `base_va` (identity + base come from the disc,
//!    not a guessed label).
//!
//! This is the foundation the whole pipeline rests on: it proves the overlay is
//! a clean copy that any user can reproduce from their disc. The runtime
//! byte-match against a resident RAM image lives in
//! `crates/mednafen/tests/static_overlay_clean_copy.rs`.
//!
//! Skips + passes when `LEGAIA_DISC_BIN` is unset or `extracted/PROT.DAT` is
//! absent (the disc-gated convention -- CI runs without disc data).

use std::path::PathBuf;

use legaia_asset::static_overlay::{self, BaseSource, Eligibility, OverlayForm};
use legaia_prot::archive::Archive;

fn prot_dat() -> Option<PathBuf> {
    std::env::var_os("LEGAIA_DISC_BIN")?;
    for p in ["extracted/PROT.DAT", "../../extracted/PROT.DAT"] {
        let f = PathBuf::from(p);
        if f.is_file() {
            return Some(f);
        }
    }
    None
}

#[test]
fn committed_overlays_reproduce_from_disc() {
    let Some(prot) = prot_dat() else {
        eprintln!("[skip] LEGAIA_DISC_BIN or extracted/PROT.DAT missing");
        return;
    };

    let map = static_overlay::overlay_map();
    assert!(!map.overlays.is_empty(), "map should not be empty");
    let mut archive = Archive::open(&prot).expect("open PROT.DAT");

    let mut checked = 0usize;
    for rec in &map.overlays {
        let entry = archive
            .entries
            .iter()
            .find(|e| e.index == rec.prot_index)
            .cloned()
            .unwrap_or_else(|| panic!("PROT entry {} not in archive", rec.prot_index));
        let mut raw = Vec::new();
        archive.read_entry(&entry, &mut raw).expect("read entry");
        let as_loaded = static_overlay::as_loaded(&raw, rec).expect("as-loaded form");

        // (1) Fingerprint reproduces.
        static_overlay::verify_fingerprint(rec, &as_loaded).unwrap_or_else(|e| panic!("{e}"));

        // (2) Static base recovery agrees with the committed base — but only
        // for rows whose base was sourced from jal-recovery. Timeshared-buffer
        // overlays (base_source = capture / cross_ref) have too sparse an
        // internal call graph to triangulate; their base comes from a capture
        // anchor or a cross-referenced RE result, and the fingerprint check
        // above keeps them non-vacuous.
        if rec.form == OverlayForm::Raw
            && rec.eligibility != Eligibility::Ineligible
            && rec.base_source == BaseSource::Jal
        {
            let recovered = static_overlay::recover_base(&as_loaded, 8).unwrap_or_else(|| {
                panic!(
                    "static base recovery found no consensus for {} (PROT {})",
                    rec.label, rec.prot_index
                )
            });
            assert_eq!(
                recovered.base_va, rec.base_va,
                "{} (PROT {}): recovered base 0x{:08x} != committed 0x{:08x} ({} corroborating targets)",
                rec.label, rec.prot_index, recovered.base_va, rec.base_va, recovered.votes
            );
            assert!(
                recovered.votes >= 8,
                "{}: weak base recovery ({} votes)",
                rec.label,
                recovered.votes
            );
        }
        checked += 1;
    }
    assert!(
        checked >= 2,
        "expected at least the field + battle overlays"
    );
    eprintln!("[ok] {checked} committed overlays reproduce + base-recover from disc");
}
