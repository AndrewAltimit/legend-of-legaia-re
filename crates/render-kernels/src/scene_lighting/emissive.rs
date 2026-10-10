//! Emissive tagging: which primitives glow, the curated mesh table and the per-mesh taggers.

use super::*;
use glam::Vec3;

// ---------------------------------------------------------------------------
// Emissive tagging
// ---------------------------------------------------------------------------

/// Bit 13 of a per-vertex TSB word (VRAM mesh) or blend word (colour mesh):
/// the prim draws **emissive** under the enhanced-lighting model. Free in
/// both words (see the module docs). WGSL / GLSL twin: `0x2000u`.
pub const EMISSIVE_BIT: u16 = 0x2000;

/// The semi-transparency enable both mesh halves carry in bit 15 (mirrors
/// `legaia_tmd::mesh::TSB_SEMI_TRANSPARENT_BIT`).
pub(super) const SEMI_BIT: u16 = 0x8000;

/// ABR blend mode, bits 5..=6.
fn abr_mode(word: u16) -> u8 {
    ((word >> 5) & 0x3) as u8
}

/// Minimum "bright" max-channel (0..255 modulation byte) for the
/// bright-warm emitter test. `0x80` is neutral; flames are authored well
/// above it.
pub const EMIT_MIN_BRIGHT: u8 = 0xB0;

/// How much warmer than blue (0..255 units) a bright prim must be to read
/// as a flame. Neutral greys - the brightened panes of a water or sky sheet
/// - fail it; candle / torch palettes clear it easily.
pub const EMIT_MIN_WARMTH: u8 = 0x18;

/// Bright-and-warm test on a raw 0..255 modulation/vertex colour: max
/// channel over [`EMIT_MIN_BRIGHT`] and red clearly above blue (candle /
/// torch palettes are warm; water, glass and grey sky sheets are not).
fn bright_warm(c: [u8; 3]) -> bool {
    let m = c[0].max(c[1]).max(c[2]);
    m >= EMIT_MIN_BRIGHT && c[0] >= c[2].saturating_add(EMIT_MIN_WARMTH)
}

/// Whether one textured prim reads as authored glow: a semi-transparent
/// prim in the PSX additive mode (ABR 1, `B + F` - the "glow" blend), or a
/// semi-transparent prim whose baked modulation is bright and warm.
pub fn textured_prim_glows(tsb: u16, color: [u8; 3]) -> bool {
    tsb & SEMI_BIT != 0 && (abr_mode(tsb) == 1 || bright_warm(color))
}

/// The untextured twin of [`textured_prim_glows`]: Legaia's untextured
/// prims carry no ABR field, so the test is ABE + bright-warm fill.
pub fn color_prim_glows(blend: u16, color: [u8; 3]) -> bool {
    blend & SEMI_BIT != 0 && bright_warm(color)
}

/// Which prims of a curated [`EmissiveMesh`] glow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CuratedTexels {
    /// Every prim.
    All,
    /// Prims whose drawn colour is green-dominant ([`texels_green`] over
    /// the mean texel times the prim's modulation, or the fill of an
    /// untextured prim) - a glowing tree's foliage, not its trunk or the
    /// ground it stands on.
    Green,
}

/// One curated emissive mesh: a surface whose glow the art implies but no
/// blend mode carries. Keyed by a content hash of the TMD's bytes
/// ([`tmd_signature`]), so the same model is recognised in every scene that
/// loads it and the table holds no disc bytes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EmissiveMesh {
    /// [`tmd_signature`] of the TMD.
    pub signature: u64,
    /// What it is (documentation only).
    pub label: &'static str,
    /// Which of its prims glow.
    pub texels: CuratedTexels,
    /// Light colour the mesh casts (0..=1 gain colour).
    pub light: [f32; 3],
    /// Influence radius of the light it casts, world units.
    pub radius: f32,
}

/// The curated emissive surfaces. Small on purpose - the blend-mode rule
/// above finds the authored glow; this lists only what it cannot.
///
/// The Genesis Tree is Rim Elm's plaza tree - a MAN scene-actor prop drawn
/// with opaque prims and no glow blend, yet the village's light: the
/// story's living Seru tree. It ships as two models, the plaza prop and the
/// full-grown tree with its ring of roots the story stages later; both are
/// listed.
pub const EMISSIVE_MESHES: &[EmissiveMesh] = &[
    EmissiveMesh {
        signature: 0x02c8_7921_de1b_8a44,
        label: "Genesis Tree (Rim Elm plaza)",
        texels: CuratedTexels::All,
        light: [0.45, 1.0, 0.55],
        radius: 1300.0,
    },
    EmissiveMesh {
        signature: 0x741a_6022_0bde_804a,
        label: "Genesis Tree (full-grown, with its roots)",
        texels: CuratedTexels::Green,
        light: [0.45, 1.0, 0.55],
        radius: 1600.0,
    },
];

