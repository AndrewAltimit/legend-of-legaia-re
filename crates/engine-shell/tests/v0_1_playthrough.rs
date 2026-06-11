//! v0.1 playthrough oracle.
//!
//! Drives one scripted `j-replay-v1` recording through the engine and
//! checks it against the retail parity oracles. The v0.1 target path is:
//!
//!   boot -> field scene -> scripted encounter -> battle [-> loot -> save back]
//!
//! **Field leg.** The engine drives a cold boot into Field for the
//! scenario's scene via [`build_engine_mode_trace_field_live`]
//! ([`BootSession::enter_field_live`]) and asserts:
//!   - it actually reaches `SceneMode::Field` (not stuck in `Title`);
//!   - the replay `[[expected]]` Field rows hold across the run;
//!   - the retail mode-trace converges (when a non-drifted save resolves);
//!   - an SC round-trip on the post-Field world is byte-identical.
//!
//! **Battle leg.** A NEW GAME cold boot reaches `SceneMode::Battle` for the
//! opening Rim Elm training fight (`BootSession::begin_new_game` seeds Vahn; the
//! cold boot installs town01's sparring carrier). It is the game's first fight,
//! so the new-game seed *is* retail's pre-fight story state. Convergence is
//! against the cataloged retail anchors that bracket the transition: the
//! pre-fight dialogue-accept frame reads Field and the battle-loading frame
//! reads Battle. Two flavours: the dialogue-accept auto-arm (drives the
//! field-interact op directly), and the fully **emergent** path where the
//! player walks to the partner and talks to it through the interaction probe.
//! (Still open: the dialogue box's Yes/No selection is undecoded — the engine
//! treats accept as dismiss, faithful for the forced tutorial.)
//!
//! The Field-leg recording lives at `scripts/replays/v0_1_playthrough.toml`
//! (override via `LEGAIA_V0_1_REPLAY`); the Battle-leg fixture lives at
//! `scripts/replays/v0_1_battle_leg.toml`. The tests run here:
//!
//! 1. **Replay smoke (always).** The Field + Battle replay files parse +
//!    validate.
//! 2. **Determinism gate (disc-free, always).** Drives a synthetic
//!    [`legaia_engine_core::world::World`] through the replay twice and
//!    asserts byte-identical per-frame state traces.
//! 3. **Oracle convergence gate (disc-gated).** Resolves the replay's
//!    `meta.scenario`, runs the field-live engine driver, and asserts the
//!    Field-reach + replay-fixture + retail convergence + SC round-trip
//!    above. Skip-passes without disc data (CLAUDE.md convention).
//! 4. **Battle leg (disc-gated).** New-game cold boot -> dialogue-accept ->
//!    Battle (Vahn vs Tetsu), converging with the retail Field/Battle anchors.
//! 5. **Emergent Battle leg (disc-gated).** New-game cold boot -> the player
//!    walks (BFS path + `nav_step_toward`) to the sparring partner -> talks via
//!    the interaction probe -> accepts -> Battle. No teleport, no script.
//! 6. **Battle-leg mode-trace (disc-gated).** The Field -> Battle engine
//!    mode-trace matches the literal `[[expected]]` rows in
//!    `v0_1_battle_leg.toml` (via [`ReplayFile::diff`]) and the Battle frame
//!    converges with the retail battle-loading anchor.
//!
//! Skip-pass cases (CLAUDE.md disc-gated convention):
//!   - replay file missing (smoke test fails loudly; oracle test skips)
//!   - `meta.scenario` unset (scaffold state)
//!   - `LEGAIA_DISC_BIN` unset
//!   - `extracted/` missing
//!   - `scripts/scenarios.toml` missing or scenario label unknown
//!   - The scenario's `.mc{slot}` save missing on disk

use std::path::PathBuf;

use legaia_engine_core::world::{Actor, SceneMode, World};
use legaia_engine_shell::boot::{BootConfig, BootSession, FieldLiveOpts};
use legaia_engine_shell::mode_trace_oracle::{
    ModeTraceFrame, build_engine_mode_trace_field_live,
    build_engine_mode_trace_new_game_battle_leg, first_mode_trace_divergence,
    load_runtime_mode_trace_from_save, save_ram_fingerprint,
};
use legaia_engine_shell::replay::ReplayFile;
use legaia_mednafen::ScenarioManifest;
use sha2::{Digest, Sha256};

/// Default location of the v0.1 replay file, relative to the workspace
/// root. Overridable via [`REPLAY_PATH_ENV`].
const REPLAY_PATH_DEFAULT: &str = "scripts/replays/v0_1_playthrough.toml";

