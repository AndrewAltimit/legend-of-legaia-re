//! Party <-> Delilas battle-model swap: rebuild a playable character's
//! assembled battle model on a Delilas monster rig (and, in the sibling
//! module direction, a Delilas model on a player rig).
//!
//! Both animation systems share one pose model: flat per-part rigid
//! transforms addressed **by part index** (`docs/formats/monster-animation.md`,
//! player streams at entry `+0xAC` per `battle_char_assembly::animation`).
//! A cross-swap therefore reduces to:
//!
//! 1. **Anatomy permutation.** Player rigs order their 15 skeleton bones
//!    `[torso, pelvis, head, armA x3, armB x3, legA x3, legB x3]`
//!    (Noa inserts a 16th hair bone at channel 3); every Delilas mesh
//!    orders its 15 parts `[head, torso, pelvis, armA x3, armB x3,
//!    legA x3, legB x3]`. Measured from the rest-pose world centroids of
//!    all six rigs - see `docs/tooling/randomizer.md` § Delilas party swap.
//! 2. **Extras merge.** The player's two equipment-visual objects ride
//!    their attach bone's pose channel exactly, so merging their geometry
//!    into the attach bone's object is pose-exact (same local frame).
//!    Noa's hair object rides its own channel and is rebased into the
//!    head's rest frame instead (rigid approximation).
//! 3. **Rest-pose bake.** Local part frames are per-rig conventions (a
//!    retail player mesh even shares one mesh between its left and right
//!    limb chains, mirrored purely by pose rotations), so each part's
//!    geometry is re-expressed through `source rest pose -> target rest
//!    pose` (see `bake_object`), with a uniform `target_height /
//!    source_height` world scale. At the target's rest frame the swapped
//!    model reproduces the source's own combat stance exactly; the
//!    target streams move it rigidly from there.
//! 4. **Texture re-layout.** Both texture systems are 4bpp indices +
//!    16-colour CLUTs, so no quantization happens in either direction:
//!    used texel islands are copied bit-exact between the player band
//!    pages (`battle_char_assembly::texture`) and the monster pool page,
//!    CLUTs are carried over (player upload semantics apply the STP pass
//!    at load; the monster pool stores entries final, so the copy ORs
//!    `0x8000` onto non-zero entries), and every textured prim's UV /
//!    CBA / TSB is rewritten to the target's authoring conventions.

use anyhow::{Context, Result, bail};
use std::collections::BTreeMap;

use crate::battle_char_assembly::{self, AUTHORING_FIRST_TEXPAGE, SECTION_COUNT, TextureUpload};
use crate::battle_data_pack;
use crate::monster_archive::{self, PartPose};
use crate::monster_model::{CBA_BASE, CLUT_COUNT, CLUT_REGION_BYTES, PAGE_HEIGHT, UV_SPACE};
use legaia_tmd::encode::{ModelGroup, ModelObject, decode_model, encode};

pub mod fieldize;
pub mod playerize;

/// Canonical part order of the swap = the Delilas mesh order shared by all
/// three siblings: `[head, torso, pelvis, armA(u,f,h), armB(u,f,h),
/// legA(t,s,f), legB(t,s,f)]`.
pub const CANONICAL_PARTS: usize = 15;

/// A playable character's battle rig, as the swap needs it: which player
/// pose channel carries each canonical part, plus the hair channel Noa
/// alone has.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlayerRig {
    /// `channel_for_canonical[c]` = the player skeleton channel driving
    /// canonical part `c`.
    pub channel_for_canonical: [u8; CANONICAL_PARTS],
    /// Noa's hair channel (merged into the head part); `None` elsewhere.
    pub hair_channel: Option<u8>,
}

/// Vahn (player file PROT 0863) and Gala (0865): 15 bones,
/// `[torso, pelvis, head, armA x3, armB x3, legA x3, legB x3]`.
pub const RIG_VAHN_GALA: PlayerRig = PlayerRig {
    channel_for_canonical: [2, 0, 1, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14],
    hair_channel: None,
};

/// Noa (player file PROT 0864): 16 bones - channel 3 is her hair, the
/// limb chains shift up by one.
pub const RIG_NOA: PlayerRig = PlayerRig {
    channel_for_canonical: [2, 0, 1, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15],
    hair_channel: Some(3),
};

