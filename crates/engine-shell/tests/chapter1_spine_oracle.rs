//! Arc 1 "Chapter 1 spine" oracle: the opening overworld traversal from Rim
//! Elm (`town01`) out onto the Drake overworld (`map01`) and down into the
//! Ravine dungeon (`keikoku`).
//!
//! **Part A - retail anchors.** The cataloged playthrough anchors codify the
//! retail progression through the spine; each is loaded from `saves/library/`
//! (pcsx-redux `.sstate` via [`legaia_pcsxr`], mednafen `.mcr` via
//! [`legaia_mednafen`] through
//! [`legaia_engine_shell::mode_trace_oracle::load_runtime_mode_trace_from_save`])
//! and its recorded scene / mode (/ position where the loader exposes it) is
//! asserted. Present anchors are checked; absent ones skip. A `checked >= 1`
//! guard keeps the part non-vacuous.
//!
//! | anchor                        | scene   | note                          |
//! |-------------------------------|---------|-------------------------------|
//! | s3_rimelm_freeroam (pcsx)     | town01  | C1 first free-roam, mode 0x03 |
//! | s5_tetsu_battle (pcsx)        | town01  | C2 first battle, mode 0x15    |
//! | door_warp_town01_to_map01     | town0c  | C4 pre-exit capture (see note)|
//! | keikoku_chest_preload (mcr)   | map01   | C5 overworld outside dungeon  |
//! | keikoku_chest_pre / _open     | keikoku | C6 inside the dungeon         |
//!
//! (The `door_warp_town01_to_map01` capture is a *pre*-transition save: its
//! session Rim Elm is the `town0c` scene variant, and it pins town01's single
//! `0x3F` exit -> `map01`; see `scripts/scenarios.toml`. The overworld/WorldMap
//! side of C4 is proven by the engine reproduction in Part B, not this anchor.)
//!
//! **Part B - engine reproduction.** The engine drives the spine itself through
//! a live [`SceneHost`]:
//!   - **Leg 1** (`town01 -> map01`): from free-roam, seating the player on the
//!     south-gate exit tile fires the gate-1 walk-on trigger -> partition-2
//!     `0x3F` record -> `SceneEntered("map01")` in `SceneMode::WorldMap`, seated
//!     at the op's entry tile (0x60, 0x19). This leg is expected to pass today.
//!   - **Leg 2** (`map01 -> keikoku`, the GAP-1 close): walking onto a keikoku
//!     overworld-portal tile emits [`FieldEvent::WorldMapTransition`] and the
//!     host's transition drain loads `keikoku` in `SceneMode::Field`. The portal
//!     set is seeded from the disc's `.MAP` walk-on tile-trigger -> partition-2
//!     record -> `0x3F` bridge (see
//!     [`legaia_engine_core::man_field_scripts::overworld_portal_sites`]).
//!
//! **Replay fixture.** `scripts/replays/chapter1_spine.toml` (`j-replay-v1`)
//! carries the `[[expected]]` spine rows; the disc-gated replay test builds the
//! real full-chain engine trace and diffs the fixture against it, and a
//! disc-free determinism gate drives a synthetic world through the replay twice
//! and asserts byte-identical traces.
//!
//! Skip-pass (CLAUDE.md disc-gated convention): `LEGAIA_DISC_BIN` unset,
//! `extracted/` missing, `scripts/replays/` / `saves/library/` absent.

use std::path::PathBuf;

use legaia_engine_core::field_events::FieldEvent;
use legaia_engine_core::scene::{SceneHost, SceneTickEvent};
use legaia_engine_core::world::{Actor, SceneMode, World, WorldMapEntityConfig};
use legaia_engine_shell::mode_trace_oracle::{ModeTraceFrame, load_runtime_mode_trace_from_save};
use legaia_engine_shell::replay::ReplayFile;
use legaia_mednafen::ScenarioManifest;

// ---------------------------------------------------------------------
// Discovery helpers
// ---------------------------------------------------------------------

