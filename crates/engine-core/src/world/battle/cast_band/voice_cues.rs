//! Cast voice and XA cues: the module voice emit, the battle XA cue and the
//! per-round voice listing.

use super::*;

impl World {
    /// Schedule the paged module's **own** CD-XA voice - the head cue every
    /// slot-B image calls the dispatcher `FUN_8004FCC8` with near its head
    /// (`docs/subsystems/cast-module.md`, "The cast's own CD-XA voice"). The
    /// call sits on one arm of the phase byte, past the module's seek and its
    /// stream wait, so the cast is heard when that arm's pass runs
    /// ([`Self::raise_cast_voice_on_arm`]). An image whose call no arm holds
    /// raises it here.
    ///
    /// The same scan names the file the module's head seek parks the drive on,
    /// which decides how long the clip takes to start.
    pub(super) fn emit_cast_module_voice(&mut self, spell_id: u8) {
        self.casting.module_voice_due = None;
        self.audio.xa_parked_clip = None;
        let Some(entry) = self.cast_module_for(spell_id) else {
            return;
        };
        let Some(pool) = self.casting.effect_pool.clone() else {
            return;
        };
        let Some(module) = pool.module(entry) else {
            return;
        };
        self.audio.xa_parked_clip =
            legaia_engine_battle_vm::battle_cast_cue::module_head_seek(&module.bytes);
        self.arm_module_drive(&module.bytes, spell_id);
        // PROT 0924 draws its kill roll on arm 0, ahead of the cue's arm.
        if entry == legaia_engine_cast::cast_module_ticks::ULTIMATE_RAVE_ENTRY {
            self.ultimate_rave_verdict();
        }
        let body = legaia_engine_cast::cast_module_ticks::capture_tick_body(entry, spell_id);
        let arm = match body {
            Some(va) => {
                legaia_engine_battle_vm::battle_cast_cue::body_head_cue_arm(&module.bytes, va)
            }
            None => legaia_engine_battle_vm::battle_cast_cue::module_head_cue_arm(&module.bytes),
        };
        match arm {
            Some(arm) => self.casting.module_voice_due = Some((spell_id, arm)),
            None => self.raise_cast_module_voice(spell_id),
        }
    }

    /// Stand the drive where a summon module finds it
    /// ([`legaia_engine_battle_vm::battle_cd_drive`]): the side-band read's
    /// head, and the battle seek-ahead selector's seek to the caster's clip
    /// file queued (`FUN_8004DA00`, which sends it once the side-band stage
    /// `ctx[+0x276]` is `0` again, [`Self::tick_cast_drive`]). A module with
    /// no seek-and-poll pair waits on nothing.
    ///
    /// REF: FUN_8004DA00, FUN_8003EAE4
    fn arm_module_drive(&mut self, image: &[u8], spell_id: u8) {
        use legaia_engine_battle_vm::battle_cd_drive as cd;
        self.casting.drive_arms =
            legaia_engine_battle_vm::battle_cast_cue::module_drive_arms(image).map(|a| (a, false));
        self.casting.drive = cd::CdDrive::at(cd::SIDE_BAND_LBA);
        self.casting.selector_seek = None;
        let body = legaia_engine_cast::cast_module_ticks::capture_tick_body(
            self.cast_module_for(spell_id).unwrap_or(0),
            spell_id,
        );
        self.casting.module_voice_gated =
            legaia_engine_battle_vm::battle_cast_cue::head_cue_waits_arm_gate(image, body);
        if self.casting.drive_arms.is_none() {
            return;
        }
        let caster = usize::from(self.battle_ctx.active_actor);
        if caster < usize::from(self.party.party_count)
            && let Some(class) = self.spell_table_class(spell_id)
        {
            let slot = self.party_roster_slot(caster) as u8 + 1;
            self.casting.selector_seek =
                cd::clip_lba(u32::from(cd::party_magic_seek_clip(slot, class)));
        }
    }

