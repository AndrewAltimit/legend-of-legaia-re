//! Minimal wgpu renderer for the Phase 1 asset viewer.
//!
//! REF: FUN_80031D00, FUN_80035CB8, FUN_80035DA0, FUN_80035E44
//! The overlay pipelines draw what the retail text-actor draw pump
//! (`FUN_80031D00` and its three callees) puts on screen, from the hosts' own
//! draw lists; the pump's dispatch and its callees' rules are ported in
//! `legaia_engine_menus::menu_list_rows` (`text_actor_arm` and siblings).
//!
//! REF: FUN_8002C488, FUN_8003C310 (their cells are cut by
//! `legaia_engine_menus::save_menu_atlas::{menu_pictogram, command_arrow_page_rect}`)
//!
//! REF: FUN_80034b78, FUN_80034e4c, FUN_8002C69C, FUN_8002B994, FUN_800337B0,
//! FUN_800349EC, FUN_80035EA8, FUN_8003C1F8 - each ported by a kernel in
//! `engine-ui`, `engine-menus` or `engine-dialog`, whose `PORT:` tag sits
//! there; the renderer draws what those kernels emit (for `FUN_8003C1F8`, the
//! per-glyph dialog-font sprite, through the legaia-font atlas and a
//! textured-quad overlay instead of the retail GP0 cell-UV push).
//!
//! Owns a wgpu device + surface, plus two render pipelines:
//!
//! * **Textured-quad** (Phase 1 TIM viewer) - `upload_texture` +
//!   `render(RenderTarget::Texture(...))`. Letterbox-preserves aspect ratio.
//! * **Flat-shaded mesh** (Phase 1 TMD viewer) - `upload_mesh` +
//!   `render(RenderTarget::Mesh { ... })`. Lit by a single directional
//!   light, depth-tested. Uses the `glam::Mat4` MVP supplied per-frame so
//!   the host can spin the model without re-uploading.
//!
//! Both pipelines share the same surface + depth attachment. PSX-faithful
//! rasterisation (affine UV warp, sub-pixel vertex jitter, 15-bit ordered
//! dithering) is opt-in via [`Renderer::set_psx_mode`]; GTE emulation and
//! batched draws are future phases.
//! REF: FUN_801D0148, FUN_801D5DE0, FUN_801D84D0, FUN_801E08D8, FUN_801E1C1C, FUN_801E36C4
//! REF: FUN_801E3EE0, FUN_801E3FF0

/// A flag on a per-draw NCLIP word ([`Renderer::set_draw_nclip`]): in PSX mode
/// the draw's prims take the depth of their corners' mean, the key retail links
/// a battle shell prim by, so a large quad sorts as one piece.
pub const DRAW_WORD_MEAN_DEPTH: u32 = 0x100;

/// [`Renderer::set_draw_order`]: this draw's primitives sort at their
/// farthest corner rather than their mean (colour word bit `0x10000000`,
/// an object record's flag `0x800`).
pub const DRAW_ORDER_FAR_CORNER: u8 = 1;
/// [`Renderer::set_draw_order`]: this draw links ten ordering-table buckets
/// nearer (`actor[+0x10] & 0x80800000`).
pub const DRAW_ORDER_NEAR_BUCKETS: u8 = 2;
/// [`Renderer::set_draw_order`]: the ten-bucket shift of
/// [`DRAW_ORDER_NEAR_BUCKETS`] holds in a clean frame too, taken per vertex
/// (each corner tests depth as if it stood ten buckets nearer) rather than
/// at the primitive's link. Set on every field actor carrying the
/// near-bucket bit - the player on a cell without object-grid bit `0x1000`,
/// an NPC the floor sampler or a prologue gave it: the actors retail stands
/// on a placed object's floor or seats in its furniture, and paints over
/// the mesh around them. Per-pixel depth alone buries them in it: King
/// Drake in his throne (`dolk2` `P2[7]`), Vahn against the cliff behind
/// `rikuroa2`'s chest ledge.
pub const DRAW_ORDER_NEAR_CLEAN: u8 = 4;
/// The `flags.w` word that stages [`DRAW_ORDER_NEAR_CLEAN`] on a clean field
/// frame, below every PSX-mode order word (`-1 - order`, `-1..=-8`).
pub(crate) const CLEAN_NEAR_FLAG_W: f32 = -16.0;

