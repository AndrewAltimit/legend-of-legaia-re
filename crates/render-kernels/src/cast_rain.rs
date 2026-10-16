//! PROT 0920's (Slippery, spell `0x92`) **Deadly Rain**: the per-frame rain
//! routine the battle draw tick calls into the module, and the streak
//! packets it builds.
//!
//! PORT: FUN_801F7B88 (PROT 0920; the rain routine, whole: spawn, fall, land, project, packets)
//!
//! # What the retail routine does
//!
//! `FUN_800480D8`, the per-actor battle draw tick, calls `0x801F7B88` once a
//! battle frame while the module's budget word `_DAT_8007BDC0` is non-zero
//! (`docs/subsystems/cast-module.md`, *SCUS calls into slot B*). The routine
//! owns `0x400` eight-byte records at `*(0x801F8448)` - `x`, `y`, `z` and a
//! length, a record with length `0` being free - and on each call:
//!
//! 1. **Spawns.** Every free record draws `rand()`; when it divides the
//!    budget evenly (`div` / `mfhi` at `0x801F7C8C`) the record is seeded
//!    from four more draws: `x = 0x600 - rand() % 0xC00`, `z = 0x600 -
//!    rand() % 0xC00`, `y = -0x400 - rand() % 0x400`, length `0x100 +
//!    rand() % 0x80`. The budget is the spawn odds: `1` in `0x204` as the
//!    module seeds it, `1` in `4` once its drain is spent.
//! 2. **Falls.** Every live record drifts against the creature's facing `f`
//!    (`x -= (sin f * 4 r) >> 12`, `z -= (cos f * 4 r) >> 12`, `r` the frame
//!    step `0x1F800393`) and drops `r << 6` (`y += r * 64`).
//! 3. **Lands.** A record whose `y` turns positive is freed. While fewer
//!    than `16` splashes have spawned this call, it is put on the ground
//!    (`y = 0`), projected (`RTPS`, `FUN_8003D368`), and - if `IR3 > 0x100`
//!    and its `SX` or `SY` lies in `4..0x13C` - a splash is spawned there
//!    through `FUN_80021B04` from the module record `0x801F81F0`, whose
//!    colour halfwords `+0x22` / `+0x24` / `+0x26` the routine refreshes
//!    from the rain colour word each call; the part's `+0x78` takes
//!    `min(IR3, 0x2000) >> 1`.
//! 4. **Streaks.** Every other record is a `0x53` `LINE_G2` (gouraud,
//!    semi-transparent): the head is the record's projection, the tail the
//!    projection of the record moved back up its fall, `2 len / 3` higher
//!    and `(sin f * len) >> 13` / `(cos f * len) >> 13` upwind. The line is
//!    dropped unless the head's `IR3` exceeds `0x80`, the two together
//!    exceed `0x100`, and one of `SX0`, `SX1` lies in `4..0x13C` or one of
//!    `SY0`, `SY1` in `4..0xDC`. The head carries the rain colour word
//!    `0x801F8444` scaled by `0x100 - min(IR3 + IR3', 0x8000) / 128`, over
//!    `0x100`; the tail is black. It links at ordering-table entry
//!    `(depth / 2) >> *(0x1F8003A4)` with a `SetDrawMode` (tpage `0x20`:
//!    ABR 1, `B + F`) linked after it at the same entry, so the line draws
//!    additively.
//!
//! The colour word is the tick's: arm `0` sets `0x808080` (grey rain), arm
//! `9` ramps it a step per vsync towards red, and its exit pins `0xFF4040`.
//!
//! # Engine mapping, disclosed
//!
//! - The routine reads its rotation, translation and `H` from whatever the
//!   GTE holds when the draw tick reaches it; [`RainCamera`] is that state,
//!   built by the caller.
//! - The splash spawn's on-screen test reads two more halfwords
//!   (`sp+0x24`, `sp+0x26`) that this routine never writes - stale stack.
//!   The port treats them as off screen, so a splash it spawns is one whose
//!   `SX` or `SY` passes.
//! - The streaks draw over the finished scene instead of interleaving with
//!   it through the ordering table, so a streak behind a body still shows.
//!   Additive blending is order-free among the streaks themselves.

