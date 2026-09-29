//! Disc-gated: retail's **visible-tile crop** (`FUN_801F7088` and the ground
//! emitter it calls) over real scene maps, through the shared
//! `field_view_window` kernel both play hosts ask.
//!
//! For every CDNAME field scene with a `.MAP`, the focus is stood at the
//! centre of every fourth ground tile in turn, the walk-region box is latched
//! at that tile the way the camera latches it, and the entry window
//! (`FIELD_DEFAULT_VIEW_WINDOW`) is cropped. What this pins:
//!
//! 1. **The region clamp is live on the disc.** `town01` has focus tiles
//!    whose window the clamp shrinks, and at such a tile the crop draws
//!    strictly fewer ground quads and terrain draws than the whole map.
//! 2. **The player never stands on a hole.** Wherever the focus tile is a
//!    ground cell at least one tile inside its region box, that cell is
//!    among the ground quads the crop keeps - except on the grid's last three
//!    rows, which the `0x7E` far-Z cap keeps the ground emitter from ever
//!    reaching.
//!
//! The per-scene table it prints (mean share of the ground / terrain lists
//! kept, and how many samples the clamp moved) is the "which scenes' draw
//! counts change" readout.
//!
//! Skips (and passes) without `LEGAIA_DISC_BIN` or `extracted/`.

use std::path::PathBuf;

use legaia_engine_core::field_ground;
use legaia_engine_core::field_regions::{self, RegionTable};
use legaia_engine_core::field_view_window::{self, CellKey};
use legaia_engine_core::mode_entry_init::FIELD_DEFAULT_VIEW_WINDOW;
use legaia_engine_core::scene::{ProtIndex, Scene};
use legaia_engine_core::world::field_npc_cull::FieldCullView;

