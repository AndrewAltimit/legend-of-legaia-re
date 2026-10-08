//! The battle pose decoder's two-keyframe blend, including its Euler-flip
//! retry.
//!
//! `FUN_8004998C` (SCUS) poses every battle body - monster seats, party seats
//! and the summon - from the `[u8 parts][u8 frames][9-byte TRS]` stream its
//! committed entry points at (`docs/formats/monster-animation.md`). For each
//! part it reads the entry of the frame the cursor sits on (`cur`, frame
//! `+0x68 >> 4`) and the entry the next-entry rule picked (`next`), and blends
//! them by the cursor's low nibble `frac = +0x68 & 0xF` (`0x80049A18`). A zero
//! nibble skips the blend and decodes `cur` alone (`beq v0,zero,0x8004A284`).
//!
//! The blend arm (`0x80049C18..0x8004A21C`, one pass per part):
//!
//! - **Translations** are sign-extended 12-bit values lerped linearly:
//!   `cur + (((next - cur) * frac) >> 4)`, `sra` then `sh`.
//! - **Angles** are unsigned 12-bit values brought onto the short arc before
//!   the lerp ([`lerp_battle_angle`]). The absolute step of each axis is
//!   summed into `gp+0xA10` (`_DAT_8007BD28`); X **stores** its magnitude
//!   (`sw` at `0x80049E0C` / `0x80049E18`) and the part's Z-translation arm
//!   zeroes the word first (`sw zero,0xa10(gp)` at `0x80049D6C`), so the sum
//!   is **per part**.
//! - **The retry**: `slti s1,s1,0xc01` at `0x80049FD8`. When the three steps
//!   total more than [`EULER_FLIP_THRESHOLD`], the three angles are blended
//!   again against the **next** frame's equivalent Euler triple -
//!   `(x + 0x800, -(y + 0x800), z + 0x800)`, each built from the journaled
//!   unwrapped `next` (`lhu` of `0x801C9060 + 0/4/8`, then `addiu 0x800`,
//!   `subu zero` for Y, `andi 0xfff`: `0x80049FE4`, `0x8004A094..0x8004A0A4`,
//!   `0x8004A15C`) - with the journaled unwrapped `cur` masked back to 12
//!   bits as the other operand (`lhu +2/+6/+A`, `andi 0xfff`). The journal
//!   is a scratch table the pass overwrites per part; the keyframe stream is
//!   never written. The retry has no second gate.
//!
//! The field blender `FUN_8001BE80` carries the same retry
//! (`legaia_asset::player_anm::blend_bone_transform`), but the two unwraps
//! differ at exactly half a turn: the field helper `FUN_8001D088` bumps on
//! `>= 0x800`, this decoder on `> 0x800` (`slti v0,v0,0x801`). A step of
//! exactly `-0x800` therefore runs backward here and forward in the field.

use legaia_asset::monster_archive::PartPose;

/// A full turn in 12-bit angle units.
const TURN: i32 = 0x1000;

/// The summed-step threshold past which the blend is retried against the
/// equivalent Euler triple (`slti s1,s1,0xc01` at `0x80049FD8`: the retry
/// runs when the sum is `> 0xC00`).
pub const EULER_FLIP_THRESHOLD: i32 = 0xC00;

/// One axis of the angle blend, with the unwrapped pair retail journals at
/// `0x801C9060` and the step magnitude it adds to `gp+0xA10`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BattleAngleLerp {
    /// The blended 12-bit angle (`andi v0,v0,0xfff`, then `sh`).
    pub value: u16,
    /// `next` after the unwrap (the journal's first halfword of the pair).
    pub next_unwrapped: i32,
    /// `cur` after the unwrap (the journal's second halfword).
    pub cur_unwrapped: i32,
    /// `|next - cur|` after the unwrap.
    pub magnitude: i32,
}

