//! Screen-space PSX primitives: the draw record `SpriteDraw` cannot express,
//! plus the ordering-table sort and the vertex builder both hosts run.
//!
//! REF: FUN_8003d2c4 (retail `AddPrim` - links a packet into the software
//! ordering table at a depth bucket) / `DrawOTag` (walks the OT back-to-front)
//!
//! # Why this is not a [`SpriteDraw`]
//!
//! [`SpriteDraw`](crate::SpriteDraw) is a semantic alias of
//! [`TextDraw`](crate::TextDraw): an axis-aligned destination rect, an atlas
//! source rect and one flat RGBA tint.
//!
//! Every screen-space effect retail draws (the field-to-battle transition
//! styles, the move-FX afterimage streak, any `screen_fx` sprite) is instead a
//! PSX primitive: four **independent** corners, four `(u, v)` pairs, a
//! `(cba, tsb)` pair selecting a CLUT and a texture page out of VRAM,
//! per-vertex colour, one of four fixed semi-transparency (ABR) equations, and
//! an ordering-table bucket instead of a depth test. None of those six fit in a
//! rect-plus-tint, which is why a builder returning `SpriteDraw` can be on the
//! host-drift gate's surface while this whole *capability* is not.
//!
//! # What the retail ordering table does, and how this mirrors it
//!
//! Retail links each drawn packet into `OT[depth]` with `AddPrim`
//! (`FUN_8003d2c4`); a bucket is a LIFO singly-linked list (`AddPrim`
//! prepends). `DrawOTag` then walks the table so **higher-index (farther)
//! buckets draw first** - the classic PSX back-to-front painter's order, which
//! is what makes additive/blended prims composite correctly with no depth
//! buffer. [`order_primitives`] reproduces exactly that: primitives sort by
//! `ot_index` **descending** (farthest first), ties broken by submission order
//! **descending** (later-submitted draws first - the LIFO bucket).
//!
//! # Why the sort lives here rather than in a host
//!
//! Two hosts sorting one primitive list differently is a divergence that shows
//! up as a compositing bug on one host only, under whichever camera angle
//! happens to stack two blended quads - which is to say, a divergence no diff
//! and no single screenshot catches. So neither host is handed a primitive
//! list to order: [`build_geometry`] is the only public route from a
//! `&[ScreenPrim]` to something drawable, and it runs [`order_primitives`]
//! itself. The native renderer uploads [`OverlayGeometry`] to wgpu; the browser
//! play page uploads the same three arrays to WebGL2. Neither can re-order,
//! because by the time either sees the data the order is baked into the index
//! buffer.
//!
//! # The coordinate space is the PSX display, not the window
//!
//! Every retail emitter authors screen coordinates in 320x240
//! ([`PSX_DISPLAY_W`] x [`PSX_DISPLAY_H`]) and clamps against it, so
//! [`build_geometry`] takes that space as its argument and the overlay
//! stretches over the whole surface. Handing it the window size instead pins a
//! 320x240 overlay into the top-left corner of a 960x720 frame.
//!
//! # Simplifications vs. hardware (documented, not hidden)
//!
//! A semi-transparent *textured* PSX prim honours the per-texel STP bit (STP=0
//! texels draw opaque even inside a blended prim). This model treats a
//! semi-transparent prim as fully blended (every non-zero texel goes through
//! the ABR equation), matching how the untextured colour-mesh blend path
//! already behaves. Texel `0x0000` is still never drawn. That is faithful for
//! the afterimage trail (additive, ABR mode 1, no opaque STP texels) and for
//! flat quads; a per-texel STP split can be layered on later without changing
//! this module's public shape.

/// PSX display width in pixels - the space every retail screen-space emitter
/// authors its corners in.
///
/// `legaia-engine-render`'s `vram_capture` carries the same pair for the
/// framebuffer readback, and `screen_overlay` pins the two together with a
/// compile-time assertion rather than a comment.
pub const PSX_DISPLAY_W: i16 = 320;

