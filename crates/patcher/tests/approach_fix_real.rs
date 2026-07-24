//! Disc-gated tests for the **attack-approach softlock fix** (see
//! `legaia_patcher::approach_fix`): retargeting the battle-action state
//! machine's walk-tag-missing jump (`j 0x801E32C4`, the state-`0x19` park) to
//! the in-range strike continuation (`j 0x801E3204`), so a monster with no
//! walk animation whose contact attack targets someone beyond its reach
//! strikes in place instead of parking the battle forever (the "endless
//! camera orbit" softlock, caught live on the Gaza rematch).
//!
//! These apply the fix to a scratch copy of the real disc and assert, off the
//! patched image, that:
//!   * the baseline hook holds the stock park jump and the two context words
//!     match the documented disassembly (non-vacuous: the recognized US
//!     build);
//!   * after patching, exactly the one hook word changed in the whole overlay
//!     entry;
//!   * the patched image still parses and the edit is byte-deterministic;
//!   * a second application is a clean no-op (the plan recognizes the fixed
//!     word), never a double-patch.
//!
//! Gates on `LEGAIA_DISC_BIN`; skips+passes when unset. The patched image
//! lives only in memory. NB the retail state machine is overlay code the
//! clean-room engine does not execute; the engine port cannot park (its host
//! range check defaults in-range), so runtime verification of the retail fix
//! is an emulator playtest against the library park savestate.

use legaia_iso::iso9660::read_file_in_image;
use legaia_patcher::apply;
use legaia_patcher::approach_fix::{
    BATTLE_ACTION_OVERLAY_PROT_INDEX, HOOK_VA, OVERLAY_BASE_VA, park_word, plan, strike_word,
};
use legaia_patcher::disc::DiscPatcher;

fn load_disc() -> Option<Vec<u8>> {
    let p = std::path::PathBuf::from(std::env::var_os("LEGAIA_DISC_BIN")?);
    p.is_file().then(|| std::fs::read(&p).ok()).flatten()
}

fn entry_word(entry: &[u8], off: usize) -> u32 {
    u32::from_le_bytes(entry[off..off + 4].try_into().unwrap())
}

#[test]
fn baseline_hook_holds_the_stock_park_jump() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let patcher = DiscPatcher::open(disc).expect("open disc");
    let overlay = patcher
        .read_entry(BATTLE_ACTION_OVERLAY_PROT_INDEX)
        .expect("read battle-action overlay");
    let hook_off = (HOOK_VA - OVERLAY_BASE_VA) as usize;
    assert_eq!(
        entry_word(&overlay, hook_off),
        park_word(),
        "hook word is the stock `j 0x801E32C4`"
    );
    // The plan against the pristine build succeeds and targets that offset.
    let fix = plan(&overlay)
        .expect("plan against retail build")
        .expect("pristine build plans a write");
    assert_eq!(fix.hook_off, hook_off);
    assert_eq!(fix.word, strike_word());
}

#[test]
fn fix_changes_exactly_the_hook_word() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut patcher = DiscPatcher::open(disc).expect("open disc");
    let before = patcher
        .read_entry(BATTLE_ACTION_OVERLAY_PROT_INDEX)
        .expect("read overlay before");

    let report = apply::apply_approach_fix(&mut patcher).expect("apply approach fix");
    assert!(report.changed);

    let after = patcher
        .read_entry(BATTLE_ACTION_OVERLAY_PROT_INDEX)
        .expect("read overlay after");
    let hook_off = (HOOK_VA - OVERLAY_BASE_VA) as usize;
    assert_eq!(entry_word(&after, hook_off), strike_word());

    // Surgical: the hook word is the only difference in the whole entry.
    let changed: Vec<usize> = before
        .iter()
        .zip(after.iter())
        .enumerate()
        .filter(|(_, (a, b))| a != b)
        .map(|(i, _)| i & !3)
        .collect();
    let mut changed_words = changed;
    changed_words.dedup();
    assert_eq!(
        changed_words,
        vec![hook_off],
        "only the hook word changed in the overlay entry"
    );

    // The patched image still parses: a named-file read walks the ISO
    // structure over re-encoded sectors.
    read_file_in_image(patcher.image(), "SCUS_942.54")
        .expect("patched image re-reads (sectors stay valid)");
}

#[test]
fn fix_is_byte_deterministic_and_idempotent() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut a = DiscPatcher::open(disc.clone()).expect("open a");
    let mut b = DiscPatcher::open(disc).expect("open b");
    apply::apply_approach_fix(&mut a).unwrap();
    apply::apply_approach_fix(&mut b).unwrap();
    assert_eq!(
        a.image(),
        b.image(),
        "the approach fix yields a byte-identical patched image"
    );
    // A second application is a clean no-op: the plan recognizes the fixed
    // word (unlike an unrecognized build, which errors).
    let again = apply::apply_approach_fix(&mut a).expect("re-apply is accepted");
    assert!(!again.changed, "second application reports no change");
    let mut c = DiscPatcher::open(a.image().to_vec()).expect("open patched");
    let again2 = apply::apply_approach_fix(&mut c).expect("plan on patched image");
    assert!(!again2.changed);
}
