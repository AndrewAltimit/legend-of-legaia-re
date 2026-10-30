//! The workbench session, native-testable: the user's disc (for the pickers
//! and the disc half of validation), the files the pack came from, and the
//! JSON conversions the page works in.
//!
//! The page edits the pack as the JSON form of [`ExpansionPack`] (serde's,
//! the same field names as the TOML). Every operation that leaves the page -
//! validation, the TOML view, the `.lxpack` - goes JSON -> [`ExpansionPack`]
//! -> [`ExpansionPack::to_files`], so what is checked is exactly what is
//! exported. Two things ride beside the JSON:
//!
//! - **extras**: the pack's `assets/` and documentation files, which the
//!   editor does not edit (serde skips them on the pack), carried through
//!   every export unchanged;
//! - **original**: the files the pack was loaded from. A file whose parsed
//!   value the editor left unchanged exports as its original bytes, so an
//!   unedited section keeps its comments and layout; an edited one is
//!   re-serialised.

use std::collections::HashMap;

use anyhow::{Context, Result, bail};
use serde::Serialize;
use serde_json::{Value, json};

use legaia_engine_expansion::progression::{enemy_scaling_permille, xp_table};
use legaia_engine_scene::expansion_map::{BaseInputs, build_field_map_image, floor_lut};
use legaia_expansion_format::dialog::{self, DialogLine, ROW_WARN_CHARS};
use legaia_expansion_format::manifest::PackManifest;
use legaia_expansion_format::map::{BaseGeometry, FLOOR_TIERS, Ground, MapDef, PropMesh};
use legaia_expansion_format::progression::{MAX_LEVEL, ProgressionSection};
use legaia_expansion_format::validate::{self, Issue, Severity};
use legaia_expansion_format::{ExpansionPack, PackFiles, bundle};
use legaia_scene_formats::field_objects::{COLLISION_GRID_OFFSET, GRID_DIM};

use crate::catalog::{DiscCatalog, SceneObjects};

/// One issue as the page shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IssueRow {
    pub severity: &'static str,
    pub path: String,
    pub field: String,
    pub message: String,
}

impl From<&Issue> for IssueRow {
    fn from(i: &Issue) -> Self {
        Self {
            severity: match i.severity {
                Severity::Error => "error",
                Severity::Warning => "warning",
            },
            path: i.path.clone(),
            field: i.field.clone(),
            message: i.message.clone(),
        }
    }
}

/// The path an issue about the editor's own JSON is filed under.
pub const EDITOR_PATH: &str = "(workbench)";

/// The session state.
#[derive(Default)]
pub struct Core {
    catalog: Option<DiscCatalog>,
    /// `assets/` and documentation files, carried through unchanged.
    extras: PackFiles,
    /// The files the pack was loaded from.
    original: PackFiles,
    /// The pack `original` parses to, for the unchanged-file test.
    original_pack: Option<ExpansionPack>,
    /// Files handed over one by one before [`Self::finish_files`].
    staged: PackFiles,
    /// Base-scene geometry packs already assembled, by scene name.
    objects: HashMap<String, SceneObjects>,
}

/// The longest timeline preview, in frames (ten minutes).
const TIMELINE_CAP: u32 = 36_000;

impl Core {
    pub fn new() -> Self {
        Self::default()
    }

    /// Parse the user's disc for the pickers and the disc half of
    /// validation. The image is not kept.
    pub fn open_disc(&mut self, image: &[u8]) -> Result<()> {
        self.catalog = Some(DiscCatalog::open(image)?);
        self.objects.clear();
        Ok(())
    }

    /// Use an already-built catalog (tests, and a host that has one).
    pub fn set_catalog(&mut self, catalog: DiscCatalog) {
        self.catalog = Some(catalog);
        self.objects.clear();
    }

    pub fn has_disc(&self) -> bool {
        self.catalog.is_some()
    }

    /// The picker lists as JSON (`null` without a disc).
    pub fn catalog_json(&self) -> String {
        match &self.catalog {
            Some(c) => serde_json::to_string(&c.lists).unwrap_or_else(|_| "null".into()),
            None => "null".into(),
        }
    }

    /// Scene `scene`'s NPC placements for the NPC model picker
    /// ([`DiscCatalog::scene_npcs`]), as JSON.
    pub fn scene_npcs_json(&self, scene: &str) -> Result<String> {
        let cat = self.catalog.as_ref().context("load your disc first")?;
        Ok(serde_json::to_string(&cat.scene_npcs(scene)?)?)
    }

