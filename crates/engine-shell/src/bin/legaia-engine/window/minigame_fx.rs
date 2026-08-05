//! The play window's **minigame effect-part pool** - the host sink for the
//! minigame overlays' part spawns (retail's shared part-spawn API
//! `FUN_80021B04` over the effect pool).
//!
//! The dance sequence-clear banner ([`legaia_engine_core::dance::good_banner_spawn`]),
//! the fishing strike splash ([`legaia_engine_core::fishing_chrome::splash_burst`]),
//! the wander-retarget ripple ([`legaia_engine_core::fishing_chrome::ripple_spawn`])
//! and the catch-celebration bursts
//! ([`legaia_engine_core::fishing_actors::celebration_bursts`]) all spawn into
//! this pool, which ages each part and hands the HUD a draw list.
//!
//! The overlays' own sprite pages are not uploaded, so each part draws as a
//! placeholder font glyph at its position - the spawn arithmetic (offsets,
//! spreads, tier gates) is the ported kernels'; the glyph and the
//! world-to-stage `>> 3` mapping of the world-space spawns are host
//! presentation glue (the same shift the dancer emit dispatch applies to its
//! `+0x14/+0x16` pair).

use super::*;

/// One live part in the pool.
pub(super) struct FxPart {
    /// Stage-pixel position (320x240 space).
    pub x: i32,
    pub y: i32,
    /// Placeholder glyph the part draws as.
    pub glyph: &'static str,
    /// Frames lived so far.
    pub age: u32,
    /// Frames until retirement.
    pub ttl: u32,
}

/// The pool itself: spawn, age, draw.
#[derive(Default)]
pub(super) struct MinigameFxPool {
    parts: Vec<FxPart>,
}

/// Default part lifetime, in frames.
const FX_TTL: u32 = 45;

impl MinigameFxPool {
    fn push(&mut self, x: i32, y: i32, glyph: &'static str) {
        // Bound the pool the way the retail effect pool is bounded: drop the
        // oldest when full.
        if self.parts.len() >= 32 {
            self.parts.remove(0);
        }
        self.parts.push(FxPart {
            x,
            y,
            glyph,
            age: 0,
            ttl: FX_TTL,
        });
    }

    /// Spawn the dance sequence-clear banner + its two flanking stars. The
    /// spec positions are actor-space (`+0x14/+0x16`); `>> 3` maps them to
    /// the stage.
    pub(super) fn spawn_good_banner(&mut self, b: &legaia_engine_core::dance::GoodBannerSpawns) {
        self.push((b.banner.x >> 3) as i32, (b.banner.y >> 3) as i32, "GOOD!");
        for s in &b.stars {
            self.push((s.x >> 3) as i32, (s.y >> 3) as i32, "*");
        }
    }

    /// Spawn the three-part fishing strike splash at its fanned-out offsets.
    pub(super) fn spawn_splash(
        &mut self,
        parts: &[legaia_engine_core::fishing_chrome::SplashPart],
    ) {
        for p in parts {
            self.push(
                p.x as i32 + (p.nudge.0 >> 3),
                p.y as i32 + (p.nudge.1 >> 3),
                "~",
            );
        }
    }

    /// Spawn a wander-retarget ripple at the actor's world XZ pair.
    pub(super) fn spawn_ripple(&mut self, r: &legaia_engine_core::fishing_chrome::RippleSpawn) {
        self.push((r.pos[0] >> 3) as i32, (r.pos[2] >> 3) as i32, "o");
    }

    /// Spawn one catch-celebration burst at its offset from the catch point.
    pub(super) fn spawn_burst(
        &mut self,
        b: &legaia_engine_core::fishing_actors::CelebrationBurst,
        origin: (i16, i16),
    ) {
        self.push(
            (origin.0 as i32 + b.offset.0 as i32) >> 3,
            (origin.1 as i32 + b.offset.2 as i32) >> 3,
            "*",
        );
    }

    /// Age every part one frame and retire the expired ones.
    pub(super) fn tick(&mut self) {
        for p in self.parts.iter_mut() {
            p.age += 1;
        }
        self.parts.retain(|p| p.age < p.ttl);
    }

    /// This frame's placeholder draws, in stage pixels (the caller applies
    /// the stage transform).
    pub(super) fn stage_draws(&self, font: &Font) -> Vec<TextDraw> {
        let mut out = Vec::new();
        for p in &self.parts {
            let fade = 1.0 - p.age as f32 / p.ttl as f32;
            let color = [1.0, 1.0, 0.8, fade];
            let layout = font.layout_ascii(p.glyph);
            out.extend(text_draws_for(&layout, (p.x, p.y), color));
        }
        out
    }
}

/// Materialise the dance run's **sprite-part** emits as stage draws.
///
/// The geometry is the port's: [`legaia_engine_core::dance::sprite_part_emit`]
/// (`FUN_801d387c`) resolves each live part's arm off its own actor record, and
/// [`legaia_engine_core::dance::sprite_part_fade_weight`] its alpha. The
/// shadowed arm is two emits - the `0x400` copy then the `0x800` one - so both
/// are drawn, the second dimmer, which is what makes the pair read as a sprite
/// over its own shadow. Without the dance overlay's 4bpp page resident there is
/// no texel source, so each emit draws as a placeholder glyph keyed on the
/// part's `+0x50` sprite id (the same degradation `MinigameFxPool` applies).
pub(super) fn dance_sprite_part_draws(
    frames: &[legaia_engine_core::dance::SpritePartFrame],
    font: &Font,
    stage_origin: (i32, i32),
    stage_scale: u32,
) -> Vec<TextDraw> {
    use legaia_engine_core::dance::SpritePartEmit;
    let mut out: Vec<TextDraw> = Vec::new();
    for f in frames {
        // The overlay's sprite ids: `0xb` is the "Good!" banner, `0x16` a star.
        let glyph = match f.sprite {
            0xb => "GOOD!",
            0x16 => "*",
            _ => "+",
        };
        let alpha = f.fade as f32 / 255.0;
        let mut emit_at = |x: i16, y: i16, dim: f32| {
            let layout = font.layout_ascii(glyph);
            out.extend(text_draws_for(
                &layout,
                (x as i32, y as i32),
                [1.0, 1.0, 0.8, (alpha * dim).clamp(0.0, 1.0)],
            ));
        };
        match f.emit {
            SpritePartEmit::Shadowed { x, y, .. } => {
                emit_at(x, y, 1.0);
                // The second (`0x800`) copy is the shadow: same cell, offset a
                // pixel and drawn at half weight.
                emit_at(x + 1, y + 1, 0.5);
            }
            SpritePartEmit::Plain { x, y, .. } | SpritePartEmit::Marker { x, y, .. } => {
                emit_at(x, y, 1.0)
            }
            SpritePartEmit::CopyTemplate | SpritePartEmit::SetTemplateZ | SpritePartEmit::None => {}
        }
    }
    legaia_engine_render::scale_stage_text_draws(&mut out, stage_origin, stage_scale);
    out
}

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
