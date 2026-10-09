//! The overworld sky band: the strip of painted sky the kingdom overworld
//! draws behind its terrain, ported from `FUN_801F73E4` (PROT 0901, the
//! world-map render module).
//!
//! PORT: FUN_801F73E4
//!
//! The routine runs once per overworld frame, unconditionally, as the first
//! call of the terrain sweep `FUN_801F69D8` (`jal 0x801F73E4` at
//! `0x801F6A18`). It emits up to five `SPRT` packets (command `0x64`) and one
//! draw-mode packet, all linked into the farthest ordering-table bucket
//! (`*0x1F8003F4 + *0x1F8003A6 * 4 - 8`), so every terrain primitive of the
//! frame draws over it.
//!
//! ## Where the band sits
//!
//! The band hangs off the projection of one far point straight ahead of an
//! **unyawed** camera (`0x801F73E4..0x801F7444`):
//!
//! 1. save the yaw word `_DAT_8007B792`, zero it, and rebuild the view
//!    (`FUN_800172C0`) - pitch and roll kept, the `6x` base matrix folded in;
//! 2. load `TR` with the raw eye trio `_DAT_800840B8/BC/C0` (`FUN_8003D1EC`),
//!    dropping the focus term the view build put there;
//! 3. `RTPS` the vector `(0, 0, 10000)` (`FUN_8003D368`) for `(sx, sy)`.
//!
//! With the yaw zeroed, `R * (0, 0, 10000)` is `10000` times the base-scaled
//! rotation's third column, `6 * (0, -sin pitch, cos pitch)` - roll drops
//! out - so `sy` is the screen row of the camera's horizon line. The band's
//! top edge is `sy + 16` (`addiu s3, v1, 0x10`).
//!
//! The horizontal scroll is
//! `x0 = ((sx + focus_word / 64 + yaw) & 0xFF) - 0xFF`
//! (`0x801F7444..0x801F7478`): the stored focus word `_DAT_80089118` (the
//! negated player X) divided toward zero, plus the saved yaw, wrapped to one
//! 256-pixel period. Sprite `n` (`n = 0..5`) sits at `x0 + 128 * n`.
//!
//! ## The sprites
//!
//! The texture is two 128x128 tiles stacked in the 8bpp page at VRAM
//! `(512, 256)` (draw mode `0x98`, `FUN_80059010(p, 0, 0, 0x98, 0)`), CLUT
//! `0x7A80` (row 490): sprite `n` samples tile `n & 1` (`v` base
//! `(n & 1) << 7`), so the band repeats every 256 pixels. Each sprite is
//! clipped against the screen's left and top edges by moving its `u` / `v`
//! origin and shrinking its size, exactly as the per-sprite arm does
//! (`0x801F74DC..0x801F7588`):
//!
//! - skipped when `x + 128 <= 0` or `x >= 320`;
//! - `x < 0`: `u = -x`, `w = 128 + x`, `x = 0`;
//! - `x + 128 > 320`: `u = 0`, `w = 128 - (x - 320)` - **wider** than the
//!   tile, not narrower. Retail subtracts the overhang's negation, so the
//!   last sprite runs past the screen edge and samples `u` past `127`; the
//!   draw area crops it. Kept as is: the retail packets read the same way
//!   (`sebucus_overworld_resident` carries a 151-wide sprite at `x = 297`);
//! - otherwise `u = 0`, `w = 128`;
//! - `y < 0`: `v = -y + ((n & 1) << 7)`, `h = 128 + y`, `y = 0`; otherwise
//!   `v = (n & 1) << 7`, `h = 128`.
//!
//! The modulation colour is `0x808080`, or `0x404040` once system flag
//! `0x14C` is set (`FUN_8003CE64(0x14C)` at `0x801F747C`).
//!
//! The routine then restores the yaw word and rebuilds the view, so the rest
//! of the frame draws under the camera it expected.

use crate::field_light::euler_rot;
use crate::gte_divide::gte_divide;

/// The sky texture's CLUT word - VRAM row 490, column 0, 256 entries.
pub const SKY_CLUT: u16 = 0x7A80;

/// The draw-mode texpage the band samples: 8bpp, page `(512, 256)`, ABR 0.
pub const SKY_TPAGE: u16 = 0x0098;

