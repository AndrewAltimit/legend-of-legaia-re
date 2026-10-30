//! Expansion packs on the browser play page: load an `.lxpack` bundle,
//! activate it through the one activation path both hosts share
//! (`SceneHost::activate_expansion_pack` -> `World::activate_expansion`),
//! and enter its maps as live field scenes.
//!
//! The page hands over the bundle's bytes (a file input, or the hand-off
//! slot the expansion workbench fills - IndexedDB database
//! `legaia-expansion`, store `packs`, key `play`). The pack is validated the
//! way the native launcher validates it; a pack with errors is refused and
//! the errors come back for the page to show. Once active, everything else
//! is the engine's: New Game and Continue play with the pack, its entrances
//! fire from the retail scenes they stand in, Select on the pause menu opens
//! its Journal, and quest toasts ride the banner rows.
//!
//! A pack map is the base scene's ordinary field entry with the pack's
//! field layer laid over it (`SceneHost::enter_expansion_map`), so the page's
//! scene build ([`LegaiaRuntime::rebuild_render_state`]) reads the map's
//! ground, walls and props off the same `Scene` it reads a retail scene's
//! from.

use std::sync::Arc;

use legaia_expansion_format::ExpansionPack;
use legaia_expansion_format::validate::{Severity, validate_files};
use serde_json::json;
use wasm_bindgen::prelude::*;

use crate::runtime::LegaiaRuntime;

/// What [`LegaiaRuntime::load_expansion`] reports.
fn info_json(pack: Option<&ExpansionPack>, errors: &[String], warnings: &[String]) -> String {
    let manifest = pack.map(|p| {
        json!({
            "id": p.manifest.id,
            "name": p.manifest.name,
            "version": p.manifest.version,
            "authors": p.manifest.authors,
            "description": p.manifest.description,
            "license": p.manifest.license,
            "schema": p.manifest.schema,
            "min_engine": p.manifest.min_engine,
            "cover": p.manifest.cover,
            "maps": p.maps.iter().map(|(id, m)| json!({ "id": id, "name": m.name })).collect::<Vec<_>>(),
            // The instances played in the world - at a table or on a
            // course of one of the pack's maps - for the page's 3D list.
            "world_modes": legaia_engine_modes::active::entries(&p.modes)
                .into_iter()
                .filter_map(|e| {
                    let (map, _) = legaia_engine_modes::active::instance_map(&p.modes, &e.id)?;
                    Some(json!({ "id": e.id, "name": e.name, "kind": e.kind.label(), "map": map }))
                })
                .collect::<Vec<_>>(),
            "quests": p.quests.quests.len(),
            "challenges": p.quests.challenges.len(),
        })
    });
    json!({
        "ok": pack.is_some() && errors.is_empty(),
        "pack": manifest,
        "errors": errors,
        "warnings": warnings,
    })
    .to_string()
}

