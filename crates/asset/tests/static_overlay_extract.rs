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

/// Every scanned `lui`+`addiu` pair's target, for a blob placed at `base_va`.
///
/// Mirrors `static_overlay::pointer_resolution`'s scan - only `lui` halves
/// equal to the base's own two high halfwords are considered, so nothing in
/// `SCUS_942.54`'s `0x8007xxxx` band is ever seen - but returns the addresses
/// instead of a hit count, because the verdict below depends on *where* a miss
/// lands.
fn self_pointer_targets(image: &[u8], base_va: u32) -> Vec<u32> {
    let hi0 = (base_va & 0xFFFF_0000) >> 16;
    let words = image.len() / 4;
    let mut out = Vec::new();
    for i in 0..words {
        let w1 = u32::from_le_bytes(image[i * 4..i * 4 + 4].try_into().unwrap());
        if w1 >> 26 != 0x0F {
            continue; // not lui
        }
        let rt = (w1 >> 16) & 0x1F;
        let hi = w1 & 0xFFFF;
        if hi != hi0 && hi != hi0 + 1 {
            continue;
        }
        for j in (i + 1)..(i + 7).min(words) {
            let w2 = u32::from_le_bytes(image[j * 4..j * 4 + 4].try_into().unwrap());
            if w2 >> 26 == 0x09 && (w2 >> 21) & 0x1F == rt {
                let lo = (w2 & 0xFFFF) as i16 as i32;
                out.push(((hi << 16) as i32).wrapping_add(lo) as u32);
                break;
            }
        }
    }
    out
}

/// `(resolved, counted)` for the slot-B base cross-check.
///
/// The predicate this replaces counted **every** reference that left the image
/// against the base, which is one-sided: a cast module legitimately reaches two
/// places outside its own bytes, and neither is evidence about where it loads.
///
/// * Below the slot-B base, inside a committed **slot-A** image's span - the
///   host battle overlay's own globals. PROT 0915's seven misses are
///   `0x801F6978` / `0x801F6980`, inside PROT 0898.
/// * At or above the image's end but still inside the slot-B **buffer** - the
///   post-image working storage a PSX overlay reaches past its loaded bytes,
///   `.bss`-shaped and by definition not in the file. PROT 0935's eight misses
///   are `0x801FA320..0x801FA3B8`; PROT 0926's two are `0x801F7D3C` /
///   `0x801F7F2C`. The buffer bound is taken from the map itself - the longest
///   committed slot-B image - not from a chosen constant.
///
/// Both are excluded from the measurement rather than credited to it, so the
/// ratio stays a statement about self-references and gets *stricter*: with the
/// noise gone the acceptance floor rises from 0.60 to 0.90.
fn slot_b_pointer_evidence(
    image: &[u8],
    base_va: u32,
    slot_a_spans: &[(u32, u32)],
    slot_b_buffer: u32,
) -> (u32, u32) {
    let end = base_va.wrapping_add(image.len() as u32);
    let buffer_end = base_va.wrapping_add(slot_b_buffer);
    let mut resolved = 0u32;
    let mut counted = 0u32;
    for a in self_pointer_targets(image, base_va) {
        if (base_va..end).contains(&a) {
            resolved += 1;
            counted += 1;
        } else if (end..buffer_end).contains(&a) {
            // post-image scratch in the shared slot-B buffer
        } else if a < base_va && slot_a_spans.iter().any(|&(lo, hi)| (lo..hi).contains(&a)) {
            // the host slot-A overlay's own data
        } else {
            counted += 1;
        }
    }
    (resolved, counted)
}

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

    // Pre-pass: every row's as-loaded length, so the checks below can ask
    // "does this address land inside another mapped image?" rather than
    // "does it leave this one?". `content_bytes` is not on `OverlayRecord`
    // (only the Python instruments read it), and for a `raw` row the
    // as-loaded length IS the entry extent, so it is measured here.
    let mut slot_a_spans: Vec<(u32, u32)> = Vec::new();
    let mut slot_b_buffer = 0u32;
    {
        let mut raw = Vec::new();
        for rec in &map.overlays {
            let Some(entry) = archive
                .entries
                .iter()
                .find(|e| e.index == rec.prot_index)
                .cloned()
            else {
                continue;
            };
            archive.read_entry(&entry, &mut raw).expect("read entry");
            let Ok(as_loaded) = static_overlay::as_loaded(&raw, rec) else {
                continue;
            };
            let len = as_loaded.len() as u32;
            if rec.base_va == static_overlay::SLOT_A_BASE {
                slot_a_spans.push((rec.base_va, rec.base_va.wrapping_add(len)));
            } else if rec.base_va == legaia_asset::summon_overlay::SUMMON_OVERLAY_LINK_BASE {
                slot_b_buffer = slot_b_buffer.max(len);
            }
        }
    }
    assert!(
        !slot_a_spans.is_empty() && slot_b_buffer > 0,
        "the map must carry both slots for the slot-B cross-check to mean anything"
    );

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
        //
        // Both halves of that measurement are bounded by the overlay's own
        // sectors, and two things follow from reading only those.
        //
        // The sample shrinks. Some slot-B images are a few sectors: PROT 0901
        // carries three pairs. It used to clear a flat `>= 8` only because the
        // reader ran past the entry and counted a *neighbouring* overlay's
        // code. A thin sample is held to a stricter bar rather than a lower
        // one: every pair must resolve.
        //
        // The acceptance range shrinks too, and that is what moves the
        // fraction. The pairs an image does not resolve in-file are not noise
        // - for PROT 0924 and 0967 they cluster in `0x801F99xx..0x801FA4xx`,
        // just above each image's end: the working storage a PSX overlay
        // reaches past its loaded image inside the slot-B buffer, which is
        // `.bss`-shaped and by definition not in the file. An over-read window
        // swallowed those addresses and scored them as hits.
        if rec.base_va == legaia_asset::summon_overlay::SUMMON_OVERLAY_LINK_BASE {
            let scanned = self_pointer_targets(&as_loaded, rec.base_va).len();
            assert!(
                scanned >= 3,
                "{} (PROT {}): too few self-pointers to confirm base ({scanned})",
                rec.label,
                rec.prot_index
            );
            let (resolved, counted) =
                slot_b_pointer_evidence(&as_loaded, rec.base_va, &slot_a_spans, slot_b_buffer);
            let frac = if counted == 0 {
                1.0
            } else {
                resolved as f64 / counted as f64
            };
            let want = if counted >= 8 { 0.90 } else { 1.0 };
            assert!(
                frac >= want,
                "{} (PROT {}): only {resolved}/{counted} evidential self-pointers resolve \
                 in-file at base 0x{:08x} ({scanned} scanned)",
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
