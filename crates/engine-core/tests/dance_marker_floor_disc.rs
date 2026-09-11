//! Disc-gated: the dance floor's **step-marker tile pool** - the per-cell
//! field-actor sink `minigame_floor::floor_tile_spawns` and
//! `minigame_floor::marker_template` were both blocked on.
//!
//! Retail's floor pass `FUN_801D2A10` walks the venue's floor rect, spawns one
//! tile actor per drawn cell, and gives a cell whose kind-1 `.MAP` record
//! resolves to clip `6..=9` the marker template `DAT_801D4314` with `clip - 6`
//! in `+0x50`. The per-frame handler `FUN_801D0640` then flips that actor's
//! mesh through the class row of the script table at `0x801D44CC`.
//!
//! What this measures, from the real disc rather than from the reading:
//! whether the dance venue (`other7`) has any marker cells at all, how many,
//! which classes they take, and that the pool's flipbook actually swaps meshes
//! over a song's worth of frames.
//!
//! Skips when `LEGAIA_DISC_BIN` / `extracted/` are missing.

use std::path::PathBuf;
use std::sync::Arc;

use legaia_engine_core::field_regions::parse_tile_triggers;
use legaia_engine_core::minigame_floor::{FloorGrid, MarkerFloor, height_ramp};
use legaia_engine_core::scene::{ProtIndex, Scene};

/// `.MAP` primary trigger block, and the `+0x12000` fallback.
const TRIGGER_BLOCK_OFFSET: usize = 0x10000;
const TRIGGER_FALLBACK_OFFSET: usize = 0x12000;

fn extracted_dir() -> Option<PathBuf> {
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

fn gate() -> Option<Arc<ProtIndex>> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    let extracted = extracted_dir().or_else(|| {
        eprintln!("[skip] extracted/ missing");
        None
    })?;
    Some(Arc::new(
        ProtIndex::open_extracted(&extracted).expect("open prot index"),
    ))
}

fn dance_overlay(index: &ProtIndex) -> Option<Vec<u8>> {
    let rec = legaia_asset::static_overlay::overlay_map()
        .by_prot_index(legaia_asset::dance_chart::DANCE_OVERLAY_PROT_INDEX as u32)?;
    let bytes = index.entry_bytes_extended(rec.prot_index).ok()?;
    legaia_asset::static_overlay::as_loaded(&bytes, rec).ok()
}

/// Build the venue's marker pool from the disc and report what it found.
#[test]
fn the_dance_venue_has_marker_cells_and_they_flip() {
    let Some(index) = gate() else { return };
    let Some(overlay) = dance_overlay(&index) else {
        panic!("dance overlay 0980 must resolve");
    };
    let script = legaia_engine_vm::dance_marker::MarkerScript::from_overlay(&overlay, 0x801C_E818)
        .expect("the marker script table must parse out of the dance overlay");
    for class in 0..legaia_engine_vm::dance_marker::MARKER_SCRIPT_ROWS {
        eprintln!("[ok] marker class {class}: {} steps", script.steps(class));
    }

    let scene =
        Scene::load(&index, legaia_asset::dance_cast::DANCE_SCENE_NAME).expect("load other7");
    let map_idx = scene.field_map_index(&index).expect("venue .MAP entry");
    let map = index.entry_bytes_extended(map_idx).expect("map bytes");
    let primary = parse_tile_triggers(&map[TRIGGER_BLOCK_OFFSET..]);
    let fallback = map
        .get(TRIGGER_FALLBACK_OFFSET..)
        .map(parse_tile_triggers)
        .unwrap_or_default();
    eprintln!(
        "[ok] other7 kind-1 triggers: {} primary + {} fallback",
        primary.len(),
        fallback.len()
    );

    let ramp = height_ramp();
    let mut floor = MarkerFloor::build(
        FloorGrid::new(&map),
        &ramp,
        0,
        0,
        legaia_engine_core::minigame_floor::GRID_EXTENT,
        legaia_engine_core::minigame_floor::GRID_EXTENT,
        false,
        &primary,
        &fallback,
        script,
    );
    let mut per_class = [0usize; legaia_engine_vm::dance_marker::MARKER_SCRIPT_ROWS];
    for t in floor.tiles() {
        if let Some(c) = per_class.get_mut(t.actor.class as usize) {
            *c += 1;
        }
    }
    eprintln!(
        "[ok] other7 marker tiles: {} total, per class {per_class:?}",
        floor.len()
    );

    // Whatever the count, the pool must not invent tiles: every one carries a
    // class inside the four-row table.
    for t in floor.tiles() {
        assert!(
            (t.actor.class as usize) < legaia_engine_vm::dance_marker::MARKER_SCRIPT_ROWS,
            "class {} is outside the script table",
            t.actor.class
        );
    }

    if floor.is_empty() {
        // A negative result is a result: record it rather than asserting a
        // count the disc does not have. The pool machinery is still exercised
        // by the synthetic tests in `minigame_floor`.
        eprintln!("[ok] the venue's own .MAP carries no clip-6..9 cell");
        return;
    }

    // The flipbook: over a song's worth of frames the staged meshes must
    // actually change, and every staged value must be a plausible pack index.
    let mut staged: Vec<Vec<i16>> = vec![Vec::new(); floor.len()];
    for _ in 0..240 {
        floor.step(1, 0);
        for (i, t) in floor.tiles().iter().enumerate() {
            if let Some(m) = t.actor.mesh
                && staged[i].last() != Some(&m)
            {
                staged[i].push(m);
            }
        }
    }
    let swapping = staged.iter().filter(|s| s.len() > 1).count();
    eprintln!(
        "[ok] {swapping} of {} marker tiles swapped mesh within 240 frames",
        floor.len()
    );
    assert!(
        swapping > 0,
        "a marker tile whose mesh never changes is not a flipbook"
    );
    for s in &staged {
        for &m in s {
            assert!(m >= 0, "a staged pack index must not be negative");
        }
    }
}