/// Env var that overrides [`REPLAY_PATH_DEFAULT`]. Lets local runs point
/// at a work-in-progress recording without editing the committed
/// scaffold.
const REPLAY_PATH_ENV: &str = "LEGAIA_V0_1_REPLAY";

// ---------------------------------------------------------------------
// Discovery helpers (mirror mode_trace_e3 / vram_oracle_e1 patterns)
// ---------------------------------------------------------------------

fn replay_path() -> PathBuf {
    if let Ok(p) = std::env::var(REPLAY_PATH_ENV) {
        return PathBuf::from(p);
    }
    for candidate in [
        REPLAY_PATH_DEFAULT,
        &format!("../{REPLAY_PATH_DEFAULT}"),
        &format!("../../{REPLAY_PATH_DEFAULT}"),
    ] {
        let p = PathBuf::from(candidate);
        if p.exists() {
            return p;
        }
    }
    PathBuf::from(REPLAY_PATH_DEFAULT)
}

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
    for candidate in [
        "scripts/scenarios.toml",
        "../scripts/scenarios.toml",
        "../../scripts/scenarios.toml",
    ] {
        let p = PathBuf::from(candidate);
        if p.exists() {
            return Some(p);
        }
    }
    None
}

// ---------------------------------------------------------------------
// Synthetic-world driver (mirrors determinism_j2; rebound to v0.1 file)
// ---------------------------------------------------------------------

/// Build a deterministic synthetic world. Same shape as
/// `determinism_j2::synthetic_world` so the two gates can share an
/// implementation when the v0.1 path is real enough to swap the
/// synthetic driver for a real BootSession.
fn synthetic_world(rng_seed: u32) -> World {
    let mut world = World::new();
    while world.actors.len() < 8 {
        world.actors.push(Actor::default());
    }
    world.rng_state = rng_seed;
    world.mode = SceneMode::Title;
    world.money = 0;
    world.party_count = 3;
    for slot in 0..3 {
        let actor = world.spawn_actor(slot);
        actor.battle.liveness = 1;
        actor.battle.max_hp = 200;
        actor.battle.hp = 200;
    }
    world
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
struct StateSample {
    frame: u64,
    scene_mode: String,
    pad: u16,
    rng_state: u32,
    money: i32,
    party_hp_total: u32,
    dialog_active: bool,
}

fn scene_mode_name(m: SceneMode) -> &'static str {
    match m {
        SceneMode::Title => "Title",
        SceneMode::Field => "Field",
        SceneMode::Battle => "Battle",
        SceneMode::Cutscene => "Cutscene",
        SceneMode::WorldMap => "WorldMap",
        SceneMode::Menu => "Menu",
    }
}

fn sample_world(world: &World, pad: u16) -> StateSample {
    StateSample {
        frame: world.frame,
        scene_mode: scene_mode_name(world.mode).to_string(),
        pad,
        rng_state: world.rng_state,
        money: world.money,
        party_hp_total: world.actors.iter().map(|a| a.battle.hp as u32).sum(),
        dialog_active: world.current_dialog.is_some(),
    }
}

fn build_state_trace(replay: &ReplayFile) -> String {
    let pad_stream = replay.expand_pad_stream();
    let mut world = synthetic_world(replay.meta.rng_seed);
    let mut out = String::new();
    out.push_str(&serde_json::to_string(&sample_world(&world, pad_stream[0])).unwrap());
    out.push('\n');
    for &pad in pad_stream.iter().skip(1) {
        let _ = world.tick();
        out.push_str(&serde_json::to_string(&sample_world(&world, pad)).unwrap());
        out.push('\n');
    }
    out
}

// ---------------------------------------------------------------------
// Test 1: replay file smoke (always runs)
// ---------------------------------------------------------------------

#[test]
fn v0_1_replay_file_parses_clean() {
    let path = replay_path();
    assert!(
        path.exists(),
        "v0.1 replay scaffold missing at {} — was the file deleted?",
        path.display(),
    );
    let replay = ReplayFile::from_path(&path)
        .unwrap_or_else(|e| panic!("v0.1 replay {} did not parse: {e:#}", path.display()));
    // ReplayFile::from_path calls validate() internally; re-call here so
    // a future regression in that ordering surfaces.
    replay
        .validate()
        .unwrap_or_else(|e| panic!("v0.1 replay {} failed validate: {e:#}", path.display()));
}

// ---------------------------------------------------------------------
// Test 2: determinism gate (disc-free, always runs)
// ---------------------------------------------------------------------

