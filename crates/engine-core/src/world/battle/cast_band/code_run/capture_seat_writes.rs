//! The capture directors' host half: the live inputs they read beside the
//! seats, and their stores into the caster and victim seats.

use super::*;

impl World {
    /// Fill the capture director inputs of `st` that the seats do not carry:
    /// both bodies' display trios, the lead party member's roster id
    /// `0x8007BD10[0]`, the frame step and whether this tick opens a battle
    /// frame.
    pub(in crate::world::battle::cast_band) fn feed_capture_cam(
        &mut self,
        caster_slot: u8,
        victim_slot: u8,
        st: &mut legaia_engine_cast::cast_module_camera::ModuleCamState,
    ) {
        let body = |w: &World, slot: u8| {
            let p = w.battle_display_trio(usize::from(slot))?;
            Some(legaia_engine_cast::cast_module_camera::ModuleSeat {
                x: p[0] as i16,
                y: p[1] as i16,
                z: p[2] as i16,
                facing: w.actors.get(usize::from(slot))?.battle.facing_angle & 0xFFF,
            })
        };
        st.caster_body = body(self, caster_slot);
        st.victim_body = body(self, victim_slot);
        st.lead_roster = (self.party_roster_slot(0) + 1) as u8;
        st.frame_step = self.battle_frame_step().max(1);
        let frame = self.clock.display_frames;
        st.opens_pass = self.battle.frame_clock.ticks_into_frame(frame) == 0
            && self.casting.capture_pass_mark != Some(frame);
        if st.opens_pass {
            self.casting.capture_pass_mark = Some(frame);
        }
    }

    /// Apply one capture arm's stores into battle seat `slot`
    /// ([`legaia_engine_cast::cast_module_camera::CaptureSeatWrite`]): the
    /// live pair (and the body pair beside it, which the next anim pass
    /// re-derives), the height, the heading, the colour word, the render
    /// flag, the anim rate and the clip cursor.
    pub(in crate::world::battle::cast_band) fn apply_capture_seat_writes(
        &mut self,
        slot: u8,
        w: legaia_engine_cast::cast_module_camera::CaptureSeatWrite,
    ) {
        if w.is_empty() {
            return;
        }
        let Some(a) = self.actors.get_mut(usize::from(slot)) else {
            return;
        };
        let (mut x, mut z) = (a.move_state.world_x, a.move_state.world_z);
        if let Some(v) = w.x {
            x = v;
        }
        z = w.z.unwrap_or(z).wrapping_add(w.dz);
        if (x, z) != (a.move_state.world_x, a.move_state.world_z) {
            let (dx, dz) = (
                x.wrapping_sub(a.move_state.world_x),
                z.wrapping_sub(a.move_state.world_z),
            );
            a.move_state.world_x = x;
            a.move_state.world_z = z;
            if let Some((bx, bz)) = a.battle.seat {
                a.battle.seat = Some((bx.wrapping_add(dx), bz.wrapping_add(dz)));
            }
        }
        a.move_state.world_y = a.move_state.world_y.wrapping_add(w.dy);
        if let Some(y) = w.y {
            a.move_state.world_y = y;
        }
        if let Some(f) = w.facing {
            a.battle.facing_angle = f;
        }
        if let Some(c) = w.colour {
            a.battle.render_color = c;
        }
        if let Some(r) = w.render_flag {
            a.battle.render_flag = r;
        }
        if let Some(r) = w.rate {
            a.battle.anim_rate = legaia_engine_battle_vm::battle_anim_rate::AnimRate(r);
        }
        if w.cursor_reset
            && let Some(p) = a.battle_animation.as_mut()
        {
            p.seek_sixteenths(0);
        }
    }
}
