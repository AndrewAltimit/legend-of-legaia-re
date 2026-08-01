//! Verify the web viewer's assembled full-scene build
//! ([`legaia_web_viewer::field_scene::build_field_scene`]) resolves a real
//! map for representative field scenes: the environment mesh pack is found
//! (the `scene_asset_table` LZS TMD pack, not a lone `scene_tmd_stream`
//! slice), the `.MAP` placement + terrain layers resolve to in-range pack
//! draws, and the ground heightfield builds. For `town01` the numbers are
//! pinned against the engine-side ground truth (env entry 4; the placement
//! count matches `Scene::field_object_placements` minus the NPC records).
//!
//! Skipped (passes) when `LEGAIA_DISC_BIN` is unset, matching the rest of
//! the disc-dependent test suite. CI runs without disc data.

#![cfg(not(target_arch = "wasm32"))]

use legaia_engine_core::scene::ProtIndex;
use legaia_web_viewer::disc::{extract_cdname_txt, extract_prot_dat};
use legaia_web_viewer::field_scene::{build_field_scene, build_hybrid_env_mesh};
use std::env;
use std::fs;

/// Representative scenes: the starter town, a dungeon (Mt. Rikuroa), and a
/// Karisto castle interior - the shapes the viewer's sidebar surfaces.
const SCENES: &[&str] = &["town01", "rikuroa", "korb3"];

#[test]
fn field_scene_assembles_full_maps() {
    let Some(disc_path) = env::var_os("LEGAIA_DISC_BIN") else {
        eprintln!("LEGAIA_DISC_BIN unset; skipping field-scene assembly test");
        return;
    };
    let disc = fs::read(&disc_path).expect("disc image");
    let prot = extract_prot_dat(&disc).expect("PROT.DAT extraction");
    let cdname = extract_cdname_txt(&disc).expect("CDNAME.TXT extraction");
    let index = ProtIndex::from_bytes(prot, Some(&cdname)).expect("ProtIndex from in-memory PROT");

    for &name in SCENES {
        let pack = build_field_scene(&index, name)
            .unwrap_or_else(|e| panic!("{name}: build_field_scene failed: {e}"));
        assert!(
            pack.env_tmds.len() > 10,
            "{name}: expected a multi-mesh env pack, got {}",
            pack.env_tmds.len()
        );
        assert!(
            !pack.placements.is_empty(),
            "{name}: no placed-object draws resolved"
        );
        assert!(
            !pack.terrain.is_empty(),
            "{name}: no terrain-tile draws resolved"
        );
        // The walk-ground heightfield is scene-shape-dependent: open maps
        // (towns, mountains) carry a `0x1000`-gated floor grid; some castle
        // interiors floor entirely with terrain-tile meshes instead (korb3).
        // Require *a* ground layer: heightfield or terrain tiles.
        //
        // This disjunction is deliberately weak and is NOT the floor guard: the
        // terrain half stays satisfied when the floor-height LUT disappears, so
        // a scene can pass here with no ground surface and every prop flattened
        // to y = 0. `engine-core/tests/field_floor_layer_disc.rs` is the guard -
        // it asserts the MAN -> LUT chain over all 101 field-map blocks. Do not
        // reformulate this line as the floor coverage; it once let 79 scenes
        // ship floorless.
        // (>= 20: korb3 floors with exactly 20 terrain-tile draws once the
        // retail emitters' FLAG_PLACED skip keeps the placement layer's
        // records out of the terrain list - they still draw, as placements.)
        let ground_quads = pack.ground.as_ref().map(|h| h.quad_count()).unwrap_or(0);
        assert!(
            ground_quads > 0 || pack.terrain.len() >= 20,
            "{name}: no ground layer (0 heightfield quads, {} terrain tiles)",
            pack.terrain.len()
        );
        // town01 and rikuroa DO carry a heightfield (korb3 does not), so pin
        // that half directly rather than leaving it to the disjunction.
        if name != "korb3" {
            assert!(
                ground_quads > 0,
                "{name}: expected a walk-ground heightfield, got 0 quads - \
                 the scene's floor-height LUT did not resolve"
            );
        }
        // Every draw must reference a valid pack slot + res TMD.
        for d in pack.placements.iter().chain(pack.terrain.iter()) {
            assert!(
                d.env_slot < pack.env_tmds.len(),
                "{name}: draw env_slot {} out of range",
                d.env_slot
            );
            assert!(
                d.res_tmd < pack.res.tmds.len(),
                "{name}: draw res_tmd {} out of range",
                d.res_tmd
            );
        }
        eprintln!(
            "{name}: {} env meshes, {} placements, {} terrain tiles, {} ground quads",
            pack.env_tmds.len(),
            pack.placements.len(),
            pack.terrain.len(),
            ground_quads
        );
    }
}