/// The pack library's dependency report (`LegaiaRuntime::expansion_library_check`):
/// each installed pack's `requires` and `conflicts` checked against every
/// other installed pack by the format's own rule
/// (`legaia_expansion_format::manifest::check_dependencies`).
///
/// `library` is `{ active, packs: [{ id, version, requires, conflicts }] }`.
/// The answer maps each pack id to `{ problems: [{ kind, dep, installed,
/// active, text }], uses: [..] }`: `kind` is `missing`, `wrong_version` or
/// `conflict`, `active` marks a conflict with the active pack, and `uses`
/// lists the requirements an installed pack does meet. The engine plays one
/// pack at a time, so a met requirement is recorded, not loaded beside it.
pub fn library_dependency_report(library: &serde_json::Value) -> serde_json::Value {
    use legaia_expansion_format::manifest::{DependencyProblem, PackManifest, check_dependencies};
    let strings = |v: &serde_json::Value| -> Vec<String> {
        v.as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|s| s.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default()
    };
    let active = library["active"].as_str();
    let packs: Vec<(String, String, Vec<String>, Vec<String>)> = library["packs"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|p| {
                    Some((
                        p["id"].as_str()?.to_string(),
                        p["version"].as_str().unwrap_or_default().to_string(),
                        strings(&p["requires"]),
                        strings(&p["conflicts"]),
                    ))
                })
                .collect()
        })
        .unwrap_or_default();
    let installed: Vec<(&str, &str)> = packs
        .iter()
        .map(|(id, v, _, _)| (id.as_str(), v.as_str()))
        .collect();
    let mut out = serde_json::Map::new();
    for (id, version, requires, conflicts) in &packs {
        let m = PackManifest {
            id: id.clone(),
            version: version.clone(),
            requires: requires.clone(),
            conflicts: conflicts.clone(),
            ..PackManifest::default()
        };
        let problems: Vec<serde_json::Value> = check_dependencies(&m, &installed)
            .into_iter()
            .map(|p| {
                let text = p.to_string();
                let (kind, dep, installed) = match p {
                    DependencyProblem::Missing { dep } => ("missing", dep, None),
                    DependencyProblem::WrongVersion { dep, installed } => {
                        ("wrong_version", dep, Some(installed))
                    }
                    DependencyProblem::Conflict { dep, installed } => {
                        ("conflict", dep, Some(installed))
                    }
                };
                let dep_id = dep.split('@').next().unwrap_or_default().to_string();
                json!({
                    "kind": kind,
                    "dep": dep,
                    "installed": installed,
                    "active": kind == "conflict" && Some(dep_id.as_str()) == active,
                    "text": text,
                })
            })
            .collect();
        let uses: Vec<serde_json::Value> = m
            .required()
            .into_iter()
            .filter_map(|d| {
                let (_, v) = installed
                    .iter()
                    .find(|(i, v)| *i != id.as_str() && d.matches(i, v))?;
                Some(json!({ "dep": d.to_string(), "installed": v }))
            })
            .collect();
        out.insert(id.clone(), json!({ "problems": problems, "uses": uses }));
    }
    serde_json::Value::Object(out)
}

/// A pack tree (`path -> bytes`, the files of a pack folder) as the
/// `.lxpack` bytes `asset expansion bundle` writes for it: only the pack
/// layout's files ([`legaia_expansion_format::is_known_path`]), through the
/// format's one deterministic writer. The play page builds the built-in
/// Rebirth this way from the tree the site serves (`packs/rebirth.json`), so
/// the browser plays the same bytes the release archive ships.
pub fn bundle_pack_tree(
    files: impl IntoIterator<Item = (String, Vec<u8>)>,
) -> anyhow::Result<Vec<u8>> {
    let files: legaia_expansion_format::PackFiles = files
        .into_iter()
        .filter(|(p, _)| legaia_expansion_format::is_known_path(p))
        .collect();
    legaia_expansion_format::bundle::write_bundle(&files)
}

#[wasm_bindgen]
impl LegaiaRuntime {
    /// [`bundle_pack_tree`] for the page: `paths[i]` names the file whose
    /// bytes are `data[i]` (a `Uint8Array`). Needs no disc; the bundle is
    /// checked when [`Self::load_expansion`] loads it.
    pub fn expansion_bundle_from_files(
        paths: Vec<String>,
        data: js_sys::Array,
    ) -> Result<Vec<u8>, JsValue> {
        if paths.len() != data.length() as usize {
            return Err(JsValue::from_str("paths and data differ in length"));
        }
        let tree = paths
            .into_iter()
            .zip(data.iter())
            .map(|(p, d)| (p, js_sys::Uint8Array::new(&d).to_vec()));
        bundle_pack_tree(tree).map_err(|e| JsValue::from_str(&format!("{e:#}")))
    }

    /// The pack library's dependency check ([`library_dependency_report`]):
    /// `library` JSON in, the per-pack report JSON out (`{}` on bad input).
    pub fn expansion_library_check(&self, library: &str) -> String {
        serde_json::from_str::<serde_json::Value>(library)
            .map(|v| library_dependency_report(&v))
            .unwrap_or_else(|_| json!({}))
            .to_string()
    }

