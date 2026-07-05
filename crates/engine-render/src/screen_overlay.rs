//! Screen-space 2D overlay pass: PSX `POLY_FT4` textured quads + flat quads
//! drawn in ordering-table order (back-to-front by OT index) with per-ABR
//! semi-transparency.
//!
//! REF: FUN_8003d2c4 (retail `AddPrim` - links a packet into the software
//! ordering table at a depth bucket) / `DrawOTag` (walks the OT back-to-front)
//!
//! This is the render capability the afterimage streak
//! ([`crate::afterimage`]) and any future engine-core `screen_fx` widget
//! (iris / letterbox / panel / sprite) need but that the wgpu renderer did
//! not previously provide: the 3D mesh path has a PSX semi-transparency
//! blend pass ([`crate::psx_blend`]) but there was no way to push a *screen
//! coordinate* `POLY_FT4` that samples PSX VRAM through the same CBA/TSB
//! CLUT decode.
//!
//! ## What the retail ordering table does, and how this mirrors it
//!
//! Retail links each drawn packet into `OT[depth]` with `AddPrim`
//! (`FUN_8003d2c4`); a bucket is a LIFO singly-linked list (`AddPrim`
//! prepends). `DrawOTag` then walks the table so **higher-index (farther)
//! buckets draw first** - the classic PSX back-to-front painter's order,
//! which is what makes additive/blended prims composite correctly with no
//! depth buffer. [`order_primitives`] reproduces exactly that ordering:
//! primitives sort by `ot_index` **descending** (farthest first), ties broken
//! by submission order **descending** (later-submitted draws first - the LIFO
//! bucket). This is the same convention [`crate::psx_blend::sort_blend_list`]
//! uses for the 3D blend pass, kept deliberately in lockstep.
//!
//! ## The GPU side
//!
//! [`build_geometry`] converts an ordered primitive list into a flat NDC
//! vertex/index buffer plus a list of [`DrawRun`]s (contiguous quads sharing
//! a blend class). [`crate::Renderer`] uploads that geometry once per frame
//! and issues one draw per run, selecting the opaque pipeline or the matching
//! per-ABR blend pipeline. Textured quads sample the shared PSX VRAM texture
//! with the same 4/8/15-bpp + CLUT decode the 3D VRAM-mesh shader uses.
//!
//! ## Simplifications vs. hardware (documented, not hidden)
//!
//! A semi-transparent *textured* PSX prim honours the per-texel STP bit
//! (STP=0 texels draw opaque even inside a blended prim). This screen
//! overlay treats a semi-transparent prim as fully blended (every non-zero
//! texel goes through the ABR equation), matching how the untextured
//! colour-mesh blend path already behaves. Texel `0x0000` is still never
//! drawn. That is faithful for the afterimage trail (additive, ABR mode 1,
//! no opaque STP texels) and for flat quads; a per-texel STP split can be
//! layered on later without changing this module's public shape.

use crate::afterimage::AfterimageQuad;

/// One screen-space textured quad (PSX `POLY_FT4`) sampling PSX VRAM.
///
/// `xy` are the four corners in **surface pixels** in the retail `POLY_FT4`
/// vertex order (`v0..v3`); [`build_geometry`] converts them to NDC using the
/// surface size. `uv`/`clut`/`tpage` drive the same VRAM CLUT decode as the
/// 3D VRAM-mesh path; `color` is the 24-bit modulation colour
/// (`0x00RRGGBB`); `ot_index` is the ordering-table bucket this quad links at
/// (larger = farther = drawn earlier).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScreenQuad {
    pub xy: [(i16, i16); 4],
    pub uv: [(u8, u8); 4],
    /// GP0 CLUT field (CBA).
    pub clut: u16,
    /// GP0 texpage field (TSB) - carries the 4/8/15-bpp depth, page origin,
    /// and (bits 5..=6) the ABR blend mode used when `semi_transparent`.
    pub tpage: u16,
    /// 24-bit modulation colour `0x00RRGGBB` (`0x808080` = passthrough).
    pub color: u32,
    pub semi_transparent: bool,
    pub ot_index: u32,
}

impl ScreenQuad {
    /// ABR blend mode (0..=3) from TSB bits 5..=6 - which fixed-function
    /// blend equation this quad uses when `semi_transparent`.
    pub fn abr_mode(&self) -> u8 {
        crate::psx_blend::abr_mode(self.tpage)
    }
}