/// PSX display height in pixels. See [`PSX_DISPLAY_W`].
pub const PSX_DISPLAY_H: i16 = 240;

/// ABR blend mode (0..=3) from texpage (TSB) bits 5..=6 - which fixed-function
/// blend equation a primitive uses when it is semi-transparent.
///
/// | ABR | equation        |
/// |-----|-----------------|
/// | 0   | `0.5*B + 0.5*F` |
/// | 1   | `B + F`         |
/// | 2   | `B - F`         |
/// | 3   | `B + 0.25*F`    |
///
/// This is the same bit field the 3D VRAM-mesh path reads out of its
/// per-vertex TSB attribute (`legaia_engine_render::psx_blend::abr_mode`); the
/// two paths pack the word identically because both take it straight off the
/// GP0 texpage.
pub fn abr_mode(tsb: u16) -> u8 {
    ((tsb >> 5) & 0x3) as u8
}

/// One screen-space textured quad (PSX `POLY_FT4`) sampling PSX VRAM.
///
/// `xy` are the four corners in PSX screen pixels in the retail `POLY_FT4`
/// vertex order (`v0..v3`); [`build_geometry`] converts them to NDC against the
/// space its caller names. `uv`/`clut`/`tpage` drive the same VRAM CLUT decode
/// as the 3D VRAM-mesh path; `color` is the 24-bit modulation colour
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
    /// Ignored when [`Self::gouraud`] is set.
    pub color: u32,
    /// Per-vertex 24-bit modulation colours, in the same `v0..v3` order as
    /// [`Self::xy`] - i.e. a `POLY_GT4` rather than a `POLY_FT4`.
    ///
    /// The field-to-battle transition styles need this: their quad carries a
    /// separate top-edge and bottom-edge colour, and "top and bottom differing
    /// is what makes the quad a gradient". The afterimage streak is the flat
    /// case and leaves this `None`.
    pub gouraud: Option<[u32; 4]>,
    pub semi_transparent: bool,
    pub ot_index: u32,
}

impl ScreenQuad {
    /// ABR blend mode (0..=3) from TSB bits 5..=6 - which fixed-function
    /// blend equation this quad uses when `semi_transparent`.
    pub fn abr_mode(&self) -> u8 {
        abr_mode(self.tpage)
    }
}

