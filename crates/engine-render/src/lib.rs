//! Minimal wgpu renderer for the Phase 1 asset viewer.
//!
//! PORT: FUN_80034b78, FUN_80034e4c, FUN_8002C69C, FUN_8002C488, FUN_8002B994, FUN_8003C310
//! PORT: FUN_80031D00, FUN_800337B0, FUN_80035CB8, FUN_80035DA0, FUN_80035E44, FUN_800349EC, FUN_80035EA8
//! PORT: FUN_8003C1F8 (per-glyph dialog-font sprite emit; the engine renders
//! in-game proportional dialog glyphs via the legaia-font atlas + textured-quad
//! overlay instead of the retail GP0 cell-UV push)
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

use anyhow::{Context, Result};
use glam::Mat4;
use legaia_tim::{VRAM_HEIGHT, VRAM_WIDTH, Vram};
use std::sync::Arc;
use wgpu::util::DeviceExt;

pub mod afterimage;
pub mod billboard;
pub mod gte;
pub mod gte_trace;
pub mod window;

pub use glam;
pub use legaia_font;
pub use legaia_tim;
pub use wgpu;

#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct Uniforms {
    /// (scale_x, scale_y, _pad, _pad) - multiplied with the unit quad to
    /// produce final NDC coordinates. Set by the host based on window vs
    /// texture aspect ratio.
    scale: [f32; 4],
}

#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct MeshUniforms {
    mvp: [[f32; 4]; 4],
    /// Direction the light is *coming from*, in world space, normalized.
    /// Stored as vec4 for std140 padding.
    light_dir: [f32; 4],
    /// PSX-faithful rendering knobs:
    /// - `[0]` viewport width in pixels (used for the sub-pixel snap)
    /// - `[1]` viewport height in pixels
    /// - `[2]` `1.0` to snap clip-space x/y to integer pixels (PSX-style
    ///   "vertex jitter"); `0.0` for smooth modern subpixel positions
    /// - `[3]` `1.0` to ordered-dither the shaded colour to 15-bit BGR555
    ///   (PSX framebuffer depth); `0.0` for full-precision colour
    psx_params: [f32; 4],
    /// GP0(0xE2) "Texture Window setting" - per-frame scene state.
    /// `[0..4]` = `(mask_x, mask_y, offset_x, offset_y)` each in 8-pixel
    /// steps (0..=31). The fragment shader applies, per texel:
    ///   `tex_x = (tex_x AND NOT (mask_x*8)) OR ((offset_x AND mask_x)*8)`
    ///   (and the same for Y), which clamps texture sampling to a smaller
    ///   window inside the texture page. No-op when all zero.
    ///
    /// Set with [`Renderer::set_texture_window`]. Defaults to all-zero so
    /// existing callers aren't affected.
    tex_window: [u32; 4],
}

pub struct UploadedTexture {
    bind_group: wgpu::BindGroup,
    width: u32,
    height: u32,
}

impl UploadedTexture {
    pub fn width(&self) -> u32 {
        self.width
    }
    pub fn height(&self) -> u32 {
        self.height
    }
}

/// A 3D mesh uploaded to the GPU. Built from
/// `(positions: Vec<[f32;3]>, indices: Vec<u32>)` where indices form
/// independent triangles (3 indices = 1 tri). Per-vertex normals are
/// computed at upload time by averaging adjacent face normals - adequate
/// for the TMD viewer where the source format only stores per-object
/// normals (not per-vertex).
pub struct UploadedMesh {
    vertex_buf: wgpu::Buffer,
    index_buf: wgpu::Buffer,
    index_count: u32,
}

impl UploadedMesh {
    pub fn index_count(&self) -> u32 {
        self.index_count
    }

    pub fn triangle_count(&self) -> u32 {
        self.index_count / 3
    }
}

/// GPU-resident textured mesh: position+UV per vertex (interleaved), index
/// buffer, plus a bind-group holding the texture+sampler used to paint it.
/// Uploaded by [`Renderer::upload_textured_mesh`].
pub struct UploadedTexturedMesh {
    vertex_buf: wgpu::Buffer,
    index_buf: wgpu::Buffer,
    index_count: u32,
}

impl UploadedTexturedMesh {
    pub fn index_count(&self) -> u32 {
        self.index_count
    }

    pub fn triangle_count(&self) -> u32 {
        self.index_count / 3
    }
}

/// GPU-resident PSX VRAM (1024×512 R16Uint texture). Built by
/// [`Renderer::upload_vram`] from a CPU-side [`Vram`]. Bound at @group(2)
/// of the VRAM-mesh pipeline so the fragment shader can do faithful PSX
/// texture-page + CLUT lookups (4bpp / 8bpp / 15bpp) using each prim's
/// per-vertex CBA + TSB.
pub struct UploadedVram {
    bind_group: wgpu::BindGroup,
    /// Monotonic per-[`Renderer`] upload stamp. Lets a host that expects a
    /// specific VRAM upload to stay GPU-resident (e.g. the battle texture
    /// across battle frames) detect that another path re-uploaded over it.
    generation: u64,
}

impl UploadedVram {
    /// The renderer-wide upload stamp this texture was created under.
    /// Strictly increasing across [`Renderer::upload_vram`] calls.
    pub fn generation(&self) -> u64 {
        self.generation
    }
}

/// VRAM-mesh: position + per-vertex UV (u8) + per-vertex CBA + TSB (u16).
/// Combined with an [`UploadedVram`] this lets the fragment shader do a
/// proper PSX texture lookup that selects the right texture page + CLUT
/// per primitive, regardless of which TIM the texels came from.
pub struct UploadedVramMesh {
    vertex_buf: wgpu::Buffer,
    index_buf: wgpu::Buffer,
    index_count: u32,
    /// `[(first_index, index_count); 4]` ranges of the per-ABR-mode
    /// semi-transparent "tail" appended past `index_count` in `index_buf`
    /// (see [`psx_blend::append_semi_tail`]). Drawn by the PSX-faithful
    /// blend pass; all-zero counts when the mesh has no semi prims.
    semi_ranges: [(u32, u32); 4],
}

impl UploadedVramMesh {
    pub fn index_count(&self) -> u32 {
        self.index_count
    }

    pub fn triangle_count(&self) -> u32 {
        self.index_count / 3
    }

    /// Per-ABR-mode index ranges of the semi-transparent tail (see
    /// [`psx_blend::append_semi_tail`]).
    pub fn semi_ranges(&self) -> [(u32, u32); 4] {
        self.semi_ranges
    }

    /// True when any prim in this mesh carries the semi-transparency enable.
    pub fn has_semi_prims(&self) -> bool {
        self.semi_ranges.iter().any(|&(_, n)| n > 0)
    }
}

/// GPU-resident **untextured** triangle mesh: per-vertex position + RGB colour,
/// flat face-shaded with no VRAM lookup. Built by [`Renderer::upload_color_mesh`]
/// from a [`legaia_tmd::mesh::ColorMesh`] for the `F*`/`G*` props whose prims
/// carry per-vertex colours instead of UVs (which the textured VRAM-mesh path
/// drops).
pub struct UploadedColorMesh {
    vertex_buf: wgpu::Buffer,
    index_buf: wgpu::Buffer,
    index_count: u32,
    /// `[(first_index, index_count); 4]` ranges of the per-ABR-mode
    /// semi-transparent "tail" appended past `index_count` in `index_buf`
    /// (see [`psx_blend::append_semi_tail_words`]). Drawn by the
    /// PSX-faithful blend pass; all-zero counts when the mesh has no semi
    /// prims (always the case via [`Renderer::upload_color_mesh`] - blend
    /// words come in through [`Renderer::upload_color_mesh_blended`]).
    semi_ranges: [(u32, u32); 4],
}

impl UploadedColorMesh {
    pub fn index_count(&self) -> u32 {
        self.index_count
    }

    pub fn triangle_count(&self) -> u32 {
        self.index_count / 3
    }

    /// Per-ABR-mode index ranges of the semi-transparent tail (see
    /// [`psx_blend::append_semi_tail_words`]).
    pub fn semi_ranges(&self) -> [(u32, u32); 4] {
        self.semi_ranges
    }

    /// True when any prim in this mesh carries the semi-transparency enable.
    pub fn has_semi_prims(&self) -> bool {
        self.semi_ranges.iter().any(|&(_, n)| n > 0)
    }
}

/// GPU-resident font atlas. Built by [`Renderer::upload_font_atlas`] from a
/// pre-decoded RGBA8 buffer. Used as the texture binding for the 2D text
/// pipeline.
pub struct UploadedFontAtlas {
    bind_group: wgpu::BindGroup,
    width: u32,
    height: u32,
}

impl UploadedFontAtlas {
    pub fn width(&self) -> u32 {
        self.width
    }
    pub fn height(&self) -> u32 {
        self.height
    }
}

/// One textured quad in screen space. Coordinates are pixel-space relative
/// to the top-left of the surface; the renderer converts to NDC at draw
/// time. Atlas coordinates are pixel-space inside the source font atlas.
#[derive(Debug, Clone, Copy)]
pub struct TextDraw {
    /// Destination rectangle: `(x, y, w, h)` in surface pixels.
    pub dst: (i32, i32, u32, u32),
    /// Source rectangle: `(x, y, w, h)` in atlas pixels.
    pub src: (u32, u32, u32, u32),
    /// RGBA tint multiplied with the sampled atlas texel.
    pub color: [f32; 4],
}

/// Batch of [`TextDraw`]s to render in one pass against a shared font atlas.
/// Cheap to construct each frame; the renderer copies the geometry into a
/// reusable dynamic buffer before drawing.
pub struct TextOverlay<'a> {
    pub atlas: &'a UploadedFontAtlas,
    pub draws: &'a [TextDraw],
}

/// Sprite types are semantic aliases of the text-quad types - both are
/// just textured quads sampled with nearest-neighbour filtering and alpha
/// blending. Sharing the GPU pipeline keeps engine-render small; the
/// distinct names keep call-sites readable.
pub type SpriteDraw = TextDraw;
pub type UploadedSpriteAtlas = UploadedFontAtlas;
pub type SpriteOverlay<'a> = TextOverlay<'a>;

/// One sprite request emitted by the World→sprite-batch glue. Holds the
/// renderer-agnostic shape of "draw glyph from this atlas page at this
/// world pixel position" so engine-core (which doesn't link wgpu) can
/// produce them and engine-render can consume them.
///
/// Engines that want richer per-sprite state (subpixel offset, rotation,
/// scale) should branch off this type - it's intentionally minimal.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpriteRequest {
    /// Top-left of the sprite in world / screen pixels.
    pub world_x: i32,
    pub world_y: i32,
    /// Atlas source rect: `(x, y, w, h)` in atlas pixels.
    pub atlas_src: (u32, u32, u32, u32),
    /// RGBA tint multiplied with the sampled atlas texel.
    pub color: [f32; 4],
}

/// Convert sprite requests to [`SpriteDraw`]s, applying a screen-space
/// `anchor` translation. The output `dst` width/height match the atlas
/// source rect 1:1 (no scaling - engines that want PSX-native 240px
/// vertical scaling should pre-scale `world_y` before calling this).
pub fn sprite_draws_for(requests: &[SpriteRequest], anchor: (i32, i32)) -> Vec<SpriteDraw> {
    requests
        .iter()
        .map(|r| SpriteDraw {
            dst: (
                anchor.0 + r.world_x,
                anchor.1 + r.world_y,
                r.atlas_src.2,
                r.atlas_src.3,
            ),
            src: r.atlas_src,
            color: r.color,
        })
        .collect()
}

/// Convert a [`legaia_font::Layout`] to a vector of [`TextDraw`]s anchored at
/// `pen` with the supplied tint. Glyph atlas coordinates come from the
/// layout; destination coordinates are pen-relative pixels with one quad per
/// glyph. The returned draws are batchable into a single [`TextOverlay`].
pub fn text_draws_for(
    layout: &legaia_font::Layout,
    pen: (i32, i32),
    color: [f32; 4],
) -> Vec<TextDraw> {
    layout
        .glyphs
        .iter()
        .map(|g| TextDraw {
            dst: (pen.0 + g.dst_x, pen.1 + g.dst_y, g.width, g.height),
            src: (g.atlas_x, g.atlas_y, g.width, g.height),
            color,
        })
        .collect()
}

/// One row in a shop or confirmation panel drawn by [`shop_draws_for`].
pub struct ShopRow<'a> {
    /// Display name for this row (item name, "Yes", "No", quantity digit, …).
    pub label: &'a str,
    /// Optional right-aligned price or value in gold. `None` for confirm /
    /// quantity rows where no price is shown.
    pub price: Option<u32>,
}

/// Build [`TextDraw`]s for a 2-D shop / confirmation panel.
///
/// Layout traced from `FUN_801d5de0` in `overlay_shop_save.bin`:
/// ```text
/// [title]
/// > item name              1500G
///   item name               200G   ← unaffordable rows are dimmed
///   …
/// Gold: 9999G
/// ```
/// Column offsets relative to `pen`:
/// - cursor `>`: x + 0 (`CURSOR_X`)
/// - item name: x + 20 (`LABEL_X`, retail `0x14`)
/// - price (left-aligned): x + 112 (`PRICE_X`, retail `0x70`)
/// - line height: 14 px (`LINE_H`, retail `0x0E`)
///
/// Rows where `gold < price` are rendered dim; selected row has a
/// gold-coloured price. `gold = None` suppresses the gold footer line.
///
/// A natural anchor for a PSX-style 320×240 surface is `(8, 140)`.
pub fn shop_draws_for<'a>(
    font: &legaia_font::Font,
    title: &str,
    rows: &[ShopRow<'a>],
    cursor: usize,
    gold: Option<i32>,
    pen: (i32, i32),
) -> Vec<TextDraw> {
    // Constants confirmed from overlay_shop_save FUN_801d5de0.
    const LINE_H: i32 = 14;
    const CURSOR_X: i32 = 0;
    const LABEL_X: i32 = 20; // retail 0x14
    const PRICE_X: i32 = 112; // retail 0x70, left edge of 6-digit price field

    let white: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
    let dim: [f32; 4] = [0.55, 0.55, 0.55, 1.0];
    let gold_col: [f32; 4] = [1.0, 0.85, 0.3, 1.0];

    let mut out = Vec::new();

    // Title line
    let title_layout = font.layout_ascii(title);
    out.extend(text_draws_for(&title_layout, pen, white));

    // Item rows
    for (i, row) in rows.iter().enumerate() {
        let row_y = pen.1 + LINE_H + i as i32 * LINE_H;
        let selected = i == cursor;

        // Retail dims rows the player cannot afford (gold < price).
        let can_afford = match (gold, row.price) {
            (Some(g), Some(p)) => g >= p as i32,
            _ => true,
        };
        let fg = if !can_afford || !selected { dim } else { white };

        if selected {
            let cur_layout = font.layout_ascii(">");
            out.extend(text_draws_for(
                &cur_layout,
                (pen.0 + CURSOR_X, row_y),
                white,
            ));
        }

        let label_layout = font.layout_ascii(row.label);
        out.extend(text_draws_for(&label_layout, (pen.0 + LABEL_X, row_y), fg));

        if let Some(price) = row.price {
            let price_str = format!("{price}G");
            let price_layout = font.layout_ascii(&price_str);
            let price_fg = if !can_afford {
                dim
            } else if selected {
                gold_col
            } else {
                dim
            };
            out.extend(text_draws_for(
                &price_layout,
                (pen.0 + PRICE_X, row_y),
                price_fg,
            ));
        }
    }

    // Gold footer (retail FUN_801d0148: gold icon at panel_x, amount at x+40).
    if let Some(g) = gold {
        let gold_y = pen.1 + LINE_H + rows.len() as i32 * LINE_H + 4;
        let gold_str = format!("Gold: {g}G");
        let gold_layout = font.layout_ascii(&gold_str);
        out.extend(text_draws_for(&gold_layout, (pen.0, gold_y), gold_col));
    }

    out
}

/// One row of plain text for the dialog presenter. Engines populate
/// this from their `DialogPanel::page_glyphs` view; the renderer wraps
/// them into `TextDraw`s without re-implementing the layout pass.
#[derive(Debug, Clone, Copy)]
pub struct DialogGlyphView {
    /// ASCII / latin glyph byte. Wide-glyph references are pre-folded
    /// to the operand byte (matches `DialogPanel`'s emit path).
    pub byte: u8,
    /// CLUT additive index (0..15). Engines pass `0` for the default
    /// pen; non-zero values come from inline `0xCF` color escapes.
    pub clut: u8,
}

/// Layout used by [`dialog_box_draws_for`]. Engines pass this once per
/// frame; the function recomputes the text wrap on the fly so engines
/// that care about retail-correct line breaks can substitute their own
/// layout pre-pass.
#[derive(Debug, Clone, Copy)]
pub struct DialogBoxLayout {
    /// Top-left of the panel in surface pixels.
    pub origin: (i32, i32),
    /// Width / height of the panel rectangle in pixels.
    pub size: (u32, u32),
    /// Internal margin between panel edge and text.
    pub padding: (i32, i32),
    /// Per-line vertical advance.
    pub line_h: i32,
    /// Maximum text columns per line (in glyphs). When a glyph would
    /// overflow this width, the renderer wraps to the next line.
    pub cols: u16,
}

impl Default for DialogBoxLayout {
    /// Retail layout traced from the dialog overlay (`FUN_801D84D0`):
    /// origin (8, 168), size (304, 56), padding (8, 8), line_h 14,
    /// cols 36 (matches the proportional dialog font's average advance
    /// at 304 px wide). Engines that don't render at 320×240 should
    /// override these.
    fn default() -> Self {
        Self {
            origin: (8, 168),
            size: (304, 56),
            padding: (8, 8),
            line_h: 14,
            cols: 36,
        }
    }
}

/// Resolve a dialog CLUT additive index to an RGBA tint. The retail
/// dialog renderer uses a small palette indexed at `_DAT_8007B454`;
/// we approximate the most common entries.
pub fn dialog_clut_color(clut: u8) -> [f32; 4] {
    match clut {
        0 => [1.0, 1.0, 1.0, 1.0],    // default white
        1 => [1.0, 0.85, 0.2, 1.0],   // gold (NPC names)
        2 => [0.5, 1.0, 0.5, 1.0],    // green (heal)
        3 => [1.0, 0.4, 0.4, 1.0],    // red (warning)
        4 => [0.4, 0.6, 1.0, 1.0],    // blue (lore)
        _ => [0.85, 0.85, 0.85, 1.0], // dim fallback
    }
}

/// Build [`TextDraw`]s for an open dialog box.
///
/// Layout: panel rectangle drawn first (engines render the rectangle
/// outside the text path; we don't emit it here since it's a quad, not
/// a glyph), then text wrapped onto sequential lines inside the
/// padded interior. Engines that want a "page break" cursor can layer
/// their own caret quad on top.
///
/// Wrapping: a simple left-to-right, glyph-width-driven greedy wrap.
/// Newline byte (`'\n'`) starts a new line. Spaces are kept literal.
/// Glyph layout uses [`legaia_font::Font::layout_ascii`] per glyph so
/// the proportional dialog font's advance values are honoured.
pub fn dialog_box_draws_for(
    font: &legaia_font::Font,
    glyphs: &[DialogGlyphView],
    layout: &DialogBoxLayout,
) -> Vec<TextDraw> {
    let interior_x = layout.origin.0 + layout.padding.0;
    let interior_y = layout.origin.1 + layout.padding.1;
    let max_x = layout.origin.0 + layout.size.0 as i32 - layout.padding.0;
    let max_y = layout.origin.1 + layout.size.1 as i32 - layout.padding.1;
    let mut pen_x = interior_x;
    let mut pen_y = interior_y;
    let mut out = Vec::with_capacity(glyphs.len());
    for g in glyphs {
        if g.byte == b'\n' {
            pen_x = interior_x;
            pen_y += layout.line_h;
            continue;
        }
        // Layout a single-byte string and look at the resulting glyph
        // width - that's the proportional advance.
        let s = [g.byte];
        let one = font.layout_ascii(std::str::from_utf8(&s).unwrap_or(" "));
        let advance = one
            .glyphs
            .first()
            .map(|gl| gl.width as i32 + 1)
            .unwrap_or(8);
        if pen_x + advance > max_x {
            pen_x = interior_x;
            pen_y += layout.line_h;
        }
        if pen_y + layout.line_h > max_y {
            // Out of vertical room - drop the rest of this page.
            break;
        }
        if let Some(gl) = one.glyphs.first() {
            out.push(TextDraw {
                dst: (pen_x + gl.dst_x, pen_y + gl.dst_y, gl.width, gl.height),
                src: (gl.atlas_x, gl.atlas_y, gl.width, gl.height),
                color: dialog_clut_color(g.clut),
            });
        }
        pen_x += advance;
    }
    out
}

/// Convenience wrapper: convert engine-core's `DialogPanel::page_glyphs`
/// shape (named `(byte, clut)` pairs) directly to [`TextDraw`]s.
///
/// Engines that import [`legaia_engine_core::dialog::PanelGlyph`] should
/// prefer this wrapper to skip the manual `DialogGlyphView` mapping.
pub fn dialog_panel_draws_for(
    font: &legaia_font::Font,
    panel_glyphs: &[(u8, u8)],
    layout: &DialogBoxLayout,
) -> Vec<TextDraw> {
    let views: Vec<DialogGlyphView> = panel_glyphs
        .iter()
        .map(|&(byte, clut)| DialogGlyphView { byte, clut })
        .collect();
    dialog_box_draws_for(font, &views, layout)
}

/// Build [`TextDraw`]s for a level-up banner overlay.
///
/// Renders two lines anchored at `pen`:
/// ```text
/// LEVEL UP!  (char_id, new_level)
/// HP +hp_gained  MP +mp_gained
/// ```
/// Designed for a PSX-style 320×240 surface; a typical anchor is around
/// `(8, 60)` to appear near the top of the screen after battle.
pub fn level_up_draws_for(
    font: &legaia_font::Font,
    char_id: u8,
    new_level: u8,
    hp_gained: u16,
    mp_gained: u16,
    pen: (i32, i32),
) -> Vec<TextDraw> {
    const LINE_H: i32 = 16;
    let yellow: [f32; 4] = [1.0, 0.9, 0.2, 1.0];
    let green: [f32; 4] = [0.4, 1.0, 0.4, 1.0];

    let line1 = format!("LEVEL UP! (char {} -> Lv {})", char_id + 1, new_level);
    let line2 = format!("HP +{}  MP +{}", hp_gained, mp_gained);

    let layout1 = font.layout_ascii(&line1);
    let layout2 = font.layout_ascii(&line2);

    let mut out = text_draws_for(&layout1, pen, yellow);
    out.extend(text_draws_for(&layout2, (pen.0, pen.1 + LINE_H), green));
    out
}

/// Build [`TextDraw`]s for the post-battle Seru-capture banner.
///
/// `text` is the single active banner line from
/// `SeruCaptureSession::current_banner` (e.g. `"Captured: Spark!"` or
/// `"Character 1 learned Aqua!"`). Drawn in cyan, the sibling of
/// [`level_up_draws_for`]; a natural anchor near the top of a 320×240 surface
/// is `(8, 40)`.
pub fn capture_banner_draws_for(
    font: &legaia_font::Font,
    text: &str,
    pen: (i32, i32),
) -> Vec<TextDraw> {
    let cyan: [f32; 4] = [0.4, 0.9, 1.0, 1.0];
    let layout = font.layout_ascii(text);
    text_draws_for(&layout, pen, cyan)
}

/// One row in the battle HUD's per-slot panel (built by
/// [`battle_hud_draws_for`]).
///
/// Engines populate this view from their HUD model on a per-frame basis.
/// The renderer is intentionally agnostic to the engine-core / engine-vm
/// types - pass plain data here to keep the layering clean.
#[derive(Clone, Copy)]
pub struct HudSlotView<'a> {
    /// Display name (character / monster). Empty string skips the row.
    pub name: &'a str,
    /// `true` for party rows (white text); `false` for monster rows
    /// (pale red text).
    pub is_party: bool,
    /// `true` if the actor is alive. Dead actors get a "K.O." overlay.
    pub alive: bool,
    pub hp: u16,
    pub hp_max: u16,
    pub mp: u16,
    pub mp_max: u16,
    /// Amount of AP committed to the action queue this turn.
    pub ap_filled: u8,
    /// Maximum AP for the slot this turn.
    pub ap_max: u8,
    /// One-letter abbreviations for active status icons. Engines pick the
    /// mapping (e.g. 'B' = Toxic, 'P' = Venom, 'S' = Curse, …).
    pub status_letters: &'a [u8],
}

/// One floating damage / heal / status popup.
#[derive(Clone, Copy)]
pub struct HudPopupView {
    pub slot: u8,
    pub amount: u16,
    pub is_heal: bool,
    pub is_crit: bool,
    /// Status letter to overlay on the popup ('B' = Toxic, etc.). `None`
    /// for plain numeric popups.
    pub status_letter: Option<u8>,
    /// Fade alpha 0..=1.0 multiplied into the text colour.
    pub alpha: f32,
}

/// One battle log line.
#[derive(Clone, Copy)]
pub struct HudLogView<'a> {
    pub text: &'a str,
    pub color: [f32; 4],
}

impl<'a> HudSlotView<'a> {
    /// Build a slot view from a plain-data row. The argument shape mirrors
    /// `legaia_engine_core::battle_hud::SlotView`; engines drive this from
    /// `BattleHud::slot_views()` without re-implementing the field copy.
    ///
    /// `name` and `status_letters` borrow from the caller; ownership stays
    /// in the engine-core view buffer.
    pub fn from_plain(meta: HudSlotMeta, name: &'a str, status_letters: &'a [u8]) -> Self {
        Self {
            name,
            is_party: meta.is_party,
            alive: meta.alive,
            hp: meta.hp,
            hp_max: meta.hp_max,
            mp: meta.mp,
            mp_max: meta.mp_max,
            ap_filled: meta.ap_filled,
            ap_max: meta.ap_max,
            status_letters,
        }
    }
}

/// Numeric fields of [`HudSlotView`] grouped into a payload struct so the
/// public constructor stays under clippy's argument-count threshold.
#[derive(Debug, Clone, Copy, Default)]
pub struct HudSlotMeta {
    pub is_party: bool,
    pub alive: bool,
    pub hp: u16,
    pub hp_max: u16,
    pub mp: u16,
    pub mp_max: u16,
    pub ap_filled: u8,
    pub ap_max: u8,
}

/// Retail HP-bar text colour index for a battle slot.
///
/// PORT: FUN_800349EC — returns the font-CLUT colour index the retail battle
/// HUD tints a character's HP readout with, keyed on the cur/max ratio. Index 2
/// is empty/K.O.; index 9 is danger (`cur <= max/4`); index 6 is caution
/// (`cur <= max/2`, or any time a status flag is set); index 7 is normal. The
/// thresholds use the same floored `max >> 2` / `max >> 1` comparisons as retail.
///
/// `status_active` models retail's per-character status byte (record `+0x36`,
/// `*(short *)(char*0x414 - 0x7ff7b7ca)`), which forces the caution tier even
/// above half HP; the engine approximates it with "any active status icon".
pub(crate) fn hp_bar_color_index(cur: u16, max: u16, status_active: bool) -> u8 {
    if cur == 0 {
        return 2;
    }
    if (max >> 2) < cur {
        if status_active || cur <= (max >> 1) {
            6
        } else {
            7
        }
    } else {
        9
    }
}

/// Retail MP-bar text colour index for a battle slot.
///
/// PORT: FUN_80035EA8 — the MP sibling of [`hp_bar_color_index`]. Same
/// `cur <= max/4` / `cur <= max/2` ratio tiers (index 9 danger, 6 caution,
/// 7 normal) but with no K.O. (2) state and no status-flag override — MP has no
/// "empty = dead" colour, so a depleted bar simply reads as danger.
pub(crate) fn mp_bar_color_index(cur: u16, max: u16) -> u8 {
    if (max >> 2) < cur {
        if cur <= (max >> 1) { 6 } else { 7 }
    } else {
        9
    }
}