    /// Load an expansion pack from its `.lxpack` bundle bytes and make it the
    /// active pack. Needs a loaded disc ([`Self::load_disc`]): activation
    /// reads the disc's progression tables. Returns JSON
    /// `{ ok, pack: { id, name, version, authors, description, maps, quests,
    /// challenges } | null, errors: [..], warnings: [..] }`; with any error
    /// the pack is not activated (and any pack already active stays).
    pub fn load_expansion(&mut self, bytes: &[u8]) -> String {
        self.load_expansion_core(bytes)
    }

    /// What a pack library lists for `.lxpack` bytes, read through every
    /// import gate (`legaia_expansion_format::share::import_bundle`) without
    /// activating anything or needing a disc: `{ ok: true, summary: { id,
    /// name, version, authors, description, license, homepage, tags, schema,
    /// min_engine, requires, conflicts, cover, files, bytes, maps, quests,
    /// cards }, errors: [..] }` (`errors` the validator's, which keep the
    /// pack from activating), or `{ ok: false, error: { kind, message } }`.
    pub fn expansion_bundle_info(&self, bytes: &[u8]) -> String {
        match legaia_expansion_format::share::import_bundle(bytes) {
            Err(e) => json!({ "ok": false, "error": e }).to_string(),
            Ok(i) => {
                let issues = validate_files(&i.files, None);
                json!({
                    "ok": true,
                    "summary": i.summary,
                    "errors": legaia_expansion_format::share::error_lines(&issues),
                })
                .to_string()
            }
        }
    }

    /// The cover image (PNG bytes) of an `.lxpack`, empty when it has none
    /// or does not import.
    pub fn expansion_bundle_cover(&self, bytes: &[u8]) -> Vec<u8> {
        legaia_expansion_format::share::import_bundle(bytes)
            .ok()
            .and_then(|i| i.cover().map(<[u8]>::to_vec))
            .unwrap_or_default()
    }

    /// Drop the active expansion pack: the game as it shipped. Its progress
    /// is kept dormant and written back into the next save.
    pub fn unload_expansion(&mut self) {
        if let Some(host) = self.scene_host.host_mut() {
            host.world.deactivate_expansion();
        }
    }

    /// The active pack as [`Self::load_expansion`] reports it (`ok: false`,
    /// `pack: null` with none), plus `dormant`: the id of a pack a loaded
    /// save names that is not the active one.
    pub fn expansion_info_json(&self) -> String {
        let Some(host) = self.scene_host.host() else {
            return info_json(None, &[], &[]);
        };
        let mut v: serde_json::Value =
            serde_json::from_str(&info_json(host.world.expansion.pack.as_deref(), &[], &[]))
                .unwrap_or_default();
        v["dormant"] = json!(host.world.expansion_dormant_pack_id());
        v["map"] = json!(host.active_expansion_map());
        // A loaded save made with another version of the pack: what it was
        // made with and what was dropped (`legaia_engine_expansion::save_compat`).
        v["compat"] = match host.world.expansion_compat_report() {
            Some(r) => json!({
                "saved_version": r.saved_version,
                "pack_version": r.pack_version,
                "notice": r.notice(),
                "lines": r.lines(),
            }),
            None => serde_json::Value::Null,
        };
        v.to_string()
    }

    /// Enter map `map_id` of the active pack at spawn `spawn` (the map's
    /// first when empty) as a live field scene - the page twin of the native
    /// `play-window --expansion <pack> --map <id>`. Returns the state JSON
    /// [`Self::enter_field`] returns.
    pub fn enter_expansion_map(&mut self, map_id: &str, spawn: &str) -> Result<String, JsValue> {
        self.enter_expansion_map_core(map_id, spawn)
            .map_err(|e| JsValue::from_str(&e))
    }
}