/// The enemy-side conversion result: a player model rebuilt on a Delilas
/// monster rig, ready for `monster_archive::replace_mesh_and_pool`.
#[derive(Debug, Clone)]
pub struct MonsterizedPlayer {
    /// Legaia TMD bytes, [`CANONICAL_PARTS`] objects in Delilas part order.
    pub tmd: Vec<u8>,
    /// Monster texture pool (`[15 CLUTs][4bpp 256x256 page]`).
    pub pool: Vec<u8>,
    /// Non-fatal notes (island downscales, dropped palettes, ...).
    pub warnings: Vec<String>,
}

// ---------------------------------------------------------------------------
// Pose math (shared with the probe measurements): PSX rotation order
// Rz * Ry * Rx, angles in 1/4096 turns, y-down GTE space.

fn rot_matrix(p: &PartPose) -> [[f32; 3]; 3] {
    let rad = |r: u16| (r as f32) * std::f32::consts::TAU / 4096.0;
    let (sx, cx) = rad(p.rx).sin_cos();
    let (sy, cy) = rad(p.ry).sin_cos();
    let (sz, cz) = rad(p.rz).sin_cos();
    [
        [cy * cz, sx * sy * cz - cx * sz, cx * sy * cz + sx * sz],
        [cy * sz, sx * sy * sz + cx * cz, cx * sy * sz - sx * cz],
        [-sy, sx * cy, cx * cy],
    ]
}

fn apply(m: &[[f32; 3]; 3], v: [f32; 3]) -> [f32; 3] {
    [
        m[0][0] * v[0] + m[0][1] * v[1] + m[0][2] * v[2],
        m[1][0] * v[0] + m[1][1] * v[1] + m[1][2] * v[2],
        m[2][0] * v[0] + m[2][1] * v[1] + m[2][2] * v[2],
    ]
}

fn apply_transposed(m: &[[f32; 3]; 3], v: [f32; 3]) -> [f32; 3] {
    [
        m[0][0] * v[0] + m[1][0] * v[1] + m[2][0] * v[2],
        m[0][1] * v[0] + m[1][1] * v[1] + m[2][1] * v[2],
        m[0][2] * v[0] + m[1][2] * v[1] + m[2][2] * v[2],
    ]
}

/// Rest-pose model height: `-min(world y)` over every vertex of every
/// posed part (y-down space; feet rest near 0). `poses[i]` drives
/// `objects[channel_of(i)]` - the caller supplies the channel mapping so
/// player (channel-indexed) and monster (identity) rigs share the code.
fn posed_height(objects: &[ModelObject], poses: &[PartPose], channel_of: &[u8]) -> f32 {
    let mut min_y = f32::MAX;
    for (oi, o) in objects.iter().enumerate() {
        let ch = channel_of.get(oi).copied().unwrap_or(0) as usize;
        let Some(pose) = poses.get(ch) else { continue };
        let m = rot_matrix(pose);
        for v in &o.vertices {
            let w = apply(&m, [v[0] as f32, v[1] as f32, v[2] as f32]);
            min_y = min_y.min(w[1] + pose.ty as f32);
        }
    }
    if min_y == f32::MAX { 0.0 } else { -min_y }
}

fn round_coord(v: f32) -> Result<i16> {
    let r = v.round();
    if !(i16::MIN as f32..=i16::MAX as f32).contains(&r) {
        bail!("baked coordinate {r} out of the i16 GTE range");
    }
    Ok(r as i16)
}

