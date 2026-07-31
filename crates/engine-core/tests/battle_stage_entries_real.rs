//! Disc-gated: which PROT entry a battle uses as its stage backdrop.
//!
//! A scene bundle is a fixed slot array - `.MAP`, v12 table, event scripts,
//! asset table, texture pack, then one `scene_tmd_stream` per sub-area. The
//! battle backdrop is the stream the retail scene loader leaves in
//! `_DAT_8007B864`, and it is **not** uniformly the block's first stream:
//! `map01` (overworld) uses its first (PROT 88), Rim Elm `town01` uses its
//! second (PROT 7) - the entry the three Tetsu tutorial anchors hold resident.
//!
//! Entry 6, the block's first stream, is a *different* sub-area's backdrop.
//! It byte-matches the Tetsu battle's resident dome only in its over-read tail
//! (past `(next_lba - lba) * 0x800`), which is exactly the phantom hit any
//! "scan the block for the dome" sweep has to reject.
use std::path::PathBuf;

use legaia_engine_core::scene::SceneHost;

fn extracted_dir() -> Option<PathBuf> {
    std::env::var_os("LEGAIA_DISC_BIN")?;
    for d in ["extracted", "../../extracted"] {
        let p = PathBuf::from(d);
        if p.join("PROT.DAT").exists() && p.join("CDNAME.TXT").exists() {
            return Some(p);
        }
    }
    None
}

/// Objects + total vertices of a stage entry's leading dome TMD.
fn dome_shape(host: &SceneHost, idx: u32) -> (usize, usize) {
    let bytes = host.index.entry_bytes(idx).expect("read stage entry");
    let s = legaia_asset::scene_tmd_stream::detect(&bytes).expect("stage is a scene_tmd_stream");
    let tmd = legaia_tmd::parse(&bytes[s.tmd_range()]).expect("parse dome TMD");
    let verts = tmd.objects.iter().map(|o| o.vertices.len()).sum();
    (tmd.objects.len(), verts)
}

/// Object 0's vertex pool - the byte run a resident dome is identified by.
fn dome_vertex_pool(host: &SceneHost, idx: u32) -> Vec<u8> {
    let bytes = host.index.entry_bytes(idx).expect("read stage entry");
    let s = legaia_asset::scene_tmd_stream::detect(&bytes).expect("stage is a scene_tmd_stream");
    let tmd = legaia_tmd::parse(&bytes[s.tmd_range()]).expect("parse dome TMD");
    tmd.objects[0]
        .vertices
        .iter()
        .flat_map(|v| {
            [v.x, v.y, v.z, 0i16]
                .into_iter()
                .flat_map(|c| c.to_le_bytes())
        })
        .collect()
}

#[test]
fn map01_battle_stage_is_prot_88() {
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ or LEGAIA_DISC_BIN missing");
        return;
    };
    let host = SceneHost::open_extracted(&extracted).expect("open SceneHost");
    let stages = host.index.battle_stage_entries("map01");
    eprintln!("map01 battle-stage entries: {stages:?}");
    assert!(
        stages.contains(&88),
        "map01 stage should include PROT 88, got {stages:?}"
    );
    // The overworld backdrop is the block's first stage stream.
    assert_eq!(host.index.battle_stage_entry_for_scene("map01"), Some(88));
    assert_eq!(dome_shape(&host, 88), (4, 340), "map01 dome shape");
}

#[test]
fn town01_battle_stage_is_prot_7_not_the_blocks_first_stream() {
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ or LEGAIA_DISC_BIN missing");
        return;
    };
    let host = SceneHost::open_extracted(&extracted).expect("open SceneHost");

    // Rim Elm's bundle carries four sub-area backdrops at bundle slots 5..=8.
    let stages = host.index.battle_stage_entries("town01");
    eprintln!("town01 battle-stage entries: {stages:?}");
    assert_eq!(
        stages,
        vec![6, 7, 8, 9],
        "town01 bundle slots 5..=8 are its stage streams"
    );

    // The Tetsu battle is fought inside the second, not the first.
    assert_eq!(
        host.index.battle_stage_entry_for_scene("town01"),
        Some(7),
        "Rim Elm's own backdrop is bundle slot 6 = PROT 7"
    );

    // Shape of the dome the retail Tetsu-battle states hold resident: two
    // objects, 311 + 30 vertices.
    assert_eq!(dome_shape(&host, 7), (2, 341), "Rim Elm dome shape");
    // Slots 5 and 8 are separable on shape alone.
    for other in [6u32, 9] {
        assert_ne!(
            dome_shape(&host, other),
            (2, 341),
            "PROT {other} must not be confusable with the Rim Elm dome"
        );
    }
    // Slot 7 (PROT 8) is the same *shape* but different geometry - only the
    // bytes separate them, which is why a shape-only match is not enough to
    // identify a resident dome.
    assert_eq!(dome_shape(&host, 8), (2, 341));
    assert_ne!(
        dome_vertex_pool(&host, 7),
        dome_vertex_pool(&host, 8),
        "PROT 7 and 8 share a vertex count but not their vertices"
    );
}

