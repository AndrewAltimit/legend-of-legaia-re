//! Unit tests for the phase gate's environment form.

use super::*;

#[test]
fn the_phase_gate_round_trips_through_its_env_form() {
    for g in [
        PhaseGate {
            action_state: 0x33,
            fade: None,
            module_phase: None,
            cam_accum: None,
            walk_yaw: None,
            done_hold: None,
            module_countdown: None,
            accum_align: None,
            camera_pass: false,
            stream_wait: false,
            sideband_wait: false,
        },
        PhaseGate {
            action_state: 0x35,
            fade: Some(RetailFade {
                to_white: false,
                age: 24,
            }),
            module_phase: None,
            cam_accum: None,
            walk_yaw: None,
            done_hold: None,
            module_countdown: None,
            accum_align: None,
            camera_pass: false,
            stream_wait: false,
            sideband_wait: false,
        },
        PhaseGate {
            action_state: 0x36,
            fade: None,
            module_phase: Some(6),
            cam_accum: Some(72),
            walk_yaw: Some(1497),
            done_hold: None,
            module_countdown: Some(-16),
            accum_align: Some(640),
            camera_pass: false,
            stream_wait: false,
            sideband_wait: false,
        },
        PhaseGate {
            action_state: 0x35,
            fade: Some(RetailFade {
                to_white: false,
                age: 20,
            }),
            module_phase: Some(2),
            cam_accum: None,
            walk_yaw: None,
            done_hold: None,
            module_countdown: None,
            accum_align: None,
            camera_pass: false,
            stream_wait: true,
            sideband_wait: false,
        },
        PhaseGate {
            action_state: 0x35,
            fade: Some(RetailFade {
                to_white: true,
                age: 31,
            }),
            module_phase: Some(2),
            cam_accum: None,
            walk_yaw: None,
            done_hold: None,
            module_countdown: None,
            accum_align: None,
            camera_pass: false,
            stream_wait: false,
            sideband_wait: true,
        },
        PhaseGate {
            action_state: 0x51,
            fade: None,
            module_phase: None,
            cam_accum: None,
            walk_yaw: None,
            done_hold: Some(104),
            module_countdown: None,
            accum_align: None,
            camera_pass: false,
            stream_wait: false,
            sideband_wait: false,
        },
        PhaseGate {
            action_state: 0x32,
            fade: None,
            module_phase: None,
            cam_accum: None,
            walk_yaw: None,
            done_hold: None,
            module_countdown: None,
            accum_align: None,
            camera_pass: true,
            stream_wait: false,
            sideband_wait: false,
        },
    ] {
        assert_eq!(PhaseGate::from_env(&g.to_env()), Some(g));
    }
}

/// A capture inside a module's stream poll takes the engine's frame on that
/// arm whatever the flash's age: the engine's arm passes on its first tick.
#[test]
fn a_stream_wait_gate_is_met_on_the_arm_regardless_of_the_flash_age() {
    let g = PhaseGate::from_env("53,black,20,m2,s").expect("parses");
    assert!(g.stream_wait);
    assert_eq!(g.module_phase, Some(2));
    assert_eq!(g.fade.map(|f| f.age), Some(20));
}

/// A capture inside a side-band wait keeps the flash's age as its clock:
/// the replay holds the arm instead of taking its first tick.
#[test]
fn a_sideband_wait_gate_keeps_the_flash_clock() {
    let g = PhaseGate::from_env("53,white,31,m2,h").expect("parses");
    assert!(g.sideband_wait);
    assert!(!g.stream_wait);
    assert_eq!(g.module_phase, Some(2));
    assert_eq!(g.fade.map(|f| f.age), Some(31));
}
