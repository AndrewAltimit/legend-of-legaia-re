//! Lit windows: the curated window art table, its VRAM hash match and the night glass shade.

use super::*;

// ---------------------------------------------------------------------------
// Lit windows
// ---------------------------------------------------------------------------

/// Bit 12 of a per-vertex TSB word: the prim samples a curated
/// [`LIT_WINDOWS`] art, so its **glass** texels ([`window_glass`]) glow
/// warm under a mood with [`LightingMood::window_glow`] above zero. Free
/// for the same reason as [`EMISSIVE_BIT`] (a retail TSB uses bits 0..=8);
/// every shader decode masks it out. WGSL / GLSL twin: `0x1000u`.
pub const WINDOW_BIT: u16 = 0x1000;

/// Glass test on a raw texel (0..=1, before the packet-colour modulation),
/// first half: blue at least this far above red, and blue the dominant
/// channel. Most of the town window art paints its panes as saturated blue
/// (night sky, sky reflection); the frame, the shutters and the wall around
/// them are greys, wood and plaster that fail it. WGSL twin
/// `WIN_GLASS_MIN_BLUE`, GLSL twin `DYN_WIN_GLASS_MIN_BLUE`.
pub const WINDOW_GLASS_MIN_BLUE: f32 = 0.19;

/// Glass test, second half: a near-black texel (every channel at most
/// this) is glass too - the art that paints its panes as dark voids
/// (Vidna's stone houses), whose mullions and frames are greys well above
/// it. WGSL twin `WIN_GLASS_BLACK_MAX`, GLSL twin `DYN_WIN_GLASS_BLACK_MAX`.
pub const WINDOW_GLASS_BLACK_MAX: f32 = 0.1;

/// The warm lamp-light colour a lit pane draws at (0..=1). WGSL twin
/// `WIN_RGB`, GLSL twin `DYN_WIN_RGB`.
pub const WINDOW_RGB: [f32; 3] = [1.0, 0.72, 0.4];

/// The pane's brightness floor: a lit pane draws at `WINDOW_RGB * (floor +
/// (1 - floor) * texel_blue)`, so the art's pane gradient (darker at the
/// top, sky-bright at the sill) survives as a gradient of lamp light. WGSL
/// twin `WIN_FLOOR`, GLSL twin `DYN_WIN_FLOOR`.
pub const WINDOW_FLOOR: f32 = 0.6;

/// One curated window art: a rectangle of one texture page under one CLUT
/// that paints a glazed window. Keyed by where the art sits in VRAM **and**
/// a content hash of its decoded texels ([`window_art_hash`]), so the same
/// atlas slot holding different art in another scene never matches, and
/// every scene that loads this art (a town and its story revisits) does.
/// The table holds coordinates and hashes only - no disc bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowArt {
    /// What it is (documentation only).
    pub label: &'static str,
    /// The CLUT word (`cba & 0x7FFF`).
    pub cba: u16,
    /// The texture-page word (`tsb & 0x1FF`).
    pub tpage: u16,
    /// Inclusive UV rectangle `[u0, v0, u1, v1]` the art occupies. A prim
    /// matches when its UV bounding box lies inside.
    pub rect: [u8; 4],
    /// [`window_art_hash`] of the rectangle.
    pub hash: u64,
}

/// The curated glazed-window arts. Picked by eye from each town's atlases
/// (the blend rule cannot find these: a window is an opaque prim on a wall
/// tile, and blue texels alone also match sky sheets, water and slate).
pub const LIT_WINDOWS: &[WindowArt] = &[
    WindowArt {
        label: "Rim Elm: arched window with shutters",
        cba: 0x7ACE,
        tpage: 0x01C,
        rect: [160, 64, 199, 111],
        hash: 0x13a4_bf6e_d22c_98e4,
    },
    WindowArt {
        label: "Rim Elm: plaster wall with a mullioned window",
        cba: 0x7ACB,
        tpage: 0x01C,
        rect: [64, 192, 127, 255],
        hash: 0xcf50_1d3e_57a7_eae1,
    },
    WindowArt {
        label: "Vidna: stone wall with a six-pane window",
        cba: 0x7A41,
        tpage: 0x01A,
        rect: [128, 0, 191, 95],
        hash: 0x6d0b_45ae_52dd_4a8c,
    },
    WindowArt {
        label: "Vidna: stone wall with a curtained window",
        cba: 0x7A46,
        tpage: 0x01A,
        rect: [0, 0, 63, 95],
        hash: 0x34ea_d49c_14d8_a5d8,
    },
    WindowArt {
        label: "Vidna (interiors): plaster wall with four-pane windows",
        cba: 0x7AC0,
        tpage: 0x00B,
        rect: [0, 0, 95, 80],
        hash: 0xe62d_54d5_b1fb_49d3,
    },
    WindowArt {
        label: "Vidna (interiors): narrow window",
        cba: 0x7ACE,
        tpage: 0x00B,
        rect: [224, 0, 255, 47],
        hash: 0xf3e1_7cd4_bf67_4500,
    },
];

