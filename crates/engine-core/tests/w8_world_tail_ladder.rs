//! Reach ladder: three routines no canonical ladder entered, each driven
//! through the **world tick and its frame tail** rather than through a direct
//! call of the routine.
//!
//! | address | routine | route |
//! |---|---|---|
//! | `801d7a5c` | `fishing_chrome::splash_burst` | `World::tick` in `SceneMode::Fishing`, a reel gesture matching a cadence template -> `PondEvent::Splash` -> the world's strike-splash spawn |
//! | `80057914` | `vram_rect_copy::build_packet` | a shipped op-`0x43` sub-`0x12` instruction stepped by the field VM -> `World::step_field_vram_effects` (the VRAM half of the hosts' frame tail) |
//! | `801e45bc` | `move_vm::ext::write_bezier_world` | a shipped prescript stager whose move program issues ext sub-op `0x0E` / `0x12`, spawned and advanced by `World::step_world_frame_tail` |
//!
//! The fishing rung is disc-free: its species and cadence tables are
//! synthetic `pub` types, as in `w1f1_fishing_pond_ladder`, because what is
//! under test is the world's event routing, not the disc's numbers. The two
//! corpus rungs take their bytecode from the disc - a hand-written instruction
//! would prove the interpreter runs, not that any shipped scene reaches the
//! arm - and skip-pass without `LEGAIA_DISC_BIN` / `extracted/`.
//!
//! Structural assertions only; no Sony bytes are printed or asserted.

use std::collections::HashSet;
use std::path::PathBuf;

use legaia_asset::fishing_species::{CadenceStep, CadenceTemplate, FishingSpecies, SPAWN_BANDS};
use legaia_asset::scene_event_scripts::move_stager_records;
use legaia_engine_core::fishing::{
    FLIGHT_FRAMES, FishingRecord, PondPhase, PondSession, WINDUP_FRAMES,
};
use legaia_engine_core::input::PadButton;
use legaia_engine_core::man_field_scripts::{
    CLEAN_RESYNC_INSNS, partition_record_span, scene_man_carriers,
};
use legaia_engine_core::scene::{ProtIndex, Scene, SceneHost};
use legaia_engine_core::world::{SceneMode, World};
use legaia_engine_vm::field_disasm::{InsnInfo, LinearWalker};
use legaia_engine_vm::move_vm::{ActorState, MoveHost, StepResult, step};

fn extracted_dir() -> Option<PathBuf> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    if let Some(d) = std::env::var_os("LEGAIA_EXTRACTED_DIR").map(PathBuf::from)
        && d.join("PROT.DAT").exists()
    {
        return Some(d);
    }
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
    None
}

// ---------------------------------------------------------------------------
// 801d7a5c - the strike splash, through the world's fishing tick
// ---------------------------------------------------------------------------

/// Frame-steps per half of the reel gesture (reel A held, then released).
const HOLD: i32 = 6;

fn species(index: usize) -> FishingSpecies {
    FishingSpecies {
        index,
        name_ptr_va: 0,
        score_value: 1_000,
        pull_factor: 250,
        dart_factor: 60,
        sink_factor: 4,
        depth_gate: 4096,
        roll_cutoff_a: 200,
        roll_cutoff_b: 512,
        roll_cutoff_c: 90,
        strike_gate: 100,
    }
}

fn frame(world: &mut World, mask: u16) {
    world.set_pad(mask);
    let _ = world.tick();
}

#[test]
fn a_matched_reel_gesture_spawns_the_strike_splash_through_the_world_tick() {
    let pond = PondSession::new(
        (0..10).map(species).collect(),
        vec![[3; SPAWN_BANDS]; 8],
        vec![CadenceTemplate {
            history_window: HOLD * 2,
            steps: vec![
                CadenceStep {
                    duration: HOLD,
                    button: 1,
                },
                CadenceStep {
                    duration: HOLD,
                    button: 0,
                },
            ],
        }],
        0,
        1,
        2,
        100,
        FishingRecord::default(),
        0,
        0x1234_5678,
    );
    let mut world = World::new();
    world.mode = SceneMode::Field;
    world.enter_fishing(pond);
    assert_eq!(world.mode, SceneMode::Fishing);
    assert_eq!(world.minigames.fx.len(), 0, "no splash before the gesture");

    // Cast through the world's pad path: Circle opens the wind-up, Circle
    // again locks the power, and the lure flies.
    frame(&mut world, 0);
    frame(&mut world, PadButton::Circle.mask());
    for _ in 0..WINDUP_FRAMES + 28 {
        frame(&mut world, 0);
    }
    frame(&mut world, PadButton::Circle.mask());
    for _ in 0..FLIGHT_FRAMES {
        frame(&mut world, 0);
    }
    let phase = |w: &World| w.minigames.fishing.as_ref().map(|s| s.phase());
    assert_eq!(phase(&world), Some(PondPhase::Waiting), "the lure settled");

    // Play the template's own gesture - Cross (reel A) held for HOLD frames,
    // released for HOLD - until the recogniser matches and the world spawns
    // the three-part splash into its minigame effect pool.
    let mut spawned = 0;
    for f in 0..600 {
        if phase(&world) != Some(PondPhase::Waiting) {
            break;
        }
        let held = (f / HOLD) % 2 == 0;
        frame(&mut world, if held { PadButton::Cross.mask() } else { 0 });
        spawned = spawned.max(world.minigames.fx.len());
        if spawned > 0 {
            break;
        }
    }
    assert!(
        spawned >= 3,
        "a cadence match must spawn the strike splash's three parts through \
         the world tick (got {spawned})"
    );
}