/// One screen-space **flat** (untextured, solid) quad. Used for letterbox
/// bars, iris fills, transition fades, and solid UI panels (the engine-core
/// `screen_fx` widget family's non-sprite draws).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FlatQuad {
    pub xy: [(i16, i16); 4],
    /// RGBA colour, 0..=255 per channel.
    pub color: [u8; 4],
    /// Per-vertex RGBA colours in the same `v0..v3` order as [`Self::xy`] -
    /// i.e. an untextured `POLY_G4` rather than a `POLY_F4`. Overrides
    /// [`Self::color`] when set.
    ///
    /// The weapon-trail bands need this: retail's `FUN_800485BC` emits
    /// `0x3B`-command gouraud quads whose leading and trailing edges carry
    /// different colours, and edge colours differing is what makes a band a
    /// gradient. Everything flat leaves it `None`.
    pub gouraud: Option<[[u8; 4]; 4]>,
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

    /// The four screen-space corners, whichever variant this is.
    pub fn corners(&self) -> [(i16, i16); 4] {
        match self {
            ScreenPrim::Textured(q) => q.xy,
            ScreenPrim::Flat(q) => q.xy,
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

impl BlendClass {
    /// A host-transport encoding: `0` = opaque, `1 + mode` = semi-transparent.
    ///
    /// The browser play page reads its run table out of a flat `u32` array, so
    /// the class has to survive the trip through a WASM boundary that carries
    /// no enums. Keeping the encoding here rather than at the call site is what
    /// stops the page and the exporter from disagreeing about which code means
    /// "additive".
    pub fn code(self) -> u32 {
        match self {
            BlendClass::Opaque => 0,
            BlendClass::Semi(m) => 1 + u32::from(m & 0x3),
        }
    }
}

/// A full display-rect flat quad - the shape every whole-screen wash, backdrop
/// and fade in the transition family is.
///
/// Retail authors these from the scratchpad display-rect words
/// (`_DAT_1F800378` / `_DAT_1F80037A`), i.e. the whole PSX display, in the
/// same `v0..v3` corner order as every other quad here.
pub fn display_rect_flat_quad(
    color: [u8; 4],
    semi_transparent: bool,
    abr_mode: u8,
    ot_index: u32,
) -> ScreenPrim {
    let (w, h) = (PSX_DISPLAY_W, PSX_DISPLAY_H);
    ScreenPrim::Flat(FlatQuad {
        xy: [(0, 0), (w, 0), (0, h), (w, h)],
        color,
        gouraud: None,
        semi_transparent,
        abr_mode,
        ot_index,
    })
}

/// The full-screen fade quad a resolved intro fade emits.
///
/// `rgb` is the ramp level smeared across all three channels
/// (`level + (level << 8) + (level << 16)`), `abr_mode` is what decides whether
/// the fade darkens (`2`, `B - F`) or brightens (`1`, `B + F`) - not the
/// colour - and `ot_index` is the OT layer the emitter links it at.
///
/// Both hosts call this. A host that hand-rolls the quad instead is how the
/// ABR mode gets quietly read as an OT depth, which puts every style's fade on
/// `0.5B + 0.5F`: the additive styles then top out at washed grey instead of a
/// white-out and the subtractive ones never reach black.
pub fn fade_prim(rgb: u32, abr_mode: u8, ot_index: u32) -> ScreenPrim {
    display_rect_flat_quad(
        [
            ((rgb >> 16) & 0xFF) as u8,
            ((rgb >> 8) & 0xFF) as u8,
            (rgb & 0xFF) as u8,
            0xFF,
        ],
        true,
        abr_mode,
        ot_index,
    )
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
/// **per-vertex** modulation (textured: a `/128` factor; flat: a `/255`
/// colour), and `flags` bit 0 = textured. The shader interpolates `color`,
/// which is what makes a `POLY_GT4` gradient work; a flat `POLY_FT4` writes
/// the same value to all four corners.
///
/// The layout is `repr(C)` and `Pod` because both hosts read it as bytes: the
/// native renderer maps it straight into a wgpu vertex buffer, and the browser
/// play page reads the same byte stream out of WASM memory into a WebGL2
/// `ARRAY_BUFFER` with [`SCREEN_VERTEX_STRIDE`] and the field offsets below.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct ScreenVertex {
    pub pos: [f32; 2],
    pub uv: [f32; 2],
    pub cba_tsb: [u32; 2],
    pub color: [f32; 4],
    pub flags: u32,
}

/// Byte stride of [`ScreenVertex`] in a host vertex buffer.
pub const SCREEN_VERTEX_STRIDE: u64 = std::mem::size_of::<ScreenVertex>() as u64;

/// Byte offset of [`ScreenVertex::pos`] within the vertex.
pub const SCREEN_VERTEX_OFF_POS: u64 = 0;
/// Byte offset of [`ScreenVertex::uv`].
pub const SCREEN_VERTEX_OFF_UV: u64 = 8;
/// Byte offset of [`ScreenVertex::cba_tsb`].
pub const SCREEN_VERTEX_OFF_CBA_TSB: u64 = 16;
/// Byte offset of [`ScreenVertex::color`].
pub const SCREEN_VERTEX_OFF_COLOR: u64 = 24;
/// Byte offset of [`ScreenVertex::flags`].
pub const SCREEN_VERTEX_OFF_FLAGS: u64 = 40;

/// `flags` bit set when a [`ScreenVertex`] belongs to a textured quad.
pub const FLAG_TEXTURED: u32 = 1;

/// One contiguous run of quads sharing a [`BlendClass`], expressed as an
/// index-buffer range. A host binds the run's pipeline / blend state once and
/// issues a single indexed draw over `index_start..index_start + index_count`.
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

    /// The vertex stream as raw bytes in the [`ScreenVertex`] layout
    /// ([`SCREEN_VERTEX_STRIDE`] + the `SCREEN_VERTEX_OFF_*` offsets).
    ///
    /// Lives here rather than at the call site so the one crate that owns the
    /// layout is also the one that hands it out as bytes: a host reading the
    /// stream into a GPU buffer never has to name `bytemuck` or restate the
    /// stride.
    pub fn vertex_bytes(&self) -> Vec<u8> {
        bytemuck::cast_slice(&self.vertices).to_vec()
    }

    /// The run table flattened for a host that cannot carry the enum across
    /// its boundary: `[class_code, index_start, index_count]` per run, with
    /// `class_code` as [`BlendClass::code`].
    pub fn run_words(&self) -> Vec<u32> {
        let mut out = Vec::with_capacity(self.runs.len() * 3);
        for r in &self.runs {
            out.push(r.class.code());
            out.push(r.index_start);
            out.push(r.index_count);
        }
        out
    }
}

fn to_ndc(x: i16, y: i16, surf_w: f32, surf_h: f32) -> [f32; 2] {
    [
        (x as f32 / surf_w) * 2.0 - 1.0,
        1.0 - (y as f32 / surf_h) * 2.0,
    ]
}

/// Emit the four vertices of one quad (retail `v0..v3` order) into `verts`
/// and its two triangles into `idx`. `color` is per-corner, so one flat
/// `POLY_FT4` colour is passed as four copies and a `POLY_GT4` gradient as
/// four distinct entries.
#[allow(clippy::too_many_arguments)]
fn push_quad(
    verts: &mut Vec<ScreenVertex>,
    idx: &mut Vec<u32>,
    xy: [(i16, i16); 4],
    uv: [(u8, u8); 4],
    cba_tsb: [u32; 2],
    color: [[f32; 4]; 4],
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
            color: color[c],
            flags,
        });
    }
    // POLY_FT4 = two triangles (v0,v1,v2) + (v1,v2,v3). Cull is disabled in
    // both hosts' pipelines so winding is irrelevant.
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
/// **coordinate space the primitives are authored in**. Primitives are drawn
/// in [`order_primitives`] order and coalesced into [`DrawRun`]s of
/// consecutive same-[`BlendClass`] quads.
///
/// `surf_w` / `surf_h` are what a corner of `(surf_w, surf_h)` maps to the
/// bottom-right of the frame; they are *not* the window size. Every retail
/// emitter authors in the [`PSX_DISPLAY_W`] x [`PSX_DISPLAY_H`] display space
/// and clamps against it, so a host passes that pair and the overlay stretches
/// over the whole surface.
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
                match q.gouraud {
                    Some(c) => std::array::from_fn(|i| tex_mod_factor(c[i])),
                    None => [tex_mod_factor(q.color); 4],
                },
                FLAG_TEXTURED,
                sw,
                sh,
            ),
            ScreenPrim::Flat(q) => {
                let corner = |c: [u8; 4]| {
                    [
                        c[0] as f32 / 255.0,
                        c[1] as f32 / 255.0,
                        c[2] as f32 / 255.0,
                        c[3] as f32 / 255.0,
                    ]
                };
                push_quad(
                    &mut verts,
                    &mut idx,
                    q.xy,
                    [(0, 0); 4],
                    [0, 0],
                    match q.gouraud {
                        Some(g) => std::array::from_fn(|i| corner(g[i])),
                        None => [corner(q.color); 4],
                    },
                    0,
                    sw,
                    sh,
                )
            }
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

    fn tex(ot: u32) -> ScreenPrim {
        ScreenPrim::Textured(ScreenQuad {
            xy: [(0, 0); 4],
            uv: [(0, 0); 4],
            clut: 0,
            tpage: 0x27,
            color: 0x808080,
            gouraud: None,
            semi_transparent: true,
            ot_index: ot,
        })
    }

    #[test]
    fn ordering_is_back_to_front_lifo_ties() {
        // Three quads at OT depths 10, 30, 30 submitted in order 0,1,2.
        // Farthest bucket (30) first; within it, later-submitted (2) before
        // earlier (1); nearest bucket (10) last.
        let prims = [tex(10), tex(30), tex(30)];
        assert_eq!(order_primitives(&prims), vec![2, 1, 0]);
    }

    #[test]
    fn abr_mode_reads_tsb_bits_5_and_6() {
        // TSB 0x0027 -> bits 5..6 = 1 (additive) - the afterimage streak mode.
        assert_eq!(abr_mode(0x0027), 1);
        assert_eq!(abr_mode(0x0000), 0);
        assert_eq!(abr_mode(0x0040), 2);
        assert_eq!(abr_mode(0x0060), 3);
        // ...and the quad's accessor reads its own tpage word.
        let ScreenPrim::Textured(q) = tex(0) else {
            panic!("textured")
        };
        assert_eq!(q.abr_mode(), 1);
        assert_eq!(tex(0).blend_class(), BlendClass::Semi(1));
    }

    #[test]
    fn build_geometry_orders_and_coalesces_runs() {
        // Three additive textured quads at increasing depth plus a nearer
        // opaque flat panel. Draw order: farthest textured first (coalesced
        // into ONE semi run), opaque flat last.
        let mut prims: Vec<ScreenPrim> = (0..3).map(|i| tex(100 + i * 10)).collect();
        prims.push(ScreenPrim::Flat(FlatQuad {
            xy: [(0, 0), (320, 0), (0, 16), (320, 16)],
            color: [0, 0, 0, 255],
            gouraud: None,
            semi_transparent: false,
            abr_mode: 0,
            ot_index: 1, // nearest -> drawn last
        }));

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

        let v0 = geo.vertices[0];
        assert_eq!(v0.flags, FLAG_TEXTURED);
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
            gouraud: None,
            semi_transparent: true,
            abr_mode: 1,
            ot_index: 50,
        });
        let b = ScreenPrim::Flat(FlatQuad {
            xy: [(0, 0), (8, 0), (0, 8), (8, 8)],
            color: [0, 255, 0, 128],
            gouraud: None,
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
        assert!(geo.run_words().is_empty());
    }

    #[test]
    fn a_gouraud_quad_gives_each_corner_its_own_modulation() {
        // The intro-transition shape: one colour on the top edge (v0/v1),
        // another on the bottom (v2/v3). A flat quad cannot express it, which
        // is why `gouraud` exists.
        let top = 0x0080_8080u32;
        let bottom = 0x0040_4040u32;
        let q = ScreenPrim::Textured(ScreenQuad {
            xy: [(0, 0), (32, 0), (0, 32), (32, 32)],
            uv: [(0, 0); 4],
            clut: 0,
            tpage: 2 << 7,
            color: 0x00FF_00FF, // must be ignored
            gouraud: Some([top, top, bottom, bottom]),
            semi_transparent: false,
            ot_index: 1,
        });
        let geo = build_geometry(&[q], 320, 240);
        assert_eq!(geo.vertices[0].color, [1.0, 1.0, 1.0, 1.0]);
        assert_eq!(geo.vertices[1].color, [1.0, 1.0, 1.0, 1.0]);
        assert_eq!(geo.vertices[2].color, [0.5, 0.5, 0.5, 1.0]);
        assert_eq!(geo.vertices[3].color, [0.5, 0.5, 0.5, 1.0]);
    }

    #[test]
    fn the_geometry_space_is_the_argument_not_the_window() {
        // The same 320x240-authored quad must fill the frame whatever the
        // window is, which is only true if the caller passes the PSX display
        // size. Passing 960x720 instead pins it into the top-left corner -
        // that is the bug a host's staging pass has to avoid.
        let full = ScreenPrim::Flat(FlatQuad {
            xy: [(0, 0), (320, 0), (0, 240), (320, 240)],
            color: [255, 255, 255, 255],
            gouraud: None,
            semi_transparent: false,
            abr_mode: 0,
            ot_index: 1,
        });
        let psx = build_geometry(&[full], 320, 240);
        assert_eq!(psx.vertices[0].pos, [-1.0, 1.0]);
        assert_eq!(psx.vertices[3].pos, [1.0, -1.0]);

        let windowed = build_geometry(&[full], 960, 720);
        assert_eq!(windowed.vertices[0].pos, [-1.0, 1.0]);
        assert!(windowed.vertices[3].pos[0] < -0.3);
    }

    #[test]
    fn the_fade_quad_is_the_whole_display_rect_and_carries_its_abr() {
        let p = fade_prim(0x0080_8080, 2, 2);
        assert_eq!(
            p.corners(),
            [
                (0, 0),
                (PSX_DISPLAY_W, 0),
                (0, PSX_DISPLAY_H),
                (PSX_DISPLAY_W, PSX_DISPLAY_H)
            ]
        );
        assert_eq!(p.blend_class(), BlendClass::Semi(2));
        assert_eq!(p.ot_index(), 2);
        let ScreenPrim::Flat(q) = p else {
            panic!("fade is a flat quad")
        };
        assert_eq!(q.color, [0x80, 0x80, 0x80, 0xFF]);
    }

    #[test]
    fn the_run_words_encoding_survives_a_host_boundary() {
        // A host that cannot carry the enum reads `[code, start, count]`
        // triples; `0` must stay reserved for opaque so a mode-0 semi run
        // (0.5B + 0.5F) is not read as "no blending".
        assert_eq!(BlendClass::Opaque.code(), 0);
        assert_eq!(BlendClass::Semi(0).code(), 1);
        assert_eq!(BlendClass::Semi(3).code(), 4);
        let geo = build_geometry(
            &[
                tex(10),
                ScreenPrim::Flat(FlatQuad {
                    xy: [(0, 0); 4],
                    color: [1, 2, 3, 4],
                    gouraud: None,
                    semi_transparent: false,
                    abr_mode: 0,
                    ot_index: 5,
                }),
            ],
            320,
            240,
        );
        assert_eq!(geo.run_words(), vec![2, 0, 6, 0, 6, 6]);
    }

    #[test]
    fn the_vertex_field_offsets_match_the_layout_hosts_read_as_bytes() {
        // The browser reads this struct as raw bytes with hand-written
        // attribute offsets, so a field reorder has to fail here rather than
        // in a shader that silently samples the wrong words.
        assert_eq!(SCREEN_VERTEX_STRIDE, 44);
        let v = ScreenVertex {
            pos: [0.0; 2],
            uv: [0.0; 2],
            cba_tsb: [0; 2],
            color: [0.0; 4],
            flags: 0,
        };
        let base = &v as *const _ as usize;
        let off = |p: *const u8| (p as usize - base) as u64;
        assert_eq!(off(v.pos.as_ptr() as *const u8), SCREEN_VERTEX_OFF_POS);
        assert_eq!(off(v.uv.as_ptr() as *const u8), SCREEN_VERTEX_OFF_UV);
        assert_eq!(
            off(v.cba_tsb.as_ptr() as *const u8),
            SCREEN_VERTEX_OFF_CBA_TSB
        );
        assert_eq!(off(v.color.as_ptr() as *const u8), SCREEN_VERTEX_OFF_COLOR);
        assert_eq!(
            off(&v.flags as *const u32 as *const u8),
            SCREEN_VERTEX_OFF_FLAGS
        );
    }
}
