//! Disc-gated: the field ground's **far-bucket** cells.
//!
//! Retail's field ground pass (`FUN_801F6D48`, PROT 0900) links a cell whose
//! object-grid word lacks bit `0x8000` into the ordering table's fixed far
//! bucket, so every other primitive paints over it. `town01`'s cell
//! `(30, 38)` - by the plateau's overhang, next to the tree at `(30, 37)` - is
//! such a cell, and three of its corners sit on the town floor while the
//! fourth is on the plateau 384 units up: a near-vertical sheet of ground
//! texture standing in front of the recessed cliff face. Retail's cliff mesh
//! paints over it; a depth buffer drew it in front, the long dark-green
//! sliver by the cave mouth on every host. The shared kernel
//! `field_ground::flat_refs` marks it so the hosts' ground shaders push it
//! behind every other draw, as the far bucket does.
//!
//! Skips (and passes) when `LEGAIA_DISC_BIN` is unset.

use legaia_engine_core::field_ground;
use legaia_engine_core::scene::{ProtIndex, Scene};
use std::path::Path;

fn open_index() -> Option<ProtIndex> {
    std::env::var_os("LEGAIA_DISC_BIN")?;
    for root in ["extracted", "../../extracted"] {
        let p = Path::new(root);
        if p.join("PROT.DAT").exists() && p.join("CDNAME.TXT").exists() {
            return ProtIndex::open_extracted(p).ok();
        }
    }
    None
}

/// The heightfield vertex range of the quad whose low corner is tile
/// `(col, row)`.
fn cell_verts(positions: &[[f32; 3]], col: i32, row: i32) -> Option<usize> {
    positions
        .chunks(4)
        .position(|c| c[0][0] == (col * 128) as f32 && c[0][2] == (row * 128) as f32)
        .map(|q| q * 4)
}

#[test]
fn town01_cave_overhang_sliver_is_a_far_bucket_cell_drawn_under_everything() {
    let Some(index) = open_index() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    };
    let scene = Scene::load(&index, "town01").expect("load town01");
    let hf = scene
        .walk_heightfield(&index)
        .expect("heightfield")
        .expect("town01 has a ground grid");
    assert_eq!(hf.far_bucket.len(), hf.positions.len());

    let v = cell_verts(&hf.positions, 30, 38).expect("cell (30, 38) is a ground cell");
    let ys: Vec<f32> = hf.positions[v..v + 4].iter().map(|p| p[1]).collect();
    let rise =
        ys.iter().cloned().fold(f32::MIN, f32::max) - ys.iter().cloned().fold(f32::MAX, f32::min);
    assert_eq!(
        rise, 384.0,
        "three corners on the floor, one on the plateau: {ys:?}"
    );
    assert!(
        hf.far_bucket[v..v + 4].iter().all(|&f| f),
        "the cell's object-grid word lacks the 0x8000 sort bit"
    );

    let positions = field_ground::render_positions(&hf);
    let refs = field_ground::flat_refs(&hf, &positions);
    assert!(
        refs[v..v + 4].iter().all(field_ground::is_far_bucket_ref),
        "the sloped far-bucket cell carries the draw-under-everything marker"
    );

    // Flat far-bucket cells keep their real depth, and no depth-sorted cell
    // is ever marked.
    let mut flat_far = 0usize;
    for (q, cell) in positions.chunks(4).enumerate() {
        let i = q * 4;
        let marked = field_ground::is_far_bucket_ref(&refs[i]);
        let flat = cell.iter().all(|p| p[1] == cell[0][1]);
        if !hf.far_bucket[i] {
            assert!(!marked, "depth-sorted cell {q} marked");
        } else if flat {
            flat_far += 1;
            assert!(!marked, "flat far-bucket cell {q} marked");
        } else {
            assert!(marked, "sloped far-bucket cell {q} unmarked");
        }
    }
    assert!(flat_far > 0);
}

/// The overworld ground keys every cell on its own corners and has no far
/// bucket: nothing on a kingdom map is marked.
#[test]
fn overworld_ground_carries_no_far_bucket_marker() {
    let Some(index) = open_index() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    };
    let scene = Scene::load(&index, "map01").expect("load map01");
    let hf = scene
        .walk_heightfield(&index)
        .expect("heightfield")
        .expect("map01 has a ground grid");
    assert!(hf.far_bucket.iter().all(|&f| !f));
    let positions = field_ground::render_positions(&hf);
    assert!(
        field_ground::flat_refs(&hf, &positions)
            .iter()
            .all(|r| !field_ground::is_far_bucket_ref(r))
    );
}
