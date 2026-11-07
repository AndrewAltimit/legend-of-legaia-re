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
//! is assumed constant over the two passes. Neither is recorded in RAM. The
//! older step's target is recomposed on the composer's own floor under the
//! previous pass's player where the engine's seat supplies it
//! ([`WalkHistory::floors`]): `kor_field_route_walk`'s walk film shows that
//! pass's staging eye Y on the floor's tier while the footing still trails.
//!
//! A capture saved inside the actor pass reads a still pair over a walking
//! pad ([`mid_pass_walk_step`]). `tower_field_walk` holds `+0x14/+0x18` equal
//! to `+0x1C/+0x20`, with `-X +Z` held: its two framebuffers stand `(5, 3)` px
//! apart and a frame seated on the RAM's player is `(9, 6)` px off the
//! displayed one, two walk steps.
//!
//! The player is rewound whether or not the camera is still easing. A
//! follow camera already on its staging pose (`dohaty_field_card_walk`,
//! `jou_field_card_walk`: a yaw-`0` block whose target does not turn) keeps
//! its live angles and eye, but its focus is the player, so the frame on
//! screen still stands two walk steps behind the RAM: `dohaty`'s retail
//! frame sits `13` px left of a frame seated on the RAM's player, and the
//! state's own newest ordering table, rasterised (`ot-raster.py`), sits on
//! the RAM's pose.

use super::*;
use legaia_engine_field::camera_zone::{CameraTarget, CameraZoneConfig, ComposeInputs, compose};

/// The follow ease's staging descriptor (`FUN_801DB510`'s `a1`).
const FOLLOW_STAGING: u32 = 0x801F_3580;
/// The ease-shift byte `B60B` of the camera block (`0x801DB5C4`).
const EASE_SHIFT_BYTE: u32 = 0x8007_B60B;
/// The held pad word the pad remap rewrites (`_DAT_8007B850`).
const HELD_PAD: u32 = 0x8007_B850;
/// The controller's last committed step deltas.
const STEP_DELTA_X: u32 = 0x8007_BDE0;
const STEP_DELTA_Z: u32 = 0x8007_BDE4;
/// The party leader's character id (`_DAT_8007B8F8`).
const PARTY_LEADER: u32 = 0x8007_B8F8;

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
    /// The composer's floor under [`Self::cur`] and [`Self::prev`]:
    /// `FUN_80019278` through the MAN-header ladder, as `FUN_801DAB90` reads
    /// it (`World::sample_field_floor_height_static`). The RAM holds no
    /// such sample, so the engine's seat supplies it
    /// ([`Self::with_floors`]); without it the footing stands in.
    pub floors: Option<[i32; 2]>,
}

/// The walk step of a capture saved inside the actor pass: the iterator
/// `FUN_8002519C` has copied `+0x14/+0x18` to `+0x1C/+0x20` for this pass
/// (`0x800251D4..0x800251F0`) and the player's own tick has not run, so the
/// pair reads still while the pad is walking it. It shows as a held direction
/// (`_DAT_8007B850`, camera-remapped by then) on a moving clip with the step
/// deltas `_DAT_8007BDE0` / `_DAT_8007BDE4` left up, where a wall press clears
/// them. The step itself is the controller's: `(base * +0x72 >> 12)` times the
/// frame step, x0.75 on a diagonal under camera mode 4 (`FUN_801D01B0`).
/// `None` for a player the pad is not walking.
fn mid_pass_walk_step(ram: &[u8], p: u32, block: Option<&CameraZoneConfig>) -> Option<[i16; 2]> {
    use legaia_engine_field::field_player_clip::{BANK_STRIDE, BASE_RUN, BASE_WALK};
    let pad = game_anchors::u32_at(ram, HELD_PAD);
    if rd16(ram, STEP_DELTA_X) == 0 && rd16(ram, STEP_DELTA_Z) == 0 {
        return None;
    }
    let leader = game_anchors::u16_at(ram, PARTY_LEADER);
    let base = game_anchors::u16_at(ram, p + 0x5C).checked_sub(leader * BANK_STRIDE)?;
    let base_step = match base {
        BASE_WALK => 8,
        BASE_RUN => 0xC,
        _ => return None,
    };
    let dir = |plus: u32, minus: u32| match (pad & plus != 0, pad & minus != 0) {
        (true, false) => 1,
        (false, true) => -1,
        _ => 0,
    };
    let (sx, sz) = (dir(0x2000, 0x8000), dir(0x1000, 0x4000));
    if sx == 0 && sz == 0 {
        return None;
    }
    let mult = i32::from(game_anchors::u16_at(ram, p + 0x72));
    let mut speed =
        ((base_step * mult) >> 12) * i32::from(crate::retail_compare_battle::frame_step(ram));
    if sx != 0 && sz != 0 && block.is_some_and(|b| b.mode >> 4 == 4) {
        speed -= speed >> 2;
    }
    Some([(sx * speed) as i16, (sz * speed) as i16])
}

