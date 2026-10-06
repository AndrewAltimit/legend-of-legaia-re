//! The play window's **dance presentation sink** - the sprite-part projection
//! the dance block needs beyond the shared builders.
//!
//! The minigame effect-part pool used to live here as a host-side sink, which
//! made the fishing splash, the wander ripples and the catch bursts a
//! native-only presentation layer. It is
//! [`legaia_engine_core::minigame_fx::MinigameFxPool`] on
//! [`legaia_engine_core::world::MinigameState`] now, aged by the world tick
//! and drawn through `legaia_engine_ui::minigame_fx`, so every host draws the
//! same parts. What is left here is the sprite-part projection; the dance HUD's
//! quads draw as screen prims (`ui_dance::dance_hud_prims`).

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