/// One screen-space **flat** (untextured, solid) quad. Used for letterbox
/// bars, iris fills, and solid UI panels (the engine-core `screen_fx` widget
/// family's non-sprite draws).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FlatQuad {
    pub xy: [(i16, i16); 4],
    /// RGBA colour, 0..=255 per channel.
    pub color: [u8; 4],
    pub semi_transparent: bool,
    /// ABR blend mode 0..=3 (only consulted when `semi_transparent`).
    pub abr_mode: u8,
    pub ot_index: u32,
}

/// A primitive linked into the screen-space ordering table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScreenPrim {
    /// Textured `POLY_FT4` sampling PSX VRAM.
    Textured(ScreenQuad),
    /// Flat solid/blended quad.
    Flat(FlatQuad),
}

impl ScreenPrim {
    /// OT bucket this primitive links at (larger = farther = drawn earlier).
    pub fn ot_index(&self) -> u32 {
        match self {
            ScreenPrim::Textured(q) => q.ot_index,
            ScreenPrim::Flat(q) => q.ot_index,
        }
    }

    /// The blend class that groups this primitive into a [`DrawRun`].
    pub fn blend_class(&self) -> BlendClass {
        match self {
            ScreenPrim::Textured(q) if q.semi_transparent => BlendClass::Semi(q.abr_mode()),
            ScreenPrim::Flat(q) if q.semi_transparent => BlendClass::Semi(q.abr_mode & 0x3),
            _ => BlendClass::Opaque,
        }
    }
}

/// Which pipeline draws a run of quads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlendClass {
    /// Opaque pipeline (replace).
    Opaque,
    /// Per-ABR semi-transparency blend pipeline (mode 0..=3).
    Semi(u8),
}

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
        semi_transparent: q.semi_transparent,
        ot_index,
    }
}

/// Return the draw order (indices into `prims`) that reproduces the retail
/// ordering-table walk: farthest OT bucket first, LIFO within a bucket.
///
/// Ties on `ot_index` resolve to **descending** submission order
/// (later-submitted draws first), matching `AddPrim`'s prepend-into-bucket +
/// `DrawOTag`'s head-first walk.
pub fn order_primitives(prims: &[ScreenPrim]) -> Vec<usize> {
    let mut order: Vec<usize> = (0..prims.len()).collect();
    order.sort_by(|&a, &b| {
        prims[b]
            .ot_index()
            .cmp(&prims[a].ot_index())
            .then(b.cmp(&a))
    });
    order
}

/// A CPU-side vertex matching the screen-overlay pipeline's vertex layout.
/// `pos` is NDC, `uv` texel coordinates (float, truncated in the shader),
/// `cba_tsb` the CLUT/texpage words (flat-interpolated), `color` the
/// per-quad modulation (textured: a `/128` factor; flat: a `/255` colour),
/// and `flags` bit 0 = textured.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct ScreenVertex {
    pub pos: [f32; 2],
    pub uv: [f32; 2],
    pub cba_tsb: [u32; 2],
    pub color: [f32; 4],
    pub flags: u32,
}

/// Byte stride of [`ScreenVertex`] in the GPU vertex buffer.
pub const SCREEN_VERTEX_STRIDE: u64 = std::mem::size_of::<ScreenVertex>() as u64;

/// `flags` bit set when a [`ScreenVertex`] belongs to a textured quad.
pub const FLAG_TEXTURED: u32 = 1;

/// One contiguous run of quads sharing a [`BlendClass`], expressed as an
/// index-buffer range. The renderer binds the run's pipeline once and issues
/// a single `draw_indexed(index_start..index_start + index_count)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DrawRun {
    pub class: BlendClass,
    pub index_start: u32,
    pub index_count: u32,
}

/// The CPU-built geometry for one screen-overlay frame: a flat NDC vertex
/// buffer, a triangle index buffer, and the ordered list of draw runs.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct OverlayGeometry {
    pub vertices: Vec<ScreenVertex>,
    pub indices: Vec<u32>,
    pub runs: Vec<DrawRun>,
}

impl OverlayGeometry {
    /// True when there is nothing to draw.
    pub fn is_empty(&self) -> bool {
        self.indices.is_empty()
    }
}

