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
//! modes.toml         game modes                  -> modes
//! maps/<id>.toml     custom maps, one per file   -> map
//! assets/...         the modder's own files
//! ```
//!
//! Every file but `pack.toml` is optional. The shareable single-file form
//! of the same tree is the `.lxpack` bundle ([`bundle`]).

#![forbid(unsafe_code)]

pub mod battle;
pub mod bundle;
pub mod cards;
pub mod dialog;
pub mod manifest;
pub mod map;
pub mod modes;
pub mod progression;
pub mod quest;
pub mod refs;
pub mod validate;

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
    pub modes: modes::ModeSection,
    /// Custom maps keyed by map id (the `maps/<id>.toml` file stem).
    pub maps: BTreeMap<String, map::MapDef>,
    /// The modder's own files under `assets/`, keyed by their path relative
    /// to `assets/` with `/` separators.
    #[serde(skip)]
    pub assets: BTreeMap<String, Vec<u8>>,
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
    pub fn from_files(files: &PackFiles) -> Result<Self> {
        let manifest = files.get("pack.toml").context("pack.toml is missing")?;
        let manifest =
            toml::from_str(std::str::from_utf8(manifest).context("pack.toml is not UTF-8")?)
                .context("parsing pack.toml")?;
        let mut maps = BTreeMap::new();
        let mut assets = BTreeMap::new();
        for (path, bytes) in files {
            if let Some(stem) = path
                .strip_prefix("maps/")
                .and_then(|p| p.strip_suffix(".toml"))
            {
                let text =
                    std::str::from_utf8(bytes).with_context(|| format!("{path} is not UTF-8"))?;
                let map: map::MapDef =
                    toml::from_str(text).with_context(|| format!("parsing {path}"))?;
                maps.insert(stem.to_string(), map);
            } else if let Some(rel) = path.strip_prefix("assets/") {
                assets.insert(rel.to_string(), bytes.clone());
            }
        }
        Ok(Self {
            manifest,
            battle: section(files, "battle.toml")?,
            progression: section(files, "progression.toml")?,
            quests: section(files, "quests.toml")?,
            dialog: section(files, "dialog.toml")?,
            cards: section(files, "cards.toml")?,
            modes: section(files, "modes.toml")?,
            maps,
            assets,
        })
    }

    /// Load a pack from a directory on disk.
    pub fn from_dir(root: &Path) -> Result<Self> {
        Self::from_files(&read_dir_files(root)?)
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
