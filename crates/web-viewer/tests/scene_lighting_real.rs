//! Disc-gated pin for enhanced lighting's curated emissive table
//! (`legaia_engine_ui::scene_lighting::EMISSIVE_MESHES`): every entry's
//! content signature must name a model the disc actually ships, and tagging
//! it must find glowing prims - otherwise the Genesis Tree silently stops
//! glowing on both hosts. Also checks the blend rule finds the authored glow
//! a town carries (town01's additive window-light sheets), and that every
//! curated lit-window art (`LIT_WINDOWS`) still matches a town prim.
//!
//! Skips (and passes) without `LEGAIA_DISC_BIN` / `extracted/`.

use legaia_engine_core::scene::ProtIndex;
use legaia_engine_core::scene_assembly::{assemble_field_scene, build_hybrid_env_mesh};
use legaia_engine_ui::scene_lighting as sl;
use std::path::PathBuf;

fn extracted_dir() -> Option<PathBuf> {
    ["extracted", "../../extracted"]
        .into_iter()
        .map(PathBuf::from)
        .find(|d| d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists())
}

#[test]
fn rim_elm_carries_every_curated_emissive_and_authored_glow() {
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing");
        return;
    };
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    }
    let index = ProtIndex::open_extracted(&extracted).expect("open ProtIndex");
    let a = assemble_field_scene(&index, "town01").expect("assemble town01");

    let mut found = vec![false; sl::EMISSIVE_MESHES.len()];
    let mut blend_tagged = 0usize;
    for rtmd in &a.res.tmds {
        let (mut mesh, flat) = build_hybrid_env_mesh(rtmd, &a.res.vram);
        let hit = sl::tag_emissive_hybrid(&rtmd.raw, &mut mesh, &flat, &a.res.vram);
        if let Some(h) = hit {
            let i = sl::EMISSIVE_MESHES
                .iter()
                .position(|e| e.signature == h.entry.signature)
                .unwrap();
            found[i] = true;
            let tagged = mesh
                .cba_tsb
                .iter()
                .filter(|c| c[1] & sl::EMISSIVE_BIT != 0)
                .count();
            assert!(tagged > 0, "{}: curated but nothing tagged", h.entry.label);
        } else {
            blend_tagged += mesh
                .cba_tsb
                .iter()
                .filter(|c| c[1] & sl::EMISSIVE_BIT != 0)
                .count();
        }
    }
    eprintln!("[ran] curated found {found:?}, blend-tagged verts {blend_tagged}");
    for (e, f) in sl::EMISSIVE_MESHES.iter().zip(&found) {
        assert!(f, "curated emissive '{}' not found in town01", e.label);
    }
    assert!(
        blend_tagged > 0,
        "town01's additive glow sheets must tag as emissive"
    );
}

/// Every curated lit-window art (`scene_lighting::LIT_WINDOWS`) must match a
/// prim in the towns it was picked from - a hash or rectangle that drifts
/// from the disc leaves those windows dark at night on both hosts, silently.
#[test]
fn every_lit_window_art_tags_a_town_prim() {
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing");
        return;
    };
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    }
    let index = ProtIndex::open_extracted(&extracted).expect("open ProtIndex");
    let mut found = vec![false; sl::LIT_WINDOWS.len()];
    for scene in ["town01", "balden"] {
        let a = assemble_field_scene(&index, scene).expect("assemble");
        let mut tagged = 0usize;
        for rtmd in &a.res.tmds {
            let (mut mesh, flat) = build_hybrid_env_mesh(rtmd, &a.res.vram);
            sl::tag_emissive_hybrid(&rtmd.raw, &mut mesh, &flat, &a.res.vram);
            for tri in mesh.indices.as_chunks::<3>().0 {
                let [cba, tsb] = mesh.cba_tsb[tri[0] as usize];
                if tsb & sl::WINDOW_BIT == 0 {
                    continue;
                }
                tagged += 1;
                let uvs: Vec<[u8; 2]> = tri.iter().map(|&i| mesh.uvs[i as usize]).collect();
                let art = sl::prim_window_art(&a.res.vram, cba, tsb, &uvs)
                    .expect("a tagged prim samples a curated art");
                let i = sl::LIT_WINDOWS.iter().position(|w| w == art).unwrap();
                found[i] = true;
            }
        }
        eprintln!("[ran] {scene}: {tagged} window triangles");
        assert!(tagged > 0, "{scene}: no lit window tagged");
    }
    for (w, f) in sl::LIT_WINDOWS.iter().zip(&found) {
        assert!(f, "lit window '{}' matched no prim", w.label);
    }
}
