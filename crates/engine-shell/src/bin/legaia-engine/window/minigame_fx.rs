//! The play window's **dance presentation sinks** - the two draw materialisers
//! the dance block needs beyond the shared builders.
//!
//! The minigame effect-part pool used to live here as a host-side sink, which
//! made the fishing splash, the wander ripples and the catch bursts a
//! native-only presentation layer. It is
//! [`legaia_engine_core::minigame_fx::MinigameFxPool`] on
//! [`legaia_engine_core::world::MinigameState`] now, aged by the world tick
//! and drawn through `legaia_engine_ui::minigame_fx`, so every host draws the
//! same parts. What is left here is the quad half of the dance HUD, which
//! needs the window's own atlas source.

use super::*;

/// Materialise a dance HUD quad list as flat draws against a solid atlas
/// source. The quads' geometry, gouraud colours and patched glyph `uv` are
/// live every frame ([`legaia_engine_core::dance::DanceGame::hud_draw_quads`]);
/// without the dance overlay's 4bpp page resident there is no texel source,
/// so `solid_src == None` (the play window today) materialises nothing - the
/// same degradation the fishing gauge fills take through
/// `FishingHudAtlas::solid_src`.
pub(super) fn dance_quad_draws(
    quads: &[legaia_engine_core::dance::DanceHudQuad],
    solid_src: Option<(u32, u32, u32, u32)>,
    stage_origin: (i32, i32),
    stage_scale: u32,
) -> Vec<TextDraw> {
    let Some(src) = solid_src else {
        return Vec::new();
    };
    let s = stage_scale.max(1) as i32;
    quads
        .iter()
        .map(|q| TextDraw {
            dst: (
                stage_origin.0 + q.x0 as i32 * s,
                stage_origin.1 + q.y0 as i32 * s,
                ((q.x1 - q.x0).max(0) as u32) * s as u32,
                ((q.y1 - q.y0).max(0) as u32) * s as u32,
            ),
            src,
            color: [
                q.rgb_top[0] as f32 / 255.0,
                q.rgb_top[1] as f32 / 255.0,
                q.rgb_top[2] as f32 / 255.0,
                1.0,
            ],
        })
        .collect()
}

/// Project the dance run's own sprite-part emits onto the shared view the
/// engine-ui builder takes.
///
/// The geometry is the port's:
/// [`legaia_engine_core::dance::sprite_part_emit`] (`FUN_801d387c`) resolves
/// each live part's arm off its own actor record, and
/// [`legaia_engine_core::dance::sprite_part_fade_weight`] its alpha. Only the
/// `Shadowed` arm draws the cell twice; the template / marker arms carry no
/// screen pair for a host to draw.
pub(super) fn dance_sprite_part_views(
    frames: &[legaia_engine_core::dance::SpritePartFrame],
) -> Vec<legaia_engine_render::minigame_fx::DanceSpritePartView> {
    use legaia_engine_core::dance::SpritePartEmit;
    use legaia_engine_render::minigame_fx::DanceSpritePartView;
    frames
        .iter()
        .filter_map(|f| {
            let (x, y, shadow) = match f.emit {
                SpritePartEmit::Shadowed { x, y, .. } => (x, y, true),
                SpritePartEmit::Plain { x, y, .. } | SpritePartEmit::Marker { x, y, .. } => {
                    (x, y, false)
                }
                SpritePartEmit::CopyTemplate
                | SpritePartEmit::SetTemplateZ
                | SpritePartEmit::None => return None,
            };
            Some(DanceSpritePartView {
                x: x as i32,
                y: y as i32,
                sprite: f.sprite,
                fade: f.fade,
                shadow,
            })
        })
        .collect()
}

/// The effect pool's parts, as the shared builder's view.
pub(super) fn fx_part_views(
    pool: &legaia_engine_core::minigame_fx::MinigameFxPool,
) -> Vec<legaia_engine_render::minigame_fx::FxPartView> {
    pool.frames()
        .into_iter()
        .map(|p| legaia_engine_render::minigame_fx::FxPartView {
            x: p.x as i32,
            y: p.y as i32,
            sprite: p.sprite,
            fade: p.fade,
        })
        .collect()
}
