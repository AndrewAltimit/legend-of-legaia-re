//! Disc-gated: the world-map oracle VRAM build loads the terrain atlas.
//!
//! For a world-map scene (`map\d\d`) the standalone oracle build must use
//! [`SceneLoadKind::WorldMap`] so the kingdom bundle's slot-0 terrain atlas (a
//! tim-pack the generic TIM scanner can't see through the descriptor table)
//! lands in VRAM. A field-kind build of the same scene misses those pages, so
//! the texpage region is materially emptier. This pins that the world-map
//! alignment (`oracle_load_kind`) genuinely lifts terrain-page residency rather
//! than silently reporting a phantom gap.
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

    let field =
        build_engine_vram_bytes_prepass_with_kind(SCENE, &extracted, None, SceneLoadKind::Field)
            .expect("field-kind build");
    let world =
        build_engine_vram_bytes_prepass_with_kind(SCENE, &extracted, None, SceneLoadKind::WorldMap)
            .expect("world-map-kind build");

    let field_n = texpage_nonzero_words(&field);
    let world_n = texpage_nonzero_words(&world);
    eprintln!("[align] {SCENE} texpage non-zero words: field={field_n} world_map={world_n}",);

    // The world-map build must add a substantial run of terrain-atlas pages the
    // field build never loads - well beyond noise.
    // Retail observed: field ~51k, world_map ~104k (the terrain atlas roughly
    // doubles residency). Require a generous fraction of that delta so the
    // guard is robust to incidental atlas-packing changes but still fails if the
    // world-map slot-0 decode regresses.
    assert!(
        world_n > field_n + 30_000,
        "world-map kind should add a large terrain-atlas run to VRAM \
         (field={field_n}, world_map={world_n})"
    );
}
