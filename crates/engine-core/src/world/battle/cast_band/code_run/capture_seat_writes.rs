//! The capture directors' host half: the live inputs they read beside the
//! seats, and their stores into the caster and victim seats.

use super::*;

impl World {
    /// PROT 0961's seat line-up on its arm 0 pass and the restore on its
    /// `0xFF` arm's, ahead of the arm's camera, which frames the lined-up
    /// caster ([`legaia_engine_cast::cast_module_camera::dead_end_crisis_line_up`]).
    /// Retail walks `actor_table[0..4]`; an empty party seat has no engine
    /// slot and is skipped. The poses saved are the engine's live trio and
    /// facing.
    pub(in crate::world::battle::cast_band) fn dead_end_crisis_seats(&mut self, phase: u8) {
        use legaia_engine_cast::cast_module_camera as cam;
        const N: usize = cam::DEAD_END_CRISIS_LINE_UP_SEATS;
        let map = self.cast_seat_map();
        let slots: [Option<usize>; N] = std::array::from_fn(|s| {
            map.engine(s as u8)
                .map(usize::from)
                .filter(|&e| e < self.actors.len())
        });
        if phase == cam::DEAD_END_CRISIS_LINE_UP_ARM {
            let mut poses = [cam::LineUpPose::default(); N];
            for (p, e) in poses.iter_mut().zip(slots) {
                if let Some(a) = e.map(|e| &self.actors[e]) {
                    let m = &a.move_state;
                    p.pos = [m.world_x, m.world_y, m.world_z, 0];
                    p.facing = a.battle.facing_angle;
                }
            }
            // The one-monster row of the seat table at `0x80077628`.
            let row_x = std::array::from_fn(|s| {
                legaia_engine_battle::battle_seats::monster_seat(1, s, false).x
            });
            let saved = cam::dead_end_crisis_line_up(&mut poses, row_x);
            self.casting.module_saved_poses.clear();
            for ((p, s), e) in poses.iter().zip(&saved).zip(slots) {
                let Some(e) = e else { continue };
                self.casting
                    .module_saved_poses
                    .push((e, [s.pos[0], s.pos[1], s.pos[2]], s.facing));
                Self::stand_battle_seat(&mut self.actors[e], p.pos[0], p.pos[2], p.facing);
            }
        } else if phase == cam::DEAD_END_CRISIS_RESTORE_ARM {
            let held = std::mem::take(&mut self.casting.module_saved_poses);
            let mut seats = [cam::LineUpPose::default(); N];
            let mut saved = [cam::LineUpPose::default(); N];
            for (i, &(e, pos, facing)) in held.iter().take(N).enumerate() {
                if let Some(a) = self.actors.get(e) {
                    let m = &a.move_state;
                    seats[i].pos = [m.world_x, m.world_y, m.world_z, 0];
                    seats[i].facing = a.battle.facing_angle;
                }
                saved[i].pos = [pos[0], pos[1], pos[2], 0];
                saved[i].facing = facing;
            }
            cam::dead_end_crisis_restore(&mut seats, &saved);
            for (i, &(e, _, _)) in held.iter().take(N).enumerate() {
                if let Some(a) = self.actors.get_mut(e) {
                    let p = seats[i];
                    a.move_state.world_y = p.pos[1];
                    Self::stand_battle_seat(a, p.pos[0], p.pos[2], p.facing);
                }
            }
        }
    }

    /// Stand an actor's live pair on `(x, z)` facing `facing`, carrying its
    /// body pair by the same step (as [`Self::line_up_party_for_sweep`] does),
    /// so a range read before the next drawn frame sees the move.
    fn stand_battle_seat(a: &mut crate::world::Actor, x: i16, z: i16, facing: u16) {
        let (dx, dz) = (
            x.wrapping_sub(a.move_state.world_x),
            z.wrapping_sub(a.move_state.world_z),
        );
        a.move_state.world_x = x;
        a.move_state.world_z = z;
        if let Some((bx, bz)) = a.battle.seat {
            a.battle.seat = Some((bx.wrapping_add(dx), bz.wrapping_add(dz)));
        }
        a.battle.facing_angle = facing;
    }

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
        let v = self.cast_view(victim_slot);
        st.victim_on_gate_clip = v.playing_anim == v.reaction_gate;
        st.victim_on_knockdown_clip = v.playing_anim == v.knockdown_anim;
        st.caster_in_reach = self.battle_range_metric(caster_slot, victim_slot) == 0;
        st.party_settled =
            (0..usize::from(self.party.party_count)).all(|s| self.chain_seat_settled(s, false));
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

    /// A module's store into a monster caster's size class and body radius
    /// ([`legaia_engine_cast::cast_module_camera::CaptureCamArm::caster_reach`]).
    pub(in crate::world::battle::cast_band) fn store_caster_reach(
        &mut self,
        caster: u8,
        (size, radius): (u8, i16),
    ) {
        if caster < self.party.party_count {
            return;
        }
        self.casting.caster_size_store = Some((caster, size));
        if let Some(a) = self.actors.get_mut(usize::from(caster)) {
            a.battle.render_radius = Some(radius);
        }
    }

