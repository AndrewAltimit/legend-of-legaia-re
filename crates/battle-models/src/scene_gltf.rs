//! glTF 2.0 (binary `.glb`) export for assembled VRAM-textured scenes.
//!
//! The scene renderers (the site's world-overview page, the asset viewer's
//! full-map mode, and the single-entry TMD inspector) all draw the same shape
//! of data: meshes whose vertices carry PSX **page-local texel UVs** plus a
//! per-vertex `(cba, tsb)` pair, sampled against the 1024x512 VRAM image in
//! the fragment shader (4bpp / 8bpp CLUT lookup or 15bpp direct). That
//! indirection has no glTF equivalent, so exporting bakes it out:
//!
//! - every distinct `(cba, tsb-page)` combination referenced by a vertex is
//!   rendered once from VRAM into a 256x256 RGBA **tile**, and the tiles are
//!   packed into one square-ish atlas (one PNG, one material);
//! - each vertex's texel UV is remapped into its tile's cell of the atlas;
//! - every vertex of a mesh that supplies the `flat_rgba` side channel keeps
//!   its packet colour through `COLOR_0`, on **both** halves: an untextured
//!   flat/gouraud vertex as a fill (pointed at a dedicated all-white tile, so
//!   one material still covers the whole scene) and a textured one as the
//!   `texel * colour / 128` modulation the page's shader applies. The two use
//!   different divisors - see [`crate::gltf_color`];
//! - instances become glTF nodes sharing mesh data, with the exact
//!   translation / Y-rotation / uniform scale the on-screen renderer applies.
//!
//! ## Coordinate convention
//!
//! Mesh-local geometry is stored PSX-style (+Y down). The site renderers
//! flip Y in the per-placement model matrix (`diag(s, -s, s)`); the export
//! bakes the same flip into the mesh geometry (negating local Y) so instance
//! transforms stay plain TRS and the model reads +Y-up in any glTF viewer -
//! matching what the page shows, mirror-handedness and all. (The Y-flip
//! commutes with a Y-rotation, so baking it into the vertices is exact.)
//! Instance yaw needs one conversion: [`SceneInstance::rot_y`] carries the
//! page's `placementModelScaledY` param, whose inline rotY block is the
//! transpose of a standard +Y rotation - the emitted node quaternion is
//! therefore `Ry(-rot_y)` (see [`quat_y`]).
//!
//! PSX transparency follows the fragment shader: a BGR555 word of `0` bakes
//! as fully transparent (alpha 0) and the material uses `MASK` alpha, so
//! cutout foliage / grates export as cutouts.
//!
//! Semi-transparent (ABE) prims split into **one material per PSX blend
//! rate** (the ABR field, TSB bits 5..6): `legaia_semi_abr0` = `B/2 + F/2`,
//! `abr1` = `B + F` (additive - window light shafts, glows), `abr2` =
//! `B - F`, `abr3` = `B + F/4`. Core glTF can only express alpha blending,
//! so every semi material ships `alphaMode: BLEND` (a depth-test-only
//! transparent queue is also what keeps retail's coincident-plane scroll
//! layers - the sea's stacked sheets - from z-fighting in depth-writing
//! importers) with the rate approximated in the base-colour alpha; the
//! material **name** is the contract an importer restores the real blend
//! state from (the Unity kit maps `abr1`/`abr3` onto an additive shader -
//! rendered as plain alpha blend they read as a grey film, not a glow).
//! The ABE flag rides each vertex's TSB bit 15
//! (`legaia_tmd::mesh::TSB_SEMI_TRANSPARENT_BIT`) on both hybrid halves;
//! [`tile_key`] masks both it and the ABR bits off, so atlas tiling is
//! unaffected.

use crate::gltf_color;
use crate::monster_gltf::{BinBuilder, TARGET_ARRAY, pack_glb, rgba_to_png};
use legaia_tim::Vram;
use serde_json::{Value, json};
use std::collections::BTreeMap;

/// Atlas tile edge in texels - one full PSX texture page window (u8 UVs
/// address 0..255 within the page picked by the vertex's `tsb`).
pub(crate) const TILE: usize = 256;

/// One reusable mesh: the exact vertex streams the WebGL scene path renders
/// (`positions` f32 xyz in PSX space, `uvs` u8 page-local texel pairs,
/// `cba_tsb` u16 `[cba, tsb]` pairs, triangle-list `indices`).
///
/// `flat_rgba` is empty when the caller has no packet-colour stream, else 4
/// bytes `[r, g, b, flag]` per vertex - the exact `a_flat_rgba` attribute the
/// site's shader reads, flag `0` = untextured (the colour is the prim's fill)
/// and `255` = textured (the colour modulates the texel). Passing it is what
/// keeps the exported shading equal to the page's; leaving it empty exports
/// the raw texels.
pub struct SceneMesh {
    pub name: String,
    pub positions: Vec<f32>,
    pub uvs: Vec<u8>,
    pub cba_tsb: Vec<u16>,
    pub indices: Vec<u32>,
    pub flat_rgba: Vec<u8>,
    /// Morph targets (glTF `targets`, Unity blendshapes): per-vertex position
    /// deltas in the same PSX-frame layout as `positions` (the bake applies
    /// the same Y flip). Empty for the ordinary static mesh. The engine's
    /// VDF vertex-morph lanes export through this - one target per lane.
    pub morph_targets: Vec<SceneMorphTarget>,
}