/// The system flag that dims the band to half intensity.
pub const SKY_DIM_FLAG: u16 = 0x14C;

/// The modulation colour of the band, `0x00RRGGBB`.
pub const SKY_COLOR: u32 = 0x0080_8080;

/// [`SKY_COLOR`] once [`SKY_DIM_FLAG`] is set.
pub const SKY_COLOR_DIM: u32 = 0x0040_4040;

/// GTE screen offsets on the overworld: `OFX = 160`, and `OFY` half the
/// field's 228-row draw area (`FUN_8001698C`). Both resident overworld
/// states' sky packets reproduce at `OFY = 114` and miss by six rows at 120.
const OFX: i64 = 160;
const OFY: i64 = 114;

/// The far point the band hangs off, straight down the unyawed view axis.
const FAR_Z: i64 = 10_000;

/// The base matrix `_DAT_8007BF10` on the overworld: `24576 * I` (6x).
const BASE_SCALE: i64 = 0x6000;

/// One `SPRT` of the band, in retail packet fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SkySprite {
    pub x: i16,
    pub y: i16,
    pub u: u8,
    pub v: u8,
    /// Width in pixels. Can exceed 128 on the right-hand sprite (see the
    /// module docs).
    pub w: i16,
    pub h: i16,
    /// Modulation colour, `0x00RRGGBB`.
    pub color: u32,
}

/// `(sx, sy)` of the far point `(0, 0, 10000)` under the unyawed view: the
/// `RTPS` `FUN_8003D368` runs with `R` = the base-scaled `Rot(pitch, 0, roll)`
/// and `TR` = the raw eye trio.
///
/// `globals` is the retail camera globals block
/// ([`crate::retail_cam::RetailCamGlobals`]): pitch / yaw / roll, the eye
/// trio, the focus trio and `H`.
pub fn far_point_screen(globals: &[i32; 10]) -> (i32, i32) {
    let pitch = globals[0] as i16;
    let roll = globals[2] as i16;
    let rot = euler_rot([pitch, 0, roll]);
    // `MulMatrix0(base, R)`: a uniform scale, so each element is
    // `(0x6000 * r) >> 12` saturated to a halfword.
    let scaled = |r: i32| ((BASE_SCALE * i64::from(r)) >> 12).clamp(-0x8000, 0x7FFF);
    let tr = [
        i64::from(globals[3]),
        i64::from(globals[4]),
        i64::from(globals[5]),
    ];
    // RTPS, sf = 1: MAC = (TR << 12 + R * V) >> 12 with V = (0, 0, FAR_Z).
    let mac: [i64; 3] = std::array::from_fn(|i| ((tr[i] << 12) + scaled(rot[i][2]) * FAR_Z) >> 12);
    let ir = |m: i64| m.clamp(-0x8000, 0x7FFF);
    let sz3 = mac[2].clamp(0, 0xFFFF) as u16;
    let h = globals[9].clamp(0, 0xFFFF) as u16;
    let (recip, _) = gte_divide(h, sz3);
    let sx = (OFX + ((ir(mac[0]) * recip) >> 16)).clamp(-0x400, 0x3FF);
    let sy = (OFY + ((ir(mac[1]) * recip) >> 16)).clamp(-0x400, 0x3FF);
    (sx as i32, sy as i32)
}