fn extracted_dir() -> Option<PathBuf> {
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

fn library_dir() -> Option<PathBuf> {
    for c in ["saves/library", "../saves/library", "../../saves/library"] {
        let d = PathBuf::from(c);
        if d.is_dir() {
            return Some(d);
        }
    }
    None
}

fn manifest_path() -> Option<PathBuf> {
    for c in [
        "scripts/scenarios.toml",
        "../scripts/scenarios.toml",
        "../../scripts/scenarios.toml",
    ] {
        let p = PathBuf::from(c);
        if p.exists() {
            return Some(p);
        }
    }
    None
}

fn replay_path() -> PathBuf {
    for c in [
        "scripts/replays/chapter1_spine.toml",
        "../scripts/replays/chapter1_spine.toml",
        "../../scripts/replays/chapter1_spine.toml",
    ] {
        let p = PathBuf::from(c);
        if p.exists() {
            return p;
        }
    }
    PathBuf::from("scripts/replays/chapter1_spine.toml")
}

fn pcsx_save(fp: &str) -> Option<PathBuf> {
    let p = library_dir()?
        .join("pcsx-redux")
        .join(format!("{fp}.sstate"));
    p.exists().then_some(p)
}

fn scene_mode_name(m: SceneMode) -> &'static str {
    match m {
        SceneMode::Title => "Title",
        SceneMode::Field => "Field",
        SceneMode::Battle => "Battle",
        SceneMode::Cutscene => "Cutscene",
        SceneMode::WorldMap => "WorldMap",
        SceneMode::Dance => "Dance",
        SceneMode::Fishing => "Fishing",
        SceneMode::SlotMachine => "SlotMachine",
        SceneMode::BakaFighter => "BakaFighter",
        SceneMode::MuscleDome => "MuscleDome",
        SceneMode::Menu => "Menu",
    }
}

// ---------------------------------------------------------------------
// Part A: retail anchors
// ---------------------------------------------------------------------

#[test]
fn part_a_spine_anchors_codify_retail_progression() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing");
        return;
    };
    // legaia_pcsxr resolves runtime VAs off the SCUS anchor; point LEGAIA_SCUS
    // at the extracted copy if the harness didn't set it.
    if std::env::var_os("LEGAIA_SCUS").is_none() {
        // SAFETY: single-threaded test setup before any save load.
        unsafe { std::env::set_var("LEGAIA_SCUS", extracted.join("SCUS_942.54")) };
    }

    let mut checked = 0usize;

    // -- pcsx-redux anchors (scene + mode + position) ------------------
    struct PcsxAnchor {
        label: &'static str,
        fp: &'static str,
        scene: &'static str,
        mode: u8,
        pos: (i16, i16),
    }
    const PCSX: &[PcsxAnchor] = &[
        PcsxAnchor {
            label: "s3_rimelm_freeroam",
            fp: "2fba9adf4ade2f14de2a10c82e066b76025ac7ded1f063b852de9d498be00a6a",
            scene: "town01",
            mode: 0x03,
            pos: (4160, 11840),
        },
        PcsxAnchor {
            label: "s5_tetsu_battle",
            fp: "4e9c1e5ffd5972c33da9bdf2304964979037cdfaf77a50df5b03a68c67a55e6f",
            scene: "town01",
            mode: 0x15,
            pos: (0, 0),
        },
    ];
    for a in PCSX {
        let Some(path) = pcsx_save(a.fp) else {
            eprintln!("[skip] {}: no pcsx save", a.label);
            continue;
        };
        let st = legaia_pcsxr::SaveState::from_path(&path).expect("load .sstate");
        eprintln!(
            "[{}] scene={:?} mode=0x{:02X} pos={:?}",
            a.label,
            st.scene_name(),
            st.game_mode(),
            st.player_pos()
        );
        assert_eq!(st.scene_name(), a.scene, "[{}] scene", a.label);
        assert_eq!(st.game_mode(), a.mode, "[{}] game_mode", a.label);
        assert_eq!(st.player_pos(), Some(a.pos), "[{}] player_pos", a.label);
        checked += 1;
    }

    // -- mednafen anchors (scene + mode via the mode-trace reader) ------
    let manifest =
        manifest_path().map(|p| ScenarioManifest::from_path(&p).expect("parse manifest"));
    let library = library_dir();
    struct MedAnchor {
        label: &'static str,
        scene: &'static str,
        // Expected engine-side scene_mode string, or None to just report it.
        scene_mode: Option<&'static str>,
    }
    // C4 door_warp is a *pre*-transition capture (session scene = town0c); C5 is
    // the overworld outside keikoku (map01); C6 is inside the dungeon (keikoku,
    // field). Assert each anchor's recorded active_scene; assert scene_mode only
    // where the mode is unambiguous (the keikoku field captures).
    const MED: &[MedAnchor] = &[
        MedAnchor {
            label: "door_warp_town01_to_map01",
            scene: "town0c",
            scene_mode: Some("Field"),
        },
        MedAnchor {
            label: "keikoku_chest_preload",
            scene: "map01",
            scene_mode: None,
        },
        MedAnchor {
            label: "keikoku_chest_pre",
            scene: "keikoku",
            scene_mode: Some("Field"),
        },
        MedAnchor {
            label: "keikoku_chest_open",
            scene: "keikoku",
            scene_mode: Some("Field"),
        },
    ];
    if let (Some(manifest), Some(library)) = (&manifest, &library) {
        for a in MED {
            let Some(scn) = manifest.scenarios.iter().find(|s| s.label == a.label) else {
                eprintln!("[skip] {}: not in scenarios.toml", a.label);
                continue;
            };
            let Some(save) = manifest.library_save_path(scn, library) else {
                eprintln!("[skip] {}: no library backup", a.label);
                continue;
            };
            if !save.exists() {
                eprintln!("[skip] {}: save missing", a.label);
                continue;
            }
            let frame = load_runtime_mode_trace_from_save(&save)
                .unwrap_or_else(|e| panic!("[{}] load snapshot: {e:#}", a.label));
            eprintln!(
                "[{}] active_scene={:?} scene_mode={} game_mode={:?}",
                a.label, frame.active_scene, frame.scene_mode, frame.game_mode
            );
            assert_eq!(
                frame.active_scene.as_deref(),
                Some(a.scene),
                "[{}] active_scene",
                a.label
            );
            if let Some(want) = a.scene_mode {
                assert_eq!(frame.scene_mode, want, "[{}] scene_mode", a.label);
            }
            checked += 1;
        }
    }

    assert!(checked >= 1, "expected at least one spine anchor present");
    eprintln!("[ok] Part A: checked {checked} spine anchor(s)");
}

