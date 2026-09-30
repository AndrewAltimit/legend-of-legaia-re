//! The slot-B summon modules' **own camera** and the **countdown** that paces
//! their arms - the half of a player Seru-magic choreography the phase-chain
//! ports in [`crate::cast_seru_ticks_a`] / [`crate::cast_seru_ticks_b`] leave
//! out.
//!
//! From the summon band's actor freeze `0x34` on, the battle action SM calls
//! no framing case of its own: `0x35` and `0x36` only re-enter the paged
//! module through `FUN_801F1ED4` (`docs/subsystems/battle-action.md`). The
//! camera in those states is therefore whatever the module arms, and every
//! player-Seru module arms it the same way - two shared kernels, parameterised
//! per arm.
//!
//! ## The shot kernel
//!
//! Each camera arm fills three stack trios and hands them to the tween builder
//! `FUN_801D829C` ([`crate::battle_camera::build_camera_angle_tween`]):
//!
//! ```text
//! sp+0x20  pitch, yaw, roll          ; roll is zeroed in the prologue
//! sp+0x28  TR x, TR y, TR z          ; TR z raw, the builder prescales it
//! sp+0x30  focus x, y, z             ; the negated world point
//! a3       duration, display frames
//! ```
//!
//! [`ModuleShot`] is that call. What varies per arm is only where the nine
//! halfwords come from: immediates, the negated position of the creature seat
//! `actor_table[7]`, a yaw folded off that seat's heading (`K - facing`), or a
//! heading taken between the caster and its victim through `FUN_80019B28`.
//!
//! ## The countdown kernel
//!
//! Each module keeps one countdown word in its own image (`0x801F7960` in
//! PROT 0903). An arm arms it as a multiple of the speed scalar
//! `*(0x1F80037D)` ([`SPEED_SCALAR`]), and every counted arm then drains it by
//! the product `*(0x1F80037D) * *(0x1F800393)` - scalar times the frame delta -
//! holding while the word is above that arm's threshold. The engine ticks once
//! per displayed frame, so its per-tick drain is [`MODULE_DRAIN_PER_TICK`]:
//! the retail product divided by the two vsyncs a retail battle frame spans.
//! [`ModuleCountdown`] carries the word and the three gate shapes the arms
//! use.
//!
//! What is here decides *when* an arm completes and *what the camera does* on
//! it; the arm's simulation writes stay in its phase-chain port, which the
//! engine calls only on the tick a gate here lets through.
//!
//! Provenance: the disassembly of each module at the slot-B base `0x801F69D8`
//! (`see ghidra/scripts/funcs/overlay_summon_<label>_<entry>_<va>.txt`).
//!
//! REF: FUN_801D829C (the tween every shot arms), FUN_80019B28 (the headings)

use crate::battle_action::{bearing_12bit_approx, motion::trig12};
use crate::battle_cam_script::{BattleCamPose, prescale_tr_z};

/// `*(0x1F80037D)` in battle - the speed scalar the battle seating seeds
/// ([`crate::battle_anim_rate::RATE_NORMAL`]).
pub const SPEED_SCALAR: i32 = crate::battle_anim_rate::RATE_NORMAL as i32;

/// The countdown drain per engine tick. Retail drains `scalar * delta` per
/// battle frame, and a battle frame spans `delta` vsyncs; the engine ticks
/// once per vsync, so one tick drains the scalar alone.
pub const MODULE_DRAIN_PER_TICK: i32 = SPEED_SCALAR;

/// One `FUN_801D829C` call out of a module arm, in retail's own value space.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModuleShot {
    /// `sp+0x20..0x24`: pitch, yaw, roll (12-bit units).
    pub angles: [i16; 3],
    /// `sp+0x28..0x2C`: the translation trio, TR z un-prescaled.
    pub tr: [i16; 3],
    /// `sp+0x30..0x34`: the focus trio, negated world position.
    pub focus: [i16; 3],
    /// `a3`: the tween duration in display frames.
    pub frames: u16,
}

