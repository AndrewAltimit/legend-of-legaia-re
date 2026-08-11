//! Monster model OBJ+PNG codec: the modder-facing export/import surface for
//! monster mesh replacement.
//!
//! **Export** renders a monster's embedded TMD + texture pool as an editable
//! pair: a Wavefront OBJ (one `o part_NN` block per TMD object, positions in
//! raw GTE units exactly like the glTF exporter, per-vertex baked colours as
//! OBJ extended `v x y z r g b`, half-texel-centred UVs) plus the 4bpp page
//! rendered to RGBA through each texel's *owning* primitive's palette.
//!
//! **Import** reverses it: OBJ + edited PNG back into `(TMD bytes, texture
//! pool bytes)` ready for [`crate::monster_archive::replace_mesh_and_pool`].
//! The pool is re-palettized from scratch: faces are greedily grouped into at
//! most 15 sixteen-colour CLUTs (slot 0 of every CLUT is the PSX transparent
//! texel, matching retail), each texel is indexed through the palette of the
//! first face that covers it, and each prim's CBA selects its group's CLUT.
//! Stored CBA/TSB use the retail pre-relocation convention pinned across the
//! archive: `cba = 0x7840 | palette_index` (row 481), `tsb = 0x25` (page x
//! 320, 4bpp, blend rate 1) - the battle loader relocates both per slot.
//!
//! A model that keeps the retail TMD's object count animates through the
//! retail keyframe streams untouched - that is the whole trick that makes
//! mesh replacement cheap: parts are posed rigidly by index, so new geometry
//! per part inherits every retail move.
//!
//! Alpha convention (both directions): `a = 0` transparent (palette slot 0),
//! `0 < a < 255` semi-transparent texel (STP bit set - blends only in ABE
//! groups), `a = 255` opaque; opaque black encodes as `0x8000` (STP-only) so
//! it doesn't collide with the transparent `0x0000`.

use anyhow::{Context, Result, bail};
use std::collections::BTreeMap;
use std::fmt::Write as _;

use crate::monster_archive::MonsterMesh;
use legaia_tmd::descriptor::PacketShape;
use legaia_tmd::encode::{
    LEGAIA_OBJECT_SCALE, ModelGroup, ModelObject, ModelPrim, decode_model, encode,
};

/// Retail pre-relocation CLUT base: `(x, y) = (palette*16, 481)`.
pub const CBA_BASE: u16 = 0x7840;
/// Retail pre-relocation texture page word: x 320, y 0, 4bpp, blend rate 1.
pub const TSB_BASE: u16 = 0x25;
/// CLUT rows in a monster pool.
pub const CLUT_COUNT: usize = 15;
/// Bytes per stored CLUT (16 BGR555 entries).
const CLUT_BYTES: usize = 32;
/// Byte length of the pool's CLUT region.
pub const CLUT_REGION_BYTES: usize = CLUT_COUNT * CLUT_BYTES;
/// Pool page height in rows (fixed - the loader's StoreImage RECT.h).
pub const PAGE_HEIGHT: usize = 256;
/// The UV address space of a 4bpp tpage (texels per axis). OBJ `vt`
/// normalization uses this, independent of the pool's stored width.
pub const UV_SPACE: usize = 256;

/// One exported model: OBJ + MTL text and the RGBA page render.
#[derive(Debug, Clone)]
pub struct ExportedModel {
    pub obj: String,
    pub mtl: String,
    /// RGBA8 page render, `page_width * PAGE_HEIGHT * 4` bytes.
    pub rgba: Vec<u8>,
    pub page_width: usize,
    pub page_height: usize,
    /// Per-texel: `true` if some primitive references this texel (rendered
    /// through its owner's palette). Unowned texels are dead bytes in the
    /// pool - rendered through CLUT 0 for context, discarded on import.
    pub owned: Vec<bool>,
}

/// One imported model, ready for the block rebuilder.
#[derive(Debug, Clone)]
pub struct ImportedModel {
    pub tmd: Vec<u8>,
    pub pool: Vec<u8>,
    /// Palette groups actually used (<= 15).
    pub palettes_used: usize,
    /// Non-fatal notes (colour quantization, unowned texels, ...).
    pub warnings: Vec<String>,
}

// ---------------------------------------------------------------------------
// BGR555 helpers (shared convention with `legaia_tim`).

fn rgba_to_bgr555(r: u8, g: u8, b: u8, a: u8) -> u16 {
    if a == 0 {
        return 0;
    }
    let v = ((r as u16) >> 3) | (((g as u16) >> 3) << 5) | (((b as u16) >> 3) << 10);
    if a < 255 {
        v | 0x8000
    } else if v == 0 {
        0x8000 // opaque black: STP-only, distinct from transparent 0x0000
    } else {
        v
    }
}

fn bgr555_to_rgba(v: u16) -> [u8; 4] {
    if v == 0 {
        return [0, 0, 0, 0];
    }
    let scale5 = |c: u16| -> u8 { ((c << 3) | (c >> 2)) as u8 };
    let r = scale5(v & 0x1F);
    let g = scale5((v >> 5) & 0x1F);
    let b = scale5((v >> 10) & 0x1F);
    let a = if v & 0x8000 != 0 && (v & 0x7FFF) != 0 {
        128
    } else {
        255
    };
    [r, g, b, a]
}

// ---------------------------------------------------------------------------
// UV-triangle rasterization (texel ownership).
//
// PSX texture sampling is nearest-texel: interpolated UVs across a face stay
// inside the UV triangle's hull up to rounding, so a strict barycentric pass
// with a small inclusive epsilon plus a 1-texel dilation covers every texel
// the GPU can sample for the face.