// ---------------------------------------------------------------------
// Part B: engine reproduction
// ---------------------------------------------------------------------

/// Rim Elm's south-gate exit tile (the gate-1 walk-on trigger whose
/// partition-2 record's `0x3F` leaves for `map01`). Pinned by
/// `walk_on_trigger_dispatch_disc::town01_exit_tiles_leave_for_the_overworld`.
const TOWN01_SOUTH_GATE: (u8, u8) = (25, 46);
/// The arrival seat on the overworld after the Rim Elm exit (the op-0x3F entry
/// tile 0x60/0x19).
const MAP01_ARRIVAL_TILE: (i16, i16) = (0x60, 0x19);

fn open_host() -> Option<SceneHost> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    let extracted = extracted_dir()?;
    Some(SceneHost::open_extracted(&extracted).expect("open SceneHost"))
}

/// Drive `town01` free-roam onto the overworld and return the entered host,
/// asserting the Leg-1 invariants. `None` when the disc gate skips.
fn drive_town01_to_map01() -> Option<SceneHost> {
    let mut host = open_host()?;
    host.enter_field_scene("town01", 0).expect("enter town01");
    assert_eq!(host.world.mode, SceneMode::Field, "town01 is a field scene");
    // Settle at the cold spawn (no trigger there), then step onto the south
    // gate (the free-roam "hold Up to the south gate" beat, modeled by seating
    // the player on the gate tile - the walk-on dispatch keys off the tile
    // crossing, not the pad).
    for _ in 0..3 {
        if let SceneTickEvent::SceneEntered { name } = host.tick().expect("tick") {
            panic!("unexpected early transition to {name}");
        }
    }
    host.world
        .seat_player_at_tile(TOWN01_SOUTH_GATE.0, TOWN01_SOUTH_GATE.1);
    let mut entered = None;
    for _ in 0..120 {
        if let SceneTickEvent::SceneEntered { name } = host.tick().expect("tick") {
            entered = Some(name);
            break;
        }
    }
    assert_eq!(
        entered.as_deref(),
        Some("map01"),
        "the south-gate trigger's 0x3F record leaves Rim Elm for the overworld"
    );
    assert_eq!(
        host.world.mode,
        SceneMode::WorldMap,
        "map01 routes through the world-map entry"
    );
    let slot = host.world.player_actor_slot.expect("player") as usize;
    let ms = &host.world.actors[slot].move_state;
    assert_eq!(
        ((ms.world_x - 0x40) >> 7, (ms.world_z - 0x40) >> 7),
        MAP01_ARRIVAL_TILE,
        "overworld arrival seats the player at the op-0x3F entry tile (0x60, 0x19)"
    );
    Some(host)
}

