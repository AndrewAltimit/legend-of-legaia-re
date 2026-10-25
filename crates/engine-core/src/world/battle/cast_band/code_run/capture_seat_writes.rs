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
        // The victim's `+0x1D9` back on idle (or a downed seat on clip `8`),
        // the poll a body's settle arm makes (PROT 0962 `0xA2` arm 4).
        st.victim_settled = self.chain_seat_settled(usize::from(victim_slot), false);
        // The caster's clip state, for the bodies that wait on it.
        let caster = self
            .actors
            .get(usize::from(caster_slot))
            .filter(|a| a.battle_action_clips.is_some());
        st.caster_clip = caster.map(|_| self.cast_view(caster_slot).playing_anim);
        st.caster_frame = caster
            .and_then(|a| a.battle_animation.as_ref())
            .map(|p| p.cursor_sixteenths());
        // `+0x21B`, the committed entry's loop count less the cycles run
        // (the player's `+0x176 >> 4` mirror).
        st.caster_loops = caster
            .and_then(|a| a.battle_animation.as_ref())
            .map(|p| p.loop_cycles_remaining());
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
        if w.loop_release {
            a.battle.hit_count_bound = 0;
            if let Some(p) = a.battle_animation.as_mut() {
                p.release_loop_window();
            }
        }
        if let Some(clip) = w.stage {
            a.battle.queued_anim = clip;
            if w.restage {
                a.battle
                    .flag_bits
                    .set(legaia_engine_battle_vm::battle_action::ActorFlags::WINDUP_DONE);
            }
            self.commit_staged_battle_anim(usize::from(slot));
            // The bump's bit 0 cuts the playing clip at once, a one-shot
            // parked on its last frame included (Lu's `0x0E` holds there
            // until PROT 0960's arm 4 stages `0x0C` over it).
            let i = usize::from(slot);
            if w.restage
                && self
                    .actors
                    .get(i)
                    .is_some_and(|a| a.battle.queued_anim != a.battle.current_anim)
            {
                self.commit_staged_battle_anim_at_boundary(i);
            }
        }
    }
}
