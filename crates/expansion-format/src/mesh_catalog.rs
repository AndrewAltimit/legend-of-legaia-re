//! The borrowed-mesh catalog: what each base-scene mesh a pack map can
//! borrow is, as a modder reads it.
//!
//! A map names a disc mesh by coordinate, `{ scene = "town01", object = 47 }`,
//! and nothing in the coordinate says that object 47 is a bed and object 48
//! the stern of a boat. The catalog says so, for the scenes the flagship
//! pack borrows from: per `(scene, object)` a human label, a kind, the
//! mesh's measured size and whether it stands in the player's way (the
//! `solid` a placement of it wants). It is labels and numbers only, no disc
//! bytes: the sizes are measured off the user's disc by
//! `legaia-engine mesh-catalog` (which also renders the contact sheets the
//! labels were read from), and the labels are hand-written.
//!
//! The data is `data/mesh_catalog.toml`, compiled in ([`catalog`]); the
//! workbench and the validator look meshes up with [`MeshCatalog::get`].
//! See `docs/formats/expansion-pack-maps.md#the-mesh-catalog`.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

/// What sort of thing a mesh is, for grouping and sanity checks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MeshKind {
    /// A house, tower, mill: a walled structure.
    Building,
    /// A tree, bush, flower, fern, mushroom.
    Plant,
    /// A rock, boulder, cliff piece, stump.
    Rock,
    /// Furniture and household goods: bed, table, shelf, barrel, crate.
    Furniture,
    /// A fence, wall run, gate, bridge, stair, signpost.
    Structure,
    /// A lamp, torch, lantern.
    Light,
    /// A boat, cart, wagon.
    Vehicle,
    /// The ground, a terrain shell or a sky backdrop: not a prop.
    Terrain,
    /// Anything else: an effect quad, a tiny marker, an unread mesh.
    Other,
}

/// One catalogued mesh.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MeshEntry {
    /// The field scene whose geometry pack holds it (CDNAME name).
    pub scene: String,
    /// The mesh's index in that pack (a map's `object`).
    pub object: u16,
    /// What it is, in a few words.
    pub label: String,
    pub kind: MeshKind,
    /// Measured at scale 1, unrotated: the base footprint's width (`X`) and
    /// depth (`Z`), and the height, world units (a tile is 128).
    pub size: [u16; 3],
    /// Stands in the player's way at scale 1 (wider than a tile and a
    /// half, taller than a step, reaching body height): a placement wants
    /// `solid = true`.
    pub solid: bool,
    /// Triangles the mesh draws (a quad counts two).
    #[serde(default)]
    pub tris: u32,
    /// A modder's note: how the mesh reads, what it suits, a pitfall.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub note: String,
}

/// The file form.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct MeshCatalogFile {
    #[serde(default)]
    pub mesh: Vec<MeshEntry>,
}

/// The catalog, keyed by `(scene, object)`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MeshCatalog {
    entries: BTreeMap<(String, u16), MeshEntry>,
}

impl MeshCatalog {
    /// Parse a catalog file.
    pub fn parse(text: &str) -> anyhow::Result<MeshCatalog> {
        let file: MeshCatalogFile = toml::from_str(text)?;
        let mut entries = BTreeMap::new();
        for e in file.mesh {
            let key = (e.scene.clone(), e.object);
            anyhow::ensure!(
                !entries.contains_key(&key),
                "{}:{} catalogued twice",
                key.0,
                key.1
            );
            entries.insert(key, e);
        }
        Ok(MeshCatalog { entries })
    }

    /// The entry for `scene`'s mesh `object`.
    pub fn get(&self, scene: &str, object: u16) -> Option<&MeshEntry> {
        self.entries.get(&(scene.to_string(), object))
    }

    /// Every entry, by scene then object.
    pub fn iter(&self) -> impl Iterator<Item = &MeshEntry> {
        self.entries.values()
    }

    /// The scenes catalogued.
    pub fn scenes(&self) -> Vec<&str> {
        let mut s: Vec<&str> = self.entries.keys().map(|(s, _)| s.as_str()).collect();
        s.dedup();
        s
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The file form, by scene then object.
    pub fn to_toml(&self) -> String {
        let file = MeshCatalogFile {
            mesh: self.entries.values().cloned().collect(),
        };
        format!(
            "{HEADER}\n{}",
            toml::to_string(&file).expect("the catalog serialises")
        )
    }

    /// Insert or replace an entry.
    pub fn insert(&mut self, e: MeshEntry) {
        self.entries.insert((e.scene.clone(), e.object), e);
    }
}

/// The comment block the file form opens with.
pub const HEADER: &str = "\
# Borrowed-mesh catalog: what each field-scene geometry-pack mesh is.
# Sizes, `solid` and `tris` are measured off the disc by
# `legaia-engine mesh-catalog --scene <s> --catalog <this file>`, which keeps
# the hand-written `label`, `kind` and `note` of every entry already here.
# Disc coordinates, labels and numbers only - no disc bytes.
# size = [width X, depth Z, height] in world units at scale 1 (a tile is 128).
";

/// The committed catalog (`data/mesh_catalog.toml`).
pub fn catalog() -> &'static MeshCatalog {
    static CAT: OnceLock<MeshCatalog> = OnceLock::new();
    CAT.get_or_init(|| {
        MeshCatalog::parse(include_str!("../data/mesh_catalog.toml"))
            .expect("the committed mesh catalog parses")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_committed_catalog_parses_and_labels_every_entry() {
        let c = catalog();
        assert!(!c.is_empty());
        for e in c.iter() {
            assert!(
                !e.label.trim().is_empty(),
                "{}:{} unlabelled",
                e.scene,
                e.object
            );
        }
    }

    #[test]
    fn a_catalog_round_trips_through_its_file_form() {
        let c = catalog();
        let again = MeshCatalog::parse(&c.to_toml()).unwrap();
        assert_eq!(&again, c);
    }

    #[test]
    fn a_duplicate_entry_is_refused() {
        let one = "[[mesh]]\nscene = \"a\"\nobject = 1\nlabel = \"x\"\nkind = \"rock\"\nsize = [1, 1, 1]\nsolid = false\n";
        assert!(MeshCatalog::parse(&format!("{one}{one}")).is_err());
    }
}
