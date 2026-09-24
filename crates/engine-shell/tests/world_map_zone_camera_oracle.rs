//! Retail oracle for the **overworld walk camera**: every settled kingdom
//! overworld state in the save library frames through the field zone camera,
//! and the port's world-map frame reproduces it.
//!
//! Retail runs the overworld as an ordinary mode-`0x03` field-run scene, and
//! its walk camera is the field overlay's follow camera - the MAN section-3
//! camera-region record composed by `FUN_801DAB90` into the staging
//! descriptor at `0x801F3580` and eased into the live globals. On a settled
//! overworld state the live pitch / yaw / eye trio / GTE `H` equal that
//! staging descriptor field for field, so the RAM is its own oracle:
//!
//! 1. **Zone camera engages** on the overworld (`Camera::zone.active` after
//!    one tick on the entered world-map scene).
//! 2. **Live pose** - the engine's `(pitch, yaw, H)` and eye trio against the
//!    live `_DAT_8007B790/92`, `_DAT_8007B6F4` and `_DAT_800840B8/BC/C0`.
//! 3. **Frame** - the shared resolver hands both hosts a
//!    `FieldCameraFrame::WorldMapWalk` whose view carries exactly those
//!    retail words (the eye trio in retail GTE units, since the walk frame
//!    applies the 6x world scale as a world transform).
//!
//! Skips (passes) unless `scripts/scenarios.toml`, `saves/library` and
//! `extracted/` are all present. Structural assertions only.

use std::path::PathBuf;

use legaia_engine_core::camera::Camera;
use legaia_engine_core::camera_view::{FieldCameraFrame, resolve_field_camera};
use legaia_engine_core::scene::SceneHost;
use legaia_mednafen::game_anchors;
use legaia_mednafen::{SaveState, ScenarioManifest};

const RAM_BASE: u32 = 0x8000_0000;
const GTE_H: u32 = 0x8007_B6F4;
const CAM_ROT: u32 = 0x8007_B790;
const CAM_EYE: u32 = 0x8008_40B8;
const STAGING: u32 = 0x801F_3580;

