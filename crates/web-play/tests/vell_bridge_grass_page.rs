//! Disc-gated: the play page grows `vell`'s Bridge Grass, as the native
//! window does.
//!
//! The grass (`P0[1]`, the object on tile `(16, 37)`) sways as the sprout
//! (model `41`) until the Fertilizer is used; the Yes branch swaps the object
//! to the bridge (`4C 50 28 00`, model `40`) and arms the growth lanes
//! `0..=6` (`4B 07 00 ..`), which `FUN_8001C604` stages into the drawn mesh.
//! The page draws a placed object from the live model table
//! (`field_placement_models`) and re-stages a morphing slot's positions
//! (`field_morph_slots` / `field_morph_positions`). With neither op reaching
//! the object, the page kept drawing the sprout and the bridge never formed.
//!
//! Skips + passes when `LEGAIA_DISC_BIN` is unset.

#![cfg(not(target_arch = "wasm32"))]

use legaia_engine_system::input::PadButton;
use legaia_web_play::runtime::LegaiaRuntime;

const GRASS: (u8, u8) = (16, 37);
const BRIDGE: u32 = 40;

#[test]
fn the_page_swaps_in_the_bridge_and_grows_it() {
    let Some(disc) = std::env::var_os("LEGAIA_DISC_BIN") else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    let mut rt = LegaiaRuntime::new();
    rt.load_disc(std::fs::read(disc).expect("read disc"), String::new())
        .expect("load disc");
    for f in [0x460, 0x078] {
        assert!(rt.set_system_flag(f, true));
    }
    rt.enter_field("vell").expect("enter vell");
    for _ in 0..60 {
        rt.set_pad(0);
        rt.tick_frame().expect("tick");
    }
    assert!(rt.debug_seat_player(17 * 128 + 64, 38 * 128 + 64));
    for _ in 0..10 {
        rt.set_pad(0);
        rt.tick_frame().expect("tick");
    }
    let models = rt.field_placement_models();
    assert!(
        models.contains(&41) && !models.contains(&(BRIDGE as i32)),
        "the sprout is drawn before the Fertilizer: {models:?}"
    );
    assert!(
        rt.debug_session_mut()
            .expect("session")
            .host
            .world
            .start_prop_interaction(GRASS),
        "the grass's record starts"
    );
    let mut reach_max = 0.0f32;
    let mut staged = false;
    for t in 0..3000u32 {
        rt.set_pad(if t % 2 == 0 {
            PadButton::Cross.mask()
        } else {
            0
        });
        rt.tick_frame().expect("tick");
        for slot in rt.field_morph_slots() {
            if slot != BRIDGE {
                continue;
            }
            let reach = rt.field_morph_reach(slot);
            reach_max = reach_max.max(reach);
            staged |= reach > 0.0 && !rt.field_morph_positions(slot).is_empty();
        }
        let w = &rt.debug_session().expect("session").host.world;
        if w.dialog.inline.is_none() && w.system_flag_test(0x2AE) {
            break;
        }
    }
    eprintln!("bridge morph reach peaked at {reach_max}");
    let models = rt.field_placement_models();
    assert!(
        models.contains(&(BRIDGE as i32)),
        "the grown grass draws the bridge model: {models:?}"
    );
    assert!(
        staged,
        "the page re-staged the bridge mesh under the growth lanes"
    );
    assert!(reach_max > 100.0, "the growth moves the bridge visibly");
}
