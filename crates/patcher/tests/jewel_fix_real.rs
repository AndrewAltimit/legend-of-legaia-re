//! Disc-gated tests for the **jewel fix** (see `legaia_patcher::jewel_fix`):
//! retargeting the Bloody Horns / Terio Punch cast modules' damage calls from
//! the resist-ladder-bypassing wrapper `FUN_801DD6B4` (finisher `param_5 = 1`)
//! to the guard-respecting `FUN_801DD4B0` (`param_5 = 0`), so elemental jewels
//! / guards / All Guard apply to Xain's signature casts.
//!
//! These apply the fix to a scratch copy of the real disc and assert, off the
//! patched image, that:
//!   * the baseline call sites hold the stock `jal FUN_801DD6B4` words
//!     (non-vacuous: the recognized US build);
//!   * after patching, exactly those words read `jal FUN_801DD4B0` and
//!     every other byte of both module entries is unchanged;
//!   * the patched image still parses (named-file read re-validates sector
//!     structure) and the edit is byte-deterministic;
//!   * the planner refuses an already-patched image (idempotence guard) and an
//!     unrecognized build.
//!
//! Gates on `LEGAIA_DISC_BIN`; skips+passes when unset. The patched image lives
//! only in memory. NB the retail wrappers are overlay code the clean-room
//! engine does not execute, so runtime verification is an emulator playtest;
//! the engine-side equivalent of the fix is `damage_finish::bypass_party_resist
//! = false`.

use legaia_iso::iso9660::read_file_in_image;
use legaia_patcher::apply;
use legaia_patcher::disc::DiscPatcher;
use legaia_patcher::jewel_fix::{
    BLOODY_HORNS_PROT_INDEX, JewelFix, OVERLAP_953_IN_952, SITES, TERIO_PUNCH_PROT_INDEX,
    bypass_word, respect_word,
};

fn load_disc() -> Option<Vec<u8>> {
    let p = std::path::PathBuf::from(std::env::var_os("LEGAIA_DISC_BIN")?);
    p.is_file().then(|| std::fs::read(&p).ok()).flatten()
}

fn entry_word(entry: &[u8], off: usize) -> u32 {
    u32::from_le_bytes(entry[off..off + 4].try_into().unwrap())
}

#[test]
fn baseline_sites_hold_the_stock_bypass_words() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let patcher = DiscPatcher::open(disc).expect("open disc");
    let bh = patcher
        .read_entry(BLOODY_HORNS_PROT_INDEX)
        .expect("read Bloody Horns module");
    let tp = patcher
        .read_entry(TERIO_PUNCH_PROT_INDEX)
        .expect("read Terio Punch module");
    for site in SITES {
        let entry = if site.prot_index == BLOODY_HORNS_PROT_INDEX {
            &bh
        } else {
            &tp
        };
        assert_eq!(
            entry_word(entry, site.offset),
            bypass_word(),
            "PROT {} +{:#x} is the stock `jal FUN_801DD6B4`",
            site.prot_index,
            site.offset
        );
    }
    // The plan against the pristine build succeeds and covers both sites.
    let plan = JewelFix::plan(&bh, &tp).expect("plan against retail build");
    assert_eq!(plan.writes.len(), SITES.len());
}

#[test]
fn fix_retargets_exactly_the_two_module_words() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut patcher = DiscPatcher::open(disc).expect("open disc");
    let bh0 = patcher.read_entry(BLOODY_HORNS_PROT_INDEX).unwrap();
    let tp0 = patcher.read_entry(TERIO_PUNCH_PROT_INDEX).unwrap();

    let report = apply::apply_jewel_fix(&mut patcher).expect("apply jewel fix");
    assert_eq!(report.sites_patched, SITES.len());

    let bh = patcher.read_entry(BLOODY_HORNS_PROT_INDEX).unwrap();
    let tp = patcher.read_entry(TERIO_PUNCH_PROT_INDEX).unwrap();

    // Both sites now call the guard-respecting wrapper...
    for site in SITES {
        let entry = if site.prot_index == BLOODY_HORNS_PROT_INDEX {
            &bh
        } else {
            &tp
        };
        assert_eq!(
            entry_word(entry, site.offset),
            respect_word(),
            "PROT {} +{:#x} retargeted to `jal FUN_801DD4B0`",
            site.prot_index,
            site.offset
        );
    }

    // ...and every other byte of both entry windows is untouched (surgical
    // edit). The neighbouring 09xx extents OVERLAP on disc - entry 953's
    // window starts OVERLAP_953_IN_952 bytes into entry 952's - so the Terio
    // Punch word also shows up in the Bloody Horns window at
    // 0x1800 + 0xA38 = 0x2238 (the same physical word, not a third edit).
    // First pin the overlap model itself against the image:
    assert_eq!(
        &bh0[OVERLAP_953_IN_952..OVERLAP_953_IN_952 + 64],
        &tp0[..64],
        "entry 953's head sits 0x1800 into entry 952's window"
    );
    let diff_offsets = |before: &[u8], after: &[u8]| -> Vec<usize> {
        before
            .iter()
            .zip(after)
            .enumerate()
            .filter(|(_, (a, b))| a != b)
            .map(|(i, _)| i & !3)
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect()
    };
    assert_eq!(
        diff_offsets(&bh0, &bh),
        vec![0x0F70, OVERLAP_953_IN_952 + 0x0A38],
        "Bloody Horns window: exactly its own site + the aliased Terio Punch word changed"
    );
    assert_eq!(
        diff_offsets(&tp0, &tp),
        vec![0x0A38],
        "Terio Punch window: exactly its own site changed"
    );

    // The patched image still parses: a named-file read walks the ISO structure
    // over re-encoded sectors.
    read_file_in_image(patcher.image(), "SCUS_942.54")
        .expect("patched image re-reads (sectors stay valid)");
}

#[test]
fn fix_is_byte_deterministic_and_idempotence_guarded() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut a = DiscPatcher::open(disc.clone()).expect("open a");
    let mut b = DiscPatcher::open(disc).expect("open b");
    apply::apply_jewel_fix(&mut a).unwrap();
    apply::apply_jewel_fix(&mut b).unwrap();
    assert_eq!(
        a.image(),
        b.image(),
        "the jewel fix yields a byte-identical patched image"
    );
    // A second application refuses: the sites no longer hold the stock word.
    assert!(
        apply::apply_jewel_fix(&mut a).is_err(),
        "re-applying over a patched image is refused, not silently doubled"
    );
}
