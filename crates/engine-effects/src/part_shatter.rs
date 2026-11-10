//! The shatter gate on an effect part's model draw.
//!
//! The per-actor dispatcher `FUN_8001ADA4` draws every model of a part
//! through one per-model loop, whatever built the model: a library mesh
//! (`model_sel >= 0`) or the draw-kind-4 emitters' own scratch TMD (case 4
//! falls into the model draw at `0x8001B160`; a `+0x5A == 2` node takes the
//! `0x8001B1A8` arm into the same loop). In that loop, a part whose floor
//! clip `+0x42` is down and whose `+0x7A` reads non-zero
//! (`lh v0,0x7a(s0)` / `bne v0,zero` at `0x8001B45C..0x8001B464`) leaves the
//! prim dispatcher `FUN_80043390` for the shatter renderer `FUN_80029888`,
//! handed `+0x7A | row[+8] << 16` (`0x8001B538..0x8001B564`) with `row` the
//! deformation-table row its `+0x6D` names
//! ([`legaia_render_kernels::shatter`]). Move-VM op `0x11` sets the per-tick
//! rate of `+0x7A` and the part tick's motion block ramps it, so a record
//! breaks its own model apart over time: Zenoir's summoning mound
//! (PROT 0908 records `0x801F85D4` / `0x801F8638`), Nighto's crystal
//! (PROT 0907 record `0x801F82C0`) and Vera's two arm-5 glows (PROT 0905
//! `0x801F8494` / `0x801F8500`, op `0x11 0x10`), which retail holds alive
//! through arm 8 with `+0x7A` climbing (`0xC0` fourteen vsyncs after their
//! seat, `0x840` at 164): the burst of yellow shards, where a whole disc
//! would grow over the screen. The gate covers the default arm, the sprite
//! arm, the ribbon and library meshes alike; a shattered default-arm node is
//! built without the bank-3 quad split, which only the prim dispatcher
//! applies.
//!
//! The row is the deformation table `0x8007BE60`'s, and a caller that holds
//! the table passes it ([`shatter_part_mesh_row`]): move-VM op `0x21` writes
//! a row and points the part's `+0x6D` at it (`0x8002377C..0x800237D4`), and
//! the field carriers set a burst offset and the shrink flag that way
//! (`rikuroa` / `rikuroa2` records `56` / `57`, `jagaroom` records `20` /
//! `21`). Two battle images write one too: PROT 0953's Terio Punch planet
//! (`0x801F7A3C`) and PROT 0959's twin record (`0x801F8EA0`) point `+0x6D` at
//! row `3` with flags `7` (no spin, no push, shrink), so the planet stays
//! whole while its `+0x7A` climbs and then shrinks away under flags `1` / `4`.
//! The scenes' draw lists take the row through `row_of`
//! (`SummonScene::part_draws_with` and its siblings); a part drawn with
//! [`shatter_part_mesh`] reads the zero row, bursting from its own origin
//! with the default spin and push.
//!
//! REF: FUN_8001ADA4 (`0x8001B45C..0x8001B568`), FUN_80029888

use legaia_render_kernels::shatter::{ShatterInput, ShatterRow, SweepFrame, shatter_vram_mesh};
use legaia_tmd::mesh::VramMesh;

/// The shattered copy of a part's local mesh for draw word `value` (the
/// part's `+0x7A`), or `None` when the word is zero and the part draws
/// whole. One object: a part's model is drawn unposed, so every triangle
/// shares the object's burst centre.
pub fn shatter_part_mesh(mesh: &VramMesh, value: i16) -> Option<VramMesh> {
    shatter_part_mesh_row(mesh, value, ShatterRow::default())
}

/// [`shatter_part_mesh`] under deformation-table row `row`, the row the
/// part's `+0x6D` names (`0x8001B538..0x8001B564` reads it for the draw
/// word's flag half; the renderer adds its offset to every vertex).
pub fn shatter_part_mesh_row(mesh: &VramMesh, value: i16, row: ShatterRow) -> Option<VramMesh> {
    let input = ShatterInput::new(value, row)?;
    let ids = vec![0u32; mesh.positions.len()];
    Some(shatter_vram_mesh(mesh, &ids, &input, &[], false, |_| {
        SweepFrame::MODEL
    }))
}

