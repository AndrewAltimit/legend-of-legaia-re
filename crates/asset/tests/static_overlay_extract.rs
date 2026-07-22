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

        // (2) Static base recovery agrees with the committed base - but only
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

        // (3) If the row pins a known function VA, it must land on a prologue at
        // the committed base - a capture-free base cross-check that keeps the
        // base claim non-vacuous even for rows whose base did not come from
        // jal-recovery (the slot-A minigame siblings sourced by doc-fn anchor).
        if let Some(anchor) = rec.anchor_va {
            assert!(
                static_overlay::anchor_lands_on_prologue(&as_loaded, anchor, rec.base_va),
                "{} (PROT {}): anchor 0x{:08x} is not a prologue at base 0x{:08x}",
                rec.label,
                rec.prot_index,
                anchor,
                rec.base_va
            );
        }

        // (4) Slot-B rows (summon link base) have too sparse a jal graph to
        // triangulate, so their base is cross-referenced. Cross-check it the
        // slot-B way: a high fraction of the overlay's internal absolute
        // self-pointers must resolve in-file at the committed base. This keeps
        // the cross_ref/capture base claims non-vacuous.
        if rec.base_va == legaia_asset::summon_overlay::SUMMON_OVERLAY_LINK_BASE {
            let (resolved, total) = static_overlay::pointer_resolution(&as_loaded, rec.base_va);
            assert!(
                total >= 8,
                "{} (PROT {}): too few self-pointers to confirm base ({total})",
                rec.label,
                rec.prot_index
            );
            let frac = resolved as f64 / total as f64;
            assert!(
                frac >= 0.70,
                "{} (PROT {}): only {resolved}/{total} self-pointers resolve in-file at base 0x{:08x}",
                rec.label,
                rec.prot_index,
                rec.base_va
            );
        }

        // (5) Two-slot string-anchor cross-check for rows WITHOUT a prologue
        // anchor. pointer_resolution alone is one-sided: an overlay that
        // densely references fixed structures in the RIVAL slot's VA band can
        // score high at a base it never loads to (the falsified
        // 0902-as-slot-B row). A pointer that decodes to the start of one of
        // the file's own string literals only does so at the true base - with
        // one caveat: references to a CO-RESIDENT overlay's head string table
        // alias onto this file's own head strings when both keep strings at
        // matching small offsets (PROT 0977's calls into the slot-B
        // field-back-read module's dev strings do exactly that), so a pinned
        // prologue anchor (check 3) outranks the raw vote count and exempts
        // the row here.
        if rec.anchor_va.is_none()
            && (rec.base_va == legaia_asset::summon_overlay::SUMMON_OVERLAY_LINK_BASE
                || rec.base_va == static_overlay::SLOT_A_BASE)
        {
            let rival = if rec.base_va == static_overlay::SLOT_A_BASE {
                legaia_asset::summon_overlay::SUMMON_OVERLAY_LINK_BASE
            } else {
                static_overlay::SLOT_A_BASE
            };
            let own = static_overlay::string_anchor_votes(&as_loaded, rec.base_va);
            let rival_votes = static_overlay::string_anchor_votes(&as_loaded, rival);
            assert!(
                own >= rival_votes,
                "{} (PROT {}): string anchors favour the RIVAL slot base \
                 ({own} at committed 0x{:08x} vs {rival_votes} at 0x{rival:08x}) - \
                 the committed slot is falsified",
                rec.label,
                rec.prot_index,
                rec.base_va
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
