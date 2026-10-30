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
//!   re-serialised. "Unchanged" is decided on the canonical form: the file
//!   [`ExpansionPack::to_files`] writes for the edited pack against the one
//!   it writes for the loaded pack, so every section (and every section a
//!   later format adds) is covered by the one test, and an edit that is
//!   reverted exports the loaded bytes again.

use std::collections::HashMap;

use anyhow::{Context, Result, bail};
use serde::Serialize;
use serde_json::{Value, json};

use legaia_engine_expansion::progression::{enemy_scaling_permille, xp_table};
use legaia_engine_scene::expansion_map::{BaseInputs, KeptFloor, build_field_map_image, floor_lut};
use legaia_expansion_format::dialog::{self, DialogLine, ROW_WARN_CHARS};
use legaia_expansion_format::manifest::PackManifest;
use legaia_expansion_format::map::{BaseGeometry, FLOOR_TIERS, Ground, MapDef, PropMesh};
use legaia_expansion_format::progression::{MAX_LEVEL, ProgressionSection};
use legaia_expansion_format::validate::{self, Issue, Severity};
use legaia_expansion_format::{ExpansionPack, PackFiles, bundle, diff, share};
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
    /// The canonical files (`to_files`) of the pack `original` parses to,
    /// for the unchanged-file test.
    original_canonical: PackFiles,
    /// Files handed over one by one before [`Self::finish_files`].
    staged: PackFiles,
    /// Base-scene geometry packs already assembled, by scene name.
    objects: HashMap<String, SceneObjects>,
    /// The disc files the catalog was built from, kept by
    /// [`Self::open_disc_owned`] until [`Self::take_disc_parts`] hands them
    /// to a second session (the page's validation worker).
    disc_parts: Option<Vec<u8>>,
}

/// One length-prefixed blob of the session encodings (`u32::MAX` for none).
fn put_part(out: &mut Vec<u8>, part: Option<&[u8]>) {
    match part {
        Some(b) => {
            out.extend_from_slice(&(b.len() as u32).to_le_bytes());
            out.extend_from_slice(b);
        }
        None => out.extend_from_slice(&u32::MAX.to_le_bytes()),
    }
}

fn take_u32(at: &mut &[u8]) -> Result<u32> {
    let (head, rest) = at
        .split_first_chunk::<4>()
        .context("truncated session data")?;
    *at = rest;
    Ok(u32::from_le_bytes(*head))
}