pub(super) fn fnv1a(mut h: u64, bytes: &[u8]) -> u64 {
    for &b in bytes {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// FNV-1a 64 offset basis.
pub const SIGNATURE_BASIS: u64 = 0xcbf2_9ce4_8422_2325;

/// The key [`EMISSIVE_MESHES`] matches: FNV-1a 64 over a TMD's **model
/// content** - each object's header counts and its vertex coordinates, as
/// parsed. Hashing the content rather than the raw slice keeps the key
/// stable across the ways one model reaches a host: a scan slice that runs
/// past the TMD's end, an exact copy out of a scene's model bank, a VDF
/// morph (applied to a parsed copy, never to these bytes). A buffer that
/// does not parse hashes as its raw bytes.
pub fn tmd_signature(raw: &[u8]) -> u64 {
    let Ok(tmd) = legaia_tmd::parse(raw) else {
        return fnv1a(SIGNATURE_BASIS, raw);
    };
    let mut h = fnv1a(SIGNATURE_BASIS, &(tmd.objects.len() as u32).to_le_bytes());
    for o in &tmd.objects {
        h = fnv1a(h, &o.header.n_vert.to_le_bytes());
        h = fnv1a(h, &o.header.n_normal.to_le_bytes());
        h = fnv1a(h, &o.header.n_primitive.to_le_bytes());
        for v in &o.vertices {
            h = fnv1a(h, &v.x.to_le_bytes());
            h = fnv1a(h, &v.y.to_le_bytes());
            h = fnv1a(h, &v.z.to_le_bytes());
        }
    }
    h
}

/// The curated entry for a TMD, if any.
pub fn curated_emissive(raw: &[u8]) -> Option<&'static EmissiveMesh> {
    let sig = tmd_signature(raw);
    EMISSIVE_MESHES.iter().find(|e| e.signature == sig)
}

/// The mean opaque texel colour (0..=1) of a textured prim, sampled on a 7x7
/// grid over its UV bounding box and decoded through its CLUT exactly as the
/// shaders do. Transparent texels are skipped; `None` when there are none.
pub fn prim_mean_texel(
    vram: &legaia_tim::Vram,
    cba: u16,
    tsb: u16,
    uvs: &[[u8; 2]],
) -> Option<[f32; 3]> {
    if uvs.is_empty() {
        return None;
    }
    let cx = ((cba & 0x3F) * 16) as usize;
    let cy = ((cba >> 6) & 0x1FF) as usize;
    let tx = ((tsb & 0xF) * 64) as usize;
    let ty = (((tsb >> 4) & 1) * 256) as usize;
    let depth = (tsb >> 7) & 0x3;
    let (mut u0, mut u1, mut v0, mut v1) = (u8::MAX, 0u8, u8::MAX, 0u8);
    for uv in uvs {
        u0 = u0.min(uv[0]);
        u1 = u1.max(uv[0]);
        v0 = v0.min(uv[1]);
        v1 = v1.max(uv[1]);
    }
    let px = |x: usize, y: usize| -> u16 {
        if x >= legaia_tim::VRAM_WIDTH || y >= legaia_tim::VRAM_HEIGHT {
            0
        } else {
            vram.pixel(x, y)
        }
    };
    const STEPS: usize = 6;
    let mut opaque = 0usize;
    let mut sum = [0.0f32; 3];
    for si in 0..=STEPS {
        for sj in 0..=STEPS {
            let u = u0 as usize + (u1 - u0) as usize * si / STEPS;
            let v = v0 as usize + (v1 - v0) as usize * sj / STEPS;
            let w = match depth {
                0 => {
                    let w = px(tx + (u >> 2), ty + v);
                    px(cx + ((w >> ((u & 3) * 4)) & 0xF) as usize, cy)
                }
                1 => {
                    let w = px(tx + (u >> 1), ty + v);
                    px(cx + ((w >> ((u & 1) * 8)) & 0xFF) as usize, cy)
                }
                _ => px(tx + u, ty + v),
            };
            if w == 0 {
                continue;
            }
            opaque += 1;
            sum[0] += (w & 31) as f32 / 31.0;
            sum[1] += ((w >> 5) & 31) as f32 / 31.0;
            sum[2] += ((w >> 10) & 31) as f32 / 31.0;
        }
    }
    (opaque > 0).then(|| sum.map(|c| c / opaque as f32))
}

/// Green-dominant test on a mean texel colour: green clearly above both
/// red and blue.
pub fn texels_green(mean: [f32; 3]) -> bool {
    mean[1] > mean[0] + 0.05 && mean[1] > mean[2] + 0.05
}

/// Whether one prim of a curated mesh glows under its [`CuratedTexels`]
/// rule. `color` is the prim's colour word (modulation for a textured prim,
/// fill for an untextured one); `textured` is `Some((vram, cba, tsb, uvs))`
/// for a textured prim.
fn curated_prim_glows(
    rule: CuratedTexels,
    color: [u8; 3],
    textured: Option<(&legaia_tim::Vram, u16, u16, &[[u8; 2]])>,
) -> bool {
    match rule {
        CuratedTexels::All => true,
        CuratedTexels::Green => {
            let c = color.map(|v| f32::from(v) / 128.0);
            let drawn = match textured {
                Some((vram, cba, tsb, uvs)) => {
                    let Some(m) = prim_mean_texel(vram, cba & 0x7FFF, tsb, uvs) else {
                        return false;
                    };
                    [m[0] * c[0], m[1] * c[1], m[2] * c[2]]
                }
                None => c,
            };
            texels_green(drawn)
        }
    }
}

/// What tagging a curated mesh found: the entry and the model-space centre
/// of its glowing prims (where its light sits).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CuratedHit {
    pub entry: &'static EmissiveMesh,
    pub center: [f32; 3],
}