#[test]
fn battle_stage_overlay_entry_matches_the_plus_0x47_band() {
    use legaia_engine_core::overlay_loader::battle_stage_overlay_entry;
    // Stage id 0 = no stage overlay (the `beq v1, zero` arm of FUN_800520F0).
    assert_eq!(battle_stage_overlay_entry(0), None);
    // The Tetsu tutorial battle: `_DAT_8007B64A = 1`, loader-B tracker 0x48.
    assert_eq!(battle_stage_overlay_entry(1), Some(967));
    // The `*_DAT_8007BD0C == 0xB5` per-formation override.
    assert_eq!(battle_stage_overlay_entry(2), Some(968));
}

/// The picked stage entry must actually surface as a parsed TMD in a
/// `SceneLoadKind::Battle` build - otherwise `build_battle_stage` silently
/// returns `None` and the battle renders with no backdrop at all.
#[test]
fn town01_battle_build_surfaces_the_stage_mesh() {
    use legaia_engine_core::scene::Scene;
    use legaia_engine_core::scene_resources::{
        BuildOptions, FIELD_SHARED_BLOCKS, SceneLoadKind, SceneResources,
    };

    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ or LEGAIA_DISC_BIN missing");
        return;
    };
    let host = SceneHost::open_extracted(&extracted).expect("open SceneHost");
    let stage_entry = host
        .index
        .battle_stage_entry_for_scene("town01")
        .expect("town01 has a stage entry");

    let scene = Scene::load(&host.index, "town01").expect("load town01");
    let shared: Vec<Scene> = FIELD_SHARED_BLOCKS
        .iter()
        .filter_map(|n| Scene::load(&host.index, n).ok())
        .collect();
    let refs: Vec<&Scene> = shared.iter().collect();
    let (res, _) = SceneResources::build_targeted_with_options(
        &scene,
        &refs,
        BuildOptions {
            kind: SceneLoadKind::Battle,
            upload_all_tims: true,
            system_ui: None,
        },
    )
    .expect("build town01 in battle mode");

    let dome = res
        .tmds
        .iter()
        .find(|t| t.entry_idx == stage_entry)
        .unwrap_or_else(|| panic!("battle build has no TMD for stage entry {stage_entry}"));
    assert_eq!(
        dome.tmd.objects.len(),
        2,
        "the Rim Elm backdrop is the 2-object dome"
    );
}

/// One TIM's VRAM footprint: `(fb_x, fb_y, width, height)`.
type FbRect = (u16, u16, u16, u16);

/// Every parseable TIM in a buffer, as `(image page, clut block)` addresses.
fn tim_addresses(bytes: &[u8]) -> Vec<(FbRect, FbRect)> {
    let mut out = Vec::new();
    for hit in legaia_asset::tim_scan::scan_buffer(bytes) {
        let end = hit.offset + hit.byte_len;
        let Some(slice) = bytes.get(hit.offset..end) else {
            continue;
        };
        let Ok(tim) = legaia_tim::parse(slice) else {
            continue;
        };
        let Some(c) = tim.clut.as_ref() else { continue };
        let i = &tim.image;
        out.push(((i.fb_x, i.fb_y, i.fb_w, i.h), (c.fb_x, c.fb_y, c.w, c.h)));
    }
    out
}

