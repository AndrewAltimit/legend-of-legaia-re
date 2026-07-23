//! CPU rasterizer scaffold - small enough to use as a regression target
//! against captured retail GTE traces without dragging in wgpu. Not
//! production-grade: it's a validation tool, not a renderer replacement.

use super::*;

/// Bounding box of a triangle in pixel coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BBox {
    pub min_x: i32,
    pub min_y: i32,
    pub max_x: i32,
    pub max_y: i32,
}

impl BBox {
    pub fn from_triangle(a: ScreenXY, b: ScreenXY, c: ScreenXY) -> Self {
        Self {
            min_x: a.x.min(b.x).min(c.x),
            min_y: a.y.min(b.y).min(c.y),
            max_x: a.x.max(b.x).max(c.x),
            max_y: a.y.max(b.y).max(c.y),
        }
    }

    /// Clamp this bounding box to a render target. Returns `None` if the
    /// triangle is entirely off-screen.
    pub fn clamp(&self, w: i32, h: i32) -> Option<Self> {
        let r = Self {
            min_x: self.min_x.max(0),
            min_y: self.min_y.max(0),
            max_x: self.max_x.min(w - 1),
            max_y: self.max_y.min(h - 1),
        };
        if r.min_x > r.max_x || r.min_y > r.max_y {
            None
        } else {
            Some(r)
        }
    }
}

/// 2D edge function - positive when `p` is on the inside (right-hand
/// side) of the directed edge `a→b` under PSX winding. Sums of three
/// edge functions over a triangle's bbox give the barycentric weights
/// for an inside-triangle test (all-positive ⇒ inside).
pub fn edge(a: ScreenXY, b: ScreenXY, px: i32, py: i32) -> i64 {
    let ab_x = (b.x - a.x) as i64;
    let ab_y = (b.y - a.y) as i64;
    let ap_x = (px - a.x) as i64;
    let ap_y = (py - a.y) as i64;
    ab_x * ap_y - ab_y * ap_x
}

/// Whether `(px, py)` lies inside the triangle `(a, b, c)` under PSX
/// winding rules. Assumes the triangle is front-facing
/// ([`super::nclip`] returned negative); caller should reject
/// back-facing triangles before rasterising.
///
/// Edges on the bottom-right are counted as outside (top-left fill
/// rule), matching the PSX rasteriser's pixel-center convention.
pub fn contains(a: ScreenXY, b: ScreenXY, c: ScreenXY, px: i32, py: i32) -> bool {
    let w0 = edge(b, c, px, py);
    let w1 = edge(c, a, px, py);
    let w2 = edge(a, b, px, py);
    // Front-facing triangle: nclip < 0; the three edge functions then
    // share sign for inside points. Accept zero-area only on top-left
    // edges to avoid double-shading shared pixels.
    (w0 < 0 && w1 < 0 && w2 < 0)
        || (w0 == 0 && top_left(b, c))
        || (w1 == 0 && top_left(c, a))
        || (w2 == 0 && top_left(a, b))
}

/// PSX top-left fill rule: an edge counts as inside if it's exactly
/// horizontal pointing leftward, OR a non-horizontal edge pointing
/// upward.
fn top_left(a: ScreenXY, b: ScreenXY) -> bool {
    let dx = b.x - a.x;
    let dy = b.y - a.y;
    (dy == 0 && dx < 0) || dy < 0
}

/// Iterate every (px, py) inside `triangle`, calling `emit(px, py, w)`
/// where `w = (w0, w1, w2)` is the unnormalised edge-function triple
/// (caller can divide by triangle area to get barycentrics).
pub fn rasterize_triangle(
    a: ScreenXY,
    b: ScreenXY,
    c: ScreenXY,
    viewport_w: i32,
    viewport_h: i32,
    mut emit: impl FnMut(i32, i32, (i64, i64, i64)),
) {
    let bbox = match BBox::from_triangle(a, b, c).clamp(viewport_w, viewport_h) {
        Some(b) => b,
        None => return,
    };
    for py in bbox.min_y..=bbox.max_y {
        for px in bbox.min_x..=bbox.max_x {
            let w0 = edge(b, c, px, py);
            let w1 = edge(c, a, px, py);
            let w2 = edge(a, b, px, py);
            if w0 < 0 && w1 < 0 && w2 < 0 {
                emit(px, py, (w0, w1, w2));
            }
        }
    }
}

/// Retail's software near-plane clip processes projected vertices held in a
/// scratch cache of these fixed 0x1C-byte records, one per vertex. The two
/// clip helpers below (`interp_clip_vertex`, and the flag bits that gate it)
/// read this layout directly.
///
/// Field offsets, read off the retail interpolation kernel:
/// - `+0x0C..0x11` - three `i16` screen components (X / Y / Z).
/// - `+0x14..0x16` - packed RGB (`+0x17` is the primitive command byte, so the
///   four bytes `+0x14..0x17` form the flat colour word).
/// - `+0x18..0x19` - packed U / V texel coordinate.
pub const CLIP_VERT_STRIDE: usize = 0x1c;

/// Flag bits for [`interp_clip_vertex`]'s `flags` word (retail arg `a2`).
pub mod clip_flags {
    /// Interpolate the U / V texel pair (`+0x18`, `+0x19`).
    pub const UV: u32 = 0x1;
    /// Interpolate the packed RGB colour (`+0x14..0x16`). When clear, the flat
    /// colour word is copied from the current vertex verbatim instead.
    pub const RGB: u32 = 0x2;
    /// Select the trailing neighbour (`cur + 0x1C`) rather than the leading one
    /// (`cur - 0x1C`) as the second interpolation endpoint.
    pub const TRAILING: u32 = 0x800;
}

