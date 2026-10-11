//! The field's **light source**: the GTE light matrix, colour matrix and back
//! colour the light-source TMD rows (group flags `0x10..=0x17`) are shaded
//! through.
//!
//! The field dispatcher `FUN_80043390` sends a group by `flags >> 1`: kinds
//! `12..=19` are the baked-colour rows (`DPCS` / `DPCT` depth cue, or no
//! colour op on bank 0), kinds `8..=11` the four light-source handlers
//! `FUN_8004409C` (flat tri, `NCCS`), `FUN_8004423C` (flat quad, `NCCS`),
//! `FUN_80044434` (gouraud tri, `NCCT`) and `FUN_800445B0` (gouraud quad,
//! `NCCT` + `NCCS`). For an object with normals (`object[+0x0C] != 0`) the
//! dispatcher stages `RGBC` as its colour argument scaled by the object's
//! `+0x18` colour word, `>> 7` per channel, and the GTE back colour from
//! `_DAT_8007B788` (each byte `<< 4`; `0x80043404..0x8004347C`).
//!
//! The light matrix is per frame: `FUN_800172C0` builds it from the angle
//! trio `_DAT_8007B780..84` with a half turn added on X (`FUN_80026988`,
//! the `Rx * Ry * Rz` Euler builder) into `0x1F8003A8`, and each draw folds
//! its own rotation in before `SetLightMatrix` - the placed-object draw
//! `FUN_8001ADA4` as `L * Rot(angles)` (`0x8001B2F4..0x8001B368`), the
//! per-cell decoration pass `FUN_801F7088` by rotating the matrix through
//! `Rz`, `Ry`, `Rx` (`0x801F781C..0x801F7870`). Either way a corner's
//! intensity is the light matrix against its **world** normal.
//!
//! The colour matrix is the static `SCUS_942.54` word block `0x800704EC`
//! (`FUN_8001DCF8` uploads it through `SetColorMatrix`): every row is
//! `(4096, 0, 0)`, so all three channels follow the first light alone.
//!
//! The scene loaders (`FUN_8003AEB0` at `0x8003B4B4..0x8003B4D8`, and the
//! field overlay's `FUN_801D6704`) reset the trio to `(0x994, 0x9CC,
//! -0x62C)` and the back colour to `0x202020`; field-VM op `4C 8A` sets all
//! four. A town that sets `0xFFFFFF` pins every lit corner at or above
//! neutral; a cave left on `0x202020` leaves a face turned from the light at
//! an eighth of its texel.

/// The GTE colour matrix every field frame shades through (`0x800704EC`):
/// each output channel takes the first light's intensity at unit gain.
pub const LIGHT_COLOUR_MATRIX: [[i32; 3]; 3] = [[4096, 0, 0], [4096, 0, 0], [4096, 0, 0]];

/// The light-angle trio and back colour the field light is built from -
/// retail's `_DAT_8007B780..84` and `_DAT_8007B788`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FieldLight {
    /// `_DAT_8007B780 / 82 / 84`, PSX 12-bit angles. `FUN_800172C0` adds
    /// `0x800` to the first before building the matrix.
    pub angles: [i16; 3],
    /// `_DAT_8007B788`'s low three bytes `[R, G, B]` - the GTE back colour,
    /// staged as `byte << 4` (`0x20` = an eighth of unity).
    pub back: [u8; 3],
}

impl FieldLight {
    /// What every scene load leaves (`FUN_8003AEB0`, `0x8003B4B4..0x8003B4D8`).
    pub const SCENE_LOAD: FieldLight = FieldLight {
        angles: [0x994, 0x9CC, -0x62C],
        back: [0x20, 0x20, 0x20],
    };

    /// Field-VM op `4C 8A`: three `i16` angles and the packed back-colour
    /// word (`R` in the low byte, as `FUN_80043390` unpacks it).
    pub fn from_op_4c_8a(angles: [i16; 3], packed: u32) -> Self {
        FieldLight {
            angles,
            back: [packed as u8, (packed >> 8) as u8, (packed >> 16) as u8],
        }
    }

    /// The world light matrix `FUN_800172C0` builds into `0x1F8003A8`.
    pub fn light_matrix(&self) -> [[i32; 3]; 3] {
        euler_rot([
            self.angles[0].wrapping_add(0x800),
            self.angles[1],
            self.angles[2],
        ])
    }

