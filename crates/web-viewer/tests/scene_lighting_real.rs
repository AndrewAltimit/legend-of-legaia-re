//! Disc-gated pin for enhanced lighting's curated emissive table
//! (`legaia_engine_ui::scene_lighting::EMISSIVE_MESHES`): every entry's
//! content signature must name a model the disc actually ships, and tagging
//! it must find glowing prims - otherwise the Genesis Tree silently stops
//! glowing on both hosts. Also checks the blend rule finds the authored glow
//! a town carries (town01's additive window-light sheets).
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
