//! Theeder's ray: seat pick, geometry, draw and trail projections, and the cone
//! seat query.

use super::*;
use legaia_engine_mode::mode::SceneMode;

impl World {
    /// PROT 0904's beam root and arm-12 ray tip for sweep word `ctx_6d8`
    /// (before the arm's ramp - the tip is built from the ramped word, as the
    /// tick builds it), from the summon seat's live position and facing
    /// (retail reads slot 7's `+0x34` / `+0x38` / `+0x46`).
    ///
    /// REF: FUN_801F69D8 (PROT 0904 arm 12, `0x801F7AF4..0x801F7C80`)
    pub(in crate::world) fn theeder_ray(
        &self,
        summon_slot: u8,
        ctx_6d8: u16,
    ) -> ([i16; 3], [i16; 3]) {
        use legaia_engine_cast::seru_magic_ticks as ta;
        let g = self.theeder_geom(summon_slot);
        let mouth = ta::theeder_mouth(g.x, g.z, g.facing);
        let tip = ta::theeder_ray_tip(mouth, g.facing, ta::theeder_sweep_phase(ctx_6d8));
        (mouth, tip)
    }

    /// PROT 0904's arm-4 seat placement
    /// ([`legaia_engine_cast::seru_magic_ticks::theeder_seat_placement`]) from the victim's
    /// and the caster's live positions.
    ///
    /// REF: FUN_801F69D8 (PROT 0904 arm 4, `0x801F6EEC..0x801F6F8C`)
    pub(in crate::world::battle::cast_band) fn theeder_seat_for(
        &self,
        caster_slot: u8,
        victim_slot: u8,
    ) -> Option<(i16, i16, u16)> {
        let pos = |s: u8| {
            self.actors
                .get(s as usize)
                .map(|a| (a.move_state.world_x, a.move_state.world_z))
        };
        Some(
            legaia_engine_cast::seru_magic_ticks::theeder_seat_placement(
                pos(victim_slot)?,
                pos(caster_slot)?,
            ),
        )
    }

    pub(in crate::world::battle::cast_band) fn pin_theeder_seat(
        &mut self,
        summon_slot: u8,
        (x, z, facing): (i16, i16, u16),
    ) {
        if let Some(a) = self.actors.get_mut(summon_slot as usize) {
            a.move_state.world_x = x;
            a.move_state.world_z = z;
            a.battle.facing_angle = facing;
        }
    }

    /// PROT 0904 arm 12's facing store on every seat it strikes: the victim
    /// turns to face the creature, `+0x46 = FUN_80019B28(victim +0x38,
    /// victim +0x34, slot 7 +0x38, slot 7 +0x34)` (`0x801F7E58..0x801F7E7C`,
    /// slot 7 read off the actor table `0x801C9370 + 0x1C` at
    /// `0x801F6A2C`). The reaction's knockback runs along the facing, so it
    /// carries the body straight away from the creature: unturned, the
    /// rising frame's Gilium stood `300..350` units off retail's, each on
    /// the facing battle setup gave it. `seats` are retail pool seats.
    ///
    /// PORT: FUN_801F69D8 (PROT 0904 arm 12, the victim's facing store)
    pub(in crate::world::battle::cast_band) fn face_theeder_victims(
        &mut self,
        seats: &[u8],
        map: legaia_engine_battle_loop::cast_seat_map::CastSeatMap,
        summon_slot: u8,
    ) {
        let Some((cx, cz)) = self
            .actors
            .get(summon_slot as usize)
            .map(|a| (a.move_state.world_x, a.move_state.world_z))
        else {
            return;
        };
        for &seat in seats {
            let Some(a) = map
                .engine(seat)
                .and_then(|e| self.actors.get_mut(usize::from(e)))
            else {
                continue;
            };
            a.battle.facing_angle = legaia_engine_battle_vm::battle_action::bearing_12bit_approx(
                a.move_state.world_z,
                a.move_state.world_x,
                cz,
                cx,
            );
        }
    }

