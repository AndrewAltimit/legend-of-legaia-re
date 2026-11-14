//! Disc-gated: when a talk's scripted shot lets the camera go, the enhanced
//! distance preset comes back over the follow ease, not in one frame.
//!
//! While a record holds a shot the follow knobs stand down
//! (`World::scripted_shot_holds_camera`). They used to return on the first
//! free frame at full reach, so under `Far` the frame jumped by the preset's
//! scaling the moment the talk ended. `Camera::knob_blend` brings them back
//! one ease step per pass the follow ease runs.
//!
//! The measure is the gap between the follow frame the camera draws and the
//! one it would draw at the retail preset, frame by frame after the talk: it
//! must open gradually. `koin4` P1[1] ends on a shot it never hands back;
//! `ropeway` P1[45] hands its shot back with an APPLY.
//!
//! Skips and passes without `LEGAIA_DISC_BIN` / `extracted/`.

use std::path::PathBuf;

use legaia_engine_camera::camera_distance::CameraDistance;
use legaia_engine_session::native::NativeSession;
use legaia_engine_session::session::{BootConfig, FieldLiveOpts};
use legaia_engine_system::input::PadButton;
use legaia_engine_world_views::camera_view::field_follow_view;

fn extracted_dir() -> Option<PathBuf> {
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

/// The distance between the drawn follow eye and the retail-preset one.
fn preset_gap(s: &NativeSession) -> Option<f32> {
    let far = field_follow_view(&s.camera, &s.host.world)?.eye();
    let mut retail = s.camera.clone();
    retail.distance = CameraDistance::Retail;
    let ret = field_follow_view(&retail, &s.host.world)?.eye();
    Some(
        far.iter()
            .zip(ret.iter())
            .map(|(a, b)| (a - b) * (a - b))
            .sum::<f32>()
            .sqrt(),
    )
}

/// `(largest one-frame growth of the gap, the gap the walk settles on)`.
fn run(extracted: &std::path::Path, scene: &str, slot: u8) -> Option<(f32, f32)> {
    let cfg = BootConfig {
        scene: scene.into(),
        enable_audio: false,
    };
    let mut s = NativeSession::open(extracted, &cfg).ok()?;
    s.host.world.toggles.use_vm_dialogue = true;
    s.enter_field_live(scene, &FieldLiveOpts::default()).ok()?;
    s.set_host_drains_queues(true);
    for _ in 0..20 {
        s.tick().ok()?;
    }
    let &(nx, nz) = s.host.world.npcs.positions.get(&slot)?;
    s.host
        .world
        .seat_player_at_tile((nx >> 7) as u8, ((nz >> 7) as u8).saturating_add(1));
    for _ in 0..4 {
        s.tick().ok()?;
    }
    s.camera.distance = CameraDistance::Far;
    s.host.world.trigger_field_interact(0, slot);
    let mut talked = false;
    let mut staged = false;
    let mut prev: Option<f32> = None;
    let mut worst = 0.0f32;
    let mut measure = |s: &NativeSession, staged: bool| {
        if let Some(g) = preset_gap(s)
            && staged
        {
            if let Some(p) = prev {
                worst = worst.max(g - p);
            }
            prev = Some(g);
        }
    };
    for t in 0..6000u32 {
        let busy = s.host.world.dialogue_owns_input();
        talked |= busy;
        staged |= busy && s.host.world.scripted_shot_holds_camera();
        if talked && !busy {
            break;
        }
        s.host.world.set_pad(if busy && t % 8 == 0 {
            PadButton::Cross.mask()
        } else {
            0
        });
        s.tick().ok()?;
        measure(&s, staged);
    }
    assert!(staged, "{scene} P1[{slot}] staged no shot to hand back");
    // Stand a moment (the shot stays until the player walks), then walk.
    for t in 0..240 {
        s.host
            .world
            .set_pad(if t < 30 { 0 } else { PadButton::Down.mask() });
        s.tick().ok()?;
        measure(&s, true);
    }
    Some((worst, prev.unwrap_or(0.0)))
}

#[test]
fn the_preset_eases_back_in_after_a_talk_shot() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        return;
    };
    for (scene, slot) in [("koin4", 1u8), ("ropeway", 45)] {
        let (worst, settled) = run(&extracted, scene, slot).expect("talk runs");
        eprintln!("[hand-back] {scene} P1[{slot}] worst step {worst:.1}, settled gap {settled:.1}");
        assert!(settled > 1.0, "{scene}: the preset never came back");
        assert!(
            worst < settled * 0.5,
            "{scene}: the preset jumped back ({worst:.1} of {settled:.1} in one frame)"
        );
    }
}
