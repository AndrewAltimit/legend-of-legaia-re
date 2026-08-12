//! Field-side of the party swap: rebuild PROT 0874 so the party's
//! walking-around (field) models depict the Delilas siblings.
//!
//! The field forms are built from the same monster-archive models the
//! battle side uses, so both forms match. The field rig is a 12-group
//! TMD per character (10 posed bones + 2 unposed equipment-template
//! groups), driven by the shared locomotion ANM bundle whose records
//! decode to the same flat per-part pose model as the battle streams -
//! so the same rest-pose bake applies, collapsed onto 10 bones
//! (torso+pelvis merge, forearm+hand merge, shin+foot merge) with a
//! **per-bone, centroid-anchored** scale: each merged source group is
//! placed at the retail field part's rest centroid and sized to its
//! extent, which keeps the chibi field proportions instead of producing
//! a lanky miniature.
//!
//! Texture: the field atlas gives each character an 80x128-texel window
//! of texpage 0x1D plus four 16-colour CLUT columns on row 478 - far
//! too small for a monster page, and retail field models are mostly
//! flat-shaded anyway (bodies are vertex-coloured; only faces carry
//! texture). The converter does the same: the **head** part stays
//! textured (its islands re-lay into the atlas window, palettes
//! union-merged to four), every other textured face converts to a
//! gouraud prim whose corner colours sample the source texture.

use super::*;
use crate::character_pack;
use crate::pack::extract_pack;
use crate::parse_player_lzs;
use crate::party_swap::playerize::{merge_palettes, monster_pool_texels, nearest_color};
use legaia_tmd::descriptor::PacketShape;
use legaia_tmd::encode::ModelPrim;

/// PROT entry the field-form container lives in.
pub const PROT_ENTRY_INDEX: usize = 874;

/// Field bones per character (the locomotion clips' channel count).
pub const FIELD_BONES: usize = 10;

/// Groups per field character slot (bones + 2 equipment templates).
pub const FIELD_GROUPS: usize = 12;

/// Field atlas geometry: per-character U window width (texels), page
/// height, CLUT columns per character.
const ATLAS_WINDOW_W: usize = 80;
const ATLAS_WINDOW_H: usize = 128;
const ATLAS_COLS_PER_CHAR: usize = 4;
/// Every field-character prim authors texpage 0x1D, ABR 1.
const FIELD_TSB: u16 = 0x003D;
/// Field CLUT row.
const FIELD_CLUT_ROW: u16 = 478;

/// Canonical battle parts feeding each field bone role.
/// Field roles: 0 head, 1 torso, 2/3 armA up/lo, 4/5 armB up/lo,
/// 6/7 legA up/lo, 8/9 legB up/lo.
const FIELD_ROLE_SOURCES: [&[usize]; FIELD_BONES] = [
    &[0],      // head
    &[1, 2],   // torso + pelvis
    &[3],      // armA upper
    &[4, 5],   // armA fore + hand
    &[6],      // armB upper
    &[7, 8],   // armB fore + hand
    &[9],      // legA thigh
    &[10, 11], // legA shin + foot
    &[12],     // legB thigh
    &[13, 14], // legB shin + foot
];