    /// The colour retail's `NCCS` gives one lit corner: `normal` is the
    /// object-local TMD normal, `rot` the draw's rotation (q3.12, the matrix
    /// the geometry is drawn with), `rgbc` the staged primary colour.
    pub fn shade(&self, rot: &[[i32; 3]; 3], normal: [i16; 3], rgbc: [u8; 3]) -> [u8; 3] {
        let l = mul_matrix0(&self.light_matrix(), rot);
        nccs(&l, &LIGHT_COLOUR_MATRIX, self.back, rgbc, normal)
    }
}

/// q3.12 sine of a PSX 12-bit angle - the retail LUT at `0x80070A2C`
/// (`4096 * sin` truncated toward zero). Same values as
/// `legaia_engine_ui::gte::trig::psx_sin`, which the disc-gated LUT oracle
/// pins entry for entry.
fn psx_sin(angle: i16) -> i32 {
    let a = f64::from((angle as u16) & 0xFFF);
    ((a * std::f64::consts::TAU / 4096.0).sin() * 4096.0).trunc() as i32
}

fn psx_cos(angle: i16) -> i32 {
    psx_sin(angle.wrapping_add(0x400))
}

/// `FUN_80026988`: the `Rx * Ry * Rz` rotation of a PSX angle trio, q3.12,
/// with the routine's own two-step products.
pub fn euler_rot(angles: [i16; 3]) -> [[i32; 3]; 3] {
    let (cx, sx) = (psx_cos(angles[0]), psx_sin(angles[0]));
    let (cy, sy) = (psx_cos(angles[1]), psx_sin(angles[1]));
    let (cz, sz) = (psx_cos(angles[2]), psx_sin(angles[2]));
    let q = |v: i32| v >> 12;
    let a = q(cz * -sy);
    let b = q(sz * -sy);
    let e = |v: i32| v.clamp(i32::from(i16::MIN), i32::from(i16::MAX));
    [
        [e(q(cz * cy)), e(q(-(sz * cy))), e(sy)],
        [
            e(q(sz * cx) - q(a * sx)),
            e(q(cz * cx) + q(b * sx)),
            e(q(-(cy * sx))),
        ],
        [
            e(q(sz * sx) + q(a * cx)),
            e(q(cz * sx) - q(b * cx)),
            e(q(cy * cx)),
        ],
    ]
}

/// `MulMatrix0` (`FUN_8005B3A8`): `a * b`, each element `>> 12` and
/// saturated to a halfword the way the GTE's `IR` registers hold it.
pub fn mul_matrix0(a: &[[i32; 3]; 3], b: &[[i32; 3]; 3]) -> [[i32; 3]; 3] {
    let mut out = [[0; 3]; 3];
    for (i, row) in out.iter_mut().enumerate() {
        for (j, v) in row.iter_mut().enumerate() {
            let s: i64 = (0..3)
                .map(|k| i64::from(a[i][k]) * i64::from(b[k][j]))
                .sum();
            *v = (s >> 12).clamp(-0x8000, 0x7FFF) as i32;
        }
    }
    out
}

/// GTE `NCCS` with `sf = 1`, `lm = 1`, as the light-source handlers issue it
/// (`cop2 0x108041B`): the light matrix against the normal, the colour
/// matrix plus the back colour against that, then the primary colour
/// modulated by the result.
pub fn nccs(
    light: &[[i32; 3]; 3],
    colour: &[[i32; 3]; 3],
    back: [u8; 3],
    rgbc: [u8; 3],
    normal: [i16; 3],
) -> [u8; 3] {
    let ir_clamp = |v: i64| v.clamp(0, 0x7FFF);
    let n = normal.map(i64::from);
    let ir: [i64; 3] = std::array::from_fn(|r| {
        let m = &light[r];
        ir_clamp((i64::from(m[0]) * n[0] + i64::from(m[1]) * n[1] + i64::from(m[2]) * n[2]) >> 12)
    });
    let ir2: [i64; 3] = std::array::from_fn(|r| {
        let m = &colour[r];
        let bk = i64::from(back[r]) << 4;
        ir_clamp(
            ((bk << 12)
                + i64::from(m[0]) * ir[0]
                + i64::from(m[1]) * ir[1]
                + i64::from(m[2]) * ir[2])
                >> 12,
        )
    });
    std::array::from_fn(|c| {
        let mac = ir_clamp(((i64::from(rgbc[c]) * ir2[c]) << 4) >> 12);
        (mac >> 4).clamp(0, 0xFF) as u8
    })
}

