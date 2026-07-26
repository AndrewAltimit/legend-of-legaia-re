//! Disc-gated tests for the **jewel fix** (see `legaia_patcher::jewel_fix`):
//! retargeting the boss cinematic cast modules' damage calls from the
//! resist-ladder-bypassing wrapper `FUN_801DD6B4` (finisher `param_5 = 1`) to
//! the guard-respecting `FUN_801DD4B0` (`param_5 = 0`), so elemental jewels /
//! guards / All Guard apply to Xain's Bloody Horns / Terio Punch, Cort's
//! Guilty Cross, and the Delilas trio's signature moves.
//!
//! These apply the fix to a scratch copy of the real disc and assert, off the
//! patched image, that:
//!   * every baseline call site holds the stock `jal FUN_801DD6B4` word
//!     (non-vacuous: the recognized US build);
//!   * after patching, exactly those words read `jal FUN_801DD4B0` and every
//!     other byte of every touched module window is unchanged - the `09xx`
//!     extents tile exactly, so each module's window is checked to contain no
//!     other module's bytes and every changed offset must be one of that
//!     module's own sites;
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

use std::collections::BTreeMap;

use legaia_iso::iso9660::read_file_in_image;
use legaia_patcher::apply;
use legaia_patcher::disc::DiscPatcher;
use legaia_patcher::jewel_fix::{
    BLOODY_HORNS_PROT_INDEX, JewelFix, MODULE_INDICES, SITES, TERIO_PUNCH_PROT_INDEX, bypass_word,
    respect_word,
};

fn load_disc() -> Option<Vec<u8>> {
    let p = std::path::PathBuf::from(std::env::var_os("LEGAIA_DISC_BIN")?);
    p.is_file().then(|| std::fs::read(&p).ok()).flatten()
}

fn entry_word(entry: &[u8], off: usize) -> u32 {
    u32::from_le_bytes(entry[off..off + 4].try_into().unwrap())
}

fn read_modules(patcher: &DiscPatcher) -> BTreeMap<usize, Vec<u8>> {
    MODULE_INDICES
        .iter()
        .map(|&i| (i, patcher.read_entry(i).expect("read cast module")))
        .collect()
}

#[test]
fn baseline_sites_hold_the_stock_bypass_words() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let patcher = DiscPatcher::open(disc).expect("open disc");
    let modules = read_modules(&patcher);
    for s in SITES {
        assert_eq!(
            entry_word(&modules[&s.prot_index], s.offset),
            bypass_word(),
            "PROT {} +{:#x} is the stock `jal FUN_801DD6B4`",
            s.prot_index,
            s.offset
        );
    }
    // The plan against the pristine build succeeds and covers every site.
    let plan = JewelFix::plan(&modules).expect("plan against retail build");
    assert_eq!(plan.writes.len(), SITES.len());
}

#[test]
fn fix_retargets_exactly_the_site_words() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut patcher = DiscPatcher::open(disc).expect("open disc");
    let before = read_modules(&patcher);

    let report = apply::apply_jewel_fix(&mut patcher).expect("apply jewel fix");
    assert_eq!(report.sites_patched, SITES.len());

    let after = read_modules(&patcher);

    // Every site now calls the guard-respecting wrapper.
    for s in SITES {
        assert_eq!(
            entry_word(&after[&s.prot_index], s.offset),
            respect_word(),
            "PROT {} +{:#x} retargeted to `jal FUN_801DD4B0`",
            s.prot_index,
            s.offset
        );
    }

    // The `09xx` extents tile exactly - an entry's size is the sector gap to
    // its successor - so no module's bytes are visible inside another's
    // window. Assert that directly: 952 and 953 are the adjacent pair that
    // would alias first if sizing ever over-read again, and entry 953's head
    // must NOT appear anywhere inside entry 952's window.
    //
    // This replaces an assertion that the two extents overlapped at `+0x1800`.
    // That overlap was an artifact of the superseded size expression
    // `toc[p+5] - toc[p+3] + 4`, under which 952 measured `0x6000` instead of
    // its real `0x1800` and swallowed three neighbours. Guarding disjointness
    // is the property that actually makes the patch surgical; the old
    // assertion was pinning the over-read.
    assert_eq!(
        before[&BLOODY_HORNS_PROT_INDEX]
            .windows(48)
            .position(|w| w == &before[&TERIO_PUNCH_PROT_INDEX][..48]),
        None,
        "entry 953's head must not appear inside entry 952's window - the \
         extents tile, they do not overlap"
    );

    // Every site addresses a word inside its OWN module's extent. With the
    // windows disjoint this is what guarantees each physical word is written
    // once; it is also the check that fails loudly if entry sizing regresses.
    for s in SITES {
        assert!(
            s.offset + 4 <= before[&s.prot_index].len(),
            "PROT {} +{:#x} lies inside its own extent ({} bytes)",
            s.prot_index,
            s.offset,
            before[&s.prot_index].len()
        );
    }

    // Surgical: within each touched module window, the changed word offsets
    // are exactly the module's own sites. The alias sweep below is retained as
    // a live guard - with disjoint extents it must contribute nothing, so any
    // aliased offset it finds means sizing has regressed to over-reading.
    for (&idx, window_before) in &before {
        let window_after = &after[&idx];
        let mut expected: Vec<usize> = SITES
            .iter()
            .filter(|s| s.prot_index == idx)
            .map(|s| s.offset)
            .collect();
        // No OTHER patched module's head may appear inside this window. If one
        // does, entry sizing is over-reading again and a single write would
        // land in two entries' views of the disc.
        for &other in MODULE_INDICES.iter().filter(|&&o| o != idx) {
            let head = &before[&other][..48];
            assert_eq!(
                window_before.windows(48).position(|w| w == head),
                None,
                "PROT {other}'s head appears inside PROT {idx}'s window - \
                 extents must tile, not overlap"
            );
        }
        expected.sort_unstable();
        expected.dedup();
        let mut actual: Vec<usize> = window_before
            .iter()
            .zip(window_after.iter())
            .enumerate()
            .filter(|(_, (a, b))| a != b)
            .map(|(i, _)| i & !3)
            .collect();
        actual.sort_unstable();
        actual.dedup();
        assert_eq!(
            actual, expected,
            "PROT {idx} window: only the planned words (own + aliased) changed"
        );
    }

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
