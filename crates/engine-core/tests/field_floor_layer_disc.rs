//! Disc-gated: every field scene on the disc must resolve a **floor**.
//!
//! A field/town scene's ground is not in its mesh pack. It is built from the
//! `.MAP` floor grid at `+0x4000` lifted by the scene's 16-entry
//! **floor-height LUT**, which retail's MAN loader `FUN_8003AEB0` installs
//! from the scene MAN's header
//! ([`legaia_asset::field_objects::build_walk_heightfield`],
//! `Scene::field_floor_height_lut`). So the floor depends on a chain of three
//! resolutions, and the last two are silent when they fail:
//!
//! ```text
//!   scene bundle entry -> MAN payload -> floor-height LUT -> walk heightfield
//! ```
//!
//! `Scene::walk_heightfield` returns `None` the moment the LUT is missing, and
//! `field_env::resolve_env_draws` falls back to `world_y = 0` for every placed
//! object and terrain tile. A scene whose MAN stops resolving therefore keeps
//! its walls and props - flattened onto the origin plane - and loses its ground
//! surface completely. That is a floorless map that no draw-count assertion
//! notices, which is exactly how it shipped once: an entry-window change took
//! the MAN out from under 79 of the disc's 101 field-map blocks and the only
//! floor coverage in the tree was a three-scene disjunction
//! (`web-viewer/tests/field_scene_assembly.rs`: `ground_quads > 0 ||
//! terrain.len() > 20`) that the surviving terrain layer kept satisfied.
//!
//! These assertions are corpus-wide and name their exceptions, so the failure
//! mode is a diff of scene names rather than a count that can be re-pinned.
//!
//! Skips (and passes) when `LEGAIA_DISC_BIN` is unset.

use legaia_engine_core::scene::{ProtIndex, Scene};
use std::path::Path;

/// Disc gate: the extracted `PROT.DAT` + `CDNAME.TXT` (either the crate-local
/// or the workspace-root copy) **and** `LEGAIA_DISC_BIN`. Absent either, the
/// test skips and passes - the repo-wide rule.
fn open_index() -> Option<ProtIndex> {
    std::env::var_os("LEGAIA_DISC_BIN")?;
    for root in ["extracted", "../../extracted"] {
        let p = Path::new(root);
        if p.join("PROT.DAT").exists() && p.join("CDNAME.TXT").exists() {
            return ProtIndex::open_extracted(p).ok();
        }
    }
    None
}

/// CDNAME blocks that resolve a field `.MAP` with a populated object layer.
/// The disc has 101 - every block a field/town/world-map load can enter, plus
/// the two `other*` aliases that share another scene's block.
const FIELD_MAP_BLOCKS: usize = 101;

/// The field-map blocks that resolve **no** MAN, and therefore no floor-height
/// LUT, and are expected not to:
///
/// - `bubu1` / `edbubu` - two of the scenes on the documented streaming-MAN
///   fallback path
///   ([`docs/formats/scene-bundles.md`](../../../docs/formats/scene-bundles.md)).
///   Their floor has never resolved through this chain; they are a standing
///   gap, not a regression, and they are named here so that stays visible
///   rather than averaged into a count.
/// - `gameover_data` - not a scene. Its CDNAME block is a strict subset of
///   `town01`'s (extraction entries 1..3 of 1..10) and holds no asset-table
///   bundle at all: only `town01`'s `.MAP` and its one-sector v12 table. The
///   MAN it used to report *was* `town01`'s MAN, reached because a
///   one-sector entry read under the historical `toc[p+5] - toc[p+3] + 4`
///   size ran into `town01`'s bundle
///   ([`docs/formats/prot.md`](../../../docs/formats/prot.md)).
const NO_MAN: &[&str] = &["bubu1", "edbubu", "gameover_data"];

/// The two field-map blocks whose object layer is placements only - no visible
/// terrain cells and no heightfield. Both are the Karisto castle exterior
/// courtyard, whose ground is placed slab meshes.
const PLACEMENTS_ONLY: &[&str] = &["edkorout", "korout"];

/// One field-map block's floor chain.
struct Floor {
    name: String,
    has_man: bool,
    has_lut: bool,
    ground_quads: usize,
    terrain: usize,
    placements: usize,
}

fn sweep(index: &ProtIndex) -> Vec<Floor> {
    let mut out = Vec::new();
    for name in index.cdname_scene_names() {
        let Ok(scene) = Scene::load(index, &name) else {
            continue;
        };
        if scene.field_map_index(index).is_none() {
            continue;
        }
        let placements = scene
            .field_object_placements(index)
            .ok()
            .flatten()
            .unwrap_or_default()
            .len();
        // The terrain layer as the play page draws it: visible cells whose
        // record is *not* `FLAG_PLACED` (those are already drawn, posed, by
        // the placement layer - `web_viewer::play::build_field_render`).
        let terrain = scene
            .field_terrain_tiles(index)
            .ok()
            .flatten()
            .unwrap_or_default()
            .iter()
            .filter(|t| t.flags & legaia_asset::field_objects::FLAG_PLACED == 0)
            .count();
        // A block with neither is an asset block that merely shares a
        // neighbour's `.MAP` index; it has no field map of its own.
        if placements == 0 && terrain == 0 {
            continue;
        }
        out.push(Floor {
            has_man: scene
                .field_man_payload(index)
                .ok()
                .flatten()
                .is_some_and(|m| !m.is_empty()),
            has_lut: scene.field_floor_height_lut(index).ok().flatten().is_some(),
            ground_quads: scene
                .walk_heightfield(index)
                .ok()
                .flatten()
                .map(|h| h.quad_count())
                .unwrap_or(0),
            terrain,
            placements,
            name,
        });
    }
    out
}