#[test]
fn town01_env_pack_matches_engine_ground_truth() {
    let Some(disc_path) = env::var_os("LEGAIA_DISC_BIN") else {
        eprintln!("LEGAIA_DISC_BIN unset; skipping town01 env-pack pin test");
        return;
    };
    let disc = fs::read(&disc_path).expect("disc image");
    let prot = extract_prot_dat(&disc).expect("PROT.DAT extraction");
    let cdname = extract_cdname_txt(&disc).expect("CDNAME.TXT extraction");
    let index = ProtIndex::from_bytes(prot, Some(&cdname)).expect("ProtIndex from in-memory PROT");

    let pack = build_field_scene(&index, "town01").expect("town01 build");
    // town01's environment geometry lives in PROT entry 4 (the
    // scene_asset_table LZS TMD pack) - the vote must land there, not on a
    // scene_tmd_stream battle-mesh entry.
    let env_entry = pack.res.tmds[pack.env_tmds[0]].entry_idx;
    assert_eq!(env_entry, 4, "town01 env pack entry");
    assert!(
        pack.env_tmds
            .iter()
            .all(|&i| pack.res.tmds[i].entry_idx == env_entry),
        "env pack spans multiple entries"
    );
    // The placed-object layer: Rim Elm's buildings/props (the engine draws
    // ~40 of 46 placements; the pack-resolved draw count sits in between
    // because mesh-level prim filtering happens later, at upload).
    assert!(
        (20..=60).contains(&pack.placements.len()),
        "town01 placement draw count {} outside expected band",
        pack.placements.len()
    );
}

