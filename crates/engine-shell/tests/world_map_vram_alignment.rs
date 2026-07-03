//! Disc-gated: the world-map oracle VRAM build loads the terrain atlas.
//!
//! For a world-map scene (`map\d\d`) the standalone oracle build must use
//! [`SceneLoadKind::WorldMap`] so the kingdom bundle's slot-0 terrain atlas (a
//! tim-pack the generic TIM scanner can't see through the descriptor table)
//! lands in VRAM. This pins the world-map alignment (`oracle_load_kind`) and
//! that the terrain-atlas pages are actually resident.
//!
//! Historical note: this guard originally compared against a Field-kind build
//! of the same scene, which missed those pages. Since `Scene::load` fetches
//! `SceneAssetTable` entries at their extended footprint, the atlas pages are
//! reachable through the bundle's own LZS streams in either kind, so the two
//! builds converge - the pin is now the world-map build's ABSOLUTE texpage
//! residency (retail-observed ~104k non-zero words; the pre-fix field build
//! sat at ~51k without the atlas).
//!
//! Skip-passes without disc data / extracted assets (CLAUDE.md convention).

use std::path::PathBuf;

use legaia_engine_core::scene_resources::SceneLoadKind;
use legaia_engine_shell::vram_oracle::{
    TEXPAGE_Y_START, build_engine_vram_bytes_prepass_with_kind, oracle_load_kind,
};

const VRAM_WIDTH: usize = 1024;
const VRAM_HEIGHT: usize = 512;
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

/// Count non-zero BGR555 words in the texpage region (`y >= TEXPAGE_Y_START`).
fn texpage_nonzero_words(bytes: &[u8]) -> usize {
    let mut n = 0;
    for y in TEXPAGE_Y_START..VRAM_HEIGHT {
        for x in 0..VRAM_WIDTH {
            let off = (y * VRAM_WIDTH + x) * 2;
            if u16::from_le_bytes([bytes[off], bytes[off + 1]]) != 0 {
                n += 1;
            }
        }
    }
    n
}

#[test]
fn world_map_kind_lifts_terrain_residency() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        return;
    };

    // The oracle auto-selects the world-map kind for `map\d\d`.
    assert_eq!(oracle_load_kind(SCENE), SceneLoadKind::WorldMap);

    let world =
        build_engine_vram_bytes_prepass_with_kind(SCENE, &extracted, None, SceneLoadKind::WorldMap)
            .expect("world-map-kind build");

    let world_n = texpage_nonzero_words(&world);
    eprintln!("[align] {SCENE} texpage non-zero words: world_map={world_n}");

    // The terrain atlas roughly doubles texpage residency over an atlas-less
    // build (~51k -> ~104k non-zero words). Require a generous absolute floor
    // so the guard is robust to incidental atlas-packing changes but still
    // fails if the terrain-atlas decode regresses.
    assert!(
        world_n > 80_000,
        "world-map build should hold the terrain-atlas texpage run in VRAM \
         (world_map={world_n})"
    );
}