    /// One vsync of a summon cast's drive, every vsync of the cast whether or
    /// not the module runs a pass (a blocked frame included): the seek in
    /// flight runs on, and the selector's queued seek goes out once the
    /// side-band applier has dropped its stage. A frame-stepped
    /// `theeder_summon_mid_cast` writes the selector's latch `0x8007BDB0`
    /// (`-1` to clip `7`) on the vsync `ctx[+0x276]` reads `0`, `8` vsyncs into
    /// the module's arm 1, and the drive leaves `summon.dat` for `XA8.XA` then.
    pub(in crate::world) fn tick_cast_drive(&mut self) {
        if self.mode != legaia_engine_mode::mode::SceneMode::Battle
            || self.casting.drive_arms.is_none()
        {
            return;
        }
        self.drive_vsync();
        self.casting.drive_vsync_done = true;
    }

    fn drive_vsync(&mut self) {
        self.casting.drive.tick();
        let invoking = self.battle_ctx.action_state
            == legaia_engine_battle_vm::battle_action::ActionState::SummonInvoke.as_byte();
        if !invoking
            && self.battle_ctx.menu_open == 0
            && let Some(lba) = self.casting.selector_seek.take()
        {
            self.casting.drive.seek(lba);
        }
    }

    /// The module's pass on `arm` with its drive: the seek arm polls the
    /// side-band stage first and returns busy while it is up, then issues
    /// `FUN_8003EAE4(0, slot)`, whose `CdlSetloc` waits out what is in flight
    /// ([`Self::cast_drive_blocks_frame`]). Whether `arm` holds this pass is
    /// [`Self::module_drive_holds`].
    pub(in crate::world::battle::cast_band) fn step_module_drive(&mut self, arm: u8) {
        self.casting.drive_ticked_at = self.clock.display_frames;
        // A pass the world's tick did not lead (a module stepped on its own)
        // runs its vsync of the drive here.
        if !std::mem::take(&mut self.casting.drive_vsync_done) {
            self.drive_vsync();
        }
        if let Some((a, issued)) = self.casting.drive_arms
            && !issued
            && arm >= a.seek_arm
            && self.module_seek_ready()
            && let Some(lba) = legaia_engine_battle_vm::battle_cd_drive::clip_lba(a.slot)
        {
            self.casting.drive.seek(lba);
            self.casting.drive_arms = Some((a, true));
        }
    }

    /// The seek arm's `CdlSetloc` goes out: the side-band stage is down, the
    /// selector's seek has been sent, and the drive has finished it.
    fn module_seek_ready(&self) -> bool {
        self.battle_ctx.menu_open == 0
            && self.casting.selector_seek.is_none()
            && self.casting.drive.idle()
    }

    /// Whether retail's game loop is held inside the seek arm's `CdlSetloc`
    /// this frame: the module stands on its seek arm, the seek not yet issued,
    /// behind a drive still busy with the selector's seek. A frame-stepped
    /// `orb_summon_mid_cast` holds the audio level `_DAT_8007B910` at `211`
    /// through those `27` vsyncs and moves only what the interrupt handlers
    /// write, then ramps on through the poll arm, which does not block.
    pub fn cast_drive_blocks_frame(&self) -> bool {
        self.mode == legaia_engine_mode::mode::SceneMode::Battle
            && self.casting.drive_ticked_at + 1 >= self.clock.display_frames
            && self.battle_ctx.menu_open == 0
            && self.casting.drive_arms.is_some_and(|(a, issued)| {
                !issued && self.casting.module_phase == a.seek_arm && !self.casting.drive.idle()
            })
    }

    /// One vsync of a frame [`Self::cast_drive_blocks_frame`] holds: retail's
    /// loop sits inside the seek arm's `CdlSetloc`, so only the drive moves
    /// (and the seek issues once it is free); no actor, camera or module pass
    /// runs, and the display-frame clock the camera and the battle passes
    /// count on stands still with them (advancing it hands the camera the
    /// held frames as a catch-up on the first pass after).
    pub fn step_blocked_cast_frame(&mut self) {
        let arm = self.casting.module_phase;
        self.step_module_drive(arm);
    }

    /// Whether the resident module's current arm waits on the drive: the
    /// seek arm behind the selector's seek, the poll arm until the module's
    /// own seek completes. The seek arm's passes before that, held on the
    /// side-band stage, wait on the applier and not on the drive. What the
    /// oracles read to place a cast's CD-seek wait (the PCM cast channel's
    /// `.drive` sidecar).
    pub fn cast_drive_waiting(&self) -> bool {
        let phase = self.casting.module_phase;
        self.mode == legaia_engine_mode::mode::SceneMode::Battle
            && self.module_drive_holds(phase)
            && !self.casting.drive_arms.is_some_and(|(a, issued)| {
                !issued
                    && phase == a.seek_arm
                    && (self.battle_ctx.menu_open != 0 || self.casting.selector_seek.is_some())
            })
    }