/// The hybrid env-mesh build (`build_hybrid_env_mesh`) must surface the
/// untextured vertex-colour prims the plain VRAM-filtered build drops - the
/// browser sibling of the native engine's colour-mesh pipeline. town01's env
/// pack carries **colour-only** placed props (slots whose textured build is
/// empty but whose flat/gouraud prims carry per-vertex RGB); before the
/// hybrid path those placements silently vanished from the assembled view.
#[test]
fn hybrid_env_mesh_recovers_vertex_colour_props() {
    let Some(disc_path) = env::var_os("LEGAIA_DISC_BIN") else {
        eprintln!("LEGAIA_DISC_BIN unset; skipping hybrid env-mesh test");
        return;
    };
    let disc = fs::read(&disc_path).expect("disc image");
    let prot = extract_prot_dat(&disc).expect("PROT.DAT extraction");
    let cdname = extract_cdname_txt(&disc).expect("CDNAME.TXT extraction");
    let index = ProtIndex::from_bytes(prot, Some(&cdname)).expect("ProtIndex from in-memory PROT");

    let pack = build_field_scene(&index, "town01").expect("town01 build");
    let referenced: std::collections::BTreeSet<usize> = pack
        .placements
        .iter()
        .chain(pack.terrain.iter())
        .map(|d| d.env_slot)
        .collect();

    let mut colour_only_recovered = 0usize;
    // Non-vacuity for the packet-colour stream: at least one textured vertex
    // somewhere in town01's referenced env slots must carry a colour word that
    // is neither white (the old placeholder) nor the neutral 0x80. Without
    // this the whole modulation could be an all-neutral no-op and every
    // structural assertion below would still pass.
    let mut non_neutral_textured = 0usize;
    for &slot in &referenced {
        let rtmd = &pack.res.tmds[pack.env_tmds[slot]];
        let textured = rtmd.build_filtered_vram_mesh(&pack.res.vram);
        let (hybrid, flat) = build_hybrid_env_mesh(rtmd, &pack.res.vram);

        // Structural invariants: flat is per-vertex [r, g, b, flag] for EVERY
        // vertex - the textured half's rgb is its `texel * colour / 128`
        // modulation word, the untextured half's is its fill - and the hybrid
        // mesh's vertex arrays stay index-aligned.
        assert_eq!(hybrid.positions.len(), hybrid.uvs.len(), "slot {slot}");
        assert_eq!(hybrid.positions.len(), hybrid.cba_tsb.len(), "slot {slot}");
        assert_eq!(hybrid.positions.len(), hybrid.colors.len(), "slot {slot}");
        if !hybrid.positions.is_empty() {
            assert_eq!(flat.len(), hybrid.positions.len() * 4, "slot {slot}");
            // The textured prefix is flagged 255, the colour tail 0.
            let n_tex = textured.positions.len();
            assert_eq!(
                flat[3],
                if n_tex == 0 { 0 } else { 255 },
                "slot {slot} head flag"
            );
            assert_eq!(
                *flat.last().unwrap(),
                if n_tex == hybrid.positions.len() {
                    255
                } else {
                    0
                },
                "slot {slot} tail flag"
            );
            for v in 0..n_tex {
                assert_eq!(flat[v * 4 + 3], 255, "slot {slot} vert {v} flag");
                let rgb = [flat[v * 4], flat[v * 4 + 1], flat[v * 4 + 2]];
                assert_ne!(rgb, [255, 255, 255], "slot {slot} vert {v}: white stand-in");
                if rgb != [0x80, 0x80, 0x80] {
                    non_neutral_textured += 1;
                }
            }
        }
        assert!(
            hybrid.indices.len() >= textured.indices.len(),
            "slot {slot}: hybrid dropped textured prims"
        );
        if textured.indices.is_empty() && !hybrid.indices.is_empty() {
            colour_only_recovered += 1;
        }
    }
    assert!(
        non_neutral_textured > 0,
        "no textured env vertex carries a non-neutral packet colour - the \
         browser's `texel * colour / 128` would be an identity no-op"
    );
    // town01 ships a handful of colour-only placed props (benches / fences /
    // small furniture; slots 31, 55, 87 at the current pack vote).
    //
    // The referenced set is exactly what `pack_mesh_index` resolves from each
    // record's `+0x10`, and env slots 97 / 109 are NOT in it. They are reachable
    // only through the falsified "field-actor band" rule (`obj_idx - 5` on object
    // ids 102 / 114) - retail resolves an object record's mesh as
    // `record[+0x10] + 5` for **every** id, band or not (a live Rim Elm actor
    // list reads obj101 -> pool 18, obj105 -> 14, obj113 -> 78, obj114 -> 84,
    // i.e. `+0x10` plus the prefix, never the id). Pool ids 93..118 *are* a real
    // model-id space - the MAN's field actors (NPCs / animated props) - but those
    // placements carry their pool index directly and never go through this
    // record path. So slots 97 / 109 belong to the NPC layer, not here.
    //
    // The assertion guards a class of mesh, not a slot set.
    assert!(
        colour_only_recovered >= 3,
        "expected >= 3 colour-only env meshes recovered on town01, got {colour_only_recovered}"
    );
}

/// The boot-resident system-UI bundle (raw PROT TOC entries 0/1, uploaded
/// under every field build via `BuildOptions::system_ui`) makes the
/// row-510-strip / `(960,256)`-atlas samplers render in the browser build:
/// town01 env slots 21/26/74 and rikuroa slots 50/51/63 (all CBA
/// `(64,510)`, texpage `(960,256)`) previously built EMPTY because no scene
/// TIM uploads that CLUT row. Guard both scenes in the new direction.
#[test]
fn system_ui_bundle_recovers_row510_samplers() {
    let Some(disc_path) = env::var_os("LEGAIA_DISC_BIN") else {
        eprintln!("LEGAIA_DISC_BIN unset; skipping row-510 sampler test");
        return;
    };
    let disc = fs::read(&disc_path).expect("disc image");
    let prot = extract_prot_dat(&disc).expect("PROT.DAT extraction");
    let cdname = extract_cdname_txt(&disc).expect("CDNAME.TXT extraction");
    let index = ProtIndex::from_bytes(prot, Some(&cdname)).expect("ProtIndex from in-memory PROT");

    for (name, slots) in [
        ("town01", &[21usize, 26, 74][..]),
        ("rikuroa", &[50, 51, 63][..]),
    ] {
        let pack = build_field_scene(&index, name)
            .unwrap_or_else(|e| panic!("{name}: build_field_scene failed: {e}"));
        // The bundle's strip CLUT is resident on row 510.
        let lit = (0..256)
            .filter(|&x| pack.res.vram.pixel(x, 510) != 0)
            .count();
        assert!(lit > 200, "{name}: row-510 strip resident (lit={lit})");
        for &slot in slots {
            let rtmd = &pack.res.tmds[pack.env_tmds[slot]];
            let mesh = rtmd.build_filtered_vram_mesh(&pack.res.vram);
            assert!(
                !mesh.indices.is_empty(),
                "{name} env slot {slot}: row-510 sampler must build non-empty \
                 with the system-UI bundle resident"
            );
        }
    }
}