/// **The residency collision the battle VRAM build has to resolve.**
///
/// Rim Elm's four backdrop streams (entries 6..=9) do not each own a corner of
/// VRAM - they all declare the *same* two 4bpp pages, `(768, 0)` and
/// `(832, 0)`, under the same two CLUT rows, `473` and `479`. Retail never has
/// to notice: the type-`0x01` chunk walker leaves one stream in
/// `_DAT_8007B864` and only that one's pages are resident. The port's battle
/// build DMAs every TIM in the bundle, so absent a final re-upload of the
/// *selected* entry the shell draws its own geometry through a sibling
/// sub-area's texels and palette.
///
/// Asserting the collision keeps the reason for
/// `upload_battle_stage_tims_into_vram` visible rather than folklore.
#[test]
fn rim_elms_four_stage_streams_all_claim_the_same_vram() {
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ or LEGAIA_DISC_BIN missing");
        return;
    };
    let host = SceneHost::open_extracted(&extracted).expect("open SceneHost");
    let mut per_entry = Vec::new();
    for idx in 6u32..=9 {
        let bytes = host.index.entry_bytes(idx).expect("read stage entry");
        let mut addrs = tim_addresses(&bytes);
        addrs.sort();
        eprintln!("entry {idx}: {addrs:?}");
        per_entry.push(addrs);
    }
    assert!(
        !per_entry[0].is_empty(),
        "the stage streams carry their own TIMs"
    );
    for (i, a) in per_entry.iter().enumerate().skip(1) {
        assert_eq!(
            *a,
            per_entry[0],
            "stage stream {} declares the same VRAM as entry 6",
            6 + i
        );
    }
    let pages: Vec<(u16, u16)> = per_entry[0].iter().map(|(i, _)| (i.0, i.1)).collect();
    assert!(pages.contains(&(768, 0)), "the rock + cloud page");
    assert!(pages.contains(&(832, 0)), "the ground-tile page");
    let cluts: Vec<(u16, u16)> = per_entry[0].iter().map(|(_, c)| (c.0, c.1)).collect();
    assert!(cluts.contains(&(0, 473)), "the cloud/rock CLUT row");
    assert!(cluts.contains(&(0, 479)), "the ground-tile CLUT row");
}

/// The fix, end to end. After a `SceneLoadKind::Battle` build some sibling
/// stream holds the shared addresses; re-uploading the selected stage entry
/// puts that entry's own bytes back at every address it declares.
///
/// The `collided > 0` half is what keeps this non-vacuous: it fails the day
/// the general build stops losing the race, at which point the re-upload is
/// no longer load-bearing and this test is the thing that says so.
#[test]
fn the_selected_stage_entry_owns_its_vram_after_the_reupload() {
    use legaia_engine_core::scene::Scene;
    use legaia_engine_core::scene_resources::{
        BuildOptions, FIELD_SHARED_BLOCKS, SceneLoadKind, SceneResources,
    };

    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ or LEGAIA_DISC_BIN missing");
        return;
    };
    let host = SceneHost::open_extracted(&extracted).expect("open SceneHost");
    let stage_entry = host
        .index
        .battle_stage_entry_for_scene("town01")
        .expect("town01 has a battle stage entry");
    assert_eq!(stage_entry, 7, "the Tetsu spar's stage stream");

    let scene = Scene::load(&host.index, "town01").expect("load town01");
    let shared: Vec<Scene> = FIELD_SHARED_BLOCKS
        .iter()
        .filter_map(|n| Scene::load(&host.index, n).ok())
        .collect();
    let refs: Vec<&Scene> = shared.iter().collect();
    let (res, _) = SceneResources::build_targeted_with_options(
        &scene,
        &refs,
        BuildOptions {
            kind: SceneLoadKind::Battle,
            upload_all_tims: true,
            system_ui: None,
        },
    )
    .expect("battle resource build");

    // The stage entry's own CLUT rows, straight off the disc.
    let raw = host.index.entry_bytes(stage_entry).expect("stage bytes");
    let want: Vec<(u16, u16, Vec<u16>)> = legaia_asset::tim_scan::scan_buffer(&raw)
        .into_iter()
        .filter_map(|hit| {
            let end = hit.offset + hit.byte_len;
            let tim = legaia_tim::parse(raw.get(hit.offset..end)?).ok()?;
            let c = tim.clut?;
            Some((c.fb_x, c.fb_y, c.entries))
        })
        .collect();
    assert!(!want.is_empty(), "the stage entry carries CLUTs");

    let read_row = |v: &legaia_tim::Vram, x: u16, y: u16, n: usize| -> Vec<u16> {
        (0..n)
            .map(|i| v.pixel(x as usize + i, y as usize))
            .collect()
    };

    let collided = want
        .iter()
        .filter(|(x, y, e)| read_row(&res.vram, *x, *y, e.len()) != *e)
        .count();
    assert!(
        collided > 0,
        "a sibling stream should be holding the shared CLUT rows before the re-upload"
    );

    let mut vram = res.vram.clone();
    let n = legaia_engine_core::scene::upload_battle_stage_tims_into_vram(
        &scene,
        stage_entry,
        &mut vram,
    );
    assert_eq!(n, want.len(), "every stage TIM re-uploaded");
    for (x, y, entries) in &want {
        assert_eq!(
            read_row(&vram, *x, *y, entries.len()),
            *entries,
            "CLUT row ({x}, {y}) is the selected stage entry's own"
        );
    }
}

