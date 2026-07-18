//! glTF 2.0 (binary `.glb`) export for monster battle models.
//!
//! Packs a monster's decoded mesh, baked texture, and **every** action
//! animation into one self-contained `.glb` file - the universal interchange
//! format that carries geometry, materials, and skeletal/rigid animation
//! together (Blender, three.js, Windows 3D Viewer, …).
//!
//! ## How the game data maps onto glTF
//!
//! The engine models each monster as a set of rigid TMD **objects** (body
//! parts), all authored at the local origin, that an animation assembles into
//! a pose by giving every object a per-frame translation + Euler rotation (see
//! [`crate::monster_archive::MonsterAnimation`], decoder `FUN_8004998c`). That
//! is exactly glTF's node-animation model:
//!
//! - one **node** per TMD object, holding that object's geometry in its own
//!   local space;
//! - the node's `translation` / `rotation` are animated per frame - the rigid
//!   transform straight from the keyframe stream;
//! - a root node rotates the whole rig 180° about X so the PSX `+Y`-down /
//!   `+Z`-forward space reads upright in glTF's `+Y`-up convention (a proper
//!   rotation, so winding and normals stay correct);
//! - the node's base transform is the idle action's frame 0, so the model
//!   reads correctly even in viewers that don't autoplay an animation.
//!
//! ## Texture
//!
//! A monster's 4bpp page is shared by every part, but each prim picks one of
//! the 15 CLUTs (`cba & 0x3F`). A single glTF material can't do per-primitive
//! palette indexing, so the page is baked into a **vertical palette atlas**:
//! the page rendered once per distinct palette in use, stacked top to bottom,
//! and each vertex's `V` is remapped into its palette's band. The result is one
//! ordinary RGBA PNG + one material, with per-part colours preserved.

use crate::monster_archive::{self, CLUT_COUNT, MonsterAnimation, MonsterTexture, TEXTURE_HEIGHT};
use anyhow::Result;
use serde_json::{Value, json};
use std::io::Write;

/// Nominal keyframe playback rate baked into the exported animation timeline.
/// The retail engine advances these keyframes once per action step rather than
/// at a fixed wall-clock rate; this matches the in-browser viewer's preview
/// cadence so exported clips play back at the same speed.
const ANIM_FPS: f32 = 14.0;

/// Export monster `id`'s mesh + texture + all action animations as a binary
/// glTF (`.glb`) byte blob.
///
/// Returns `Ok(None)` for an empty / filler slot or a slot whose mesh doesn't
/// decode to any textured geometry. The returned bytes are a complete `.glb`
/// (12-byte header + JSON chunk + BIN chunk) ready to write to disk or hand to
/// a browser download.
pub fn export_glb(entry: &[u8], id: u16) -> Result<Option<Vec<u8>>> {
    let Some(monster) = monster_archive::mesh(entry, id)? else {
        return Ok(None);
    };
    let Ok(tmd) = legaia_tmd::parse(monster.tmd_bytes()) else {
        return Ok(None);
    };
    let (mesh, object_ids) =
        legaia_tmd::mesh::tmd_to_vram_mesh_with_object_ids(&tmd, monster.tmd_bytes());
    if mesh.positions.is_empty() || mesh.indices.is_empty() {
        return Ok(None);
    }
    let texture = monster.texture();
    let animations: Vec<MonsterAnimation> = monster_archive::animations(entry, id)?
        .unwrap_or_default()
        .into_iter()
        .filter(|a| a.frame_count > 1 && a.part_count > 0)
        .collect();

    let name = monster_archive::record(entry, id)?
        .map(|r| r.name)
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| format!("monster_{id}"));

    Ok(Some(build_glb(
        &name,
        &mesh,
        &object_ids,
        texture.as_ref(),
        &animations,
    )))
}

// ---------------------------------------------------------------------------
// glTF assembly
// ---------------------------------------------------------------------------

