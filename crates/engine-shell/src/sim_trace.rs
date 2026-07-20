//! Recomp-differential sim trace - the engine side of the canonical
//! frame-tagged state-trace JSONL that `scripts/recomp/trace_capture.py`
//! captures from the static recomp. Both sides emit the same shape in
//! RETAIL units (PSX 12-bit angles, retail world units) so
//! `scripts/recomp/trace_diff.py` aligns and compares them 1:1.
//!
//! Canonical line shape (one JSON object per line; every field except
//! `frame` optional per line):
//!
//! ```json
//! {"frame": 0, "scene": "town01", "mode": 3,
//!  "cam": {"pitch":32,"yaw":3718,"roll":0,"h":256,
//!          "eye":[0,1280,7920], "focus":[0,0,0]},
//!  "player": {"x":100,"z":200,"heading":0},
//!  "actors": [{"i":1,"x":-40,"z":80,"heading":1024}]}
//! ```
//!
//! Retail-unit mapping on the engine side:
//!
//! - Every `cam.*` channel is the corresponding **live retail camera global**
//!   off [`Camera::globals`](legaia_engine_core::camera::Camera), which is the
//!   same word `trace_capture.py` reads out of the recomp:
//!
//!   | channel | global | note |
//!   |---|---|---|
//!   | `pitch` / `yaw` / `roll` | `0x8007B790/92/94` | 12-bit, masked to `0xFFF` |
//!   | `eye` | `0x800840B8` | the eye-**space** translation trio, *not* a world eye position |
//!   | `focus` | `0x80089118` | the focus as retail stores it - X and Z **negated** |
//!   | `h` | `0x8007B6F4` | GTE projection H; absent until a configure carries slot 9 |
//!
//!   The `eye` / `focus` rows are the ones worth reading twice. Emitting the
//!   runtime camera's world-space `eye` / `look_at` here instead - as this
//!   module originally did - compares different quantities in different
//!   coordinate frames, which reads as a total divergence on every scene and
//!   cannot be closed by any change to the camera itself.
//! - `player` / `actors[i]`: `move_state.world_x` / `world_z` (retail world
//!   units) + `render_26` (the retail `+0x26` heading) masked to 12 bits.
//! - `mode`: the retail game-mode word (`_DAT_8007B83C` space) mapped from
//!   [`SceneMode`]: Field=3, WorldMap=13, Battle=21, Menu=23, Cutscene=27.
//!   Omitted for modes with no modeled retail equivalent (Title, the
//!   minigame sessions), so a diff flags them as absent instead of faking a
//!   match.
//!
//! [`World::camera_state`]: legaia_engine_core::world::World

use std::f32::consts::TAU;
use std::path::Path;

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::{BootConfig, BootSession};
use legaia_engine_core::world::SceneMode;

/// Camera pose sample in retail units.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CamSample {
    /// 12-bit pitch (4096 = full turn), from the camera controller.
    pub pitch: u16,
    /// 12-bit yaw.
    pub yaw: u16,
    /// 12-bit roll from the last op-`0x45` slot-2 payload; absent until a
    /// configure carries slot 2.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub roll: Option<u16>,
    /// GTE projection H from the last op-`0x45` slot-9 payload; absent until
    /// a configure carries slot 9.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub h: Option<u16>,
    /// Eye position, world units.
    pub eye: [i32; 3],
    /// Focus (look-at) point, world units.
    pub focus: [i32; 3],
}

/// Player position + heading sample in retail units.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlayerSample {
    pub x: i32,
    pub z: i32,
    /// 12-bit heading (`render_26` masked; 0 faces +Z).
    pub heading: u16,
}

/// One active actor slot's position + heading.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActorSample {
    /// Slot index in [`World::actors`](legaia_engine_core::world::World).
    pub i: u8,
    pub x: i32,
    pub z: i32,
    pub heading: u16,
}

/// One frame of the canonical trace.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SimTraceFrame {
    pub frame: u64,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub scene: Option<String>,
    /// Retail game-mode word equivalent; see the module docs for the
    /// [`SceneMode`] mapping.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub mode: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub cam: Option<CamSample>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub player: Option<PlayerSample>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub actors: Vec<ActorSample>,
}