/// Re-express `o`'s geometry (authored in `src`'s local frame) in `dst`'s
/// local frame, with a uniform world scale `s` about the model origin:
/// `v' = R_dst^T (s (R_src v + T_src) - T_dst)`.
///
/// Local part frames are per-rig conventions (retail player meshes even
/// share one mesh between the left and right limb chains, mirrored purely
/// by the pose rotations), so a raw geometry copy scatters parts. Baking
/// through the two rest poses makes the swapped model reproduce the
/// source rig's own rest stance exactly at the target's rest frame, and
/// the target streams then move it rigidly from there - no local-frame
/// assumptions at all.
fn bake_object(o: &mut ModelObject, src: &PartPose, dst: &PartPose, s: f32) -> Result<()> {
    let ms = rot_matrix(src);
    let md = rot_matrix(dst);
    for v in o.vertices.iter_mut() {
        let w = apply(&ms, [v[0] as f32, v[1] as f32, v[2] as f32]);
        let world = [
            s * (w[0] + src.tx as f32) - dst.tx as f32,
            s * (w[1] + src.ty as f32) - dst.ty as f32,
            s * (w[2] + src.tz as f32) - dst.tz as f32,
        ];
        let l = apply_transposed(&md, world);
        *v = [round_coord(l[0])?, round_coord(l[1])?, round_coord(l[2])?];
    }
    Ok(())
}

/// Lossless object compaction: dedup identical vertex positions (prim
/// indices remapped) and merge same-render-state groups. The section
/// splice + extras merge leaves both kinds of slack, and the compressed
/// archive slot is a hard 0x14000 budget.
fn compact_object(o: &mut ModelObject) {
    // Vertex dedup by exact position.
    let mut first: BTreeMap<[i16; 3], u16> = BTreeMap::new();
    let mut remap: Vec<u16> = Vec::with_capacity(o.vertices.len());
    let mut kept: Vec<[i16; 3]> = Vec::new();
    for v in &o.vertices {
        let id = *first.entry(*v).or_insert_with(|| {
            kept.push(*v);
            (kept.len() - 1) as u16
        });
        remap.push(id);
    }
    o.vertices = kept;
    for g in o.groups.iter_mut() {
        for p in g.prims.iter_mut() {
            for vi in p.vertices.iter_mut() {
                *vi = remap[*vi as usize];
            }
        }
    }
    // Group merge: same (shape, semi) groups concatenate.
    let mut merged: Vec<ModelGroup> = Vec::new();
    for g in o.groups.drain(..) {
        if let Some(last) = merged
            .last_mut()
            .filter(|m| m.shape == g.shape && m.semi_transparent == g.semi_transparent)
        {
            last.prims.extend(g.prims);
        } else {
            merged.push(g);
        }
    }
    // Non-adjacent same-state groups too (order across groups does not
    // matter for opaque draws; semi groups keep their relative order).
    let mut out: Vec<ModelGroup> = Vec::new();
    for g in merged {
        if let Some(prev) = out.iter_mut().find(|m| {
            m.shape == g.shape && m.semi_transparent == g.semi_transparent && !g.semi_transparent
        }) {
            prev.prims.extend(g.prims);
        } else {
            out.push(g);
        }
    }
    o.groups = out;
}

/// Append `src`'s geometry into `dst` (vertex indices rebased).
fn merge_object(dst: &mut ModelObject, src: &ModelObject) {
    let base = dst.vertices.len() as u16;
    dst.vertices.extend_from_slice(&src.vertices);
    for g in &src.groups {
        let mut g2 = g.clone();
        for p in g2.prims.iter_mut() {
            for vi in p.vertices.iter_mut() {
                *vi += base;
            }
        }
        dst.groups.push(g2);
    }
}

/// Rebase `src` (posed by `src_pose`) into `dst_pose`'s local frame and
/// merge it into `dst` - the rigid approximation used for Noa's hair:
/// `v' = R_dst^T (R_src v + T_src - T_dst)`.
fn rebase_merge(
    dst: &mut ModelObject,
    dst_pose: &PartPose,
    src: &ModelObject,
    src_pose: &PartPose,
) -> Result<()> {
    let ms = rot_matrix(src_pose);
    let md = rot_matrix(dst_pose);
    let mut moved = src.clone();
    for v in moved.vertices.iter_mut() {
        let w = apply(&ms, [v[0] as f32, v[1] as f32, v[2] as f32]);
        let rel = [
            w[0] + (src_pose.tx - dst_pose.tx) as f32,
            w[1] + (src_pose.ty - dst_pose.ty) as f32,
            w[2] + (src_pose.tz - dst_pose.tz) as f32,
        ];
        let l = apply_transposed(&md, rel);
        *v = [round_coord(l[0])?, round_coord(l[1])?, round_coord(l[2])?];
    }
    merge_object(dst, &moved);
    Ok(())
}

