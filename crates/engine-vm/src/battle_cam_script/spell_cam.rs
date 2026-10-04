//! The cast-effect driver's camera script - `FUN_801DC0A0`.
//!
//! A monster spell (and any cast below the player Seru block) is filmed by
//! `FUN_801DC0A0`, not by `FUN_801D5854`: the magic band's `0x2A` / `0x2B` /
//! `0x2C` / `0x2D` arms call it every pass with a byte off the caster's
//! action queue and call no framing case of their own, so whatever it arms
//! is the camera. Its prologue advances `ctx[+0x26E]` / `ctx[+0x87C]` on
//! `FUN_801D5854`'s law, then a 20-way jump table (`0x801CECAC`, PROT 0898)
//! picks a shot it hands to the tween builder `FUN_801D829C`. Case `0x12`
//! is the summon close-up ([`summon_cast_framing`]); this module ports the
//! other nineteen.
//!
//! **The selector is a state machine.** Every case may rewrite the very
//! queue byte it was called with (`s3 = actor + 0x1DF + ctx[+0x15]`, the
//! byte the next pass reads back): the spin-in shots `3` / `5` hand on to
//! `4` once the move-FX counter `ctx[+0x6C6]` runs below `0x21`; the
//! caster close-ups `7` / `0xA` / `0xC` / `0xE` / `0x13` hand on to the
//! projectile shot `8` (`0xB` / `9` for a group target, `0xF` for a monster
//! whose first magic slot is `0x3A`) the frame an effect child is live
//! (`ctx[+0x24D] != 0`). So a cast's camera follows its effect without the
//! action SM knowing.
//!
//! What the monster pick stages: `FUN_801E9FD4` writes the spell id at
//! `+0x1DF`, the cast clip at `+0x1E0` and the opening shot at `+0x1E1` -
//! case `7` for an id below `0x25`, otherwise the byte at
//! `0x801F66D8 + id - 0x25` (`0x801EA548..0x801EA574`), then `0xFF`
//! (`legaia_asset::spell_anim_pairs::SpellAnimPairs::opening_shot`).
//! `battle_gimard_tail_fire_a` holds `[0x27, 8, 8, 0xFF]`: the table's `7`
//! for Tail Fire, already rewritten to `8` by the live flame, and retail's
//! camera reads case 8 exactly - pitch `0x40`, `TR (0, 0x400, 0x800)`, yaw
//! `0x200 - facing`, focus the flame at `ctx[+0x1144]`.
//!
//! Provenance: `see ghidra/scripts/funcs/overlay_battle_action_801dc0a0.txt`
//! (PROT 0898 at base `0x801CE818`).
//!
//! REF: FUN_801D829C (the tween builder every arm hands its shot to)
//! REF: FUN_801DCEAC (the group centroid + extent, [`crate::battle_target_group`])
//! REF: FUN_801F0348 (the `ctx[+0x6D0]` depth the `0xA` / `0xC` / `0xE` /
//! `0x11` / `0x13` arms refresh before reading it)

use super::*;

/// The magic-band states whose pass calls `FUN_801DC0A0` and no
/// `FUN_801D5854` case: `0x2A` (`0x801E47F0`), `0x2B` (`0x801E4834`),
/// `0x2C` (`0x801E487C`) and `0x2D` (while `ctx[+0x24D] != 0`). A pass of one
/// that makes no call leaves the last armed tween to land.
pub const SPELL_CAM_STATES: std::ops::RangeInclusive<u8> = 0x2A..=0x2E;

/// `DAT_1F800393`, the frame step in vsyncs, as the camera runs it: one
/// camera step is one two-vsync game frame.
pub const SPELL_CAM_FRAME_STEP: u8 = 2;

/// The projectile shot every caster close-up hands on to once an effect
/// child is live.
pub const SPELL_CAM_PROJECTILE_CASE: u8 = 8;

/// What the case's target byte `actor[+0x1DD]` resolves to.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum SpellCamTarget {
    /// A slot code (`< 8`): the target's live pair `+0x34` / `+0x38` and its
    /// body radius `actor[+0x22C][+0x58]`.
    Slot { world: [f32; 2], radius: i32 },
    /// A group code: `FUN_801DCEAC`'s centroid, as a world position (retail
    /// stores it negated), and its extent - the larger horizontal span,
    /// floored at `0x400`.
    Group { centroid: [f32; 2], extent: i32 },
    /// No resolvable target (an all-dead group, a missing slot).
    #[default]
    None,
}