    /// Whether the module's `arm` returns busy on the drive this pass: its seek
    /// arm until the drive is free to take the seek, its poll arm until the
    /// seek completes.
    pub(in crate::world::battle::cast_band) fn module_drive_holds(&self, arm: u8) -> bool {
        let Some((a, issued)) = self.casting.drive_arms else {
            return false;
        };
        (arm == a.seek_arm && !issued)
            || (arm == a.poll_arm && issued && !self.casting.drive.idle())
    }

    /// The module's pass on `arm` makes the head cue's call once `arm` has
    /// reached the cue's. An arm that leaves the module (a chain that skips the
    /// cue's arm) still raises it on the next pass it runs.
    pub fn raise_cast_voice_on_arm(&mut self, arm: u8) {
        // The cue's arm polls the drive first and returns busy until its seek
        // is done (`FUN_8003F2B8(1)`): no call on a busy pass.
        if self.module_drive_holds(arm) {
            return;
        }
        // A call behind its arm's own gate waits for the pass the arm's hold
        // lets through ([`Self::raise_cast_voice_on_pass`]).
        if self.casting.module_voice_gated
            && self
                .casting
                .module_voice_due
                .is_some_and(|(_, due)| due == arm)
        {
            return;
        }
        self.raise_due_cast_voice(arm);
    }

    /// The module's pass on `arm` went through its hold this tick: a head cue
    /// behind the arm's own gate (Gilium's countdown ahead of the poll) makes
    /// its call now.
    pub(in crate::world::battle::cast_band) fn raise_cast_voice_on_pass(&mut self, arm: u8) {
        if self.casting.module_voice_gated && !self.module_drive_holds(arm) {
            self.raise_due_cast_voice(arm);
        }
    }

    fn raise_due_cast_voice(&mut self, arm: u8) {
        if let Some((spell_id, due)) = self.casting.module_voice_due
            && arm >= due
        {
            self.casting.module_voice_due = None;
            self.raise_cast_module_voice(spell_id);
        }
    }

    /// Scan the module's head cue and run it through the dispatcher's CD-XA arm
    /// ([`legaia_engine_battle_vm::battle_cast_cue::admit_voice_cue`]). The
    /// other gate is the live one: a cast inside the previous clip's read span
    /// (`battle_xa_busy_frames`, retail's `gp+0x91C`) plays no voice, exactly as
    /// `FUN_8003DE7C(1)` declines it. The port's side-band is resident, so
    /// `VoiceCueGates::side_band_stage` is `0`.
    ///
    /// The request lands on [`crate::world::AudioState::battle_xa_cues`],
    /// the `(clip_slot, channel, dur)` channel both hosts already play
    /// through their XA lane, and arms the busy span the way the melee
    /// kernel's clip start does. Nothing here touches gameplay state; a
    /// disc-free build (no pool, no span table) emits nothing.
    ///
    /// REF: FUN_8004FCC8 (the CD-XA arm), FUN_8003D53C (the starter the
    /// request stands for)
    fn raise_cast_module_voice(&mut self, spell_id: u8) {
        let Some(entry) = self.cast_module_for(spell_id) else {
            return;
        };
        let Some(pool) = self.casting.effect_pool.clone() else {
            return;
        };
        let Some(module) = pool.module(entry) else {
            return;
        };
        let body = legaia_engine_cast::cast_module_ticks::capture_tick_body(entry, spell_id);
        let head = match body {
            Some(va) => legaia_engine_battle_vm::battle_cast_cue::body_head_cue(&module.bytes, va),
            None => legaia_engine_battle_vm::battle_cast_cue::module_head_cue(&module.bytes),
        };
        let Some(head) = head else {
            return;
        };
        let id = if entry == legaia_engine_cast::cast_module_ticks::ULTIMATE_RAVE_ENTRY {
            legaia_engine_cast::cast_module_ticks::ultimate_rave_cue(self.ultimate_rave_verdict())
        } else if (entry, spell_id) == (944, 0x37) {
            self.guilty_cross_line()
        } else if let Some(cue) = legaia_engine_battle_vm::battle_cast_cue::latch_forked_head_cue(
            entry,
            self.caster_latch(self.battle_ctx.active_actor).unwrap_or(0),
        ) {
            // The cue's arm-0 pass raises it ahead of the arm's own latch write
            // (`run_cast_module_code`), so this reads the branch's latch.
            cue
        } else if let Some(line) = self.forked_head_cue(entry, spell_id) {
            let Some(cue) = line else {
                return;
            };
            cue
        } else {
            head.resolve(|| self.next_rand())
        };
        self.emit_battle_xa_cue(id);
    }

