//! Firing SFX: the explicit voice-attribute key-on, the scheduler queue and
//! its per-frame drain, the ring ops, and the field / battle / world cue
//! routers.

use super::*;

impl<S: AudioSink> AudioDirector<S> {
    /// Key one voice from an explicit
    /// [`VoiceAttr`](legaia_engine_audio::VoiceAttr) set - the shape retail's
    /// `FUN_80065034` takes, and the one a caller that already holds the
    /// program / tone / note / volume needs. The catalog path
    /// ([`Self::enqueue_sfx`]) is for a cue that names itself by id.
    ///
    /// The Muscle Dome's between-leg tally roll is the live caller: its per-
    /// lane cue (`FUN_801D1288`) resolves a full attr set rather than a cue
    /// id, so nothing in the id-keyed path could sound it.
    ///
    /// `vab_id` picks the bank the way retail's does - it is a **VAB id**, and
    /// this director resolves it as an SFX slot, falling back to the active
    /// scene BGM bank when that slot has nothing staged (the same fallback
    /// [`Self::tick_sfx_frame`] uses on a disc-free boot). Returns whether a
    /// voice keyed on.
    pub fn key_on_voice_attr(&mut self, attr: legaia_engine_audio::VoiceAttr) -> bool {
        let slot = u8::try_from(attr.vab_id).unwrap_or(0);
        let Some(vab) = self
            .sfx_vabs
            .get(&slot)
            .or_else(|| self.bank.as_ref().filter(|_| !self.sfx_evicted))
        else {
            return false;
        };
        self.audio
            .with_spu(|spu| legaia_engine_audio::key_on_voice_attr(&attr, spu, vab))
    }

    /// Queue a one-shot sound cue to fire `frames` after this call (the
    /// strike's `timing_frames`). `id` is the [`SfxBank`] descriptor id
    /// directly (the art-record `HitCue::kind`), played without
    /// `classify_cue`. `actor` / `target` ride along for HUD context.
    pub fn enqueue_sfx(&mut self, id: u16, frames: u16, actor: u8, target: u8) {
        self.commands
            .push(super::AudioCommand::SfxEnqueue(id, frames));
        self.sfx_sched
            .enqueue(PendingCue::new(id, frames).with_actors(actor, target));
    }

    /// Queue a screen's blip - a pause-menu cursor / confirm / cancel, a
    /// shop's, the inn prompt's - the one routing both hosts use: on the
    /// delay scheduler at zero delay, as retail writes the cue into its ring
    /// and the frame's drainer keys it. It sounds at the tick's scheduler
    /// step ([`Self::tick_sfx_frame`]: the overlay step under a screen that
    /// freezes the field, the audio frame otherwise), with every other cue
    /// due that tick, and ages nothing already queued.
    pub fn queue_ui_cue(&mut self, cue: u16) {
        self.enqueue_sfx(cue, 0, 0, 0);
    }

    /// Advance the SFX scheduler one frame and fire any matured cue through
    /// the SPU ([`Self::fire_batch`]). Call once per simulation tick so
    /// delayed cues advance even when none are enqueued that frame.
    pub fn tick_sfx_frame(&mut self) -> SfxFrameReport {
        let batch = self.sfx_sched.tick_frame();
        for cue in &batch.fired {
            self.commands.push(super::AudioCommand::SfxDue(cue.id));
        }
        for &id in &batch.ring {
            self.commands.push(super::AudioCommand::RingDue(id));
        }
        self.fire_batch(&batch)
    }

    /// Fire one cue **now**, without ticking the scheduler: a UI blip sounded
    /// on the frame of the press must not age every other queued cue a frame
    /// per blip ([`SfxFireBatch::immediate`]).
    pub fn fire_now(&mut self, id: u16) -> SfxFrameReport {
        self.commands.push(super::AudioCommand::SfxNow(id));
        self.fire_batch(&SfxFireBatch::immediate(id))
    }