    /// PROT 0906 arm 13's facing store on every seat its sweep reaches
    /// ([`legaia_engine_cast::seru_magic_ticks::gizam_victim_facing`]): each
    /// turns to face the creature, so its reaction's knockback runs away from
    /// it. `seats` are retail pool seats.
    ///
    /// REF: FUN_801F69F4 (PROT 0906 arm 13, the struck seat's facing store)
    pub(in crate::world::battle::cast_band) fn face_gizam_victims(
        &mut self,
        seats: &[u8],
        map: legaia_engine_battle_loop::cast_seat_map::CastSeatMap,
        summon_slot: u8,
    ) {
        let Some(creature) = self
            .actors
            .get(summon_slot as usize)
            .map(|a| (a.move_state.world_x, a.move_state.world_z))
        else {
            return;
        };
        for &seat in seats {
            let Some(a) = map
                .engine(seat)
                .and_then(|e| self.actors.get_mut(usize::from(e)))
            else {
                continue;
            };
            a.battle.facing_angle = legaia_engine_cast::seru_magic_ticks::gizam_victim_facing(
                creature,
                (a.move_state.world_x, a.move_state.world_z),
            );
        }
    }

    /// The summon seat's live `(x, z)` and facing, as PROT 0904 reads slot 7.
    pub(in crate::world::battle::cast_band) fn theeder_geom(
        &self,
        summon_slot: u8,
    ) -> legaia_engine_cast::seru_magic_ticks::TheederGeom {
        self.actors
            .get(summon_slot as usize)
            .map(|a| legaia_engine_cast::seru_magic_ticks::TheederGeom {
                x: a.move_state.world_x,
                z: a.move_state.world_z,
                facing: a.battle.facing_angle & 0x0FFF,
                // `+0x21B`, the loop counter the anim tick keeps on the
                // creature (`World::mirrors_loop_counter`).
                clip_loops: a.battle.hit_count_bound,
            })
            .unwrap_or_default()
    }

    /// PROT 0904's packets for this frame - what the Theeder module's last
    /// tick drew (the arm-9 prongs, the arm-11 charge beam, the arm-11/12
    /// sweeping beam and its trail, the arm-13 retract) - while its cast is
    /// in the band. Both hosts project the points with their battle camera
    /// and build the primitives with `legaia_engine_ui::cast_theeder`.
    ///
    /// REF: FUN_801F815C, FUN_801F83A4, FUN_801F8634, FUN_801F8B84
    pub fn theeder_draw(
        &self,
    ) -> Option<legaia_render_kernels::cast_theeder_packet::TheederPacket> {
        if self.mode != SceneMode::Battle || self.casting.summon_stager.is_none() {
            return None;
        }
        self.casting.module_theeder.packet
    }

    /// The trail ring [`Self::theeder_draw`]'s fan packets index
    /// (`hist[0]` newest).
    pub fn theeder_trail(&self) -> &[[i16; 3]] {
        &self.casting.module_theeder.trail.hist
    }

    /// PROT 0904 shows every seat it damages: each arm-12 hit stores the
    /// presentation word `+0x04 = 0x3FF0000`, the render flag `0` and
    /// `+0x21F = 2` beside the HP write (`0x801F7D9C` / `0x801F7DB4`), the
    /// one damage site the module has. The port's fold stands in for that
    /// site's magnitude and may strike a seat the ray has not crossed yet, the
    /// summon band still hiding it (`0x34`'s `+0x21C = 0xFF`): a hidden body
    /// does not tick (`0x800474D4`), so its reaction never ended and arm 14's
    /// row poll held the cast for good (`theeder_summon_mid_cast` sat on it
    /// past the recording, the score left ducked). The fold carries the hit's
    /// presentation stores to every monster it set reacting.
    ///
    /// REF: FUN_801F69D8 (PROT 0904 arm 12, the hit's presentation stores)
    pub(in crate::world::battle) fn show_theeder_fold_targets(&mut self) {
        use legaia_engine_cast::seru_magic_ticks::{
            THEEDER_HIT_PRESENT_WORD, THEEDER_HIT_RENDER_21F,
        };
        let pc = usize::from(self.party.party_count);
        for i in pc..self.actors.len() {
            if self.actors[i].battle_monster_id.is_none()
                || self.actors[i].battle_reaction.is_none()
                || !self.battle_body_hidden(i)
            {
                continue;
            }
            let a = &mut self.actors[i];
            a.battle.render_flag = 0;
            a.battle.render_color = THEEDER_HIT_PRESENT_WORD;
            a.battle.impact_state = THEEDER_HIT_RENDER_21F;
        }
    }