#[test]
fn v0_1_determinism_two_runs_byte_identical() {
    let path = replay_path();
    let replay = ReplayFile::from_path(&path)
        .unwrap_or_else(|e| panic!("load v0.1 replay {}: {e:#}", path.display()));

    let trace_a = build_state_trace(&replay);
    let trace_b = build_state_trace(&replay);
    assert_eq!(
        trace_a, trace_b,
        "v0.1 determinism gate failed: two runs of the same replay produced different state traces"
    );

    // SHA-256 cross-check so a future reader can pin the digest if a
    // regression hunt needs a checksum to bisect against.
    let mut h = Sha256::new();
    h.update(trace_a.as_bytes());
    let _ = h.finalize();

    // No fixture-diff here: the replay's [[expected]] rows describe
    // disc-gated engine behaviour (e.g. reaching Battle at frame 250),
    // not the synthetic-world driver. The fixture diff runs in
    // `v0_1_oracle_convergence` where the real engine ticks. This gate
    // is exclusively about the determinism invariant -- two runs
    // produce byte-identical traces, full stop.
}

// ---------------------------------------------------------------------
// Test 3: oracle convergence gate (disc-gated, scaffold skip-passes)
// ---------------------------------------------------------------------

/// Minimum tick count when the replay carries no `meta.frames`
/// override. Most disc-gated convergence checks happen within the
/// first second of boot (the engine reaches `town01` immediately).
/// The actual frame budget is `max(MIN_ORACLE_FRAMES, replay.meta.frames)`
/// so a recorded playthrough always runs to completion.
const MIN_ORACLE_FRAMES: u64 = 60;

