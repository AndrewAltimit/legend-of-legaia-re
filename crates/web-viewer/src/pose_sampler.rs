//! Between-keyframe pose sampling for the viewer pages' animation player
//! (`site/js/mesh-view.js`).
//!
//! A battle body's clip is a list of keyframes, and retail does not hold each
//! one until the next: the anim tick `FUN_80047430` advances a 12.4 cursor
//! (`node+0x68`) and the decoder `FUN_8004998C` blends the cursor's keyframe
//! toward the next one by the cursor's low nibble
//! ([`legaia_engine_battle_vm::battle_pose_blend::blend_part_pose`]). The
//! cursor moves once a battle frame, so retail shows a keyframe, then one or
//! more sixteenth-step blends, then the next keyframe - at the battle frame's
//! cadence, two or more vsyncs apart.
//!
//! [`PoseSampler`] gives the page both readings of one clip:
//!
//! - [`PoseSampler::sample_retail`] - the retail decode at a 12.4 cursor the
//!   page quantises to whole battle frames: the decoder's own blend, nibble
//!   and all.
//! - [`PoseSampler::sample_smooth`] - the enhancement: the same blend at a
//!   continuous fraction, so the body moves on every display frame. Angles take
//!   the decoder's short arc and its Euler-flip retry, so a part never swings
//!   the long way round between two keyframes a few degrees apart across
//!   `0`/`4096`.
//!
//! The next keyframe after the last is frame `0`: the viewer loops every clip,
//! which is what the idle's re-commit does in retail (the loop re-stages
//! itself, `docs/subsystems/battle-actor-rendering.md`).

use legaia_actor_formats::monster_archive::PartPose;
use legaia_engine_battle_vm::battle_pose_blend::{
    EULER_FLIP_THRESHOLD, blend_part_pose, lerp_battle_angle,
};
use wasm_bindgen::prelude::*;

/// A full turn in 12-bit angle units.
const TURN: f64 = 4096.0;

/// One clip's keyframes, held on the WASM side so a per-frame sample copies
/// only the posed parts out, not the whole clip in.
#[wasm_bindgen]
pub struct PoseSampler {
    frames: Vec<PartPose>,
    parts: usize,
    count: usize,
}

#[wasm_bindgen]
impl PoseSampler {
    /// `frames` is the pages' pose layout: `frame_count * part_count` rows of
    /// `[tx, ty, tz, rx, ry, rz]`.
    #[wasm_bindgen(constructor)]
    pub fn new(frames: &[i32], part_count: u32, frame_count: u32) -> PoseSampler {
        let parts = part_count as usize;
        let count = (frame_count as usize).min(frames.len() / (parts * 6).max(1));
        let (rows, _) = frames.as_chunks::<6>();
        let frames = rows
            .iter()
            .take(count * parts)
            .map(|p| PartPose {
                tx: p[0] as i16,
                ty: p[1] as i16,
                tz: p[2] as i16,
                rx: (p[3] & 0xFFF) as u16,
                ry: (p[4] & 0xFFF) as u16,
                rz: (p[5] & 0xFFF) as u16,
            })
            .collect();
        PoseSampler {
            frames,
            parts,
            count,
        }
    }

    pub fn part_count(&self) -> u32 {
        self.parts as u32
    }

    pub fn frame_count(&self) -> u32 {
        self.count as u32
    }

    /// Retail decode at the 12.4 cursor `cursor16` (keyframe `cursor16 >> 4`,
    /// blend nibble `cursor16 & 0xF`), wrapped onto the clip. Writes
    /// `part_count * 6` values into `out`.
    pub fn sample_retail(&self, cursor16: u32, out: &mut [f32]) {
        if self.count == 0 {
            return;
        }
        let total = (self.count as u32) * 16;
        let c = cursor16 % total;
        let (f, frac) = ((c >> 4) as usize, (c & 0xF) as u8);
        let next = (f + 1) % self.count;
        for p in 0..self.parts {
            let b = blend_part_pose(self.part(f, p), self.part(next, p), frac, 0);
            write_part(
                out,
                p,
                b.translation.map(f64::from),
                b.rotation.map(f64::from),
            );
        }
    }

    /// The smooth reading at the continuous keyframe position `cursor`
    /// (wrapped onto the clip). Writes `part_count * 6` values into `out`.
    pub fn sample_smooth(&self, cursor: f64, out: &mut [f32]) {
        if self.count == 0 || !cursor.is_finite() {
            return;
        }
        let n = self.count as f64;
        let c = cursor.rem_euclid(n);
        let f = (c.floor() as usize).min(self.count - 1);
        let t = c - f as f64;
        let next = (f + 1) % self.count;
        for p in 0..self.parts {
            let (tr, rot) = blend_smooth(self.part(f, p), self.part(next, p), t);
            write_part(out, p, tr, rot);
        }
    }
}

impl PoseSampler {
    fn part(&self, frame: usize, part: usize) -> PartPose {
        self.frames[frame * self.parts + part]
    }
}

