//! Part C: every captured scene is one the engine can enter.

use super::*;

// ---------------------------------------------------------------------------
// Part C: cross-check against the capture library
// ---------------------------------------------------------------------------

/// Retail was demonstrably standing in these scenes, so the engine has to be
/// able to enter them.
///
/// The cataloged captures are the only *retail* evidence this repo has about
/// chapter-1 scenes past the Ravine. For every capture whose recorded scene
/// is in the chapter-1 closure, the ladder's verdict for that scene must have
/// cleared at least rung 3 (`enter`) - an engine claim checked against a
/// capture, rather than against itself.
///
/// Capture-grounded. Skips when the library or the manifest is absent.
#[test]
fn part_c_captured_scenes_are_scenes_the_engine_can_enter() {
    let Some(mut host) = open_host() else {
        return;
    };
    let (Some(manifest_path), Some(library)) = (manifest_path(), library_dir()) else {
        eprintln!("[skip] scripts/scenarios.toml or saves/library missing (capture-gated)");
        return;
    };
    let manifest =
        legaia_ram_image::ScenarioManifest::from_path(&manifest_path).expect("parse manifest");

    let Closure { order, .. } = chapter1_closure(&host.index);
    let closure: BTreeSet<&str> = order.iter().map(String::as_str).collect();

    // Retail-observed scene names, read out of the capture's own main RAM
    // (not out of the manifest's `expected_active_scene` annotation - the
    // annotation is a human note, the RAM is the measurement).
    let mut observed: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for scn in &manifest.scenarios {
        let Some(path) = manifest.library_save_path(scn, library.as_path()) else {
            continue;
        };
        if !path.exists() {
            continue;
        }
        let ram = match path.extension().and_then(|e| e.to_str()) {
            Some("sstate") => legaia_pcsx_redux::SaveState::from_path(&path)
                .ok()
                .map(|s| s.main_ram().to_vec()),
            _ => legaia_mednafen::SaveState::from_path(&path)
                .ok()
                .and_then(|s| s.main_ram().ok().map(<[u8]>::to_vec)),
        };
        let Some(ram) = ram else { continue };
        let scene = legaia_ram_image::game_anchors::scene_name(&ram);
        if closure.contains(scene.as_str()) {
            observed.entry(scene).or_default().push(scn.label.clone());
        }
    }
    if observed.is_empty() {
        eprintln!("[skip] no capture in the library sits in a chapter-1 closure scene");
        return;
    }

    // Rung 3 is what this part asserts, so it drives rung 3 rather than the
    // whole ladder: the claim is "retail was standing here, so the engine has
    // to be able to enter it", and running the walk control + exit attempts
    // for nine scenes would cost minutes to test something Part B already
    // scored.
    let base = FlagBaseline::snapshot(&host);
    let mut failures: Vec<String> = Vec::new();
    for (scene, labels) in &observed {
        let entered = enter(&mut host, scene, &base);
        let settled =
            entered && matches!(settle(&mut host), Settle::Released | Settle::Departed(_));
        eprintln!(
            "[capture] {scene:<10} enter={entered} settle={settled} <- {} capture(s): {}",
            labels.len(),
            labels.join(", ")
        );
        if !entered {
            failures.push(format!(
                "{scene}: retail was in it ({}) but the engine cannot enter it \
                 (mode {:?})",
                labels.join(", "),
                host.world.mode
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "captured chapter-1 scenes the engine cannot enter:\n{}",
        failures.join("\n")
    );
    eprintln!(
        "[ok] Part C: {} captured chapter-1 scene(s) all enter in-engine",
        observed.len()
    );
}