// ---------------------------------------------------------------------------
// 80057914 - op 0x43 sub 0x12, a shipped carrier, through the VRAM tail
// ---------------------------------------------------------------------------

#[test]
fn a_shipped_vram_rect_copy_runs_through_the_frame_tail() {
    let Some(extracted) = extracted_dir() else {
        return;
    };
    let index = ProtIndex::open_extracted(&extracted).expect("open ProtIndex");
    let mut site: Option<(String, Vec<u8>, usize)> = None;
    'scenes: for name in index.cdname_scene_names() {
        let Ok(scene) = Scene::load(&index, &name) else {
            continue;
        };
        for carrier in scene_man_carriers(&index, &scene) {
            let man = &carrier.payload;
            let Ok(man_file) = legaia_asset::man_section::parse(man) else {
                continue;
            };
            for partition in 0..3 {
                let count = (*man_file
                    .header
                    .partition_counts
                    .get(partition)
                    .unwrap_or(&0))
                .max(0) as usize;
                for record in 0..count {
                    let Some((start, pc0, len)) =
                        partition_record_span(&man_file, man, partition, record)
                    else {
                        continue;
                    };
                    let body = &man[start..start + len];
                    let mut ok_run = CLEAN_RESYNC_INSNS;
                    for insn in LinearWalker::new(body, pc0) {
                        let Ok(insn) = insn else {
                            ok_run = 0;
                            continue;
                        };
                        let clean = ok_run >= CLEAN_RESYNC_INSNS;
                        ok_run += 1;
                        if clean && matches!(insn.info, InsnInfo::ActorCtrl { sub_op: 0x12, .. }) {
                            site = Some((name.clone(), body.to_vec(), insn.pc));
                            break 'scenes;
                        }
                    }
                }
            }
        }
    }
    let (scene, body, pc) = site.expect("a shipped scene carries op 0x43 sub 0x12");
    eprintln!("[43 12] {scene} pc={pc:#x}");

    let mut world = World {
        mode: SceneMode::Field,
        ..World::default()
    };
    world.party.roster = legaia_save::Party::zeroed(3);
    world.load_field_script_at(body, pc);
    world
        .step_field()
        .expect("step the shipped `43 12` instruction");

    let mut vram = legaia_tim::Vram::new();
    assert!(
        world.step_field_vram_effects(&mut vram, false),
        "{scene}: the instruction queued a rect copy the VRAM tail must drain \
         into a packet and apply"
    );
}

// ---------------------------------------------------------------------------
// 801e45bc - a shipped Bezier stager, through the world frame tail
// ---------------------------------------------------------------------------

struct NullHost;
impl MoveHost for NullHost {}

fn words_of(bytes: &[u8]) -> Vec<u16> {
    bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect()
}

/// Walk one program from `pc` at decoded instruction boundaries and report
/// whether it issues move-VM ext sub-op `0x0E` or `0x12` (`2F 0E` / `2F 12`).
fn issues_bezier(words: &[u16], pc: i16) -> bool {
    let mut host = NullHost;
    let mut state = ActorState::new();
    state.pc = pc;
    let mut seen = HashSet::new();
    for _ in 0..20_000 {
        let pc = state.pc as usize;
        if pc + 1 >= words.len() || !seen.insert(pc) {
            return false;
        }
        if words[pc] == 0x2F && matches!(words[pc + 1], 0x0E | 0x12) {
            return true;
        }
        match step(&mut host, &mut state, words) {
            StepResult::Advance | StepResult::Wait => {}
            _ => return false,
        }
    }
    false
}

#[test]
fn a_shipped_bezier_stager_runs_through_the_world_frame_tail() {
    let Some(extracted) = extracted_dir() else {
        return;
    };
    let mut host = SceneHost::open_extracted(&extracted).expect("open SceneHost");
    let names = host.index.cdname_scene_names();
    let mut found: Option<(String, Vec<u8>, usize)> = None;
    for name in &names {
        if host.load_scene(name).is_err() {
            continue;
        }
        let scene = host.scene.as_ref().expect("scene loaded");
        let Some(scripts) = scene.find_event_scripts() else {
            continue;
        };
        let Some(records) = move_stager_records(scripts.bytes) else {
            continue;
        };
        for (id, rec) in records.iter().enumerate() {
            let words = words_of(&scripts.bytes[rec.record_off..rec.bytecode.end]);
            if words.len() >= 3 && issues_bezier(&words, 2) {
                found = Some((name.clone(), scripts.bytes.to_vec(), id));
                break;
            }
        }
        if found.is_some() {
            break;
        }
    }
    let Some((scene, bytes, id)) = found else {
        panic!("no shipped prescript stager issues move-VM ext 0x0E / 0x12");
    };
    eprintln!("[2F 0E/12] {scene} prescript stager {id}");

    let mut world = World::new();
    world.mode = SceneMode::Field;
    world.install_field_stagers(&bytes);
    assert!(world.spawn_field_stager(id, [0, 0, 0]), "the stager seats");
    let before = world.active_field_fx_part_draws().len();
    for _ in 0..600 {
        let _ = world.step_world_frame_tail(None, None, |_| None);
    }
    eprintln!(
        "[2F 0E/12] {scene}: {} live field-fx scene(s), {before} part draw(s) at spawn",
        world.props.active_fx.len()
    );
    assert_eq!(
        world.props.active_fx.len(),
        1,
        "the frame tail advances the one seated effect and keeps it"
    );
}