fn raster_triangle(
    w: usize,
    h: usize,
    uv: [(f32, f32); 3],
    eps: f32,
    mut mark: impl FnMut(usize, usize),
) {
    let (min_x, max_x) = {
        let xs = [uv[0].0, uv[1].0, uv[2].0];
        let lo = xs.iter().cloned().fold(f32::INFINITY, f32::min);
        let hi = xs.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
        (
            (lo.floor() as i64 - 1).max(0) as usize,
            (hi.ceil() as i64 + 1).clamp(0, w as i64 - 1) as usize,
        )
    };
    let (min_y, max_y) = {
        let ys = [uv[0].1, uv[1].1, uv[2].1];
        let lo = ys.iter().cloned().fold(f32::INFINITY, f32::min);
        let hi = ys.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
        (
            (lo.floor() as i64 - 1).max(0) as usize,
            (hi.ceil() as i64 + 1).clamp(0, h as i64 - 1) as usize,
        )
    };
    if min_x > max_x || min_y > max_y {
        return;
    }
    let edge = |a: (f32, f32), b: (f32, f32), p: (f32, f32)| -> f32 {
        (b.0 - a.0) * (p.1 - a.1) - (b.1 - a.1) * (p.0 - a.0)
    };
    let area = edge(uv[0], uv[1], uv[2]);
    // Distance from point to segment, for the degenerate (zero-area) case.
    let seg_dist = |a: (f32, f32), b: (f32, f32), p: (f32, f32)| -> f32 {
        let (dx, dy) = (b.0 - a.0, b.1 - a.1);
        let len2 = dx * dx + dy * dy;
        let t = if len2 <= f32::EPSILON {
            0.0
        } else {
            (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / len2).clamp(0.0, 1.0)
        };
        let (cx, cy) = (a.0 + t * dx - p.0, a.1 + t * dy - p.1);
        (cx * cx + cy * cy).sqrt()
    };
    for y in min_y..=max_y {
        for x in min_x..=max_x {
            let p = (x as f32 + 0.5, y as f32 + 0.5);
            let (w0, w1, w2) = (
                edge(uv[1], uv[2], p),
                edge(uv[2], uv[0], p),
                edge(uv[0], uv[1], p),
            );
            let inside = if area.abs() < 1e-3 {
                // Degenerate (line/point) face: the GPU only samples along
                // the collapsed edge - accept texels near the segments, NOT
                // the whole bounding box (which over-claims ownership and
                // floods the face's colour set). Even a strict pass keeps
                // the half-texel band the line actually crosses.
                seg_dist(uv[0], uv[1], p)
                    .min(seg_dist(uv[1], uv[2], p))
                    .min(seg_dist(uv[2], uv[0], p))
                    <= eps.max(0.55)
            } else if area > 0.0 {
                w0 >= -eps * area.sqrt().max(1.0)
                    && w1 >= -eps * area.sqrt().max(1.0)
                    && w2 >= -eps * area.sqrt().max(1.0)
            } else {
                w0 <= eps * (-area).sqrt().max(1.0)
                    && w1 <= eps * (-area).sqrt().max(1.0)
                    && w2 <= eps * (-area).sqrt().max(1.0)
            };
            if inside {
                mark(x, y);
            }
        }
    }
}

/// Strict raster epsilon: barely-inclusive edges, no dilation. What the
/// GPU's interpolated UVs actually stay within (plus float slack).
const EPS_STRICT: f32 = 0.05;
/// Generous raster epsilon: ~half a texel of dilation, covering nearest-texel
/// rounding at prim edges.
const EPS_DILATED: f32 = 0.75;

/// Rasterize one prim's UV coverage (tri, or quad as two Z-order tris).
fn raster_prim(w: usize, h: usize, uvs: &[(u8, u8)], eps: f32, mut mark: impl FnMut(usize, usize)) {
    let c = |i: usize| (uvs[i].0 as f32 + 0.5, uvs[i].1 as f32 + 0.5);
    match uvs.len() {
        3 => raster_triangle(w, h, [c(0), c(1), c(2)], eps, &mut mark),
        4 => {
            raster_triangle(w, h, [c(0), c(1), c(2)], eps, &mut mark);
            raster_triangle(w, h, [c(1), c(3), c(2)], eps, &mut mark);
        }
        _ => {}
    }
}

/// Canonical texel ownership: which palette a texel reads/writes through.
///
/// A texel referenced by several prims is stored once but rendered through
/// each prim's palette; the codec must pick ONE palette per texel, and the
/// pick must be **independent of face iteration order** - the importer
/// re-buckets faces into groups, so an order-dependent rule (first writer)
/// would resolve contested seam texels differently on each side of the
/// round trip. Claim tiers: a *corner* claim (the GPU is guaranteed to
/// sample it) beats a *strict-interior* claim, which beats a *dilated-ring*
/// claim (edge-rounding slack that must never steal a neighbouring island's
/// interior texels); within a tier the lowest palette id wins.
pub fn compute_owners(faces: &[(Vec<(u8, u8)>, u16)], width: usize) -> Vec<Option<u16>> {
    // claim key: (tier, palette) - lower wins.
    let mut best: Vec<Option<(u8, u16)>> = vec![None; width * PAGE_HEIGHT];
    let claim = |x: usize, y: usize, key: (u8, u16), best: &mut Vec<Option<(u8, u16)>>| {
        if x >= width {
            return;
        }
        let slot = &mut best[y * width + x];
        if slot.is_none_or(|cur| key < cur) {
            *slot = Some(key);
        }
    };
    for (uvs, pal) in faces {
        for &(u, v) in uvs {
            claim(u as usize, v as usize, (0, *pal), &mut best);
        }
        raster_prim(width, PAGE_HEIGHT, uvs, EPS_STRICT, |x, y| {
            claim(x, y, (1, *pal), &mut best);
        });
        raster_prim(width, PAGE_HEIGHT, uvs, EPS_DILATED, |x, y| {
            claim(x, y, (2, *pal), &mut best);
        });
    }
    best.into_iter().map(|b| b.map(|(_, p)| p)).collect()
}

// ---------------------------------------------------------------------------
// Export.

