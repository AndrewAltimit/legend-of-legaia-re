//! Disc-gated tests for the **bonus equipment drop** — the additive code hook
//! that grants one extra random equipment piece from the battle-end reward
//! routine (see `legaia_rando::bonus_drop`).
//!
//! The injection is two same-size `SCUS_942.54` edits: a detour at the
//! reward-routine hook (`0x8004f610`) and a routine + equipment-id table written
//! into preserved rodata padding (`0x8007AB80`). These tests apply it to a
//! scratch copy of the real disc and assert, off the patched image, that:
//!   * the detour jumps to the routine and the routine + table decode as the
//!     hand-assembled bytes (so the patch is the code we intend);
//!   * the edit is surgical (only the hook + routine regions change), the
//!     equipment table holds pool ids, and the disc still parses;
//!   * a fixed chance is byte-deterministic; and
//!   * the planner refuses an unrecognized build instead of corrupting it.
//!
//! Gates on `LEGAIA_DISC_BIN`; skips+passes when unset. The patched image lives
//! only in memory (never written to disk). NB the clean-room engine can't run
//! injected MIPS, so unlike the data-edit randomizers this feature has no engine
//! runtime oracle — verification is the byte/disassembly checks here plus an
//! emulator playtest.

use legaia_asset::item_names::file_offset_for_va;
use legaia_iso::iso9660::read_file_in_image;
use legaia_rando::apply;
use legaia_rando::bonus_drop::{
    self, BonusDropInjection, DISPLACED, HOOK_VA, RETURN_VA, ROUTINE_VA, assemble_routine,
    detour_words,
};
use legaia_rando::disc::DiscPatcher;
use legaia_rando::equipment::{equipment_ids, equipment_pool};
use std::collections::HashSet;

fn load_disc() -> Option<Vec<u8>> {
    let p = std::path::PathBuf::from(std::env::var_os("LEGAIA_DISC_BIN")?);
    p.is_file().then(|| std::fs::read(&p).ok()).flatten()
}

fn words_at(scus: &[u8], va: u32, n: usize) -> Vec<u32> {
    let off = file_offset_for_va(scus, va).expect("resolve va");
    (0..n)
        .map(|i| u32::from_le_bytes(scus[off + i * 4..off + i * 4 + 4].try_into().unwrap()))
        .collect()
}

#[test]
fn equipment_pool_classifies_weapons_armor_accessories() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let scus = read_file_in_image(&disc, "SCUS_942.54").expect("SCUS in image");
    let pool = equipment_pool(&scus).expect("build equipment pool");

    // ~150 of the 155 curated equipment names resolve.
    assert!(pool.len() >= 120, "pool unexpectedly small: {}", pool.len());
    let ids: Vec<u8> = pool.iter().map(|e| e.id).collect();
    let unique: HashSet<u8> = ids.iter().copied().collect();
    assert_eq!(ids.len(), unique.len(), "pool ids must be unique");
    let mut sorted = ids.clone();
    sorted.sort_unstable();
    assert_eq!(ids, sorted, "pool is sorted by id");
    // Survival Knife (0x22) is gear; Honey (0x65) is a consumable, not gear.
    assert!(ids.contains(&0x22), "Survival Knife (0x22) in pool");
    assert!(
        !ids.contains(&0x65),
        "Honey (0x65) is a consumable, excluded"
    );
}