/// Derived anatomy of one retail field rig: `bone_for_role[r]` = the
/// field bone index playing canonical role `r` (see
/// [`FIELD_ROLE_SOURCES`]). Derived from rest-pose world centroids per
/// slot - the group order differs between characters (Vahn's arm pair
/// orders upper/lower differently from Noa's).
fn derive_field_roles(model: &[ModelObject], rest: &[PartPose]) -> Result<[usize; FIELD_BONES]> {
    if model.len() < FIELD_BONES || rest.len() < FIELD_BONES {
        bail!(
            "field rig has {} groups / {} channels, expected at least {FIELD_BONES}",
            model.len(),
            rest.len()
        );
    }
    #[derive(Clone, Copy)]
    struct Info {
        idx: usize,
        cx: f32,
        cy: f32,
        max_y: f32,
    }
    let mut infos = Vec::with_capacity(FIELD_BONES);
    for (i, o) in model.iter().take(FIELD_BONES).enumerate() {
        let m = rot_matrix(&rest[i]);
        let mut c = (0f32, 0f32);
        let mut max_y = f32::MIN;
        for v in &o.vertices {
            let w = apply(&m, [v[0] as f32, v[1] as f32, v[2] as f32]);
            c.0 += w[0] + rest[i].tx as f32;
            c.1 += w[1] + rest[i].ty as f32;
            max_y = max_y.max(w[1] + rest[i].ty as f32);
        }
        let n = o.vertices.len().max(1) as f32;
        infos.push(Info {
            idx: i,
            cx: c.0 / n,
            cy: c.1 / n,
            max_y,
        });
    }
    // Head = highest centroid (y-down: most negative).
    infos.sort_by(|a, b| a.cy.partial_cmp(&b.cy).unwrap());
    let head = infos[0].idx;
    // Torso = the most x-central of the rest.
    let torso = infos[1..]
        .iter()
        .min_by(|a, b| a.cx.abs().partial_cmp(&b.cx.abs()).unwrap())
        .unwrap()
        .idx;
    // Legs = the four remaining bones whose geometry reaches lowest
    // (largest max_y); arms = the other four.
    let mut rest_bones: Vec<Info> = infos
        .iter()
        .filter(|i| i.idx != head && i.idx != torso)
        .copied()
        .collect();
    rest_bones.sort_by(|a, b| b.max_y.partial_cmp(&a.max_y).unwrap());
    let (legs, arms) = rest_bones.split_at(4);
    let side = |set: &[Info], neg: bool| -> Vec<Info> {
        let mut v: Vec<Info> = set
            .iter()
            .filter(|i| (i.cx < 0.0) == neg)
            .copied()
            .collect();
        // Upper = higher centroid (more negative y).
        v.sort_by(|a, b| a.cy.partial_cmp(&b.cy).unwrap());
        v
    };
    let (arm_l, arm_r) = (side(arms, true), side(arms, false));
    let (leg_l, leg_r) = (side(legs, true), side(legs, false));
    if arm_l.len() != 2 || arm_r.len() != 2 || leg_l.len() != 2 || leg_r.len() != 2 {
        bail!(
            "field rig chains did not split 2/2/2/2 (arms {}/{}, legs {}/{})",
            arm_l.len(),
            arm_r.len(),
            leg_l.len(),
            leg_r.len()
        );
    }
    Ok([
        head,
        torso,
        arm_l[0].idx,
        arm_l[1].idx,
        arm_r[0].idx,
        arm_r[1].idx,
        leg_l[0].idx,
        leg_l[1].idx,
        leg_r[0].idx,
        leg_r[1].idx,
    ])
}

/// Rest-pose world centroid + bbox diagonal of a set of posed parts.
fn group_world_stats(parts: &[(&ModelObject, &PartPose)]) -> ((f32, f32, f32), f32) {
    let mut c = [0f64; 3];
    let mut n = 0f64;
    let mut min = [f32::MAX; 3];
    let mut max = [f32::MIN; 3];
    for (o, p) in parts {
        let m = rot_matrix(p);
        for v in &o.vertices {
            let w = apply(&m, [v[0] as f32, v[1] as f32, v[2] as f32]);
            let w = [w[0] + p.tx as f32, w[1] + p.ty as f32, w[2] + p.tz as f32];
            for k in 0..3 {
                c[k] += w[k] as f64;
                min[k] = min[k].min(w[k]);
                max[k] = max[k].max(w[k]);
            }
            n += 1.0;
        }
    }
    let n = n.max(1.0);
    let centroid = ((c[0] / n) as f32, (c[1] / n) as f32, (c[2] / n) as f32);
    let diag = ((max[0] - min[0]).powi(2) + (max[1] - min[1]).powi(2) + (max[2] - min[2]).powi(2))
        .sqrt()
        .max(1.0);
    (centroid, diag)
}

/// Bake `o` (source local frame `src`, world anchored at `c_src`) into
/// the target local frame `dst`, scaled by `s` about the source anchor
/// and re-anchored at `c_dst`:
/// `v' = R_dst^T (s (R_src v + T_src - C_src) + C_dst - T_dst)`.
fn bake_anchored(
    o: &mut ModelObject,
    src: &PartPose,
    c_src: (f32, f32, f32),
    dst: &PartPose,
    c_dst: (f32, f32, f32),
    s: f32,
) -> Result<()> {
    let ms = rot_matrix(src);
    let md = rot_matrix(dst);
    for v in o.vertices.iter_mut() {
        let w = apply(&ms, [v[0] as f32, v[1] as f32, v[2] as f32]);
        let world = [
            s * (w[0] + src.tx as f32 - c_src.0) + c_dst.0 - dst.tx as f32,
            s * (w[1] + src.ty as f32 - c_src.1) + c_dst.1 - dst.ty as f32,
            s * (w[2] + src.tz as f32 - c_src.2) + c_dst.2 - dst.tz as f32,
        ];
        let l = apply_transposed(&md, world);
        *v = [round_coord(l[0])?, round_coord(l[1])?, round_coord(l[2])?];
    }
    Ok(())
}