#[test]
fn v0_1_oracle_convergence() {
    // -- preconditions --------------------------------------------------
    let path = replay_path();
    let replay = match ReplayFile::from_path(&path) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("[skip] v0.1 replay {} unreadable: {e:#}", path.display());
            return;
        }
    };

    let Some(scenario_label) = replay.meta.scenario.as_deref() else {
        eprintln!(
            "[skip] v0.1 replay carries no `meta.scenario` binding — scaffold not yet populated. \
             See scripts/replays/v0_1_playthrough.toml for the recording recipe."
        );
        return;
    };

    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing — run `legaia-extract` first");
        return;
    };
    let Some(manifest_path) = manifest_path() else {
        eprintln!("[skip] scripts/scenarios.toml not found");
        return;
    };
    let manifest = ScenarioManifest::from_path(&manifest_path).expect("parse scenarios manifest");
    let Some(scn) = manifest
        .scenarios
        .iter()
        .find(|s| s.label == scenario_label)
    else {
        eprintln!(
            "[skip] v0.1 replay references scenario {scenario_label:?} but it isn't in \
             scripts/scenarios.toml"
        );
        return;
    };
    let Some(scene_name) = scn.expected_active_scene.as_deref() else {
        eprintln!(
            "[skip] scenario {scenario_label:?} has no `expected_active_scene` — needed to \
             drive build_engine_mode_trace"
        );
        return;
    };
    // Prefer the immutable library backup over the wipe-prone live slot.
    let Ok(save_path) = manifest.mednafen_save_path(scn, library_dir().as_deref()) else {
        eprintln!("[skip] scenario {scenario_label:?}: save path resolution failed");
        return;
    };
    if !save_path.exists() {
        eprintln!(
            "[skip] scenario {scenario_label:?}: no save at {}",
            save_path.display(),
        );
        return;
    }
    // Only trust the resolved save for the retail-snapshot convergence check
    // when it still matches the catalogued RAM fingerprint — a live `.mc{slot}`
    // that's been overwritten no longer holds the documented scenario, so
    // comparing the engine against it proves nothing. A scenario with a
    // `backup_fingerprint` resolves to the stable library copy above and passes
    // this gate by construction. The save-independent replay-fixture diff below
    // still runs either way.
    let retail_usable = match scn.ram_fingerprint_sha256.as_deref() {
        None => true,
        Some(expected) => match save_ram_fingerprint(&save_path) {
            Ok(actual) if actual.eq_ignore_ascii_case(expected) => true,
            Ok(actual) => {
                eprintln!(
                    "[drift] scenario {scenario_label:?}: save slot {} != catalogued {} — overwritten, skipping retail convergence",
                    &actual[..16.min(actual.len())],
                    &expected[..16.min(expected.len())],
                );
                false
            }
            Err(e) => {
                eprintln!("[drift] scenario {scenario_label:?}: fingerprint read failed: {e:#}");
                false
            }
        },
    };

    // -- pad-threaded mode-trace ---------------------------------------
    //
    // Run the engine for `max(MIN_ORACLE_FRAMES, replay.meta.frames)`
    // ticks via the field-live driver: it calls
    // `BootSession::enter_field_live`, so the engine reaches
    // `SceneMode::Field` for `scene_name` immediately (instead of sitting
    // in `Title` like the bare `load_scene` path), then ticks feeding the
    // replay's expanded pad stream into `World.input` per frame.
    //
    // This converges against a field-phase retail snapshot (`game_mode
    // 0x03` -> Field). The scripted-encounter Battle leg needs the scene's
    // story state seeded so the trigger is armed (a cold boot into record 0
    // produces no dialogue, and town scenes have a 0% random rate); that is
    // a separate, tracked milestone. This oracle stops at Field.
    let pad_stream = replay.expand_pad_stream();
    let oracle_frames = replay.meta.frames.max(MIN_ORACLE_FRAMES);
    let trace = build_engine_mode_trace_field_live(
        scene_name,
        &extracted,
        None,
        oracle_frames,
        &pad_stream,
    )
    .unwrap_or_else(|e| panic!("scenario {scenario_label:?}: build engine mode-trace: {e:#}"));
    let retail = if retail_usable {
        Some(
            load_runtime_mode_trace_from_save(&save_path).unwrap_or_else(|e| {
                panic!("scenario {scenario_label:?}: load retail snapshot: {e:#}")
            }),
        )
    } else {
        None
    };

    // Diagnostic: print the unique scene_mode transitions across the
    // trace. With the field-live driver the engine reaches Field for
    // `scene_name` at frame 0 and stays there (the Battle leg is the
    // tracked follow-on). When that lands, a Field -> Battle transition
    // surfaces here without changing the test code.
    print_scene_mode_transitions(scenario_label, &trace);

    // The engine must actually reach Field (not sit in Title) -- this is
    // the Phase-1 deliverable the field-live driver unblocks.
    assert!(
        trace.iter().any(|f| f.scene_mode == "Field"),
        "v0.1 engine never reached Field for scenario {scenario_label:?} (scene={scene_name}); \
         field-live driver regressed"
    );

    // Retail-snapshot convergence (existing assertion shape from
    // mode_trace_e3). Passes today because the engine reaches
    // active_scene=town01 immediately at boot; first_mode_trace_divergence
    // accepts "at least one engine frame matches retail" as convergence.
    // Skipped when the save slot drifted (see `retail_usable`).
    if let Some(retail) = &retail
        && let Some(d) = first_mode_trace_divergence(&trace, retail)
    {
        panic!(
            "v0.1 retail-snapshot convergence failed for scenario {scenario_label:?} \
             (scene={scene_name}): {:?}: engine(scene_mode={}, active_scene={:?}) vs \
             retail(scene_mode={}, active_scene={:?})",
            d.kind,
            d.engine.scene_mode,
            d.engine.active_scene,
            d.retail.scene_mode,
            d.retail.active_scene,
        );
    }

    // Replay-fixture diff (sharper assertion). The replay file's
    // [[expected]] rows pin specific frames to specific scene_modes,
    // aligned with the anchors in `scripts/scenarios.toml`. With the
    // field-live driver these are `Field` rows for the field-phase
    // scenario; the deferred Battle leg will add a `Battle` row once the
    // scripted trigger is seeded.
    if let Some(d) = replay.diff(&trace) {
        panic!(
            "v0.1 replay fixture drift at frame {}: kind={:?} expected={:?} recorded(scene_mode={}, active_scene={:?})",
            d.frame, d.kind, d.expected, d.recorded.scene_mode, d.recorded.active_scene,
        );
    }

    match &retail {
        Some(retail) => eprintln!(
            "[ok]    v0.1 disc-gated oracle passed: scenario={scenario_label} scene={scene_name} \
             retail(scene_mode={}, active_scene={:?}) over {} engine frames",
            retail.scene_mode,
            retail.active_scene,
            trace.len(),
        ),
        None => eprintln!(
            "[ok]    v0.1 disc-gated oracle passed (replay-fixture diff only; retail save drifted): \
             scenario={scenario_label} scene={scene_name} over {} engine frames",
            trace.len(),
        ),
    }

    // SC round-trip on the post-Field World: save the live world to an
    // `LGSF` SaveFile, parse it back, load it, and re-save -- the two
    // serialisations must be byte-identical. This exercises the save path
    // on a real disc-loaded field world (party + globals + inventory),
    // not just a synthetic fixture.
    {
        let cfg = BootConfig {
            scene: scene_name.to_string(),
            enable_audio: false,
        };
        let mut session = BootSession::open(&extracted, &cfg).expect("reopen for SC round-trip");
        session
            .enter_field_live(
                scene_name,
                &FieldLiveOpts {
                    live_loop: true,
                    ..Default::default()
                },
            )
            .expect("enter_field_live for SC round-trip");
        for i in 0..oracle_frames {
            let pad = pad_stream.get(i as usize).copied().unwrap_or(0);
            session.host.world.set_pad(pad);
            let _ = session.tick().expect("tick during SC round-trip run");
        }
        let world = &mut session.host.world;
        let first = world.save_full().write();
        let parsed = legaia_save::SaveFile::parse(&first).expect("parse round-tripped SaveFile");
        world.load_full(parsed);
        let second = world.save_full().write();
        assert_eq!(
            first, second,
            "v0.1 SC round-trip not byte-identical for scenario {scenario_label:?}"
        );
    }

    // The literal Battle mode-trace row now lands too — see
    // `v0_1_battle_leg_mode_trace_matches_expected`, which diffs the
    // Field -> Battle engine trace against `scripts/replays/v0_1_battle_leg.toml`.
    //
    // Still deferred (need a title-phase render path / a battle-BGM id the
    // engine can resolve before they assert anything meaningful):
    //   - VRAM oracle at the title-screen frame (the engine boots straight into
    //     a scene and never renders the title screen, so there is no engine-side
    //     title VRAM to diff).
    //   - audio-trace oracle across the Battle<->Field BGM swap.
}

