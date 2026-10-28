//! The frame a capture shows while the pad walks the player: two passes
//! behind the RAM, under a follow camera two ease steps short of the pose
//! the RAM holds.
//!
//! The screen scans out the table the GPU drew the pass before the newest
//! (the two-pass law, [`crate::retail_compare_battle::display_lag_vsyncs`]).
//! On a pass the player moved, the follow ease `FUN_801DB510` steps the six
//! descriptor-list globals once toward the staging pose the composer just
//! wrote at `0x801F3580` (`0x801DB5AC..0x801DB724`). So a capture whose
//! player moved on its last pass (`+0x14/+0x18` against the previous-pass
//! pair `+0x1C/+0x20`, the ease's own stationary test at
//! `0x801DB578..0x801DB5A4`) has its RAM camera two steps past the frame on
//! screen, and its player one walk step past `+0x1C/+0x20`. A seat on the
//! RAM's own pose frames a scene shifted by those two steps -
//! `kor3_field_route_walk`'s yaw `511` stands `87` units past the frame's.
//!
//! The rewind takes each global back two ease steps against the staging
//! target and stands the player one more walk step back. Both are
//! **inference**: the staging target is the newest pass's, and the walk step
//! is assumed constant over the two passes. Neither is recorded in RAM.

use super::*;
use legaia_engine_field::camera_zone::{CameraTarget, CameraZoneConfig, ComposeInputs, compose};

/// The follow ease's staging descriptor (`FUN_801DB510`'s `a1`).
const FOLLOW_STAGING: u32 = 0x801F_3580;
/// The ease-shift byte `B60B` of the camera block (`0x801DB5C4`).
const EASE_SHIFT_BYTE: u32 = 0x8007_B60B;

/// The player and follow-camera pose the displayed frame was drawn under.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WalkRewind {
    /// Player `(X, Z)` two passes back.
    pub seat: [i16; 2],
    /// Eye translation `0x800840B8/BC/C0` two ease steps back.
    pub eye: [i32; 3],
    /// Pitch / yaw / `H` two ease steps back.
    pub angles: [i32; 3],
}

/// The live value one ease step before `live`: the `v` nearest `live` with
/// `ease_step(v, target, code) == live`, or `live` itself when no value
/// steps there (a value already on its target, or a step that overshoots).
pub fn ease_step_back(live: i32, target: i32, code: u8) -> i32 {
    use legaia_engine_field::camera_zone::ease_step;
    if live == target {
        return live;
    }
    // The ease moves `live` toward `target`, so the previous value lies on
    // the far side of `live`; one step covers at most the whole gap.
    let away = if target > live { -1 } else { 1 };
    (1..=0x2_0000)
        .map(|k| live + away * k)
        .find(|&v| ease_step(v, target, code) == live)
        .unwrap_or(live)
}

/// [`ease_step_back`] for a halfword global (pitch / yaw / `H`), whose ease
/// reads both operands with `lh` and stores the result with `sh`.
fn ease_step_back_i16(live: i32, target: i16, code: u8) -> i32 {
    i32::from(ease_step_back(i32::from(live as i16), i32::from(target), code) as i16)
}

/// What a walking capture's RAM holds of its last two passes: the player
/// now and on the pass before, the follow camera now, the staging target
/// the newest pass eased it toward, and the block that composed it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WalkHistory {
    /// Player `(X, footing, Z)` now (`+0x14/+0x16/+0x18`).
    pub cur: [i16; 3],
    /// The same on the previous pass (`+0x1C/+0x1E/+0x20`).
    pub prev: [i16; 3],
    /// The ease shift code (`EASE_SHIFT_TABLE[B60B >> 4]`).
    pub code: u8,
    /// The staging pose at `0x801F3580`.
    pub staging: CameraTarget,
    /// Live pitch / yaw / `H`.
    pub angles: [i32; 3],
    /// Live eye translation.
    pub eye: [i32; 3],
    /// The camera block the composer read (`0x8007B607..`).
    pub block: Option<CameraZoneConfig>,
    /// `_DAT_8007B6A8 != 0`, the composer's halved eye height.
    pub half_eye_y: bool,
}

