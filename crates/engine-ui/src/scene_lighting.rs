//! **Enhanced lighting** - the one engine-side source of truth both hosts
//! light a scene from: the derived point-light list, the emissive surface
//! set, the scene mood (ambient / key light / time of day) and the glow
//! sprites that stand in for bloom.
//!
//! # This is NOT retail
//!
//! Retail field rendering has no light source at all: both TMD renderers
//! issue exactly one GTE colour op (`DPCS`, the depth cue) and never an
//! `NC*` lighting op, so all field shading is baked into the TMD colour
//! words and applied as `texel * colour / 128`. Everything in this module
//! feeds a deliberate enhancement layered over that baked shading, behind
//! one runtime toggle per host (`I` in `play-window`, the "Enhanced
//! lighting" checkbox on the browser play page). With the toggle off no
//! host reads any of it and the render is pixel-identical to the faithful
//! path.
//!
//! The native renderer (`engine-render`) and the browser play page
//! (`site/js/webgl-*.js`, fed by `web-viewer`) consume the *same* values
//! from here: the light list and emissive tags are derived by these
//! functions on both hosts, and the mood a frame is lit under is
//! [`LightingMood::for_scene`] / [`TimeOfDay::mood`] on both hosts. The
//! shading law itself is expressed twice (WGSL and GLSL) and is mirrored on
//! the CPU by [`shade`], whose constants are paired with both shader
//! twins by tests and by `check-ui-host-drift.py`.
//!
//! # Light derivation
//!
//! What the player reads as candles, lamps and lit windows is
//! emissive-looking *geometry*: small additive-blended (ABE, ABR mode 1)
//! glow prims and bright-modulated lamp meshes whose glow is baked into the
//! art. [`vram_mesh_emitters`] / [`color_mesh_emitters`] read that
//! authoring signal back out of the mesh data, and the curated
//! [`EMISSIVE_MESHES`] table adds the surfaces whose glow the art implies
//! but no blend mode carries (the Genesis Tree). Samples are instanced into
//! world space by the host's own placement transforms and clustered
//! ([`cluster_all_scene_lights`]); the [`nearest_lights`] to the player
//! shade each frame.
//!
//! # Emissive tagging
//!
//! A prim that emits is also drawn as **emissive**: it ignores the mood's
//! darkening and draws at [`LightingMood::emissive_gain`], so a lamp stays
//! lit at night while the wall around it falls into shadow. The tag rides
//! bit 13 ([`EMISSIVE_BIT`]) of the per-vertex TSB word (textured prims)
//! and of the colour mesh's blend word (untextured prims) - bits retail's
//! words never use (a TSB uses bits 0..=8, a blend word bits 5..=6 and 15,
//! the double-sided flag bit 14) - and every shader masks it out of every
//! decode. Shaders read it only while the enhancement is on.

use glam::{Mat4, Vec3};

// ---------------------------------------------------------------------------
// Emissive tagging
// ---------------------------------------------------------------------------

/// Bit 13 of a per-vertex TSB word (VRAM mesh) or blend word (colour mesh):
/// the prim draws **emissive** under the enhanced-lighting model. Free in
/// both words (see the module docs). WGSL / GLSL twin: `0x2000u`.
pub const EMISSIVE_BIT: u16 = 0x2000;

/// The semi-transparency enable both mesh halves carry in bit 15 (mirrors
/// `legaia_tmd::mesh::TSB_SEMI_TRANSPARENT_BIT`).
const SEMI_BIT: u16 = 0x8000;

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

fn fnv1a(mut h: u64, bytes: &[u8]) -> u64 {
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
    Some(CuratedHit {
        entry: curated?,
        center: bounds.center()?,
    })
}

// ---------------------------------------------------------------------------
// Point-light derivation
// ---------------------------------------------------------------------------

/// Hard cap on lights shading at once - also the WGSL / GLSL array length
/// and the shadow-map layer count. WGSL twin: `SCENE_LIGHT_MAX`.
pub const MAX_SCENE_LIGHTS: usize = 8;

/// Triangles larger than this (model-space area, PSX units squared) never
/// count as emitters - glow *props* are small; big additive surfaces are
/// water / sky sheets.
pub const EMIT_MAX_TRI_AREA: f32 = 20_000.0;

/// Greedy cluster merge distance (world units). Two emitter samples
/// closer than this are one prop (a flame's two crossed quads, a
/// chandelier's candle ring).
pub const CLUSTER_MERGE_DIST: f32 = 320.0;

/// A cluster whose samples spread farther than this from its centroid is
/// a surface, not a prop - dropped.
pub const CLUSTER_MAX_EXTENT: f32 = 480.0;

/// Point-light radius from cluster weight: `radius = RADIUS_SCALE *
/// sqrt(weight)`, clamped to [`RADIUS_MIN`] ..= [`RADIUS_MAX`].
pub const RADIUS_SCALE: f32 = 14.0;
pub const RADIUS_MIN: f32 = 520.0;
pub const RADIUS_MAX: f32 = 1600.0;

/// How far above its emitting geometry (world units, -Y in the Y-down
/// field frame) a derived light sits. Lamps are authored on surfaces - a
/// glow decal on the ground, a pane in a wall - and a light exactly on the
/// surface grazes it; lifted, it pools onto the floor around it.
pub const LIGHT_LIFT: f32 = 48.0;

/// One derived world-space point light.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScenePointLight {
    /// World position (retail Y-down field frame - the same space the
    /// scene draws' model matrices target).
    pub pos: [f32; 3],
    /// Light colour, 0..=1 per channel (a *gain* colour: the fragment's
    /// baked colour is scaled by `1 + Σ colour_i * att_i * ...`).
    pub color: [f32; 3],
    /// Influence radius in world units; the attenuation reaches exactly
    /// zero here ([`point_attenuation`]).
    pub radius: f32,
}

/// One emitter-looking triangle, in whatever space the mesh data was in.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EmitterSample {
    pub pos: [f32; 3],
    /// Normalised 0..=1 colour of the glow.
    pub color: [f32; 3],
    /// Ranking weight: `sqrt(area) * luminance`.
    pub weight: f32,
}

fn tri_area(a: [f32; 3], b: [f32; 3], c: [f32; 3]) -> f32 {
    let ab = Vec3::from(b) - Vec3::from(a);
    let ac = Vec3::from(c) - Vec3::from(a);
    ab.cross(ac).length() * 0.5
}

fn centroid(a: [f32; 3], b: [f32; 3], c: [f32; 3]) -> [f32; 3] {
    [
        (a[0] + b[0] + c[0]) / 3.0,
        (a[1] + b[1] + c[1]) / 3.0,
        (a[2] + b[2] + c[2]) / 3.0,
    ]
}