impl ModuleShot {
    /// The shot as the battle camera holds a pose: yaw masked to 12 bits,
    /// TR z prescaled, the focus un-negated. Returns the raw TR z beside it,
    /// which is what the tween builder is handed.
    pub fn pose(&self) -> (BattleCamPose, i32) {
        let raw_z = i32::from(self.tr[2]);
        (
            BattleCamPose {
                pitch: f32::from(self.angles[0]),
                yaw: f32::from(self.angles[1] & 0xFFF),
                tr: [
                    f32::from(self.tr[0]),
                    f32::from(self.tr[1]),
                    prescale_tr_z(raw_z),
                ],
                focus: [
                    -f32::from(self.focus[0]),
                    -f32::from(self.focus[1]),
                    -f32::from(self.focus[2]),
                ],
            },
            raw_z,
        )
    }
}

/// A seat's world position and battle heading (`+0x34`, `+0x36`, `+0x38`,
/// `+0x46`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ModuleSeat {
    pub x: i16,
    pub y: i16,
    pub z: i16,
    pub facing: u16,
}

/// The seats a module's camera arms read, resolved the way every module's
/// prologue resolves them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ModuleCamSeats {
    /// `actor_table[ctx + 0x13]`.
    pub caster: ModuleSeat,
    /// `actor_table[caster + 0x1DD]`.
    pub victim: ModuleSeat,
}

/// The module-resident state the camera arms carry between ticks: the
/// countdown word and the creature seat's pose as the module placed it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ModuleCamState {
    /// The module's countdown word.
    pub countdown: ModuleCountdown,
    /// The creature seat `actor_table[7]` once an arm has placed it.
    pub creature: Option<ModuleSeat>,
    /// The creature's live seat, as the host last moved it (its walk-in).
    /// Fed in by the host each tick; `None` before it is seated.
    pub creature_live: Option<ModuleSeat>,
    /// Whether the creature's walk has reached the victim - the host's
    /// answer to the range test `FUN_8004E2F0(7, victim)` a walk arm polls.
    pub creature_arrived: bool,
    /// `ctx[+0x6DA]` as the module drives it: the yaw base the action
    /// framing it hands back to (`FUN_801D5854` case 6) subtracts the
    /// creature's heading from.
    pub yaw_base: i32,
}

/// `FUN_801D5854(7, 6)` out of a module arm: the action SM's own case-6
/// framing aimed at the creature seat, re-armed every pass so it chases the
/// walking creature. The camera fills in the rest of case 6's inputs
/// (`ctx[+0xD]` style) from its own live context.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModuleFollow {
    /// The seat case 6 frames.
    pub seat: ModuleSeat,
    /// `ctx[+0x6DA]`.
    pub yaw_base: i32,
    /// `ctx[+0x6D0]` - the raw eye depth.
    pub depth_raw: i32,
}

/// A module's countdown word.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ModuleCountdown(pub i32);

impl ModuleCountdown {
    /// `sw (scalar << shift), countdown` - the arming store.
    pub fn arm(&mut self, shift: u32) {
        self.0 = SPEED_SCALAR << shift;
    }

    /// `countdown += scalar * n` - the re-arm the later arms use.
    pub fn add(&mut self, n: i32) {
        self.0 += SPEED_SCALAR * n;
    }

    /// One drain.
    pub fn drain(&mut self) {
        self.0 -= MODULE_DRAIN_PER_TICK;
    }

    /// Drain, then report whether the word is still above `threshold` - the
    /// `subu; slt/bgtz` gate every counted arm opens with.
    pub fn drain_above(&mut self, threshold: i32) -> bool {
        self.drain();
        self.0 > threshold
    }
}

