//! Expansion packs for the port: a modder-authored bundle of new content
//! (maps, battles, bosses, quests, dialogue, trading cards, game modes) that
//! the engine loads beside the user's disc.
//!
//! A pack carries **no Sony bytes**. Every disc asset it uses - a monster, a
//! mesh, a texture, a music cue, a scene - is named by a coordinate
//! ([`refs::DiscRef`]) that the engine resolves against the disc the player
//! supplies, the same rule a shareable language pack follows. What a pack
//! may embed is the modder's own work: text, numbers, layouts and their own
//! art under `assets/`.
//!
//! On disk a pack is a directory:
//!
//! ```text
//! pack.toml          manifest (required)        -> manifest::PackManifest
//! battle.toml        monsters, formations, encounter overrides -> battle
//! progression.toml   level curve, rewards        -> progression
//! quests.toml        quests and challenges       -> quest
//! dialog.toml        dialogue lines              -> dialog
//! cards.toml         trading cards               -> cards
//! cutscenes.toml     scripted scenes             -> cutscene
//! items.toml         items, equipment, shops     -> items
//! arts.toml          Tactical Arts, chains       -> arts
//! spells.toml        Seru magic                  -> spells
//! modes.toml         game modes                  -> modes
//! audio.toml         music tracks, sounds, music picks -> audio
//! maps/<id>.toml     custom maps, one per file   -> map
//! assets/...         the modder's own files
//! README.md, LICENSE, CHANGELOG.md   the pack's own documentation
//! ```
//!
//! Every file but `pack.toml` is optional. The shareable single-file form
//! of the same tree is the `.lxpack` bundle ([`bundle`]).

#![forbid(unsafe_code)]

pub mod arts;
pub mod audio;
pub mod audio_clip;
pub mod battle;
pub mod boss_script;
pub mod bundle;
pub mod cards;
pub mod cutscene;
pub mod dialog;
pub mod diff;
pub mod items;
pub mod manifest;
pub mod map;
pub mod map_detail;
pub mod map_detail_rules;
pub mod map_rules;
pub mod modes;
pub mod progression;
pub mod quest;
pub mod refs;
pub mod share;
pub mod spells;
pub mod terrain;
pub mod terrain_rules;
pub mod validate;
pub mod version;

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// One loaded expansion pack: the manifest and every content section.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ExpansionPack {
    pub manifest: manifest::PackManifest,
    pub battle: battle::BattleSection,
    pub progression: progression::ProgressionSection,
    pub quests: quest::QuestSection,
    pub dialog: dialog::DialogSection,
    pub cards: cards::CardSection,
    pub cutscenes: cutscene::CutsceneSection,
    pub modes: modes::ModeSection,
    /// New items, equipment, sets and shops.
    pub items: items::ItemSection,
    /// The pack's own music and sounds, and its music picks.
    pub audio: audio::AudioSection,
    /// New Tactical Arts and chains.
    pub arts: arts::ArtSection,
    /// New Seru magic.
    pub spells: spells::SpellSection,
    /// Custom maps keyed by map id (the `maps/<id>.toml` file stem).
    pub maps: BTreeMap<String, map::MapDef>,
    /// The modder's own files under `assets/`, keyed by their path relative
    /// to `assets/` with `/` separators.
    #[serde(skip)]
    pub assets: BTreeMap<String, Vec<u8>>,
    /// The pack's documentation files at its root ([`DOC_FILES`]), kept so
    /// an exported pack carries them.
    #[serde(skip)]
    pub docs: BTreeMap<String, Vec<u8>>,
}

/// The content-section files at the pack root, in load order.
pub const SECTION_FILES: &[&str] = &[
    "battle.toml",
    "progression.toml",
    "quests.toml",
    "dialog.toml",
    "cards.toml",
    "cutscenes.toml",
    "modes.toml",
    "items.toml",
    "audio.toml",
    "arts.toml",
    "spells.toml",
];

/// Documentation files a pack may carry at its root.
pub const DOC_FILES: &[&str] = &[
    "README.md",
    "LICENSE",
    "LICENSE.md",
    "LICENSE.txt",
    "CHANGELOG.md",
];

