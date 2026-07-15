//! Disc-gated regression: driving the field-VM inline-dialogue runner over
//! every scene's NPC interaction records must never panic - across the whole
//! story-flag branch space.
//!
//! The browser "play" page runs the engine compiled to `wasm32` with
//! `panic = "abort"`, so any Rust panic on the dialogue path (an out-of-bounds
//! index, an `unwrap` on `None`, a slice past the bytecode end, a bad
//! MES/Picker decode, a field-VM opcode operand read) becomes an `unreachable`
//! trap that poisons the WASM instance and freezes the page. The path is: NPC
//! contact -> `trigger_field_interact` -> `drive_inline_dialogue` ->
//! `step_inline_dialogue` -> `legaia_engine_vm::field::step` on the record's
//! real disc bytecode, plus the per-frame `page_bytes`/`picker` HUD decode.
//!
//! A single default-state pass only walks **one** branch of each interaction
//! prologue (its `SysFlag.Test`/`JmpRel` chain is gated on story flags, all
//! zero on a fresh world). As the player progresses those flags flip and other
//! branches - other text segments, other pickers - execute. This test forces
//! that coverage by re-running every record under a spread of randomized
//! story/system-flag states, so every prologue branch is exercised. A panic
//! anywhere is a test failure - which is the browser freeze.
//!
//! Skip-passes without disc data / extracted assets (CLAUDE.md convention).

use legaia_engine_core::man_field_scripts::{placement_inline_prologue, scene_destinations};
use legaia_engine_core::scene::{ProtIndex, Scene, SceneHost, is_world_map_scene};
use legaia_engine_core::world::World;
use std::collections::BTreeSet;
use std::path::PathBuf;

fn extracted_dir() -> Option<PathBuf> {
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

/// Tiny SplitMix64 so the fuzz is deterministic (byte-reproducible reruns).
fn mix(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// Mirror the browser per-frame HUD dialogue decode
/// (`web_viewer::runtime::LegaiaRuntime::dialog_value`): read the live panel's
/// glyph page + picker options. Another panic surface.
fn read_dialog_hud(world: &World) {
    if let Some(id) = world.inline_dialogue.as_ref() {
        let _ = id.page_bytes();
        let _ = id.menu_active();
        let _ = id.picker_cursor();
        if let Some(pk) = id.picker() {
            for opt in &pk.options {
                let _ = opt.label.len();
            }
        }
    }
}

/// Drive one interaction record through the VM-dialogue runner with `seed`d
/// story/system-flag state, reading the HUD accessors each tick, exactly as the
/// play page does. Never panics on well-formed engine code.
fn drive_record(body: &[u8], entry_pc: usize, first_segment: usize, seed: u64) {
    let mut rng = seed;
    let mut world = World::new();
    world.use_vm_dialogue = true;
    // Randomized story state so the prologue's flag-gated branches all get a
    // chance to execute across seeds.
    world.story_flags = mix(&mut rng) as u32;
    world.extra_flags = mix(&mut rng) as u32;
    world.system_flags = (0..32).map(|_| mix(&mut rng) as u8).collect();

    world.start_inline_dialogue_with_prologue(body.to_vec(), entry_pc, first_segment);

    for i in 0..256u32 {
        // Confirm every few ticks to page boxes / commit menu options; nudge
        // the menu cursor so picker jump targets are exercised too. The runner
        // takes the edges explicitly (the browser passes pad-derived edges the
        // same way).
        let confirm = i % 5 == 4;
        let up = i % 13 == 6;
        let down = i % 7 == 3;
        world.step_inline_dialogue(confirm, up, down);
        read_dialog_hud(&world);
        match world.inline_dialogue.as_ref() {
            Some(id) if id.is_done() => break,
            Some(_) => {}
            None => break,
        }
    }
}

#[test]
fn inline_dialogue_runner_never_panics_across_field_scenes() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        return;
    };

    let index = ProtIndex::open_extracted(&extracted).expect("open ProtIndex");
    let cdname = legaia_prot::cdname::parse(&extracted.join("CDNAME.TXT")).expect("parse cdname");
    let mut scene_names: Vec<String> = cdname.values().cloned().collect();
    scene_names.sort();
    scene_names.dedup();

    // Flag-state spread per record. Seed 0 keeps a plain all-zero-ish run
    // (`mix` still perturbs, but seed 0 is the deterministic anchor).
    const SEEDS: u64 = 12;

    let mut scenes_scanned = 0usize;
    let mut records_driven = 0usize;

    for scene_name in &scene_names {
        let Ok(scene) = Scene::load(&index, scene_name) else {
            continue;
        };
        let man_bytes = match scene.field_man_payload(&index) {
            Ok(Some(b)) => b,
            _ => continue,
        };
        let Ok(man_file) = legaia_asset::man_section::parse(&man_bytes) else {
            continue;
        };
        scenes_scanned += 1;
        let placements = man_file.actor_placements(&man_bytes);
        for p in &placements {
            let Some(prologue) = placement_inline_prologue(&man_file, &man_bytes, p) else {
                continue;
            };
            for seed in 0..SEEDS {
                // Any panic below is the browser freeze; the test would fail.
                drive_record(
                    &prologue.body,
                    prologue.entry_pc,
                    prologue.first_segment,
                    seed,
                );
            }
            records_driven += 1;
        }
    }

    eprintln!(
        "[inline-dialogue] drove {records_driven} interaction records x {SEEDS} flag states \
         across {scenes_scanned} field scenes with no panic"
    );
    assert!(
        records_driven > 100,
        "expected to drive many NPC dialogue records (got {records_driven})"
    );
}

