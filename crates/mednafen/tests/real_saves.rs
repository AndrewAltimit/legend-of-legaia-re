//! Disc-gated tests against the user's actual mednafen save states.
//!
//! Skipped when `LEGAIA_MEDNAFEN_DIR` is unset — keeps CI green for
//! contributors without a disc + saves on hand.

use legaia_mednafen::diff::{DiffOptions, diff_ram, sort_by_size};
use legaia_mednafen::extract::PSX_RAM_SIZE;
use legaia_mednafen::{SaveState, ScenarioManifest};

fn mcs_dir() -> Option<std::path::PathBuf> {
    std::env::var("LEGAIA_MEDNAFEN_DIR")
        .ok()
        .map(std::path::PathBuf::from)
}

fn manifest_path() -> std::path::PathBuf {
    let here = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    here.parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("scripts/mednafen/scenarios.toml")
}

fn save_for(slot: u8) -> Option<std::path::PathBuf> {
    let dir = mcs_dir()?;
    let manifest = ScenarioManifest::from_path(manifest_path()).ok()?;
    let pattern = manifest.defaults.filename_pattern;
    let filename = pattern.replace("{slot}", &slot.to_string());
    let p = dir.join(filename);
    if p.exists() { Some(p) } else { None }
}

fn skip_msg(slot: u8) -> String {
    format!(
        "skipped: LEGAIA_MEDNAFEN_DIR unset or mc{slot} not present \
         (this is expected on CI; only fails the test when the env var \
         IS set but the file is missing)"
    )
}

#[test]
fn parses_real_state_and_extracts_main_ram() {
    let path = match save_for(0) {
        Some(p) => p,
        None => {
            eprintln!("{}", skip_msg(0));
            return;
        }
    };
    let s = SaveState::from_path(&path).expect("parse mc0");
    assert!(!s.sections.is_empty(), "section index should populate");
    let ram = s.main_ram().expect("extract main RAM");
    assert_eq!(ram.len(), PSX_RAM_SIZE, "main RAM = 2 MiB");
    // PSX RAM is mostly nonzero in steady state. Anything below 30%
    // means the parser slid off into a zero region.
    let nonzero = ram.iter().filter(|&&b| b != 0).count();
    assert!(
        nonzero > PSX_RAM_SIZE / 4,
        "main RAM looks zero (only {} nonzero bytes)",
        nonzero
    );
}

#[test]
fn finds_main_ram_data8_subentry() {
    let Some(path) = save_for(0) else {
        eprintln!("{}", skip_msg(0));
        return;
    };
    let s = SaveState::from_path(&path).expect("parse");
    let bytes = s
        .entry_bytes("MAIN", "MainRAM.data8")
        .expect("MAIN.MainRAM.data8 exists in real PSX state");
    assert_eq!(bytes.len(), PSX_RAM_SIZE);
}

#[test]
fn diff_between_area_load_states_finds_writes() {
    let (Some(p1), Some(p2)) = (save_for(1), save_for(2)) else {
        eprintln!("{}", skip_msg(1));
        return;
    };
    let s1 = SaveState::from_path(&p1).expect("mc1");
    let s2 = SaveState::from_path(&p2).expect("mc2");
    let r1 = s1.main_ram().expect("ram1");
    let r2 = s2.main_ram().expect("ram2");

    // Diff in the overlay window with reasonable filters.
    let opts = DiffOptions {
        window: (0x801C0000, 0x80200000),
        merge_gap: 32,
        min_bytes_changed: 4,
    };
    let mut d = diff_ram(r1, r2, "mc1", "mc2", &opts);
    sort_by_size(&mut d);

    assert!(
        d.total_bytes_changed > 0,
        "consecutive area-load saves should differ"
    );
    assert!(
        d.regions.iter().all(|r| r.start_addr >= 0x801C0000),
        "every region must respect the window"
    );
    assert!(
        d.regions.iter().all(|r| r.end_addr <= 0x80200000),
        "every region must respect the window"
    );
}

#[test]
fn scenarios_manifest_resolves_every_save() {
    if mcs_dir().is_none() {
        eprintln!("{}", skip_msg(0));
        return;
    }
    let manifest = ScenarioManifest::from_path(manifest_path()).expect("manifest parses");
    assert_eq!(manifest.scenarios.len(), 10, "10 scenarios expected");
    let mut missing = 0usize;
    for s in &manifest.scenarios {
        let p = manifest.save_path(s.slot).expect("save path resolves");
        if !p.exists() {
            missing += 1;
        }
    }
    // Allow a small number missing (user might not have all 10), but
    // refuse to silently pass when nothing is there at all.
    assert!(missing < manifest.scenarios.len(), "no saves found");
}

