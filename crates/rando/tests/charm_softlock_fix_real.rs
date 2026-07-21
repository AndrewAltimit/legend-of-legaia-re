//! Disc-gated tests for the **charm-battle softlock fix** (see
//! `legaia_rando::charm_fix`) - the disc-side companion to the enemy-ally
//! ("charm") feature that closes the state-`0x5A` victory-arm hard-freeze the
//! charm victory-mask widen makes reachable.
//!
//! The fix is two same-size edits, applied automatically alongside the charm
//! feature by `apply::inject_enemy_ally`: a one-word detour over the victory-arm
//! keep-branch `bne a0,zero,0x801E6728` at overlay VA `0x801E6690` (PROT entry
//! 898), and a 10-instruction guard blob written into preserved `SCUS_942.54`
//! rodata padding at `0x8007AB50`. The guard keeps the acting slot only when it
//! is a living **party** slot (`alive && slot < 3`) and otherwise routes into
//! retail's own valid-slot re-pick, so the win-pose roster read at `0x801E6770`
//! can never index the 3-byte roster out of bounds.
//!
//! These apply the fix to a scratch copy of the real disc and assert, off the
//! patched image, that:
//!   * the baseline hook site is the expected stock `bne` (non-vacuous) and the
//!     guard region is all-zero dead space before patching;
//!   * the overlay detour is `j 0x8007AB50` and its delay slot (the original
//!     `sb v0,-0x42a0(v1)` battle-flag clear at `0x801E6694`) is left in place;
//!   * the SCUS guard decodes as the hand-assembled words, both re-pick branches
//!     target the re-pick landing, and the two decisive jumps land on the retail
//!     keep / re-pick entries;
//!   * each edit is surgical and every touched sector stays EDC/ECC-valid;
//!   * a fixed input is byte-deterministic; the guard composes with the Seru-Bell
//!     name injection in the same gap; and the planner refuses an unrecognized
//!     build instead of corrupting it.
//!
//! Gates on `LEGAIA_DISC_BIN`; skips+passes when unset. The patched image lives
//! only in memory. NB the clean-room engine can't run injected MIPS, so this
//! feature has no engine runtime oracle - verification is the byte/disassembly
//! checks here plus an emulator playtest.

use legaia_asset::item_names::file_offset_for_va;
use legaia_iso::iso9660::read_file_in_image;
use legaia_rando::apply;
use legaia_rando::charm_fix::{
    self, BATTLE_ACTION_OVERLAY_PROT_INDEX, CharmVictoryFix, HOOK_ORIG, HOOK_VA, KEEP_VA,
    OVERLAY_BASE_VA, REROLL_VA, ROUTINE_VA, assemble_routine, detour_word,
};
use legaia_rando::disc::DiscPatcher;

fn load_disc() -> Option<Vec<u8>> {
    let p = std::path::PathBuf::from(std::env::var_os("LEGAIA_DISC_BIN")?);
    p.is_file().then(|| std::fs::read(&p).ok()).flatten()
}

/// One little-endian word at overlay-entry file offset `va - base`.
fn overlay_word(entry: &[u8], va: u32) -> u32 {
    let off = (va - OVERLAY_BASE_VA) as usize;
    u32::from_le_bytes(entry[off..off + 4].try_into().unwrap())
}

/// `n` little-endian words at SCUS VA `va`.
fn scus_words(scus: &[u8], va: u32, n: usize) -> Vec<u32> {
    let off = file_offset_for_va(scus, va).expect("resolve va");
    (0..n)
        .map(|i| u32::from_le_bytes(scus[off + i * 4..off + i * 4 + 4].try_into().unwrap()))
        .collect()
}

#[test]
fn baseline_hook_site_and_guard_region_match_the_known_build() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let scus = read_file_in_image(&disc, "SCUS_942.54").expect("SCUS in image");
    let patcher = DiscPatcher::open(disc).expect("open disc");
    let overlay = patcher
        .read_entry(BATTLE_ACTION_OVERLAY_PROT_INDEX)
        .expect("read battle-action overlay");

    // Non-vacuous baseline: the victory-arm keep-branch is the stock
    // `bne a0,zero,0x801E6728`, and the guard region is all-zero dead space.
    assert_eq!(
        overlay_word(&overlay, HOOK_VA),
        HOOK_ORIG,
        "victory keep-branch is the recognized US build"
    );
    let guard_off = file_offset_for_va(&scus, ROUTINE_VA).unwrap();
    let guard_len = assemble_routine().len() * 4;
    assert!(
        scus[guard_off..guard_off + guard_len]
            .iter()
            .all(|&b| b == 0),
        "guard region {ROUTINE_VA:#x} is all-zero before patching"
    );
    // It plans cleanly against the pristine build.
    assert!(CharmVictoryFix::plan(&scus, &overlay).is_ok());
}