// ---------------------------------------------------------------------------
// Source texel space: the player band's two authoring pages.

/// One 256x256 4bpp page as decoded texel indices + presence.
struct Page {
    indices: Vec<u8>,
    present: Vec<bool>,
}

impl Page {
    fn new() -> Self {
        Page {
            indices: vec![0; UV_SPACE * PAGE_HEIGHT],
            present: vec![false; UV_SPACE * PAGE_HEIGHT],
        }
    }
}

/// The player band in authoring space: page 0x15 / 0x16 bitmaps plus the
/// CLUT row keyed by CBA column (`cba & 0x3F`).
struct BandTexels {
    pages: [Page; 2],
    /// Palette per CBA column, STP pass applied (`e |= 0x8000` on
    /// non-zero entries - the retail upload semantics).
    palettes: BTreeMap<u8, [u16; 16]>,
}

fn band_texels(uploads: &[TextureUpload]) -> BandTexels {
    let mut pages = [Page::new(), Page::new()];
    let mut palettes: BTreeMap<u8, [u16; 16]> = BTreeMap::new();
    for u in uploads {
        // Pixel half: band-relative rect, page split at halfword 0x40.
        let hw_w = u.rect.w as usize;
        for row in 0..u.rect.h as usize {
            for hw in 0..hw_w {
                let abs_hw = u.rect.x0 as usize + hw;
                let (page, page_hw) = if abs_hw < 0x40 {
                    (0usize, abs_hw)
                } else {
                    (1usize, abs_hw - 0x40)
                };
                let src = (row * hw_w + hw) * 2;
                let Some(&lo) = u.pixels.get(src) else {
                    continue;
                };
                let hi = u.pixels.get(src + 1).copied().unwrap_or(0);
                let y = u.rect.y0 as usize + row;
                if y >= PAGE_HEIGHT || page_hw >= 0x40 {
                    continue;
                }
                let word = u16::from_le_bytes([lo, hi]);
                for t in 0..4 {
                    let x = page_hw * 4 + t;
                    let idx = ((word >> (t * 4)) & 0xF) as u8;
                    let at = y * UV_SPACE + x;
                    pages[page].indices[at] = idx;
                    pages[page].present[at] = true;
                }
            }
        }
        // CLUT half: entries land at clut_x on the runtime row; the CBA
        // column addresses the same x in 16-halfword steps.
        if !u.clut.is_empty() && u.clut_x % 16 == 0 {
            let first_col = (u.clut_x / 16) as u8;
            for (chunk_i, chunk) in u.clut.chunks(16).enumerate() {
                let mut pal = [0u16; 16];
                for (i, &e) in chunk.iter().enumerate() {
                    pal[i] = if e == 0 { 0 } else { e | 0x8000 };
                }
                palettes.insert(first_col + chunk_i as u8, pal);
            }
        }
    }
    BandTexels { pages, palettes }
}

// ---------------------------------------------------------------------------
// Texture re-layout: cluster textured faces by UV-bbox overlap, shelf-pack
// the clusters into the target page, copy indices, rewrite UV/CBA/TSB.

struct FaceRef {
    obj: usize,
    group: usize,
    prim: usize,
    page: usize,
    bbox: (u8, u8, u8, u8), // (x0, y0, x1, y1) inclusive
}

fn face_bbox(uvs: &[(u8, u8)]) -> (u8, u8, u8, u8) {
    let mut b = (u8::MAX, u8::MAX, 0u8, 0u8);
    for &(u, v) in uvs {
        b.0 = b.0.min(u);
        b.1 = b.1.min(v);
        b.2 = b.2.max(u);
        b.3 = b.3.max(v);
    }
    b
}

fn boxes_touch(a: (u8, u8, u8, u8), b: (u8, u8, u8, u8)) -> bool {
    // Inflate by 1 texel so the importer-style dilation ring can't split
    // an island across two clusters.
    let (ax0, ay0, ax1, ay1) = (
        a.0 as i32 - 1,
        a.1 as i32 - 1,
        a.2 as i32 + 1,
        a.3 as i32 + 1,
    );
    let (bx0, by0, bx1, by1) = (b.0 as i32, b.1 as i32, b.2 as i32, b.3 as i32);
    ax0 <= bx1 && bx0 <= ax1 && ay0 <= by1 && by0 <= ay1
}