/// What one module arm decided this tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ArmDirection {
    /// `true` when the arm holds: its phase-chain body must not run.
    pub hold: bool,
    /// The camera shot the arm armed, if any.
    pub shot: Option<ModuleShot>,
    /// The case-6 follow the arm re-armed, if any.
    pub follow: Option<ModuleFollow>,
}

impl ArmDirection {
    const HOLD: Self = Self {
        hold: true,
        shot: None,
        follow: None,
    };
    const PASS: Self = Self {
        hold: false,
        shot: None,
        follow: None,
    };
    fn shot(shot: ModuleShot) -> Self {
        Self {
            hold: false,
            shot: Some(shot),
            follow: None,
        }
    }
}

/// `FUN_80019B28(a.z, a.x, b.z, b.x)` - the heading from `a` to `b`.
pub fn heading(a: ModuleSeat, b: ModuleSeat) -> u16 {
    bearing_12bit_approx(a.z, a.x, b.z, b.x)
}

/// Half a trig sample, rounded toward zero - the `srl 0x1f; addu; sra 1`
/// idiom every placement arm uses.
fn half(v: i16) -> i32 {
    i32::from(v) / 2
}

/// A yaw folded off a heading: `(k - heading) & 0xFFF`.
fn yaw_from(k: i32, heading: u16) -> i16 {
    ((k - i32::from(heading)) & 0xFFF) as i16
}

/// The focus trio on a seat at floor height: `(-x, 0, -z)`.
fn focus_on(seat: ModuleSeat) -> [i16; 3] {
    [seat.x.wrapping_neg(), 0, seat.z.wrapping_neg()]
}

// ---------------------------------------------------------------------------
// PROT 0903 - Gimard
// ---------------------------------------------------------------------------

/// PROT 0903's countdown word.
pub const GIMARD_COUNTDOWN_VA: u32 = 0x801F_7960;
/// The eye depth `ctx[+0x6D0]` PROT 0903's arm 10 stores for the walk-in's
/// case-6 framing.
pub const GIMARD_WALK_DEPTH: i32 = 0x800;
/// The yaw base `ctx[+0x6DA]` PROT 0903's arm 10 stores.
pub const GIMARD_WALK_YAW_BASE: i32 = 0x200;
/// The phase arm PROT 0903's creature walks in: clip 1, staged in arm 10,
/// played until the range poll clears.
pub const GIMARD_WALK_ARM: u8 = 11;

