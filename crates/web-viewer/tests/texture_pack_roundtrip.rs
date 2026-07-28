//! Round-trip and verification guard for texture change packs.
//!
//! A pack is authored against one disc and run against another, so the whole
//! design rests on one claim: an entry either resolves to the texture it was
//! authored against, or says precisely why it does not. This test exercises
//! that against real disc coordinates - author a pack from a live scan,
//! serialize it, read it back, and grade every entry - and then the case that
//! matters most, a texture that has *already been patched* on the target
//! disc, which must be reported rather than silently replaced twice.
//!
//! Skipped (passes) when `LEGAIA_DISC_BIN` is unset, matching the rest of the
//! disc-dependent test suite. CI runs without disc data.

#![cfg(not(target_arch = "wasm32"))]

use std::env;
use std::fs;

use legaia_patcher::disc::DiscPatcher;
use legaia_patcher::texture::TextureTarget;
use legaia_tim::encode::EncodeOptions;
use legaia_web_viewer::texture_pack::{
    EntryStatus, PackEntry, PackMeta, from_json, to_json, verify,
};
use legaia_web_viewer::texture_registry::{self as reg, Rgba, ScanCtx, TIER_RAW, TexRow};

fn disc() -> Option<Vec<u8>> {
    let path = env::var("LEGAIA_DISC_BIN").ok()?;
    if path.is_empty() {
        return None;
    }
    fs::read(path).ok()
}

fn prot_and_spans(image: &[u8]) -> (Vec<u8>, Vec<(u64, u64, u32)>) {
    let prot = legaia_iso::iso9660::read_file_in_image(image, "PROT.DAT").expect("PROT.DAT");
    let archive = legaia_prot::archive::Archive::from_bytes(prot.clone()).expect("TOC");
    let spans = archive
        .entries
        .iter()
        .map(|e| (e.byte_offset, e.size_bytes, e.index))
        .collect();
    (prot, spans)
}

/// A handful of real rows spread across every family the registry lists.
fn sample_rows(prot: &[u8], spans: &[(u64, u64, u32)], per_tier: usize) -> Vec<TexRow> {
    let ctx = ScanCtx::new(prot, spans);
    let mut out: Vec<TexRow> = Vec::new();
    let mut sink = |row: TexRow, _rgba: Option<Rgba>| -> Result<(), String> {
        if out
            .iter()
            .filter(|r| r.coord.tier == row.coord.tier)
            .count()
            < per_tier
        {
            out.push(row);
        }
        Ok(())
    };
    reg::scan_all(&ctx, false, &mut sink).expect("scan");
    out
}

/// A minimal but genuine PNG of `w`x`h`, written through the project's own
/// encoder so the bytes are what a browser would hand us.
fn png_of(w: usize, h: usize, rgba: &[u8]) -> Vec<u8> {
    let dir = env::temp_dir().join(format!(
        "legaia-pack-rt-{}-{}",
        std::process::id(),
        w * 31 + h
    ));
    fs::create_dir_all(&dir).expect("temp dir");
    let path = dir.join("t.png");
    legaia_tim::write_png(&path, w, h, rgba).expect("write png");
    let bytes = fs::read(&path).expect("read png");
    let _ = fs::remove_file(&path);
    let _ = fs::remove_dir(&dir);
    bytes
}

fn entry_for(row: &TexRow, png: Vec<u8>) -> PackEntry {
    PackEntry {
        coord: row.coord,
        original_fnv1a: row.fnv1a,
        original_width: row.width,
        original_height: row.height,
        original_bpp: row.bpp,
        label: row.label.as_deref().unwrap_or("").to_string(),
        quantize: false,
        png,
    }
}

#[test]
fn a_pack_authored_from_a_scan_verifies_against_the_same_disc() {
    let Some(image) = disc() else {
        eprintln!("LEGAIA_DISC_BIN unset - skipping");
        return;
    };
    let (prot, spans) = prot_and_spans(&image);
    let rows = sample_rows(&prot, &spans, 3);
    assert!(rows.len() >= 6, "expected rows from several families");
    drop(prot);

    let entries: Vec<PackEntry> = rows.iter().map(|r| entry_for(r, Vec::new())).collect();
    let meta = PackMeta {
        name: "round trip".to_string(),
        author: "test".to_string(),
        note: String::new(),
    };

    // Serialize, hand the text back to the reader, and confirm nothing moved.
    let text = to_json(&meta, &entries);
    let back = from_json(&text).expect("re-reads its own output");
    assert_eq!(back.entries, entries);
    assert_eq!(back.meta, meta);

    // Every entry resolves on the disc it was authored from. Read-only
    // families cannot be pinned by a pack at all, which is itself the
    // reportable outcome rather than a panic.
    let patcher = DiscPatcher::open(image).expect("open disc");
    for e in &back.entries {
        let status = verify(&patcher, e);
        let replaceable = reg::tier(e.coord.tier).is_some_and(|t| t.replaceable);
        if replaceable {
            assert_eq!(
                status,
                EntryStatus::Ok,
                "{:?} should verify clean on its own disc",
                e.coord
            );
        } else {
            assert_eq!(
                status.tag(),
                "not-found",
                "{:?} is a read-only family, so a pack must refuse it",
                e.coord
            );
        }
    }
}