    /// Fire one batch through the SPU. Each cue resolves against the
    /// resident SFX bank **its own `+4` category names**
    /// ([`Self::sfx_vab_for_cue`]) - the retail path, where `FUN_80065034`
    /// repoints the current-bank globals at the cue's slot before the program
    /// lookup - and falls back to the active scene BGM bank ([`Self::bank`])
    /// when nothing is staged at all (the disc-free boot). A cue is silently
    /// dropped when no bank is staged, its id isn't in the descriptor bank, its
    /// program / tone isn't resident, or no SPU voice is free (matching the
    /// retail "no voice / no program -> skip" behaviour). The report counts the
    /// ring cues that came due and the voice-leg cues declined whether or not
    /// anything keyed, so a host can tell a silent source from an absent one.
    fn fire_batch(&mut self, batch: &SfxFireBatch) -> SfxFrameReport {
        let mut report = SfxFrameReport {
            ring_due: batch.ring.clone(),
            ..SfxFrameReport::default()
        };
        // The queue is a `u16` because the battle cue space is - the action
        // SM's cast cues run to `0x20E` - while the SFX descriptor table is
        // `0x00..=0x63`. Truncating with `as u8` did not make an out-of-band
        // cue silent, it made it play the **wrong** descriptor: `0x20C`
        // became `0x0C`, `0x118` became `0x18`, and every one of those is a
        // populated entry. Classify instead, exactly as `FUN_8004FCC8` does.
        //
        // Only the `Voice` band is re-routed here. The `Ring` band's `id - 1`
        // resolution below `0x40` is retail's (`route_sfx_cue`'s low leg) but
        // the producers feed this queue an art-record `HitCue::kind` / a menu
        // descriptor id the bank is already indexed by, so applying it would
        // silently move every cue that works today. Left as an open thread
        // rather than changed without an oracle.
        let mut descriptors: Vec<(u16, u8)> = Vec::with_capacity(batch.fired.len());
        for cue in &batch.fired {
            match legaia_engine_audio::classify_cue(u32::from(cue.id)) {
                legaia_engine_audio::CueDispatch::Voice {
                    channel, submode, ..
                } => {
                    // A streamed CD-XA voice, not an SPU descriptor. No
                    // producer feeds this queue such ids: the `FUN_801F3990`
                    // cast-cue band (the item-use voice) and the cast
                    // module's head cue both resolve at the world into the
                    // `(clip, channel, dur)` channel `play_xa_clip` plays, so
                    // a voice id here is a stray and is declined.
                    log::debug!(
                        "battle cue {:#06x} is a CD-XA voice (clip channel {channel:#04x} \
                         submode {submode}) on the SFX queue; declined",
                        cue.id
                    );
                    report.voice_declined += 1;
                }
                // A `Ring` id out of the descriptor space could only come
                // from a `0` cue wrapping to `0xFFFF`; keying descriptor
                // `0xFF` is a no-op in every bank.
                legaia_engine_audio::CueDispatch::Ring { .. } => {
                    descriptors.push((cue.id, cue.id.min(0xFF) as u8))
                }
            }
        }
        if (batch.ring.is_empty() && descriptors.is_empty())
            || (self.sfx_vabs.is_empty() && self.bank.is_none())
        {
            return report;
        }
        let bank = &self.sfx_bank;
        // Resolve the ring's ids before borrowing the SPU: below `0x200` a
        // static-table id keyed through its category's bank, at or above it a
        // runtime row keyed through the bank its own `+4` category names.
        let ring: Vec<(u16, RingFire<'_>)> = batch
            .ring
            .iter()
            .filter_map(|&id| {
                let fire = match u8::try_from(id) {
                    Ok(small) => RingFire::Static(small, self.sfx_vab_for_cue(small)?),
                    Err(_) => {
                        let row = legaia_engine_core::world::runtime_sfx_descriptor_in(
                            &self.runtime_sfx_bundle,
                            id,
                        )?;
                        // No fallback: a runtime row's program indexes the
                        // bank its category names, and no other bank.
                        RingFire::Runtime(row, self.sfx_vabs.get(&row[4])?)
                    }
                };
                Some((id as u16, fire))
            })
            .collect();
        // The cue's category picks its bank; a closed slot is silent, and
        // with nothing staged the scene BGM VAB stands in.
        let queued: Vec<(u16, u8, &VabBank)> = descriptors
            .iter()
            .filter_map(|&(queued_id, id)| Some((queued_id, id, self.sfx_vab_for_cue(id)?)))
            .collect();
        let fired = &mut report.fired;
        self.audio.with_spu(|spu| {
            for (id, fire) in &ring {
                let voice = match fire {
                    RingFire::Static(small, vab) => bank.play_one_shot(*small, spu, vab),
                    RingFire::Runtime(row, vab) => bank.play_descriptor(row, spu, vab),
                };
                if let Some(v) = voice {
                    fired.push((*id, v));
                }
            }
            for &(queued_id, id, vab) in &queued {
                if let Some(voice) = bank.play_one_shot(id, spu, vab) {
                    fired.push((queued_id, voice));
                }
            }
        });
        report
    }

    /// Re-check the tail borrowers against the live track's sample end and
    /// forget the ones it overran ([`BgmTail::observe_bgm_end`]). Runs after
    /// every track upload; idempotent.
    pub(super) fn observe_track(&mut self) {
        let end = legaia_engine_audio::bgm_tail::track_end(self.bank.as_ref());
        for slot in self.tail.observe_bgm_end(end) {
            self.sfx_vabs.remove(&slot);
        }
    }

    /// Key off SPU voices a field-VM op stopped (`FUN_800653C8`, the side-band
    /// teardown's top two).
    // REF: FUN_800653C8
    pub fn stop_sfx_voices(&mut self, voices: &[u8]) {
        let mask = voices
            .iter()
            .filter(|&&v| v < 24)
            .fold(0u32, |m, &v| m | (1 << v));
        if mask != 0 {
            self.audio.with_spu(|spu| spu.key_off_mask(mask));
        }
    }

    /// Replay the world's SFX ring producer calls onto the retail ring half
    /// of the scheduler, and install the step the ring ages by per
    /// [`Self::tick_sfx_frame`] - the vsyncs one call spans (retail ages by
    /// `DAT_1F800393` once per game tick of that many vsyncs; a host that
    /// ticks per vsync ages by 1). Call once per sim tick, after the world tick and
    /// before [`Self::tick_sfx_frame`] - the order retail's frame runs the
    /// producers and the drainer in.
    // REF: FUN_80035B50, FUN_80035BAC, FUN_80035BD0
    pub fn apply_sfx_ring_ops(&mut self, ops: &[SfxRingOp], frame_step: u8) {
        self.sfx_sched.set_frame_step(frame_step);
        for op in ops {
            match *op {
                SfxRingOp::Push(id) => {
                    self.sfx_sched.push_ring_cue(id);
                }
                SfxRingOp::SetLastDelay(d) => self.sfx_sched.set_ring_cue_delay(d),
                SfxRingOp::ReplaceLast(id) => self.sfx_sched.replace_ring_cue(id),
                SfxRingOp::WriteSlot(slot, id) => self.sfx_sched.write_ring_slot(slot.into(), id),
                SfxRingOp::ArmSlot(slot, id, delay) => {
                    self.sfx_sched
                        .arm_ring_slot(slot.into(), id, i32::from(delay))
                }
                SfxRingOp::ReleaseSustained => {
                    self.release_sustained_sfx();
                }
            }
        }
    }

    /// The sustained-cue teardown `FUN_80017910` over the live voices: key
    /// off the run the drainer holds from voice 7 and clear its count
    /// ([`legaia_engine_audio::SfxBank::release_sustained`]). Returns how
    /// many voices it released.
    // REF: FUN_80017910
    pub fn release_sustained_sfx(&mut self) -> u8 {
        self.commands.push(super::AudioCommand::SfxReleaseSustained);
        let bank = &self.sfx_bank;
        self.audio.with_spu(|spu| bank.release_sustained(spu))
    }

    /// Mirror the field-side SFX sources the ring's cues resolve against:
    /// the scene's prescript bundle (the runtime descriptor rows, cue ids
    /// `>= 0x200`) and the side-band bank the scripts' op-`0x36` sub-`1`
    /// requests hold in VAB slot `3`. `side_band` is
    /// [`legaia_engine_core::world::World::side_band_bank`] in a field-family
    /// mode and `None` elsewhere - retail has slot 3 open only in the field
    /// (`docs/formats/sfx-table.md`). `field_family` says the world is in such
    /// a mode, where the field init `FUN_801D6704` has closed slot `11`
    /// (`0x801D68B4`), so a parked reward bank is dropped. `read_entry` reads
    /// an extraction-frame PROT entry. The bank is staged behind the BGM, in
    /// the free tail of its region, the way the reward bank is; the
    /// residency rule and the retry memo are [`BgmTail`]'s.
    // REF: FUN_800243F0, FUN_8001E54C, FUN_801D6704
    pub fn sync_field_sfx(
        &mut self,
        bundle: &[u8],
        field_family: bool,
        side_band: Option<SideBandBank>,
        read_entry: impl FnOnce(u32) -> Option<Vec<u8>>,
    ) {
        if self.runtime_sfx_bundle.as_slice() != bundle {
            self.runtime_sfx_bundle = bundle.to_vec();
        }
        // The drainer rolls one-shots over voices 23..=22 in the field and
        // 23..=20 elsewhere (`FUN_80016B6C`).
        self.sfx_bank.set_field_family(field_family);
        if field_family && let Some(slot) = self.tail.drop_reward() {
            self.sfx_vabs.remove(&slot);
        }
        self.observe_track();
        let Some(want) = side_band else {
            if let Some(slot) = self.tail.drop_side_band() {
                self.sfx_vabs.remove(&slot);
            }
            return;
        };
        if !self.tail.begin_side_band_attempt(want) {
            return;
        }
        if let Some(slot) = self.tail.drop_side_band() {
            self.sfx_vabs.remove(&slot);
        }
        let Some(bytes) = read_entry(want.prot_entry) else {
            log::debug!("side-band bank PROT {} unreadable", want.prot_entry);
            return;
        };
        match self.park_tail_bank(want.slot, &bytes) {
            Some(borrow) => {
                self.tail.commit_side_band(want, borrow);
                log::debug!(
                    "side-band bank {} (PROT {}) staged in slot {} behind the BGM",
                    want.request,
                    want.prot_entry,
                    want.slot
                );
            }
            None => log::debug!(
                "side-band bank {} (PROT {}) does not fit behind the BGM",
                want.request,
                want.prot_entry
            ),
        }
    }

    /// Keep the battle's `monster.snd` banks staged - `want` is
    /// [`legaia_engine_core::world::World::battle_monster_sound_banks`]
    /// (`(VAB slot, bank index)`, empty outside battle), `read_archive` reads
    /// PROT 891. Retail's battle scene loader `FUN_800520F0` streams them into
    /// slots `7` / `8` at their own SPU bases; the port parks them behind the
    /// BGM like the reward bank, and drops them when the battle ends. A
    /// request that does not fit is not re-read until the free tail moves.
    // REF: FUN_800520F0, FUN_8003E104
    pub fn sync_battle_monster_banks(
        &mut self,
        want: &[(u8, u16)],
        read_archive: impl FnOnce() -> Option<Vec<u8>>,
    ) {
        self.observe_track();
        let parked = self.tail.monsters().map(|(k, _)| k.clone());
        if parked.as_deref() == Some(want) {
            return;
        }
        if parked.is_some() {
            for slot in self.tail.drop_monsters() {
                self.sfx_vabs.remove(&slot);
            }
        }
        let key: legaia_engine_audio::bgm_tail::MonsterBankKey = want.to_vec();
        if key.is_empty() || !self.tail.begin_monster_attempt(&key) {
            return;
        }
        let Some(archive) = read_archive() else {
            log::debug!("monster.snd (PROT 891) unreadable");
            return;
        };
        for &(slot, bank) in want {
            let Some(bytes) =
                legaia_container_formats::vab_multi_bank::bank_bytes(&archive, usize::from(bank))
            else {
                log::debug!("monster.snd bank {bank} absent");
                continue;
            };
            match self.park_tail_bank(slot, bytes) {
                Some(borrow) => {
                    self.tail.commit_monster(&key, borrow);
                    log::debug!("monster.snd bank {bank} staged in slot {slot} behind the BGM");
                }
                None => log::debug!("monster.snd bank {bank} does not fit behind the BGM"),
            }
        }
    }

    /// One tick of the world's field-side SFX sources, in the order both
    /// hosts run them after the world tick: the ring producer calls replayed
    /// onto the scheduler ([`Self::apply_sfx_ring_ops`]), the scene's runtime
    /// descriptor rows and side-band bank ([`Self::sync_field_sfx`]), the
    /// shared slot-2 / slot-6 region for the current mode (every tick -
    /// [`World::sync_sfx_residency`] is the residency's one writer), the
    /// battle's `monster.snd` banks, then the voice stops before the direct
    /// voice keys, so a release and a re-key of one voice in the same tick
    /// end keyed. The field's CD-XA cues are not drained here: each host
    /// plays them on its own XA lane, first.
    ///
    /// [`World::sync_sfx_residency`]: legaia_engine_core::world::World::sync_sfx_residency
    // REF: FUN_80035B50, FUN_80035BAC, FUN_80035BD0, FUN_800653C8
    pub fn route_world_sfx(
        &mut self,
        world: &mut legaia_engine_core::world::World,
        index: &legaia_engine_scene::scene::ProtIndex,
    ) {
        // A battle's entry and its exit are each a retail mode init
        // (`FUN_8001DCF8`, the battle mode word and the field init's), and
        // the init tears down the held sustained run before the new mode's
        // first cue (`jal 0x80017910` at `0x8001DDA8`).
        let in_battle = world.mode == legaia_engine_mode::mode::SceneMode::Battle;
        if self.sfx_in_battle.is_some_and(|was| was != in_battle) {
            self.release_sustained_sfx();
        }
        self.sfx_in_battle = Some(in_battle);
        let ops = world.take_sfx_ring_ops();
        // One `World::tick` is one vsync, and the scheduler ticks once per
        // `World::tick`, so the ring ages by the vsyncs one tick spans
        // (`display_frame_step`, always 1) - not by the game-tick cadence
        // `frame_step`, which retail applies once per *game tick* of that
        // many vsyncs. The two schedules are the same in wall time.
        self.apply_sfx_ring_ops(&ops, world.clock.display_frame_step.clamp(1, 255) as u8);
        let field_family = matches!(
            world.mode,
            legaia_engine_mode::mode::SceneMode::Field
                | legaia_engine_mode::mode::SceneMode::WorldMap
        );
        // A slot-6 side-band bank is not a tail borrower: retail streams it
        // over the field bank in the shared region, and the residency below
        // carries it.
        let side_band = world.tail_side_band_bank().filter(|b| b.slot != 6);
        self.sync_field_sfx(world.runtime_sfx_bundle(), field_family, side_band, |e| {
            index.entry_bytes_extended(e).ok()
        });
        // The slot-2 / slot-6 region follows the mode: the field bank in the
        // field, the class-2 bank in battle, a minigame's own in its mode.
        let shared = world.sync_sfx_residency();
        self.sync_shared_region(shared, |e| index.entry_bytes_extended(e).ok());
        // The battle's two monster.snd banks (VAB slots 7 / 8).
        let monster_banks = world.battle_monster_sound_banks();
        self.sync_battle_monster_banks(&monster_banks, || {
            index
                .entry_bytes_extended(
                    legaia_container_formats::vab_multi_bank::MONSTER_SND_PROT_INDEX as u32,
                )
                .ok()
        });
        self.stop_sfx_voices(&world.take_sfx_voice_stops());
        // A minigame's directly keyed voices (the slot machine's reel motor).
        for k in world.take_sfx_voice_keys() {
            let keyed = self.key_on_voice_attr(legaia_engine_audio::VoiceAttr::from_cue_words(
                k.voice,
                k.vab_program_tone,
                k.note_and_fine,
                k.volume,
            ));
            log::debug!(
                "direct voice {:#04x} {:?} keyed: {keyed}",
                k.voice,
                k.vab_program_tone
            );
        }
        // The positional emitters' key / volume / release calls.
        self.drive_sfx_emitters(&world.take_sfx_emitter_ops());
    }

    /// Drive the positional sound emitters' voices (the part tick's
    /// render-mode-5 arm, `legaia_engine_vm::sfx_emitter`). Each op names a
    /// cue and its first voice; the cue's descriptor - the static table below
    /// `0x200`, the scene's runtime bank at or above it, exactly as the ring
    /// resolves - gives the voice count, program, tone and note, and its
    /// category the bank. A key-on runs the cue key-on per voice
    /// (`FUN_80065034` with the volume pair); a volume op writes the raw
    /// volume to each voice (`FUN_800657D0`); a release keys them off
    /// (`FUN_800250D4`).
    ///
    /// PORT: FUN_80021DF4 (the mode-5 arm's driver calls)
    pub fn drive_sfx_emitters(&mut self, ops: &[legaia_engine_vm::sfx_emitter::EmitterOp]) {
        use legaia_engine_vm::sfx_emitter::EmitterOp;
        for op in ops {
            let (cue, voice) = match *op {
                EmitterOp::Key { cue, voice, .. }
                | EmitterOp::Volume { cue, voice, .. }
                | EmitterOp::Release { cue, voice } => (cue, voice),
            };
            // (program, tone, note, count) and the bank the category names.
            let resolved = if (0..0x200).contains(&cue) {
                let id = cue as u8;
                self.sfx_bank.get(id).copied().and_then(|e| {
                    Some((
                        (e.program_index, e.tone, e.key, e.voices.max(1)),
                        self.sfx_vab_for_cue(id)?,
                    ))
                })
            } else {
                legaia_engine_core::world::runtime_sfx_descriptor_in(
                    &self.runtime_sfx_bundle,
                    i16::try_from(cue).unwrap_or(i16::MAX),
                )
                .and_then(|row| {
                    Some((
                        (row[0], row[1], row[2], (row[3] & 0x1F).max(1)),
                        self.sfx_vabs.get(&row[4])?,
                    ))
                })
            };
            let Some(((program, tone, note, count), vab)) = resolved else {
                continue;
            };
            let voices = (0..count).filter_map(|i| {
                let v = i32::from(voice) + i32::from(i);
                (0..24).contains(&v).then_some((v as usize, i))
            });
            match *op {
                EmitterOp::Key { vol_l, vol_r, .. } => {
                    let voices: Vec<_> = voices.collect();
                    self.audio.with_spu(|spu| {
                        for (v, i) in voices {
                            vab.play_tone_pair(
                                spu,
                                v,
                                usize::from(program),
                                usize::from(tone) + usize::from(i),
                                note,
                                vol_l,
                                vol_r,
                            );
                        }
                    });
                }
                EmitterOp::Volume { left, right, .. } => {
                    let voices: Vec<_> = voices.collect();
                    self.audio.with_spu(|spu| {
                        for (v, _) in voices {
                            spu.voices[v].vol_left = left as i16;
                            spu.voices[v].vol_right = right as i16;
                        }
                    });
                }
                EmitterOp::Release { .. } => {
                    let stop: Vec<u8> = voices.map(|(v, _)| v as u8).collect();
                    self.stop_sfx_voices(&stop);
                }
            }
        }
    }

    /// Queue one tick's battle strike / cast cues at their strike-relative
    /// timing, `(actor, target)` riding along. A results-frame
    /// [`LEVEL_UP_CUE`](legaia_engine_battle::victory::LEVEL_UP_CUE) first parks
    /// the reward bank behind the BGM ([`Self::ensure_reward_bank`]), as
    /// retail loads PROT 0889 at results time.
    // REF: FUN_8004E568
    pub fn enqueue_battle_cues(
        &mut self,
        cues: &[legaia_engine_battle::battle_events::BattleSfxCue],
        index: &legaia_engine_scene::scene::ProtIndex,
    ) {
        if cues
            .iter()
            .any(|c| c.kind == legaia_engine_battle::victory::LEVEL_UP_CUE)
            && !self.ensure_reward_bank(|e| index.entry_bytes_extended(e).ok())
        {
            log::info!("level-up jingle bank (PROT 0889) did not stage behind the BGM");
        }
        for cue in cues {
            self.enqueue_sfx(cue.kind, cue.timing_frames, cue.actor_slot, cue.target_slot);
        }
    }

    /// The per-tick tail of the audio frame: rest the duck on the world's
    /// configured level (a loaded save's `_DAT_8008457C`), ramp it one unit,
    /// then advance the scheduler and fire what matured. The duck runs in
    /// every mode - the ramp back to full outlives the battle.
    pub fn tick_audio_frame(&mut self, configured_level: i32) -> SfxFrameReport {
        self.set_duck_reference(configured_level);
        self.tick_duck();
        self.tick_sfx_frame()
    }

    /// Drop every queued SFX cue (scene transition / battle abort).
    pub fn clear_sfx(&mut self) {
        self.sfx_sched.clear();
    }
}