/// Tile of the first overworld portal to `dest` on the currently-loaded map.
fn find_portal_tile(host: &SceneHost, dest: &str) -> Option<(u8, u8)> {
    host.world
        .world_map_entity_configs
        .iter()
        .zip(host.world.world_map_entity_positions.iter())
        .find_map(|(cfg, &(x, z))| match cfg {
            WorldMapEntityConfig::OverworldPortal { scene_name, .. } if scene_name == dest => {
                Some(((x >> 7) as u8, (z >> 7) as u8))
            }
            _ => None,
        })
}

#[test]
fn part_b_leg1_town01_south_gate_to_map01() {
    let Some(_host) = drive_town01_to_map01() else {
        return;
    };
    eprintln!(
        "[ok] Leg 1: town01 free-roam -> south gate -> map01 (WorldMap), seated at 0x60/0x19"
    );
}

#[test]
fn part_b_leg2_map01_portal_to_keikoku() {
    let Some(host) = drive_town01_to_map01() else {
        return;
    };

    // The overworld portal set is seeded from the disc's `.MAP` trigger -> P2 ->
    // 0x3F bridge; keikoku (the Ravine) is one of the Drake overworld's
    // destinations.
    let keikoku_tile = find_portal_tile(&host, "keikoku")
        .expect("map01 installs a keikoku overworld portal from the 0x3F bridge");

    // -- Leg 2a: EMIT. Walking onto the portal tile makes the world-map entity
    // SM emit FieldEvent::WorldMapTransition (drive World::tick directly, before
    // the host's transition drain consumes it). This is the GAP-1 producer now
    // reaching a real consumer.
    {
        let mut host = drive_town01_to_map01().expect("re-drive to map01");
        host.world
            .seat_player_at_tile(keikoku_tile.0, keikoku_tile.1);
        host.world.set_pad(0);
        let _ = host.world.tick();
        let emitted = host
            .world
            .pending_field_events
            .iter()
            .any(|e| matches!(e, FieldEvent::WorldMapTransition { .. }));
        assert!(
            emitted,
            "walking onto the keikoku portal tile emits FieldEvent::WorldMapTransition"
        );
    }

    // -- Leg 2b: LOAD. The host tick emits + drains the transition, loading
    // keikoku in field mode (the GAP-1 close).
    {
        let mut host = drive_town01_to_map01().expect("re-drive to map01");
        host.world
            .seat_player_at_tile(keikoku_tile.0, keikoku_tile.1);
        host.world.set_pad(0);
        let mut entered = None;
        for _ in 0..8 {
            if let SceneTickEvent::SceneEntered { name } = host.tick().expect("tick") {
                entered = Some(name);
                break;
            }
        }
        assert_eq!(
            entered.as_deref(),
            Some("keikoku"),
            "the keikoku overworld portal loads the Ravine dungeon"
        );
        assert_eq!(
            host.world.mode,
            SceneMode::Field,
            "keikoku is a field-mode dungeon scene"
        );
    }

    eprintln!(
        "[ok] Leg 2 (GAP-1): map01 keikoku portal at tile {keikoku_tile:?} \
         -> WorldMapTransition -> keikoku (Field)"
    );
}

