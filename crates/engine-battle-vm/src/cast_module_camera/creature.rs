//! The shared unit of the summon-creature directors, and PROT 0920
//! (Slippery), whose director carries its whole phase chain.
//!
//! Every summon-creature module (PROT 0914..0934) now has a director that
//! carries its tick's whole phase chain ([`ModuleProfile::director_body`]):
//! [`super::aluru`], [`super::gola_mushura_barra`],
//! [`super::gilium_palma_horn`], [`super::jedo_iota_mule`] and
//! [`super::meta_terra_ozma`], beside Slippery's below.

use super::*;

/// Vsyncs per engine tick: the unit of the delta-only drains
/// (`*(0x1F800393)` per battle frame, a battle frame being that many vsyncs).
pub const DELTA_PER_TICK: i32 = 1;

/// The PROT entry of Slippery's module (`summon_slippery`).
pub const SLIPPERY_ENTRY: u32 = 920;

/// PROT 0920's damage arm: its exit applies the rain's hit to every living
/// monster (`jal 0x801DD0AC` at `0x801F7844`), so the band's outcome lands
/// once the phase has passed it.
pub const SLIPPERY_DAMAGE_ARM: u8 = 9;

/// The arm whose exit seeds the rain budget `_DAT_8007BDC0 = 0x204`
/// (`0x801F6FEC`).
pub const SLIPPERY_RAIN_SEED_ARM: u8 = 5;

/// The arm that drains the rain budget by `2 * delta` a pass
/// (`0x801F70CC`) and pins it at `4` on its exit (`0x801F712C`).
pub const SLIPPERY_RAIN_DRAIN_ARM: u8 = 6;

/// The arm that ramps the rain colour `0x801F8444` towards red a step per
/// vsync (`0x801F7740..0x801F777C`) and pins it at `0xFF4040` on its exit
/// (`0x801F77B0`).
pub const SLIPPERY_COLOUR_ARM: u8 = 9;

/// The arm that waits out the hits and stores phase `0xFF` (`0x801F7B44`).
pub const SLIPPERY_LAST_ARM: u8 = 10;