/// The regression guard: the MAN -> floor-LUT chain must resolve for every
/// field-map block on the disc bar the three named ones.
///
/// This is the assertion that fails when a PROT-entry-window or bundle-detection
/// change takes a scene's MAN away. It failed 79-wide once and nothing caught
/// it, because the geometry that *does* survive (walls, props, terrain tiles)
/// keeps every draw-count test green.
#[test]
fn every_field_scene_resolves_its_floor_height_lut() {
    let Some(index) = open_index() else {
        eprintln!("[skip] LEGAIA_DISC_BIN / extracted PROT.DAT unset; skipping floor-layer sweep");
        return;
    };
    let floors = sweep(&index);
    assert_eq!(
        floors.len(),
        FIELD_MAP_BLOCKS,
        "field-map block count changed - the sweep is measuring a different corpus, \
         so the per-scene assertions below are no longer comparable"
    );

    let mut no_man: Vec<&str> = floors
        .iter()
        .filter(|f| !f.has_man)
        .map(|f| f.name.as_str())
        .collect();
    no_man.sort_unstable();
    assert_eq!(
        no_man, NO_MAN,
        "the set of field-map blocks with no MAN changed. A scene that appears \
         here has lost its floor-height LUT and renders with no ground surface \
         and every prop flattened to y = 0 (see this file's module docs)"
    );

    // The LUT is the MAN header's, so the two sets must coincide exactly - a
    // scene with a MAN whose LUT does not resolve is a second, distinct
    // failure and would otherwise hide behind the assertion above.
    let mut no_lut: Vec<&str> = floors
        .iter()
        .filter(|f| !f.has_lut)
        .map(|f| f.name.as_str())
        .collect();
    no_lut.sort_unstable();
    assert_eq!(
        no_lut, NO_MAN,
        "floor-height LUT resolution diverged from MAN resolution"
    );
}

/// Every field-map block must present *a* floor layer: a walk heightfield, or
/// the terrain-tile meshes that floor the castle interiors, or - for the two
/// courtyards - placed slabs.
#[test]
fn every_field_scene_presents_a_ground_layer() {
    let Some(index) = open_index() else {
        eprintln!("[skip] LEGAIA_DISC_BIN / extracted PROT.DAT unset; skipping ground-layer sweep");
        return;
    };
    let floors = sweep(&index);
    assert_eq!(floors.len(), FIELD_MAP_BLOCKS);

    // Hard floor on the heightfield population. 81 blocks carry one; the rest
    // are interiors that floor with terrain-tile meshes instead. A change that
    // takes the LUT away drops this to a handful.
    let with_heightfield = floors.iter().filter(|f| f.ground_quads > 0).count();
    assert!(
        with_heightfield >= 81,
        "only {with_heightfield} of {} field-map blocks build a walk heightfield \
         (expected at least 81)",
        floors.len()
    );

    let mut placements_only: Vec<&str> = floors
        .iter()
        .filter(|f| f.ground_quads == 0 && f.terrain == 0)
        .map(|f| f.name.as_str())
        .collect();
    placements_only.sort_unstable();
    assert_eq!(
        placements_only, PLACEMENTS_ONLY,
        "a field-map block has neither a heightfield nor terrain tiles"
    );

    for f in &floors {
        assert!(
            f.ground_quads > 0 || f.terrain > 0 || f.placements > 0,
            "{}: no floor layer at all",
            f.name
        );
    }
}

/// The two scenes the floorless-map report named, pinned by value: Rim Elm
/// (the opening town) and Biron Monastery. Both floor with a heightfield, so
/// each is a single number that goes to zero the moment the chain breaks.
#[test]
fn rim_elm_and_biron_monastery_floor_with_a_heightfield() {
    let Some(index) = open_index() else {
        eprintln!("[skip] LEGAIA_DISC_BIN / extracted PROT.DAT unset; skipping named-scene pins");
        return;
    };
    for (name, quads) in [("town01", 1946usize), ("bylon", 1574)] {
        let scene = Scene::load(&index, name).expect("scene loads");
        assert!(
            scene
                .field_floor_height_lut(&index)
                .expect("LUT read")
                .is_some(),
            "{name}: no floor-height LUT"
        );
        let hf = scene
            .walk_heightfield(&index)
            .expect("heightfield read")
            .expect("heightfield present");
        assert_eq!(hf.quad_count(), quads, "{name}: ground quad count");
    }
}
