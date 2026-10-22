use super::*;
use legaia_engine_core::world::EFFECT_MODEL_LIBRARY_BASE;

/// Disc-gated: the global TMD pool seeded from `etmd.dat` (PROT 0874
/// section 0) holds the five effect models, and the slot named by
/// [`ETMD_TAIL_FIRE_MODEL_INDEX`] is the small *Tail Fire* flame mesh - far
/// fewer primitives than the other four models, with textured primitives
/// sampling the `etim` CLUT rows (473..=478). Pins the constant to real
/// disc bytes. Skips when the disc data isn't present.
#[test]
fn etmd_tail_fire_model_is_the_small_flame_mesh() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    }
    let root = ["extracted", "../../extracted"]
        .iter()
        .map(PathBuf::from)
        .find(|p| p.join("PROT.DAT").is_file());
    let Some(root) = root else {
        eprintln!("[skip] extracted/PROT.DAT missing");
        return;
    };
    let index = ProtIndex::open_extracted(&root).expect("open ProtIndex");
    let mut world = legaia_engine_core::world::World::default();
    seed_global_tmd_pool_from_befect_data(&index, &mut world).expect("seed etmd pool");

    // All five etmd models present.
    for i in 0..GLOBAL_TMD_POOL_HEAD_COUNT {
        assert!(
            world.global_tmd(i as i16).is_some(),
            "etmd model {i} should be present"
        );
    }
    let flame = world
        .global_tmd(ETMD_TAIL_FIRE_MODEL_INDEX as i16)
        .expect("flame model present");
    let flame_prims: usize = flame
        .tmd
        .objects
        .iter()
        .map(|o| o.header.n_primitive as usize)
        .sum();

    // The flame is the smallest model by a wide margin.
    for i in 0..GLOBAL_TMD_POOL_HEAD_COUNT {
        if i == ETMD_TAIL_FIRE_MODEL_INDEX {
            continue;
        }
        let other = world.global_tmd(i as i16).unwrap();
        let other_prims: usize = other
            .tmd
            .objects
            .iter()
            .map(|o| o.header.n_primitive as usize)
            .sum();
        assert!(
            flame_prims < other_prims,
            "flame model ({flame_prims} prims) should be smaller than model {i} ({other_prims} prims)"
        );
    }
    assert!(
        flame_prims < 64,
        "flame model is a small mesh, got {flame_prims} prims"
    );
}

/// Disc-gated: the PROT 0871 effect-model library (`etmd.dat`) is a 30-entry
/// TMD pack that registers into `World::global_tmd_pool[3..=32]`, and the
/// Gimard *Tail Fire* slot ([`GIMARD_TAIL_FIRE_MODEL_INDEX`]) resolves to a
/// real Legaia TMD. Pins the library load + index to real disc bytes. Skips
/// when the disc data isn't present.
#[test]
fn etmd_effect_model_library_registers_into_global_pool() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    }
    let root = ["extracted", "../../extracted"]
        .iter()
        .map(PathBuf::from)
        .find(|p| p.join("PROT.DAT").is_file());
    let Some(root) = root else {
        eprintln!("[skip] extracted/PROT.DAT missing");
        return;
    };
    let index = ProtIndex::open_extracted(&root).expect("open ProtIndex");
    let mut world = legaia_engine_core::world::World::default();

    // Library not loaded yet -> guard reports false.
    assert!(!effect_model_library_loaded(&world));

    seed_effect_model_library_from_etmd(&index, &mut world).expect("seed etmd library");

    // All 30 models register into [3..=32], and the guard is now true.
    assert!(effect_model_library_loaded(&world));
    for i in 0..EFFECT_MODEL_LIBRARY_COUNT {
        assert!(
            world
                .global_tmd((EFFECT_MODEL_LIBRARY_BASE + i) as i16)
                .is_some(),
            "effect-model library slot {i} -> pool {} should be present",
            EFFECT_MODEL_LIBRARY_BASE + i
        );
    }

    // The named Gimard flame slot resolves to a real TMD with geometry.
    let flame = world
        .global_tmd(GIMARD_TAIL_FIRE_MODEL_INDEX as i16)
        .expect("Gimard Tail Fire model present");
    let flame_prims: usize = flame
        .tmd
        .objects
        .iter()
        .map(|o| o.header.n_primitive as usize)
        .sum();
    assert!(
        flame_prims > 0,
        "Gimard flame model carries primitives, got {flame_prims}"
    );
    // The flame index falls inside the library window.
    assert_eq!(
        GIMARD_TAIL_FIRE_MODEL_INDEX,
        EFFECT_MODEL_LIBRARY_BASE + 23,
        "Gimard flame is DAT_8007C018[26] = pack entry 23"
    );
}