/// Build [`TextDraw`]s for the battle HUD.
///
/// Layout (anchored at `pen`):
/// ```text
/// pen.x                                                pen.x + 240
///   ┌─────────────────────────────────────────────────────┐
///   │ Vahn          HP 250/300    MP  10/30   AP ●●●○○    │
///   │ Noa           HP 180/220    MP   5/20   AP ●●●●○    │
///   │ Gala          HP  90/280    MP   0/15   AP ○○○○○    │
///   │                                                     │
///   │ M Goblin      HP  50/100                            │
///   │ M Goblin      HP   0/100  K.O.                      │
///   └─────────────────────────────────────────────────────┘
///
/// pen.y + 80   [popup]  -25
///              [popup]  HEAL +50
/// ```
///
/// The log column uses `pen.x` and stacks downward from `pen.y +
/// slot_count * LINE_H`. Popups are drawn over each slot's row.
///
/// Constants:
/// - `LINE_H` = 14
/// - Status icons are tiled at x + 220 with 8 px stride
/// - Damage popups are placed at `pen.x + 80, slot_y - 16`
pub fn battle_hud_draws_for(
    font: &legaia_font::Font,
    slots: &[HudSlotView<'_>],
    popups: &[HudPopupView],
    log: &[HudLogView<'_>],
    pen: (i32, i32),
) -> Vec<TextDraw> {
    const LINE_H: i32 = 14;
    const STATUS_X: i32 = 220;
    const STATUS_STEP: i32 = 8;

    let white: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
    let monster: [f32; 4] = [1.0, 0.7, 0.7, 1.0];
    let dim: [f32; 4] = [0.5, 0.5, 0.5, 1.0];
    let red: [f32; 4] = [1.0, 0.4, 0.4, 1.0];
    let green: [f32; 4] = [0.5, 1.0, 0.5, 1.0];
    let yellow: [f32; 4] = [1.0, 0.95, 0.4, 1.0];
    let cyan: [f32; 4] = [0.5, 0.85, 1.0, 1.0];

    let mut out = Vec::new();

    for (i, slot) in slots.iter().enumerate() {
        if slot.name.is_empty() {
            continue;
        }
        let row_y = pen.1 + i as i32 * LINE_H;
        let row_color = if !slot.alive {
            dim
        } else if slot.is_party {
            white
        } else {
            monster
        };

        let name_layout = font.layout_ascii(slot.name);
        out.extend(text_draws_for(&name_layout, (pen.0, row_y), row_color));

        // Retail tints HP/MP readouts by the cur/max ratio (FUN_800349EC /
        // FUN_80035EA8). Map the returned colour index to the HUD palette: the
        // "normal" tier (7) keeps the row's base colour so monster rows stay
        // tinted; danger -> red, caution -> yellow, K.O. -> dim.
        let bar_color = |idx: u8| -> [f32; 4] {
            match idx {
                9 => red,
                6 => yellow,
                2 => dim,
                _ => row_color,
            }
        };

        let hp_text = format!("HP {:>3}/{:>3}", slot.hp, slot.hp_max);
        let hp_layout = font.layout_ascii(&hp_text);
        let hp_color = if !slot.alive {
            dim
        } else {
            bar_color(hp_bar_color_index(
                slot.hp,
                slot.hp_max,
                !slot.status_letters.is_empty(),
            ))
        };
        out.extend(text_draws_for(&hp_layout, (pen.0 + 70, row_y), hp_color));

        if slot.mp_max > 0 {
            let mp_text = format!("MP {:>3}/{:>3}", slot.mp, slot.mp_max);
            let mp_layout = font.layout_ascii(&mp_text);
            let mp_color = if !slot.alive {
                dim
            } else {
                bar_color(mp_bar_color_index(slot.mp, slot.mp_max))
            };
            out.extend(text_draws_for(&mp_layout, (pen.0 + 140, row_y), mp_color));
        }

        if slot.ap_max > 0 {
            let mut ap_text = String::with_capacity(2 + slot.ap_max as usize);
            ap_text.push_str("AP");
            for n in 0..slot.ap_max {
                if n < slot.ap_filled {
                    ap_text.push('o'); // filled
                } else {
                    ap_text.push('-'); // empty
                }
            }
            let ap_layout = font.layout_ascii(&ap_text);
            out.extend(text_draws_for(&ap_layout, (pen.0 + 200, row_y), row_color));
        }

        if !slot.alive {
            let ko_layout = font.layout_ascii("K.O.");
            out.extend(text_draws_for(&ko_layout, (pen.0 + 110, row_y), red));
        }

        for (k, letter) in slot.status_letters.iter().enumerate() {
            let s = (*letter as char).to_string();
            let layout = font.layout_ascii(&s);
            out.extend(text_draws_for(
                &layout,
                (pen.0 + STATUS_X + k as i32 * STATUS_STEP, row_y - 12),
                yellow,
            ));
        }
    }

    let log_x = pen.0;
    let log_y = pen.1 + slots.len() as i32 * LINE_H + 4;
    for (i, line) in log.iter().enumerate() {
        let layout = font.layout_ascii(line.text);
        out.extend(text_draws_for(
            &layout,
            (log_x, log_y + i as i32 * LINE_H),
            line.color,
        ));
    }

    for popup in popups {
        if (popup.slot as usize) >= slots.len() {
            continue;
        }
        let slot_y = pen.1 + popup.slot as i32 * LINE_H;
        let popup_color = match (popup.is_heal, popup.is_crit) {
            (true, _) => apply_alpha(green, popup.alpha),
            (_, true) => apply_alpha(yellow, popup.alpha),
            _ => apply_alpha(cyan, popup.alpha),
        };
        let text = if let Some(letter) = popup.status_letter {
            format!("[{}]", letter as char)
        } else if popup.is_heal {
            format!("+{}", popup.amount)
        } else {
            format!("-{}", popup.amount)
        };
        let layout = font.layout_ascii(&text);
        out.extend(text_draws_for(
            &layout,
            (pen.0 + 80, slot_y - 16),
            popup_color,
        ));
    }

    out
}

fn apply_alpha(color: [f32; 4], alpha: f32) -> [f32; 4] {
    [
        color[0],
        color[1],
        color[2],
        color[3] * alpha.clamp(0.0, 1.0),
    ]
}

/// Build [`TextDraw`]s for the title screen.
///
/// Phase argument controls which UI is rendered:
/// - `phase` = 0: fade-in (no text - engines fade the screen to black);
/// - `phase` = 1: "Press START" prompt (centered roughly mid-screen);
/// - `phase` = 2: main menu (New Game / Continue / Options stacked).
///
/// `cursor` is ignored for phases 0/1 and selects the highlighted row
/// (0..=2) in phase 2. `continue_enabled = false` dims the Continue row.
/// `blink_on` toggles the prompt visibility on phase 1 every blink_period
/// frames; engines drive this from the title session's blink phase.
///
/// When the engine has uploaded the PROT 0888 title TIM atlas, pass
/// `atlas_present = true` to suppress the font-rendered "PRESS START"
/// prompt (phase 1) - the TIM's own "PRESS START BUTTON" band is drawn
/// in its place by the sprite layer. The menu rows (phase 2) are
/// still rendered via font because retail uses larger font glyphs
/// there too, not the tiny "NEW GAME CONTINUE" band at the bottom of
/// the TIM.
///
/// A natural anchor for a 320×240 surface is `pen = (96, 100)` - the
/// renderer offsets each line from this top-left.
pub fn title_draws_for(
    font: &legaia_font::Font,
    phase: u8,
    cursor: u8,
    continue_enabled: bool,
    blink_on: bool,
    atlas_present: bool,
    pen: (i32, i32),
) -> Vec<TextDraw> {
    const LINE_H: i32 = 16;
    let white: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
    let dim: [f32; 4] = [0.45, 0.45, 0.45, 1.0];
    let gold: [f32; 4] = [1.0, 0.85, 0.3, 1.0];

    let mut out = Vec::new();

    match phase {
        0 => {}
        1 if blink_on && !atlas_present => {
            let l = font.layout_ascii("PRESS START");
            out.extend(text_draws_for(&l, pen, white));
        }
        1 => {}
        2 => {
            // Retail title menu carries only two rows; Options lives in
            // the in-game field menu. Color is the selection indicator
            // (selected = white, unselected = dim) — no arrow / cursor
            // mark in retail. The disabled-Continue row reads the same
            // as a non-highlighted row.
            let _ = (gold, continue_enabled);
            let rows = ["NEW GAME", "CONTINUE"];
            for (i, label) in rows.iter().enumerate() {
                let row_y = pen.1 + i as i32 * LINE_H;
                let selected = i as u8 == cursor;
                let color = if selected { white } else { dim };
                let l = font.layout_ascii(label);
                out.extend(text_draws_for(&l, (pen.0, row_y), color));
            }
        }
        _ => {}
    }
    out
}

/// Build [`SpriteDraw`]s for the title-screen main-menu rows ("NEW GAME"
/// / "CONTINUE") sampling the dedicated menu-glyph atlas from
/// `PROT.DAT` (see [`legaia_asset::menu_glyph_atlas`]).
///
/// Retail-faithful equivalent of phase 2 in [`title_draws_for`] — same
/// row labels and cursor / dim semantics, but each row is a horizontal
/// strip of sprite cells sampled from the menu-glyph atlas instead of
/// dialog-font glyphs. Selected row gets a gold tint; the Continue
/// row is dimmed when `continue_enabled = false`. Retail's title menu
/// only carries two rows (NEW GAME / CONTINUE); Options is reached via
/// the in-game field menu, not from the title.
///
/// `cell_scale` is an integer multiplier applied to source-pixel sizes
/// so engines can match the title-art's `play-window` stage scale
/// (mirrors the per-band SpriteDraw scaling). `pen` is the top-left
/// corner of the first row's first glyph in surface pixels.
///
/// Note: the menu-glyph atlas carries only uppercase letters and
/// digits — no cursor marks.
///
/// Returns an empty vec for any phase other than 2.
pub fn title_menu_draws_for(
    phase: u8,
    cursor: u8,
    continue_enabled: bool,
    pen: (i32, i32),
    cell_scale: u32,
) -> Vec<SpriteDraw> {
    if phase != 2 {
        return Vec::new();
    }
    // Retail uses color as the SELECTION INDICATOR: the highlighted row
    // is bright white and unselected rows are dim gray. There is no
    // arrow / cursor mark — the brightness IS the cursor. Disabled
    // (Continue with no save) reads the same as a non-highlighted row.
    let white: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
    let dim: [f32; 4] = [0.55, 0.55, 0.55, 1.0];

    use legaia_asset::menu_glyph_atlas as mga;
    let cell_w = mga::GLYPH_W as i32;
    let cell_h = mga::ALPHABET_GLYPH_H as i32;
    let scale = cell_scale.max(1) as i32;
    // One blank row of padding between rows so the small-caps glyphs
    // sit clearly apart (matches the retail menu vertical pitch).
    let line_h = cell_h + 2;

    let rows = ["NEW GAME", "CONTINUE"];
    let mut out = Vec::new();
    for (i, label) in rows.iter().enumerate() {
        let row_y = pen.1 + i as i32 * line_h * scale;
        let selected = i as u8 == cursor;
        let row_disabled = i == 1 && !continue_enabled;
        let _ = row_disabled; // disabled rows render the same as unselected
        let color = if selected { white } else { dim };
        let mut x = pen.0;
        for c in label.chars() {
            if let Some((sx, sy, sw, sh)) = mga::glyph_rect(c) {
                out.push(SpriteDraw {
                    dst: (x, row_y, sw * scale as u32, sh * scale as u32),
                    src: (sx, sy, sw, sh),
                    color,
                });
            }
            x += cell_w * scale;
        }
    }
    out
}

/// One slot row passed into [`save_select_draws_for`]. Plain-data view
/// so the renderer doesn't depend on `engine-core::save_select`.
pub struct SaveSelectRow<'a> {
    pub label: &'a str,
    pub present: bool,
    pub party_lv: u8,
    pub play_time_seconds: u32,
    pub money: u32,
    pub location: &'a str,
}

/// **Canonical PSX framebuffer stage for the boot UI**. All retail-
/// pinned positions (panel, pills, cursor, title art) are expressed
/// in 320×240 framebuffer coords; the boot-UI stage maps 1:1 to
/// this so everything stays in lockstep at any window resolution.
///
/// Engines compute `stage_scale = min(surface_w / 320, surface_h /
/// 240).clamp(1, 4)`, center the resulting `320*scale × 240*scale`
/// rectangle inside the surface, and use that as the `stage_origin`
/// for every boot-UI sprite emission.
pub const BOOT_UI_STAGE_W: u32 = 320;
/// Companion to [`BOOT_UI_STAGE_W`].
pub const BOOT_UI_STAGE_H: u32 = 240;

/// Retail PSX framebuffer position of the load-screen panel top-left.
/// Mirror of [`legaia_asset::title_pak::OVERLAY_SAVE_PANEL_RETAIL_DST`].
pub const SAVE_SELECT_PANEL_POS: (i32, i32) = (6, 4);
/// Total size of the load-screen panel in source pixels.
pub const SAVE_SELECT_PANEL_SIZE: (i32, i32) = (81, 29);
/// Retail PSX framebuffer position of the SLOT 1 pill **sprite top
/// edge**. Pinned via direct framebuffer-pixel inspection at sstate9
/// — the rounded pill outline starts at `fb_y=99` (transition pixels),
/// the saturated-blue body at `y=101`, sprite bottom at `y=112`. The
/// earlier `y=102` pin tracked the saturated-blue body, not the
/// sprite-top edge — drawing at that offset made the cursor finger
/// look too high relative to the pill chrome.
pub const SAVE_SELECT_SLOT1_POS: (i32, i32) = (137, 99);
/// Retail pin of the SLOT 1 pill sprite top-left **after the user
/// has committed to loading a slot** — once the load flow enters
/// `NowChecking` / `SlotPreview`, retail relocates the active pill
/// up under the Load panel. Pinned via the slide-in primitive
/// `FUN_801E1C1C` mode 2 in `overlay_save_ui_select_801dd35c.txt`:
/// the dispatcher calls `FUN_801e1c1c(2, DAT_801ef194, 0xa0, 0x60,
/// 0x30, 0x28)` — slide from `(160, 96)` to **target `(48, 40)`**.
/// Mode 2's GPU emit pre-shifts `sVar6 = param_3 - 0x18` so the
/// composite's top-left lands at `(24, 40)`. The earlier
/// screenshot-derived `(22, 41)` was ~2px off due to anti-aliased
/// sprite-edge sampling. Pairs with [`SAVE_SELECT_CURSOR_POS_LOAD_ACTIVE`].
pub const SAVE_SELECT_SLOT1_POS_LOAD_ACTIVE: (i32, i32) = (24, 40);
/// Vertical pitch between consecutive slot pill sprite tops in
/// framebuffer pixels. SLOT 1 sprite top at y=99, SLOT 2 at y=115.
pub const SAVE_SELECT_SLOT_PITCH_Y: i32 = 16;
/// Retail PSX framebuffer position of the pointing-finger cursor when
/// pointing at SLOT 1. Mirror of
/// [`legaia_asset::title_pak::OVERLAY_SAVE_CURSOR_RETAIL_DST`]. SLOT 2
/// shifts the cursor down by [`SAVE_SELECT_SLOT_PITCH_Y`].
pub const SAVE_SELECT_CURSOR_POS: (i32, i32) = (114, 100);
/// Pin of the pointing-finger cursor sprite top-left during the
/// Load-active states (NowChecking / SlotPreview), matching the
/// SLOT 1 pill at [`SAVE_SELECT_SLOT1_POS_LOAD_ACTIVE`]. Retail
/// hides the pill cursor while the dialog is up and the grid
/// emits its own cursor, so this constant is currently unused in
/// emission — kept here for parity with the Browsing pin in case
/// a future variant needs it.
pub const SAVE_SELECT_CURSOR_POS_LOAD_ACTIVE: (i32, i32) = (10, 41);
/// **DEPRECATED** — superseded by [`SAVE_SELECT_CURSOR_POS`]. Old
/// callers used this with `SAVE_SELECT_SLOT1_POS` to derive cursor
/// placement; new code should use [`SAVE_SELECT_CURSOR_POS`] directly.
pub const SAVE_SELECT_CURSOR_X_OFFSET: i32 = -14;

/// Retail PSX framebuffer position of the **left edge of the first
/// title glyph** drawn inside the load-screen panel. Pinned via
/// GPULog primitive scan at sstate9 (parked on the load screen):
/// retail emits four 14x15 textured-sprite primitives at dst
/// `(35, 13)`, `(42, 13)`, `(48, 13)`, `(55, 13)` for `L`, `o`, `a`,
/// `d`. Stage coords; the engine applies `stage_origin + pos *
/// stage_scale` when emitting screen-pixel draws.
///
/// Pairs with [`SAVE_SELECT_TITLE_COLOR`] (the bright-text CLUT entry
/// retail picks from the menu CLUT block at VRAM `(208, 510)`).
pub const SAVE_SELECT_TITLE_POS: (i32, i32) = (35, 13);

/// RGBA tint applied to the dialog-font stencil when rendering the
/// load-screen title word. Pinned to retail's framebuffer pixel
/// colour at sstate9: every bright "Load" texel is RGB `(206, 206,
/// 206)` (= entry `[15]` of the menu CLUT at VRAM `(208, 510)`).
/// The dialog-font atlas is whitewashed at load (see
/// `legaia_font::Font::load_paths`), so `color * texel = color` at
/// opaque texels — making the tint the source of truth for the
/// final pixel colour.
pub const SAVE_SELECT_TITLE_COLOR: [f32; 4] = [206.0 / 255.0, 206.0 / 255.0, 206.0 / 255.0, 1.0];

/// Retail PSX framebuffer position of the title-art atlas top-left.
/// Pinned via GPU primitive scan at sstate9: retail draws the title
/// quad at dst `(33, 6)..(287, 154)` sampling source `(0, 0, 254, 148)`
/// of the 256×256 title TIM (PROT 0888). Used as the anchor when
/// composing the title-screen on the canonical 320×240 stage.
pub const TITLE_ART_POS: (i32, i32) = (33, 6);
/// Retail-pinned size the title quad is drawn at — same as its source
/// sub-rect dimensions (no scaling).
pub const TITLE_ART_SIZE: (i32, i32) = (254, 148);

/// Source rects for the save-menu atlas, mirrored from
/// [`legaia_asset::title_pak::OVERLAY_SYSTEM_UI_PANEL_*`] (9-slice
/// panel chrome) + `OVERLAY_SAVE_MENU_BAND_SLOT[12]` (pills). Passed
/// into [`save_select_chrome_draws_for`] so it can build SpriteDraws
/// without depending on the engine-core build of the atlas.
///
/// The 9-slice panel tiles were pinned via
/// `scripts/pcsx-redux/scan_panel_prims.py` against the PCSX-Redux
/// sstate9 RAM dump — retail draws the panel as 14 separate
/// `GP0_TEXTURED_SPRITE` primitives sampling CLUT row 2 of the
/// system-UI sprite sheet at `PROT.DAT[0x018E0]`.
#[derive(Debug, Clone, Copy)]
pub struct SaveMenuAtlasRects {
    /// Panel top-left corner tile (4x4, CLUT row 2 of system-UI TIM).
    pub panel_tl: (u32, u32, u32, u32),
    /// Panel top-right corner tile (4x4).
    pub panel_tr: (u32, u32, u32, u32),
    /// Panel bottom-left corner tile (4x4).
    pub panel_bl: (u32, u32, u32, u32),
    /// Panel bottom-right corner tile (4x4).
    pub panel_br: (u32, u32, u32, u32),
    /// Panel top edge tile (24x4) — repeated horizontally between
    /// the top corners with a 1-wide remainder if the panel width
    /// doesn't divide evenly.
    pub panel_top: (u32, u32, u32, u32),
    /// Panel bottom edge tile (24x4).
    pub panel_bot: (u32, u32, u32, u32),
    /// Panel left edge tile (4x21).
    pub panel_left: (u32, u32, u32, u32),
    /// Panel right edge tile (4x21).
    pub panel_right: (u32, u32, u32, u32),
    /// SLOT 1 pill source rect (CLUT 7, bright blue).
    pub slot1: (u32, u32, u32, u32),
    /// SLOT 2 pill source rect (CLUT 7, bright blue).
    pub slot2: (u32, u32, u32, u32),
    /// Pointing-finger cursor sprite (16x16, CLUT row 7 of the
    /// system-UI TIM). Retail renders this to the left of the active
    /// slot pill in the SaveSelect menu.
    pub cursor: (u32, u32, u32, u32),
    /// Panel interior fill tile (32x29, gradient-baked). Retail draws
    /// this as 3 gouraud-shaded textured-quad primitives sampling
    /// the marbled-blue stippled region of the system-UI TIM. The
    /// atlas builder pre-bakes the gouraud gradient so the engine
    /// can draw the tile as a regular SpriteDraw, tiled horizontally
    /// 2× full-width + 1× 17-wide-remainder.
    pub panel_interior: (u32, u32, u32, u32),
    /// Load-screen empty-cell frame sprite (32x32, 20x20 blue hollow
    /// frame centered with 6px transparent margin). Used by the slot-
    /// preview screen to draw the 5x3 grid of save-slot boxes. When
    /// `None`, the slot-preview falls back to a solid blue rect.
    pub load_empty_frame: Option<(u32, u32, u32, u32)>,
    /// Up to 3 character portrait sub-rects (16x16 each, decoded
    /// from PROT.DAT[0x1AC90..0x1AF30]). Index = char_id (0=Vahn,
    /// 1=Noa, 2=Gala). `None` for char_ids past the 3-portrait atlas.
    pub load_portrait_by_char: [Option<(u32, u32, u32, u32)>; 3],
}

/// Build [`SpriteDraw`]s for the retail save-screen chrome (9-slice
/// panel frame + slot pills) anchored at the supplied stage origin.
///
/// Retail composes the 81×29 panel from 14 textured-sprite primitives
/// — 4 corners (4×4 each), top + bottom edges (24×4 repeated 3× with
/// a 1×4 remainder), and left + right edges (4×21). This function
/// reproduces that composition exactly, pulling tiles from the
/// system-UI sprite sheet at the byte-pinned source rects in
/// `legaia_asset::title_pak::OVERLAY_SYSTEM_UI_PANEL_*`.
///
/// No interior fill is drawn — retail leaves the middle of the
/// 9-slice frame empty so the dimmed title art behind shows through.
///
/// Layout (positions in stage pixels, relative to `stage_origin`):
/// ```text
///   ┌──────────┐
///   │   Load   │           ← panel @ SAVE_SELECT_PANEL_POS, 81x29
///   └──────────┘
///                  SLOT 1  ← pill @ SAVE_SELECT_SLOT1_POS
///                  SLOT 2  ← stacked at +SAVE_SELECT_SLOT_PITCH_Y
/// ```
///
/// `pills` lists the slot indices whose pills are drawn. Pills are
/// rendered at `pill_anchor + (0, slot_index * PITCH)`, and slot
/// index `0` uses the SLOT 1 sprite while every other index falls
/// back to the SLOT 2 sprite. Retail draws all pills during Browsing
/// (`&[0, 1]`) but shows only the selected pill once a slot has been
/// confirmed (`&[selected_slot]`) — the NowChecking dialog and
/// SlotPreview grid both hide the non-selected pills.
///
/// `pill_anchor` is the framebuffer top-left of the slot-index-0
/// pill. Pass [`SAVE_SELECT_SLOT1_POS`] during Browsing and
/// [`SAVE_SELECT_SLOT1_POS_LOAD_ACTIVE`] during the Load-active
/// states (NowChecking / SlotPreview), matching retail's pill
/// relocation up under the Load panel once a slot is committed.
///
/// `stage_scale` multiplies every dst dimension so callers that
/// upscale a 256x256 stage into a larger surface keep the chrome
/// in lockstep with the title-art bands.
pub fn save_select_chrome_draws_for(
    rects: &SaveMenuAtlasRects,
    pills: &[u8],
    pill_anchor: (i32, i32),
    stage_origin: (i32, i32),
    stage_scale: u32,
) -> Vec<SpriteDraw> {
    let scale = stage_scale.max(1);
    let mut out: Vec<SpriteDraw> = Vec::new();
    let white: [f32; 4] = [1.0, 1.0, 1.0, 1.0];

    let push = |out: &mut Vec<SpriteDraw>,
                src: (u32, u32, u32, u32),
                dst_stage_x: i32,
                dst_stage_y: i32,
                dst_w_stage: i32,
                dst_h_stage: i32,
                color: [f32; 4]| {
        out.push(SpriteDraw {
            dst: (
                stage_origin.0 + dst_stage_x * scale as i32,
                stage_origin.1 + dst_stage_y * scale as i32,
                (dst_w_stage as u32) * scale,
                (dst_h_stage as u32) * scale,
            ),
            src,
            color,
        });
    };

    // 9-slice panel composition. All dst coords are stage pixels;
    // the byte-perfect retail dimensions are (81 wide, 29 tall) with
    // 4-pixel corners + 24-wide edge tiles repeated. Pinned via
    // GP0 primitive scan — see `project_load_screen_panel_source_pinned`.
    let (panel_x, panel_y) = SAVE_SELECT_PANEL_POS;
    let (panel_w, panel_h) = SAVE_SELECT_PANEL_SIZE;
    let corner_w = rects.panel_tl.2 as i32; // 4
    let corner_h = rects.panel_tl.3 as i32; // 4
    let edge_w = rects.panel_top.2 as i32; // 24
    let edge_h = rects.panel_top.3 as i32; // 4
    let v_edge_h = rects.panel_left.3 as i32; // 21

    // --- Panel interior (drawn FIRST so the 9-slice border draws on top) ---
    // Retail emits 3 textured-gouraud quads sampling the same 32x29
    // marbled-blue region from the system-UI TIM with a vertical
    // gray gradient pre-baked into the atlas tile. The quads tile
    // horizontally to cover the full 81-wide panel: 2 full 32-wide
    // copies + 1 17-wide remainder.
    let interior_w = rects.panel_interior.2 as i32; // 32
    let interior_h = rects.panel_interior.3 as i32; // 29
    let mut x_int = panel_x;
    let interior_right = panel_x + panel_w;
    while x_int < interior_right {
        let remaining = interior_right - x_int;
        let this_w = remaining.min(interior_w);
        // Narrow the src rect's width when we're on the last
        // (partial) tile so engines sample only the columns retail
        // actually covers.
        let (sx, sy, _, sh) = rects.panel_interior;
        let src = (sx, sy, this_w as u32, sh);
        push(&mut out, src, x_int, panel_y, this_w, interior_h, white);
        x_int += this_w;
    }

    // --- Corners (4 tiles) ---
    push(
        &mut out,
        rects.panel_tl,
        panel_x,
        panel_y,
        corner_w,
        corner_h,
        white,
    );
    push(
        &mut out,
        rects.panel_tr,
        panel_x + panel_w - corner_w,
        panel_y,
        corner_w,
        corner_h,
        white,
    );
    push(
        &mut out,
        rects.panel_bl,
        panel_x,
        panel_y + panel_h - corner_h,
        corner_w,
        corner_h,
        white,
    );
    push(
        &mut out,
        rects.panel_br,
        panel_x + panel_w - corner_w,
        panel_y + panel_h - corner_h,
        corner_w,
        corner_h,
        white,
    );

    // --- Top + bottom edges (repeating 24-wide tiles with remainder) ---
    let edge_span = panel_w - 2 * corner_w; // 73 pixels between corners
    let full_tiles = edge_span / edge_w; // 3 full 24-wide tiles
    let remainder = edge_span - full_tiles * edge_w; // 1 pixel remainder
    let edge_y_top = panel_y;
    let edge_y_bot = panel_y + panel_h - edge_h;
    let mut x = panel_x + corner_w;
    for _ in 0..full_tiles {
        push(
            &mut out,
            rects.panel_top,
            x,
            edge_y_top,
            edge_w,
            edge_h,
            white,
        );
        push(
            &mut out,
            rects.panel_bot,
            x,
            edge_y_bot,
            edge_w,
            edge_h,
            white,
        );
        x += edge_w;
    }
    if remainder > 0 {
        // Sample only the first `remainder` columns of the edge tile
        // — retail dispatches this as a separate sprite with width
        // narrowed to the remainder.
        let (ux, uy, _, uh) = rects.panel_top;
        let top_rem = (ux, uy, remainder as u32, uh);
        let (bx, by, _, bh) = rects.panel_bot;
        let bot_rem = (bx, by, remainder as u32, bh);
        push(&mut out, top_rem, x, edge_y_top, remainder, edge_h, white);
        push(&mut out, bot_rem, x, edge_y_bot, remainder, edge_h, white);
    }

    // --- Left + right edges (single tall tile each) ---
    push(
        &mut out,
        rects.panel_left,
        panel_x,
        panel_y + corner_h,
        corner_w,
        v_edge_h,
        white,
    );
    push(
        &mut out,
        rects.panel_right,
        panel_x + panel_w - corner_w,
        panel_y + corner_h,
        corner_w,
        v_edge_h,
        white,
    );

    // --- Slot pills (atlas decoded with CLUT 7) at their natural row
    // positions anchored at `pill_anchor`. Each pill is drawn at
    // `pill_anchor + (0, slot_index*PITCH)` so retail-pinned positions
    // stay stable regardless of which subset of pills is currently
    // visible (selected-only during NowChecking / SlotPreview vs. all
    // pills during Browsing) AND so retail's Load-active relocation of
    // SLOT 1 under the Load panel is just a different `pill_anchor`.
    for &slot in pills {
        let dst_y = pill_anchor.1 + (slot as i32) * SAVE_SELECT_SLOT_PITCH_Y;
        let src = if slot == 0 { rects.slot1 } else { rects.slot2 };
        push(
            &mut out,
            src,
            pill_anchor.0,
            dst_y,
            src.2 as i32,
            src.3 as i32,
            white,
        );
    }

    out
}

/// Build the [`SpriteDraw`] for the pointing-finger cursor sprite.
/// Separate from [`save_select_chrome_draws_for`] so callers can
/// choose whether to draw the cursor (e.g. suppress during fade-out)
/// and where to anchor it (typically `cursor_row` selects which slot
/// pill the finger points at).
///
/// `cursor_row` is the 0-indexed pill the cursor sits next to.
pub fn save_select_cursor_draw_for(
    rects: &SaveMenuAtlasRects,
    cursor_row: usize,
    stage_origin: (i32, i32),
    stage_scale: u32,
) -> SpriteDraw {
    let scale = stage_scale.max(1);
    let src = rects.cursor;
    // Cursor at retail's byte-pinned framebuffer position when
    // pointing at SLOT 1; shifts down by SAVE_SELECT_SLOT_PITCH_Y
    // per pill row.
    let dst_stage_x = SAVE_SELECT_CURSOR_POS.0;
    let dst_stage_y = SAVE_SELECT_CURSOR_POS.1 + (cursor_row as i32) * SAVE_SELECT_SLOT_PITCH_Y;
    SpriteDraw {
        dst: (
            stage_origin.0 + dst_stage_x * scale as i32,
            stage_origin.1 + dst_stage_y * scale as i32,
            src.2 * scale,
            src.3 * scale,
        ),
        src,
        color: [1.0, 1.0, 1.0, 1.0],
    }
}

// -----------------------------------------------------------------------
// Generic 9-slice panel composition + "Now checking" dialog + slot-preview
// grid + slot-info panel rendering.
//
// All positions are stage pixels (32x240 boot-UI stage); `stage_scale`
// upscales to surface pixels. Pinned against the captured retail
// framebuffer in `captures/slot_info_dump/.../now_checking_fb.png`
// (sstate9 → CROSS → ~30 vsyncs) and `slot_info_fb.png` (~170 vsyncs).
// -----------------------------------------------------------------------

/// Compose a 9-slice panel at arbitrary `(dst_x, dst_y, dst_w, dst_h)`
/// stage pixels into `out`. Tiles the top/bottom 24-wide edges with a
/// remainder, and tiles the left/right 4×21 edges vertically with a
/// remainder. Used by both [`save_select_chrome_draws_for`] (which has
/// its own legacy code path that retains byte-exact behaviour) and
/// [`now_checking_panel_draws_for`].
///
/// Interior fill: a horizontal tiling of `rects.panel_interior` with
/// the per-tile width narrowed on the last (partial) column. The
/// retail engine emits the interior FIRST (3 gouraud-shaded quads
/// covering 32+32+17 of the 81-wide panel), then the border on top.
fn nine_slice_panel_into(
    out: &mut Vec<SpriteDraw>,
    rects: &SaveMenuAtlasRects,
    dst_stage: (i32, i32, i32, i32), // (x, y, w, h)
    stage_origin: (i32, i32),
    stage_scale: u32,
) {
    let scale = stage_scale.max(1) as i32;
    let white = [1.0, 1.0, 1.0, 1.0];
    let (px, py, pw, ph) = dst_stage;

    let push = |out: &mut Vec<SpriteDraw>,
                src: (u32, u32, u32, u32),
                sx: i32,
                sy: i32,
                sw: i32,
                sh: i32| {
        out.push(SpriteDraw {
            dst: (
                stage_origin.0 + sx * scale,
                stage_origin.1 + sy * scale,
                (sw as u32) * scale as u32,
                (sh as u32) * scale as u32,
            ),
            src,
            color: white,
        });
    };

    let cw = rects.panel_tl.2 as i32;
    let ch = rects.panel_tl.3 as i32;
    let edge_w = rects.panel_top.2 as i32;
    let edge_h = rects.panel_top.3 as i32;
    let v_edge_h = rects.panel_left.3 as i32;

    // Interior fill — horizontal tiling.
    let int_w = rects.panel_interior.2 as i32;
    let int_h = rects.panel_interior.3 as i32;
    // Match interior tile height to actual panel height (retail's panel
    // is 29 tall but here we may want 32+). If panel is taller than the
    // 29-tall interior tile, stretch vertically by emitting a single
    // sprite with full height.
    let interior_h = ph.min(int_h.max(ph));
    let mut x_int = px;
    while x_int < px + pw {
        let remaining = px + pw - x_int;
        let this_w = remaining.min(int_w);
        let (sx, sy, _, sh) = rects.panel_interior;
        let actual_sh = sh.min(interior_h as u32);
        let src = (sx, sy, this_w as u32, actual_sh);
        push(out, src, x_int, py, this_w, interior_h);
        x_int += this_w;
    }

    // Four corners.
    push(out, rects.panel_tl, px, py, cw, ch);
    push(out, rects.panel_tr, px + pw - cw, py, cw, ch);
    push(out, rects.panel_bl, px, py + ph - ch, cw, ch);
    push(out, rects.panel_br, px + pw - cw, py + ph - ch, cw, ch);

    // Top + bottom edges with remainder.
    let edge_span = pw - 2 * cw;
    let full_tiles = edge_span / edge_w;
    let remainder = edge_span - full_tiles * edge_w;
    let edge_y_top = py;
    let edge_y_bot = py + ph - edge_h;
    let mut x = px + cw;
    for _ in 0..full_tiles {
        push(out, rects.panel_top, x, edge_y_top, edge_w, edge_h);
        push(out, rects.panel_bot, x, edge_y_bot, edge_w, edge_h);
        x += edge_w;
    }
    if remainder > 0 {
        let (ux, uy, _, uh) = rects.panel_top;
        let top_rem = (ux, uy, remainder as u32, uh);
        let (bx, by, _, bh) = rects.panel_bot;
        let bot_rem = (bx, by, remainder as u32, bh);
        push(out, top_rem, x, edge_y_top, remainder, edge_h);
        push(out, bot_rem, x, edge_y_bot, remainder, edge_h);
    }

    // Left + right edges. The source tile is 4x21; tile vertically
    // with a remainder for taller-than-21 interiors.
    let vert_span = ph - 2 * ch;
    let v_full = vert_span / v_edge_h;
    let v_rem = vert_span - v_full * v_edge_h;
    let mut y = py + ch;
    for _ in 0..v_full {
        push(out, rects.panel_left, px, y, cw, v_edge_h);
        push(out, rects.panel_right, px + pw - cw, y, cw, v_edge_h);
        y += v_edge_h;
    }
    if v_rem > 0 {
        let (lx, ly, lw, _) = rects.panel_left;
        let left_rem = (lx, ly, lw, v_rem as u32);
        let (rx, ry, rw, _) = rects.panel_right;
        let right_rem = (rx, ry, rw, v_rem as u32);
        push(out, left_rem, px, y, cw, v_rem);
        push(out, right_rem, px + pw - cw, y, cw, v_rem);
    }
}

/// Retail PSX framebuffer placement of the "Now checking" dialog panel.
/// Pinned via gold-border pixel scan on
/// `captures/slot_info_dump/2026-05-18T09-04-46Z/slot_info_fb.png`:
/// dialog gold borders at fb-y rows 97 (top) and 135 (bottom), spanning
/// fb-x 70..249 (width 180, height 39). The dialog is horizontally
/// centered on the 320-wide stage (`(320 - 180) / 2 = 70`).
pub const NOW_CHECKING_PANEL_POS: (i32, i32) = (70, 97);
pub const NOW_CHECKING_PANEL_SIZE: (u32, u32) = (180, 39);

/// Retail slide-in start position for the "Now checking" dialog's
/// **center x** before it has slid into place. From Ghidra trace
/// `FUN_801e1c1c(0, DAT_801ef160, 0x1a0, 0x70, 0xa0, 0x70)` — slide
/// from `(0x1a0=416, 0x70=112)` to target `(0xa0=160, 0x70=112)`. The
/// dialog starts off-screen to the right and slides left over 16
/// frames. Engine code interpolates `slide_offset_x = (start - target) *
/// (1 - t/4096)`, where `t = session.slide_anim_t()`.
pub const NOW_CHECKING_SLIDE_START_X: i32 = 416;
pub const NOW_CHECKING_SLIDE_TARGET_X: i32 = 160;

/// Center X used by retail's dialog renderer for every messagebox
/// text line. Pinned via Ghidra: every `FUN_801E3EE0(string, x, y)`
/// call in `overlay_save_ui_select_801dd35c.txt` passes
/// `x = 0xA0 = 160` (= stage horizontal center) and renders the
/// glyphs at `(x - text_width/2, y + 7)`. The +7 offset is baked
/// into the renderer itself (see `overlay_menu_801e3ee0.txt`).
pub const DIALOG_TEXT_CENTER_X: i32 = 160;
/// "Now checking." line: retail `FUN_801E3EE0(string, 0xA0, 0x60)`
/// → text top y = 0x60 + 7 = 103. Source:
/// `overlay_save_ui_select_801dd35c.txt:1054`.
pub const NOW_CHECKING_TEXT_LINE1_Y: i32 = 103;
/// "Do not remove MEMORY CARD" line: retail
/// `FUN_801E3EE0(string, 0xA0, 0x70)` → text top y = 0x70 + 7 = 119.
/// Source: `overlay_save_ui_select_801dd35c.txt:809`.
pub const NOW_CHECKING_TEXT_LINE2_Y: i32 = 119;
/// Backwards-compat: left-edge positions derived from
/// `center_x - retail_text_width / 2` for the two lines (computed
/// at runtime in `now_checking_text_draws_for` from the actual
/// font metrics). Kept as inert constants for callers that don't
/// have a font reference handy.
pub const NOW_CHECKING_TEXT_LINE1: (i32, i32) = (122, NOW_CHECKING_TEXT_LINE1_Y);
pub const NOW_CHECKING_TEXT_LINE2: (i32, i32) = (78, NOW_CHECKING_TEXT_LINE2_Y);

/// Build [`SpriteDraw`]s for the "Now checking" dialog's 9-slice
/// panel only (no text). `slide_offset` is added to the panel
/// position so callers can drive the retail slide-in animation
/// (Ghidra-pinned: dialog slides from x=416 to x=160 over 16 frames
/// via `FUN_801E1C1C` mode 0). Pass `(0, 0)` for the static
/// fully-arrived case.
pub fn now_checking_panel_draws_for(
    rects: &SaveMenuAtlasRects,
    stage_origin: (i32, i32),
    stage_scale: u32,
    slide_offset: (i32, i32),
) -> Vec<SpriteDraw> {
    let mut out = Vec::with_capacity(16);
    let (px, py) = NOW_CHECKING_PANEL_POS;
    let (pw, ph) = NOW_CHECKING_PANEL_SIZE;
    nine_slice_panel_into(
        &mut out,
        rects,
        (
            px + slide_offset.0,
            py + slide_offset.1,
            pw as i32,
            ph as i32,
        ),
        stage_origin,
        stage_scale,
    );
    out
}

/// Build [`TextDraw`]s for the "Now checking. Do not remove MEMORY
/// CARD" two-line dialog text. Each line is **horizontally centered
/// on stage x = [`DIALOG_TEXT_CENTER_X`]** matching retail's
/// `FUN_801E3EE0(string, center_x, top_y)` renderer
/// (`overlay_menu_801e3ee0.txt`), with the layout's left edge
/// computed as `center_x - text_width / 2` from the actual font
/// metrics rather than hard-coded.
pub fn now_checking_text_draws_for(
    font: &legaia_font::Font,
    stage_origin: (i32, i32),
    stage_scale: u32,
    slide_offset: (i32, i32),
) -> Vec<TextDraw> {
    let scale = stage_scale.max(1);
    let color = SAVE_SELECT_TITLE_COLOR;
    let mut out = Vec::with_capacity(40);

    let emit_centered = |out: &mut Vec<TextDraw>, text: &str, top_y: i32| {
        let layout = font.layout_ascii(text);
        let left_x = DIALOG_TEXT_CENTER_X - (layout.advance_x as i32 / 2) + slide_offset.0;
        let top_y = top_y + slide_offset.1;
        for g in &layout.glyphs {
            let sx = left_x + g.dst_x;
            let sy = top_y + g.dst_y;
            out.push(TextDraw {
                dst: (
                    stage_origin.0 + sx * scale as i32,
                    stage_origin.1 + sy * scale as i32,
                    g.width * scale,
                    g.height * scale,
                ),
                src: (g.atlas_x, g.atlas_y, g.width, g.height),
                color,
            });
        }
    };

    emit_centered(&mut out, "Now checking.", NOW_CHECKING_TEXT_LINE1_Y);
    emit_centered(
        &mut out,
        "Do not remove MEMORY CARD",
        NOW_CHECKING_TEXT_LINE2_Y,
    );
    out
}

/// Retail PSX framebuffer placement of the slot-preview 5×3 grid.
/// Mirror of `legaia_asset::title_pak::OVERLAY_LOAD_SLOT_GRID_*` —
/// retail-pinned via per-row/per-column blue-outline scan on
/// `slot_info_fb.png`: cell visible top-left corners at fb-y rows
/// 35 (row 0), 55 (row 1), 75 (row 2) and fb-x columns 104, 144,
/// 184, 224, 264 (col 0..4). Pitch X = 40, pitch Y = 20.
pub const SLOT_GRID_ORIGIN: (i32, i32) = (104, 35);
pub const SLOT_GRID_PITCH_X: i32 = 40;
pub const SLOT_GRID_PITCH_Y: i32 = 20;
pub const SLOT_GRID_COLS: usize = 5;
pub const SLOT_GRID_ROWS: usize = 3;

/// Per-cell view passed into [`slot_preview_grid_draws_for`]. Each
/// memory-card block becomes one cell; `present=false` cells render
/// as plain empty frames. When a save is present, `portrait_char_id`
/// (= lead party member's char_id) selects which 16×16 portrait
/// (0=Vahn, 1=Noa, 2=Gala) is drawn inside the frame; `None` falls
/// back to the empty frame.
#[derive(Debug, Clone, Copy, Default)]
pub struct SlotGridCell {
    pub present: bool,
    pub portrait_char_id: Option<u8>,
}

/// Build [`SpriteDraw`]s for the 5×3 slot-preview grid. Each cell
/// gets the empty-frame sprite (32×32 with 20×20 visible border).
/// Filled cells additionally get a 16×16 portrait centred in the
/// frame. The cursor sprite sits to the left of the currently
/// selected cell.
pub fn slot_preview_grid_draws_for(
    rects: &SaveMenuAtlasRects,
    cells: &[SlotGridCell],
    cursor_slot: u8,
    stage_origin: (i32, i32),
    stage_scale: u32,
) -> Vec<SpriteDraw> {
    let scale = stage_scale.max(1) as i32;
    let white = [1.0, 1.0, 1.0, 1.0];
    let mut out = Vec::with_capacity(SLOT_GRID_COLS * SLOT_GRID_ROWS + 2);

    let push = |out: &mut Vec<SpriteDraw>,
                src: (u32, u32, u32, u32),
                sx: i32,
                sy: i32,
                sw: i32,
                sh: i32| {
        out.push(SpriteDraw {
            dst: (
                stage_origin.0 + sx * scale,
                stage_origin.1 + sy * scale,
                (sw as u32) * scale as u32,
                (sh as u32) * scale as u32,
            ),
            src,
            color: white,
        });
    };

    let max_slots = (SLOT_GRID_COLS * SLOT_GRID_ROWS).min(cells.len());
    for (slot, cell) in cells.iter().take(max_slots).enumerate() {
        let col = slot % SLOT_GRID_COLS;
        let row = slot / SLOT_GRID_COLS;
        // Empty-frame sprite top-left in stage pixels. The 32×32
        // sprite has a 6px transparent margin; the visible 20×20
        // frame's top-left should land at (origin.x + col*pitch_x,
        // origin.y + row*pitch_y). So sprite origin = grid pos - 6.
        let cell_x = SLOT_GRID_ORIGIN.0 + (col as i32) * SLOT_GRID_PITCH_X;
        let cell_y = SLOT_GRID_ORIGIN.1 + (row as i32) * SLOT_GRID_PITCH_Y;
        if let Some(frame) = rects.load_empty_frame {
            // The full 32×32 sprite is drawn with its top-left at
            // (cell_x - 6, cell_y - 6) so the visible 20×20 border
            // sits at the cell position. Engines may instead sample
            // sub-rect (6, 6, 20, 20) and skip the margin — both
            // produce the same on-screen pixels.
            push(&mut out, frame, cell_x - 6, cell_y - 6, 32, 32);
        }
        if cell.present
            && let Some(char_id) = cell.portrait_char_id
            && let Some(portrait) = rects
                .load_portrait_by_char
                .get(char_id as usize)
                .copied()
                .flatten()
        {
            // Portrait centred inside the 20×20 visible frame
            // (16×16 portrait + 2px margin each side).
            push(&mut out, portrait, cell_x + 2, cell_y + 2, 16, 16);
        }
    }

    // Cursor sprite to the left of the currently-selected cell.
    // Retail pin: in `slot_info_fb.png` the pointing-finger cursor
    // bbox sits at fb-x 90..105 (16 wide) pointing at cell (0, 0) at
    // fb-x 104. That puts the cursor's right edge 1 px shy of the
    // cell's left edge — i.e. `cursor_x = cell_x - 14` (not -16).
    let cursor_col = (cursor_slot as usize) % SLOT_GRID_COLS;
    let cursor_row = (cursor_slot as usize) / SLOT_GRID_COLS;
    let cursor_x = SLOT_GRID_ORIGIN.0 + (cursor_col as i32) * SLOT_GRID_PITCH_X - 14;
    let cursor_y = SLOT_GRID_ORIGIN.1 + (cursor_row as i32) * SLOT_GRID_PITCH_Y;
    push(&mut out, rects.cursor, cursor_x, cursor_y, 16, 16);

    out
}

/// Retail PSX framebuffer placement of the slot-info panel (bottom
/// of stage), parked / fully-slid-in. Pinned to FUN_801E08D8's
/// `FUN_801e36c4(0xA0, local_34, 0x11c, 0x40)` call: panel chrome
/// top at `local_34` (= 138 when `DAT_801ef1a0 = 0x1000`), width 293,
/// height 77. Matches the visual gold-border scan in `slot_info_fb.png`.
pub const SLOT_INFO_PANEL_POS: (i32, i32) = (11, 138);
pub const SLOT_INFO_PANEL_SIZE: (u32, u32) = (293, 77);
/// Panel-y origin when fully slid-in (= `local_34` at anim_t=0x1000).
pub const SLOT_INFO_PANEL_PARKED_Y: i32 = 138;

/// Per-element offsets relative to the panel-y origin (= `local_34`),
/// derived from the Ghidra trace of `FUN_801E08D8`. The renderer adds
/// the live `panel_y` (interpolated through the slide-in animation) to
/// every offset.
///
/// Title-row offsets (`local_34 + 4` in retail = panel_y + 4):
/// - `SLOT_INFO_NO_OFFSET`: "No." badge (retail emits a sprite via
///   `FUN_801E3FF0` modes 2/3 at `(8, local_34 - 8)` with a CLUT row
///   selected by `DAT_801e5062 = slot_index << 4`. The engine renders
///   it as text at the same screen position, glyph-baseline corrected).
/// - `SLOT_INFO_LOCATION_OFFSET`: kingdom name string.
/// - `SLOT_INFO_TIME_LABEL_OFFSET`: "Time " prefix.
/// - `SLOT_INFO_TIME_VALUE_OFFSET`: HH:MM:SS digits. Retail splits the
///   digits across three calls (hours / minutes / seconds, with
///   sprite-colon separators) at x=236/252/260/276/284; the engine
///   renders one packed string at the leftmost x for simplicity.
pub const SLOT_INFO_NO_OFFSET: (i32, i32) = (8, -8);
pub const SLOT_INFO_LOCATION_OFFSET: (i32, i32) = (48, 4);
pub const SLOT_INFO_TIME_LABEL_OFFSET: (i32, i32) = (208, 4);
pub const SLOT_INFO_TIME_VALUE_OFFSET: (i32, i32) = (236, 4);

/// Per-character row offsets (column 0 of the 3-column slot grid;
/// retail loops `iVar4 = 0x10 + i*0x60` for columns 0/1/2 at
/// x = 16/112/208). For slots with one party member (Vahn-only starter
/// state) only column 0 renders. Y offsets relative to the panel y
/// origin via the retail `s3 = local_34 + 0x14` (= 158) per-character
/// base, then `s3 + N` for each row.
pub const SLOT_INFO_PORTRAIT_OFFSET: (i32, i32) = (16, 16);
pub const SLOT_INFO_NAME_OFFSET: (i32, i32) = (40, 20);
pub const SLOT_INFO_LV_LABEL_OFFSET: (i32, i32) = (16, 33);
pub const SLOT_INFO_LV_VALUE_OFFSET: (i32, i32) = (48, 33);
pub const SLOT_INFO_HP_LABEL_OFFSET: (i32, i32) = (16, 46);
pub const SLOT_INFO_HP_VALUE_OFFSET: (i32, i32) = (32, 46);
pub const SLOT_INFO_MP_LABEL_OFFSET: (i32, i32) = (16, 59);
pub const SLOT_INFO_MP_VALUE_OFFSET: (i32, i32) = (40, 59);

/// Plain-data view of the per-slot info passed to the info-panel
/// renderer. Engines build one from the `SlotSnapshot` of the
/// currently-focused slot plus a `Party::from_retail_sc_block` lift
/// for the leader's HP/MP.
#[derive(Debug, Clone, Copy)]
pub struct SlotInfoView<'a> {
    pub slot_no: u8,
    pub location: &'a str,
    pub play_time: &'a str,
    pub leader_name: &'a str,
    pub leader_level: u8,
    pub leader_hp: (u16, u16),
    pub leader_mp: (u16, u16),
    pub leader_char_id: u8,
}

