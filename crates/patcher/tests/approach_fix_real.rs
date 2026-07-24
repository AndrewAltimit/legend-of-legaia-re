//! Disc-gated tests for the **attack-approach softlock fix** (see
//! `legaia_patcher::approach_fix`): rewriting the battle-action state-`0x19`
//! arm's redundant facing recompute into a guard that re-stages a monster's
//! dead approach animation (the summon-then-melee clip death behind the
//! "endless camera orbit" softlock) by bouncing the state byte back to
//! `0x14`, where retail's own staging re-runs.
//!
//! These apply the fix to a scratch copy of the real disc and assert, off the
//! patched image, that:
//!   * the baseline window holds the stock nine facing-recompute words and
//!     the context words around it match the documented disassembly
//!     (non-vacuous: the recognized US build);
//!   * after patching, exactly the nine window words changed in the overlay
//!     entry and nothing else anywhere in it;
//!   * the patched image still parses and the edit is byte-deterministic;
//!   * a second application is a clean no-op, never a double-patch.
//!
//! Gates on `LEGAIA_DISC_BIN`; skips+passes when unset. The patched image
//! lives only in memory. Runtime verification of the guard's behaviour is the
//! emulator replay `autorun_gaza2_approach_fix_verify.lua` against the
//! library park savestates (the clean-room engine does not execute overlay
//! code).

use legaia_iso::iso9660::read_file_in_image;
use legaia_patcher::apply;
use legaia_patcher::approach_fix::{
    BATTLE_ACTION_OVERLAY_PROT_INDEX, OVERLAY_BASE_VA, STOCK_WINDOW, WINDOW_VA, assemble_window,
    plan,
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
fn baseline_window_holds_the_stock_facing_recompute() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let patcher = DiscPatcher::open(disc).expect("open disc");
    let overlay = patcher
        .read_entry(BATTLE_ACTION_OVERLAY_PROT_INDEX)
        .expect("read battle-action overlay");
    let window_off = (WINDOW_VA - OVERLAY_BASE_VA) as usize;
    for (i, want) in STOCK_WINDOW.iter().enumerate() {
        assert_eq!(
            entry_word(&overlay, window_off + i * 4),
            *want,
            "stock window word {i} matches the documented disassembly"
        );
    }
    // The plan against the pristine build succeeds and targets the window.
    let fix = plan(&overlay)
        .expect("plan against retail build")
        .expect("pristine build plans the rewrite");
    assert_eq!(fix.window_off, window_off);
    assert_eq!(fix.bytes.len(), STOCK_WINDOW.len() * 4);
}

#[test]
fn fix_changes_exactly_the_window_words() {
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
    let window_off = (WINDOW_VA - OVERLAY_BASE_VA) as usize;
    for (i, want) in assemble_window().iter().enumerate() {
        assert_eq!(entry_word(&after, window_off + i * 4), *want);
    }

    // Surgical: every changed byte lies inside the nine-word window.
    let changed: Vec<usize> = before
        .iter()
        .zip(after.iter())
        .enumerate()
        .filter(|(_, (a, b))| a != b)
        .map(|(i, _)| i)
        .collect();
    assert!(!changed.is_empty());
    assert!(
        changed.first().copied().unwrap() >= window_off
            && changed.last().copied().unwrap() < window_off + STOCK_WINDOW.len() * 4,
        "changes confined to the guard window"
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
    // A second application is a clean no-op: the plan recognizes the guard.
    let again = apply::apply_approach_fix(&mut a).expect("re-apply is accepted");
    assert!(!again.changed, "second application reports no change");
    let mut c = DiscPatcher::open(a.image().to_vec()).expect("open patched");
    let again2 = apply::apply_approach_fix(&mut c).expect("plan on patched image");
    assert!(!again2.changed);
}
