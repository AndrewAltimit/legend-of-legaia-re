//! The post-action **banners**: the level-up summary and the Seru-capture
//! line, both ticked down by `World::tick`.

use crate::{CAPTURE_PEN, LEVEL_UP_PEN, ScreenInputs};
use legaia_engine_core::world::SceneMode;
use legaia_engine_ui::{self as ui, TextDraw};

/// The banner rows for one frame, in **stage** pixels.
///
/// With the system-UI chrome loaded the message rides retail's framed
/// top-of-screen banner (the widget the `noa_levelup_banner` capture
/// pinned), read through `engine-core::battle_hud::battle_banner_message`.
/// The two framed paths are mutually exclusive by mode: in battle
/// `battle_hud_draws_for` already emitted the banner (and yielded the
/// plaque's seat to it), so this returns nothing; outside battle the rows
/// are emitted here, because the port raises both messages a mode-tick after
/// the fight has handed the frame back to the field.
///
/// Without the chrome there is no frame to put a message in, so the loose
/// pens ([`LEVEL_UP_PEN`], [`CAPTURE_PEN`]) draw instead.
pub fn banner_stage_draws(inputs: &ScreenInputs<'_>) -> Vec<TextDraw> {
    let world = inputs.world;
    let font = inputs.font;
    let mut out = Vec::new();
    if inputs.chrome.is_some()
        && let Some(message) = legaia_engine_core::battle_hud::battle_banner_message(world)
    {
        if world.mode != SceneMode::Battle {
            out.extend(ui::battle_hud_chrome::message_banner_text_draws_for(
                font, &message,
            ));
        }
        return out;
    }
    if let Some(b) = world.party.current_level_up_banner.as_ref() {
        out.extend(ui::level_up_draws_for(
            font,
            b.char_id,
            b.new_level,
            b.hp_gained,
            b.mp_gained,
            LEVEL_UP_PEN,
        ));
    }
    if let Some(b) = world.party.current_capture_banner.as_ref()
        && let Some(text) = b.current_banner()
    {
        out.extend(ui::capture_banner_draws_for(font, &text, CAPTURE_PEN));
    }
    out
}