/// Build the chrome [`SpriteDraw`]s for the slot-info panel (9-slice
/// frame + optional leader portrait, no text). Pair with
/// [`slot_info_panel_text_draws_for`] for the labels.
///
/// `panel_y_offset` is the slide-in delta from the parked y
/// (positive = pushed below parked, used while the panel slides up).
/// Engine callers compute it from `interpolate_anim((0, OFFSCREEN),
/// (0, PARKED), session.info_panel_slide_anim_t()).1 - PARKED`.
pub fn slot_info_panel_draws_for(
    rects: &SaveMenuAtlasRects,
    info: Option<&SlotInfoView<'_>>,
    panel_y_offset: i32,
    stage_origin: (i32, i32),
    stage_scale: u32,
) -> Vec<SpriteDraw> {
    let mut out = Vec::with_capacity(20);
    let (px, py_base) = SLOT_INFO_PANEL_POS;
    let py = py_base + panel_y_offset;
    let (pw, ph) = SLOT_INFO_PANEL_SIZE;
    nine_slice_panel_into(
        &mut out,
        rects,
        (px, py, pw as i32, ph as i32),
        stage_origin,
        stage_scale,
    );

    // Leader portrait (16x16) inside the info panel — drawn only
    // when a save is present at the current slot. Position pinned
    // from FUN_801E08D8's `FUN_801e3ff0(0, _, iVar4=16, s3-4=154)`
    // with s3 = local_34 + 20: portrait top-left at (16, panel_y+16).
    if let Some(info) = info
        && let Some(portrait) = rects
            .load_portrait_by_char
            .get(info.leader_char_id as usize)
            .copied()
            .flatten()
    {
        let scale = stage_scale.max(1) as i32;
        let px = SLOT_INFO_PORTRAIT_OFFSET.0;
        let pyy = py_base + SLOT_INFO_PORTRAIT_OFFSET.1 + panel_y_offset;
        out.push(SpriteDraw {
            dst: (
                stage_origin.0 + px * scale,
                stage_origin.1 + pyy * scale,
                16 * scale as u32,
                16 * scale as u32,
            ),
            src: portrait,
            color: [1.0, 1.0, 1.0, 1.0],
        });
    }
    out
}

/// Build [`TextDraw`]s for the slot-info panel labels (No., kingdom
/// name, time, character stats). Returns empty when `info` is `None`.
/// `panel_y_offset` matches the value passed to
/// [`slot_info_panel_draws_for`].
pub fn slot_info_panel_text_draws_for(
    font: &legaia_font::Font,
    info: Option<&SlotInfoView<'_>>,
    panel_y_offset: i32,
    stage_origin: (i32, i32),
    stage_scale: u32,
) -> Vec<TextDraw> {
    let Some(info) = info else { return Vec::new() };
    let scale = stage_scale.max(1);
    let color = SAVE_SELECT_TITLE_COLOR;
    let panel_y = SLOT_INFO_PANEL_PARKED_Y + panel_y_offset;
    let mut out = Vec::with_capacity(80);

    let emit_at = |out: &mut Vec<TextDraw>, text: &str, base: (i32, i32)| {
        let layout = font.layout_ascii(text);
        for g in &layout.glyphs {
            let sx = base.0 + g.dst_x;
            let sy = base.1 + g.dst_y;
            out.push(TextDraw {
                dst: (
                    stage_origin.0 + sx * scale as i32,
                    stage_origin.1 + sy * scale as i32,
                    g.width * scale,
                    g.height * scale,
                ),
                src: (g.atlas_x, g.atlas_y, g.width, g.height),
                color,
            });
        }
    };

    // Title row.
    emit_at(
        &mut out,
        &format!("No.{}", info.slot_no),
        (SLOT_INFO_NO_OFFSET.0, panel_y + SLOT_INFO_NO_OFFSET.1),
    );
    emit_at(
        &mut out,
        info.location,
        (
            SLOT_INFO_LOCATION_OFFSET.0,
            panel_y + SLOT_INFO_LOCATION_OFFSET.1,
        ),
    );
    emit_at(
        &mut out,
        "Time",
        (
            SLOT_INFO_TIME_LABEL_OFFSET.0,
            panel_y + SLOT_INFO_TIME_LABEL_OFFSET.1,
        ),
    );
    emit_at(
        &mut out,
        info.play_time,
        (
            SLOT_INFO_TIME_VALUE_OFFSET.0,
            panel_y + SLOT_INFO_TIME_VALUE_OFFSET.1,
        ),
    );

    // Character row (column 0 only — multi-character party expansion
    // would re-iterate at base_x += 96).
    emit_at(
        &mut out,
        info.leader_name,
        (SLOT_INFO_NAME_OFFSET.0, panel_y + SLOT_INFO_NAME_OFFSET.1),
    );
    emit_at(
        &mut out,
        "LV",
        (
            SLOT_INFO_LV_LABEL_OFFSET.0,
            panel_y + SLOT_INFO_LV_LABEL_OFFSET.1,
        ),
    );
    emit_at(
        &mut out,
        &format!("{}", info.leader_level),
        (
            SLOT_INFO_LV_VALUE_OFFSET.0,
            panel_y + SLOT_INFO_LV_VALUE_OFFSET.1,
        ),
    );
    emit_at(
        &mut out,
        "HP",
        (
            SLOT_INFO_HP_LABEL_OFFSET.0,
            panel_y + SLOT_INFO_HP_LABEL_OFFSET.1,
        ),
    );
    emit_at(
        &mut out,
        &format!("{}/{}", info.leader_hp.0, info.leader_hp.1),
        (
            SLOT_INFO_HP_VALUE_OFFSET.0,
            panel_y + SLOT_INFO_HP_VALUE_OFFSET.1,
        ),
    );
    emit_at(
        &mut out,
        "MP",
        (
            SLOT_INFO_MP_LABEL_OFFSET.0,
            panel_y + SLOT_INFO_MP_LABEL_OFFSET.1,
        ),
    );
    emit_at(
        &mut out,
        &format!("{}/{}", info.leader_mp.0, info.leader_mp.1),
        (
            SLOT_INFO_MP_VALUE_OFFSET.0,
            panel_y + SLOT_INFO_MP_VALUE_OFFSET.1,
        ),
    );
    out
}

/// Build [`TextDraw`]s for the save-select panel.
///
/// Retail layout (positions in stage pixels — pairs with
/// [`save_select_chrome_draws_for`] for the panel / pill sprites):
/// ```text
///   ┌──────────┐
///   │   Load   │           ← title word centered inside panel
///   └──────────┘
///                  SLOT 1  ← pill (sprite, label baked in)
///              >   SLOT 2  ← cursor arrow points at selected pill
/// ```
///
/// The function emits ONLY text: the panel/pill chrome lives on the
/// sprite-overlay layer via [`save_select_chrome_draws_for`]. The
/// `font`'s tinted glyphs are used for the title word, the cursor
/// arrow, and confirm-prompt overlays.
///
/// `cursor` selects the highlighted row. When `confirm` is `Some`,
/// the Yes/No prompt is rendered below the slot stack with the
/// highlighted option determined by the second tuple element (0 =
/// Yes, 1 = No).
///
/// `rows` is retained for API compatibility — the row count and
/// label strings drive cursor placement; per-slot Lv/play-time/gold
/// details are deliberately not rendered (retail's load screen
/// surfaces those on a separate sub-screen).
/// `emit_text_cursor` controls whether to emit an ASCII `>` cursor
/// glyph next to the active pill. When the sprite cursor (the
/// pointing-finger from the system-UI TIM) is being emitted by
/// [`save_select_cursor_draw_for`] alongside this call, pass `false`
/// to avoid drawing both. When the save-menu atlas isn't available
/// (no disc / atlas build failed), pass `true` to fall back to the
/// text-glyph cursor so the player still sees a visual selection
/// indicator.
#[allow(clippy::too_many_arguments)]
pub fn save_select_draws_for(
    font: &legaia_font::Font,
    title: &str,
    rows: &[SaveSelectRow<'_>],
    cursor: usize,
    confirm: Option<(&str, u8)>,
    stage_origin: (i32, i32),
    stage_scale: u32,
    emit_text_cursor: bool,
) -> Vec<TextDraw> {
    const LINE_H: i32 = 14;
    let scale = stage_scale.max(1);
    let white: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
    let gold: [f32; 4] = [1.0, 0.85, 0.3, 1.0];

    let mut out = Vec::new();

    // Helper: emit a layout's glyphs scaled by `glyph_scale` (in
    // addition to the stage origin/scale already applied to `pen`).
    // Used for the panel title + cursor so they read large enough to
    // match the chrome's stage_scale (the chrome sprites are blitted
    // at stage_scale × source pixels, so glyphs should follow).
    let emit_scaled = |out: &mut Vec<TextDraw>,
                       layout: &legaia_font::Layout,
                       pen_stage: (i32, i32),
                       glyph_scale: u32,
                       color: [f32; 4]| {
        let gs = glyph_scale as i32;
        let pen_screen = (
            stage_origin.0 + pen_stage.0 * scale as i32,
            stage_origin.1 + pen_stage.1 * scale as i32,
        );
        for g in &layout.glyphs {
            out.push(TextDraw {
                dst: (
                    pen_screen.0 + g.dst_x * gs,
                    pen_screen.1 + g.dst_y * gs,
                    g.width * glyph_scale,
                    g.height * glyph_scale,
                ),
                src: (g.atlas_x, g.atlas_y, g.width, g.height),
                color,
            });
        }
    };

    // Title word ("Load" / "Save") drawn from the dialog-font
    // stencil. Retail emits one textured-sprite primitive per glyph
    // from the VRAM-resident dialog font at byte-pinned dst
    // positions starting at `SAVE_SELECT_TITLE_POS`, sampling the
    // bright-text CLUT entry `SAVE_SELECT_TITLE_COLOR` at VRAM
    // `(208, 510)`. The engine font's layout (variable advances
    // from `dialog_font_widths.csv` + `INTER_GLYPH_PAD = 1`) is
    // byte-equal to retail's per-glyph dst deltas, so a plain
    // `font.layout_ascii` placed at the pinned origin lines up 1:1.
    //
    // Unit discipline: the layout runs in STAGE pixels and
    // `glyph_screen_scale = stage_scale`, so each engine glyph
    // pixel becomes exactly `stage_scale` screen pixels — matching
    // the chrome sprites composed on the same 320x240 stage.
    let title_l = font.layout_ascii(title);
    emit_scaled(
        &mut out,
        &title_l,
        SAVE_SELECT_TITLE_POS,
        scale,
        SAVE_SELECT_TITLE_COLOR,
    );

    // Cursor arrow next to the selected slot pill. Pills sit at
    // SAVE_SELECT_SLOT1_POS + i*SAVE_SELECT_SLOT_PITCH_Y; arrow goes
    // to the left of the pill by SAVE_SELECT_CURSOR_X_OFFSET.
    // Skipped when the caller is also emitting the sprite cursor.
    if emit_text_cursor && !rows.is_empty() {
        let cursor_row = cursor.min(rows.len().saturating_sub(1));
        let cur_layout = font.layout_ascii(">");
        let cx = SAVE_SELECT_SLOT1_POS.0 + SAVE_SELECT_CURSOR_X_OFFSET;
        let cy = SAVE_SELECT_SLOT1_POS.1 + (cursor_row as i32) * SAVE_SELECT_SLOT_PITCH_Y;
        emit_scaled(&mut out, &cur_layout, (cx, cy), scale, gold);
    }

    if let Some((prompt, c_cursor)) = confirm {
        // Confirm prompt sits below the pill stack. Each row is
        // SAVE_SELECT_SLOT_PITCH_Y tall.
        let n = rows.len() as i32;
        let prompt_y = SAVE_SELECT_SLOT1_POS.1 + n * SAVE_SELECT_SLOT_PITCH_Y + LINE_H;
        let p_l = font.layout_ascii(prompt);
        out.extend(text_draws_for(
            &p_l,
            (
                stage_origin.0 + SAVE_SELECT_SLOT1_POS.0 * scale as i32,
                stage_origin.1 + prompt_y * scale as i32,
            ),
            white,
        ));
        for (i, opt) in ["Yes", "No"].iter().enumerate() {
            let x = SAVE_SELECT_SLOT1_POS.0 + 12 + i as i32 * 32;
            let y = prompt_y + LINE_H;
            let color = if i as u8 == c_cursor { gold } else { white };
            if i as u8 == c_cursor {
                let cur = font.layout_ascii(">");
                out.extend(text_draws_for(
                    &cur,
                    (
                        stage_origin.0 + (x - 8) * scale as i32,
                        stage_origin.1 + y * scale as i32,
                    ),
                    color,
                ));
            }
            let l = font.layout_ascii(opt);
            out.extend(text_draws_for(
                &l,
                (
                    stage_origin.0 + x * scale as i32,
                    stage_origin.1 + y * scale as i32,
                ),
                color,
            ));
        }
    }

    out
}

/// Build [`TextDraw`]s for the encounter transition banner.
///
/// Drawn during [`crate::EncounterPhase::Transition`] (where the engine
/// type is `legaia_engine_core::encounter::EncounterPhase`). Renders a
/// large centered "ENCOUNTER!" line plus the formation label below.
/// Engines fade the surface independently - this just produces the
/// glyph draws.
pub fn encounter_banner_draws_for(
    font: &legaia_font::Font,
    formation_label: &str,
    pen: (i32, i32),
) -> Vec<TextDraw> {
    const LINE_H: i32 = 16;
    let yellow: [f32; 4] = [1.0, 0.9, 0.3, 1.0];
    let white: [f32; 4] = [1.0, 1.0, 1.0, 1.0];

    let mut out = Vec::new();
    let head = font.layout_ascii("ENCOUNTER!");
    out.extend(text_draws_for(&head, pen, yellow));
    if !formation_label.is_empty() {
        let body = font.layout_ascii(formation_label);
        out.extend(text_draws_for(&body, (pen.0, pen.1 + LINE_H), white));
    }
    out
}

/// Plain-data row for the field-menu draw. Engines build these from
/// `engine_core::field_menu::FieldMenuView::rows` so this crate doesn't
/// depend on engine-core.
pub struct FieldMenuRowView<'a> {
    pub label: &'a str,
    pub enabled: bool,
}

/// Build [`TextDraw`]s for the field (pause) menu panel. `cursor` is the
/// row index; greyed-out rows render dim. The corner badges show money
/// and the H:MM:SS play-time.
pub fn field_menu_draws_for(
    font: &legaia_font::Font,
    rows: &[FieldMenuRowView<'_>],
    cursor: u8,
    money: u32,
    play_time_seconds: u32,
    pen: (i32, i32),
) -> Vec<TextDraw> {
    const LINE_H: i32 = 16;
    let white: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
    let dim: [f32; 4] = [0.45, 0.45, 0.45, 1.0];
    let gold: [f32; 4] = [1.0, 0.85, 0.3, 1.0];

    let mut out = Vec::new();
    let title = font.layout_ascii("MENU");
    out.extend(text_draws_for(&title, pen, white));

    for (i, row) in rows.iter().enumerate() {
        let y = pen.1 + LINE_H + i as i32 * LINE_H;
        let selected = i as u8 == cursor;
        let color = if !row.enabled {
            dim
        } else if selected {
            gold
        } else {
            white
        };
        if selected && row.enabled {
            let cur = font.layout_ascii(">");
            out.extend(text_draws_for(&cur, (pen.0, y), color));
        }
        let l = font.layout_ascii(row.label);
        out.extend(text_draws_for(&l, (pen.0 + 14, y), color));
    }

    let foot_y = pen.1 + LINE_H + rows.len() as i32 * LINE_H + LINE_H;
    let g = format!("{}G", money);
    let g_l = font.layout_ascii(&g);
    out.extend(text_draws_for(&g_l, (pen.0, foot_y), white));
    let h = play_time_seconds / 3600;
    let m = (play_time_seconds % 3600) / 60;
    let s = play_time_seconds % 60;
    let t = format!("{h:02}:{m:02}:{s:02}");
    let t_l = font.layout_ascii(&t);
    out.extend(text_draws_for(&t_l, (pen.0 + 110, foot_y), white));

    out
}

/// One stat row for the status screen.
pub struct StatusStatRow<'a> {
    pub label: &'a str,
    pub value: u32,
}

/// Plain-data view of a single character's status panel.
pub struct StatusPanelView<'a> {
    pub name: &'a str,
    pub level: u8,
    pub xp: u32,
    pub xp_to_next: u32,
    pub hp: u16,
    pub hp_max: u16,
    pub mp: u16,
    pub mp_max: u16,
    pub ap: u8,
    pub ap_max: u8,
    pub stat_rows: &'a [StatusStatRow<'a>],
    pub equip_rows: &'a [(&'a str, &'a str)],
}

/// Build [`TextDraw`]s for the status panel of one character. `nav_hint`
/// is rendered in the bottom-right corner ("L1/R1: Switch  Circle: Back")
/// and is `None` when the engine renders the hint elsewhere.
pub fn status_screen_draws_for(
    font: &legaia_font::Font,
    panel: &StatusPanelView<'_>,
    nav_hint: Option<&str>,
    pen: (i32, i32),
) -> Vec<TextDraw> {
    const LINE_H: i32 = 14;
    let white: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
    let gold: [f32; 4] = [1.0, 0.85, 0.3, 1.0];
    let dim: [f32; 4] = [0.6, 0.6, 0.6, 1.0];

    let mut out = Vec::new();
    let head = format!("{}  Lv.{}", panel.name, panel.level);
    out.extend(text_draws_for(&font.layout_ascii(&head), pen, gold));

    let xp_line = format!("XP {} / {}", panel.xp, panel.xp_to_next);
    out.extend(text_draws_for(
        &font.layout_ascii(&xp_line),
        (pen.0, pen.1 + LINE_H),
        white,
    ));

    let hpmp = format!(
        "HP {:>4} / {:<4}   MP {:>3} / {:<3}   AP {:>2} / {:<2}",
        panel.hp, panel.hp_max, panel.mp, panel.mp_max, panel.ap, panel.ap_max
    );
    out.extend(text_draws_for(
        &font.layout_ascii(&hpmp),
        (pen.0, pen.1 + LINE_H * 2),
        white,
    ));

    for (i, sr) in panel.stat_rows.iter().enumerate() {
        let y = pen.1 + LINE_H * 4 + i as i32 * LINE_H;
        let line = format!("{:<8} {:>3}", sr.label, sr.value);
        out.extend(text_draws_for(&font.layout_ascii(&line), (pen.0, y), white));
    }
    let after_stats_y = pen.1 + LINE_H * 4 + panel.stat_rows.len() as i32 * LINE_H + LINE_H;
    out.extend(text_draws_for(
        &font.layout_ascii("Equipment"),
        (pen.0, after_stats_y),
        gold,
    ));
    for (i, (slot, item)) in panel.equip_rows.iter().enumerate() {
        let y = after_stats_y + LINE_H + i as i32 * LINE_H;
        let line = format!("{:<10} {}", slot, item);
        out.extend(text_draws_for(&font.layout_ascii(&line), (pen.0, y), white));
    }

    if let Some(hint) = nav_hint {
        let after_equip_y =
            after_stats_y + LINE_H + panel.equip_rows.len() as i32 * LINE_H + LINE_H;
        out.extend(text_draws_for(
            &font.layout_ascii(hint),
            (pen.0, after_equip_y),
            dim,
        ));
    }
    out
}

/// One row in the spell-menu list.
pub struct SpellRowView<'a> {
    pub name: &'a str,
    pub mp_cost: u8,
    pub admissible: bool,
}

/// One row in the spell-menu target picker.
pub struct SpellTargetView<'a> {
    pub name: &'a str,
    pub hp: u16,
    pub hp_max: u16,
    pub alive: bool,
}

/// Inputs for [`spell_menu_draws_for`]. Bundled so the function takes a
/// single payload struct instead of 12 positional arguments.
pub struct SpellMenuDrawArgs<'a> {
    pub party_names: &'a [&'a str],
    pub party_hp: &'a [(u16, u16)],
    pub party_mp: &'a [(u16, u16)],
    pub selected_caster: Option<u8>,
    pub spells: &'a [SpellRowView<'a>],
    pub selected_spell: Option<u8>,
    pub targets: &'a [SpellTargetView<'a>],
    pub selected_target: Option<u8>,
    /// Cursor row inside the active phase column.
    pub cursor: u8,
    /// `0` = caster column, `1` = spell column, `2` = target column.
    pub phase: u8,
}

/// Build [`TextDraw`]s for the field spell menu.
pub fn spell_menu_draws_for(
    font: &legaia_font::Font,
    args: SpellMenuDrawArgs<'_>,
    pen: (i32, i32),
) -> Vec<TextDraw> {
    let SpellMenuDrawArgs {
        party_names,
        party_hp,
        party_mp,
        selected_caster,
        spells,
        selected_spell,
        targets,
        selected_target,
        cursor,
        phase,
    } = args;
    const LINE_H: i32 = 14;
    let white: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
    let gold: [f32; 4] = [1.0, 0.85, 0.3, 1.0];
    let dim: [f32; 4] = [0.55, 0.55, 0.55, 1.0];
    let red: [f32; 4] = [1.0, 0.55, 0.55, 1.0];

    let mut out = Vec::new();

    out.extend(text_draws_for(&font.layout_ascii("SPELLS"), pen, gold));

    // Caster column.
    for (i, name) in party_names.iter().enumerate() {
        let y = pen.1 + LINE_H + i as i32 * LINE_H;
        let selected_here = phase == 0 && i as u8 == cursor;
        let confirmed = selected_caster == Some(i as u8);
        let (cur_hp, _) = party_hp.get(i).copied().unwrap_or((0, 0));
        let (cur_mp, max_mp) = party_mp.get(i).copied().unwrap_or((0, 0));
        let alive = cur_hp > 0;
        let _ = confirmed;
        let color = if !alive {
            dim
        } else if selected_here {
            gold
        } else {
            white
        };
        if selected_here {
            out.extend(text_draws_for(&font.layout_ascii(">"), (pen.0, y), color));
        }
        let line = format!("{:<8} MP {:>3}/{:<3}", name, cur_mp, max_mp);
        out.extend(text_draws_for(
            &font.layout_ascii(&line),
            (pen.0 + 14, y),
            color,
        ));
    }

    // Spell column (when entered).
    if let Some(_caster) = selected_caster {
        let col_x = pen.0 + 200;
        for (i, sp) in spells.iter().enumerate() {
            let y = pen.1 + LINE_H + i as i32 * LINE_H;
            let selected_here = phase == 1 && i as u8 == cursor;
            let _ = selected_spell;
            let color = if !sp.admissible {
                dim
            } else if selected_here {
                gold
            } else {
                white
            };
            if selected_here {
                out.extend(text_draws_for(&font.layout_ascii(">"), (col_x, y), color));
            }
            let line = format!("{:<14} {:>3}MP", sp.name, sp.mp_cost);
            out.extend(text_draws_for(
                &font.layout_ascii(&line),
                (col_x + 14, y),
                color,
            ));
        }
    }

    // Target column (when entered).
    if let Some(_spell) = selected_spell {
        let col_x = pen.0 + 380;
        for (i, t) in targets.iter().enumerate() {
            let y = pen.1 + LINE_H + i as i32 * LINE_H;
            let selected_here = phase == 2 && i as u8 == cursor;
            let _ = selected_target;
            let color = if !t.alive {
                red
            } else if selected_here {
                gold
            } else {
                white
            };
            if selected_here {
                out.extend(text_draws_for(&font.layout_ascii(">"), (col_x, y), color));
            }
            let line = format!("{:<8} {:>4}/{:<4}", t.name, t.hp, t.hp_max);
            out.extend(text_draws_for(
                &font.layout_ascii(&line),
                (col_x + 14, y),
                color,
            ));
        }
    }
    out
}

/// Build [`TextDraw`]s for the game-over panel.
pub fn game_over_draws_for(
    font: &legaia_font::Font,
    cursor: u8,
    continue_enabled: bool,
    pen: (i32, i32),
) -> Vec<TextDraw> {
    const LINE_H: i32 = 16;
    let red: [f32; 4] = [1.0, 0.4, 0.4, 1.0];
    let white: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
    let dim: [f32; 4] = [0.4, 0.4, 0.4, 1.0];
    let gold: [f32; 4] = [1.0, 0.85, 0.3, 1.0];

    let mut out = Vec::new();
    out.extend(text_draws_for(&font.layout_ascii("GAME OVER"), pen, red));

    let rows = ["Continue", "Retry", "Quit"];
    for (i, row) in rows.iter().enumerate() {
        let y = pen.1 + LINE_H * 2 + i as i32 * LINE_H;
        let row_disabled = i == 0 && !continue_enabled;
        let color = if row_disabled {
            dim
        } else if i as u8 == cursor {
            gold
        } else {
            white
        };
        if i as u8 == cursor && !row_disabled {
            out.extend(text_draws_for(&font.layout_ascii(">"), (pen.0, y), color));
        }
        out.extend(text_draws_for(
            &font.layout_ascii(row),
            (pen.0 + 14, y),
            color,
        ));
    }
    out
}

/// One row in the options panel.
pub struct OptionsRowView<'a> {
    pub label: &'a str,
    pub value: &'a str,
}

/// Build [`TextDraw`]s for the options screen.
pub fn options_draws_for(
    font: &legaia_font::Font,
    rows: &[OptionsRowView<'_>],
    cursor: u8,
    pen: (i32, i32),
) -> Vec<TextDraw> {
    const LINE_H: i32 = 16;
    let white: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
    let gold: [f32; 4] = [1.0, 0.85, 0.3, 1.0];
    let dim: [f32; 4] = [0.6, 0.6, 0.6, 1.0];

    let mut out = Vec::new();
    out.extend(text_draws_for(&font.layout_ascii("CONFIG"), pen, gold));

    for (i, row) in rows.iter().enumerate() {
        let y = pen.1 + LINE_H * 2 + i as i32 * LINE_H;
        let color = if i as u8 == cursor { gold } else { white };
        if i as u8 == cursor {
            out.extend(text_draws_for(&font.layout_ascii(">"), (pen.0, y), color));
        }
        out.extend(text_draws_for(
            &font.layout_ascii(row.label),
            (pen.0 + 14, y),
            color,
        ));
        out.extend(text_draws_for(
            &font.layout_ascii(row.value),
            (pen.0 + 180, y),
            color,
        ));
    }
    out.extend(text_draws_for(
        &font.layout_ascii("Cross/Start: Save  Circle: Cancel"),
        (
            pen.0,
            pen.1 + LINE_H * 2 + rows.len() as i32 * LINE_H + LINE_H,
        ),
        dim,
    ));
    out
}

/// Build [`TextDraw`]s for the key-rebind panel. Each row shows a button
/// label paired with the currently-bound key string.
pub fn key_rebind_draws_for(
    font: &legaia_font::Font,
    rows: &[(&str, &str)],
    cursor: u8,
    awaiting: bool,
    pen: (i32, i32),
) -> Vec<TextDraw> {
    const LINE_H: i32 = 14;
    let white: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
    let gold: [f32; 4] = [1.0, 0.85, 0.3, 1.0];
    let dim: [f32; 4] = [0.6, 0.6, 0.6, 1.0];

    let mut out = Vec::new();
    out.extend(text_draws_for(&font.layout_ascii("KEY REBIND"), pen, gold));

    for (i, (button, key)) in rows.iter().enumerate() {
        let y = pen.1 + LINE_H * 2 + i as i32 * LINE_H;
        let selected = i as u8 == cursor;
        let color = if selected { gold } else { white };
        if selected {
            out.extend(text_draws_for(&font.layout_ascii(">"), (pen.0, y), color));
        }
        out.extend(text_draws_for(
            &font.layout_ascii(button),
            (pen.0 + 14, y),
            color,
        ));
        let value = if selected && awaiting { "..." } else { *key };
        out.extend(text_draws_for(
            &font.layout_ascii(value),
            (pen.0 + 100, y),
            color,
        ));
    }
    out.extend(text_draws_for(
        &font.layout_ascii("Cross: Bind  Circle: Cancel  Start: Save"),
        (
            pen.0,
            pen.1 + LINE_H * 2 + rows.len() as i32 * LINE_H + LINE_H,
        ),
        dim,
    ));
    out
}

/// Renderer-agnostic view of the name-entry overlay (engine-core's
/// `name_entry::NameEntry` projected to primitives, so this crate stays
/// decoupled from engine-core). Built per frame by the shell.
pub struct NameEntryView<'a> {
    /// The six selectable glyph rows (`|` marks a non-selectable separator).
    pub grid_rows: &'a [&'a str],
    /// Bottom control-bar button labels, left to right (e.g. Back / Space / End).
    pub control_labels: &'a [&'a str],
    /// Working name buffer.
    pub name: &'a str,
    /// Highlighted glyph cell `(row, col)` when the cursor is in the grid.
    pub grid_cursor: Option<(usize, usize)>,
    /// Highlighted control button index when the cursor is on the bar.
    pub control_cursor: Option<usize>,
    /// `true` while the "Is this name okay?" Yes/No prompt is showing.
    pub confirming: bool,
    /// Yes/No selection during the confirm prompt (`true` = Yes).
    pub confirm_yes: bool,
    /// Caret blink state (the trailing `_` after the name).
    pub caret_on: bool,
}

/// Build [`TextDraw`]s for the name-entry overlay - the screen the opening
/// `town01` field script opens so the player names the lead character.
///
/// Clean-room layout (the retail renderer is `FUN_801E6B34`): a heading +
/// working name with a blinking caret, the 6x17 character grid (separators
/// skipped), a control bar (Back / Space / End), and a Yes/No box while
/// confirming. Grid metrics: 14 px per column, 16 px per row.
///
/// REF: FUN_801E6B34
pub fn name_entry_draws_for(
    font: &legaia_font::Font,
    view: &NameEntryView<'_>,
    pen: (i32, i32),
) -> Vec<TextDraw> {
    const COL_W: i32 = 14;
    const ROW_H: i32 = 16;
    let white: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
    let gold: [f32; 4] = [1.0, 0.85, 0.3, 1.0];
    let dim: [f32; 4] = [0.6, 0.6, 0.6, 1.0];

    let mut out = Vec::new();
    out.extend(text_draws_for(
        &font.layout_ascii("Select your name."),
        pen,
        gold,
    ));

    // Working name + blinking caret.
    let name_y = pen.1 + ROW_H;
    out.extend(text_draws_for(
        &font.layout_ascii(view.name),
        (pen.0, name_y),
        white,
    ));
    if view.caret_on {
        let name_w = font.layout_ascii(view.name).advance_x as i32;
        out.extend(text_draws_for(
            &font.layout_ascii("_"),
            (pen.0 + name_w, name_y),
            white,
        ));
    }

    // Character grid.
    let grid_y0 = pen.1 + ROW_H * 3;
    for (r, row) in view.grid_rows.iter().enumerate() {
        for (c, ch) in row.bytes().enumerate() {
            if ch == b'|' || ch == b' ' {
                continue;
            }
            let selected = view.grid_cursor == Some((r, c));
            let color = if selected { gold } else { white };
            let x = pen.0 + c as i32 * COL_W;
            let y = grid_y0 + r as i32 * ROW_H;
            if selected {
                // Cursor bracket to the left of the highlighted glyph.
                out.extend(text_draws_for(
                    &font.layout_ascii(">"),
                    (x - COL_W / 2, y),
                    gold,
                ));
            }
            out.extend(text_draws_for(
                &font.layout_ascii(&(ch as char).to_string()),
                (x, y),
                color,
            ));
        }
    }

    // Control bar (Back / Space / End).
    let bar_y = grid_y0 + view.grid_rows.len() as i32 * ROW_H + ROW_H / 2;
    let mut bx = pen.0;
    for (i, label) in view.control_labels.iter().enumerate() {
        let selected = view.control_cursor == Some(i);
        let color = if selected { gold } else { white };
        if selected {
            out.extend(text_draws_for(
                &font.layout_ascii(">"),
                (bx - COL_W / 2, bar_y),
                gold,
            ));
        }
        out.extend(text_draws_for(
            &font.layout_ascii(label),
            (bx, bar_y),
            color,
        ));
        bx += font.layout_ascii(label).advance_x as i32 + COL_W;
    }

    // Confirm Yes/No box.
    if view.confirming {
        let cy = bar_y + ROW_H * 2;
        out.extend(text_draws_for(
            &font.layout_ascii("Is this name okay?"),
            (pen.0, cy),
            gold,
        ));
        let yes_color = if view.confirm_yes { gold } else { white };
        let no_color = if view.confirm_yes { white } else { gold };
        out.extend(text_draws_for(
            &font.layout_ascii("Yes"),
            (pen.0, cy + ROW_H),
            yes_color,
        ));
        out.extend(text_draws_for(
            &font.layout_ascii("No"),
            (pen.0 + 64, cy + ROW_H),
            no_color,
        ));
    } else {
        out.extend(text_draws_for(
            &font.layout_ascii("Cross: Select  Triangle: Back"),
            (
                pen.0,
                grid_y0 + view.grid_rows.len() as i32 * ROW_H + ROW_H * 2,
            ),
            dim,
        ));
    }

    out
}

/// Build [`TextDraw`]s for one opening-cutscene narration page (a single
/// subtitle line), horizontally centered at `center_x` with its baseline at
/// `top_y`, both in surface pixels. The host calls this with the active
/// page text from [`legaia_engine_core::cutscene_narration::CutsceneNarration`]
/// each frame; an empty / completed narration draws nothing.
///
/// Centering is computed from the font metrics (`center_x - width / 2`), the
/// same scheme [`now_checking_text_draws_for`] uses for retail-style centered
/// dialog. Subtitles are bottom-anchored by the caller's `top_y`.
pub fn cutscene_narration_draws_for(
    font: &legaia_font::Font,
    text: &str,
    center_x: i32,
    top_y: i32,
    color: [f32; 4],
) -> Vec<TextDraw> {
    if text.is_empty() {
        return Vec::new();
    }
    let layout = font.layout_ascii(text);
    let left_x = center_x - (layout.advance_x as i32 / 2);
    text_draws_for(&layout, (left_x, top_y), color)
}

/// One row in the inventory item-use list. Plain-data view so the
/// renderer doesn't depend on `engine-core::inventory_use`.
pub struct InventoryItemRow<'a> {
    pub name: &'a str,
    pub count: u8,
    /// `true` when the item passes the active context's filter
    /// (battle/field). Failing items still appear, dimmed.
    pub admissible: bool,
}

/// One row in the inventory item-use target picker.
pub struct InventoryTargetRow<'a> {
    pub name: &'a str,
    pub hp: u16,
    pub hp_max: u16,
    pub mp: u16,
    pub mp_max: u16,
    pub alive: bool,
}

/// Bundle of arguments for [`inventory_use_draws_for`]. Bundled so the
/// function takes one payload struct rather than ten positional args.
pub struct InventoryUseDrawArgs<'a> {
    pub items: &'a [InventoryItemRow<'a>],
    pub targets: &'a [InventoryTargetRow<'a>],
    /// `true` for battle context (target column shows monsters too);
    /// `false` for field (party only). Drives the title.
    pub in_battle: bool,
    /// Cursor row inside the active phase column.
    pub cursor: u8,
    /// `0` = item column, `1` = target column.
    pub phase: u8,
    /// Selected item id when in target phase. `None` while browsing.
    pub selected_item_name: Option<&'a str>,
}

