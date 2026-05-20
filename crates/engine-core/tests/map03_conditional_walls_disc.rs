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
//!   (col 66, row 102).
//! - flag `0x378` (TEST@0x4f) -> the contiguous paint cluster at 0x56 / 0x5c
//!   / 0x62 (three sub-0 "clear walls" ops; map03's base grid is already
//!   open there, so they execute but change no tiles).
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

/// `true` if the collision-grid tile at `(col, row)` has any wall sub-cell set.
fn tile_is_wall(grid: &[u8], col: usize, row: usize) -> bool {
    grid.get(row * STRIDE + col).is_some_and(|b| b & 0xF0 != 0)
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
    // tile it would block stays walkable.
    {
        let mut host = SceneHost::open_extracted(&extracted).expect("open SceneHost");
        host.enter_field_scene("map03", 0).expect("enter map03");
        run_prescript(&mut host, 600);
        assert!(
            !tile_is_wall(&host.world.field_collision_grid, 66, 102),
            "with no story flags, the 0x6C2-gated wall at (66,102) must stay clear"
        );
    }

    // Seed flag 0x6C2 -> the entry script's TEST@0x2c routes into paint@0x33
    // (sub-1 block-all) over tile (col 66, row 102).
    {
        let mut host = SceneHost::open_extracted(&extracted).expect("open SceneHost");
        host.enter_field_scene("map03", 0).expect("enter map03");
        host.world.system_flag_set(0x6C2);
        run_prescript(&mut host, 600);
        assert!(
            tile_is_wall(&host.world.field_collision_grid, 66, 102),
            "seeding story flag 0x6C2 must make the nibble-7 wall at (66,102) fire"
        );
        // The paint targets exactly that tile (block-all sets the high nibble).
        assert_eq!(
            host.world.field_collision_grid[102 * STRIDE + 66] & 0xF0,
            0xF0,
            "block-all paint sets all four wall sub-cells"
        );
    }
}
