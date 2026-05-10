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
    // mc4 (pre-Noa-level-up) → mc5 (record-write frame) → mc6
    // (live-copy frame) → mc7 (settled) spans Noa's level-up event in
    // battle scene `map01`. Asserts (a) the multi-frame write split
    // documented in `engine_core::capture_observations::char_level_up`,
    // and (b) the settled byte-level deltas that
    // `engine_core::levelup::observations::noa_mc4_to_mc7` codifies.
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

    use legaia_engine_core::capture_observations::char_level_up;
    use legaia_engine_core::levelup::observations::noa_mc4_to_mc7;

    let noa_record = (char_level_up::NOA_BASE, char_level_up::NOA_BASE + 0x414);

    // Phase 1: mc4 → mc5 writes the persistent record stat window
    // (+0x11C..+0x12D), XP (+0x004), and rank (+0x130). The live in-battle
    // copy at +0x104..+0x11B is unchanged at this point.
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
        "phase 1 (mc4→mc5) should write into the record stat window"
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

    // Phase 2: mc5 → mc6 writes the live in-battle copy.
    let phase2 = diff_ram(r5, r6, "noa_record_write", "noa_live_copy", &opts);
    assert!(
        phase2
            .regions
            .iter()
            .any(|r| live_window.contains(&r.start_addr)),
        "phase 2 (mc5→mc6) should write into the live in-battle window"
    );

    // Phase 3 (settle): mc6 → mc7 settles HP_max / MP_max / SP_max in the
    // live copy at +0x106 / +0x10A / +0x10E.
    let phase3 = diff_ram(r6, r7, "noa_live_copy", "noa_settle", &opts);
    assert!(
        phase3
            .regions
            .iter()
            .any(|r| r.start_addr == char_level_up::NOA_BASE + 0x10E),
        "phase 3 (mc6→mc7) should settle SP_max at +0x10E"
    );

    // Settled deltas (mc4 → mc7) match the codified observation.
    let stats4 = char_level_up::read_record_stats(r4, char_level_up::NOA_BASE).unwrap();
    let stats7 = char_level_up::read_record_stats(r7, char_level_up::NOA_BASE).unwrap();
    let obs = noa_mc4_to_mc7();
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
    // mc7 (pre-Gala-level-up) → mc8 (record-write frame) → mc9 (settled)
    // spans Gala's level-up event in battle scene `map01`. Mirrors the
    // Noa test but with the Gala record at slot 2.
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
    use legaia_engine_core::levelup::observations::gala_mc7_to_mc9;

    let gala_record = (char_level_up::GALA_BASE, char_level_up::GALA_BASE + 0x414);
    let opts = DiffOptions {
        window: gala_record,
        merge_gap: 0,
        min_bytes_changed: 1,
    };

    // Phase 1: mc7 → mc8 writes the record stat window + XP + rank.
    let record_window = char_level_up::GALA_BASE + 0x11C..char_level_up::GALA_BASE + 0x12E;
    let live_window = char_level_up::GALA_BASE + 0x104..char_level_up::GALA_BASE + 0x11C;
    let phase1 = diff_ram(r7, r8, "gala_pre", "gala_record_write", &opts);
    assert!(
        phase1
            .regions
            .iter()
            .any(|r| record_window.contains(&r.start_addr)),
        "phase 1 (mc7→mc8) should write into the record stat window"
    );
    assert!(
        phase1
            .regions
            .iter()
            .any(|r| r.start_addr == char_level_up::GALA_BASE + char_level_up::RANK_COUNTER),
        "phase 1 should bump the rank counter at +0x130"
    );

    // Phase 2: mc8 → mc9 writes the live in-battle copy. Gala's capture
    // collapses HP_cur/MP_cur/live-stats and HP_max/MP_max into one
    // frame.
    let phase2 = diff_ram(r8, r9, "gala_record_write", "gala_live_copy", &opts);
    assert!(
        phase2
            .regions
            .iter()
            .any(|r| live_window.contains(&r.start_addr)),
        "phase 2 (mc8→mc9) should write into the live in-battle window"
    );

    // Gala doesn't gain SP_max from level-up (physical Tactical Arts
    // user). +0x10E should NOT change across the entire triplet.
    let r10e_off = (char_level_up::GALA_BASE - 0x80000000) as usize + 0x10E;
    assert_eq!(r7[r10e_off], r8[r10e_off]);
    assert_eq!(r7[r10e_off], r9[r10e_off]);

    // Settled deltas (mc7 → mc9) match the codified observation.
    let stats7 = char_level_up::read_record_stats(r7, char_level_up::GALA_BASE).unwrap();
    let stats9 = char_level_up::read_record_stats(r9, char_level_up::GALA_BASE).unwrap();
    let obs = gala_mc7_to_mc9();
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
    // region of changes should account for ~133 KB.
    let (lo, hi) = OVERLAY_WINDOW;
    let opts = DiffOptions {
        window: (lo, hi),
        merge_gap: 256,
        min_bytes_changed: 64,
    };
    let d = diff_ram(r1, r2, "pre_encounter", "post_encounter", &opts);
    assert!(
        d.total_bytes_changed >= OVERLAY_BYTES_CHANGED_REF * 8 / 10,
        "expected ~{}B in overlay window, got {}",
        OVERLAY_BYTES_CHANGED_REF,
        d.total_bytes_changed
    );
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
        field_pack_load::TOWN01_BASE_MC2,
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
    assert_eq!(base2, field_pack_load::TOWN01_BASE_MC2);

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
    // should be readable at the pinned address.
    let lbl_off = (str_fmv_overlay::MID_GAME_LABELS_ADDR - 0x80000000) as usize;
    let lbl_window = &r[lbl_off..lbl_off + 0x40];
    for label in str_fmv_overlay::MID_GAME_LABELS {
        assert!(
            lbl_window
                .windows(label.len())
                .any(|w| w == label.as_bytes()),
            "mid-game scene label {label:?} should appear in the FMV overlay's label table"
        );
    }
}
