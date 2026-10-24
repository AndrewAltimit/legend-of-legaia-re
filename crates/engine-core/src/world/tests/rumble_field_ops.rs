//! Op `0x4C` nibble-4 subs `6` / `7` reach the rumble motor words, and the
//! options screen's Events vibration row gates them: 215 such writes sit in
//! 40 shipped scenes (`asset field-op-census --only '4C 46'`).

use super::*;
use legaia_engine_mode::mode::SceneMode;

/// `4C 47 C0 00 00 00` (large motor to `0xC0` now), `4C 46 FF 00 00 00`
/// (pulse word to `0xFF` now), then a park.
fn world(events_off: bool) -> World {
    let mut w = World::new();
    w.mode = SceneMode::Field;
    w.install_field_player(0);
    w.toggles.retail_options.vibration_events = u8::from(events_off);
    w.field_bytecode = vec![
        0x4C, 0x47, 0xC0, 0x00, 0x00, 0x00, 0x4C, 0x46, 0xFF, 0x00, 0x00, 0x00, 0x21,
    ];
    w.field_pc = 0;
    w
}

#[test]
fn a_field_script_rumbles_the_controller() {
    let mut w = world(false);
    let _ = w.step_field();
    let _ = w.step_field();
    assert_eq!(w.rumble.motors.level, 0xC0);
    assert_eq!(w.rumble.motors.pulse, 0xFF);
    for _ in 0..8 {
        w.tick_rumble();
    }
    let f = w.rumble_frame();
    assert!(f.strong > 0.7 && f.weak > 0.5, "{f:?}");
}

#[test]
fn events_vibration_off_holds_the_motors_still() {
    let mut w = world(true);
    w.rumble.motors.level = 0x80;
    let _ = w.step_field();
    let _ = w.step_field();
    assert_eq!(w.rumble.motors, Default::default());
    w.tick_rumble();
    assert!(!w.rumble_frame().is_active());
}