/// PROT 0920 (Slippery, "Deadly Rain"): every arm is a gate on the module
/// countdown `0x801F8440` (drained by `scalar * delta` a pass) plus writes to
/// the camera globals, so this director carries the whole phase chain
/// ([`ModuleProfile::director_body`]):
///
/// * `0` - a cut on the caster - pitch `0`, yaw `0x800 - facing`, TR `(0,
///   0x580, 0x600)` (`0x801F6B90`) - and the countdown `= scalar << 7`;
/// * `1`, `2` - TR z out by `p` a pass while the countdown runs, arm 2
///   holding until it is spent, then seating the creature and re-arming the
///   countdown by `scalar << 6`;
/// * `3` - TR z out by `p` a pass while it runs; on exit `+= scalar << 7`;
/// * `4` - on exit a `0x20`-frame cut on the creature - pitch `0x20`, yaw
///   `0x800 - facing`, TR `(0, 0x400, 0x600)` (`0x801F6F20`) - and `+=
///   scalar << 7`;
/// * `5` - on exit `+= scalar << 8` (and the rain budget is seeded);
/// * `6` - every pass pitch `-= p >> 3`, TR y `+= p >> 2`, TR z `+= p >> 1`
///   (`0x801F7054..0x801F70B4`); on exit a cut - pitch `0`, yaw `0xC00 -
///   facing`, TR `(0xA00, 0x400, 0x600)` on the creature (`0x801F7138`) - and
///   `+= scalar << 8`;
/// * `7` - every pass TR x `-= p` (`0x801F7390`); on exit a cut - pitch `0`,
///   yaw `0x800 - facing`, TR `(0, 0x400, 0x600)` on the creature
///   (`0x801F73E4`) - and `+= scalar << 5`;
/// * `8` - every pass TR z `-= p / 2` (`0x801F7504`); on exit a cut on the
///   victim - pitch `0x100`, yaw `-facing` of the creature, TR `(0, 0x400,
///   ctx[+0x6D0] / 2)` (`0x801F763C`) - and `+= scalar << 7`;
/// * `9` - every pass TR z `+= 4 p` (`0x801F7724`); on exit `+= scalar << 7`
///   (the damage arm);
/// * `10` - TR z `+= 4 p` while the countdown runs, `+= p >> 3` after it;
///   once it is spent the arm stores phase `0xFF`;
/// * `0xFF` - holds (retail returns `0`: the module is done).
///
/// Not carried: the creature's own `+0x36` rise in arms 2 and 3, the arm 3 /
/// 5 captions, the arm 6 breath parts, the arm 7..9 fades; arm 8's
/// group-target framing (a target byte of `8` or more frames the group
/// centre through `FUN_801DCEAC` with a doubled TR z) frames the latched
/// victim seat instead; and arm 10's wait for every monster to be idle
/// (`+0x1D9 == 0`, `0x801F7A90..0x801F7B18`) - the arm gates on the
/// countdown alone.
///
/// PORT: FUN_801F69D8 (PROT 0920; the whole phase chain's gates and camera writes - captions, parts, fades and the arm 10 idle wait not carried)
pub fn slippery_direct(st: &mut ModuleCamState, phase: u8, seats: ModuleCamSeats) -> ArmDirection {
    let c = seats.caster;
    let p = MODULE_DRAIN_PER_TICK;
    let creature = st.creature_live.or(st.creature).unwrap_or_default();
    let nudge = |tr_x: i32, tr_y: i32, tr_z: i32, pitch: i32| ModuleNudge {
        pitch: pitch as i16,
        tr_x: tr_x as i16,
        tr_y: tr_y as i16,
        tr_z: tr_z as i16,
    };
    let on_creature = |pitch: i16, yaw: i32, tr: [i16; 3], frames: u16| {
        ArmDirection::shot(ModuleShot {
            angles: [pitch, yaw_from(yaw, creature.facing), 0],
            tr,
            focus: focus_on(creature),
            frames,
        })
    };
    let push = nudge(0, 0, p, 0);
    match phase {
        0 => {
            st.countdown.arm(7);
            ArmDirection::shot(ModuleShot {
                angles: [0, yaw_from(0x800, c.facing), 0],
                tr: [0, 0x580, 0x600],
                focus: focus_on(c),
                frames: 1,
            })
        }
        1 => {
            if st.countdown.0 > 0 {
                st.countdown.drain();
                return ArmDirection::PASS.nudged(push);
            }
            ArmDirection::PASS
        }
        2 => {
            if st.countdown.0 > 0 {
                st.countdown.drain();
                return ArmDirection::HOLD.nudged(push);
            }
            st.countdown.add(1 << 6);
            ArmDirection::PASS.nudged(push)
        }
        3 => {
            let mut drift = None;
            if st.countdown.0 > 0 {
                drift = Some(push);
                if st.countdown.drain_above(0) {
                    return ArmDirection::HOLD.nudged(push);
                }
            }
            st.countdown.add(1 << 7);
            ArmDirection {
                nudge: drift,
                ..ArmDirection::PASS
            }
        }
        4 => {
            if st.countdown.drain_above(0) {
                return ArmDirection::HOLD;
            }
            st.countdown.add(1 << 7);
            on_creature(0x20, 0x800, [0, 0x400, 0x600], 0x20)
        }
        SLIPPERY_RAIN_SEED_ARM => {
            if st.countdown.drain_above(0) {
                return ArmDirection::HOLD;
            }
            st.countdown.add(1 << 8);
            ArmDirection::PASS
        }
        SLIPPERY_RAIN_DRAIN_ARM => {
            if st.countdown.drain_above(0) {
                return ArmDirection::HOLD.nudged(nudge(0, p >> 2, p >> 1, -(p >> 3)));
            }
            st.countdown.add(1 << 8);
            on_creature(0, 0xC00, [0xA00, 0x400, 0x600], 1)
        }
        7 => {
            if st.countdown.drain_above(0) {
                return ArmDirection::HOLD.nudged(nudge(-p, 0, 0, 0));
            }
            st.countdown.add(1 << 5);
            on_creature(0, 0x800, [0, 0x400, 0x600], 1)
        }
        8 => {
            if st.countdown.drain_above(0) {
                return ArmDirection::HOLD.nudged(nudge(0, 0, -(p >> 1), 0));
            }
            st.countdown.add(1 << 7);
            // `sh v0,0x2c(sp)` of `ctx[+0x6D0] / 2` (halfword, rounded
            // toward zero).
            let depth = i32::from(seats.depth_raw as i16) / 2;
            ArmDirection::shot(ModuleShot {
                angles: [0x100, (creature.facing as i16).wrapping_neg(), 0],
                tr: [0, 0x400, depth as i16],
                focus: focus_on(seats.victim),
                frames: 1,
            })
        }
        SLIPPERY_COLOUR_ARM => {
            if st.countdown.drain_above(0) {
                return ArmDirection::HOLD.nudged(nudge(0, 0, 4 * p, 0));
            }
            st.countdown.add(1 << 7);
            ArmDirection::PASS
        }
        SLIPPERY_LAST_ARM => {
            let drift = if st.countdown.0 > 0 {
                st.countdown.drain();
                nudge(0, 0, 4 * p, 0)
            } else {
                nudge(0, 0, (p * DELTA_PER_TICK) >> 3, 0)
            };
            if st.countdown.0 > 0 {
                return ArmDirection::HOLD.nudged(drift);
            }
            ArmDirection {
                jump: Some(crate::seru_magic_ticks::SERU_B_DONE_PHASE),
                ..ArmDirection::PASS.nudged(drift)
            }
        }
        _ => ArmDirection::HOLD,
    }
}