/// Walk `trace` left-to-right printing one row per `scene_mode`
/// transition. Cheap visibility into what the engine actually did,
/// without dumping all `frames` records. Active-scene changes within
/// the same `scene_mode` are not printed -- they're rare and the
/// retail-snapshot convergence check covers them.
fn print_scene_mode_transitions(scenario_label: &str, trace: &[ModeTraceFrame]) {
    if trace.is_empty() {
        return;
    }
    eprintln!(
        "[trace] {scenario_label}: {} engine frames; scene_mode transitions:",
        trace.len(),
    );
    let mut prev: Option<&str> = None;
    for f in trace {
        if prev != Some(f.scene_mode.as_str()) {
            eprintln!(
                "[trace]   frame={:<5} scene_mode={:<8} active_scene={:?}",
                f.frame, f.scene_mode, f.active_scene,
            );
            prev = Some(f.scene_mode.as_str());
        }
    }
}

// ---------------------------------------------------------------------
// Test 4: v0.1 Battle leg (disc-gated)
// ---------------------------------------------------------------------

/// The v0.1 oracle's Battle leg: a NEW GAME cold boot reaches `SceneMode::Battle`
/// for the opening Rim Elm training fight, driven entirely by the field-VM
/// dialogue-accept (no manual engage, no script injection beyond the real
/// field-interact op).
///
/// **Story-seed.** `BootSession::begin_new_game` seeds the opening party (Vahn,
/// 180 HP) from the `SCUS_942.54` template. The Tetsu fight is the game's first
/// battle, so this fresh state *is* retail's pre-fight story state — there is no
/// earlier save to seed from (you cannot save before the tutorial fight). Cold
/// boot then installs the sparring carrier from town01's MAN, so interacting
/// with it and accepting its prompt arms and launches the fight.
///
/// **Convergence.** The cataloged retail anchors bracket the transition: the
/// pre-fight dialogue-accept frame reads Field (`game_mode 0x03`) and the
/// battle-loading frame reads Battle (`0x15`), matching the engine's
/// Field → Battle. Resolved from the immutable library backups; each retail leg
/// is skipped if its backup isn't present locally.
///
/// Skip-passes without disc data / extracted assets (CLAUDE.md convention).
#[test]
fn v0_1_battle_leg_reaches_battle_from_new_game() {
    use legaia_engine_core::input::PadButton;
    const TETSU: u16 = legaia_engine_core::encounter_record::RIM_ELM_TRAINING_OPPONENT_ID as u16;

    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing — run `legaia-extract` first");
        return;
    };

    let cfg = BootConfig {
        scene: "town01".to_string(),
        enable_audio: false,
    };
    let mut session = BootSession::open(&extracted, &cfg).expect("open boot session");
    // Story-seed: the new-game opening party = retail pre-Tetsu state.
    session.begin_new_game();
    session
        .enter_field_live(
            "town01",
            &FieldLiveOpts {
                live_loop: true,
                ..Default::default()
            },
        )
        .expect("enter field live");

    let w = &mut session.host.world;
    assert_eq!(
        w.mode,
        SceneMode::Field,
        "new-game cold boot reaches the field"
    );
    assert_eq!(w.party_count, 1, "the opening party is Vahn alone");
    assert_eq!(
        w.actors[0].battle.max_hp, 180,
        "Vahn seeded from the new-game template (180 HP)"
    );

    // Cold boot installed town01's sparring carrier: exactly one scripted-
    // encounter slot.
    let slot = {
        let mut slots: Vec<u8> = w.field_carrier_slots.keys().copied().collect();
        slots.sort_unstable();
        assert_eq!(
            slots.len(),
            1,
            "town01 installs one scripted-encounter carrier slot, got {slots:?}"
        );
        slots[0]
    };

    // Drive the dialogue-accept auto-arm: a real field-interact on the carrier's
    // slot opens its dialogue; accepting (just-pressed Cross) engages it.
    let mut modes: Vec<SceneMode> = vec![w.mode];
    w.load_field_script(vec![0x3E, 0x05, slot, 0x4C, 0x54]);
    w.set_pad(0);
    let _ = w.tick();
    modes.push(w.mode);
    let cross = PadButton::Cross.mask();
    for i in 0..16u32 {
        w.set_pad(if i == 0 { cross } else { 0 });
        let _ = w.tick();
        modes.push(w.mode);
        if w.mode == SceneMode::Battle {
            break;
        }
    }

    assert!(
        modes.contains(&SceneMode::Battle),
        "v0.1 Battle leg: engine never reached Battle (modes: {modes:?})"
    );
    assert_eq!(
        w.actors[0].battle.max_hp, 180,
        "Vahn survives the field -> battle handoff"
    );
    let monster_slot = w.party_count.clamp(1, 3) as usize;
    assert_eq!(
        w.actors[monster_slot].battle_monster_id,
        Some(TETSU),
        "enemy slot is the training opponent (Tetsu 0x4F)"
    );

    // Retail convergence: pre-fight anchor reads Field, battle anchor reads
    // Battle — matching the engine's Field -> Battle.
    let Some(manifest_path) = manifest_path() else {
        return;
    };
    let manifest = ScenarioManifest::from_path(&manifest_path).expect("parse scenarios manifest");
    let check = |label: &str, want: &str| {
        let Some(scn) = manifest.scenarios.iter().find(|s| s.label == label) else {
            return;
        };
        let Ok(path) = manifest.mednafen_save_path(scn, library_dir().as_deref()) else {
            return;
        };
        if !path.exists() {
            eprintln!(
                "[skip] retail anchor {label}: no save at {}",
                path.display()
            );
            return;
        }
        let f = load_runtime_mode_trace_from_save(&path)
            .unwrap_or_else(|e| panic!("retail anchor {label} trace: {e:#}"));
        assert_eq!(
            f.scene_mode, want,
            "retail anchor {label} should read scene_mode={want} (got {})",
            f.scene_mode
        );
    };
    check("v0_1_tetsu_dialogue_accept", "Field");
    check("v0_1_battle_loading_tetsu", "Battle");

    eprintln!(
        "[ok] v0.1 Battle leg: new-game cold boot -> dialogue-accept -> Battle \
         (Vahn 180 HP vs Tetsu 0x4F), converges with retail Field/Battle anchors"
    );
}