    /// The base scene's geometry pack (see [`DiscCatalog::scene_objects`]),
    /// assembled once per scene.
    pub fn scene_objects_json(&mut self, scene: &str) -> Result<String> {
        let cat = self.catalog.as_ref().context("load your disc first")?;
        if !self.objects.contains_key(scene) {
            let objs = cat.scene_objects(scene)?;
            self.objects.insert(scene.to_string(), objs);
        }
        Ok(serde_json::to_string(&self.objects[scene])?)
    }

    // --- Loading ---------------------------------------------------------------

    /// Start handing over a pack tree file by file.
    pub fn begin_files(&mut self) {
        self.staged.clear();
    }

    pub fn add_file(&mut self, path: &str, bytes: Vec<u8>) {
        self.staged.insert(path.to_string(), bytes);
    }

    /// Add (or replace) a file under the pack's `assets/` - a glTF model or
    /// a PNG the page imported. `path` is relative to `assets/`. Carried
    /// with the pack's other extras into validation and export.
    pub fn add_asset(&mut self, path: &str, bytes: Vec<u8>) {
        self.extras.insert(format!("assets/{path}"), bytes);
    }

    /// The paths (relative to `assets/`) of the pack's asset files.
    pub fn asset_paths(&self) -> Vec<String> {
        self.extras
            .keys()
            .filter_map(|p| p.strip_prefix("assets/"))
            .map(str::to_string)
            .collect()
    }

    /// Load the staged tree. See [`Self::load_files`].
    pub fn finish_files(&mut self) -> Value {
        let files = std::mem::take(&mut self.staged);
        self.load_files(files)
    }

    /// Load a `.lxpack` bundle.
    pub fn load_bundle(&mut self, bytes: &[u8]) -> Result<Value> {
        let files = bundle::read_bundle(bytes).context("reading the .lxpack")?;
        Ok(self.load_files(files))
    }

    /// Load a pack tree: `{ pack, files, issues }`. `pack` is `null` when the
    /// tree does not parse (the issues say why); otherwise it becomes the
    /// session's original.
    pub fn load_files(&mut self, files: PackFiles) -> Value {
        // A folder picked in the browser arrives under its own name
        // (`rebirth/pack.toml`): strip one shared leading directory.
        let files = strip_common_root(files);
        let issues = self.check_files(&files);
        let list: Vec<Value> = files
            .iter()
            .map(|(p, b)| json!({ "path": p, "size": b.len() }))
            .collect();
        match ExpansionPack::from_files(&files) {
            Ok(pack) => {
                self.extras = files
                    .iter()
                    .filter(|(p, _)| p.starts_with("assets/") || is_doc(p))
                    .map(|(p, b)| (p.clone(), b.clone()))
                    .collect();
                self.original = files;
                self.original_pack = Some(pack.clone());
                json!({ "pack": pack, "files": list, "issues": issues })
            }
            Err(_) => json!({ "pack": Value::Null, "files": list, "issues": issues }),
        }
    }

    /// A new pack with only a manifest.
    pub fn start_fresh(&mut self, id: &str, name: &str) -> Value {
        self.extras.clear();
        self.original.clear();
        self.original_pack = None;
        let pack = ExpansionPack {
            manifest: PackManifest::new(id, name, "0.1.0"),
            ..Default::default()
        };
        json!(pack)
    }

    // --- The pack as files -------------------------------------------------------

    /// The pack the page holds, from its JSON, with the session's assets and
    /// documentation re-attached.
    pub fn pack_from_json(&self, pack_json: &str) -> Result<ExpansionPack> {
        let mut pack: ExpansionPack =
            serde_json::from_str(pack_json).context("the editor's pack does not parse")?;
        for (path, bytes) in &self.extras {
            if let Some(rel) = path.strip_prefix("assets/") {
                pack.assets.insert(rel.to_string(), bytes.clone());
            } else {
                pack.docs.insert(path.clone(), bytes.clone());
            }
        }
        Ok(pack)
    }

    /// The file tree the pack exports as. A manifest, section or map whose
    /// value equals what it was loaded as keeps its original bytes.
    pub fn pack_files(&self, pack_json: &str) -> Result<PackFiles> {
        let pack = self.pack_from_json(pack_json)?;
        let mut files = pack.to_files()?;
        if let Some(orig) = &self.original_pack {
            for (path, bytes) in files.iter_mut() {
                if let Some(o) = self.original.get(path)
                    && unchanged(path, &pack, orig)
                {
                    bytes.clone_from(o);
                }
            }
        }
        Ok(files)
    }