/// The staging pose at [`FOLLOW_STAGING`] as a camera observation, on `live`'s
/// focus: the staging focus is the player, and the focus pair has its own
/// seat and gate.
pub(super) fn follow_staging_pose(ram: &[u8], live: &CameraObs) -> CameraObs {
    let st = FOLLOW_STAGING;
    CameraObs {
        pitch: rd16(ram, st + 0x02),
        yaw: rd16(ram, st + 0x06),
        h: rd16(ram, st + 0x26),
        eye: [
            i32::from(rd16(ram, st + 0x0E)),
            i32::from(rd16(ram, st + 0x12)),
            i32::from(rd16(ram, st + 0x16)),
        ],
        focus: live.focus,
    }
}

/// The [`WalkHistory`] of a capture whose player moved on its last pass;
/// `None` for a player who stood still on it.
pub(super) fn walk_history(ram: &[u8], camera: &CameraObs) -> Option<WalkHistory> {
    let p = game_anchors::player_ptr(ram)?;
    let at = |o: u32| [rd16(ram, p + o), rd16(ram, p + o + 2), rd16(ram, p + o + 4)];
    let (cur, mut prev) = (at(0x14), at(0x1C));
    let block = camera_block_bytes(ram).and_then(|b| CameraZoneConfig::from_retail_block(&b));
    if [cur[0], cur[2]] == [prev[0], prev[2]] {
        let [dx, dz] = mid_pass_walk_step(ram, p, block.as_ref())?;
        prev[0] = cur[0].wrapping_sub(dx);
        prev[2] = cur[2].wrapping_sub(dz);
    }
    let code = legaia_engine_field::camera_zone::EASE_SHIFT_TABLE
        [usize::from(game_anchors::u8_at(ram, EASE_SHIFT_BYTE) >> 4)];
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
    Some(WalkHistory {
        cur,
        prev,
        code,
        staging,
        angles,
        eye: camera.eye,
        block,
        half_eye_y: game_anchors::u32_at(ram, SAVE_ALLOW) != 0,
        floors: None,
    })
}

impl WalkHistory {
    /// The same history with the composer's floor under both passes.
    pub fn with_floors(mut self, floors: [i32; 2]) -> Self {
        self.floors = Some(floors);
        self
    }

    /// The composer's target for the player standing at `player` over
    /// `floor` (the footing where no floor sample is carried).
    fn compose_at(
        &self,
        block: &CameraZoneConfig,
        attr_box: [u8; 4],
        player: [i16; 3],
        floor: Option<i32>,
    ) -> CameraTarget {
        compose(
            block,
            &ComposeInputs {
                player: player.map(i32::from),
                floor_y: floor.unwrap_or(i32::from(player[1])),
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
            .filter(|&(b, a)| {
                self.compose_at(&b, a, self.cur, self.floors.map(|f| f[0])) == self.staging
            })
            .map(|(b, a)| self.compose_at(&b, a, self.prev, self.floors.map(|f| f[1])))
            .unwrap_or(self.staging);
        let code = self.code;
        let back = |live: [i32; 3], t: CameraTarget, eye: bool| -> [i32; 3] {
            // A snap (code `0`) leaves no ease step to undo: the live pose is
            // the newest target, the nearest the RAM holds to the older one.
            if code == 0 {
                live
            } else if eye {
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

    fn history(code: u8, on_target: bool) -> WalkHistory {
        let staging = CameraTarget {
            pitch: 744,
            yaw: 0,
            eye: [-87, 555, 11200],
            h: 512,
        };
        WalkHistory {
            cur: [9106, 0, 6958],
            prev: [9130, 0, 6958],
            code,
            staging,
            angles: if on_target {
                [744, 0, 512]
            } else {
                [744, 40, 512]
            },
            eye: [-87, 555, 11200],
            block: None,
            half_eye_y: false,
            floors: None,
        }
    }

    #[test]
    fn a_camera_on_its_target_still_rewinds_the_walk() {
        // `dohaty_field_card_walk`'s pair: `-24` X a pass, the camera on its
        // staging pose.
        let r = history(3, true).rewind(None);
        assert_eq!(r.seat, [9154, 6958]);
        assert_eq!(r.angles, [744, 0, 512]);
        assert_eq!(r.eye, [-87, 555, 11200]);
    }

    #[test]
    fn a_snapping_ease_keeps_the_live_camera() {
        let r = history(0, false).rewind(None);
        assert_eq!(r.seat, [9154, 6958]);
        assert_eq!(r.angles, [744, 40, 512]);
    }

    #[test]
    fn a_value_on_its_target_has_no_step_back() {
        assert_eq!(ease_step_back(256, 256, 2), 256);
    }
}