/// One morph target of a [`SceneMesh`]: a named per-vertex position-delta
/// stream (`deltas.len() == positions.len()`).
pub struct SceneMorphTarget {
    pub name: String,
    pub deltas: Vec<f32>,
}

/// A sampled morph-weight animation over the scene's instances: every node
/// whose mesh has a track gets a glTF `weights` channel. `times` are seconds;
/// `tracks[mesh_index]` is key-major flattened weights
/// (`times.len() * mesh.morph_targets.len()` values in `0.0..=1.0`).
pub struct SceneWeightsAnim {
    pub name: String,
    pub times: Vec<f32>,
    pub tracks: BTreeMap<usize, Vec<f32>>,
}

/// One placement of a [`SceneMesh`]: the same `(translation, rot_y, scale)`
/// triple the page's `placementModelScaledY` builds its model matrix from
/// (`translation` in the renderer's world frame, `rot_y` in radians about
/// +Y, uniform `scale`).
pub struct SceneInstance {
    pub mesh: usize,
    pub translation: [f32; 3],
    pub rot_y: f32,
    pub scale: f32,
}

/// Key of one baked atlas tile. `cba` selects the CLUT row/column, `tsb` is
/// masked to the bits the sampler actually reads (page x/y + depth), and
/// `cba` is zeroed for 15bpp pages (no CLUT involved) so direct-colour tiles
/// dedupe across CLUT bits.
pub(crate) fn tile_key(cba: u16, tsb: u16) -> (u16, u16) {
    let tsb = tsb & 0x019F; // bits 0-3 page x, bit 4 page y, bits 7-8 depth
    let depth = (tsb >> 7) & 3;
    if depth >= 2 { (0, tsb) } else { (cba, tsb) }
}

/// Decode one texel the way the site's fragment shader does: page origin
/// from `tsb`, 4/8bpp CLUT indirection via `cba` (or 15bpp direct), BGR555
/// to RGBA with word `0` = fully transparent.
pub(crate) fn bake_tile(
    vram: &Vram,
    cba: u16,
    tsb: u16,
    out: &mut [u8],
    out_w: usize,
    x0: usize,
    y0: usize,
) {
    let tpage_x = ((tsb & 0xF) as usize) * 64;
    let tpage_y = (((tsb >> 4) & 1) as usize) * 256;
    let depth = (tsb >> 7) & 3;
    let clut_x = ((cba & 0x3F) as usize) * 16;
    let clut_y = ((cba >> 6) & 0x1FF) as usize;
    for v in 0..TILE {
        for u in 0..TILE {
            let word = match depth {
                0 => {
                    let w = vram.pixel(tpage_x + (u >> 2), tpage_y + v);
                    let idx = (w >> ((u & 3) * 4)) & 0xF;
                    vram.pixel(clut_x + idx as usize, clut_y)
                }
                1 => {
                    let w = vram.pixel(tpage_x + (u >> 1), tpage_y + v);
                    let idx = (w >> ((u & 1) * 8)) & 0xFF;
                    vram.pixel(clut_x + idx as usize, clut_y)
                }
                _ => vram.pixel(tpage_x + u, tpage_y + v),
            };
            let o = ((y0 + v) * out_w + (x0 + u)) * 4;
            // 5-bit channel -> 8-bit, matching the shader's /31.0 scaling.
            out[o] = (((word & 0x1F) * 255 + 15) / 31) as u8;
            out[o + 1] = ((((word >> 5) & 0x1F) * 255 + 15) / 31) as u8;
            out[o + 2] = ((((word >> 10) & 0x1F) * 255 + 15) / 31) as u8;
            out[o + 3] = if word == 0 { 0 } else { 255 };
        }
    }
}

/// Quaternion `[x, y, z, w]` for a **standard** rotation of `a` radians
/// about +Y (`[c,0,s; 0,1,0; -s,0,c]` acting on column vectors).
///
/// This is the TRANSPOSE of the inline rotY block in the page's
/// `placementModelScaledY` (whose column-major literal reads as
/// `Ry(-param)`), so a [`SceneInstance::rot_y`] - which carries the page
/// param by contract - must be **negated** at the emission site to
/// reproduce the page's facing. Emitting `quat_y(rot_y)` unnegated is the
/// bug that faced every yawed building the wrong way in exported worlds
/// while leaving positions (and therefore layout comparisons) untouched.
fn quat_y(a: f32) -> [f32; 4] {
    let h = a * 0.5;
    [0.0, h.sin(), 0.0, h.cos()]
}

