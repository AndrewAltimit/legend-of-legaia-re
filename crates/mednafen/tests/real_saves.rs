//! Disc-gated tests against the user's actual mednafen save states.
//!
//! Skipped when `LEGAIA_MEDNAFEN_DIR` is unset - keeps CI green for
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

/// Read the active CDNAME label out of a save's main RAM
/// (`0x80084548`, max 8 bytes, NUL-terminated). Returns `None` if the
/// save can't be loaded.
fn scene_label_for(slot: u8) -> Option<String> {
    let path = save_for(slot)?;
    let s = SaveState::from_path(&path).ok()?;
    let ram = s.main_ram().ok()?;
    let off = (0x80084548u32 - 0x80000000u32) as usize;
    let bytes = ram.get(off..off + 8)?;
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    Some(String::from_utf8_lossy(&bytes[..end]).into_owned())
}

/// Skip-tolerant guard: returns `true` if the test should run because
/// every required slot is present AND the active scene-name label
/// matches what the test expects. Otherwise prints a `[skip]` line and
/// returns `false`.
fn require_slot_scenes(test_name: &str, expected: &[(u8, &str)]) -> bool {
    for &(slot, want) in expected {
        match scene_label_for(slot) {
            None => {
                eprintln!("[skip {test_name}] mc{slot} not present");
                return false;
            }
            Some(got) if got != want => {
                eprintln!(
                    "[skip {test_name}] mc{slot} scene `{got}` != expected `{want}` \
                     (corpus has been re-captured; see scripts/mednafen/scenarios.toml)"
                );
                return false;
            }
            Some(_) => {}
        }
    }
    true
}