/// Drop prims whose baked extent falls under `threshold` (invisible at
/// the chibi field scale) and prune the vertices nothing references.
fn decimate_object(o: &mut ModelObject, threshold: f32) {
    if threshold <= 0.0 {
        return;
    }
    for g in o.groups.iter_mut() {
        g.prims.retain(|p| {
            let mut min = [i16::MAX; 3];
            let mut max = [i16::MIN; 3];
            for &vi in &p.vertices {
                let Some(v) = o.vertices.get(vi as usize) else {
                    return true;
                };
                for k in 0..3 {
                    min[k] = min[k].min(v[k]);
                    max[k] = max[k].max(v[k]);
                }
            }
            let d = (0..3)
                .map(|k| (max[k] - min[k]) as f32)
                .fold(0f32, |a, b| a.max(b));
            d >= threshold
        });
    }
    o.groups.retain(|g| !g.prims.is_empty());
    // Prune unreferenced vertices.
    let mut used = vec![false; o.vertices.len()];
    for g in &o.groups {
        for p in &g.prims {
            for &vi in &p.vertices {
                if let Some(u) = used.get_mut(vi as usize) {
                    *u = true;
                }
            }
        }
    }
    let mut remap = vec![0u16; o.vertices.len()];
    let mut kept = Vec::new();
    for (i, v) in o.vertices.iter().enumerate() {
        if used[i] {
            remap[i] = kept.len() as u16;
            kept.push(*v);
        }
    }
    o.vertices = kept;
    for g in o.groups.iter_mut() {
        for p in g.prims.iter_mut() {
            for vi in p.vertices.iter_mut() {
                *vi = remap[*vi as usize];
            }
        }
    }
}

/// Convert a textured prim into an untextured gouraud prim whose corner
/// colours sample the source page (colour = texel through its palette,
/// modulated by the prim's own colour, `0x80` = neutral).
fn flatten_prim(p: &ModelPrim, cluts: &[[u16; 16]], indices: &[u8], width: usize) -> ModelPrim {
    let sample = |uv: (u8, u8), k: usize| -> [u8; 3] {
        let (u, v) = (uv.0 as usize, uv.1 as usize);
        let idx = if u < width && v < PAGE_HEIGHT {
            indices[v * width + u] as usize
        } else {
            0
        };
        let pal = (p.cba & 0x3F) as usize;
        let c = cluts.get(pal).map_or(0, |cl| cl[idx]);
        let scale5 = |x: u16| ((x << 3) | (x >> 2)) as u32;
        let (r, g, b) = (
            scale5(c & 0x1F),
            scale5((c >> 5) & 0x1F),
            scale5((c >> 10) & 0x1F),
        );
        let mc = if p.colors.len() == p.vertices.len() {
            p.colors[k]
        } else {
            p.colors.first().copied().unwrap_or([0x80; 3])
        };
        [
            ((r * mc[0] as u32) / 0x80).min(255) as u8,
            ((g * mc[1] as u32) / 0x80).min(255) as u8,
            ((b * mc[2] as u32) / 0x80).min(255) as u8,
        ]
    };
    // One flat colour (the corner average): F3/F4 store a single colour
    // word, half the size of a gouraud prim - the field budget is tight
    // and the sprites are tiny on screen.
    let mut acc = [0u32; 3];
    for (k, &uv) in p.uvs.iter().enumerate() {
        let c = sample(uv, k);
        for (a, ch) in acc.iter_mut().zip(c) {
            *a += ch as u32;
        }
    }
    let n = p.uvs.len().max(1) as u32;
    let avg = [(acc[0] / n) as u8, (acc[1] / n) as u8, (acc[2] / n) as u8];
    ModelPrim {
        vertices: p.vertices.clone(),
        uvs: Vec::new(),
        cba: 0,
        tsb: 0,
        colors: vec![avg],
    }
}

/// The rebuilt PROT 0874 entry.
#[derive(Debug, Clone)]
pub struct FieldizedPack {
    pub entry: Vec<u8>,
    pub warnings: Vec<String>,
}

/// One character's field conversion: the sibling's model on the field
/// rig + its atlas window content.
struct FieldSlot {
    tmd: Vec<u8>,
    /// 4bpp texel indices, `ATLAS_WINDOW_W x ATLAS_WINDOW_H`.
    window: Vec<u8>,
    /// Up to four 16-colour palettes.
    palettes: Vec<Vec<u16>>,
}