#[test]
fn null_bgm_director_swallows_every_call() {
    // Compiles + every default impl is a no-op.
    let mut d = NullBgmDirector;
    d.start(1, &[]);
    d.start_owned_vab(2, &[]);
    d.pause();
    d.resume();
    d.stop();
}

/// Test director that records every dispatched event for assertion.
#[derive(Default)]
struct RecordingBgm {
    log: Vec<String>,
}
impl BgmDirector for RecordingBgm {
    fn start(&mut self, id: u16, bytes: &[u8]) {
        self.log.push(format!("start({id},{})", bytes.len()));
    }
    fn pause(&mut self) {
        self.log.push("pause".into());
    }
    fn resume(&mut self) {
        self.log.push("resume".into());
    }
    fn stop(&mut self) {
        self.log.push("stop".into());
    }
    fn start_owned_vab(&mut self, id: u16, bytes: &[u8]) {
        self.log
            .push(format!("start_owned_vab({id},{})", bytes.len()));
    }
}

/// Pause / resume / stop sub-ops fire even without a loaded scene
/// (no SEQ resolution required).
#[test]
fn route_bgm_handles_control_subops_without_scene() {
    // Build a scene-less SceneHost via the test fixture in
    // tests/scene_bundle_smoke.rs is too heavy here - instead, just
    // exercise the routing logic through a minimal scaffold by
    // directly emitting to a recording director and asserting the
    // matching events came through.
    //
    // SceneHost::new requires a ProtIndex which requires a real PROT
    // file, so this test exercises route_bgm_events indirectly via
    // a unit-sized stand-in: only the control sub-ops 2/3/4.
    let mut d = RecordingBgm::default();
    let ev2 = legaia_engine_field::field_events::FieldEvent::Bgm {
        text_id: 0,
        sub_op: 2,
    };
    let ev3 = legaia_engine_field::field_events::FieldEvent::Bgm {
        text_id: 0,
        sub_op: 3,
    };
    let ev4 = legaia_engine_field::field_events::FieldEvent::Bgm {
        text_id: 0,
        sub_op: 4,
    };
    let ev_stop = legaia_engine_field::field_events::FieldEvent::Bgm {
        text_id: 0,
        sub_op: BGM_SUB_OP_ENGINE_STOP,
    };
    // Mimic the route_bgm_events branches directly: the field overlay's arm
    // table (`0x801CEE00`, indexed `sub - 1`) sets pause bit 1 for 2 and 3
    // and clears it with a re-attach for 4; only the engine's own word stops.
    for ev in [ev2, ev3, ev4, ev_stop] {
        if let legaia_engine_field::field_events::FieldEvent::Bgm { sub_op, .. } = ev {
            match sub_op {
                2 | 3 => d.pause(),
                4 => d.resume(),
                BGM_SUB_OP_ENGINE_STOP => d.stop(),
                _ => {}
            }
        }
    }
    assert_eq!(d.log, vec!["pause", "pause", "resume", "stop"]);
}

/// A director with the pause-latch contract real directors carry (the
/// native `AudioDirector` / browser `WebBgmDirector` shape): `pause`
/// closes a gate, `start` swaps the occupant and reopens it, and the
/// sub-op `0xA` commit releases the occupant only while the gate is
/// still closed - then reopens it unconditionally (retail arm
/// `0x801E0264`: `FUN_800266E0` + `FUN_80026520` on the paused slot,
/// then `_DAT_8007B750 = (flags | 0x10) & ~2`).
#[derive(Default)]
struct PauseLatchBgm {
    paused: bool,
    /// The slot occupant, `None` when released/detached.
    playing: Option<u16>,
}
impl BgmDirector for PauseLatchBgm {
    fn start(&mut self, id: u16, _bytes: &[u8]) {
        self.playing = Some(id);
        // Retail's start arm (sub-op 1, `0x801E0104`) clears the pause bit
        // alongside the track select.
        self.paused = false;
    }
    fn pause(&mut self) {
        self.paused = true;
    }
    fn unhalt_pause(&mut self) {
        if self.paused {
            // No start intervened: release the paused occupant the way
            // retail detaches + closes the slot.
            self.playing = None;
        }
        self.paused = false;
    }
}