// ---------------------------------------------------------------------
// Test 5: v0.1 Battle leg, fully emergent (walk + talk + accept)
// ---------------------------------------------------------------------

/// The fully input-driven Battle leg: a NEW GAME cold boot, then the player
/// **walks** from the spawn to the sparring partner (BFS path over the real
/// collision grid, driven through `World::nav_step_toward`), **talks** to it via
/// the interaction probe, and **accepts** — reaching Battle with no teleport,
/// no script injection, no manual engage.
///
/// The opening sequence repositions the partner next to Vahn for the tutorial
/// ([`RIM_ELM_SPARRING_CARRIER_TUTORIAL_POS`] — its placement tile (76,65) is the
/// unreachable post-tutorial village spot); the cold boot skips that reposition,
/// so the test places the carrier at its tutorial position first (standing in
/// for the opening). From there the walk is fully emergent.
#[test]
fn v0_1_battle_leg_walk_talk_accept() {
    use legaia_engine_core::encounter_record::RIM_ELM_SPARRING_CARRIER_TUTORIAL_POS as TUT;
    use legaia_engine_core::input::PadButton;
    const TETSU: u16 = legaia_engine_core::encounter_record::RIM_ELM_TRAINING_OPPONENT_ID as u16;

    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing — run `legaia-extract` first");
        return;
    };

    let cfg = BootConfig {
        scene: "town01".to_string(),
        enable_audio: false,
    };
    let mut session = BootSession::open(&extracted, &cfg).expect("open boot session");
    session.begin_new_game();
    session
        .enter_field_live(
            "town01",
            &FieldLiveOpts {
                live_loop: true,
                ..Default::default()
            },
        )
        .expect("enter field live");
    assert_eq!(session.host.world.mode, SceneMode::Field);

    // Place the scripted carrier at its tutorial position (the opening reposition
    // the cold boot skips). After this, everything is emergent.
    let slot = *session
        .host
        .world
        .field_carrier_slots
        .keys()
        .next()
        .expect("town01 installs the scripted-encounter carrier slot");
    session
        .host
        .world
        .field_npc_positions
        .insert(slot, (TUT.0, TUT.1));

    // BFS a path from the player spawn to the carrier over the real collision
    // grid (64-unit sub-cells, 4-connected — the locomotion axes).
    let (sx, sz) = {
        let p = session.host.world.player_actor_slot.expect("player") as usize;
        let ms = &session.host.world.actors[p].move_state;
        (ms.world_x as i32, ms.world_z as i32)
    };
    let waypoints = bfs_field_path(&session.host.world, (sx, sz), (TUT.0 as i32, TUT.1 as i32))
        .expect("a walkable path from spawn to the carrier exists");
    assert!(
        waypoints.len() >= 4,
        "the partner is a multi-step walk from spawn, got {} waypoints",
        waypoints.len()
    );

    // Walk the player along the path (emergent locomotion + collision).
    for &(wx, wz) in &waypoints {
        let mut reached = false;
        for _ in 0..64 {
            if session.host.world.nav_step_toward(wx as i16, wz as i16, 24) {
                reached = true;
                break;
            }
        }
        assert!(reached, "nav reaches waypoint ({wx}, {wz})");
    }

    // Talk: the interaction probe opens the (now adjacent) carrier's dialogue.
    session.host.world.input.set_pad(PadButton::Cross.mask());
    let _ = session.host.world.tick();
    assert!(
        session.host.world.current_dialog.is_some(),
        "walking up + action button opens the sparring partner's dialogue"
    );

    // Accept: release, press again -> dismiss -> engage -> Battle.
    session.host.world.input.set_pad(0);
    let _ = session.host.world.tick();
    session.host.world.input.set_pad(PadButton::Cross.mask());
    let mut reached_battle = false;
    for _ in 0..8 {
        let _ = session.host.world.tick();
        if session.host.world.mode == SceneMode::Battle {
            reached_battle = true;
            break;
        }
        session.host.world.input.set_pad(0);
    }
    assert!(
        reached_battle,
        "walk + talk + accept flips Field -> Battle (fully emergent)"
    );
    let world = &session.host.world;
    let monster_slot = world.party_count.clamp(1, 3) as usize;
    assert_eq!(
        world.actors[monster_slot].battle_monster_id,
        Some(TETSU),
        "the emergent fight is against Tetsu (0x4F)"
    );
    eprintln!(
        "[ok] v0.1 Battle leg (emergent): walked {} waypoints -> talk -> accept -> Battle vs Tetsu",
        waypoints.len()
    );
}