fn sample_from_tri(pos: [[f32; 3]; 3], color: [u8; 3]) -> Option<EmitterSample> {
    let area = tri_area(pos[0], pos[1], pos[2]);
    if area <= 0.0 || area > EMIT_MAX_TRI_AREA {
        return None;
    }
    let m = color[0].max(color[1]).max(color[2]).max(1) as f32;
    // Neutral-modulated additive prims (0x80,0x80,0x80) glow with their
    // texel colour, which we don't decode here - assume warm flame.
    let (rgb, lum) = if color == [0x80; 3] {
        ([1.0, 0.82, 0.55], 1.0)
    } else {
        (
            [
                color[0] as f32 / m,
                color[1] as f32 / m,
                color[2] as f32 / m,
            ],
            (m / 255.0).min(1.0),
        )
    };
    Some(EmitterSample {
        pos: centroid(pos[0], pos[1], pos[2]),
        color: rgb,
        weight: area.sqrt() * lum,
    })
}

/// Emitter samples of a textured VRAM mesh (`legaia_tmd::mesh::VramMesh`
/// data, model space). A triangle is a sample when
/// [`textured_prim_glows`] holds for it and it is small enough to be a
/// prop ([`EMIT_MAX_TRI_AREA`]). With `texels` (`(vram, uvs)`), a glow prim
/// whose modulation is neutral takes its light colour from its own texels
/// (an additive green glow casts green), instead of the assumed flame.
pub fn vram_mesh_emitters(
    positions: &[[f32; 3]],
    cba_tsb: &[[u16; 2]],
    colors: &[[u8; 3]],
    indices: &[u32],
    texels: Option<(&legaia_tim::Vram, &[[u8; 2]])>,
) -> Vec<EmitterSample> {
    let mut out = Vec::new();
    for tri in indices.as_chunks::<3>().0 {
        let i0 = tri[0] as usize;
        if i0 >= cba_tsb.len() || i0 >= colors.len() {
            continue;
        }
        let c = colors[i0];
        if !textured_prim_glows(cba_tsb[i0][1], c) {
            continue;
        }
        let p = |i: u32| positions.get(i as usize).copied().unwrap_or([0.0; 3]);
        if let Some(mut s) = sample_from_tri([p(tri[0]), p(tri[1]), p(tri[2])], c) {
            if let Some((vram, uvs)) = texels {
                let corner_uvs: Vec<[u8; 2]> = tri
                    .iter()
                    .filter_map(|&i| uvs.get(i as usize).copied())
                    .collect();
                let [ct_cba, ct_tsb] = cba_tsb[i0];
                let mean =
                    prim_mean_texel(vram, ct_cba & 0x7FFF, ct_tsb, &corner_uvs).unwrap_or([0.0; 3]);
                let modulated = [
                    mean[0] * c[0] as f32 / 128.0,
                    mean[1] * c[1] as f32 / 128.0,
                    mean[2] * c[2] as f32 / 128.0,
                ];
                let m = modulated[0].max(modulated[1]).max(modulated[2]);
                if m > 0.05 {
                    s.color = modulated.map(|v| v / m);
                    s.weight *= m.min(1.0);
                }
            }
            out.push(s);
        }
    }
    out
}

/// Emitter samples of an untextured colour mesh
/// (`legaia_tmd::mesh::ColorMesh` data, model space): ABE + bright-warm
/// vertex colour ([`color_prim_glows`]).
pub fn color_mesh_emitters(
    positions: &[[f32; 3]],
    colors: &[[u8; 3]],
    blend: &[u16],
    indices: &[u32],
) -> Vec<EmitterSample> {
    let mut out = Vec::new();
    for tri in indices.as_chunks::<3>().0 {
        let i0 = tri[0] as usize;
        let (Some(&word), Some(&c)) = (blend.get(i0), colors.get(i0)) else {
            continue;
        };
        if !color_prim_glows(word, c) {
            continue;
        }
        let p = |i: u32| positions.get(i as usize).copied().unwrap_or([0.0; 3]);
        if let Some(s) = sample_from_tri([p(tri[0]), p(tri[1]), p(tri[2])], c) {
            out.push(s);
        }
    }
    out
}

/// Emitter samples of a **hybrid** stream (the browser's merged textured +
/// untextured mesh; `flat` is its `[r, g, b, flag]` side channel, `flag ==
/// 0` an untextured vertex whose TSB slot carries the colour half's blend
/// word). Each prim takes the test of the half it came from, so this is
/// exactly [`vram_mesh_emitters`] + [`color_mesh_emitters`] over the two
/// halves the native window keeps apart.
pub fn hybrid_mesh_emitters(
    positions: &[[f32; 3]],
    cba_tsb: &[[u16; 2]],
    colors: &[[u8; 3]],
    flat: &[u8],
    indices: &[u32],
    texels: Option<(&legaia_tim::Vram, &[[u8; 2]])>,
) -> Vec<EmitterSample> {
    let mut out = Vec::new();
    for tri in indices.as_chunks::<3>().0 {
        let i0 = tri[0] as usize;
        let Some(ct) = cba_tsb.get(i0) else {
            continue;
        };
        let untextured = flat.get(i0 * 4 + 3) == Some(&0);
        let (glows, c) = if untextured {
            let c = [flat[i0 * 4], flat[i0 * 4 + 1], flat[i0 * 4 + 2]];
            (color_prim_glows(ct[1], c), c)
        } else {
            let Some(&c) = colors.get(i0) else {
                continue;
            };
            (textured_prim_glows(ct[1], c), c)
        };
        if !glows {
            continue;
        }
        if untextured {
            let p = |i: u32| positions.get(i as usize).copied().unwrap_or([0.0; 3]);
            if let Some(s) = sample_from_tri([p(tri[0]), p(tri[1]), p(tri[2])], c) {
                out.push(s);
            }
        } else {
            // The textured half goes through the one textured rule (texel
            // tint included), one prim at a time.
            out.extend(vram_mesh_emitters(positions, cba_tsb, colors, tri, texels));
        }
    }
    out
}

/// A `.MAP` placement's model matrix in the retail field frame: translate
/// to the placement's world position, rotate by its three authored angles in
/// retail's `Rx * Ry * Rz` order ([`crate::battle_intro::placement_rotation`]),
/// scale. The transform both hosts instance static emitters through.
pub fn placement_model(world: [f32; 3], rot: [u16; 3], scale: f32) -> Mat4 {
    Mat4::from_translation(Vec3::from(world))
        * crate::battle_intro::placement_rotation(rot[0], rot[1], rot[2])
        * Mat4::from_scale(Vec3::splat(scale))
}