struct Cluster {
    faces: Vec<usize>,
    page: usize,
    bbox: (u8, u8, u8, u8),
    /// Source-space downscale divisor (1 = 1:1, 2 = half resolution).
    scale: u32,
    /// Placement in the target page.
    dst: (usize, usize),
}

fn union_bbox(a: (u8, u8, u8, u8), b: (u8, u8, u8, u8)) -> (u8, u8, u8, u8) {
    (a.0.min(b.0), a.1.min(b.1), a.2.max(b.2), a.3.max(b.3))
}

/// Shelf-pack `w x h` rects (sorted tallest-first by the caller) into a
/// `UV_SPACE x PAGE_HEIGHT` page. Returns placements or `None` on overflow.
fn shelf_pack(sizes: &[(usize, usize)]) -> Option<Vec<(usize, usize)>> {
    let mut order: Vec<usize> = (0..sizes.len()).collect();
    order.sort_by_key(|&i| std::cmp::Reverse(sizes[i].1));
    let mut out = vec![(0usize, 0usize); sizes.len()];
    let (mut x, mut y, mut shelf_h) = (0usize, 0usize, 0usize);
    for &i in &order {
        let (w, h) = sizes[i];
        if w > UV_SPACE {
            return None;
        }
        if x + w > UV_SPACE {
            y += shelf_h;
            x = 0;
            shelf_h = 0;
        }
        if y + h > PAGE_HEIGHT {
            return None;
        }
        out[i] = (x, y);
        x += w;
        shelf_h = shelf_h.max(h);
    }
    Some(out)
}