fn fieldize_slot(
    pack: &character_pack::CharacterPack,
    anm: &crate::player_anm::PlayerAnmBundle,
    slot: usize,
    archive_entry: &[u8],
    source_id: u16,
    decimate: f32,
    warnings: &mut Vec<String>,
) -> Result<FieldSlot> {
    // Retail field rig + rest pose.
    let cs = pack
        .slot(slot)
        .ok_or_else(|| anyhow::anyhow!("field slot {slot} missing"))?;
    let field_tmd = legaia_tmd::parse(&cs.tmd_bytes).context("field TMD")?;
    let field_model = decode_model(&field_tmd, &cs.tmd_bytes)?;
    let idle_rec =
        character_pack::locomotion_record_index(slot, character_pack::LOCOMOTION_IDLE_SLOT);
    let idle = anm
        .record_to_monster_animation(idle_rec)
        .ok_or_else(|| anyhow::anyhow!("field slot {slot}: idle clip missing"))?;
    let field_rest = idle
        .frames
        .first()
        .ok_or_else(|| anyhow::anyhow!("field idle has no frames"))?;
    let roles = derive_field_roles(&field_model, field_rest)?;

    // Source model + rest pose (canonical Delilas order).
    let mesh = monster_archive::mesh(archive_entry, source_id)?
        .ok_or_else(|| anyhow::anyhow!("monster id {source_id}: empty slot"))?;
    let src_tmd = legaia_tmd::parse(mesh.tmd_bytes())?;
    let src_model = decode_model(&src_tmd, mesh.tmd_bytes())?;
    if src_model.len() != CANONICAL_PARTS {
        bail!("monster id {source_id} has {} parts", src_model.len());
    }
    let pool = mesh
        .texture_pool_bytes()
        .ok_or_else(|| anyhow::anyhow!("monster id {source_id}: no texture pool"))?;
    let (cluts, indices, width) = monster_pool_texels(pool)?;
    let src_idle = monster_archive::idle_animation(archive_entry, source_id)?
        .ok_or_else(|| anyhow::anyhow!("monster id {source_id}: no idle"))?;
    let src_rest = src_idle
        .frames
        .first()
        .ok_or_else(|| anyhow::anyhow!("monster idle empty"))?;

    // Per field bone: bake the mapped canonical parts, anchored at the
    // retail field part's centroid + extent.
    let mut bones: Vec<ModelObject> = Vec::with_capacity(FIELD_GROUPS);
    for _ in 0..FIELD_GROUPS {
        bones.push(ModelObject {
            vertices: Vec::new(),
            groups: Vec::new(),
            scale: legaia_tmd::encode::LEGAIA_OBJECT_SCALE,
        });
    }
    for (role, sources) in FIELD_ROLE_SOURCES.iter().enumerate() {
        let bone = roles[role];
        let dst_pose = field_rest[bone];
        let dst_parts = [(&field_model[bone], &dst_pose)];
        let (c_dst, d_dst) = group_world_stats(&dst_parts);
        let src_parts: Vec<(&ModelObject, &PartPose)> = sources
            .iter()
            .map(|&c| (&src_model[c], &src_rest[c]))
            .collect();
        let (c_src, d_src) = group_world_stats(&src_parts);
        let s = (d_dst / d_src).clamp(0.05, 2.0);
        for &c in sources.iter() {
            let mut part = src_model[c].clone();
            // Flatten every textured prim except on the head.
            if role != 0 {
                for g in part.groups.iter_mut() {
                    if !g.shape.is_textured() {
                        continue;
                    }
                    let new_shape = if g.shape.n_vertices() == 4 {
                        PacketShape::F4
                    } else {
                        PacketShape::F3
                    };
                    for p in g.prims.iter_mut() {
                        *p = flatten_prim(p, &cluts, &indices, width);
                    }
                    g.shape = new_shape;
                    g.semi_transparent = false;
                }
            }
            bake_anchored(&mut part, &src_rest[c], c_src, &dst_pose, c_dst, s)
                .with_context(|| format!("field bake role {role} canonical {c}"))?;
            compact_object(&mut part);
            merge_object(&mut bones[bone], &part);
        }
    }
    for b in bones.iter_mut() {
        compact_object(b);
        decimate_object(b, decimate);
    }

    // Head texture: re-lay its islands into the 80x128 atlas window.
    let head_bone = roles[0];
    let window = layout_head_window(
        &mut bones[head_bone],
        &cluts,
        &indices,
        width,
        slot,
        warnings,
    )?;

    let mut tmd = encode(&bones).context("encode field TMD")?;
    // The runtime equipment toggle (FUN_8001EBEC) copies template
    // descriptor 10 or 11 over group {0 Vahn, 3 Noa, 5 Gala} on every
    // equip evaluation - an empty template would vanish that body part.
    // Make both templates byte-copies of the final patched-group
    // descriptor so the toggle is a no-op.
    const PATCHED_GROUP: [usize; 3] = [0, 3, 5];
    let patched = PATCHED_GROUP.get(slot).copied().unwrap_or(0);
    let src = 0x0C + patched * 0x1C;
    let desc: [u8; 0x1C] = tmd[src..src + 0x1C].try_into().unwrap();
    for tpl in [0x0C + 10 * 0x1C, 0x0C + 11 * 0x1C] {
        tmd[tpl..tpl + 0x1C].copy_from_slice(&desc);
    }
    Ok(FieldSlot {
        tmd,
        window: window.0,
        palettes: window.1,
    })
}

