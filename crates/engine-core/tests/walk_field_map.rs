//! Disc-gated regression: the free-roam **walk** `.MAP` resolver
//! ([`Scene::walk_field_map_index`] / [`Scene::walk_terrain_tiles`]) reads the
//! real walk `.MAP` for the kingdom overworld scenes.
//!
//! Background: the runtime loads a scene's field `.MAP` from its CDNAME index
//! through `toc[idx+2]`, which for the overlapping kingdom PROT clusters
//! resolves the entry two below the per-entry extractor's block start. The
//! within-block [`FIELD_MAP_LEN`] entry that [`Scene::field_map_index`] picks
//! is a decoy with the wrong continent (the old sparse-scatter bug). The walk
//! resolver takes `block_start - 2` and gates the continent on bit `0x1000`.
//!
//! Skips silently when `extracted/` or `LEGAIA_DISC_BIN` is missing.

use std::path::PathBuf;

use legaia_engine_core::scene::{ProtIndex, Scene};

fn extracted_dir() -> Option<PathBuf> {
    for p in ["extracted", "../../extracted"] {
        let d = PathBuf::from(p);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

#[test]
fn walk_map_resolves_real_continent_for_kingdoms() {
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing");
        return;
    };
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    }
    let index = ProtIndex::open_extracted(&extracted).expect("open index");

    // The kingdom overworld walk scenes: each resolves its walk `.MAP` two
    // entries below the within-block decoy, and the `0x1000` sweep yields a
    // dense continent (>10k tiles) whose mesh indices all fit the 40-mesh
    // slot-1 landmark pool.
    for name in ["map01", "map02", "map03"] {
        let scene = Scene::load(&index, name).expect("load scene");
        let walk = scene.walk_field_map_index(&index).expect("walk .MAP index");
        let decoy = scene.field_map_index(&index).expect("within-block index");
        assert_ne!(
            walk, decoy,
            "{name}: walk resolver should differ from the within-block decoy"
        );
        assert_eq!(walk, scene.start - 2, "{name}: walk .MAP is block_start - 2");

        let tiles = scene
            .walk_terrain_tiles(&index)
            .expect("walk terrain")
            .expect("has field map");
        assert!(
            tiles.len() > 10_000,
            "{name}: expected a dense walk continent, got {} tiles",
            tiles.len()
        );
        // Every continent tile's mesh is `record[+0x10]`, which must index the
        // 40-mesh kingdom slot-1 pack (party prefix is absorbed engine-side).
        let max_pack = tiles.iter().filter_map(|p| p.pack_index).max().unwrap_or(0);
        assert!(
            max_pack < 40,
            "{name}: walk pack index {max_pack} exceeds the 40-mesh pool"
        );
    }
}