/// The band's sprites for one frame, in emission order.
///
/// A sprite whose top clip leaves no rows (`h <= 0`, the band wholly above
/// the screen) is dropped: retail would hand the GPU a negative height, which
/// wraps to a tall garbage sprite in the farthest bucket under the terrain.
/// No overworld camera pitches that far.
pub fn sky_band_sprites(globals: &[i32; 10], dim: bool) -> Vec<SkySprite> {
    let (sx, sy) = far_point_screen(globals);
    let yaw = globals[1] as i16;
    // `_DAT_80089118` / 64, rounded toward zero (`bgez` / `addiu 0x3f` /
    // `sra 6`).
    let focus = globals[6] / 64;
    let x0 = ((sx + focus + i32::from(yaw)) & 0xFF) - 0xFF;
    let y0 = (sy + 0x10) as i16;
    let color = if dim { SKY_COLOR_DIM } else { SKY_COLOR };
    let mut out = Vec::with_capacity(5);
    for n in 0..5i32 {
        let mut x = (n * 0x80 + x0) as i16;
        if i32::from(x) + 0x80 <= 0 || x >= 0x140 {
            continue;
        }
        let tile_v = ((n & 1) << 7) as u8;
        let (u, w);
        if x < 0 {
            u = (-x) as u8;
            w = 0x80 + x;
            x = 0;
        } else if i32::from(x) + 0x80 > 0x140 {
            u = 0;
            w = 0x80 - (x - 0x140);
        } else {
            u = 0;
            w = 0x80;
        }
        let (mut y, v, h) = (y0, tile_v, 0x80i16);
        let (v, h) = if y < 0 {
            let clip = -y;
            y = 0;
            ((clip as u8).wrapping_add(v), h - clip)
        } else {
            (v, h)
        };
        if h <= 0 {
            continue;
        }
        out.push(SkySprite {
            x,
            y,
            u,
            v,
            w,
            h,
            color,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A camera block with the given pitch / yaw, eye trio, focus X and H.
    fn globals(pitch: i32, yaw: i32, eye: [i32; 3], focus_x: i32, h: i32) -> [i32; 10] {
        [pitch, yaw, 0, eye[0], eye[1], eye[2], focus_x, 0, 0, h]
    }

    #[test]
    fn far_point_is_the_horizon_row_of_the_unyawed_view() {
        // The Sebucus resident camera: pitch 360, eye (-71, 536, 9139),
        // H 368. Its sky packets put the band top at -60, i.e. sy = -76.
        let g = globals(360, 0, [-71, 536, 9139], -7616, 368);
        assert_eq!(far_point_screen(&g), (159, -76));
        // Yaw does not move the far point: the routine zeroes it first.
        let g2 = globals(360, 0x321, [-71, 536, 9139], -7616, 368);
        assert_eq!(far_point_screen(&g2), (159, -76));
    }

    #[test]
    fn sebucus_band_reproduces_the_retail_packets() {
        let g = globals(360, 0, [-71, 536, 9139], -7616, 368);
        let s = sky_band_sprites(&g, false);
        let fields: Vec<_> = s.iter().map(|p| (p.x, p.y, p.u, p.v, p.w, p.h)).collect();
        assert_eq!(
            fields,
            vec![
                (0, 0, 87, 188, 41, 68),
                (41, 0, 0, 60, 128, 68),
                (169, 0, 0, 188, 128, 68),
                (297, 0, 0, 60, 151, 68),
            ]
        );
        assert!(s.iter().all(|p| p.color == SKY_COLOR));
    }

    #[test]
    fn karisto_band_reproduces_the_retail_packets() {
        let g = globals(476, 0, [-86, 406, 11041], -9280, 368);
        let s = sky_band_sprites(&g, true);
        let fields: Vec<_> = s.iter().map(|p| (p.x, p.y, p.u, p.v, p.w, p.h)).collect();
        assert_eq!(
            fields,
            vec![
                (0, 0, 113, 215, 15, 41),
                (15, 0, 0, 87, 128, 41),
                (143, 0, 0, 215, 128, 41),
                (271, 0, 0, 87, 177, 41),
            ]
        );
        assert!(s.iter().all(|p| p.color == SKY_COLOR_DIM));
    }

    #[test]
    fn yaw_scrolls_the_band_with_a_256_pixel_period() {
        let base = globals(360, 0, [-71, 536, 9139], -7616, 368);
        let mut turned = base;
        turned[1] = 0x100;
        assert_eq!(
            sky_band_sprites(&base, false),
            sky_band_sprites(&turned, false)
        );
        turned[1] = 10;
        let a = sky_band_sprites(&base, false);
        let b = sky_band_sprites(&turned, false);
        assert_ne!(a, b);
    }

    #[test]
    fn a_band_on_screen_keeps_its_full_height() {
        // A shallow pitch puts the horizon low on the screen: no top clip.
        let g = globals(0, 0, [0, 0, 9000], 0, 368);
        let s = sky_band_sprites(&g, false);
        assert!(!s.is_empty());
        assert!(s.iter().all(|p| p.h == 128 && p.y > 0));
    }
}