/// Convert the camera controller's radians to 12-bit retail units.
pub fn radians_to_units(rad: f32) -> u16 {
    let units = rad / TAU * 4096.0;
    (units.round().rem_euclid(4096.0)) as u16 & 0xFFF
}

/// Map the engine [`SceneMode`] onto the retail game-mode word
/// (`_DAT_8007B83C` space, per-frame handler modes). `None` for modes the
/// port hosts without a retail game-mode equivalent.
pub fn scene_mode_to_retail(mode: SceneMode) -> Option<u16> {
    match mode {
        SceneMode::Field => Some(3),     // MAIN MODE (field/town per-frame)
        SceneMode::WorldMap => Some(13), // MAPDISP MODE
        SceneMode::Battle => Some(21),   // battle per-frame (0x15)
        SceneMode::Menu => Some(23),     // CARD MODE (0x17)
        SceneMode::Cutscene => Some(27), // STR per-frame
        SceneMode::Title
        | SceneMode::Dance
        | SceneMode::Fishing
        | SceneMode::SlotMachine
        | SceneMode::BakaFighter
        | SceneMode::MuscleDome => None,
    }
}

/// Sample one canonical frame from a live [`BootSession`].
pub fn sample_frame(session: &BootSession) -> SimTraceFrame {
    let world = &session.host.world;
    let cam = &session.camera;

    // Roll / H come from the raw op-0x45 configure payload; the runtime
    // camera doesn't model them.
    let slot = |s: u8| {
        world
            .camera_state
            .params
            .iter()
            .find(|p| p.slot == s)
            .map(|p| p.value)
    };
    // Every camera channel is read from the engine's live retail camera
    // globals - the same ten words `trace_capture.py` reads out of the recomp
    // (`0x8007B790/92/94`, `0x800840B8`, `0x80089118`, `0x8007B6F4`). Emitting
    // the runtime camera's world-space `eye` / `look_at` instead made these
    // channels incomparable by construction: retail's `eye` is the eye-SPACE
    // translation trio and its `focus` is stored negated in X/Z, so the two
    // sides were reporting different quantities in different frames and the
    // diff read as a total divergence no camera fix could ever have closed.
    let g = &cam.globals;
    let angles = g.angles();
    let cam_sample = CamSample {
        pitch: (angles[0] as u16) & 0xFFF,
        yaw: (angles[1] as u16) & 0xFFF,
        roll: Some((angles[2] as u16) & 0xFFF),
        h: slot(9).map(|_| g.h() as u16),
        eye: g.tr_eye(),
        focus: g.focus_stored(),
    };

    let actor_sample = |i: usize, a: &legaia_engine_core::world::Actor| ActorSample {
        i: i as u8,
        x: a.move_state.world_x as i32,
        z: a.move_state.world_z as i32,
        heading: (a.move_state.render_26 as u16) & 0xFFF,
    };
    let actors: Vec<ActorSample> = world
        .actors
        .iter()
        .enumerate()
        .filter(|(_, a)| a.active)
        .map(|(i, a)| actor_sample(i, a))
        .collect();
    // Slot 0 is the field player (`World::install_field_player(0)`).
    let player = world
        .actors
        .first()
        .filter(|a| a.active)
        .map(|a| PlayerSample {
            x: a.move_state.world_x as i32,
            z: a.move_state.world_z as i32,
            heading: (a.move_state.render_26 as u16) & 0xFFF,
        });

    SimTraceFrame {
        frame: session.frames,
        scene: session.host.scene.as_ref().map(|s| s.name.clone()),
        mode: scene_mode_to_retail(world.mode),
        cam: Some(cam_sample),
        player,
        actors,
    }
}