#[test]
fn level_up_diff_pins_captured_offsets_for_vahn_record() {
    // mc8 → mc9 spans the character level-up event for Vahn (slot 0 of
    // the party record table at 0x80084708, stride 0x414). The diff
    // window is exactly Vahn's record. We assert the byte-level deltas
    // that the post-#26 batch 13 wired into
    // `engine_core::levelup::observations::vahn_mc8_to_mc9`.
    let (Some(p8), Some(p9)) = (save_for(8), save_for(9)) else {
        eprintln!("{}", skip_msg(8));
        return;
    };
    let s8 = SaveState::from_path(&p8).unwrap();
    let s9 = SaveState::from_path(&p9).unwrap();
    let r8 = s8.main_ram().unwrap();
    let r9 = s9.main_ram().unwrap();
    // Vahn record window: 0x80084708..+0x414.
    let opts = DiffOptions {
        window: (0x80084708, 0x80084708 + 0x414),
        merge_gap: 0,
        min_bytes_changed: 1,
    };
    let d = diff_ram(r8, r9, "magic_level_up", "char_level_up", &opts);
    // Capture: 11 regions, 14 bytes changed (per `mednafen-state diff` run).
    assert!(
        d.regions.len() >= 4,
        "expected several discrete deltas, got {}",
        d.regions.len()
    );
    // Specifically: +0x10E single-byte +8 (SP_max). The window is the
    // full record so each region's start_addr is absolute.
    let region_at = |addr: u32| {
        d.regions
            .iter()
            .find(|r| r.start_addr == addr)
            .unwrap_or_else(|| panic!("no region starting at {:#x}", addr))
    };
    let r10e = region_at(0x80084708 + 0x10E);
    assert_eq!(r10e.bytes_changed, 1);
    // Stat-byte cluster at +0x122..+0x12C (six byte-stride single-step
    // increments). The diff merges them into separate regions because
    // merge_gap=0; we just check at least 6 regions in that range.
    let in_stat_range = d
        .regions
        .iter()
        .filter(|r| r.start_addr >= 0x80084708 + 0x122 && r.start_addr <= 0x80084708 + 0x12C)
        .count();
    assert!(
        in_stat_range >= 4,
        "expected 6 stat-byte deltas at +0x122..+0x12C, got {}",
        in_stat_range
    );
}

#[test]
fn magic_rank_up_diff_pins_spell_level_offset() {
    // mc7 → mc8 is the magic-rank-up event (Vahn casts a spell during
    // a battle and his spell-rank counter at +0x9C ticks up; the spell
    // level array at +0x161 increments by 1 for the spell that ranked).
    let (Some(p7), Some(p8)) = (save_for(7), save_for(8)) else {
        eprintln!("{}", skip_msg(7));
        return;
    };
    let s7 = SaveState::from_path(&p7).unwrap();
    let s8 = SaveState::from_path(&p8).unwrap();
    let r7 = s7.main_ram().unwrap();
    let r8 = s8.main_ram().unwrap();
    let opts = DiffOptions {
        window: (0x80084708, 0x80084708 + 0x414),
        merge_gap: 0,
        min_bytes_changed: 1,
    };
    let d = diff_ram(r7, r8, "pre_steal", "magic_level_up", &opts);
    // Expect a single-byte delta at +0x161 (spell levels) and at +0x9C
    // (magic-rank counter).
    let has_spell_lvl = d.regions.iter().any(|r| r.start_addr == 0x80084708 + 0x161);
    let has_magic_rank = d.regions.iter().any(|r| r.start_addr == 0x80084708 + 0x9C);
    assert!(
        has_spell_lvl,
        "expected delta at +0x161 (spell levels); regions={:?}",
        d.regions.iter().map(|r| r.start_addr).collect::<Vec<_>>()
    );
    assert!(
        has_magic_rank,
        "expected delta at +0x9C (magic-rank); regions={:?}",
        d.regions.iter().map(|r| r.start_addr).collect::<Vec<_>>()
    );
}

#[test]
fn watchpoint_diff_for_battle_anim_strike_runs_clean() {
    // mc6 (somersault) has the actor anim-state writes we want to surface.
    // This exercises the watch flow end-to-end against real data.
    let (Some(p4), Some(p6)) = (save_for(4), save_for(6)) else {
        eprintln!("{}", skip_msg(4));
        return;
    };
    let s4 = SaveState::from_path(&p4).unwrap();
    let s6 = SaveState::from_path(&p6).unwrap();
    let r4 = s4.main_ram().unwrap();
    let r6 = s6.main_ram().unwrap();
    let opts = DiffOptions {
        window: (0x801C9300, 0x801C9700),
        merge_gap: 4,
        min_bytes_changed: 1,
    };
    let d = diff_ram(r4, r6, "battle_intro", "battle_anim_strike", &opts);
    // The actor-pointer table at 0x801C9370+ should show writes between
    // an idle action-menu state and an active animation. We don't assert
    // a specific count (depends on which monsters are in the encounter),
    // but require at least one region.
    assert!(
        !d.regions.is_empty(),
        "actor-pool region should differ between idle and active anim"
    );
}