/// One decoded 15-bit texel word of a page under a CLUT (`0` outside VRAM),
/// exactly as the shaders decode it.
pub fn page_texel(vram: &legaia_tim::Vram, cba: u16, tsb: u16, u: usize, v: usize) -> u16 {
    let cx = ((cba & 0x3F) * 16) as usize;
    let cy = ((cba >> 6) & 0x1FF) as usize;
    let tx = ((tsb & 0xF) * 64) as usize;
    let ty = (((tsb >> 4) & 1) * 256) as usize;
    let px = |x: usize, y: usize| -> u16 {
        if x >= legaia_tim::VRAM_WIDTH || y >= legaia_tim::VRAM_HEIGHT {
            0
        } else {
            vram.pixel(x, y)
        }
    };
    match (tsb >> 7) & 0x3 {
        0 => {
            let w = px(tx + (u >> 2), ty + v);
            px(cx + ((w >> ((u & 3) * 4)) & 0xF) as usize, cy)
        }
        1 => {
            let w = px(tx + (u >> 1), ty + v);
            px(cx + ((w >> ((u & 1) * 8)) & 0xFF) as usize, cy)
        }
        _ => px(tx + u, ty + v),
    }
}

/// FNV-1a 64 over the decoded texel words of `rect` (row-major) - the
/// [`WindowArt::hash`] key.
pub fn window_art_hash(vram: &legaia_tim::Vram, cba: u16, tpage: u16, rect: [u8; 4]) -> u64 {
    let mut h = SIGNATURE_BASIS;
    for v in rect[1]..=rect[3] {
        for u in rect[0]..=rect[2] {
            h = fnv1a(
                h,
                &page_texel(vram, cba, tpage, u as usize, v as usize).to_le_bytes(),
            );
        }
    }
    h
}

/// The curated window art a textured prim samples, if any: same CLUT and
/// page, its UV box inside the art's rectangle, and the rectangle's texels
/// hashing to the entry's key in this VRAM.
pub fn prim_window_art(
    vram: &legaia_tim::Vram,
    cba: u16,
    tsb: u16,
    uvs: &[[u8; 2]],
) -> Option<&'static WindowArt> {
    LIT_WINDOWS
        .iter()
        .find(|w| prim_samples_art(w, vram, cba, tsb, uvs))
}

/// Whether a textured prim (`cba`, `tsb`, corner `uvs`) samples `art` in
/// this VRAM - the [`prim_window_art`] test for one entry.
pub fn prim_samples_art(
    art: &WindowArt,
    vram: &legaia_tim::Vram,
    cba: u16,
    tsb: u16,
    uvs: &[[u8; 2]],
) -> bool {
    let (cba, tpage) = (cba & 0x7FFF, tsb & 0x1FF);
    if uvs.is_empty() || art.cba != cba || art.tpage != tpage {
        return false;
    }
    let inside = uvs.iter().all(|uv| {
        (art.rect[0]..=art.rect[2]).contains(&uv[0]) && (art.rect[1]..=art.rect[3]).contains(&uv[1])
    });
    inside && window_art_hash(vram, cba, tpage, art.rect) == art.hash
}

/// Set [`WINDOW_BIT`] on every corner of each textured prim (`tri`s of
/// `indices`) that samples a [`LIT_WINDOWS`] art; `untextured(i0)` skips a
/// hybrid stream's colour prims. Returns the number of triangles tagged.
pub(super) fn tag_windows(
    cba_tsb: &mut [[u16; 2]],
    uvs: &[[u8; 2]],
    indices: &[u32],
    vram: &legaia_tim::Vram,
    untextured: impl Fn(usize) -> bool,
) -> usize {
    let mut n = 0;
    for tri in indices.as_chunks::<3>().0 {
        let i0 = tri[0] as usize;
        if untextured(i0) {
            continue;
        }
        let Some(&[cba, tsb]) = cba_tsb.get(i0) else {
            continue;
        };
        let corner: Vec<[u8; 2]> = tri
            .iter()
            .filter_map(|&i| uvs.get(i as usize).copied())
            .collect();
        if prim_window_art(vram, cba, tsb, &corner).is_some() {
            for &i in tri {
                if let Some(ct) = cba_tsb.get_mut(i as usize) {
                    ct[1] |= WINDOW_BIT;
                }
            }
            n += 1;
        }
    }
    n
}

/// Glass test on a raw texel: saturated blue ([`WINDOW_GLASS_MIN_BLUE`])
/// or near-black ([`WINDOW_GLASS_BLACK_MAX`]).
pub fn window_glass(texel: [f32; 3]) -> bool {
    let blue = texel[2] - texel[0] >= WINDOW_GLASS_MIN_BLUE && texel[2] >= texel[1];
    let black = texel[0].max(texel[1]).max(texel[2]) <= WINDOW_GLASS_BLACK_MAX;
    blue || black
}

/// CPU mirror of the shaders' `dyn_window`: a [`WINDOW_BIT`] prim's glass
/// texel blends from its lit colour toward the warm pane colour by the
/// mood's window glow; every other texel (and every texel while the
/// enhancement is off or the glow is zero) keeps `lit`.
///
/// ```text
/// glass = (texel.b - texel.r >= WINDOW_GLASS_MIN_BLUE && texel.b >= texel.g)
///      || max(texel) <= WINDOW_GLASS_BLACK_MAX
/// pane  = WINDOW_RGB * (WINDOW_FLOOR + (1 - WINDOW_FLOOR) * texel.b)
/// out   = window && glass ? mix(lit, pane, window_glow) : lit
/// ```
pub fn shade_window(
    lit: [f32; 3],
    texel: [f32; 3],
    uniforms: [[f32; 4]; 3],
    window_glow: f32,
    window: bool,
) -> [f32; 3] {
    if uniforms[0][3] < 0.5 || !window || window_glow <= 0.0 || !window_glass(texel) {
        return lit;
    }
    let k = WINDOW_FLOOR + (1.0 - WINDOW_FLOOR) * texel[2];
    let t = window_glow.clamp(0.0, 1.0);
    [0, 1, 2].map(|i| (lit[i] + (WINDOW_RGB[i] * k - lit[i]) * t).clamp(0.0, 1.0))
}
