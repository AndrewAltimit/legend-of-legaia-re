//! Disc-gated: opening the pause menu leaves the field camera where it was.
//!
//! The pause menu freezes the field under its wipe: retail swaps the field
//! overlay out for the menu's (PROT 0899 over 0897), so nothing writes the
//! camera's focus globals until it closes. At `suimon`'s cold seat the
//! focus stands on the scene's edge clamp (`FUN_801DAA50`), some 770 units
//! off the player. The port's follow camera stopped treating a menu frame as
//! a zone-camera frame and pinned the focus on the raw player, so the frozen
//! field jumped sideways - past the map's edge, into the void - for the
//! frames the wipe shows before the menu covers it.
//!
//! Skips + passes when `LEGAIA_DISC_BIN` is unset.

#![cfg(not(target_arch = "wasm32"))]

use legaia_web_play::runtime::LegaiaRuntime;

/// The pad word's Start bit.
const START: u16 = 0x0008;

#[test]
fn menu_wipe_keeps_the_field_camera_or_skip() {
    let Ok(disc) = std::env::var("LEGAIA_DISC_BIN") else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    let mut rt = LegaiaRuntime::new();
    rt.load_disc(std::fs::read(&disc).expect("read disc"), String::new())
        .expect("load disc");
    rt.enter_field("suimon").expect("enter suimon");
    for _ in 0..40 {
        rt.play_frame_ticks(1, 0, 0).expect("tick");
    }
    let before = rt.play_camera_vp(640.0, 480.0);
    assert_eq!(before.len(), 16, "the field frame has an engine camera");
    rt.play_frame_ticks(1, START, START).expect("Start");
    assert!(rt.play_menu_is_open(), "Start opens the pause menu");
    for f in 0..30 {
        let vp = rt.play_camera_vp(640.0, 480.0);
        assert_eq!(
            vp, before,
            "menu frame {f}: the frozen field's camera moved"
        );
        rt.play_frame_ticks(1, 0, 0).expect("tick");
    }
}