/// Overworld story-flag gating (the Arc-1 close): on a fresh Drake arrival the
/// Ravine (`keikoku`) entrance is open (`0x193`/403 clear) and the mist-wall
/// force-walk bands fire (`0x482`/1154 clear); flipping each flag flips
/// reachability. Direct `map01` entry is a valid fresh arrival (the portal set
/// + collision grid load the same way the town-exit transition seeds them).
#[test]
fn part_b_leg3_overworld_story_gating() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(_) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing");
        return;
    };

    // -- Keikoku portal gate (0x193 / 403). --------------------------------
    // Fresh arrival: flag clear -> the Ravine entrance is installed + reachable.
    let mut fresh = open_host().expect("open host");
    fresh.enter_world_map_scene("map01").expect("enter map01");
    assert_eq!(fresh.world.mode, SceneMode::WorldMap, "map01 is WorldMap");
    let keikoku_fresh = find_portal_tile(&fresh, "keikoku");
    assert!(
        keikoku_fresh.is_some(),
        "keikoku entrance is reachable on a fresh Drake arrival (0x193 clear)"
    );

    // Flag set -> the entrance drops out of the installed portal set.
    let mut gated = open_host().expect("open host");
    gated.world.system_flag_set(403); // 0x193
    gated.enter_world_map_scene("map01").expect("enter map01");
    assert!(
        find_portal_tile(&gated, "keikoku").is_none(),
        "setting 0x193 gates the keikoku entrance out of the overworld portal set"
    );

    // -- Mist-wall beat band gate (0x482 / 1154). --------------------------
    // The Drake mist-wall force-walk bands are gate-1 partition-2 records
    // (`map01` P2[34..36], C1=[0x482]) with no `0x3F` - beat records the
    // overworld walk-on dispatch spawns as a cutscene timeline while 0x482 is
    // clear, and stops spawning once it latches.
    let band_tile = {
        let scene = fresh.scene.as_ref().expect("scene");
        let (primary, fallback) = scene
            .field_tile_triggers(&fresh.index)
            .expect("map01 tile triggers");
        primary
            .iter()
            .chain(fallback.iter())
            .find(|t| t.gate == 1 && matches!(t.record, 34..=36))
            .map(|t| (t.tile_x, t.tile_z))
    };
    let Some((bx, bz)) = band_tile else {
        // No mist-band trigger present in this build's map01 - skip that half
        // rather than fail; the portal-gate half above stays authoritative.
        eprintln!("[skip] no mist-wall band trigger on map01");
        return;
    };

    // 0x482 clear: crossing onto the band tile spawns the beat timeline.
    let mut open = open_host().expect("open host");
    open.enter_world_map_scene("map01").expect("enter map01");
    walk_onto_tile(&mut open, bx, bz);
    assert!(
        open.world.cutscene_timeline_active(),
        "with 0x482 clear the mist-wall band installs (force-walk) on walk-on"
    );

    // 0x482 set: the C1 one-shot blocks the band - crossing the same tile spawns
    // nothing.
    let mut walled = open_host().expect("open host");
    walled.world.system_flag_set(1154); // 0x482
    walled.enter_world_map_scene("map01").expect("enter map01");
    walk_onto_tile(&mut walled, bx, bz);
    assert!(
        !walled.world.cutscene_timeline_active(),
        "setting 0x482 blocks the mist-wall band (its C1 one-shot latch)"
    );

    eprintln!("[ok] Leg 3: overworld story gating (keikoku 0x193, mist-wall 0x482)");
}

/// Seat the player one tile away then onto `(tx, tz)`, ticking between so the
/// walk-on dispatch sees a genuine tile crossing.
fn walk_onto_tile(host: &mut SceneHost, tx: u8, tz: u8) {
    let seat = |h: &mut SceneHost, x: i16, z: i16| {
        let s = h.world.player_actor_slot.expect("player") as usize;
        h.world.actors[s].move_state.world_x = x * 128 + 0x40;
        h.world.actors[s].move_state.world_z = z * 128 + 0x40;
    };
    host.world.set_pad(0);
    let off = if tx > 0 { tx as i16 - 1 } else { tx as i16 + 1 };
    seat(host, off, tz as i16);
    let _ = host.tick();
    seat(host, tx as i16, tz as i16);
    let _ = host.tick();
}

// ---------------------------------------------------------------------
// Replay fixture: full-chain trace + determinism
// ---------------------------------------------------------------------

/// Build the canonical full-chain engine trace (`town01 -> map01 -> keikoku`)
/// as `ModeTraceFrame` rows, one per tick, with `frame` = row index. The frame
/// layout is deterministic (walk-on trigger + record run + overworld-portal
/// engage are all frame-stable), so the committed fixture's `[[expected]]` rows
/// pin exact frames.
fn build_chain_trace() -> Option<Vec<ModeTraceFrame>> {
    let mut host = open_host()?;
    host.enter_field_scene("town01", 0).expect("enter town01");
    let mut trace: Vec<ModeTraceFrame> = Vec::new();
    let mut fi = 0u64;
    let mut push = |host: &SceneHost, fi: &mut u64| {
        trace.push(ModeTraceFrame {
            frame: *fi,
            game_mode: None,
            game_mode_name: None,
            scene_mode: scene_mode_name(host.world.mode).to_string(),
            active_scene: host.scene.as_ref().map(|s| s.name.clone()),
        });
        *fi += 1;
    };
    // Frame 0: fresh town01 field.
    push(&host, &mut fi);
    // Step onto the south gate, then tick the town segment (frames 1..=11): the
    // walk-on trigger fires, its P2 record runs to the 0x3F, and map01 enters
    // (WorldMap) within a few ticks; the tail idles on the overworld.
    host.world
        .seat_player_at_tile(TOWN01_SOUTH_GATE.0, TOWN01_SOUTH_GATE.1);
    for _ in 0..11 {
        host.tick().expect("tick");
        push(&host, &mut fi);
    }
    // Step onto the keikoku overworld portal, then tick the overworld segment
    // (frames 12..=23): the entity SM emits WorldMapTransition, the host drain
    // loads keikoku (Field), and the tail idles inside the dungeon.
    let keikoku_tile = find_portal_tile(&host, "keikoku").expect("keikoku portal on map01");
    host.world
        .seat_player_at_tile(keikoku_tile.0, keikoku_tile.1);
    for _ in 0..12 {
        host.tick().expect("tick");
        push(&host, &mut fi);
    }
    Some(trace)
}