fn set_emissive(cba_tsb: &mut [[u16; 2]], tri: &[u32; 3]) {
    for &i in tri {
        if let Some(ct) = cba_tsb.get_mut(i as usize) {
            ct[1] |= EMISSIVE_BIT;
        }
    }
}

/// Bounding-box centre accumulator over tagged prims.
#[derive(Default)]
struct Bounds(Option<(Vec3, Vec3)>);

impl Bounds {
    fn add(&mut self, positions: &[[f32; 3]], tri: &[u32; 3]) {
        for &i in tri {
            if let Some(p) = positions.get(i as usize) {
                let p = Vec3::from(*p);
                self.0 = Some(match self.0 {
                    Some((lo, hi)) => (lo.min(p), hi.max(p)),
                    None => (p, p),
                });
            }
        }
    }
    fn center(&self) -> Option<[f32; 3]> {
        self.0.map(|(lo, hi)| ((lo + hi) * 0.5).to_array())
    }
}

/// Tag the glowing prims of a textured VRAM mesh (`cba_tsb` / `colors` /
/// `uvs` parallel per vertex, `indices` a triangle list) by setting
/// [`EMISSIVE_BIT`] on every corner of each glowing prim: the blend rule
/// ([`textured_prim_glows`]) always, plus `curated`'s texel rule when the
/// mesh is a curated entry. Returns the number of triangles tagged and the
/// model-space centre of the curated prims.
pub fn tag_emissive_vram(
    positions: &[[f32; 3]],
    cba_tsb: &mut [[u16; 2]],
    colors: &[[u8; 3]],
    uvs: &[[u8; 2]],
    indices: &[u32],
    curated: Option<(&EmissiveMesh, &legaia_tim::Vram)>,
) -> (usize, Option<[f32; 3]>) {
    let mut n = 0;
    let mut bounds = Bounds::default();
    for tri in indices.as_chunks::<3>().0 {
        let i0 = tri[0] as usize;
        let (Some(&[cba, tsb]), Some(&c)) = (cba_tsb.get(i0), colors.get(i0)) else {
            continue;
        };
        let by_curated = curated.is_some_and(|(e, vram)| {
            let corner: Vec<[u8; 2]> = tri
                .iter()
                .filter_map(|&i| uvs.get(i as usize).copied())
                .collect();
            curated_prim_glows(e.texels, c, Some((vram, cba, tsb, &corner)))
        });
        if by_curated {
            bounds.add(positions, tri);
        }
        if by_curated || textured_prim_glows(tsb, c) {
            set_emissive(cba_tsb, tri);
            n += 1;
        }
    }
    (n, bounds.center())
}

/// [`tag_emissive_vram`] for an untextured colour mesh's blend words (a
/// curated entry's [`CuratedTexels::All`] rule reaches these too; a texel
/// rule cannot - there are no texels).
pub fn tag_emissive_color(
    positions: &[[f32; 3]],
    blend: &mut [u16],
    colors: &[[u8; 3]],
    indices: &[u32],
    curated: Option<&EmissiveMesh>,
) -> (usize, Option<[f32; 3]>) {
    let mut n = 0;
    let mut bounds = Bounds::default();
    for tri in indices.as_chunks::<3>().0 {
        let i0 = tri[0] as usize;
        let (Some(&w), Some(&c)) = (blend.get(i0), colors.get(i0)) else {
            continue;
        };
        let by_curated = curated.is_some_and(|e| curated_prim_glows(e.texels, c, None));
        if by_curated {
            bounds.add(positions, tri);
        }
        if by_curated || color_prim_glows(w, c) {
            for &i in tri {
                if let Some(w) = blend.get_mut(i as usize) {
                    *w |= EMISSIVE_BIT;
                }
            }
            n += 1;
        }
    }
    (n, bounds.center())
}