/// Assemble the `.glb` for an instanced scene. Returns `None` when no
/// instance references a mesh with any triangles.
pub fn build_scene_glb(
    name: &str,
    meshes: &[SceneMesh],
    instances: &[SceneInstance],
    vram: &Vram,
) -> Option<Vec<u8>> {
    build_scene_glb_animated(name, meshes, instances, vram, None)
}

/// [`build_scene_glb`] plus an optional morph-weight animation: meshes with
/// [`SceneMesh::morph_targets`] emit glTF `targets` (Unity blendshapes), and
/// `anim`'s tracks become one looping `weights` channel per instance node of
/// each tracked mesh.
pub fn build_scene_glb_animated(
    name: &str,
    meshes: &[SceneMesh],
    instances: &[SceneInstance],
    vram: &Vram,
    anim: Option<&SceneWeightsAnim>,
) -> Option<Vec<u8>> {
    // Which meshes are actually placed (and structurally sound)?
    let mesh_used: Vec<bool> = meshes
        .iter()
        .enumerate()
        .map(|(i, m)| {
            !m.indices.is_empty()
                && m.positions.len() >= 3
                && instances.iter().any(|inst| inst.mesh == i)
        })
        .collect();
    if !mesh_used.iter().any(|&u| u) {
        return None;
    }

    // --- Collect the distinct (cba, tsb-page) tiles the vertices sample. ---
    let mut tiles: BTreeMap<(u16, u16), usize> = BTreeMap::new();
    let mut any_flat = false;
    for (mi, m) in meshes.iter().enumerate() {
        if !mesh_used[mi] {
            continue;
        }
        let nverts = m.positions.len() / 3;
        for vi in 0..nverts {
            let flat = m.flat_rgba.len() >= (vi + 1) * 4 && m.flat_rgba[vi * 4 + 3] < 128;
            if flat {
                any_flat = true;
                continue;
            }
            let (Some(&cba), Some(&tsb)) = (m.cba_tsb.get(vi * 2), m.cba_tsb.get(vi * 2 + 1))
            else {
                continue;
            };
            let key = tile_key(cba, tsb);
            let next = tiles.len();
            tiles.entry(key).or_insert(next);
        }
    }
    // Flat vertices sample a dedicated all-white tile so COLOR_0 shows
    // through the shared textured material.
    let white_tile = if any_flat {
        Some(tiles.len())
    } else if tiles.is_empty() {
        return None; // no textured and no flat vertices at all
    } else {
        None
    };
    let tile_count = tiles.len() + usize::from(white_tile.is_some());

    // --- Bake the atlas. ---
    let cols = (tile_count as f64).sqrt().ceil() as usize;
    let cols = cols.clamp(1, 16);
    let rows = tile_count.div_ceil(cols);
    let (aw, ah) = (cols * TILE, rows * TILE);
    let mut atlas = vec![0u8; aw * ah * 4];
    for (&(cba, tsb), &slot) in &tiles {
        let (x0, y0) = ((slot % cols) * TILE, (slot / cols) * TILE);
        bake_tile(vram, cba, tsb, &mut atlas, aw, x0, y0);
    }
    if let Some(slot) = white_tile {
        let (x0, y0) = ((slot % cols) * TILE, (slot / cols) * TILE);
        for v in 0..TILE {
            let o = ((y0 + v) * aw + x0) * 4;
            atlas[o..o + TILE * 4].fill(255);
        }
    }

    let mut b = BinBuilder::default();
    let png = rgba_to_png(aw, ah, &atlas);
    let png_view = b.push_view(&png, None);

    // --- Per-mesh geometry -> glTF meshes. ---
    let uv_of = |slot: usize, u: f32, v: f32| -> [f32; 2] {
        let (x0, y0) = ((slot % cols) * TILE, (slot / cols) * TILE);
        [(x0 as f32 + u) / aw as f32, (y0 as f32 + v) / ah as f32]
    };
    let mut gltf_meshes: Vec<Value> = Vec::new();
    // meshes[] index -> glTF mesh index (only used meshes are emitted).
    let mut gltf_mesh_of: BTreeMap<usize, usize> = BTreeMap::new();
    // ABR blend rate -> materials[] index, allocated on first use (opaque is
    // always materials[0]). BTreeMap so allocation order is deterministic in
    // insertion order via the running length.
    let mut semi_mat_of: BTreeMap<u8, usize> = BTreeMap::new();
    for (mi, m) in meshes.iter().enumerate() {
        if !mesh_used[mi] {
            continue;
        }
        let nverts = m.positions.len() / 3;
        let mut positions: Vec<[f32; 3]> = Vec::with_capacity(nverts);
        let mut uvs: Vec<[f32; 2]> = Vec::with_capacity(nverts);
        // "Has a packet-colour stream" - not "has untextured prims". Both
        // halves ride it; only the divisor differs.
        let has_colors = m.flat_rgba.len() >= nverts * 4;
        let mut colors: Vec<[f32; 4]> = Vec::new();
        for vi in 0..nverts {
            positions.push([
                m.positions[vi * 3],
                -m.positions[vi * 3 + 1], // bake the renderer's Y flip
                m.positions[vi * 3 + 2],
            ]);
            let flat = has_colors && m.flat_rgba[vi * 4 + 3] < 128;
            if flat {
                let slot = white_tile.expect("flat vertex implies white tile");
                uvs.push(uv_of(slot, TILE as f32 * 0.5, TILE as f32 * 0.5));
            } else {
                let key = tile_key(
                    m.cba_tsb.get(vi * 2).copied().unwrap_or(0),
                    m.cba_tsb.get(vi * 2 + 1).copied().unwrap_or(0),
                );
                let slot = tiles.get(&key).copied().unwrap_or(0);
                // +0.5 texel centre, matching the shader's point sampling.
                let u = m.uvs.get(vi * 2).copied().unwrap_or(0) as f32 + 0.5;
                let v = m.uvs.get(vi * 2 + 1).copied().unwrap_or(0) as f32 + 0.5;
                uvs.push(uv_of(slot, u, v));
            }
            if has_colors {
                let w = [
                    m.flat_rgba[vi * 4],
                    m.flat_rgba[vi * 4 + 1],
                    m.flat_rgba[vi * 4 + 2],
                ];
                // An untextured vert's word is a fill (/255); a textured
                // vert's is the texture-blend factor (/128). Pushing white
                // for the textured half is what dropped the shading.
                colors.push(if flat {
                    gltf_color::fill_color(w)
                } else {
                    gltf_color::modulation_color(w)
                });
            }
        }
        // Clamp indices defensively (a bad index would make the file invalid).
        let indices: Vec<u32> = m
            .indices
            .iter()
            .map(|&i| i.min(nverts.saturating_sub(1) as u32))
            .collect();
        // Partition triangles by the prim's semi-transparency enable and,
        // for ABE prims, the ABR blend rate (TSB bits 5..6) - every corner
        // of a prim carries the same TSB, so the first corner decides.
        // Keyed by materials[] index: 0 = opaque, semi rates allocate
        // lazily; BTreeMap iteration then emits opaque first.
        let mut buckets: BTreeMap<usize, Vec<u32>> = BTreeMap::new();
        for tri in indices.as_chunks::<3>().0 {
            let tsb = m.cba_tsb.get(tri[0] as usize * 2 + 1).copied().unwrap_or(0);
            let mat = if tsb & legaia_tmd::mesh::TSB_SEMI_TRANSPARENT_BIT != 0 {
                let abr = ((tsb >> 5) & 3) as u8;
                let next = 1 + semi_mat_of.len();
                *semi_mat_of.entry(abr).or_insert(next)
            } else {
                0
            };
            buckets.entry(mat).or_default().extend_from_slice(tri);
        }

        let pos_acc = b.push_vec3(&positions, Some(TARGET_ARRAY), true);
        let uv_acc = b.push_vec2(&uvs, Some(TARGET_ARRAY));
        let mut attrs = json!({ "POSITION": pos_acc, "TEXCOORD_0": uv_acc });
        if has_colors {
            let col_acc = b.push_vec4(&colors);
            attrs["COLOR_0"] = json!(col_acc);
        }
        // Morph targets: one POSITION-delta accessor per target, with the
        // same Y flip the base positions get.
        let mut target_attrs: Vec<Value> = Vec::new();
        let mut target_names: Vec<Value> = Vec::new();
        for t in &m.morph_targets {
            if t.deltas.len() != m.positions.len() {
                continue; // malformed target - drop rather than desync streams
            }
            let deltas: Vec<[f32; 3]> = (0..nverts)
                .map(|vi| {
                    [
                        t.deltas[vi * 3],
                        -t.deltas[vi * 3 + 1],
                        t.deltas[vi * 3 + 2],
                    ]
                })
                .collect();
            let acc = b.push_vec3(&deltas, Some(TARGET_ARRAY), true);
            target_attrs.push(json!({ "POSITION": acc }));
            target_names.push(json!(t.name));
        }
        let mut prims: Vec<Value> = Vec::new();
        for (material, idx) in &buckets {
            if idx.is_empty() {
                continue;
            }
            let idx_acc = b.push_indices(idx);
            let mut prim = json!({
                "attributes": attrs.clone(), "indices": idx_acc,
                "material": material, "mode": 4
            });
            if !target_attrs.is_empty() {
                prim["targets"] = json!(target_attrs.clone());
            }
            prims.push(prim);
        }
        gltf_mesh_of.insert(mi, gltf_meshes.len());
        let mut mesh_json = json!({ "name": m.name, "primitives": prims });
        if !target_attrs.is_empty() {
            mesh_json["weights"] = json!(vec![0.0f32; target_attrs.len()]);
            // The convention Unity/glTFast (and Blender) read blendshape
            // names from.
            mesh_json["extras"] = json!({ "targetNames": target_names });
        }
        gltf_meshes.push(mesh_json);
    }

    // --- Instances -> nodes. ---
    let mut nodes: Vec<Value> = Vec::new();
    let mut children: Vec<usize> = Vec::new();
    // (node index, caller mesh index) pairs, for the weights channels below.
    let mut inst_nodes: Vec<(usize, usize)> = Vec::new();
    for inst in instances {
        let Some(&gm) = gltf_mesh_of.get(&inst.mesh) else {
            continue;
        };
        let mut node = json!({
            "mesh": gm,
            "translation": inst.translation,
        });
        if inst.rot_y != 0.0 {
            // `rot_y` is the page's `placementModelScaledY` param, and that
            // function's inline rotY block is the transpose of a standard
            // +Y rotation - negate to land on the same facing (see quat_y).
            node["rotation"] = json!(quat_y(-inst.rot_y));
        }
        if inst.scale != 1.0 {
            node["scale"] = json!([inst.scale, inst.scale, inst.scale]);
        }
        children.push(nodes.len());
        inst_nodes.push((nodes.len(), inst.mesh));
        nodes.push(node);
    }
    if children.is_empty() {
        return None;
    }
    let root = nodes.len();
    nodes.push(json!({ "name": name, "children": children }));

    let mut materials = vec![json!({
        "name": "legaia_opaque",
        "pbrMetallicRoughness": {
            "baseColorTexture": { "index": 0 },
            "metallicFactor": 0.0, "roughnessFactor": 1.0
        },
        "extensions": { "KHR_materials_unlit": {} },
        "alphaMode": "MASK", "alphaCutoff": 0.5, "doubleSided": true
    })];
    // One BLEND material per ABR rate in use, appended at its allocated
    // index. The name is the importer's contract (core glTF cannot express
    // additive/subtractive blending); the alpha only approximates the rate
    // for plain viewers - mode 0 (B/2 + F/2) IS alpha blending at 0.5,
    // mode 3 (B + F/4) reads about right at 0.25, and modes 1/2 have no
    // alpha-blend equivalent at all, so they keep 0.5 as the least-bad
    // stand-in. Importers keep BLEND out of the depth-writing opaque queue,
    // which is what retail's coincident ABE scroll layers rely on.
    let mut semi_sorted: Vec<(usize, u8)> = semi_mat_of.iter().map(|(&a, &i)| (i, a)).collect();
    semi_sorted.sort_unstable();
    for (mat_idx, abr) in semi_sorted {
        debug_assert_eq!(mat_idx, materials.len());
        let alpha = if abr == 3 { 0.25 } else { 0.5 };
        materials.push(json!({
            "name": format!("legaia_semi_abr{abr}"),
            "pbrMetallicRoughness": {
                "baseColorTexture": { "index": 0 },
                "baseColorFactor": [1.0, 1.0, 1.0, alpha],
                "metallicFactor": 0.0, "roughnessFactor": 1.0
            },
            "extensions": { "KHR_materials_unlit": {} },
            "alphaMode": "BLEND", "doubleSided": true
        }));
    }

    // --- Morph-weight animation (one channel per node of a tracked mesh). ---
    let mut animations: Vec<Value> = Vec::new();
    if let Some(a) = anim
        && !a.times.is_empty()
    {
        let mut samplers: Vec<Value> = Vec::new();
        let mut channels: Vec<Value> = Vec::new();
        let time_acc = b.push_scalar_f32(&a.times);
        for (&mesh_idx, weights) in &a.tracks {
            let targets = meshes.get(mesh_idx).map_or(0, |m| m.morph_targets.len());
            if targets == 0 || weights.len() != a.times.len() * targets {
                continue;
            }
            let out_acc = b.push_scalar_f32(weights);
            let sampler = samplers.len();
            samplers.push(json!({
                "input": time_acc, "output": out_acc, "interpolation": "LINEAR"
            }));
            for &(node, mi) in &inst_nodes {
                if mi == mesh_idx {
                    channels.push(json!({
                        "sampler": sampler,
                        "target": { "node": node, "path": "weights" }
                    }));
                }
            }
        }
        if !channels.is_empty() {
            animations.push(json!({
                "name": a.name, "samplers": samplers, "channels": channels
            }));
        }
    }

    let mut root_json = json!({
        "asset": { "version": "2.0", "generator": "legend-of-legaia-re scene exporter" },
        "extensionsUsed": [gltf_color::UNLIT_EXTENSION],
        "scene": 0,
        "scenes": [{ "nodes": [root] }],
        "nodes": nodes,
        "meshes": gltf_meshes,
        "materials": materials,
        "images": [{ "bufferView": png_view, "mimeType": "image/png" }],
        // NEAREST + clamp, matching the PSX point-sampled pages.
        "samplers": [{ "magFilter": 9728, "minFilter": 9728, "wrapS": 33071, "wrapT": 33071 }],
        "textures": [{ "source": 0, "sampler": 0 }],
        "accessors": b.accessors,
        "bufferViews": b.buffer_views,
        "buffers": [{ "byteLength": b.bin.len() }]
    });
    if !animations.is_empty() {
        root_json["animations"] = json!(animations);
    }
    Some(pack_glb(&root_json, &b.bin))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn quad_mesh(cba: u16, tsb: u16) -> SceneMesh {
        SceneMesh {
            name: "quad".into(),
            positions: vec![0.0, 0.0, 0.0, 64.0, 0.0, 0.0, 0.0, -64.0, 0.0],
            uvs: vec![0, 0, 63, 0, 0, 63],
            cba_tsb: vec![cba, tsb, cba, tsb, cba, tsb],
            indices: vec![0, 1, 2],
            flat_rgba: Vec::new(),
            morph_targets: Vec::new(),
        }
    }

    /// Morph targets export as glTF `targets` (Y-flipped like the base
    /// positions, named through `extras.targetNames`), and a weights
    /// animation lands one channel per instance node of the tracked mesh.
    #[test]
    fn morph_targets_and_weights_animation_export() {
        let vram = Vram::new();
        let mut m = quad_mesh(0x1234, 0x0005);
        m.morph_targets = vec![SceneMorphTarget {
            name: "vdf_lane_0".into(),
            // Move vertex 1 down by 32 in PSX space (+Y down).
            deltas: vec![0.0, 0.0, 0.0, 0.0, 32.0, 0.0, 0.0, 0.0, 0.0],
        }];
        let instances = [
            SceneInstance {
                mesh: 0,
                translation: [0.0; 3],
                rot_y: 0.0,
                scale: 1.0,
            },
            SceneInstance {
                mesh: 0,
                translation: [128.0, 0.0, 0.0],
                rot_y: 0.0,
                scale: 1.0,
            },
        ];
        let anim = SceneWeightsAnim {
            name: "vdf_pulse".into(),
            times: vec![0.0, 0.5, 1.0],
            tracks: BTreeMap::from([(0usize, vec![0.0, 1.0, 0.0])]),
        };
        let glb = build_scene_glb_animated("morph", &[m], &instances, &vram, Some(&anim)).unwrap();
        let (root, bin) = crate::gltf_color::glb_probe::split(&glb).expect("glb container");
        let mesh = &root["meshes"][0];
        let targets = mesh["primitives"][0]["targets"].as_array().unwrap();
        assert_eq!(targets.len(), 1);
        assert_eq!(mesh["extras"]["targetNames"][0], "vdf_lane_0");
        assert_eq!(mesh["weights"][0], 0.0);
        // The delta stream gets the same Y flip as the base positions.
        let acc = targets[0]["POSITION"].as_u64().unwrap() as usize;
        let d = crate::gltf_color::glb_probe::floats(&root, bin, acc).expect("deltas");
        assert_eq!(d[1][1], -32.0, "PSX +Y-down delta flips to -Y");
        // One weights channel per instance node, sharing one sampler.
        let anims = root["animations"].as_array().unwrap();
        assert_eq!(anims.len(), 1);
        assert_eq!(anims[0]["name"], "vdf_pulse");
        let channels = anims[0]["channels"].as_array().unwrap();
        assert_eq!(channels.len(), 2);
        for c in channels {
            assert_eq!(c["target"]["path"], "weights");
            assert_eq!(c["sampler"], 0);
        }

        // The plain entry point stays animation- and target-free.
        let glb =
            build_scene_glb("plain", &[quad_mesh(0, 0x0005)], &instances[..1], &vram).unwrap();
        let (root, _) = crate::gltf_color::glb_probe::split(&glb).expect("glb container");
        assert!(root["animations"].is_null());
        assert!(root["meshes"][0]["primitives"][0]["targets"].is_null());
    }

    #[test]
    fn empty_scene_exports_none() {
        let vram = Vram::new();
        assert!(build_scene_glb("empty", &[], &[], &vram).is_none());
        // A mesh nobody places also exports nothing.
        assert!(build_scene_glb("unplaced", &[quad_mesh(0, 0)], &[], &vram).is_none());
    }

    #[test]
    fn textured_instance_round_trips_glb_container() {
        let vram = Vram::new();
        let meshes = [quad_mesh(0x1234, 0x0005)];
        let instances = [
            SceneInstance {
                mesh: 0,
                translation: [100.0, 0.0, 200.0],
                rot_y: std::f32::consts::FRAC_PI_2,
                scale: 6.0,
            },
            SceneInstance {
                mesh: 0,
                translation: [300.0, 10.0, 400.0],
                rot_y: 0.0,
                scale: 1.0,
            },
        ];
        let glb = build_scene_glb("test", &meshes, &instances, &vram).unwrap();
        assert_eq!(&glb[0..4], b"glTF");
        let total = u32::from_le_bytes(glb[8..12].try_into().unwrap()) as usize;
        assert_eq!(total, glb.len());
        let json_len = u32::from_le_bytes(glb[12..16].try_into().unwrap()) as usize;
        let root: Value = serde_json::from_slice(&glb[20..20 + json_len]).unwrap();
        // 1 shared mesh, 2 instance nodes + 1 root.
        assert_eq!(root["meshes"].as_array().unwrap().len(), 1);
        assert_eq!(root["nodes"].as_array().unwrap().len(), 3);
        // Y-flip is baked into geometry: -64 becomes +64 in the POSITION max.
        let max_y = root["accessors"][0]["max"][1].as_f64().unwrap();
        assert!((max_y - 64.0).abs() < 1e-3);
        // Instance TRS carried through.
        assert_eq!(root["nodes"][0]["translation"][0], 100.0);
        assert_eq!(root["nodes"][0]["scale"][0], 6.0);
        // The yaw SIGN: rot_y is the page's `placementModelScaledY` param,
        // whose inline rotY block is transposed - the node quaternion must
        // be the standard Ry of the NEGATED param. For rot_y = +PI/2 that
        // is quat [0, sin(-PI/4), 0, cos(PI/4)]: y strictly negative. The
        // unnegated emission faced every yawed building backwards in Unity
        // and Blender while leaving all positions (and layout tests) right.
        let qy = root["nodes"][0]["rotation"][1].as_f64().unwrap();
        let qw = root["nodes"][0]["rotation"][3].as_f64().unwrap();
        assert!(
            (qy + std::f64::consts::FRAC_PI_4.sin()).abs() < 1e-6,
            "rotation must be Ry(-rot_y), got quat y = {qy}"
        );
        assert!((qw - std::f64::consts::FRAC_PI_4.cos()).abs() < 1e-6);
    }

    #[test]
    fn tile_key_then_bake_reads_the_second_page_row() {
        // The assembled battle characters sample tsb 0x38: page x = 8 (512),
        // page y = 1 (256), 4bpp. The atlas bake goes through tile_key first,
        // so the key must keep bit 4 - masking it off bakes the tile from
        // page y = 0 and the whole character comes out transparent.
        let (cba, tsb) = (0x7844u16, 0x0038u16);
        assert_ne!(
            tile_key(cba, tsb),
            tile_key(cba, tsb & !0x10),
            "tile_key must distinguish page y"
        );
        let mut vram = Vram::new();
        // One 4bpp word at texel (0, 44) of the y=256 page: index 1 in the
        // low nibble. CLUT row 400, column 2: entry 1 = opaque white.
        vram.write_block(512, 300, 1, 1, &[0x01, 0x00]);
        let mut clut = [0u8; 32];
        clut[2] = 0xFF;
        clut[3] = 0x7F;
        vram.write_clut_row(32, 400, &clut);
        let cba = (400u16 << 6) | 2;
        let (kc, kt) = tile_key(cba, tsb);
        let mut out = vec![0u8; TILE * TILE * 4];
        bake_tile(&vram, kc, kt, &mut out, TILE, 0, 0);
        let px = &out[(44 * TILE) * 4..(44 * TILE) * 4 + 4];
        assert_eq!(px, [255, 255, 255, 255], "texel decodes via the CLUT");
    }

    /// A hybrid mesh's two halves take **different divisors**: vertices 0/1
    /// are textured (their word modulates the texel, `/128`) and vertex 2 is
    /// untextured (its word is the fill, `/255`). Exporting white for the
    /// textured half - which is what this used to do - drops the shading the
    /// page draws.
    #[test]
    fn both_halves_of_the_packet_stream_reach_color0() {
        let vram = Vram::new();
        let mut m = quad_mesh(0, 0x0005);
        // v0/v1 textured with a hot orange word; v2 untextured, red fill.
        m.flat_rgba = vec![
            0xF8, 0x80, 0x00, 255, // textured, ~2x red
            0x40, 0x40, 0x40, 255, // textured, heavy darkening
            255, 0, 0, 0, // untextured fill
        ];
        let instances = [SceneInstance {
            mesh: 0,
            translation: [0.0; 3],
            rot_y: 0.0,
            scale: 1.0,
        }];
        let glb = build_scene_glb("hybrid", &[m], &instances, &vram).unwrap();
        let (root, bin) = crate::gltf_color::glb_probe::split(&glb).expect("glb container");
        assert_eq!(root["extensionsUsed"][0], "KHR_materials_unlit");
        let attrs = &root["meshes"][0]["primitives"][0]["attributes"];
        let acc = attrs["COLOR_0"]
            .as_u64()
            .expect("hybrid mesh needs COLOR_0") as usize;
        let c = crate::gltf_color::glb_probe::floats(&root, bin, acc).expect("COLOR_0 floats");
        // Every channel is the sRGB-linearized display ratio; re-encoding
        // recovers word / 128 (textured) or word / 255 (fill).
        use crate::gltf_color::linear_to_srgb_ratio as enc;
        assert!((enc(c[0][0]) - 248.0 / 128.0).abs() < 1e-5, "{:?}", c[0]);
        assert!(c[0][0] > 1.0, "the over-bright tail survives the encoding");
        assert!((enc(c[1][0]) - 64.0 / 128.0).abs() < 1e-5, "{:?}", c[1]);
        assert_ne!(c[0][0], 1.0, "textured vert must not export white");
        // The untextured fill keeps the /255 divisor (white stays exact).
        assert!((c[2][0] - 1.0).abs() < 1e-6, "{:?}", c[2]);
        assert_eq!(c[2][1], 0.0);
    }

    /// ABE (semi-transparent) triangles split off into per-ABR-rate
    /// primitives on named BLEND materials; opaque scenes keep the
    /// single-material layout.
    #[test]
    fn abe_triangles_split_into_a_blend_primitive() {
        let vram = Vram::new();
        // Three triangles sharing a vertex pool: verts 0-2 opaque, 3-5 ABE
        // rate 0 (TSB bit 15 - `pack_tsb_semi` sets it from the group's ABE
        // flag), 6-8 ABE rate 1 (additive - ABR in TSB bits 5..6).
        let abe_tsb = 0x0005 | legaia_tmd::mesh::TSB_SEMI_TRANSPARENT_BIT;
        let add_tsb = abe_tsb | (1 << 5);
        let m = SceneMesh {
            name: "water".into(),
            positions: vec![
                0.0, 0.0, 0.0, 64.0, 0.0, 0.0, 0.0, -64.0, 0.0, //
                0.0, 0.0, 8.0, 64.0, 0.0, 8.0, 0.0, -64.0, 8.0, //
                0.0, 0.0, 16.0, 64.0, 0.0, 16.0, 0.0, -64.0, 16.0,
            ],
            uvs: vec![0, 0, 63, 0, 0, 63, 0, 0, 63, 0, 0, 63, 0, 0, 63, 0, 0, 63],
            cba_tsb: vec![
                0, 0x0005, 0, 0x0005, 0, 0x0005, //
                0, abe_tsb, 0, abe_tsb, 0, abe_tsb, //
                0, add_tsb, 0, add_tsb, 0, add_tsb,
            ],
            indices: vec![0, 1, 2, 3, 4, 5, 6, 7, 8],
            flat_rgba: Vec::new(),
            morph_targets: Vec::new(),
        };
        let instances = [SceneInstance {
            mesh: 0,
            translation: [0.0; 3],
            rot_y: 0.0,
            scale: 1.0,
        }];
        let glb = build_scene_glb("abe", &[m], &instances, &vram).unwrap();
        let (root, _) = crate::gltf_color::glb_probe::split(&glb).expect("glb container");
        let prims = root["meshes"][0]["primitives"].as_array().unwrap();
        assert_eq!(prims.len(), 3, "opaque + one blend primitive per rate");
        assert_eq!(prims[0]["material"], 0);
        assert_eq!(prims[1]["material"], 1);
        assert_eq!(prims[2]["material"], 2);
        let mats = root["materials"].as_array().unwrap();
        assert_eq!(mats.len(), 3);
        assert_eq!(mats[0]["alphaMode"], "MASK");
        assert_eq!(mats[0]["name"], "legaia_opaque");
        assert_eq!(mats[1]["alphaMode"], "BLEND");
        assert_eq!(mats[1]["name"], "legaia_semi_abr0");
        assert_eq!(
            mats[1]["pbrMetallicRoughness"]["baseColorFactor"][3], 0.5,
            "PSX average blend approximated at half alpha"
        );
        assert_eq!(
            mats[2]["name"], "legaia_semi_abr1",
            "additive prims land on their own named material"
        );
        assert_eq!(mats[2]["alphaMode"], "BLEND");

        // An all-opaque scene keeps exactly one material and one primitive.
        let glb = build_scene_glb("plain", &[quad_mesh(0, 0x0005)], &instances, &vram).unwrap();
        let (root, _) = crate::gltf_color::glb_probe::split(&glb).expect("glb container");
        assert_eq!(root["materials"].as_array().unwrap().len(), 1);
        assert_eq!(root["meshes"][0]["primitives"].as_array().unwrap().len(), 1);
    }

    /// A caller with no colour stream still exports - nothing to modulate by,
    /// so no COLOR_0 at all (the neutral read).
    #[test]
    fn a_mesh_without_a_packet_stream_omits_color0() {
        let vram = Vram::new();
        let instances = [SceneInstance {
            mesh: 0,
            translation: [0.0; 3],
            rot_y: 0.0,
            scale: 1.0,
        }];
        let glb = build_scene_glb("plain", &[quad_mesh(0, 0x0005)], &instances, &vram).unwrap();
        let (root, _) = crate::gltf_color::glb_probe::split(&glb).expect("glb container");
        assert!(
            root["meshes"][0]["primitives"][0]["attributes"]["COLOR_0"].is_null(),
            "no stream, no attribute"
        );
    }
}