#[test]
fn injection_writes_the_expected_detour_routine_and_table() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let scus0 = read_file_in_image(&disc, "SCUS_942.54").expect("SCUS in image");
    let ids = equipment_ids(&scus0).expect("equipment ids");
    let pool_ids: HashSet<u8> = ids.iter().copied().collect();

    // Plan up front so we know the offsets/lengths we expect to change.
    let chance = 7u8;
    let plan = BonusDropInjection::plan(&scus0, &ids, chance).expect("plan");
    let table_len = plan.table_len;

    let mut patcher = DiscPatcher::open(disc.clone()).expect("open disc");
    let report = apply::inject_equipment_bonus_drop(&mut patcher, chance).expect("inject");
    assert_eq!(report.chance_pct, chance);
    assert_eq!(report.table_len, table_len);

    let scus = read_file_in_image(patcher.image(), "SCUS_942.54").expect("patched SCUS");

    // 1. The detour at the hook site: `j ROUTINE_VA` + nop.
    let hook = words_at(&scus, HOOK_VA, 2);
    assert_eq!(hook, detour_words().to_vec(), "detour is `j routine` + nop");

    // 2. The routine decodes as the hand-assembled words.
    let table_va = ROUTINE_VA + 22 * 4;
    let expect_routine = assemble_routine(table_va, table_len, chance);
    let got_routine = words_at(&scus, ROUTINE_VA, expect_routine.len());
    assert_eq!(got_routine, expect_routine, "routine matches the assembler");
    // The routine replays the two original displaced instructions and returns.
    assert_eq!(got_routine[18], DISPLACED[0]);
    assert_eq!(got_routine[19], DISPLACED[1]);
    assert_eq!(
        (got_routine[20] & 0x03ff_ffff) << 2,
        RETURN_VA & 0x0fff_ffff
    );

    // 3. The id table after the routine holds the equipment pool ids.
    let tab_off = file_offset_for_va(&scus, table_va).expect("table off");
    let table = &scus[tab_off..tab_off + table_len];
    assert_eq!(table, &ids[..table_len], "embedded table = equipment ids");
    for &id in table {
        assert!(pool_ids.contains(&id), "table id {id} is pool equipment");
    }

    // 4. Surgical: only the hook (8 bytes) and the routine+table blob change.
    let hook_off = file_offset_for_va(&scus0, HOOK_VA).unwrap();
    let blob_off = file_offset_for_va(&scus0, ROUTINE_VA).unwrap();
    let blob_len = plan.blob.len();
    assert_eq!(scus.len(), scus0.len(), "SCUS size unchanged");
    for (i, (&a, &b)) in scus0.iter().zip(scus.iter()).enumerate() {
        let in_hook = (hook_off..hook_off + 8).contains(&i);
        let in_blob = (blob_off..blob_off + blob_len).contains(&i);
        if !in_hook && !in_blob {
            assert_eq!(a, b, "byte {i:#x} changed outside the patched regions");
        }
    }

    // 5. The disc still parses (monster drops decode off the patched image).
    let drops = apply::current_drops(&patcher).expect("drops still decode");
    assert!(!drops.is_empty(), "monster archive still readable");
}

#[test]
fn injection_is_byte_deterministic() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut a = DiscPatcher::open(disc.clone()).expect("open a");
    let mut b = DiscPatcher::open(disc).expect("open b");
    apply::inject_equipment_bonus_drop(&mut a, bonus_drop::DEFAULT_CHANCE_PCT).unwrap();
    apply::inject_equipment_bonus_drop(&mut b, bonus_drop::DEFAULT_CHANCE_PCT).unwrap();
    assert_eq!(
        a.image(),
        b.image(),
        "a fixed chance yields a byte-identical patched image"
    );
}

#[test]
fn planner_refuses_an_unrecognized_build() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut scus = read_file_in_image(&disc, "SCUS_942.54").expect("SCUS in image");
    let ids = equipment_ids(&scus).expect("ids");
    // Sanity: it plans on the real build.
    assert!(BonusDropInjection::plan(&scus, &ids, 5).is_ok());

    // Corrupt the detour-site fingerprint -> refuse rather than patch.
    let hook_off = file_offset_for_va(&scus, HOOK_VA).unwrap();
    scus[hook_off] ^= 0xFF;
    assert!(
        BonusDropInjection::plan(&scus, &ids, 5).is_err(),
        "must refuse a build whose hook site doesn't match"
    );

    // Restore the hook, then dirty the routine landing zone -> also refused.
    scus[hook_off] ^= 0xFF;
    let blob_off = file_offset_for_va(&scus, ROUTINE_VA).unwrap();
    scus[blob_off + 4] = 0x42;
    assert!(
        BonusDropInjection::plan(&scus, &ids, 5).is_err(),
        "must refuse when the routine region isn't dead space"
    );
}