    /// Which of `seats` lie inside a `+-half_width` cone about `bearing`, as
    /// seen from `centre` - the geometry PROT 0904's swinging-ray sweep
    /// gates each hit on, and the one thing the module's arm 12 needs from its
    /// host.
    ///
    /// Retail's shape, read off `0x801F7CA0..0x801F7D14`: two calls to the
    /// 12-bit atan2 `FUN_80019B28` against the **same** second point (the
    /// beam root) - one from the ray's tip, one from
    /// the seat - each `+0x800` and masked to `0xFFF`, then
    /// `|ref - seat| - 0x30` compared **unsigned** against `0xFB1`. That
    /// comparison is the wrap: a difference below `0x30` underflows past
    /// `0xFB1` and a difference at or above `0xFE1` exceeds it, so both ends
    /// of the cone are in and everything between is out. The `+0x800` cancels
    /// in the difference and the bearings are measured toward the centre in
    /// both calls, so measuring outward from the centre gives the same
    /// difference.
    ///
    /// The atan2 is the ported `FUN_80019B28`
    /// ([`legaia_engine_battle_vm::battle_action::bearing_12bit_approx`]) over the
    /// computed arctan LUT, which is the retail table.
    ///
    /// REF: FUN_801F69D8 (PROT 0904 arm 12 cone gate), FUN_80019B28
    pub fn seats_in_cone(
        &self,
        centre: (i16, i16),
        bearing: u16,
        half_width: u16,
        seats: std::ops::Range<u8>,
    ) -> Vec<u8> {
        use legaia_engine_battle_vm::battle_action::bearing_12bit_approx;
        const FULL_TURN: u16 = 0x1000;
        seats
            .filter(|&seat| {
                let (sx, sz) = self.cast_seat_xz(seat);
                let to_seat = bearing_12bit_approx(centre.1, centre.0, sz, sx);
                let d = (to_seat.wrapping_sub(bearing)) & (FULL_TURN - 1);
                d <= half_width || d >= FULL_TURN - half_width
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use crate::world::World;
    use crate::world::types::Actor;

    /// Each struck seat turns to face the creature, as arm 12's
    /// `FUN_80019B28` store leaves it: the rising frame's Gilium at
    /// `(-1317, 602)` from a creature on `(699, -768)`, retail's `1420`
    /// within the atan table's step; an unstruck seat keeps its facing.
    #[test]
    fn a_struck_seat_faces_the_creature() {
        let mut world = World::default();
        world.party.party_count = 1;
        for _ in 0..8 {
            world.actors.push(Actor::default());
        }
        let map = world.cast_seat_map();
        let creature = 7;
        world.actors[creature].move_state.world_x = 699;
        world.actors[creature].move_state.world_z = -768;
        let victim = usize::from(map.engine(3).unwrap());
        let other = usize::from(map.engine(4).unwrap());
        world.actors[victim].move_state.world_x = -1317;
        world.actors[victim].move_state.world_z = 602;
        world.actors[other].battle.facing_angle = 2048;
        world.face_theeder_victims(&[3], map, creature as u8);
        let f = world.actors[victim].battle.facing_angle;
        assert!((i32::from(f) - 1413).abs() <= 2, "{f}");
        assert_eq!(world.actors[other].battle.facing_angle, 2048);
    }
}