/// Decode the pool's raw CLUTs (BGR555) + 4bpp indices.
fn decode_pool(pool: &[u8]) -> Option<(Vec<[u16; 16]>, Vec<u8>, usize)> {
    if pool.len() <= CLUT_REGION_BYTES {
        return None;
    }
    let cluts: Vec<[u16; 16]> = (0..CLUT_COUNT)
        .map(|c| {
            let mut pal = [0u16; 16];
            for (i, slot) in pal.iter_mut().enumerate() {
                *slot = legaia_bytes::u16_le(pool, c * CLUT_BYTES + i * 2).unwrap_or(0);
            }
            pal
        })
        .collect();
    let pixels = &pool[CLUT_REGION_BYTES..];
    let bytes_per_row = pixels.len() / PAGE_HEIGHT;
    if bytes_per_row == 0 {
        return None;
    }
    let width = bytes_per_row * 2;
    let mut indices = vec![0u8; width * PAGE_HEIGHT];
    for y in 0..PAGE_HEIGHT {
        for xb in 0..bytes_per_row {
            let b = pixels[y * bytes_per_row + xb];
            indices[y * width + xb * 2] = b & 0x0F;
            indices[y * width + xb * 2 + 1] = b >> 4;
        }
    }
    Some((cluts, indices, width))
}

/// Export a monster's mesh + pool as OBJ / MTL / RGBA page.
///
/// `stem` names the sibling files inside the OBJ/MTL text (`stem.mtl`,
/// `stem.png`).
pub fn export_obj(mesh: &MonsterMesh, stem: &str) -> Result<ExportedModel> {
    let pool = mesh
        .texture_pool_bytes()
        .ok_or_else(|| anyhow::anyhow!("monster has no texture pool"))?;
    export_obj_parts(mesh.tmd_bytes(), pool, stem)
}

/// [`export_obj`] from raw parts: TMD bytes + texture pool bytes. Also the
/// second half of the codec's fixed-point property - exporting an imported
/// model reproduces the OBJ geometry and the owned texels of the PNG.
pub fn export_obj_parts(tmd_bytes: &[u8], pool: &[u8], stem: &str) -> Result<ExportedModel> {
    let tmd = legaia_tmd::parse(tmd_bytes).context("monster TMD unparseable")?;
    let model = decode_model(&tmd, tmd_bytes)?;
    let (cluts, indices, width) =
        decode_pool(pool).ok_or_else(|| anyhow::anyhow!("texture pool too small"))?;

    // Texel ownership: which palette a texel renders through in the PNG.
    // Canonical order-free rule shared with the importer - see
    // [`compute_owners`]. UVs address the full 256-texel tpage; a narrow
    // (128-wide) pool only backs the left half, out-of-pool texels skip.
    let claim_faces: Vec<(Vec<(u8, u8)>, u16)> = model
        .iter()
        .flat_map(|o| &o.groups)
        .filter(|g| g.shape.is_textured())
        .flat_map(|g| &g.prims)
        .map(|p| (p.uvs.clone(), p.cba & 0x3F))
        .collect();
    let owner = compute_owners(&claim_faces, width);

    // Page render: owned texels through their palette, unowned through 0.
    let mut rgba = vec![0u8; width * PAGE_HEIGHT * 4];
    for (t, &idx) in indices.iter().enumerate() {
        let pal = owner[t].unwrap_or(0) as usize;
        let px = bgr555_to_rgba(cluts.get(pal).map_or(0, |c| c[idx as usize]));
        rgba[t * 4..t * 4 + 4].copy_from_slice(&px);
    }

    // OBJ text. Vertices are duplicated per distinct (position, colour) so
    // per-corner gouraud colours survive; UVs dedup globally.
    let mut obj = String::new();
    let _ = writeln!(
        obj,
        "# Legend of Legaia monster model (raw GTE units, y-down)"
    );
    let _ = writeln!(
        obj,
        "# {} parts; part order = TMD object order = animation index",
        model.len()
    );
    let _ = writeln!(obj, "mtllib {stem}.mtl");
    let mut vt_map: BTreeMap<(u8, u8), usize> = BTreeMap::new();
    let mut vt_lines = String::new();
    let mut vt_next = 1usize;
    // UVs are normalized against the full 256-texel tpage space, not the
    // pool width - retail narrow-pool meshes legally carry u == 128 (the
    // column just past the upload), and clamping it would move geometry.
    let mut vt_of = |uv: (u8, u8), vt_lines: &mut String| -> usize {
        *vt_map.entry(uv).or_insert_with(|| {
            let u = (uv.0 as f32 + 0.5) / UV_SPACE as f32;
            let v = 1.0 - (uv.1 as f32 + 0.5) / PAGE_HEIGHT as f32;
            let _ = writeln!(vt_lines, "vt {u:.6} {v:.6}");
            let i = vt_next;
            vt_next += 1;
            i
        })
    };

    struct FaceRef {
        part: usize,
        semi: bool,
        /// GPU blend rate `(tsb >> 5) & 3` - only meaningful on semi faces.
        abr: u8,
        /// CLUT index (`cba & 0x3F`) - carried into the material name so a
        /// re-import reproduces the palette partition exactly.
        palette: u8,
        textured: bool,
        v: Vec<usize>,  // global 1-based OBJ vertex ids
        vt: Vec<usize>, // global 1-based OBJ vt ids (textured only)
    }
    let mut v_lines = String::new();
    let mut v_next = 1usize;
    let mut faces: Vec<FaceRef> = Vec::new();
    for (oi, o) in model.iter().enumerate() {
        let mut v_map: BTreeMap<([i16; 3], [u8; 3]), usize> = BTreeMap::new();
        for g in &o.groups {
            for p in &g.prims {
                let n = p.vertices.len();
                // Corner colours: gouraud stores n, flat stores 1 (replicate).
                let corner_color = |k: usize| -> [u8; 3] {
                    if p.colors.len() == n {
                        p.colors[k]
                    } else {
                        p.colors[0]
                    }
                };
                // Stored Z-order -> perimeter order for OBJ.
                let order: &[usize] = if n == 4 { &[0, 1, 3, 2] } else { &[0, 1, 2] };
                let mut fv = Vec::with_capacity(n);
                let mut fvt = Vec::with_capacity(n);
                for &k in order {
                    let pos = o.vertices[p.vertices[k] as usize];
                    let col = corner_color(k);
                    let id = *v_map.entry((pos, col)).or_insert_with(|| {
                        let _ = writeln!(
                            v_lines,
                            "v {} {} {} {:.6} {:.6} {:.6}",
                            pos[0],
                            pos[1],
                            pos[2],
                            col[0] as f32 / 255.0,
                            col[1] as f32 / 255.0,
                            col[2] as f32 / 255.0
                        );
                        let i = v_next;
                        v_next += 1;
                        i
                    });
                    fv.push(id);
                    if g.shape.is_textured() {
                        fvt.push(vt_of(p.uvs[k], &mut vt_lines));
                    }
                }
                faces.push(FaceRef {
                    part: oi,
                    semi: g.semi_transparent,
                    abr: ((p.tsb >> 5) & 3) as u8,
                    palette: (p.cba & 0x3F) as u8,
                    textured: g.shape.is_textured(),
                    v: fv,
                    vt: fvt,
                });
            }
        }
    }
    obj.push_str(&v_lines);
    obj.push_str(&vt_lines);
    // Faces are emitted in the exact model walk order (part -> group ->
    // prim), switching `usemtl` state as the classification changes. The
    // OBJ face order IS the codec's texel-ownership order - the importer's
    // first-writer passes must see faces in the same sequence the exporter's
    // ownership passes did, or contested seam texels flip palettes.
    // Opaque faces ride `skin`; semi-transparent (ABE) faces ride
    // `skin_semi_abrN` (N = the GPU blend rate the group's TSB selects).
    let mut mtls_seen: std::collections::BTreeSet<String> = Default::default();
    let mut cur_part = usize::MAX;
    let mut cur_mtl = String::new();
    for f in &faces {
        if f.part != cur_part {
            let _ = writeln!(obj, "o part_{:02}", f.part);
            cur_part = f.part;
            cur_mtl.clear();
        }
        // Material name carries the full render state: `_semi_abrN` for the
        // ABE groups, `_pNN` for the CLUT the face reads through (so a
        // re-import reproduces the palette partition exactly - and a modder
        // can steer faces onto palettes deliberately).
        let mtl = if f.textured {
            if f.semi {
                format!("skin_semi_abr{}_p{:02}", f.abr, f.palette)
            } else {
                format!("skin_p{:02}", f.palette)
            }
        } else {
            "flat".to_string()
        };
        mtls_seen.insert(mtl.clone());
        if mtl != cur_mtl {
            let _ = writeln!(obj, "usemtl {mtl}");
            cur_mtl = mtl;
        }
        obj.push('f');
        for (k, &vid) in f.v.iter().enumerate() {
            if f.textured {
                let _ = write!(obj, " {}/{}", vid, f.vt[k]);
            } else {
                let _ = write!(obj, " {vid}");
            }
        }
        obj.push('\n');
    }

    let mut mtl = String::from(
        "# skin_pNN = opaque geometry reading CLUT NN; skin_semi_abrN_pNN =\n\
         # semi-transparent (ABE) geometry with GPU blend rate N (1 =\n\
         # additive, 0 = 50/50, 2 = subtractive, 3 = quarter-additive);\n\
         # flat = untextured vertex-coloured geometry.\n\
         # The _pNN suffix is a palette HINT - faces without one are\n\
         # palletized automatically on import.\n",
    );
    for name in mtls_seen {
        let semi = name.contains("_semi");
        let _ = write!(mtl, "\nnewmtl {name}\n");
        if semi {
            let _ = writeln!(mtl, "d 0.5");
        }
        if name != "flat" {
            let _ = writeln!(mtl, "map_Kd {stem}.png");
        }
    }

    Ok(ExportedModel {
        obj,
        mtl,
        rgba,
        page_width: width,
        page_height: PAGE_HEIGHT,
        owned: owner.iter().map(|o| o.is_some()).collect(),
    })
}