fn take_part(at: &mut &[u8]) -> Result<Option<Vec<u8>>> {
    let n = take_u32(at)?;
    if n == u32::MAX {
        return Ok(None);
    }
    let n = n as usize;
    if at.len() < n {
        bail!("truncated session data");
    }
    let (b, rest) = at.split_at(n);
    *at = rest;
    Ok(Some(b.to_vec()))
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

    /// [`Self::open_disc`] over an image the session may consume; with
    /// `keep_parts` the three disc files the catalog reads (`PROT.DAT`,
    /// `CDNAME.TXT`, `SCUS_942.54`) are kept, encoded, for
    /// [`Self::take_disc_parts`]. The image is dropped before the copy, so
    /// the copy reuses its memory.
    pub fn open_disc_owned(&mut self, image: Vec<u8>, keep_parts: bool) -> Result<()> {
        let prot = legaia_iso::image::extract_prot_dat(&image)
            .context("PROT.DAT not found in the disc image (is it a Mode 2/2352 .bin?)")?;
        let cdname = legaia_iso::image::extract_cdname_txt(&image);
        let scus = legaia_iso::image::extract_scus(&image);
        drop(image);
        self.disc_parts = keep_parts.then(|| {
            let mut out = Vec::with_capacity(prot.len() + 0x8_0000);
            put_part(&mut out, Some(&prot));
            put_part(&mut out, cdname.as_deref().map(str::as_bytes));
            put_part(&mut out, scus.as_deref());
            out
        });
        self.open_disc_files(prot, cdname.as_deref(), scus.as_deref())
    }

    fn open_disc_files(
        &mut self,
        prot: Vec<u8>,
        cdname: Option<&str>,
        scus: Option<&[u8]>,
    ) -> Result<()> {
        let index = legaia_engine_scene::scene::ProtIndex::from_bytes(prot, cdname)
            .context("PROT index")?;
        self.catalog = Some(DiscCatalog::from_parts(index, scus)?);
        self.objects.clear();
        Ok(())
    }

    /// The disc files [`Self::open_disc_owned`] kept, once.
    pub fn take_disc_parts(&mut self) -> Option<Vec<u8>> {
        self.disc_parts.take()
    }

    /// Open the disc from [`Self::take_disc_parts`]'s bytes: the same
    /// catalog, without the image.
    pub fn open_disc_parts(&mut self, parts: &[u8]) -> Result<()> {
        let mut at = parts;
        let prot = take_part(&mut at)?.context("the disc parts hold no PROT.DAT")?;
        let cdname = take_part(&mut at)?
            .map(String::from_utf8)
            .transpose()
            .context("CDNAME.TXT")?;
        let scus = take_part(&mut at)?;
        self.open_disc_files(prot, cdname.as_deref(), scus.as_deref())
    }

    /// The session's files besides the pack JSON - the loaded tree, its
    /// canonical form, the assets and documentation - encoded for
    /// [`Self::set_session_files`] on a second session, which then
    /// validates exactly as this one does.
    pub fn session_files(&self) -> Vec<u8> {
        let mut out = Vec::new();
        for set in [&self.original, &self.original_canonical, &self.extras] {
            out.extend_from_slice(&(set.len() as u32).to_le_bytes());
            for (path, bytes) in set {
                put_part(&mut out, Some(path.as_bytes()));
                put_part(&mut out, Some(bytes));
            }
        }
        out
    }

    /// Take over [`Self::session_files`] from another session.
    pub fn set_session_files(&mut self, data: &[u8]) -> Result<()> {
        let mut at = data;
        let mut sets: [PackFiles; 3] = Default::default();
        for set in &mut sets {
            let n = take_u32(&mut at)?;
            for _ in 0..n {
                let path = take_part(&mut at)?.context("a session file without a path")?;
                let bytes = take_part(&mut at)?.unwrap_or_default();
                set.insert(
                    String::from_utf8(path).context("a session file path")?,
                    bytes,
                );
            }
        }
        let [original, canonical, extras] = sets;
        self.original = original;
        self.original_canonical = canonical;
        self.extras = extras;
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

    /// The bytes of asset `path` (relative to `assets/`), for the page to
    /// preview: an audio file plays through the browser's own decoder.
    pub fn asset_bytes(&self, path: &str) -> Option<&[u8]> {
        self.extras
            .get(&format!("assets/{path}"))
            .map(Vec::as_slice)
    }

    /// What audio asset `path` is: `{ rate, channels, frames, seconds,
    /// loop }` from the same probe the validator runs, or `{ error }`.
    pub fn audio_info(&self, path: &str) -> Value {
        let Some(bytes) = self.asset_bytes(path) else {
            return json!({ "error": format!("assets/{path} is not in the pack") });
        };
        match legaia_expansion_format::audio_clip::probe(bytes) {
            Ok(i) => json!({
                "rate": i.sample_rate,
                "channels": i.channels,
                "frames": i.frames,
                "seconds": i.seconds(),
                "loop": i.loop_points.map(|(s, e)| [s, e]),
            }),
            Err(e) => json!({ "error": format!("{e:#}") }),
        }
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
            Ok(mut pack) => {
                canonicalise(&mut pack);
                self.original_canonical = pack.to_files().unwrap_or_default();
                self.extras = files
                    .iter()
                    .filter(|(p, _)| p.starts_with("assets/") || is_doc(p))
                    .map(|(p, b)| (p.clone(), b.clone()))
                    .collect();
                self.original = files;
                json!({ "pack": pack, "files": list, "issues": issues })
            }
            Err(_) => json!({ "pack": Value::Null, "files": list, "issues": issues }),
        }
    }

    /// A new pack with only a manifest.
    pub fn start_fresh(&mut self, id: &str, name: &str) -> Value {
        self.extras.clear();
        self.original.clear();
        self.original_canonical.clear();
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
        canonicalise(&mut pack);
        for (path, bytes) in &self.extras {
            if let Some(rel) = path.strip_prefix("assets/") {
                pack.assets.insert(rel.to_string(), bytes.clone());
            } else {
                pack.docs.insert(path.clone(), bytes.clone());
            }
        }
        let assets = pack.assets.clone();
        for m in pack.maps.values_mut() {
            let _ = m.resolve_terrain_images(&assets);
        }
        Ok(pack)
    }

    /// The images `def`'s terrain heightmaps name, out of the pack's
    /// `assets/`, keyed relative to `assets/`.
    fn heightmap_assets(&self, def: &MapDef) -> std::collections::BTreeMap<String, Vec<u8>> {
        let names: Vec<&str> = def
            .terrain
            .iter()
            .flat_map(|t| t.shapes.iter())
            .filter_map(|s| match s {
                legaia_expansion_format::terrain::TerrainShape::Heightmap { image, .. } => {
                    Some(image.strip_prefix("assets/").unwrap_or(image))
                }
                _ => None,
            })
            .collect();
        names
            .into_iter()
            .filter_map(|n| {
                Some((
                    n.to_string(),
                    self.extras.get(&format!("assets/{n}"))?.clone(),
                ))
            })
            .collect()
    }

    /// The file tree the pack exports as. A manifest, section or map whose
    /// value equals what it was loaded as keeps its original bytes - and so
    /// does a section file the pack was loaded with whose value is the
    /// default (an empty `battle.toml`), which [`ExpansionPack::to_files`]
    /// alone would leave out. So an unedited pack exports exactly the layout
    /// files it was loaded from: `.lxpack` in, the same `.lxpack` out.
    pub fn pack_files(&self, pack_json: &str) -> Result<PackFiles> {
        let pack = self.pack_from_json(pack_json)?;
        let mut files = pack.to_files()?;
        for (path, bytes) in files.iter_mut() {
            // Assets and documentation are carried, never edited.
            if path.starts_with("assets/")
                || legaia_expansion_format::DOC_FILES.contains(&path.as_str())
            {
                continue;
            }
            if let Some(o) = self.original.get(path)
                && self.original_canonical.get(path) == Some(bytes)
            {
                bytes.clone_from(o);
            }
        }
        // A section file the pack was loaded with that holds only its
        // default (an empty `battle.toml`) and is still the default: `to_files`
        // writes no file for it, so keep the loaded one. With this an unedited
        // pack exports exactly the layout files it came from.
        for (path, o) in &self.original {
            let section = legaia_expansion_format::SECTION_FILES.contains(&path.as_str());
            if section && !files.contains_key(path) && !self.original_canonical.contains_key(path) {
                files.insert(path.clone(), o.clone());
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

    // --- Sharing -----------------------------------------------------------------

    /// Import a shared `.lxpack` through every gate
    /// ([`legaia_expansion_format::share::import_bundle`]) and load it:
    /// `{ ok: true, pack, files, issues, summary }`, or `{ ok: false,
    /// error: { kind, message } }` naming the gate that refused it
    /// (`not_a_bundle`, `corrupt`, `unsupported_version`, `too_large`,
    /// `bad_path`, `newer_format`, `older_format`, `newer_engine`, `unsafe`,
    /// `unparsable`). A refused bundle leaves the session as it was.
    pub fn import_bundle(&mut self, bytes: &[u8]) -> Value {
        match share::import_bundle(bytes) {
            Err(e) => json!({ "ok": false, "error": e }),
            Ok(imported) => {
                let summary = imported.summary.clone();
                let mut v = self.load_files(imported.files);
                v["ok"] = json!(true);
                v["summary"] = json!(summary);
                v
            }
        }
    }

    /// The pack the page holds against an older version of it (`old`, the
    /// `.lxpack` the page was handed): `{ diff, counts: [added, removed,
    /// changed], bump, suggested_version, changelog, same_pack }`
    /// ([`legaia_expansion_format::diff`]).
    pub fn diff_against(&self, old: &[u8], pack_json: &str) -> Result<Value> {
        let old = share::import_bundle(old)
            .map_err(|e| anyhow::anyhow!("{}", e.message))?
            .pack;
        let new = self.pack_from_json(pack_json)?;
        let d = diff::diff_packs(&old, &new);
        let (a, r, c) = d.counts();
        Ok(json!({
            "old": { "id": old.manifest.id, "version": old.manifest.version },
            "same_pack": old.manifest.id == new.manifest.id,
            "counts": [a, r, c],
            "bump": d.bump(),
            "suggested_version": d.suggested_version(),
            "changelog": d.changelog(),
            "diff": d,
        }))
    }

    /// Put a documentation file (`CHANGELOG.md`, `README.md`, ...) into the
    /// pack, carried with its assets. Refused for any other path.
    pub fn set_doc(&mut self, path: &str, bytes: Vec<u8>) -> Result<()> {
        if !is_doc(path) {
            bail!("{path} is not one of the pack's documentation files");
        }
        self.extras.insert(path.to_string(), bytes);
        Ok(())
    }

    /// A documentation file's text (empty when absent).
    pub fn doc_text(&self, path: &str) -> String {
        self.extras
            .get(path)
            .filter(|_| is_doc(path))
            .map(|b| String::from_utf8_lossy(b).into_owned())
            .unwrap_or_default()
    }

    /// The pack's source tree as a zip under one `<id>/` folder
    /// ([`share::sources_zip`]): the editable form, which unpacks into a
    /// directory Open folder reads back.
    pub fn sources_zip(&self, pack_json: &str) -> Result<Vec<u8>> {
        let files = self.pack_files(pack_json)?;
        let pack = self.pack_from_json(pack_json)?;
        let root = if pack.manifest.id.is_empty() {
            "pack"
        } else {
            pack.manifest.id.as_str()
        };
        Ok(share::sources_zip(root, &files)?)
    }

    /// The pack's identity as the sharing panel shows it: can this engine
    /// load it, what it needs, its cover's size. `{ engine, schema,
    /// schema_min, schema_max, compatible, why, cover_bytes, requires:
    /// [{ text, id, range, ok }], conflicts: [...] }`.
    pub fn share_status(&self, pack_json: &str) -> Result<Value> {
        use legaia_expansion_format::manifest::{
            ENGINE_VERSION, MIN_SCHEMA_VERSION, PackDep, SCHEMA_VERSION,
        };
        let pack = self.pack_from_json(pack_json)?;
        let m = &pack.manifest;
        let deps = |list: &[String]| -> Vec<Value> {
            list.iter()
                .map(|t| match PackDep::parse(t) {
                    Ok(d) => json!({
                        "text": t, "id": d.id,
                        "range": d.version.map(|r| r.to_string()), "ok": true,
                    }),
                    Err(e) => json!({ "text": t, "ok": false, "error": e }),
                })
                .collect()
        };
        let cover = m
            .cover
            .as_ref()
            .and_then(|c| c.strip_prefix("assets/"))
            .and_then(|c| pack.assets.get(c))
            .map(Vec::len);
        let compat = m.compatibility();
        Ok(json!({
            "engine": ENGINE_VERSION,
            "schema": m.schema,
            "schema_min": MIN_SCHEMA_VERSION,
            "schema_max": SCHEMA_VERSION,
            "compatible": compat.is_ok(),
            "why": compat.err().map(|e| e.to_string()),
            "cover_bytes": cover,
            "requires": deps(&m.requires),
            "conflicts": deps(&m.conflicts),
        }))
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

    /// A pack art's damage preview through the retail melee kernel
    /// (`battle_formulas::physical_excess`, `FUN_801EC3E4`): per hit beat, the
    /// low / mean / high damage over 64 rolls for an attacker of `atk` (the
    /// art folds half the weapon in - pass the folded figure) against a
    /// defence of `def`, after the art's `damage_pct`; `beats` is how many
    /// power bytes each segment names (the clip's own beats are the disc's).
    pub fn art_preview(&self, art_json: &str, atk: u16, def: u16) -> Result<Value> {
        use legaia_engine_battle_vm::battle_formulas::{
            PhysicalHit, command_power_scalar, physical_excess,
        };
        let art: legaia_expansion_format::arts::ArtDef =
            serde_json::from_str(art_json).context("art does not parse")?;
        let mut seed: u32 = 0x1234_5678;
        let mut rand = move || {
            seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12_345);
            ((seed >> 16) & 0x7FFF) as u16
        };
        let mut hits = Vec::new();
        let mut total = [0u32; 3];
        for (si, s) in art.segments.iter().enumerate() {
            for p in &s.power {
                let Some(b) = p.byte() else { continue };
                let hit = PhysicalHit {
                    attacker_atk: atk,
                    attacker_hp: 0,
                    defender_def: def,
                    command_scalar: command_power_scalar(b),
                    staged_anim: 0x1B,
                    art_power_bit: false,
                    affinity_pct: 100,
                    attacker_status: 0,
                    defender_status: 0,
                    defender_guarding: false,
                    combo_scale: 1,
                    attack_ramp: 0,
                    guard_ramp: 0,
                };
                let rolls: Vec<u32> = (0..64)
                    .map(|_| {
                        let raw = physical_excess(&hit, &mut rand).min(9999);
                        (raw * u32::from(art.damage_pct) / 100).min(9999)
                    })
                    .collect();
                let lo = *rolls.iter().min().unwrap_or(&0);
                let hi = *rolls.iter().max().unwrap_or(&0);
                let mean = rolls.iter().sum::<u32>() / rolls.len().max(1) as u32;
                for (t, v) in total.iter_mut().zip([lo, mean, hi]) {
                    *t += v;
                }
                hits.push(json!({
                    "segment": si,
                    "power": legaia_expansion_format::arts::power_label(b),
                    "low": lo,
                    "mean": mean,
                    "high": hi,
                }));
            }
        }
        Ok(json!({ "hits": hits, "low": total[0], "mean": total[1], "high": total[2] }))
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
        let mut def: MapDef = serde_json::from_str(map_json).context("map does not parse")?;
        let heightmaps = def.resolve_terrain_images(&self.heightmap_assets(&def));
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
        // The terrain's own meshes bake from nothing the preview loads
        // either; its walls and floor come back below, from the same pure
        // functions the engine's image builder runs.
        let terrain_walls = match &def.terrain {
            Some(_) => Some(def.terrain_walls(&grid)?),
            None => None,
        };
        let terrain_field = def.terrain_heightfield()?;
        probe.terrain = None;
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
        // A kept room's own floor steps (stairs, a dais, terraces): the
        // floor the player walks there and a prop's `offset[1]` stands on.
        let kept = KeptFloor::new(&def, &grid, base_map.as_deref());
        let [ox, oy] = def.tiles.origin.map(usize::from);
        let byte = |c: usize, r: usize| img[COLLISION_GRID_OFFSET + (oy + r) * GRID_DIM + ox + c];
        let (w, h) = (grid.width, grid.height);
        let mut tier = Vec::with_capacity(w * h);
        let mut walls = Vec::with_capacity(w * h);
        let mut void = Vec::with_capacity(w * h);
        let mut corners = Vec::with_capacity(w * h);
        let mut flat = Vec::with_capacity(w * h);
        let mut floor = Vec::with_capacity(w * h);
        for r in 0..h {
            for c in 0..w {
                let t = grid.get(c, r).expect("in range");
                tier.push(if terrain_walls.is_some() {
                    0
                } else {
                    byte(c, r) & 0x0F
                });
                walls.push(
                    (byte(c, r + 1) >> 4)
                        | terrain_walls.as_ref().map_or(0, |w| w[r * grid.width + c]),
                );
                void.push(t.void);
                let k = [
                    byte(c, r) & 0x0F,
                    byte(c + 1, r) & 0x0F,
                    byte(c, r + 1) & 0x0F,
                    byte(c + 1, r + 1) & 0x0F,
                ];
                flat.push(k.iter().all(|&x| x == k[0]));
                let t0 = if terrain_walls.is_some() {
                    0
                } else {
                    byte(c, r) & 0x0F
                };
                floor.push(
                    i32::from(ladder[usize::from(t0)])
                        + kept.step_up_on(&def, c as u16, r as u16, 0, 0),
                );
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
            // The floor at each tile's centre, world units up: its tier
            // plus, in a kept room, the retail floor step there - what the
            // player stands on and what a prop's `offset[1]` counts from.
            "floor": floor,
            "kept_steps": kept.records().len(),
            "ladder": ladder.to_vec(),
            "ladder_from_base": def.floor_heights.is_empty() && base_lut.is_some(),
            "tiers": FLOOR_TIERS,
            "off_grid": off_grid,
            // The sculpted floor (`[terrain]`): the heightfield over the
            // map and its skirt, `cols x rows` vertices `res` a tile, and
            // the water level, for the painter's relief shading.
            "terrain": terrain_field.as_ref().map(|f| json!({
                "cols": f.cols,
                "rows": f.rows,
                "res": f.res,
                "skirt": f.skirt,
                "step": f.step,
                "origin": f.origin,
                "heights": f.heights,
                "problems": heightmaps,
                "water": def.terrain.as_ref().and_then(|t| t.water.as_ref()).map(|w| w.level),
            })),
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

/// The one form of a value the editor can spell two ways, so the
/// unchanged-file test and the export agree whichever the page wrote: a shop
/// row with no gate and no price of its own is a bare item reference (the
/// Items tab edits every row as `{ item }`).
fn canonicalise(pack: &mut ExpansionPack) {
    use legaia_expansion_format::items::StockEntry;
    for shop in &mut pack.items.shops {
        for row in &mut shop.stock {
            if let StockEntry::Gated {
                item,
                requires,
                price: None,
            } = row
                && requires.is_empty()
            {
                *row = StockEntry::Item(item.clone());
            }
        }
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
