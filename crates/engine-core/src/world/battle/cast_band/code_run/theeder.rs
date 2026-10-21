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
    /// approximated arctan LUT, the same one the enemy target cursor uses.
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