fn to_ndc(x: i16, y: i16, surf_w: f32, surf_h: f32) -> [f32; 2] {
    [
        (x as f32 / surf_w) * 2.0 - 1.0,
        1.0 - (y as f32 / surf_h) * 2.0,
    ]
}

/// Emit the four vertices of one quad (retail `v0..v3` order) into `verts`
/// and its two triangles into `idx`.
#[allow(clippy::too_many_arguments)]
fn push_quad(
    verts: &mut Vec<ScreenVertex>,
    idx: &mut Vec<u32>,
    xy: [(i16, i16); 4],
    uv: [(u8, u8); 4],
    cba_tsb: [u32; 2],
    color: [f32; 4],
    flags: u32,
    surf_w: f32,
    surf_h: f32,
) {
    let base = verts.len() as u32;
    for c in 0..4 {
        verts.push(ScreenVertex {
            pos: to_ndc(xy[c].0, xy[c].1, surf_w, surf_h),
            uv: [uv[c].0 as f32, uv[c].1 as f32],
            cba_tsb,
            color,
            flags,
        });
    }
    // POLY_FT4 = two triangles (v0,v1,v2) + (v1,v2,v3). Cull is disabled in
    // the pipeline so winding is irrelevant.
    idx.extend_from_slice(&[base, base + 1, base + 2, base + 1, base + 2, base + 3]);
}

/// Convert a 24-bit `0x00RRGGBB` modulation colour into the per-vertex `/128`
/// factor the textured shader multiplies the sampled texel by. PSX texture
/// modulation is `texel * (colour / 128)`, so the neutral `0x808080` maps to
/// factor `1.0`.
fn tex_mod_factor(color: u32) -> [f32; 4] {
    let r = ((color >> 16) & 0xFF) as f32 / 128.0;
    let g = ((color >> 8) & 0xFF) as f32 / 128.0;
    let b = (color & 0xFF) as f32 / 128.0;
    [r, g, b, 1.0]
}