impl LegaiaRuntime {
    /// JsValue-free body of [`Self::load_expansion`].
    pub(crate) fn load_expansion_core(&mut self, bytes: &[u8]) -> String {
        // Every import gate first (bundle integrity, trust, the pack-format
        // and engine versions), each refusal worded for the player.
        let files = match legaia_expansion_format::share::import_bundle(bytes) {
            Ok(i) => i.files,
            Err(e) => return info_json(None, &[e.message], &[]),
        };
        let issues = validate_files(&files, None);
        let line = |i: &legaia_expansion_format::validate::Issue| {
            if i.field.is_empty() {
                format!("{}: {}", i.path, i.message)
            } else {
                format!("{}: {}: {}", i.path, i.field, i.message)
            }
        };
        let errors: Vec<String> = issues
            .iter()
            .filter(|i| i.severity == Severity::Error)
            .map(line)
            .collect();
        let warnings: Vec<String> = issues
            .iter()
            .filter(|i| i.severity == Severity::Warning)
            .map(line)
            .collect();
        let pack = match ExpansionPack::from_files(&files) {
            Ok(p) => p,
            Err(e) => {
                let mut errors = errors;
                errors.push(format!("{e:#}"));
                return info_json(None, &errors, &warnings);
            }
        };
        if !errors.is_empty() {
            return info_json(Some(&pack), &errors, &warnings);
        }
        let Some(host) = self.scene_host.host_mut() else {
            return info_json(
                Some(&pack),
                &["load the disc first: a pack activates against it".into()],
                &warnings,
            );
        };
        let pack = Arc::new(pack);
        if let Err(e) = host.activate_expansion_pack(pack.clone()) {
            return info_json(Some(&pack), &[format!("{e:#}")], &warnings);
        }
        info_json(Some(&pack), &[], &warnings)
    }

    /// JsValue-free body of [`Self::enter_expansion_map`]: the staging
    /// [`Self::enter_field_core`] runs around a scene entry, around the
    /// pack map's entry instead.
    pub(crate) fn enter_expansion_map_core(
        &mut self,
        map_id: &str,
        spawn: &str,
    ) -> Result<String, String> {
        self.enter_expansion_map_seated_core(map_id, spawn, None)
    }

    /// [`Self::enter_expansion_map_core`] with the player seated at world
    /// `(x, z)` facing `facing` instead of at the spawn
    /// ([`legaia_engine_scene_host::scene_host::SceneHost::enter_expansion_map_seated`]):
    /// the workbench's hot reload re-entering the map where the player stood.
    pub(crate) fn enter_expansion_map_seated_core(
        &mut self,
        map_id: &str,
        spawn: &str,
        seat: Option<(i16, i16, u16)>,
    ) -> Result<String, String> {
        let host = self
            .scene_host
            .host_mut()
            .ok_or_else(|| "enter_expansion_map: call load_disc first".to_string())?;
        host.world.toggles.use_vm_dialogue = true;
        host.world.enable_frame_grab(true);
        host.world.locomotion.follow_terrain_height = true;
        host.world.locomotion.leading_edge_wall_probes = true;
        host.world.npcs.solid = true;
        host.world.npcs.animate = true;
        self.apply_page_toggles();
        let host = self
            .scene_host
            .host_mut()
            .ok_or_else(|| "enter_expansion_map: call load_disc first".to_string())?;
        host.enter_expansion_map_seated(map_id, spawn, seat)
            .map_err(|e| format!("enter_expansion_map({map_id}): {e:#}"))?;
        let base = host
            .scene
            .as_ref()
            .map(|s| s.name.clone())
            .unwrap_or_default();
        let preset = host.world.expansion.map.as_ref().map(|m| m.def.camera);
        self.scene_host.camera_mut().reset_for_scene_entry();
        if let Some(distance) = preset.and_then(camera_distance) {
            self.scene_host.camera_mut().distance = distance;
        }
        self.cutscene_glide.reset();
        let _ = self.seat_enter(legaia_engine_mode::mode::GameMode::MainInit);
        if self.live_battles {
            // The map rolls no retail encounters (its zones name pack
            // tables), and the arm keeps the pack's monsters and formations
            // (`World::arm_live_loop`).
            self.arm_live_battles(&base);
        }
        self.rebuild_render_state()
            .map_err(|_| "enter_expansion_map: the scene build failed".to_string())?;
        self.on_scene_change_audio();
        if let Some(d) = self.scene_host.director_mut() {
            d.last_started = None;
        }
        Ok(self.state_json())
    }
}

