//! PROT 0913 (Nova)'s writes into the victim and into the parts it keeps
//! handles on: the whirlwind's lift and spin, the cloud banks it carries
//! over to the victim, and the parts that ride the victim down.

use crate::world::World;
use legaia_engine_cast::cast_module_camera::{
    NOVA_CLOUD_RECORDS, NOVA_LIFT_RECORDS, NOVA_RING_RECORD,
};
use legaia_engine_cast::seru_magic_ticks::{NOVA_DAMAGE_PHASE, NOVA_LIFT_PHASE};
use legaia_engine_system::trig::trig12;

/// The arm whose pass carries the cloud banks over to the victim
/// (`0x801F7A54..0x801F7B0C`).
pub const NOVA_CLOUD_CARRY_ARM: u8 = 0x0D;

/// What PROT 0913 arm `0x13` turns the victim by a pass while it holds
/// (`addiu v0,v0,-0x600` at `0x801F82B8`), per engine tick: a battle frame
/// is two ticks.
pub const NOVA_LANDING_SPIN_PER_TICK: u16 = 0x300;

/// The victim's height under PROT 0913 arm `0x10`'s climb for the countdown
/// `w`: `-sin(w) * ctx[+0x6D0] >> 14` (`0x801F803C..0x801F8064`, the `+0x3FFF`
/// rounding of a negative product included).
pub fn nova_lift_height(w: i32, depth: i16) -> i16 {
    let (sin, _) = trig12((w & 0xFFF) as u16);
    let p = -i32::from(sin) * i32::from(depth);
    let p = if p < 0 { p + 0x3FFF } else { p };
    (p >> 14) as i16
}

/// The victim's height while arm `0x13` brings it down, countdown `w`:
/// `-sin(w) * ctx[+0x6D0] >> 12` (`0x801F82C0..0x801F82F4`).
pub fn nova_landing_height(w: i32, depth: i16) -> i16 {
    let (sin, _) = trig12((w & 0xFFF) as u16);
    let p = -i32::from(sin) * i32::from(depth);
    let p = if p < 0 { p + 0xFFF } else { p };
    (p >> 12) as i16
}

/// What arm `0x10` takes off the victim's heading a tick for countdown `w`:
/// `sin(w) / 2` a battle frame (`0x801F8000..0x801F8038`), half of it on
/// each of the frame's two engine ticks.
pub fn nova_lift_spin_per_tick(w: i32) -> i16 {
    let (sin, _) = trig12((w & 0xFFF) as u16);
    sin / 4
}

