//! Building one field slot: the slot and source records, the monster slot
//! source, boundary sealing and the per-slot fieldize pass.

use super::super::*;
use super::atlas::{layout_head_window, template_box, template_fill_color};
use super::decimate::{decimate_object, flatten_prim};
use super::roles::{
    FIELD_CHILD, FIELD_PARENT, FIELD_ROLE_SOURCES, derive_field_roles, group_world_stats,
};
use super::{FIELD_BONES, FIELD_GROUPS};
use crate::playerize::monster_pool_texels;
use legaia_actor_formats::character_pack;
use legaia_tmd::descriptor::PacketShape;
use legaia_tmd::encode::ModelPrim;

/// One character's field conversion: the sibling's model on the field
/// rig + its atlas window content.
pub(super) struct FieldSlot {
    pub(super) tmd: Vec<u8>,
    /// 4bpp texel indices, `ATLAS_WINDOW_W x ATLAS_WINDOW_H`.
    pub(super) window: Vec<u8>,
    /// Up to four 16-colour palettes.
    pub(super) palettes: Vec<Vec<u16>>,
}

/// A field-conversion source: a posed multi-part model plus the texel
/// space its textured prims sample (`cba & 0x3F` indexes `cluts`, UVs
/// index `indices` as a `width x PAGE_HEIGHT` 4bpp page). Built from
/// either the battle monster archive or the siblings' own field NPC
/// meshes ([`npc_slot_source`]).
pub(super) struct SlotSource {
    pub(super) model: Vec<ModelObject>,
    pub(super) rest: Vec<PartPose>,
    /// Canonical field role -> source part indices.
    pub(super) role_sources: [Vec<usize>; FIELD_BONES],
    pub(super) cluts: Vec<[u16; 16]>,
    pub(super) indices: Vec<u8>,
    pub(super) width: usize,
}

/// Source from the monster archive (battle model, 15 canonical parts
/// merged onto the 10 field roles).
pub(super) fn monster_slot_source(archive_entry: &[u8], source_id: u16) -> Result<SlotSource> {
    let mesh = monster_archive::mesh(archive_entry, source_id)?
        .ok_or_else(|| anyhow::anyhow!("monster id {source_id}: empty slot"))?;
    let src_tmd = legaia_tmd::parse(mesh.tmd_bytes())?;
    let model = decode_model(&src_tmd, mesh.tmd_bytes())?;
    if model.len() != CANONICAL_PARTS {
        bail!("monster id {source_id} has {} parts", model.len());
    }
    let pool = mesh
        .texture_pool_bytes()
        .ok_or_else(|| anyhow::anyhow!("monster id {source_id}: no texture pool"))?;
    let (cluts, indices, width) = monster_pool_texels(pool)?;
    let src_idle = monster_archive::idle_animation(archive_entry, source_id)?
        .ok_or_else(|| anyhow::anyhow!("monster id {source_id}: no idle"))?;
    let rest = src_idle
        .frames
        .first()
        .ok_or_else(|| anyhow::anyhow!("monster idle empty"))?
        .clone();
    Ok(SlotSource {
        model,
        rest,
        role_sources: FIELD_ROLE_SOURCES.map(|s| s.to_vec()),
        cluts,
        indices,
        width,
    })
}