#[test]
fn a_wrong_fingerprint_or_size_is_reported_not_applied() {
    let Some(image) = disc() else {
        eprintln!("LEGAIA_DISC_BIN unset - skipping");
        return;
    };
    let (prot, spans) = prot_and_spans(&image);
    let row = sample_rows(&prot, &spans, 1)
        .into_iter()
        .find(|r| r.coord.tier == TIER_RAW)
        .expect("a raw-tier row");
    drop(prot);
    let patcher = DiscPatcher::open(image).expect("open disc");

    assert_eq!(
        verify(&patcher, &entry_for(&row, Vec::new())),
        EntryStatus::Ok
    );

    // Authored against different bytes at the same coordinate: a different
    // disc revision, or a texture already patched.
    let mut wrong_hash = entry_for(&row, Vec::new());
    wrong_hash.original_fnv1a ^= 1;
    match verify(&patcher, &wrong_hash) {
        EntryStatus::HashMismatch { expected, found } => {
            assert_eq!(expected, row.fnv1a ^ 1);
            assert_eq!(found, row.fnv1a);
        }
        other => panic!("expected a hash mismatch, got {other:?}"),
    }

    // Authored against a different size: the replacement could never fit.
    let mut wrong_size = entry_for(&row, Vec::new());
    wrong_size.original_width += 8;
    assert_eq!(verify(&patcher, &wrong_size).tag(), "size-mismatch");

    // A coordinate that is not a texture on this disc at all.
    let mut nowhere = entry_for(&row, Vec::new());
    nowhere.coord.offset += 3;
    assert_eq!(verify(&patcher, &nowhere).tag(), "not-found");
}

#[test]
fn re_importing_onto_an_already_patched_texture_is_caught() {
    let Some(image) = disc() else {
        eprintln!("LEGAIA_DISC_BIN unset - skipping");
        return;
    };
    let (prot, spans) = prot_and_spans(&image);
    // A raw-tier texture: written in place at the same size, so the apply
    // cannot fail for fit reasons and the test measures only the pack logic.
    let row = sample_rows(&prot, &spans, 1)
        .into_iter()
        .find(|r| r.coord.tier == TIER_RAW && r.width * r.height > 0)
        .expect("a raw-tier row");
    let ctx = ScanCtx::new(&prot, &spans);
    let original = reg::read_row(&ctx, &row.coord).expect("decode");
    drop(ctx);
    drop(prot);

    // Perturb one pixel so the replacement is a real change but still uses
    // only colours already in the texture's palette.
    let mut edited = original.data.clone();
    let last = edited.len() - 4;
    edited.copy_within(last..last + 4, 0);
    let png = png_of(original.w, original.h, &edited);

    let mut patcher = DiscPatcher::open(image).expect("open disc");
    let entry = entry_for(&row, png.clone());
    assert_eq!(verify(&patcher, &entry), EntryStatus::Ok, "clean before");

    let target = TextureTarget {
        entry: (row.coord.entry >= 0).then_some(row.coord.entry as u32),
        lzs_section: None,
        offset: row.coord.offset,
    };
    let (w, h, rgba) = legaia_tim::encode::decode_png_rgba(&png).expect("our own png");
    legaia_patcher::texture::replace_texture(
        &mut patcher,
        &target,
        &rgba,
        w,
        h,
        &EncodeOptions { quantize: true },
        false,
    )
    .expect("apply the replacement");

    // The same pack, run again on the now-patched disc. This is the case the
    // fingerprint exists for: without it the entry would re-apply silently on
    // top of the user's own edit.
    match verify(&patcher, &entry) {
        EntryStatus::HashMismatch { expected, found } => {
            assert_eq!(expected, row.fnv1a);
            assert_ne!(found, row.fnv1a, "the texture really did change");
        }
        other => panic!("expected the patched texture to be flagged, got {other:?}"),
    }
}
