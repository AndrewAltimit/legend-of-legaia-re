//! Disc-gated tests for the **run-away EXP reward** — the code hook that banks a
//! slice of a fled fight's experience into the party (see `legaia_rando::flee_exp`).
//!
//! The injection is two same-size edits: a detour at the battle-action escape
//! teardown (PROT entry 898, the raw overlay, VA `0x801E5A10`) and an EXP-grant
//! routine written into preserved `SCUS_942.54` rodata padding (`0x8007AD00`).
//! These tests apply it to a scratch copy of the real disc and assert, off the
//! patched image, that:
//!   * the overlay detour jumps to the routine and the routine decodes as the
//!     hand-assembled bytes (so the patch is the code we intend);
//!   * each edit is surgical (only the 8-byte hook and the routine region change)
//!     and every touched sector stays EDC/ECC-valid;
//!   * a fixed percent is byte-deterministic; and
//!   * the planner refuses an unrecognized build instead of corrupting it.
//!
//! Gates on `LEGAIA_DISC_BIN`; skips+passes when unset. The patched image lives
//! only in memory. NB the clean-room engine can't run injected MIPS, so unlike
//! the data-edit randomizers this feature has no engine runtime oracle —
//! verification is the byte/disassembly checks here plus an emulator playtest.

use legaia_asset::item_names::file_offset_for_va;
use legaia_iso::iso9660::read_file_in_image;
use legaia_rando::apply;
use legaia_rando::disc::DiscPatcher;
use legaia_rando::flee_exp::{
    self, BATTLE_ACTION_OVERLAY_PROT_INDEX, DISPLACED, FleeExpInjection, HOOK_VA, OVERLAY_BASE_VA,
    ROUTINE_VA, assemble_routine, detour_words,
};

fn load_disc() -> Option<Vec<u8>> {
    let p = std::path::PathBuf::from(std::env::var_os("LEGAIA_DISC_BIN")?);
    p.is_file().then(|| std::fs::read(&p).ok()).flatten()
}

/// `n` little-endian words at SCUS VA `va`.
fn scus_words(scus: &[u8], va: u32, n: usize) -> Vec<u32> {
    let off = file_offset_for_va(scus, va).expect("resolve va");
    (0..n)
        .map(|i| u32::from_le_bytes(scus[off + i * 4..off + i * 4 + 4].try_into().unwrap()))
        .collect()
}

/// `n` little-endian words at overlay-entry file offset `va - base`.
fn overlay_words(entry: &[u8], va: u32, n: usize) -> Vec<u32> {
    let off = (va - OVERLAY_BASE_VA) as usize;
    (0..n)
        .map(|i| u32::from_le_bytes(entry[off + i * 4..off + i * 4 + 4].try_into().unwrap()))
        .collect()
}

#[test]
fn baseline_hook_site_matches_the_known_build() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let patcher = DiscPatcher::open(disc).expect("open disc");
    let overlay = patcher
        .read_entry(BATTLE_ACTION_OVERLAY_PROT_INDEX)
        .expect("read battle-action overlay");
    // The escape-teardown handler entry is the expected `lui v1,0x801d` /
    // `addiu a0,v1,-0x6f90` pair — the build fingerprint the planner guards on.
    assert_eq!(
        overlay_words(&overlay, HOOK_VA, 2),
        DISPLACED.to_vec(),
        "escape-teardown hook is the recognized US build"
    );
}