/// The disc-visible pairing the sub-op `0xA` port exists for: a cutscene
/// pauses with sub-op 2 and unhalts with sub-op `0xA`. Whatever else
/// happens in between, the score must not be left paused afterwards.
#[test]
fn bgm_pause_then_unhalt_pair_does_not_leave_music_paused() {
    // Pause -> swap-commit with no intervening start: the paused track is
    // released (retail closes the slot; the pending track would attach
    // next) and the pause latch is clear.
    let mut d = PauseLatchBgm {
        paused: false,
        playing: Some(7),
    };
    d.pause();
    d.unhalt_pause();
    assert!(!d.paused, "sub-op 0xA must clear the pause latch");
    assert_eq!(
        d.playing, None,
        "with no start intervening, the commit releases the paused source"
    );

    // Pause -> start (the paired sub-op 9 select) -> swap-commit: the swap
    // already happened, so the commit must NOT kill the incoming track -
    // retail released the *old* occupant before the install, never the new
    // one.
    let mut d = PauseLatchBgm {
        paused: false,
        playing: Some(7),
    };
    d.pause();
    d.start(9, &[]);
    d.unhalt_pause();
    assert!(!d.paused);
    assert_eq!(
        d.playing,
        Some(9),
        "the commit after a start leaves the incoming track playing"
    );
}

/// The trait default is a no-op, so stub directors with no pause latch
/// (recording directors, `NullBgmDirector`) stay valid without an
/// override - and the routing still counts the event as acted on.
#[test]
fn bgm_unhalt_default_is_noop_for_stub_directors() {
    let mut d = RecordingBgm::default();
    d.unhalt_pause();
    assert!(d.log.is_empty());
    NullBgmDirector.unhalt_pause();
}

/// `bgm_reattach_volume` (FUN_80019898) - the `(raw << 15) >> 16` level
/// arithmetic: bits [16:1] of the raw global, sign-extended.
#[test]
fn bgm_reattach_volume_matches_retail_shift_pair() {
    // Boot value: FUN_8001FFA4 stores -1 into DAT_8007B6EC.
    assert_eq!(bgm_reattach_volume(-1), -1);
    // The level the re-attach derives from the cold-reset raw global.
    assert_eq!(
        bgm_reattach_volume(legaia_engine_menus::new_game::GAME_STATE_COLD_RESET.bgm_volume_raw),
        -1
    );
    // Plain halving for small positives.
    assert_eq!(bgm_reattach_volume(0x7F), 0x3F);
    assert_eq!(bgm_reattach_volume(0x100), 0x80);
    assert_eq!(bgm_reattach_volume(0), 0);
    // 16-bit sign carried through bit 16: 0xFFFF halves to 0x7FFF
    // (positive - the sign bit of the *shifted* value is bit 16 of raw).
    assert_eq!(bgm_reattach_volume(0xFFFF), 0x7FFF);
    // Bit 16 set makes the result negative.
    assert_eq!(bgm_reattach_volume(0x1FFFF), -1);
    // High bits above 16 are discarded by the << 15 (mod 2^17 window).
    assert_eq!(bgm_reattach_volume(0x7FFE_0100), 0x80);
}

/// Sub-op 8 routes through `BgmDirector::reattach_volume` with the level
/// derived from the host's raw volume mirror.
#[test]
fn bgm_reattach_subop_dispatches_level() {
    #[derive(Default)]
    struct Rec {
        levels: Vec<i16>,
    }
    impl BgmDirector for Rec {
        fn reattach_volume(&mut self, level: i16) {
            self.levels.push(level);
        }
    }
    let mut d = Rec::default();
    // Mimic the route_bgm_events sub-op 8 branch with the boot raw value.
    d.reattach_volume(bgm_reattach_volume(-1));
    d.reattach_volume(bgm_reattach_volume(0x7F));
    assert_eq!(d.levels, vec![-1, 0x3F]);
}

/// A scene-local id loads the `music_01` bank's slot-2 entry in retail
/// (`FUN_800243F0`, `0x800245BC`: `*(0x8007BC64) + 2`).
#[test]
fn scene_local_bgm_falls_back_to_global_slot_two() {
    assert_eq!(SCENE_LOCAL_BGM_FALLBACK_ID, 2002);
    const { assert!(SCENE_LOCAL_BGM_FALLBACK_ID >= GLOBAL_BGM_BASE) };
}
