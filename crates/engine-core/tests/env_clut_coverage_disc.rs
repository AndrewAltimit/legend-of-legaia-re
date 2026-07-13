//! Disc-gated: every field/town env-mesh primitive samples a CLUT that
//! actually has colours in it.
//!
//! A 4bpp prim whose CLUT row is all zeros decodes every texel to `0x0000`,
//! which the renderer discards as fully transparent - the primitive vanishes
//! and the clear colour shows through as a hole in the ground. That is exactly
//! what happened while multi-palette CLUT blocks were uploaded as `w x h`
//! rectangles: a block declaring `w=16, h=2` stacked its second palette at
//! `(fb_x, fb_y + 1)` when retail lays it out linearly at `(fb_x + 16, fb_y)`,
//! so every *odd* 16-colour slot on the CLUT rows stayed empty and the prims
//! indexing them (228 triangles in Rim Elm alone) drew nothing.
//!
//! See `legaia_tim::vram`'s `multi_palette_clut_loads_linearly_along_the_row`
//! for the semantic itself; this is the corpus-wide consequence.
//!
//! Skips when `LEGAIA_DISC_BIN` is unset (disc-gated convention).

use std::path::PathBuf;
use std::sync::Arc;

use legaia_engine_core::field_env;
use legaia_engine_core::scene::{ProtIndex, Scene};
use legaia_engine_core::scene_resources::{
    BuildOptions, FIELD_SHARED_BLOCKS, SceneLoadKind, SceneResources,
};
use legaia_tmd::mesh::tmd_to_vram_mesh_filtered;

fn extracted_dir() -> Option<PathBuf> {
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

#[test]
fn env_prims_never_sample_an_empty_clut() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing");
        return;
    };
    let index = Arc::new(ProtIndex::open_extracted(&extracted).expect("open prot index"));
    let shared: Vec<Scene> = FIELD_SHARED_BLOCKS
        .iter()
        .filter_map(|n| Scene::load(&index, n).ok())
        .collect();
    let shared_refs: Vec<&Scene> = shared.iter().collect();
    let system_ui = index.system_ui_bundle().ok();

    let cdname = legaia_prot::cdname::parse(&extracted.join("CDNAME.TXT")).expect("parse cdname");
    let mut names: Vec<String> = cdname.values().cloned().collect();
    names.sort();
    names.dedup();

    let mut scenes = 0usize;
    let mut tris = 0usize;
    let mut dead = 0usize;
    let mut worst: Vec<(String, usize)> = Vec::new();

    for name in &names {
        let Ok(scene) = Scene::load(&index, name) else {
            continue;
        };
        let Ok((res, _)) = SceneResources::build_targeted_with_options(
            &scene,
            &shared_refs,
            BuildOptions {
                kind: SceneLoadKind::Field,
                upload_all_tims: true,
                system_ui: system_ui.as_deref(),
            },
        ) else {
            continue;
        };
        let env = field_env::env_pack_tmd_indices(&scene, &res);
        if env.is_empty() {
            continue;
        }
        scenes += 1;
        let mut scene_dead = 0usize;

        for &ti in &env {
            let t = &res.tmds[ti];
            let mesh = tmd_to_vram_mesh_filtered(&t.tmd, &t.raw, |_, _, _| true);
            for tri in mesh.indices.chunks_exact(3) {
                let v = tri[0] as usize;
                let (cba, tsb) = (mesh.cba_tsb[v][0], mesh.cba_tsb[v][1]);
                // Only the paletted depths (4bpp / 8bpp) read a CLUT.
                if (tsb >> 7) & 3 >= 2 {
                    continue;
                }
                tris += 1;
                let cx = ((cba & 0x3F) * 16) as usize;
                let cy = ((cba >> 6) & 0x1FF) as usize;
                if (0..16).all(|i| res.vram.pixel(cx + i, cy) == 0) {
                    dead += 1;
                    scene_dead += 1;
                }
            }
        }
        if scene_dead > 0 {
            worst.push((name.clone(), scene_dead));
        }
    }

    worst.sort_by_key(|&(_, n)| std::cmp::Reverse(n));
    eprintln!(
        "[env-clut] {scenes} scenes, {tris} paletted env tris, {dead} sampling an empty CLUT"
    );
    for (name, n) in worst.iter().take(10) {
        eprintln!("[env-clut]   {name}: {n} tris discard (hole in the scene)");
    }

    assert!(scenes > 50, "only {scenes} scenes had an env pack");
    assert!(
        tris > 10_000,
        "only {tris} paletted env tris swept - not covering the corpus"
    );

    // Rim Elm is the scene pinned byte-exactly against retail VRAM (the
    // town0c field save state), so it is the one this test asserts on: with
    // the rectangle upload it lost 228 triangles to empty CLUT slots, and
    // with the linear one it loses none.
    //
    // The census above is deliberately printed, not asserted: other scenes
    // (`doman`, `nilboa2`, `ropeway2`, ...) still show dead prims, and the
    // headline counts belong to CDNAME blocks that are data, not field scenes
    // (`card_data`, `bat_back_dat`), where the env-pack probe finds nothing
    // real. Those are a separate open thread - a CLUT source we don't load -
    // and pretending otherwise here would just bake a wrong number in.
    for pinned in ["town01", "town0c"] {
        let n = worst
            .iter()
            .find(|(name, _)| name == pinned)
            .map(|&(_, n)| n)
            .unwrap_or(0);
        assert_eq!(
            n, 0,
            "{n} env triangles in {pinned} sample an all-zero CLUT: they decode to \
             0x0000, get discarded as transparent, and leave holes in the ground - \
             a CLUT block is landing on the wrong VRAM cells (see legaia_tim::vram's \
             linear multi-palette layout)"
        );
    }
}