/// PROT 0903 (Gimard) - the camera and countdown half of the tick
/// `0x801F69D8`, one call per tick before the phase-chain body
/// ([`crate::cast_seru_ticks_a::gimard_tick`]) runs.
///
/// `b` below is `FUN_80019B28(victim, caster)`, the heading from the victim
/// to the caster, and `h = (b + 0x800) & 0xFFF` its reverse:
///
/// | arm | gate | camera / state |
/// |---:|---|---|
/// | 0 | - | snap: pitch `0x400`, yaw `-(b + 0x800)`, TR `(0, -0x40, 0x2000)`, focus origin (`0x801F6B7C`) |
/// | 1 | - | pitch `0x380`, yaw `0x800 - h`, TR `(0, 0x40, 0x400)`, focus the point half a unit past the victim along `h`, over `0x40` frames (`0x801F6C6C`); countdown `= scalar << 6` |
/// | 2 | drain while positive, hold above `0` | the creature stream load (`FUN_8003EAE4(0, 7)`), taken as ready |
/// | 3 | - | seats the creature half a unit from the victim toward the caster, facing `h`, `y = 0x200`; countdown `= scalar << 6` |
/// | 4 | creature sinks; hold above `scalar << 5` | snap: pitch `-0x1C0`, yaw `0x700 - facing`, TR `(0, 0x380, 0x1E0)`, focus the creature (`0x801F6F18`) |
/// | 5 | creature sinks; hold above `0` | creature lands (`y = 0`), the spell caption; countdown `+= scalar * 180` |
/// | 6 | hold above `0` | snap: pitch `0x80`, yaw `0x880 - facing`, TR `(0, 0x400, 0x400)`, focus the creature (`0x801F712C`); countdown `+= scalar * 192` |
/// | 7 | one drain | pitch `0x140`, yaw `0x940 - facing`, TR `(0, 0x340, 0xA00)`, focus the creature, over `0xC0` frames (`0x801F721C`) |
/// | 8 | hold above `scalar << 7` | - |
/// | 9 | hold above `scalar * 96` | - |
/// | 10 | hold above `0` | - |
///
/// Arm 10 also stores the walk-in's framing context: depth `ctx[+0x6D0] =
/// 0x800`, yaw base `ctx[+0x6DA] = 0x200`. Arm 11 is the walk-in: every pass
/// it turns the creature onto the victim, swings the yaw base by
/// `6 * scalar * delta`, frames the creature through `FUN_801D5854(7, 6)` -
/// the action SM's own case 6 on the creature seat ([`ModuleFollow`]) - and
/// holds on the range poll `FUN_8004E2F0(7, victim)`; once in range it
/// re-arms the countdown by `scalar * 192` and lands the hit.
///
/// Arm 2's CD poll (`0x8007BDB0 == 7`) and arm 3's `FUN_8003F2B8(1)` are the
/// streamed creature record's load; the engine has it resident, so both read
/// as ready.
///
/// PORT: FUN_801F69D8 (PROT 0903; the camera arms 0/1/4/6/7, the countdown gates of arms 2/4/5/6/7/8/9/10 and arm 3's creature placement)
pub fn gimard_direct(st: &mut ModuleCamState, phase: u8, seats: ModuleCamSeats) -> ArmDirection {
    let b = heading(seats.victim, seats.caster);
    let h = (b.wrapping_add(0x800)) & 0xFFF;
    let creature = st.creature.unwrap_or(seats.victim);
    match phase {
        0 => ArmDirection::shot(ModuleShot {
            angles: [0x400, yaw_from(-0x800, b), 0],
            tr: [0, -0x40, 0x2000],
            focus: [0; 3],
            frames: 1,
        }),
        1 => {
            let (sin, cos) = trig12(h);
            st.countdown.arm(6);
            ArmDirection::shot(ModuleShot {
                angles: [0x380, yaw_from(0x800, h), 0],
                tr: [0, 0x40, 0x400],
                focus: [
                    (half(sin) - i32::from(seats.victim.x)) as i16,
                    0,
                    (half(cos) - i32::from(seats.victim.z)) as i16,
                ],
                frames: 0x40,
            })
        }
        2 => {
            if st.countdown.0 > 0 {
                st.countdown.drain();
            }
            if st.countdown.0 > 0 {
                ArmDirection::HOLD
            } else {
                ArmDirection::PASS
            }
        }
        3 => {
            let (sin, cos) = trig12(h);
            st.creature = Some(ModuleSeat {
                x: (i32::from(seats.victim.x) - half(sin)) as i16,
                y: 0x200,
                z: (i32::from(seats.victim.z) - half(cos)) as i16,
                facing: h,
            });
            st.countdown.arm(6);
            ArmDirection::PASS
        }
        4 => {
            if let Some(c) = st.creature.as_mut() {
                c.y = c.y.wrapping_sub(MODULE_DRAIN_PER_TICK as i16);
            }
            if st.countdown.drain_above(SPEED_SCALAR << 5) {
                return ArmDirection::HOLD;
            }
            ArmDirection::shot(ModuleShot {
                angles: [-0x1C0, yaw_from(0x700, creature.facing), 0],
                tr: [0, 0x380, 0x1E0],
                focus: focus_on(creature),
                frames: 1,
            })
        }
        5 => {
            if let Some(c) = st.creature.as_mut() {
                c.y = c.y.wrapping_sub(MODULE_DRAIN_PER_TICK as i16);
            }
            if st.countdown.drain_above(0) {
                return ArmDirection::HOLD;
            }
            if let Some(c) = st.creature.as_mut() {
                c.y = 0;
            }
            st.countdown.add(180);
            ArmDirection::PASS
        }
        6 => {
            if st.countdown.drain_above(0) {
                return ArmDirection::HOLD;
            }
            st.countdown.add(192);
            ArmDirection::shot(ModuleShot {
                angles: [0x80, yaw_from(0x880, creature.facing), 0],
                tr: [0, 0x400, 0x400],
                focus: focus_on(creature),
                frames: 1,
            })
        }
        7 => {
            st.countdown.drain();
            ArmDirection::shot(ModuleShot {
                angles: [0x140, yaw_from(0x940, creature.facing), 0],
                tr: [0, 0x340, 0xA00],
                focus: focus_on(creature),
                frames: 0xC0,
            })
        }
        8 => gate(st.countdown.drain_above(SPEED_SCALAR << 7)),
        9 => gate(st.countdown.drain_above(SPEED_SCALAR * 96)),
        10 => {
            if st.countdown.drain_above(0) {
                return ArmDirection::HOLD;
            }
            // `sh 0x800, 0x6D0(ctx)` / `sh 0x200, 0x6DA(ctx)`
            // (`0x801F737C..0x801F738C`).
            st.yaw_base = GIMARD_WALK_YAW_BASE;
            ArmDirection::PASS
        }
        11 => {
            // The creature turns onto the victim, the yaw base swings by
            // `6 * scalar * delta` (`0x801F73C8..0x801F7400`), and case 6
            // frames the creature (`jal 0x801D5854` with `(7, 6)`), every
            // pass; the arm then holds on the range poll.
            let mut seat = st.creature_live.or(st.creature).unwrap_or(seats.victim);
            seat.facing = heading(seats.victim, seat).wrapping_add(0x800) & 0xFFF;
            st.yaw_base = (st.yaw_base + 6 * MODULE_DRAIN_PER_TICK) & 0xFFF;
            let follow = Some(ModuleFollow {
                seat,
                yaw_base: st.yaw_base,
                depth_raw: GIMARD_WALK_DEPTH,
            });
            // No creature seated (a headless host) has nothing to walk.
            if !st.creature_arrived && st.creature_live.is_some() {
                return ArmDirection {
                    hold: true,
                    shot: None,
                    follow,
                };
            }
            st.countdown.add(192);
            ArmDirection {
                hold: false,
                shot: None,
                follow,
            }
        }
        _ => ArmDirection::PASS,
    }
}

