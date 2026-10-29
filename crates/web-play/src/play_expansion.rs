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
            "maps": p.maps.iter().map(|(id, m)| json!({ "id": id, "name": m.name })).collect::<Vec<_>>(),
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

#[wasm_bindgen]
impl LegaiaRuntime {
    /// Load an expansion pack from its `.lxpack` bundle bytes and make it the
    /// active pack. Needs a loaded disc ([`Self::load_disc`]): activation
    /// reads the disc's progression tables. Returns JSON
    /// `{ ok, pack: { id, name, version, authors, description, maps, quests,
    /// challenges } | null, errors: [..], warnings: [..] }`; with any error
    /// the pack is not activated (and any pack already active stays).
    pub fn load_expansion(&mut self, bytes: &[u8]) -> String {
        self.load_expansion_core(bytes)
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
        let files = match legaia_expansion_format::bundle::read_bundle(bytes) {
            Ok(f) => f,
            Err(e) => return info_json(None, &[format!("not an .lxpack bundle: {e:#}")], &[]),
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
        host.enter_expansion_map(map_id, spawn)
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