/// Build [`TextDraw`]s for the inventory item-use overlay shared by the
/// field menu's Items row and the battle command-menu's Items option.
///
/// Layout (anchored at `pen`):
/// ```text
/// ITEMS
/// > Healing Leaf            x 04         | Vahn        HP 250/300
///   Magic Leaf              x 02         | Noa         HP 180/220
///   Antidote Leaf           x 01         | Gala        HP  90/280
///   ...                                  |
/// ```
///
/// The right-hand target column is only drawn when `phase == 1` (target
/// select). Failing items (filtered out by the active context) render
/// dimmed but stay visible so the player understands why their item
/// disappeared.
pub fn inventory_use_draws_for(
    font: &legaia_font::Font,
    args: InventoryUseDrawArgs<'_>,
    pen: (i32, i32),
) -> Vec<TextDraw> {
    const LINE_H: i32 = 14;
    let white: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
    let gold: [f32; 4] = [1.0, 0.85, 0.3, 1.0];
    let dim: [f32; 4] = [0.55, 0.55, 0.55, 1.0];
    let red: [f32; 4] = [1.0, 0.55, 0.55, 1.0];

    let mut out = Vec::new();

    let title = if args.in_battle { "ITEMS [B]" } else { "ITEMS" };
    out.extend(text_draws_for(&font.layout_ascii(title), pen, gold));

    if args.items.is_empty() {
        let l = font.layout_ascii("(no usable items)");
        out.extend(text_draws_for(&l, (pen.0, pen.1 + LINE_H), dim));
        return out;
    }

    // Item column.
    for (i, item) in args.items.iter().enumerate() {
        let y = pen.1 + LINE_H + i as i32 * LINE_H;
        let selected_here = args.phase == 0 && i as u8 == args.cursor;
        let color = if !item.admissible {
            dim
        } else if selected_here {
            gold
        } else {
            white
        };
        if selected_here {
            out.extend(text_draws_for(&font.layout_ascii(">"), (pen.0, y), color));
        }
        let line = format!("{:<20} x{:>3}", item.name, item.count);
        out.extend(text_draws_for(
            &font.layout_ascii(&line),
            (pen.0 + 14, y),
            color,
        ));
    }

    // Target column when picking a target.
    if args.phase == 1 {
        let col_x = pen.0 + 240;
        if let Some(name) = args.selected_item_name {
            let head = format!("Use: {name}");
            out.extend(text_draws_for(
                &font.layout_ascii(&head),
                (col_x, pen.1),
                gold,
            ));
        }
        for (i, t) in args.targets.iter().enumerate() {
            let y = pen.1 + LINE_H + i as i32 * LINE_H;
            let selected_here = i as u8 == args.cursor;
            let color = if !t.alive {
                red
            } else if selected_here {
                gold
            } else {
                white
            };
            if selected_here {
                out.extend(text_draws_for(&font.layout_ascii(">"), (col_x, y), color));
            }
            let line = if t.mp_max > 0 {
                format!(
                    "{:<8} HP {:>3}/{:<3} MP {:>3}/{:<3}",
                    t.name, t.hp, t.hp_max, t.mp, t.mp_max
                )
            } else {
                format!("{:<8} HP {:>3}/{:<3}", t.name, t.hp, t.hp_max)
            };
            out.extend(text_draws_for(
                &font.layout_ascii(&line),
                (col_x + 14, y),
                color,
            ));
        }
        if args.targets.is_empty() {
            let l = font.layout_ascii("(no targets)");
            out.extend(text_draws_for(&l, (col_x, pen.1 + LINE_H), dim));
        }
    }
    out
}

/// One slot row in the equipment screen.
pub struct EquipSlotRow<'a> {
    pub label: &'a str,
    /// Currently-equipped item display name. "(empty)" for an unfilled
    /// slot.
    pub current_name: &'a str,
}

/// One candidate row in the per-slot item picker.
pub struct EquipCandidateRow<'a> {
    pub name: &'a str,
    pub count: u8,
    /// Stat preview delta vs. the current equipped item: positive deltas
    /// are tinted green, negatives red. Engines compute these by running
    /// `compute_battle_stats` once with the candidate id installed.
    pub atk_delta: i16,
    pub udf_delta: i16,
}

/// Phase tag for [`equipment_session_draws_for`]. Mirrors
/// `engine-core::equip_session::EquipState` without naming the enum so
/// the renderer doesn't pull engine-core in as a dependency.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EquipDrawPhase {
    /// Cursor on the slot grid.
    SlotPicker,
    /// Cursor on the candidate-item list for the active slot.
    ItemPicker,
    /// Yes/No confirmation prompt (`cursor` == 0 for Yes, 1 for No).
    Confirm,
}

/// Bundle for [`equipment_session_draws_for`].
pub struct EquipDrawArgs<'a> {
    /// Display name of the character being equipped.
    pub character_name: &'a str,
    pub slots: &'a [EquipSlotRow<'a>],
    /// Candidate items for the active slot. Empty in `SlotPicker` phase.
    pub candidates: &'a [EquipCandidateRow<'a>],
    pub phase: EquipDrawPhase,
    /// Cursor row inside the active phase column.
    pub cursor: u16,
    /// Active slot index (0..=7) when in `ItemPicker` / `Confirm`.
    pub active_slot: u8,
    /// Optional pending swap label rendered above the Yes/No prompt
    /// ("Equip Iron Sword?"). Only consumed in `Confirm`.
    pub confirm_label: Option<&'a str>,
}

/// Build [`TextDraw`]s for the equipment overlay shared by the field
/// menu's Equip row and the shop's "buy then equip" flow.
///
/// Layout (anchored at `pen`):
/// ```text
/// EQUIP - Vahn
/// > Weapon       Iron Sword
///   Helmet       Leather Cap
///   Body Armor   (empty)
///   ...
///                                      | Iron Sword     ATK +10
///                                      | Wood Sword     ATK +5
///                                      | (empty)
///   Equip Iron Sword?  Yes  No
/// ```
pub fn equipment_session_draws_for(
    font: &legaia_font::Font,
    args: EquipDrawArgs<'_>,
    pen: (i32, i32),
) -> Vec<TextDraw> {
    const LINE_H: i32 = 14;
    let white: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
    let gold: [f32; 4] = [1.0, 0.85, 0.3, 1.0];
    let dim: [f32; 4] = [0.55, 0.55, 0.55, 1.0];
    let green: [f32; 4] = [0.5, 1.0, 0.5, 1.0];
    let red: [f32; 4] = [1.0, 0.55, 0.55, 1.0];

    let mut out = Vec::new();

    let head = format!("EQUIP - {}", args.character_name);
    out.extend(text_draws_for(&font.layout_ascii(&head), pen, gold));

    // Slot column.
    for (i, slot) in args.slots.iter().enumerate() {
        let y = pen.1 + LINE_H + i as i32 * LINE_H;
        let cursor_here = args.phase == EquipDrawPhase::SlotPicker && i as u16 == args.cursor;
        let row_active = args.phase != EquipDrawPhase::SlotPicker && args.active_slot as usize == i;
        let color = if cursor_here || row_active {
            gold
        } else {
            white
        };
        if cursor_here {
            out.extend(text_draws_for(&font.layout_ascii(">"), (pen.0, y), color));
        }
        out.extend(text_draws_for(
            &font.layout_ascii(slot.label),
            (pen.0 + 14, y),
            color,
        ));
        let item_color = if slot.current_name == "(empty)" {
            dim
        } else {
            color
        };
        out.extend(text_draws_for(
            &font.layout_ascii(slot.current_name),
            (pen.0 + 110, y),
            item_color,
        ));
    }

    // Candidate column.
    if args.phase != EquipDrawPhase::SlotPicker {
        let col_x = pen.0 + 250;
        let head = if let Some(slot) = args.slots.get(args.active_slot as usize) {
            format!("> {}", slot.label)
        } else {
            "Slot".to_string()
        };
        out.extend(text_draws_for(
            &font.layout_ascii(&head),
            (col_x, pen.1),
            gold,
        ));

        if args.candidates.is_empty() {
            out.extend(text_draws_for(
                &font.layout_ascii("(no items)"),
                (col_x, pen.1 + LINE_H),
                dim,
            ));
        }
        for (i, c) in args.candidates.iter().enumerate() {
            let y = pen.1 + LINE_H + i as i32 * LINE_H;
            let selected_here = args.phase == EquipDrawPhase::ItemPicker && i as u16 == args.cursor;
            let color = if selected_here { gold } else { white };
            if selected_here {
                out.extend(text_draws_for(&font.layout_ascii(">"), (col_x, y), color));
            }
            let line = format!("{:<14} x{:>2}", c.name, c.count);
            out.extend(text_draws_for(
                &font.layout_ascii(&line),
                (col_x + 14, y),
                color,
            ));
            let mut delta_x = col_x + 14 + 130;
            if c.atk_delta != 0 {
                let s = format!("ATK {:+}", c.atk_delta);
                let dc = if c.atk_delta > 0 { green } else { red };
                out.extend(text_draws_for(&font.layout_ascii(&s), (delta_x, y), dc));
                delta_x += 56;
            }
            if c.udf_delta != 0 {
                let s = format!("UDF {:+}", c.udf_delta);
                let dc = if c.udf_delta > 0 { green } else { red };
                out.extend(text_draws_for(&font.layout_ascii(&s), (delta_x, y), dc));
            }
        }
    }

    // Confirm prompt at the bottom.
    if args.phase == EquipDrawPhase::Confirm {
        let prompt_y = pen.1 + LINE_H + args.slots.len() as i32 * LINE_H + LINE_H;
        if let Some(label) = args.confirm_label {
            out.extend(text_draws_for(
                &font.layout_ascii(label),
                (pen.0, prompt_y),
                white,
            ));
        }
        for (i, opt) in ["Yes", "No"].iter().enumerate() {
            let x = pen.0 + 110 + i as i32 * 50;
            let selected = i as u16 == args.cursor;
            let color = if selected { gold } else { white };
            if selected {
                out.extend(text_draws_for(
                    &font.layout_ascii(">"),
                    (x - 10, prompt_y + LINE_H),
                    color,
                ));
            }
            out.extend(text_draws_for(
                &font.layout_ascii(opt),
                (x, prompt_y + LINE_H),
                color,
            ));
        }
    }
    out
}

/// One saved Tactical Arts chain row in the editor's browse list.
pub struct ArtsChainRow<'a> {
    pub name: &'a str,
    /// One-line stringification of the command sequence ("L R D U R").
    /// Engines build this with `SavedChain::pretty_sequence()`.
    pub pretty_sequence: &'a str,
}

/// Phase tag for [`tactical_arts_editor_draws_for`]. Mirrors
/// `engine-core::tactical_arts_editor::EditorPhase` without depending on
/// the enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArtsEditorPhase {
    /// Cursor on the saved-chain list (browse).
    Browsing,
    /// Player editing a sequence of directions.
    Editing,
    /// Player picking a name for the new chain.
    Naming,
}

/// Bundle for [`tactical_arts_editor_draws_for`].
pub struct ArtsEditorDrawArgs<'a> {
    pub character_name: &'a str,
    pub phase: ArtsEditorPhase,
    pub saved: &'a [ArtsChainRow<'a>],
    /// Cursor row inside the saved list. Only consumed in `Browsing`.
    pub browse_cursor: u8,
    /// Live working-sequence pretty string, e.g. "L R D".
    pub editing_pretty: &'a str,
    /// Live working-sequence length (used to display "len 3 / 7" status).
    pub editing_len: usize,
    /// Min / max sequence length the editor enforces (3..=7 in retail).
    pub min_len: usize,
    pub max_len: usize,
    /// Currently-picked name in the naming phase ("Combo A", ...).
    pub naming_name: &'a str,
    /// `true` when there is room in the library for one more saved
    /// chain - the browse list shows a trailing "+ New" row only then.
    pub can_add_new: bool,
}

/// Build [`TextDraw`]s for the Tactical Arts editor overlay shared by
/// the field menu's Arts row.
///
/// Layout (anchored at `pen`) - varies per phase:
/// ```text
/// Browsing:
///   ARTS - Vahn
///   > Combo A     L R D U
///     Striker     U U L R D
///     + New
///
/// Editing:
///   ARTS - Vahn  (Editing)
///   Sequence: L R D     (3 / 7)
///   D-Pad: append   Triangle: pop   Cross: name
///
/// Naming:
///   ARTS - Vahn  (Naming)
///   Name: Combo B
///   Square: cycle    Cross: save    Circle: back
/// ```
pub fn tactical_arts_editor_draws_for(
    font: &legaia_font::Font,
    args: ArtsEditorDrawArgs<'_>,
    pen: (i32, i32),
) -> Vec<TextDraw> {
    const LINE_H: i32 = 14;
    let white: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
    let gold: [f32; 4] = [1.0, 0.85, 0.3, 1.0];
    let dim: [f32; 4] = [0.55, 0.55, 0.55, 1.0];
    let green: [f32; 4] = [0.5, 1.0, 0.5, 1.0];

    let mut out = Vec::new();

    let head = match args.phase {
        ArtsEditorPhase::Browsing => format!("ARTS - {}", args.character_name),
        ArtsEditorPhase::Editing => format!("ARTS - {}  (Editing)", args.character_name),
        ArtsEditorPhase::Naming => format!("ARTS - {}  (Naming)", args.character_name),
    };
    out.extend(text_draws_for(&font.layout_ascii(&head), pen, gold));

    match args.phase {
        ArtsEditorPhase::Browsing => {
            // Saved chains.
            for (i, chain) in args.saved.iter().enumerate() {
                let y = pen.1 + LINE_H + i as i32 * LINE_H;
                let selected = i as u8 == args.browse_cursor;
                let color = if selected { gold } else { white };
                if selected {
                    out.extend(text_draws_for(&font.layout_ascii(">"), (pen.0, y), color));
                }
                out.extend(text_draws_for(
                    &font.layout_ascii(chain.name),
                    (pen.0 + 14, y),
                    color,
                ));
                out.extend(text_draws_for(
                    &font.layout_ascii(chain.pretty_sequence),
                    (pen.0 + 110, y),
                    color,
                ));
            }
            // Trailing "+ New" row.
            if args.can_add_new {
                let i = args.saved.len();
                let y = pen.1 + LINE_H + i as i32 * LINE_H;
                let selected = i as u8 == args.browse_cursor;
                let color = if selected { gold } else { white };
                if selected {
                    out.extend(text_draws_for(&font.layout_ascii(">"), (pen.0, y), color));
                }
                out.extend(text_draws_for(
                    &font.layout_ascii("+ New"),
                    (pen.0 + 14, y),
                    color,
                ));
            }
            let foot_y = pen.1
                + LINE_H
                + (args.saved.len() + if args.can_add_new { 1 } else { 0 }) as i32 * LINE_H
                + LINE_H;
            out.extend(text_draws_for(
                &font.layout_ascii("Cross: Edit  Triangle: Delete  Circle: Back"),
                (pen.0, foot_y),
                dim,
            ));
        }
        ArtsEditorPhase::Editing => {
            let line1 = format!(
                "Sequence: {}     ({} / {})",
                args.editing_pretty, args.editing_len, args.max_len
            );
            let len_ok = args.editing_len >= args.min_len;
            let color = if len_ok { green } else { white };
            out.extend(text_draws_for(
                &font.layout_ascii(&line1),
                (pen.0, pen.1 + LINE_H),
                color,
            ));
            out.extend(text_draws_for(
                &font.layout_ascii("D-Pad: append   Triangle: pop"),
                (pen.0, pen.1 + LINE_H * 3),
                dim,
            ));
            let cross_hint = if len_ok {
                "Cross: Name & Save"
            } else {
                "Cross: Name & Save  (need 3+ inputs)"
            };
            out.extend(text_draws_for(
                &font.layout_ascii(cross_hint),
                (pen.0, pen.1 + LINE_H * 4),
                if len_ok { gold } else { dim },
            ));
            out.extend(text_draws_for(
                &font.layout_ascii("Circle: Back"),
                (pen.0, pen.1 + LINE_H * 5),
                dim,
            ));
        }
        ArtsEditorPhase::Naming => {
            let l = format!("Name: {}", args.naming_name);
            out.extend(text_draws_for(
                &font.layout_ascii(&l),
                (pen.0, pen.1 + LINE_H),
                gold,
            ));
            let sequence = format!("Sequence: {}", args.editing_pretty);
            out.extend(text_draws_for(
                &font.layout_ascii(&sequence),
                (pen.0, pen.1 + LINE_H * 2),
                white,
            ));
            out.extend(text_draws_for(
                &font.layout_ascii("Square: cycle name   Cross: Save   Circle: Back"),
                (pen.0, pen.1 + LINE_H * 4),
                dim,
            ));
        }
    }

    out
}

/// A wireframe line mesh: position + per-vertex RGB color, drawn as
/// `LineList` (every pair of indices is one line segment). Unlit and
/// depth-tested. Used by the stage-geometry viewer.
pub struct UploadedLines {
    vertex_buf: wgpu::Buffer,
    index_buf: wgpu::Buffer,
    index_count: u32,
}

impl UploadedLines {
    pub fn index_count(&self) -> u32 {
        self.index_count
    }

    pub fn line_count(&self) -> u32 {
        self.index_count / 2
    }
}

/// What to draw this frame.
pub enum RenderTarget<'a> {
    /// Clear-only - the PROT browser uses this for entries with no preview.
    Clear,
    /// 2D textured quad (TIM viewer, sprite previews).
    Texture(&'a UploadedTexture),
    /// 3D triangulated mesh (TMD viewer). `mvp` is the full
    /// model-view-projection matrix, supplied per-frame so the host can
    /// drive rotation/zoom without re-uploading geometry.
    Mesh { mesh: &'a UploadedMesh, mvp: Mat4 },
    /// 3D textured mesh: same as `Mesh` but samples a bound texture using
    /// per-vertex UVs. Used by the TMD viewer when a paired TIM is found.
    TexturedMesh {
        mesh: &'a UploadedTexturedMesh,
        texture: &'a UploadedTexture,
        mvp: Mat4,
    },
    /// 3D mesh with full PSX VRAM emulation: per-vertex UV/CBA/TSB
    /// addresses into a 1024×512 software VRAM, with 4/8/15bpp + CLUT
    /// decode in the fragment shader.
    VramMesh {
        mesh: &'a UploadedVramMesh,
        vram: &'a UploadedVram,
        mvp: Mat4,
    },
    /// Wireframe line mesh (stage-geometry viewer). Same depth-tested 3D
    /// pipeline; per-vertex color, no lighting.
    Lines { mesh: &'a UploadedLines, mvp: Mat4 },
    /// Multi-actor scene: N VRAM meshes drawn in one render pass with a
    /// shared VRAM and per-actor MVPs. Optionally overlays a single
    /// wireframe-lines mesh (stage geometry / debug grid) drawn after the
    /// solid actors.
    ///
    /// Used by the `world` viewer to render every active actor in the
    /// World composite per frame.
    Scene(&'a Scene<'a>),
    /// 2D text-only frame: clear to background, then draw a single
    /// [`TextOverlay`]. Used by the dialog viewer / any UI mode that
    /// has no 3D scene to render.
    TextOnly(&'a TextOverlay<'a>),
}

/// Per-actor draw inside a [`Scene`].
pub struct SceneDraw<'a> {
    pub mesh: &'a UploadedVramMesh,
    pub mvp: Mat4,
}

/// An untextured, vertex-coloured mesh draw inside a [`Scene`] (the `F*`/`G*`
/// props). Drawn after the textured [`SceneDraw`]s, on the same depth buffer.
pub struct ColorSceneDraw<'a> {
    pub mesh: &'a UploadedColorMesh,
    pub mvp: Mat4,
}

/// Multi-actor scene payload. Drawn against a single shared VRAM with one
/// MVP per actor. Optionally overlays a [`UploadedLines`] mesh (e.g. a
/// stage-geometry wireframe) using the supplied MVP, and/or a 2D text
/// batch (HUD / debug text / dialog).
pub struct Scene<'a> {
    pub vram: &'a UploadedVram,
    pub draws: &'a [SceneDraw<'a>],
    /// Untextured vertex-coloured meshes (`F*`/`G*` props), drawn after
    /// [`Self::draws`] on the shared depth buffer. Usually empty.
    pub color_draws: &'a [ColorSceneDraw<'a>],
    pub overlay_lines: Option<(&'a UploadedLines, Mat4)>,
    /// 2D sprite batch drawn after the 3D meshes and lines, before
    /// [`Self::overlay_text`]. Used by the actor sprite pipeline.
    pub overlay_sprites: Option<&'a SpriteOverlay<'a>>,
    pub overlay_text: Option<&'a TextOverlay<'a>>,
    /// Optional second sprite-overlay slot drawn between
    /// [`Self::overlay_sprites`] and [`Self::overlay_text`]. Used when
    /// two distinct sprite atlases need to render on the same frame
    /// (e.g. title-art bands + menu-glyph atlas during the title
    /// menu phase).
    pub overlay_sprites_2: Option<&'a SpriteOverlay<'a>>,
    /// Optional override of the surface clear colour (linear RGBA). When
    /// `None` the renderer falls back to its default dark-grey clear.
    /// Used during the boot publisher-logos phase to force pure black.
    pub clear_color: Option<[f32; 4]>,
}

pub struct Renderer {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    /// Quad pipeline (Phase 1 TIM viewer).
    pipeline: wgpu::RenderPipeline,
    sampler: wgpu::Sampler,
    bind_group_layout: wgpu::BindGroupLayout,
    uniforms_buf: wgpu::Buffer,
    uniforms_bg: wgpu::BindGroup,
    /// Mesh pipeline (Phase 1 TMD viewer).
    mesh_pipeline: wgpu::RenderPipeline,
    mesh_uniforms_buf: wgpu::Buffer,
    mesh_uniforms_bg: wgpu::BindGroup,
    /// Textured-mesh pipeline (Phase 1 TMD viewer with paired TIM).
    /// Reuses [`Self::bind_group_layout`] for the per-mesh texture binding.
    textured_mesh_pipeline: wgpu::RenderPipeline,
    /// VRAM-mesh pipeline: per-vertex CBA/TSB + R16Uint VRAM texture lookup.
    vram_mesh_pipeline: wgpu::RenderPipeline,
    /// PSX semi-transparency blend pipelines for [`Self::vram_mesh_pipeline`],
    /// one per ABR mode 0..=3 (see [`psx_blend`]). Drawn as a second pass
    /// over each mesh's semi-transparent index tail when PSX-faithful mode
    /// is on ([`Self::set_psx_mode`]).
    vram_mesh_blend_pipelines: [wgpu::RenderPipeline; 4],
    vram_bgl: wgpu::BindGroupLayout,
    /// Multi-actor "scene" VRAM-mesh pipeline. Identical to
    /// [`Self::vram_mesh_pipeline`] but binds [`Self::scene_uniforms_bgl`]
    /// at group 0 (with `has_dynamic_offset = true`) so a single render
    /// pass can draw N actors with one bind group + N dynamic offsets.
    scene_vram_mesh_pipeline: wgpu::RenderPipeline,
    /// Scene-layout twins of [`Self::vram_mesh_blend_pipelines`].
    scene_vram_mesh_blend_pipelines: [wgpu::RenderPipeline; 4],
    /// Lines pipeline shadowing the scene path (uses the scene-uniforms
    /// dynamic-offset layout). Used by [`Self::render`] when a `Scene`
    /// carries `overlay_lines`.
    scene_lines_pipeline: wgpu::RenderPipeline,
    /// Untextured vertex-colour mesh pipeline shadowing the scene path (same
    /// scene-uniforms dynamic-offset layout, no VRAM group). Draws a `Scene`'s
    /// `color_draws` (`F*`/`G*` props).
    scene_color_mesh_pipeline: wgpu::RenderPipeline,
    /// PSX semi-transparency blend pipelines for
    /// [`Self::scene_color_mesh_pipeline`], one per ABR mode 0..=3 (see
    /// [`psx_blend`]). Unlike the textured blend pass there is no per-texel
    /// STP gate: an untextured ABE prim blends *all* its pixels, so the
    /// fragment entries just emit the interpolated vertex colour (mode 3
    /// pre-scaled by 0.25) and the fixed-function blend state does the rest.
    scene_color_mesh_blend_pipelines: [wgpu::RenderPipeline; 4],
    scene_uniforms_bgl: wgpu::BindGroupLayout,
    scene_uniforms_bg: std::cell::RefCell<wgpu::BindGroup>,
    scene_uniforms_buf: std::cell::RefCell<wgpu::Buffer>,
    /// Capacity (in `MeshUniforms` slots) of [`Self::scene_uniforms_buf`].
    scene_uniforms_capacity: std::cell::Cell<usize>,
    /// `min_uniform_buffer_offset_alignment` reported by the adapter.
    /// Per-actor uniform writes are padded up to this stride.
    uniform_offset_alignment: u32,
    /// Lines pipeline: LineList topology, per-vertex color, depth-tested.
    /// Used for wireframe rendering of stage geometry.
    lines_pipeline: wgpu::RenderPipeline,
    /// Text pipeline: 2D textured quads, alpha-blended, no depth. Group 0
    /// binds a sampled font atlas. Used for HUD / debug / dialog overlays.
    text_pipeline: wgpu::RenderPipeline,
    /// Bind-group layout for the font-atlas texture binding (group 0 of
    /// [`Self::text_pipeline`]). Reused when uploading new atlases.
    text_atlas_bgl: wgpu::BindGroupLayout,
    /// Sampler used by the text pipeline. Nearest-neighbour to keep PSX
    /// pixel-art glyphs crisp.
    text_sampler: wgpu::Sampler,
    /// Per-frame text vertex / index buffers (RefCell-borrowed from the
    /// non-mut `render` API; resized geometrically on demand).
    text_vbuf: std::cell::RefCell<wgpu::Buffer>,
    text_ibuf: std::cell::RefCell<wgpu::Buffer>,
    /// Capacity of [`Self::text_vbuf`] in vertex slots and
    /// [`Self::text_ibuf`] in index slots. Both grow together - one quad
    /// per `TextDraw` is 4 vertices and 6 indices.
    text_vertex_capacity: std::cell::Cell<u32>,
    text_index_capacity: std::cell::Cell<u32>,
    /// Per-overlay quad ranges from the most recent staging call -
    /// `[(base_quad, count), ...]` in the same order overlays were passed.
    /// Drained by the in-pass draw to issue one `draw_indexed` per overlay
    /// with the matching atlas bound.
    scene_quad_ranges: std::cell::RefCell<Vec<(u32, u32)>>,
    /// Depth target - recreated on resize.
    depth_view: wgpu::TextureView,
    /// PSX-faithful rendering mode. When `true`, the VRAM-mesh shader uses
    /// affine (linear-in-screen-space) UV interpolation instead of
    /// perspective-correct, and snaps clip-space x/y to integer pixel
    /// positions to reproduce the GTE's per-vertex sub-pixel-truncation
    /// "vertex jitter." Default `false` for clean smooth rendering.
    psx_mode: std::cell::Cell<bool>,
    /// Count of [`Self::upload_vram`] calls; stamps each [`UploadedVram`]
    /// with a strictly-increasing generation (see
    /// [`UploadedVram::generation`]).
    vram_upload_counter: std::cell::Cell<u64>,
    /// GP0(0xE2) "Texture Window setting" - `(mask_x, mask_y, off_x, off_y)`
    /// each in 8-pixel steps (0..=31). Applied per-fragment in the
    /// VRAM-mesh shader. Defaults to all-zero (no-op), which matches
    /// retail Legaia's typical state - the register only gets non-zero
    /// values from a handful of effect / scene-init scripts.
    tex_window: std::cell::Cell<[u32; 4]>,
}

impl Renderer {
    /// Constructs a renderer attached to a winit-style window. Caller passes
    /// an `Arc<Window>` so the Surface can outlive the borrow.
    pub fn new<W>(window: Arc<W>, width: u32, height: u32) -> Result<Self>
    where
        W: wgpu::WindowHandle + 'static,
    {
        pollster::block_on(Self::new_async(window, width, height))
    }