/// The single sample a curated [`EMISSIVE_MESHES`] entry contributes: one
/// strong light at the centre of its glowing prims (model space - the
/// [`CuratedHit::center`] tagging found), weighted so it clusters into a
/// light of the entry's radius.
pub fn curated_mesh_sample(hit: &CuratedHit) -> EmitterSample {
    EmitterSample {
        pos: hit.center,
        color: hit.entry.light,
        weight: (hit.entry.radius / RADIUS_SCALE).powi(2),
    }
}

/// Transform model-space samples into world space by `model`.
pub fn transform_samples(samples: &[EmitterSample], model: &Mat4) -> Vec<EmitterSample> {
    samples
        .iter()
        .map(|s| EmitterSample {
            pos: model.transform_point3(Vec3::from(s.pos)).to_array(),
            ..*s
        })
        .collect()
}

struct Cluster {
    pos_sum: Vec3,
    color_sum: Vec3,
    weight: f32,
    extent: f32,
}

/// Greedy weight-ordered clustering of world-space emitter samples into
/// at most [`MAX_SCENE_LIGHTS`] point lights (see the module docs).
pub fn cluster_scene_lights(samples: &[EmitterSample]) -> Vec<ScenePointLight> {
    let mut lights = cluster_all_scene_lights(samples);
    lights.truncate(MAX_SCENE_LIGHTS);
    lights
}

/// [`cluster_scene_lights`] without the count cap: every surviving
/// cluster, strongest first. A scene can carry dozens of candle props
/// while only [`MAX_SCENE_LIGHTS`] can shade at once, so a host that
/// knows the viewpoint keeps this full set and picks the nearest per
/// frame ([`nearest_lights`]).
pub fn cluster_all_scene_lights(samples: &[EmitterSample]) -> Vec<ScenePointLight> {
    let mut order: Vec<&EmitterSample> = samples.iter().collect();
    order.sort_by(|a, b| b.weight.total_cmp(&a.weight));
    let mut clusters: Vec<Cluster> = Vec::new();
    for s in order {
        let sp = Vec3::from(s.pos);
        let mut merged = false;
        for c in clusters.iter_mut() {
            let cpos = c.pos_sum / c.weight.max(1e-6);
            let d = cpos.distance(sp);
            if d < CLUSTER_MERGE_DIST {
                c.pos_sum += sp * s.weight;
                c.color_sum += Vec3::from(s.color) * s.weight;
                c.weight += s.weight;
                c.extent = c.extent.max(d);
                merged = true;
                break;
            }
        }
        if !merged {
            clusters.push(Cluster {
                pos_sum: sp * s.weight,
                color_sum: Vec3::from(s.color) * s.weight,
                weight: s.weight,
                extent: 0.0,
            });
        }
    }
    clusters.retain(|c| c.weight > 1e-3 && c.extent <= CLUSTER_MAX_EXTENT);
    clusters.sort_by(|a, b| b.weight.total_cmp(&a.weight));
    clusters
        .iter()
        .map(|c| {
            let w = c.weight.max(1e-6);
            let color = (c.color_sum / w).clamp(Vec3::ZERO, Vec3::ONE);
            ScenePointLight {
                pos: (c.pos_sum / w - Vec3::Y * LIGHT_LIFT).to_array(),
                color: color.to_array(),
                radius: (RADIUS_SCALE * c.weight.sqrt()).clamp(RADIUS_MIN, RADIUS_MAX),
            }
        })
        .collect()
}

/// One scene-actor prop's light set (a MAN-placed lamp, a glowing tree),
/// clustered in world space at the prop's spawn anchor. Props move - a
/// script walks them, hides them, re-spawns them - so the set is re-placed
/// every frame at the actor's live anchor ([`place_prop_lights`]).
#[derive(Debug, Clone, PartialEq)]
pub struct PropLights {
    /// The placement slot (the MAN placement index) the set belongs to.
    pub slot: u8,
    /// The spawn anchor `[x, floor y, z]` the lights were derived at.
    pub spawn: [f32; 3],
    /// The prop's lights, world space at `spawn`.
    pub lights: Vec<ScenePointLight>,
}

/// Every prop's light set moved to its live anchor. `live(slot, spawn_xz)`
/// is the host's actor position for the slot (the engine's
/// `World::field_npc_live_anchor` on both hosts); `None` (not drawn this
/// frame) drops the set.
pub fn place_prop_lights(
    props: &[PropLights],
    live: impl Fn(u8, (i16, i16)) -> Option<[f32; 3]>,
) -> Vec<ScenePointLight> {
    let mut out = Vec::new();
    for p in props {
        let Some(at) = live(p.slot, (p.spawn[0] as i16, p.spawn[2] as i16)) else {
            continue;
        };
        let d = Vec3::from(at) - Vec3::from(p.spawn);
        out.extend(p.lights.iter().map(|l| ScenePointLight {
            pos: (Vec3::from(l.pos) + d).to_array(),
            ..*l
        }));
    }
    out
}

/// The up-to-[`MAX_SCENE_LIGHTS`] lights nearest `focus` (typically the
/// player), nearest first - the per-frame selection over
/// [`cluster_all_scene_lights`]'s full set. Distance is measured to the
/// light's influence sphere (`dist - radius`), so a big nearby light
/// never loses its slot to a tiny slightly-closer one.
pub fn nearest_lights(lights: &[ScenePointLight], focus: [f32; 3]) -> Vec<ScenePointLight> {
    let f = Vec3::from(focus);
    let mut sorted: Vec<ScenePointLight> = lights.to_vec();
    sorted.sort_by(|a, b| {
        let da = Vec3::from(a.pos).distance(f) - a.radius;
        let db = Vec3::from(b.pos).distance(f) - b.radius;
        da.total_cmp(&db)
    });
    sorted.truncate(MAX_SCENE_LIGHTS);
    sorted
}

/// CPU mirror of the shaders' point-light attenuation:
/// `att = (1 - (d/r)^2)^2`, clamped - 1.0 at the light, exactly 0.0 at
/// the radius, smooth in between.
pub fn point_attenuation(dist: f32, radius: f32) -> f32 {
    if radius <= 0.0 || dist >= radius {
        return 0.0;
    }
    let x = (1.0 - (dist * dist) / (radius * radius)).clamp(0.0, 1.0);
    x * x
}

/// CPU mirror of the shaders' point-light gain *analytic* part (no shadow
/// term - the PCF lives on the GPU): per-channel gain added on top of the
/// global gain. `scale` is the mood's [`LightingMood::point_scale`].
pub fn point_gain(
    world_pos: [f32; 3],
    normal: [f32; 3],
    lights: &[ScenePointLight],
    scale: f32,
) -> [f32; 3] {
    let p = Vec3::from(world_pos);
    let n = Vec3::from(normal);
    let n_len = n.length();
    let mut gain = Vec3::ZERO;
    for l in lights.iter().take(MAX_SCENE_LIGHTS) {
        let to_l = Vec3::from(l.pos) - p;
        let dist = to_l.length();
        let att = point_attenuation(dist, l.radius);
        if att <= 0.0 {
            continue;
        }
        let lam = if n_len > 1e-6 && dist > 1e-3 {
            // Half-Lambert wrap: a point light grazing a wall still lifts it,
            // matching the soft pools of light the enhancement aims for.
            (n.dot(to_l) / (n_len * dist)).abs() * 0.5 + 0.5
        } else {
            LAMBERT_FALLBACK
        };
        gain += Vec3::from(l.color) * att * lam;
    }
    (gain * scale).to_array()
}