fn extracted_dir() -> Option<PathBuf> {
    if let Some(d) = std::env::var_os("LEGAIA_EXTRACTED_DIR").map(PathBuf::from)
        && d.join("PROT.DAT").exists()
    {
        return Some(d);
    }
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

#[derive(Default, Debug)]
struct SceneCrop {
    samples: usize,
    clamped: usize,
    ground_total: usize,
    terrain_total: usize,
    ground_kept_sum: f64,
    terrain_kept_sum: f64,
    /// A clamped sample where both layers lost draws: `(tile, ground kept,
    /// terrain kept)`.
    clamped_example: Option<((i32, i32), usize, usize)>,
    /// Interior ground focus tiles the crop failed to draw.
    holes: Vec<(i32, i32)>,
}

fn crop_scene(index: &ProtIndex, name: &str) -> Option<SceneCrop> {
    let scene = Scene::load(index, name).ok()?;
    let map_idx = scene.field_map_index(index)?;
    let map = index.entry_bytes_extended(map_idx).ok()?;
    let block = map.get(0x10000..0x12000)?;
    let table = RegionTable::parse(block);
    let hf = scene.walk_heightfield(index).ok().flatten()?;
    if hf.indices.is_empty() {
        return None;
    }
    let indices = field_ground::render_indices(&hf);
    let terrain: Vec<CellKey> = scene
        .field_terrain_tiles(index)
        .ok()
        .flatten()
        .unwrap_or_default()
        .into_iter()
        .filter(|p| p.flags & legaia_asset::field_objects::FLAG_PLACED == 0)
        .map(|p| CellKey {
            cell: (p.col, p.row),
            cull_radius: p.cull_radius,
        })
        .collect();
    // The ground cells, from each quad's lowest vertex.
    let ground_cells: Vec<(i32, i32)> = indices
        .chunks(6)
        .filter_map(|q| q.iter().min())
        .map(|&b| {
            let p = hf.positions[b as usize];
            ((p[0] / 128.0) as i32, (p[2] / 128.0) as i32)
        })
        .collect();
    let ground_set: std::collections::HashSet<_> = ground_cells.iter().copied().collect();
    let (x0, z0, x1, z1) = FIELD_DEFAULT_VIEW_WINDOW;
    let mut out = SceneCrop {
        ground_total: ground_cells.len(),
        terrain_total: terrain.len(),
        ..Default::default()
    };
    for &(tx, tz) in ground_cells.iter().step_by(4) {
        let (_, attrs) = field_regions::refresh_region_attributes(table.as_ref(), tx, tz, false);
        let view = FieldCullView {
            focus_stored: [-(tx * 128 + 64), -(tz * 128 + 64)],
            attr_box: attrs.box_bytes,
            window: [x0, z0, x1, z1],
        };
        let [r0, r1, r2, r3] = attrs.box_bytes.map(i32::from);
        if tx < r0 || tx >= r2 || tz < r1 || tz >= r3 {
            continue; // the policy draws such a frame whole
        }
        let cells = field_view_window::view_cells(&view);
        let ground_kept =
            field_ground::crop_indices(&hf.positions, &indices, Some(&cells)).len() / 6;
        let terrain_kept = terrain
            .iter()
            .filter(|k| field_view_window::terrain_draw_visible(Some(&cells), **k))
            .count();
        out.samples += 1;
        out.ground_kept_sum += ground_kept as f64 / out.ground_total.max(1) as f64;
        out.terrain_kept_sum += terrain_kept as f64 / out.terrain_total.max(1) as f64;
        if cells.window != view.window {
            out.clamped += 1;
            if out.clamped_example.is_none()
                && ground_kept < out.ground_total
                && terrain_kept < out.terrain_total
            {
                out.clamped_example = Some(((tx, tz), ground_kept, terrain_kept));
            }
        }
        // Row 125 and up are never ground-drawn at all: the clamp caps the
        // far Z bound at 0x7E, and the ground walks one row short of it
        // (`FUN_801F7088` 0x801F73B4..0x801F73D0 with FUN_801F6D48's row loop).
        let interior = tx > r0 && tx + 1 < r2 && tz > r1 && tz + 1 < r3 && tz <= 0x7C;
        if interior && ground_set.contains(&(tx, tz)) && !cells.ground_visible(tx, tz) {
            out.holes.push((tx, tz));
        }
    }
    Some(out)
}

#[test]
fn the_visible_tile_crop_over_every_field_scene() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        return;
    };
    let index = ProtIndex::open_extracted(&extracted).expect("open ProtIndex");
    let mut rows = Vec::new();
    for name in index.cdname_scene_names() {
        if legaia_engine_core::scene::is_world_map_scene(&name) {
            continue;
        }
        if let Some(c) = crop_scene(&index, &name)
            && c.samples > 0
        {
            rows.push((name, c));
        }
    }
    eprintln!("[ran] {} field scenes with a ground layer", rows.len());
    eprintln!("scene      samples clamped  ground%  terrain%  (whole: ground/terrain)");
    for (name, c) in &rows {
        eprintln!(
            "{:<10} {:>7} {:>7} {:>8.1} {:>9.1}  ({}/{})",
            name,
            c.samples,
            c.clamped,
            100.0 * c.ground_kept_sum / c.samples as f64,
            100.0 * c.terrain_kept_sum / c.samples as f64,
            c.ground_total,
            c.terrain_total
        );
    }
    let town01 = rows
        .iter()
        .find(|(n, _)| n == "town01")
        .map(|(_, c)| c)
        .expect("town01 has a ground layer");
    assert!(
        town01.clamped > 0,
        "town01: no focus tile clamps the window"
    );
    let (tile, g, t) = town01
        .clamped_example
        .expect("town01: a clamped sample drops draws from both layers");
    eprintln!(
        "town01 clamped at {tile:?}: ground {g}/{}, terrain {t}/{}",
        town01.ground_total, town01.terrain_total
    );
    assert!(g > 0, "town01: the clamped crop kept no ground");
    let holes: Vec<_> = rows
        .iter()
        .filter(|(_, c)| !c.holes.is_empty())
        .map(|(n, c)| (n.clone(), c.holes.len(), c.holes[0]))
        .collect();
    assert!(
        holes.is_empty(),
        "focus ground cells cropped away: {holes:?}"
    );
}
