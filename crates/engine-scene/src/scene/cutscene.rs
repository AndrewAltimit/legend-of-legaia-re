//! Cutscene-scene helpers: CDNAME label detection and FMV (`MVn.STR`) mapping.

use super::*;

/// Return `true` if a CDNAME scene label is an in-engine cutscene scene
/// (prefixed with `op` or `ed`, which use the dialogue actor overlay).
///
/// In-engine cutscenes are distinct from FMV (`MOV/MV*.STR`): they run
/// the dialogue-actor overlay and are not backed by STR video files.
/// Use `play-str` for FMV; use `play --scene` for these scenes.
pub fn is_cutscene_label(label: &str) -> bool {
    label.starts_with("op") || label.starts_with("ed")
}

/// Return the FMV [`MOV/MVn.STR`] filename associated with a CDNAME
/// cutscene scene, if any. The retail engine reads the mapping from
/// the cutscene overlay; until that table is captured the mapping is
/// derived from CDNAME ordering: the five `op*` opening scenes map
/// 1:1 to `MV1.STR..MV5.STR`, and the first `ed*` ending scene maps to
/// `MV6.STR`. Other ending scenes are dialogue-actor-overlay driven
/// (no FMV).
///
/// Returns `Some("MOV/MVn.STR")` for resolvable scenes, `None`
/// otherwise. Engines join the relative path against their extracted
/// root (or read the named ISO9660 entry from a `.bin` disc image).
///
/// Heuristic mapping pinned to the disc's MV1..MV6 file count + the
/// CDNAME ordering of the `op*` / `ed*` scene labels. The exact retail
/// table lives in the cutscene overlay (not yet captured); when it
/// lands, this function should be updated to read the captured map.
pub fn cutscene_str_for(scene_label: &str) -> Option<&'static str> {
    match scene_label {
        // op* opening cutscenes - five in CDNAME order.
        "opdeene" => Some("MOV/MV1.STR"),
        "opstati" => Some("MOV/MV2.STR"),
        "opkorout" => Some("MOV/MV3.STR"),
        "opurud" => Some("MOV/MV4.STR"),
        "opmap01" => Some("MOV/MV5.STR"),
        // ed* - only the first ending scene is FMV-backed; the rest are
        // dialogue-actor-overlay driven and have no associated MV file.
        "edteien" => Some("MOV/MV6.STR"),
        _ => None,
    }
}

/// Inverse of [`cutscene_str_for`]: resolve a `MOV/MVn.STR` filename to
/// its CDNAME scene label, if known. Useful for diagnostic dumps that
/// want to surface "this STR plays during scene X".
pub fn cutscene_label_for_str(str_filename: &str) -> Option<&'static str> {
    let trimmed = str_filename
        .rsplit_once('/')
        .map(|(_, name)| name)
        .unwrap_or(str_filename);
    match trimmed.to_ascii_uppercase().as_str() {
        "MV1.STR" => Some("opdeene"),
        "MV2.STR" => Some("opstati"),
        "MV3.STR" => Some("opkorout"),
        "MV4.STR" => Some("opurud"),
        "MV5.STR" => Some("opmap01"),
        "MV6.STR" => Some("edteien"),
        _ => None,
    }
}

/// All known FMV-backed cutscene scenes in CDNAME order.
pub const FMV_CUTSCENE_SCENES: [(&str, &str); 6] = [
    ("opdeene", "MOV/MV1.STR"),
    ("opstati", "MOV/MV2.STR"),
    ("opkorout", "MOV/MV3.STR"),
    ("opurud", "MOV/MV4.STR"),
    ("opmap01", "MOV/MV5.STR"),
    ("edteien", "MOV/MV6.STR"),
];

/// Field scenes the FMV cutscene overlay knows about by name (it carries
/// their CDNAME labels in its data section) - distinct from the `op*`
/// opening / `ed*` ending scenes covered by [`FMV_CUTSCENE_SCENES`].
///
/// NOT the trigger-op scene list: the disc-walked per-scene trigger table
/// (`man_field_scripts::scene_fmv_triggers`, pinned by the
/// `scene_fmv_triggers_disc` test) finds the `0x4C 0xE2` ops in `town01`,
/// `garmel`, `deroa`, `chitei2`, `dohaty`, `town0d`, `uru` and `jouine` -
/// only `chitei2` overlaps this list. These labels are the scenes the
/// overlay itself references (e.g. for the post-playback scene
/// restore), recorded as found in its data section.
pub const FMV_TRIGGER_FIELD_SCENES: [&str; 7] = [
    "town0b", "map01", "chitei2", "map02", "jou", "uru2", "town0e",
];

/// Return `true` if a CDNAME label is a field scene the FMV overlay
/// carries in its data section (see [`FMV_TRIGGER_FIELD_SCENES`] - not
/// the trigger-op scene set). Distinct from [`is_cutscene_label`] (which
/// covers the `op*` / `ed*` engine cutscene scenes).
pub fn is_fmv_trigger_field_scene(label: &str) -> bool {
    FMV_TRIGGER_FIELD_SCENES.contains(&label)
}

