//! Standing a battle capture's combatants on the ground the capture holds,
//! at the captured phase (`LEGAIA_BATTLE_GROUND_SNAP`).
//!
//! The drive reaches a capture's phase by replaying its action from the
//! seed's ground, and a replayed push, approach or back-off lands within some
//! units of retail's, not on it: the undrift search
//! ([`docs/tooling/retail-compare.md`](../../../docs/tooling/retail-compare.md))
//! takes most of it back, but the framings that look at the combatants (case
//! 6 on the acting seat's live pair, cases 7 / 8 on body pairs, the heading
//! every case subtracts) read what is left. The capture's RAM holds every
//! combatant's live pair `+0x34` / `+0x38` and heading `+0x46` exactly, so
//! once the phase holds, the seat stands them there, re-derives the body
//! pairs from the poses they now strike, and asks how far the camera's
//! framing law moves its endpoint for that ground
//! ([`World::battle_camera_framing_now`], [`snap_battle_ground`]). The
//! glide is aligned onto retail's tween only on the components whose
//! endpoint that framing agrees with, as before: a framing the law gets
//! wrong still reads as one.

use legaia_engine_camera::projection::BattleCamPose;
use legaia_engine_core::world::World;

/// One combatant's captured ground, by **engine** battle slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GroundSnap {
    pub slot: u8,
    /// Live pair `+0x34` / `+0x38`.
    pub x: i16,
    pub z: i16,
    /// Heading `+0x46`, when the capture read one.
    pub facing: Option<u16>,
    /// Body pair `+0x3C` / `+0x40` to stand the body on instead of the live
    /// pair, when set.
    pub body: Option<[i16; 2]>,
}

/// `slot:x:z[:facing],...`.
pub fn ground_snaps_to_env(snaps: &[GroundSnap]) -> String {
    snaps
        .iter()
        .map(|s| {
            let f = s.facing.map_or("-".to_string(), |f| f.to_string());
            match s.body {
                Some([bx, bz]) => format!("{}:{}:{}:{f}:{bx}:{bz}", s.slot, s.x, s.z),
                None => format!("{}:{}:{}:{f}", s.slot, s.x, s.z),
            }
        })
        .collect::<Vec<_>>()
        .join(",")
}

/// Parse [`ground_snaps_to_env`]'s form; malformed entries are skipped.
pub fn ground_snaps_from_env(v: &str) -> Vec<GroundSnap> {
    v.split(',')
        .filter_map(|e| {
            let mut it = e.trim().split(':');
            let slot = it.next()?.parse().ok()?;
            let x = it.next()?.parse().ok()?;
            let z = it.next()?.parse().ok()?;
            let facing = match it.next() {
                Some("-") | None => None,
                Some(f) => Some(f.parse().ok()?),
            };
            let body = match (it.next(), it.next()) {
                (Some(bx), Some(bz)) => Some([bx.parse().ok()?, bz.parse().ok()?]),
                _ => None,
            };
            Some(GroundSnap {
                slot,
                x,
                z,
                facing,
                body,
            })
        })
        .collect()
}

/// Stand each listed combatant on its captured live pair and heading (a
/// reacting body on its captured body pair), re-derive every body pair from
/// it, and return where the camera's framing heads on that ground, read off
/// a probe step of the framing law ([`World::battle_camera_framing_now`])
/// before and after the move. `None` when there is no battle camera.
pub fn snap_battle_ground(world: &mut World, snaps: &[GroundSnap]) -> Option<BattleCamPose> {
    let before = world.battle_camera_framing_now();
    for s in snaps {
        let Some(a) = world.actors.get_mut(usize::from(s.slot)) else {
            continue;
        };
        a.move_state.world_x = s.x;
        a.move_state.world_z = s.z;
        if let Some(f) = s.facing {
            a.battle.facing_angle = f & 0xFFF;
        }
    }
    world.refresh_battle_body_pairs_now();
    // A body whose pose the replay strikes at another keyframe than retail's
    // stands its centroid elsewhere for the same live pair; one given a body
    // pair is moved so its body pair lands there.
    let mut moved = false;
    for s in snaps {
        let Some([bx, bz]) = s.body else { continue };
        let Some(a) = world.actors.get_mut(usize::from(s.slot)) else {
            continue;
        };
        // Only a body the engine poses has a centroid to move: one with no
        // pose stands its body pair on its live pair.
        if a.battle_animation.is_none() || a.pose_frame.is_none() {
            continue;
        }
        let Some((ex, ez)) = a.battle.seat else {
            continue;
        };
        a.move_state.world_x = a.move_state.world_x.wrapping_add(bx.wrapping_sub(ex));
        a.move_state.world_z = a.move_state.world_z.wrapping_add(bz.wrapping_sub(ez));
        moved = true;
    }
    if moved {
        world.refresh_battle_body_pairs_now();
    }
    let after = world.battle_camera_framing_now();
    let target = world.battle.camera.as_ref()?.glide_target();
    let (Some(before), Some(after)) = (before, after) else {
        return Some(target);
    };
    let turn = |a: f32, b: f32| (a - b + 2048.0).rem_euclid(4096.0) - 2048.0;
    // Per component: a glide already heading where the law's next arm heads
    // on the replay's ground keeps its endpoint, moved by what the snap moved
    // the law's (which cancels the probe step's own counter advance); a glide
    // still on another arm's endpoint takes the law's arm on the new ground.
    let angle = |t: f32, b: f32, a: f32| {
        if turn(t, b).abs() <= PROBE_ANGLE_SLACK {
            t + turn(a, b)
        } else {
            a
        }
    };
    let linear = |t: f32, b: f32, a: f32| {
        if (t - b).abs() <= PROBE_LINEAR_SLACK {
            t + a - b
        } else {
            a
        }
    };
    Some(BattleCamPose {
        pitch: angle(target.pitch, before.pitch, after.pitch),
        yaw: angle(target.yaw, before.yaw, after.yaw),
        tr: std::array::from_fn(|k| linear(target.tr[k], before.tr[k], after.tr[k])),
        focus: std::array::from_fn(|k| linear(target.focus[k], before.focus[k], after.focus[k])),
    })
}

/// How far the glide's endpoint may stand from the probe's on an angle
/// component and still be the same arm: the per-pass yaw counter advance
/// (`max(1, 4 * step / 3)`) at the largest step, with a unit of rounding.
const PROBE_ANGLE_SLACK: f32 = 8.0;

/// The same for a translation or focus component: what a pass of pose
/// refresh moves a body pair, not a different framing.
const PROBE_LINEAR_SLACK: f32 = 16.0;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_env_form_round_trips() {
        let snaps = vec![
            GroundSnap {
                slot: 0,
                x: -195,
                z: -214,
                facing: Some(484),
                body: None,
            },
            GroundSnap {
                slot: 3,
                x: 5,
                z: 5,
                facing: None,
                body: Some([20, 20]),
            },
        ];
        let env = ground_snaps_to_env(&snaps);
        assert_eq!(env, "0:-195:-214:484,3:5:5:-:20:20");
        assert_eq!(ground_snaps_from_env(&env), snaps);
    }
}
