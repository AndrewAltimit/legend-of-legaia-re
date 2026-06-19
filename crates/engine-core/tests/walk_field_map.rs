//! Disc-gated regression: the free-roam **walk** `.MAP` resolver
//! ([`Scene::walk_field_map_index`]) reads the real walk `.MAP` for the kingdom
//! overworld scenes, and the continent ground builds as a **heightfield**
//! surface ([`Scene::walk_heightfield`]).
//!
//! Background: the runtime loads a scene's field `.MAP` from its CDNAME index
//! through `toc[idx+2]`, which for the overlapping scene PROT clusters
//! resolves the entry two below the per-entry extractor's block start - the
//! universal field-map rule (census-pinned: live keikoku / koin3 field
//! buffers match their `define-2` entries with zero diffs). The within-block
//! [`FIELD_MAP_LEN`] entry is the *next* scene's map (for the kingdoms it
//! reads as the wrong continent - the old sparse-scatter bug).
//!
//! The continent ground is a procedural heightfield (corner elevations from the
//! `+0x4000` floor-nibble grid, gated on the object-grid `0x1000` bit - the
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

    // The kingdom overworld walk scenes: each resolves its `.MAP` two
    // entries below the extractor's block start (the universal field-map
    // rule; the first 0x12000 entry INSIDE the block is the next scene's
    // map), and the floor grid builds a dense heightfield continent
    // (>10k quads) with real elevation variation.
    for name in ["map01", "map02", "map03"] {
        let scene = Scene::load(&index, name).expect("load scene");
        let walk = scene.walk_field_map_index(&index).expect("walk .MAP index");
        assert_eq!(
            walk,
            scene.field_map_index(&index).expect("field .MAP index"),
            "{name}: walk + field paths share one resolver"
        );
        // Regression guard for the superseded in-block rule: the first
        // FIELD_MAP_LEN entry inside the block is NOT this scene's map.
        let in_block = (scene.start..scene.end).find(|&idx| {
            index
                .entries()
                .get(idx as usize)
                .is_some_and(|e| e.size_bytes as usize == legaia_engine_core::scene::FIELD_MAP_LEN)
        });
        assert_ne!(
            Some(walk),
            in_block,
            "{name}: the in-block 0x12000 entry belongs to the next block"
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
            hf.positions.len(),
            hf.cba_tsb.len(),
            "{name}: one [clut, tpage] per vertex"
        );
        assert_eq!(
            hf.indices.len(),
            hf.quad_count() * 6,
            "{name}: two triangles per quad"
        );
        // The continent ground is a terrain-type-keyed MULTI-page atlas (grass /
        // mountain / water / forest each on their own VRAM page), selected per
        // cell from the record's +0x15 byte - not a single shared page. A
        // sea-surrounded kingdom continent must touch several terrain pages,
        // including a water page (0x1B / 0x1C).
        let mut pages: Vec<u16> = hf.cba_tsb.iter().map(|ct| ct[1]).collect();
        pages.sort_unstable();
        pages.dedup();
        assert!(
            pages.len() >= 3,
            "{name}: expected several terrain pages, got {pages:?}"
        );
        assert!(
            pages.iter().any(|&p| p == 0x001B || p == 0x001C),
            "{name}: expected a water terrain page (0x1B/0x1C) in {pages:?}"
        );
        // +0x14 is the 8x8 atlas index: every baked UV origin lands on the
        // 32-texel grid within a 256x256 page.
        assert!(
            hf.uvs
                .iter()
                .all(|[u, v]| (u % 32 == 0 || u % 32 == 31) && (v % 32 == 0 || v % 32 == 31)),
            "{name}: ground UVs should tile the 32x32 atlas grid"
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

/// The `define - 2` field-map rule is UNIVERSAL (not kingdom-specific), and
/// the Rim Elm variants share one byte-identical map. Pins the save-library
/// census findings structurally on the disc:
///
/// - every field scene's `.MAP` resolves at `block_start - 2`;
/// - town01 / town0b / town0c resolve to byte-identical map content (which
///   is why a town0c session's live grid byte-matches "town01's" map);
/// - keikoku / koin3 resolve to the entries their live field buffers match
///   with zero diffs in the census (PROT 0109 / 0559) - NOT the in-block
///   entries the superseded rule picked (0118 / 0568).
#[test]
fn field_map_rule_is_universal_and_rim_elm_variants_share_one_map() {
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing");
        return;
    };
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    }
    let index = ProtIndex::open_extracted(&extracted).expect("open index");

    let map_bytes = |scene_name: &str| -> (u32, Vec<u8>) {
        let scene = Scene::load(&index, scene_name).expect("load scene");
        let idx = scene.field_map_index(&index).expect("field .MAP index");
        assert_eq!(idx, scene.start - 2, "{scene_name}: .MAP is define - 2");
        let bytes = index.entry_bytes_extended(idx).expect("read .MAP entry");
        (idx, bytes)
    };

    // Census anchors: the entries the live field buffers match exactly.
    let (keikoku_idx, _) = map_bytes("keikoku");
    assert_eq!(keikoku_idx, 109, "keikoku .MAP = PROT 0109 (census-pinned)");
    let (koin3_idx, _) = map_bytes("koin3");
    assert_eq!(koin3_idx, 559, "koin3 .MAP = PROT 0559 (census-pinned)");

    // The Rim Elm variant family shares one byte-identical map.
    let (t01_idx, t01) = map_bytes("town01");
    let (t0b_idx, t0b) = map_bytes("town0b");
    let (t0c_idx, t0c) = map_bytes("town0c");
    assert_eq!((t01_idx, t0b_idx, t0c_idx), (1, 10, 19));
    assert_eq!(t01, t0b, "town01 and town0b maps are byte-identical");
    assert_eq!(t01, t0c, "town01 and town0c maps are byte-identical");

    // And the non-variant neighbour is NOT a copy: PROT 0028 is izumi's map
    // (define 30 - 2), distinct from the Rim Elm family - the entry an
    // earlier reading misattributed as "town0c's own different .MAP".
    let (izumi_idx, izumi) = map_bytes("izumi");
    assert_eq!(izumi_idx, 28, "izumi .MAP = PROT 0028");
    assert_ne!(t0c, izumi, "town0c's map is not izumi's");
}