use crate::gte::{GteMat3, GteVec3, ScreenXY, gte_divide, gte_persp_term, psx_cos, psx_sin};
use crate::screen_prim::{ScreenPrim, line_quad};

/// The routine's record count (`slti v0,s4,0x400`).
pub const RAIN_DROPS: usize = 0x400;
/// Splashes one call may spawn (`slti v0,s7,0x10` at `0x801F7E28`).
pub const RAIN_SPLASH_CAP: usize = 0x10;
/// ABR mode the streaks draw under (`SetDrawMode` tpage `0x20`).
pub const RAIN_ABR_MODE: u8 = 1;
/// The module VA of the splash's spawn record (`addiu a2,a2,-0x7e10`).
pub const RAIN_SPLASH_RECORD: u32 = 0x801F_81F0;
/// Where the screen-prim overlay links the streaks: behind every fixed
/// packet entry, as their depth-sorted retail entries are.
pub const RAIN_OT: u32 = 0x200;

/// One rain record (`x`, `y`, `z`, length; length `0` = free).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RainDrop {
    pub x: i16,
    pub y: i16,
    pub z: i16,
    pub len: i16,
}

/// The GTE state the routine's `RTPS` calls see.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RainCamera {
    /// Rotation `RT`, q3.12.
    pub rot: GteMat3,
    /// Translation `TR`.
    pub tr: GteVec3,
    /// Projection distance `H`.
    pub h: u16,
    /// Screen offsets `OFX` / `OFY`, whole pixels.
    pub ofx: i32,
    pub ofy: i32,
}

/// The battle world scale the draw tick's GTE rotation carries: the
/// camera matrix composed with the base matrix `0x8007BF10 = 16384 * I`
/// (`4` in q3.12), as every actor draw's is.
pub const RAIN_WORLD_SCALE: i32 = 4;

impl RainCamera {
    /// The GTE state the battle draw tick leaves for the rain: the camera
    /// rotation `Rx(pitch) * Ry(yaw)` ([`crate::battle_intro::euler_rot_psx`])
    /// scaled by [`RAIN_WORLD_SCALE`], `TR = tr + (R * stored_focus) >> 12`
    /// with `stored_focus` the negated focus the globals `0x80089118..20`
    /// hold, `H = 0x100`, `OFX = 160`, `OFY = 114`.
    ///
    /// Read off the GTE control registers at the routine's entry in a live
    /// cast (`capture`): pitch `0`, yaw `0x7F3`, TR globals `(0, 0x440,
    /// 0xA19)` and stored focus `(-63, 0, 591)` gave `RT11 = -16380`, `RT13 =
    /// 324`, `RT22 = 16384` and `TR = (298, 1088, 226)`, which this
    /// composition reproduces.
    pub fn battle(pitch: i16, yaw: i16, tr: [i32; 3], focus: [i32; 3]) -> Self {
        let r = crate::battle_intro::euler_rot_psx((pitch, yaw, 0));
        let scale = |e: i16| (i32::from(e) * RAIN_WORLD_SCALE).clamp(-0x8000, 0x7FFF) as i16;
        let rot = GteMat3 {
            m: r.m.map(|row| row.map(scale)),
        };
        let off = rot.mul_vec(GteVec3::new(-focus[0], -focus[1], -focus[2]));
        Self {
            rot,
            tr: GteVec3::new(tr[0] + off.x, tr[1] + off.y, tr[2] + off.z),
            h: 0x100,
            ofx: 160,
            ofy: 114,
        }
    }

