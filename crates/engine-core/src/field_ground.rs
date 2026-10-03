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
//!   TMDs. Nothing notices under the both-sided passes, but the field
//!   pass's NCLIP cull (`camera_view::nclip_cull_mode` = 2) discards one
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

/// [`render_positions`] under the **live** floor-height ladder: each vertex's
/// Y re-resolved from its corner tier
/// ([`WalkHeightfield::corner_tiers`]) through `world_lut`, the runtime
/// scratchpad ladder (`World::terrain.floor_height_lut`, `0x1F80035C`).
///
/// Retail's field ground emitter (`FUN_801F6D48`, PROT 0900) takes each
/// cell's four corner tiers through that ladder on every frame it draws, so
/// when the field VM animates the ladder (op `0x4C` nibble 9: `jou`'s
/// organic floor, `jouina`'s pulsing path, `concnow`'s flesh pits) the
/// ground surface deforms per vertex - and the floor sampler `FUN_80019278`
/// reads the same ladder, so what the player stands on is what is drawn.
/// The scratchpad ladder is the negation of the MAN-header one the
/// heightfield was built from, which is the frame the built positions are
/// already in, so the rung value is the Y directly.
///
/// A heightfield without tier data (an older builder) falls back to its
/// baked positions.
///
/// REF: FUN_801F6D48 (the per-frame ladder read of the ground pass)
pub fn live_render_positions(hf: &WalkHeightfield, world_lut: &[i16; 16]) -> Vec<[f32; 3]> {
    if hf.corner_tiers.len() != hf.positions.len() {
        return render_positions(hf);
    }
    hf.positions
        .iter()
        .zip(&hf.corner_tiers)
        .map(|(p, &tier)| {
            let y = f32::from(world_lut[usize::from(tier & 0x0F)]);
            [p[0], y + crate::coplanar_draws::GROUND_SINK, p[2]]
        })
        .collect()
}

/// The tile `(col, row)` of each heightfield vertex's cell: read off each
/// quad's lowest vertex (the builder places it at `(col * 128, _, row *
/// 128)` and gives every cell its own four corners), `None` for a vertex no
/// quad references.
pub fn vertex_cells(hf: &WalkHeightfield) -> Vec<Option<(i32, i32)>> {
    let mut out = vec![None; hf.positions.len()];
    for quad in hf.indices.chunks(6) {
        let Some(&base) = quad.iter().min() else {
            continue;
        };
        let Some(p) = hf.positions.get(base as usize) else {
            continue;
        };
        let cell = ((p[0] / 128.0).floor() as i32, (p[2] / 128.0).floor() as i32);
        for &i in quad {
            if let Some(c) = out.get_mut(i as usize) {
                *c = Some(cell);
            }
        }
    }
    out
}

/// [`live_render_positions`] with a ladder **per cell**: vertex `i` resolves
/// through `lut_of(cells[i])` ([`vertex_cells`]). A whole-map view draws
/// every room at once, and a scene whose rooms each install their own ladder
/// (`concnow`'s system script re-installs it per region) shows each room on
/// the ladder it would hold with the player standing in it. A vertex with no
/// cell takes `lut_of(None)`.
pub fn live_render_positions_by_cell<'a>(
    hf: &WalkHeightfield,
    cells: &[Option<(i32, i32)>],
    lut_of: impl Fn(Option<(i32, i32)>) -> &'a [i16; 16],
) -> Vec<[f32; 3]> {
    if hf.corner_tiers.len() != hf.positions.len() {
        return render_positions(hf);
    }
    hf.positions
        .iter()
        .zip(&hf.corner_tiers)
        .enumerate()
        .map(|(i, (p, &tier))| {
            let lut = lut_of(cells.get(i).copied().flatten());
            let y = f32::from(lut[usize::from(tier & 0x0F)]);
            [p[0], y + crate::coplanar_draws::GROUND_SINK, p[2]]
        })
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

/// Crop an already-built ground index list to this frame's visible cells: keep
/// each quad (six indices, as [`render_indices`] or the builder emits them)
/// whose cell the ground emitters visit
/// ([`crate::field_view_window::ViewCells::ground_visible`]). The cell is read
/// back off the quad's lowest vertex, which the builder places at
/// `(col * 128, _, row * 128)` - X and Z survive the sink untouched, so the
/// same call serves [`render_positions`]. `cells = None` returns the list
/// whole.
///
/// Both hosts upload the result as the ground's index buffer whenever
/// [`crate::field_view_window::ViewCells::stamp`] moves.
pub fn crop_indices(
    positions: &[[f32; 3]],
    indices: &[u32],
    cells: Option<&crate::field_view_window::ViewCells>,
) -> Vec<u32> {
    let Some(cells) = cells else {
        return indices.to_vec();
    };
    let mut out = Vec::with_capacity(indices.len());
    for quad in indices.chunks(6) {
        let Some(&base) = quad.iter().min() else {
            continue;
        };
        let Some(p) = positions.get(base as usize) else {
            continue;
        };
        let col = (p[0] / 128.0).floor() as i32;
        let row = (p[2] / 128.0).floor() as i32;
        if cells.ground_visible(col, row) {
            out.extend_from_slice(quad);
        }
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
            corner_tiers: vec![0, 0, 1, 1],
        }
    }

    #[test]
    fn live_positions_follow_the_ladder_per_vertex() {
        let hf = grid();
        // The MAN ladder the grid was built from (tier 1 = 32 up), as the
        // scratchpad holds it: negated.
        let mut lut = [0i16; 16];
        lut[1] = -32;
        assert_eq!(live_render_positions(&hf, &lut), render_positions(&hf));
        // The script swells tier 1 to 80 up: only the tier-1 corners move.
        lut[1] = -80;
        let live = live_render_positions(&hf, &lut);
        let sink = crate::coplanar_draws::GROUND_SINK;
        assert_eq!(live[0][1], sink);
        assert_eq!(live[1][1], sink);
        assert_eq!(live[2][1], -80.0 + sink);
        assert_eq!(live[3][1], -80.0 + sink);
        assert_eq!(live[2][0], 0.0);
        assert_eq!(live[2][2], 128.0);
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

    #[test]
    fn crop_keeps_only_the_quads_the_ground_emitter_visits() {
        // Two cells: (0, 2) and (5, 2).
        let mut positions = Vec::new();
        for col in [0.0f32, 5.0] {
            let (x, z) = (col * 128.0, 256.0);
            positions.extend([
                [x, 0.0, z],
                [x + 128.0, 0.0, z],
                [x, 0.0, z + 128.0],
                [x + 128.0, 0.0, z + 128.0],
            ]);
        }
        let indices = vec![0, 2, 1, 1, 2, 3, 4, 6, 5, 5, 6, 7];
        assert_eq!(crop_indices(&positions, &indices, None), indices);
        let view = crate::world::field_npc_cull::FieldCullView {
            // Focus tile (4, 3) with a (-2, 0, 4, 2) window: first cell
            // (2, 3), so the ground walks columns 2..=7 by rows 2..=3.
            focus_stored: [-(4 * 128), -(3 * 128)],
            attr_box: [0, 0, 127, 127],
            window: [-2, 0, 4, 2],
        };
        let cells = crate::field_view_window::view_cells(&view);
        assert_eq!(cells.first, [2, 3]);
        assert_eq!(
            crop_indices(&positions, &indices, Some(&cells)),
            vec![4, 6, 5, 5, 6, 7]
        );
    }
}