fn library() -> Option<(ScenarioManifest, PathBuf)> {
    let manifest = ["scripts/scenarios.toml", "../../scripts/scenarios.toml"]
        .into_iter()
        .map(PathBuf::from)
        .find(|p| p.exists())?;
    let lib = ["saves/library", "../../saves/library"]
        .into_iter()
        .map(PathBuf::from)
        .find(|p| p.is_dir())?;
    Some((ScenarioManifest::from_path(&manifest).ok()?, lib))
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

fn off(va: u32) -> usize {
    (va - RAM_BASE) as usize & 0x1F_FFFF
}

fn rs16(ram: &[u8], va: u32) -> i16 {
    let o = off(va);
    i16::from_le_bytes([ram[o], ram[o + 1]])
}

fn rs32(ram: &[u8], va: u32) -> i32 {
    let o = off(va);
    i32::from_le_bytes([ram[o], ram[o + 1], ram[o + 2], ram[o + 3]])
}

struct Overworld {
    label: String,
    scene: String,
    player: [i16; 3],
    /// Live `(pitch, yaw, H)`.
    live: (i16, i16, i16),
    live_eye: [i32; 3],
    /// Whether the staging descriptor is this frame's composition: its focus
    /// names the state's player and its pose equals the live one.
    settled: bool,
}

fn overworld_states(manifest: &ScenarioManifest, lib: &std::path::Path) -> Vec<Overworld> {
    let mut out = Vec::new();
    for sc in &manifest.scenarios {
        let Some(path) = manifest.library_save_path(sc, lib) else {
            continue;
        };
        if !path.exists() || path.extension().is_some_and(|e| e != "mcr") {
            continue;
        }
        let Ok(st) = SaveState::from_path(&path) else {
            continue;
        };
        let Ok(ram) = st.main_ram() else {
            continue;
        };
        if game_anchors::game_mode(ram) != 0x03 {
            continue;
        }
        let scene = game_anchors::scene_name(ram);
        if !matches!(scene.as_str(), "map01" | "map02" | "map03") {
            continue;
        }
        let Some(p) = game_anchors::player_ptr(ram) else {
            continue;
        };
        let player = [
            rs16(ram, p + 0x14),
            rs16(ram, p + 0x16),
            rs16(ram, p + 0x18),
        ];
        let live = (rs16(ram, CAM_ROT), rs16(ram, CAM_ROT + 2), rs16(ram, GTE_H));
        let live_eye = [
            rs32(ram, CAM_EYE),
            rs32(ram, CAM_EYE + 4),
            rs32(ram, CAM_EYE + 8),
        ];
        let staged = (
            rs16(ram, STAGING + 0x02),
            rs16(ram, STAGING + 0x06),
            rs16(ram, STAGING + 0x26),
        );
        let staged_eye = [
            i32::from(rs16(ram, STAGING + 0x0E)),
            i32::from(rs16(ram, STAGING + 0x12)),
            i32::from(rs16(ram, STAGING + 0x16)),
        ];
        let staged_at = [-rs16(ram, STAGING + 0x1A), -rs16(ram, STAGING + 0x22)];
        let settled =
            staged == live && staged_eye == live_eye && staged_at == [player[0], player[2]];
        out.push(Overworld {
            label: sc.label.clone(),
            scene,
            player,
            live,
            live_eye,
            settled,
        });
    }
    out
}

#[test]
fn the_overworld_walk_camera_is_the_zone_camera() {
    let Some((manifest, lib)) = library() else {
        eprintln!("[skip] scripts/scenarios.toml or saves/library missing");
        return;
    };
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing");
        return;
    };
    let states = overworld_states(&manifest, &lib);
    if states.is_empty() {
        eprintln!("[skip] no overworld state in the library");
        return;
    }
    let settled: Vec<&Overworld> = states.iter().filter(|s| s.settled).collect();
    assert!(
        !settled.is_empty(),
        "no settled overworld state: retail's live camera should equal the \
         follow composer's staging descriptor on a resident overworld"
    );

    let mut pose_ok = 0usize;
    let mut misses = Vec::new();
    for s in &settled {
        let tag = format!("{} ({})", s.scene, s.label);
        let mut host = SceneHost::open_extracted(&extracted).expect("scene host");
        host.world.begin_new_game();
        host.enter_world_map_scene(&s.scene)
            .unwrap_or_else(|e| panic!("{tag}: enter: {e}"));
        let slot = host.world.player_actor_slot.expect("player actor");
        {
            let a = &mut host.world.actors[slot as usize];
            a.move_state.world_x = s.player[0];
            a.move_state.world_y = s.player[1];
            a.move_state.world_z = s.player[2];
        }
        host.world.cutscene.timeline = None;
        host.world.camera.state.params.clear();
        let mut cam = Camera::default();
        cam.reset_globals_for_scene_entry();
        cam.tick(&host.world);
        assert!(
            cam.zone.active,
            "{tag}: the zone camera must own the overworld walk camera"
        );

        let g = &cam.globals.0;
        let engine = (g[0] as i16, g[1] as i16, g[9] as i16);
        let engine_eye = [g[3], g[4], g[5]];
        let ok = engine == s.live && engine_eye == s.live_eye;
        pose_ok += usize::from(ok);
        if !ok {
            misses.push(format!(
                "{tag}: engine (pitch, yaw, H) {engine:?} eye {engine_eye:?} vs retail {:?} {:?}",
                s.live, s.live_eye
            ));
            continue;
        }

        // The frame both hosts render: the world-map walk arm, carrying the
        // retail words verbatim.
        let frame = resolve_field_camera(&host.world, &cam, None, [0.0, 0.0]);
        let FieldCameraFrame::WorldMapWalk { view, player } = frame else {
            panic!("{tag}: expected the world-map walk frame, got {frame:?}");
        };
        let to_units = |r: f32| (r / std::f32::consts::TAU * 4096.0).round() as i32;
        assert_eq!(to_units(view.pitch), i32::from(s.live.0), "{tag}: pitch");
        assert_eq!(to_units(view.yaw), i32::from(s.live.1), "{tag}: yaw");
        assert_eq!(view.h, f32::from(s.live.2), "{tag}: H");
        for (i, (&e, &r)) in view.tr_eye.iter().zip(s.live_eye.iter()).enumerate() {
            assert!(
                (e - r as f32).abs() < 0.01,
                "{tag}: eye[{i}] {e} vs retail {r}"
            );
        }
        assert_eq!(player[0], f32::from(s.player[0]), "{tag}: focus X");
        assert_eq!(player[2], f32::from(s.player[2]), "{tag}: focus Z");
        assert_eq!(player[1], 0.0, "{tag}: retail's focus Y is 0");
    }
    for m in &misses {
        eprintln!("[miss] {m}");
    }
    assert!(
        misses.is_empty(),
        "{} of {} settled overworld states frame off retail's pose",
        misses.len(),
        settled.len()
    );
    eprintln!(
        "[ok] {pose_ok}/{} settled overworld states (of {} overworld states) frame at \
         retail's live pose through the zone camera",
        settled.len(),
        states.len()
    );
}
