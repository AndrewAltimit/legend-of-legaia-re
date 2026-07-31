//! Disc-gated oracle for the browser play page's **battle 3D scene**
//! (`crate::play_battle_render`, drawn by `site/js/play-app.js`'s
//! `_battleFrame`).
//!
//! What is pinned, structurally (no Sony bytes asserted):
//!
//! 1. **The battle render builds on the engine's own `Field -> Battle`
//!    edge.** A scripted formation off the town MAN enters a live battle and
//!    `play_battle_active` rises - the same latch that arms the ENCOUNTER!
//!    banner.
//! 2. **Every layer the page draws is present**: a battle VRAM that differs
//!    from the field VRAM (stage + flame atlas + texture bands landed), a
//!    backdrop whose geometry carries the second copy (even index/vertex
//!    halves - `append_scaled` doubles the shell), the ground grid, and one
//!    mesh per bound actor with index-parallel transforms (`5` floats each)
//!    and a non-empty pose for every poseable actor (rest fallback counts).
//! 3. **The camera export is live**: the battle opens on the far menu framing
//!    with the formation-sized depth at or above the retail floor, and an
//!    executing action takes the camera into the per-action close-up with a
//!    pose that actually moves.
//!
//! Skips + passes when `LEGAIA_DISC_BIN` is unset.

#![cfg(not(target_arch = "wasm32"))]

use legaia_web_viewer::runtime::LegaiaRuntime;

fn loaded_in_town() -> Option<LegaiaRuntime> {
    let disc = std::env::var("LEGAIA_DISC_BIN").ok()?;
    let bytes = std::fs::read(&disc).ok()?;
    let mut rt = LegaiaRuntime::new();
    rt.load_disc(bytes, String::new()).ok()?;
    rt.enter_field("town01").ok()?;
    Some(rt)
}

#[test]
fn battle_render_builds_backdrop_grid_actors_and_camera() {
    let Some(mut rt) = loaded_in_town() else {
        eprintln!("LEGAIA_DISC_BIN unset - skipping");
        return;
    };
    let field_vram = rt.field_vram_bytes();
    assert!(!field_vram.is_empty(), "field VRAM present before battle");
    assert!(
        !rt.play_battle_active(),
        "no battle render before a battle starts"
    );

    assert!(
        rt.debug_start_test_battle(),
        "a scripted formation off the town MAN enters battle"
    );
    assert!(
        rt.play_battle_active(),
        "battle render built on the Field -> Battle edge"
    );
    let generation = rt.play_battle_generation();
    assert!(generation > 0, "generation stamps the build");

    // Battle VRAM: present and not the field image (stage + flame atlas +
    // monster/party texture bands landed in the throwaway copy).
    let battle_vram = rt.play_battle_vram_bytes();
    assert_eq!(battle_vram.len(), field_vram.len(), "full VRAM export");
    assert_ne!(battle_vram, field_vram, "battle VRAM differs from field");

    // Backdrop: town01 has a stage stream, and the shell is drawn twice -
    // the appended second copy doubles both streams exactly.
    let pos = rt.play_battle_backdrop_positions();
    let idx = rt.play_battle_backdrop_indices();
    assert!(!pos.is_empty() && !idx.is_empty(), "backdrop geometry");
    assert_eq!(pos.len() % 6, 0, "vertex stream doubled by the second copy");
    assert_eq!(idx.len() % 6, 0, "index stream doubled by the second copy");
    let flat = rt.play_battle_backdrop_flat_rgba();
    assert_eq!(flat.len() * 3, pos.len() * 4, "hybrid flat RGBA per vertex");

    // Ground grid + its exported depth-cue parameters.
    assert!(
        !rt.play_battle_ground_positions().is_empty(),
        "ground grid geometry"
    );
    let cue: serde_json::Value =
        serde_json::from_str(&rt.play_battle_ground_cue_json()).expect("cue json");
    assert!(
        cue["far"].as_array().map(|a| a.len()) == Some(3),
        "cue far colour is an RGB triple: {cue}"
    );

    // Actors: at least the monster + the party leader, transforms
    // index-parallel at 5 floats per actor, and every poseable actor
    // reports a pose (live pose_frame or the build-time rest fallback).
    let n = rt.play_battle_actor_count();
    assert!(n >= 2, "monster + party actors bound (got {n})");
    let tf = rt.play_battle_actor_transforms();
    assert_eq!(tf.len(), n as usize * 5, "transforms index-parallel");
    let mut monsters = 0;
    for i in 0..n {
        let apos = rt.play_battle_actor_positions(i);
        assert!(!apos.is_empty(), "actor {i} geometry");
        if tf[i as usize * 5 + 3] > 0.5 {
            monsters += 1;
        }
        if !rt.play_battle_actor_object_ids(i).is_empty() {
            let pose = rt.play_battle_actor_pose(i);
            assert!(!pose.is_empty(), "actor {i} pose (live or rest)");
            assert_eq!(pose.len() % 6, 0, "actor {i} pose stride");
        }
    }
    assert!(monsters >= 1, "at least one enemy-side mesh");

    // Camera. On the Field -> Battle edge the shared phase script opens on
    // the far menu framing, sized to the live formation and at or above the
    // retail minimum depth (`0x800` world units through the
    // `(z << 8) / 0xA0` prescale = 3276).
    let cam: serde_json::Value =
        serde_json::from_str(&rt.play_battle_camera_json()).expect("camera json");
    assert_eq!(cam["active"], true, "camera active: {cam}");
    assert_eq!(
        cam["phase"], "menu",
        "battle opens on the far framing: {cam}"
    );
    assert!(
        cam["tr"][2].as_f64().unwrap_or(0.0) >= 3276.0,
        "menu framing depth at or above the retail floor: {cam}"
    );

    // This battle is not player-driven, so the SM arms an action on its own
    // within a few ticks and the camera follows it into the per-action
    // close-up (`FUN_801D5854` case 6). Both halves matter: the phase has to
    // change, and the pose has to actually move - a phase flip whose pose
    // never leaves the far framing would mean the script is not being driven
    // on this host's clock.
    //
    // The idle orbit itself is not asserted here: an auto-driven fight has no
    // settled idle window to observe it in. Its law lives in
    // `engine-vm::battle_cam_script` (`ORBIT_STEP` + the phase predicate).
    let pose0 = cam["tr"].clone();
    let mut acted = false;
    let mut moved = false;
    let mut last = cam.clone();
    for _ in 0..60 {
        let _ = rt.tick_frame();
        last = serde_json::from_str(&rt.play_battle_camera_json()).expect("camera json 2");
        acted |= last["phase"] == "action";
        moved |= last["tr"] != pose0;
        if acted && moved {
            break;
        }
    }
    assert!(acted, "an executing action takes the camera: {last}");
    assert!(moved, "the camera pose advances across ticks: {last}");
}