/// Re-layout every textured prim of `objects` from the player band into a
/// fresh 256x256 monster page. Rewrites UVs and CBA/TSB in place; returns
/// the finished pool.
fn relayout_to_monster_pool(
    objects: &mut [ModelObject],
    band: &BandTexels,
    base_scale: u32,
    warnings: &mut Vec<String>,
) -> Result<Vec<u8>> {
    // Collect textured faces.
    let mut faces: Vec<FaceRef> = Vec::new();
    for (oi, o) in objects.iter().enumerate() {
        for (gi, g) in o.groups.iter().enumerate() {
            if !g.shape.is_textured() {
                continue;
            }
            for (pi, p) in g.prims.iter().enumerate() {
                let page = if p.tsb & 0x1F == AUTHORING_FIRST_TEXPAGE {
                    0
                } else {
                    1
                };
                faces.push(FaceRef {
                    obj: oi,
                    group: gi,
                    prim: pi,
                    page,
                    bbox: face_bbox(&p.uvs),
                });
            }
        }
    }

    // Union-find clustering by inflated-bbox overlap, per page.
    let mut parent: Vec<usize> = (0..faces.len()).collect();
    fn find(parent: &mut Vec<usize>, i: usize) -> usize {
        if parent[i] != i {
            let r = find(parent, parent[i]);
            parent[i] = r;
        }
        parent[i]
    }
    // O(n^2) over ~1k faces - fine.
    for i in 0..faces.len() {
        for j in (i + 1)..faces.len() {
            if faces[i].page == faces[j].page && boxes_touch(faces[i].bbox, faces[j].bbox) {
                let (ri, rj) = (find(&mut parent, i), find(&mut parent, j));
                if ri != rj {
                    parent[ri] = rj;
                }
            }
        }
    }
    let mut clusters: BTreeMap<usize, Cluster> = BTreeMap::new();
    #[allow(clippy::needless_range_loop)] // `find` needs `&mut parent` alongside the index
    for i in 0..faces.len() {
        let root = find(&mut parent, i);
        let e = clusters.entry(root).or_insert(Cluster {
            faces: Vec::new(),
            page: faces[i].page,
            bbox: faces[i].bbox,
            scale: base_scale,
            dst: (0, 0),
        });
        e.bbox = union_bbox(e.bbox, faces[i].bbox);
        e.faces.push(i);
    }
    let mut clusters: Vec<Cluster> = clusters.into_values().collect();

    // Pack; on overflow halve the largest clusters until it fits.
    loop {
        let sizes: Vec<(usize, usize)> = clusters
            .iter()
            .map(|c| {
                let w = (c.bbox.2 - c.bbox.0) as usize + 1;
                let h = (c.bbox.3 - c.bbox.1) as usize + 1;
                (w.div_ceil(c.scale as usize), h.div_ceil(c.scale as usize))
            })
            .collect();
        if let Some(placed) = shelf_pack(&sizes) {
            for (c, p) in clusters.iter_mut().zip(placed) {
                c.dst = p;
            }
            break;
        }
        // Halve the largest un-halved cluster (cap at base/4).
        let cap = base_scale * 4;
        let Some(big) = clusters
            .iter_mut()
            .filter(|c| c.scale < cap)
            .max_by_key(|c| {
                let w = (c.bbox.2 - c.bbox.0) as usize + 1;
                let h = (c.bbox.3 - c.bbox.1) as usize + 1;
                (w / c.scale as usize) * (h / c.scale as usize)
            })
        else {
            bail!("texture islands exceed the 256x256 monster page even at quarter resolution");
        };
        big.scale *= 2;
        warnings.push(format!(
            "texture island {}x{} downscaled to 1/{} to fit the monster page",
            big.bbox.2 - big.bbox.0 + 1,
            big.bbox.3 - big.bbox.1 + 1,
            big.scale
        ));
    }

    // Palette mapping: CBA columns used by textured faces -> pool slots.
    let mut col_to_slot: BTreeMap<u8, u8> = BTreeMap::new();
    for f in &faces {
        let col = (objects[f.obj].groups[f.group].prims[f.prim].cba & 0x3F) as u8;
        let next = col_to_slot.len() as u8;
        col_to_slot.entry(col).or_insert(next);
    }
    if col_to_slot.len() > CLUT_COUNT {
        bail!(
            "player mesh samples {} palettes - the monster pool holds {CLUT_COUNT}",
            col_to_slot.len()
        );
    }

    // Copy indices + rewrite prims.
    let mut page_indices = vec![0u8; UV_SPACE * PAGE_HEIGHT];
    for c in &clusters {
        let (sw, sh) = (
            (c.bbox.2 - c.bbox.0) as usize + 1,
            (c.bbox.3 - c.bbox.1) as usize + 1,
        );
        let s = c.scale as usize;
        for dy in 0..sh.div_ceil(s) {
            for dx in 0..sw.div_ceil(s) {
                let sx = c.bbox.0 as usize + dx * s;
                let sy = c.bbox.1 as usize + dy * s;
                let (tx, ty) = (c.dst.0 + dx, c.dst.1 + dy);
                if sx >= UV_SPACE || sy >= PAGE_HEIGHT || tx >= UV_SPACE || ty >= PAGE_HEIGHT {
                    continue;
                }
                page_indices[ty * UV_SPACE + tx] = band.pages[c.page].indices[sy * UV_SPACE + sx];
            }
        }
        for &fi in &c.faces {
            let f = &faces[fi];
            let p = &mut objects[f.obj].groups[f.group].prims[f.prim];
            for uv in p.uvs.iter_mut() {
                let nx = c.dst.0 + (uv.0 as usize - c.bbox.0 as usize) / s;
                let ny = c.dst.1 + (uv.1 as usize - c.bbox.1 as usize) / s;
                *uv = (nx.min(UV_SPACE - 1) as u8, ny.min(PAGE_HEIGHT - 1) as u8);
            }
            let col = (p.cba & 0x3F) as u8;
            let slot = col_to_slot[&col] as u16;
            p.cba = CBA_BASE | slot;
            // Keep the ABR bits, force page 5 (x 320) + 4bpp.
            p.tsb = (p.tsb & 0x0060) | 0x0005;
        }
    }

    // Pool: 15 CLUTs then the 4bpp page.
    let mut pool = vec![0u8; CLUT_REGION_BYTES];
    for (&col, &slot) in &col_to_slot {
        let pal = band.palettes.get(&col).copied().unwrap_or_else(|| {
            warnings.push(format!("CBA column {col} has no uploaded palette; black"));
            [0u16; 16]
        });
        for (i, e) in pal.iter().enumerate() {
            let at = slot as usize * 32 + i * 2;
            pool[at..at + 2].copy_from_slice(&e.to_le_bytes());
        }
    }
    for y in 0..PAGE_HEIGHT {
        for xb in 0..UV_SPACE / 2 {
            let lo = page_indices[y * UV_SPACE + xb * 2] & 0xF;
            let hi = page_indices[y * UV_SPACE + xb * 2 + 1] & 0xF;
            pool.push(lo | (hi << 4));
        }
    }
    Ok(pool)
}