/// **A backdrop shell is not all texture.** Its `F*`/`G*` flat / gouraud
/// panels - the sky band, the painted wall faces, the flat water - carry a
/// baked colour word and no UVs, and `tmd_to_vram_mesh` drops every prim with
/// no UVs because such a prim would sample nothing from VRAM. A host that
/// renders only the VRAM half therefore leaves holes in the arena.
///
/// Retail draws one primitive list: `FUN_8001ADA4` case 3 walks the whole
/// group chain and the GPU takes `POLY_F*` / `POLY_G*` packets as readily as
/// `POLY_*T*` ones. This pins how much of the shell the colour half is - a
/// double-digit share on the shells the engine actually renders - and that
/// `ColorMesh::append_scaled` carries it through the second copy the same way
/// the textured builder does.
#[test]
fn the_backdrop_shells_untextured_half_is_a_double_digit_share() {
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ or LEGAIA_DISC_BIN missing");
        return;
    };
    let host = SceneHost::open_extracted(&extracted).expect("open SceneHost");
    // town01's Tetsu arena and map01's overworld dome - the two stage shapes.
    for (idx, min_share) in [(7u32, 0.10f32), (88, 0.02)] {
        let bytes = host.index.entry_bytes(idx).expect("read stage entry");
        let s = legaia_asset::scene_tmd_stream::detect(&bytes).expect("scene_tmd_stream");
        let raw = &bytes[s.tmd_range()];
        let tmd = legaia_tmd::parse(raw).expect("parse dome TMD");
        let drawn = legaia_asset::battle_backdrop::drawn_objects_tmd(&tmd);
        let textured = legaia_tmd::mesh::tmd_to_vram_mesh(&drawn, raw);
        let mut colour = legaia_tmd::mesh::tmd_to_color_mesh(&drawn, raw);
        let (t_tris, c_tris) = (textured.indices.len() / 3, colour.indices.len() / 3);
        let share = c_tris as f32 / (t_tris + c_tris) as f32;
        eprintln!("stage {idx}: {t_tris} textured tris, {c_tris} colour tris ({share:.3})");
        assert!(
            share >= min_share,
            "stage {idx}: the untextured half is {share:.3} of the shell, \
             below the {min_share} the renderer must not silently drop"
        );
        // The second copy has to carry the colour half too, or the shell
        // closes on one texture class and not the other.
        let first = colour.clone();
        colour.append_scaled(&first, [-1.0, 1.0, 1.0]);
        assert_eq!(colour.indices.len() / 3, c_tris * 2, "second copy appended");
        assert_eq!(colour.positions.len(), first.positions.len() * 2);
        // A negative-determinant scale reverses winding, exactly like the
        // textured builder's own `append_scaled`.
        assert_eq!(
            colour.indices[c_tris * 3..c_tris * 3 + 3],
            [
                first.positions.len() as u32,
                first.positions.len() as u32 + 2,
                first.positions.len() as u32 + 1
            ]
        );

        // **Cross-host guard.** The two hosts assemble the same shell two
        // different ways: the native window builds a textured VRAM mesh plus
        // a separate `ColorMesh` on the untextured pipeline, the browser page
        // builds ONE hybrid mesh carrying a per-vertex textured flag. Those
        // are different code paths over the same primitive list, so nothing
        // but an assertion keeps them drawing the same triangles - and the
        // shipped host-drift gate only asks whether a host CALLS a shared
        // builder, never whether both build the same model. This is the
        // shape that let the native window silently lose the shell's
        // untextured half while the browser kept it.
        let (hybrid, _oids, shading) = legaia_tmd::mesh::tmd_to_vram_mesh_field_hybrid(&drawn, raw);
        assert_eq!(
            hybrid.indices.len() / 3,
            t_tris + c_tris,
            "stage {idx}: the browser's hybrid shell and the native window's \
             textured + colour halves must be the same triangle set"
        );
        assert_eq!(
            hybrid.positions.len(),
            textured.positions.len() + first.positions.len()
        );
        let untextured_verts = shading.textured.iter().filter(|&&t| t == 0).count();
        assert_eq!(
            untextured_verts,
            first.positions.len(),
            "stage {idx}: the hybrid's untextured verts are exactly the colour half"
        );
    }
}