/// Accumulates the single glTF binary buffer plus the `bufferViews` / `accessors`
/// that index into it. Keeps every view 4-byte aligned. Shared with the scene
/// exporter ([`crate::scene_gltf`]).
#[derive(Default)]
pub(crate) struct BinBuilder {
    pub(crate) bin: Vec<u8>,
    pub(crate) buffer_views: Vec<Value>,
    pub(crate) accessors: Vec<Value>,
}

// glTF component types.
const COMP_U32: u32 = 5125;
const COMP_F32: u32 = 5126;
// glTF bufferView targets.
pub(crate) const TARGET_ARRAY: u32 = 34962;
pub(crate) const TARGET_ELEMENT: u32 = 34963;

impl BinBuilder {
    pub(crate) fn pad4(&mut self) {
        while !self.bin.len().is_multiple_of(4) {
            self.bin.push(0);
        }
    }

    /// Append raw bytes as a new bufferView, returning its index.
    pub(crate) fn push_view(&mut self, bytes: &[u8], target: Option<u32>) -> usize {
        self.pad4();
        let offset = self.bin.len();
        self.bin.extend_from_slice(bytes);
        let mut view = json!({ "buffer": 0, "byteOffset": offset, "byteLength": bytes.len() });
        if let Some(t) = target {
            view["target"] = json!(t);
        }
        self.buffer_views.push(view);
        self.buffer_views.len() - 1
    }

    pub(crate) fn push_accessor(&mut self, accessor: Value) -> usize {
        self.accessors.push(accessor);
        self.accessors.len() - 1
    }

    /// f32 VEC3 accessor (with min/max - required for `POSITION`).
    pub(crate) fn push_vec3(
        &mut self,
        data: &[[f32; 3]],
        target: Option<u32>,
        with_bounds: bool,
    ) -> usize {
        let mut bytes = Vec::with_capacity(data.len() * 12);
        for v in data {
            for &c in v {
                bytes.extend_from_slice(&c.to_le_bytes());
            }
        }
        let view = self.push_view(&bytes, target);
        let mut acc = json!({
            "bufferView": view, "componentType": COMP_F32, "count": data.len(), "type": "VEC3"
        });
        if with_bounds {
            let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
            for v in data {
                for i in 0..3 {
                    lo[i] = lo[i].min(v[i]);
                    hi[i] = hi[i].max(v[i]);
                }
            }
            acc["min"] = json!(lo);
            acc["max"] = json!(hi);
        }
        self.push_accessor(acc)
    }

    pub(crate) fn push_vec2(&mut self, data: &[[f32; 2]], target: Option<u32>) -> usize {
        let mut bytes = Vec::with_capacity(data.len() * 8);
        for v in data {
            for &c in v {
                bytes.extend_from_slice(&c.to_le_bytes());
            }
        }
        let view = self.push_view(&bytes, target);
        self.push_accessor(json!({
            "bufferView": view, "componentType": COMP_F32, "count": data.len(), "type": "VEC2"
        }))
    }

    pub(crate) fn push_vec4(&mut self, data: &[[f32; 4]]) -> usize {
        let mut bytes = Vec::with_capacity(data.len() * 16);
        for v in data {
            for &c in v {
                bytes.extend_from_slice(&c.to_le_bytes());
            }
        }
        let view = self.push_view(&bytes, None);
        self.push_accessor(json!({
            "bufferView": view, "componentType": COMP_F32, "count": data.len(), "type": "VEC4"
        }))
    }

    /// Scalar f32 accessor with min/max (animation sampler inputs need bounds).
    pub(crate) fn push_scalar_f32(&mut self, data: &[f32]) -> usize {
        let mut bytes = Vec::with_capacity(data.len() * 4);
        for &c in data {
            bytes.extend_from_slice(&c.to_le_bytes());
        }
        let view = self.push_view(&bytes, None);
        let lo = data.iter().copied().fold(f32::MAX, f32::min);
        let hi = data.iter().copied().fold(f32::MIN, f32::max);
        self.push_accessor(json!({
            "bufferView": view, "componentType": COMP_F32, "count": data.len(),
            "type": "SCALAR", "min": [lo], "max": [hi]
        }))
    }