/// Pack the head part's texture islands into the character's atlas
/// window; rewrites the head prims' UV/CBA/TSB in place. Returns the
/// window texels + palettes.
#[allow(clippy::type_complexity)]
fn layout_head_window(
    head: &mut ModelObject,
    cluts: &[[u16; 16]],
    indices: &[u8],
    width: usize,
    slot: usize,
    warnings: &mut Vec<String>,
) -> Result<(Vec<u8>, Vec<Vec<u16>>)> {
    let u_base = slot * ATLAS_WINDOW_W;
    let col_base = (slot * ATLAS_COLS_PER_CHAR) as u16;

    // Collect the head's textured faces + their palettes.
    let mut faces: Vec<(usize, usize, (u8, u8, u8, u8))> = Vec::new();
    let mut used: Vec<u8> = Vec::new();
    for (gi, g) in head.groups.iter().enumerate() {
        if !g.shape.is_textured() {
            continue;
        }
        for (pi, p) in g.prims.iter().enumerate() {
            faces.push((gi, pi, face_bbox(&p.uvs)));
            let pal = (p.cba & 0x3F) as u8;
            if !used.contains(&pal) {
                used.push(pal);
            }
        }
    }
    let mut window = vec![0u8; ATLAS_WINDOW_W * ATLAS_WINDOW_H];
    if faces.is_empty() {
        return Ok((window, Vec::new()));
    }
    used.sort_unstable();
    let (group_of, group_colors) = merge_palettes(&used, cluts, ATLAS_COLS_PER_CHAR, warnings)?;

    // Cluster faces by inflated-bbox overlap so shared texels pack once.
    let mut parent: Vec<usize> = (0..faces.len()).collect();
    fn find(parent: &mut Vec<usize>, i: usize) -> usize {
        if parent[i] != i {
            let r = find(parent, parent[i]);
            parent[i] = r;
        }
        parent[i]
    }
    for i in 0..faces.len() {
        for j in (i + 1)..faces.len() {
            if boxes_touch(faces[i].2, faces[j].2) {
                let (ri, rj) = (find(&mut parent, i), find(&mut parent, j));
                if ri != rj {
                    parent[ri] = rj;
                }
            }
        }
    }
    let mut clusters: BTreeMap<usize, (Vec<usize>, (u8, u8, u8, u8))> = BTreeMap::new();
    #[allow(clippy::needless_range_loop)]
    for i in 0..faces.len() {
        let root = find(&mut parent, i);
        let e = clusters.entry(root).or_insert((Vec::new(), faces[i].2));
        e.1 = union_bbox(e.1, faces[i].2);
        e.0.push(i);
    }
    let clusters: Vec<(Vec<usize>, (u8, u8, u8, u8))> = clusters.into_values().collect();

    // Shelf-pack the cluster bboxes into the window; halve on overflow.
    let mut placed: Vec<(usize, usize)> = Vec::with_capacity(clusters.len());
    let mut scale = 1usize;
    'retry: loop {
        placed.clear();
        let (mut x, mut y, mut shelf) = (0usize, 0usize, 0usize);
        for &(_, bb) in &clusters {
            let w = ((bb.2 - bb.0) as usize + 1).div_ceil(scale);
            let h = ((bb.3 - bb.1) as usize + 1).div_ceil(scale);
            if w > ATLAS_WINDOW_W || {
                if x + w > ATLAS_WINDOW_W {
                    y += shelf;
                    x = 0;
                    shelf = 0;
                }
                y + h > ATLAS_WINDOW_H
            } {
                if scale >= 8 {
                    bail!("head texture does not fit the field atlas window");
                }
                scale *= 2;
                warnings.push(format!(
                    "field head texture at 1/{scale} resolution (atlas window)"
                ));
                continue 'retry;
            }
            placed.push((x, y));
            x += w;
            shelf = shelf.max(h);
        }
        break;
    }

    for (ci, (members, bb)) in clusters.iter().enumerate() {
        let (dx, dy) = placed[ci];
        for &fi in members {
            let (gi, pi, _) = faces[fi];
            let p = &mut head.groups[gi].prims[pi];
            let src_pal = (p.cba & 0x3F) as usize;
            let group = group_of[&(src_pal as u8)];
            let colors = &group_colors[group];
            let fb = faces[fi].2;
            for sy in (fb.1 as usize..=fb.3 as usize).step_by(scale) {
                for sx in (fb.0 as usize..=fb.2 as usize).step_by(scale) {
                    let tx = dx + (sx - bb.0 as usize) / scale;
                    let ty = dy + (sy - bb.1 as usize) / scale;
                    if tx >= ATLAS_WINDOW_W || ty >= ATLAS_WINDOW_H {
                        continue;
                    }
                    let idx = if sx < width && sy < PAGE_HEIGHT {
                        indices[sy * width + sx] as usize
                    } else {
                        0
                    };
                    let color = cluts[src_pal][idx];
                    let new_idx = if idx == 0 || color == 0 {
                        0
                    } else {
                        match colors.iter().position(|&c| c == color) {
                            Some(pos) => pos + 1,
                            None => nearest_color(colors, color) + 1,
                        }
                    };
                    window[ty * ATLAS_WINDOW_W + tx] = new_idx as u8;
                }
            }
            for uv in p.uvs.iter_mut() {
                let nx = u_base + dx + (uv.0 as usize - bb.0 as usize) / scale;
                let ny = dy + (uv.1 as usize - bb.1 as usize) / scale;
                *uv = (nx.min(255) as u8, ny.min(255) as u8);
            }
            p.cba = (FIELD_CLUT_ROW << 6) | (col_base + group as u16);
            p.tsb = FIELD_TSB;
        }
    }
    Ok((window, group_colors))
}