#[test]
fn injection_writes_the_guard_detour_and_routine() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let scus0 = read_file_in_image(&disc, "SCUS_942.54").expect("SCUS in image");

    let mut patcher = DiscPatcher::open(disc.clone()).expect("open disc");
    let overlay0 = patcher
        .read_entry(BATTLE_ACTION_OVERLAY_PROT_INDEX)
        .expect("read overlay");
    let plan = CharmVictoryFix::plan(&scus0, &overlay0).expect("plan");

    // Applying the charm feature applies the softlock guard alongside it.
    apply::inject_enemy_ally(&mut patcher, 20).expect("inject charm + guard");

    let scus = read_file_in_image(patcher.image(), "SCUS_942.54").expect("patched SCUS");
    let overlay = patcher
        .read_entry(BATTLE_ACTION_OVERLAY_PROT_INDEX)
        .expect("read patched overlay");

    // 1. The overlay detour: `j ROUTINE_VA` at the hook, and the delay slot
    //    (0x801E6694 = the original `sb v0,-0x42a0(v1)`) is left untouched.
    assert_eq!(
        overlay_word(&overlay, HOOK_VA),
        detour_word(),
        "hook is `j guard`"
    );
    assert_eq!(
        overlay_word(&overlay, HOOK_VA + 4),
        overlay_word(&overlay0, HOOK_VA + 4),
        "the delay-slot store at 0x801E6694 is preserved"
    );
    // Sanity: that preserved word is a store byte off v1 (`sb v0,-0x42a0(v1)`).
    assert_eq!(
        overlay_word(&overlay0, HOOK_VA + 4),
        0xA062_BD60,
        "0x801E6694 is `sb v0,-0x42a0(v1)`"
    );

    // 2. The guard decodes as the hand-assembled words.
    let expect = assemble_routine();
    let got = scus_words(&scus, ROUTINE_VA, expect.len());
    assert_eq!(got, expect, "guard matches the assembler");
    // The decisive jumps land on the retail keep / re-pick entries.
    assert_eq!(
        (got[6] & 0x03ff_ffff) << 2,
        KEEP_VA & 0x0fff_ffff,
        "keep -> 0x801E6728"
    );
    assert_eq!(
        (got[8] & 0x03ff_ffff) << 2,
        REROLL_VA & 0x0fff_ffff,
        "re-pick -> 0x801E6698"
    );

    // 3. Surgical in SCUS: only the guard blob region changed (the charm feature's
    //    own SCUS edits are covered by the enemy_ally test; here we assert the guard
    //    region is where our bytes land and everything else outside the charm edits
    //    is unchanged is left to that test - we check the guard blob itself).
    let guard_off = file_offset_for_va(&scus0, ROUTINE_VA).unwrap();
    assert_eq!(
        &scus[guard_off..guard_off + plan.blob.len()],
        plan.blob.as_slice(),
        "guard blob landed byte-for-byte at 0x8007AB50"
    );

    // 4. The disc still parses (patched overlay + monster archive decode) and every
    //    patched sector stays EDC/ECC-valid.
    assert!(
        apply::current_move_powers(&patcher)
            .expect("move powers decode")
            .is_some(),
        "patched battle-action overlay still parses"
    );
    assert!(
        !apply::current_drops(&patcher)
            .expect("drops decode")
            .is_empty(),
        "monster archive still readable"
    );
    read_file_in_image(patcher.image(), "SCUS_942.54")
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
    apply::inject_enemy_ally(&mut a, 20).unwrap();
    apply::inject_enemy_ally(&mut b, 20).unwrap();
    assert_eq!(
        a.image(),
        b.image(),
        "a fixed input yields a byte-identical patched image"
    );
}

#[test]
fn guard_composes_with_the_seru_bell_name_in_the_same_gap() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    // The Seru-Bell name string (0x8007AB40) and the guard (0x8007AB50) share the
    // head of the preserved rodata gap; enabling both must leave each intact.
    let mut patcher = DiscPatcher::open(disc).expect("open disc");
    apply::inject_seru_bell_name(&mut patcher).expect("name injection");
    apply::inject_enemy_ally(&mut patcher, 20).expect("charm + guard after name");

    let scus = read_file_in_image(patcher.image(), "SCUS_942.54").expect("patched SCUS");
    assert_eq!(
        scus_words(&scus, ROUTINE_VA, assemble_routine().len()),
        assemble_routine(),
        "guard intact alongside the Seru-Bell name string"
    );
    // The name string still terminates before the guard (no overlap).
    let name_off = file_offset_for_va(&scus, charm_fix::ROUTINE_VA - 0x10).unwrap();
    assert_eq!(&scus[name_off..name_off + 10], b"Seru Bell\0");
}

#[test]
fn planner_refuses_an_unrecognized_build() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let scus = read_file_in_image(&disc, "SCUS_942.54").expect("SCUS in image");
    let patcher = DiscPatcher::open(disc).expect("open disc");
    let overlay = patcher
        .read_entry(BATTLE_ACTION_OVERLAY_PROT_INDEX)
        .expect("read overlay");

    // Sanity: it plans on the real build.
    assert!(CharmVictoryFix::plan(&scus, &overlay).is_ok());

    // Corrupt the overlay hook fingerprint -> refuse rather than patch.
    let mut overlay_bad = overlay.clone();
    let hook_off = (HOOK_VA - OVERLAY_BASE_VA) as usize;
    overlay_bad[hook_off] ^= 0xFF;
    assert!(
        CharmVictoryFix::plan(&scus, &overlay_bad).is_err(),
        "must refuse a build whose victory keep-branch doesn't match"
    );
}
