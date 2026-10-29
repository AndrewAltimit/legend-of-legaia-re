//! PROT 0911 (Orb)'s writes outside its tick kernel: the opening star
//! swarm arm 2 seats and arm 3 draws in, the part-pool flushes, the roster
//! panels arm 9's pass raises and the creature seat arm 5's pass moves. Its
//! stager is `cast_module_ticks::orb_stager_spawns`, run with the other
//! part-called stagers.

use crate::world::World;
use legaia_engine_battle_loop::orb_stager::*;

impl World {
    /// PROT 0911's writes around its tick, every tick of the module:
    ///
    /// - arm 2's pass seats the sixteen stars ([`orb_star_pair`]) on the
    ///   caster's facing, on arm 3's first tick with the pass's other calls;
    /// - while arm 3 holds, each star moves to [`orb_star_place`] of the
    ///   countdown the pass left;
    /// - arms 3 and 9 flush the part pool on their pass (`FUN_80050E74`):
    ///   every part the module seated through `FUN_80050ED4` gets its halt
    ///   bit;
    /// - arm 9's pass raises the roster panels
    ///   ([`BattleState::raise_module_roster_panels`]) and half-cues the backdrop
    ///   pair ([`ORB_HEAL_BACKDROP_CUE`]).
    ///
    /// REF: FUN_80050E74, FUN_801D8DE8
    pub(in crate::world::battle) fn orb_arm_writes(&mut self, phase_in: u8, phase_out: u8) {
        let passed = phase_out != phase_in;
        // Arm 2's pass seats the swarm beside its flash discs; the
        // body-paced director hands that pass's calls over on arm 3's first
        // tick, and the stars go with them.
        if phase_in == 2 && passed {
            self.casting.module_orb.stars_owed = true;
        }
        if phase_in == 3 && std::mem::take(&mut self.casting.module_orb.stars_owed) {
            self.orb_seat_stars();
        }
        if phase_in == 3 && !passed {
            self.orb_place_stars();
        }
        if passed && ORB_POOL_FLUSH_ARMS.contains(&phase_in) {
            self.casting.orb_pool_flush();
        }
        if passed && phase_in == ORB_PANEL_ARM {
            self.battle.raise_module_roster_panels();
            // `li v0,0x800` / `sh v0,0x78(..)` into both backdrop records
            // (`ctx[+0x106C]` / `ctx[+0x1070]`, `0x801F7848..0x801F7860`):
            // the stage the heal stands on comes back half-cued, and the
            // ambient pass ramps it from there.
            self.battle.backdrop_cue = ORB_HEAL_BACKDROP_CUE;
        }
    }

    /// The caster's plane pair and facing, as arms 2 and 3 copy them off
    /// `actor_table[ctx[+0x13]]`.
    fn orb_caster_seat(&self) -> Option<(i16, i16, u16)> {
        let a = self.actors.get(usize::from(self.battle_ctx.active_actor))?;
        Some((
            a.move_state.world_x,
            a.move_state.world_z,
            a.battle.facing_angle & 0xFFF,
        ))
    }

    fn orb_seat_stars(&mut self) {
        use legaia_engine_cast::cast_module_camera::{ModuleSpawn, SpawnAnchor, SpawnRecord};
        let Some((cx, cz, f)) = self.orb_caster_seat() else {
            return;
        };
        let mut offsets = Vec::with_capacity(ORB_STAR_COUNT);
        let mut spawns = Vec::with_capacity(ORB_STAR_COUNT);
        for _ in 0..ORB_STAR_COUNT / 2 {
            for (offset, pos) in orb_star_pair(cx, cz, f, || self.next_rand()) {
                offsets.push(offset);
                spawns.push(ModuleSpawn {
                    record: SpawnRecord::Module(ORB_STAR_RECORD),
                    anchor: SpawnAnchor::Literal {
                        pos,
                        rot: [0, f as i16, 0],
                    },
                });
            }
        }
        self.casting.module_orb.star_offsets = offsets;
        self.stage_module_arm_spawns(911, &spawns, None, None);
    }

    fn orb_place_stars(&mut self) {
        let Some((cx, cz, f)) = self.orb_caster_seat() else {
            return;
        };
        let countdown = self.casting.module_settle_countdown;
        let offsets = &self.casting.module_orb.star_offsets;
        let Some(scene) = self.casting.active_summon.as_mut() else {
            return;
        };
        let stars = scene
            .parts
            .iter_mut()
            .filter(|p| p.tag == Some(ORB_STAR_RECORD));
        for (part, offset) in stars.zip(offsets) {
            let [x, y, z] = orb_star_place(*offset, cx, cz, f, countdown);
            part.state.world_x = x;
            part.state.world_y = y;
            part.state.world_z = z;
        }
    }

    /// Put a held arm-5 seat on the creature, ahead of the tick's camera
    /// feed.
    pub(in crate::world::battle) fn orb_land_deferred_seat(&mut self) {
        let Some([x, y, z]) = self.casting.module_orb.deferred_seat.take() else {
            return;
        };
        if let Some(a) = self
            .casting
            .summon_actor_slot
            .and_then(|s| self.actors.get_mut(usize::from(s)))
        {
            a.move_state.world_x = x;
            a.move_state.world_y = y;
            a.move_state.world_z = z;
        }
    }
}
