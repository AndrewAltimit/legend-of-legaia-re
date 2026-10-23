//! Page ladder for window 31, the **Point Card** toast (`FUN_801DCE20`,
//! `engine-ui::amount_prompt_draws_for`) - a `docs/tooling/reach-triage.md`
//! (b) row whose gate is one item in the bag.
//!
//! Retail's buy commits hand the widget VM `[open 0x1F]` right after crediting
//! the card (`0x801E4EDC` from the quantity commit, `0x801E4EA8` from the
//! recipient picker), and the credit is gated on **holding** the card, item
//! `0xFE` (`FUN_80042F4C(0xFE) != 0`). A cold-start bag has no card, so every
//! shop rung in the union buys without ever painting the window. This ladder
//! seeds the one piece of state the gate is - the card, through the page's own
//! item cheat - and walks the equipment shop's buy list to a recipient-picker
//! commit by pad, the route `menu_depth_ladder`'s first rung drives.
//!
//! Scored against the same walk without the card: the commit must arm the
//! runtime's toast beat, and the composed overlay frame must differ from the
//! no-card frame at the same step - a painter that runs and emits nothing would
//! leave the two equal.
//!
//! Coverage export: the recipe in `scripts/ci/replay-port-coverage.py`.
//! Skips + passes when `LEGAIA_DISC_BIN` is unset.

#![cfg(not(target_arch = "wasm32"))]

use legaia_engine_system::input::PadButton;
use legaia_web_play::runtime::LegaiaRuntime;

const W: u32 = 320;
const H: u32 = 240;

fn settle_shop(rt: &mut LegaiaRuntime) {
    let frames = legaia_engine_menu_runtime::menu_runtime::SHOP_FADE_FRAMES as usize
        + usize::from(legaia_engine_menus::shop::SHOP_SLIDE_FRAMES)
        + 2;
    for _ in 0..frames {
        rt.play_shop_input(0);
    }
}

/// Walk the equipment shop to a recipient-picker commit on the bag row and
/// return `(toast armed, composed overlay frame)` on the frame after it.
fn buy_to_bag(rt: &mut LegaiaRuntime) -> Result<(Option<i32>, String), String> {
    if !rt.debug_open_equipment_shop() {
        return Err("equipment shop did not open".into());
    }
    settle_shop(rt);
    // Top picker row 0 = Buy.
    rt.play_shop_input(PadButton::Cross.mask());
    settle_shop(rt);
    let mut opened = false;
    for _ in 0..4u32 {
        rt.play_shop_input(PadButton::Cross.mask());
        if rt.debug_recipient_picker_open() {
            opened = true;
            break;
        }
        rt.play_shop_input(PadButton::Circle.mask());
        rt.play_shop_input(PadButton::Down.mask());
    }
    if !opened {
        return Err("no buy-list row took the recipient-picker route".into());
    }
    settle_shop(rt);
    // The picker opens on row 0, the bag row: a commit that buys into the
    // bag, open to every item.
    rt.play_shop_input(PadButton::Cross.mask());
    rt.play_shop_input(0);
    let toast = rt.debug_menu_runtime().point_card_toast();
    let frame = rt.play_overlay_draws_json(W, H);
    Ok((toast, frame))
}

fn fresh(bytes: &[u8]) -> LegaiaRuntime {
    let mut rt = LegaiaRuntime::new();
    rt.load_disc(bytes.to_vec(), String::new())
        .expect("load disc");
    rt.enter_field("town01").expect("enter town01");
    for _ in 0..8 {
        rt.tick_frame().expect("tick");
    }
    rt
}

#[test]
fn point_card_toast_paints_on_a_buy_while_the_card_is_held() {
    let Some(disc) = std::env::var_os("LEGAIA_DISC_BIN") else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    let bytes = std::fs::read(&disc).expect("read disc");

    // Control: the same walk with a cold bag - no card, no window.
    let mut control = fresh(&bytes);
    let (toast_cold, frame_cold) = buy_to_bag(&mut control).expect("control walk");
    assert_eq!(toast_cold, None, "a cold bag holds no Point Card");

    let mut rt = fresh(&bytes);
    let granted = rt.cheat_give_item("0xfe", 1);
    eprintln!("[seed] {granted}");
    let (toast, frame) = buy_to_bag(&mut rt).expect("card walk");
    let credit = toast.expect("the buy commit armed no Point Card toast while the card was held");
    assert!(credit >= 0, "credit {credit}");
    assert_ne!(
        frame, frame_cold,
        "the toast is up and the overlay composed the same frame as the no-card walk - \
         window 31 painted nothing"
    );
    eprintln!("[ok] Point Card toast armed (credit {credit}) and window 31 composed");
}
