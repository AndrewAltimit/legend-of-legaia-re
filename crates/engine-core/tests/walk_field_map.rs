//! Disc-gated regression: the free-roam **walk** `.MAP` resolver
//! ([`Scene::walk_field_map_index`]) reads the real walk `.MAP` for the kingdom
//! overworld scenes, and the continent ground builds as a **heightfield**
//! surface ([`Scene::walk_heightfield`]).
//!
//! Background: the runtime loads a scene's field `.MAP` from its CDNAME index
//! through `toc[idx+2]`, which for the overlapping kingdom PROT clusters
//! resolves the entry two below the per-entry extractor's block start. The
//! within-block [`FIELD_MAP_LEN`] entry that [`Scene::field_map_index`] picks
//! is a decoy with the wrong continent (the old sparse-scatter bug). The walk
//! resolver takes `block_start - 2`.
//!
//! The continent ground is a procedural heightfield (corner elevations from the
//! `+0x4000` floor-nibble grid, gated on the object-grid `0x1000` bit — the
//! model `FUN_80019278` pins), NOT a per-cell pack-mesh sweep. The superseded
//! `walk_terrain_tiles` per-cell sweep flooded ~97% of cells with pool-5
//! because the bulk-terrain records carry `+0x10 == 0`.
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
    // entries below the within-block decoy, and the floor grid builds a dense
    // heightfield continent (>10k quads) with real elevation variation.
    for name in ["map01", "map02", "map03"] {
        let scene = Scene::load(&index, name).expect("load scene");
        let walk = scene.walk_field_map_index(&index).expect("walk .MAP index");
        let decoy = scene.field_map_index(&index).expect("within-block index");
        assert_ne!(
            walk, decoy,
            "{name}: walk resolver should differ from the within-block decoy"
        );
        assert_eq!(
            walk,
            scene.start - 2,
            "{name}: walk .MAP is block_start - 2"
        );

        let hf = scene
            .walk_heightfield(&index)
            .expect("walk heightfield")
            .expect("has field map + floor LUT");
        assert!(
            hf.quad_count() > 10_000,
            "{name}: expected a dense heightfield continent, got {} quads",
            hf.quad_count()
        );
        assert_eq!(
            hf.positions.len(),
            hf.tile_ids.len(),
            "{name}: one tile id per vertex"
        );
        assert_eq!(
            hf.indices.len(),
            hf.quad_count() * 6,
            "{name}: two triangles per quad"
        );
        // The terrain is a real heightfield, not a flat plane: at least two
        // distinct corner elevations across the continent.
        let mut ys: Vec<i32> = hf.positions.iter().map(|p| p[1] as i32).collect();
        ys.sort_unstable();
        ys.dedup();
        assert!(
            ys.len() > 1,
            "{name}: heightfield should have elevation variation, got {} distinct Y",
            ys.len()
        );
    }
}