/// World-map scenes: the assembled pack must carry **both** sparse walk-frame
/// layers - the placed landmarks *and* the decoration cells (crossed-quad
/// trees, mountain groups, props).
///
/// The browser path previously resolved only `walk_object_placements`, so the
/// site's full-map view of Drake / Sebucus / Karisto lost the whole decoration
/// layer while the native play-window
/// (`resolve_world_map_terrain_draws`) drew it. The two are pinned against each
/// other here: the pack's placement draws must equal what
/// `walk_object_placements ++ walk_decoration_placements` resolves through the
/// same `field_env` kernel, and the decoration half must be a real share of it.
#[test]
fn world_map_scenes_include_the_walk_decoration_layer() {
    use legaia_engine_core::field_env;
    use legaia_engine_core::scene::Scene;

    /// The three kingdom world maps.
    const KINGDOMS: &[&str] = &["map01", "map02", "map03"];

    let Some(disc_path) = env::var_os("LEGAIA_DISC_BIN") else {
        eprintln!("LEGAIA_DISC_BIN unset; skipping world-map decoration test");
        return;
    };
    let disc = fs::read(&disc_path).expect("disc image");
    let prot = extract_prot_dat(&disc).expect("PROT.DAT extraction");
    let cdname = extract_cdname_txt(&disc).expect("CDNAME.TXT extraction");
    let index = ProtIndex::from_bytes(prot, Some(&cdname)).expect("ProtIndex from in-memory PROT");

    for &name in KINGDOMS {
        let pack = build_field_scene(&index, name)
            .unwrap_or_else(|e| panic!("{name}: build_field_scene failed: {e}"));
        let scene = Scene::load(&index, name).expect("scene load");
        let floor_lut = scene.field_floor_height_lut(&index).ok().flatten();

        // Engine side, resolved exactly the way the native window does.
        let landmarks = scene
            .walk_object_placements(&index)
            .expect("walk_object_placements")
            .unwrap_or_else(|| panic!("{name}: no walk landmarks"));
        let deco = scene
            .walk_decoration_placements(&index)
            .expect("walk_decoration_placements")
            .unwrap_or_else(|| panic!("{name}: no walk decorations"));
        assert!(!deco.is_empty(), "{name}: decoration record layer is empty");

        let (landmark_draws, _) =
            field_env::resolve_env_draws(&pack.env_tmds, &landmarks, floor_lut);
        let mut both = landmarks.clone();
        both.extend(deco.iter().cloned());
        let (expected, _) = field_env::resolve_env_draws(&pack.env_tmds, &both, floor_lut);

        assert_eq!(
            pack.placements, expected,
            "{name}: assembled world-map layer must be landmarks ++ decorations"
        );
        let deco_draws = expected.len() - landmark_draws.len();
        assert!(
            deco_draws > 0,
            "{name}: the decoration layer resolved to no draws - the browser \
             full-map view would be missing the tree / mountain decor the \
             native play-window draws"
        );
        // World maps carry no bulk terrain-tile layer; their ground is the
        // continent heightfield.
        assert!(pack.terrain.is_empty(), "{name}: unexpected terrain layer");
        assert!(
            pack.ground.as_ref().is_some_and(|h| h.quad_count() > 0),
            "{name}: no continent heightfield"
        );
        for d in &pack.placements {
            assert!(d.env_slot < pack.env_tmds.len(), "{name}: env_slot range");
            assert!(d.res_tmd < pack.res.tmds.len(), "{name}: res_tmd range");
        }
        eprintln!(
            "{name}: {} walk draws = {} landmark + {deco_draws} decoration \
             ({} decoration records)",
            pack.placements.len(),
            landmark_draws.len(),
            deco.len()
        );
    }
}