/// The follow camera's distance a map's `camera` preset names (`None` keeps
/// the player's own).
fn camera_distance(
    preset: legaia_expansion_format::map::CameraPreset,
) -> Option<legaia_engine_camera::camera_distance::CameraDistance> {
    use legaia_engine_camera::camera_distance::CameraDistance;
    use legaia_expansion_format::map::CameraPreset;
    match preset {
        CameraPreset::Default => None,
        CameraPreset::Retail => Some(CameraDistance::Retail),
        CameraPreset::Far => Some(CameraDistance::Far),
        CameraPreset::Farther => Some(CameraDistance::Farther),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rebirth_bundle() -> Vec<u8> {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../packs/rebirth");
        let files = legaia_expansion_format::read_files(std::path::Path::new(dir)).expect("read");
        legaia_expansion_format::bundle::write_bundle(&files).expect("bundle")
    }

    /// The page's built-in Rebirth: every file of the pack folder (README
    /// included, as the site's `packs/rebirth.json` carries it) bundles to
    /// the bytes `asset expansion bundle` writes for the release archive.
    #[test]
    fn the_page_tree_bundles_like_the_release_cli() {
        let dir = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../packs/rebirth"));
        let mut all = legaia_expansion_format::read_dir_files(dir).expect("read");
        // A stray file outside the layout is dropped, as the CLI drops it.
        all.insert("notes.txt".into(), b"scratch".to_vec());
        let cli: legaia_expansion_format::PackFiles = all
            .iter()
            .filter(|(p, _)| legaia_expansion_format::is_known_path(p))
            .map(|(p, b)| (p.clone(), b.clone()))
            .collect();
        let want = legaia_expansion_format::bundle::write_bundle(&cli).unwrap();
        let mut reversed: Vec<(String, Vec<u8>)> = all.into_iter().collect();
        reversed.reverse();
        let got = bundle_pack_tree(reversed).unwrap();
        assert_eq!(got, want);
        let rt = LegaiaRuntime::new();
        let info: serde_json::Value =
            serde_json::from_str(&rt.expansion_bundle_info(&got)).unwrap();
        assert_eq!(info["ok"], true, "{info}");
        assert_eq!(info["summary"]["id"], "rebirth");
    }

    /// The library's read of a bundle: Rebirth's summary and cover, and a
    /// newer-format pack refused with its gate named.
    #[test]
    fn bundle_info_reads_the_library_fields() {
        let rt = LegaiaRuntime::new();
        let info: serde_json::Value =
            serde_json::from_str(&rt.expansion_bundle_info(&rebirth_bundle())).unwrap();
        assert_eq!(info["ok"], true, "{info}");
        assert_eq!(info["summary"]["id"], "rebirth");
        assert_eq!(info["summary"]["cover"], "assets/cover.png");
        assert!(info["errors"].as_array().unwrap().is_empty());
        assert!(
            rt.expansion_bundle_cover(&rebirth_bundle())
                .starts_with(b"\x89PNG")
        );
        let mut f = legaia_expansion_format::PackFiles::new();
        f.insert(
            "pack.toml".into(),
            b"id = \"n\"\nname = \"N\"\nversion = \"1.0.0\"\nschema = 50\n".to_vec(),
        );
        let newer = legaia_expansion_format::bundle::write_bundle(&f).unwrap();
        let info: serde_json::Value =
            serde_json::from_str(&rt.expansion_bundle_info(&newer)).unwrap();
        assert_eq!(info["error"]["kind"], "newer_format");
        assert!(rt.expansion_bundle_cover(&newer).is_empty());
        let mut rt = rt;
        let loaded: serde_json::Value = serde_json::from_str(&rt.load_expansion(&newer)).unwrap();
        assert_eq!(loaded["ok"], false);
        assert!(
            loaded["errors"][0]
                .as_str()
                .unwrap()
                .contains("update the engine"),
            "{loaded}"
        );
    }

    /// The library check reads each pack's requirements against the rest of
    /// the library: a missing pack, a version out of range, a conflict with
    /// the active pack.
    #[test]
    fn library_check_names_unmet_requires_and_active_conflicts() {
        let lib = json!({
            "active": "base",
            "packs": [
                { "id": "base", "version": "1.4.0", "requires": [], "conflicts": [] },
                { "id": "addon", "version": "0.1.0", "requires": ["base@^1.2", "extra"], "conflicts": [] },
                { "id": "rival", "version": "2.0.0", "requires": [], "conflicts": ["base@>=1"] },
                { "id": "old", "version": "0.1.0", "requires": ["base@^2"], "conflicts": [] },
            ],
        });
        let r = library_dependency_report(&lib);
        let addon = &r["addon"];
        assert_eq!(addon["problems"].as_array().unwrap().len(), 1);
        assert_eq!(addon["problems"][0]["kind"], "missing");
        assert_eq!(addon["problems"][0]["dep"], "extra");
        assert_eq!(addon["uses"][0]["installed"], "1.4.0");
        assert_eq!(r["rival"]["problems"][0]["kind"], "conflict");
        assert_eq!(r["rival"]["problems"][0]["active"], true);
        assert_eq!(r["old"]["problems"][0]["kind"], "wrong_version");
        assert_eq!(r["base"]["problems"].as_array().unwrap().len(), 0);
        let rt = LegaiaRuntime::new();
        assert_eq!(rt.expansion_library_check("not json"), "{}");
    }

    /// The Rebirth bundle reads and validates; with no disc loaded it is
    /// refused for activation (the pack info still comes back), and junk
    /// bytes are refused as not a bundle.
    #[test]
    fn load_expansion_reads_the_rebirth_bundle() {
        let mut rt = LegaiaRuntime::new();
        let v: serde_json::Value =
            serde_json::from_str(&rt.load_expansion_core(&rebirth_bundle())).unwrap();
        assert_eq!(v["pack"]["id"], "rebirth");
        assert_eq!(v["pack"]["name"], "Legend of Legaia: Rebirth");
        assert!(
            v["pack"]["maps"]
                .as_array()
                .unwrap()
                .iter()
                .any(|m| m["id"] == "rebirth_tower_1f")
        );
        assert_eq!(v["ok"], false, "no disc: not activated");
        assert!(v["errors"][0].as_str().unwrap().contains("load the disc"));

        let junk: serde_json::Value =
            serde_json::from_str(&rt.load_expansion_core(b"not a pack")).unwrap();
        assert_eq!(junk["ok"], false);
        assert!(junk["pack"].is_null());
        let none: serde_json::Value = serde_json::from_str(&rt.expansion_info_json()).unwrap();
        assert!(none["pack"].is_null());
    }

    /// Disc-gated: on a loaded disc the Rebirth bundle activates, its map
    /// enters as a live field scene with a scene build the page draws, and
    /// unloading drops it.
    #[test]
    fn load_expansion_activates_and_enters_the_map_on_a_disc() {
        let Some(disc) = std::env::var_os("LEGAIA_DISC_BIN") else {
            eprintln!("[skip] LEGAIA_DISC_BIN unset");
            return;
        };
        let bytes = std::fs::read(disc).expect("read the disc");
        let mut rt = LegaiaRuntime::new();
        assert!(rt.load_disc(bytes, String::new()).is_ok(), "load the disc");
        let v: serde_json::Value =
            serde_json::from_str(&rt.load_expansion_core(&rebirth_bundle())).unwrap();
        assert_eq!(v["ok"], true, "{v}");
        rt.enter_expansion_map_core("rebirth_tower_1f", "")
            .expect("enter the pack map");
        let host = rt.scene_host.host().unwrap();
        assert_eq!(host.active_expansion_map(), Some("rebirth_tower_1f"));
        // The page arms its live battles after the map's monster merge; the
        // arm keeps the pack's monsters and rolls no retail encounters.
        for id in host.world.expansion_base_monster_ids() {
            assert!(
                host.world.tables.monster_catalog.get(id).is_some(),
                "monster {id} has no catalog entry"
            );
        }
        assert!(host.world.encounters.session.is_none());
        let field = rt.field.as_ref().expect("the page built the scene");
        assert!(!field.placements.is_empty(), "the map's props draw");
        let info: serde_json::Value = serde_json::from_str(&rt.expansion_info_json()).unwrap();
        assert_eq!(info["map"], "rebirth_tower_1f");
        rt.unload_expansion();
        assert!(!rt.scene_host.host().unwrap().world.expansion.is_active());
    }
}