    /// A module's store into a monster caster's body radius alone
    /// ([`legaia_engine_cast::cast_module_camera::CaptureCamArm::caster_radius`]);
    /// `None` puts it back on the seat's size class
    /// ([`legaia_engine_cast::cast_module_camera::CaptureCamArm::caster_radius_restore`]).
    pub(in crate::world::battle::cast_band) fn store_caster_radius(
        &mut self,
        caster: u8,
        radius: Option<i16>,
    ) {
        if caster < self.party.party_count {
            return;
        }
        if let Some(a) = self.actors.get_mut(usize::from(caster)) {
            a.battle.render_radius = radius;
        }
    }

    /// A module's stores into the loop-window bytes `+0x84..+0x86` of a
    /// monster caster's action entry `clip`
    /// ([`legaia_engine_cast::cast_module_camera::CaptureCamArm::caster_clip_window`]).
    /// Retail edits the entry the archive holds, so every later commit of the
    /// clip seeds from the new bytes; the actor's own bank stands for it.
    pub(in crate::world::battle::cast_band) fn write_clip_loop_window(
        &mut self,
        caster: u8,
        clip: u8,
        window: [u8; 3],
    ) {
        use legaia_actor_formats::monster_archive::ENTRY_LOOP_WINDOW_OFFSET as AT;
        if caster < self.party.party_count {
            return;
        }
        if let Some(entry) = self
            .actors
            .get_mut(usize::from(caster))
            .and_then(|a| a.battle_action_clips.as_mut())
            .and_then(|clips| std::sync::Arc::make_mut(clips).get_mut(usize::from(clip)))
            .and_then(Option::as_mut)
            && let Some(bytes) = entry.effect_script.get_mut(AT..AT + 3)
        {
            bytes.copy_from_slice(&window);
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
        // Retail plays a reaction on the seat's one clip channel, so an idle
        // staged with the bump cuts it as it cuts any other clip (PROT
        // 0958's arm 25 on a victim still in its get-up).
        if w.stage == Some(0) && w.restage {
            self.cut_battle_reaction_to_idle(usize::from(slot));
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
        if let Some(b) = w.blend {
            a.battle.render_blend = b;
        }
        if let Some(r) = w.render_flag {
            a.battle.render_flag = r;
        }
        if let Some(r) = w.rate {
            a.battle.anim_rate = legaia_engine_battle_vm::battle_anim_rate::AnimRate(r);
        }
        if let Some(floor) = w.seek_floor
            && let Some(p) = a.battle_animation.as_mut()
            && p.cursor_sixteenths() < floor
        {
            p.seek_sixteenths(floor);
        }
        if w.cursor_reset
            && let Some(p) = a.battle_animation.as_mut()
        {
            p.seek_sixteenths(0);
        }
        if let Some(at) = w.seek
            && let Some(p) = a.battle_animation.as_mut()
        {
            p.seek_sixteenths(at);
        }
        if w.ghost_ring_clear {
            for f in a.battle_pose_history.iter_mut() {
                f.ghost_eligible = false;
            }
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
        // The seat's own `+0x1F1` staged with the bump: its knockdown cuts
        // in at once, as the damage primitive's does.
        if w.stage_knockdown
            && let Some(map) = self.battle_reaction_map(usize::from(slot))
        {
            self.commit_battle_reaction_entry(usize::from(slot), map[2]);
        }
    }

    /// PROT 0965 arm `0x0B`'s line-up: each party seat's pair and heading
    /// kept for the restore, then the seat stood at half the seat table's x
    /// for the party count (`DOOMSDAY_LINE_UP_X`), `z = 0`, heading `0x800`.
    pub(in crate::world::battle::cast_band) fn line_up_party_for_sweep(&mut self) {
        let pc = usize::from(self.party.party_count).clamp(1, 3);
        for i in 0..pc {
            let Some(a) = self.actors.get_mut(i) else {
                continue;
            };
            self.casting.module_cam.party_saved[i] = Some((
                a.move_state.world_x,
                a.move_state.world_z,
                a.battle.facing_angle,
            ));
            let x = legaia_engine_cast::cast_module_camera::DOOMSDAY_LINE_UP_X[pc - 1][i];
            let (dx, dz) = (
                x.wrapping_sub(a.move_state.world_x),
                0i16.wrapping_sub(a.move_state.world_z),
            );
            a.move_state.world_x = x;
            a.move_state.world_z = 0;
            if let Some((bx, bz)) = a.battle.seat {
                a.battle.seat = Some((bx.wrapping_add(dx), bz.wrapping_add(dz)));
            }
            a.battle.facing_angle = 0x800;
        }
    }

    /// PROT 0965's `0xFF` arm: the seats [`Self::line_up_party_for_sweep`]
    /// moved, back where it found them.
    pub(in crate::world::battle::cast_band) fn restore_lined_up_party(&mut self) {
        for i in 0..3 {
            let Some((x, z, facing)) = self.casting.module_cam.party_saved[i].take() else {
                continue;
            };
            let Some(a) = self.actors.get_mut(i) else {
                continue;
            };
            let (dx, dz) = (
                x.wrapping_sub(a.move_state.world_x),
                z.wrapping_sub(a.move_state.world_z),
            );
            a.move_state.world_x = x;
            a.move_state.world_z = z;
            if let Some((bx, bz)) = a.battle.seat {
                a.battle.seat = Some((bx.wrapping_add(dx), bz.wrapping_add(dz)));
            }
            a.battle.facing_angle = facing;
        }
    }
}
