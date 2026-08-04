//! Screen-space 2D overlay pass: PSX `POLY_FT4` textured quads + flat quads
//! drawn in ordering-table order (back-to-front by OT index) with per-ABR
//! semi-transparency.
//!
//! REF: FUN_8003d2c4 (retail `AddPrim` - links a packet into the software
//! ordering table at a depth bucket) / `DrawOTag` (walks the OT back-to-front)
//!
//! # Where the model lives
//!
//! The primitive record, the ordering-table sort and the vertex builder are
//! **not** here: they are [`legaia_engine_ui::screen_prim`], the wgpu-free
//! layer both hosts link. This module is the native side's re-export of that
//! model plus the pieces that need `engine-render`'s own types - which today is
//! the afterimage wire and the constants pin below.
//!
//! Everything the old inline definitions offered is still reachable at this
//! path (`crate::screen_overlay::ScreenPrim`, `::build_geometry`, ...), so
//! call sites and tests read unchanged; what changed is that the browser play
//! page now reaches the same definitions instead of needing a second set.
//!
//! # The GPU side
//!
//! [`build_geometry`] converts a primitive list into a flat NDC vertex/index
//! buffer plus a list of [`DrawRun`]s (contiguous quads sharing a blend class).
//! [`crate::Renderer`] uploads that geometry once per frame and issues one draw
//! per run, selecting the opaque pipeline or the matching per-ABR blend
//! pipeline. Textured quads sample the shared PSX VRAM texture with the same
//! 4/8/15-bpp + CLUT decode the 3D VRAM-mesh shader uses.
//!
//! # Two things about how this reaches a frame
//!
//! **It composites over a scene.** [`crate::RenderTarget::ScreenOverlay`] is a
//! whole-frame mode - clear, then quads and nothing else - so on its own it can
//! never put a streak over a battle scene or a transition strip over a field
//! scene, which is the only thing any consumer wants.
//! [`crate::RenderTarget::SceneWithScreenPrims`] draws both in one frame.
//! Retail draws no such distinction: 3D primitives and screen-space packets go
//! into the *same* ordering table and one `DrawOTag` walks it.
//!
//! **The coordinate space is the PSX display, not the window.** The renderer
//! hands [`build_geometry`] the [`crate::vram_capture::PSX_SCREEN_WIDTH`] /
//! `PSX_SCREEN_HEIGHT` pair and the overlay stretches over the whole surface -
//! matching the `orthographic_rh(0, 320, 240, 0)` the native shell's
//! `screen_fx` meshes already use.
//!
//! # Wiring status
//!
//! The main consumer is [`crate::battle_intro`]: all five field-to-battle
//! transition styles emit through this pass, driven per frame by the native
//! play window. The browser play page draws a **subset** of the same list
//! through its own WebGL2 pass - see `docs/tooling/host-drift.md` for exactly
//! which primitives it can and cannot put on screen.

use crate::afterimage::AfterimageQuad;

pub use legaia_engine_ui::screen_prim::{
    BlendClass, DrawRun, FLAG_TEXTURED, FlatQuad, OverlayGeometry, PSX_DISPLAY_H, PSX_DISPLAY_W,
    SCREEN_VERTEX_OFF_CBA_TSB, SCREEN_VERTEX_OFF_COLOR, SCREEN_VERTEX_OFF_FLAGS,
    SCREEN_VERTEX_OFF_POS, SCREEN_VERTEX_OFF_UV, SCREEN_VERTEX_STRIDE, ScreenPrim, ScreenQuad,
    ScreenVertex, build_geometry, display_rect_flat_quad, fade_prim, order_primitives,
};

/// The display rect the shared model authors in is the same rect this crate
/// captures the framebuffer into. Pinned at compile time rather than by a
/// comment: the two constants live in different crates for a reason (the
/// wgpu-free half cannot see `vram_capture`), and a silent divergence would
/// show up only as a transition drawn at the wrong scale.
const _: () = assert!(PSX_DISPLAY_W as u16 == crate::vram_capture::PSX_SCREEN_WIDTH);
const _: () = assert!(PSX_DISPLAY_H as u16 == crate::vram_capture::PSX_SCREEN_HEIGHT);

/// Build a screen-space afterimage `POLY_FT4` from a projected+jittered
/// [`AfterimageQuad`] (see [`crate::afterimage::build_afterimage_quad`]) and
/// the OT bucket the retail caller links it at (the billboard projection's
/// returned `depth`; see [`crate::billboard::BillboardCorners::depth`]).
///
/// This is the wire that connects the (previously unwired) afterimage +
/// billboard ports to an actual draw path.
pub fn afterimage_screen_quad(q: &AfterimageQuad, ot_index: u32) -> ScreenQuad {
    ScreenQuad {
        xy: q.xy,
        uv: q.uv,
        clut: q.clut,
        tpage: q.tpage,
        color: q.color,
        gouraud: None,
        semi_transparent: q.semi_transparent,
        ot_index,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::afterimage::build_afterimage_quad;

    /// A deterministic zero rng (min jitter, base band) so the afterimage
    /// corner geometry is predictable in the ordering tests.
    fn zero_rng() -> impl FnMut() -> u32 {
        || 0
    }

    #[test]
    fn afterimage_wire_preserves_packet_fields() {
        let corners = [(100, 200), (110, 200), (100, 260), (110, 260)];
        let q = build_afterimage_quad(corners, 0x12, zero_rng());
        let sq = afterimage_screen_quad(&q, 250);
        assert_eq!(sq.xy, q.xy);
        assert_eq!(sq.uv, q.uv);
        assert_eq!(sq.clut, q.clut);
        assert_eq!(sq.tpage, q.tpage);
        assert_eq!(sq.color, q.color);
        assert!(sq.semi_transparent);
        assert_eq!(sq.ot_index, 250);
        // TSB 0x0027 -> ABR bits 5..6 = 1 (additive) - the trail streak mode.
        assert_eq!(sq.abr_mode(), 1);
    }

    #[test]
    fn an_afterimage_streak_coalesces_into_one_blended_run() {
        // Three additive quads at increasing depth, drawn farthest-first by
        // the shared ordering table. The native pass then binds one blend
        // pipeline for the whole streak.
        let streak: Vec<ScreenPrim> = (0..3)
            .map(|i| {
                let q =
                    build_afterimage_quad([(50, 60), (70, 60), (50, 90), (70, 90)], 0, zero_rng());
                let mut sq = afterimage_screen_quad(&q, 100 + i * 10);
                sq.ot_index = 100 + i * 10;
                ScreenPrim::Textured(sq)
            })
            .collect();
        let geo = build_geometry(&streak, 320, 240);
        assert_eq!(geo.runs.len(), 1);
        assert_eq!(geo.runs[0].class, BlendClass::Semi(1));
        assert_eq!(geo.runs[0].index_count, 18);
        // Farthest streak quad (ot 120) is emitted first: its first vertex is
        // the textured top-left corner in NDC. build_afterimage_quad applies
        // its zero-rng jitter (-2 x, -8 y) so corner (50,60) -> (48,52).
        let v0 = geo.vertices[0];
        assert_eq!(v0.flags, FLAG_TEXTURED);
        assert!((v0.pos[0] - (48.0 / 320.0 * 2.0 - 1.0)).abs() < 1e-6);
        assert!((v0.pos[1] - (1.0 - 52.0 / 240.0 * 2.0)).abs() < 1e-6);
    }
}
