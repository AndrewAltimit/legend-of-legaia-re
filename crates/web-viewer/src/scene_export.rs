//! Scene `.glb` export session for `LegaiaViewer`.
//!
//! Builder-style API so the JS pages can export **exactly what they render**:
//! the page feeds the same mesh buffers it uploads to WebGL plus the same
//! per-draw `(translation, rotY, scale)` triples it builds model matrices
//! from, then asks for the baked glTF. Texture indirection (page-local UVs +
//! per-vertex `(cba, tsb)` sampled against VRAM) is baked into a single RGBA
//! atlas by [`legaia_asset::scene_gltf`].
//!
//! Typical JS flow:
//!
//! ```js
//! viewer.scene_export_begin('drake_kingdom');
//! viewer.scene_export_set_vram(viewer.pack_vram_bytes());
//! const mi = viewer.scene_export_add_mesh('mesh_3',
//!   positions, uvs, cbaTsb, indices, new Uint8Array(0));
//! viewer.scene_export_add_instance(mi, x, y, z, rotY, scale);
//! const glb = viewer.scene_export_finish();   // Uint8Array (empty = nothing)
//! ```

use super::*;
use legaia_asset::scene_gltf::{SceneInstance, SceneMesh, build_scene_glb};

/// Accumulated export session: registered meshes, placements, target VRAM.
pub struct SceneExportState {
    name: String,
    meshes: Vec<SceneMesh>,
    instances: Vec<SceneInstance>,
    vram: legaia_tim::Vram,
}

#[wasm_bindgen]
impl LegaiaViewer {
    /// Start a fresh export session named `name` (becomes the glTF root
    /// node name). Discards any prior unfinished session.
    pub fn scene_export_begin(&mut self, name: &str) {
        self.scene_export = Some(SceneExportState {
            name: name.to_string(),
            meshes: Vec::new(),
            instances: Vec::new(),
            vram: legaia_tim::Vram::new(),
        });
    }

    /// Supply the 1 MiB VRAM image (`1024*512` LE u16 words - the same bytes
    /// the page uploads to its R16UI texture) the atlas bake reads from.
    pub fn scene_export_set_vram(&mut self, bytes: &[u8]) {
        if let Some(s) = self.scene_export.as_mut() {
            s.vram = legaia_tim::Vram::new();
            s.vram.write_block(0, 0, 1024, 512, bytes);
        }
    }

    /// Register a reusable mesh (the exact streams the page renders:
    /// `positions` f32 xyz PSX-space, `uvs` u8 page-local texel pairs,
    /// `cba_tsb` u16 `[cba, tsb]` pairs, u32 triangle indices, and the
    /// optional hybrid `flat_rgba` side channel - pass an empty array for
    /// pure-textured meshes). Returns the mesh handle for
    /// [`Self::scene_export_add_instance`], or `u32::MAX` when no session
    /// is open.
    #[allow(clippy::too_many_arguments)]
    pub fn scene_export_add_mesh(
        &mut self,
        name: &str,
        positions: &[f32],
        uvs: &[u8],
        cba_tsb: &[u16],
        indices: &[u32],
        flat_rgba: &[u8],
    ) -> u32 {
        let Some(s) = self.scene_export.as_mut() else {
            return u32::MAX;
        };
        s.meshes.push(SceneMesh {
            name: name.to_string(),
            positions: positions.to_vec(),
            uvs: uvs.to_vec(),
            cba_tsb: cba_tsb.to_vec(),
            indices: indices.to_vec(),
            flat_rgba: flat_rgba.to_vec(),
        });
        (s.meshes.len() - 1) as u32
    }

    /// Place mesh handle `mesh` at `(tx, ty, tz)` with `rot_y` radians about
    /// +Y and uniform `scale` - the same triple the page's
    /// `placementModelScaledY` builds its model matrix from.
    pub fn scene_export_add_instance(
        &mut self,
        mesh: u32,
        tx: f32,
        ty: f32,
        tz: f32,
        rot_y: f32,
        scale: f32,
    ) {
        if let Some(s) = self.scene_export.as_mut()
            && (mesh as usize) < s.meshes.len()
        {
            s.instances.push(SceneInstance {
                mesh: mesh as usize,
                translation: [tx, ty, tz],
                rot_y,
                scale,
            });
        }
    }

    /// Bake the accumulated session into `.glb` bytes and close it. Returns
    /// an empty array when the session is missing or contains no drawable
    /// geometry.
    pub fn scene_export_finish(&mut self) -> Vec<u8> {
        let Some(s) = self.scene_export.take() else {
            return Vec::new();
        };
        build_scene_glb(&s.name, &s.meshes, &s.instances, &s.vram).unwrap_or_default()
    }
}