    pub(crate) fn push_indices(&mut self, data: &[u32]) -> usize {
        let mut bytes = Vec::with_capacity(data.len() * 4);
        for &i in data {
            bytes.extend_from_slice(&i.to_le_bytes());
        }
        let view = self.push_view(&bytes, Some(TARGET_ELEMENT));
        self.push_accessor(json!({
            "bufferView": view, "componentType": COMP_U32, "count": data.len(), "type": "SCALAR"
        }))
    }
}

/// One TMD object's slice of the global mesh, remapped to a local vertex array.
struct ObjectGeom {
    object_id: u32,
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    uvs: Vec<[f32; 2]>,
    indices: Vec<u32>,
}

/// Partition the global mesh into per-object geometry, remapping vertex indices
/// into each object's own local array and rewriting UVs into the palette atlas.
fn partition_objects(
    mesh: &legaia_tmd::mesh::VramMesh,
    object_ids: &[u32],
    tex: Option<&MonsterTexture>,
    palette_slot: &dyn Fn(usize) -> usize,
    palette_bands: usize,
) -> Vec<ObjectGeom> {
    use std::collections::BTreeMap;
    // Group source vertices by object id, preserving order.
    let mut by_object: BTreeMap<u32, ObjectGeom> = BTreeMap::new();
    // Map global vertex index -> (object_id, local index).
    let mut local_of = vec![u32::MAX; mesh.positions.len()];

    let (tw, th) = tex
        .map(|t| (t.width as f32, t.height as f32))
        .unwrap_or((1.0, 1.0));
    // A global vertex belongs to exactly one object (prims never span objects),
    // so a populated `local_of` slot is unambiguously this object's local index.
    for tri in mesh.indices.chunks_exact(3) {
        // All three corners of a prim share one object id; key on the first.
        let oid = object_ids.get(tri[0] as usize).copied().unwrap_or(0);
        let geom = by_object.entry(oid).or_insert_with(|| ObjectGeom {
            object_id: oid,
            positions: Vec::new(),
            normals: Vec::new(),
            uvs: Vec::new(),
            indices: Vec::new(),
        });
        for &gv in tri {
            let gv = gv as usize;
            if local_of[gv] == u32::MAX {
                // Fresh local vertex for this object.
                let li = geom.positions.len() as u32;
                geom.positions.push(mesh.positions[gv]);
                geom.normals
                    .push(mesh.normals.get(gv).copied().unwrap_or([0.0; 3]));
                // Remap UV into the vertex's palette band of the atlas.
                let uv = mesh.uvs.get(gv).copied().unwrap_or([0, 0]);
                let pal = (mesh.cba_tsb.get(gv).map(|ct| ct[0]).unwrap_or(0) & 0x3F) as usize;
                let slot = palette_slot(pal) as f32;
                let u = (uv[0] as f32 + 0.5) / tw;
                let v_local = (uv[1] as f32 + 0.5) / th;
                let v = if palette_bands > 0 {
                    (slot + v_local) / palette_bands as f32
                } else {
                    v_local
                };
                geom.uvs.push([u, v]);
                local_of[gv] = li;
                geom.indices.push(li);
            } else {
                geom.indices.push(local_of[gv]);
            }
        }
    }
    by_object.into_values().collect()
}