// ---------------------------------------------------------------------------
// Mood: ambient / key light / time of day
// ---------------------------------------------------------------------------

/// Weight of the orientation (`|N.L|`) term. Shader twins: `DYN_DIFFUSE`.
pub const DIFFUSE: f32 = 0.55;
/// Gain ceiling of the global (ambient + key + pool) term relative to the
/// baked colour. Shader twins: `DYN_MAX_GAIN`.
pub const MAX_GAIN: f32 = 1.3;
/// Gain ceiling with the point-light layer added on top - just below the
/// PSX modulation maximum of 255/128. Shader twins: `DYN_TOTAL_MAX_GAIN`.
pub const TOTAL_MAX_GAIN: f32 = 1.9;
/// Orientation term used when no normal is available. Shader twins:
/// `DYN_LAMBERT_FALLBACK`.
pub const LAMBERT_FALLBACK: f32 = 0.6;
/// Pool centre in 0..1 screen fractions (x, y). Shader twins:
/// `DYN_POOL_CENTER`.
pub const POOL_CENTER: [f32; 2] = [0.5, 0.45];
/// Radius (screen fraction) inside which the pool is full-strength.
pub const POOL_INNER: f32 = 0.15;
/// Radius (screen fraction) beyond which the pool has fully faded.
pub const POOL_OUTER: f32 = 0.75;

/// The lighting conditions one frame is shaded under. Every field is a
/// staged uniform on both hosts (see [`LightingMood::uniforms`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LightingMood {
    /// Stable name (HUD / page label, logs).
    pub name: &'static str,
    /// Key light direction (retail Y-down field frame; "from above" is -Y).
    pub key_dir: [f32; 3],
    /// Key light colour x strength (the `|N.L|` and pool terms' tint).
    pub key_rgb: [f32; 3],
    /// Ambient floor gain - what a surface the key light misses gets.
    /// Below 1.0 darkens the baked colour (night, caves).
    pub ambient_rgb: [f32; 3],
    /// Weight of the screen-centred light pool.
    pub pool: f32,
    /// Multiplier on every point light's colour (lamps read stronger at
    /// night than at noon).
    pub point_scale: f32,
    /// Gain an emissive fragment draws at, independent of the mood.
    pub emissive_gain: f32,
    /// Strength of the glow sprites (halos + shafts) 0..=1; 0 draws none.
    pub glow: f32,
}

impl LightingMood {
    /// Daylight: a warm sun over the baked shading, close to the retail
    /// brightness on average, lamps barely visible.
    pub const DAY: Self = Self {
        name: "day",
        key_dir: [0.32, -0.89, 0.31],
        key_rgb: [0.45, 0.42, 0.36],
        ambient_rgb: [0.70, 0.70, 0.70],
        pool: 0.25,
        point_scale: 0.35,
        emissive_gain: 1.15,
        glow: 0.25,
    };
    /// Dusk: low amber sun, cooling shadows, lamps coming on.
    pub const DUSK: Self = Self {
        name: "dusk",
        key_dir: [0.80, -0.45, 0.40],
        key_rgb: [0.70, 0.45, 0.28],
        ambient_rgb: [0.40, 0.38, 0.48],
        pool: 0.20,
        point_scale: 0.85,
        emissive_gain: 1.30,
        glow: 0.65,
    };
    /// Night: moonlit blue ambient, every lamp and window carries the scene.
    pub const NIGHT: Self = Self {
        name: "night",
        key_dir: [-0.35, -0.85, 0.40],
        key_rgb: [0.34, 0.42, 0.66],
        ambient_rgb: [0.25, 0.29, 0.47],
        pool: 0.18,
        point_scale: 1.35,
        emissive_gain: 1.45,
        glow: 1.0,
    };
    /// Enclosed spaces (caves, dungeons, interiors): dim neutral ambient,
    /// the authored lamps and torches doing the lighting.
    pub const CAVE: Self = Self {
        name: "cave",
        key_dir: [0.20, -0.95, 0.20],
        key_rgb: [0.55, 0.52, 0.50],
        ambient_rgb: [0.42, 0.40, 0.40],
        pool: 0.30,
        point_scale: 1.15,
        emissive_gain: 1.35,
        glow: 0.8,
    };

    /// The mood a scene is lit under when the player has not picked a time
    /// of day ([`TimeOfDay::Auto`]): a curated table of the enclosed scenes
    /// (caves, towers, dungeons) falls back to daylight for everything else.
    pub fn for_scene(scene: &str) -> Self {
        let s = scene.to_ascii_lowercase();
        if ENCLOSED_SCENE_PREFIXES.iter().any(|p| s.starts_with(p)) {
            Self::CAVE
        } else {
            Self::DAY
        }
    }

    /// The four shader uniforms the mood stages, shared layout on both
    /// hosts: `light_dir = (key_dir, enable)`, `light_color = (key_rgb,
    /// pool)`, `light_ambient = (ambient_rgb, emissive_gain)`. `enable` is
    /// 1.0 for the enhancement on and 0.0 off (the identity).
    pub fn uniforms(&self, enable: bool) -> [[f32; 4]; 3] {
        let d = Vec3::from(self.key_dir).normalize_or_zero();
        [
            [d.x, d.y, d.z, if enable { 1.0 } else { 0.0 }],
            [self.key_rgb[0], self.key_rgb[1], self.key_rgb[2], self.pool],
            [
                self.ambient_rgb[0],
                self.ambient_rgb[1],
                self.ambient_rgb[2],
                self.emissive_gain,
            ],
        ]
    }

    /// The mood as a small JSON object (the browser play page's staging).
    pub fn to_json(&self) -> String {
        let [d, c, a] = self.uniforms(true);
        format!(
            "{{\"name\":\"{}\",\"dir\":[{},{},{}],\"key\":[{},{},{},{}],\"ambient\":[{},{},{},{}],\"point_scale\":{},\"glow\":{}}}",
            self.name,
            d[0],
            d[1],
            d[2],
            c[0],
            c[1],
            c[2],
            c[3],
            a[0],
            a[1],
            a[2],
            a[3],
            self.point_scale,
            self.glow
        )
    }
}