    fn check_files(&self, files: &PackFiles) -> Vec<IssueRow> {
        let resolver = self
            .catalog
            .as_ref()
            .map(|c| c as &dyn legaia_expansion_format::refs::DiscResolver);
        validate::validate_files(files, resolver)
            .iter()
            .map(IssueRow::from)
            .collect()
    }

    /// Every issue in the pack the page holds, against the disc when one is
    /// loaded.
    pub fn validate(&self, pack_json: &str) -> Vec<IssueRow> {
        match self.pack_files(pack_json) {
            Ok(files) => self.check_files(&files),
            Err(e) => vec![IssueRow {
                severity: "error",
                path: EDITOR_PATH.into(),
                field: String::new(),
                message: format!("{e:#}"),
            }],
        }
    }

    /// The exported tree for the TOML view: `[{ path, size, text }]`, `text`
    /// `null` for a binary asset.
    pub fn files_view(&self, pack_json: &str) -> Result<Value> {
        let files = self.pack_files(pack_json)?;
        Ok(Value::Array(
            files
                .iter()
                .map(|(p, b)| {
                    let text = (!p.starts_with("assets/") || p.ends_with(".toml"))
                        .then(|| std::str::from_utf8(b).ok())
                        .flatten();
                    json!({ "path": p, "size": b.len(), "text": text })
                })
                .collect(),
        ))
    }

    /// The `.lxpack` bytes. Refused while the pack has errors, as
    /// `asset expansion bundle` refuses.
    pub fn export_bundle(&self, pack_json: &str) -> Result<Vec<u8>> {
        let files = self.pack_files(pack_json)?;
        let issues = self.check_files(&files);
        let errors: Vec<&IssueRow> = issues.iter().filter(|i| i.severity == "error").collect();
        if let Some(first) = errors.first() {
            bail!(
                "the pack has {} error(s); the first: {} [{}]: {}",
                errors.len(),
                first.path,
                first.field,
                first.message
            );
        }
        bundle::write_bundle(&files)
    }

    // --- Previews ----------------------------------------------------------------

    /// The level curve a progression section makes: cumulative XP to reach
    /// each level, retail and the pack's, and the enemy scaling multiplier
    /// (thousandths) per party level.
    pub fn xp_chart(&self, progression_json: &str) -> Result<Value> {
        let prog: ProgressionSection =
            serde_json::from_str(progression_json).context("progression does not parse")?;
        let retail = legaia_engine_battle::levelup::retail_xp_table();
        let pack = xp_table(&prog.xp_curve, &retail);
        let scaling: Option<Vec<u32>> = prog.enemy_scaling.as_ref().map(|s| {
            (1..=MAX_LEVEL)
                .map(|l| enemy_scaling_permille(s, l))
                .collect()
        });
        Ok(json!({
            "retail": retail,
            "pack": pack,
            "xp_mul": prog.xp_mul,
            "scaling": scaling,
            "problems": prog.problems(),
        }))
    }

    /// One dialogue line checked as the validator checks it, row by row.
    pub fn check_dialog(&self, line_json: &str) -> Result<Value> {
        let line: DialogLine = serde_json::from_str(line_json).context("line does not parse")?;
        let rows = |text: &str| -> Vec<Vec<Value>> {
            dialog::pages(text)
                .iter()
                .map(|page| {
                    page.iter()
                        .map(|row| {
                            let n = row.chars().count();
                            let bad: Vec<String> = match dialog::encode_markup(row) {
                                Ok(_) => Vec::new(),
                                Err(issues) => issues.iter().map(|i| i.to_string()).collect(),
                            };
                            json!({
                                "text": row,
                                "chars": n,
                                "long": n > ROW_WARN_CHARS,
                                "issues": bad,
                            })
                        })
                        .collect()
                })
                .collect()
        };
        let choices: Vec<Value> = line
            .choices
            .iter()
            .map(|c| {
                json!({
                    "label": rows(&c.label),
                    "reply": c.reply.as_deref().map(rows),
                })
            })
            .collect();
        let compiled = dialog::compile_line(&line);
        Ok(json!({
            "pages": rows(&line.text),
            "choices": choices,
            "row_limit": ROW_WARN_CHARS,
            "bytes": compiled.as_ref().map(Vec::len).ok(),
            "error": compiled.err().map(|e| e.to_string()),
        }))
    }

