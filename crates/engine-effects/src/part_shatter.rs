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
//! The row is taken as zero: the battle rows read zero in every capture
//! ([`legaia_render_kernels::shatter::ShatterRow`]), so a battle part's
//! burst centre is its own origin and its flags are the default spin and
//! push.
//!
//! REF: FUN_8001ADA4 (`0x8001B45C..0x8001B568`), FUN_80029888

use legaia_render_kernels::shatter::{ShatterInput, ShatterRow, SweepFrame, shatter_vram_mesh};
use legaia_tmd::mesh::VramMesh;

/// The shattered copy of a part's local mesh for draw word `value` (the
/// part's `+0x7A`), or `None` when the word is zero and the part draws
/// whole. One object: a part's model is drawn unposed, so every triangle
/// shares the object's burst centre.
pub fn shatter_part_mesh(mesh: &VramMesh, value: i16) -> Option<VramMesh> {
    let input = ShatterInput::new(value, ShatterRow::default())?;
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
    if let Some(m) = shatter_part_mesh(mesh, value) {
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
}