/// Is `path` part of the pack layout: the manifest, a section file, a map,
/// an asset or a root documentation file. The loader ignores anything else.
pub fn is_known_path(path: &str) -> bool {
    path == "pack.toml"
        || SECTION_FILES.contains(&path)
        || DOC_FILES.contains(&path)
        || path.starts_with("assets/")
        || path
            .strip_prefix("maps/")
            .and_then(|p| p.strip_suffix(".toml"))
            .is_some_and(|stem| !stem.is_empty() && !stem.contains('/'))
}

/// The pack tree as named files (path relative to the pack root, `/`
/// separators -> bytes). The directory loader, the `.lxpack` bundle and the
/// browser workbench all produce this, so [`ExpansionPack::from_files`] is
/// the one parser.
pub type PackFiles = BTreeMap<String, Vec<u8>>;

fn section<T: for<'de> Deserialize<'de> + Default>(files: &PackFiles, name: &str) -> Result<T> {
    match files.get(name) {
        None => Ok(T::default()),
        Some(bytes) => {
            let text =
                std::str::from_utf8(bytes).with_context(|| format!("{name} is not UTF-8"))?;
            toml::from_str(text).with_context(|| format!("parsing {name}"))
        }
    }
}

impl ExpansionPack {
    /// Parse a pack from its file tree.
    ///
    /// The pack-format and engine gates ([`manifest::check_compatibility`])
    /// run first, on a lenient read of `pack.toml`, so a pack from a newer
    /// format is refused with "update the engine" rather than with whatever
    /// key of the new format the strict parse trips on.
    pub fn from_files(files: &PackFiles) -> Result<Self> {
        let manifest = files.get("pack.toml").context("pack.toml is missing")?;
        let text = std::str::from_utf8(manifest).context("pack.toml is not UTF-8")?;
        if let Some(peek) = manifest::ManifestPeek::read(text)
            && let Err(why) = peek.compatibility()
        {
            let who = match (peek.id.is_empty(), peek.version.is_empty()) {
                (false, false) => format!("pack {:?} {}", peek.id, peek.version),
                (false, true) => format!("pack {:?}", peek.id),
                _ => "the pack".to_string(),
            };
            anyhow::bail!("{who} cannot be loaded: {why}");
        }
        let manifest = toml::from_str(text).context("parsing pack.toml")?;
        let mut maps = BTreeMap::new();
        let mut assets = BTreeMap::new();
        let mut docs = BTreeMap::new();
        for (path, bytes) in files {
            if let Some(stem) = path
                .strip_prefix("maps/")
                .and_then(|p| p.strip_suffix(".toml"))
                .filter(|stem| !stem.contains('/'))
            {
                let text =
                    std::str::from_utf8(bytes).with_context(|| format!("{path} is not UTF-8"))?;
                let map: map::MapDef =
                    toml::from_str(text).with_context(|| format!("parsing {path}"))?;
                maps.insert(stem.to_string(), map);
            } else if let Some(rel) = path.strip_prefix("assets/") {
                assets.insert(rel.to_string(), bytes.clone());
            } else if DOC_FILES.contains(&path.as_str()) {
                docs.insert(path.clone(), bytes.clone());
            }
        }
        Ok(Self {
            manifest,
            battle: section(files, "battle.toml")?,
            progression: section(files, "progression.toml")?,
            quests: section(files, "quests.toml")?,
            dialog: section(files, "dialog.toml")?,
            cards: section(files, "cards.toml")?,
            cutscenes: section(files, "cutscenes.toml")?,
            modes: section(files, "modes.toml")?,
            items: section(files, "items.toml")?,
            audio: section(files, "audio.toml")?,
            arts: section(files, "arts.toml")?,
            spells: section(files, "spells.toml")?,
            maps,
            assets,
            docs,
        })
    }