/// Build one frame's screen-overlay geometry from a primitive list and the
/// surface size. Primitives are drawn in [`order_primitives`] order and
/// coalesced into [`DrawRun`]s of consecutive same-[`BlendClass`] quads.
pub fn build_geometry(prims: &[ScreenPrim], surf_w: u32, surf_h: u32) -> OverlayGeometry {
    let sw = surf_w.max(1) as f32;
    let sh = surf_h.max(1) as f32;
    let order = order_primitives(prims);

    let mut verts: Vec<ScreenVertex> = Vec::with_capacity(order.len() * 4);
    let mut idx: Vec<u32> = Vec::with_capacity(order.len() * 6);
    let mut runs: Vec<DrawRun> = Vec::new();

    for &pi in &order {
        let class = prims[pi].blend_class();
        let run_start = idx.len() as u32;
        match &prims[pi] {
            ScreenPrim::Textured(q) => push_quad(
                &mut verts,
                &mut idx,
                q.xy,
                q.uv,
                [q.clut as u32, q.tpage as u32],
                tex_mod_factor(q.color),
                FLAG_TEXTURED,
                sw,
                sh,
            ),
            ScreenPrim::Flat(q) => push_quad(
                &mut verts,
                &mut idx,
                q.xy,
                [(0, 0); 4],
                [0, 0],
                [
                    q.color[0] as f32 / 255.0,
                    q.color[1] as f32 / 255.0,
                    q.color[2] as f32 / 255.0,
                    q.color[3] as f32 / 255.0,
                ],
                0,
                sw,
                sh,
            ),
        }
        let added = idx.len() as u32 - run_start;
        match runs.last_mut() {
            Some(last) if last.class == class => last.index_count += added,
            _ => runs.push(DrawRun {
                class,
                index_start: run_start,
                index_count: added,
            }),
        }
    }

    OverlayGeometry {
        vertices: verts,
        indices: idx,
        runs,
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
    fn ordering_is_back_to_front_lifo_ties() {
        // Three quads at OT depths 10, 30, 30 submitted in order 0,1,2.
        let mk = |ot: u32| {
            ScreenPrim::Textured(ScreenQuad {
                xy: [(0, 0); 4],
                uv: [(0, 0); 4],
                clut: 0,
                tpage: 0x27,
                color: 0x808080,
                semi_transparent: true,
                ot_index: ot,
            })
        };
        let prims = [mk(10), mk(30), mk(30)];
        // Farthest bucket (30) first; within it, later-submitted (2) before
        // earlier (1); nearest bucket (10) last.
        assert_eq!(order_primitives(&prims), vec![2, 1, 0]);
    }

    #[test]
    fn build_geometry_orders_and_coalesces_runs() {
        // A streak (three additive textured quads at increasing depth) plus a
        // nearer opaque flat panel. Draw order: farthest textured first
        // (coalesced into ONE semi run), opaque flat last.
        let streak: Vec<ScreenPrim> = (0..3)
            .map(|i| {
                let q =
                    build_afterimage_quad([(50, 60), (70, 60), (50, 90), (70, 90)], 0, zero_rng());
                let mut sq = afterimage_screen_quad(&q, 100 + i * 10);
                sq.ot_index = 100 + i * 10;
                ScreenPrim::Textured(sq)
            })
            .collect();
        let panel = ScreenPrim::Flat(FlatQuad {
            xy: [(0, 0), (320, 0), (0, 16), (320, 16)],
            color: [0, 0, 0, 255],
            semi_transparent: false,
            abr_mode: 0,
            ot_index: 1, // nearest -> drawn last
        });
        let mut prims = streak;
        prims.push(panel);

        let geo = build_geometry(&prims, 320, 240);
        // 4 prims -> 16 vertices, 24 indices.
        assert_eq!(geo.vertices.len(), 16);
        assert_eq!(geo.indices.len(), 24);
        // Two runs: one coalesced semi run (the 3 additive quads), then the
        // opaque flat run.
        assert_eq!(geo.runs.len(), 2);
        assert_eq!(geo.runs[0].class, BlendClass::Semi(1));
        assert_eq!(geo.runs[0].index_start, 0);
        assert_eq!(geo.runs[0].index_count, 18); // 3 quads * 6
        assert_eq!(geo.runs[1].class, BlendClass::Opaque);
        assert_eq!(geo.runs[1].index_start, 18);
        assert_eq!(geo.runs[1].index_count, 6);

        // Farthest streak quad (ot 120) is emitted first: its first vertex is
        // the textured top-left corner in NDC. build_afterimage_quad applies
        // its zero-rng jitter (-2 x, -8 y) so corner (50,60) -> (48,52).
        let v0 = geo.vertices[0];
        assert_eq!(v0.flags, FLAG_TEXTURED);
        assert!((v0.pos[0] - (48.0 / 320.0 * 2.0 - 1.0)).abs() < 1e-6);
        assert!((v0.pos[1] - (1.0 - 52.0 / 240.0 * 2.0)).abs() < 1e-6);
        // Neutral 0x808080 modulation -> /128 factor of 1.0.
        assert_eq!(v0.color, [1.0, 1.0, 1.0, 1.0]);

        // The opaque flat panel's vertices carry flags=0 and its raw colour.
        let flat_v0 = geo.vertices[12];
        assert_eq!(flat_v0.flags, 0);
        assert_eq!(flat_v0.color, [0.0, 0.0, 0.0, 1.0]);
    }

    #[test]
    fn mixed_blend_modes_split_runs() {
        let a = ScreenPrim::Flat(FlatQuad {
            xy: [(0, 0), (8, 0), (0, 8), (8, 8)],
            color: [255, 0, 0, 128],
            semi_transparent: true,
            abr_mode: 1,
            ot_index: 50,
        });
        let b = ScreenPrim::Flat(FlatQuad {
            xy: [(0, 0), (8, 0), (0, 8), (8, 8)],
            color: [0, 255, 0, 128],
            semi_transparent: true,
            abr_mode: 2,
            ot_index: 40,
        });
        let geo = build_geometry(&[a, b], 320, 240);
        // Different ABR modes never coalesce, even back-to-back.
        assert_eq!(geo.runs.len(), 2);
        assert_eq!(geo.runs[0].class, BlendClass::Semi(1));
        assert_eq!(geo.runs[1].class, BlendClass::Semi(2));
    }

    #[test]
    fn empty_input_is_empty_geometry() {
        let geo = build_geometry(&[], 320, 240);
        assert!(geo.is_empty());
        assert!(geo.runs.is_empty());
    }
}