/// Rebuild the PROT 0874 entry: slots 0..2 wear the mapped siblings'
/// models, the locomotion bundle survives verbatim, and each
/// character's atlas TIM entry repaints to the new head window.
/// `mapping[slot]` = the monster id whose model character `slot` wears.
pub fn fieldize_pack(
    prot_0874: &[u8],
    entry_len: usize,
    archive_entry: &[u8],
    mapping: [u16; 3],
) -> Result<FieldizedPack> {
    // Detail ladder: try full detail first, then progressively drop
    // prims under a size threshold (invisible at the chibi field scale)
    // until the rebuilt container fits its PROT entry.
    let mut last_err = None;
    for decimate in [0.0f32, 2.0, 3.5, 5.0, 7.0, 9.0, 12.0] {
        match fieldize_pack_at(prot_0874, entry_len, archive_entry, mapping, decimate) {
            Ok(mut out) => {
                if decimate > 0.0 {
                    out.warnings
                        .push(format!("field detail reduced (min prim size {decimate})"));
                }
                return Ok(out);
            }
            Err(e) => last_err = Some(e),
        }
    }
    Err(last_err.unwrap_or_else(|| anyhow::anyhow!("field rebuild failed")))
        .context("rebuilt PROT 0874 does not fit its entry at any detail level")
}