/// CDNAME prefixes lit as enclosed spaces under [`TimeOfDay::Auto`]. The
/// labels are the disc's own (`docs/reference/scene-names.md`).
pub const ENCLOSED_SCENE_PREFIXES: &[&str] = &[
    "cave",     // Snowdrift Cave
    "jiji",     // Ancient Wind Cave
    "tunnel",   // Ancient Path / Fire Path
    "retockin", // Mt. Letona interior
    "chitei",   // Jette's Fortress transport system
    "korb",     // Sol Tower underground
    "koin",     // Sol Tower rooms
    "jouin",    // Bio Castle interiors
    "juui",     // Bio Castle
    "garmel",   // Zeto's Dungeon
    "stone",    // Shadow Gate
    "dohaty",   // Dohati's Castle
    "jagaroom", // Juggernaut Room
    "rugi",     // Rogue's Tower
    "deroa",    // Jette's Fortress
];

/// The player-facing time-of-day knob over the mood: `Auto` follows the
/// scene ([`LightingMood::for_scene`]), the rest force one preset. Cycled
/// at runtime on both hosts (`O` in `play-window`, the page's selector).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TimeOfDay {
    #[default]
    Auto,
    Day,
    Dusk,
    Night,
}

impl TimeOfDay {
    /// Every value, in cycle order.
    pub const ALL: [Self; 4] = [Self::Auto, Self::Day, Self::Dusk, Self::Night];

    /// The next value in cycle order (wraps).
    pub fn next(self) -> Self {
        let i = Self::ALL.iter().position(|&t| t == self).unwrap_or(0);
        Self::ALL[(i + 1) % Self::ALL.len()]
    }

    /// Stable lower-case name (`auto` / `day` / `dusk` / `night`).
    pub fn name(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Day => "day",
            Self::Dusk => "dusk",
            Self::Night => "night",
        }
    }

    /// Parse [`Self::name`] (case-insensitive).
    pub fn from_name(s: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|t| t.name().eq_ignore_ascii_case(s.trim()))
    }

    /// The mood this setting lights `scene` under.
    pub fn mood(self, scene: &str) -> LightingMood {
        match self {
            Self::Auto => LightingMood::for_scene(scene),
            Self::Day => LightingMood::DAY,
            Self::Dusk => LightingMood::DUSK,
            Self::Night => LightingMood::NIGHT,
        }
    }
}

/// CPU mirror of the per-fragment shading law (`dyn_light` in both shader
/// twins).
///
/// ```text
/// base  = ambient + (DIFFUSE * |N.L| + pool_w * pool(frag)) * key
/// gain  = min(min(base, MAX_GAIN) + point_gain, TOTAL_MAX_GAIN)
/// lit   = rgb * gain
/// out   = emissive ? rgb * min(emissive_gain + point_gain, TOTAL_MAX_GAIN)
///                  : lit                                (saturating)
/// ```
///
/// `uniforms` is [`LightingMood::uniforms`]; with its enable at zero the
/// function is the exact identity (the faithful path).
#[allow(clippy::too_many_arguments)] // mirrors the shader twins 1:1
pub fn shade(
    rgb: [f32; 3],
    normal: [f32; 3],
    frag_px: [f32; 2],
    viewport: [f32; 2],
    uniforms: [[f32; 4]; 3],
    point_gain: [f32; 3],
    emissive: bool,
) -> [f32; 3] {
    let [dir, key, amb] = uniforms;
    if dir[3] < 0.5 {
        return rgb;
    }
    if emissive {
        let mut out = [0.0f32; 3];
        for i in 0..3 {
            out[i] = (rgb[i] * (amb[3] + point_gain[i]).min(TOTAL_MAX_GAIN)).clamp(0.0, 1.0);
        }
        return out;
    }
    let mut lambert = LAMBERT_FALLBACK;
    let n = Vec3::from(normal);
    let l = Vec3::new(dir[0], dir[1], dir[2]);
    if n.length() > 1e-6 && l.length() > 0.0 {
        lambert = n.normalize().dot(l.normalize()).abs();
    }
    let pool = pool_factor(frag_px, viewport);
    let mut out = [0.0f32; 3];
    for i in 0..3 {
        let base = amb[i] + (DIFFUSE * lambert + key[3] * pool) * key[i];
        let gain = (base.min(MAX_GAIN) + point_gain[i]).min(TOTAL_MAX_GAIN);
        out[i] = (rgb[i] * gain).clamp(0.0, 1.0);
    }
    out
}

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// The screen-space light-pool factor for a fragment at `frag_px` on a
/// `viewport`-sized surface: 1.0 at [`POOL_CENTER`], fading to 0.0 past
/// [`POOL_OUTER`]. Returns 0.0 for a degenerate viewport.
pub fn pool_factor(frag_px: [f32; 2], viewport: [f32; 2]) -> f32 {
    if viewport[0] <= 0.0 || viewport[1] <= 0.0 {
        return 0.0;
    }
    let dx = frag_px[0] / viewport[0] - POOL_CENTER[0];
    let dy = frag_px[1] / viewport[1] - POOL_CENTER[1];
    let d = (dx * dx + dy * dy).sqrt();
    1.0 - smoothstep(POOL_INNER, POOL_OUTER, d)
}

// ---------------------------------------------------------------------------
// Glow sprites (bloom stand-in) and light shafts
// ---------------------------------------------------------------------------

/// What a [`GlowSprite`] draws as.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GlowKind {
    /// A camera-facing round halo around the light (the bloom stand-in).
    Halo,
    /// A vertical soft shaft from the light down toward the floor,
    /// camera-facing about the vertical axis.
    Shaft,
}

/// One additive glow sprite in world space. Both hosts draw the same list
/// with the same falloff: radial `(1 - r^2)^2` for a halo, a horizontal
/// `(1 - x^2)^2` times a linear vertical fade for a shaft.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GlowSprite {
    pub kind: GlowKind,
    /// Centre of a halo; the top of a shaft (retail Y-down frame).
    pub pos: [f32; 3],
    /// Halo radius / shaft half-width, world units.
    pub size: f32,
    /// Shaft length (downward, +Y); 0 for a halo.
    pub length: f32,
    /// Additive colour at the brightest point (premultiplied by strength).
    pub color: [f32; 3],
}

/// Halo radius as a fraction of the light's influence radius.
pub const HALO_RADIUS_FRAC: f32 = 0.22;
/// Halo peak intensity at full mood glow.
pub const HALO_INTENSITY: f32 = 0.55;
/// Shaft half-width as a fraction of the light's radius.
pub const SHAFT_WIDTH_FRAC: f32 = 0.10;
/// Shaft length as a fraction of the light's radius.
pub const SHAFT_LENGTH_FRAC: f32 = 0.45;
/// Shaft peak intensity at full mood glow.
pub const SHAFT_INTENSITY: f32 = 0.16;