#[test]
fn replay_fixture_parses_and_pins_the_spine() {
    let path = replay_path();
    assert!(
        path.exists(),
        "chapter1_spine.toml missing at {}",
        path.display()
    );
    let replay = ReplayFile::from_path(&path)
        .unwrap_or_else(|e| panic!("chapter1_spine.toml did not parse: {e:#}"));
    replay
        .validate()
        .unwrap_or_else(|e| panic!("chapter1_spine.toml failed validate: {e:#}"));
    // The fixture must pin the three spine scenes.
    let scenes: Vec<&str> = replay
        .expected
        .iter()
        .filter_map(|e| e.active_scene.as_deref())
        .collect();
    for want in ["town01", "map01", "keikoku"] {
        assert!(
            scenes.contains(&want),
            "chapter1_spine fixture must pin an {want:?} row (got {scenes:?})"
        );
    }
    // And the WorldMap mode (the overworld leg).
    assert!(
        replay.expected.iter().any(|e| e.scene_mode == "WorldMap"),
        "chapter1_spine fixture must pin a WorldMap row"
    );
}

/// Determinism gate (disc-free): drive a synthetic world through the replay's
/// expanded pad stream twice and assert byte-identical state traces. Mirrors
/// `v0_1_playthrough::v0_1_determinism_two_runs_byte_identical`; the disc-gated
/// full-chain diff below is a separate test.
#[test]
fn replay_determinism_two_runs_byte_identical() {
    let path = replay_path();
    let replay =
        ReplayFile::from_path(&path).unwrap_or_else(|e| panic!("load chapter1_spine: {e:#}"));

    let run = || -> String {
        let pad_stream = replay.expand_pad_stream();
        let mut world = World::new();
        while world.actors.len() < 8 {
            world.actors.push(Actor::default());
        }
        world.rng_state = replay.meta.rng_seed;
        let mut out = String::new();
        for (i, &pad) in pad_stream.iter().enumerate() {
            if i > 0 {
                let _ = world.tick();
            }
            out.push_str(&format!(
                "{}|{}|{}|{}\n",
                world.frame,
                scene_mode_name(world.mode),
                pad,
                world.rng_state
            ));
        }
        out
    };
    assert_eq!(
        run(),
        run(),
        "chapter1_spine determinism: two runs diverged"
    );
}

/// Disc-gated: the committed fixture's `[[expected]]` rows match the real
/// full-chain engine trace (`town01 -> map01 -> keikoku`). Diffs the fixture
/// against the trace via [`ReplayFile::diff`].
#[test]
fn replay_fixture_matches_engine_chain() {
    let Some(trace) = build_chain_trace() else {
        return; // disc gate
    };
    let path = replay_path();
    let replay =
        ReplayFile::from_path(&path).unwrap_or_else(|e| panic!("load chapter1_spine: {e:#}"));

    // Diagnostic: scene_mode/active_scene transitions across the chain.
    let mut prev: Option<(String, Option<String>)> = None;
    for f in &trace {
        let cur = (f.scene_mode.clone(), f.active_scene.clone());
        if prev.as_ref() != Some(&cur) {
            eprintln!(
                "[chain] frame={:<3} scene_mode={:<8} active_scene={:?}",
                f.frame, f.scene_mode, f.active_scene
            );
            prev = Some(cur);
        }
    }

    if let Some(d) = replay.diff(&trace) {
        panic!(
            "chapter1_spine fixture drift at frame {}: kind={:?} expected={:?} \
             recorded(scene_mode={}, active_scene={:?})",
            d.frame, d.kind, d.expected, d.recorded.scene_mode, d.recorded.active_scene
        );
    }
    eprintln!(
        "[ok] chapter1_spine fixture matches the real engine chain over {} frames",
        trace.len()
    );
}