/// Assemble the full `.glb` byte blob.
fn build_glb(
    name: &str,
    mesh: &legaia_tmd::mesh::VramMesh,
    object_ids: &[u32],
    tex: Option<&MonsterTexture>,
    animations: &[MonsterAnimation],
) -> Vec<u8> {
    let mut b = BinBuilder::default();

    // --- Texture: bake the palette atlas (one band per CLUT in use). ---
    let mut used_palettes: Vec<usize> = Vec::new();
    if tex.is_some() {
        let mut seen = [false; CLUT_COUNT];
        for ct in &mesh.cba_tsb {
            let p = (ct[0] & 0x3F) as usize;
            let p = p.min(CLUT_COUNT - 1);
            if !seen[p] {
                seen[p] = true;
                used_palettes.push(p);
            }
        }
        used_palettes.sort_unstable();
    }
    let palette_slot = |pal: usize| -> usize {
        used_palettes
            .iter()
            .position(|&p| p == pal.min(CLUT_COUNT - 1))
            .unwrap_or(0)
    };
    let bands = used_palettes.len();

    let (mut images, mut textures, mut samplers) = (Vec::new(), Vec::new(), Vec::new());
    let material = if let (Some(tex), true) = (tex, bands > 0) {
        let atlas = bake_palette_atlas(tex, &used_palettes);
        let png = rgba_to_png(tex.width, TEXTURE_HEIGHT * bands, &atlas);
        let view = b.push_view(&png, None);
        images.push(json!({ "bufferView": view, "mimeType": "image/png" }));
        // NEAREST sampling + clamp, matching the PSX point-sampled page.
        samplers
            .push(json!({ "magFilter": 9728, "minFilter": 9728, "wrapS": 33071, "wrapT": 33071 }));
        textures.push(json!({ "source": 0, "sampler": 0 }));
        json!({
            "pbrMetallicRoughness": {
                "baseColorTexture": { "index": 0 },
                "metallicFactor": 0.0, "roughnessFactor": 1.0
            },
            "alphaMode": "MASK", "alphaCutoff": 0.5, "doubleSided": true
        })
    } else {
        json!({
            "pbrMetallicRoughness": {
                "baseColorFactor": [0.8, 0.8, 0.8, 1.0],
                "metallicFactor": 0.0, "roughnessFactor": 1.0
            },
            "doubleSided": true
        })
    };

    // --- Per-object geometry -> nodes + meshes. ---
    let objects = partition_objects(mesh, object_ids, tex, &palette_slot, bands);

    // Idle frame-0 pose drives each node's base transform (so a non-animating
    // viewer still shows the rest pose, not the collapsed origin model).
    let idle = animations.first();

    let mut nodes: Vec<Value> = Vec::new();
    let mut meshes: Vec<Value> = Vec::new();
    let mut object_node_for: std::collections::BTreeMap<u32, usize> =
        std::collections::BTreeMap::new();
    let mut child_nodes: Vec<usize> = Vec::new();

    for geom in &objects {
        let pos = b.push_vec3(&geom.positions, Some(TARGET_ARRAY), true);
        let nrm = b.push_vec3(&geom.normals, Some(TARGET_ARRAY), false);
        let attrs = if tex.is_some() {
            let uv = b.push_vec2(&geom.uvs, Some(TARGET_ARRAY));
            json!({ "POSITION": pos, "NORMAL": nrm, "TEXCOORD_0": uv })
        } else {
            json!({ "POSITION": pos, "NORMAL": nrm })
        };
        let idx = b.push_indices(&geom.indices);
        meshes.push(json!({
            "primitives": [{ "attributes": attrs, "indices": idx, "material": 0, "mode": 4 }]
        }));
        let mesh_index = meshes.len() - 1;

        // Base TRS from idle frame 0 for objects the idle pose drives.
        let mut node = json!({ "name": format!("object_{}", geom.object_id), "mesh": mesh_index });
        if let Some(idle) = idle
            && let Some(frame0) = idle.frame(0)
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

    // Root node: reorient PSX -> glTF (180° about X) and parent the object nodes.
    let root_index = nodes.len();
    nodes.push(json!({
        "name": name,
        "rotation": [1.0, 0.0, 0.0, 0.0], // 180° about X
        "children": child_nodes
    }));

    // --- Animations: one glTF animation per action. ---
    let labels = monster_archive::action_labels(animations);
    let gltf_animations = build_animations(&mut b, animations, &labels, &object_node_for);

    // --- Assemble the JSON. ---
    let mut root = json!({
        "asset": { "version": "2.0", "generator": "legend-of-legaia-re monster exporter" },
        "scene": 0,
        "scenes": [{ "nodes": [root_index] }],
        "nodes": nodes,
        "meshes": meshes,
        "materials": [material],
        "accessors": b.accessors,
        "bufferViews": b.buffer_views,
        "buffers": [{ "byteLength": b.bin.len() }]
    });
    if !images.is_empty() {
        root["images"] = json!(images);
        root["textures"] = json!(textures);
        root["samplers"] = json!(samplers);
    }
    if !gltf_animations.is_empty() {
        root["animations"] = json!(gltf_animations);
    }

    pack_glb(&root, &b.bin)
}

/// Build the glTF `animations` array: a translation + rotation channel per
/// animated object node, sampled at [`ANIM_FPS`].
fn build_animations(
    b: &mut BinBuilder,
    animations: &[MonsterAnimation],
    labels: &[String],
    object_node_for: &std::collections::BTreeMap<u32, usize>,
) -> Vec<Value> {
    let mut out = Vec::new();
    for (ai, anim) in animations.iter().enumerate() {
        // Shared timeline for every channel of this animation.
        let times: Vec<f32> = (0..anim.frame_count).map(|f| f as f32 / ANIM_FPS).collect();
        let time_acc = b.push_scalar_f32(&times);

        let mut samplers: Vec<Value> = Vec::new();
        let mut channels: Vec<Value> = Vec::new();

        for part in 0..anim.part_count {
            let Some(&node) = object_node_for.get(&(part as u32)) else {
                continue; // this part has no emitted geometry
            };
            // Gather this part's per-frame translation + (sign-stabilised) rotation.
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
                // Keep consecutive quaternions on the same hemisphere so a
                // viewer's linear interpolation takes the short way round.
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
        let name = labels
            .get(ai)
            .cloned()
            .unwrap_or_else(|| format!("action_{ai}"));
        out.push(json!({ "name": name, "samplers": samplers, "channels": channels }));
    }
    out
}

/// Compose the quaternion `[x, y, z, w]` for the engine's `Rz * Ry * Rx` Euler
/// order, with each raw 12-bit angle (`4096` = a full turn) in turns.
pub(crate) fn euler_zyx_quat(rx: u16, ry: u16, rz: u16) -> [f32; 4] {
    let ang = |raw: u16| (raw as f32) * (std::f32::consts::TAU / 4096.0);
    let (hx, hy, hz) = (ang(rx) * 0.5, ang(ry) * 0.5, ang(rz) * 0.5);
    let qx = [hx.sin(), 0.0, 0.0, hx.cos()];
    let qy = [0.0, hy.sin(), 0.0, hy.cos()];
    let qz = [0.0, 0.0, hz.sin(), hz.cos()];
    quat_mul(quat_mul(qz, qy), qx)
}

/// Hamilton product of two `[x, y, z, w]` quaternions.
fn quat_mul(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    [
        a[3] * b[0] + a[0] * b[3] + a[1] * b[2] - a[2] * b[1],
        a[3] * b[1] - a[0] * b[2] + a[1] * b[3] + a[2] * b[0],
        a[3] * b[2] + a[0] * b[1] - a[1] * b[0] + a[2] * b[3],
        a[3] * b[3] - a[0] * b[0] - a[1] * b[1] - a[2] * b[2],
    ]
}

/// Bake the 4bpp page once per palette in `used`, stacked top-to-bottom into a
/// single `width × (256 * used.len())` RGBA image.
fn bake_palette_atlas(tex: &MonsterTexture, used: &[usize]) -> Vec<u8> {
    let w = tex.width;
    let mut atlas = vec![0u8; w * TEXTURE_HEIGHT * used.len() * 4];
    for (slot, &pal) in used.iter().enumerate() {
        let band = tex.to_rgba(pal); // width * 256 * 4
        let base = slot * w * TEXTURE_HEIGHT * 4;
        atlas[base..base + band.len()].copy_from_slice(&band);
    }
    atlas
}

/// Encode an RGBA8 image to in-memory PNG bytes.
pub(crate) fn rgba_to_png(width: usize, height: usize, rgba: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, width as u32, height as u32);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().expect("png header");
        writer.write_image_data(rgba).expect("png data");
    }
    out
}