impl World {
    /// PROT 0913's writes for the arm the tick body just ran (`phase_in`,
    /// the body leaving it on `phase_out`), with the module countdown `w`
    /// after the body's update.
    ///
    /// - Arm `0x0D`'s pass moves the two cloud banks arm 10 seated onto the
    ///   victim's plane pair and turns them by the victim's heading less
    ///   the creature's (`0x801F7A54..0x801F7B0C`).
    /// - Arm `0x10`, every pass: the victim climbs to
    ///   [`nova_lift_height`] and turns by [`nova_lift_spin_per_tick`];
    ///   each whirlwind ring is jittered round the victim
    ///   (`rand() % 63 - 0x20` on both plane coordinates,
    ///   `0x801F7F48..0x801F7FCC`). Its pass halts the sixteen rings
    ///   (`0x801F8124..0x801F8140`).
    /// - Arm `0x13` while it holds: the victim turns `0x600` a frame and
    ///   comes down along [`nova_landing_height`], arm 18's three parts
    ///   riding on it (`0x801F82FC..0x801F8368`). Its pass stands it on the
    ///   floor facing the creature (`0x801F8374..0x801F83AC`).
    ///
    /// PORT: FUN_801F69F0 (PROT 0913; the victim's lift, spin and landing
    /// and the handle writes of arms 0x0D, 0x10 and 0x13)
    pub(in crate::world::battle::cast_band) fn nova_victim_motion(
        &mut self,
        phase_in: u8,
        phase_out: u8,
        w: i32,
        victim_slot: u8,
    ) {
        let depth = self.battle.camera_frame_height;
        let creature = self
            .casting
            .summon_actor_slot
            .and_then(|s| self.actors.get(usize::from(s)))
            .map(|a| {
                (
                    a.move_state.world_x,
                    a.move_state.world_z,
                    a.battle.facing_angle & 0xFFF,
                )
            });
        let Some(v) = self.actors.get(usize::from(victim_slot)) else {
            return;
        };
        let (vx, vz, vf) = (
            v.move_state.world_x,
            v.move_state.world_z,
            v.battle.facing_angle & 0xFFF,
        );
        let passed = phase_out != phase_in;
        match phase_in {
            NOVA_CLOUD_CARRY_ARM if passed => {
                let turn = creature.map_or(0, |c| vf.wrapping_sub(c.2));
                if let Some(scene) = self.casting.active_summon.as_mut() {
                    for part in &mut scene.parts {
                        if part.tag.is_some_and(|t| NOVA_CLOUD_RECORDS.contains(&t)) {
                            part.state.world_x = vx;
                            part.state.world_z = vz;
                            part.state.render_26 =
                                (part.state.render_26 as u16).wrapping_add(turn) as i16 & 0xFFF;
                        }
                    }
                }
            }
            NOVA_LIFT_PHASE => {
                let spin = nova_lift_spin_per_tick(w);
                if let Some(a) = self.actors.get_mut(usize::from(victim_slot)) {
                    a.move_state.world_y = nova_lift_height(w, depth);
                    a.battle.facing_angle =
                        (a.battle.facing_angle as i16).wrapping_sub(spin) as u16 & 0xFFF;
                }
                let mut seed = self.casting.module_settle_countdown as u32 ^ 0x6E6F_7661;
                if let Some(scene) = self.casting.active_summon.as_mut() {
                    for part in &mut scene.parts {
                        if part.tag != Some(NOVA_RING_RECORD) {
                            continue;
                        }
                        if passed {
                            part.finished = true;
                            continue;
                        }
                        let mut jitter = || {
                            let r = legaia_engine_system::rand::psyq_rand_step(&mut seed);
                            (i32::from(r) % 63) as i16 - 0x20
                        };
                        part.state.world_x = vx.wrapping_add(jitter());
                        part.state.world_z = vz.wrapping_add(jitter());
                    }
                }
            }
            NOVA_DAMAGE_PHASE if !passed => {
                let y = nova_landing_height(w, depth);
                if let Some(a) = self.actors.get_mut(usize::from(victim_slot)) {
                    a.move_state.world_y = y;
                    a.battle.facing_angle = a
                        .battle
                        .facing_angle
                        .wrapping_sub(NOVA_LANDING_SPIN_PER_TICK)
                        & 0xFFF;
                }
                if let Some(scene) = self.casting.active_summon.as_mut() {
                    for part in &mut scene.parts {
                        if part.tag.is_some_and(|t| NOVA_LIFT_RECORDS.contains(&t)) {
                            part.state.world_x = vx;
                            part.state.world_y = y;
                            part.state.world_z = vz;
                        }
                    }
                }
            }
            NOVA_DAMAGE_PHASE => {
                // `FUN_80019B28(v.z, v.x, c.z, c.x)` (`0x801F8374..0x801F8388`).
                let toward = creature.map(|c| {
                    use legaia_engine_cast::cast_module_camera::{ModuleSeat, heading};
                    let seat = |x: i16, z: i16| ModuleSeat {
                        x,
                        y: 0,
                        z,
                        facing: 0,
                    };
                    heading(seat(vx, vz), seat(c.0, c.1))
                });
                if let Some(a) = self.actors.get_mut(usize::from(victim_slot)) {
                    a.move_state.world_y = 0;
                    if let Some(h) = toward {
                        a.battle.facing_angle = h & 0xFFF;
                    }
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_lift_peaks_a_quarter_depth_up_at_a_quarter_turn() {
        assert_eq!(nova_lift_height(0, 0xC00), 0);
        assert_eq!(nova_lift_height(0x400, 0xC00), -0x300);
        assert_eq!(nova_landing_height(0x400, 0xC00), -0xC00);
        assert_eq!(nova_lift_spin_per_tick(0x400), 0x400);
    }
}
