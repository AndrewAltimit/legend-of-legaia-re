//! The `#[wasm_bindgen]` edge of [`Core`]: strings and byte vectors in and
//! out, errors as JS strings. Every JSON shape is [`Core`]'s.

use wasm_bindgen::prelude::*;

use crate::session::Core;

fn err(e: anyhow::Error) -> JsValue {
    JsValue::from_str(&format!("{e:#}"))
}

/// The workbench session, held by the page for as long as it edits.
#[wasm_bindgen]
pub struct ExpansionWorkbench {
    core: Core,
}

impl Default for ExpansionWorkbench {
    fn default() -> Self {
        Self::new()
    }
}

#[wasm_bindgen]
impl ExpansionWorkbench {
    #[wasm_bindgen(constructor)]
    pub fn new() -> ExpansionWorkbench {
        ExpansionWorkbench { core: Core::new() }
    }

    /// Parse the user's disc image and return the picker lists (JSON).
    pub fn open_disc(&mut self, image: Vec<u8>) -> Result<String, JsValue> {
        self.core.open_disc(&image).map_err(err)?;
        Ok(self.core.catalog_json())
    }

    pub fn has_disc(&self) -> bool {
        self.core.has_disc()
    }

    /// The picker lists (JSON, `null` without a disc).
    pub fn catalog(&self) -> String {
        self.core.catalog_json()
    }

    /// A base scene's geometry pack: `{ scene, entry, objects: [{ index,
    /// prims, size, placed, footprint }] }`.
    pub fn scene_objects(&mut self, scene: &str) -> Result<String, JsValue> {
        self.core.scene_objects_json(scene).map_err(err)
    }

    /// A scene's NPC placements: `{ scene, npcs: [{ index, model_index,
    /// anim_id, special, kind, label }] }`.
    pub fn scene_npcs(&self, scene: &str) -> Result<String, JsValue> {
        self.core.scene_npcs_json(scene).map_err(err)
    }

    /// Start handing over a pack tree (a folder pick or the published
    /// example), one [`Self::add_file`] per file.
    pub fn begin_files(&mut self) {
        self.core.begin_files();
    }

    pub fn add_file(&mut self, path: &str, bytes: Vec<u8>) {
        self.core.add_file(path, bytes);
    }

    /// Import a file into the pack's `assets/` (`path` relative to it): a
    /// glTF model for a prop's `{ asset = ... }` mesh, or its PNG texture.
    pub fn add_asset(&mut self, path: &str, bytes: Vec<u8>) {
        self.core.add_asset(path, bytes);
    }

    /// The pack's asset paths, as a JSON array.
    pub fn asset_paths(&self) -> String {
        serde_json::to_string(&self.core.asset_paths()).unwrap_or_else(|_| "[]".into())
    }

    /// Load the handed-over tree: `{ pack, files, issues }`.
    pub fn finish_files(&mut self) -> String {
        self.core.finish_files().to_string()
    }

    /// Load a `.lxpack`: `{ pack, files, issues }`.
    pub fn load_bundle(&mut self, bytes: Vec<u8>) -> Result<String, JsValue> {
        self.core
            .load_bundle(&bytes)
            .map(|v| v.to_string())
            .map_err(err)
    }

    /// A new pack holding only its manifest (pack JSON).
    pub fn start_fresh(&mut self, id: &str, name: &str) -> String {
        self.core.start_fresh(id, name).to_string()
    }

    /// Every issue in `pack` (JSON array of `{ severity, path, field,
    /// message }`).
    pub fn validate(&self, pack: &str) -> String {
        serde_json::to_string(&self.core.validate(pack)).unwrap_or_else(|_| "[]".into())
    }

    /// The exported file tree: `[{ path, size, text }]`.
    pub fn files_view(&self, pack: &str) -> Result<String, JsValue> {
        self.core
            .files_view(pack)
            .map(|v| v.to_string())
            .map_err(err)
    }

    /// The `.lxpack` bytes; refused while the pack has errors.
    pub fn export_bundle(&self, pack: &str) -> Result<Vec<u8>, JsValue> {
        self.core.export_bundle(pack).map_err(err)
    }

    /// `{ retail, pack, xp_mul, scaling, problems }` for a progression
    /// section.
    pub fn xp_chart(&self, progression: &str) -> Result<String, JsValue> {
        self.core
            .xp_chart(progression)
            .map(|v| v.to_string())
            .map_err(err)
    }

    /// One dialogue line checked row by row.
    pub fn check_dialog(&self, line: &str) -> Result<String, JsValue> {
        self.core
            .check_dialog(line)
            .map(|v| v.to_string())
            .map_err(err)
    }

    /// A cutscene's text timeline (`Core::cutscene_timeline`); `picks` is
    /// the option taken at each menu, in order.
    pub fn cutscene_timeline(
        &self,
        pack: &str,
        id: &str,
        picks: Vec<u32>,
    ) -> Result<String, JsValue> {
        let picks: Vec<usize> = picks.into_iter().map(|p| p as usize).collect();
        self.core
            .cutscene_timeline(pack, id, &picks)
            .map(|v| v.to_string())
            .map_err(err)
    }

    /// The engine's reading of a map's tile layer (or `{ error }`).
    pub fn map_preview(&self, map: &str) -> String {
        self.core.map_preview(map).to_string()
    }
}