    /// PROT 0944's `0x37` arm 0: the roll, latched for arm 3, and the line that
    /// follows it (`0x801F6AD8..0x801F6B38`).
    ///
    /// PORT: FUN_801F6A04 (arm 0's roll and line)
    fn guilty_cross_line(&mut self) -> u16 {
        use legaia_engine_cast::cast_arm_ticks as arms;
        let caster = self.battle_ctx.active_actor;
        let victim = self
            .actors
            .get(usize::from(caster))
            .map_or(0, |a| a.battle.active_target);
        let roll = self
            .capture_module_roll(&arms::GUILTY_CROSS_SHAPE, caster, victim)
            .unwrap_or(0);
        self.casting.module_cam.latched_hit = Some(roll);
        let hp = self
            .actors
            .get(usize::from(victim))
            .map_or(0, |a| a.battle.hp);
        arms::guilty_cross_cue(roll, hp)
    }

    /// The line a forked module picks for this caster
    /// ([`legaia_engine_battle_vm::battle_cast_cue::forked_head_cue`]).
    fn forked_head_cue(&self, entry: u32, spell_id: u8) -> Option<Option<u16>> {
        let acting = self
            .actors
            .get(usize::from(self.battle_ctx.active_actor))
            .and_then(|a| a.battle_monster_id)
            .unwrap_or(0);
        let head = self.formation_slot0_monster_id().map_or(0, u16::from);
        legaia_engine_battle_vm::battle_cast_cue::forked_head_cue(entry, spell_id, acting, head)
    }

    /// PROT 0924's arm-0 kill roll (`ultimate_rave_kill_verdict`), drawn
    /// once per cast off the world `rand()` stream and held on the module
    /// state for the cue and the finale arm.
    ///
    /// REF: overlay_stager_ultimate_rave_0924_801f6a18 (`0x801F6B0C..0x801F6B2C`)
    pub(in crate::world) fn ultimate_rave_verdict(&mut self) -> bool {
        if let Some(k) = self.casting.module_cam.kill {
            return k;
        }
        let k = legaia_engine_cast::cast_module_ticks::ultimate_rave_kill_verdict(self.next_rand());
        self.casting.module_cam.kill = Some(k);
        k
    }

    /// Run one CD-XA cue id through the dispatcher's CD-XA arm
    /// ([`legaia_engine_battle_vm::battle_cast_cue::admit_voice_cue`]) onto
    /// [`crate::world::AudioState::battle_xa_cues`] - the half of
    /// [`Self::emit_cast_module_voice`] after the module's head cue is
    /// resolved, and what a module arm's own `FUN_8004FCC8(id >= 0x100)` call
    /// raises.
    ///
    /// REF: FUN_8004FCC8 (the CD-XA arm)
    pub(in crate::world) fn emit_battle_xa_cue(&mut self, id: u16) {
        let raw = self
            .audio
            .xa_cue_durations
            .as_deref()
            .and_then(|t| t.get(usize::from(id).wrapping_sub(0x100)).copied());
        let gates = legaia_engine_battle_vm::battle_cast_cue::VoiceCueGates {
            side_band_stage: 0,
            clip_span_left: self.audio.battle_xa_busy_frames,
        };
        if let legaia_engine_battle_vm::battle_cast_cue::VoiceCueVerdict::Play(req) =
            legaia_engine_battle_vm::battle_cast_cue::admit_voice_cue(id, gates, raw)
        {
            // The starter holds the drive for the clip's read span
            // (`FUN_8003D53C`; the same latch `push_battle_xa_cue` arms for
            // the melee kernel's clips).
            self.audio.battle_xa_busy_frames = req.duration_sectors.min(u16::MAX as u32) as u16;
            self.audio
                .battle_xa_cues
                .push(legaia_engine_system::sound_state::XaVoiceClip {
                    clip: req.clip_slot,
                    channel: req.channel,
                    duration_sectors: req.duration_sectors,
                });
        }
    }