    /// Serialise the pack back into its file tree: `pack.toml`, each
    /// section that differs from its default, one file per map, the assets
    /// and the documentation files. `from_files(&to_files()?)` gives back an
    /// equal pack, and the output is deterministic, so the workbench exports
    /// through it.
    pub fn to_files(&self) -> Result<PackFiles> {
        fn put<T: Serialize + Default + PartialEq>(
            out: &mut PackFiles,
            name: &str,
            value: &T,
        ) -> Result<()> {
            if *value != T::default() {
                let text = toml::to_string(value).with_context(|| format!("writing {name}"))?;
                out.insert(name.to_string(), text.into_bytes());
            }
            Ok(())
        }
        let mut out = PackFiles::new();
        let manifest = toml::to_string(&self.manifest).context("writing pack.toml")?;
        out.insert("pack.toml".into(), manifest.into_bytes());
        put(&mut out, "battle.toml", &self.battle)?;
        put(&mut out, "progression.toml", &self.progression)?;
        put(&mut out, "quests.toml", &self.quests)?;
        put(&mut out, "dialog.toml", &self.dialog)?;
        put(&mut out, "cards.toml", &self.cards)?;
        put(&mut out, "cutscenes.toml", &self.cutscenes)?;
        put(&mut out, "modes.toml", &self.modes)?;
        put(&mut out, "items.toml", &self.items)?;
        put(&mut out, "audio.toml", &self.audio)?;
        put(&mut out, "arts.toml", &self.arts)?;
        put(&mut out, "spells.toml", &self.spells)?;
        for (id, map) in &self.maps {
            let name = format!("maps/{id}.toml");
            let text = toml::to_string(map).with_context(|| format!("writing {name}"))?;
            out.insert(name, text.into_bytes());
        }
        for (rel, bytes) in &self.assets {
            out.insert(format!("assets/{rel}"), bytes.clone());
        }
        for (name, bytes) in &self.docs {
            out.insert(name.clone(), bytes.clone());
        }
        Ok(out)
    }

    /// Parse a pack from `.lxpack` bundle bytes.
    pub fn from_bundle(bytes: &[u8]) -> Result<Self> {
        Self::from_files(&bundle::read_bundle(bytes)?)
    }

    /// Load a pack from a directory on disk.
    pub fn from_dir(root: &Path) -> Result<Self> {
        Self::from_files(&read_dir_files(root)?)
    }
}

/// Read a pack tree from `path`: a pack directory, or an `.lxpack` bundle.
pub fn read_files(path: &Path) -> Result<PackFiles> {
    if path.is_dir() {
        read_dir_files(path)
    } else {
        let bytes = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
        bundle::read_bundle(&bytes).with_context(|| format!("reading {}", path.display()))
    }
}

/// Read every file under `root` into a [`PackFiles`] tree.
pub fn read_dir_files(root: &Path) -> Result<PackFiles> {
    fn walk(root: &Path, dir: &Path, out: &mut PackFiles) -> Result<()> {
        for entry in std::fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))? {
            let path = entry?.path();
            if path.is_dir() {
                walk(root, &path, out)?;
            } else {
                let rel = path
                    .strip_prefix(root)?
                    .components()
                    .map(|c| c.as_os_str().to_string_lossy())
                    .collect::<Vec<_>>()
                    .join("/");
                out.insert(rel, std::fs::read(&path)?);
            }
        }
        Ok(())
    }
    let mut out = PackFiles::new();
    walk(root, root, &mut out)?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn to_files_round_trips_every_part() {
        let mut pack = ExpansionPack {
            manifest: manifest::PackManifest::new("t", "T", "1.0.0"),
            ..Default::default()
        };
        pack.manifest.requires = vec!["base".into()];
        pack.manifest.homepage = Some("https://example.org".into());
        pack.maps.insert("tower_1".into(), map::MapDef::default());
        pack.assets
            .insert("art/a.png".into(), b"\x89PNG\r\n\x1a\n".to_vec());
        pack.docs.insert("README.md".into(), b"# T".to_vec());
        let files = pack.to_files().unwrap();
        let names: Vec<&str> = files.keys().map(String::as_str).collect();
        assert_eq!(
            names,
            [
                "README.md",
                "assets/art/a.png",
                "maps/tower_1.toml",
                "pack.toml"
            ],
            "default sections are not written"
        );
        assert_eq!(ExpansionPack::from_files(&files).unwrap(), pack);
    }

    #[test]
    fn layout_paths() {
        for p in [
            "pack.toml",
            "battle.toml",
            "maps/a.toml",
            "assets/x/y.png",
            "LICENSE",
        ] {
            assert!(is_known_path(p), "{p}");
        }
        for p in ["maps/a/b.toml", "maps/.toml", "notes.txt", "Battle.toml"] {
            assert!(!is_known_path(p), "{p}");
        }
    }
}
