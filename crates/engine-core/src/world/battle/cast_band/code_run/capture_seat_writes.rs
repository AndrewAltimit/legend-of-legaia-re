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

    /// Seat `slot`'s display trio `+0x3C..+0x40` and heading `+0x46`, as a
    /// module arm reads the body where its walk reads the live pair.
    pub(in crate::world::battle::cast_band) fn module_body_seat(
        &self,
        slot: u8,
    ) -> Option<legaia_engine_cast::cast_module_camera::ModuleSeat> {
        let p = self.battle_display_trio(usize::from(slot))?;
        Some(legaia_engine_cast::cast_module_camera::ModuleSeat {
            x: p[0] as i16,
            y: p[1] as i16,
            z: p[2] as i16,
            facing: self.actors.get(usize::from(slot))?.battle.facing_angle & 0xFFF,
        })
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
        st.caster_body = self.module_body_seat(caster_slot);
        st.victim_body = self.module_body_seat(victim_slot);
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
        st.target_unsettled = !self.capture_target_settled(caster_slot);
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

    /// Whether the seats the caster's target code names have settled, as
    /// PROT 0956's `0x71` arm 3 walks them (`0x801F7C2C..0x801F7DA4`,
    /// [`legaia_engine_cast::cast_module_camera::ModuleCamState::target_unsettled`]):
    /// a party seat idle, or on clip `8` once down; a monster seat idle, or
    /// faded out (`+0x04` zero) once dead.
    pub(in crate::world::battle::cast_band) fn capture_target_settled(
        &self,
        caster_slot: u8,
    ) -> bool {
        let map = self.cast_seat_map();
        let pc = usize::from(self.party.party_count);
        let monster = |slot: usize| match self.actors.get(slot) {
            Some(a) if a.battle.hp == 0 => a.battle.render_color == 0 || !a.active,
            _ => self.chain_seat_settled(slot, false),
        };
        let code = self.cast_view(caster_slot).target_code;
        match code {
            0..=2 => map
                .engine(code)
                .is_none_or(|e| self.chain_seat_settled(usize::from(e), false)),
            3..=7 => map.engine(code).is_none_or(|e| monster(usize::from(e))),
            8 => (0..pc).all(|s| self.chain_seat_settled(s, false)),
            _ => (pc..self.actors.len()).all(monster),
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

    /// A module's stores into the bounds `+0x85` / `+0x86` of a monster
    /// caster's action entry `clip`, the loop count `+0x84` kept
    /// ([`legaia_engine_cast::cast_module_camera::CaptureCamArm::caster_clip_bounds`]).
    pub(in crate::world::battle::cast_band) fn write_clip_loop_bounds(
        &mut self,
        caster: u8,
        clip: u8,
        bounds: [u8; 2],
    ) {
        use legaia_actor_formats::monster_archive::ENTRY_LOOP_WINDOW_OFFSET as AT;
        if caster < self.party.party_count {
            return;
        }
        let Some(a) = self.actors.get_mut(usize::from(caster)) else {
            return;
        };
        if let Some(entry) = a
            .battle_action_clips
            .as_mut()
            .and_then(|clips| std::sync::Arc::make_mut(clips).get_mut(usize::from(clip)))
            .and_then(Option::as_mut)
            && let Some(bytes) = entry.effect_script.get_mut(AT + 1..AT + 3)
        {
            bytes.copy_from_slice(&bounds);
        }
        // The window test reads the committed entry on every tick
        // (`FUN_80047430`), so a clip already playing loops on the new bounds
        // at once.
        if a.battle.current_anim == clip
            && let Some(p) = a.battle_animation.as_mut()
        {
            p.set_loop_frames(bounds[0], bounds[1]);
        }
    }

    /// A module's store into the root speed `+0x0C` of a monster caster's
    /// action entry `clip`
    /// ([`legaia_engine_cast::cast_module_camera::CaptureCamArm::caster_clip_speed`]).
    /// Retail edits the archive's entry, and the root-motion term reads the
    /// committed entry every tick (`lh v0,0xc(s3)` at `0x80047D34`), so a
    /// clip already playing changes speed at once: the playing player takes
    /// the value beside the bank.
    pub(in crate::world::battle::cast_band) fn write_clip_root_speed(
        &mut self,
        caster: u8,
        clip: u8,
        speed: i16,
    ) {
        use legaia_actor_formats::monster_archive::ENTRY_ROOT_SPEED_OFFSET as AT;
        if caster < self.party.party_count {
            return;
        }
        let Some(a) = self.actors.get_mut(usize::from(caster)) else {
            return;
        };
        if let Some(entry) = a
            .battle_action_clips
            .as_mut()
            .and_then(|clips| std::sync::Arc::make_mut(clips).get_mut(usize::from(clip)))
            .and_then(Option::as_mut)
            && let Some(bytes) = entry.effect_script.get_mut(AT..AT + 2)
        {
            bytes.copy_from_slice(&speed.to_le_bytes());
        }
        if a.battle.current_anim == clip
            && let Some(p) = a.battle_animation.as_mut()
        {
            p.set_root_speed(speed);
        }
    }

    /// The every-pass move in a held arm's seat write - `(slot, dz, dy)` -
    /// that the rest of a handing-over pass repeats (`World::module_pass`):
    /// only a write that stores no position of its own.
    pub(in crate::world::battle::cast_band) fn seat_move_of(
        slot: u8,
        w: &legaia_engine_cast::cast_module_camera::CaptureSeatWrite,
    ) -> Option<(u8, i16, i16)> {
        let dz = if w.z.is_none() { w.dz } else { 0 };
        let dy = if w.y.is_none() { w.dy } else { 0 };
        (dz != 0 || dy != 0).then_some((slot, dz, dy))
    }

    /// Repeat arm `phase`'s every-pass seat moves and focus drift, as its
    /// last held tick made them, for the `ticks` of the pass it handed over
    /// on that the port does not run (`World::module_pass`, beside the camera
    /// drift it repeats). Retail's passing pass moves the whole step ahead of
    /// the arm's gate.
    pub(in crate::world::battle) fn repeat_module_seat_moves(&mut self, phase: u8, ticks: u8) {
        let Some(moves) = self.casting.module_held_moves.take() else {
            return;
        };
        if moves.phase != phase {
            return;
        }
        for _ in 0..ticks {
            for &(slot, dz, dy) in moves.seats.iter().flatten() {
                self.apply_capture_seat_writes(
                    slot,
                    legaia_engine_cast::cast_module_camera::CaptureSeatWrite {
                        dz,
                        dy,
                        ..Default::default()
                    },
                );
            }
            if moves.focus_z != 0
                && let Some(cam) = self.battle.camera.as_mut()
            {
                cam.drift_module_focus_z(moves.focus_z);
            }
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
            use legaia_engine_battle_vm::battle_anim_rate::{AnimRate, commit_rate_decay};
            // A seat the tick's own bump committed this pass: retail stores
            // the rate in the SM and commits in the anim tick after it, so
            // the commit's decay still lands on this store. PROT 0959's arm
            // `0x0A` stores Che's `2` every pass (`0x801F7598`) and bumps her
            // next clip on the last (`0x801F768C`); she jumps at `4`.
            let bumped = w.stage.is_none()
                && slot < 8
                && self.casting.module_bump_commits & (1 << slot) != 0;
            a.battle.anim_rate = if bumped {
                commit_rate_decay(AnimRate(r))
            } else {
                AnimRate(r)
            };
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

    /// PROT 0966's (Evil Seru Magic) seat moves: arm 0 stands the party in a
    /// half-size row at the back of the stage, arms 12..15 slide each struck
    /// member back while it plays its reaction, arm 21 lifts the caster above
    /// the row at half size, arm 27 puts every seat back and arm 28 repeats
    /// the party's restore every pass. `before` is the party row as the arm
    /// found it (the slide reads the clip the release loop read).
    // PORT: FUN_801F6A74, overlay_cast_evil_seru_magic_0966_801f6a74 (PROT 0966; arm 0 `0x801F6BE0..0x801F6D58`, arms 12..15 `0x801F7418..0x801F750C`, arm 18 `0x801F7C10..0x801F7C28`, arm 21 `0x801F7E80..0x801F7EB4`, arm 27 `0x801F8828..0x801F8904`, arm 28 `0x801F89E0..0x801F8A3C`)
    pub(in crate::world::battle::cast_band) fn evil_seru_magic_row(
        &mut self,
        phase: u8,
        passed: bool,
        caster_slot: usize,
        before: &[legaia_engine_cast::cast_module_ticks::CastActorState],
    ) {
        use legaia_engine_cast::cast_module_ticks::{
            ESM_CASTER_FACING, ESM_CASTER_Y, ESM_CASTER_Z, ESM_PARTY_RISE_Z, ESM_ROW_SCALE,
            ESM_ROW_Z, esm_get_up_slide, esm_half_end_step, esm_row_x,
        };
        let pc = usize::from(self.party.party_count).clamp(1, 3);
        let move_to = |a: &mut crate::world::types::Actor, x: i16, z: i16| {
            let (dx, dz) = (
                x.wrapping_sub(a.move_state.world_x),
                z.wrapping_sub(a.move_state.world_z),
            );
            a.move_state.world_x = x;
            a.move_state.world_z = z;
            if let Some((bx, bz)) = a.battle.seat {
                a.battle.seat = Some((bx.wrapping_add(dx), bz.wrapping_add(dz)));
            }
        };
        match phase {
            0 if self.casting.module_cam.party_saved[0].is_none() => {
                let row = super::summon_body_writes::HORN_FORMATION[pc];
                for (i, &[fx, _]) in row.iter().enumerate().take(pc) {
                    let radius = self.battle_node_radius(i);
                    let Some(a) = self.actors.get_mut(i) else {
                        continue;
                    };
                    self.casting.module_cam.party_saved[i] = Some((
                        a.move_state.world_x,
                        a.move_state.world_z,
                        a.battle.facing_angle,
                    ));
                    move_to(a, esm_row_x(fx), ESM_ROW_Z);
                    a.battle.facing_angle = 0;
                    a.battle.render_scale = Some(ESM_ROW_SCALE);
                    a.battle.render_radius = Some(radius >> 1);
                    // The entry-5 end step goes to half with the model.
                    self.casting.module_cam.party_end_step_saved[i] =
                        rescale_end_step(a, |s| Some(esm_half_end_step(s)));
                }
                if let Some(c) = self.actors.get(caster_slot) {
                    self.casting.module_cam.caster_saved = Some((
                        c.move_state.world_x,
                        c.move_state.world_z,
                        c.battle.facing_angle,
                    ));
                    self.casting.module_cam.caster_saved_y = Some(c.move_state.world_y);
                }
            }
            12..=15 => {
                // `dt * 0x801FA460[id]` with `dt` the frame delta
                // `0x1F800393` (`0x801F74B8..0x801F74E8`): the body runs once
                // a vsync here, so a tick slides by one delta's worth, as the
                // countdown drains (`cast_module_camera::evil_seru_magic_camera`).
                let step = 1;
                for (i, s) in before.iter().enumerate().take(pc) {
                    let char_id = self.party_roster_slot(i) as u8 + 1;
                    let dz = esm_get_up_slide(s, char_id, step);
                    if dz != 0
                        && let Some(a) = self.actors.get_mut(i)
                    {
                        let (x, z) = (a.move_state.world_x, a.move_state.world_z.wrapping_add(dz));
                        move_to(a, x, z);
                    }
                }
            }
            // Arm 18's pass stands every party seat at `z = -0x400` as it
            // hides the row (`li a2,-0x400` / `sh a2,0x38(v0)` at
            // `0x801F7C10..0x801F7C28`), PROT 0927's monster move: arm 21
            // shows the row there, in front of Juggernaut, and arm 26's
            // knockdowns slide it on toward the lens.
            18 if passed => {
                for i in 0..pc {
                    if let Some(a) = self.actors.get_mut(i) {
                        let x = a.move_state.world_x;
                        move_to(a, x, ESM_PARTY_RISE_Z);
                    }
                }
            }
            21 if passed => {
                if let Some(c) = self.actors.get_mut(caster_slot) {
                    move_to(c, 0, ESM_CASTER_Z);
                    c.move_state.world_y = ESM_CASTER_Y;
                    c.battle.facing_angle = ESM_CASTER_FACING;
                    c.battle.render_scale = Some(ESM_ROW_SCALE);
                }
            }
            27 | 28 if passed || phase == 28 => {
                for i in 0..pc {
                    let Some((x, z, facing)) = self.casting.module_cam.party_saved[i] else {
                        continue;
                    };
                    let radius = (phase == 27).then(|| self.battle_node_radius(i));
                    let Some(a) = self.actors.get_mut(i) else {
                        continue;
                    };
                    move_to(a, x, z);
                    a.battle.facing_angle = facing;
                    if let Some(r) = radius {
                        a.battle.render_scale = Some(0x1000);
                        a.battle.render_radius = Some(r.wrapping_shl(1));
                        if let Some(saved) = self.casting.module_cam.party_end_step_saved[i].take()
                        {
                            rescale_end_step(a, |_| Some(saved));
                        }
                    }
                }
                if phase == 27
                    && let Some((x, z, facing)) = self.casting.module_cam.caster_saved
                    && let Some(c) = self.actors.get_mut(caster_slot)
                {
                    move_to(c, x, z);
                    if let Some(y) = self.casting.module_cam.caster_saved_y {
                        c.move_state.world_y = y;
                    }
                    c.battle.facing_angle = facing;
                    c.battle.render_scale = Some(0x1000);
                }
            }
            _ => {}
        }
    }
}

impl World {
    /// PROT 0966's (Evil Seru Magic) creature, seat 7: arm 10 seats
    /// Juggernaut's body on the stage behind the half-size party row
    /// (`jal 0x801F19EC` at `0x801F70E4`), arms 11..24 play its clips
    /// ([`legaia_engine_cast::cast_module_ticks::esm_creature_writes`]),
    /// arm 18 lifts it, arm 19 walks it, arm 26 drops its floor clip and
    /// arm 27 hides it and turns its node off (`+0x21C = 0xFF`, `+0x04 = 0`
    /// at `0x801F8918` / `0x801F8928`, node `+0x10 |= 8` at `0x801F8990`).
    /// The arms are PROT 0927's (`World::apply_juggernaut_writes`), on the
    /// same arm numbers but arm 10's seat.
    // PORT: FUN_801F6A74, overlay_cast_evil_seru_magic_0966_801f6a74 (PROT 0966; arm 10 `0x801F70E4..0x801F7148`, arm 18 `0x801F7BCC..0x801F7BFC`, arm 26 `0x801F84F4`, arm 27 `0x801F8918..0x801F8990`)
    pub(in crate::world::battle::cast_band) fn evil_seru_magic_creature(
        &mut self,
        phase: u8,
        passed: bool,
        caster_slot: usize,
    ) {
        use legaia_engine_cast::cast_module_ticks::{
            ESM_CREATURE_FACING, ESM_CREATURE_RISE, ESM_CREATURE_SPELL, ESM_CREATURE_Z,
            esm_creature_writes,
        };
        if phase == 10 && passed && self.casting.summon_actor_slot.is_none() {
            let y = self
                .actors
                .get(caster_slot)
                .map_or(0, |c| c.move_state.world_y);
            self.casting.module_cam.creature =
                Some(legaia_engine_cast::cast_module_camera::ModuleSeat {
                    x: 0,
                    y,
                    z: ESM_CREATURE_Z,
                    facing: ESM_CREATURE_FACING,
                });
            self.casting.pending_summon_spawn = Some((ESM_CREATURE_SPELL, [0, y, ESM_CREATURE_Z]));
            self.object_effect.write(1, [0, 0, 0, -0x300, 0]);
            self.casting.module_creature_clip_kind = 2;
            self.casting.module_cam.creature_floor_clip = true;
        }
        if passed {
            match phase {
                18 => self.object_effect.write(1, [0x400, 0, 0, -0x800, 0x800]),
                26 => {
                    self.casting.module_creature_clip_kind = 0;
                    self.casting.module_cam.creature_floor_clip = false;
                }
                _ => {}
            }
        }
        let Some(slot) = self.casting.summon_actor_slot else {
            return;
        };
        let cursor = self
            .actors
            .get(usize::from(slot))
            .and_then(|a| a.battle_animation.as_ref())
            .map_or(0, |p| p.cursor_sixteenths());
        // The body drains its countdown and walks by the frame delta, a
        // vsync a tick here (`cast_module_camera::evil_seru_magic_camera`).
        let mut s = self.cast_view(slot);
        let walk = esm_creature_writes(phase, passed, i32::from(cursor), 1, &mut s);
        self.write_cast_view(slot, &s);
        let floor_clip = self.casting.module_cam.creature_floor_clip;
        if let Some(a) = self.actors.get_mut(usize::from(slot)) {
            a.battle.floor_clip = floor_clip;
            a.move_state.world_z = a.move_state.world_z.wrapping_sub(walk);
            if phase == 18 && passed {
                (a.move_state.world_y, a.move_state.world_z) = ESM_CREATURE_RISE;
            }
            if phase == 27 && passed {
                a.battle.render_color = 0;
                a.battle.render_flag = 0xFF;
            }
        }
        if phase == 27 && passed {
            self.despawn_summon_actor();
        }
    }
}

/// Rewrite the `+0x0E` end step of `a`'s action entry
/// [`legaia_engine_cast::cast_module_ticks::ESM_END_STEP_ENTRY`] through
/// `f` (the module writes the entry in place, so the next commit of that
/// clip reads it), returning the step it found.
fn rescale_end_step(
    a: &mut crate::world::types::Actor,
    f: impl FnOnce(i16) -> Option<i16>,
) -> Option<i16> {
    use legaia_actor_formats::monster_archive::ENTRY_END_STEP_OFFSET as OFF;
    let clips = a.battle_action_clips.as_mut()?;
    let k = legaia_engine_cast::cast_module_ticks::ESM_END_STEP_ENTRY;
    let old = {
        let head = &clips.get(k)?.as_ref()?.effect_script;
        let b = head.get(OFF..OFF + 2)?;
        i16::from_le_bytes([b[0], b[1]])
    };
    let new = f(old)?;
    if new != old
        && let Some(Some(c)) = std::sync::Arc::make_mut(clips).get_mut(k)
    {
        c.effect_script[OFF..OFF + 2].copy_from_slice(&new.to_le_bytes());
    }
    Some(old)
}