/// Disc-gated: the faithful browser tick path - enter each field scene through
/// a real `SceneHost` and drive every NPC interaction - must return `Ok` from
/// **every** `host.tick()`.
///
/// `LegaiaRuntime::tick_frame` does `host.tick().map_err(JsValue)?`, so a
/// graceful `Err` from any tick is thrown into JS and the play page's rAF catch
/// stops the loop - a freeze indistinguishable from a Rust panic. Asserting
/// `Ok` on every tick (the earlier no-panic harness ignored the `Result`)
/// surfaces exactly that class of freeze.
#[test]
fn faithful_dialogue_tick_never_errors() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        return;
    };

    let mut host = SceneHost::open_extracted(&extracted).expect("open SceneHost");
    let cdname = legaia_prot::cdname::parse(&extracted.join("CDNAME.TXT")).expect("parse cdname");
    let mut scene_names: Vec<String> = cdname.values().cloned().collect();
    scene_names.sort();
    scene_names.dedup();

    let mut npcs_driven = 0usize;
    for scene_name in &scene_names {
        if is_world_map_scene(scene_name) {
            continue;
        }
        host.world.use_vm_dialogue = true;
        host.world.follow_terrain_height = true;
        host.world.leading_edge_wall_probes = true;
        host.world.solid_field_npcs = true;
        host.world.animate_field_npcs = true;
        if host.enter_field_scene(scene_name, 0).is_err() {
            continue;
        }

        let mut slots: Vec<u8> = host
            .world
            .field_npc_dialog_prologue
            .keys()
            .copied()
            .chain(host.world.field_npc_dialog.keys().copied())
            .collect();
        slots.sort_unstable();
        slots.dedup();

        for slot in slots {
            host.world.trigger_field_interact(0, slot);
            for i in 0..300u32 {
                let mut mask = 0u16;
                if i % 4 == 0 {
                    mask |= legaia_engine_core::input::PadButton::Cross.mask();
                }
                if i % 11 == 5 {
                    mask |= legaia_engine_core::input::PadButton::Down.mask();
                }
                host.world.set_pad(mask);
                host.tick().unwrap_or_else(|e| {
                    panic!("host.tick() Err in scene {scene_name} NPC slot {slot}: {e:#}")
                });
                read_dialog_hud(&host.world);
                if host.world.inline_dialogue.is_none()
                    && host.world.current_dialog.is_none()
                    && i > 2
                {
                    break;
                }
            }
            host.world.inline_dialogue = None;
            host.world.current_dialog = None;
            npcs_driven += 1;
        }
    }
    eprintln!("[faithful-tick] drove {npcs_driven} NPC interactions, every tick Ok");
    assert!(
        npcs_driven > 100,
        "expected many NPC interactions (got {npcs_driven})"
    );
}

/// Disc-gated: every op-`0x3F` named scene-change destination reachable from a
/// scene's dialogue / walk-on / cutscene scripts must `enter` without error.
///
/// `SceneHost::tick` runs the pending named scene transition with
/// `enter_field_scene(&name)?` / `enter_world_map_scene(&name)?` - the `?`
/// **propagates** any enter error straight out of `tick()`. In the browser
/// `LegaiaRuntime::tick_frame` turns that `Err` into a thrown JS exception, and
/// the play page's rAF `catch` treats it as fatal and stops the loop - the
/// "freeze on dialogue/cutscene" symptom. (It is a graceful `Err`, not only a
/// Rust panic; the earlier faithful-tick harness masked it by ignoring the
/// `Result`.) A destination that fails to enter is therefore a freeze, so this
/// asserts every one loads.
#[test]
fn scene_change_destinations_all_enter_without_error() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        return;
    };

    let index = ProtIndex::open_extracted(&extracted).expect("open ProtIndex");
    let cdname = legaia_prot::cdname::parse(&extracted.join("CDNAME.TXT")).expect("parse cdname");
    let mut scene_names: Vec<String> = cdname.values().cloned().collect();
    scene_names.sort();
    scene_names.dedup();

    // Collect the union of every scene-change destination name across all
    // scenes (the targets a dialogue / cutscene / walk-on op can warp to).
    let mut destinations: BTreeSet<String> = BTreeSet::new();
    for scene_name in &scene_names {
        let Ok(scene) = Scene::load(&index, scene_name) else {
            continue;
        };
        let man_bytes = match scene.field_man_payload(&index) {
            Ok(Some(b)) => b,
            _ => continue,
        };
        let Ok(man_file) = legaia_asset::man_section::parse(&man_bytes) else {
            continue;
        };
        for d in scene_destinations(&man_file, &man_bytes) {
            destinations.insert(d.scene_name);
        }
    }

    let mut host = SceneHost::open_extracted(&extracted).expect("open SceneHost");
    let mut failures: Vec<(String, String)> = Vec::new();
    let mut entered_ok = 0usize;
    for dest in &destinations {
        // Same play-page arming + world-map routing `SceneHost::tick` uses.
        host.world.use_vm_dialogue = true;
        host.world.follow_terrain_height = true;
        host.world.leading_edge_wall_probes = true;
        host.world.solid_field_npcs = true;
        host.world.animate_field_npcs = true;
        let res = if is_world_map_scene(dest) {
            host.enter_world_map_scene(dest)
        } else {
            host.enter_field_scene(dest, 0)
        };
        match res {
            Ok(()) => entered_ok += 1,
            Err(e) => failures.push((dest.clone(), format!("{e:#}"))),
        }
    }

    eprintln!(
        "[scene-change] {entered_ok}/{} scene-change destinations entered OK",
        destinations.len()
    );
    assert!(
        failures.is_empty(),
        "these scene-change destinations fail to enter (each is a browser freeze \
         when a dialogue/cutscene/walk-on op warps to it): {failures:?}"
    );
}
