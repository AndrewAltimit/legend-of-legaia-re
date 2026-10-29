//! The room screen and the roaming panel off a [`NetPlay`], as
//! `legaia_engine_ui` draw lists both hosts append to their frame: the
//! prims to the screen-prim pass under the text layer, the labels (in
//! stage pixels) to the text layer.

use legaia_engine_core::world::World;
use legaia_engine_ui::TextDraw;
use legaia_engine_ui::expansion_modes::netplay::{
    NetPanel, NetScreen, net_panel_prims, net_panel_strings, net_panel_texts, net_screen_prims,
    net_screen_strings, net_screen_texts,
};
use legaia_render_kernels::screen_prim::ScreenPrim;

use crate::room::NetPlay;

/// Chat lines the roaming panel shows.
const PANEL_CHAT: usize = 3;

/// What the controller draws this frame.
enum Overlay {
    None,
    Screen,
    Panel,
}

fn overlay(net: &NetPlay) -> Overlay {
    if net.in_mode() {
        Overlay::None
    } else if net.screen_open || net.draft.is_some() || net.closed {
        Overlay::Screen
    } else {
        Overlay::Panel
    }
}

/// Run `f` over the frame's screen or panel.
fn with_overlay<R>(
    net: &NetPlay,
    world: &World,
    screen: impl FnOnce(&NetScreen<'_>) -> R,
    panel: impl FnOnce(&NetPanel<'_>) -> R,
    none: R,
) -> R {
    let title = net.title();
    let players = net.player_rows();
    match overlay(net) {
        Overlay::None => none,
        Overlay::Screen => {
            let rows = net.menu_rows(world);
            screen(&NetScreen {
                title: &title,
                address: net.address.as_deref(),
                players: &players,
                rows: &rows,
                cursor: net.cursor(),
                chat: &net.chat,
                draft: net.draft.as_deref(),
                notice: net.notice.as_deref(),
            })
        }
        Overlay::Panel => {
            let start = net.chat.len().saturating_sub(PANEL_CHAT);
            panel(&NetPanel {
                title: &title,
                players: &players,
                chat: &net.chat[start..],
                notice: net.notice.as_deref(),
            })
        }
    }
}

/// Whether the room screen owns the frame (the host draws nothing of the
/// field's HUD over it).
pub fn screen_owns_frame(net: &NetPlay) -> bool {
    matches!(overlay(net), Overlay::Screen)
}

/// The overlay's flat prims, in stage pixels.
pub fn net_overlay_prims(net: &NetPlay, world: &World) -> Vec<ScreenPrim> {
    with_overlay(net, world, net_screen_prims, net_panel_prims, Vec::new())
}

/// The overlay's labels, in stage pixels.
pub fn net_overlay_stage_texts(
    net: &NetPlay,
    world: &World,
    font: &legaia_font::Font,
) -> Vec<TextDraw> {
    with_overlay(
        net,
        world,
        |s| net_screen_texts(font, s),
        |p| net_panel_texts(font, p),
        Vec::new(),
    )
}

/// The overlay's labels as strings (tests, a text readout).
pub fn net_overlay_strings(net: &NetPlay, world: &World) -> Vec<String> {
    with_overlay(
        net,
        world,
        net_screen_strings,
        net_panel_strings,
        Vec::new(),
    )
}