/// Which objects of a stage TMD the backdrop actually draws.
///
/// Retail's registration drops object index 1 and keeps the rest
/// (`FUN_800513f0`, ported as `legaia_asset::battle_backdrop::drawn_object_indices`), so
/// the two stage shapes on the disc resolve differently: the two-object shells
/// keep object 0 alone, and the four-object overworld domes keep 0, 2 and 3.
/// Drawing object 0 alone - the port's old behaviour - is right for the shells
/// and loses the mountains and the ground ring on the domes.
#[test]
fn backdrop_object_selection_matches_the_two_stage_shapes() {
    use legaia_asset::battle_backdrop::drawn_object_indices;

    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ or LEGAIA_DISC_BIN missing");
        return;
    };
    let host = SceneHost::open_extracted(&extracted).expect("open SceneHost");

    // Rim Elm: 2 objects -> object 0 only. Unchanged from the old truncation,
    // which is why the Tetsu ground truth cannot regress.
    let (rim_objs, _) = dome_shape(&host, 7);
    assert_eq!(rim_objs, 2);
    assert_eq!(drawn_object_indices(rim_objs), vec![0]);

    // map01 overworld dome: 4 objects -> sky, mountains, ground ring.
    let (map_objs, _) = dome_shape(&host, 88);
    assert_eq!(map_objs, 4);
    assert_eq!(drawn_object_indices(map_objs), vec![0, 2, 3]);

    // Object 3 of the overworld dome is the flat ground ring at Y = 0 - the
    // piece the truncation was dropping, and the reason an overworld battle
    // had sky but no floor behind the procedural grid.
    let bytes = host.index.entry_bytes(88).expect("read stage entry");
    let s = legaia_asset::scene_tmd_stream::detect(&bytes).expect("scene_tmd_stream");
    let tmd = legaia_tmd::parse(&bytes[s.tmd_range()]).expect("parse dome TMD");
    let ys: Vec<i16> = tmd.objects[3].vertices.iter().map(|v| v.y).collect();
    assert!(
        ys.iter().all(|y| *y == 0),
        "map01 object 3 is the flat Y=0 ground ring"
    );
    // ...and object 2 is the mountain band, which reaches well above it.
    let min_y2 = tmd.objects[2].vertices.iter().map(|v| v.y).min().unwrap();
    assert!(
        min_y2 < -2000,
        "map01 object 2 is the mountain ring, got min y {min_y2}"
    );
}

/// The four-object domes are a small, closed set: only the three overworld
/// maps have them, and every other stage stream on the disc is a two-object
/// shell (or smaller). This is what makes "drop index 1" a one-line rule
/// rather than a per-scene table.
#[test]
fn four_object_stages_are_only_the_overworld_domes() {
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ or LEGAIA_DISC_BIN missing");
        return;
    };
    let host = SceneHost::open_extracted(&extracted).expect("open SceneHost");

    let mut multi = Vec::new();
    let mut counts = std::collections::BTreeMap::<usize, usize>::new();
    for idx in 0..host.index.entry_count() as u32 {
        let Ok(bytes) = host.index.entry_bytes(idx) else {
            continue;
        };
        let Some(s) = legaia_asset::scene_tmd_stream::detect(&bytes) else {
            continue;
        };
        let Ok(tmd) = legaia_tmd::parse(&bytes[s.tmd_range()]) else {
            continue;
        };
        *counts.entry(tmd.objects.len()).or_default() += 1;
        if tmd.objects.len() > 2 {
            multi.push(idx);
        }
    }
    eprintln!("stage stream object-count histogram: {counts:?}");
    eprintln!("stage streams with >2 objects: {multi:?}");
    assert_eq!(
        multi,
        vec![88, 89, 90, 247, 248, 249, 394],
        "only the map01/map02/map03 overworld domes carry more than two objects"
    );
}
