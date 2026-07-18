//! Disc-gated: the New-Game opening's `map01` leg stages the retail Rim Elm
//! aerial fly-in through op-`0x45` camera beats - the ground truth a live
//! per-frame RAM capture of the camera globals (`0x8007B790` angles /
//! `0x800840B8` eye trio / `0x80089118` focus / `0x8007B6F4` H) pins:
//!
//! - beat B snaps to the high aerial shot (pitch 735, yaw 93, H 368,
//!   `tr_eye (-1268, -3756, 18784)`, focus `(12162, ?, 3510)` = over the
//!   Rim Elm coast), then
//! - beat C re-stages with `apply 900` and mode 2 (`45 0B ..` = quadratic
//!   ease-out on every component) - the descent to pitch 355 / yaw 333 /
//!   `tr_eye (412, -2336, 12384)` that the live capture matches to within
//!   sampling noise over the whole 6330-unit `tr_eye.z` travel.
//!
//! The test executes the chain by execution (opdeene → opstati → opurud →
//! map01) and asserts the map01 timeline's camera staging ends on exactly
//! the beat-C fly-in pose with the mode-2 / apply-900 glide parameters, so
//! the windowed world-map cutscene camera (which reads this staged state)
//! frames the retail fly-in.

use legaia_engine_core::scene::SceneHost;
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

fn skip_or_host() -> Option<SceneHost> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        return None;
    };
    Some(SceneHost::open_extracted(&extracted).expect("open SceneHost"))
}

#[test]
fn map01_leg_stages_the_retail_flyin_camera_beats() {
    let Some(mut host) = skip_or_host() else {
        return;
    };
    host.world.begin_new_game();
    host.enter_field_scene(legaia_asset::new_game::OPENING_CUTSCENE_SCENE, 0)
        .expect("enter opdeene");

    // Run the chain to the map01 leg (zero input, by execution).
    let mut ticks = 0u32;
    while host.world.active_scene_label != "map01" && ticks < 90_000 {
        let _ = host.tick();
        ticks += 1;
    }
    assert_eq!(
        host.world.active_scene_label, "map01",
        "the opening chain reaches the map01 fly-in leg (ticked {ticks})"
    );
    assert!(
        host.world.cutscene_timeline_active(),
        "map01's fly-in record installs as the cutscene timeline"
    );

    // Drive the map01 leg, watching for the mode-2 ease-out staging (the
    // `45 0B .. apply 900` descent beat) and the aerial beat-B pose.
    let mut saw_aerial_snap = false;
    let mut saw_mode2_descent = false;
    // Snapshot of the last map01-leg staging (the camera_state param set is
    // cleared on the town01 scene entry, so the end-of-leg pose must be
    // latched while the leg is still live).
    let mut last: Vec<(u8, i16)> = Vec::new();
    ticks = 0;
    while host.world.active_scene_label == "map01" && ticks < 30_000 {
        let _ = host.tick();
        ticks += 1;
        // Beat B (the aerial snap) and beat C (the glide) execute in the
        // SAME tick - the VM runs until yield and no yield separates the two
        // ops - so beat B is only observable in the per-beat event stream,
        // not in the merged `camera_state` (which ends the tick on beat C).
        // This is exactly why the windowed renderer replays drained
        // `apply == 0` beats as snaps before arming the glide.
        for ev in host.world.drain_field_events() {
            let legaia_engine_core::field_events::FieldEvent::CameraConfigure {
                params,
                apply_trigger,
                ..
            } = ev
            else {
                continue;
            };
            let beat = |slot: u8| {
                params
                    .iter()
                    .find(|p| p.slot == slot)
                    .map(|p| p.value as i16)
            };
            if apply_trigger == 0 && beat(5) == Some(18784) {
                // Beat B: the high aerial shot over the Rim Elm coast.
                saw_aerial_snap = true;
                assert_eq!(beat(0), Some(735), "aerial pitch");
                assert_eq!(beat(9), Some(368), "fly-in GTE H");
            }
        }
        if host.world.camera_state.mode == 2 && host.world.camera_state.apply_trigger == 900 {
            saw_mode2_descent = true;
        }
        // The transition tick clears camera_state (scene entry), so only
        // latch the pose while the leg is still live.
        if host.world.active_scene_label == "map01" {
            last = host
                .world
                .camera_state
                .params
                .iter()
                .map(|p| (p.slot, p.value as i16))
                .collect();
        }
    }
    assert!(saw_aerial_snap, "beat B staged the aerial snap pose");
    assert!(
        saw_mode2_descent,
        "beat C staged the mode-2 (all-components ease-out) apply-900 descent"
    );

    // The leg ends holding the beat-C fly-in pose: the capture-pinned final
    // camera (pitch 355, yaw 333, H 368, eye trio (412, -2336, 12384),
    // focus (12162, ?, 3510) - negated in slots 6/8).
    let end = |slot: u8| last.iter().find(|(s, _)| *s == slot).map(|(_, v)| *v);
    assert_eq!(end(0), Some(355), "final pitch");
    assert_eq!(end(1), Some(333), "final yaw");
    assert_eq!(end(3), Some(412), "final eye dx");
    assert_eq!(end(4), Some(-2336), "final eye dy");
    assert_eq!(end(5), Some(12384), "final eye depth");
    assert_eq!(end(6), Some(-12162), "focus X (negated)");
    assert_eq!(end(8), Some(-3510), "focus Z (negated)");
    assert_eq!(end(9), Some(368), "final H");
    assert_eq!(
        host.world.active_scene_label, "town01",
        "the fly-in chains into Rim Elm (ticked {ticks})"
    );
}