fn write_part(out: &mut [f32], part: usize, tr: [f64; 3], rot: [f64; 3]) {
    let o = part * 6;
    if let Some(s) = out.get_mut(o..o + 6) {
        s[0] = tr[0] as f32;
        s[1] = tr[1] as f32;
        s[2] = tr[2] as f32;
        s[3] = rot[0] as f32;
        s[4] = rot[1] as f32;
        s[5] = rot[2] as f32;
    }
}

/// The decoder's blend at a continuous `t` in `[0, 1)`: translations lerped,
/// each angle on the pair [`lerp_battle_angle`] unwraps onto the short arc,
/// and the Euler-flip retry taken on the same summed-step test, so the smooth
/// path turns the way the retail one does at every sixteenth it shares.
fn blend_smooth(cur: PartPose, next: PartPose, t: f64) -> ([f64; 3], [f64; 3]) {
    let lerp = |c: i16, n: i16| f64::from(c) + (f64::from(n) - f64::from(c)) * t;
    let tr = [
        lerp(cur.tx, next.tx),
        lerp(cur.ty, next.ty),
        lerp(cur.tz, next.tz),
    ];
    let x = lerp_battle_angle(i32::from(next.rx), i32::from(cur.rx), 0);
    let y = lerp_battle_angle(i32::from(next.ry), i32::from(cur.ry), 0);
    let z = lerp_battle_angle(i32::from(next.rz), i32::from(cur.rz), 0);
    let pairs = if x.magnitude + y.magnitude + z.magnitude > EULER_FLIP_THRESHOLD {
        [
            lerp_battle_angle(x.next_unwrapped + 0x800, x.cur_unwrapped, 0),
            lerp_battle_angle(-(y.next_unwrapped + 0x800), y.cur_unwrapped, 0),
            lerp_battle_angle(z.next_unwrapped + 0x800, z.cur_unwrapped, 0),
        ]
    } else {
        [x, y, z]
    };
    let rot = pairs.map(|a| {
        let (c, n) = (f64::from(a.cur_unwrapped), f64::from(a.next_unwrapped));
        (c + (n - c) * t).rem_euclid(TURN)
    });
    (tr, rot)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two parts, three frames: part 0 turns across zero (`0xFF0 -> 0x010`),
    /// part 1 walks along X.
    fn clip() -> PoseSampler {
        #[rustfmt::skip]
        let frames = [
            0, 0, 0, 0xFF0, 0, 0,     0, 0, 0, 0, 0, 0,
            0, 0, 0, 0x010, 0, 0,     160, 0, 0, 0, 0, 0,
            0, 0, 0, 0x030, 0, 0,     320, 0, 0, 0, 0, 0,
        ];
        PoseSampler::new(&frames, 2, 3)
    }

    #[test]
    fn retail_matches_the_decoder_blend_at_each_nibble() {
        let s = clip();
        let mut out = [0f32; 12];
        for c in 0..48u32 {
            s.sample_retail(c, &mut out);
            let (f, frac) = ((c >> 4) as usize, (c & 0xF) as u8);
            let b = blend_part_pose(s.part(f, 1), s.part((f + 1) % 3, 1), frac, 0);
            assert_eq!(out[6], f32::from(b.translation[0]), "cursor {c}");
        }
        // Half way from frame 0 to 1 the turn is at 0x000, not at 0x800.
        s.sample_retail(8, &mut out);
        assert_eq!(out[3], 0.0);
    }

    #[test]
    fn smooth_agrees_with_retail_on_the_sixteenths() {
        let s = clip();
        let (mut a, mut b) = ([0f32; 12], [0f32; 12]);
        for c in [0u32, 4, 8, 12, 16, 24, 40] {
            s.sample_retail(c, &mut a);
            s.sample_smooth(f64::from(c) / 16.0, &mut b);
            for i in 0..12 {
                assert!(
                    (a[i] - b[i]).abs() <= 1.0,
                    "cursor {c} value {i}: {a:?} {b:?}"
                );
            }
        }
    }

    #[test]
    fn smooth_takes_the_short_arc_across_zero() {
        let s = clip();
        let mut out = [0f32; 12];
        // A quarter of the way from 0xFF0 to 0x010: 0xFF8, never near 0x800.
        s.sample_smooth(0.25, &mut out);
        assert!((out[3] - 4088.0).abs() < 1e-3, "{}", out[3]);
        s.sample_smooth(0.75, &mut out);
        assert!((out[3] - 8.0).abs() < 1e-3, "{}", out[3]);
    }

    #[test]
    fn the_last_keyframe_blends_back_to_the_first() {
        let s = clip();
        let mut out = [0f32; 12];
        s.sample_smooth(2.5, &mut out);
        assert!((out[6] - 160.0).abs() < 1e-3);
        s.sample_smooth(3.0, &mut out);
        assert_eq!(out[6], 0.0);
    }
}