    /// The text timeline of cutscene `id` of `pack_json`, played on the
    /// engine's sequencer against a stand-in world
    /// (`legaia_engine_expansion::cutscene::preview`): `{ rows: [{ frame,
    /// seconds, track, step, kind, text }], frames, ended, fight, flags }`.
    /// `picks` is the option taken at each menu, in order.
    pub fn cutscene_timeline(&self, pack_json: &str, id: &str, picks: &[usize]) -> Result<Value> {
        let pack = self.pack_from_json(pack_json)?;
        let v = legaia_engine_expansion::cutscene::preview::preview(&pack, id, picks, TIMELINE_CAP)
            .with_context(|| format!("no cutscene {id:?}"))?;
        let rows: Vec<Value> = v
            .rows
            .iter()
            .map(|r| {
                json!({
                    "frame": r.frame,
                    "seconds": f64::from(r.frame) / 60.0,
                    "track": r.track,
                    "step": r.step,
                    "kind": r.kind,
                    "text": r.text,
                })
            })
            .collect();
        Ok(json!({
            "rows": rows,
            "frames": v.frames,
            "ended": v.ended,
            "fight": v.fight,
            "flags": v.flags,
        }))
    }

    /// What the engine makes of a map's tile layer: per tile the floor tier
    /// and wall quarters it bakes into the collision grid, and per cell the
    /// four corner heights the floor sampler interpolates between (a cell is
    /// flat only where they agree). Read back from the synthetic field image
    /// the engine itself builds, so the painter shows the engine's rule, not
    /// a copy of it.
    pub fn map_preview(&self, map_json: &str) -> Value {
        match self.map_preview_inner(map_json) {
            Ok(v) => v,
            Err(e) => json!({ "error": format!("{e:#}") }),
        }
    }