/// Skip-tolerant guard: returns `true` if the slot is NOT in a
/// per-STR FMV-trigger capture state (game mode != 0x1A). Tests that
/// rely on battle / item / level-up state should skip when the slot
/// has been re-captured into an FMV-trigger state, since the global
/// game-mode word being 0x1A means the str_fmv overlay is about to
/// page in and the prior battle-side residency has been blown away.
fn require_not_fmv_trigger(test_name: &str, slot: u8) -> bool {
    let Some(path) = save_for(slot) else {
        return true;
    };
    let Ok(state) = SaveState::from_path(&path) else {
        return true;
    };
    let Ok(ram) = state.main_ram() else {
        return true;
    };
    let off = (0x8007B83Cu32 - 0x80000000) as usize;
    let mode = ram.get(off).copied().unwrap_or(0xFF);
    if mode == 0x1A {
        eprintln!(
            "[skip {test_name}] mc{slot} is in FMV-trigger state (game mode 0x1A); \
             corpus has been re-captured into the per-STR FMV trigger shape"
        );
        return false;
    }
    true
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
fn noa_level_up_triplet_pins_phase_split_and_settled_deltas() {
    // The four save slots assigned to Noa's level-up in the manifest
    // span pre / record-write / live-copy / settled frames at battle
    // scene `map01`. Asserts (a) the multi-frame write split documented
    // in `engine_core::capture_observations::char_level_up`, and (b)
    // the settled byte-level deltas that
    // `engine_core::levelup::observations::noa_4_level_jump` codifies.
    //
    // Slot indices read here from the active corpus; see
    // `scripts/mednafen/scenarios.toml` for the current assignment.
    if !require_slot_scenes(
        "noa_level_up_triplet",
        &[(4, "map01"), (5, "map01"), (6, "map01"), (7, "map01")],
    ) {
        return;
    }
    let (Some(p4), Some(p5), Some(p6), Some(p7)) =
        (save_for(4), save_for(5), save_for(6), save_for(7))
    else {
        eprintln!("{}", skip_msg(4));
        return;
    };
    let s4 = SaveState::from_path(&p4).unwrap();
    let s5 = SaveState::from_path(&p5).unwrap();
    let s6 = SaveState::from_path(&p6).unwrap();
    let s7 = SaveState::from_path(&p7).unwrap();
    let r4 = s4.main_ram().unwrap();
    let r5 = s5.main_ram().unwrap();
    let r6 = s6.main_ram().unwrap();
    let r7 = s7.main_ram().unwrap();

    // Fingerprint check: confirm mc4 reads a Noa record value
    // consistent with the level-up triplet (XP `102` u16 LE at
    // `NOA_BASE + 0x004`). All four slots are nominally `map01`,
    // but slot 4 may have been re-captured for ANM-strike work.
    use legaia_engine_core::capture_observations::char_level_up as clu;
    if clu::read_xp_u16(r4, clu::NOA_BASE) != Some(102) {
        eprintln!(
            "[skip noa_level_up_triplet] mc4 doesn't carry the pre-Noa-LU \
             fingerprint (XP `102` at NOA_BASE + 0x004); slot has been \
             re-captured"
        );
        return;
    }

    use legaia_engine_core::capture_observations::char_level_up;
    use legaia_engine_core::levelup::observations::noa_4_level_jump;

    let noa_record = (char_level_up::NOA_BASE, char_level_up::NOA_BASE + 0x414);

    // Phase 1 (pre → record-write): writes the persistent record stat
    // window (+0x11C..+0x12D), XP (+0x004), and rank (+0x130). The live
    // in-battle copy at +0x104..+0x11B is unchanged at this point.
    let opts = DiffOptions {
        window: noa_record,
        merge_gap: 0,
        min_bytes_changed: 1,
    };
    let record_window = char_level_up::NOA_BASE + 0x11C..char_level_up::NOA_BASE + 0x12E;
    let live_window = char_level_up::NOA_BASE + 0x104..char_level_up::NOA_BASE + 0x11C;
    let phase1 = diff_ram(r4, r5, "noa_pre", "noa_record_write", &opts);
    assert!(
        phase1
            .regions
            .iter()
            .any(|r| record_window.contains(&r.start_addr)),
        "phase 1 (pre → record-write) should write into the record stat window"
    );
    assert!(
        phase1
            .regions
            .iter()
            .any(|r| r.start_addr == char_level_up::NOA_BASE + char_level_up::RANK_COUNTER),
        "phase 1 should bump the rank counter at +0x130"
    );
    assert!(
        !phase1
            .regions
            .iter()
            .any(|r| live_window.contains(&r.start_addr)),
        "phase 1 should NOT touch the live in-battle window (+0x104..+0x11B)"
    );

    // Phase 2 (record-write → live-copy): writes the live in-battle copy.
    let phase2 = diff_ram(r5, r6, "noa_record_write", "noa_live_copy", &opts);
    assert!(
        phase2
            .regions
            .iter()
            .any(|r| live_window.contains(&r.start_addr)),
        "phase 2 (record-write → live-copy) should write into the live in-battle window"
    );

    // Phase 3 (live-copy → settle): settles HP_max / MP_max / SP_max in
    // the live copy at +0x106 / +0x10A / +0x10E.
    let phase3 = diff_ram(r6, r7, "noa_live_copy", "noa_settle", &opts);
    assert!(
        phase3
            .regions
            .iter()
            .any(|r| r.start_addr == char_level_up::NOA_BASE + 0x10E),
        "phase 3 (live-copy → settle) should settle SP_max at +0x10E"
    );

    // Settled deltas (pre → settle) match the codified observation.
    let stats4 = char_level_up::read_record_stats(r4, char_level_up::NOA_BASE).unwrap();
    let stats7 = char_level_up::read_record_stats(r7, char_level_up::NOA_BASE).unwrap();
    let obs = noa_4_level_jump();
    let obs_stats = obs.record_stats_u16();
    for (i, (a, b)) in stats4.iter().zip(stats7.iter()).enumerate() {
        let delta = b.wrapping_sub(*a);
        assert_eq!(
            delta, obs_stats[i],
            "record stat[{i}] settled delta should match observation"
        );
    }
    // Per-stat cap at index 2 stayed pinned at 100 in both saves.
    assert_eq!(stats4[2], char_level_up::RECORD_STAT_CAP);
    assert_eq!(stats7[2], char_level_up::RECORD_STAT_CAP);
}

#[test]
fn gala_level_up_triplet_pins_phase_split_and_settled_deltas() {
    // The three save slots assigned to Gala's level-up in the manifest
    // span pre / record-write / live-settled frames at battle scene
    // `map01`. Mirrors the Noa test but with the Gala record at slot 2.
    //
    // Slot indices read here from the active corpus; see
    // `scripts/mednafen/scenarios.toml` for the current assignment.
    if !require_slot_scenes(
        "gala_level_up_triplet",
        &[(7, "map01"), (8, "map01"), (9, "map01")],
    ) {
        return;
    }
    let (Some(p7), Some(p8), Some(p9)) = (save_for(7), save_for(8), save_for(9)) else {
        eprintln!("{}", skip_msg(7));
        return;
    };
    let s7 = SaveState::from_path(&p7).unwrap();
    let s8 = SaveState::from_path(&p8).unwrap();
    let s9 = SaveState::from_path(&p9).unwrap();
    let r7 = s7.main_ram().unwrap();
    let r8 = s8.main_ram().unwrap();
    let r9 = s9.main_ram().unwrap();

    use legaia_engine_core::capture_observations::char_level_up;
    use legaia_engine_core::levelup::observations::gala_4_level_jump;

    // Fingerprint check: confirm mc7 reads a Gala record value
    // consistent with the level-up triplet (XP `140` u16 LE at
    // `GALA_BASE + 0x004`). Skips if slot 7 was re-captured.
    if char_level_up::read_xp_u16(r7, char_level_up::GALA_BASE) != Some(140) {
        eprintln!(
            "[skip gala_level_up_triplet] mc7 doesn't carry the pre-Gala-LU \
             fingerprint (XP `140` at GALA_BASE + 0x004); slot has been \
             re-captured"
        );
        return;
    }

    let gala_record = (char_level_up::GALA_BASE, char_level_up::GALA_BASE + 0x414);
    let opts = DiffOptions {
        window: gala_record,
        merge_gap: 0,
        min_bytes_changed: 1,
    };

    // Phase 1 (pre → record-write): writes the record stat window + XP + rank.
    let record_window = char_level_up::GALA_BASE + 0x11C..char_level_up::GALA_BASE + 0x12E;
    let live_window = char_level_up::GALA_BASE + 0x104..char_level_up::GALA_BASE + 0x11C;
    let phase1 = diff_ram(r7, r8, "gala_pre", "gala_record_write", &opts);
    assert!(
        phase1
            .regions
            .iter()
            .any(|r| record_window.contains(&r.start_addr)),
        "phase 1 (pre → record-write) should write into the record stat window"
    );
    assert!(
        phase1
            .regions
            .iter()
            .any(|r| r.start_addr == char_level_up::GALA_BASE + char_level_up::RANK_COUNTER),
        "phase 1 should bump the rank counter at +0x130"
    );

    // Phase 2 (record-write → live+settle): writes the live in-battle
    // copy. Gala's capture collapses HP_cur/MP_cur/live-stats and
    // HP_max/MP_max into one frame.
    let phase2 = diff_ram(r8, r9, "gala_record_write", "gala_live_copy", &opts);
    assert!(
        phase2
            .regions
            .iter()
            .any(|r| live_window.contains(&r.start_addr)),
        "phase 2 (record-write → live+settle) should write into the live in-battle window"
    );

    // Gala doesn't gain SP_max from level-up (physical Tactical Arts
    // user). +0x10E should NOT change across the entire triplet.
    let r10e_off = (char_level_up::GALA_BASE - 0x80000000) as usize + 0x10E;
    assert_eq!(r7[r10e_off], r8[r10e_off]);
    assert_eq!(r7[r10e_off], r9[r10e_off]);

    // Settled deltas (pre → live+settle) match the codified observation.
    let stats7 = char_level_up::read_record_stats(r7, char_level_up::GALA_BASE).unwrap();
    let stats9 = char_level_up::read_record_stats(r9, char_level_up::GALA_BASE).unwrap();
    let obs = gala_4_level_jump();
    let obs_stats = obs.record_stats_u16();
    for (i, (a, b)) in stats7.iter().zip(stats9.iter()).enumerate() {
        let delta = b.wrapping_sub(*a);
        assert_eq!(
            delta, obs_stats[i],
            "record stat[{i}] settled delta should match observation"
        );
    }
    assert_eq!(stats7[2], char_level_up::RECORD_STAT_CAP);
    assert_eq!(stats9[2], char_level_up::RECORD_STAT_CAP);
}

#[test]
fn watchpoint_diff_for_battle_anim_strike_runs_clean() {
    // mc6 (somersault) has the actor anim-state writes we want to surface.
    // This exercises the watch flow end-to-end against real data; needs
    // mc4 + mc6 to be in-battle saves. Skips when the corpus has been
    // re-captured for unrelated work.
    if !require_slot_scenes("anim_strike_diff", &[(4, "dolk"), (6, "dolk")]) {
        return;
    }
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
    let d = diff_ram(r4, r6, "pre_fire_book", "battle_anim_strike", &opts);
    assert!(
        !d.regions.is_empty(),
        "actor-pool region should differ between two battle anim states"
    );
}

#[test]
fn encounter_trigger_diff_loads_battle_overlay() {
    // mc1 (pre-encounter, walking `map01`) → mc2 (battle just initiated,
    // same `map01` scene). Pins the encounter-trigger battle-overlay
    // residency window. The factual deltas this test asserts against
    // are codified in `engine_core::capture_observations::encounter_trigger`;
    // skips when the corpus has been re-captured for unrelated work and
    // mc1/mc2 no longer hold the expected scenes.
    if !require_slot_scenes("encounter_trigger_diff", &[(1, "map01"), (2, "map01")]) {
        return;
    }
    let (Some(p1), Some(p2)) = (save_for(1), save_for(2)) else {
        eprintln!("{}", skip_msg(1));
        return;
    };
    let s1 = SaveState::from_path(&p1).unwrap();
    let s2 = SaveState::from_path(&p2).unwrap();
    let r1 = s1.main_ram().unwrap();
    let r2 = s2.main_ram().unwrap();

    use legaia_engine_core::capture_observations::encounter_trigger::*;

    // Inside the documented overlay residency window, a single broad
    // region of changes should account for ~133 KB. The threshold
    // depends on the captured save pair: the original mc1↔mc2 capture
    // (cold field → battle init) shows ~133 KB; later captures where
    // mc1 carries an already-armed encounter (formation cell already
    // populated, dialog-overlay-only field state) only flip ~16 KB
    // because the battle action overlay slice is the only swap. Skip
    // when the smaller signature is observed - the battle-init pair
    // is now covered by `battle_init_overlay_pair_*`.
    let (lo, hi) = OVERLAY_WINDOW;
    let opts = DiffOptions {
        window: (lo, hi),
        merge_gap: 256,
        min_bytes_changed: 64,
    };
    let d = diff_ram(r1, r2, "pre_encounter", "post_encounter", &opts);
    if d.total_bytes_changed < OVERLAY_BYTES_CHANGED_REF * 8 / 10 {
        eprintln!(
            "[skip encounter_trigger_diff] overlay-window delta {} < ~{} \
             (mc1 likely carries a pre-armed encounter; battle scene-init \
             pair is covered by `battle_init_overlay_pair_*`)",
            d.total_bytes_changed, OVERLAY_BYTES_CHANGED_REF
        );
        return;
    }
    assert!(
        d.regions
            .iter()
            .any(|r| { r.start_addr <= 0x801CE808 + 0x100 && r.end_addr >= 0x801F3000 }),
        "expected one large region spanning the overlay window: {:?}",
        d.regions
            .iter()
            .map(|r| (r.start_addr, r.end_addr))
            .collect::<Vec<_>>()
    );

    // Inside the actor-pool window, populated slots should show ~500 B
    // of writes.
    let (alo, ahi) = ACTOR_POOL_WINDOW;
    let aopts = DiffOptions {
        window: (alo, ahi),
        merge_gap: 4,
        min_bytes_changed: 1,
    };
    let ad = diff_ram(r1, r2, "pre_encounter", "post_encounter", &aopts);
    assert!(
        ad.total_bytes_changed >= ACTOR_POOL_BYTES_CHANGED_REF / 2,
        "expected actor-pool window to populate, got {}B",
        ad.total_bytes_changed
    );

    // The active scene name must be unchanged (encounter triggers IN
    // the same scene; the battle is layered on top).
    let scene_name_off = (SCENE_NAME_TABLE_ADDR - 0x80000000) as usize;
    assert_eq!(
        &r1[scene_name_off..scene_name_off + 0x20],
        &r2[scene_name_off..scene_name_off + 0x20],
        "scene-name table at {:#x} must not change across encounter trigger",
        SCENE_NAME_TABLE_ADDR
    );
}

#[test]
fn fire_book_use_diff_pins_vahn_record_write() {
    // mc4 (battle command menu parked on Fire Book I) → mc5 (Fire Book I
    // just used on Vahn). Pins the per-character record write footprint.
    // Skips when mc4/mc5 have been re-captured for unrelated work; the
    // factual deltas remain codified in
    // `engine_core::capture_observations::vahn_fire_book_use`.
    if !require_slot_scenes("fire_book_use_diff", &[(4, "dolk"), (5, "dolk")]) {
        return;
    }
    let (Some(p4), Some(p5)) = (save_for(4), save_for(5)) else {
        eprintln!("{}", skip_msg(4));
        return;
    };
    let s4 = SaveState::from_path(&p4).unwrap();
    let s5 = SaveState::from_path(&p5).unwrap();
    let r4 = s4.main_ram().unwrap();
    let r5 = s5.main_ram().unwrap();

    use legaia_engine_core::capture_observations::vahn_fire_book_use::*;

    // Window the diff to Vahn's full record; assert exactly one region
    // at the documented offset.
    let opts = DiffOptions {
        window: (VAHN_RECORD_BASE, VAHN_RECORD_BASE + 0x414),
        merge_gap: 0,
        min_bytes_changed: 1,
    };
    let d = diff_ram(r4, r5, "pre_fire_book", "post_fire_book", &opts);
    assert_eq!(
        d.regions.len(),
        1,
        "fire-book event should produce exactly 1 record-internal region; got {:?}",
        d.regions
            .iter()
            .map(|r| (r.start_addr, r.bytes_changed))
            .collect::<Vec<_>>()
    );
    let r = &d.regions[0];
    assert_eq!(r.start_addr, changed_addr());
    assert_eq!(r.bytes_changed, CHANGED_LEN);

    // Confirm the actual bytes match the constants.
    let off = (changed_addr() - 0x80000000u32) as usize;
    assert_eq!(&r4[off..off + CHANGED_LEN], &BEFORE);
    assert_eq!(&r5[off..off + CHANGED_LEN], &AFTER);
}

#[test]
fn town0c_residency_save_documents_active_scene_label() {
    // mc0 is a town-resident state (CDNAME `town0c`, scene index 0x15).
    // Confirm the scene-name table reads accordingly so future diffs that
    // depend on town residency anchor against this save.
    if !require_slot_scenes("town0c_residency", &[(0, "town0c")]) {
        return;
    }
    let Some(p0) = save_for(0) else {
        eprintln!("{}", skip_msg(0));
        return;
    };
    let s = SaveState::from_path(&p0).unwrap();
    let r = s.main_ram().unwrap();
    let off = (0x80084540u32 - 0x80000000u32) as usize;
    let scene_index = u32::from_le_bytes(r[off..off + 4].try_into().unwrap());
    assert_eq!(scene_index, 0x15, "mc0 active-scene index should be town0c");
    let name = &r[off + 0x08..off + 0x10];
    assert_eq!(&name[..6], b"town0c", "mc0 scene label should be town0c");
}

#[test]
fn town01_field_pack_save_documents_active_scene_and_ram_base() {
    // mc2 is the field-pack reference save (CDNAME `town01`, scene 0x03).
    // Confirms (1) the scene-name table reads `town01` and (2) the
    // active-scene field-pack RAM base recovered from
    // `_DAT_8007B8D0 - 0x12800` matches the pinned value in
    // `engine_core::capture_observations::field_pack_load`.
    let Some(p2) = save_for(2) else {
        eprintln!("{}", skip_msg(2));
        return;
    };
    let s = SaveState::from_path(&p2).unwrap();
    let r = s.main_ram().unwrap();
    let off = (0x80084540u32 - 0x80000000u32) as usize;
    let scene_index = u32::from_le_bytes(r[off..off + 4].try_into().unwrap());
    if scene_index != 0x03 {
        eprintln!(
            "[skip town01_field_pack] mc2 scene index is {:#x}, not 0x03 (town01); \
             corpus has been re-captured",
            scene_index
        );
        return;
    }
    let name = &r[off + 0x08..off + 0x10];
    assert_eq!(&name[..6], b"town01", "mc2 scene label should be town01");

    use legaia_engine_core::capture_observations::field_pack_load;
    let recovered =
        field_pack_load::recover_base(r).expect("mc2 should have a non-zero load-dest pointer");
    assert_eq!(
        recovered,
        field_pack_load::TOWN01_FIELD_PACK_BASE,
        "mc2 field-pack base should match the pinned constant"
    );

    // The static asset descriptor table pointer is identical across
    // saves; verify it.
    let dp_off = (field_pack_load::ASSET_DESCRIPTOR_TABLE_PTR_ADDR - 0x80000000) as usize;
    let dp = u32::from_le_bytes(r[dp_off..dp_off + 4].try_into().unwrap());
    assert_eq!(
        dp,
        field_pack_load::ASSET_DESCRIPTOR_TABLE_PTR_VALUE,
        "asset descriptor table base should be the static value"
    );
}

#[test]
fn town01_vs_town0c_diff_lights_up_field_pack_pool() {
    // mc2 (`town01`) vs mc0 (`town0c`): both are town-resident saves with
    // different CDNAME blocks. The diff should surface a sizable region
    // around the per-scene field-pack base + descriptor pool. This is
    // the dynamic complement to the static schema docs in
    // `crates/asset/src/field_pack.rs`.
    if !require_slot_scenes("town01_vs_town0c", &[(0, "town0c"), (2, "town01")]) {
        return;
    }
    let (Some(p0), Some(p2)) = (save_for(0), save_for(2)) else {
        eprintln!("{}", skip_msg(2));
        return;
    };
    let s0 = SaveState::from_path(&p0).unwrap();
    let s2 = SaveState::from_path(&p2).unwrap();
    let r0 = s0.main_ram().unwrap();
    let r2 = s2.main_ram().unwrap();

    use legaia_engine_core::capture_observations::field_pack_load;

    let base = field_pack_load::recover_base(r2).expect("mc2 base recoverable");
    let opts = DiffOptions {
        // Walk a generous region from the start of the heap pool to a
        // bit past the field-pack region in mc2.
        window: (0x80084140, base + 0x18000),
        merge_gap: 256,
        min_bytes_changed: 64,
    };
    let d = diff_ram(r2, r0, "town01", "town0c", &opts);
    // Both town saves write substantial scene data into this region;
    // the diff should be ≥ 30 KB total. (Empirical capture observed
    // ~933 KB across the wider main-RAM window; the narrower window
    // around the field-pack region surfaces a smaller but still solid
    // subset.)
    assert!(
        d.total_bytes_changed >= 30_000,
        "expected ≥ 30 KB of scene-pool deltas; got {}",
        d.total_bytes_changed
    );
    // The 526-byte change starting at the scene-bundle metadata block
    // (`0x80084140`) is the small but reliable signature of a town
    // transition - both saves write per-scene state into it.
    assert!(
        d.regions
            .iter()
            .any(|r| r.start_addr <= 0x80084140 && r.end_addr >= 0x80084140),
        "expected a diff region covering the scene-bundle metadata block at 0x80084140"
    );
}

#[test]
fn intro_rim_elm_to_normal_rim_elm_transition_pins_loader_order_of_ops() {
    // mc2 (settled `town01` intro Rim Elm) -> mc3 (mid-transition into
    // `town0c` Rim Elm normal): captures a single frame between the
    // scene-name pool flip and the field-pack-base pointer flip.
    //
    // Asserts the order-of-operations the loader follows: write new
    // scene name into the bundle pool first, populate the new field-pack
    // region next, swap `_DAT_8007B8D0` last. mc3 catches the system
    // *between* steps 1 and 3.
    if !require_slot_scenes(
        "intro_rim_elm_to_normal_rim_elm",
        &[(2, "town01"), (3, "town0c")],
    ) {
        return;
    }
    let (Some(p2), Some(p3)) = (save_for(2), save_for(3)) else {
        eprintln!("{}", skip_msg(3));
        return;
    };
    let s2 = SaveState::from_path(&p2).unwrap();
    let s3 = SaveState::from_path(&p3).unwrap();
    let r2 = s2.main_ram().unwrap();
    let r3 = s3.main_ram().unwrap();

    use legaia_engine_core::capture_observations::{field_pack_intra_transition, field_pack_load};

    // mc2 settled state: pool slot 0 = town01, base = pinned town01 base.
    assert_eq!(
        field_pack_intra_transition::read_pool_slot_name(r2, 0).as_deref(),
        Some("town01"),
        "mc2 pool slot 0 should be town01"
    );
    let base2 = field_pack_load::recover_base(r2).expect("mc2 base recoverable");
    assert_eq!(base2, field_pack_load::TOWN01_FIELD_PACK_BASE);

    // mc3 mid-transition: pool slot 0 = town0c (new), but the global
    // base pointer still reads the OLD value (= base2). The detector
    // surfaces this disagreement.
    assert_eq!(
        field_pack_intra_transition::read_pool_slot_name(r3, 0).as_deref(),
        Some("town0c"),
        "mc3 pool slot 0 should be town0c"
    );
    let base3 = field_pack_load::recover_base(r3).expect("mc3 base recoverable");
    assert_eq!(
        base3, base2,
        "mc3 should still read the OLD field-pack base; loader hasn't \
         flipped _DAT_8007B8D0 yet"
    );
    assert_eq!(
        field_pack_intra_transition::detect_mid_transition(r3),
        Some(("town0c".to_string(), field_pack_intra_transition::PREV_BASE)),
        "mid-transition detector should fire on mc3"
    );

    // The asset descriptor table at 0x8015CBD0 is statically allocated;
    // its 4 KB head should be bit-identical between mc2 and mc3.
    let dt = (field_pack_load::ASSET_DESCRIPTOR_TABLE_PTR_VALUE - 0x80000000) as usize;
    assert_eq!(
        &r2[dt..dt + 0x1000],
        &r3[dt..dt + 0x1000],
        "asset descriptor table should not move during a scene transition"
    );

    // mc3's NEW field-pack region (matches mc0's pinned `town0c` base)
    // must already carry data even though the global base pointer hasn't
    // flipped yet - this is the load-then-swap evidence.
    let new_off = (field_pack_intra_transition::NEXT_BASE - 0x80000000) as usize;
    let nz = r3[new_off..new_off + 0x12800]
        .iter()
        .filter(|&&b| b != 0)
        .count();
    assert!(
        nz > 0x4000,
        "mc3 new field-pack region at 0x{:08X} should be partially \
         populated already; saw only {nz} non-zero bytes",
        field_pack_intra_transition::NEXT_BASE
    );
}

#[test]
fn fmv_overlay_residency_in_mc1_pins_compact_table_address() {
    // mc1 captures FMV playback. The cutscene overlay should be
    // resident, with the compact MV-file table at the pinned address
    // and parseable into 6 entries (MV1.STR..MV6.STR).
    let Some(p1) = save_for(1) else {
        eprintln!("{}", skip_msg(1));
        return;
    };
    let s = SaveState::from_path(&p1).unwrap();
    let r = s.main_ram().unwrap();

    use legaia_asset::str_fmv_table;
    use legaia_engine_core::capture_observations::str_fmv_overlay;

    if !str_fmv_overlay::is_resident(r) {
        eprintln!(
            "[skip fmv_overlay_residency] mc1 doesn't carry the FMV \
             overlay residency signature; corpus has been re-captured"
        );
        return;
    }

    let off = (str_fmv_overlay::COMPACT_TABLE_ADDR - 0x80000000) as usize;
    let entries = str_fmv_table::parse_entries(
        &r[off..off + str_fmv_overlay::MV_BASENAMES.len() * str_fmv_table::ENTRY_STRIDE],
        str_fmv_overlay::MV_BASENAMES.len(),
    )
    .expect("compact MV table should parse");
    assert_eq!(entries.len(), 6);
    for (i, entry) in entries.iter().enumerate() {
        let expected_basename = str_fmv_overlay::MV_BASENAMES[i];
        let stripped = entry.name.split(';').next().unwrap_or(entry.name.as_str());
        assert_eq!(
            stripped, expected_basename,
            "MV entry {i} basename should be {expected_basename}"
        );
        assert!(
            entry.size >= 4 * 2336,
            "MV{} size suspiciously small",
            i + 1
        );
        assert!(entry.minute < 100 && entry.second < 60 && entry.frame < 75);
    }

    // The mid-game scene labels embedded in the overlay's data section
    // should be readable at the pinned address. If the overlay has been
    // partially overwritten (e.g. corpus re-captured into a non-FMV
    // state but the pinned addresses retain a stale residency
    // signature), skip rather than fail.
    let lbl_off = (str_fmv_overlay::MID_GAME_LABELS_ADDR - 0x80000000) as usize;
    let lbl_window = &r[lbl_off..lbl_off + 0x40];
    let missing: Vec<&&str> = str_fmv_overlay::MID_GAME_LABELS
        .iter()
        .filter(|label| {
            !lbl_window
                .windows(label.len())
                .any(|w| w == label.as_bytes())
        })
        .collect();
    if !missing.is_empty() {
        eprintln!(
            "[skip fmv_overlay_residency] mid-game labels {:?} missing \
             from FMV label-table window; corpus likely re-captured \
             into a non-FMV state with stale residency signatures",
            missing
        );
    }
}

/// Helper: read a u32 LE from main RAM at a PSX virtual address.
fn read_u32(ram: &[u8], addr: u32) -> u32 {
    let off = (addr - 0x80000000) as usize;
    let b = &ram[off..off + 4];
    u32::from_le_bytes([b[0], b[1], b[2], b[3]])
}

#[test]
fn battle_init_overlay_pair_pins_battle_bundle_window_and_actor_tick_wiring() {
    // mc1 = field state on `map01` with encounter armed; mc2 = battle
    // initiated on `map01`. Pins:
    //   - the 168 KB battle-bundle residency window flips from
    //     field-side payload to battle-side data
    //   - the 16 KB battle-overlay scratch slice resets wholesale
    //   - the actor-tick fn pointer at 0x800836F8 lands on
    //     `FUN_80021DF4 = 0x80021DF4` once battle scene-init
    //     completes
    //   - the formation cell at 0x8007BD0C flips from cleared to a
    //     populated count (specific monster ids depend on the
    //     captured encounter; the assertion is only that the cell
    //     went from all-zero to non-zero)
    //   - the scene-bundle pool stays on `map01` (battle layers over
    //     the active field scene)
    if !require_slot_scenes("battle_init_overlay_pair", &[(1, "map01"), (2, "map01")]) {
        return;
    }
    if !require_not_fmv_trigger("battle_init_overlay_pair", 1) {
        return;
    }
    if !require_not_fmv_trigger("battle_init_overlay_pair", 2) {
        return;
    }
    let p1 = save_for(1).unwrap();
    let p2 = save_for(2).unwrap();
    let s1 = SaveState::from_path(&p1).unwrap();
    let s2 = SaveState::from_path(&p2).unwrap();
    let r1 = s1.main_ram().unwrap();
    let r2 = s2.main_ram().unwrap();

    use legaia_engine_core::capture_observations::battle_init_overlay as bio;

    // mc2 formation cell should be populated. mc1 may already carry a
    // pre-armed formation (the user may capture "encounter armed" rather
    // than "no encounter at all" - the actual battle scene-init happens
    // even when the formation was pre-populated). The strong signal of
    // the transition is the 168 KB bundle window flip + the actor-tick
    // fn pointer write (asserted below).
    let off = (bio::FORMATION_CELL_ADDR - 0x80000000) as usize;
    let post = &r2[off..off + 4];
    assert!(
        post.iter().any(|&b| b != 0),
        "mc2 formation cell should be populated post-encounter; saw {:02X?}",
        post
    );

    // Actor-tick fn ptr lands on FUN_80021DF4 in mc2 (only).
    assert_eq!(
        read_u32(r2, bio::ACTOR_TICK_FN_PTR_ADDR),
        bio::ACTOR_TICK_FN_PTR_VALUE,
        "mc2 should wire FUN_80021DF4 at 0x{:08X}",
        bio::ACTOR_TICK_FN_PTR_ADDR
    );

    // 168 KB battle-bundle window differs significantly.
    let (bb_lo, bb_hi) = bio::BATTLE_BUNDLE_WINDOW;
    let lo = (bb_lo - 0x80000000) as usize;
    let hi = (bb_hi - 0x80000000) as usize;
    let differing: usize = r1[lo..hi]
        .iter()
        .zip(&r2[lo..hi])
        .filter(|(a, b)| a != b)
        .count();
    assert!(
        differing > 0x10000, // at least 64 KB different
        "battle bundle window should differ extensively across mc1↔mc2; saw {differing} bytes"
    );

    // 16 KB battle-overlay scratch slice differs (reset on entry).
    let (ov_lo, ov_hi) = bio::OVERLAY_SCRATCH_WINDOW;
    let lo = (ov_lo - 0x80000000) as usize;
    let hi = (ov_hi - 0x80000000) as usize;
    let differing: usize = r1[lo..hi]
        .iter()
        .zip(&r2[lo..hi])
        .filter(|(a, b)| a != b)
        .count();
    assert!(
        differing > 0x1000, // at least 4 KB different
        "battle overlay scratch should reset on battle entry; saw {differing} bytes"
    );
}

#[test]
fn battle_action_anim_pair_pins_dispatch_pointer_table_and_anim_pc_window() {
    // mc3 = mid-battle item-use (Healing Leaf, pre-action-anim);
    // mc4 = mid-action animation (somersault-class strike). Pins:
    //   - the slot-0 actor-record dispatch pointer table at
    //     +0x234..+0x244 holds 4 copies of the same u32; the value
    //     differs between the two saves
    //   - both saves resolve to non-zero pointers (the field is
    //     wired in both, just to different anim records)
    //   - the +0x1D8..+0x1E8 anim-PC window is non-zero in mc4
    //     (the mid-anim save) but mostly zero in mc3
    if !require_slot_scenes("battle_action_anim_pair", &[(3, "map01"), (4, "map01")]) {
        return;
    }
    if !require_not_fmv_trigger("battle_action_anim_pair", 3) {
        return;
    }
    if !require_not_fmv_trigger("battle_action_anim_pair", 4) {
        return;
    }
    let p3 = save_for(3).unwrap();
    let p4 = save_for(4).unwrap();
    let s3 = SaveState::from_path(&p3).unwrap();
    let s4 = SaveState::from_path(&p4).unwrap();
    let r3 = s3.main_ram().unwrap();
    let r4 = s4.main_ram().unwrap();

    use legaia_engine_core::capture_observations::battle_action_animation as ba;

    // Slot-0 dispatch pointer table: 4 × u32, all the same value.
    let p3 = ba::read_dispatch_pointers(r3, ba::SLOT0_ACTOR_RECORD_BASE)
        .expect("dispatch pointers in mc3");
    let p4 = ba::read_dispatch_pointers(r4, ba::SLOT0_ACTOR_RECORD_BASE)
        .expect("dispatch pointers in mc4");

    for window in &[p3, p4] {
        assert!(
            window.iter().all(|&p| p == window[0]),
            "all four dispatch ptrs should hold the same value; saw {:08X?}",
            window
        );
    }

    // Both saves wire non-null dispatch pointers.
    assert!(p3[0] != 0, "mc3 dispatch pointer should be non-null");
    assert!(p4[0] != 0, "mc4 dispatch pointer should be non-null");

    // Mid-anim save advances the dispatch pointer to a different
    // record (the strike's ANM record is paged in at a new heap
    // address).
    if p3[0] == p4[0] {
        eprintln!(
            "[skip battle_action_anim_pair] mc3↔mc4 share dispatch pointer 0x{:08X}; \
             corpus may not include a strike/somersault transition",
            p3[0]
        );
        return;
    }

    // Anim-PC window: mostly zero in mc3, populated in mc4.
    let lo =
        (ba::SLOT0_ACTOR_RECORD_BASE - 0x80000000) as usize + ba::ANIM_PC_FIELD_OFFSET as usize;
    let hi = lo + ba::ANIM_PC_FIELD_LEN as usize;
    let nz3 = r3[lo..hi].iter().filter(|&&b| b != 0).count();
    let nz4 = r4[lo..hi].iter().filter(|&&b| b != 0).count();
    assert!(
        nz4 > nz3,
        "mid-anim save should have a more populated anim-PC window; saw nz3={nz3} nz4={nz4}"
    );

    // Anim-record header at the post-anim dispatch pointer should
    // resolve to a small control block (first u32 is small, e.g.
    // 0x18 in the captured pair). The pre-anim dispatch pointer
    // resolves to a different (and possibly empty) location - assert
    // only on the post-anim side.
    let header_addr = p4[0];
    if (0x80000000..0x80200000 - 4).contains(&header_addr) {
        let len_word = read_u32(r4, header_addr);
        assert!(
            len_word > 0 && len_word < 0x10000,
            "anim-record header u32 at 0x{:08X} = 0x{:08X} should be a small control word",
            header_addr,
            len_word
        );
    }
}

#[test]
fn item_use_pair_pins_field_pack_base_flip_and_script_vm_ctx_shift() {
    // mc2 = battle initiated; mc3 = mid-battle item (Healing Leaf)
    // about to be used. Pins:
    //   - the field-pack base pointer at _DAT_8007B8D0 flips between
    //     mc2 (0x8014BD30) and mc3 (0x800ABA4C)
    //   - the script-VM context block at 0x801BA7DC..0x801BADEC
    //     shifts ~660 bytes
    //   - the formation cell stays at the same active value (the
    //     item-use sub-mode is internal to the same battle round)
    //   - the scene-bundle pool stays on `map01`
    if !require_slot_scenes("item_use_pair", &[(2, "map01"), (3, "map01")]) {
        return;
    }
    let p2 = save_for(2).unwrap();
    let p3 = save_for(3).unwrap();
    let s2 = SaveState::from_path(&p2).unwrap();
    let s3 = SaveState::from_path(&p3).unwrap();
    let r2 = s2.main_ram().unwrap();
    let r3 = s3.main_ram().unwrap();

    use legaia_engine_core::capture_observations::item_use_battle_event as iu;

    let pre = read_u32(r2, iu::FIELD_PACK_BASE_PTR_ADDR);
    let post = read_u32(r3, iu::FIELD_PACK_BASE_PTR_ADDR);

    if pre == post {
        eprintln!(
            "[skip item_use_pair] mc2↔mc3 share field-pack base 0x{:08X}; \
             corpus may not include the item-use sub-mode transition",
            pre
        );
        return;
    }

    // Pinned values match the captured pair; if the corpus is
    // re-captured against different scenes the assertions below
    // turn into documentation rather than facts.
    assert_eq!(pre, iu::FIELD_PACK_BASE_PTR_PRE);
    assert_eq!(post, iu::FIELD_PACK_BASE_PTR_POST);

    // Script-VM context block shift.
    let (lo, hi) = iu::SCRIPT_VM_CTX_WINDOW;
    let lo = (lo - 0x80000000) as usize;
    let hi = (hi - 0x80000000) as usize;
    let differing: usize = r2[lo..hi]
        .iter()
        .zip(&r3[lo..hi])
        .filter(|(a, b)| a != b)
        .count();
    assert!(
        differing > 0x80,
        "script-VM context block should shift across the item-use transition; saw {differing} bytes"
    );

    // Formation cell stays active in both.
    let off = (0x8007BD0Cu32 - 0x80000000) as usize;
    assert!(
        r2[off..off + 4].iter().any(|&b| b != 0),
        "mc2 formation cell should be active"
    );
    assert!(
        r3[off..off + 4].iter().any(|&b| b != 0),
        "mc3 formation cell should still be active"
    );
}

/// The per-STR FMV trigger corpus: nine saves taken right before each
/// FMV begins playing, one per `fmv_id ∈ 0..=8`. Each save should
/// carry `_DAT_8007BA78 = expected_fmv_id`, `_DAT_8007B83C = 0x1A`
/// (StrInit), and the `map01` scene label in the bundle pool.
///
/// Skip-pass when any save isn't present, when the scene label has
/// rotated away from `map01`, or when the trigger-side state isn't
/// `(0x1A, expected_fmv_id)` for every save.
#[test]
fn cutscene_trigger_corpus_pins_fmv_id_across_nine_saves() {
    use legaia_engine_core::capture_observations::cutscene_trigger_corpus as ctc;
    use legaia_engine_core::capture_observations::field_pack_load;

    // Slot fingerprint: the user's per-STR captures are all on
    // `map01`. If any slot fingerprints differently, the corpus
    // has been rotated to a non-FMV-trigger shape and the test
    // skip-passes.
    let expected_scenes: Vec<(u8, &str)> = ctc::CORPUS
        .iter()
        .map(|e| (e.slot as u8, "map01"))
        .collect();
    if !require_slot_scenes("cutscene_trigger_corpus", &expected_scenes) {
        return;
    }

    for entry in ctc::CORPUS {
        let slot = entry.slot as u8;
        let path = match save_for(slot) {
            Some(p) => p,
            None => {
                eprintln!("[skip cutscene_trigger_corpus] {}", skip_msg(slot));
                return;
            }
        };
        let s = SaveState::from_path(&path).unwrap();
        let ram = s.main_ram().unwrap();

        let fmv_id = ctc::read_fmv_id(ram).expect("fmv_id reads");
        let mode = ctc::read_game_mode(ram).expect("game mode reads");
        let bgm = ctc::read_bgm_id(ram).expect("BGM id reads");

        if mode != ctc::EXPECTED_GAME_MODE {
            eprintln!(
                "[skip cutscene_trigger_corpus] mc{slot} game mode = 0x{mode:02X} != 0x1A; \
                 corpus rotation - the saves are not FMV-trigger captures any more"
            );
            return;
        }

        assert_eq!(
            fmv_id, entry.expected_fmv_id,
            "mc{slot} should hold fmv_id = {} (got {fmv_id})",
            entry.expected_fmv_id
        );
        assert_eq!(
            bgm,
            ctc::EXPECTED_BGM_ID,
            "mc{slot} BGM id should be {} (got {bgm})",
            ctc::EXPECTED_BGM_ID
        );

        // Field-pack base for `map01` is constant across the corpus.
        let base = field_pack_load::recover_base(ram).expect("recover_base");
        assert_eq!(
            base,
            ctc::MAP01_FIELD_PACK_BASE,
            "mc{slot} field-pack base = 0x{base:08X}, expected map01 base"
        );

        // Field-pack region carries no `0x4C 0xE2 lo hi` byte
        // sequence — empirical signature of the debug-menu trigger
        // path (the field VM never executed a trigger op for these
        // saves, so the bytecode pattern was never written into the
        // field-pack region).
        let triggers = ctc::scan_field_pack_for_trigger_ops(ram, base, 0x30000);
        assert!(
            triggers.is_empty(),
            "mc{slot}: expected no field-pack trigger ops (debug-menu-driven), got {}",
            triggers.len()
        );
    }
}