fn fieldize_pack_at(
    prot_0874: &[u8],
    entry_len: usize,
    archive_entry: &[u8],
    mapping: [u16; 3],
    decimate: f32,
) -> Result<FieldizedPack> {
    let mut warnings = Vec::new();
    let pack = character_pack::parse(prot_0874).context("parse PROT 0874")?;
    let anm = character_pack::field_locomotion_anm(prot_0874).context("locomotion bundle")?;
    let container = parse_player_lzs(prot_0874, character_pack::CONTAINER_DESCRIPTORS)?;

    let mut slots: Vec<FieldSlot> = Vec::with_capacity(3);
    for (slot, &source_id) in mapping.iter().enumerate() {
        slots.push(
            fieldize_slot(
                &pack,
                &anm,
                slot,
                archive_entry,
                source_id,
                decimate,
                &mut warnings,
            )
            .with_context(|| format!("field slot {slot} <- monster {source_id}"))?,
        );
    }

    // New §0: the 5-body pack with slots 0..2 replaced. Its DECODED size
    // must stay EXACTLY retail's: the battle scene loader registers the
    // container's first three header words (`meta[1]`, `type<<24|size0`,
    // `offset0`) as battle-VDF pointers off the raw entry base at every
    // battle load (`FUN_800520F0` state 0xc first loop -> `FUN_8001FBCC`),
    // and `meta[1]` doubles as the byte offset of the VDF tail that lives
    // PAST the LZS payload inside the same PROT entry. Changing either
    // word points the effect system at garbage and hangs the battle load.
    // Retail's own §0 carries ~19 KB of trailing pack padding, so padding
    // to the exact size is the retail shape.
    let sec0 = &container.descriptors[character_pack::CONTAINER_SECTION];
    let sec0_decoded = crate::decode(prot_0874, sec0, crate::DecodeMode::Lzs)?;
    let bodies = extract_pack(&sec0_decoded)?;
    let mut new_bodies: Vec<Vec<u8>> = Vec::with_capacity(bodies.len());
    for (i, b) in bodies.iter().enumerate() {
        if i < 3 {
            new_bodies.push(slots[i].tmd.clone());
        } else {
            new_bodies.push(b.to_vec());
        }
    }
    let mut sec0_new = Vec::new();
    sec0_new.extend_from_slice(&(new_bodies.len() as u32).to_le_bytes());
    let table_words = 1 + new_bodies.len();
    let mut cursor = table_words;
    let mut offsets = Vec::with_capacity(new_bodies.len());
    for b in &new_bodies {
        offsets.push(cursor as u32);
        cursor += b.len().div_ceil(4);
    }
    for off in &offsets {
        sec0_new.extend_from_slice(&off.to_le_bytes());
    }
    for b in &new_bodies {
        sec0_new.extend_from_slice(b);
        while sec0_new.len() % 4 != 0 {
            sec0_new.push(0);
        }
    }
    if sec0_new.len() > sec0_decoded.len() {
        bail!(
            "rebuilt §0 pack decodes to {} bytes, retail's {} is a hard cap \
             (battle-VDF header words must stay byte-exact)",
            sec0_new.len(),
            sec0_decoded.len()
        );
    }
    sec0_new.resize(sec0_decoded.len(), 0);

    // New §2: the atlas with each character entry's pixels + CLUT
    // repainted in place (same TIM sizes).
    let sec2 = &container.descriptors[2];
    let mut sec2_new = crate::decode(prot_0874, sec2, crate::DecodeMode::Lzs)?;
    patch_atlas(&mut sec2_new, &slots)?;

    // §1 (locomotion) survives as its original compressed byte span.
    let sec1 = &container.descriptors[1];
    let spans: Vec<(usize, usize)> = {
        let mut offs: Vec<usize> = container
            .descriptors
            .iter()
            .map(|d| d.data_offset as usize)
            .collect();
        offs.push(entry_len);
        (0..container.descriptors.len())
            .map(|i| (offs[i], offs[i + 1]))
            .collect()
    };
    let sec1_raw = prot_0874
        .get(spans[1].0..spans[1].1.min(prot_0874.len()))
        .ok_or_else(|| anyhow::anyhow!("section 1 span out of range"))?
        .to_vec();

    // Reassemble the container: header + pairs + section data. Greedy
    // LZS first; the optimal parse only when the entry budget misses.
    let mut s0 = legaia_lzs::compress(&sec0_new);
    let mut s2 = legaia_lzs::compress(&sec2_new);
    let header_and_slack = 0x20 + 8;
    if header_and_slack + s0.len() + sec1_raw.len() + s2.len() > entry_len {
        s0 = legaia_lzs::compress_optimal(&sec0_new);
        s2 = legaia_lzs::compress_optimal(&sec2_new);
    }
    if std::env::var("LEGAIA_FIELDIZE_DEBUG").is_ok() {
        eprintln!(
            "[fieldize] decimate {decimate}: sec0 {} (lzs {}), sec1 raw {}, sec2 {} (lzs {}), entry {}",
            sec0_new.len(),
            s0.len(),
            sec1_raw.len(),
            sec2_new.len(),
            s2.len(),
            entry_len
        );
    }
    let streams = [s0, sec1_raw, s2];
    let sizes = [sec0_new.len() as u32, sec1.size, sec2_new.len() as u32];
    let types = [sec0.type_byte, sec1.type_byte, sec2.type_byte];
    let mut out = Vec::with_capacity(entry_len);
    out.extend_from_slice(&container.meta[0].to_le_bytes());
    // meta[1] = total decompressed budget (sum of the three sizes).
    let meta1: u32 = sizes.iter().sum();
    out.extend_from_slice(&meta1.to_le_bytes());
    let header_len = 8 + 3 * 8;
    let mut offset = header_len.max(container.descriptors[0].data_offset as usize);
    let mut pairs = Vec::new();
    for i in 0..3 {
        pairs.push(((types[i] as u32) << 24 | sizes[i], offset as u32));
        offset += streams[i].len();
        offset = offset.div_ceil(4) * 4;
    }
    for (ts, off) in &pairs {
        out.extend_from_slice(&ts.to_le_bytes());
        out.extend_from_slice(&off.to_le_bytes());
    }
    for (i, s) in streams.iter().enumerate() {
        let start = pairs[i].1 as usize;
        if out.len() > start {
            bail!("container section {i} overlaps the previous one");
        }
        out.resize(start, 0);
        out.extend_from_slice(s);
    }
    if out.len() > entry_len {
        bail!(
            "rebuilt PROT 0874 is {} bytes, entry holds {}",
            out.len(),
            entry_len
        );
    }
    out.resize(entry_len, 0);
    Ok(FieldizedPack {
        entry: out,
        warnings,
    })
}

