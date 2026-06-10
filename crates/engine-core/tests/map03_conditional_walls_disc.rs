//! Disc-gated verification that map03's story-conditional collision-grid
//! wall paints fire when their gating system flags are seeded.
//!
//! map03 runs its MAN scene-entry system script on field entry (see
//! `SceneHost::enter_field_scene` / `Scene::field_man_entry_script`). That
//! script's `0x4C` outer-nibble-7 collision paints are gated behind
//! system-flag `TEST` opcodes, so they stay dormant at a fresh boot (all
//! flags clear) and only fire once the matching story flags are set - which
//! in real gameplay come from the save's story-flag block. The gates, found
//! by tracing the entry script:
//!
//! - flag `0x6C2` (TEST@0x2c) -> paint@0x33: sub-1 "block all" on tile
//!   (col 66, row 102). map03's real base grid (the `define-2` `.MAP`,
//!   PROT 0389) already walls two of that tile's four sub-cells (nibble
//!   `0xC`); the gated paint upgrades it to all-quads `0xF`.
//! - flag `0x378` (TEST@0x4f) -> the contiguous paint cluster at 0x56 / 0x5c
//!   / 0x62 (three sub-0 "clear walls" ops).
//!
//! Reaching the paints also exercises the two nibble-7 fixes this test
//! guards: the row range is `[row0+1, row1+2)` (not `[row0, row1+1)`), and
//! sub-0/1 paints are 6-byte ops (no mask byte) while sub-2/3 are 7-byte.
//!
//! Skips silently when `extracted/` or `LEGAIA_DISC_BIN` is missing.

use std::path::PathBuf;

use legaia_engine_core::scene::SceneHost;

fn extracted_dir() -> Option<PathBuf> {
    for p in ["extracted", "../../extracted"] {
        let d = PathBuf::from(p);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

const STRIDE: usize = 0x80;

/// The wall nibble of the collision-grid tile at `(col, row)`.
fn wall_nibble(grid: &[u8], col: usize, row: usize) -> u8 {
    grid.get(row * STRIDE + col).map_or(0, |b| b >> 4)
}

fn run_prescript(host: &mut SceneHost, frames: usize) {
    for _ in 0..frames {
        host.world.set_pad(0);
        let _ = host.world.tick();
    }
}

#[test]
fn map03_conditional_wall_fires_when_story_flag_seeded() {
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing");
        return;
    };
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    }

    // Baseline: no flags seeded -> the 0x6C2-gated paint never runs, so the
    // tile keeps its base wall nibble (0xC in the real define-2 .MAP - two
    // of four sub-cells walled, not the paint's all-quads 0xF).
    {
        let mut host = SceneHost::open_extracted(&extracted).expect("open SceneHost");
        host.enter_field_scene("map03", 0).expect("enter map03");
        run_prescript(&mut host, 600);
        assert_eq!(
            wall_nibble(&host.world.field_collision_grid, 66, 102),
            0xC,
            "with no story flags, tile (66,102) keeps the base grid's 0xC nibble"
        );
    }

    // Seed flag 0x6C2 -> the entry script's TEST@0x2c routes into paint@0x33
    // (sub-1 block-all) over tile (col 66, row 102).
    {
        let mut host = SceneHost::open_extracted(&extracted).expect("open SceneHost");
        host.enter_field_scene("map03", 0).expect("enter map03");
        host.world.system_flag_set(0x6C2);
        run_prescript(&mut host, 600);
        // The paint targets exactly that tile (block-all sets the high nibble).
        assert_eq!(
            wall_nibble(&host.world.field_collision_grid, 66, 102),
            0xF,
            "seeding story flag 0x6C2 must fire the block-all paint at (66,102)"
        );
    }
}