    /// Every cast-voice clip `spell_id`'s module can raise - the
    /// [`Self::emit_cast_module_voice`] resolution without its gates or its
    /// coin flip: a literal head cue gives one request, a random one
    /// (`base + rand() % span`, PROT 0936 / 0937) one per candidate. Empty
    /// when the module, its head cue or the span table is missing.
    pub(in crate::world) fn cast_module_voice_candidates(
        &self,
        spell_id: u8,
    ) -> Vec<legaia_engine_system::sound_state::XaVoiceClip> {
        let Some(entry) = self.cast_module_for(spell_id) else {
            return Vec::new();
        };
        let Some(module) = self
            .casting
            .effect_pool
            .as_ref()
            .and_then(|pool| pool.module(entry))
        else {
            return Vec::new();
        };
        let head = match legaia_engine_cast::cast_module_ticks::capture_tick_body(entry, spell_id) {
            Some(va) => legaia_engine_battle_vm::battle_cast_cue::body_head_cue(&module.bytes, va),
            None => legaia_engine_battle_vm::battle_cast_cue::module_head_cue(&module.bytes),
        };
        let ids: Vec<u16> = match head {
            Some(legaia_engine_battle_vm::battle_cast_cue::ModuleHeadCue::Literal(id)) => {
                vec![id]
            }
            Some(legaia_engine_battle_vm::battle_cast_cue::ModuleHeadCue::Random {
                base,
                span,
            }) => (0..u16::from(span.max(1)))
                .map(|k| base.wrapping_add(k))
                .collect(),
            None => Vec::new(),
        };
        // PROT 0924's cue follows its kill roll: either path's literal.
        let ids = if entry == legaia_engine_cast::cast_module_ticks::ULTIMATE_RAVE_ENTRY {
            [true, false]
                .map(legaia_engine_cast::cast_module_ticks::ultimate_rave_cue)
                .to_vec()
        } else if (entry, spell_id) == (944, 0x37) {
            vec![0x1AD, 0x1AB]
        } else if let Some(line) = self.forked_head_cue(entry, spell_id) {
            line.into_iter().collect()
        } else {
            ids
        };
        let open = legaia_engine_battle_vm::battle_cast_cue::VoiceCueGates {
            side_band_stage: 0,
            clip_span_left: 0,
        };
        ids.into_iter()
            .filter_map(|id| {
                let raw = self
                    .audio
                    .xa_cue_durations
                    .as_deref()
                    .and_then(|t| t.get(usize::from(id).wrapping_sub(0x100)).copied());
                match legaia_engine_battle_vm::battle_cast_cue::admit_voice_cue(id, open, raw) {
                    legaia_engine_battle_vm::battle_cast_cue::VoiceCueVerdict::Play(req) => {
                        Some(legaia_engine_system::sound_state::XaVoiceClip {
                            clip: req.clip_slot,
                            channel: req.channel,
                            duration_sectors: req.duration_sectors,
                        })
                    }
                    _ => None,
                }
            })
            .collect()
    }

    /// List the cast voices the round's committed party spells may raise onto
    /// [`crate::world::AudioState::battle_xa_prestage`] - called at the
    /// round's start, before the first action dispatches.
    pub fn list_round_cast_voices(&mut self) {
        let spells: Vec<u8> = self
            .battle
            .round_flow
            .pending
            .iter()
            .filter_map(|p| match p {
                Some(legaia_engine_battle_loop::round_flow::PendingPartyAction::Spell {
                    spell_id,
                    ..
                }) => Some(*spell_id),
                _ => None,
            })
            .collect();
        for spell in spells {
            for clip in self.cast_module_voice_candidates(spell) {
                if !self.audio.battle_xa_prestage.contains(&clip) {
                    self.audio.battle_xa_prestage.push(clip);
                }
            }
        }
    }
}
