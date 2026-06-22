//! Disc-gated tests for the **shiny Seru** feature (see `legaia_rando::shiny_seru`):
//! a rare capturable enemy with +35% stats whose captured Seru deals +35% damage
//! forever (+ translucent summon and a "+35% DMG!" cast banner). The injection is
//! nine same-size detours + their routines, split across a new preserved
//! `SCUS_942.54` rodata gap (`0x80077728`), the battle-action overlay's move-power
//! padding (`0x801F4FC4`), and steal-table-adjacent SCUS runs. The persistent
//! shiny flag lives in a parallel per-spell byte at `record+0x1C0`, not the
//! spell-level byte (which keeps the level-up + display reads clean).
//!
//! These apply it to a scratch copy of the real disc and assert, off the patched
//! image, that every hook is the recognized US build, each detour becomes
//! `j routine` + `nop`, every byte outside the planned edits is untouched, the
//! disc still parses + stays EDC/ECC-valid, a fixed percent is deterministic, it
//! composes with the enemy-ally feature (disjoint gaps), and the planner refuses
//! an unrecognized build. Gates on `LEGAIA_DISC_BIN`; skips+passes when unset.
//! The patched image lives only in memory. No engine runtime oracle exists for
//! injected MIPS (the clean-room engine can't run it) - the engine path is
//! covered by `legaia-engine-core`'s `shiny_*` unit tests instead.

use legaia_asset::item_names::file_offset_for_va;
use legaia_iso::iso9660::read_file_in_image;
use legaia_rando::apply;
use legaia_rando::disc::DiscPatcher;
use legaia_rando::shiny_seru::{
    self, BATTLE_ACTION_OVERLAY_PROT_INDEX, HOOK_BANNER_VA, HOOK_BMENU_LVL_VA, HOOK_CAPTURE_VA,
    HOOK_DAMAGE_VA, HOOK_FADE_VA, HOOK_GRANT_SHIFT_VA, HOOK_GRANT_VA, HOOK_MENU_VA, HOOK_SETUP_VA,
    MENU_OVERLAY_PROT_INDEX, OVERLAY_BASE_VA, SCUS_GAP_END_VA, SCUS_GAP_VA, ShinySeruInjection,
};

fn load_disc() -> Option<Vec<u8>> {
    let p = std::path::PathBuf::from(std::env::var_os("LEGAIA_DISC_BIN")?);
    p.is_file().then(|| std::fs::read(&p).ok()).flatten()
}

fn scus_word(scus: &[u8], va: u32) -> u32 {
    let off = file_offset_for_va(scus, va).expect("resolve va");
    u32::from_le_bytes(scus[off..off + 4].try_into().unwrap())
}

fn overlay_word(entry: &[u8], va: u32) -> u32 {
    let off = (va - OVERLAY_BASE_VA) as usize;
    u32::from_le_bytes(entry[off..off + 4].try_into().unwrap())
}

/// The expected first word at each hook (the recognized US build fingerprints).
const HOOK_FINGERPRINTS: &[(u32, u32)] = &[
    (HOOK_SETUP_VA, 0x3C02_8008),       // lui v0,0x8008
    (HOOK_CAPTURE_VA, 0xA082_0269),     // sb v0,0x269(a0)
    (HOOK_GRANT_VA, 0xA043_0729),       // sb v1,0x729(v0)
    (HOOK_GRANT_SHIFT_VA, 0x9046_0704), // lbu a2,0x704(v0)
    (HOOK_DAMAGE_VA, 0x9042_0729),      // lbu v0,0x729(v0)
    (HOOK_MENU_VA, 0x8C63_46B0),        // lw v1,0x46b0(v1)
    (HOOK_BMENU_LVL_VA, 0x9043_0729),   // lbu v1,0x729(v0)
    (HOOK_BANNER_VA, 0x8E84_0018),      // lw a0,0x18(s4) (SCUS)
    (HOOK_FADE_VA, 0x9222_0226),        // lbu v0,0x226(s1) (SCUS)
];