/// The primary colour the dispatcher stages for a lit object: its colour
/// argument times the object's colour word, `>> 7` per channel
/// (`0x80043404..0x8004347C`).
pub fn staged_rgbc(draw_colour: [u8; 3], object_rgb: [u8; 3]) -> [u8; 3] {
    std::array::from_fn(|c| ((u32::from(draw_colour[c]) * u32::from(object_rgb[c])) >> 7) as u8)
}

/// The colour argument both field sweeps hand a scene mesh: the decoration
/// pass reads `_DAT_8007BB48`, the placed-object draw its actor's `+0x74`;
/// both hold `0x808080` on every field frame the corpus captures.
pub const FIELD_DRAW_COLOUR: [u8; 3] = [0x80, 0x80, 0x80];

#[cfg(test)]
mod tests {
    use super::*;

    const IDENTITY: [[i32; 3]; 3] = [[4096, 0, 0], [0, 4096, 0], [0, 0, 4096]];

    /// The scene-load trio builds the light matrix a `cave01` field capture
    /// holds at `0x1F8003A8` (`cave01_attached_light`), element for element.
    #[test]
    fn scene_load_light_matrix_matches_the_cave_capture() {
        assert_eq!(
            FieldLight::SCENE_LOAD.light_matrix(),
            [
                [2347, -2051, -2656],
                [-1030, -3527, 1810],
                [-3194, -370, -2538],
            ]
        );
    }

    /// A face turned from the light keeps only the back colour: `0x20 << 4`
    /// is an eighth of unity, so a neutral primary comes out at `0x10`.
    #[test]
    fn a_face_turned_away_keeps_the_back_colour_floor() {
        let l = FieldLight::SCENE_LOAD;
        let m = l.light_matrix();
        let away = [-m[0][0] as i16, -m[0][1] as i16, -m[0][2] as i16];
        assert_eq!(l.shade(&IDENTITY, away, [0x80; 3]), [0x10; 3]);
    }

    /// A face square to the light takes the back colour plus unit gain:
    /// `0x80 * (0x200 + 0x1000) >> 12 = 0x90`.
    #[test]
    fn a_face_square_to_the_light_takes_full_gain() {
        let l = FieldLight::SCENE_LOAD;
        let m = l.light_matrix();
        let toward = [m[0][0] as i16, m[0][1] as i16, m[0][2] as i16];
        let c = l.shade(&IDENTITY, toward, [0x80; 3]);
        assert!((0x8E..=0x90).contains(&c[0]), "{c:?}");
        assert_eq!(c[0], c[1]);
        assert_eq!(c[1], c[2]);
    }

    /// A town's `0xFFFFFF` back colour pins every lit corner at neutral or
    /// brighter, saturating at `0xFF`.
    #[test]
    fn a_white_back_colour_never_darkens() {
        let l = FieldLight::from_op_4c_8a([0x9C4, 0x9C4, -0x5DC], 0xFFFF_FFFF);
        assert_eq!(l.back, [0xFF; 3]);
        let m = l.light_matrix();
        let away = [-m[0][0] as i16, -m[0][1] as i16, -m[0][2] as i16];
        let toward = [m[0][0] as i16, m[0][1] as i16, m[0][2] as i16];
        assert_eq!(l.shade(&IDENTITY, away, [0x80; 3]), [0x7F; 3]);
        assert_eq!(l.shade(&IDENTITY, toward, [0x80; 3]), [0xFF; 3]);
    }

    /// A yawed draw turns its normals with it: the face that is lit at rest
    /// goes dark once the object is turned half a revolution.
    #[test]
    fn a_draw_rotation_turns_the_normal() {
        let l = FieldLight::SCENE_LOAD;
        let n = [4096i16, 0, 0];
        let half = euler_rot([0, 0x800, 0]);
        let a = l.shade(&IDENTITY, n, [0x80; 3]);
        let b = l.shade(&half, n, [0x80; 3]);
        assert!(a[0] > 0x40 && b[0] == 0x10, "{a:?} {b:?}");
    }

    #[test]
    fn staged_primary_is_the_product_over_128() {
        assert_eq!(
            staged_rgbc([0x80; 3], [0x80, 0x40, 0xFF]),
            [0x80, 0x40, 0xFF]
        );
        assert_eq!(staged_rgbc([0x40; 3], [0x80; 3]), [0x40; 3]);
    }
}