    async fn new_async<W>(window: Arc<W>, width: u32, height: u32) -> Result<Self>
    where
        W: wgpu::WindowHandle + 'static,
    {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::PRIMARY,
            ..Default::default()
        });
        let surface = instance
            .create_surface(window)
            .context("create wgpu surface")?;
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::LowPower,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .context("request adapter")?;
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("legaia engine device"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::downlevel_defaults()
                    .using_resolution(adapter.limits()),
                memory_hints: wgpu::MemoryHints::default(),
                trace: wgpu::Trace::Off,
            })
            .await
            .context("request device")?;

        let caps = surface.get_capabilities(&adapter);
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| f.is_srgb())
            .unwrap_or(caps.formats[0]);
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: width.max(1),
            height: height.max(1),
            present_mode: wgpu::PresentMode::AutoVsync,
            desired_maximum_frame_latency: 2,
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
        };
        surface.configure(&device, &config);

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("legaia textured quad shader"),
            source: wgpu::ShaderSource::Wgsl(SHADER_SRC.into()),
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("texture bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        multisampled: false,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        let uniforms_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("uniforms bgl"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });

        let uniforms_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("uniforms"),
            contents: bytemuck::cast_slice(&[Uniforms {
                scale: [1.0, 1.0, 0.0, 0.0],
            }]),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let uniforms_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("uniforms bg"),
            layout: &uniforms_bgl,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniforms_buf.as_entire_binding(),
            }],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("legaia pipeline layout"),
            bind_group_layouts: &[&bind_group_layout, &uniforms_bgl],
            push_constant_ranges: &[],
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("legaia textured quad pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: config.format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleStrip,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("texture sampler"),
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });

        // Mesh pipeline: 3D triangle list, depth-tested, single directional light.
        let mesh_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("legaia mesh shader"),
            source: wgpu::ShaderSource::Wgsl(compose_psx_shader(MESH_SHADER_SRC).into()),
        });
        let mesh_uniforms_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("mesh uniforms bgl"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let mesh_uniforms_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("mesh uniforms"),
            contents: bytemuck::cast_slice(&[MeshUniforms {
                mvp: Mat4::IDENTITY.to_cols_array_2d(),
                light_dir: [0.4, -0.8, 0.4, 0.0],
                psx_params: [width as f32, height as f32, 0.0, 0.0],
                tex_window: [0; 4],
            }]),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let mesh_uniforms_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("mesh uniforms bg"),
            layout: &mesh_uniforms_bgl,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: mesh_uniforms_buf.as_entire_binding(),
            }],
        });
        let mesh_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("legaia mesh pipeline layout"),
            bind_group_layouts: &[&mesh_uniforms_bgl],
            push_constant_ranges: &[],
        });
        // Vertex layout: 3 floats position. Normals are computed in the shader
        // from screen-space derivatives - no per-vertex normal needed, which
        // keeps the upload format dead-simple for the source TMDs (which only
        // store per-object normals, not per-vertex).
        let vertex_layout = wgpu::VertexBufferLayout {
            array_stride: 12,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[wgpu::VertexAttribute {
                offset: 0,
                shader_location: 0,
                format: wgpu::VertexFormat::Float32x3,
            }],
        };
        let mesh_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("legaia mesh pipeline"),
            layout: Some(&mesh_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &mesh_shader,
                entry_point: Some("vs_main"),
                buffers: &[vertex_layout],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &mesh_shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: config.format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: true,
                depth_compare: wgpu::CompareFunction::Less,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        // Textured-mesh pipeline: same depth + MVP path as the flat mesh
        // pipeline, but with a per-vertex UV attribute and a fragment shader
        // that samples a bound texture. Reuses `bind_group_layout` (the
        // texture+sampler layout from the quad pipeline) at group 1.
        let textured_mesh_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("legaia textured mesh shader"),
            source: wgpu::ShaderSource::Wgsl(compose_psx_shader(TEXTURED_MESH_SHADER_SRC).into()),
        });
        let textured_mesh_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("legaia textured mesh pipeline layout"),
            bind_group_layouts: &[&mesh_uniforms_bgl, &bind_group_layout],
            push_constant_ranges: &[],
        });
        let textured_vertex_layout = wgpu::VertexBufferLayout {
            // 3 floats position + 2 floats UV = 20 bytes.
            array_stride: 20,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[
                wgpu::VertexAttribute {
                    offset: 0,
                    shader_location: 0,
                    format: wgpu::VertexFormat::Float32x3,
                },
                wgpu::VertexAttribute {
                    offset: 12,
                    shader_location: 1,
                    format: wgpu::VertexFormat::Float32x2,
                },
            ],
        };
        let textured_mesh_pipeline =
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("legaia textured mesh pipeline"),
                layout: Some(&textured_mesh_layout),
                vertex: wgpu::VertexState {
                    module: &textured_mesh_shader,
                    entry_point: Some("vs_main"),
                    buffers: &[textured_vertex_layout],
                    compilation_options: Default::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &textured_mesh_shader,
                    entry_point: Some("fs_main"),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: config.format,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: Default::default(),
                }),
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    cull_mode: None,
                    ..Default::default()
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH_FORMAT,
                    depth_write_enabled: true,
                    depth_compare: wgpu::CompareFunction::Less,
                    stencil: wgpu::StencilState::default(),
                    bias: wgpu::DepthBiasState::default(),
                }),
                multisample: wgpu::MultisampleState::default(),
                multiview: None,
                cache: None,
            });

        // VRAM-mesh pipeline: per-vertex (UV, CBA, TSB) + a 1024×512 R16Uint
        // texture holding the whole PSX VRAM. The fragment shader does its
        // own page+CLUT lookup so a single mesh can sample multiple texture
        // pages and palettes correctly.
        let vram_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("psx vram bgl"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    multisampled: false,
                    view_dimension: wgpu::TextureViewDimension::D2,
                    sample_type: wgpu::TextureSampleType::Uint,
                },
                count: None,
            }],
        });
        let vram_mesh_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("legaia vram mesh shader"),
            source: wgpu::ShaderSource::Wgsl(compose_psx_shader(VRAM_MESH_SHADER_SRC).into()),
        });
        let vram_mesh_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("legaia vram mesh pipeline layout"),
            bind_group_layouts: &[&mesh_uniforms_bgl, &vram_bgl],
            push_constant_ranges: &[],
        });
        // 12 (pos) + 4 (uv as Uint8x4) + 4 (cba/tsb as Uint16x2) + 12
        // (normal as Float32x3) = 32 bytes
        let vram_vertex_layout = wgpu::VertexBufferLayout {
            array_stride: 32,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[
                wgpu::VertexAttribute {
                    offset: 0,
                    shader_location: 0,
                    format: wgpu::VertexFormat::Float32x3,
                },
                wgpu::VertexAttribute {
                    offset: 12,
                    shader_location: 1,
                    format: wgpu::VertexFormat::Uint8x4,
                },
                wgpu::VertexAttribute {
                    offset: 16,
                    shader_location: 2,
                    format: wgpu::VertexFormat::Uint16x2,
                },
                wgpu::VertexAttribute {
                    offset: 20,
                    shader_location: 3,
                    format: wgpu::VertexFormat::Float32x3,
                },
            ],
        };
        let vram_mesh_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("legaia vram mesh pipeline"),
            layout: Some(&vram_mesh_layout),
            vertex: wgpu::VertexState {
                module: &vram_mesh_shader,
                entry_point: Some("vs_main"),
                buffers: std::slice::from_ref(&vram_vertex_layout),
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &vram_mesh_shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: config.format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: true,
                depth_compare: wgpu::CompareFunction::Less,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        // Wireframe lines pipeline: LineList topology, per-vertex color,
        // depth-tested. Reuses `mesh_uniforms_bgl` for the MVP. Per-vertex
        // layout = 12 (position) + 4 (color as Uint8x4) = 16 bytes.
        let lines_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("legaia lines shader"),
            source: wgpu::ShaderSource::Wgsl(LINES_SHADER_SRC.into()),
        });
        let lines_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("legaia lines pipeline layout"),
            bind_group_layouts: &[&mesh_uniforms_bgl],
            push_constant_ranges: &[],
        });
        let lines_vertex_layout = wgpu::VertexBufferLayout {
            array_stride: 16,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[
                wgpu::VertexAttribute {
                    offset: 0,
                    shader_location: 0,
                    format: wgpu::VertexFormat::Float32x3,
                },
                wgpu::VertexAttribute {
                    offset: 12,
                    shader_location: 1,
                    format: wgpu::VertexFormat::Unorm8x4,
                },
            ],
        };
        let lines_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("legaia lines pipeline"),
            layout: Some(&lines_layout),
            vertex: wgpu::VertexState {
                module: &lines_shader,
                entry_point: Some("vs_main"),
                buffers: &[lines_vertex_layout],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &lines_shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: config.format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::LineList,
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: true,
                depth_compare: wgpu::CompareFunction::Less,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        // Scene-uniforms layout: a single dynamic-offset uniform buffer
        // holding N `MeshUniforms` slots, each `uniform_offset_alignment`
        // bytes apart. Reused for the multi-actor VRAM-mesh and lines
        // pipelines below.
        let uniform_offset_alignment = device.limits().min_uniform_buffer_offset_alignment.max(256);
        let scene_uniforms_bgl =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("scene mesh uniforms bgl"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: true,
                        min_binding_size: std::num::NonZeroU64::new(
                            std::mem::size_of::<MeshUniforms>() as u64,
                        ),
                    },
                    count: None,
                }],
            });
        // Initial capacity: one slot. Grown on demand by render_scene.
        let initial_scene_capacity: usize = 1;
        let scene_uniforms_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("scene mesh uniforms"),
            size: (initial_scene_capacity * uniform_offset_alignment as usize) as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let scene_uniforms_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("scene mesh uniforms bg"),
            layout: &scene_uniforms_bgl,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: &scene_uniforms_buf,
                    offset: 0,
                    size: std::num::NonZeroU64::new(std::mem::size_of::<MeshUniforms>() as u64),
                }),
            }],
        });

        let scene_vram_mesh_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("legaia scene vram mesh pipeline layout"),
                bind_group_layouts: &[&scene_uniforms_bgl, &vram_bgl],
                push_constant_ranges: &[],
            });
        let scene_vram_mesh_pipeline =
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("legaia scene vram mesh pipeline"),
                layout: Some(&scene_vram_mesh_layout),
                vertex: wgpu::VertexState {
                    module: &vram_mesh_shader,
                    entry_point: Some("vs_main"),
                    buffers: &[wgpu::VertexBufferLayout {
                        array_stride: 32,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &[
                            wgpu::VertexAttribute {
                                offset: 0,
                                shader_location: 0,
                                format: wgpu::VertexFormat::Float32x3,
                            },
                            wgpu::VertexAttribute {
                                offset: 12,
                                shader_location: 1,
                                format: wgpu::VertexFormat::Uint8x4,
                            },
                            wgpu::VertexAttribute {
                                offset: 16,
                                shader_location: 2,
                                format: wgpu::VertexFormat::Uint16x2,
                            },
                            wgpu::VertexAttribute {
                                offset: 20,
                                shader_location: 3,
                                format: wgpu::VertexFormat::Float32x3,
                            },
                        ],
                    }],
                    compilation_options: Default::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &vram_mesh_shader,
                    entry_point: Some("fs_main"),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: config.format,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: Default::default(),
                }),
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    cull_mode: None,
                    ..Default::default()
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH_FORMAT,
                    depth_write_enabled: true,
                    depth_compare: wgpu::CompareFunction::Less,
                    stencil: wgpu::StencilState::default(),
                    bias: wgpu::DepthBiasState::default(),
                }),
                multisample: wgpu::MultisampleState::default(),
                multiview: None,
                cache: None,
            });

        // PSX semi-transparency blend pipelines: one per ABR mode, for both
        // the single-mesh layout and the scene (dynamic-offset) layout. Same
        // shader module + vertex state as the opaque VRAM-mesh pipelines;
        // the blend-pass fragment entry keeps only STP texels and the
        // per-mode fixed-function [`psx_blend::blend_state`] applies the PSX
        // equation (mode 3 pre-scales F by 0.25 via its own entry point).
        // Depth: test against the opaque pass but don't write (the PSX has
        // no depth buffer and blended fragments must not occlude later
        // draws); LessEqual so decal prims coplanar with already-drawn
        // geometry aren't z-rejected.
        let make_blend_pipeline = |label: &'static str,
                                   layout: &wgpu::PipelineLayout,
                                   mode: u8|
         -> wgpu::RenderPipeline {
            let entry = if psx_blend::src_shader_scale(mode) == 1.0 {
                "fs_blend"
            } else {
                "fs_blend_quarter"
            };
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(layout),
                vertex: wgpu::VertexState {
                    module: &vram_mesh_shader,
                    entry_point: Some("vs_main"),
                    buffers: std::slice::from_ref(&vram_vertex_layout),
                    compilation_options: Default::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &vram_mesh_shader,
                    entry_point: Some(entry),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: config.format,
                        blend: Some(psx_blend::blend_state(mode)),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: Default::default(),
                }),
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    cull_mode: None,
                    ..Default::default()
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH_FORMAT,
                    depth_write_enabled: false,
                    depth_compare: wgpu::CompareFunction::LessEqual,
                    stencil: wgpu::StencilState::default(),
                    bias: wgpu::DepthBiasState::default(),
                }),
                multisample: wgpu::MultisampleState::default(),
                multiview: None,
                cache: None,
            })
        };
        let vram_mesh_blend_pipelines: [wgpu::RenderPipeline; 4] = std::array::from_fn(|m| {
            make_blend_pipeline(
                "legaia vram mesh blend pipeline",
                &vram_mesh_layout,
                m as u8,
            )
        });
        let scene_vram_mesh_blend_pipelines: [wgpu::RenderPipeline; 4] = std::array::from_fn(|m| {
            make_blend_pipeline(
                "legaia scene vram mesh blend pipeline",
                &scene_vram_mesh_layout,
                m as u8,
            )
        });

        let scene_lines_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("legaia scene lines pipeline layout"),
            bind_group_layouts: &[&scene_uniforms_bgl],
            push_constant_ranges: &[],
        });
        let scene_lines_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("legaia scene lines pipeline"),
            layout: Some(&scene_lines_layout),
            vertex: wgpu::VertexState {
                module: &lines_shader,
                entry_point: Some("vs_main"),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: 16,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &[
                        wgpu::VertexAttribute {
                            offset: 0,
                            shader_location: 0,
                            format: wgpu::VertexFormat::Float32x3,
                        },
                        wgpu::VertexAttribute {
                            offset: 12,
                            shader_location: 1,
                            format: wgpu::VertexFormat::Unorm8x4,
                        },
                    ],
                }],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &lines_shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: config.format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::LineList,
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: true,
                depth_compare: wgpu::CompareFunction::Less,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        // Vertex-colour mesh pipeline (untextured F*/G* props): same scene-
        // uniforms dynamic-offset layout as the lines pipeline (group 0 only,
        // no VRAM), TriangleList, position(12) + Unorm8x4 colour(4) +
        // Uint32 blend word(4) = 20 bytes. The blend word carries the prim's
        // ABE/ABR state in the low 16 bits ([`psx_blend::pack_blend_word`]).
        let color_mesh_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("legaia color mesh shader"),
            source: wgpu::ShaderSource::Wgsl(compose_psx_shader(COLOR_MESH_SHADER_SRC).into()),
        });
        let scene_color_mesh_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("legaia scene color mesh pipeline layout"),
                bind_group_layouts: &[&scene_uniforms_bgl],
                push_constant_ranges: &[],
            });
        let color_mesh_attributes = [
            wgpu::VertexAttribute {
                offset: 0,
                shader_location: 0,
                format: wgpu::VertexFormat::Float32x3,
            },
            wgpu::VertexAttribute {
                offset: 12,
                shader_location: 1,
                format: wgpu::VertexFormat::Unorm8x4,
            },
            wgpu::VertexAttribute {
                offset: 16,
                shader_location: 2,
                format: wgpu::VertexFormat::Uint32,
            },
        ];
        let color_mesh_vertex_layout = wgpu::VertexBufferLayout {
            array_stride: 20,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &color_mesh_attributes,
        };
        let scene_color_mesh_pipeline =
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("legaia scene color mesh pipeline"),
                layout: Some(&scene_color_mesh_layout),
                vertex: wgpu::VertexState {
                    module: &color_mesh_shader,
                    entry_point: Some("vs_main"),
                    buffers: std::slice::from_ref(&color_mesh_vertex_layout),
                    compilation_options: Default::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &color_mesh_shader,
                    entry_point: Some("fs_main"),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: config.format,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: Default::default(),
                }),
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    ..Default::default()
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH_FORMAT,
                    depth_write_enabled: true,
                    depth_compare: wgpu::CompareFunction::Less,
                    stencil: wgpu::StencilState::default(),
                    bias: wgpu::DepthBiasState::default(),
                }),
                multisample: wgpu::MultisampleState::default(),
                multiview: None,
                cache: None,
            });
        // PSX semi-transparency blend pipelines for the colour-mesh path,
        // one per ABR mode. Same shader module + vertex layout as the opaque
        // colour pipeline; the blend-pass fragment entries emit the prim
        // colour (mode 3 pre-scales by 0.25) and the per-mode fixed-function
        // [`psx_blend::blend_state`] applies the PSX equation. Unlike the
        // VRAM-mesh blend pass there is no STP discard - an untextured ABE
        // prim blends every pixel. Depth: LessEqual without writing, like
        // the textured blend pass.
        let scene_color_mesh_blend_pipelines: [wgpu::RenderPipeline; 4] =
            std::array::from_fn(|m| {
                let mode = m as u8;
                let entry = if psx_blend::src_shader_scale(mode) == 1.0 {
                    "fs_blend"
                } else {
                    "fs_blend_quarter"
                };
                device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some("legaia scene color mesh blend pipeline"),
                    layout: Some(&scene_color_mesh_layout),
                    vertex: wgpu::VertexState {
                        module: &color_mesh_shader,
                        entry_point: Some("vs_main"),
                        buffers: std::slice::from_ref(&color_mesh_vertex_layout),
                        compilation_options: Default::default(),
                    },
                    fragment: Some(wgpu::FragmentState {
                        module: &color_mesh_shader,
                        entry_point: Some(entry),
                        targets: &[Some(wgpu::ColorTargetState {
                            format: config.format,
                            blend: Some(psx_blend::blend_state(mode)),
                            write_mask: wgpu::ColorWrites::ALL,
                        })],
                        compilation_options: Default::default(),
                    }),
                    primitive: wgpu::PrimitiveState {
                        topology: wgpu::PrimitiveTopology::TriangleList,
                        cull_mode: None,
                        ..Default::default()
                    },
                    depth_stencil: Some(wgpu::DepthStencilState {
                        format: DEPTH_FORMAT,
                        depth_write_enabled: false,
                        depth_compare: wgpu::CompareFunction::LessEqual,
                        stencil: wgpu::StencilState::default(),
                        bias: wgpu::DepthBiasState::default(),
                    }),
                    multisample: wgpu::MultisampleState::default(),
                    multiview: None,
                    cache: None,
                })
            });

        // Text pipeline: 2D textured quads in NDC, alpha blended, no depth.
        // Vertex layout = 8 (pos: Float32x2) + 8 (uv: Float32x2) +
        // 16 (color: Float32x4) = 32 bytes.
        let text_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("legaia text shader"),
            source: wgpu::ShaderSource::Wgsl(TEXT_SHADER_SRC.into()),
        });
        let text_atlas_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("text atlas bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        multisampled: false,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let text_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("text atlas sampler"),
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });
        let text_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("legaia text pipeline layout"),
            bind_group_layouts: &[&text_atlas_bgl],
            push_constant_ranges: &[],
        });
        let text_vertex_layout = wgpu::VertexBufferLayout {
            array_stride: 32,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[
                wgpu::VertexAttribute {
                    offset: 0,
                    shader_location: 0,
                    format: wgpu::VertexFormat::Float32x2,
                },
                wgpu::VertexAttribute {
                    offset: 8,
                    shader_location: 1,
                    format: wgpu::VertexFormat::Float32x2,
                },
                wgpu::VertexAttribute {
                    offset: 16,
                    shader_location: 2,
                    format: wgpu::VertexFormat::Float32x4,
                },
            ],
        };
        let text_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("legaia text pipeline"),
            layout: Some(&text_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &text_shader,
                entry_point: Some("vs_main"),
                buffers: &[text_vertex_layout],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &text_shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: config.format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                ..Default::default()
            },
            // Scene render pass binds a depth attachment; every pipeline used
            // in that pass must declare a matching depth-stencil format.
            // Text never reads or writes depth - `Always` + write disabled
            // keeps it a pure overlay pass.
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: false,
                depth_compare: wgpu::CompareFunction::Always,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });
        let initial_text_quads: u32 = 64;
        let text_vbuf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("text vertex buffer"),
            size: (initial_text_quads as u64) * 4 * 32,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let text_ibuf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("text index buffer"),
            size: (initial_text_quads as u64) * 6 * 4,
            usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let depth_view = create_depth_view(&device, config.width, config.height);

        Ok(Self {
            surface,
            device,
            queue,
            config,
            pipeline,
            sampler,
            bind_group_layout,
            uniforms_buf,
            uniforms_bg,
            mesh_pipeline,
            mesh_uniforms_buf,
            mesh_uniforms_bg,
            textured_mesh_pipeline,
            vram_mesh_pipeline,
            vram_mesh_blend_pipelines,
            vram_bgl,
            scene_vram_mesh_pipeline,
            scene_vram_mesh_blend_pipelines,
            scene_lines_pipeline,
            scene_color_mesh_pipeline,
            scene_color_mesh_blend_pipelines,
            scene_uniforms_bgl,
            scene_uniforms_bg: std::cell::RefCell::new(scene_uniforms_bg),
            scene_uniforms_buf: std::cell::RefCell::new(scene_uniforms_buf),
            scene_uniforms_capacity: std::cell::Cell::new(initial_scene_capacity),
            uniform_offset_alignment,
            lines_pipeline,
            text_pipeline,
            text_atlas_bgl,
            text_sampler,
            text_vbuf: std::cell::RefCell::new(text_vbuf),
            text_ibuf: std::cell::RefCell::new(text_ibuf),
            text_vertex_capacity: std::cell::Cell::new(initial_text_quads * 4),
            text_index_capacity: std::cell::Cell::new(initial_text_quads * 6),
            scene_quad_ranges: std::cell::RefCell::new(Vec::new()),
            depth_view,
            psx_mode: std::cell::Cell::new(false),
            vram_upload_counter: std::cell::Cell::new(0),
            tex_window: std::cell::Cell::new([0; 4]),
        })
    }

    /// Toggle PSX-style rendering on the 3D mesh pipelines. With
    /// `psx_mode = true` the pipeline mimics the retail GTE/GPU: UVs
    /// interpolate linearly in screen space (no perspective correction → the
    /// characteristic surface-warp on slanted surfaces), clip-space X/Y are
    /// snapped to integer pixels (the GTE's per-vertex jitter), and the
    /// shaded colour is ordered-dithered down to 15-bit BGR555 (the PSX
    /// framebuffer depth — see [`PSX_DITHER_WGSL`] / [`psx_dither`]). Default
    /// `false` (smooth, full-precision modern rendering).
    pub fn set_psx_mode(&self, enable: bool) {
        self.psx_mode.set(enable);
    }

    /// Read current PSX-mode flag.
    pub fn psx_mode(&self) -> bool {
        self.psx_mode.get()
    }

    /// Set the GP0(0xE2) "Texture Window setting" register state used by
    /// the VRAM-mesh fragment shader. Values are in 8-pixel steps
    /// (0..=31): `mask` selects which texel-coordinate bits get forced
    /// from `offset` (the standard PSX wrap-window mechanic). All-zero
    /// (the default) is a no-op - texel coords pass through unchanged.
    ///
    /// Retail Legaia leaves this register at zero almost everywhere; a
    /// handful of effect / scene-init scripts in the move-VM extension
    /// table touch it. Exposed primarily so that future work driving the
    /// runtime LoadImage / DMA-to-VRAM trace can replay the register
    /// state faithfully.
    pub fn set_texture_window(&self, mask_x: u8, mask_y: u8, off_x: u8, off_y: u8) {
        self.tex_window.set([
            (mask_x as u32) & 0x1F,
            (mask_y as u32) & 0x1F,
            (off_x as u32) & 0x1F,
            (off_y as u32) & 0x1F,
        ]);
    }

    /// Read the current `(mask_x, mask_y, off_x, off_y)` texture window
    /// register state, in 8-pixel steps (0..=31). All zero means no-op.
    pub fn texture_window(&self) -> [u32; 4] {
        self.tex_window.get()
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        self.config.width = width.max(1);
        self.config.height = height.max(1);
        self.surface.configure(&self.device, &self.config);
        self.depth_view = create_depth_view(&self.device, self.config.width, self.config.height);
    }

    pub fn surface_size(&self) -> (u32, u32) {
        (self.config.width, self.config.height)
    }

    pub fn upload_mesh(&self, positions: &[[f32; 3]], indices: &[u32]) -> Result<UploadedMesh> {
        if !indices.len().is_multiple_of(3) {
            anyhow::bail!("mesh index count {} is not a multiple of 3", indices.len());
        }
        if let Some(&max_idx) = indices.iter().max()
            && (max_idx as usize) >= positions.len()
        {
            anyhow::bail!("mesh index {} >= vertex count {}", max_idx, positions.len());
        }
        let vertex_buf = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("mesh vertex buffer"),
                contents: bytemuck::cast_slice(positions),
                usage: wgpu::BufferUsages::VERTEX,
            });
        let index_buf = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("mesh index buffer"),
                contents: bytemuck::cast_slice(indices),
                usage: wgpu::BufferUsages::INDEX,
            });
        Ok(UploadedMesh {
            vertex_buf,
            index_buf,
            index_count: indices.len() as u32,
        })
    }

    /// Upload a textured mesh: positions + UVs (paired by index, same length)
    /// plus triangle indices. Vertex+UV data is interleaved as `[x,y,z,u,v]`
    /// so it matches the textured-mesh pipeline's vertex layout in one buffer.
    pub fn upload_textured_mesh(
        &self,
        positions: &[[f32; 3]],
        uvs: &[[f32; 2]],
        indices: &[u32],
    ) -> Result<UploadedTexturedMesh> {
        if positions.len() != uvs.len() {
            anyhow::bail!(
                "textured mesh: positions ({}) and uvs ({}) length mismatch",
                positions.len(),
                uvs.len()
            );
        }
        if !indices.len().is_multiple_of(3) {
            anyhow::bail!(
                "textured mesh: index count {} is not a multiple of 3",
                indices.len()
            );
        }
        if let Some(&max_idx) = indices.iter().max()
            && (max_idx as usize) >= positions.len()
        {
            anyhow::bail!(
                "textured mesh index {} >= vertex count {}",
                max_idx,
                positions.len()
            );
        }
        // Interleave: [x,y,z,u,v] per vertex (5 f32 = 20 bytes, matches the
        // pipeline's 20-byte stride).
        let mut interleaved = Vec::with_capacity(positions.len() * 5);
        for (p, uv) in positions.iter().zip(uvs.iter()) {
            interleaved.push(p[0]);
            interleaved.push(p[1]);
            interleaved.push(p[2]);
            interleaved.push(uv[0]);
            interleaved.push(uv[1]);
        }
        let vertex_buf = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("textured mesh vertex buffer"),
                contents: bytemuck::cast_slice(&interleaved),
                usage: wgpu::BufferUsages::VERTEX,
            });
        let index_buf = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("textured mesh index buffer"),
                contents: bytemuck::cast_slice(indices),
                usage: wgpu::BufferUsages::INDEX,
            });
        Ok(UploadedTexturedMesh {
            vertex_buf,
            index_buf,
            index_count: indices.len() as u32,
        })
    }

    /// Upload a CPU-side [`Vram`] as a 1024×512 R16Uint texture. The fragment
    /// shader reads from it via `textureLoad` (no sampler - Uint textures
    /// aren't filterable on most backends, and PSX texture lookup is
    /// integer-exact anyway).
    pub fn upload_vram(&self, vram: &Vram) -> Result<UploadedVram> {
        let size = wgpu::Extent3d {
            width: VRAM_WIDTH as u32,
            height: VRAM_HEIGHT as u32,
            depth_or_array_layers: 1,
        };
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("psx vram"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R16Uint,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            vram.as_bytes(),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(VRAM_WIDTH as u32 * 2),
                rows_per_image: Some(VRAM_HEIGHT as u32),
            },
            size,
        );
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("psx vram bg"),
            layout: &self.vram_bgl,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&view),
            }],
        });
        let generation = self.vram_upload_counter.get() + 1;
        self.vram_upload_counter.set(generation);
        Ok(UploadedVram {
            bind_group,
            generation,
        })
    }

    /// Upload a VRAM mesh: position + per-vertex `(u, v)` (each 0..255) +
    /// per-vertex `(cba, tsb)` PSX VRAM addresses, plus triangle indices.
    /// Vertex layout matches the VRAM-mesh pipeline's 20-byte stride.
    pub fn upload_vram_mesh(
        &self,
        positions: &[[f32; 3]],
        uvs: &[[u8; 2]],
        cba_tsb: &[[u16; 2]],
        normals: &[[f32; 3]],
        indices: &[u32],
    ) -> Result<UploadedVramMesh> {
        if positions.len() != uvs.len()
            || positions.len() != cba_tsb.len()
            || positions.len() != normals.len()
        {
            anyhow::bail!(
                "vram mesh attribute length mismatch: pos={} uvs={} cba_tsb={} normals={}",
                positions.len(),
                uvs.len(),
                cba_tsb.len(),
                normals.len()
            );
        }
        if !indices.len().is_multiple_of(3) {
            anyhow::bail!(
                "vram mesh: index count {} is not a multiple of 3",
                indices.len()
            );
        }
        if let Some(&max_idx) = indices.iter().max()
            && (max_idx as usize) >= positions.len()
        {
            anyhow::bail!(
                "vram mesh index {} >= vertex count {}",
                max_idx,
                positions.len()
            );
        }
        let mut bytes = Vec::with_capacity(positions.len() * 32);
        for (((pos, uv), ct), n) in positions
            .iter()
            .zip(uvs.iter())
            .zip(cba_tsb.iter())
            .zip(normals.iter())
        {
            bytes.extend_from_slice(bytemuck::cast_slice(pos));
            // UV padded to 4 bytes (Uint8x4 - extra bytes ignored by shader).
            bytes.push(uv[0]);
            bytes.push(uv[1]);
            bytes.push(0);
            bytes.push(0);
            bytes.extend_from_slice(&ct[0].to_le_bytes());
            bytes.extend_from_slice(&ct[1].to_le_bytes());
            bytes.extend_from_slice(bytemuck::cast_slice(n));
        }
        let vertex_buf = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("vram mesh vertex buffer"),
                contents: &bytes,
                usage: wgpu::BufferUsages::VERTEX,
            });
        // Append the per-ABR-mode semi-transparent tail for the PSX-faithful
        // blend pass. The opaque pass still draws `0..indices.len()`
        // (`index_count` below), so the default path is unchanged.
        let (indices_with_tail, semi_ranges) = psx_blend::append_semi_tail(indices, cba_tsb);
        let index_buf = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("vram mesh index buffer"),
                contents: bytemuck::cast_slice(&indices_with_tail),
                usage: wgpu::BufferUsages::INDEX,
            });
        Ok(UploadedVramMesh {
            vertex_buf,
            index_buf,
            index_count: indices.len() as u32,
            semi_ranges,
        })
    }

    /// Upload an untextured vertex-colour triangle mesh: position + per-vertex
    /// `[r, g, b]` (each 0..255, alpha forced opaque) + triangle indices. The
    /// inverse of [`Self::upload_vram_mesh`] for the `F*`/`G*` props that carry
    /// colours instead of UVs ([`legaia_tmd::mesh::ColorMesh`]). Every prim is
    /// treated as opaque; use [`Self::upload_color_mesh_blended`] when the
    /// source prims carry semi-transparency (ABE) state.
    pub fn upload_color_mesh(
        &self,
        positions: &[[f32; 3]],
        colors: &[[u8; 3]],
        indices: &[u32],
    ) -> Result<UploadedColorMesh> {
        self.upload_color_mesh_blended(positions, colors, indices, &[])
    }

    /// [`Self::upload_color_mesh`] plus per-vertex PSX semi-transparency
    /// state. `blend` is index-aligned with `positions`: each entry is a
    /// blend word packing the prim's ABE enable into bit 15 and its ABR
    /// blend mode into bits 5..=6 ([`psx_blend::pack_blend_word`] - the
    /// same packing the textured path rides on the TSB attribute). All
    /// corners of a triangle must share one word (the mesh builders emit
    /// fresh per-corner vertices per prim). An empty slice means "all
    /// opaque" and is what [`Self::upload_color_mesh`] passes.
    ///
    /// Semi-transparent triangles are duplicated into a per-ABR-mode index
    /// tail ([`psx_blend::append_semi_tail_words`]) drawn by the
    /// PSX-faithful blend pass; on real hardware an untextured ABE prim
    /// blends **all** its pixels (there is no per-texel STP gate), so in
    /// PSX mode the opaque pass discards those prims entirely and the
    /// blend pass owns them. The default (non-PSX) path still draws
    /// everything opaque, unchanged.
    pub fn upload_color_mesh_blended(
        &self,
        positions: &[[f32; 3]],
        colors: &[[u8; 3]],
        indices: &[u32],
        blend: &[u16],
    ) -> Result<UploadedColorMesh> {
        if positions.len() != colors.len() {
            anyhow::bail!(
                "color mesh: positions ({}) and colors ({}) length mismatch",
                positions.len(),
                colors.len()
            );
        }
        if !blend.is_empty() && blend.len() != positions.len() {
            anyhow::bail!(
                "color mesh: positions ({}) and blend words ({}) length mismatch",
                positions.len(),
                blend.len()
            );
        }
        if !indices.len().is_multiple_of(3) {
            anyhow::bail!(
                "color mesh: index count {} is not a multiple of 3",
                indices.len()
            );
        }
        if let Some(&max_idx) = indices.iter().max()
            && (max_idx as usize) >= positions.len()
        {
            anyhow::bail!(
                "color mesh index {} >= vertex count {}",
                max_idx,
                positions.len()
            );
        }
        let mut bytes = Vec::with_capacity(positions.len() * 20);
        for (i, (pos, c)) in positions.iter().zip(colors.iter()).enumerate() {
            bytes.extend_from_slice(bytemuck::cast_slice(pos));
            bytes.push(c[0]);
            bytes.push(c[1]);
            bytes.push(c[2]);
            bytes.push(0xFF); // opaque alpha (Unorm8x4)
            let word = blend.get(i).copied().unwrap_or(0) as u32;
            bytes.extend_from_slice(&word.to_le_bytes());
        }
        let vertex_buf = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("color mesh vertex buffer"),
                contents: &bytes,
                usage: wgpu::BufferUsages::VERTEX,
            });
        // Append the per-ABR-mode semi-transparent tail for the PSX-faithful
        // blend pass. The opaque pass still draws `0..indices.len()`
        // (`index_count` below), so the default path is unchanged.
        let (indices_with_tail, semi_ranges) = if blend.is_empty() {
            (indices.to_vec(), [(0u32, 0u32); 4])
        } else {
            psx_blend::append_semi_tail_words(indices, blend)
        };
        let index_buf = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("color mesh index buffer"),
                contents: bytemuck::cast_slice(&indices_with_tail),
                usage: wgpu::BufferUsages::INDEX,
            });
        Ok(UploadedColorMesh {
            vertex_buf,
            index_buf,
            index_count: indices.len() as u32,
            semi_ranges,
        })
    }

    /// Upload a wireframe line mesh: position + per-vertex `[r, g, b, a]`
    /// (each 0..255), plus line indices. Indices form a `LineList`: every
    /// 2 indices = 1 segment.
    pub fn upload_lines(
        &self,
        positions: &[[f32; 3]],
        colors: &[[u8; 4]],
        indices: &[u32],
    ) -> Result<UploadedLines> {
        if positions.len() != colors.len() {
            anyhow::bail!(
                "lines: positions ({}) and colors ({}) length mismatch",
                positions.len(),
                colors.len()
            );
        }
        if !indices.len().is_multiple_of(2) {
            anyhow::bail!(
                "lines: index count {} is not a multiple of 2",
                indices.len()
            );
        }
        if let Some(&max_idx) = indices.iter().max()
            && (max_idx as usize) >= positions.len()
        {
            anyhow::bail!(
                "lines: index {} >= vertex count {}",
                max_idx,
                positions.len()
            );
        }
        // Interleave pos (12) + color (4) = 16 bytes/vertex.
        let mut bytes = Vec::with_capacity(positions.len() * 16);
        for (p, c) in positions.iter().zip(colors.iter()) {
            bytes.extend_from_slice(bytemuck::cast_slice(p));
            bytes.extend_from_slice(c);
        }
        let vertex_buf = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("lines vertex buffer"),
                contents: &bytes,
                usage: wgpu::BufferUsages::VERTEX,
            });
        let index_buf = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("lines index buffer"),
                contents: bytemuck::cast_slice(indices),
                usage: wgpu::BufferUsages::INDEX,
            });
        Ok(UploadedLines {
            vertex_buf,
            index_buf,
            index_count: indices.len() as u32,
        })
    }

    /// Upload a [`legaia_font::Font`]'s atlas to the GPU. Convenience wrapper
    /// around [`Self::upload_font_atlas`] that pulls dimensions and pixels
    /// from the parsed font directly. Use this when the caller is loading
    /// the dialog font; use the lower-level `upload_font_atlas` for custom
    /// atlases (debug fonts, sprite glyph sheets, etc).
    pub fn upload_font(&self, font: &legaia_font::Font) -> Result<UploadedFontAtlas> {
        let (w, h) = font.atlas_dimensions();
        self.upload_font_atlas(font.atlas_rgba(), w, h)
    }

    /// Upload a sprite atlas. Alias of [`Self::upload_font_atlas`] - sprites
    /// and font glyphs share the textured-quad pipeline (see [`SpriteDraw`]).
    pub fn upload_sprite_atlas(
        &self,
        rgba: &[u8],
        width: u32,
        height: u32,
    ) -> Result<UploadedSpriteAtlas> {
        self.upload_font_atlas(rgba, width, height)
    }

    /// Upload a font atlas. Used by the 2D text pipeline; one atlas can back
    /// many [`TextOverlay`] batches.
    pub fn upload_font_atlas(
        &self,
        rgba: &[u8],
        width: u32,
        height: u32,
    ) -> Result<UploadedFontAtlas> {
        if rgba.len() as u32 != width * height * 4 {
            anyhow::bail!(
                "font atlas RGBA length {} doesn't match {}x{} (expected {})",
                rgba.len(),
                width,
                height,
                width * height * 4
            );
        }
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("font atlas"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(width * 4),
                rows_per_image: Some(height),
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("font atlas bg"),
            layout: &self.text_atlas_bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.text_sampler),
                },
            ],
        });
        Ok(UploadedFontAtlas {
            bind_group,
            width,
            height,
        })
    }

    pub fn upload_texture(&self, rgba: &[u8], width: u32, height: u32) -> Result<UploadedTexture> {
        let expected = (width as usize) * (height as usize) * 4;
        if rgba.len() != expected {
            anyhow::bail!(
                "rgba length mismatch: got {}, expected {}",
                rgba.len(),
                expected
            );
        }
        let size = wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        };
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("uploaded texture"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(width * 4),
                rows_per_image: Some(height),
            },
            size,
        );
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("texture bg"),
            layout: &self.bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        });
        Ok(UploadedTexture {
            bind_group,
            width,
            height,
        })
    }

    /// Render the scene. Dispatches by [`RenderTarget`]:
    /// * `Clear` - clear to dark gray, no draws.
    /// * `Texture(t)` - letterboxed quad (Phase 1 TIM viewer).
    /// * `Mesh { mesh, mvp }` - depth-tested 3D mesh draw (Phase 1 TMD viewer).
    pub fn render(&self, target: RenderTarget<'_>) -> Result<()> {
        // Stage uniform writes before begin_render_pass.
        match &target {
            RenderTarget::Texture(t) => {
                let (sx, sy) =
                    letterbox_scale(self.config.width, self.config.height, t.width, t.height);
                self.queue.write_buffer(
                    &self.uniforms_buf,
                    0,
                    bytemuck::cast_slice(&[Uniforms {
                        scale: [sx, sy, 0.0, 0.0],
                    }]),
                );
            }
            RenderTarget::Mesh { mvp, .. }
            | RenderTarget::TexturedMesh { mvp, .. }
            | RenderTarget::VramMesh { mvp, .. }
            | RenderTarget::Lines { mvp, .. } => {
                let snap = if self.psx_mode.get() { 1.0f32 } else { 0.0 };
                self.queue.write_buffer(
                    &self.mesh_uniforms_buf,
                    0,
                    bytemuck::cast_slice(&[MeshUniforms {
                        mvp: mvp.to_cols_array_2d(),
                        // Light coming from upper-back-left in world space.
                        light_dir: normalize3([0.4, -0.8, 0.4]),
                        psx_params: [
                            self.config.width as f32,
                            self.config.height as f32,
                            snap,
                            snap, // .w = dither_enable (shares the psx_mode flag)
                        ],
                        tex_window: self.tex_window.get(),
                    }]),
                );
            }
            RenderTarget::Scene(scene) => {
                self.stage_scene_uniforms(scene);
                let mut overlays: Vec<&TextOverlay<'_>> = Vec::with_capacity(3);
                if let Some(s) = scene.overlay_sprites {
                    overlays.push(s);
                }
                if let Some(s) = scene.overlay_sprites_2 {
                    overlays.push(s);
                }
                if let Some(t) = scene.overlay_text {
                    overlays.push(t);
                }
                if !overlays.is_empty() {
                    self.scene_quad_ranges
                        .borrow_mut()
                        .clone_from(&self.stage_quad_overlays(&overlays));
                } else {
                    self.scene_quad_ranges.borrow_mut().clear();
                }
            }
            RenderTarget::TextOnly(overlay) => {
                self.scene_quad_ranges
                    .borrow_mut()
                    .clone_from(&self.stage_quad_overlays(&[overlay]));
            }
            RenderTarget::Clear => {}
        }

        let frame = self
            .surface
            .get_current_texture()
            .context("get current swapchain texture")?;
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut enc = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("frame encoder"),
            });
        {
            // Mesh paths use the depth attachment; texture/clear paths skip it
            // (it would just sit unused, but keeping the depth-stencil-attachment
            // optional avoids needing wgpu to validate it for 2D-only frames).
            let depth_attachment = matches!(
                target,
                RenderTarget::Mesh { .. }
                    | RenderTarget::TexturedMesh { .. }
                    | RenderTarget::VramMesh { .. }
                    | RenderTarget::Lines { .. }
                    | RenderTarget::Scene(_)
                    | RenderTarget::TextOnly(_)
            )
            .then(|| wgpu::RenderPassDepthStencilAttachment {
                view: &self.depth_view,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.0),
                    store: wgpu::StoreOp::Discard,
                }),
                stencil_ops: None,
            });
            let clear_rgba = match &target {
                RenderTarget::Scene(s) => s
                    .clear_color
                    .map(|c| wgpu::Color {
                        r: c[0] as f64,
                        g: c[1] as f64,
                        b: c[2] as f64,
                        a: c[3] as f64,
                    })
                    .unwrap_or(wgpu::Color {
                        r: 0.05,
                        g: 0.05,
                        b: 0.07,
                        a: 1.0,
                    }),
                _ => wgpu::Color {
                    r: 0.05,
                    g: 0.05,
                    b: 0.07,
                    a: 1.0,
                },
            };
            let mut rp = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("legaia frame pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(clear_rgba),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: depth_attachment,
                occlusion_query_set: None,
                timestamp_writes: None,
            });
            match target {
                RenderTarget::Clear => {}
                RenderTarget::Texture(t) => {
                    rp.set_pipeline(&self.pipeline);
                    rp.set_bind_group(0, &t.bind_group, &[]);
                    rp.set_bind_group(1, &self.uniforms_bg, &[]);
                    rp.draw(0..4, 0..1);
                }
                RenderTarget::Mesh { mesh, .. } => {
                    rp.set_pipeline(&self.mesh_pipeline);
                    rp.set_bind_group(0, &self.mesh_uniforms_bg, &[]);
                    rp.set_vertex_buffer(0, mesh.vertex_buf.slice(..));
                    rp.set_index_buffer(mesh.index_buf.slice(..), wgpu::IndexFormat::Uint32);
                    rp.draw_indexed(0..mesh.index_count, 0, 0..1);
                }
                RenderTarget::TexturedMesh { mesh, texture, .. } => {
                    rp.set_pipeline(&self.textured_mesh_pipeline);
                    rp.set_bind_group(0, &self.mesh_uniforms_bg, &[]);
                    rp.set_bind_group(1, &texture.bind_group, &[]);
                    rp.set_vertex_buffer(0, mesh.vertex_buf.slice(..));
                    rp.set_index_buffer(mesh.index_buf.slice(..), wgpu::IndexFormat::Uint32);
                    rp.draw_indexed(0..mesh.index_count, 0, 0..1);
                }
                RenderTarget::VramMesh { mesh, vram, .. } => {
                    rp.set_pipeline(&self.vram_mesh_pipeline);
                    rp.set_bind_group(0, &self.mesh_uniforms_bg, &[]);
                    rp.set_bind_group(1, &vram.bind_group, &[]);
                    rp.set_vertex_buffer(0, mesh.vertex_buf.slice(..));
                    rp.set_index_buffer(mesh.index_buf.slice(..), wgpu::IndexFormat::Uint32);
                    rp.draw_indexed(0..mesh.index_count, 0, 0..1);
                    // PSX-faithful semi-transparency blend pass (see
                    // [`psx_blend`]): re-draw the per-ABR-mode index tail
                    // with the matching blend pipeline. Gated like the rest
                    // of the faithful extras.
                    if self.psx_mode.get() && mesh.has_semi_prims() {
                        let c = psx_blend::MODE0_BLEND_CONSTANT;
                        rp.set_blend_constant(wgpu::Color {
                            r: c,
                            g: c,
                            b: c,
                            a: c,
                        });
                        for (mode, &(start, count)) in mesh.semi_ranges().iter().enumerate() {
                            if count == 0 {
                                continue;
                            }
                            rp.set_pipeline(&self.vram_mesh_blend_pipelines[mode]);
                            rp.draw_indexed(start..start + count, 0, 0..1);
                        }
                    }
                }
                RenderTarget::Lines { mesh, .. } => {
                    rp.set_pipeline(&self.lines_pipeline);
                    rp.set_bind_group(0, &self.mesh_uniforms_bg, &[]);
                    rp.set_vertex_buffer(0, mesh.vertex_buf.slice(..));
                    rp.set_index_buffer(mesh.index_buf.slice(..), wgpu::IndexFormat::Uint32);
                    rp.draw_indexed(0..mesh.index_count, 0, 0..1);
                }
                RenderTarget::Scene(scene) => {
                    let bg_borrow = self.scene_uniforms_bg.borrow();
                    let bg: &wgpu::BindGroup = &bg_borrow;
                    rp.set_pipeline(&self.scene_vram_mesh_pipeline);
                    rp.set_bind_group(1, &scene.vram.bind_group, &[]);
                    let stride = self.uniform_offset_alignment;
                    for (i, draw) in scene.draws.iter().enumerate() {
                        let off = (i as u32) * stride;
                        rp.set_bind_group(0, bg, &[off]);
                        rp.set_vertex_buffer(0, draw.mesh.vertex_buf.slice(..));
                        rp.set_index_buffer(
                            draw.mesh.index_buf.slice(..),
                            wgpu::IndexFormat::Uint32,
                        );
                        rp.draw_indexed(0..draw.mesh.index_count, 0, 0..1);
                    }
                    if let Some((lines, _mvp)) = scene.overlay_lines {
                        // Overlay-lines uniforms live at slot N (one past
                        // the last actor), staged by `stage_scene_uniforms`.
                        let off = (scene.draws.len() as u32) * stride;
                        rp.set_pipeline(&self.scene_lines_pipeline);
                        rp.set_bind_group(0, bg, &[off]);
                        rp.set_vertex_buffer(0, lines.vertex_buf.slice(..));
                        rp.set_index_buffer(lines.index_buf.slice(..), wgpu::IndexFormat::Uint32);
                        rp.draw_indexed(0..lines.index_count, 0, 0..1);
                    }
                    if !scene.color_draws.is_empty() {
                        // Untextured F*/G* props: slots follow the draws + the
                        // optional overlay-lines slot (see stage_scene_uniforms).
                        let color_base =
                            scene.draws.len() as u32 + scene.overlay_lines.is_some() as u32;
                        rp.set_pipeline(&self.scene_color_mesh_pipeline);
                        for (i, draw) in scene.color_draws.iter().enumerate() {
                            let off = (color_base + i as u32) * stride;
                            rp.set_bind_group(0, bg, &[off]);
                            rp.set_vertex_buffer(0, draw.mesh.vertex_buf.slice(..));
                            rp.set_index_buffer(
                                draw.mesh.index_buf.slice(..),
                                wgpu::IndexFormat::Uint32,
                            );
                            rp.draw_indexed(0..draw.mesh.index_count, 0, 0..1);
                        }
                    }
                    // PSX-faithful semi-transparency blend pass (see
                    // [`psx_blend`]): after every opaque draw, re-draw each
                    // actor's per-ABR-mode semi-transparent index tail with
                    // the matching blend pipeline. Runs last among the 3D
                    // draws so blended fragments (which don't write depth)
                    // can't be overwritten by later opaque geometry.
                    if self.psx_mode.get() && scene.draws.iter().any(|d| d.mesh.has_semi_prims()) {
                        let c = psx_blend::MODE0_BLEND_CONSTANT;
                        rp.set_blend_constant(wgpu::Color {
                            r: c,
                            g: c,
                            b: c,
                            a: c,
                        });
                        for (i, draw) in scene.draws.iter().enumerate() {
                            if !draw.mesh.has_semi_prims() {
                                continue;
                            }
                            let off = (i as u32) * stride;
                            rp.set_vertex_buffer(0, draw.mesh.vertex_buf.slice(..));
                            rp.set_index_buffer(
                                draw.mesh.index_buf.slice(..),
                                wgpu::IndexFormat::Uint32,
                            );
                            for (mode, &(start, count)) in
                                draw.mesh.semi_ranges().iter().enumerate()
                            {
                                if count == 0 {
                                    continue;
                                }
                                rp.set_pipeline(&self.scene_vram_mesh_blend_pipelines[mode]);
                                rp.set_bind_group(0, bg, &[off]);
                                rp.draw_indexed(start..start + count, 0, 0..1);
                            }
                        }
                    }
                    // Untextured (colour-mesh) semi-transparency blend pass:
                    // the opaque colour pass discarded every ABE prim (an
                    // untextured semi prim blends ALL its pixels - no STP
                    // gate), so re-draw each colour draw's per-ABR-mode
                    // index tail with the matching blend pipeline. Same
                    // ordering rationale as the textured blend pass above.
                    if self.psx_mode.get()
                        && scene.color_draws.iter().any(|d| d.mesh.has_semi_prims())
                    {
                        let c = psx_blend::MODE0_BLEND_CONSTANT;
                        rp.set_blend_constant(wgpu::Color {
                            r: c,
                            g: c,
                            b: c,
                            a: c,
                        });
                        let color_base =
                            scene.draws.len() as u32 + scene.overlay_lines.is_some() as u32;
                        for (i, draw) in scene.color_draws.iter().enumerate() {
                            if !draw.mesh.has_semi_prims() {
                                continue;
                            }
                            let off = (color_base + i as u32) * stride;
                            rp.set_vertex_buffer(0, draw.mesh.vertex_buf.slice(..));
                            rp.set_index_buffer(
                                draw.mesh.index_buf.slice(..),
                                wgpu::IndexFormat::Uint32,
                            );
                            for (mode, &(start, count)) in
                                draw.mesh.semi_ranges().iter().enumerate()
                            {
                                if count == 0 {
                                    continue;
                                }
                                rp.set_pipeline(&self.scene_color_mesh_blend_pipelines[mode]);
                                rp.set_bind_group(0, bg, &[off]);
                                rp.draw_indexed(start..start + count, 0, 0..1);
                            }
                        }
                    }
                    let mut overlays: Vec<&TextOverlay<'_>> = Vec::with_capacity(3);
                    if let Some(s) = scene.overlay_sprites {
                        overlays.push(s);
                    }
                    if let Some(s) = scene.overlay_sprites_2 {
                        overlays.push(s);
                    }
                    if let Some(t) = scene.overlay_text {
                        overlays.push(t);
                    }
                    if !overlays.is_empty() {
                        let ranges = self.scene_quad_ranges.borrow();
                        if !ranges.iter().all(|(_, n)| *n == 0) {
                            rp.set_pipeline(&self.text_pipeline);
                            let vbuf_borrow = self.text_vbuf.borrow();
                            let ibuf_borrow = self.text_ibuf.borrow();
                            rp.set_vertex_buffer(0, vbuf_borrow.slice(..));
                            rp.set_index_buffer(ibuf_borrow.slice(..), wgpu::IndexFormat::Uint32);
                            for (overlay, (base_quad, count)) in overlays.iter().zip(ranges.iter())
                            {
                                if *count == 0 {
                                    continue;
                                }
                                rp.set_bind_group(0, &overlay.atlas.bind_group, &[]);
                                let start = base_quad * 6;
                                let end = (base_quad + count) * 6;
                                rp.draw_indexed(start..end, 0, 0..1);
                            }
                        }
                    }
                }
                RenderTarget::TextOnly(text) => {
                    let ranges = self.scene_quad_ranges.borrow();
                    if let Some(&(base_quad, count)) = ranges.first()
                        && count > 0
                    {
                        rp.set_pipeline(&self.text_pipeline);
                        rp.set_bind_group(0, &text.atlas.bind_group, &[]);
                        let vbuf_borrow = self.text_vbuf.borrow();
                        let ibuf_borrow = self.text_ibuf.borrow();
                        rp.set_vertex_buffer(0, vbuf_borrow.slice(..));
                        rp.set_index_buffer(ibuf_borrow.slice(..), wgpu::IndexFormat::Uint32);
                        let start = base_quad * 6;
                        let end = (base_quad + count) * 6;
                        rp.draw_indexed(start..end, 0, 0..1);
                    }
                }
            }
        }
        self.queue.submit(std::iter::once(enc.finish()));
        frame.present();
        Ok(())
    }

    /// Resize the scene-uniforms buffer (and its bind group) to hold at
    /// least `slots` `MeshUniforms` entries, then write each entry.
    fn stage_scene_uniforms(&self, scene: &Scene<'_>) {
        let stride = self.uniform_offset_alignment as usize;
        let needed =
            scene.draws.len() + scene.overlay_lines.is_some() as usize + scene.color_draws.len();
        if needed == 0 {
            return;
        }
        let mut cap = self.scene_uniforms_capacity.get();
        if cap < needed {
            // Grow geometrically so we don't churn on small N.
            cap = needed.next_power_of_two().max(needed);
            let new_buf = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("scene mesh uniforms (resized)"),
                size: (cap * stride) as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let new_bg = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("scene mesh uniforms bg (resized)"),
                layout: &self.scene_uniforms_bgl,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: &new_buf,
                        offset: 0,
                        size: std::num::NonZeroU64::new(std::mem::size_of::<MeshUniforms>() as u64),
                    }),
                }],
            });
            *self.scene_uniforms_buf.borrow_mut() = new_buf;
            *self.scene_uniforms_bg.borrow_mut() = new_bg;
            self.scene_uniforms_capacity.set(cap);
        }
        // Build a flat byte buffer with one MeshUniforms entry per slot,
        // padded to `stride`. wgpu rejects overlapping writes, so we hand
        // the queue a single contiguous range.
        let total = needed * stride;
        let mut bytes = vec![0u8; total];
        let snap = if self.psx_mode.get() { 1.0f32 } else { 0.0 };
        let psx_params = [
            self.config.width as f32,
            self.config.height as f32,
            snap,
            snap, // .w = dither_enable (shares the psx_mode flag)
        ];
        let tex_window = self.tex_window.get();
        let push = |bytes: &mut [u8], slot: usize, mvp: Mat4| {
            let u = MeshUniforms {
                mvp: mvp.to_cols_array_2d(),
                light_dir: normalize3([0.4, -0.8, 0.4]),
                psx_params,
                tex_window,
            };
            let off = slot * stride;
            let n = std::mem::size_of::<MeshUniforms>();
            bytes[off..off + n].copy_from_slice(bytemuck::bytes_of(&u));
        };
        for (i, draw) in scene.draws.iter().enumerate() {
            push(&mut bytes, i, draw.mvp);
        }
        if let Some((_, mvp)) = scene.overlay_lines {
            push(&mut bytes, scene.draws.len(), mvp);
        }
        // Colour-mesh slots follow the draws + the optional overlay-lines slot.
        let color_base = scene.draws.len() + scene.overlay_lines.is_some() as usize;
        for (i, draw) in scene.color_draws.iter().enumerate() {
            push(&mut bytes, color_base + i, draw.mvp);
        }
        let buf_borrow = self.scene_uniforms_buf.borrow();
        let buf: &wgpu::Buffer = &buf_borrow;
        self.queue.write_buffer(buf, 0, &bytes);
    }
}