    /// `FUN_8003D368`: `RTPS` one point, returning the saturated `SXY` and
    /// `IR3`.
    pub fn rtps(&self, p: [i16; 3]) -> ((i16, i16), i32) {
        let v = self.rot.mul_vec(GteVec3::new(
            i32::from(p[0]),
            i32::from(p[1]),
            i32::from(p[2]),
        ));
        let (x, y, z) = (v.x + self.tr.x, v.y + self.tr.y, v.z + self.tr.z);
        let ir = |v: i32| v.clamp(i32::from(i16::MIN), i32::from(i16::MAX));
        let sz3 = z.clamp(0, 0xFFFF) as u16;
        let (recip, _) = gte_divide(self.h, sz3);
        let sx = self.ofx + gte_persp_term(ir(x), recip) as i32;
        let sy = self.ofy + gte_persp_term(ir(y), recip) as i32;
        let s = ScreenXY::new(sx, sy).saturate_sxy();
        ((s.x as i16, s.y as i16), ir(z))
    }
}

/// One streak this call drew.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RainLine {
    /// The record's projection.
    pub head: (i16, i16),
    /// The projection of the record moved back up its fall.
    pub tail: (i16, i16),
    /// The head's colour (the tail is black).
    pub rgb: [u8; 3],
    /// The summed `IR3` (capped at `0x8000`) the entry is formed from.
    pub depth: i32,
}

/// One splash this call spawned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RainSplash {
    /// The landed record, `y = 0`.
    pub at: [i16; 3],
    /// What the part's `+0x78` is given: `min(IR3, 0x2000) >> 1`.
    pub depth_half: i16,
}

/// What one call drew and spawned.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RainFrame {
    pub lines: Vec<RainLine>,
    pub splashes: Vec<RainSplash>,
}

/// The routine's records.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeadlyRain {
    pub drops: Vec<RainDrop>,
}

impl Default for DeadlyRain {
    fn default() -> Self {
        Self {
            drops: vec![RainDrop::default(); RAIN_DROPS],
        }
    }
}

/// `(a * b) >> 8` with the routine's rounding toward zero.
fn scale8(c: u32, s: i32) -> u8 {
    let p = c as i32 * s;
    let p = if p < 0 { p + 0xFF } else { p };
    (p >> 8) as u8
}

/// A `u16` coordinate test, `(v - 4) as u16 < bound`.
fn in_band(v: i16, bound: u16) -> bool {
    (v as u16).wrapping_sub(4) < bound
}

impl DeadlyRain {
    /// Free every record (the tick's arm `0`, `0x801F6BE4..0x801F6BFC`).
    pub fn clear(&mut self) {
        self.drops.iter_mut().for_each(|d| *d = RainDrop::default());
    }

    /// Whether any record is live.
    pub fn any_live(&self) -> bool {
        self.drops.iter().any(|d| d.len != 0)
    }