/// The [`WalkHistory`] of a capture whose player moved on its last pass under
/// a follow camera still easing; `None` otherwise (a stationary player, an
/// ease that snaps, or a camera already on its target).
pub(super) fn walk_history(ram: &[u8], camera: &CameraObs) -> Option<WalkHistory> {
    let p = game_anchors::player_ptr(ram)?;
    let at = |o: u32| [rd16(ram, p + o), rd16(ram, p + o + 2), rd16(ram, p + o + 4)];
    let (cur, prev) = (at(0x14), at(0x1C));
    if [cur[0], cur[2]] == [prev[0], prev[2]] {
        return None;
    }
    let code = legaia_engine_field::camera_zone::EASE_SHIFT_TABLE
        [usize::from(game_anchors::u8_at(ram, EASE_SHIFT_BYTE) >> 4)];
    if code == 0 {
        return None;
    }
    let st = FOLLOW_STAGING;
    let staging = CameraTarget {
        pitch: rd16(ram, st + 0x02),
        yaw: rd16(ram, st + 0x06),
        eye: [
            rd16(ram, st + 0x0E),
            rd16(ram, st + 0x12),
            rd16(ram, st + 0x16),
        ],
        h: rd16(ram, st + 0x26),
    };
    let angles = [
        i32::from(camera.pitch),
        i32::from(camera.yaw),
        i32::from(camera.h),
    ];
    let target_angles = [staging.pitch, staging.yaw, staging.h].map(i32::from);
    if angles == target_angles && camera.eye == staging.eye.map(i32::from) {
        return None;
    }
    Some(WalkHistory {
        cur,
        prev,
        code,
        staging,
        angles,
        eye: camera.eye,
        block: camera_block_bytes(ram).and_then(|b| CameraZoneConfig::from_retail_block(&b)),
        half_eye_y: game_anchors::u32_at(ram, SAVE_ALLOW) != 0,
    })
}

impl WalkHistory {
    /// The composer's target for the player standing at `player`, on the
    /// assumption that the floor under the player is its footing.
    fn compose_at(
        &self,
        block: &CameraZoneConfig,
        attr_box: [u8; 4],
        player: [i16; 3],
    ) -> CameraTarget {
        compose(
            block,
            &ComposeInputs {
                player: player.map(i32::from),
                floor_y: i32::from(player[1]),
                attr_box,
                live_pitch: self.angles[0],
                live_yaw: self.angles[1],
                half_eye_y: self.half_eye_y,
            },
        )
        .target
    }

    /// The displayed frame's pose. `attr_box` is the walk-region box at
    /// scratchpad `0x1F800384..87`. With it, and when recomposing the newest
    /// pass at the RAM's player reproduces the staging pose, the previous
    /// pass's target is recomposed at its own player position (a mode-4
    /// yaw aims from the box centre, so it turns as the player walks);
    /// otherwise both steps go back against the staging target.
    pub fn rewind(&self, attr_box: Option<[u8; 4]>) -> WalkRewind {
        let prev_target = self
            .block
            .zip(attr_box)
            .filter(|&(b, a)| self.compose_at(&b, a, self.cur) == self.staging)
            .map(|(b, a)| self.compose_at(&b, a, self.prev))
            .unwrap_or(self.staging);
        let code = self.code;
        let back = |live: [i32; 3], t: CameraTarget, eye: bool| -> [i32; 3] {
            if eye {
                [0, 1, 2].map(|i| ease_step_back(live[i], i32::from(t.eye[i]), code))
            } else {
                let tt = [t.pitch, t.yaw, t.h];
                [0, 1, 2].map(|i| ease_step_back_i16(live[i], tt[i], code))
            }
        };
        let step = |c: i16, q: i16| (2 * i32::from(q) - i32::from(c)) as i16;
        WalkRewind {
            seat: [
                step(self.cur[0], self.prev[0]),
                step(self.cur[2], self.prev[2]),
            ],
            eye: back(back(self.eye, self.staging, true), prev_target, true),
            angles: back(back(self.angles, self.staging, false), prev_target, false),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use legaia_engine_field::camera_zone::ease_step;

    #[test]
    fn a_step_back_steps_forward_onto_the_live_value() {
        // Shift code 2 (`B60B` nibble 4), the Sol Tower stairs' block.
        for (live, target) in [(731, 816), (1163, 1152), (511, 621), (1702, 1605)] {
            let prev = ease_step_back(live, target, 2);
            assert_eq!(ease_step(prev, target, 2), live, "{live} -> {target}");
            assert!((prev - target).abs() > (live - target).abs());
        }
        // Two-shift codes invert the same way.
        let prev = ease_step_back(500, 900, 0x44);
        assert_eq!(ease_step(prev, 900, 0x44), 500);
    }

    #[test]
    fn a_value_on_its_target_has_no_step_back() {
        assert_eq!(ease_step_back(256, 256, 2), 256);
    }
}