/// Return `true` if a CDNAME label is an overworld (world-map) scene.
///
/// Legaia's three kingdom overworlds are `map01` / `map02` / `map03`; they are
/// the only CDNAME `mapNN` labels. The overworld shares game mode `0x03` with
/// towns and fields (see `docs/subsystems/field-locomotion.md` "Town / field
/// parity") - retail does not give it a distinct mode. It distinguishes the
/// overworld by the scene's own code overlay: the kingdom bundles carry a
/// world-map-overlay slot (type byte `0x05`, see
/// `docs/formats/world-map-overlay.md`) that towns/dungeons lack. The engine
/// classifies by the stable CDNAME label, which is `map` followed by two
/// digits for exactly these three scenes. A scene that classifies as a world
/// map is entered through [`SceneHost::enter_world_map_scene`] (which seeds the
/// region-keyed encounter table + world-map camera) instead of the plain
/// field path.
pub fn is_world_map_scene(label: &str) -> bool {
    match label.strip_prefix("map") {
        Some(rest) => rest.len() == 2 && rest.bytes().all(|b| b.is_ascii_digit()),
        None => false,
    }
}

/// Engine-override CDNAME→MV cutscene map.
///
/// The retail table lives in the FMV-cutscene overlay (game modes 26/27,
/// see [`docs/subsystems/cutscene.md`](../../../../docs/subsystems/cutscene.md)).
/// The retail overlay isn't in the captured corpus yet - `dump_str_fmv_overlay.py`
/// is staged but not run - so the heuristic in [`cutscene_str_for`] ships
/// as the default. Once the overlay capture lands, engines can populate a
/// [`CutsceneMap`] from the captured table and override the heuristic via
/// [`CutsceneMap::resolve`].
///
/// The map is *additive*: entries the user inserts win, missing entries
/// fall back to the heuristic. Engines feed it from a captured-data file
/// or from the boot config.
#[derive(Debug, Clone, Default)]
pub struct CutsceneMap {
    forward: std::collections::HashMap<String, String>,
}

impl CutsceneMap {
    pub fn new() -> Self {
        Self::default()
    }

    /// Build a map seeded with the heuristic mapping ([`FMV_CUTSCENE_SCENES`]).
    /// Engines can then override or extend specific entries.
    pub fn from_heuristic() -> Self {
        let mut m = Self::default();
        for (label, str_path) in FMV_CUTSCENE_SCENES.iter() {
            m.forward
                .insert((*label).to_string(), (*str_path).to_string());
        }
        m
    }

    /// Insert / replace a CDNAME-label → STR-path mapping.
    pub fn insert(&mut self, scene_label: impl Into<String>, str_path: impl Into<String>) {
        self.forward.insert(scene_label.into(), str_path.into());
    }

    /// Resolve a CDNAME label to its STR path. Falls through to the
    /// hard-coded heuristic if the map doesn't carry the label.
    pub fn resolve(&self, scene_label: &str) -> Option<String> {
        if let Some(path) = self.forward.get(scene_label) {
            return Some(path.clone());
        }
        cutscene_str_for(scene_label).map(|s| s.to_string())
    }

    /// Number of explicit entries (excludes heuristic fallbacks).
    pub fn len(&self) -> usize {
        self.forward.len()
    }

    pub fn is_empty(&self) -> bool {
        self.forward.is_empty()
    }

    /// Iterate over the explicit `(scene_label, str_path)` entries the
    /// caller has installed (excludes heuristic fallbacks). Order is
    /// unspecified.
    pub fn entries(&self) -> impl Iterator<Item = (&str, &str)> {
        self.forward.iter().map(|(k, v)| (k.as_str(), v.as_str()))
    }

    /// Parse a [`CutsceneMap`] from a TOML document. The document holds a
    /// top-level `[scenes]` table that maps CDNAME labels to STR-file
    /// paths:
    ///
    /// ```toml
    /// [scenes]
    /// opdeene = "MOV/MV1.STR"
    /// opstati = "MOV/MV2.STR"
    /// edteien = "MOV/MV6.STR"
    /// ```
    ///
    /// Unknown top-level keys are ignored so engines can layer engine-
    /// specific config alongside.
    pub fn from_toml_str(toml_str: &str) -> Result<Self> {
        #[derive(serde::Deserialize)]
        struct Doc {
            scenes: Option<HashMap<String, String>>,
        }
        let doc: Doc =
            toml::from_str(toml_str).with_context(|| "parse cutscene-map TOML document")?;
        let mut map = Self::default();
        if let Some(table) = doc.scenes {
            for (k, v) in table {
                map.forward.insert(k, v);
            }
        }
        Ok(map)
    }

    /// Read a TOML config from disk. Wraps [`Self::from_toml_str`].
    pub fn from_toml_path(path: &Path) -> Result<Self> {
        let s = std::fs::read_to_string(path)
            .with_context(|| format!("read cutscene-map at {}", path.display()))?;
        Self::from_toml_str(&s).with_context(|| format!("parse {}", path.display()))
    }

    /// Serialise the map back to TOML form. Round-trips with
    /// [`Self::from_toml_str`].
    pub fn to_toml_string(&self) -> String {
        let mut out = String::from("[scenes]\n");
        let mut keys: Vec<&String> = self.forward.keys().collect();
        keys.sort();
        for k in keys {
            let v = &self.forward[k];
            // TOML basic-string escaping: paths use `\\` separators which
            // need explicit escaping. Use single-quoted literal strings to
            // sidestep that entirely.
            let safe_v = if v.contains('\'') {
                format!("\"{}\"", v.replace('\\', "\\\\").replace('"', "\\\""))
            } else {
                format!("'{}'", v)
            };
            out.push_str(&format!("{} = {}\n", k, safe_v));
        }
        out
    }
}