/// Blend one 12-bit angle from `cur` toward `next` by `frac / 16` along the
/// short arc, the way `FUN_8004998C` does per axis (`0x80049D80..0x80049E38`
/// for X; Y and Z are the same shape).
///
/// Two sequential guards, each a signed `slti 0x801`: if `next - cur > 0x800`
/// add a turn to `cur`; then, re-reading the bumped `cur`, if
/// `cur - next > 0x800` add a turn to `next`. The result is
/// `(cur + (((next - cur) * frac) >> 4)) & 0xFFF`. Both inputs are masked to
/// 12 bits first, which is what the retry's `andi 0xfff` operands and the
/// 12-bit stream reads already are.
pub fn lerp_battle_angle(next: i32, cur: i32, frac: i32) -> BattleAngleLerp {
    let mut next = next & 0xFFF;
    let mut cur = cur & 0xFFF;
    if next - cur > TURN / 2 {
        cur += TURN;
    }
    if cur - next > TURN / 2 {
        next += TURN;
    }
    BattleAngleLerp {
        value: ((cur + (((next - cur) * frac) >> 4)) & 0xFFF) as u16,
        next_unwrapped: next,
        cur_unwrapped: cur,
        magnitude: (next - cur).abs(),
    }
}

/// One part's blended pose, plus whether the Euler-flip retry replaced the
/// angles.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlendedPart {
    /// Blended translation (the pose buffer's first three halfwords).
    pub translation: [i16; 3],
    /// Blended 12-bit angles (the pose buffer's last three halfwords).
    pub rotation: [u16; 3],
    /// `true` when the summed step exceeded [`EULER_FLIP_THRESHOLD`] and the
    /// angles come from the retry.
    pub retried: bool,
}

