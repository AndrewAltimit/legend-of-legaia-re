//! Disc-gated: the world-map ocean CLUT animation is wired to terrain the
//! engine actually draws.
//!
//! The live world-map ocean shimmer overwrites the 16 CLUT entries at VRAM
//! `(0, 506)` (CBA word `0x7E80`) each animation step. For that to be visible,
//! the continent heightfield must contain water cells that sample that CLUT.
//! This pins both halves: the kingdom bundle ships the 13-frame animation
//! table, and the heightfield references the ocean CLUT.
//!
//! Skip-passes without extracted assets (CLAUDE.md convention).

use std::path::PathBuf;

use legaia_engine_core::scene::{SceneHost, is_world_map_scene};

/// CBA word for the ocean CLUT at VRAM `(0, 506)`: `(506 << 6) | (0 >> 4)`.
const OCEAN_CLUT_CBA: u16 = 0x7E80;
const SCENE: &str = "map01";

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
fn world_map_ocean_clut_is_referenced_by_terrain_or_skip() {
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        return;
    };
    let mut host = SceneHost::open_extracted(&extracted).expect("open extracted");
    host.load_scene(SCENE).expect("load map01");
    let scene = host.scene.as_ref().expect("scene loaded");
    assert!(
        is_world_map_scene(&scene.name),
        "{SCENE} is a world-map scene"
    );

    // The kingdom bundle's slot-0 TIM_LIST carries the 13-frame ocean CLUT
    // animation table (the same resolver the live engine uses at load).
    let frames = scene
        .entries
        .iter()
        .find_map(|e| {
            let slot0 = legaia_asset::kingdom_bundle::decode_slot(&e.bytes, 0).ok()?;
            legaia_asset::ocean::find_ocean_assets(&slot0).map(|o| o.animation_frames)
        })
        .expect("ocean animation table present in the map01 kingdom bundle");
    assert_eq!(frames.len(), 13 * 32, "13 frames x 32 bytes");

    // The continent heightfield must contain cells that sample the ocean CLUT,
    // or the per-frame CLUT swap would animate nothing.
    let hf = scene
        .walk_heightfield(&host.index)
        .expect("heightfield build")
        .expect("heightfield present for the world map");
    let ocean_verts = hf
        .cba_tsb
        .iter()
        .filter(|[clut, _tpage]| *clut == OCEAN_CLUT_CBA)
        .count();
    eprintln!(
        "[ocean] {SCENE}: {ocean_verts}/{} heightfield verts sample the ocean CLUT (0x7E80)",
        hf.cba_tsb.len()
    );
    assert!(
        ocean_verts > 0,
        "no heightfield cell references the ocean CLUT (0, 506) - the animation would be invisible"
    );
}