/// Repaint each character's atlas TIM (entries 1..=3 of §2) with the new
/// head window + palettes, in place inside the decoded §2 pack bytes.
fn patch_atlas(sec2: &mut [u8], slots: &[FieldSlot]) -> Result<()> {
    let entries = crate::pack::parse_pack(sec2)?;
    for (slot, fs) in slots.iter().enumerate() {
        let e = entries
            .get(slot + 1)
            .ok_or_else(|| anyhow::anyhow!("atlas entry {} missing", slot + 1))?;
        let (start, len) = (e.byte_offset, e.size);
        let tim = sec2
            .get_mut(start..start + len)
            .ok_or_else(|| anyhow::anyhow!("atlas entry {} out of range", slot + 1))?;
        patch_tim(tim, fs).with_context(|| format!("atlas entry {}", slot + 1))?;
    }
    Ok(())
}

/// Patch one 4bpp CLUT'd TIM in place: CLUT entries (4 x 16 colours) and
/// pixel data (20 halfwords x 128 rows) replaced, geometry untouched.
/// TIM layout: `[u32 0x10][u32 flags][clut block][image block]`, each
/// block `[u32 len][u16 x][u16 y][u16 w][u16 h][data]`.
fn patch_tim(tim: &mut [u8], fs: &FieldSlot) -> Result<()> {
    let magic = u32::from_le_bytes(tim.get(0..4).unwrap_or(&[0; 4]).try_into().unwrap());
    let flags = u32::from_le_bytes(tim.get(4..8).unwrap_or(&[0; 4]).try_into().unwrap());
    if magic != 0x10 || flags & 0x8 == 0 {
        bail!("not a CLUT'd TIM (magic {magic:#x}, flags {flags:#x})");
    }
    let clut_len = u32::from_le_bytes(tim[8..12].try_into().unwrap()) as usize;
    let clut_w = u16::from_le_bytes(tim[16..18].try_into().unwrap()) as usize;
    let clut_h = u16::from_le_bytes(tim[18..20].try_into().unwrap()) as usize;
    let clut_data = 20;
    let clut_n = clut_w * clut_h;
    if clut_n != ATLAS_COLS_PER_CHAR * 16 {
        bail!("atlas CLUT holds {clut_n} colours, expected 64");
    }
    let dst = tim
        .get_mut(clut_data..clut_data + clut_n * 2)
        .ok_or_else(|| anyhow::anyhow!("CLUT data out of range"))?;
    dst.fill(0);
    for (g, colors) in fs.palettes.iter().enumerate().take(ATLAS_COLS_PER_CHAR) {
        for (i, &c) in colors.iter().enumerate().take(15) {
            let at = (g * 16 + 1 + i) * 2;
            dst[at..at + 2].copy_from_slice(&c.to_le_bytes());
        }
    }
    // Image block follows the CLUT block (block len covers len..data end).
    let img = 8 + clut_len;
    let img_w = u16::from_le_bytes(
        tim.get(img + 8..img + 10)
            .ok_or_else(|| anyhow::anyhow!("image block out of range"))?
            .try_into()
            .unwrap(),
    ) as usize;
    let img_h = u16::from_le_bytes(tim[img + 10..img + 12].try_into().unwrap()) as usize;
    if img_w * 2 * 2 != ATLAS_WINDOW_W || img_h != ATLAS_WINDOW_H {
        bail!("atlas image is {img_w}hw x {img_h}, expected 20x128");
    }
    let data = img + 12;
    let dst = tim
        .get_mut(data..data + img_w * 2 * img_h)
        .ok_or_else(|| anyhow::anyhow!("image data out of range"))?;
    for yy in 0..ATLAS_WINDOW_H {
        for xb in 0..ATLAS_WINDOW_W / 2 {
            let lo = fs.window[yy * ATLAS_WINDOW_W + xb * 2] & 0xF;
            let hi = fs.window[yy * ATLAS_WINDOW_W + xb * 2 + 1] & 0xF;
            dst[yy * (ATLAS_WINDOW_W / 2) + xb] = lo | (hi << 4);
        }
    }
    Ok(())
}