#[test]
fn baseline_hooks_match_the_known_build() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let patcher = DiscPatcher::open(disc.clone()).expect("open disc");
    let scus = read_file_in_image(&disc, "SCUS_942.54").expect("SCUS in image");
    let ov0898 = patcher
        .read_entry(BATTLE_ACTION_OVERLAY_PROT_INDEX)
        .expect("read 0898");
    let ov0899 = patcher
        .read_entry(MENU_OVERLAY_PROT_INDEX)
        .expect("read 0899");

    for &(va, want) in HOOK_FINGERPRINTS {
        let got = match va {
            HOOK_SETUP_VA | HOOK_FADE_VA | HOOK_BANNER_VA => scus_word(&scus, va),
            HOOK_MENU_VA => overlay_word(&ov0899, va),
            _ => overlay_word(&ov0898, va),
        };
        assert_eq!(got, want, "hook {va:#x} is the recognized US build");
    }

    // The SCUS arenas that host the routines/data are preserved dead space - AND
    // none of them overlaps a live static table (the trap that put the old layout
    // inside the victory mouth-override table at 0x800781B0 and the move-power
    // table at 0x801F4FC4). The mouth-override table rows must read as the clean
    // game's keyframes, not our code.
    for (start, end, label) in [
        (SCUS_GAP_VA, SCUS_GAP_END_VA, "gap 1"),
        (shiny_seru::ARENA1_VA, shiny_seru::ARENA1_END_VA, "arena 1"),
        (shiny_seru::ARENA2_VA, shiny_seru::ARENA2_END_VA, "arena 2"),
        (shiny_seru::SLOT6_VA, shiny_seru::SLOT6_END_VA, "slot 6"),
    ] {
        let off = file_offset_for_va(&scus, start).unwrap();
        let len = (end - start) as usize;
        assert!(
            scus[off..off + len].iter().all(|&b| b == 0),
            "SCUS arena {label} ({start:#x}..{end:#x}) is all-zero dead space"
        );
    }

    // The victory mouth-override table rows the OLD layout clobbered must be the
    // clean game's data (the mouth bug = our routines were read here as facial
    // keyframes). Spot-check the first addressed row is non-trivial keyframes.
    let mouth_row0 = file_offset_for_va(&scus, 0x8007_81B0).unwrap();
    assert!(
        scus[mouth_row0..mouth_row0 + 12].iter().any(|&b| b != 0),
        "victory mouth-override table row 0 carries the clean game's keyframes"
    );
}

#[test]
fn injection_writes_detours_and_routines_surgically() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let pct = 17u8;
    let scus0 = read_file_in_image(&disc, "SCUS_942.54").expect("SCUS in image");
    let mut patcher = DiscPatcher::open(disc).expect("open disc");
    let ov0898_0 = patcher
        .read_entry(BATTLE_ACTION_OVERLAY_PROT_INDEX)
        .unwrap();
    let ov0899_0 = patcher.read_entry(MENU_OVERLAY_PROT_INDEX).unwrap();

    // Capturable-Seru ids derived from the disc (same as apply does).
    let archive = patcher.read_entry(867).unwrap();
    let ids = shiny_seru::capturable_monster_ids(&archive).expect("capturable ids");
    assert!(
        ids.contains(&10) && !ids.contains(&4),
        "Gimard (10) capturable, gobu (4) not: {ids:?}"
    );
    // Plan first so we can check exactly the planned bytes landed.
    let plan = ShinySeruInjection::plan(&scus0, &ov0898_0, &ov0899_0, pct, &ids).expect("plan");
    let report = apply::inject_shiny_seru(&mut patcher, pct).expect("inject");
    assert_eq!(report.pct, pct);

    let scus = read_file_in_image(patcher.image(), "SCUS_942.54").expect("patched SCUS");
    let ov0898 = patcher
        .read_entry(BATTLE_ACTION_OVERLAY_PROT_INDEX)
        .unwrap();
    let ov0899 = patcher.read_entry(MENU_OVERLAY_PROT_INDEX).unwrap();

    // Every detour landed as a `j` (opcode 0x02) + nop.
    for &(va, _) in HOOK_FINGERPRINTS {
        let w = match va {
            HOOK_SETUP_VA | HOOK_FADE_VA | HOOK_BANNER_VA => scus_word(&scus, va),
            HOOK_MENU_VA => overlay_word(&ov0899, va),
            _ => overlay_word(&ov0898, va),
        };
        assert_eq!(w >> 26, 0x02, "hook {va:#x} became a `j`");
        let delay = match va {
            HOOK_SETUP_VA | HOOK_FADE_VA | HOOK_BANNER_VA => scus_word(&scus, va + 4),
            HOOK_MENU_VA => overlay_word(&ov0899, va + 4),
            _ => overlay_word(&ov0898, va + 4),
        };
        assert_eq!(delay, 0, "hook {va:#x} delay slot is nop");
    }

    // Surgical: every patched byte is one the plan declared, and nothing else
    // moved. Build the expected byte map per target from the plan.
    let mut scus_edits: Vec<(usize, &[u8])> = Vec::new();
    let mut ov0898_edits: Vec<(usize, &[u8])> = Vec::new();
    let mut ov0899_edits: Vec<(usize, &[u8])> = Vec::new();
    for e in &plan.edits {
        match e.prot_index {
            None => scus_edits.push((e.file_off, &e.bytes)),
            Some(i) if i == BATTLE_ACTION_OVERLAY_PROT_INDEX => {
                ov0898_edits.push((e.file_off, &e.bytes))
            }
            Some(i) if i == MENU_OVERLAY_PROT_INDEX => ov0899_edits.push((e.file_off, &e.bytes)),
            Some(i) => panic!("unexpected PROT index {i}"),
        }
    }
    let in_any = |edits: &[(usize, &[u8])], i: usize| {
        edits
            .iter()
            .any(|&(off, b)| (off..off + b.len()).contains(&i))
    };
    assert_eq!(scus.len(), scus0.len());
    for (i, (&a, &b)) in scus0.iter().zip(scus.iter()).enumerate() {
        if !in_any(&scus_edits, i) {
            assert_eq!(a, b, "SCUS byte {i:#x} changed outside a planned edit");
        }
    }
    assert_eq!(ov0898.len(), ov0898_0.len());
    for (i, (&a, &b)) in ov0898_0.iter().zip(ov0898.iter()).enumerate() {
        if !in_any(&ov0898_edits, i) {
            assert_eq!(a, b, "0898 byte {i:#x} changed outside a planned edit");
        }
    }
    assert_eq!(ov0899.len(), ov0899_0.len());
    for (i, (&a, &b)) in ov0899_0.iter().zip(ov0899.iter()).enumerate() {
        if !in_any(&ov0899_edits, i) {
            assert_eq!(a, b, "0899 byte {i:#x} changed outside a planned edit");
        }
    }

    // The disc still parses + every patched sector stays EDC/ECC-valid.
    assert!(
        !apply::current_drops(&patcher)
            .expect("drops decode")
            .is_empty(),
        "monster archive still readable"
    );
    read_file_in_image(patcher.image(), "SCUS_942.54").expect("patched image re-reads");
}

