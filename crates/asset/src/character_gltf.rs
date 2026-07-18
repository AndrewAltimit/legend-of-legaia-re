//! glTF 2.0 (binary `.glb`) export for assembled player battle characters
//! with their full battle-animation bank baked in.
//!
//! Combines the two sibling exporters: the **node/animation model** of
//! [`crate::monster_gltf`] (one node per rigid TMD object, per-frame TRS
//! keyframe channels straight from the keyframe stream - the engine's
//! `R_bone . v_object_local + T_bone` pose model expressed natively in glTF)
//! with the **VRAM atlas bake** of [`crate::scene_gltf`] (the assembled
//! character samples runtime VRAM through per-vertex `(cba, tsb)` pairs, an
//! indirection glTF can't express, so every distinct `(cba, tsb-page)` pair
//! becomes a 256x256 tile of one RGBA atlas and the UVs are remapped).
//!
//! The caller supplies the clips already **expanded per assembled object**
//! (`battle_char_assembly::expand_animation_for_objects`), so animation
//! channel `i` drives TMD object `i` directly - the same contract the site's
//! JS pose loop consumes. A root node rotates the rig 180 degrees about X so
//! the PSX +Y-down space reads upright in glTF's +Y-up convention.
//!
//! Consumed by the web viewer's arts page (`LegaiaArts::export_character_glb`,
//! "download this character with every battle animation") and testable
//! natively; disc-gated coverage: `crates/web-viewer/tests/arts_view_real.rs`.

use crate::monster_archive::MonsterAnimation;
use crate::monster_gltf::{BinBuilder, TARGET_ARRAY, euler_zyx_quat, pack_glb, rgba_to_png};
use crate::scene_gltf::{TILE, bake_tile, tile_key};
use legaia_tim::Vram;
use serde_json::{Value, json};
use std::collections::BTreeMap;

/// One named animation to bake: a clip already expanded per assembled object
/// (channel `i` = TMD object `i`) plus the display name and playback rate the
/// exported timeline uses. `fps` comes from the record's retail rate byte
/// (`7.5 * rate`; see `FUN_80047430` - `rate / 8` keyframes per 60 Hz tick).
pub struct CharacterClip<'a> {
    pub name: String,
    pub fps: f32,
    pub anim: &'a MonsterAnimation,
}

/// One TMD object's slice of the global mesh, remapped to a local vertex
/// array with atlas UVs.
struct ObjectGeom {
    object_id: u32,
    positions: Vec<[f32; 3]>,
    uvs: Vec<[f32; 2]>,
    indices: Vec<u32>,
}