pub mod battle_intro;
pub mod gte_trace;
pub mod window;

// The pure, wgpu-free UI draw-list layer lives in `legaia-engine-ui`; its
// items are named as `legaia_engine_ui::<item>`.
use legaia_engine_ui::TextDraw;

pub mod dyn_light;
pub mod occlusion_fade;
pub mod profile;
pub mod psx_blend;
pub mod psx_dither;
pub mod psx_light;
mod renderer;
pub mod scene_lights;
pub mod screen_overlay;
mod shaders;

pub use renderer::*;

/// Batch of [`TextDraw`]s to render in one pass against a shared font atlas.
/// Cheap to construct each frame; the renderer copies the geometry into a
/// reusable dynamic buffer before drawing.
pub struct TextOverlay<'a> {
    pub atlas: &'a UploadedFontAtlas,
    pub draws: &'a [TextDraw],
    /// Runs of [`Self::draws`] that are PSX semi-transparent packets. Every
    /// draw outside a span goes through the ordinary alpha-blend pipeline;
    /// a draw inside one goes through the span's ABR equation instead. Empty
    /// for an overlay with no semi-transparent packets.
    pub blend: &'a [OverlayBlendSpan],
}

/// A run of an overlay's draws blended with one PSX semi-transparency
/// equation (ABR, tpage bits 5..=6) instead of alpha: `0` = `B/2 + F/2`,
/// `1` = `B + F`, `2` = `B - F`, `3` = `B + F/4`, where `F` is the tinted
/// texel and `B` the framebuffer ([`psx_blend::blend_state`]). A texel with
/// atlas alpha `0` is PSX colour `0x0000` and is not drawn; the tint's alpha
/// is ignored.
///
/// Spans are ordered, do not overlap, and index [`TextOverlay::draws`];
/// draw order is unchanged - the pipeline switches at each span edge, so a
/// semi packet between two opaque ones stays between them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OverlayBlendSpan {
    /// Index of the span's first draw.
    pub start: u32,
    /// Number of draws in the span.
    pub count: u32,
    /// The ABR equation (`0..=3`).
    pub abr: u8,
}

impl OverlayBlendSpan {
    /// Split `count` draws starting at `base` into `(start, count, abr)`
    /// segments, one per maximal run of a single blend class - `None` for
    /// the alpha pipeline, `Some(abr)` for a span. Spans past `base + count`
    /// are clipped; a malformed (overlapping) span list keeps its first
    /// claim on each draw.
    pub fn segments(spans: &[Self], count: u32) -> Vec<(u32, u32, Option<u8>)> {
        let mut out = Vec::new();
        let mut at = 0u32;
        for sp in spans {
            let s = sp.start.max(at).min(count);
            let e = sp.start.saturating_add(sp.count).min(count);
            if s > at {
                out.push((at, s - at, None));
            }
            if e > s {
                out.push((s, e - s, Some(sp.abr & 3)));
                at = e;
            } else {
                at = at.max(s);
            }
        }
        if count > at {
            out.push((at, count - at, None));
        }
        out
    }
}

/// GPU-resident aliases of the moved [`SpriteDraw`](legaia_engine_ui::SpriteDraw) / [`TextOverlay`] shapes.
/// [`SpriteDraw`](legaia_engine_ui::SpriteDraw) itself lives in `legaia-engine-ui`; these two hold a wgpu
/// [`UploadedFontAtlas`] handle so they stay in this crate.
pub type UploadedSpriteAtlas = UploadedFontAtlas;
pub type SpriteOverlay<'a> = TextOverlay<'a>;

#[cfg(test)]
mod tests;