/// Tag both halves of one TMD's mesh build (the native window keeps the
/// textured and untextured halves on separate pipelines). Returns the
/// curated hit when the TMD is an [`EMISSIVE_MESHES`] entry and any of its
/// prims glow.
pub fn tag_emissive_meshes(
    raw: &[u8],
    vmesh: &mut legaia_tmd::mesh::VramMesh,
    cmesh: &mut legaia_tmd::mesh::ColorMesh,
    vram: &legaia_tim::Vram,
) -> Option<CuratedHit> {
    let curated = curated_emissive(raw);
    let (_, vc) = tag_emissive_vram(
        &vmesh.positions,
        &mut vmesh.cba_tsb,
        &vmesh.colors,
        &vmesh.uvs,
        &vmesh.indices,
        curated.map(|e| (e, vram)),
    );
    let (_, cc) = tag_emissive_color(
        &cmesh.positions,
        &mut cmesh.blend,
        &cmesh.colors,
        &cmesh.indices,
        curated,
    );
    tag_windows(&mut vmesh.cba_tsb, &vmesh.uvs, &vmesh.indices, vram, |_| {
        false
    });
    let entry = curated?;
    Some(CuratedHit {
        entry,
        center: vc.or(cc)?,
    })
}

/// [`tag_emissive_meshes`] for a lone textured half (a re-posed frame that
/// rebuilds only the textured mesh).
pub fn tag_emissive_vram_mesh(
    raw: &[u8],
    vmesh: &mut legaia_tmd::mesh::VramMesh,
    vram: &legaia_tim::Vram,
) {
    let curated = curated_emissive(raw);
    tag_emissive_vram(
        &vmesh.positions,
        &mut vmesh.cba_tsb,
        &vmesh.colors,
        &vmesh.uvs,
        &vmesh.indices,
        curated.map(|e| (e, vram)),
    );
    tag_windows(&mut vmesh.cba_tsb, &vmesh.uvs, &vmesh.indices, vram, |_| {
        false
    });
}

/// [`tag_emissive_meshes`] for a lone untextured half.
pub fn tag_emissive_color_mesh(raw: &[u8], cmesh: &mut legaia_tmd::mesh::ColorMesh) {
    tag_emissive_color(
        &cmesh.positions,
        &mut cmesh.blend,
        &cmesh.colors,
        &cmesh.indices,
        curated_emissive(raw),
    );
}

/// [`tag_emissive_meshes`] over a **hybrid** stream (the browser's merged
/// textured + untextured mesh): `flat` is the parallel `[r, g, b, flag]`
/// array, `flag == 0` marking an untextured vertex whose fill colour (not
/// its neutral modulation word) is what the glow test reads, and whose TSB
/// slot carries the colour half's blend word. Each prim takes the rule of
/// the half it came from, so the result is bit-for-bit the two native
/// halves' tags.
pub fn tag_emissive_hybrid(
    raw: &[u8],
    mesh: &mut legaia_tmd::mesh::VramMesh,
    flat: &[u8],
    vram: &legaia_tim::Vram,
) -> Option<CuratedHit> {
    let curated = curated_emissive(raw);
    let mut bounds = Bounds::default();
    for tri in mesh.indices.as_chunks::<3>().0 {
        let i0 = tri[0] as usize;
        let Some(&[cba, tsb]) = mesh.cba_tsb.get(i0) else {
            continue;
        };
        let untextured = flat.get(i0 * 4 + 3) == Some(&0);
        let (by_curated, by_blend) = if untextured {
            let c = [flat[i0 * 4], flat[i0 * 4 + 1], flat[i0 * 4 + 2]];
            (
                curated.is_some_and(|e| curated_prim_glows(e.texels, c, None)),
                color_prim_glows(tsb, c),
            )
        } else {
            let corner: Vec<[u8; 2]> = tri
                .iter()
                .filter_map(|&i| mesh.uvs.get(i as usize).copied())
                .collect();
            let c = mesh.colors.get(i0).copied().unwrap_or([0x80; 3]);
            (
                curated.is_some_and(|e| {
                    curated_prim_glows(e.texels, c, Some((vram, cba, tsb, &corner)))
                }),
                mesh.colors
                    .get(i0)
                    .is_some_and(|&c| textured_prim_glows(tsb, c)),
            )
        };
        if by_curated {
            bounds.add(&mesh.positions, tri);
        }
        if by_curated || by_blend {
            set_emissive(&mut mesh.cba_tsb, tri);
        }
    }
    tag_windows(&mut mesh.cba_tsb, &mesh.uvs, &mesh.indices, vram, |i0| {
        flat.get(i0 * 4 + 3) == Some(&0)
    });
    Some(CuratedHit {
        entry: curated?,
        center: bounds.center()?,
    })
}