/// Number of quads in `text` capped at u32::MAX/6, or `None` if there's
/// nothing to draw. Pulled out so the pre-pass staging and the in-pass draw
/// agree on what counts as renderable.
fn text_quad_count(text: &TextOverlay<'_>) -> Option<u32> {
    let n = text.draws.len();
    if n == 0 {
        return None;
    }
    Some(n as u32)
}

#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct TextVertex {
    pos: [f32; 2],
    uv: [f32; 2],
    color: [f32; 4],
}

impl Renderer {
    /// Build the per-frame text vertex/index buffers from one or more 2D
    /// quad overlays (sprite batches and text batches share the same
    /// pipeline; the only per-batch difference is the bound atlas). Quads
    /// are concatenated in input order; the returned `[(base_quad, count)]`
    /// pairs let the render pass issue one `draw_indexed` per overlay with
    /// the matching atlas bind group.
    ///
    /// Pixel coords are converted to NDC using the current surface size;
    /// atlas pixel coords are converted to `[0, 1]` UVs using each
    /// overlay's atlas size.
    fn stage_quad_overlays(&self, overlays: &[&TextOverlay<'_>]) -> Vec<(u32, u32)> {
        let mut total_quads: u32 = 0;
        let mut ranges: Vec<(u32, u32)> = Vec::with_capacity(overlays.len());
        for o in overlays {
            let n = text_quad_count(o).unwrap_or(0);
            ranges.push((total_quads, n));
            total_quads = total_quads.saturating_add(n);
        }
        if total_quads == 0 {
            return ranges;
        }
        let needed_v = total_quads * 4;
        let needed_i = total_quads * 6;
        if needed_v > self.text_vertex_capacity.get() {
            let cap = needed_v.next_power_of_two().max(needed_v);
            *self.text_vbuf.borrow_mut() = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("quad2d vertex buffer (resized)"),
                size: (cap as u64) * 32,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            self.text_vertex_capacity.set(cap);
        }
        if needed_i > self.text_index_capacity.get() {
            let cap = needed_i.next_power_of_two().max(needed_i);
            *self.text_ibuf.borrow_mut() = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("quad2d index buffer (resized)"),
                size: (cap as u64) * 4,
                usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            self.text_index_capacity.set(cap);
        }

        let surf_w = self.config.width.max(1) as f32;
        let surf_h = self.config.height.max(1) as f32;

        let mut verts: Vec<TextVertex> = Vec::with_capacity(needed_v as usize);
        let mut idxs: Vec<u32> = Vec::with_capacity(needed_i as usize);
        let mut quad_idx: u32 = 0;
        for overlay in overlays {
            let atlas_w = overlay.atlas.width.max(1) as f32;
            let atlas_h = overlay.atlas.height.max(1) as f32;
            for draw in overlay.draws {
                let (dx, dy, dw, dh) = draw.dst;
                let (sx, sy, sw, sh) = draw.src;
                let nx0 = (dx as f32 / surf_w) * 2.0 - 1.0;
                let ny0 = 1.0 - (dy as f32 / surf_h) * 2.0;
                let nx1 = ((dx + dw as i32) as f32 / surf_w) * 2.0 - 1.0;
                let ny1 = 1.0 - ((dy + dh as i32) as f32 / surf_h) * 2.0;
                let u0 = sx as f32 / atlas_w;
                let v0 = sy as f32 / atlas_h;
                let u1 = (sx + sw) as f32 / atlas_w;
                let v1 = (sy + sh) as f32 / atlas_h;
                let color = draw.color;
                let base = quad_idx * 4;
                verts.push(TextVertex {
                    pos: [nx0, ny0],
                    uv: [u0, v0],
                    color,
                });
                verts.push(TextVertex {
                    pos: [nx1, ny0],
                    uv: [u1, v0],
                    color,
                });
                verts.push(TextVertex {
                    pos: [nx0, ny1],
                    uv: [u0, v1],
                    color,
                });
                verts.push(TextVertex {
                    pos: [nx1, ny1],
                    uv: [u1, v1],
                    color,
                });
                idxs.extend_from_slice(&[base, base + 2, base + 1, base + 1, base + 2, base + 3]);
                quad_idx += 1;
            }
        }
        let vbuf_borrow = self.text_vbuf.borrow();
        let ibuf_borrow = self.text_ibuf.borrow();
        self.queue
            .write_buffer(&vbuf_borrow, 0, bytemuck::cast_slice(&verts));
        self.queue
            .write_buffer(&ibuf_borrow, 0, bytemuck::cast_slice(&idxs));
        ranges
    }
}

const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

fn create_depth_view(device: &wgpu::Device, width: u32, height: u32) -> wgpu::TextureView {
    let tex = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("depth target"),
        size: wgpu::Extent3d {
            width: width.max(1),
            height: height.max(1),
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: DEPTH_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    tex.create_view(&wgpu::TextureViewDescriptor::default())
}

fn normalize3(v: [f32; 3]) -> [f32; 4] {
    let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt().max(1e-6);
    [v[0] / len, v[1] / len, v[2] / len, 0.0]
}

fn letterbox_scale(win_w: u32, win_h: u32, tex_w: u32, tex_h: u32) -> (f32, f32) {
    let win_aspect = win_w as f32 / win_h.max(1) as f32;
    let tex_aspect = tex_w as f32 / tex_h.max(1) as f32;
    if win_aspect > tex_aspect {
        // Window wider than texture - pillarbox
        (tex_aspect / win_aspect, 1.0)
    } else {
        // Window taller than texture - letterbox
        (1.0, win_aspect / tex_aspect)
    }
}

const SHADER_SRC: &str = r#"
struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

struct Uniforms {
    scale: vec4<f32>,
};
@group(1) @binding(0) var<uniform> u: Uniforms;

@vertex
fn vs_main(@builtin(vertex_index) vidx: u32) -> VertexOutput {
    // Unit quad in NDC, drawn as TriangleStrip with vertices in the order
    // (-1,-1), (1,-1), (-1,1), (1,1). The vertex_index pattern maps:
    //   vidx 0 -> (0,0)
    //   vidx 1 -> (1,0)
    //   vidx 2 -> (0,1)
    //   vidx 3 -> (1,1)
    let x_unit = f32(vidx & 1u);
    let y_unit = f32((vidx >> 1u) & 1u);
    let ndc_x = (x_unit * 2.0 - 1.0) * u.scale.x;
    let ndc_y = (y_unit * 2.0 - 1.0) * u.scale.y;

    var out: VertexOutput;
    out.position = vec4<f32>(ndc_x, ndc_y, 0.0, 1.0);
    // Flip Y for texture coordinates: image Y grows down, NDC Y grows up.
    out.uv = vec2<f32>(x_unit, 1.0 - y_unit);
    return out;
}

@group(0) @binding(0) var t_diffuse: texture_2d<f32>;
@group(0) @binding(1) var s_diffuse: sampler;

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    return textureSample(t_diffuse, s_diffuse, in.uv);
}
"#;

/// PSX 24-bit -> 15-bit ordered-dither helper, prepended to every shaded
/// 3D shader via [`compose_psx_shader`]. Mirrors [`psx_dither`] on the CPU
/// (kept in lockstep - see that module's tests). Gated on `enable` so the
/// default (non-PSX) render path is bit-unchanged.
const PSX_DITHER_WGSL: &str = r#"
// PSX GPU ordered dithering: a signed 4x4 offset is added to each 8-bit
// colour component before truncation to the 5-bit-per-channel BGR555
// framebuffer. `enable < 0.5` passes the colour through unchanged.
// Reference: nocash PSX-SPX "GPU - Dithering/Color-Depth".
fn psx_dither(rgb: vec3<f32>, frag: vec2<f32>, dither_on: f32) -> vec3<f32> {
    if (dither_on < 0.5) {
        return rgb;
    }
    var dm = array<f32, 16>(
        -4.0,  0.0, -3.0,  1.0,
         2.0, -2.0,  3.0, -1.0,
        -3.0,  1.0, -4.0,  0.0,
         3.0, -1.0,  2.0, -2.0,
    );
    let xi = u32(frag.x) & 3u;
    let yi = u32(frag.y) & 3u;
    let d = dm[yi * 4u + xi];
    var outc = vec3<f32>(0.0, 0.0, 0.0);
    for (var i = 0u; i < 3u; i = i + 1u) {
        let c8 = clamp(rgb[i] * 255.0 + d, 0.0, 255.0);
        let c5 = floor(c8 / 8.0);            // truncate to 5 bits
        let e8 = c5 * 8.0 + floor(c5 / 4.0); // expand 5->8: (c5<<3)|(c5>>2)
        outc[i] = e8 / 255.0;
    }
    return outc;
}
"#;

/// Prepend [`PSX_DITHER_WGSL`] to a shaded 3D shader source so its fragment
/// stage can call `psx_dither`. WGSL module-scope declarations are
/// order-independent, so the leading helper is valid ahead of the shader's
/// own structs and entry points.
fn compose_psx_shader(base: &str) -> String {
    format!("{PSX_DITHER_WGSL}\n{base}")
}

/// CPU mirror of the [`PSX_DITHER_WGSL`] shader helper, kept byte-for-byte
/// equivalent so the dither algorithm can be unit-tested without a GPU. The
/// PSX GPU dithers when packing a 24-bit shaded colour into the 15-bit
/// (BGR555) framebuffer: a signed 4x4 matrix offset is added to each 8-bit
/// component, the result is clamped, truncated to 5 bits, then expanded back
/// to 8 bits by bit-replication.
pub mod psx_dither {
    /// The PSX GPU's 4x4 ordered-dither offset matrix, row-major by
    /// `(y & 3, x & 3)`. Reference: nocash PSX-SPX "GPU - Dithering".
    pub const DITHER_MATRIX: [i32; 16] = [
        -4, 0, -3, 1, //
        2, -2, 3, -1, //
        -3, 1, -4, 0, //
        3, -1, 2, -2,
    ];

    /// Dither + quantize one 8-bit colour component at screen pixel
    /// `(x, y)`. Returns the 5-bit-truncated value re-expanded to 8 bits
    /// (the value the BGR555 framebuffer would read back as).
    pub fn dither_component(c8: i32, x: u32, y: u32) -> u8 {
        let d = DITHER_MATRIX[((y & 3) * 4 + (x & 3)) as usize];
        let c = (c8 + d).clamp(0, 255);
        let c5 = c >> 3; // truncate to 5 bits
        ((c5 << 3) | (c5 >> 2)) as u8
    }

    /// Dither a linear `[0, 1]` RGB triple at pixel `(x, y)`, returning the
    /// quantized triple back in `[0, 1]`.
    pub fn dither_rgb(rgb: [f32; 3], x: u32, y: u32) -> [f32; 3] {
        let mut out = [0.0f32; 3];
        for i in 0..3 {
            let c8 = (rgb[i] * 255.0).round() as i32;
            out[i] = dither_component(c8, x, y) as f32 / 255.0;
        }
        out
    }
}

/// PSX GPU semi-transparency (per-prim blend modes) for the VRAM-mesh path.
///
/// A PSX primitive is semi-transparent when its packet's ABE bit is set; for
/// textured prims the texel's own BGR555 STP bit (bit 15) then decides *per
/// pixel*: STP=1 texels blend, STP=0 texels draw opaque even inside a
/// semi-transparent prim (texel `0x0000` is never drawn at all, and `0x8000`
/// — black with STP — blends). The blend equation comes from texpage (TSB)
/// bits 5..=6 ("ABR"):
///
/// | ABR | equation            | wgpu mapping                                  |
/// |-----|---------------------|-----------------------------------------------|
/// | 0   | `0.5*B + 0.5*F`     | src=Constant, dst=Constant, Add (constant 0.5)|
/// | 1   | `B + F`             | src=One, dst=One, Add                         |
/// | 2   | `B - F`             | src=One, dst=One, ReverseSubtract             |
/// | 3   | `B + 0.25*F`        | src=One, dst=One, Add; F pre-scaled 0.25      |
///
/// (`B` = destination/background, `F` = source/foreground.) Mode 3's `0.25*F`
/// has no fixed-function factor, so the blend-pass fragment shader pre-scales
/// the output by [`src_shader_scale`] and the pipeline stays plain-additive.
///
/// The mesh builders (`legaia_tmd::mesh`) pack the per-prim ABE bit into bit
/// 15 of the per-vertex TSB attribute ([`TSB_SEMI_TRANSPARENT_BIT`]), which
/// is unused by the TMD TSB encoding. With one fixed blend state per
/// pipeline, per-texel STP inside one prim is handled with **two passes**:
/// the opaque pass draws every triangle and discards STP texels of
/// semi-transparent prims in the shader, then a blend pass re-draws only the
/// semi-transparent triangles (grouped per ABR mode by
/// [`append_semi_tail`]) and discards everything *except* STP texels. Both
/// the shader discard and the blend pass are gated on the PSX-faithful mode
/// flag ([`Renderer::set_psx_mode`]), so the default path is unchanged.
pub mod psx_blend {
    /// Bit 15 of the per-vertex TSB attribute = "prim is semi-transparent"
    /// (the TMD mode byte's ABE bit). Engine-side packing; kept in lockstep
    /// with `legaia_tmd::mesh::TSB_SEMI_TRANSPARENT_BIT`.
    pub const TSB_SEMI_TRANSPARENT_BIT: u16 = 0x8000;

    /// Blend constant bound while drawing ABR mode 0 (`0.5*B + 0.5*F`):
    /// both factors are `BlendFactor::Constant`.
    pub const MODE0_BLEND_CONSTANT: f64 = 0.5;

    /// True when the prim that produced this TSB attribute had its ABE
    /// (semi-transparency) bit set.
    pub fn prim_semi_transparent(tsb: u16) -> bool {
        tsb & TSB_SEMI_TRANSPARENT_BIT != 0
    }

    /// ABR blend mode from TSB bits 5..=6 (0..=3).
    pub fn abr_mode(tsb: u16) -> u8 {
        ((tsb >> 5) & 0x3) as u8
    }

    /// Pack a prim's semi-transparency state into a blend word using the
    /// same bit positions the textured path rides on the TSB attribute:
    /// ABE → bit 15 ([`TSB_SEMI_TRANSPARENT_BIT`]), ABR → bits 5..=6.
    /// The inverse of [`prim_semi_transparent`] + [`abr_mode`]. This is
    /// the per-vertex word [`crate::Renderer::upload_color_mesh_blended`]
    /// consumes for untextured prims (which carry no real TSB - their ABE
    /// comes from the TMD group mode byte, and the blend mode from
    /// whatever texpage/draw-env state the caller resolves; mode 0 is the
    /// PSX draw-env default).
    pub fn pack_blend_word(abe: bool, abr: u8) -> u16 {
        (if abe { TSB_SEMI_TRANSPARENT_BIT } else { 0 }) | (((abr & 0x3) as u16) << 5)
    }

    /// Foreground pre-scale applied in the blend-pass fragment shader for
    /// the given ABR mode. Only mode 3 (`B + 0.25*F`) scales; the other
    /// modes get their factors from the fixed-function blend state.
    pub fn src_shader_scale(mode: u8) -> f32 {
        if mode == 3 { 0.25 } else { 1.0 }
    }

    /// wgpu blend state for one ABR mode (see the module table). The alpha
    /// component always replaces (the surface alpha is unused).
    pub fn blend_state(mode: u8) -> wgpu::BlendState {
        use wgpu::{BlendComponent, BlendFactor, BlendOperation};
        let color = match mode {
            0 => BlendComponent {
                src_factor: BlendFactor::Constant,
                dst_factor: BlendFactor::Constant,
                operation: BlendOperation::Add,
            },
            2 => BlendComponent {
                // ReverseSubtract = dst - src = B - F.
                src_factor: BlendFactor::One,
                dst_factor: BlendFactor::One,
                operation: BlendOperation::ReverseSubtract,
            },
            // Modes 1 and 3 are both plain additive; mode 3's 0.25 factor
            // is pre-applied in the shader (`src_shader_scale`).
            _ => BlendComponent {
                src_factor: BlendFactor::One,
                dst_factor: BlendFactor::One,
                operation: BlendOperation::Add,
            },
        };
        wgpu::BlendState {
            color,
            alpha: wgpu::BlendComponent {
                src_factor: BlendFactor::One,
                dst_factor: BlendFactor::Zero,
                operation: BlendOperation::Add,
            },
        }
    }

    /// Reference PSX blend arithmetic for one colour channel (`b` =
    /// background, `f` = foreground, both `[0, 1]`). The GPU clamps the
    /// 5-bit result; so does every wgpu blend op on a normalized target.
    /// Unit tests evaluate [`blend_state`] + [`src_shader_scale`] against
    /// this to keep the pipeline mapping honest.
    pub fn blend_apply(mode: u8, b: f32, f: f32) -> f32 {
        let v = match mode {
            0 => 0.5 * b + 0.5 * f,
            1 => b + f,
            2 => b - f,
            _ => b + 0.25 * f,
        };
        v.clamp(0.0, 1.0)
    }

    /// Append a per-ABR-mode "semi tail" to a triangle index list: the
    /// original indices stay untouched at the front (the opaque pass draws
    /// `0..indices.len()` exactly as before), and every semi-transparent
    /// triangle is *duplicated* into one of four contiguous tail buckets,
    /// one per ABR mode. Returns the extended index list plus
    /// `[(first_index, index_count); 4]` tail ranges for the blend pass.
    ///
    /// A triangle's prim flags are read from its first vertex - the mesh
    /// builders emit fresh per-corner vertices per prim, so all corners
    /// share one `(cba, tsb)`.
    pub fn append_semi_tail(indices: &[u32], cba_tsb: &[[u16; 2]]) -> (Vec<u32>, [(u32, u32); 4]) {
        append_semi_tail_by(indices, |v| cba_tsb[v][1])
    }

    /// [`append_semi_tail`] for the untextured colour-mesh path, whose
    /// vertices carry a bare per-vertex blend word ([`pack_blend_word`])
    /// instead of a `(cba, tsb)` pair. Same bucketing semantics.
    pub fn append_semi_tail_words(indices: &[u32], blend: &[u16]) -> (Vec<u32>, [(u32, u32); 4]) {
        append_semi_tail_by(indices, |v| blend[v])
    }

    /// Shared bucketing core: `word_of` maps a vertex index to its packed
    /// blend word (ABE bit 15, ABR bits 5..=6).
    fn append_semi_tail_by(
        indices: &[u32],
        word_of: impl Fn(usize) -> u16,
    ) -> (Vec<u32>, [(u32, u32); 4]) {
        let mut buckets: [Vec<u32>; 4] = Default::default();
        for tri in indices.chunks_exact(3) {
            let word = word_of(tri[0] as usize);
            if prim_semi_transparent(word) {
                buckets[abr_mode(word) as usize].extend_from_slice(tri);
            }
        }
        let mut out = indices.to_vec();
        let mut ranges = [(0u32, 0u32); 4];
        for (mode, bucket) in buckets.iter().enumerate() {
            ranges[mode] = (out.len() as u32, bucket.len() as u32);
            out.extend_from_slice(bucket);
        }
        (out, ranges)
    }
}

/// Mesh shader: transforms positions by the host-supplied MVP, computes a
/// per-fragment normal from screen-space derivatives, lights with a single
/// directional light, and outputs a flat-shaded result. With `psx_mode`
/// (see [`Renderer::set_psx_mode`]) the result is also ordered-dithered to
/// 15-bit via [`PSX_DITHER_WGSL`]; affine warp + vertex jitter live in the
/// VRAM-mesh path.
const MESH_SHADER_SRC: &str = r#"
struct MeshUniforms {
    mvp: mat4x4<f32>,
    light_dir: vec4<f32>,
    // (viewport_w, viewport_h, snap_enable, dither_enable)
    psx_params: vec4<f32>,
    // GP0(0xE2) "Texture Window setting":
    //   .x = mask_x  (in 8-pixel steps, 0..31)
    //   .y = mask_y  (in 8-pixel steps, 0..31)
    //   .z = offset_x (in 8-pixel steps, 0..31)
    //   .w = offset_y (in 8-pixel steps, 0..31)
    // No-op when all four are zero (Legaia's default; the register only
    // gets written by some effect / scene-init scripts in retail).
    tex_window: vec4<u32>,
};
@group(0) @binding(0) var<uniform> u: MeshUniforms;

struct VsOut {
    @builtin(position) clip_pos: vec4<f32>,
    @location(0) world_pos: vec3<f32>,
};

@vertex
fn vs_main(@location(0) position: vec3<f32>) -> VsOut {
    var out: VsOut;
    out.clip_pos = u.mvp * vec4<f32>(position, 1.0);
    out.world_pos = position;
    return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    // Compute face normal from screen-space derivatives. This gives flat
    // per-triangle shading regardless of vertex normal availability - the
    // source TMDs only carry per-object normals, so true Gouraud would need
    // additional work to map normals back to verts.
    let dx = dpdx(in.world_pos);
    let dy = dpdy(in.world_pos);
    let n = normalize(cross(dx, dy));
    let l = -normalize(u.light_dir.xyz);
    let lambert = max(dot(n, l), 0.0);
    // Soft amber-tinted ambient + directional fill, matching the site theme.
    let ambient = vec3<f32>(0.18, 0.20, 0.26);
    let diffuse = vec3<f32>(0.80, 0.78, 0.70) * lambert;
    let rgb = psx_dither(ambient + diffuse, in.clip_pos.xy, u.psx_params.w);
    return vec4<f32>(rgb, 1.0);
}
"#;

/// Textured-mesh shader: same depth-tested 3D pipeline as `MESH_SHADER_SRC`,
/// but the fragment samples a bound texture (group 1) using the per-vertex
/// UVs from attribute location 1. UVs are pre-normalized to `[0, 1)` by the
/// CPU side (PSX UV bytes / 256.0). Light is applied as a multiplicative
/// shade on top of the texel.
const TEXTURED_MESH_SHADER_SRC: &str = r#"
struct MeshUniforms {
    mvp: mat4x4<f32>,
    light_dir: vec4<f32>,
    // (viewport_w, viewport_h, snap_enable, dither_enable)
    psx_params: vec4<f32>,
    // GP0(0xE2) "Texture Window setting":
    //   .x = mask_x  (in 8-pixel steps, 0..31)
    //   .y = mask_y  (in 8-pixel steps, 0..31)
    //   .z = offset_x (in 8-pixel steps, 0..31)
    //   .w = offset_y (in 8-pixel steps, 0..31)
    // No-op when all four are zero (Legaia's default; the register only
    // gets written by some effect / scene-init scripts in retail).
    tex_window: vec4<u32>,
};
@group(0) @binding(0) var<uniform> u: MeshUniforms;
@group(1) @binding(0) var t_color: texture_2d<f32>;
@group(1) @binding(1) var s_color: sampler;

struct VsOut {
    @builtin(position) clip_pos: vec4<f32>,
    @location(0) world_pos: vec3<f32>,
    @location(1) uv: vec2<f32>,
};

@vertex
fn vs_main(@location(0) position: vec3<f32>, @location(1) uv: vec2<f32>) -> VsOut {
    var out: VsOut;
    out.clip_pos = u.mvp * vec4<f32>(position, 1.0);
    out.world_pos = position;
    out.uv = uv;
    return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let dx = dpdx(in.world_pos);
    let dy = dpdy(in.world_pos);
    let n = normalize(cross(dx, dy));
    let l = -normalize(u.light_dir.xyz);
    let lambert = max(dot(n, l), 0.0);
    // Bias so unlit areas aren't pitch black (PSX hardware doesn't really
    // do per-face lighting; we just want some shape readable).
    // In PSX-faithful mode the engine's synthetic directional Lambert is
    // disabled: retail bakes lighting into the GTE-shaded vertex colours /
    // texels rather than re-lighting per frame from a made-up light dir, so
    // faithful mode shows the source data unlit. Default keeps the readable
    // ambient-biased shade.
    let shade = select(0.45 + 0.55 * lambert, 1.0, u.psx_params.z >= 0.5);
    let texel = textureSample(t_color, s_color, in.uv);
    let rgb = psx_dither(texel.rgb * shade, in.clip_pos.xy, u.psx_params.w);
    return vec4<f32>(rgb, texel.a);
}
"#;

/// VRAM-mesh shader: faithful PSX texture lookup.
///
/// Each vertex carries `(u, v, cba, tsb)` alongside its position. The
/// fragment shader computes, per-fragment:
///   * texture-page origin from `tsb` (`tpage_x = (tsb & 0xF) * 64`,
///     `tpage_y = ((tsb >> 4) & 1) * 256`),
///   * pixel-format from `tsb` bits 7..8 (0 = 4bpp, 1 = 8bpp, 2 = 15bpp),
///   * for 4/8 bpp, indexes into the in-VRAM CLUT at
///     `(cba & 0x3F) * 16, (cba >> 6) & 0x1FF`,
///   * decodes the resulting BGR555 + STP word to RGBA.
///
/// Same Lambert-with-ambient-bias shading as the textured-mesh path so the
/// silhouette stays readable; PSX hardware doesn't really do per-face
/// lighting either.
const VRAM_MESH_SHADER_SRC: &str = r#"
struct MeshUniforms {
    mvp: mat4x4<f32>,
    light_dir: vec4<f32>,
    // (viewport_w, viewport_h, snap_enable, dither_enable)
    psx_params: vec4<f32>,
    // GP0(0xE2) "Texture Window setting":
    //   .x = mask_x  (in 8-pixel steps, 0..31)
    //   .y = mask_y  (in 8-pixel steps, 0..31)
    //   .z = offset_x (in 8-pixel steps, 0..31)
    //   .w = offset_y (in 8-pixel steps, 0..31)
    // No-op when all four are zero (Legaia's default; the register only
    // gets written by some effect / scene-init scripts in retail).
    tex_window: vec4<u32>,
};
@group(0) @binding(0) var<uniform> u: MeshUniforms;
@group(1) @binding(0) var t_vram: texture_2d<u32>;

struct VsOut {
    @builtin(position) clip_pos: vec4<f32>,
    @location(0) world_pos: vec3<f32>,
    // PSX hardware does affine (linear-in-screen-space) UV interpolation,
    // not perspective-correct. WGSL's `@interpolate(linear)` gives exactly
    // that. The float UV is converted to integer texel coordinates in the
    // fragment shader.
    @location(1) @interpolate(linear) uv_affine: vec2<f32>,
    @location(2) @interpolate(flat) cba_tsb: vec2<u32>,
    @location(3) normal: vec3<f32>,
};

// Snap a clip-space x/y to the nearest integer pixel of a viewport sized
// (vp_w, vp_h). Returns the snapped clip position with z/w preserved. This
// is the GTE-style "vertex jitter" that gives PSX rendering its
// characteristic shimmer on slow-moving geometry.
fn psx_snap_clip(clip: vec4<f32>, vp_w: f32, vp_h: f32) -> vec4<f32> {
    if vp_w <= 0.0 || vp_h <= 0.0 {
        return clip;
    }
    // NDC after perspective divide.
    let ndc_x = clip.x / clip.w;
    let ndc_y = clip.y / clip.w;
    // Pixel coords (NDC -> [0, vp]).
    let px = (ndc_x * 0.5 + 0.5) * vp_w;
    let py = (ndc_y * 0.5 + 0.5) * vp_h;
    // Snap to nearest integer pixel.
    let snapped_x = floor(px + 0.5);
    let snapped_y = floor(py + 0.5);
    // Back to NDC.
    let nx = (snapped_x / vp_w) * 2.0 - 1.0;
    let ny = (snapped_y / vp_h) * 2.0 - 1.0;
    // Re-multiply by w to rebuild clip space.
    return vec4<f32>(nx * clip.w, ny * clip.w, clip.z, clip.w);
}

@vertex
fn vs_main(
    @location(0) position: vec3<f32>,
    @location(1) uv_in: vec4<u32>,
    @location(2) cba_tsb_in: vec2<u32>,
    @location(3) normal_in: vec3<f32>,
) -> VsOut {
    var out: VsOut;
    var clip = u.mvp * vec4<f32>(position, 1.0);
    if u.psx_params.z >= 0.5 {
        clip = psx_snap_clip(clip, u.psx_params.x, u.psx_params.y);
    }
    out.clip_pos = clip;
    out.world_pos = position;
    out.uv_affine = vec2<f32>(f32(uv_in.x), f32(uv_in.y));
    out.cba_tsb = cba_tsb_in;
    out.normal = normal_in;
    return out;
}

fn bgr555_to_rgba(c: u32) -> vec4<f32> {
    let r = f32(c & 0x1Fu) / 31.0;
    let g = f32((c >> 5u) & 0x1Fu) / 31.0;
    let b = f32((c >> 10u) & 0x1Fu) / 31.0;
    let stp = (c >> 15u) & 1u;
    var alpha = 1.0;
    if c == 0u && stp == 0u {
        alpha = 0.0;
    }
    return vec4<f32>(r, g, b, alpha);
}

// Fetch the raw BGR555+STP VRAM word for one fragment: texture-window
// remap, texture-page origin, 4/8/15bpp decode + CLUT lookup.
fn fetch_vram_word(uv_affine: vec2<f32>, cba: u32, tsb: u32) -> u32 {
    // Convert linearly-interpolated affine UV float -> integer texel.
    // Truncate (PSX behaviour: GP0 G3 commands transmit signed 8-bit UV
    // bytes; the rasterizer takes the integer part of the interpolated
    // position).
    var u_pix = u32(max(uv_affine.x, 0.0)) & 0xFFu;
    var v_pix = u32(max(uv_affine.y, 0.0)) & 0xFFu;
    // Apply GP0(0xE2) texture-window register, in pixel space:
    //   coord = (coord & ~(mask*8)) | ((offset & mask) * 8)
    // No-op when mask == 0 (the all-zero default) since the AND-NOT is
    // identity and the OR adds zero. Hardware reference: GPU command list
    // section "GP0(E2h) - Texture Window setting (Mask/Offset)".
    let mask_x = u.tex_window.x * 8u;
    let mask_y = u.tex_window.y * 8u;
    let off_x = (u.tex_window.z & u.tex_window.x) * 8u;
    let off_y = (u.tex_window.w & u.tex_window.y) * 8u;
    u_pix = (u_pix & (~mask_x & 0xFFu)) | (off_x & 0xFFu);
    v_pix = (v_pix & (~mask_y & 0xFFu)) | (off_y & 0xFFu);

    let tpage_x = (tsb & 0xFu) * 64u;
    let tpage_y = ((tsb >> 4u) & 1u) * 256u;
    let depth = (tsb >> 7u) & 0x3u; // 0=4bpp, 1=8bpp, 2=15bpp

    if depth == 0u {
        // 4bpp: 4 nibbles per VRAM word.
        let vx = i32(tpage_x + (u_pix >> 2u));
        let vy = i32(tpage_y + v_pix);
        let word = textureLoad(t_vram, vec2<i32>(vx, vy), 0).r;
        let nibble = u_pix & 3u;
        let pal_idx = (word >> (nibble * 4u)) & 0xFu;
        let cx = i32((cba & 0x3Fu) * 16u + pal_idx);
        let cy = i32((cba >> 6u) & 0x1FFu);
        return textureLoad(t_vram, vec2<i32>(cx, cy), 0).r;
    } else if depth == 1u {
        // 8bpp: 2 bytes per VRAM word.
        let vx = i32(tpage_x + (u_pix >> 1u));
        let vy = i32(tpage_y + v_pix);
        let word = textureLoad(t_vram, vec2<i32>(vx, vy), 0).r;
        let byte_sel = u_pix & 1u;
        let pal_idx = (word >> (byte_sel * 8u)) & 0xFFu;
        let cx = i32((cba & 0x3Fu) * 16u + pal_idx);
        let cy = i32((cba >> 6u) & 0x1FFu);
        return textureLoad(t_vram, vec2<i32>(cx, cy), 0).r;
    }
    // 15/16 bpp direct: one VRAM word per texel.
    let vx = i32(tpage_x + u_pix);
    let vy = i32(tpage_y + v_pix);
    return textureLoad(t_vram, vec2<i32>(vx, vy), 0).r;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let tsb = in.cba_tsb.y;
    let cba = in.cba_tsb.x;
    let word = fetch_vram_word(in.uv_affine, cba, tsb);
    let color = bgr555_to_rgba(word);

    // Discard fully transparent texels (PSX STP=0 with all-zero pixel) so
    // characters with cutout textures don't render solid black quads.
    if color.a <= 0.0 {
        discard;
    }

    // PSX-faithful semi-transparency, opaque pass: STP texels of a
    // semi-transparent prim (TSB bit 15 = the engine-packed ABE enable)
    // belong to the blend pass - defer them. Gated on the same faithful
    // flag as the blend pass so the default path draws everything opaque.
    if u.psx_params.z >= 0.5 && (tsb & 0x8000u) != 0u && ((word >> 15u) & 1u) == 1u {
        discard;
    }

    // Per-vertex normals smooth-shade connected geometry. Mesh-builder
    // emits the zero vector for unbinned positions (singleton triangles or
    // degenerate fallback); detect that and fall back to screen-space
    // derivatives so the result still looks shaded.
    let n_len = length(in.normal);
    var n: vec3<f32>;
    if n_len > 0.001 {
        n = in.normal / n_len;
    } else {
        let dx = dpdx(in.world_pos);
        let dy = dpdy(in.world_pos);
        n = normalize(cross(dx, dy));
    }
    let l = -normalize(u.light_dir.xyz);
    let lambert = max(dot(n, l), 0.0);
    // In PSX-faithful mode the engine's synthetic directional Lambert is
    // disabled: retail bakes lighting into the GTE-shaded vertex colours /
    // texels rather than re-lighting per frame from a made-up light dir, so
    // faithful mode shows the source data unlit. Default keeps the readable
    // ambient-biased shade.
    let shade = select(0.45 + 0.55 * lambert, 1.0, u.psx_params.z >= 0.5);
    let rgb = psx_dither(color.rgb * shade, in.clip_pos.xy, u.psx_params.w);
    return vec4<f32>(rgb, color.a);
}

// PSX semi-transparency blend pass: re-draws the semi-transparent prims
// (the per-ABR-mode index tail) keeping ONLY texels whose STP bit is set -
// the exact complement of the opaque pass's deferral. `f_scale` pre-scales
// the foreground for ABR mode 3 (`B + 0.25*F`); the rest of the equation is
// fixed-function blend state. Runs only in PSX-faithful mode, so the colour
// is unlit (matching the opaque pass's faithful branch). No dither here:
// the PSX dithers the post-blend value during the VRAM write, which a
// fixed-function blend can't reproduce without a destination read-back.
fn blend_pass_color(in: VsOut, f_scale: f32) -> vec4<f32> {
    let tsb = in.cba_tsb.y;
    let cba = in.cba_tsb.x;
    let word = fetch_vram_word(in.uv_affine, cba, tsb);
    // Texel 0x0000 never draws; STP=0 texels were already drawn opaque by
    // the first pass. 0x8000 (black + STP) correctly blends.
    if word == 0u || ((word >> 15u) & 1u) == 0u {
        discard;
    }
    let color = bgr555_to_rgba(word);
    return vec4<f32>(color.rgb * f_scale, 1.0);
}

@fragment
fn fs_blend(in: VsOut) -> @location(0) vec4<f32> {
    return blend_pass_color(in, 1.0);
}

@fragment
fn fs_blend_quarter(in: VsOut) -> @location(0) vec4<f32> {
    return blend_pass_color(in, 0.25);
}
"#;

/// Vertex-colour mesh shader: untextured `F*`/`G*` props. Each vertex carries a
/// position, an RGB colour, and a blend word (ABE bit 15 + ABR bits 5..=6,
/// see `psx_blend::pack_blend_word`), flat face-shaded (screen-space-derivative
/// normal times a Lambert term, the same ambient-biased shade as the textured /
/// VRAM paths) so the silhouette reads. No VRAM lookup - the colour comes
/// straight from the TMD's per-prim colour block (`legaia_tmd::mesh::ColorMesh`).
///
/// PSX semi-transparency (`psx_params.z >= 0.5`): an untextured ABE prim
/// blends **all** its pixels on retail hardware (no per-texel STP gate), so
/// the opaque entry discards those prims entirely and the `fs_blend` /
/// `fs_blend_quarter` entries re-draw them through the per-ABR-mode
/// fixed-function blend pipelines.
const COLOR_MESH_SHADER_SRC: &str = r#"
struct MeshUniforms {
    mvp: mat4x4<f32>,
    light_dir: vec4<f32>,
    psx_params: vec4<f32>,
    tex_window: vec4<u32>,
};
@group(0) @binding(0) var<uniform> u: MeshUniforms;

struct VsOut {
    @builtin(position) clip_pos: vec4<f32>,
    @location(0) world_pos: vec3<f32>,
    @location(1) color: vec4<f32>,
    @location(2) @interpolate(flat) blend: u32,
};

@vertex
fn vs_main(
    @location(0) position: vec3<f32>,
    @location(1) color: vec4<f32>,
    @location(2) blend: u32,
) -> VsOut {
    var out: VsOut;
    out.clip_pos = u.mvp * vec4<f32>(position, 1.0);
    out.world_pos = position;
    out.color = color;
    out.blend = blend;
    return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    // PSX-faithful mode: a semi-transparent (ABE) untextured prim blends
    // every pixel, so nothing of it belongs in the opaque pass - the blend
    // pass re-draws it from the per-ABR-mode index tail. Mirrors the
    // textured opaque pass discarding STP texels of semi prims.
    if (u.psx_params.z >= 0.5 && (in.blend & 0x8000u) != 0u) {
        discard;
    }
    let dx = dpdx(in.world_pos);
    let dy = dpdy(in.world_pos);
    let n = normalize(cross(dx, dy));
    let l = -normalize(u.light_dir.xyz);
    let lambert = max(dot(n, l), 0.0);
    // In PSX-faithful mode the engine's synthetic directional Lambert is
    // disabled: retail bakes lighting into the GTE-shaded vertex colours /
    // texels rather than re-lighting per frame from a made-up light dir, so
    // faithful mode shows the source data unlit. Default keeps the readable
    // ambient-biased shade.
    let shade = select(0.45 + 0.55 * lambert, 1.0, u.psx_params.z >= 0.5);
    let rgb = psx_dither(in.color.rgb * shade, in.clip_pos.xy, u.psx_params.w);
    return vec4<f32>(rgb, 1.0);
}

// Blend-pass entries: emit the prim colour as the foreground term F; the
// per-mode fixed-function blend state combines it with the framebuffer.
// Only runs in PSX-faithful mode, where the synthetic Lambert is off
// (shade = 1.0), and skips the dither stage like the textured blend pass
// (retail dithers the post-blend value during the VRAM write).
fn blend_pass_color(in: VsOut, f_scale: f32) -> vec4<f32> {
    return vec4<f32>(in.color.rgb * f_scale, 1.0);
}

@fragment
fn fs_blend(in: VsOut) -> @location(0) vec4<f32> {
    return blend_pass_color(in, 1.0);
}

@fragment
fn fs_blend_quarter(in: VsOut) -> @location(0) vec4<f32> {
    return blend_pass_color(in, 0.25);
}
"#;

/// Wireframe lines shader: pass per-vertex color through, output unchanged.
/// Stage geometry is unlit - there are no normals on a line - so the host
/// gets to encode whatever color signal it wants (per-record, depth-shade,
/// etc.) at upload time.
const LINES_SHADER_SRC: &str = r#"
struct MeshUniforms {
    mvp: mat4x4<f32>,
    light_dir: vec4<f32>,
    // (viewport_w, viewport_h, snap_enable, dither_enable)
    psx_params: vec4<f32>,
    // GP0(0xE2) "Texture Window setting":
    //   .x = mask_x  (in 8-pixel steps, 0..31)
    //   .y = mask_y  (in 8-pixel steps, 0..31)
    //   .z = offset_x (in 8-pixel steps, 0..31)
    //   .w = offset_y (in 8-pixel steps, 0..31)
    // No-op when all four are zero (Legaia's default; the register only
    // gets written by some effect / scene-init scripts in retail).
    tex_window: vec4<u32>,
};
@group(0) @binding(0) var<uniform> u: MeshUniforms;

struct VsOut {
    @builtin(position) clip_pos: vec4<f32>,
    @location(0) color: vec4<f32>,
};

@vertex
fn vs_main(@location(0) position: vec3<f32>, @location(1) color: vec4<f32>) -> VsOut {
    var out: VsOut;
    out.clip_pos = u.mvp * vec4<f32>(position, 1.0);
    out.color = color;
    return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    return in.color;
}
"#;

/// 2D text shader: pre-converted NDC positions + atlas UVs + per-vertex
/// RGBA tint. The fragment multiplies the tint with the sampled atlas
/// texel; the alpha-blend pipeline handles final compositing.
const TEXT_SHADER_SRC: &str = r#"
struct VsOut {
    @builtin(position) clip_pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
};

@vertex
fn vs_main(
    @location(0) pos: vec2<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) color: vec4<f32>,
) -> VsOut {
    var out: VsOut;
    out.clip_pos = vec4<f32>(pos, 0.0, 1.0);
    out.uv = uv;
    out.color = color;
    return out;
}