    fn map_preview_inner(&self, map_json: &str) -> Result<Value> {
        let def: MapDef = serde_json::from_str(map_json).context("map does not parse")?;
        let grid = def.tile_grid()?;
        let base = def
            .base
            .scene_name()
            .and_then(|n| self.catalog.as_ref()?.base_field(n));
        let (base_map, base_lut) = base.unwrap_or((None, None));
        // The collision grid is all the preview reads; what draws on a tile
        // does not change it, so a ground or prop the disc would have to
        // resolve is dropped rather than failing the preview.
        let mut probe = def.clone();
        if base_map.is_none() {
            probe.base.geometry = BaseGeometry::None;
            for style in probe.tiles.legend.values_mut() {
                if matches!(style.ground, Some(Ground::BaseCell { .. })) {
                    style.ground = None;
                }
            }
        }
        for prop in &mut probe.props {
            if !matches!(prop.mesh, PropMesh::Object { .. }) {
                prop.mesh = PropMesh::Object { object: 0 };
            }
        }
        // Decorations draw only, and bake from mesh bytes the preview does
        // not load.
        probe.decor.clear();
        probe.scatter.clear();
        probe.edges.clear();
        probe.decals.clear();
        probe.emitters.clear();
        let img = build_field_map_image(
            &probe,
            &grid,
            BaseInputs {
                base_map: base_map.as_deref(),
                geometry_entry: None,
                // Wall boxes do not touch the collision grid; any pack
                // length places them.
                geometry_len: Some(0),
                library: None,
            },
        )?;
        let ladder = floor_lut(&def, base_lut);
        let [ox, oy] = def.tiles.origin.map(usize::from);
        let byte = |c: usize, r: usize| img[COLLISION_GRID_OFFSET + (oy + r) * GRID_DIM + ox + c];
        let (w, h) = (grid.width, grid.height);
        let mut tier = Vec::with_capacity(w * h);
        let mut walls = Vec::with_capacity(w * h);
        let mut void = Vec::with_capacity(w * h);
        let mut corners = Vec::with_capacity(w * h);
        let mut flat = Vec::with_capacity(w * h);
        for r in 0..h {
            for c in 0..w {
                let t = grid.get(c, r).expect("in range");
                tier.push(byte(c, r) & 0x0F);
                walls.push(byte(c, r + 1) >> 4);
                void.push(t.void);
                let k = [
                    byte(c, r) & 0x0F,
                    byte(c + 1, r) & 0x0F,
                    byte(c, r + 1) & 0x0F,
                    byte(c + 1, r + 1) & 0x0F,
                ];
                flat.push(k.iter().all(|&x| x == k[0]));
                corners.push(k.map(|n| ladder[usize::from(n)]));
            }
        }
        let off_grid: Vec<Value> = def
            .off_grid_refs(&grid)
            .into_iter()
            .map(|(what, c, r)| json!({ "what": what, "col": c, "row": r }))
            .collect();
        Ok(json!({
            "width": w,
            "height": h,
            "tier": tier,
            "walls": walls,
            "void": void,
            "corners": corners,
            "flat": flat,
            "ladder": ladder.to_vec(),
            "ladder_from_base": def.floor_heights.is_empty() && base_lut.is_some(),
            "tiers": FLOOR_TIERS,
            "off_grid": off_grid,
            // Standable prop tops in map-local tiles: `[col0, row0, col1,
            // row1, top]`, the painter's overlay of what action movement
            // stands on.
            "tops": def
                .floor_tops()
                .iter()
                .map(|t| {
                    let to_tile = |w: i32, o: u8| f64::from(w) / 128.0 - f64::from(o);
                    json!([
                        to_tile(t.rect[0], def.tiles.origin[0]),
                        to_tile(t.rect[1], def.tiles.origin[1]),
                        to_tile(t.rect[2], def.tiles.origin[0]),
                        to_tile(t.rect[3], def.tiles.origin[1]),
                        t.top,
                    ])
                })
                .collect::<Vec<_>>(),
            // Every decoration and decal at its position, map-local tiles
            // `[x, z, kind]` (`0` a decoration, `1` a decal), so the painter
            // draws what the map bakes; empty when a brush does not expand.
            "decor": def
                .decorations()
                .map(|d| {
                    d.iter()
                        .map(|x| {
                            json!([
                                f64::from(x.tile[0]) + 0.5 + f64::from(x.offset[0]) / 128.0,
                                f64::from(x.tile[1]) + 0.5 + f64::from(x.offset[2]) / 128.0,
                                0
                            ])
                        })
                        .chain(def.decals().unwrap_or_default().iter().map(|x| {
                            json!([
                                f64::from(x.tile[0]) + 0.5 + f64::from(x.offset[0]) / 128.0,
                                f64::from(x.tile[1]) + 0.5 + f64::from(x.offset[2]) / 128.0,
                                1
                            ])
                        }))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default(),
        }))
    }
}

fn is_doc(path: &str) -> bool {
    legaia_expansion_format::DOC_FILES.contains(&path)
}

/// Whether the file at `path` holds the same value in `a` and `b`.
fn unchanged(path: &str, a: &ExpansionPack, b: &ExpansionPack) -> bool {
    match path {
        "pack.toml" => a.manifest == b.manifest,
        "battle.toml" => a.battle == b.battle,
        "progression.toml" => a.progression == b.progression,
        "quests.toml" => a.quests == b.quests,
        "dialog.toml" => a.dialog == b.dialog,
        "cards.toml" => a.cards == b.cards,
        "modes.toml" => a.modes == b.modes,
        "items.toml" => a.items == b.items,
        _ => match path
            .strip_prefix("maps/")
            .and_then(|p| p.strip_suffix(".toml"))
        {
            Some(id) => a.maps.contains_key(id) && a.maps.get(id) == b.maps.get(id),
            // Assets and documentation are carried, never edited.
            None => true,
        },
    }
}

/// Drop one leading directory every path shares, unless the tree already
/// has `pack.toml` at its root.
fn strip_common_root(files: PackFiles) -> PackFiles {
    if files.contains_key("pack.toml") {
        return files;
    }
    let Some(root) = files
        .keys()
        .next()
        .and_then(|p| p.split_once('/'))
        .map(|(r, _)| format!("{r}/"))
    else {
        return files;
    };
    if !files.keys().all(|p| p.starts_with(&root)) {
        return files;
    }
    files
        .into_iter()
        .map(|(p, b)| (p[root.len()..].to_string(), b))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_folder_pick_loses_its_root_directory() {
        let mut f = PackFiles::new();
        f.insert("rebirth/pack.toml".into(), b"x".to_vec());
        f.insert("rebirth/maps/a.toml".into(), b"y".to_vec());
        let s = strip_common_root(f);
        assert!(s.contains_key("pack.toml") && s.contains_key("maps/a.toml"));
    }

    #[test]
    fn a_fresh_pack_validates_clean_of_errors() {
        let mut core = Core::new();
        let pack = core.start_fresh("tower", "The Tower");
        let issues = core.validate(&pack.to_string());
        assert!(issues.iter().all(|i| i.severity != "error"), "{issues:?}");
        let bytes = core.export_bundle(&pack.to_string()).unwrap();
        assert!(bundle::is_bundle(&bytes));
    }

    #[test]
    fn bad_editor_json_is_one_issue() {
        let core = Core::new();
        let issues = core.validate("{\"manifest\": 3}");
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].path, EDITOR_PATH);
    }
}