/// Assemble the `.glb` for an assembled character + its animation bank.
/// Returns `None` when the mesh has no triangles (or samples no texture).
pub fn build_character_glb(
    name: &str,
    mesh: &legaia_tmd::mesh::VramMesh,
    object_ids: &[u32],
    vram: &Vram,
    clips: &[CharacterClip<'_>],
) -> Option<Vec<u8>> {
    if mesh.indices.is_empty() || mesh.positions.len() < 3 {
        return None;
    }

    // --- Distinct (cba, tsb-page) tiles the vertices sample -> atlas. ---
    let mut tiles: BTreeMap<(u16, u16), usize> = BTreeMap::new();
    for ct in &mesh.cba_tsb {
        let key = tile_key(ct[0], ct[1]);
        let next = tiles.len();
        tiles.entry(key).or_insert(next);
    }
    if tiles.is_empty() {
        return None;
    }
    let cols = (tiles.len() as f64).sqrt().ceil() as usize;
    let cols = cols.clamp(1, 16);
    let rows = tiles.len().div_ceil(cols);
    let (aw, ah) = (cols * TILE, rows * TILE);
    let mut atlas = vec![0u8; aw * ah * 4];
    for (&(cba, tsb), &slot) in &tiles {
        let (x0, y0) = ((slot % cols) * TILE, (slot / cols) * TILE);
        bake_tile(vram, cba, tsb, &mut atlas, aw, x0, y0);
    }

    let mut b = BinBuilder::default();
    let png = rgba_to_png(aw, ah, &atlas);
    let png_view = b.push_view(&png, None);

    // --- Partition the global mesh per object, remapping UVs into the atlas. ---
    let uv_of = |slot: usize, u: f32, v: f32| -> [f32; 2] {
        let (x0, y0) = ((slot % cols) * TILE, (slot / cols) * TILE);
        [(x0 as f32 + u) / aw as f32, (y0 as f32 + v) / ah as f32]
    };
    let mut by_object: BTreeMap<u32, ObjectGeom> = BTreeMap::new();
    let mut local_of = vec![u32::MAX; mesh.positions.len()];
    for tri in mesh.indices.chunks_exact(3) {
        // All three corners of a prim share one object id; key on the first.
        let oid = object_ids.get(tri[0] as usize).copied().unwrap_or(0);
        let geom = by_object.entry(oid).or_insert_with(|| ObjectGeom {
            object_id: oid,
            positions: Vec::new(),
            uvs: Vec::new(),
            indices: Vec::new(),
        });
        for &gv in tri {
            let gv = gv as usize;
            if local_of[gv] == u32::MAX {
                let li = geom.positions.len() as u32;
                geom.positions.push(mesh.positions[gv]);
                let uv = mesh.uvs.get(gv).copied().unwrap_or([0, 0]);
                let ct = mesh.cba_tsb.get(gv).copied().unwrap_or([0, 0]);
                let slot = tiles.get(&tile_key(ct[0], ct[1])).copied().unwrap_or(0);
                // +0.5 texel centre, matching the shader's point sampling.
                geom.uvs
                    .push(uv_of(slot, uv[0] as f32 + 0.5, uv[1] as f32 + 0.5));
                local_of[gv] = li;
                geom.indices.push(li);
            } else {
                geom.indices.push(local_of[gv]);
            }
        }
    }
    let objects: Vec<ObjectGeom> = by_object.into_values().collect();

    // --- Per-object geometry -> nodes + meshes. ---
    // The first clip (the battle idle) supplies frame-0 base transforms so a
    // viewer that doesn't autoplay still shows the assembled rest pose.
    let rest = clips.first().map(|c| c.anim);
    let mut nodes: Vec<Value> = Vec::new();
    let mut meshes: Vec<Value> = Vec::new();
    let mut object_node_for: BTreeMap<u32, usize> = BTreeMap::new();
    let mut child_nodes: Vec<usize> = Vec::new();
    for geom in &objects {
        let pos = b.push_vec3(&geom.positions, Some(TARGET_ARRAY), true);
        let uv = b.push_vec2(&geom.uvs, Some(TARGET_ARRAY));
        let idx = b.push_indices(&geom.indices);
        meshes.push(json!({
            "primitives": [{
                "attributes": { "POSITION": pos, "TEXCOORD_0": uv },
                "indices": idx, "material": 0, "mode": 4
            }]
        }));
        let mut node = json!({
            "name": format!("object_{}", geom.object_id),
            "mesh": meshes.len() - 1
        });
        if let Some(rest) = rest
            && let Some(frame0) = rest.frame(0)
            && let Some(pose) = frame0.get(geom.object_id as usize)
        {
            node["translation"] = json!([pose.tx as f32, pose.ty as f32, pose.tz as f32]);
            node["rotation"] = json!(euler_zyx_quat(pose.rx, pose.ry, pose.rz));
        }
        let node_index = nodes.len();
        nodes.push(node);
        object_node_for.insert(geom.object_id, node_index);
        child_nodes.push(node_index);
    }

    // Root: reorient PSX +Y-down -> glTF +Y-up (180 degrees about X).
    let root_index = nodes.len();
    nodes.push(json!({
        "name": name,
        "rotation": [1.0, 0.0, 0.0, 0.0],
        "children": child_nodes
    }));

    // --- Animations: one glTF animation per clip. ---
    let mut gltf_animations: Vec<Value> = Vec::new();
    for clip in clips {
        let anim = clip.anim;
        if anim.frame_count == 0 || anim.part_count == 0 {
            continue;
        }
        let fps = if clip.fps > 0.0 { clip.fps } else { 15.0 };
        let times: Vec<f32> = (0..anim.frame_count).map(|f| f as f32 / fps).collect();
        let time_acc = b.push_scalar_f32(&times);
        let mut samplers: Vec<Value> = Vec::new();
        let mut channels: Vec<Value> = Vec::new();
        for part in 0..anim.part_count {
            let Some(&node) = object_node_for.get(&(part as u32)) else {
                continue; // this channel's object emitted no geometry
            };
            let mut trans: Vec<[f32; 3]> = Vec::with_capacity(anim.frame_count);
            let mut rots: Vec<[f32; 4]> = Vec::with_capacity(anim.frame_count);
            let mut prev: Option<[f32; 4]> = None;
            for f in 0..anim.frame_count {
                let pose = anim
                    .frame(f)
                    .and_then(|fr| fr.get(part))
                    .copied()
                    .unwrap_or_default();
                trans.push([pose.tx as f32, pose.ty as f32, pose.tz as f32]);
                let mut q = euler_zyx_quat(pose.rx, pose.ry, pose.rz);
                // Keep consecutive quaternions on one hemisphere so linear
                // interpolation takes the short way round.
                if let Some(p) = prev {
                    let dot = p[0] * q[0] + p[1] * q[1] + p[2] * q[2] + p[3] * q[3];
                    if dot < 0.0 {
                        q = [-q[0], -q[1], -q[2], -q[3]];
                    }
                }
                prev = Some(q);
                rots.push(q);
            }
            let trans_acc = b.push_vec3(&trans, None, false);
            let rot_acc = b.push_vec4(&rots);
            let t_sampler = samplers.len();
            samplers
                .push(json!({ "input": time_acc, "output": trans_acc, "interpolation": "LINEAR" }));
            channels.push(
                json!({ "sampler": t_sampler, "target": { "node": node, "path": "translation" } }),
            );
            let r_sampler = samplers.len();
            samplers
                .push(json!({ "input": time_acc, "output": rot_acc, "interpolation": "LINEAR" }));
            channels.push(
                json!({ "sampler": r_sampler, "target": { "node": node, "path": "rotation" } }),
            );
        }
        if channels.is_empty() {
            continue;
        }
        gltf_animations.push(json!({
            "name": clip.name, "samplers": samplers, "channels": channels
        }));
    }

    // --- Assemble the JSON. ---
    let mut root = json!({
        "asset": { "version": "2.0", "generator": "legend-of-legaia-re character exporter" },
        "scene": 0,
        "scenes": [{ "nodes": [root_index] }],
        "nodes": nodes,
        "meshes": meshes,
        "materials": [{
            "pbrMetallicRoughness": {
                "baseColorTexture": { "index": 0 },
                "metallicFactor": 0.0, "roughnessFactor": 1.0
            },
            "alphaMode": "MASK", "alphaCutoff": 0.5, "doubleSided": true
        }],
        "images": [{ "bufferView": png_view, "mimeType": "image/png" }],
        // NEAREST + clamp, matching the PSX point-sampled pages.
        "samplers": [{ "magFilter": 9728, "minFilter": 9728, "wrapS": 33071, "wrapT": 33071 }],
        "textures": [{ "source": 0, "sampler": 0 }],
        "accessors": b.accessors,
        "bufferViews": b.buffer_views,
        "buffers": [{ "byteLength": b.bin.len() }]
    });
    if !gltf_animations.is_empty() {
        root["animations"] = json!(gltf_animations);
    }
    Some(pack_glb(&root, &b.bin))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::monster_archive::PartPose;

    fn two_object_mesh() -> (legaia_tmd::mesh::VramMesh, Vec<u32>) {
        let mesh = legaia_tmd::mesh::VramMesh {
            positions: vec![
                [0.0, 0.0, 0.0],
                [8.0, 0.0, 0.0],
                [0.0, 8.0, 0.0],
                [0.0, 0.0, 0.0],
                [4.0, 0.0, 0.0],
                [0.0, 4.0, 0.0],
            ],
            uvs: vec![[0, 0], [15, 0], [0, 15], [16, 16], [31, 16], [16, 31]],
            cba_tsb: vec![[0x1234, 5]; 6],
            indices: vec![0, 1, 2, 3, 4, 5],
            normals: vec![[0.0, 0.0, 1.0]; 6],
            colors: vec![[legaia_tmd::legaia_prims::MODULATION_NEUTRAL; 3]; 6],
        };
        (mesh, vec![0, 0, 0, 1, 1, 1])
    }

    fn clip(frames: usize) -> MonsterAnimation {
        MonsterAnimation {
            action_id: 0,
            rate: 2,
            part_count: 2,
            frame_count: frames,
            frames: (0..frames)
                .map(|f| {
                    (0..2)
                        .map(|p| PartPose {
                            tx: (f * 10 + p) as i16,
                            ty: 0,
                            tz: 0,
                            rx: 0,
                            ry: (f * 100) as u16,
                            rz: 0,
                        })
                        .collect()
                })
                .collect(),
        }
    }

    #[test]
    fn character_glb_bakes_nodes_and_named_animations() {
        let (mesh, ids) = two_object_mesh();
        let vram = Vram::new();
        let idle = clip(4);
        let art = clip(8);
        let clips = [
            CharacterClip {
                name: "battle idle".into(),
                fps: 15.0,
                anim: &idle,
            },
            CharacterClip {
                name: "Somersault (0x10)".into(),
                fps: 30.0,
                anim: &art,
            },
        ];
        let glb = build_character_glb("Vahn", &mesh, &ids, &vram, &clips).unwrap();
        assert_eq!(&glb[0..4], b"glTF");
        let total = u32::from_le_bytes(glb[8..12].try_into().unwrap()) as usize;
        assert_eq!(total, glb.len());
        let json_len = u32::from_le_bytes(glb[12..16].try_into().unwrap()) as usize;
        let root: Value = serde_json::from_slice(&glb[20..20 + json_len]).unwrap();
        // 2 object nodes + 1 root.
        assert_eq!(root["nodes"].as_array().unwrap().len(), 3);
        assert_eq!(root["meshes"].as_array().unwrap().len(), 2);
        let anims = root["animations"].as_array().unwrap();
        assert_eq!(anims.len(), 2);
        assert_eq!(anims[0]["name"], "battle idle");
        assert_eq!(anims[1]["name"], "Somersault (0x10)");
        // Each animation: translation + rotation channel per object.
        assert_eq!(anims[0]["channels"].as_array().unwrap().len(), 4);
        // Timeline duration: 8 frames at 30 fps ~= 0.233 s max input.
        let in_acc = anims[1]["samplers"][0]["input"].as_u64().unwrap() as usize;
        let max_t = root["accessors"][in_acc]["max"][0].as_f64().unwrap();
        assert!((max_t - 7.0 / 30.0).abs() < 1e-4, "max_t = {max_t}");
        // Root node reorients PSX -> glTF.
        assert_eq!(root["nodes"][2]["rotation"][0], 1.0);
    }

    #[test]
    fn empty_mesh_exports_none() {
        let vram = Vram::new();
        let mesh = legaia_tmd::mesh::VramMesh {
            positions: vec![],
            uvs: vec![],
            cba_tsb: vec![],
            indices: vec![],
            normals: vec![],
            colors: vec![],
        };
        assert!(build_character_glb("x", &mesh, &[], &vram, &[]).is_none());
    }
}