/// The glow sprites for a frame's picked lights under `mood`: one halo per
/// light plus a soft downward shaft. Empty when the mood's glow is zero.
pub fn glow_sprites(lights: &[ScenePointLight], mood: &LightingMood) -> Vec<GlowSprite> {
    let g = mood.glow.clamp(0.0, 1.0);
    if g <= 0.0 {
        return Vec::new();
    }
    let mut out = Vec::with_capacity(lights.len() * 2);
    for l in lights {
        let c = Vec3::from(l.color);
        out.push(GlowSprite {
            kind: GlowKind::Halo,
            pos: l.pos,
            size: l.radius * HALO_RADIUS_FRAC,
            length: 0.0,
            color: (c * HALO_INTENSITY * g).to_array(),
        });
        out.push(GlowSprite {
            kind: GlowKind::Shaft,
            pos: l.pos,
            size: l.radius * SHAFT_WIDTH_FRAC,
            length: l.radius * SHAFT_LENGTH_FRAC,
            color: (c * SHAFT_INTENSITY * g).to_array(),
        });
    }
    out
}

/// One glow-sprite vertex: world position, the falloff coordinate
/// (`uv` in -1..1 across a halo; `x` across / `y` down a shaft) and the
/// additive colour with the kind in `.w` (0 = halo, 1 = shaft).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GlowVertex {
    pub pos: [f32; 3],
    pub uv: [f32; 2],
    pub color: [f32; 4],
}

/// Expand glow sprites into camera-facing quads (two triangles each, six
/// vertices, no index buffer). `cam_right` / `cam_up` are the camera's
/// world-space basis vectors (unit length); a shaft keeps world vertical and
/// turns only about it to face the camera.
pub fn glow_vertices(
    sprites: &[GlowSprite],
    cam_right: [f32; 3],
    cam_up: [f32; 3],
) -> Vec<GlowVertex> {
    let r = Vec3::from(cam_right);
    let u = Vec3::from(cam_up);
    // Horizontal right vector for shafts: the camera right flattened.
    let flat_r = Vec3::new(r.x, 0.0, r.z).normalize_or(Vec3::X);
    let mut out = Vec::with_capacity(sprites.len() * 6);
    for s in sprites {
        let p = Vec3::from(s.pos);
        let (corners, uvs, kind) = match s.kind {
            GlowKind::Halo => {
                let rr = r * s.size;
                let uu = u * s.size;
                (
                    [p - rr - uu, p + rr - uu, p + rr + uu, p - rr + uu],
                    [[-1.0, -1.0], [1.0, -1.0], [1.0, 1.0], [-1.0, 1.0]],
                    0.0,
                )
            }
            GlowKind::Shaft => {
                let rr = flat_r * s.size;
                let down = Vec3::Y * s.length;
                (
                    [p - rr, p + rr, p + rr + down, p - rr + down],
                    [[-1.0, 0.0], [1.0, 0.0], [1.0, 1.0], [-1.0, 1.0]],
                    1.0,
                )
            }
        };
        let color = [s.color[0], s.color[1], s.color[2], kind];
        for &i in &[0usize, 1, 2, 0, 2, 3] {
            out.push(GlowVertex {
                pos: corners[i].to_array(),
                uv: uvs[i],
                color,
            });
        }
    }
    out
}

/// One frame's lighting for a host that draws in its own process space (the
/// browser play page): the [`nearest_lights`] to `focus` with the mood's
/// [`LightingMood::point_scale`] folded into their colours, followed by the
/// glow-sprite vertices expanded against the camera basis - the exact pair
/// the native renderer derives from `set_scene_lights` + `set_glow_sprites`.
///
/// Layout (all `f32`, retail Y-down frame): `[n_lights, n_glow_vertices]`,
/// then `n_lights x [x, y, z, radius, r, g, b]`, then `n_glow_vertices x
/// [x, y, z, u, v, r, g, b, kind]`.
pub fn frame_packet(
    lights: &[ScenePointLight],
    focus: [f32; 3],
    mood: &LightingMood,
    cam_right: [f32; 3],
    cam_up: [f32; 3],
) -> Vec<f32> {
    let picked = nearest_lights(lights, focus);
    let verts = glow_vertices(&glow_sprites(&picked, mood), cam_right, cam_up);
    let mut out = Vec::with_capacity(2 + picked.len() * 7 + verts.len() * 9);
    out.push(picked.len() as f32);
    out.push(verts.len() as f32);
    for l in &picked {
        out.extend_from_slice(&l.pos);
        out.push(l.radius);
        out.extend(l.color.iter().map(|c| c * mood.point_scale));
    }
    for v in &verts {
        out.extend_from_slice(&v.pos);
        out.extend_from_slice(&v.uv);
        out.extend_from_slice(&v.color);
    }
    out
}