/// Boot a scene and tick `frames` sim frames, sampling the canonical trace
/// after boot and after every tick (`frames + 1` records). With
/// `field_live`, the session drops into a live field scene first
/// ([`BootSession::enter_field_live`]) so the field VM + locomotion +
/// camera events actually run, the same way the windowed host arms them.
pub fn build_sim_trace(
    scene_name: &str,
    extracted_root: &Path,
    disc: Option<&Path>,
    frames: u64,
    field_live: bool,
) -> Result<Vec<SimTraceFrame>> {
    let cfg = BootConfig {
        scene: scene_name.to_string(),
        enable_audio: false,
    };
    let mut session = match disc {
        Some(p) => BootSession::open_disc(p, &cfg)?,
        None => BootSession::open(extracted_root, &cfg)?,
    };
    if field_live {
        session.enter_field_live(scene_name, &crate::boot::FieldLiveOpts::default())?;
    }
    let mut out = Vec::with_capacity((frames as usize).saturating_add(1));
    out.push(sample_frame(&session));
    for _ in 0..frames {
        session.tick()?;
        out.push(sample_frame(&session));
    }
    Ok(out)
}

/// Serialize a trace to the canonical JSONL (one object per line).
pub fn sim_trace_to_jsonl(trace: &[SimTraceFrame]) -> String {
    let mut s = String::new();
    for frame in trace {
        s.push_str(&serde_json::to_string(frame).expect("SimTraceFrame serializes"));
        s.push('\n');
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn radians_round_trip_retail_units() {
        // The op-0x45 decode is `units * TAU / 4096`; the trace encode must
        // invert it exactly over the whole 12-bit space.
        for units in [0u16, 1, 32, 1024, 2048, 3718, 4095] {
            let rad = (units as i16) as f32 * TAU / 4096.0;
            assert_eq!(radians_to_units(rad), units, "units {units}");
        }
        // Negative radians wrap into the 12-bit space.
        assert_eq!(radians_to_units(-TAU / 4.0), 3072);
    }

    #[test]
    fn scene_mode_mapping_matches_retail_table() {
        assert_eq!(scene_mode_to_retail(SceneMode::Field), Some(3));
        assert_eq!(scene_mode_to_retail(SceneMode::Battle), Some(0x15));
        assert_eq!(scene_mode_to_retail(SceneMode::Menu), Some(0x17));
        assert_eq!(scene_mode_to_retail(SceneMode::WorldMap), Some(13));
        assert_eq!(scene_mode_to_retail(SceneMode::Title), None);
    }

    #[test]
    fn jsonl_shape_is_canonical() {
        let frame = SimTraceFrame {
            frame: 7,
            scene: Some("town01".into()),
            mode: Some(3),
            cam: Some(CamSample {
                pitch: 32,
                yaw: 3718,
                roll: Some(0),
                h: Some(256),
                eye: [0, 1280, 7920],
                focus: [0, 0, 0],
            }),
            player: Some(PlayerSample {
                x: 100,
                z: 200,
                heading: 0,
            }),
            actors: vec![ActorSample {
                i: 1,
                x: -40,
                z: 80,
                heading: 1024,
            }],
        };
        let line = sim_trace_to_jsonl(std::slice::from_ref(&frame));
        assert_eq!(line.matches('\n').count(), 1);
        let v: serde_json::Value = serde_json::from_str(line.trim()).unwrap();
        assert_eq!(v["frame"], 7);
        assert_eq!(v["scene"], "town01");
        assert_eq!(v["mode"], 3);
        assert_eq!(v["cam"]["yaw"], 3718);
        assert_eq!(v["cam"]["eye"][2], 7920);
        assert_eq!(v["player"]["heading"], 0);
        assert_eq!(v["actors"][0]["i"], 1);
        // Round-trips through serde.
        let back: SimTraceFrame = serde_json::from_str(line.trim()).unwrap();
        assert_eq!(back, frame);
    }

    #[test]
    fn optional_fields_are_omitted_not_zeroed() {
        let frame = SimTraceFrame {
            frame: 0,
            scene: None,
            mode: None,
            cam: None,
            player: None,
            actors: vec![],
        };
        let line = sim_trace_to_jsonl(&[frame]);
        let v: serde_json::Value = serde_json::from_str(line.trim()).unwrap();
        assert!(v.get("scene").is_none());
        assert!(v.get("mode").is_none());
        assert!(v.get("cam").is_none());
        assert!(v.get("player").is_none());
        assert!(v.get("actors").is_none());
    }
}