    /// One call of `FUN_801F7B88`.
    ///
    /// `budget` is `_DAT_8007BDC0` (the caller only calls while it is
    /// non-zero, as retail's gate does), `rate` the frame step, `facing` the
    /// creature's `+0x46`, `colour` the word `0x801F8444`, and `rand` the
    /// retail `rand()` (`0..=0x7FFF`).
    pub fn step(
        &mut self,
        budget: i32,
        rate: u8,
        facing: u16,
        colour: u32,
        cam: &RainCamera,
        mut rand: impl FnMut() -> u32,
    ) -> RainFrame {
        let f = facing & 0xFFF;
        let (cos, sin) = (psx_cos(f), psx_sin(f));
        let r4 = i32::from(rate) << 2;
        let dz = (cos * r4) >> 12;
        let dx = (sin * r4) >> 12;
        let mut out = RainFrame::default();
        if budget == 0 {
            // Retail's `div` would trap; its caller never gets here.
            return out;
        }
        for d in self.drops.iter_mut() {
            if d.len != 0 {
                continue;
            }
            if (rand() as i32) % budget != 0 {
                continue;
            }
            d.x = (0x600 - (rand() as i32) % 0xC00) as i16;
            d.z = (0x600 - (rand() as i32) % 0xC00) as i16;
            d.y = (-0x400 - (rand() as i32) % 0x400) as i16;
            d.len = ((rand() as i32) % 0x80 + 0x100) as i16;
        }
        let [cr, cg, cb] = [(colour >> 16) & 0xFF, (colour >> 8) & 0xFF, colour & 0xFF];
        for d in self.drops.iter_mut() {
            if d.len == 0 {
                continue;
            }
            d.z = d.z.wrapping_sub(dz as i16);
            d.x = d.x.wrapping_sub(dx as i16);
            d.y = d.y.wrapping_add((i32::from(rate) << 6) as i16);
            if d.y > 0 {
                d.len = 0;
                if out.splashes.len() >= RAIN_SPLASH_CAP {
                    continue;
                }
                d.y = 0;
                let ((sx, sy), ir3) = cam.rtps([d.x, d.y, d.z]);
                if ir3 <= 0x100 {
                    continue;
                }
                if !(in_band(sx, 0x138) || in_band(sy, 0x138)) {
                    continue;
                }
                out.splashes.push(RainSplash {
                    at: [d.x, d.y, d.z],
                    depth_half: (ir3.min(0x2000) >> 1) as i16,
                });
                continue;
            }
            let (head, z0) = cam.rtps([d.x, d.y, d.z]);
            if z0 <= 0x80 {
                continue;
            }
            let len = i32::from(d.len);
            let tail_pt = [
                (i32::from(d.x) - ((sin * len) >> 13)) as i16,
                (i32::from(d.y) - (2 * len) / 3) as i16,
                (i32::from(d.z) - ((cos * len) >> 13)) as i16,
            ];
            let (tail, z1) = cam.rtps(tail_pt);
            let depth = z0 + z1;
            if depth <= 0x100 {
                continue;
            }
            if !(in_band(head.0, 0x138)
                || in_band(tail.0, 0x138)
                || in_band(head.1, 0xD8)
                || in_band(tail.1, 0xD8))
            {
                continue;
            }
            let depth = depth.min(0x8000);
            let s = 0x100 - depth / 128;
            out.lines.push(RainLine {
                head,
                tail,
                rgb: [scale8(cr, s), scale8(cg, s), scale8(cb, s)],
                depth,
            });
        }
        out
    }
}