@group(0) @binding(0) var t_atlas: texture_2d<f32>;
@group(0) @binding(1) var s_atlas: sampler;

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let texel = textureSample(t_atlas, s_atlas, in.uv);
    return vec4<f32>(in.color.rgb * texel.rgb, in.color.a * texel.a);
}
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn letterbox_scale_pillarbox() {
        let (sx, sy) = letterbox_scale(800, 400, 100, 100);
        assert!((sx - 0.5).abs() < 1e-4, "sx={}", sx);
        assert!((sy - 1.0).abs() < 1e-4, "sy={}", sy);
    }

    #[test]
    fn letterbox_scale_letterbox() {
        let (sx, sy) = letterbox_scale(400, 800, 100, 100);
        assert!((sx - 1.0).abs() < 1e-4, "sx={}", sx);
        assert!((sy - 0.5).abs() < 1e-4, "sy={}", sy);
    }

    #[test]
    fn sprite_draws_translate_world_positions_with_anchor() {
        let reqs = vec![
            SpriteRequest {
                world_x: 5,
                world_y: 7,
                atlas_src: (16, 0, 14, 15),
                color: [1.0, 1.0, 1.0, 1.0],
            },
            SpriteRequest {
                world_x: 0,
                world_y: 0,
                atlas_src: (0, 16, 14, 15),
                color: [1.0, 0.0, 0.0, 1.0],
            },
        ];
        let draws = sprite_draws_for(&reqs, (100, 200));
        assert_eq!(draws.len(), 2);
        assert_eq!(draws[0].dst, (105, 207, 14, 15));
        assert_eq!(draws[0].src, (16, 0, 14, 15));
        assert_eq!(draws[1].dst, (100, 200, 14, 15));
        assert_eq!(draws[1].color, [1.0, 0.0, 0.0, 1.0]);
    }

    #[test]
    fn dialog_clut_color_distinct_palette() {
        let white = dialog_clut_color(0);
        let gold = dialog_clut_color(1);
        let red = dialog_clut_color(3);
        assert_eq!(white[0], 1.0);
        assert!(gold[0] > 0.9 && gold[2] < 0.5);
        assert!(red[0] > 0.9 && red[1] < 0.5);
        // Out-of-range index falls through to dim.
        let oob = dialog_clut_color(99);
        assert!(oob[0] < 0.9);
    }

    #[test]
    fn dialog_box_default_layout_origin() {
        let l = DialogBoxLayout::default();
        assert_eq!(l.origin, (8, 168));
        assert_eq!(l.line_h, 14);
    }

    #[test]
    fn dialog_box_draws_emits_one_quad_per_glyph() {
        let font = legaia_font::synthetic_for_tests();
        let glyphs = vec![
            DialogGlyphView {
                byte: b'a',
                clut: 0,
            },
            DialogGlyphView {
                byte: b'b',
                clut: 0,
            },
            DialogGlyphView {
                byte: b'c',
                clut: 1,
            },
        ];
        let layout = DialogBoxLayout::default();
        let draws = dialog_box_draws_for(&font, &glyphs, &layout);
        assert_eq!(draws.len(), 3);
        // Third glyph uses gold tint.
        assert!(draws[2].color[2] < 0.5);
    }

    #[test]
    fn dialog_box_draws_handle_newline() {
        let font = legaia_font::synthetic_for_tests();
        let glyphs = vec![
            DialogGlyphView {
                byte: b'a',
                clut: 0,
            },
            DialogGlyphView {
                byte: b'\n',
                clut: 0,
            },
            DialogGlyphView {
                byte: b'b',
                clut: 0,
            },
        ];
        let layout = DialogBoxLayout::default();
        let draws = dialog_box_draws_for(&font, &glyphs, &layout);
        // Two glyph quads (newline isn't drawn).
        assert_eq!(draws.len(), 2);
        // Second glyph y > first glyph y by at least line_h.
        assert!(draws[1].dst.1 - draws[0].dst.1 >= layout.line_h - 4);
    }

    #[test]
    fn dialog_box_draws_wrap_when_too_wide() {
        let font = legaia_font::synthetic_for_tests();
        // Tiny panel that fits maybe 2-3 glyphs per row.
        let layout = DialogBoxLayout {
            origin: (0, 0),
            size: (40, 60),
            padding: (2, 2),
            line_h: 14,
            cols: 4,
        };
        let glyphs: Vec<_> = (0..12)
            .map(|_| DialogGlyphView {
                byte: b'a',
                clut: 0,
            })
            .collect();
        let draws = dialog_box_draws_for(&font, &glyphs, &layout);
        // Expect more than one row and the y coordinates to vary.
        let rows: std::collections::HashSet<i32> = draws.iter().map(|d| d.dst.1).collect();
        assert!(rows.len() >= 2);
    }

    #[test]
    fn dialog_panel_draws_for_wrapper() {
        let font = legaia_font::synthetic_for_tests();
        let panel: Vec<(u8, u8)> = vec![(b'a', 0), (b'b', 1)];
        let layout = DialogBoxLayout::default();
        let draws = dialog_panel_draws_for(&font, &panel, &layout);
        assert_eq!(draws.len(), 2);
    }

    #[test]
    fn text_draws_translate_layout_to_screen_space() {
        let font = legaia_font::synthetic_for_tests();
        let layout = font.layout(b"Ab");
        let pen = (10, 20);
        let color = [1.0, 0.5, 0.25, 1.0];
        let draws = text_draws_for(&layout, pen, color);
        assert_eq!(draws.len(), layout.glyphs.len());
        let g0 = layout.glyphs[0];
        let d0 = draws[0];
        assert_eq!(d0.dst.0, pen.0 + g0.dst_x);
        assert_eq!(d0.dst.1, pen.1 + g0.dst_y);
        assert_eq!(d0.dst.2, g0.width);
        assert_eq!(d0.src, (g0.atlas_x, g0.atlas_y, g0.width, g0.height));
        assert_eq!(d0.color, color);
    }

    #[test]
    fn shop_draws_for_buy_mode_produces_draws() {
        let font = legaia_font::synthetic_for_tests();
        let rows = [
            ShopRow {
                label: "Healing Leaf",
                price: Some(50),
            },
            ShopRow {
                label: "Healing Fruit",
                price: Some(100),
            },
        ];
        let draws = shop_draws_for(&font, "[BUY]", &rows, 0, Some(1500), (8, 140));
        // Title + 2 rows (label + price each, cursor on row 0) + gold line
        assert!(!draws.is_empty(), "expected non-empty draw list");
    }

    #[test]
    fn shop_draws_for_confirm_mode_no_gold() {
        let font = legaia_font::synthetic_for_tests();
        let rows = [
            ShopRow {
                label: "Yes",
                price: None,
            },
            ShopRow {
                label: "No",
                price: None,
            },
        ];
        let draws = shop_draws_for(&font, "[CONFIRM?]", &rows, 0, None, (8, 140));
        assert!(!draws.is_empty());
    }

    #[test]
    fn shop_draws_for_cursor_on_second_row() {
        let font = legaia_font::synthetic_for_tests();
        let rows = [
            ShopRow {
                label: "Item A",
                price: Some(10),
            },
            ShopRow {
                label: "Item B",
                price: Some(20),
            },
        ];
        // cursor=1 → no crash
        let draws = shop_draws_for(&font, "[SELL]", &rows, 1, Some(100), (0, 0));
        assert!(!draws.is_empty());
    }

    #[test]
    fn level_up_draws_for_produces_two_line_draws() {
        let font = legaia_font::synthetic_for_tests();
        let draws = level_up_draws_for(&font, 0, 5, 10, 5, (8, 60));
        // Two non-empty lines - at minimum the title line must produce glyphs.
        assert!(!draws.is_empty());
    }

    #[test]
    fn capture_banner_draws_for_produces_glyphs_at_the_pen() {
        let font = legaia_font::synthetic_for_tests();
        let draws = capture_banner_draws_for(&font, "Captured: Spark!", (8, 40));
        assert!(!draws.is_empty(), "banner text produces glyph draws");
        assert!(
            draws.iter().all(|d| d.dst.1 >= 40),
            "all glyphs render at or below the banner pen y"
        );
    }

    #[test]
    fn level_up_draws_for_second_line_below_first() {
        let font = legaia_font::synthetic_for_tests();
        let draws = level_up_draws_for(&font, 0, 2, 10, 5, (8, 60));
        // At least two distinct Y positions (line 1 at 60, line 2 at 76).
        let y_vals: std::collections::HashSet<i32> = draws.iter().map(|d| d.dst.1).collect();
        assert!(
            y_vals.len() >= 2,
            "expected draws at two distinct y positions"
        );
    }

    #[test]
    fn battle_hud_draws_for_party_row_includes_name_hp_mp_ap() {
        let font = legaia_font::synthetic_for_tests();
        let slot = HudSlotView {
            name: "Vahn",
            is_party: true,
            alive: true,
            hp: 250,
            hp_max: 300,
            mp: 12,
            mp_max: 30,
            ap_filled: 2,
            ap_max: 5,
            status_letters: &[],
        };
        let draws = battle_hud_draws_for(&font, &[slot], &[], &[], (8, 100));
        // Row produces glyphs for name, HP, MP, AP - at minimum one draw.
        assert!(!draws.is_empty());
    }

    #[test]
    fn battle_hud_draws_for_skips_empty_slot_name() {
        let font = legaia_font::synthetic_for_tests();
        let slot = HudSlotView {
            name: "",
            is_party: true,
            alive: true,
            hp: 0,
            hp_max: 0,
            mp: 0,
            mp_max: 0,
            ap_filled: 0,
            ap_max: 0,
            status_letters: &[],
        };
        let draws = battle_hud_draws_for(&font, &[slot], &[], &[], (8, 100));
        assert!(draws.is_empty());
    }

    #[test]
    fn battle_hud_draws_for_dead_slot_shows_ko_overlay() {
        let font = legaia_font::synthetic_for_tests();
        let slot = HudSlotView {
            name: "Vahn",
            is_party: true,
            alive: false,
            hp: 0,
            hp_max: 300,
            mp: 0,
            mp_max: 30,
            ap_filled: 0,
            ap_max: 0,
            status_letters: &[],
        };
        let draws = battle_hud_draws_for(&font, &[slot], &[], &[], (8, 100));
        // Should include the K.O. label glyphs.
        assert!(!draws.is_empty());
    }

    #[test]
    fn battle_hud_draws_for_low_hp_uses_red_color() {
        let font = legaia_font::synthetic_for_tests();
        let slot = HudSlotView {
            name: "Vahn",
            is_party: true,
            alive: true,
            hp: 10,
            hp_max: 100,
            mp: 0,
            mp_max: 0,
            ap_filled: 0,
            ap_max: 0,
            status_letters: &[],
        };
        let draws = battle_hud_draws_for(&font, &[slot], &[], &[], (8, 100));
        // Find any draw with the dim/red HP coloring - red has more red than green.
        let any_red = draws.iter().any(|d| d.color[0] > d.color[1]);
        assert!(any_red, "low HP should produce a red-tinted glyph");
    }

    #[test]
    fn hp_bar_color_index_tiers_match_retail() {
        // K.O. -> 2 regardless of max.
        assert_eq!(hp_bar_color_index(0, 100, false), 2);
        // cur <= max/4 -> 9 (danger). max>>2 = 25, so 25 is still danger.
        assert_eq!(hp_bar_color_index(25, 100, false), 9);
        assert_eq!(hp_bar_color_index(1, 100, false), 9);
        // max/4 < cur <= max/2 -> 6 (caution). 26..=50.
        assert_eq!(hp_bar_color_index(26, 100, false), 6);
        assert_eq!(hp_bar_color_index(50, 100, false), 6);
        // cur > max/2 -> 7 (normal).
        assert_eq!(hp_bar_color_index(51, 100, false), 7);
        assert_eq!(hp_bar_color_index(100, 100, false), 7);
        // The status flag forces caution (6) even at full HP.
        assert_eq!(hp_bar_color_index(100, 100, true), 6);
        // ...but never overrides K.O. or danger.
        assert_eq!(hp_bar_color_index(0, 100, true), 2);
        assert_eq!(hp_bar_color_index(10, 100, true), 9);
    }

    #[test]
    fn mp_bar_color_index_tiers_match_retail() {
        // No K.O. tier: empty MP reads as danger (9), not 2.
        assert_eq!(mp_bar_color_index(0, 40), 9);
        assert_eq!(mp_bar_color_index(10, 40), 9); // cur <= max/4
        assert_eq!(mp_bar_color_index(11, 40), 6); // max/4 < cur <= max/2
        assert_eq!(mp_bar_color_index(20, 40), 6);
        assert_eq!(mp_bar_color_index(21, 40), 7); // cur > max/2
        assert_eq!(mp_bar_color_index(40, 40), 7);
    }

    #[test]
    fn battle_hud_caution_mp_uses_yellow_not_row_color() {
        let font = legaia_font::synthetic_for_tests();
        let slot = HudSlotView {
            name: "Noa",
            is_party: true,
            alive: true,
            hp: 100,
            hp_max: 100,
            mp: 15,
            mp_max: 40, // 15 is in (10, 20] -> caution -> yellow
            ap_filled: 0,
            ap_max: 0,
            status_letters: &[],
        };
        let draws = battle_hud_draws_for(&font, &[slot], &[], &[], (8, 100));
        // Yellow = [1.0, 0.95, 0.4]: high R+G, low B. Row color (white) has B==1.
        let any_yellow = draws.iter().any(|d| d.color[1] > 0.9 && d.color[2] < 0.5);
        assert!(
            any_yellow,
            "caution MP should produce a yellow-tinted glyph"
        );
    }

    #[test]
    fn battle_hud_draws_for_includes_log_lines_below_slots() {
        let font = legaia_font::synthetic_for_tests();
        let slot = HudSlotView {
            name: "Vahn",
            is_party: true,
            alive: true,
            hp: 100,
            hp_max: 100,
            mp: 0,
            mp_max: 0,
            ap_filled: 0,
            ap_max: 0,
            status_letters: &[],
        };
        let log = [HudLogView {
            text: "Vahn attacks.",
            color: [1.0, 1.0, 1.0, 1.0],
        }];
        let draws_no_log = battle_hud_draws_for(&font, &[slot], &[], &[], (8, 100));
        let n_no_log = draws_no_log.len();
        let draws_with_log = battle_hud_draws_for(&font, &[slot], &[], &log, (8, 100));
        assert!(draws_with_log.len() > n_no_log);
    }

    #[test]
    fn battle_hud_draws_for_popup_anchored_above_slot_row() {
        let font = legaia_font::synthetic_for_tests();
        let slot = HudSlotView {
            name: "Vahn",
            is_party: true,
            alive: true,
            hp: 100,
            hp_max: 100,
            mp: 0,
            mp_max: 0,
            ap_filled: 0,
            ap_max: 0,
            status_letters: &[],
        };
        let popup = HudPopupView {
            slot: 0,
            amount: 250,
            is_heal: false,
            is_crit: false,
            status_letter: None,
            alpha: 1.0,
        };
        let pen = (8, 100);
        let draws = battle_hud_draws_for(&font, &[slot], &[popup], &[], pen);
        // Find a draw whose y is above pen.1 (popup is at pen.1 - 16).
        let any_above = draws.iter().any(|d| d.dst.1 < pen.1);
        assert!(any_above, "popup should sit above the slot row");
    }

    #[test]
    fn battle_hud_draws_for_status_letters_render_above_row() {
        let font = legaia_font::synthetic_for_tests();
        let slot = HudSlotView {
            name: "Vahn",
            is_party: true,
            alive: true,
            hp: 100,
            hp_max: 100,
            mp: 0,
            mp_max: 0,
            ap_filled: 0,
            ap_max: 0,
            status_letters: b"BP",
        };
        let pen = (8, 100);
        let draws = battle_hud_draws_for(&font, &[slot], &[], &[], pen);
        // Status icons render at y - 12.
        let icons = draws.iter().filter(|d| d.dst.1 == pen.1 - 12).count();
        assert!(icons > 0, "expected status icons rendered above the row");
    }

    #[test]
    fn battle_hud_draws_for_popup_for_invalid_slot_is_dropped() {
        let font = legaia_font::synthetic_for_tests();
        let slot = HudSlotView {
            name: "Vahn",
            is_party: true,
            alive: true,
            hp: 100,
            hp_max: 100,
            mp: 0,
            mp_max: 0,
            ap_filled: 0,
            ap_max: 0,
            status_letters: &[],
        };
        let popup = HudPopupView {
            slot: 99,
            amount: 50,
            is_heal: false,
            is_crit: false,
            status_letter: None,
            alpha: 1.0,
        };
        let with_popup = battle_hud_draws_for(&font, &[slot], &[popup], &[], (8, 100));
        let no_popup = battle_hud_draws_for(&font, &[slot], &[], &[], (8, 100));
        assert_eq!(with_popup.len(), no_popup.len());
    }

    #[test]
    fn battle_hud_draws_for_heal_popup_uses_green_tint() {
        let font = legaia_font::synthetic_for_tests();
        let slot = HudSlotView {
            name: "Vahn",
            is_party: true,
            alive: true,
            hp: 100,
            hp_max: 100,
            mp: 0,
            mp_max: 0,
            ap_filled: 0,
            ap_max: 0,
            status_letters: &[],
        };
        let popup = HudPopupView {
            slot: 0,
            amount: 60,
            is_heal: true,
            is_crit: false,
            status_letter: None,
            alpha: 1.0,
        };
        let draws = battle_hud_draws_for(&font, &[slot], &[popup], &[], (8, 100));
        // Heal color is green: [0.5, 1.0, 0.5, 1.0]; any glyph with that profile.
        let any_green = draws
            .iter()
            .any(|d| d.color[1] >= 0.95 && d.color[0] < d.color[1]);
        assert!(any_green);
    }

    #[test]
    fn apply_alpha_scales_only_alpha_channel() {
        let c = [0.5, 0.6, 0.7, 1.0];
        let scaled = apply_alpha(c, 0.5);
        assert_eq!(scaled, [0.5, 0.6, 0.7, 0.5]);
    }

    #[test]
    fn title_phase_2_renders_three_rows() {
        let font = legaia_font::synthetic_for_tests();
        let draws = title_draws_for(&font, 2, 0, true, true, false, (96, 100));
        // At least three rows (NEW GAME / CONTINUE / OPTIONS) plus a cursor.
        assert!(!draws.is_empty());
    }

    #[test]
    fn title_phase_1_blink_off_is_empty() {
        let font = legaia_font::synthetic_for_tests();
        let draws = title_draws_for(&font, 1, 0, true, false, false, (96, 100));
        // blink off → no glyphs.
        assert!(draws.is_empty());
    }

    #[test]
    fn title_continue_dimmed_when_disabled() {
        let font = legaia_font::synthetic_for_tests();
        let draws = title_draws_for(&font, 2, 0, false, true, false, (96, 100));
        // dim color is [0.45,0.45,0.45]; gold is [1.0,0.85,0.3]; white is [1,1,1].
        let any_dim = draws.iter().any(|d| d.color[0] < 0.5 && d.color[3] >= 0.99);
        assert!(any_dim);
    }

    #[test]
    fn title_phase_1_press_start_suppressed_with_atlas() {
        let font = legaia_font::synthetic_for_tests();
        // Without atlas: blink_on emits the font-rendered "PRESS START".
        let without = title_draws_for(&font, 1, 0, true, true, false, (96, 100));
        assert!(
            !without.is_empty(),
            "phase 1 with blink should emit text when no atlas"
        );
        // With atlas: the title TIM's "PRESS START BUTTON" band covers
        // it, so the font overlay stays empty.
        let with_atlas = title_draws_for(&font, 1, 0, true, true, true, (96, 100));
        assert!(
            with_atlas.is_empty(),
            "phase 1 must not emit font text when title atlas is uploaded"
        );
    }

    #[test]
    fn save_select_renders_with_present_and_empty_rows() {
        let font = legaia_font::synthetic_for_tests();
        let rows = [
            SaveSelectRow {
                label: "Slot 1",
                present: true,
                party_lv: 12,
                play_time_seconds: 3 * 3600 + 5 * 60 + 7,
                money: 4500,
                location: "Town01",
            },
            SaveSelectRow {
                label: "Slot 2",
                present: false,
                party_lv: 0,
                play_time_seconds: 0,
                money: 0,
                location: "",
            },
        ];
        let draws = save_select_draws_for(&font, "LOAD", &rows, 0, None, (0, 0), 1, true);
        assert!(!draws.is_empty());
    }

    /// Each char of the title word ("Load") must be emitted at the
    /// retail-pinned dst position (stage `(35, 13)` for L; subsequent
    /// glyphs advance by `width + INTER_GLYPH_PAD` per the dialog-font
    /// widths CSV) and tinted with `SAVE_SELECT_TITLE_COLOR`, NOT pure
    /// white. Retail-pinned at sstate9 — see `SAVE_SELECT_TITLE_POS` /
    /// `SAVE_SELECT_TITLE_COLOR` doc comments. Regression-guard so a
    /// future "tidy up the centering math" patch can't silently revert
    /// the byte-equal alignment.
    #[test]
    fn save_select_title_uses_retail_pinned_pos_and_color() {
        // `synthetic_for_tests` widths are not retail's, but the layout
        // pen advances by `widths[c] + INTER_GLYPH_PAD` regardless of
        // backing font — the property under test is that the FIRST
        // glyph is placed at SAVE_SELECT_TITLE_POS in stage pixels and
        // every title glyph carries SAVE_SELECT_TITLE_COLOR. That's
        // what makes the engine port byte-equal to retail's 4-sprite
        // emit at stage (35,13)/(42,13)/(48,13)/(55,13).
        let font = legaia_font::synthetic_for_tests();
        let rows: [SaveSelectRow<'_>; 0] = [];
        let stage_origin = (0, 0);
        let stage_scale = 1u32;
        let draws = save_select_draws_for(
            &font,
            "Load",
            &rows,
            0,
            None,
            stage_origin,
            stage_scale,
            false,
        );
        // First glyph dst must equal SAVE_SELECT_TITLE_POS (1:1 stage).
        assert!(!draws.is_empty(), "title must emit at least one glyph");
        let first = &draws[0];
        assert_eq!(
            (first.dst.0, first.dst.1),
            SAVE_SELECT_TITLE_POS,
            "first title glyph must start at retail-pinned stage pos"
        );
        // First four draws are the title glyphs; assert all share the
        // retail tint (no white / gold sneaking in).
        for (i, d) in draws.iter().take(4).enumerate() {
            assert_eq!(
                d.color, SAVE_SELECT_TITLE_COLOR,
                "title glyph {i} must use SAVE_SELECT_TITLE_COLOR (retail tint)"
            );
        }
        // Sanity: the title tint is the dialog-font CLUT row 13 bright
        // text entry (206, 206, 206) at VRAM (208, 510). Locked in
        // hex/float so the constant can't drift to "true white".
        assert_eq!(
            SAVE_SELECT_TITLE_COLOR,
            [206.0 / 255.0, 206.0 / 255.0, 206.0 / 255.0, 1.0]
        );
    }

    #[test]
    fn save_select_with_confirm_prompt() {
        let font = legaia_font::synthetic_for_tests();
        let rows = [SaveSelectRow {
            label: "Slot 1",
            present: true,
            party_lv: 1,
            play_time_seconds: 0,
            money: 0,
            location: "T",
        }];
        let draws = save_select_draws_for(
            &font,
            "LOAD",
            &rows,
            0,
            Some(("Load this slot?", 0)),
            (0, 0),
            1,
            true,
        );
        assert!(!draws.is_empty());
    }

    /// Helper: build a [`SaveMenuAtlasRects`] populated with the
    /// byte-pinned retail tile coords. The unit tests use these to
    /// verify the 9-slice composition math.
    fn pinned_save_menu_rects() -> SaveMenuAtlasRects {
        SaveMenuAtlasRects {
            panel_tl: (160, 0, 4, 4),
            panel_tr: (188, 0, 4, 4),
            panel_bl: (160, 28, 4, 4),
            panel_br: (188, 28, 4, 4),
            panel_top: (164, 0, 24, 4),
            panel_bot: (164, 28, 24, 4),
            panel_left: (160, 4, 4, 21),
            panel_right: (188, 4, 4, 21),
            slot1: (33, 97, 45, 15),
            slot2: (33, 113, 45, 15),
            cursor: (152, 64, 16, 16),
            panel_interior: (128, 0, 32, 29),
            load_empty_frame: Some((200, 64, 32, 32)),
            load_portrait_by_char: [
                Some((200, 96, 16, 16)),
                Some((216, 96, 16, 16)),
                Some((232, 96, 16, 16)),
            ],
        }
    }

    #[test]
    fn save_select_sprite_cursor_anchors_to_active_pill() {
        let rects = pinned_save_menu_rects();
        // Cursor for SLOT 1 at stage_scale=2, origin=(0, 0).
        let draw_row0 = save_select_cursor_draw_for(&rects, 0, (0, 0), 2);
        // src must be the byte-pinned (152, 64, 16, 16) sprite.
        assert_eq!(draw_row0.src, (152, 64, 16, 16));
        // dst.x = SAVE_SELECT_CURSOR_POS.x = 114, ×scale=2 → 228.
        assert_eq!(draw_row0.dst.0, SAVE_SELECT_CURSOR_POS.0 * 2);
        // dst.y = SAVE_SELECT_CURSOR_POS.y = 100, ×scale=2 → 200.
        assert_eq!(draw_row0.dst.1, SAVE_SELECT_CURSOR_POS.1 * 2);
        // dst size = src size × scale = 32x32.
        assert_eq!((draw_row0.dst.2, draw_row0.dst.3), (32, 32));

        // Cursor for SLOT 2 must shift down by SLOT_PITCH_Y × scale = 34.
        let draw_row1 = save_select_cursor_draw_for(&rects, 1, (0, 0), 2);
        assert_eq!(
            draw_row1.dst.1 - draw_row0.dst.1,
            SAVE_SELECT_SLOT_PITCH_Y * 2
        );
    }

    #[test]
    fn save_select_chrome_emits_9slice_panel_and_pills() {
        let rects = pinned_save_menu_rects();
        let draws =
            save_select_chrome_draws_for(&rects, &[0, 1], SAVE_SELECT_SLOT1_POS, (10, 20), 2);
        // 3 interior tiles + 14 border tiles + 2 pills = 19.
        // (Interior: 2 full 32-wide + 1 17-wide remainder for the 81-
        //  wide panel.) Border: 4 corners + 3 top + 3 bottom + 1 top-rem
        // + 1 bot-rem + 1 left + 1 right = 14.
        assert_eq!(draws.len(), 19);
        let origin = (10, 20);
        // Interior tile #1 at stage (6, 4), sized 32x29. Screen scale=2:
        // dst = (10 + 6*2, 20 + 4*2, 32*2, 29*2) = (22, 28, 64, 58).
        assert_eq!(draws[0].dst, (22, 28, 64, 58));
        assert_eq!(draws[0].src, (128, 0, 32, 29));
        // Interior tile #3 (remainder) at stage (70, 4), sized 17x29.
        // Src width narrowed to 17 to match retail's quad sampling.
        assert_eq!(draws[2].src, (128, 0, 17, 29));
        // Top-left corner draws AFTER interior (idx 3), at stage (6, 4):
        assert_eq!(draws[3].dst, (22, 28, 8, 8));
        assert_eq!(draws[3].src, (160, 0, 4, 4));
        // Top-right corner draws after (idx 4):
        assert_eq!(draws[4].dst, (176, 28, 8, 8));
        assert_eq!(draws[4].src, (188, 0, 4, 4));
        // The 1-pixel remainder tile must use a 1-wide src rect at
        // (164, 0, 1, 4) — verifies the remainder slicing logic.
        let has_remainder = draws.iter().any(|d| d.src == (164, 0, 1, 4));
        assert!(
            has_remainder,
            "9-slice composition must include the 1-pixel top remainder tile"
        );
        // Left edge at stage (6, 4 + 4) = (6, 8), 4x21.
        // dst = (10 + 6*2, 20 + 8*2, 4*2, 21*2) = (22, 36, 8, 42).
        let left_edge = draws.iter().find(|d| d.src == (160, 4, 4, 21)).unwrap();
        assert_eq!(left_edge.dst, (22, 36, 8, 42));
        // Slot pills sit at SAVE_SELECT_SLOT1_POS = (137, 102) with
        // SAVE_SELECT_SLOT_PITCH_Y = 17 between rows; scale=2 origin (10, 20)
        // → SLOT 1 screen (10+274, 20+204) = (284, 224), size 45*2 × 15*2.
        let slot1 = draws.iter().find(|d| d.src == (33, 97, 45, 15)).unwrap();
        assert_eq!(slot1.dst.0, 10 + SAVE_SELECT_SLOT1_POS.0 * 2);
        assert_eq!(slot1.dst.1, 20 + SAVE_SELECT_SLOT1_POS.1 * 2);
        assert_eq!((slot1.dst.2, slot1.dst.3), (90, 30));
        let slot2 = draws.iter().find(|d| d.src == (33, 113, 45, 15)).unwrap();
        assert_eq!(slot2.dst.0, slot1.dst.0);
        assert_eq!(slot2.dst.1 - slot1.dst.1, SAVE_SELECT_SLOT_PITCH_Y * 2);
        // All draws use white (no gold tint — CLUT row 2 has the
        // gold gradient baked in).
        for d in &draws {
            assert_eq!(d.color, [1.0, 1.0, 1.0, 1.0]);
        }
        // origin must not get scaled out from under us when scale=2.
        for d in &draws {
            assert!(d.dst.0 >= origin.0);
            assert!(d.dst.1 >= origin.1);
        }
    }

    #[test]
    fn save_select_chrome_zero_slots_emits_panel_only() {
        let rects = pinned_save_menu_rects();
        let draws = save_select_chrome_draws_for(&rects, &[], SAVE_SELECT_SLOT1_POS, (0, 0), 1);
        // 3 interior + 14 border = 17 panel tiles, no pills.
        assert_eq!(draws.len(), 17);
    }

    #[test]
    fn save_select_chrome_selected_pill_only_draws_one_pill_at_natural_row() {
        // Retail's NowChecking + SlotPreview phases hide every pill
        // except the one the user picked, but the selected pill stays
        // pinned to its natural row position. `pills = &[1]` must emit
        // SLOT 2's sprite (at SLOT_PITCH_Y * 1 below SLOT 1) and no
        // SLOT 1 sprite.
        let rects = pinned_save_menu_rects();
        let draws = save_select_chrome_draws_for(&rects, &[1], SAVE_SELECT_SLOT1_POS, (0, 0), 1);
        // 17 panel tiles + 1 pill = 18.
        assert_eq!(draws.len(), 18);
        // SLOT 1 sprite (33, 97, 45, 15) must NOT appear.
        let any_slot1 = draws.iter().any(|d| d.src == (33, 97, 45, 15));
        assert!(!any_slot1, "SLOT 1 sprite must be suppressed");
        // SLOT 2 sprite must appear at row 1's y (SLOT1.y + PITCH).
        let slot2 = draws.iter().find(|d| d.src == (33, 113, 45, 15)).unwrap();
        assert_eq!(slot2.dst.0, SAVE_SELECT_SLOT1_POS.0);
        assert_eq!(
            slot2.dst.1,
            SAVE_SELECT_SLOT1_POS.1 + SAVE_SELECT_SLOT_PITCH_Y
        );
    }

    #[test]
    fn save_select_chrome_load_active_anchor_relocates_pill() {
        // During NowChecking / SlotPreview retail moves the SLOT 1
        // pill up to (22, 41) under the Load panel. Passing
        // SAVE_SELECT_SLOT1_POS_LOAD_ACTIVE as `pill_anchor` must
        // land the pill there instead of the Browsing position.
        let rects = pinned_save_menu_rects();
        let draws = save_select_chrome_draws_for(
            &rects,
            &[0],
            SAVE_SELECT_SLOT1_POS_LOAD_ACTIVE,
            (0, 0),
            1,
        );
        let slot1 = draws.iter().find(|d| d.src == (33, 97, 45, 15)).unwrap();
        assert_eq!(
            slot1.dst.0, SAVE_SELECT_SLOT1_POS_LOAD_ACTIVE.0,
            "Load-active anchor must relocate SLOT 1 pill X"
        );
        assert_eq!(
            slot1.dst.1, SAVE_SELECT_SLOT1_POS_LOAD_ACTIVE.1,
            "Load-active anchor must relocate SLOT 1 pill Y"
        );
    }

    #[test]
    fn save_select_chrome_tile_count_matches_retail_scan() {
        // Retail's `scan_panel_prims.py` against load_screen_ram.bin
        // returned 14 unique panel-chrome primitives + 3 interior
        // textured-gouraud quads + 4 "Load" text glyphs. Our chrome
        // emitter mirrors the 14 border + 3 interior = 17 primitive
        // count.
        let rects = pinned_save_menu_rects();
        let draws = save_select_chrome_draws_for(&rects, &[], SAVE_SELECT_SLOT1_POS, (0, 0), 1);
        assert_eq!(
            draws.len(),
            17,
            "must produce 17 panel chrome+interior tiles"
        );
    }

    #[test]
    fn encounter_banner_renders_label() {
        let font = legaia_font::synthetic_for_tests();
        let draws = encounter_banner_draws_for(&font, "Goblin x2", (100, 80));
        // ENCOUNTER! header in yellow plus body in white = at least 2 distinct colors.
        let any_yellow = draws.iter().any(|d| d.color[2] < 0.5 && d.color[0] > 0.9);
        let any_white = draws
            .iter()
            .any(|d| d.color[0] >= 0.99 && d.color[1] >= 0.99);
        assert!(any_yellow);
        assert!(any_white);
    }

    #[test]
    fn encounter_banner_empty_label_only_header() {
        let font = legaia_font::synthetic_for_tests();
        let draws = encounter_banner_draws_for(&font, "", (100, 80));
        let any_white = draws
            .iter()
            .any(|d| d.color[0] >= 0.99 && d.color[1] >= 0.99);
        assert!(!any_white); // no body line.
    }

    #[test]
    fn field_menu_draws_emit_rows_and_footer() {
        let font = legaia_font::synthetic_for_tests();
        let rows = [
            FieldMenuRowView {
                label: "Items",
                enabled: true,
            },
            FieldMenuRowView {
                label: "Equip",
                enabled: true,
            },
            FieldMenuRowView {
                label: "Save",
                enabled: false,
            },
        ];
        let draws = field_menu_draws_for(&font, &rows, 0, 1234, 90, (16, 32));
        assert!(!draws.is_empty());
        // Selected row should produce ">" cursor glyph at the row x.
        let any_gold = draws.iter().any(|d| d.color[1] > 0.7 && d.color[2] < 0.5);
        assert!(any_gold);
    }

    #[test]
    fn status_screen_draws_pack_panel() {
        let font = legaia_font::synthetic_for_tests();
        let stat_rows = [
            StatusStatRow {
                label: "STR",
                value: 12,
            },
            StatusStatRow {
                label: "DEF",
                value: 9,
            },
        ];
        let equip_rows = [("Weapon", "Bronze Sword"), ("Helmet", "(none)")];
        let panel = StatusPanelView {
            name: "Vahn",
            level: 5,
            xp: 200,
            xp_to_next: 350,
            hp: 60,
            hp_max: 60,
            mp: 24,
            mp_max: 24,
            ap: 0,
            ap_max: 4,
            stat_rows: &stat_rows,
            equip_rows: &equip_rows,
        };
        let draws = status_screen_draws_for(&font, &panel, Some("L1/R1: Switch"), (16, 32));
        assert!(!draws.is_empty());
    }

    #[test]
    fn game_over_dim_continue_when_disabled() {
        let font = legaia_font::synthetic_for_tests();
        let draws = game_over_draws_for(&font, 1, false, (100, 80));
        let any_dim = draws.iter().any(|d| d.color[0] < 0.5);
        assert!(any_dim);
    }

    #[test]
    fn options_draws_render_rows() {
        let font = legaia_font::synthetic_for_tests();
        let rows = [
            OptionsRowView {
                label: "BGM",
                value: "8/10",
            },
            OptionsRowView {
                label: "SFX",
                value: "8/10",
            },
        ];
        let draws = options_draws_for(&font, &rows, 0, (16, 32));
        assert!(!draws.is_empty());
    }

    #[test]
    fn key_rebind_awaiting_renders_dots() {
        let font = legaia_font::synthetic_for_tests();
        let rows = [("Cross", "Z"), ("Circle", "S")];
        let draws = key_rebind_draws_for(&font, &rows, 0, true, (16, 32));
        assert!(!draws.is_empty());
    }

    #[test]
    fn inventory_use_draws_render_item_rows_with_counts() {
        let font = legaia_font::synthetic_for_tests();
        let items = vec![
            InventoryItemRow {
                name: "Healing Leaf",
                count: 4,
                admissible: true,
            },
            InventoryItemRow {
                name: "Magic Leaf",
                count: 2,
                admissible: true,
            },
        ];
        let args = InventoryUseDrawArgs {
            items: &items,
            targets: &[],
            in_battle: false,
            cursor: 0,
            phase: 0,
            selected_item_name: None,
        };
        let draws = inventory_use_draws_for(&font, args, (16, 32));
        // Title + cursor + 2 rows worth of glyphs.
        assert!(!draws.is_empty());
    }

    #[test]
    fn inventory_use_draws_empty_inventory_shows_message() {
        let font = legaia_font::synthetic_for_tests();
        let args = InventoryUseDrawArgs {
            items: &[],
            targets: &[],
            in_battle: false,
            cursor: 0,
            phase: 0,
            selected_item_name: None,
        };
        let draws = inventory_use_draws_for(&font, args, (16, 32));
        // Title plus the "no usable items" line, no cursor.
        assert!(!draws.is_empty());
    }

    #[test]
    fn inventory_use_draws_target_phase_renders_target_column() {
        let font = legaia_font::synthetic_for_tests();
        let items = vec![InventoryItemRow {
            name: "Healing Leaf",
            count: 4,
            admissible: true,
        }];
        let targets = vec![InventoryTargetRow {
            name: "Vahn",
            hp: 100,
            hp_max: 200,
            mp: 10,
            mp_max: 30,
            alive: true,
        }];
        let no_target = inventory_use_draws_for(
            &font,
            InventoryUseDrawArgs {
                items: &items,
                targets: &targets,
                in_battle: true,
                cursor: 0,
                phase: 0,
                selected_item_name: None,
            },
            (16, 32),
        );
        let with_target = inventory_use_draws_for(
            &font,
            InventoryUseDrawArgs {
                items: &items,
                targets: &targets,
                in_battle: true,
                cursor: 0,
                phase: 1,
                selected_item_name: Some("Healing Leaf"),
            },
            (16, 32),
        );
        // Phase 1 layers the target column on top of the items column.
        assert!(with_target.len() > no_target.len());
    }

    #[test]
    fn equipment_session_draws_render_slot_grid_in_picker_phase() {
        let font = legaia_font::synthetic_for_tests();
        let slots = vec![
            EquipSlotRow {
                label: "Weapon",
                current_name: "Iron Sword",
            },
            EquipSlotRow {
                label: "Helmet",
                current_name: "(empty)",
            },
        ];
        let args = EquipDrawArgs {
            character_name: "Vahn",
            slots: &slots,
            candidates: &[],
            phase: EquipDrawPhase::SlotPicker,
            cursor: 0,
            active_slot: 0,
            confirm_label: None,
        };
        let draws = equipment_session_draws_for(&font, args, (16, 32));
        assert!(!draws.is_empty());
    }

    #[test]
    fn equipment_session_draws_item_picker_renders_candidate_column() {
        let font = legaia_font::synthetic_for_tests();
        let slots = vec![EquipSlotRow {
            label: "Weapon",
            current_name: "(empty)",
        }];
        let candidates = vec![
            EquipCandidateRow {
                name: "Iron Sword",
                count: 1,
                atk_delta: 5,
                udf_delta: 0,
            },
            EquipCandidateRow {
                name: "Wood Sword",
                count: 1,
                atk_delta: -2,
                udf_delta: 0,
            },
        ];
        let picker_only = equipment_session_draws_for(
            &font,
            EquipDrawArgs {
                character_name: "Vahn",
                slots: &slots,
                candidates: &candidates,
                phase: EquipDrawPhase::ItemPicker,
                cursor: 0,
                active_slot: 0,
                confirm_label: None,
            },
            (16, 32),
        );
        let no_picker = equipment_session_draws_for(
            &font,
            EquipDrawArgs {
                character_name: "Vahn",
                slots: &slots,
                candidates: &[],
                phase: EquipDrawPhase::SlotPicker,
                cursor: 0,
                active_slot: 0,
                confirm_label: None,
            },
            (16, 32),
        );
        assert!(picker_only.len() > no_picker.len());
    }

    #[test]
    fn equipment_session_draws_confirm_phase_shows_yes_no_prompt() {
        let font = legaia_font::synthetic_for_tests();
        let slots = vec![EquipSlotRow {
            label: "Weapon",
            current_name: "Iron Sword",
        }];
        let candidates = vec![EquipCandidateRow {
            name: "Steel Sword",
            count: 1,
            atk_delta: 3,
            udf_delta: 0,
        }];
        let draws = equipment_session_draws_for(
            &font,
            EquipDrawArgs {
                character_name: "Vahn",
                slots: &slots,
                candidates: &candidates,
                phase: EquipDrawPhase::Confirm,
                cursor: 0,
                active_slot: 0,
                confirm_label: Some("Equip Steel Sword?"),
            },
            (16, 32),
        );
        // Confirm draws should include candidate column glyphs.
        assert!(!draws.is_empty());
    }

    #[test]
    fn tactical_arts_editor_draws_browsing_lists_saved_chains() {
        let font = legaia_font::synthetic_for_tests();
        let saved = vec![
            ArtsChainRow {
                name: "Combo A",
                pretty_sequence: "L R D U",
            },
            ArtsChainRow {
                name: "Striker",
                pretty_sequence: "U U L R D",
            },
        ];
        let args = ArtsEditorDrawArgs {
            character_name: "Vahn",
            phase: ArtsEditorPhase::Browsing,
            saved: &saved,
            browse_cursor: 1,
            editing_pretty: "",
            editing_len: 0,
            min_len: 3,
            max_len: 7,
            naming_name: "",
            can_add_new: true,
        };
        let draws = tactical_arts_editor_draws_for(&font, args, (16, 32));
        assert!(!draws.is_empty());
    }

    #[test]
    fn tactical_arts_editor_draws_editing_shows_running_sequence() {
        let font = legaia_font::synthetic_for_tests();
        let args = ArtsEditorDrawArgs {
            character_name: "Vahn",
            phase: ArtsEditorPhase::Editing,
            saved: &[],
            browse_cursor: 0,
            editing_pretty: "L R D",
            editing_len: 3,
            min_len: 3,
            max_len: 7,
            naming_name: "",
            can_add_new: true,
        };
        let draws = tactical_arts_editor_draws_for(&font, args, (16, 32));
        // Editing emits at least: title, sequence line, two hint lines.
        assert!(!draws.is_empty());
    }

    #[test]
    fn tactical_arts_editor_draws_naming_shows_name_and_sequence() {
        let font = legaia_font::synthetic_for_tests();
        let args = ArtsEditorDrawArgs {
            character_name: "Vahn",
            phase: ArtsEditorPhase::Naming,
            saved: &[],
            browse_cursor: 0,
            editing_pretty: "L R D",
            editing_len: 3,
            min_len: 3,
            max_len: 7,
            naming_name: "Combo A",
            can_add_new: true,
        };
        let draws = tactical_arts_editor_draws_for(&font, args, (16, 32));
        assert!(!draws.is_empty());
    }

    #[test]
    fn tactical_arts_editor_draws_browse_no_new_when_full() {
        let font = legaia_font::synthetic_for_tests();
        let saved = vec![
            ArtsChainRow {
                name: "C1",
                pretty_sequence: "L R D",
            },
            ArtsChainRow {
                name: "C2",
                pretty_sequence: "L R D",
            },
        ];
        let with_new = tactical_arts_editor_draws_for(
            &font,
            ArtsEditorDrawArgs {
                character_name: "Vahn",
                phase: ArtsEditorPhase::Browsing,
                saved: &saved,
                browse_cursor: 0,
                editing_pretty: "",
                editing_len: 0,
                min_len: 3,
                max_len: 7,
                naming_name: "",
                can_add_new: true,
            },
            (16, 32),
        );
        let no_new = tactical_arts_editor_draws_for(
            &font,
            ArtsEditorDrawArgs {
                character_name: "Vahn",
                phase: ArtsEditorPhase::Browsing,
                saved: &saved,
                browse_cursor: 0,
                editing_pretty: "",
                editing_len: 0,
                min_len: 3,
                max_len: 7,
                naming_name: "",
                can_add_new: false,
            },
            (16, 32),
        );
        // Without "+ New" we have fewer glyphs (no extra row).
        assert!(with_new.len() > no_new.len());
    }

    #[test]
    fn spell_menu_draws_in_each_phase() {
        let font = legaia_font::synthetic_for_tests();
        let names = ["Vahn", "Noa"];
        let hp = [(60, 60), (50, 50)];
        let mp = [(20, 24), (24, 24)];
        let spells = [SpellRowView {
            name: "Heal",
            mp_cost: 4,
            admissible: true,
        }];
        let targets = [SpellTargetView {
            name: "Vahn",
            hp: 30,
            hp_max: 60,
            alive: true,
        }];
        let names_slice: &[&str] = &names;
        let draws = spell_menu_draws_for(
            &font,
            SpellMenuDrawArgs {
                party_names: names_slice,
                party_hp: &hp,
                party_mp: &mp,
                selected_caster: None,
                spells: &spells,
                selected_spell: None,
                targets: &targets,
                selected_target: None,
                cursor: 0,
                phase: 0,
            },
            (16, 32),
        );
        assert!(!draws.is_empty());
        // Phase 2 with all confirmed selections renders all three columns.
        let draws2 = spell_menu_draws_for(
            &font,
            SpellMenuDrawArgs {
                party_names: names_slice,
                party_hp: &hp,
                party_mp: &mp,
                selected_caster: Some(0),
                spells: &spells,
                selected_spell: Some(0),
                targets: &targets,
                selected_target: Some(0),
                cursor: 0,
                phase: 2,
            },
            (16, 32),
        );
        assert!(draws2.len() > draws.len());
    }

    // ── Load-screen NowChecking + SlotPreview rendering ──────────────────

    #[test]
    fn now_checking_panel_draws_a_9_slice_frame_at_centered_pos() {
        let rects = pinned_save_menu_rects();
        let draws = now_checking_panel_draws_for(&rects, (0, 0), 1, (0, 0));
        // 9-slice: 4 corners + N top/bot edge tiles + N left/right edge
        // tiles + interior fill (variable). At minimum we expect the
        // four corners + at least one top/bot/left/right tile each.
        assert!(
            draws.len() >= 4 + 4 + 2,
            "expected at least 10 sprites for the 9-slice panel + interior; got {}",
            draws.len()
        );
        // Every sprite's dst.x is within the panel rect bounds.
        let (px, py) = NOW_CHECKING_PANEL_POS;
        let (pw, ph) = NOW_CHECKING_PANEL_SIZE;
        for d in &draws {
            assert!(
                d.dst.0 >= px && d.dst.0 < px + pw as i32 + 4,
                "sprite dst.x {} outside panel x range [{}, {})",
                d.dst.0,
                px,
                px + pw as i32
            );
            assert!(
                d.dst.1 >= py && d.dst.1 < py + ph as i32 + 4,
                "sprite dst.y {} outside panel y range [{}, {})",
                d.dst.1,
                py,
                py + ph as i32
            );
        }
    }

    #[test]
    fn now_checking_text_emits_two_lines_at_distinct_y() {
        let font = legaia_font::synthetic_for_tests();
        let draws = now_checking_text_draws_for(&font, (0, 0), 1, (0, 0));
        // Two text lines → expect glyphs at two distinct y positions.
        let ys: std::collections::HashSet<i32> = draws.iter().map(|d| d.dst.1).collect();
        assert!(
            ys.len() >= 2,
            "expected >= 2 distinct y positions, got {ys:?}"
        );
        // Sanity check: the message starts above the second line.
        let min_y = ys.iter().min().copied().unwrap();
        let max_y = ys.iter().max().copied().unwrap();
        assert!(max_y > min_y, "line 2 must be below line 1");
    }

    #[test]
    fn slot_preview_grid_emits_one_frame_per_cell_plus_portraits_plus_cursor() {
        let rects = pinned_save_menu_rects();
        // 4 of 15 slots present, slot 0 = Vahn portrait.
        let mut cells = [SlotGridCell::default(); 15];
        cells[0] = SlotGridCell {
            present: true,
            portrait_char_id: Some(0),
        };
        cells[6] = SlotGridCell {
            present: true,
            portrait_char_id: Some(1),
        };
        cells[7] = SlotGridCell {
            present: true,
            portrait_char_id: Some(2),
        };
        cells[8] = SlotGridCell {
            present: true,
            portrait_char_id: None,
        };
        let draws = slot_preview_grid_draws_for(&rects, &cells, 0, (0, 0), 1);
        // 15 empty-frame sprites + 3 portraits (slot 8 has present=true
        // but portrait_char_id=None so no portrait sprite) + 1 cursor.
        assert_eq!(
            draws.len(),
            15 + 3 + 1,
            "expected 15 frames + 3 portraits + 1 cursor; got {}",
            draws.len()
        );
        // Cursor (the last sprite) sits to the left of slot 0's cell.
        let cursor = draws.last().unwrap();
        assert_eq!(cursor.src, rects.cursor);
        // Retail pin: cursor right edge sits 1 px shy of cell left,
        // giving a -14 (not -16) offset from the cell's top-left.
        assert_eq!(cursor.dst.0, SLOT_GRID_ORIGIN.0 - 14);
        assert_eq!(cursor.dst.1, SLOT_GRID_ORIGIN.1);
    }

    #[test]
    fn slot_preview_grid_cursor_follows_selected_slot() {
        let rects = pinned_save_menu_rects();
        let cells = [SlotGridCell::default(); 15];
        // Slot 7 = row 1 col 2.
        let draws = slot_preview_grid_draws_for(&rects, &cells, 7, (0, 0), 1);
        let cursor = draws.last().unwrap();
        let expected_x = SLOT_GRID_ORIGIN.0 + 2 * SLOT_GRID_PITCH_X - 14;
        let expected_y = SLOT_GRID_ORIGIN.1 + SLOT_GRID_PITCH_Y;
        assert_eq!(
            (cursor.dst.0, cursor.dst.1),
            (expected_x, expected_y),
            "cursor should anchor to row 1 col 2"
        );
    }

    #[test]
    fn slot_info_panel_skips_chrome_portrait_when_no_save() {
        let rects = pinned_save_menu_rects();
        let chrome_with = slot_info_panel_draws_for(
            &rects,
            Some(&SlotInfoView {
                slot_no: 1,
                location: "Drake Kingdom",
                play_time: "00:43:09",
                leader_name: "Vahn",
                leader_level: 2,
                leader_hp: (203, 221),
                leader_mp: (27, 27),
                leader_char_id: 0,
            }),
            0,
            (0, 0),
            1,
        );
        let chrome_none = slot_info_panel_draws_for(&rects, None, 0, (0, 0), 1);
        // With Some, expect the chrome PLUS one portrait sprite.
        assert!(
            chrome_with.len() > chrome_none.len(),
            "info-panel with save should emit at least one extra portrait sprite"
        );
        assert_eq!(
            chrome_with.len() - chrome_none.len(),
            1,
            "delta should be exactly the leader portrait"
        );
    }

    #[test]
    fn slot_info_panel_text_emits_all_six_lines() {
        let font = legaia_font::synthetic_for_tests();
        let info = SlotInfoView {
            slot_no: 1,
            location: "Drake Kingdom",
            play_time: "00:43:09",
            leader_name: "Vahn",
            leader_level: 2,
            leader_hp: (203, 221),
            leader_mp: (27, 27),
            leader_char_id: 0,
        };
        let draws = slot_info_panel_text_draws_for(&font, Some(&info), 0, (0, 0), 1);
        // Empty-save case must emit zero glyphs.
        assert!(slot_info_panel_text_draws_for(&font, None, 0, (0, 0), 1).is_empty());
        // The panel emits 10 distinct text rows (No, location, Time
        // label, time value, name, LV label, LV value, HP label,
        // HP value, MP label, MP value). Their y-coords cluster into
        // a few distinct rows; expect at least 4 distinct y values.
        let ys: std::collections::HashSet<i32> = draws.iter().map(|d| d.dst.1).collect();
        assert!(
            ys.len() >= 4,
            "expected >= 4 distinct y positions across the info-panel rows; got {ys:?}"
        );
    }

    #[test]
    fn slot_info_panel_slide_offset_shifts_everything_below_parked() {
        let rects = pinned_save_menu_rects();
        let font = legaia_font::synthetic_for_tests();
        let info = SlotInfoView {
            slot_no: 1,
            location: "Drake Kingdom",
            play_time: "00:43:09",
            leader_name: "Vahn",
            leader_level: 2,
            leader_hp: (203, 221),
            leader_mp: (27, 27),
            leader_char_id: 0,
        };
        let chrome_landed = slot_info_panel_draws_for(&rects, Some(&info), 0, (0, 0), 1);
        let chrome_slid = slot_info_panel_draws_for(&rects, Some(&info), 50, (0, 0), 1);
        assert_eq!(chrome_landed.len(), chrome_slid.len());
        for (a, b) in chrome_landed.iter().zip(chrome_slid.iter()) {
            assert_eq!(a.dst.0, b.dst.0, "x must not change with slide");
            assert_eq!(
                b.dst.1 - a.dst.1,
                50,
                "y must shift by exactly slide offset"
            );
        }
        let text_landed = slot_info_panel_text_draws_for(&font, Some(&info), 0, (0, 0), 1);
        let text_slid = slot_info_panel_text_draws_for(&font, Some(&info), 50, (0, 0), 1);
        assert_eq!(text_landed.len(), text_slid.len());
        for (a, b) in text_landed.iter().zip(text_slid.iter()) {
            assert_eq!(b.dst.1 - a.dst.1, 50);
        }
    }

    // ---- PSX dithering ----

    #[test]
    fn dither_matrix_is_balanced_4x4() {
        // The 16 offsets span the documented [-4, +3] range and (being a
        // balanced ordered-dither pattern) sum to a small bias near zero.
        let m = psx_dither::DITHER_MATRIX;
        assert_eq!(m.len(), 16);
        assert_eq!(*m.iter().min().unwrap(), -4);
        assert_eq!(*m.iter().max().unwrap(), 3);
        assert_eq!(m.iter().sum::<i32>(), -8);
    }

    #[test]
    fn dither_component_quantizes_to_5bit_expanded() {
        // Every output is a 5-bit value re-expanded by bit-replication:
        // (c5 << 3) | (c5 >> 2). Check the endpoints and that all outputs
        // belong to that 32-value set regardless of pixel / input.
        let valid: std::collections::HashSet<u8> =
            (0..32).map(|c5| ((c5 << 3) | (c5 >> 2)) as u8).collect();
        for c8 in 0..=255i32 {
            for y in 0..4u32 {
                for x in 0..4u32 {
                    let out = psx_dither::dither_component(c8, x, y);
                    assert!(valid.contains(&out), "c8={c8} -> {out} not a 5-bit level");
                }
            }
        }
        // Black stays black, white stays white (no offset escapes the clamp).
        assert_eq!(psx_dither::dither_component(0, 1, 1), 0);
        assert_eq!(psx_dither::dither_component(255, 1, 1), 255);
    }

    #[test]
    fn dither_varies_across_the_4x4_cell() {
        // A mid-grey that sits between two 5-bit levels must resolve to
        // different quantized values across the dither cell - that spatial
        // variation IS the dithering. Pick a value off the 5-bit grid.
        let c8 = 134; // straddles the 5-bit boundary at 136 (134-4=130, 134+3=137)
        let mut seen = std::collections::HashSet::new();
        for y in 0..4u32 {
            for x in 0..4u32 {
                seen.insert(psx_dither::dither_component(c8, x, y));
            }
        }
        assert!(seen.len() >= 2, "dither produced no spatial variation");
    }

    #[test]
    fn dither_rgb_disabled_path_is_identity_in_shader_only() {
        // The CPU helper always dithers; the *shader* gates on enable. Here
        // we just confirm the CPU triple path stays in range and quantizes.
        let out = psx_dither::dither_rgb([0.5, 0.25, 1.0], 2, 3);
        for c in out {
            assert!((0.0..=1.0).contains(&c));
        }
    }

    /// Every shaded 3D shader (with the dither helper prepended) must parse
    /// and pass naga validation - this is the GPU-free guard that the WGSL
    /// edits are well-formed, since the render pipelines can't build in CI.
    #[test]
    fn psx_shaders_parse_and_validate() {
        use wgpu::naga;
        let sources = [
            ("mesh", compose_psx_shader(MESH_SHADER_SRC)),
            (
                "textured_mesh",
                compose_psx_shader(TEXTURED_MESH_SHADER_SRC),
            ),
            ("vram_mesh", compose_psx_shader(VRAM_MESH_SHADER_SRC)),
            ("color_mesh", compose_psx_shader(COLOR_MESH_SHADER_SRC)),
        ];
        for (name, src) in sources {
            let module = naga::front::wgsl::parse_str(&src)
                .unwrap_or_else(|e| panic!("{name} shader failed to parse: {e:?}"));
            let mut validator = naga::valid::Validator::new(
                naga::valid::ValidationFlags::all(),
                naga::valid::Capabilities::all(),
            );
            validator
                .validate(&module)
                .unwrap_or_else(|e| panic!("{name} shader failed to validate: {e:?}"));
        }
    }

    /// The VRAM-mesh and colour-mesh shaders must expose the blend-pass
    /// entry points the semi-transparency pipelines compile against.
    #[test]
    fn vram_shader_has_blend_entry_points() {
        for entry in ["fs_main", "fs_blend", "fs_blend_quarter"] {
            assert!(
                VRAM_MESH_SHADER_SRC.contains(&format!("fn {entry}(")),
                "vram shader missing entry point {entry}"
            );
            assert!(
                COLOR_MESH_SHADER_SRC.contains(&format!("fn {entry}(")),
                "color mesh shader missing entry point {entry}"
            );
        }
    }

    #[test]
    fn psx_blend_semi_bit_matches_tmd_packing() {
        // `legaia_tmd::mesh::TSB_SEMI_TRANSPARENT_BIT` packs the prim ABE
        // flag into TSB bit 15; the renderer-side mirror must agree (the
        // crates deliberately don't depend on each other).
        assert_eq!(psx_blend::TSB_SEMI_TRANSPARENT_BIT, 0x8000);
        assert!(psx_blend::prim_semi_transparent(0x8000));
        assert!(psx_blend::prim_semi_transparent(0x801A));
        assert!(!psx_blend::prim_semi_transparent(0x001A));
        assert!(!psx_blend::prim_semi_transparent(0x7FFF));
    }

    #[test]
    fn psx_blend_abr_mode_extracts_tsb_bits_5_6() {
        for mode in 0u16..4 {
            // ABR sits in bits 5..=6, independent of page / depth bits.
            assert_eq!(psx_blend::abr_mode(mode << 5), mode as u8);
            assert_eq!(psx_blend::abr_mode(0x8F1F | (mode << 5)), mode as u8);
        }
    }

    #[test]
    fn psx_blend_src_scale_only_quarters_mode_3() {
        assert_eq!(psx_blend::src_shader_scale(0), 1.0);
        assert_eq!(psx_blend::src_shader_scale(1), 1.0);
        assert_eq!(psx_blend::src_shader_scale(2), 1.0);
        assert_eq!(psx_blend::src_shader_scale(3), 0.25);
    }

    /// Evaluate one wgpu blend factor as used by [`psx_blend::blend_state`]
    /// (none of the selected factors depend on the source/dest colour).
    fn eval_factor(f: wgpu::BlendFactor) -> f32 {
        match f {
            wgpu::BlendFactor::One => 1.0,
            wgpu::BlendFactor::Zero => 0.0,
            wgpu::BlendFactor::Constant => psx_blend::MODE0_BLEND_CONSTANT as f32,
            other => panic!("unexpected blend factor {other:?}"),
        }
    }

    /// Fixed-function blend evaluator: `op(src*src_factor, dst*dst_factor)`
    /// clamped to the normalized target range, exactly what the GPU ROP does.
    fn eval_blend(comp: wgpu::BlendComponent, dst: f32, src: f32) -> f32 {
        let s = src * eval_factor(comp.src_factor);
        let d = dst * eval_factor(comp.dst_factor);
        let v = match comp.operation {
            wgpu::BlendOperation::Add => d + s,
            wgpu::BlendOperation::ReverseSubtract => d - s,
            other => panic!("unexpected blend op {other:?}"),
        };
        v.clamp(0.0, 1.0)
    }

    /// blend_state(mode) + the shader-side foreground pre-scale must
    /// reproduce the PSX equations (0.5B+0.5F / B+F / B-F / B+0.25F)
    /// for every ABR mode, including the clamped corners.
    #[test]
    fn psx_blend_states_reproduce_psx_equations() {
        let samples = [
            (0.0f32, 0.0f32),
            (0.25, 0.5),
            (0.5, 0.25),
            (1.0, 1.0), // clamps modes 1 and 0's unclamped sum
            (0.1, 0.9), // clamps mode 2 (B - F < 0)
            (0.75, 0.75),
        ];
        for mode in 0u8..4 {
            let state = psx_blend::blend_state(mode);
            // Alpha always replaces - the surface alpha is unused.
            assert_eq!(state.alpha.src_factor, wgpu::BlendFactor::One);
            assert_eq!(state.alpha.dst_factor, wgpu::BlendFactor::Zero);
            assert_eq!(state.alpha.operation, wgpu::BlendOperation::Add);
            for (b, f) in samples {
                // The blend-pass fragment shader outputs F * src_shader_scale.
                let shader_out = f * psx_blend::src_shader_scale(mode);
                let got = eval_blend(state.color, b, shader_out);
                let want = psx_blend::blend_apply(mode, b, f);
                assert!(
                    (got - want).abs() < 1e-6,
                    "mode {mode} B={b} F={f}: pipeline gives {got}, PSX wants {want}"
                );
            }
        }
    }

    #[test]
    fn psx_blend_append_semi_tail_buckets_per_mode() {
        // 4 prims x 3 per-corner verts: opaque, semi ABR 0, semi ABR 2,
        // semi ABR 3 (in that order).
        let semi = psx_blend::TSB_SEMI_TRANSPARENT_BIT;
        let mut cba_tsb = Vec::new();
        for tsb in [0x001Au16, semi, semi | (2 << 5), semi | (3 << 5)] {
            cba_tsb.extend_from_slice(&[[0u16, tsb]; 3]);
        }
        let indices: Vec<u32> = (0..12).collect();
        let (out, ranges) = psx_blend::append_semi_tail(&indices, &cba_tsb);
        // Original indices untouched at the front (the opaque pass range).
        assert_eq!(&out[..12], indices.as_slice());
        // Tail: 3 semi triangles bucketed per ABR mode, mode 1 empty.
        assert_eq!(ranges[0], (12, 3));
        assert_eq!(ranges[1], (15, 0));
        assert_eq!(ranges[2], (15, 3));
        assert_eq!(ranges[3], (18, 3));
        assert_eq!(&out[12..15], &[3, 4, 5]);
        assert_eq!(&out[15..18], &[6, 7, 8]);
        assert_eq!(&out[18..21], &[9, 10, 11]);
        assert_eq!(out.len(), 21);
    }

    #[test]
    fn psx_blend_append_semi_tail_all_opaque_is_identity() {
        let cba_tsb = vec![[0u16, 0x001Au16]; 6];
        let indices: Vec<u32> = vec![0, 1, 2, 3, 4, 5];
        let (out, ranges) = psx_blend::append_semi_tail(&indices, &cba_tsb);
        assert_eq!(out, indices);
        assert_eq!(ranges, [(6, 0); 4]);
    }

    /// `pack_blend_word` must round-trip through the extractors the blend
    /// pass uses, and must agree bit-for-bit with the TSB packing the
    /// textured path rides (ABE bit 15, ABR bits 5..=6).
    #[test]
    fn psx_blend_pack_blend_word_round_trips() {
        for abr in 0u8..4 {
            let semi = psx_blend::pack_blend_word(true, abr);
            assert!(psx_blend::prim_semi_transparent(semi));
            assert_eq!(psx_blend::abr_mode(semi), abr);
            assert_eq!(semi, 0x8000 | ((abr as u16) << 5));
            let opaque = psx_blend::pack_blend_word(false, abr);
            assert!(!psx_blend::prim_semi_transparent(opaque));
            assert_eq!(psx_blend::abr_mode(opaque), abr);
        }
        // Out-of-range ABR is masked to 2 bits.
        assert_eq!(psx_blend::abr_mode(psx_blend::pack_blend_word(true, 7)), 3);
    }

    /// The word-slice variant (untextured colour-mesh path) must bucket
    /// identically to `append_semi_tail` given equivalent per-vertex words.
    #[test]
    fn psx_blend_append_semi_tail_words_buckets_per_mode() {
        // 4 prims x 3 per-corner verts: opaque, semi ABR 0, semi ABR 2,
        // semi ABR 3 (in that order) - the colour-mesh packing of the
        // textured test's TSB values.
        let mut blend = Vec::new();
        for (abe, abr) in [(false, 0u8), (true, 0), (true, 2), (true, 3)] {
            blend.extend_from_slice(&[psx_blend::pack_blend_word(abe, abr); 3]);
        }
        let indices: Vec<u32> = (0..12).collect();
        let (out, ranges) = psx_blend::append_semi_tail_words(&indices, &blend);
        // Original indices untouched at the front (the opaque pass range).
        assert_eq!(&out[..12], indices.as_slice());
        assert_eq!(ranges[0], (12, 3));
        assert_eq!(ranges[1], (15, 0));
        assert_eq!(ranges[2], (15, 3));
        assert_eq!(ranges[3], (18, 3));
        assert_eq!(&out[12..15], &[3, 4, 5]);
        assert_eq!(&out[15..18], &[6, 7, 8]);
        assert_eq!(&out[18..21], &[9, 10, 11]);

        // Cross-check against the textured-path partitioner on the same data.
        let cba_tsb: Vec<[u16; 2]> = blend.iter().map(|&w| [0u16, w]).collect();
        let (out_tsb, ranges_tsb) = psx_blend::append_semi_tail(&indices, &cba_tsb);
        assert_eq!(out, out_tsb);
        assert_eq!(ranges, ranges_tsb);
    }

    #[test]
    fn psx_blend_append_semi_tail_words_all_opaque_is_identity() {
        let blend = vec![psx_blend::pack_blend_word(false, 1); 6];
        let indices: Vec<u32> = vec![0, 1, 2, 3, 4, 5];
        let (out, ranges) = psx_blend::append_semi_tail_words(&indices, &blend);
        assert_eq!(out, indices);
        assert_eq!(ranges, [(6, 0); 4]);
    }
}