// ---------------------------------------------------------------------------
// Import.

#[derive(Debug, Clone)]
struct ObjVertex {
    pos: [i16; 3],
    color: [u8; 3],
}

#[derive(Debug, Clone)]
struct ObjFace {
    part: usize,
    semi: bool,
    /// Blend rate for semi faces (from `skin_semi_abrN`; plain `_semi` = 1).
    abr: u8,
    /// CLUT the face wants (`_pNN` material suffix). `None` = auto-palletize.
    palette_hint: Option<u8>,
    v: Vec<usize>,          // 0-based into vertex list
    vt: Vec<Option<usize>>, // 0-based into vt list
}

struct ParsedObj {
    vertices: Vec<ObjVertex>,
    uvs: Vec<(f32, f32)>,
    faces: Vec<ObjFace>,
    part_count: usize,
}

fn parse_obj(text: &str) -> Result<ParsedObj> {
    let mut vertices = Vec::new();
    let mut uvs = Vec::new();
    let mut faces = Vec::new();
    let mut cur_part: Option<usize> = None;
    let mut parts_seen = 0usize;
    let mut semi = false;
    let mut abr = 1u8;
    let mut palette_hint: Option<u8> = None;
    for (ln, raw) in text.lines().enumerate() {
        let line = raw.trim();
        let ctx = || format!("OBJ line {}", ln + 1);
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut it = line.split_whitespace();
        match it.next() {
            Some("v") => {
                let nums: Vec<f32> = it.map(|s| s.parse().unwrap_or(f32::NAN)).collect();
                if nums.len() < 3 || nums[..3.min(nums.len())].iter().any(|n| n.is_nan()) {
                    bail!("{}: malformed vertex", ctx());
                }
                let pos = [
                    round_i16(nums[0]).with_context(ctx)?,
                    round_i16(nums[1]).with_context(ctx)?,
                    round_i16(nums[2]).with_context(ctx)?,
                ];
                let color = if nums.len() >= 6 && nums[3..6].iter().all(|n| n.is_finite()) {
                    [
                        (nums[3].clamp(0.0, 1.0) * 255.0).round() as u8,
                        (nums[4].clamp(0.0, 1.0) * 255.0).round() as u8,
                        (nums[5].clamp(0.0, 1.0) * 255.0).round() as u8,
                    ]
                } else {
                    [0x80; 3] // neutral modulation
                };
                vertices.push(ObjVertex { pos, color });
            }
            Some("vt") => {
                let u: f32 = it.next().and_then(|s| s.parse().ok()).unwrap_or(f32::NAN);
                let v: f32 = it.next().and_then(|s| s.parse().ok()).unwrap_or(0.0);
                if !u.is_finite() || !v.is_finite() {
                    bail!("{}: malformed vt", ctx());
                }
                uvs.push((u, v));
            }
            Some("o") | Some("g") => {
                let name = it.next().unwrap_or("");
                // Accept `part_NN` anywhere in the name (Blender may append
                // suffixes); otherwise assign sequentially per `o` statement.
                let by_name = name
                    .split(|c: char| !c.is_ascii_digit())
                    .rfind(|s| !s.is_empty())
                    .and_then(|d| d.parse::<usize>().ok())
                    .filter(|_| name.contains("part"));
                cur_part = Some(by_name.unwrap_or(parts_seen));
                parts_seen += 1;
            }
            Some("usemtl") => {
                let name = it.next().unwrap_or("");
                semi = name.contains("_semi");
                let digits_after = |tag: &str| -> Option<u8> {
                    name.rsplit_once(tag).and_then(|(_, rest)| {
                        let d: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
                        d.parse().ok()
                    })
                };
                abr = digits_after("_abr").filter(|&a| a <= 3).unwrap_or(1);
                palette_hint = digits_after("_p").filter(|&p| (p as usize) < CLUT_COUNT);
            }
            Some("f") => {
                let part = cur_part.unwrap_or(0);
                let mut v = Vec::new();
                let mut vt = Vec::new();
                for corner in it {
                    let mut segs = corner.split('/');
                    let vi: i64 = segs
                        .next()
                        .and_then(|s| s.parse().ok())
                        .ok_or_else(|| anyhow::anyhow!("{}: malformed face corner", ctx()))?;
                    let ti: Option<i64> = segs.next().and_then(|s| s.parse().ok());
                    let resolve = |i: i64, len: usize| -> Result<usize> {
                        let idx = if i < 0 { len as i64 + i } else { i - 1 };
                        if idx < 0 || idx as usize >= len {
                            bail!("{}: index {i} out of range", ctx());
                        }
                        Ok(idx as usize)
                    };
                    v.push(resolve(vi, vertices.len())?);
                    vt.push(match ti {
                        Some(t) => Some(resolve(t, uvs.len())?),
                        None => None,
                    });
                }
                if !(3..=4).contains(&v.len()) {
                    bail!(
                        "{}: {}-corner face - triangulate or use quads",
                        ctx(),
                        v.len()
                    );
                }
                faces.push(ObjFace {
                    part,
                    semi,
                    abr,
                    palette_hint,
                    v,
                    vt,
                });
            }
            _ => {} // vn, s, mtllib, l, ... ignored
        }
    }
    if faces.is_empty() {
        bail!("OBJ contains no faces");
    }
    let part_count = faces.iter().map(|f| f.part).max().unwrap_or(0) + 1;
    Ok(ParsedObj {
        vertices,
        uvs,
        faces,
        part_count,
    })
}

