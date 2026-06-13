//! Disc-gated end-to-end test for the element-affinity randomizer: shuffle the
//! 8×8 affinity matrix in PROT entry 0898 on a scratch copy of the disc, then
//! re-parse the patched overlay off the patched image and confirm:
//!
//! - the matrix's scale-percentage multiset is preserved (shuffle is 1:1);
//! - the parse still succeeds (a 64-byte raw edit, table stays well-formed);
//! - the per-character element assignment + summon-power rows are untouched;
//! - a fixed seed reproduces the patched image byte-for-byte.
//!
//! Skips + passes when `LEGAIA_DISC_BIN` is unset.

use legaia_asset::element_affinity::{BATTLE_ACTION_OVERLAY_PROT_INDEX, ElementAffinity};
use legaia_rando::apply;
use legaia_rando::disc::DiscPatcher;
use legaia_rando::drops::DropMode;

fn load_disc() -> Option<Vec<u8>> {
    let p = std::path::PathBuf::from(std::env::var_os("LEGAIA_DISC_BIN")?);
    p.is_file().then(|| std::fs::read(&p).ok()).flatten()
}

fn affinity(patcher: &DiscPatcher) -> ElementAffinity {
    let entry = patcher
        .read_entry(BATTLE_ACTION_OVERLAY_PROT_INDEX)
        .expect("read 0898");
    ElementAffinity::parse(&entry).expect("affinity parses")
}

fn matrix_multiset(aff: &ElementAffinity) -> Vec<u8> {
    let mut v: Vec<u8> = aff.matrix.iter().flatten().copied().collect();
    v.sort_unstable();
    v
}

#[test]
fn shuffle_affinity_round_trips_on_disc() {
    let Some(original) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let seed = 0x05EA_1F00_DE1E_0001;

    let base = DiscPatcher::open(original.clone()).expect("open");
    let before = affinity(&base);
    let before_ms = matrix_multiset(&before);

    let mut patcher = DiscPatcher::open(original.clone()).expect("open");
    let changed = apply::randomize_element_affinity(&mut patcher, seed, DropMode::Shuffle)
        .expect("randomize");
    assert!(
        changed > 0,
        "a shuffle should move at least one affinity cell"
    );

    // Re-parse off the PATCHED image.
    let after = affinity(&patcher);
    assert_eq!(
        matrix_multiset(&after),
        before_ms,
        "shuffle must preserve the scale-percentage multiset"
    );
    // Sibling tables are left untouched.
    assert_eq!(
        after.character_elements, before.character_elements,
        "per-character element assignment must be untouched"
    );
    assert_eq!(
        after.summon_power, before.summon_power,
        "summon-power rows must be untouched"
    );

    // Determinism.
    let mut patcher2 = DiscPatcher::open(original).expect("open");
    let changed2 = apply::randomize_element_affinity(&mut patcher2, seed, DropMode::Shuffle)
        .expect("randomize");
    assert_eq!(changed2, changed);
    assert!(
        patcher2.image() == patcher.image(),
        "same seed must reproduce the patched image"
    );

    eprintln!(
        "element-affinity shuffle seed {seed:#x}: {changed} cells changed; multiset + sibling tables preserved"
    );
}