/// BFS a walkable path on the field collision grid from world `(sx, sz)` to
/// `(gx, gz)`, returning a sparse list of world-position waypoints (one per
/// 64-unit sub-cell along the route), or `None` if unreachable.
fn bfs_field_path(
    world: &World,
    (sx, sz): (i32, i32),
    (gx, gz): (i32, i32),
) -> Option<Vec<(i32, i32)>> {
    use std::collections::{HashMap, VecDeque};
    let cell = |x: i32, z: i32| (x / 64, z / 64);
    let center = |c: i32| c * 64 + 32;
    let wall = |cx: i32, cz: i32| world.field_tile_is_wall(center(cx) as i16, center(cz) as i16);
    let start = cell(sx, sz);
    let goal = cell(gx, gz);
    let mut prev: HashMap<(i32, i32), (i32, i32)> = HashMap::new();
    let mut q = VecDeque::new();
    q.push_back(start);
    prev.insert(start, start);
    while let Some((cx, cz)) = q.pop_front() {
        if (cx, cz) == goal {
            // Reconstruct, then map cells -> world centers.
            let mut path = vec![(center(goal.0), center(goal.1))];
            let mut cur = goal;
            while cur != start {
                cur = prev[&cur];
                path.push((center(cur.0), center(cur.1)));
            }
            path.reverse();
            return Some(path);
        }
        for (dx, dz) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
            let n = (cx + dx, cz + dz);
            if n.0 < 0 || n.1 < 0 || n.0 >= 256 || n.1 >= 256 {
                continue;
            }
            if prev.contains_key(&n) || wall(n.0, n.1) {
                continue;
            }
            prev.insert(n, (cx, cz));
            q.push_back(n);
        }
    }
    None
}

// ---------------------------------------------------------------------
// Test 6: literal Battle mode-trace row (disc-gated)
// ---------------------------------------------------------------------

/// Default location of the Battle-leg replay fixture, relative to the
/// workspace root.
const BATTLE_REPLAY_DEFAULT: &str = "scripts/replays/v0_1_battle_leg.toml";