/// The shatter word a part's model draw takes: its `+0x7A`, or zero while
/// its floor clip `+0x42` is raised - the raised arm (`bne s7,zero` at
/// `0x8001B454`) skips the `+0x7A` test for the clipped draw.
pub fn part_shatter_word(s: &legaia_engine_vm::move_vm::ActorState) -> i16 {
    if s.field_42 != 0 {
        0
    } else {
        s.field_7a as i16
    }
}

/// Replace `mesh` with its shattered copy when `value` is non-zero.
pub fn apply_part_shatter(mesh: &mut VramMesh, value: i16) {
    apply_part_shatter_row(mesh, value, ShatterRow::default());
}

/// [`apply_part_shatter`] under deformation-table row `row`.
pub fn apply_part_shatter_row(mesh: &mut VramMesh, value: i16, row: ShatterRow) {
    if let Some(m) = shatter_part_mesh_row(mesh, value, row) {
        *mesh = m;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn quad() -> VramMesh {
        VramMesh {
            positions: vec![
                [-64.0, 0.0, -64.0],
                [64.0, 0.0, -64.0],
                [-64.0, 0.0, 64.0],
                [64.0, 0.0, 64.0],
            ],
            uvs: vec![[0, 0], [16, 0], [0, 16], [16, 16]],
            cba_tsb: vec![[0x7A00, 0x0D]; 4],
            normals: vec![[0.0; 3]; 4],
            colors: vec![[0x80, 0x80, 0x80]; 4],
            indices: vec![0, 1, 2, 2, 1, 3],
        }
    }

    #[test]
    fn a_zero_word_draws_whole() {
        assert!(shatter_part_mesh(&quad(), 0).is_none());
        let mut m = quad();
        apply_part_shatter(&mut m, 0);
        assert_eq!(m.positions, quad().positions);
        assert_eq!(m.indices, quad().indices);
    }

    /// A non-zero word de-indexes the mesh (each triangle its own three
    /// corners) and pushes every triangle off the burst centre by `v / 128`
    /// of its centroid's distance.
    #[test]
    fn a_raised_word_breaks_the_triangles_apart() {
        let whole = quad();
        let m = shatter_part_mesh(&whole, 0x100).expect("shattering");
        assert_eq!(m.indices, (0..6).collect::<Vec<u32>>());
        assert_eq!(m.positions.len(), 6);
        let centroid = |t: &[[f32; 3]]| -> [f32; 3] {
            std::array::from_fn(|k| (t[0][k] + t[1][k] + t[2][k]) / 3.0)
        };
        let before = centroid(&[whole.positions[0], whole.positions[1], whole.positions[2]]);
        let after = centroid(&m.positions[0..3]);
        let r = |c: [f32; 3]| (c[0] * c[0] + c[2] * c[2]).sqrt();
        assert!(r(after) > r(before) * 2.0, "{before:?} -> {after:?}");
    }

    /// Terio Punch's row: flags `7` (no spin, no push, shrink by
    /// `0x1000 - v`) keeps every triangle about its own centroid, so a small
    /// word draws the mesh whole.
    #[test]
    fn a_no_spin_no_push_row_keeps_the_mesh_whole() {
        let whole = quad();
        let row = ShatterRow {
            flags: 7,
            ..Default::default()
        };
        let m = shatter_part_mesh_row(&whole, 28, row).expect("shattering");
        for (k, &i) in [0usize, 1, 2].iter().enumerate() {
            for c in 0..3 {
                assert!(
                    (m.positions[k][c] - whole.positions[i][c]).abs() < 2.0,
                    "{:?} vs {:?}",
                    m.positions[k],
                    whole.positions[i]
                );
            }
        }
    }
}