/// Wrap a glTF JSON document + binary buffer into the GLB container:
/// `[magic "glTF"][version 2][total len] + JSON chunk + BIN chunk`.
pub(crate) fn pack_glb(root: &Value, bin: &[u8]) -> Vec<u8> {
    let mut json = serde_json::to_vec(root).expect("serialize glTF json");
    while !json.len().is_multiple_of(4) {
        json.push(b' '); // JSON chunk padded with spaces
    }
    let mut bin = bin.to_vec();
    while !bin.len().is_multiple_of(4) {
        bin.push(0); // BIN chunk padded with zeros
    }

    let total = 12 + 8 + json.len() + 8 + bin.len();
    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(b"glTF");
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&(total as u32).to_le_bytes());
    // JSON chunk.
    out.extend_from_slice(&(json.len() as u32).to_le_bytes());
    out.extend_from_slice(b"JSON");
    out.extend_from_slice(&json);
    // BIN chunk.
    out.extend_from_slice(&(bin.len() as u32).to_le_bytes());
    out.extend_from_slice(b"BIN\0");
    out.write_all(&bin).expect("write glb bin");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn euler_zyx_quat_identity_and_quarter_turn() {
        let id = euler_zyx_quat(0, 0, 0);
        assert!((id[3] - 1.0).abs() < 1e-6 && id[0].abs() < 1e-6);
        // Quarter turn about X = 1024/4096 -> sin/cos(45°).
        let q = euler_zyx_quat(1024, 0, 0);
        let s = std::f32::consts::FRAC_1_SQRT_2;
        assert!((q[0] - s).abs() < 1e-5 && (q[3] - s).abs() < 1e-5);
    }

    #[test]
    fn glb_header_is_well_formed() {
        // Minimal hand-built mesh: one object, one triangle, no texture.
        let mesh = legaia_tmd::mesh::VramMesh {
            positions: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            uvs: vec![[0, 0], [1, 0], [0, 1]],
            cba_tsb: vec![[0, 0], [0, 0], [0, 0]],
            indices: vec![0, 1, 2],
            normals: vec![[0.0, 0.0, 1.0]; 3],
            colors: vec![[legaia_tmd::legaia_prims::MODULATION_NEUTRAL; 3]; 3],
        };
        let glb = build_glb("test", &mesh, &[0, 0, 0], None, &[]);
        assert_eq!(&glb[0..4], b"glTF");
        assert_eq!(u32::from_le_bytes(glb[4..8].try_into().unwrap()), 2);
        let total = u32::from_le_bytes(glb[8..12].try_into().unwrap()) as usize;
        assert_eq!(total, glb.len());
        // JSON chunk parses.
        let json_len = u32::from_le_bytes(glb[12..16].try_into().unwrap()) as usize;
        assert_eq!(&glb[16..20], b"JSON");
        let json: Value = serde_json::from_slice(&glb[20..20 + json_len]).unwrap();
        assert_eq!(json["asset"]["version"], "2.0");
        assert_eq!(json["meshes"].as_array().unwrap().len(), 1);
    }
}