fn round_i16(v: f32) -> Result<i16> {
    let r = v.round();
    if !(i16::MIN as f32..=i16::MAX as f32).contains(&r) {
        bail!("coordinate {v} out of the i16 GTE range");
    }
    Ok(r as i16)
}

/// Import an OBJ + RGBA page into `(TMD, texture pool)` bytes.
///
/// `expected_parts` is the retail TMD's object count (the animation streams
/// pose parts by index); `page_width` must match the retail pool's width
/// (the battle loader's VRAM upload geometry is derived from it).
pub fn import_obj(
    obj_text: &str,
    rgba: &[u8],
    page_width: usize,
    expected_parts: usize,
) -> Result<ImportedModel> {
    if !(page_width == 128 || page_width == 256) {
        bail!("page width {page_width} is not a monster page (128 or 256)");
    }
    if rgba.len() != page_width * PAGE_HEIGHT * 4 {
        bail!(
            "PNG is not {page_width}x{PAGE_HEIGHT} RGBA ({} bytes, want {})",
            rgba.len(),
            page_width * PAGE_HEIGHT * 4
        );
    }
    let parsed = parse_obj(obj_text)?;
    if parsed.part_count != expected_parts {
        bail!(
            "model has {} parts, the retail mesh has {expected_parts} - the animation \
             streams pose parts by index, the count must match (empty parts are allowed \
             but must exist as `o part_NN` groups with at least one face)",
            parsed.part_count
        );
    }
    let mut warnings = Vec::new();

    // BGR555 view of the page. Texels past the pool width (a narrow pool
    // only backs the left half of the 256-texel UV space) read transparent.
    let texel = |x: usize, y: usize| -> u16 {
        if x >= page_width {
            return 0;
        }
        let t = (y * page_width + x) * 4;
        rgba_to_bgr555(rgba[t], rgba[t + 1], rgba[t + 2], rgba[t + 3])
    };
    let uv8 = |uv: (f32, f32)| -> (u8, u8) {
        let u = (uv.0 * UV_SPACE as f32 - 0.5)
            .round()
            .clamp(0.0, UV_SPACE as f32 - 1.0) as u8;
        let v = ((1.0 - uv.1) * PAGE_HEIGHT as f32 - 0.5)
            .round()
            .clamp(0.0, PAGE_HEIGHT as f32 - 1.0) as u8;
        (u, v)
    };

    // Per-face colour sets (from covered texels), then greedy palette
    // grouping: place each face in a group whose 15-colour budget survives
    // the union, opening groups up to the 15-CLUT cap.
    //
    // The grouping is **seam-aware**: a texel is stored as one 4-bit index
    // but rendered through each referencing face's palette, so faces that
    // share texels must share a palette or the non-owner renders the shared
    // texel through the wrong CLUT. Faces therefore prefer the group that
    // already owns one of their corner texels; only when that group's
    // budget can't absorb them does the seam split (bounded residual, same
    // limitation retail sidesteps by hand-aligning its CLUTs).
    struct FaceTex {
        face: usize,
        uvs: Vec<(u8, u8)>,
        colors: Vec<u16>, // distinct non-transparent BGR555
    }
    let mut face_tex: Vec<FaceTex> = Vec::new();
    for (fi, f) in parsed.faces.iter().enumerate() {
        if f.vt.iter().any(|t| t.is_none()) {
            continue; // untextured face
        }
        let uvs: Vec<(u8, u8)> = f.vt.iter().map(|t| uv8(parsed.uvs[t.unwrap()])).collect();
        // OBJ quads are perimeter-ordered; the rasterizer (like the stored
        // prim) wants PSX Z-order - reorder here so coverage matches the
        // exporter's exactly (the diagonal split differs otherwise).
        let uvs: Vec<(u8, u8)> = if uvs.len() == 4 {
            vec![uvs[0], uvs[1], uvs[3], uvs[2]]
        } else {
            uvs
        };
        // Colour collection is STRICT (corners + barely-inclusive interior):
        // the dilated ring belongs to neighbouring islands and dragging its
        // colours in overflows the 15-colour palette budget.
        let mut colors = std::collections::BTreeSet::new();
        {
            let mut collect = |x: usize, y: usize| {
                let c = texel(x, y);
                if c != 0 {
                    colors.insert(c);
                }
            };
            for &(u, v) in &uvs {
                collect(u as usize, v as usize);
            }
            raster_prim(page_width, PAGE_HEIGHT, &uvs, EPS_STRICT, &mut collect);
        }
        face_tex.push(FaceTex {
            face: fi,
            uvs,
            colors: colors.into_iter().collect(),
        });
    }
    // Fixed 15-slot palette table (slot = CLUT index = the value stored in
    // the prim's CBA low bits). A face with a `_pNN` material hint lands on
    // that exact slot - the exporter writes hints, so a re-import reproduces
    // retail's palette partition verbatim, and a modder can steer faces
    // deliberately. Unhinted faces are palletized greedily over the
    // remaining budget, preferring the slot that already owns one of their
    // corner texels (faces sharing texels must share a palette - a texel is
    // stored as ONE index but rendered through each referencing face's
    // CLUT).
    let mut groups: Vec<std::collections::BTreeSet<u16>> = vec![Default::default(); CLUT_COUNT];
    let mut slot_used = [false; CLUT_COUNT];
    let mut face_group: BTreeMap<usize, usize> = BTreeMap::new(); // face idx -> slot
    let mut corner_group: BTreeMap<(u8, u8), usize> = BTreeMap::new(); // texel -> slot
    // Hinted faces first claim their slots...
    for ft in &face_tex {
        if let Some(p) = parsed.faces[ft.face].palette_hint {
            let gi = p as usize;
            slot_used[gi] = true;
            let before = groups[gi].len();
            for &c in &ft.colors {
                groups[gi].insert(c);
            }
            if groups[gi].len() > 15 && before <= 15 {
                warnings.push(format!(
                    "palette {gi} exceeds 15 colours - some will be quantized"
                ));
            }
            face_group.insert(ft.face, gi);
            for uv in &ft.uvs {
                corner_group.entry(*uv).or_insert(gi);
            }
        }
    }
    // ...then the unhinted faces are placed greedily.
    for ft in &face_tex {
        if parsed.faces[ft.face].palette_hint.is_some() {
            continue;
        }
        let growth_in = |gi: usize, colors: &[u16]| -> usize {
            colors.iter().filter(|c| !groups[gi].contains(c)).count()
        };
        // Preferred: slots already owning one of this face's corner texels,
        // most-shared first.
        let mut pref: BTreeMap<usize, usize> = BTreeMap::new();
        for uv in &ft.uvs {
            if let Some(&gi) = corner_group.get(uv) {
                *pref.entry(gi).or_default() += 1;
            }
        }
        let mut pref: Vec<(usize, usize)> = pref.into_iter().collect();
        pref.sort_by_key(|&(_, shared)| std::cmp::Reverse(shared));
        let mut chosen = pref
            .iter()
            .map(|&(gi, _)| gi)
            .find(|&gi| groups[gi].len() + growth_in(gi, &ft.colors) <= 15);
        if chosen.is_none() {
            // Any used slot the union fits in, minimal growth first.
            chosen = (0..CLUT_COUNT)
                .filter(|&gi| slot_used[gi])
                .filter(|&gi| groups[gi].len() + growth_in(gi, &ft.colors) <= 15)
                .min_by_key(|&gi| growth_in(gi, &ft.colors));
        }
        let gi = match chosen {
            Some(gi) => gi,
            None => match (0..CLUT_COUNT).find(|&gi| !slot_used[gi]) {
                Some(gi) => {
                    if ft.colors.len() > 15 {
                        warnings.push(format!(
                            "face {} alone uses {} colours (15 per palette) - some \
                             will be quantized",
                            ft.face,
                            ft.colors.len()
                        ));
                    }
                    gi
                }
                None => {
                    // Every slot full: take the fewest-new-colours one and
                    // quantize the overflow later.
                    let gi = (0..CLUT_COUNT)
                        .min_by_key(|&gi| growth_in(gi, &ft.colors))
                        .unwrap();
                    warnings.push(format!(
                        "face {} overflows every palette - colours will be quantized",
                        ft.face
                    ));
                    gi
                }
            },
        };
        slot_used[gi] = true;
        for &c in &ft.colors {
            groups[gi].insert(c);
        }
        face_group.insert(ft.face, gi);
        for uv in &ft.uvs {
            corner_group.entry(*uv).or_insert(gi);
        }
    }

    // Canonical order-free ownership over the final group assignment (the
    // same rule the re-export renders through - see [`compute_owners`]).
    let claim_faces: Vec<(Vec<(u8, u8)>, u16)> = face_tex
        .iter()
        .map(|ft| (ft.uvs.clone(), face_group[&ft.face] as u16))
        .collect();
    let owner = compute_owners(&claim_faces, page_width);

    // Final palettes come from the texels each group actually OWNS - the
    // per-face colour sets above only steered the grouping. An owned-texel
    // palette is minimal by construction: it holds exactly the colours the
    // group's stored indices must reproduce (a face's corner that lands on
    // another group's texel no longer pollutes this group's budget).
    let mut final_colors: Vec<std::collections::BTreeSet<u16>> =
        vec![Default::default(); CLUT_COUNT];
    let mut owns_transparent = [false; CLUT_COUNT];
    for (t, o) in owner.iter().enumerate() {
        if let Some(gi) = o {
            let c = texel(t % page_width, t / page_width);
            if c != 0 {
                final_colors[*gi as usize].insert(c);
            } else {
                owns_transparent[*gi as usize] = true;
            }
        }
    }
    // Build the CLUTs, colours in value order. Slot 0 is the PSX transparent
    // texel and is reserved when the group owns any transparent texel - but a
    // fully-opaque group may use all 16 slots (retail does: monster CLUTs
    // exist with 16 opaque colours and no transparent entry). Overflow keeps
    // the first 15/16 deterministically (value order) with a warning.
    let mut cluts: Vec<[u16; 16]> = Vec::with_capacity(CLUT_COUNT);
    for (gi, g) in final_colors.iter().enumerate() {
        let budget = if owns_transparent[gi] || g.len() <= 15 {
            15
        } else {
            16
        };
        let mut colors: Vec<u16> = g.iter().copied().collect();
        if colors.len() > budget {
            warnings.push(format!(
                "palette {gi} owns {} distinct colours ({budget} fit) - quantizing",
                colors.len()
            ));
            colors.truncate(budget);
        }
        let mut clut = [0u16; 16];
        let base = if budget == 15 { 1 } else { 0 };
        for (i, c) in colors.iter().enumerate() {
            clut[base + i] = *c;
        }
        cluts.push(clut);
    }
    let nearest = |clut: &[u16; 16], c: u16| -> u8 {
        if c == 0 {
            return 0;
        }
        let dist = |a: u16, b: u16| -> u32 {
            let (ar, ag, ab) = (a & 0x1F, (a >> 5) & 0x1F, (a >> 10) & 0x1F);
            let (br, bg, bb) = (b & 0x1F, (b >> 5) & 0x1F, (b >> 10) & 0x1F);
            let d = |x: u16, y: u16| (x as i32 - y as i32).pow(2) as u32;
            // The STP bit is a channel too - a semi-transparent texel must
            // not snap to its opaque twin (or vice versa).
            let stp = if (a ^ b) & 0x8000 != 0 { 1 << 20 } else { 0 };
            d(ar, br) + d(ag, bg) + d(ab, bb) + stp
        };
        // Slot 0 participates when it carries a colour (fully-opaque 16-slot
        // CLUTs); zero-valued slots are transparent/unused and never match an
        // opaque texel.
        (0..16)
            .filter(|&i| clut[i] != 0)
            .min_by_key(|&i| dist(clut[i], c))
            .unwrap_or(1) as u8
    };

    // Texel indexing: the owning group's CLUT picks each texel's stored
    // index, so the re-export renders the exact PNG colour back.
    let mut indices = vec![0u8; page_width * PAGE_HEIGHT];
    for (t, o) in owner.iter().enumerate() {
        if let Some(gi) = o {
            let (x, y) = (t % page_width, t / page_width);
            indices[t] = nearest(&cluts[*gi as usize], texel(x, y));
        }
    }

    // Assemble the pool: CLUT region + 4bpp rows.
    let mut pool = vec![0u8; CLUT_REGION_BYTES + page_width / 2 * PAGE_HEIGHT];
    for (gi, clut) in cluts.iter().enumerate() {
        for (i, &c) in clut.iter().enumerate() {
            pool[gi * CLUT_BYTES + i * 2..gi * CLUT_BYTES + i * 2 + 2]
                .copy_from_slice(&c.to_le_bytes());
        }
    }
    for y in 0..PAGE_HEIGHT {
        for xb in 0..page_width / 2 {
            let lo = indices[y * page_width + xb * 2] & 0x0F;
            let hi = indices[y * page_width + xb * 2 + 1] & 0x0F;
            pool[CLUT_REGION_BYTES + y * (page_width / 2) + xb] = lo | (hi << 4);
        }
    }

    // Assemble the TMD: per part, dedup positions, bucket faces into groups
    // by (shape, semi), corner colours from the OBJ vertex colours.
    let mut objects: Vec<ModelObject> = Vec::new();
    for part in 0..expected_parts {
        let mut v_map: BTreeMap<[i16; 3], u16> = BTreeMap::new();
        let mut vertices: Vec<[i16; 3]> = Vec::new();
        let mut buckets: BTreeMap<(u8, bool, u8), Vec<ModelPrim>> = BTreeMap::new();
        for (fi, f) in parsed.faces.iter().enumerate() {
            if f.part != part {
                continue;
            }
            let textured = f.vt.iter().all(|t| t.is_some());
            let n = f.v.len();
            let mut vid = Vec::with_capacity(n);
            let mut cols = Vec::with_capacity(n);
            for &ovi in &f.v {
                let ov = &parsed.vertices[ovi];
                let id = *v_map.entry(ov.pos).or_insert_with(|| {
                    vertices.push(ov.pos);
                    (vertices.len() - 1) as u16
                });
                vid.push(id);
                cols.push(ov.color);
            }
            // Perimeter -> stored Z-order.
            let order: &[usize] = if n == 4 { &[0, 1, 3, 2] } else { &[0, 1, 2] };
            let vid: Vec<u16> = order.iter().map(|&k| vid[k]).collect();
            let cols: Vec<[u8; 3]> = order.iter().map(|&k| cols[k]).collect();
            let flat = cols.iter().all(|c| *c == cols[0]);
            let quad = n == 4;
            let shape = match (textured, flat, quad) {
                (true, true, false) => PacketShape::Ft3,
                (true, true, true) => PacketShape::Ft4,
                (true, false, false) => PacketShape::Gt3,
                (true, false, true) => PacketShape::Gt4,
                (false, true, false) => PacketShape::F3,
                (false, true, true) => PacketShape::F4,
                (false, false, false) => PacketShape::G3,
                (false, false, true) => PacketShape::G4,
            };
            // Untextured semi-transparent groups don't exist on the disc;
            // fold them to opaque with a warning rather than failing.
            let semi = f.semi && textured;
            if f.semi && !textured {
                warnings.push(format!(
                    "face {fi}: semi-transparent untextured face folded to opaque"
                ));
            }
            let (uvs, cba, tsb) = if textured {
                let uvs: Vec<(u8, u8)> = f.vt.iter().map(|t| uv8(parsed.uvs[t.unwrap()])).collect();
                let uvs: Vec<(u8, u8)> = order.iter().map(|&k| uvs[k]).collect();
                let gi = face_group.get(&fi).copied().unwrap_or(0);
                let tsb = if semi {
                    (TSB_BASE & !0x60) | ((f.abr as u16) << 5)
                } else {
                    TSB_BASE
                };
                (uvs, CBA_BASE | gi as u16, tsb)
            } else {
                (Vec::new(), 0, 0)
            };
            let colors = if flat { vec![cols[0]] } else { cols };
            let shape_key = shape as u8;
            buckets
                .entry((shape_key, semi, if semi { f.abr } else { 1 }))
                .or_default()
                .push(ModelPrim {
                    vertices: vid,
                    uvs,
                    cba,
                    tsb,
                    colors,
                });
        }
        if vertices.is_empty() {
            bail!(
                "part {part} has no faces - every retail part must keep at least one \
                 (a stray invisible triangle works)"
            );
        }
        let mut groups_out: Vec<ModelGroup> = Vec::new();
        for ((shape_key, semi, _abr), prims) in buckets {
            let shape = [
                PacketShape::F3,
                PacketShape::Ft3,
                PacketShape::G3,
                PacketShape::Gt3,
                PacketShape::F4,
                PacketShape::Ft4,
                PacketShape::G4,
                PacketShape::Gt4,
            ]
            .into_iter()
            .find(|s| *s as u8 == shape_key)
            .expect("key from shape");
            groups_out.push(ModelGroup {
                shape,
                semi_transparent: semi,
                prims,
            });
        }
        objects.push(ModelObject {
            vertices,
            groups: groups_out,
            scale: LEGAIA_OBJECT_SCALE,
        });
    }

    let tmd = encode(&objects)?;
    Ok(ImportedModel {
        tmd,
        pool,
        palettes_used: slot_used.iter().filter(|&&u| u).count(),
        warnings,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a tiny OBJ + PNG, import, and check the pieces line up.
    #[test]
    fn import_builds_a_valid_tmd_and_pool() {
        let obj = r#"
# test
o part_00
v 0 0 0 0.5 0.5 0.5
v 100 0 0 0.5 0.5 0.5
v 0 100 0 0.5 0.5 0.5
vt 0.1 0.9
vt 0.2 0.9
vt 0.1 0.8
usemtl skin
f 1/1 2/2 3/3
o part_01
v 0 0 50
v 100 0 50
v 0 100 50
f 4 5 6
"#;
        let mut rgba = vec![0u8; 256 * 256 * 4];
        // A red square where the face's UVs land (top-left area).
        for y in 0..64 {
            for x in 0..64 {
                let t = (y * 256 + x) * 4;
                rgba[t] = 200;
                rgba[t + 3] = 255;
            }
        }
        let m = import_obj(obj, &rgba, 256, 2).unwrap();
        assert_eq!(m.palettes_used, 1);
        let tmd = legaia_tmd::parse(&m.tmd).unwrap();
        assert_eq!(tmd.objects.len(), 2);
        let model = decode_model(&tmd, &m.tmd).unwrap();
        // Part 0: one textured flat tri (uniform colour) -> Ft3, cba group 0.
        assert_eq!(model[0].groups.len(), 1);
        assert_eq!(model[0].groups[0].shape, PacketShape::Ft3);
        assert_eq!(model[0].groups[0].prims[0].cba, CBA_BASE);
        assert_eq!(model[0].groups[0].prims[0].tsb, TSB_BASE);
        // Part 1: untextured flat tri (no colours given -> neutral) -> F3.
        assert_eq!(model[1].groups[0].shape, PacketShape::F3);
        // Pool: CLUT region + 128 bytes/row * 256 rows.
        assert_eq!(m.pool.len(), CLUT_REGION_BYTES + 128 * 256);
        // Palette slot 0 transparent, slot 1 = the red.
        let c1 = u16::from_le_bytes([m.pool[2], m.pool[3]]);
        assert_eq!(u16::from_le_bytes([m.pool[0], m.pool[1]]), 0);
        assert_eq!(c1, rgba_to_bgr555(200, 0, 0, 255));
    }

    #[test]
    fn import_rejects_part_count_mismatch() {
        let obj = "o part_00\nv 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3\n";
        let rgba = vec![0u8; 256 * 256 * 4];
        assert!(import_obj(obj, &rgba, 256, 2).is_err());
    }

    #[test]
    fn bgr555_round_trips_alpha_classes() {
        // Transparent, opaque, semi, opaque black.
        assert_eq!(rgba_to_bgr555(10, 10, 10, 0), 0);
        let opaque = rgba_to_bgr555(255, 0, 0, 255);
        assert_eq!(bgr555_to_rgba(opaque)[3], 255);
        let semi = rgba_to_bgr555(255, 0, 0, 128);
        assert!(semi & 0x8000 != 0);
        assert_eq!(bgr555_to_rgba(semi)[3], 128);
        assert_eq!(rgba_to_bgr555(0, 0, 0, 255), 0x8000);
        assert_eq!(bgr555_to_rgba(0x8000)[3], 255);
    }

    #[test]
    fn quad_winding_round_trips_through_obj_order() {
        // A quad face in perimeter order becomes stored Z-order and back.
        let obj = r#"
o part_00
v 0 0 0
v 10 0 0
v 10 10 0
v 0 10 0
f 1 2 3 4
"#;
        let rgba = vec![0u8; 256 * 256 * 4];
        let m = import_obj(obj, &rgba, 256, 1).unwrap();
        let tmd = legaia_tmd::parse(&m.tmd).unwrap();
        let model = decode_model(&tmd, &m.tmd).unwrap();
        let p = &model[0].groups[0].prims[0];
        // Stored Z-order: perimeter (a,b,c,d) -> (a,b,d,c).
        let pos: Vec<[i16; 3]> = p
            .vertices
            .iter()
            .map(|&v| model[0].vertices[v as usize])
            .collect();
        assert_eq!(pos, vec![[0, 0, 0], [10, 0, 0], [0, 10, 0], [10, 10, 0]]);
    }
}