/// Everything one call of `FUN_801DC0A0` reads besides its two arguments'
/// actor record.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct SpellCamInputs {
    /// The case byte the action SM passed.
    pub case: u8,
    /// The caster: facing `+0x46` and live pair `+0x34` / `+0x38`.
    pub actor: BattleCamActor,
    /// `DAT_8007BD10[seat]` - case `0`'s per-character yaw offset.
    pub char_id: u8,
    /// The caster sits in a monster slot (retail seat `>= 3`).
    pub monster_seat: bool,
    /// The monster record's first magic slot `+0x21` reads `0x3A` (case
    /// `7`'s `0xF` hand-off, `0x801DC7A0..0x801DC7C4`).
    pub first_magic_3a: bool,
    /// The target byte `+0x1DD` is a group code (`>= 8`).
    pub group_target: bool,
    pub target: SpellCamTarget,
    /// `ctx[+0x1144]` / `ctx[+0x1148]` - the effect slot's position (the
    /// launch point the move-power terminator seeds; the flight steps it).
    pub fx_position: [f32; 2],
    /// `_DAT_8007B792`, the live yaw the spin shots step from.
    pub live_yaw: f32,
    /// `DAT_1F800393`, the frame step in vsyncs.
    pub frame_step: u8,
    /// `ctx[+0x6C6]` - the move-FX counter.
    pub fx_timer: i16,
    /// `ctx[+0x24D]` - the live effect-child count.
    pub fx_children: u8,
    /// `ctx[+0x24E]` - effect slot 0's phase byte.
    pub fx_phase: u8,
    /// `ctx[+0x87C]` after this call's prologue advance.
    pub accum: u32,
    /// `ctx[+0x6D0]` as `FUN_801F0348` leaves it.
    pub depth_raw: i32,
    /// `actor[+0x22C][+0x68]` - the anim cursor in sixteenths of a keyframe.
    pub anim_cursor: i16,
    /// `actor[+0x21B]` - the hit-count bound.
    pub hit_bound: u8,
    /// `actor[+0x1D9]` - the current anim.
    pub current_anim: u8,
}

/// The shot one call arms: the target pose (TR.z prescaled), its raw
/// eye-space depth, and the tween duration in display frames.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpellCamShot {
    pub pose: BattleCamPose,
    pub raw_z: i32,
    pub frames: u32,
}

/// Everything one call decided.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct SpellCamStep {
    /// The shot handed to `FUN_801D829C`, or `None` (case `0xD`'s phase-3
    /// arm and every byte `>= 0x14` arm nothing).
    pub shot: Option<SpellCamShot>,
    /// The value written back over the queue byte, when the case hands on.
    pub next_case: Option<u8>,
    /// `actor[+0x21B] = 0; actor[+0x176] = 0` (cases `6` and `0xD`).
    pub clear_hit_bound: bool,
    /// `ctx[+0x24C] = 0xFD` (case `1`).
    pub hit_counter_fd: bool,
    /// `(actor[+0x21C], actor[+0x21F])` (case `0x13`).
    pub render: Option<(u8, u8)>,
}

/// `trunc(28 * extent / 10) + 0x800` - the group shots' depth
/// (`mult 0x66666667` / `sra 2` at `0x801DC960..0x801DC988`).
fn group_depth(extent: i32) -> i32 {
    let v = i32::from(extent as i16) * 28;
    v / 10 + 0x800
}

/// `trunc(2 * depth / 3)` (`mult 0x55555556` over `depth << 1`).
fn two_thirds(depth: i32) -> i32 {
    (i32::from(depth as i16) * 2) / 3
}

fn bearing(target: [f32; 2], actor: [f32; 3]) -> i32 {
    i32::from(crate::battle_action::bearing_12bit_approx(
        target[1] as i16,
        target[0] as i16,
        actor[2] as i16,
        actor[0] as i16,
    ))
}

