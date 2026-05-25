//! Disc-gated regression for the deep PROT.DAT TIM catalog (TIMs recovered
//! from inside LZS-compressed sections).
//!
//! Rebuilds the deep catalog from the real `extracted/PROT.DAT` and asserts it
//! matches the committed reference (`tests/data/prot_tim_deep_catalog.tsv`)
//! byte-for-byte, plus the pinned count and rollup digest.
//!
//! ## What the pins mean
//!
//! Unlike the flat [`legaia_asset::tim_catalog`], the deep catalog has no
//! external oracle - the reference decoder that pins the flat catalog reads
//! only RAW bytes and never decompresses. So this guards two things instead:
//! the decode path (LZS container parse + decompress) and the validity gate
//! (`legaia_tim::parse_strict` + decode-to-RGBA on the decompressed bytes). A
//! drift in the LZS decoder, the strict TIM validator, or the scan would move
//! the digest and fail here. No decompressed Sony bytes (or TIM pixel data)
//! are committed; the reference holds only derived metadata + FNV fingerprints
//! of the decoded TIM bytes.
//!
//! Skips and passes when `extracted/PROT.DAT` is absent or `LEGAIA_DISC_BIN`
//! is unset, so CI runs disc-free.

use std::path::PathBuf;

use legaia_asset::tim_deep_catalog;

/// Number of standard PSX TIMs recovered from inside LZS-compressed sections
/// of the retail NA `PROT.DAT`. A stable invariant of the disc image.
const RETAIL_NA_DEEP_TIM_COUNT: usize = 3007;

/// FNV-1a-64 fold over every deep TIM's structural fields (see
/// [`tim_deep_catalog::rollup`]). Regenerate with `asset tim-deep-catalog
/// extracted/PROT.DAT --rollup` if the catalog legitimately changes.
const RETAIL_NA_DEEP_ROLLUP_DIGEST: u64 = 0xe9ea_e252_f627_1058;

fn prot_dat() -> Option<PathBuf> {
    for p in ["extracted", "../../extracted"] {
        let d = PathBuf::from(p).join("PROT.DAT");
        if d.exists() {
            return Some(d);
        }
    }
    None
}

fn reference_tsv_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/data/prot_tim_deep_catalog.tsv")
}

#[test]
fn deep_catalog_matches_pinned_reference() {
    let Some(prot) = prot_dat() else {
        eprintln!("[skip] extracted/PROT.DAT missing");
        return;
    };
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    }

    let catalog = tim_deep_catalog::build_from_path(&prot).expect("build deep TIM catalog");

    // Count: the disc holds exactly this many strict-valid compressed TIMs.
    assert_eq!(
        catalog.len(),
        RETAIL_NA_DEEP_TIM_COUNT,
        "deep TIM count drifted"
    );

    // Rollup digest: a single number guarding every deep TIM's key + dims +
    // clut + decoded-byte fingerprint.
    let r = tim_deep_catalog::rollup(&catalog);
    assert_eq!(r.count, RETAIL_NA_DEEP_TIM_COUNT);
    assert_eq!(
        r.digest, RETAIL_NA_DEEP_ROLLUP_DIGEST,
        "deep catalog rollup digest drifted (0x{:016x})",
        r.digest
    );

    // Byte-for-byte equality against the committed reference, so a drift shows
    // exactly which rows changed.
    let built = tim_deep_catalog::to_tsv(&catalog);
    let reference = std::fs::read_to_string(reference_tsv_path()).expect("read reference catalog");
    if built != reference {
        for (i, (a, b)) in built.lines().zip(reference.lines()).enumerate() {
            if a != b {
                panic!(
                    "deep catalog row {} differs:\n  built: {}\n  ref:   {}",
                    i, a, b
                );
            }
        }
        panic!(
            "deep catalog length differs: built {} lines, reference {} lines",
            built.lines().count(),
            reference.lines().count()
        );
    }
}

/// Every cataloged deep TIM must re-decode to RGBA without error - the same
/// gate the builder applies, re-asserted here so the committed reference can
/// never contain a row that would panic a consumer.
#[test]
fn deep_catalog_tims_all_decode() {
    let Some(prot) = prot_dat() else {
        eprintln!("[skip] extracted/PROT.DAT missing");
        return;
    };
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    }

    let mut archive = legaia_prot::archive::Archive::open(&prot).expect("open archive");
    let entries = archive.entries.clone();
    let mut buf = Vec::new();
    let mut decoded = 0usize;
    for entry in &entries {
        archive.read_entry(entry, &mut buf).expect("read entry");
        let Ok(sections) = legaia_lzs::decompress_container(&buf) else {
            continue;
        };
        for section in &sections {
            let mut off = 0usize;
            while off + 4 <= section.len() {
                let magic = u32::from_le_bytes(section[off..off + 4].try_into().unwrap());
                if magic == legaia_tim::TIM_MAGIC
                    && let Ok(tim) = legaia_tim::parse_strict(&section[off..])
                    && tim.pixel_width() > 0
                    && tim.pixel_height() > 0
                {
                    // Decode every palette, not just palette 0, so a bad CLUT
                    // row in any variant is caught.
                    for clut in 0..tim.palette_count().max(1) {
                        legaia_tim::decode_rgba8(&tim, clut)
                            .expect("cataloged deep TIM must decode");
                    }
                    decoded += 1;
                }
                off += 4;
            }
        }
    }
    assert!(decoded > 0, "expected to decode some deep TIMs");
}
