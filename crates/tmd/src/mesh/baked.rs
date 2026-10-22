//! How much of a mesh is baked black on the disc.
//!
//! Retail multiplies every texel by the prim's colour word
//! (`texel * colour / 128`) and draws an untextured prim in that colour, so
//! a prim whose colour word is near black draws black however its texture
//! resolves. A viewer that shows such a mesh dark is showing the data, not a
//! missed texture page; the measurement here lets the desktop tools say so.

use crate::Tmd;

/// A baked colour channel below this reads as black once the texel is
/// scaled by it (`16 / 128` of the texel at most).
pub const DARK_BAKED_CHANNEL: u8 = 16;

/// A [`dark_baked_share`] at or above this marks a mesh that is mostly
/// black on the disc. Black untextured prims (door voids, shadow blobs) are
/// common in scene meshes, so the bar is "most of the surface", not "some
/// of it".
pub const MOSTLY_DARK_SHARE: f32 = 0.5;

/// Share of the surface (`0.0..=1.0`, by triangle area) whose per-vertex
/// baked colour is near black at every corner - every channel below
/// [`DARK_BAKED_CHANNEL`]. `colors` is the colour word each vertex draws
/// with: the modulation of a textured prim, the fill of an untextured one.
/// Triangles with an index out of range are skipped.
pub fn dark_baked_share(positions: &[[f32; 3]], colors: &[[u8; 3]], indices: &[u32]) -> f32 {
    let (mut dark, mut total) = (0.0f64, 0.0f64);
    for tri in indices.as_chunks::<3>().0 {
        let v = tri.map(|i| i as usize);
        let (Some(p0), Some(p1), Some(p2)) = (
            positions.get(v[0]),
            positions.get(v[1]),
            positions.get(v[2]),
        ) else {
            continue;
        };
        let a = [p1[0] - p0[0], p1[1] - p0[1], p1[2] - p0[2]];
        let b = [p2[0] - p0[0], p2[1] - p0[1], p2[2] - p0[2]];
        let n = [
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        ];
        let area = f64::from((n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt());
        total += area;
        let black = v.iter().all(|&i| {
            colors
                .get(i)
                .is_some_and(|c| c.iter().all(|&k| k < DARK_BAKED_CHANNEL))
        });
        if black {
            dark += area;
        }
    }
    if total > 0.0 {
        (dark / total) as f32
    } else {
        0.0
    }
}

/// [`dark_baked_share`] over a whole TMD - textured and untextured prims
/// alike, through the field hybrid walk
/// ([`super::tmd_to_vram_mesh_field_hybrid`]).
pub fn tmd_dark_baked_share(tmd: &Tmd, buf: &[u8]) -> f32 {
    let (mesh, _, shading) = super::tmd_to_vram_mesh_field_hybrid(tmd, buf);
    let colors: Vec<[u8; 3]> = (0..mesh.positions.len())
        .map(|i| {
            if shading.textured.get(i) == Some(&1) {
                mesh.colors.get(i).copied()
            } else {
                shading.colors.get(i).copied()
            }
            .unwrap_or([0x80; 3])
        })
        .collect();
    dark_baked_share(&mesh.positions, &colors, &mesh.indices)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn share_is_weighted_by_area() {
        // A big black triangle and a small lit one.
        let positions = [
            [0.0, 0.0, 0.0],
            [3.0, 0.0, 0.0],
            [0.0, 3.0, 0.0],
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
        ];
        let colors = [[0; 3], [8; 3], [15; 3], [0x80; 3], [0; 3], [0; 3]];
        let share = dark_baked_share(&positions, &colors, &[0, 1, 2, 3, 4, 5]);
        assert!((share - 0.9).abs() < 1e-6, "{share}");
        assert_eq!(dark_baked_share(&positions, &colors, &[]), 0.0);
        // One bright corner keeps a triangle out.
        assert_eq!(dark_baked_share(&positions, &colors, &[3, 4, 5]), 0.0);
    }
}
