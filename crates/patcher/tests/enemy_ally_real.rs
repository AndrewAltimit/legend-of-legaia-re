//! Disc-gated tests for the **enemy-ally ("charm")** feature - the code hook that,
//! with a per-battle chance, flags one enemy so it fights on the player's side
//! (see `legaia_patcher::enemy_ally`).
//!
//! The injection is three same-size edits: a detour at the battle-setup hook
//! (`SCUS_942.54`, `FUN_800513F0`, VA `0x80051990`), a charm routine in preserved
//! `SCUS_942.54` rodata padding (`0x8007ACA0`), and a one-word widen of the
//! victory check in the battle-action overlay (PROT entry 898, VA `0x801E6638`).
//! These tests apply it to a scratch copy of the real disc and assert, off the
//! patched image, that:
//!   * the SCUS detour jumps to the routine and the routine decodes as the
//!     hand-assembled bytes (so the patch is the code we intend);
//!   * the victory `andi v0,v0,0x4` is widened to `andi v0,v0,0x384`;
//!   * each edit is surgical and every touched sector stays EDC/ECC-valid;
//!   * a fixed percent is byte-deterministic; and
//!   * the planner refuses an unrecognized build instead of corrupting it.
//!
//! Gates on `LEGAIA_DISC_BIN`; skips+passes when unset. The patched image lives
//! only in memory. NB the clean-room engine can't run injected MIPS, so unlike
//! the data-edit randomizers this feature has no engine runtime oracle -
//! verification is the byte/disassembly checks here plus an emulator playtest.

use legaia_asset::item_names::file_offset_for_va;
use legaia_iso::iso9660::read_file_in_image;
use legaia_patcher::apply;
use legaia_patcher::disc::DiscPatcher;
use legaia_patcher::enemy_ally::{
    self, BATTLE_ACTION_OVERLAY_PROT_INDEX, DISPLACED, EnemyAllyInjection, HOOK_VA,
    OVERLAY_BASE_VA, ROUTINE_VA, VICTORY_ORIG, VICTORY_PATCHED, VICTORY_VA, assemble_routine,
    detour_words,
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

/// One little-endian word at overlay-entry file offset `va - base`.
fn overlay_word(entry: &[u8], va: u32) -> u32 {
    let off = (va - OVERLAY_BASE_VA) as usize;
    u32::from_le_bytes(entry[off..off + 4].try_into().unwrap())
}

#[test]
fn baseline_sites_match_the_known_build() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let patcher = DiscPatcher::open(disc.clone()).expect("open disc");
    let scus = read_file_in_image(&disc, "SCUS_942.54").expect("SCUS in image");
    // The battle-setup hook is the expected `lui v1,0x8008` / `lbu v1,-0x42f4(v1)`
    // pair (the build fingerprint the planner guards on).
    assert_eq!(
        scus_words(&scus, HOOK_VA, 2),
        DISPLACED.to_vec(),
        "battle-setup hook is the recognized US build"
    );
    // And the victory check is the stock `andi v0,v0,0x4`.
    let overlay = patcher
        .read_entry(BATTLE_ACTION_OVERLAY_PROT_INDEX)
        .expect("read battle-action overlay");
    assert_eq!(
        overlay_word(&overlay, VICTORY_VA),
        VICTORY_ORIG,
        "victory check is the recognized `andi v0,v0,0x4`"
    );
}