/// Blend part `cur` toward `next` by `frac / 16`.
///
/// `z_bias` is the term `FUN_8004998C` adds to the Z-translation delta before
/// the multiply (`lw s7,0x18(sp)` / `addu v1,v1,s7` at `0x80049D5C..0x80049D64`):
/// the committed entry's `+0xE` halfword when the next entry is frame 0 of the
/// queued clip and the actor's `+0x228` byte is clear (`0x80049BA8..0x80049BC8`),
/// `0` otherwise.
///
/// `frac == 0` returns `cur` exactly: retail skips the blend arm on a zero
/// nibble, so whole-frame samples are the plain decode.
///
/// PORT: FUN_8004998C (the per-part blend arm `0x80049C18..0x8004A21C`: translation lerp, short-arc angle lerp, the `gp+0xA10` step sum and the Euler-flip retry)
pub fn blend_part_pose(cur: PartPose, next: PartPose, frac: u8, z_bias: i16) -> BlendedPart {
    let frac = i32::from(frac & 0xF);
    if frac == 0 {
        return BlendedPart {
            translation: [cur.tx, cur.ty, cur.tz],
            rotation: [cur.rx & 0xFFF, cur.ry & 0xFFF, cur.rz & 0xFFF],
            retried: false,
        };
    }
    let lerp_t = |c: i16, n: i16, bias: i16| -> i16 {
        let (c, n) = (i32::from(c), i32::from(n));
        (c + (((n - c + i32::from(bias)) * frac) >> 4)) as i16
    };
    let x = lerp_battle_angle(i32::from(next.rx), i32::from(cur.rx), frac);
    let y = lerp_battle_angle(i32::from(next.ry), i32::from(cur.ry), frac);
    let z = lerp_battle_angle(i32::from(next.rz), i32::from(cur.rz), frac);
    let sum = x.magnitude + y.magnitude + z.magnitude;
    let (rotation, retried) = if sum > EULER_FLIP_THRESHOLD {
        let rx = lerp_battle_angle(x.next_unwrapped + 0x800, x.cur_unwrapped, frac).value;
        let ry = lerp_battle_angle(-(y.next_unwrapped + 0x800), y.cur_unwrapped, frac).value;
        let rz = lerp_battle_angle(z.next_unwrapped + 0x800, z.cur_unwrapped, frac).value;
        ([rx, ry, rz], true)
    } else {
        ([x.value, y.value, z.value], false)
    };
    BlendedPart {
        translation: [
            lerp_t(cur.tx, next.tx, 0),
            lerp_t(cur.ty, next.ty, 0),
            lerp_t(cur.tz, next.tz, z_bias),
        ],
        rotation,
        retried,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn part(t: [i16; 3], r: [u16; 3]) -> PartPose {
        PartPose {
            tx: t[0],
            ty: t[1],
            tz: t[2],
            rx: r[0],
            ry: r[1],
            rz: r[2],
        }
    }

    #[test]
    fn a_zero_nibble_is_the_plain_decode() {
        let cur = part([-5, 7, 100], [0xFFF, 0x800, 3]);
        let next = part([900, -900, 0], [0x7FF, 0, 0x900]);
        let b = blend_part_pose(cur, next, 0, 55);
        assert_eq!(b.translation, [-5, 7, 100]);
        assert_eq!(b.rotation, [0xFFF, 0x800, 3]);
        assert!(!b.retried);
    }

    #[test]
    fn translations_lerp_with_an_arithmetic_shift() {
        let cur = part([0, 10, 0], [0; 3]);
        let next = part([-1, 0, 16], [0; 3]);
        let b = blend_part_pose(cur, next, 8, 0);
        // (-1 * 8) >> 4 = -1 (sra rounds toward minus infinity), not 0.
        assert_eq!(b.translation, [-1, 5, 8]);
    }

    #[test]
    fn the_z_bias_joins_the_z_delta_before_the_multiply() {
        let cur = part([0, 0, 0], [0; 3]);
        let next = part([0, 0, 0], [0; 3]);
        assert_eq!(blend_part_pose(cur, next, 8, 32).translation, [0, 0, 16]);
    }

    #[test]
    fn angles_take_the_short_arc_across_zero() {
        let a = lerp_battle_angle(0x010, 0xFF0, 8);
        assert_eq!(a.value, 0x000);
        assert_eq!(a.magnitude, 0x20);
        let b = lerp_battle_angle(0xFF0, 0x010, 8);
        assert_eq!(b.value, 0x000);
    }

    #[test]
    fn half_a_turn_bumps_nothing_in_either_direction() {
        // `> 0x800`, not `>= 0x800`: neither guard fires at exactly half a
        // turn, so +0x800 runs forward and -0x800 runs backward.
        let f = lerp_battle_angle(0x800, 0x000, 8);
        assert_eq!((f.next_unwrapped, f.cur_unwrapped), (0x800, 0x000));
        assert_eq!(f.value, 0x400);
        let b = lerp_battle_angle(0x000, 0x800, 8);
        assert_eq!((b.next_unwrapped, b.cur_unwrapped), (0x000, 0x800));
        assert_eq!(b.value, 0x400);
        // One past half a turn wraps.
        let w = lerp_battle_angle(0x801, 0x000, 8);
        assert_eq!(w.cur_unwrapped, 0x1000);
        assert_eq!(w.magnitude, 0x7FF);
    }

    #[test]
    fn a_long_summed_step_retries_against_the_equivalent_triple() {
        // Each axis steps 0x500 (sum 0xF00 > 0xC00). The equivalent triple of
        // next = (0x500, 0x500, 0x500) is (0xD00, 0x300, 0xD00), whose steps
        // from cur = 0 are 0x300 / 0x300 / 0x300.
        let cur = part([0; 3], [0, 0, 0]);
        let next = part([0; 3], [0x500, 0x500, 0x500]);
        let b = blend_part_pose(cur, next, 8, 0);
        assert!(b.retried);
        assert_eq!(b.rotation, [0xE80, 0x180, 0xE80]);
    }

    #[test]
    fn the_threshold_is_exclusive() {
        let cur = part([0; 3], [0, 0, 0]);
        let at = part([0; 3], [0x400, 0x400, 0x400]);
        assert!(!blend_part_pose(cur, at, 8, 0).retried);
        let past = part([0; 3], [0x401, 0x400, 0x400]);
        assert!(blend_part_pose(cur, past, 8, 0).retried);
    }
}