#[test]
fn injection_writes_the_expected_detour_and_routine() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let scus0 = read_file_in_image(&disc, "SCUS_942.54").expect("SCUS in image");
    let pct = 7u8;

    let mut patcher = DiscPatcher::open(disc.clone()).expect("open disc");
    let overlay0 = patcher
        .read_entry(BATTLE_ACTION_OVERLAY_PROT_INDEX)
        .expect("read overlay");
    let plan = FleeExpInjection::plan(&scus0, &overlay0, pct).expect("plan");

    let report = apply::inject_flee_exp(&mut patcher, pct).expect("inject");
    assert_eq!(report.pct, pct);

    // 1. The detour in the overlay PROT entry: `j ROUTINE_VA` + nop.
    let overlay = patcher
        .read_entry(BATTLE_ACTION_OVERLAY_PROT_INDEX)
        .expect("read patched overlay");
    assert_eq!(
        overlay_words(&overlay, HOOK_VA, 2),
        detour_words().to_vec(),
        "detour is `j routine` + nop"
    );

    // 2. The routine in SCUS decodes as the hand-assembled words, ending by
    //    replaying the displaced pair and jumping back.
    let scus = read_file_in_image(patcher.image(), "SCUS_942.54").expect("patched SCUS");
    let expect = assemble_routine(pct);
    let got = scus_words(&scus, ROUTINE_VA, expect.len());
    assert_eq!(got, expect, "routine matches the assembler");
    assert_eq!(got[60], DISPLACED[0], "routine replays displaced[0]");
    assert_eq!(got[61], DISPLACED[1], "routine replays displaced[1]");

    // 3. Surgical: only the routine blob region of SCUS changed.
    let blob_off = file_offset_for_va(&scus0, ROUTINE_VA).unwrap();
    let blob_len = plan.blob.len();
    assert_eq!(scus.len(), scus0.len(), "SCUS size unchanged");
    for (i, (&a, &b)) in scus0.iter().zip(scus.iter()).enumerate() {
        if !(blob_off..blob_off + blob_len).contains(&i) {
            assert_eq!(a, b, "SCUS byte {i:#x} changed outside the routine region");
        }
    }

    // 4. Surgical: only the 8-byte hook of the overlay entry changed.
    let hook_off = (HOOK_VA - OVERLAY_BASE_VA) as usize;
    assert_eq!(
        overlay.len(),
        overlay0.len(),
        "overlay entry size unchanged"
    );
    for (i, (&a, &b)) in overlay0.iter().zip(overlay.iter()).enumerate() {
        if !(hook_off..hook_off + 8).contains(&i) {
            assert_eq!(a, b, "overlay byte {i:#x} changed outside the hook");
        }
    }

    // 5. The disc still parses (monster archive + move-power table decode off the
    //    patched image), and every patched sector stays EDC/ECC-valid.
    assert!(
        !apply::current_drops(&patcher)
            .expect("drops decode")
            .is_empty(),
        "monster archive still readable"
    );
    assert!(
        apply::current_move_powers(&patcher)
            .expect("move powers decode")
            .is_some(),
        "patched battle-action overlay still parses"
    );
    legaia_iso::iso9660::read_file_in_image(patcher.image(), "SCUS_942.54")
        .expect("patched image re-reads (sectors stay valid)");
}

#[test]
fn injection_is_byte_deterministic() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut a = DiscPatcher::open(disc.clone()).expect("open a");
    let mut b = DiscPatcher::open(disc).expect("open b");
    apply::inject_flee_exp(&mut a, flee_exp::DEFAULT_PCT).unwrap();
    apply::inject_flee_exp(&mut b, flee_exp::DEFAULT_PCT).unwrap();
    assert_eq!(
        a.image(),
        b.image(),
        "a fixed percent yields a byte-identical patched image"
    );
}

#[test]
fn planner_refuses_an_unrecognized_build() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let scus = read_file_in_image(&disc, "SCUS_942.54").expect("SCUS in image");
    let patcher = DiscPatcher::open(disc).expect("open disc");
    let mut overlay = patcher
        .read_entry(BATTLE_ACTION_OVERLAY_PROT_INDEX)
        .expect("read overlay");

    // Sanity: it plans on the real build.
    assert!(FleeExpInjection::plan(&scus, &overlay, 5).is_ok());

    // Corrupt the detour-site fingerprint -> refuse rather than patch.
    let hook_off = (HOOK_VA - OVERLAY_BASE_VA) as usize;
    overlay[hook_off] ^= 0xFF;
    assert!(
        FleeExpInjection::plan(&scus, &overlay, 5).is_err(),
        "must refuse a build whose hook site doesn't match"
    );
    overlay[hook_off] ^= 0xFF;

    // Dirty the routine landing zone in SCUS -> also refused.
    let mut scus_dirty = scus.clone();
    let blob_off = file_offset_for_va(&scus_dirty, ROUTINE_VA).unwrap();
    scus_dirty[blob_off + 8] = 0x42;
    assert!(
        FleeExpInjection::plan(&scus_dirty, &overlay, 5).is_err(),
        "must refuse when the routine region isn't dead space"
    );
}