#[test]
fn injection_writes_the_detour_routine_and_victory_widen() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let scus0 = read_file_in_image(&disc, "SCUS_942.54").expect("SCUS in image");
    let pct = 33u8;

    let mut patcher = DiscPatcher::open(disc.clone()).expect("open disc");
    let overlay0 = patcher
        .read_entry(BATTLE_ACTION_OVERLAY_PROT_INDEX)
        .expect("read overlay");
    let plan = EnemyAllyInjection::plan(&scus0, &overlay0, pct).expect("plan");

    let report = apply::inject_enemy_ally(&mut patcher, pct).expect("inject");
    assert_eq!(report.pct, pct);

    let scus = read_file_in_image(patcher.image(), "SCUS_942.54").expect("patched SCUS");
    let overlay = patcher
        .read_entry(BATTLE_ACTION_OVERLAY_PROT_INDEX)
        .expect("read patched overlay");

    // 1. The SCUS detour: `j ROUTINE_VA` + nop.
    assert_eq!(
        scus_words(&scus, HOOK_VA, 2),
        detour_words().to_vec(),
        "detour is `j routine` + nop"
    );

    // 2. The routine decodes as the hand-assembled words, ending by replaying the
    //    displaced pair and jumping back.
    let expect = assemble_routine(pct);
    let got = scus_words(&scus, ROUTINE_VA, expect.len());
    assert_eq!(got, expect, "routine matches the assembler");
    assert_eq!(got[20], DISPLACED[0], "routine replays displaced[0]");
    assert_eq!(got[21], DISPLACED[1], "routine replays displaced[1]");

    // 3. The overlay victory check is widened to `andi v0,v0,0x384`.
    assert_eq!(
        overlay_word(&overlay, VICTORY_VA),
        VICTORY_PATCHED,
        "victory mask widened to 0x384"
    );

    // 4. Surgical: only the 8-byte hook + the routine blob region + the charm-fix
    //    guard blob of SCUS changed (the softlock guard ships with the charm edits).
    let hook_off = file_offset_for_va(&scus0, HOOK_VA).unwrap();
    let blob_off = file_offset_for_va(&scus0, ROUTINE_VA).unwrap();
    let blob_len = plan.blob.len();
    let guard_off = file_offset_for_va(&scus0, legaia_patcher::charm_fix::ROUTINE_VA).unwrap();
    let guard_len = legaia_patcher::charm_fix::assemble_routine().len() * 4;
    assert_eq!(scus.len(), scus0.len(), "SCUS size unchanged");
    for (i, (&a, &b)) in scus0.iter().zip(scus.iter()).enumerate() {
        let in_hook = (hook_off..hook_off + 8).contains(&i);
        let in_blob = (blob_off..blob_off + blob_len).contains(&i);
        let in_guard = (guard_off..guard_off + guard_len).contains(&i);
        if !in_hook && !in_blob && !in_guard {
            assert_eq!(
                a, b,
                "SCUS byte {i:#x} changed outside the hook/routine/guard"
            );
        }
    }

    // 5. Surgical: only the 4-byte victory word + the 4-byte charm-fix guard detour
    //    of the overlay entry changed.
    let victory_off = (VICTORY_VA - OVERLAY_BASE_VA) as usize;
    let guard_hook_off =
        (legaia_patcher::charm_fix::HOOK_VA - legaia_patcher::charm_fix::OVERLAY_BASE_VA) as usize;
    assert_eq!(
        overlay.len(),
        overlay0.len(),
        "overlay entry size unchanged"
    );
    for (i, (&a, &b)) in overlay0.iter().zip(overlay.iter()).enumerate() {
        let in_victory = (victory_off..victory_off + 4).contains(&i);
        let in_guard_hook = (guard_hook_off..guard_hook_off + 4).contains(&i);
        if !in_victory && !in_guard_hook {
            assert_eq!(
                a, b,
                "overlay byte {i:#x} changed outside the victory/guard word"
            );
        }
    }

    // 6. The disc still parses (monster archive + move-power table decode off the
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
    apply::inject_enemy_ally(&mut a, enemy_ally::DEFAULT_PCT).unwrap();
    apply::inject_enemy_ally(&mut b, enemy_ally::DEFAULT_PCT).unwrap();
    assert_eq!(
        a.image(),
        b.image(),
        "a fixed percent yields a byte-identical patched image"
    );
}

#[test]
fn composes_with_flee_exp_in_the_same_gap() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    // Both features live in the preserved rodata gap; enemy-ally at 0x8007ACA0 sits
    // below flee-EXP at 0x8007AD00, so enabling both must succeed and each routine
    // must land intact.
    let mut patcher = DiscPatcher::open(disc).expect("open disc");
    apply::inject_flee_exp(&mut patcher, legaia_patcher::flee_exp::DEFAULT_PCT).expect("flee");
    apply::inject_enemy_ally(&mut patcher, enemy_ally::DEFAULT_PCT).expect("ally after flee");

    let scus = read_file_in_image(patcher.image(), "SCUS_942.54").expect("patched SCUS");
    assert_eq!(
        scus_words(
            &scus,
            ROUTINE_VA,
            assemble_routine(enemy_ally::DEFAULT_PCT).len()
        ),
        assemble_routine(enemy_ally::DEFAULT_PCT),
        "enemy-ally routine intact alongside flee-EXP"
    );
    assert_eq!(
        scus_words(&scus, legaia_patcher::flee_exp::ROUTINE_VA, 4)[0],
        legaia_patcher::flee_exp::assemble_routine(legaia_patcher::flee_exp::DEFAULT_PCT)[0],
        "flee-EXP routine still intact"
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
    let overlay = patcher
        .read_entry(BATTLE_ACTION_OVERLAY_PROT_INDEX)
        .expect("read overlay");

    // Sanity: it plans on the real build.
    assert!(EnemyAllyInjection::plan(&scus, &overlay, 20).is_ok());

    // Corrupt the SCUS hook fingerprint -> refuse rather than patch.
    let mut scus_bad = scus.clone();
    let hook_off = file_offset_for_va(&scus_bad, HOOK_VA).unwrap();
    scus_bad[hook_off] ^= 0xFF;
    assert!(
        EnemyAllyInjection::plan(&scus_bad, &overlay, 20).is_err(),
        "must refuse a build whose hook site doesn't match"
    );

    // Dirty the routine landing zone -> also refused.
    let mut scus_dirty = scus.clone();
    let blob_off = file_offset_for_va(&scus_dirty, ROUTINE_VA).unwrap();
    scus_dirty[blob_off + 8] = 0x42;
    assert!(
        EnemyAllyInjection::plan(&scus_dirty, &overlay, 20).is_err(),
        "must refuse when the routine region isn't dead space"
    );

    // Corrupt the victory word -> refused.
    let mut overlay_bad = overlay.clone();
    let victory_off = (VICTORY_VA - OVERLAY_BASE_VA) as usize;
    overlay_bad[victory_off] ^= 0xFF;
    assert!(
        EnemyAllyInjection::plan(&scus, &overlay_bad, 20).is_err(),
        "must refuse when the victory check isn't the recognized `andi v0,v0,0x4`"
    );
}