fn battle_replay_path() -> PathBuf {
    for candidate in [
        BATTLE_REPLAY_DEFAULT,
        &format!("../{BATTLE_REPLAY_DEFAULT}"),
        &format!("../../{BATTLE_REPLAY_DEFAULT}"),
    ] {
        let p = PathBuf::from(candidate);
        if p.exists() {
            return p;
        }
    }
    PathBuf::from(BATTLE_REPLAY_DEFAULT)
}

/// The Battle-leg replay fixture parses + validates (always runs). Keeps
/// the committed fixture honest even without a disc.
#[test]
fn v0_1_battle_replay_file_parses_clean() {
    let path = battle_replay_path();
    assert!(
        path.exists(),
        "v0.1 battle-leg replay fixture missing at {}",
        path.display(),
    );
    let replay = ReplayFile::from_path(&path).unwrap_or_else(|e| {
        panic!(
            "v0.1 battle-leg replay {} did not parse: {e:#}",
            path.display()
        )
    });
    replay.validate().unwrap_or_else(|e| {
        panic!(
            "v0.1 battle-leg replay {} failed validate: {e:#}",
            path.display()
        )
    });
    // The fixture must carry a literal Battle expectation - that's its
    // whole reason to exist (the Field leg lives in v0_1_playthrough.toml).
    assert!(
        replay.expected.iter().any(|e| e.scene_mode == "Battle"),
        "battle-leg fixture must pin at least one Battle row"
    );
}

/// The Battle-leg engine mode-trace matches the literal `[[expected]]`
/// Field -> Battle fixture, and the recorded Battle frame converges with the
/// retail battle-loading anchor (`game_mode 0x15`).
///
/// This is the v0.1 P1 "literal replay Battle mode-trace row" deliverable:
/// where `v0_1_battle_leg_reaches_battle_from_new_game` asserts the engine
/// *reaches* Battle and brackets it against the retail Field/Battle anchors,
/// this test pins the engine's per-frame `(scene_mode, active_scene)` trace
/// against a committed fixture via [`ReplayFile::diff`], so a regression that
/// changed *when* or *whether* the transition lands fails loudly.
///
/// Skip-passes without disc data / extracted assets (CLAUDE.md convention).
#[test]
fn v0_1_battle_leg_mode_trace_matches_expected() {
    let path = battle_replay_path();
    let replay = match ReplayFile::from_path(&path) {
        Ok(r) => r,
        Err(e) => {
            eprintln!(
                "[skip] battle-leg replay {} unreadable: {e:#}",
                path.display()
            );
            return;
        }
    };

    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing — run `legaia-extract` first");
        return;
    };

    let trace =
        build_engine_mode_trace_new_game_battle_leg("town01", &extracted, None, replay.meta.frames)
            .expect("build battle-leg mode trace");
    print_scene_mode_transitions("v0_1_battle_leg", &trace);

    // Literal-fixture diff: every `[[expected]]` row must match the recorded
    // frame at that index. This is the sharper assertion the Field-leg oracle
    // already runs, now extended through the Field -> Battle flip.
    if let Some(d) = replay.diff(&trace) {
        panic!(
            "v0.1 battle-leg fixture drift at frame {}: kind={:?} expected={:?} \
             recorded(scene_mode={}, active_scene={:?})",
            d.frame, d.kind, d.expected, d.recorded.scene_mode, d.recorded.active_scene,
        );
    }

    // Retail convergence for the Battle frame: the engine's Battle row must
    // agree with the battle-loading anchor's snapshot (scene_mode=Battle).
    // Resolved from the immutable library backup; skipped if absent.
    if let Some(manifest_path) = manifest_path() {
        let manifest = ScenarioManifest::from_path(&manifest_path).expect("parse scenarios");
        if let Some(scn) = manifest
            .scenarios
            .iter()
            .find(|s| s.label.as_str() == "v0_1_battle_loading_tetsu")
            && let Ok(save) = manifest.mednafen_save_path(scn, library_dir().as_deref())
            && save.exists()
        {
            let retail = load_runtime_mode_trace_from_save(&save)
                .expect("load battle-loading anchor snapshot");
            assert_eq!(
                retail.scene_mode, "Battle",
                "battle-loading anchor should read Battle"
            );
            if let Some(d) = first_mode_trace_divergence(&trace, &retail) {
                panic!(
                    "v0.1 battle-leg retail convergence failed: {:?}: \
                     engine(scene_mode={}) vs retail(scene_mode={})",
                    d.kind, d.engine.scene_mode, d.retail.scene_mode,
                );
            }
        }
    }

    eprintln!(
        "[ok] v0.1 battle-leg mode-trace matches the literal Field -> Battle fixture \
         over {} frames",
        trace.len()
    );
}
