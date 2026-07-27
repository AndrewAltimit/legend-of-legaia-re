//! Disc-gated: the terrain-tile world-Y math against real `balden` (Vidna)
//! disc bytes - the scene that exposed the stair-step defect.
//!
//! Retail's per-cell terrain emitters (`FUN_801F69D8` in PROT 0901 and the
//! field sibling `FUN_801F7088` in PROT 0900) resolve a visible cell's Y from
//! the **average of the four corner tiles' floor-LUT heights** (round toward
//! zero) plus the record's `y_off` - so a cell on a floor-tier edge lands
//! mid-slope, where its mesh's baked ramp expects it. Sampling only the
//! cell's own nibble (the placed-object formula, `FUN_8003A55C`) snaps every
//! edge cell a full tier up or down, shearing Vidna's terraced streets into
//! stair-stepped plateaus with dark gaps between them.
//!
//! Skips silently when `extracted/` or `LEGAIA_DISC_BIN` is missing.
use std::path::PathBuf;

use legaia_engine_core::field_env;
use legaia_engine_core::scene::{Scene, SceneHost};

fn extracted_dir() -> Option<PathBuf> {
    for d in ["extracted", "../../extracted"] {
        let p = PathBuf::from(d);
        if p.join("PROT.DAT").exists() && p.join("CDNAME.TXT").exists() {
            return Some(p);
        }
    }
    None
}

#[test]
fn balden_terrain_tiles_resolve_the_retail_corner_average_y() {
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing");
        return;
    };
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    }

    let host = SceneHost::open_extracted(&extracted).expect("open SceneHost");
    let index = host.index.clone();
    let scene = Scene::load(&index, "balden").expect("load balden");

    let lut = scene
        .field_floor_height_lut(&index)
        .expect("read floor LUT")
        .expect("balden MAN carries a floor LUT");
    // The balden LUT is the linear tier ramp (32 units per tier) - the
    // constant the pinned Y values below are computed against.
    assert_eq!(lut[0], 0);
    assert_eq!(lut[4], 128);
    assert_eq!(lut[8], 256);

    let terrain = scene
        .field_terrain_tiles(&index)
        .expect("read terrain tiles")
        .expect("balden has a field map");
    assert!(
        terrain.len() > 400,
        "balden terrain sweep collapsed: {} tiles",
        terrain.len()
    );

    // Every terrain tile carries the 2x2 corner block the retail emitters
    // average - the discriminator `field_env` keys the terrain formula on.
    assert!(
        terrain.iter().all(|t| t.floor_corner_nibbles.is_some()),
        "terrain tiles missing the corner-nibble block"
    );
    // The class the fix targets is non-empty: tier-edge cells whose corner
    // block is mixed (the corner average differs from the single nibble).
    let edge_cells = terrain
        .iter()
        .filter(|t| {
            t.floor_corner_nibbles
                .is_some_and(|c| c.iter().any(|&n| n != c[0]))
        })
        .count();
    assert!(
        edge_cells > 100,
        "balden should have >100 tier-edge terrain cells, found {edge_cells}"
    );

    // Resolve through the shared kernel with an identity env-pack mapping.
    let max_pack = terrain
        .iter()
        .filter_map(|t| t.pack_index)
        .max()
        .unwrap_or(0) as usize;
    let env_tmds: Vec<usize> = (0..=max_pack).collect();
    let (draws, _) = field_env::resolve_env_draws(&env_tmds, &terrain, Some(lut));
    let y_at = |col: u8, row: u8| -> i32 {
        let i = terrain
            .iter()
            .position(|t| t.col == col && t.row == row)
            .unwrap_or_else(|| panic!("no terrain tile at ({col}, {row})"));
        draws[i].world_y
    };

    // Two adjacent quay-street ramp cells, both object 154 (mesh pack 82,
    // y_off +64): (61, 2) spans tiers 0->4 (corners [0, 4, 0, 4]) and lands
    // mid-slope at Y = -64 + 64 = 0; (62, 2) spans tiers 4->8 (corners
    // [4, 8, 4, 8]) and lands at Y = -192 + 64 = -128. The superseded
    // single-nibble sample put them at +64 / -64 - each snapped a half-tier
    // off its baked ramp, which is the visible stair-step.
    let t61 = terrain
        .iter()
        .find(|t| (t.col, t.row) == (61, 2))
        .expect("terrain tile at (61, 2)");
    assert_eq!(t61.obj_idx, 154);
    assert_eq!(t61.floor_corner_nibbles, Some([0, 4, 0, 4]));
    assert_eq!(t61.y_off, 64);
    assert_eq!(y_at(61, 2), 0);
    let t62 = terrain
        .iter()
        .find(|t| (t.col, t.row) == (62, 2))
        .expect("terrain tile at (62, 2)");
    assert_eq!(t62.floor_corner_nibbles, Some([4, 8, 4, 8]));
    assert_eq!(y_at(62, 2), -128);

    // The placed-object layer keeps the single-nibble retail formula
    // (`FUN_8003A55C`): balden's lamp object 132 at (25, 10) sits on a
    // tier-4 tile -> Y = -128.
    let placements = scene
        .field_object_placements(&index)
        .expect("read placements")
        .expect("field map");
    let lamp = placements
        .iter()
        .find(|p| p.obj_idx == 132 && (p.col, p.row) == (25, 10))
        .expect("balden placement 132 at (25, 10)");
    assert_eq!(lamp.floor_nibble, Some(4));
    assert_eq!(lamp.floor_corner_nibbles, None);
    let (pdraws, _) = field_env::resolve_env_draws(
        &(0..=lamp.pack_index.unwrap() as usize).collect::<Vec<_>>(),
        std::slice::from_ref(lamp),
        Some(lut),
    );
    assert_eq!(pdraws[0].world_y, -128);

    eprintln!(
        "balden: {} terrain tiles ({edge_cells} tier-edge), corner-average Y verified",
        terrain.len()
    );
}
