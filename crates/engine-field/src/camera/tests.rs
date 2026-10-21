use super::*;

#[test]
fn zone_arrival_counts_down_then_queries_at_region_tile() {
    let mut cd: i16 = 2;
    // Two waiting frames (2 -> 1, 1 -> 0), then expiry at -1.
    assert_eq!(
        camera_zone_arrival_tick(&mut cd, (0, 0), |_, _| true),
        CameraZoneArrival::Waiting
    );
    assert_eq!(
        camera_zone_arrival_tick(&mut cd, (0, 0), |_, _| true),
        CameraZoneArrival::Waiting
    );
    let mut seen = None;
    let r = camera_zone_arrival_tick(&mut cd, (1838, 2526), |x, z| {
        seen = Some((x, z));
        true
    });
    assert_eq!(r, CameraZoneArrival::LoadZoneConfig);
    // Region-refresh quantisation: (pos - 0x40) >> 7.
    assert_eq!(seen, Some(((1838 - 0x40) >> 7, (2526 - 0x40) >> 7)));
}

#[test]
fn zone_arrival_miss_installs_defaults() {
    let mut cd: i16 = 0;
    assert_eq!(
        camera_zone_arrival_tick(&mut cd, (0x40, 0x40), |_, _| false),
        CameraZoneArrival::LoadDefaults
    );
    // The default set includes the scene-entry pitch and GTE far plane.
    assert!(CAMERA_ZONE_DEFAULTS.contains(&(0x8007B60C, 0x1B8)));
    assert_eq!(CAMERA_ZONE_DEFAULTS.len(), 9);
}

/// Scene entry restores the `FUN_80025C24` field defaults so a departing
/// scene's shot cannot leak into the next one - the six axes retail
/// writes. Focus and `H` are the scene's to establish, so they survive.
#[test]
fn scene_entry_resets_globals_to_field_defaults() {
    let mut c = Camera {
        globals: RetailCamGlobals([1, 2, 3, 4, 5, 6, 7, 8, 9, 10]),
        ..Default::default()
    };
    c.reset_globals_for_scene_entry();
    assert_eq!(c.globals.angles(), [0x1B8, 0x64, 0]);
    assert_eq!(c.globals.tr_eye(), [0, -256, 16420]);
    assert_eq!(c.globals.0[6..=8], [7, 8, 9], "focus is not a reset axis");
    assert_eq!(c.globals.h(), 10, "H keeps the register's live value");
}

/// The field draw context (`FUN_801DE37C`) re-stamps the visible-tile
/// window on every field entry, so a scene that scripted a wide window
/// through op `0x46` cannot hand it to the next scene's focus edge clamp.
#[test]
fn scene_entry_restamps_the_field_draw_context_view_window() {
    let mut c = Camera::new();
    let default = {
        let (x0, z0, x1, z1) =
            legaia_engine_system::mode_entry_init::field_draw_context().view_window;
        [x0, z0, x1, z1]
    };
    assert_eq!(c.zone.view_window, default, "boot seeds the field box");
    // A script (op `0x46`) widens the window in the departing scene.
    c.zone.view_window = [-30, -30, 30, 30];
    c.reset_globals_for_scene_entry();
    assert_eq!(
        c.zone.view_window, default,
        "the next scene starts from the draw context's own box"
    );
}

/// A camera that never carried an `H` (the boot default, or a headless
/// host) gets the field register value, never `0`: a glide beat naming
/// slot `9` must start from the `H` retail was projecting through.
#[test]
fn scene_entry_seeds_a_missing_h_with_the_field_value() {
    let mut c = Camera {
        globals: RetailCamGlobals([0; AXIS_COUNT]),
        ..Default::default()
    };
    c.reset_globals_for_scene_entry();
    assert_eq!(c.globals.h(), 512);
    assert_eq!(
        Camera::new().globals.h(),
        512,
        "the boot default carries it too"
    );
}

