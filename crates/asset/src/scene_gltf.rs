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
//! - "hybrid" meshes (the field env packs) carry untextured flat/gouraud
//!   vertices tagged in a `flat_rgba` side channel - those vertices keep
//!   their colour via `COLOR_0` and point their UV at a dedicated all-white
//!   tile, so one material still covers the whole scene;
//! - instances become glTF nodes sharing mesh data, with the exact
//!   translation / Y-rotation / uniform scale the on-screen renderer applies.
//!
//! ## Coordinate convention
//!
//! Mesh-local geometry is stored PSX-style (+Y down). The site renderers
//! flip Y in the per-placement model matrix (`diag(s, -s, s)`); the export
//! bakes the same flip into the mesh geometry (negating local Y) so instance
//! transforms stay plain TRS and the model reads +Y-up in any glTF viewer -
//! matching what the page shows, mirror-handedness and all.
//!
//! PSX transparency follows the fragment shader: a BGR555 word of `0` bakes
//! as fully transparent (alpha 0) and the material uses `MASK` alpha, so
//! cutout foliage / grates export as cutouts.

use crate::monster_gltf::{BinBuilder, TARGET_ARRAY, pack_glb, rgba_to_png};
use legaia_tim::Vram;
use serde_json::{Value, json};
use std::collections::BTreeMap;

/// Atlas tile edge in texels - one full PSX texture page window (u8 UVs
/// address 0..255 within the page picked by the vertex's `tsb`).
pub(crate) const TILE: usize = 256;

/// One reusable mesh: the exact vertex streams the WebGL scene path renders
/// (`positions` f32 xyz in PSX space, `uvs` u8 page-local texel pairs,
/// `cba_tsb` u16 `[cba, tsb]` pairs, triangle-list `indices`). `flat_rgba`
/// is empty for pure-textured meshes, else 4 bytes `[r, g, b, flag]` per
/// vertex with flag `0` = untextured (use the colour) and `255` = textured.
pub struct SceneMesh {
    pub name: String,
    pub positions: Vec<f32>,
    pub uvs: Vec<u8>,
    pub cba_tsb: Vec<u16>,
    pub indices: Vec<u32>,
    pub flat_rgba: Vec<u8>,
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

/// Quaternion `[x, y, z, w]` for a rotation of `a` radians about +Y -
/// produces the same linear map as the page's `placementModelScaled` rotY
/// block (`[c,0,s; 0,1,0; -s,0,c]`).
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
    for (mi, m) in meshes.iter().enumerate() {
        if !mesh_used[mi] {
            continue;
        }
        let nverts = m.positions.len() / 3;
        let mut positions: Vec<[f32; 3]> = Vec::with_capacity(nverts);
        let mut uvs: Vec<[f32; 2]> = Vec::with_capacity(nverts);
        let has_flat = m.flat_rgba.len() >= nverts * 4;
        let mut colors: Vec<[f32; 4]> = Vec::new();
        for vi in 0..nverts {
            positions.push([
                m.positions[vi * 3],
                -m.positions[vi * 3 + 1], // bake the renderer's Y flip
                m.positions[vi * 3 + 2],
            ]);
            let flat = has_flat && m.flat_rgba[vi * 4 + 3] < 128;
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
            if has_flat {
                if flat {
                    colors.push([
                        m.flat_rgba[vi * 4] as f32 / 255.0,
                        m.flat_rgba[vi * 4 + 1] as f32 / 255.0,
                        m.flat_rgba[vi * 4 + 2] as f32 / 255.0,
                        1.0,
                    ]);
                } else {
                    colors.push([1.0, 1.0, 1.0, 1.0]);
                }
            }
        }
        // Clamp indices defensively (a bad index would make the file invalid).
        let indices: Vec<u32> = m
            .indices
            .iter()
            .map(|&i| i.min(nverts.saturating_sub(1) as u32))
            .collect();

        let pos_acc = b.push_vec3(&positions, Some(TARGET_ARRAY), true);
        let uv_acc = b.push_vec2(&uvs, Some(TARGET_ARRAY));
        let idx_acc = b.push_indices(&indices);
        let mut attrs = json!({ "POSITION": pos_acc, "TEXCOORD_0": uv_acc });
        if has_flat {
            let col_acc = b.push_vec4(&colors);
            attrs["COLOR_0"] = json!(col_acc);
        }
        gltf_mesh_of.insert(mi, gltf_meshes.len());
        gltf_meshes.push(json!({
            "name": m.name,
            "primitives": [{ "attributes": attrs, "indices": idx_acc, "material": 0, "mode": 4 }]
        }));
    }

    // --- Instances -> nodes. ---
    let mut nodes: Vec<Value> = Vec::new();
    let mut children: Vec<usize> = Vec::new();
    for inst in instances {
        let Some(&gm) = gltf_mesh_of.get(&inst.mesh) else {
            continue;
        };
        let mut node = json!({
            "mesh": gm,
            "translation": inst.translation,
        });
        if inst.rot_y != 0.0 {
            node["rotation"] = json!(quat_y(inst.rot_y));
        }
        if inst.scale != 1.0 {
            node["scale"] = json!([inst.scale, inst.scale, inst.scale]);
        }
        children.push(nodes.len());
        nodes.push(node);
    }
    if children.is_empty() {
        return None;
    }
    let root = nodes.len();
    nodes.push(json!({ "name": name, "children": children }));

    let root_json = json!({
        "asset": { "version": "2.0", "generator": "legend-of-legaia-re scene exporter" },
        "scene": 0,
        "scenes": [{ "nodes": [root] }],
        "nodes": nodes,
        "meshes": gltf_meshes,
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
        }
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

    #[test]
    fn hybrid_flat_vertices_get_color0_and_white_tile() {
        let vram = Vram::new();
        let mut m = quad_mesh(0, 0x0005);
        // Vertex 2 untextured, carrying red.
        m.flat_rgba = vec![255, 255, 255, 255, 255, 255, 255, 255, 255, 0, 0, 0];
        let instances = [SceneInstance {
            mesh: 0,
            translation: [0.0; 3],
            rot_y: 0.0,
            scale: 1.0,
        }];
        let glb = build_scene_glb("hybrid", &[m], &instances, &vram).unwrap();
        let json_len = u32::from_le_bytes(glb[12..16].try_into().unwrap()) as usize;
        let root: Value = serde_json::from_slice(&glb[20..20 + json_len]).unwrap();
        let attrs = &root["meshes"][0]["primitives"][0]["attributes"];
        assert!(attrs["COLOR_0"].is_number(), "hybrid mesh needs COLOR_0");
    }
}