/// Close a part's OPEN boundary loops with flat fan fills. The NPC
/// meshes leave genuine openings (the hair shell's underside, regions
/// the fixed scene camera never exposed) that read as missing polygons
/// from the field camera's free angles - no winding trick can draw a
/// polygon that does not exist. A boundary edge is an undirected
/// triangle edge used exactly once; each closed loop of them gains a
/// centroid vertex + an F3 fan in the part's fill colour. Winding is
/// arbitrary - the head is double-sided afterwards.
fn seal_boundaries(o: &mut ModelObject, color: [u8; 3]) {
    use std::collections::{BTreeMap, BTreeSet};
    let key = |a: u16, b: u16| (a.min(b), a.max(b));
    let mut count: BTreeMap<(u16, u16), usize> = BTreeMap::new();
    for g in &o.groups {
        for p in &g.prims {
            let v = &p.vertices;
            let tris: Vec<[u16; 3]> = match v.len() {
                3 => vec![[v[0], v[1], v[2]]],
                4 => vec![[v[0], v[1], v[2]], [v[1], v[3], v[2]]],
                _ => continue,
            };
            for t in tris {
                for e in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
                    *count.entry(key(e.0, e.1)).or_default() += 1;
                }
            }
        }
    }
    let boundary: Vec<(u16, u16)> = count
        .iter()
        .filter(|&(_, &c)| c == 1)
        .map(|(&e, _)| e)
        .collect();
    if boundary.is_empty() {
        return;
    }
    let mut adj: BTreeMap<u16, Vec<u16>> = BTreeMap::new();
    for &(a, b) in &boundary {
        adj.entry(a).or_default().push(b);
        adj.entry(b).or_default().push(a);
    }
    let mut used: BTreeSet<(u16, u16)> = BTreeSet::new();
    let mut loops: Vec<Vec<u16>> = Vec::new();
    for &(a0, b0) in &boundary {
        if used.contains(&key(a0, b0)) {
            continue;
        }
        used.insert(key(a0, b0));
        let mut path = vec![a0, b0];
        loop {
            let cur = *path.last().unwrap();
            let prev = path[path.len() - 2];
            let next = adj[&cur]
                .iter()
                .copied()
                .find(|&n| n != prev && !used.contains(&key(cur, n)));
            let Some(next) = next else { break };
            used.insert(key(cur, next));
            if next == path[0] {
                loops.push(path.clone());
                break;
            }
            path.push(next);
        }
    }
    for lv in loops {
        if lv.len() < 3 {
            continue;
        }
        let mut c = [0f32; 3];
        for &vi in &lv {
            for (k, ck) in c.iter_mut().enumerate() {
                *ck += o.vertices[vi as usize][k] as f32;
            }
        }
        let n = lv.len() as f32;
        let ci = o.vertices.len() as u16;
        o.vertices.push([
            (c[0] / n).round() as i16,
            (c[1] / n).round() as i16,
            (c[2] / n).round() as i16,
        ]);
        let prims: Vec<ModelPrim> = (0..lv.len())
            .map(|w| ModelPrim {
                vertices: vec![ci, lv[w], lv[(w + 1) % lv.len()]],
                uvs: Vec::new(),
                cba: 0,
                tsb: 0,
                colors: vec![color],
            })
            .collect();
        o.groups.push(ModelGroup {
            shape: PacketShape::F3,
            semi_transparent: false,
            prims,
        });
    }
}

/// How much of the rig gets the reversed-twin treatment - the doubled
/// geometry must still fit the container's pinned decoded budget, so a
/// ladder tries progressively smaller scopes (the head is where a
/// culled hole is most visible: the textured face).
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum DoubleSideScope {
    All,
    HeadTorso,
    Head,
    None,
}

