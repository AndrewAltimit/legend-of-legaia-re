//! PROT 0913 (Nova)'s writes into the victim and into the parts it keeps
//! handles on: the whirlwind's lift and spin, the cloud banks it carries
//! over to the victim, and the parts that ride the victim down.

use crate::world::World;
use legaia_engine_battle_loop::nova_motion::*;
use legaia_engine_cast::cast_module_camera::{
    NOVA_CLOUD_RECORDS, NOVA_LIFT_RECORDS, NOVA_RING_RECORD,
};
use legaia_engine_cast::seru_magic_ticks::{
    NOVA_DAMAGE_PHASE, NOVA_LIFT_PHASE, SETTLE_DRAIN_PER_TICK,
};

impl World {
    /// PROT 0913's writes for the arm the tick body just ran (`phase_in`,
    /// the body leaving it on `phase_out`), with the module countdown `w`
    /// after the body's update.
    ///
    /// - Arm `0x0D`'s pass moves the two cloud banks arm 10 seated onto the
    ///   victim's plane pair and turns them by the victim's heading less
    ///   the creature's (`0x801F7A54..0x801F7B0C`).
    ///   The same pass draws each whirlwind ring's yaw (`rand() % 0xFFF`,
    ///   [`nova_ring_yaw`]), stored once the rings are seated.
    /// - Arms `0x0E..=0x10`, every pass: each whirlwind ring is jittered
    ///   round the victim on the world `rand()` stream
    ///   ([`nova_ring_jitter`] on both plane coordinates, ring by ring,
    ///   `0x801F7C80`, `0x801F7D8C`, `0x801F7F48`).
    /// - Arm `0x10`, every pass: the victim climbs to
    ///   [`nova_lift_height`], and on the tick that opens each pass turns by
    ///   [`nova_lift_spin_per_pass`] at the countdown the pass leaves. Its
    ///   pass halts the sixteen rings (`0x801F8124..0x801F8140`).
    /// - Arm `0x13` while it holds: the victim turns `0x600` a pass and
    ///   comes down along [`nova_landing_height`], arm 18's three parts
    ///   riding on it (`0x801F82FC..0x801F8368`). Its pass stands it on the
    ///   floor facing the creature (`0x801F8374..0x801F83AC`).
    ///
    /// PORT: FUN_801F69F0 (PROT 0913; the victim's lift, spin and landing
    /// and the handle writes of arms 0x0D, 0x10 and 0x13)
    pub(crate) fn nova_victim_motion(
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
        // Retail turns the victim a fixed step a pass in arms `0x10` and
        // `0x13`, so the port turns it once, on the tick that opens the pass.
        let opens_pass = self.module_tick_opens_pass();
        let pass_ticks = i32::from(self.module_frame_step());
        // Arm `0x0D`'s pass seats the sixteen rings, each on a yaw of its own
        // `rand() % 0xFFF` drawn in seat order (`0x801F7BBC..0x801F7C5C`);
        // the draws are this pass's, the stores wait for the seated rings.
        if phase_in < NOVA_CLOUD_CARRY_ARM {
            // A fresh cast holds no stores of an earlier one.
            self.casting.nova_ring_yaws = None;
            self.casting.nova_ring_places = None;
        }
        if phase_in == NOVA_CLOUD_CARRY_ARM && passed {
            let yaws: [i16; 16] = std::array::from_fn(|_| nova_ring_yaw(self.next_rand()));
            self.casting.nova_ring_yaws = Some(yaws);
        }
        // Arms `0x0E..=0x10` jitter every ring round the victim on each pass,
        // ahead of the arm's countdown, so the passing pass jitters too:
        // `x = v.x + rand() % 63 - 0x20`, then `z` the same, ring by ring
        // over the sixteen handles. Arm `0x0E` is a one-pass arm whose
        // single run need not open the engine's pass, so it jitters
        // whenever it runs.
        let jitter_pass = opens_pass || (phase_in == NOVA_RING_JITTER_ARMS[0] && passed);
        if NOVA_RING_JITTER_ARMS.contains(&phase_in) && jitter_pass {
            let places: [(i16, i16); 16] = std::array::from_fn(|_| {
                let x = vx.wrapping_add(nova_ring_jitter(self.next_rand()));
                (x, vz.wrapping_add(nova_ring_jitter(self.next_rand())))
            });
            self.casting.nova_ring_places = Some(places);
        }
        // The stores land on the seated rings, in seat order.
        let is_ring = |p: &legaia_engine_effects::summon::SummonPartRuntime| {
            p.tag == Some(NOVA_RING_RECORD) && !p.finished
        };
        if let Some(scene) = self.casting.active_summon.as_mut()
            && scene.parts.iter().any(is_ring)
        {
            if let Some(yaws) = self.casting.nova_ring_yaws.take() {
                for (part, yaw) in scene.parts.iter_mut().filter(|p| is_ring(p)).zip(yaws) {
                    part.state.render_26 = yaw;
                }
            }
            if let Some(places) = self.casting.nova_ring_places.take() {
                for (part, (x, z)) in scene.parts.iter_mut().filter(|p| is_ring(p)).zip(places) {
                    part.state.world_x = x;
                    part.state.world_z = z;
                }
            }
        }
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
                // The pass's sine reads the countdown after the whole
                // pass's climb (`W += (rate * speed) >> 1`, `0x801F7FE4..
                // 0x801F7FF8`), where the port's word has climbed one tick.
                let w_pass = w + (pass_ticks - 1) * (SETTLE_DRAIN_PER_TICK >> 1);
                let spin = if opens_pass && !passed {
                    nova_lift_spin_per_pass(w_pass)
                } else {
                    0
                };
                if let Some(a) = self.actors.get_mut(usize::from(victim_slot)) {
                    a.move_state.world_y = nova_lift_height(w, depth);
                    a.battle.facing_angle =
                        (a.battle.facing_angle as i16).wrapping_sub(spin) as u16 & 0xFFF;
                }
                // Its pass halts the sixteen rings (`0x801F8124..0x801F8140`).
                if passed && let Some(scene) = self.casting.active_summon.as_mut() {
                    for part in &mut scene.parts {
                        if part.tag == Some(NOVA_RING_RECORD) {
                            part.finished = true;
                        }
                    }
                }
            }
            NOVA_DAMAGE_PHASE if !passed => {
                let y = nova_landing_height(w, depth);
                if let Some(a) = self.actors.get_mut(usize::from(victim_slot)) {
                    a.move_state.world_y = y;
                    a.battle.facing_angle = a.battle.facing_angle.wrapping_sub(if opens_pass {
                        NOVA_LANDING_SPIN_PER_PASS
                    } else {
                        0
                    }) & 0xFFF;
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