#[test]
fn injection_is_byte_deterministic() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut a = DiscPatcher::open(disc.clone()).expect("open a");
    let mut b = DiscPatcher::open(disc).expect("open b");
    apply::inject_shiny_seru(&mut a, shiny_seru::DEFAULT_PCT).unwrap();
    apply::inject_shiny_seru(&mut b, shiny_seru::DEFAULT_PCT).unwrap();
    assert_eq!(a.image(), b.image(), "a fixed percent is byte-identical");
}

#[test]
fn composes_with_enemy_ally() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    // enemy-ally lives in the 0x8007AB38 gap + the 0898 victory word; shiny lives
    // in the disjoint 0x80077728 gap + the 0898 move-power padding. Enabling both
    // must succeed and leave each feature's hooks intact.
    let mut patcher = DiscPatcher::open(disc).expect("open disc");
    apply::inject_enemy_ally(&mut patcher, legaia_rando::enemy_ally::DEFAULT_PCT).expect("ally");
    apply::inject_shiny_seru(&mut patcher, shiny_seru::DEFAULT_PCT).expect("shiny after ally");

    let scus = read_file_in_image(patcher.image(), "SCUS_942.54").expect("patched SCUS");
    let ov0898 = patcher
        .read_entry(BATTLE_ACTION_OVERLAY_PROT_INDEX)
        .unwrap();
    // Shiny's setup detour is a `j`.
    assert_eq!(
        scus_word(&scus, HOOK_SETUP_VA) >> 26,
        0x02,
        "shiny setup detour"
    );
    // enemy-ally's victory widen is still in place (its 0898 edit, untouched by shiny).
    assert_eq!(
        overlay_word(&ov0898, legaia_rando::enemy_ally::VICTORY_VA),
        legaia_rando::enemy_ally::VICTORY_PATCHED,
        "enemy-ally victory widen survives alongside shiny"
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
    let ov0898 = patcher
        .read_entry(BATTLE_ACTION_OVERLAY_PROT_INDEX)
        .unwrap();
    let ov0899 = patcher.read_entry(MENU_OVERLAY_PROT_INDEX).unwrap();
    let ids = shiny_seru::capturable_monster_ids(&patcher.read_entry(867).unwrap()).unwrap();

    // Plans on the real build.
    assert!(ShinySeruInjection::plan(&scus, &ov0898, &ov0899, 20, &ids).is_ok());

    // Corrupt the SCUS setup hook -> refuse.
    let mut scus_bad = scus.clone();
    let off = file_offset_for_va(&scus_bad, HOOK_SETUP_VA).unwrap();
    scus_bad[off] ^= 0xFF;
    assert!(ShinySeruInjection::plan(&scus_bad, &ov0898, &ov0899, 20, &ids).is_err());

    // Dirty the SCUS routine landing zone -> refuse.
    let mut scus_dirty = scus.clone();
    let goff = file_offset_for_va(&scus_dirty, SCUS_GAP_VA).unwrap();
    scus_dirty[goff + 8] = 0x42;
    assert!(ShinySeruInjection::plan(&scus_dirty, &ov0898, &ov0899, 20, &ids).is_err());

    // Corrupt a 0898 hook -> refuse.
    let mut ov_bad = ov0898.clone();
    let doff = (HOOK_DAMAGE_VA - OVERLAY_BASE_VA) as usize;
    ov_bad[doff] ^= 0xFF;
    assert!(ShinySeruInjection::plan(&scus, &ov_bad, &ov0899, 20, &ids).is_err());

    // Dirty the arena-1 routine region (non-dead) -> refuse.
    let mut scus_g2 = scus.clone();
    let g2off = file_offset_for_va(&scus_g2, shiny_seru::ARENA1_VA).unwrap();
    scus_g2[g2off + 4] = 0x42;
    assert!(ShinySeruInjection::plan(&scus_g2, &ov0898, &ov0899, 20, &ids).is_err());
}