// ---------------------------------------------------------------------------
// The enemy-side conversion.

/// Rebuild a playable character's **default-equipment** battle model on a
/// Delilas monster rig. `player_file` is the character's `PLAYERn` bytes
/// (extraction PROT 863..865), `rig` the matching [`PlayerRig`];
/// `archive_entry` + `target_id` name the monster block whose rig (and
/// rest-pose height) the model is rebuilt for.
pub fn monsterize_player(
    player_file: &[u8],
    rig: &PlayerRig,
    archive_entry: &[u8],
    target_id: u16,
) -> Result<MonsterizedPlayer> {
    monsterize_player_scaled(player_file, rig, archive_entry, target_id, 1)
}

/// [`monsterize_player`] with an explicit global texture downscale (`1` =
/// full resolution). [`swap_into_block`] raises it only when the rebuilt
/// block misses the compressed archive-slot budget.
fn monsterize_player_scaled(
    player_file: &[u8],
    rig: &PlayerRig,
    archive_entry: &[u8],
    target_id: u16,
    texture_downscale: u32,
) -> Result<MonsterizedPlayer> {
    let mut warnings = Vec::new();
    if texture_downscale > 1 {
        warnings.push(format!(
            "texture at 1/{texture_downscale} resolution to fit the compressed archive slot"
        ));
    }
    let pack = battle_data_pack::parse(player_file).context("parse player battle file")?;
    let equipped = [0u8; SECTION_COUNT];
    let asm = battle_char_assembly::assemble_character(player_file, &pack, &equipped)
        .context("assemble default-equipment battle mesh")?;
    let tmd = legaia_tmd::parse(&asm.tmd).context("parse assembled TMD")?;
    let mut source = decode_model(&tmd, &asm.tmd).context("decode assembled model")?;

    let idle = battle_char_assembly::idle_battle_animation(player_file)?
        .ok_or_else(|| anyhow::anyhow!("player file has no idle animation"))?;
    let rest = idle
        .frames
        .first()
        .ok_or_else(|| anyhow::anyhow!("idle animation has no frames"))?
        .clone();

    // Source height before any surgery (channel map = anm_bones).
    let src_height = posed_height(&source, &rest, &asm.anm_bones);

    // Merge the equipment extras into their attach bones' objects (same
    // channel = same local frame; plain concat).
    let skeleton = rest.len();
    let mut merged: Vec<Option<ModelObject>> = source.drain(..).map(Some).collect();
    for (oi, &ch) in asm.anm_bones.iter().enumerate() {
        if oi < skeleton {
            continue; // skeleton object
        }
        let Some(extra) = merged[oi].take() else {
            continue;
        };
        if let Some(dst) = merged.get_mut(ch as usize).and_then(|d| d.as_mut()) {
            merge_object(dst, &extra);
        }
    }
    // Noa's hair: rebase into the head frame.
    if let Some(hair_ch) = rig.hair_channel {
        let head_ch = rig.channel_for_canonical[0] as usize;
        if let Some(hair) = merged.get_mut(hair_ch as usize).and_then(|h| h.take()) {
            let head_pose = rest[head_ch];
            let hair_pose = rest[hair_ch as usize];
            if let Some(head) = merged.get_mut(head_ch).and_then(|d| d.as_mut()) {
                rebase_merge(head, &head_pose, &hair, &hair_pose)?;
            }
        }
    }

    // Permute into the canonical (Delilas) part order.
    let mut objects: Vec<ModelObject> = Vec::with_capacity(CANONICAL_PARTS);
    for c in 0..CANONICAL_PARTS {
        let ch = rig.channel_for_canonical[c] as usize;
        let obj = merged
            .get_mut(ch)
            .and_then(|o| o.take())
            .ok_or_else(|| anyhow::anyhow!("player channel {ch} has no object"))?;
        objects.push(obj);
    }

    for o in objects.iter_mut() {
        compact_object(o);
    }

    // Scale to the target rig's rest height.
    let target_mesh = monster_archive::mesh(archive_entry, target_id)?
        .ok_or_else(|| anyhow::anyhow!("monster id {target_id}: empty slot"))?;
    let target_tmd = legaia_tmd::parse(target_mesh.tmd_bytes()).context("target monster TMD")?;
    let target_model = decode_model(&target_tmd, target_mesh.tmd_bytes())?;
    if target_model.len() != CANONICAL_PARTS {
        bail!(
            "monster id {target_id} has {} parts, expected {CANONICAL_PARTS}",
            target_model.len()
        );
    }
    let target_idle = monster_archive::idle_animation(archive_entry, target_id)?
        .ok_or_else(|| anyhow::anyhow!("monster id {target_id}: no idle animation"))?;
    let target_rest = target_idle
        .frames
        .first()
        .ok_or_else(|| anyhow::anyhow!("monster idle has no frames"))?;
    let identity: Vec<u8> = (0..CANONICAL_PARTS as u8).collect();
    let dst_height = posed_height(&target_model, target_rest, &identity);
    let s = if src_height > 1.0 && dst_height > 1.0 {
        dst_height / src_height
    } else {
        1.0
    };
    if (s - 1.0).abs() > 0.01 {
        warnings.push(format!(
            "geometry scaled by {s:.3} to the target rig height"
        ));
    }
    // Bake each part through source-channel rest -> target-part rest (see
    // `bake_object` - this is what makes arbitrary rig pairings pose
    // coherently despite per-rig local-frame conventions).
    for (c, o) in objects.iter_mut().enumerate() {
        let src_pose = rest[rig.channel_for_canonical[c] as usize];
        let dst_pose = target_rest[c];
        bake_object(o, &src_pose, &dst_pose, s)
            .with_context(|| format!("bake canonical part {c}"))?;
    }

    // Texture re-layout from the player band into the monster page.
    let uploads = battle_char_assembly::character_texture_uploads(player_file, &pack, &equipped, 0)
        .context("decode player texture uploads")?;
    let band = band_texels(&uploads);
    let pool = relayout_to_monster_pool(&mut objects, &band, texture_downscale, &mut warnings)?;

    let tmd = encode(&objects).context("encode swapped TMD")?;
    Ok(MonsterizedPlayer {
        tmd,
        pool,
        warnings,
    })
}