/// CPU mirror of the glow sprites' fragment falloff (both shader twins).
pub fn glow_falloff(uv: [f32; 2], kind_shaft: bool) -> f32 {
    if kind_shaft {
        let x = (1.0 - uv[0] * uv[0]).clamp(0.0, 1.0);
        x * x * (1.0 - uv[1]).clamp(0.0, 1.0)
    } else {
        let r2 = uv[0] * uv[0] + uv[1] * uv[1];
        let x = (1.0 - r2).clamp(0.0, 1.0);
        x * x
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flame_quad(center: [f32; 3], half: f32) -> ([[f32; 3]; 4], [u32; 6]) {
        let [x, y, z] = center;
        (
            [
                [x - half, y - half, z],
                [x + half, y - half, z],
                [x - half, y + half, z],
                [x + half, y + half, z],
            ],
            [0, 1, 2, 1, 3, 2],
        )
    }

    const ADDITIVE: u16 = SEMI_BIT | (1 << 5);

    /// An additive (ABR 1) semi prim is an emitter even at neutral
    /// modulation colour; an opaque prim never is.
    #[test]
    fn additive_prims_are_emitters() {
        let (pos, idx) = flame_quad([100.0, -80.0, 40.0], 16.0);
        let cba_tsb = [[0u16, ADDITIVE]; 4];
        let colors = [[0x80u8; 3]; 4];
        let got = vram_mesh_emitters(&pos, &cba_tsb, &colors, &idx, None);
        assert_eq!(got.len(), 2, "both triangles of the flame quad");
        let plain = [[0u16, 0u16]; 4];
        assert!(vram_mesh_emitters(&pos, &plain, &colors, &idx, None).is_empty());
    }

    /// A bright-warm ABE prim (mode 0) counts; a blue one (water) doesn't.
    #[test]
    fn bright_warm_gate() {
        let (pos, idx) = flame_quad([0.0, 0.0, 0.0], 16.0);
        let cba_tsb = [[0u16, SEMI_BIT]; 4];
        let warm = [[0xE0u8, 0xB0, 0x60]; 4];
        assert_eq!(
            vram_mesh_emitters(&pos, &cba_tsb, &warm, &idx, None).len(),
            2
        );
        let blue = [[0x40u8, 0x80, 0xE0]; 4];
        assert!(vram_mesh_emitters(&pos, &cba_tsb, &blue, &idx, None).is_empty());
    }

    /// Big additive sheets (water) are rejected by the area cap.
    #[test]
    fn big_sheets_are_rejected() {
        let (pos, idx) = flame_quad([0.0, 0.0, 0.0], 500.0);
        let cba_tsb = [[0u16, ADDITIVE]; 4];
        let colors = [[0xFFu8, 0xC0, 0x60]; 4];
        assert!(vram_mesh_emitters(&pos, &cba_tsb, &colors, &idx, None).is_empty());
    }

    /// Colour-mesh emitters key off the packed blend word + warm colour.
    #[test]
    fn color_mesh_emitters_gate_on_abe() {
        let (pos, idx) = flame_quad([0.0, 0.0, 0.0], 16.0);
        let warm = [[0xF0u8, 0xC0, 0x60]; 4];
        let on = [SEMI_BIT; 4];
        let off = [0u16; 4];
        assert_eq!(color_mesh_emitters(&pos, &warm, &on, &idx).len(), 2);
        assert!(color_mesh_emitters(&pos, &warm, &off, &idx).is_empty());
    }

    /// The emissive tag sets bit 13 on exactly the glowing prims' corners
    /// and leaves every other bit of the word alone.
    #[test]
    fn tagging_sets_only_the_emissive_bit() {
        let (_, idx) = flame_quad([0.0; 3], 8.0);
        let pos = [[0.0f32; 3]; 4];
        let uvs = [[0u8; 2]; 4];
        let mut ct = [[0x1234u16, ADDITIVE | 0x0105]; 4];
        let colors = [[0x80u8; 3]; 4];
        assert_eq!(
            tag_emissive_vram(&pos, &mut ct, &colors, &uvs, &idx, None).0,
            2
        );
        for c in ct {
            assert_eq!(c[0], 0x1234, "CBA untouched");
            assert_eq!(c[1], ADDITIVE | 0x0105 | EMISSIVE_BIT);
        }
        let mut plain = [[0u16, 0x0105u16]; 4];
        assert_eq!(
            tag_emissive_vram(&pos, &mut plain, &colors, &uvs, &idx, None).0,
            0
        );
        assert!(plain.iter().all(|c| c[1] == 0x0105));
        // A curated whole-mesh entry tags everything, and reports where.
        let e = EmissiveMesh {
            signature: 0,
            label: "test",
            texels: CuratedTexels::All,
            light: [1.0; 3],
            radius: 800.0,
        };
        let vram = legaia_tim::Vram::new();
        let (n, center) =
            tag_emissive_vram(&pos, &mut plain, &colors, &uvs, &idx, Some((&e, &vram)));
        assert_eq!(n, 2);
        assert_eq!(center, Some([0.0; 3]));
        assert!(plain.iter().all(|c| c[1] & EMISSIVE_BIT != 0));
    }

    /// The emissive bit collides with no bit a retail TSB, a blend word or
    /// the engine's own packings use.
    #[test]
    fn emissive_bit_is_free() {
        const TSB_USED: u16 = 0x01FF; // page x/y, ABR, depth
        const DOUBLE_SIDED_BLEND: u16 = 0x4000;
        assert_eq!(EMISSIVE_BIT & (TSB_USED | SEMI_BIT | DOUBLE_SIDED_BLEND), 0);
    }

    /// Hybrid tagging reads an untextured vertex's fill colour from the
    /// flat side channel, not its neutral modulation word.
    #[test]
    fn hybrid_tagging_reads_the_fill() {
        let (pos, idx) = flame_quad([0.0; 3], 8.0);
        let mesh = |cba_tsb: [u16; 2]| legaia_tmd::mesh::VramMesh {
            positions: pos.to_vec(),
            uvs: vec![[0, 0]; 4],
            cba_tsb: vec![cba_tsb; 4],
            indices: idx.to_vec(),
            normals: vec![[0.0; 3]; 4],
            colors: vec![[0x80; 3]; 4],
        };
        let vram = legaia_tim::Vram::new();
        let mut warm_mesh = mesh([0, SEMI_BIT]);
        let flat: Vec<u8> = (0..4).flat_map(|_| [0xF0, 0xC0, 0x60, 0]).collect();
        assert!(tag_emissive_hybrid(&[], &mut warm_mesh, &flat, &vram).is_none());
        assert!(warm_mesh.cba_tsb.iter().all(|c| c[1] & EMISSIVE_BIT != 0));
        let mut dark_mesh = mesh([0, SEMI_BIT]);
        let dark: Vec<u8> = (0..4).flat_map(|_| [0x30, 0x30, 0x30, 0]).collect();
        tag_emissive_hybrid(&[], &mut dark_mesh, &dark, &vram);
        assert!(dark_mesh.cba_tsb.iter().all(|c| c[1] & EMISSIVE_BIT == 0));
    }

    /// The curated table's keys are unique and every entry has a light.
    #[test]
    fn curated_table_is_well_formed() {
        for (i, a) in EMISSIVE_MESHES.iter().enumerate() {
            assert!(
                a.radius > 0.0 && a.light.iter().any(|&c| c > 0.0),
                "{}",
                a.label
            );
            for b in &EMISSIVE_MESHES[i + 1..] {
                assert_ne!(a.signature, b.signature);
            }
        }
        assert_eq!(tmd_signature(b""), SIGNATURE_BASIS);
    }

    /// Nearby samples merge into one light; the cap keeps the strongest.
    #[test]
    fn clustering_merges_and_caps() {
        let mut samples = Vec::new();
        for dx in [0.0, 20.0] {
            samples.push(EmitterSample {
                pos: [100.0 + dx, -60.0, 100.0],
                color: [1.0, 0.8, 0.5],
                weight: 10.0,
            });
        }
        for i in 0..12 {
            samples.push(EmitterSample {
                pos: [5000.0 + 2000.0 * i as f32, 0.0, 0.0],
                color: [1.0, 1.0, 1.0],
                weight: 1.0,
            });
        }
        let lights = cluster_scene_lights(&samples);
        assert!(lights.len() <= MAX_SCENE_LIGHTS);
        let strongest = &lights[0];
        assert!((strongest.pos[0] - 110.0).abs() < 1.0, "{strongest:?}");
        assert!(strongest.radius >= RADIUS_MIN && strongest.radius <= RADIUS_MAX);
    }

    /// A cluster that spreads wider than a prop is dropped.
    #[test]
    fn wide_clusters_are_dropped() {
        let samples: Vec<EmitterSample> = (0..10)
            .map(|i| EmitterSample {
                pos: [i as f32 * 250.0, 0.0, 0.0],
                color: [1.0, 0.9, 0.6],
                weight: 100.0 - i as f32,
            })
            .collect();
        let lights = cluster_scene_lights(&samples);
        for l in &lights {
            assert!(l.radius <= RADIUS_MAX);
        }
    }

    /// Attenuation: 1 at the light, 0 at the radius, monotonic between.
    #[test]
    fn attenuation_shape() {
        assert!((point_attenuation(0.0, 1000.0) - 1.0).abs() < 1e-6);
        assert_eq!(point_attenuation(1000.0, 1000.0), 0.0);
        assert_eq!(point_attenuation(1500.0, 1000.0), 0.0);
        let mut last = 1.0;
        for i in 1..10 {
            let a = point_attenuation(i as f32 * 100.0, 1000.0);
            assert!(a < last, "attenuation must decrease");
            last = a;
        }
    }

    /// The point-gain mirror: straight under the light the wrap term is
    /// 1.0, so the gain is colour x attenuation x scale; out of radius zero.
    #[test]
    fn point_gain_mirror() {
        let l = ScenePointLight {
            pos: [0.0, -100.0, 0.0],
            color: [1.0, 0.8, 0.5],
            radius: 800.0,
        };
        let g = point_gain([0.0, 0.0, 0.0], [0.0, -1.0, 0.0], &[l], 2.0);
        let att = point_attenuation(100.0, 800.0);
        assert!((g[0] - 2.0 * att).abs() < 1e-5, "{g:?} vs att {att}");
        assert!((g[1] - 1.6 * att).abs() < 1e-5);
        let far = point_gain([5000.0, 0.0, 0.0], [0.0, -1.0, 0.0], &[l], 1.0);
        assert_eq!(far, [0.0; 3]);
    }

    /// The load-bearing invariant: enable off is the exact identity, even
    /// for an emissive fragment under a huge point gain.
    #[test]
    fn disabled_is_exact_identity() {
        let rgb = [0.123, 0.456, 0.789];
        let u = LightingMood::NIGHT.uniforms(false);
        for emissive in [false, true] {
            let out = shade(
                rgb,
                [0.0, -1.0, 0.0],
                [10.0, 10.0],
                [960.0, 720.0],
                u,
                [9.0; 3],
                emissive,
            );
            assert_eq!(out, rgb);
        }
    }

    /// Night darkens an unlit surface well below the baked colour while an
    /// emissive one stays at (or above) it - the whole point of the mood.
    #[test]
    fn night_darkens_surfaces_but_not_emissives() {
        let rgb = [0.5; 3];
        let u = LightingMood::NIGHT.uniforms(true);
        let wall = shade(
            rgb,
            [0.0, 0.0, 1.0],
            [0.0, 0.0],
            [960.0, 720.0],
            u,
            [0.0; 3],
            false,
        );
        let lamp = shade(
            rgb,
            [0.0, 0.0, 1.0],
            [0.0, 0.0],
            [960.0, 720.0],
            u,
            [0.0; 3],
            true,
        );
        assert!(wall.iter().all(|&c| c < 0.35), "{wall:?}");
        assert!(lamp.iter().all(|&c| c >= 0.5), "{lamp:?}");
        // A nearby lamp lifts the wall back toward its baked colour.
        let lit = shade(
            rgb,
            [0.0, 0.0, 1.0],
            [0.0, 0.0],
            [960.0, 720.0],
            u,
            [0.6; 3],
            false,
        );
        assert!(lit[0] > wall[0] + 0.2, "{lit:?} vs {wall:?}");
    }

    /// Daylight stays near the retail brightness on average (no wash-out):
    /// an up-facing fragment mid-pool lands within 30% of the baked colour.
    #[test]
    fn day_is_near_retail_brightness() {
        let rgb = [0.5; 3];
        let u = LightingMood::DAY.uniforms(true);
        let out = shade(
            rgb,
            [0.0, -1.0, 0.0],
            [480.0, 324.0],
            [960.0, 720.0],
            u,
            [0.0; 3],
            false,
        );
        for c in out {
            assert!((0.35..=0.65).contains(&c), "{out:?}");
        }
    }

    /// The gain never exceeds TOTAL_MAX_GAIN x baked, whatever the lights.
    #[test]
    fn gain_is_capped() {
        let rgb = [0.4; 3];
        for mood in [
            LightingMood::DAY,
            LightingMood::DUSK,
            LightingMood::NIGHT,
            LightingMood::CAVE,
        ] {
            let out = shade(
                rgb,
                [0.0, -1.0, 0.0],
                [480.0, 324.0],
                [960.0, 720.0],
                mood.uniforms(true),
                [100.0; 3],
                false,
            );
            for c in out {
                assert!(c <= 0.4 * TOTAL_MAX_GAIN + 1e-6, "{} {out:?}", mood.name);
            }
        }
    }

    #[test]
    fn time_of_day_cycles_and_parses() {
        let mut t = TimeOfDay::Auto;
        let mut seen = vec![t];
        for _ in 0..3 {
            t = t.next();
            seen.push(t);
        }
        assert_eq!(seen, TimeOfDay::ALL.to_vec());
        assert_eq!(t.next(), TimeOfDay::Auto);
        for t in TimeOfDay::ALL {
            assert_eq!(TimeOfDay::from_name(t.name()), Some(t));
        }
        assert_eq!(TimeOfDay::Auto.mood("cave01").name, "cave");
        assert_eq!(TimeOfDay::Auto.mood("town01").name, "day");
        assert_eq!(TimeOfDay::Night.mood("cave01").name, "night");
    }

    /// Halo falloff: 1 at the centre, 0 at the rim; shafts fade downward.
    #[test]
    fn glow_falloff_shape() {
        assert!((glow_falloff([0.0, 0.0], false) - 1.0).abs() < 1e-6);
        assert_eq!(glow_falloff([1.0, 0.0], false), 0.0);
        assert!(glow_falloff([0.0, 0.2], true) > glow_falloff([0.0, 0.8], true));
        assert_eq!(glow_falloff([0.0, 1.0], true), 0.0);
    }

    /// Two sprites per light, six vertices per sprite; no glow at zero.
    #[test]
    fn glow_geometry_counts() {
        let l = ScenePointLight {
            pos: [0.0, -200.0, 0.0],
            color: [1.0, 0.8, 0.5],
            radius: 800.0,
        };
        let s = glow_sprites(&[l, l], &LightingMood::NIGHT);
        assert_eq!(s.len(), 4);
        let v = glow_vertices(&s, [1.0, 0.0, 0.0], [0.0, -1.0, 0.0]);
        assert_eq!(v.len(), 24);
        let mut none = LightingMood::NIGHT;
        none.glow = 0.0;
        assert!(glow_sprites(&[l], &none).is_empty());
    }
}
