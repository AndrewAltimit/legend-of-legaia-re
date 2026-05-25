//! Disc-gated regression for the PROT.DAT TIM catalog.
//!
//! Rebuilds the catalog from the real `extracted/PROT.DAT` and asserts it
//! matches the committed reference (`tests/data/prot_tim_catalog.tsv`)
//! byte-for-byte, plus the pinned count and rollup digest.
//!
//! ## What the pins mean
//!
//! The reference catalog was cross-checked, item-for-item, against jPSXdec's
//! own index of the same PROT.DAT (`java -jar jpsxdec.jar -f PROT.DAT -x
//! index.idx`): all 1132 TIM items match on absolute offset, decoded
//! dimensions, bit depth, and palette count. So pinning the catalog also pins
//! jPSXdec parity - if our strict TIM validator (`legaia_tim::parse_strict`)
//! ever drifts from jPSXdec's TIM detector, the digest changes and this test
//! fails. No jPSXdec output (or any TIM pixel data) is committed; the
//! reference holds only derived metadata + FNV fingerprints.
//!
//! Skips and passes when `extracted/PROT.DAT` is absent or `LEGAIA_DISC_BIN`
//! is unset, so CI runs disc-free.

use std::path::PathBuf;

use legaia_asset::tim_catalog;

/// Number of standard PSX TIMs jPSXdec finds in the retail NA `PROT.DAT`. A
/// stable invariant of the disc image (like the PROT entry count), not a
/// project-progress count.
const RETAIL_NA_TIM_COUNT: usize = 1132;

/// FNV-1a-64 fold over every cataloged TIM's structural fields (see
/// [`tim_catalog::rollup`]). Regenerate with `asset tim-catalog
/// extracted/PROT.DAT --rollup` if the catalog legitimately changes.
const RETAIL_NA_ROLLUP_DIGEST: u64 = 0x2b67_9388_f526_7594;

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
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/data/prot_tim_catalog.tsv")
}

#[test]
fn catalog_matches_jpsxdec_pinned_reference() {
    let Some(prot) = prot_dat() else {
        eprintln!("[skip] extracted/PROT.DAT missing");
        return;
    };
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    }

    let catalog = tim_catalog::build_from_path(&prot).expect("build TIM catalog");

    // Count: the disc holds exactly the jPSXdec-equivalent set.
    assert_eq!(
        catalog.len(),
        RETAIL_NA_TIM_COUNT,
        "TIM count drifted from the jPSXdec-equivalent set"
    );

    // Rollup digest: a single number guarding every TIM's offset/dims/clut/fnv.
    let r = tim_catalog::rollup(&catalog);
    assert_eq!(r.count, RETAIL_NA_TIM_COUNT);
    assert_eq!(
        r.digest, RETAIL_NA_ROLLUP_DIGEST,
        "catalog rollup digest drifted (0x{:016x})",
        r.digest
    );

    // Byte-for-byte equality against the committed reference, so a drift
    // shows exactly which rows changed.
    let built = tim_catalog::to_tsv(&catalog);
    let reference = std::fs::read_to_string(reference_tsv_path()).expect("read reference catalog");
    if built != reference {
        // Surface the first differing line to make a failure actionable.
        for (i, (a, b)) in built.lines().zip(reference.lines()).enumerate() {
            if a != b {
                panic!("catalog row {} differs:\n  built: {}\n  ref:   {}", i, a, b);
            }
        }
        panic!(
            "catalog length differs: built {} lines, reference {} lines",
            built.lines().count(),
            reference.lines().count()
        );
    }
}

/// The byte-exact reverse-engineered pins ([`legaia_asset::tim_labels`])
/// resolve to the expected catalog ids. These textures sit at fixed offsets on
/// the retail NA image (a stable invariant), so the ids are safe to assert.
/// Guards the fingerprint table against a catalog-fingerprint or table drift.
#[test]
fn pinned_textures_carry_their_label() {
    let Some(prot) = prot_dat() else {
        eprintln!("[skip] extracted/PROT.DAT missing");
        return;
    };
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    }

    let catalog = tim_catalog::build_from_path(&prot).expect("build TIM catalog");
    let label = |id: usize| catalog[id].label;

    assert_eq!(label(8), Some("menu glyph atlas"));
    assert_eq!(label(1083), Some("main-title sprite sheet"));
    assert_eq!(label(1), Some("load-screen UI sheet"));
    assert_eq!(label(10), Some("load-screen portrait"));
    assert_eq!(label(11), Some("load-screen portrait"));
    assert_eq!(label(12), Some("load-screen portrait"));
    assert_eq!(label(13), Some("load-screen empty-slot frame"));
    for id in 1086..=1089 {
        assert_eq!(label(id), Some("publisher logo"), "logo id {id}");
    }

    // Every assigned label is in the controlled vocabulary.
    for t in &catalog {
        if let Some(l) = t.label {
            assert!(
                legaia_asset::tim_labels::VALID_LABELS.contains(&l),
                "id {} has out-of-vocabulary label {l:?}",
                t.id
            );
        }
    }
}
