//! The walk-ground heightfield **as a render surface** - the one kernel both
//! hosts feed their ground draw through.
//!
//! [`legaia_asset::field_objects::WalkHeightfield`] is the `.MAP` floor grid
//! as data: authored heights, and triangles in whatever order its builder
//! emits them. Two things change on the way to the screen, and both are
//! render-site decisions rather than facts about the grid:
//!
//! - **Height**: the surface sinks by [`GROUND_SINK`] so the env pack's
//!   authored floor art on the same plane wins the depth test.
//! - **Winding**: the heightfield is engine-synthesised geometry with no
//!   retail winding to preserve, and its builder winds opposite to the scene
//!   TMDs. Nothing notices under the both-sided passes, but the cutscene
//!   camera's NCLIP pass (`camera_view::nclip_cull_mode` = 2) discards one
//!   facing, and a ground wound against the disc meshes is the half it
//!   discards. Every triangle is reversed here so the ground carries the
//!   disc meshes' parity.
//!
//! The winding half used to live in the native window only
//! (`heightfield_to_vram_mesh`), so the browser play page drew the grid as
//! the builder left it: under the opdeene prologue's cutscene camera the page
//! culled the whole floor under the Genesis-tree vignette, which the native
//! window and retail both draw.
//!
//! [`GROUND_SINK`]: crate::coplanar_draws::GROUND_SINK

use legaia_asset::field_objects::WalkHeightfield;

/// The heightfield's vertex positions as drawn: authored positions sunk by
/// [`crate::coplanar_draws::GROUND_SINK`] (retail Y-down frame, so the sink
/// is added).
pub fn render_positions(hf: &WalkHeightfield) -> Vec<[f32; 3]> {
    hf.positions
        .iter()
        .map(|p| [p[0], p[1] + crate::coplanar_draws::GROUND_SINK, p[2]])
        .collect()
}

/// The heightfield's triangle indices as drawn: every triangle reversed
/// (`[a, b, c]` -> `[a, c, b]`) onto the scene TMDs' winding parity. A
/// trailing partial triangle, which a well-formed grid never has, is kept
/// as-is.
pub fn render_indices(hf: &WalkHeightfield) -> Vec<u32> {
    let mut out = hf.indices.clone();
    for tri in out.as_chunks_mut::<3>().0 {
        tri.swap(1, 2);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid() -> WalkHeightfield {
        WalkHeightfield {
            positions: vec![
                [0.0, 0.0, 0.0],
                [128.0, 0.0, 0.0],
                [0.0, -32.0, 128.0],
                [128.0, -32.0, 128.0],
            ],
            tile_ids: vec![0; 4],
            uvs: vec![[0, 0]; 4],
            cba_tsb: vec![[0, 0]; 4],
            colors: vec![legaia_asset::field_objects::GROUND_PRIM_COLOR; 4],
            indices: vec![0, 1, 2, 1, 3, 2],
        }
    }

    #[test]
    fn every_triangle_is_reversed_and_keeps_its_vertices() {
        let hf = grid();
        assert_eq!(render_indices(&hf), vec![0, 2, 1, 1, 2, 3]);
    }

    #[test]
    fn positions_sink_by_the_shared_constant() {
        let hf = grid();
        let pos = render_positions(&hf);
        assert_eq!(pos.len(), hf.positions.len());
        for (a, b) in pos.iter().zip(&hf.positions) {
            assert_eq!(a[0], b[0]);
            assert_eq!(a[1], b[1] + crate::coplanar_draws::GROUND_SINK);
            assert_eq!(a[2], b[2]);
        }
    }
}
