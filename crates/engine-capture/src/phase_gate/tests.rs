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
        },
    ] {
        assert_eq!(PhaseGate::from_env(&g.to_env()), Some(g));
    }
}