fn gate(hold: bool) -> ArmDirection {
    if hold {
        ArmDirection::HOLD
    } else {
        ArmDirection::PASS
    }
}

/// The director for a player-Seru module, by owning PROT entry. `None` for a
/// module whose camera arms are not ported: the phase chain then runs
/// ungated, as before.
pub fn module_director(
    prot_entry: u32,
) -> Option<fn(&mut ModuleCamState, u8, ModuleCamSeats) -> ArmDirection> {
    match prot_entry {
        903 => Some(gimard_direct),
        _ => None,
    }
}

/// The phase arm a directed module lands its outcome in: once the module's
/// phase has passed it, the hit has been applied. `None` for a module with no
/// director.
pub fn module_hit_arm(prot_entry: u32) -> Option<u8> {
    module_walk_arm(prot_entry)
}

/// The phase arm a directed module walks its creature in; the host starts
/// the creature's walk there and reports its arrival back through
/// [`ModuleCamState::creature_arrived`].
pub fn module_walk_arm(prot_entry: u32) -> Option<u8> {
    match prot_entry {
        903 => Some(GIMARD_WALK_ARM),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seats() -> ModuleCamSeats {
        ModuleCamSeats {
            caster: ModuleSeat {
                x: 82,
                y: 0,
                z: -542,
                facing: 0,
            },
            victim: ModuleSeat {
                x: 0,
                y: 0,
                z: 800,
                facing: 0x800,
            },
        }
    }

    /// Walk a director through its arms, running the phase chain's advance
    /// on every pass the way the engine seam does. Returns the tick each arm
    /// was first passed on, and every shot with the tick it was armed.
    fn walk(last: u8) -> (Vec<u32>, Vec<(u32, u8, ModuleShot)>) {
        let mut st = ModuleCamState::default();
        let mut phase = 0u8;
        let mut passed = Vec::new();
        let mut shots = Vec::new();
        for tick in 0..4000u32 {
            let d = gimard_direct(&mut st, phase, seats());
            if let Some(s) = d.shot {
                shots.push((tick, phase, s));
            }
            if !d.hold {
                passed.push(tick);
                phase += 1;
                if phase > last {
                    break;
                }
            }
        }
        (passed, shots)
    }

    #[test]
    fn gimard_arm_timing_follows_the_countdown() {
        let (passed, shots) = walk(10);
        // Arms 0 and 1 pass on consecutive ticks; arm 2 drains `scalar << 6`
        // at `scalar` a tick.
        assert_eq!(passed[0], 0);
        assert_eq!(passed[1], 1);
        assert_eq!(passed[2] - passed[1], 64);
        // Arm 4 holds until the word falls to `scalar << 5`, arm 5 until it
        // is spent: half each.
        assert_eq!(passed[4] - passed[3], 32);
        assert_eq!(passed[5] - passed[4], 32);
        // Arm 6 waits out the caption's `scalar * 180`.
        assert_eq!(passed[6] - passed[5], 180);
        // Five shots: arms 0, 1, 4, 6, 7.
        let arms: Vec<u8> = shots.iter().map(|s| s.1).collect();
        assert_eq!(arms, vec![0, 1, 4, 6, 7]);
        // Arm 7's pan is the long one.
        assert_eq!(shots[4].2.frames, 0xC0);
    }

    #[test]
    fn gimard_walk_arm_follows_the_creature_until_it_arrives() {
        let mut st = ModuleCamState::default();
        let s = seats();
        st.creature_live = Some(s.caster);
        let d = gimard_direct(&mut st, 10, s);
        assert!(!d.hold);
        assert_eq!(st.yaw_base, GIMARD_WALK_YAW_BASE);
        let d = gimard_direct(&mut st, 11, s);
        assert!(d.hold, "holds on the range poll");
        let f = d.follow.expect("case 6 on the creature");
        assert_eq!(f.depth_raw, GIMARD_WALK_DEPTH);
        assert_eq!(f.yaw_base, GIMARD_WALK_YAW_BASE + 6 * MODULE_DRAIN_PER_TICK);
        st.creature_arrived = true;
        let d = gimard_direct(&mut st, 11, s);
        assert!(!d.hold);
        assert!(d.follow.is_some());
    }

    #[test]
    fn gimard_creature_shots_frame_the_placed_creature() {
        let mut st = ModuleCamState::default();
        let s = seats();
        let _ = gimard_direct(&mut st, 3, s);
        let c = st.creature.expect("arm 3 places the creature");
        let h = (heading(s.victim, s.caster) + 0x800) & 0xFFF;
        assert_eq!(c.facing, h);
        // Toward the caster from the victim, half a unit out.
        assert!(c.z < s.victim.z);
        st.countdown.0 = 0;
        let d = gimard_direct(&mut st, 6, s);
        let shot = d.shot.expect("arm 6 snaps");
        assert_eq!(shot.focus, [c.x.wrapping_neg(), 0, c.z.wrapping_neg()]);
        assert_eq!(shot.angles[1], ((0x880 - i32::from(h)) & 0xFFF) as i16);
        let (pose, raw_z) = shot.pose();
        assert_eq!(raw_z, 0x400);
        assert_eq!(pose.focus, [f32::from(c.x), 0.0, f32::from(c.z)]);
    }
}