/// The full enemy-side block swap: convert, splice into the retail block,
/// and encode the archive slot - retrying at half / quarter texture
/// resolution when the compressed stream misses the fixed `0x14000` slot.
#[derive(Debug, Clone)]
pub struct SwappedBlock {
    /// The rebuilt decoded block (retail head / entries / tail, swapped
    /// mesh + pool).
    pub block: Vec<u8>,
    /// The re-encoded `[u32 len][LZS]` archive slot, `SLOT_STRIDE` bytes.
    pub slot: Vec<u8>,
    pub warnings: Vec<String>,
}

pub fn swap_into_block(
    player_file: &[u8],
    rig: &PlayerRig,
    archive_entry: &[u8],
    target_id: u16,
) -> Result<SwappedBlock> {
    let retail_block = monster_archive::decode_block(archive_entry, target_id)?
        .ok_or_else(|| anyhow::anyhow!("monster id {target_id}: empty / filler slot"))?;
    let mut last_err = None;
    for downscale in [1u32, 2, 4] {
        let out = monsterize_player_scaled(player_file, rig, archive_entry, target_id, downscale)?;
        let block =
            monster_archive::replace_mesh_and_pool(&retail_block, Some(&out.tmd), Some(&out.pool))?;
        match monster_archive::encode_slot(&block) {
            Ok(slot) => {
                return Ok(SwappedBlock {
                    block,
                    slot,
                    warnings: out.warnings,
                });
            }
            Err(e) => last_err = Some(e),
        }
    }
    Err(last_err.unwrap_or_else(|| anyhow::anyhow!("slot encode failed"))).context(format!(
        "swap for monster id {target_id} does not fit the archive slot"
    ))
}