#[test]
fn compass_azimuth_defaults_to_zero_and_sums_bias() {
    let c = Camera::default();
    assert_eq!(c.compass_azimuth_units(), 0, "defaults keep the old feed");
    let c = Camera {
        yaw: std::f32::consts::PI,
        manual_orbit: std::f32::consts::FRAC_PI_2,
        render_yaw_bias: std::f32::consts::FRAC_PI_2,
        ..Default::default()
    };
    // pi + pi/2 + pi/2 = full turn -> wraps to 0.
    assert_eq!(c.compass_azimuth_units(), 0);
}

/// The composed pitch is continuous in the tilt and exact at zero, even
/// for a scene pitch outside the knob range.
#[test]
fn composed_pitch_is_exact_at_zero_tilt_and_clamps_otherwise() {
    use follow_knobs::{PITCH_MAX, PITCH_MIN, composed_pitch};
    assert_eq!(composed_pitch(0.69, 0.0), 0.69);
    assert_eq!(
        composed_pitch(0.05, 0.0),
        0.05,
        "an out-of-range scene pitch passes"
    );
    assert_eq!(composed_pitch(1.5, 0.0), 1.5);
    assert_eq!(composed_pitch(0.69, 5.0), PITCH_MAX);
    assert_eq!(composed_pitch(0.69, -5.0), PITCH_MIN);
    assert_eq!(
        composed_pitch(1.5, 5.0),
        1.5,
        "widened range ends at the scene pitch"
    );
    assert!(
        (composed_pitch(0.69, 0.001) - 0.691).abs() < 1e-6,
        "no snap"
    );
}

/// A direct scene entry (picker / warp / load) after an interrupted
/// cutscene: the shot's globals and the focus latch are gone, the follow
/// camera owns the frame again, and the user's knobs survive.
#[test]
fn reset_for_scene_entry_drops_an_interrupted_shot_and_keeps_the_knobs() {
    let mut c = Camera {
        mode: CameraMode::Cinematic,
        yaw: 1.0,
        pitch: 0.4,
        roll: 0.1,
        manual_orbit: 0.5,
        manual_tilt: 0.2,
        manual_zoom: 1.5,
        ..Default::default()
    };
    // The interrupted shot: angles, eye trio, a focus the script owns.
    c.globals.0[0] = 700;
    c.globals.0[1] = -900;
    c.globals.0[2] = 50;
    c.globals.0[3] = -1234;
    c.globals.0[4] = 2345;
    c.globals.0[5] = 20000;
    c.script_owns_focus = true;
    c.mover = Some(CameraMover::default());
    c.reset_for_scene_entry();
    for axis in RetailCamGlobals::FIELD_RESET_AXES {
        assert_eq!(
            c.globals.0[axis],
            RetailCamGlobals::FIELD_RESET.0[axis],
            "axis {axis}"
        );
    }
    assert!(!c.script_owns_focus, "the follow camera re-pins the focus");
    assert!(c.mover.is_none(), "no glide survives");
    assert_eq!(c.mode, CameraMode::Follow);
    assert_eq!((c.yaw, c.pitch, c.roll), (0.0, 0.0, 0.0));
    assert_eq!(
        (c.manual_orbit, c.manual_tilt, c.manual_zoom),
        (0.5, 0.2, 1.5),
        "user framing intent is kept"
    );
}

#[test]
fn tick_script_advances_motion_state() {
    let mut c = Camera::default();
    c.motion_state.speed = 2;
    c.motion_target = MotionTarget {
        x: 4,
        y: 0,
        z: 0,
        id: 0,
    };
    // 0x41 compass walk: direction 6 (+X), div 4, two units (total 8
    // speed units); speed 2 at rate 0x40 moves X by 0x40*2/4 = 32.
    let bc = [0x41, 0x06, 0x02];
    let r1 = c.tick_script(&bc);
    assert_eq!(r1, StepResult::Yield);
    assert_eq!(c.motion_state.world_x, 32);
}