/// One call of `FUN_801DC0A0`, minus its prologue (the camera owns the
/// `ctx[+0x26E]` / `ctx[+0x87C]` advance) and minus case `0x12`.
///
/// PORT: FUN_801DC0A0 (cases `0x00..=0x11`, `0x13`)
pub fn spell_cam_case(i: &SpellCamInputs) -> SpellCamStep {
    let facing = i.actor.facing;
    let me = [i.actor.world[0], i.actor.world[2]];
    let step = i32::from(i.frame_step);
    let live_yaw = i.live_yaw as i32;
    let mut out = SpellCamStep::default();
    // pitch, yaw, TR (x, y, raw z), focus, duration.
    let shot = |pitch: i32, yaw: i32, tr: [i32; 3], focus: [f32; 2], frames: u32| SpellCamShot {
        pose: BattleCamPose {
            pitch: pitch as i16 as f32,
            yaw: (yaw as i16 as i32).rem_euclid(4096) as f32,
            tr: [
                tr[0] as i16 as f32,
                tr[1] as i16 as f32,
                prescale_tr_z(i32::from(tr[2] as i16)),
            ],
            focus: [focus[0], 0.0, focus[1]],
        },
        raw_z: i32::from(tr[2] as i16),
        frames,
    };
    // A group arm: `FUN_801DCEAC`'s centroid and extent, the bearing from
    // the caster to it, pitch `pitch`, `TR (0, 0x500, 2.8 * extent + 0x800)`.
    // Cases `9` / `0xB` call it whatever the code, and a slot code decodes to
    // a one-slot group: the slot itself, at the floored extent.
    let group = |pitch: i32, yaw_add: i32, frames: u32| -> Option<SpellCamShot> {
        let (centroid, extent) = match i.target {
            SpellCamTarget::Group { centroid, extent } => (centroid, extent),
            SpellCamTarget::Slot { world, .. } => (
                world,
                i32::from(crate::battle_target_group::MIN_GROUP_EXTENT),
            ),
            SpellCamTarget::None => return None,
        };
        let yaw = (yaw_add - bearing(centroid, i.actor.world)) & 0xFFF;
        Some(shot(
            pitch,
            yaw,
            [0, 0x500, group_depth(extent)],
            centroid,
            frames,
        ))
    };
    match i.case {
        // `0x801DC204`: the caster from beside, per character.
        0x00 => {
            let add = match i.char_id {
                2 => 0x500,
                1 | 3 => 0x600,
                _ => 0,
            };
            out.shot = Some(shot(0x20, add - facing, [0, 0x480, 0x580], me, 3));
        }
        // `0x801DC2A0`: over the caster, then hand to `2`.
        0x01 => {
            out.shot = Some(shot(0x80, -facing - 0x200, [0, 0x400, 0xA00], me, 1));
            if i.hit_bound < 0xFE {
                out.next_case = Some(2);
                out.hit_counter_fd = true;
            }
        }
        // `0x801DC320`: the target, from the caster's side.
        0x02 => match i.target {
            SpellCamTarget::Slot { world, radius } => {
                let yaw = (-bearing(world, i.actor.world) - 0x200) & 0xFFF;
                out.shot = Some(shot(0, yaw, [0, 0x500, radius * 4], world, 1));
            }
            _ => out.shot = group(0x20, 0, 1),
        },
        // `0x801DC3F4`: spin on the caster-effect midpoint, then `4`.
        0x03 => {
            let mid = [
                ((i.fx_position[0] as i32 + me[0] as i32) >> 1) as f32,
                ((i.fx_position[1] as i32 + me[1] as i32) >> 1) as f32,
            ];
            out.shot = Some(shot(0x80, live_yaw - step * 8, [0, 0x480, 0x800], mid, 1));
            if i.fx_timer < 0x21 {
                out.next_case = Some(4);
            }
        }
        // `0x801DC46C`: the target from high up.
        0x04 => match i.target {
            SpellCamTarget::Slot { world, radius } => {
                let yaw = (0x780 - bearing(world, i.actor.world)) & 0xFFF;
                let z = (i32::from(radius as i16) * 20) / 3;
                out.shot = Some(shot(0x320, yaw, [0, 0x500, z], world, 1));
            }
            _ => out.shot = group(0x40, 0x700, 1),
        },
        // `0x801DC578`: a wide spin on the effect, then `4`.
        0x05 => {
            out.shot = Some(shot(
                0x80,
                live_yaw + step * 16,
                [0, 0x480, 0xE00],
                i.fx_position,
                1,
            ));
            if i.fx_timer < 0x21 {
                out.next_case = Some(4);
            }
        }
        // `0x801DC604`: behind the caster, pulling in with the counter.
        0x06 => {
            let t = i32::from(i.fx_timer);
            let z = if t != 0 {
                (0x3C0 - t) * 4 + 0x200
            } else {
                0x200
            };
            let y = if t < 0x200 && i.fx_children != 0 {
                0x880 - t * 2
            } else {
                0x480
            };
            out.shot = Some(shot(0x40, 0x800 - facing, [0, y, z], me, 1));
            if t < 0x21 && i.hit_bound != 0xFF {
                out.next_case = Some(2);
                out.clear_hit_bound = true;
            }
        }
        // `0x801DC6F0`: the caster close-up; the effect's launch hands on.
        0x07 => {
            let z = (i.accum as u16 as i32) + two_thirds(i.depth_raw);
            out.shot = Some(shot(0x40, 0x700 - facing, [0, 0x400, z], me, 1));
            if i.fx_children != 0 {
                let mut next = SPELL_CAM_PROJECTILE_CASE;
                if i.monster_seat && i.first_magic_3a {
                    next = 0xF;
                }
                if i.group_target {
                    next = 9;
                }
                out.next_case = Some(next);
            }
        }
        // `0x801DC7D4`: the projectile.
        0x08 => {
            out.shot = Some(shot(
                0x40,
                (0x200 - facing) & 0xFFF,
                [0, 0x400, 0x800],
                i.fx_position,
                6,
            ));
        }
        // `0x801DC82C`: the group, from the caster's side.
        0x09 => out.shot = group(0x40, 0x900, 6),
        // `0x801DC898`: behind the caster, swinging with its clip.
        0x0A => {
            let yaw = i32::from(i.anim_cursor) * 2 + 0x700 - facing;
            let z = (i32::from(i.depth_raw as i16) * 5) >> 2;
            out.shot = Some(shot(0x40, yaw, [0, 0x400, z], me, 6));
            if i.fx_children != 0 {
                out.next_case = Some(if i.group_target {
                    0x0B
                } else {
                    SPELL_CAM_PROJECTILE_CASE
                });
            }
        }
        // `0x801DC8F8`: the group, a quarter-turn round.
        0x0B => out.shot = group(0x40, 0x100, 6),
        // `0x801DC99C`: a slow close-up on the caster.
        0x0C => {
            let z = two_thirds(i.depth_raw);
            out.shot = Some(shot(0x40, 0x400 - facing, [0, 0x400, z], me, 0x1E));
            if i.fx_children != 0 {
                out.next_case = Some(if i.group_target {
                    9
                } else {
                    SPELL_CAM_PROJECTILE_CASE
                });
            }
        }
        // `0x801DCA48`: offset over the caster, pulled by the effect phase.
        0x0D => {
            if i.fx_phase == 3 {
                out.next_case = Some(2);
                out.clear_hit_bound = true;
            } else {
                let t = i32::from(i.fx_timer);
                let z = if i.fx_phase == 1 {
                    if t != 0 {
                        (0x100 - t) * 4 + 0xA00
                    } else {
                        0xE00
                    }
                } else {
                    0xA00
                };
                out.shot = Some(shot(0x40, 0x780 - facing, [0x200, 0x600, z], me, 1));
            }
        }
        // `0x801DCB18`: low on the caster, turning.
        0x0E => {
            out.shot = Some(shot(
                -0x10,
                live_yaw + step * 4,
                [0, 0x300, i32::from(i.depth_raw as i16)],
                me,
                1,
            ));
            if i.fx_children != 0 {
                out.next_case = Some(if i.group_target {
                    0x0B
                } else {
                    SPELL_CAM_PROJECTILE_CASE
                });
            }
        }
        // `0x801DCBB4`: the projectile, from above.
        0x0F => {
            out.shot = Some(shot(
                0xC0,
                (0x200 - facing) & 0xFFF,
                [0, 0x400, 0x800],
                i.fx_position,
                6,
            ));
        }
        // `0x801DCC0C`: a fast spin on the caster, then `0x11`.
        0x10 => {
            out.shot = Some(shot(0x80, live_yaw - step * 16, [0, 0x400, 0x900], me, 6));
            if i.fx_children != 0 {
                out.next_case = Some(0x11);
            }
        }
        // `0x801DCC90`: the projectile at the caster's depth.
        0x11 => {
            out.shot = Some(shot(
                0x40,
                (0x200 - facing) & 0xFFF,
                [0, 0x400, i32::from(i.depth_raw as i16)],
                i.fx_position,
                6,
            ));
        }
        // `0x801DCDA0`: case 7's twin from behind, raising the caster's
        // render state through its clip, then `0xF`.
        0x13 => {
            let z = (i.accum as u16 as i32) + two_thirds(i.depth_raw);
            out.shot = Some(shot(0x40, 0x800 - facing, [0, 0x400, z], me, 1));
            if i.current_anim != 0 && i.anim_cursor >= 0xC0 {
                out.render = Some((4, 2));
            }
            if i.fx_children != 0 {
                out.next_case = Some(0xF);
                out.render = Some((0, 0));
            }
        }
        // `0x12` is the summon close-up ([`summon_cast_framing`]); `>= 0x14`
        // falls out of the jump table's bound with only the prologue run.
        _ => {}
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `battle_gimard_tail_fire_a`: Gimard (facing `2020`) in `0x2B` on case
    /// 8 with the flame at `ctx[+0x1144] = (125, -645)`; the capture's tween
    /// table holds pitch `64`, yaw `2588`, `TR (0, 1024, 3276)`, focus the
    /// flame.
    #[test]
    fn case_8_is_the_tail_fire_capture() {
        let i = SpellCamInputs {
            case: 8,
            actor: BattleCamActor {
                facing: 2020,
                world: [114.0, 0.0, -398.0],
                height: None,
            },
            fx_position: [125.0, -645.0],
            ..Default::default()
        };
        let s = spell_cam_case(&i).shot.expect("case 8 arms a shot");
        assert_eq!(s.pose.pitch, 64.0);
        assert_eq!(s.pose.yaw, 2588.0);
        assert_eq!(s.pose.tr, [0.0, 1024.0, 3276.0]);
        assert_eq!(s.pose.focus, [125.0, 0.0, -645.0]);
        assert_eq!(s.frames, 6);
        assert_eq!(spell_cam_case(&i).next_case, None);
    }

    /// Case 7 is the caster close-up until an effect child is live, then
    /// hands on to the projectile (`9` for a group, `0xF` for a monster whose
    /// first magic is `0x3A`).
    #[test]
    fn case_7_hands_on_when_the_effect_launches() {
        let mut i = SpellCamInputs {
            case: 7,
            depth_raw: 0xC00,
            accum: 432,
            ..Default::default()
        };
        let s = spell_cam_case(&i);
        assert_eq!(s.next_case, None);
        assert_eq!(s.shot.unwrap().raw_z, 432 + 0x800);
        i.fx_children = 1;
        assert_eq!(spell_cam_case(&i).next_case, Some(8));
        i.monster_seat = true;
        i.first_magic_3a = true;
        assert_eq!(spell_cam_case(&i).next_case, Some(0xF));
        i.group_target = true;
        assert_eq!(spell_cam_case(&i).next_case, Some(9));
    }

    /// The group shots size their depth off `FUN_801DCEAC`'s extent and a
    /// slot code decodes to a one-slot group at the floored extent.
    #[test]
    fn group_depth_is_two_point_eight_extents() {
        let mut i = SpellCamInputs {
            case: 9,
            target: SpellCamTarget::Group {
                centroid: [0.0, 800.0],
                extent: 0x500,
            },
            ..Default::default()
        };
        assert_eq!(
            spell_cam_case(&i).shot.unwrap().raw_z,
            0x500 * 28 / 10 + 0x800
        );
        i.target = SpellCamTarget::Slot {
            world: [0.0, 800.0],
            radius: 0,
        };
        assert_eq!(
            spell_cam_case(&i).shot.unwrap().raw_z,
            0x400 * 28 / 10 + 0x800
        );
        assert_eq!(
            spell_cam_case(&SpellCamInputs { case: 0x14, ..i }).shot,
            None
        );
    }
}