pub(super) fn fieldize_slot(
    pack: &character_pack::CharacterPack,
    anm: &legaia_actor_formats::player_anm::PlayerAnmBundle,
    slot: usize,
    source: &SlotSource,
    decimate: f32,
    double_sided: DoubleSideScope,
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

    let (src_model, src_rest) = (&source.model, &source.rest);
    let (cluts, indices, width) = (&source.cluts, &source.indices, source.width);

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
    // Pivot-anchored bake (see `party_swap::bake_object_pivot`): each
    // role's geometry is expressed relative to its source bone's rest
    // pivot, re-aimed into the retail field bone's rest frame, and
    // scaled axially so the far end lands on the retail child joint -
    // the locomotion clips rotate each bone about its pivot, so this is
    // what keeps the walking chains (and the neck) closed. The radial
    // scale is the uniform whole-rig height ratio, so the sibling's own
    // proportions survive.
    let radial = {
        let dst_parts: Vec<(&ModelObject, &PartPose)> = roles
            .iter()
            .map(|&b| (&field_model[b], &field_rest[b]))
            .collect();
        let src_parts: Vec<(&ModelObject, &PartPose)> = source
            .role_sources
            .iter()
            .flatten()
            .map(|&c| (&src_model[c], &src_rest[c]))
            .collect();
        let (_, e_dst) = group_world_stats(&dst_parts);
        let (_, e_src) = group_world_stats(&src_parts);
        (e_dst[1] / e_src[1]).clamp(0.25, 4.0)
    };
    let pivot_of = |p: &PartPose| [p.tx as f32, p.ty as f32, p.tz as f32];
    let src_role_pivots: Vec<[f32; 3]> = source
        .role_sources
        .iter()
        .map(|s| pivot_of(&src_rest[s[0]]))
        .collect();
    let dst_role_pivots: Vec<[f32; 3]> = roles.iter().map(|&b| pivot_of(&field_rest[b])).collect();
    let src_frames = bone_frames(&src_role_pivots, &FIELD_CHILD, &FIELD_PARENT);
    let dst_frames = bone_frames(&dst_role_pivots, &FIELD_CHILD, &FIELD_PARENT);
    for (role, sources) in source.role_sources.iter().enumerate() {
        let bone = roles[role];
        let dst_pose = field_rest[bone];
        let pb = pivot_bake_params(&src_frames[role], &dst_frames[role], radial);
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
                        *p = flatten_prim(p, cluts, indices, width);
                    }
                    g.shape = new_shape;
                    g.semi_transparent = false;
                }
            }
            bake_object_pivot(
                &mut part,
                &src_rest[c],
                src_role_pivots[role],
                &dst_pose,
                &pb,
            )
            .with_context(|| format!("field bake role {role} canonical {c}"))?;
            if role == 0 {
                // Seat the head on the neck (near edge along the bone
                // axis lands where the retail head's sat).
                seat_terminal_axial(&mut part, &field_model[bone], &dst_pose, pb.x_dst)?;
            }
            compact_object(&mut part);
            merge_object(&mut bones[bone], &part);
        }
    }
    for b in bones.iter_mut() {
        compact_object(b);
        decimate_object(b, decimate);
    }

    // Head texture: re-lay its islands into the 80x128 atlas window.
    // The NPC source authors some face prims semi-transparent (scene
    // ABR context); in the party atlas context ABE blends them against
    // the field background - a see-through face. Retail field heads are
    // opaque; clear the flag.
    let head_bone = roles[0];
    for g in bones[head_bone].groups.iter_mut() {
        g.semi_transparent = false;
    }
    // Close the head's real openings (hair underside) before the UV
    // relayout - the fills are flat prims the layout never touches.
    let fill = template_fill_color(&bones[head_bone], cluts);
    seal_boundaries(&mut bones[head_bone], fill);
    let window = layout_head_window(&mut bones[head_bone], cluts, indices, width, slot, warnings)?;

    // Equipment templates (groups 10/11). Retail's are LOW-POLY
    // world-map variants of group {0 Vahn, 3 Noa, 5 Gala}: the world-map
    // walker's equip toggle (FUN_8001EBEC) copies one of them over that
    // group, while the FIELD renderer draws all 12 objects - the
    // templates render unposed at the actor origin, so they must be
    // SMALL like retail's (16-vert bits that hide at the feet). The
    // first ship made them byte-copies of the full patched group, which
    // drew a second unposed HEAD riding the walker on the Vahn slot
    // (whose patched group is the head). Emit a tiny flat-colored box
    // spanning the patched group's local bbox instead: harmless in the
    // field, a plausible low-poly stand-in on the world map.
    const PATCHED_GROUP: [usize; 3] = [0, 3, 5];
    let patched = PATCHED_GROUP.get(slot).copied().unwrap_or(0);
    let template = template_box(&bones[patched], cluts, indices, width);
    bones[10] = template.clone();
    bones[11] = template;

    let ds_bones: Vec<usize> = match double_sided {
        DoubleSideScope::All => roles.to_vec(),
        DoubleSideScope::HeadTorso => vec![roles[0], roles[1]],
        DoubleSideScope::Head => vec![roles[0]],
        DoubleSideScope::None => Vec::new(),
    };
    for b in ds_bones {
        double_side(&mut bones[b]);
    }

    let tmd = encode(&bones).context("encode field TMD")?;
    Ok(FieldSlot {
        tmd,
        window: window.0,
        palettes: window.1,
    })
}
