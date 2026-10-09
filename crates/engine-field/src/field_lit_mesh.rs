//! Shading the field's **light-source** TMD rows - the one kernel both play
//! hosts run over an environment mesh before they draw it.
//!
//! Groups with flags `0x10..=0x17` carry no colour word: retail colours each
//! corner through the GTE light (`NCCS` / `NCCT`, see
//! [`legaia_engine_vm::field_light`]) against the draw's world normal. The
//! mesh builders leave those vertices at the neutral `0x80`
//! ([`legaia_tmd::mesh::LitVertex`] records what the shading needs), so a
//! host that draws them unshaded paints every such face at its raw texel.
//! `cave01`'s rock walls are all lit rows: under the scene-load light the
//! faces turned from it fall to an eighth of their texel, the ones facing it
//! rise past neutral.
//!
//! The intensity depends on the draw's rotation (the light matrix is folded
//! with it before `SetLightMatrix`), so a mesh instanced at several yaws
//! needs one shaded colour stream per yaw; both hosts key their variants by
//! [`draw_rotation`].

use legaia_engine_vm::field_light::{self, FieldLight};
use legaia_tmd::mesh::LitVertex;

/// The q3.12 rotation retail draws a placed record with (`FUN_80026988` over
/// the record's `+0x08 / +0x0A / +0x0C` angles) - the key a host's shaded
/// variants are cached under.
pub fn draw_rotation(rot_x: u16, rot_y: u16, rot_z: u16) -> [[i32; 3]; 3] {
    field_light::euler_rot([rot_x as i16, rot_y as i16, rot_z as i16])
}

/// The q3.12 rotation of a host's float model matrix (row-major upper 3x3,
/// any uniform render scale divided out) - for a host whose draw list kept
/// the matrix rather than the record angles. Rounds to retail's grid, so it
/// lands within a unit of [`draw_rotation`].
pub fn rotation_from_matrix(m: [[f32; 3]; 3]) -> [[i32; 3]; 3] {
    let col_len = |c: usize| (m[0][c] * m[0][c] + m[1][c] * m[1][c] + m[2][c] * m[2][c]).sqrt();
    let s: [f32; 3] = std::array::from_fn(|c| col_len(c).max(1e-6));
    std::array::from_fn(|r| std::array::from_fn(|c| (m[r][c] / s[c] * 4096.0).round() as i32))
}

/// Whether a mesh has any lit-row vertex - the hosts' "needs shading" test.
pub fn has_lit_rows(lit: &[Option<LitVertex>]) -> bool {
    lit.iter().any(Option::is_some)
}

/// Overwrite the modulation colour of every lit-row vertex with the colour
/// retail's light-source handler gives it under `light`, drawn at `rot`.
/// Baked-colour vertices (`None`) keep theirs. `colors` and `lit` are
/// index-aligned (the textured half of a mesh; any colour-mesh vertices
/// appended after it are untouched).
pub fn shade_lit_rows(
    colors: &mut [[u8; 3]],
    lit: &[Option<LitVertex>],
    light: &FieldLight,
    rot: &[[i32; 3]; 3],
) {
    let l = field_light::mul_matrix0(&light.light_matrix(), rot);
    for (c, v) in colors.iter_mut().zip(lit) {
        if let Some(v) = v {
            let rgbc = field_light::staged_rgbc(field_light::FIELD_DRAW_COLOUR, v.object_rgb);
            *c = field_light::nccs(
                &l,
                &field_light::LIGHT_COLOUR_MATRIX,
                light.back,
                rgbc,
                v.normal,
            );
        }
    }
}

/// [`shade_lit_rows`] for a hybrid mesh's `[r, g, b, flag]` side stream
/// (`legaia_engine_core::scene_assembly::build_hybrid_env_mesh`): the textured half
/// leads it, one entry per lit-mask vertex, and its RGB is the same packet
/// colour.
pub fn shade_lit_rows_rgba(
    colors: &mut [[u8; 3]],
    flat_rgba: &mut [u8],
    lit: &[Option<LitVertex>],
    light: &FieldLight,
    rot: &[[i32; 3]; 3],
) {
    shade_lit_rows(colors, lit, light, rot);
    for (i, (c, v)) in colors.iter().zip(lit).enumerate() {
        if v.is_some()
            && let Some(px) = flat_rgba.get_mut(i * 4..i * 4 + 3)
        {
            px.copy_from_slice(c);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lit(normal: [i16; 3]) -> Option<LitVertex> {
        Some(LitVertex {
            normal,
            object_rgb: [0x80; 3],
        })
    }

    #[test]
    fn baked_vertices_keep_their_colour() {
        let mut colors = vec![[0x55, 0x66, 0x77], [0x80; 3]];
        let l = FieldLight::SCENE_LOAD;
        let m = l.light_matrix();
        let away = [-m[0][0] as i16, -m[0][1] as i16, -m[0][2] as i16];
        shade_lit_rows(&mut colors, &[None, lit(away)], &l, &draw_rotation(0, 0, 0));
        assert_eq!(colors, vec![[0x55, 0x66, 0x77], [0x10; 3]]);
    }

    #[test]
    fn the_rgba_stream_follows_the_shaded_colour() {
        let mut colors = vec![[0x80; 3], [0x80; 3]];
        let mut rgba = vec![0x80, 0x80, 0x80, 255, 9, 9, 9, 0];
        let l = FieldLight::SCENE_LOAD;
        let m = l.light_matrix();
        let away = [-m[0][0] as i16, -m[0][1] as i16, -m[0][2] as i16];
        shade_lit_rows_rgba(
            &mut colors,
            &mut rgba,
            &[lit(away)],
            &l,
            &draw_rotation(0, 0, 0),
        );
        assert_eq!(rgba, vec![0x10, 0x10, 0x10, 255, 9, 9, 9, 0]);
        assert_eq!(colors[1], [0x80; 3], "past the lit mask stays untouched");
    }

    #[test]
    fn a_float_model_rotation_lands_on_the_record_rotation() {
        let r = draw_rotation(0, 0x400, 0);
        let f: [[f32; 3]; 3] =
            std::array::from_fn(|i| std::array::from_fn(|j| r[i][j] as f32 / 4096.0 * 2.0));
        let back = rotation_from_matrix(f);
        for i in 0..3 {
            for j in 0..3 {
                assert!((back[i][j] - r[i][j]).abs() <= 1, "{back:?} vs {r:?}");
            }
        }
    }
}