/// One interpolation channel: `nb + (((cur - nb) * frac) >> 12)`.
///
/// `frac` is the q12 crossing fraction. The retail kernel sign-extends the
/// `i16` screen components for the difference but zero-extends colour / UV
/// bytes; the low bits of the sum are what gets stored, so the extension of the
/// added base term is irrelevant after truncation.
fn lerp_q12(cur: i32, nb: i32, frac: i32) -> i32 {
    nb + (((cur - nb) * frac) >> 12)
}

/// Synthesise a clipped vertex by interpolating between a current vertex and a
/// neighbour at the q12 crossing fraction `frac`.
///
/// PORT: FUN_80029724
///
/// NOT WIRED: the engine has no software near-plane clip stage to synthesise
/// a vertex for. Retail clips in software against a scratch cache of
/// [`CLIP_VERT_STRIDE`]-byte projected vertices before handing the GPU a
/// packet; the port projects with the GTE and then submits triangles to wgpu,
/// which clips in hardware, so no `verts` cache is ever materialised. The
/// prerequisite is that cache plus the clip walker that decides *where* the
/// crossing is and with which [`clip_flags`] - this function is only the
/// per-crossing interpolation. Changing that is a rasterisation-path change
/// and is measured by the VRAM oracle, so it does not belong to a wiring
/// pass.
///
/// `verts` is the projected-vertex scratch cache ([`CLIP_VERT_STRIDE`] records);
/// `cur_off` is the byte offset of the current vertex within it. The neighbour
/// is `cur_off - 0x1C`, or `cur_off + 0x1C` when [`clip_flags::TRAILING`] is
/// set. The 16-byte output slot `out` receives:
/// - `+0x0`, `+0x2`, `+0x4` - the interpolated `i16` X / Y / Z (always).
/// - `+0x8..` - the colour: when [`clip_flags::RGB`] is set, three interpolated
///   RGB bytes at `+0x8..0x0A`; otherwise the current vertex's flat colour word
///   (`+0x14..0x17`) copied verbatim to `+0x8..0x0B`.
/// - `+0x0C`, `+0x0D` - interpolated U / V, only when [`clip_flags::UV`] is set.
pub fn interp_clip_vertex(out: &mut [u8], verts: &[u8], cur_off: usize, flags: u32, frac: i32) {
    let nb_off = if flags & clip_flags::TRAILING != 0 {
        cur_off + CLIP_VERT_STRIDE
    } else {
        cur_off - CLIP_VERT_STRIDE
    };

    let rd_i16 = |base: usize, f: usize| -> i32 {
        i16::from_le_bytes([verts[base + f], verts[base + f + 1]]) as i32
    };
    let rd_u8 = |base: usize, f: usize| -> i32 { verts[base + f] as i32 };

    // Screen X / Y / Z (i16 fields at +0x0C / +0x0E / +0x10 -> out +0/+2/+4).
    for (i, f) in [0xc, 0xe, 0x10].into_iter().enumerate() {
        let v = lerp_q12(rd_i16(cur_off, f), rd_i16(nb_off, f), frac) as u16;
        let b = v.to_le_bytes();
        out[i * 2] = b[0];
        out[i * 2 + 1] = b[1];
    }

    // U / V (bytes at +0x18 / +0x19 -> out +0x0C / +0x0D).
    if flags & clip_flags::UV != 0 {
        out[0xc] = lerp_q12(rd_u8(cur_off, 0x18), rd_u8(nb_off, 0x18), frac) as u8;
        out[0xd] = lerp_q12(rd_u8(cur_off, 0x19), rd_u8(nb_off, 0x19), frac) as u8;
    }

    // Colour: interpolate 3 RGB bytes, or copy the flat 4-byte colour word.
    if flags & clip_flags::RGB != 0 {
        for (i, f) in [0x14, 0x15, 0x16].into_iter().enumerate() {
            out[0x8 + i] = lerp_q12(rd_u8(cur_off, f), rd_u8(nb_off, f), frac) as u8;
        }
    } else {
        out[0x8..0xc].copy_from_slice(&verts[cur_off + 0x14..cur_off + 0x18]);
    }
}

/// Pack per-vertex RGB triples into a gouraud `POLY_*` primitive packet.
///
/// PORT: FUN_80036c4c
///
/// NOT WIRED: the engine emits no `POLY_G3` / `POLY_G4` GPU packets, so there
/// is no `packet` byte buffer whose `+4 + 8*i` colour fields want filling.
/// Per-vertex colour reaches the wgpu pipeline as a vertex-buffer attribute
/// built by the mesh uploader; the packed-packet layout this writes into
/// exists only in the retail command stream. The prerequisite is a GPU-packet
/// emitter (the same one [`interp_clip_vertex`] wants), not a caller.
///
/// `colors` is the source, one 4-byte word per vertex (`[R, G, B, code]`, LE);
/// only the low three bytes are used. `count` is 3 (`POLY_G3`) or 4 (`POLY_G4`);
/// any other value is a no-op, matching retail. The RGB bytes are written into
/// `packet`'s colour fields at `+4 + 8*i` (`+0x04`, `+0x0C`, `+0x14`, `+0x1C`),
/// leaving each colour word's command byte (`+7 + 8*i`) untouched.
pub fn spread_prim_colors(packet: &mut [u8], colors: &[u8], count: usize) {
    if count != 3 && count != 4 {
        return;
    }
    for i in 0..count {
        let s = i * 4;
        let d = 4 + i * 8;
        packet[d] = colors[s]; // R
        packet[d + 1] = colors[s + 1]; // G
        packet[d + 2] = colors[s + 2]; // B
    }
}
