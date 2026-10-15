//! The minigames' screen-space PSX primitives: the dance hall's HUD quads and
//! count-in banner and the Baka Fighter HUD strips, projected off the world's
//! minigame seat onto the shared `engine-ui` emitters. Both play hosts used to
//! carry a copy of each.

use legaia_engine_minigame_scenes::minigame_state::MinigameState;
use legaia_engine_ui::dance as ud;
use legaia_render_kernels::screen_prim::ScreenPrim;

/// The dance HUD's textured quads, off the seat's one predicate
/// (`MinigameState::dance_hud_quads`: HUD up and page resident). Empty
/// otherwise, when the hosts draw the text rows.
pub fn dance_hud_prims(mg: &MinigameState) -> Vec<ScreenPrim> {
    let views: Vec<ud::DanceHudQuadView> = mg
        .dance_hud_quads()
        .iter()
        .map(|q| ud::DanceHudQuadView {
            poly_code: q.poly_code,
            rect: (q.x0, q.y0, q.x1, q.y1),
            uv: q.uv,
            rgb_top: q.rgb_top,
            rgb_bottom: q.rgb_bottom,
            clut: q.clut,
            tpage: q.tpage_attr,
        })
        .collect();
    ud::dance_hud_prims(&views, ud::COUNTIN_OT)
}

/// The count-in banner (READY, then `GO!` - `FUN_801cf470` states 4 / 5).
/// Empty outside the count-in and while the hall's HUD page is not resident,
/// in which case the hosts draw the placeholder letterforms instead. The art
/// comes off the run's own widget table, so the cell, page and palette are
/// the disc's.
pub fn dance_countin_prims(mg: &MinigameState) -> Vec<ScreenPrim> {
    if !mg.dance_hud_art_staged {
        return Vec::new();
    }
    if let Some(go) = mg.dance_countin_go {
        return mg
            .dance
            .as_ref()
            .and_then(|g| g.widget(ud::COUNTIN_GO_WIDGET))
            .map(|(w, abr)| {
                ud::dance_go_prims(
                    go,
                    ud::DanceCountInArt::from_widget(&w, abr),
                    ud::COUNTIN_OT,
                )
            })
            .unwrap_or_default();
    }
    let Some(env) = mg.dance_countin_banner.as_ref() else {
        return Vec::new();
    };
    let art = mg
        .dance
        .as_ref()
        .and_then(|g| g.widget(0))
        .map(|(w, abr)| ud::DanceCountInArt::from_widget(&w, abr))
        .unwrap_or_default();
    ud::dance_countin_prims(
        ud::DanceCountInView {
            x_offset: env.x_offset,
            brightness: env.brightness,
            hold: env.hold,
        },
        art,
        ud::COUNTIN_OT,
    )
}

/// The Baka Fighter HUD's textured strips for the duel surface's widget quads
/// (`BakaDuelSurface::hud_quads`). The caller gates on its HUD art being
/// drawn and a fight being up.
pub fn baka_hud_prims(
    quads: &[legaia_engine_minigames::baka_fighter::HudWidgetQuad],
) -> Vec<ScreenPrim> {
    let views: Vec<ud::DanceHudQuadView> = quads
        .iter()
        .map(|q| ud::DanceHudQuadView {
            poly_code: q.poly_code,
            rect: (q.x0, q.y0, q.x1, q.y1),
            uv: q.uv,
            rgb_top: q.rgb_top,
            rgb_bottom: q.rgb_bottom,
            clut: q.clut,
            tpage: q.tpage_attr,
        })
        .collect();
    legaia_engine_ui::baka_strips::baka_hud_prims(&views)
}
