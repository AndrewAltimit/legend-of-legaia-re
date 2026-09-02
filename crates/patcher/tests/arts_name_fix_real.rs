//! Disc-gated oracle for the **arts name-length fix, by ZetaPhoenix**
//! (see `legaia_patcher::arts_name_fix`): a 3-word hook at `0x8004BC3C` and
//! his 18-word re-centre routine, parked in verified-dead arena 1 (standalone
//! at the arena head; behind the Super Arts Pack's battle-load stub when the
//! two install together).
//!
//! Gates on `LEGAIA_DISC_BIN`; skips and passes when unset.

use legaia_asset::item_names::file_offset_for_va;
use legaia_patcher::apply;
use legaia_patcher::arts_name_fix::{
    ArtsNameFixInjection, HOOK_DISPLACED, HOOK_VA, ROUTINE_BYTES, ROUTINE_WORDS, assemble_hook,
};
use legaia_patcher::disc::DiscPatcher;
use legaia_patcher::shiny_seru::ARENA1_VA;
use legaia_patcher::super_arts_pack;

fn load_disc() -> Option<Vec<u8>> {
    let p = std::path::PathBuf::from(std::env::var_os("LEGAIA_DISC_BIN")?);
    p.is_file().then(|| std::fs::read(&p).ok()).flatten()
}

fn scus_words(scus: &[u8], va: u32, n: usize) -> Vec<u32> {
    let off = file_offset_for_va(scus, va).expect("resolve VA");
    (0..n)
        .map(|i| u32::from_le_bytes(scus[off + i * 4..off + i * 4 + 4].try_into().unwrap()))
        .collect()
}

#[test]
fn baseline_hook_site_matches_the_known_build() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let patcher = DiscPatcher::open(disc).expect("open disc");
    let scus = patcher.read_named_file("SCUS_942.54").expect("SCUS");
    assert_eq!(
        scus_words(&scus, HOOK_VA, 3),
        HOOK_DISPLACED.to_vec(),
        "li a0,0x4C / jal FUN_801D8DE8 / move a1,zero"
    );
    // The plan is the guard.
    ArtsNameFixInjection::plan(&scus, ARENA1_VA).expect("plans on the retail US build");
}

#[test]
fn standalone_fix_lands_and_is_byte_surgical() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let clean = DiscPatcher::open(disc.clone()).expect("open clean");
    let scus0 = clean.read_named_file("SCUS_942.54").expect("SCUS");

    let mut patcher = DiscPatcher::open(disc).expect("open disc");
    let report = apply::inject_arts_name_fix(&mut patcher, ARENA1_VA).expect("inject");
    assert_eq!(report.routine_va, ARENA1_VA);
    assert_eq!(report.edits, 2);

    let scus = patcher.read_named_file("SCUS_942.54").expect("SCUS");
    assert_eq!(
        scus_words(&scus, HOOK_VA, 3),
        assemble_hook(ARENA1_VA).to_vec()
    );
    assert_eq!(
        scus_words(&scus, ARENA1_VA, ROUTINE_WORDS.len()),
        ROUTINE_WORDS.to_vec(),
        "ZetaPhoenix's routine, verbatim"
    );

    // Byte-surgical: nothing outside the hook and the routine moved.
    let off_of = |va: u32| file_offset_for_va(&scus0, va).expect("resolve");
    let allowed = [
        off_of(HOOK_VA)..off_of(HOOK_VA) + 12,
        off_of(ARENA1_VA)..off_of(ARENA1_VA) + ROUTINE_BYTES as usize,
    ];
    assert_eq!(scus.len(), scus0.len());
    for (i, (&a, &b)) in scus0.iter().zip(scus.iter()).enumerate() {
        if !allowed.iter().any(|r| r.contains(&i)) {
            assert_eq!(a, b, "SCUS byte {i:#x} changed outside a planned edit");
        }
    }

    // A second application must refuse (the arena head is no longer zero).
    assert!(
        apply::inject_arts_name_fix(&mut patcher, ARENA1_VA).is_err(),
        "double-apply must refuse"
    );
}

#[test]
fn fix_rides_behind_the_super_arts_pack_stub() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut patcher = DiscPatcher::open(disc).expect("open disc");
    apply::inject_super_arts_pack(&mut patcher).expect("inject pack");
    let report = apply::inject_arts_name_fix(&mut patcher, super_arts_pack::ARENA_USED_END_VA)
        .expect("inject fix behind the stub");
    assert_eq!(report.routine_va, super_arts_pack::ARENA_USED_END_VA);

    let scus = patcher.read_named_file("SCUS_942.54").expect("SCUS");
    // Both claims hold their words: the pack's loader hook and the fix's hook.
    assert_eq!(
        scus_words(&scus, super_arts_pack::LOAD_HOOK_VA, 1)[0] >> 26,
        0x02,
        "pack loader hook is a j"
    );
    assert_eq!(
        scus_words(&scus, HOOK_VA, 3),
        assemble_hook(super_arts_pack::ARENA_USED_END_VA).to_vec()
    );
    assert_eq!(
        scus_words(
            &scus,
            super_arts_pack::ARENA_USED_END_VA,
            ROUTINE_WORDS.len()
        ),
        ROUTINE_WORDS.to_vec()
    );
}

#[test]
fn injection_is_byte_deterministic() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let run = |disc: Vec<u8>| -> Vec<u8> {
        let mut p = DiscPatcher::open(disc).expect("open");
        apply::inject_arts_name_fix(&mut p, ARENA1_VA).expect("inject");
        p.into_image()
    };
    assert_eq!(run(disc.clone()), run(disc));
}