/// The streaks as screen primitives: one-pixel additive lines from the
/// coloured head to the black tail.
pub fn rain_line_prims(lines: &[RainLine]) -> Vec<ScreenPrim> {
    lines
        .iter()
        .filter_map(|l| {
            let a = [l.rgb[0], l.rgb[1], l.rgb[2], 0xFF];
            let b = [0, 0, 0, 0xFF];
            let mut q = line_quad(
                (f32::from(l.head.0), f32::from(l.head.1)),
                (f32::from(l.tail.0), f32::from(l.tail.1)),
                a,
                true,
                RAIN_ABR_MODE,
                RAIN_OT,
            )?;
            // `line_quad` lays its corners out `[a0, b0, a1, b1]`.
            q.gouraud = Some([a, b, a, b]);
            Some(ScreenPrim::Flat(q))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cam() -> RainCamera {
        RainCamera {
            rot: GteMat3::IDENTITY,
            tr: GteVec3::new(0, 0, 0x800),
            h: 0x100,
            ofx: 160,
            ofy: 114,
        }
    }

    /// A rand that cycles a fixed list.
    fn seq(v: &'static [u32]) -> impl FnMut() -> u32 {
        let mut i = 0;
        move || {
            let r = v[i % v.len()];
            i += 1;
            r
        }
    }

    /// The battle camera reproduces the GTE state captured at the routine's
    /// entry.
    #[test]
    fn the_battle_camera_matches_the_captured_gte_state() {
        let c = RainCamera::battle(0, 0x7F3, [0, 0x440, 0xA19], [63, 0, -591]);
        assert_eq!(c.rot.m[0][0], -16380);
        assert_eq!(c.rot.m[0][2], 324);
        assert_eq!(c.rot.m[1][1], 16384);
        assert_eq!(c.rot.m[2][0], -324);
        assert_eq!(c.rot.m[2][2], -16380);
        assert_eq!((c.tr.x, c.tr.y, c.tr.z), (298, 1088, 226));
    }

    /// A free record spawns only when its gate draw divides the budget, and
    /// takes the next four draws in the order `x`, `z`, `y`, length.
    #[test]
    fn a_spawn_takes_five_draws_in_retail_order() {
        let mut rain = DeadlyRain::default();
        rain.drops.truncate(1);
        const DRAWS: [u32; 5] = [4, 0xC00 + 0x10, 0x20, 0x30, 0x81];
        rain.step(4, 0, 0, 0x80_8080, &cam(), seq(&DRAWS));
        let d = rain.drops[0];
        assert_eq!(d.x, 0x600 - 0x10);
        assert_eq!(d.z, 0x600 - 0x20);
        assert_eq!(d.y, -0x400 - 0x30);
        assert_eq!(d.len, 0x101);
        // A draw that does not divide the budget leaves the record free.
        let mut rain = DeadlyRain::default();
        rain.drops.truncate(1);
        rain.step(4, 0, 0, 0, &cam(), seq(&[5]));
        assert_eq!(rain.drops[0].len, 0);
    }

    /// A falling record draws a line from its projection to its tail, the
    /// head in the rain colour scaled by depth, and drifts against the
    /// facing.
    #[test]
    fn a_falling_record_streaks_and_drifts() {
        let mut rain = DeadlyRain::default();
        rain.drops.truncate(1);
        rain.drops[0] = RainDrop {
            x: 0,
            y: -0x200,
            z: 0,
            len: 0x120,
        };
        // Facing 0: `sin = 0`, `cos = 0x1000`, so z drifts by `4 r`.
        let f = rain.step(4, 1, 0, 0x80_8080, &cam(), seq(&[1]));
        assert_eq!(rain.drops[0].z, -4);
        assert_eq!(rain.drops[0].y, -0x200 + 0x40);
        assert_eq!(f.lines.len(), 1);
        let l = f.lines[0];
        assert_eq!(l.head.0, 160);
        // The tail is above the head.
        assert!(l.tail.1 < l.head.1);
        let s = 0x100 - l.depth / 128;
        assert_eq!(l.rgb[0], ((0x80 * s) >> 8) as u8);
    }

    /// A record that crosses the ground is freed and, on screen and far
    /// enough, spawns a splash - at most sixteen a call.
    #[test]
    fn landing_frees_the_record_and_caps_the_splashes() {
        let mut rain = DeadlyRain::default();
        for d in rain.drops.iter_mut().take(20) {
            *d = RainDrop {
                x: 0,
                y: -1,
                z: 0,
                len: 0x100,
            };
        }
        let f = rain.step(0x7FFF, 1, 0, 0x80_8080, &cam(), seq(&[1]));
        assert_eq!(f.splashes.len(), RAIN_SPLASH_CAP);
        assert!(rain.drops.iter().take(20).all(|d| d.len == 0));
        assert_eq!(f.splashes[0].at[1], 0);
        assert_eq!(f.splashes[0].depth_half, (0x800 - 4) >> 1);
    }

    /// The streaks draw additively, coloured head to black tail.
    #[test]
    fn streak_prims_are_additive_gradient_lines() {
        let p = rain_line_prims(&[RainLine {
            head: (100, 100),
            tail: (90, 60),
            rgb: [0x40, 0x40, 0x40],
            depth: 0x1000,
        }]);
        let ScreenPrim::Flat(q) = p[0] else {
            panic!("flat")
        };
        assert!(q.semi_transparent);
        assert_eq!(q.abr_mode, RAIN_ABR_MODE);
        let g = q.gouraud.unwrap();
        assert_eq!(g[0], [0x40, 0x40, 0x40, 0xFF]);
        assert_eq!(g[1], [0, 0, 0, 0xFF]);
    }
}
