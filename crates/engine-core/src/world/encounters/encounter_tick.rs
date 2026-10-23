//! The encounter roll per field step, the battle intro tick, the holds it
//! places on the field, and the hand-off into and out of battle.

use super::*;
use legaia_engine_mode::mode::SceneMode;

impl World {
    /// Field-step trigger. Engines call this once per "the player walked
    /// one map cell" (typically when the player actor's grid coord moves)
    /// to advance the encounter tracker. Returns `true` if a battle
    /// transition was triggered this step.
    ///
    /// The method is a no-op when no [`legaia_engine_battle::encounter::EncounterSession`]
    /// is installed, when the session is not in `Idle`, or when the world
    /// is not in [`SceneMode::Field`].
    pub fn on_field_step(&mut self) -> bool {
        if !matches!(self.mode, SceneMode::Field) {
            return false;
        }
        // Scripted/forced formation (install_man_formation /
        // install_encounter_from_record): a one-shot override that fires on this
        // step regardless of any per-region random tracker. Retail copies the
        // carrier's `entity[+0x94]` formation into the battle cell independent of
        // the random-roll path (`FUN_801D9E1C`), so a 0%-random scene still
        // starts the scripted fight. Drive the forced 0xFF session directly and
        // consume the flag.
        if self.encounters.scripted_formation_pending {
            self.encounters.scripted_formation_pending = false;
            let rng = self.next_rng();
            return match self.encounters.session.as_mut() {
                Some(session) => {
                    // Retail's scripted path bypasses the rate math entirely;
                    // keep the forced 0xFF-rate roll free of the accessory /
                    // status modifiers (a >>1 would halve a forced trigger).
                    session.tracker_mut().set_rate_modifiers(Default::default());
                    session.on_step(rng)
                }
                None => {
                    log::error!(
                        "encounter: a scripted formation was armed for scene '{}' but no encounter \
                         session is installed - the scripted fight is lost",
                        self.active_scene_label
                    );
                    false
                }
            };
        }
        // Per-region path: when a field region tracker is installed, the
        // player's active region (rate increment + formation-range pick)
        // drives the roll (`FUN_801D9E1C`) instead of the session's
        // aggregated mean rate. The session still owns the transition /
        // grace SM, so only roll while it is `Idle` (mirroring
        // [`legaia_engine_battle::encounter::EncounterSession::on_step`]'s own gate) and feed
        // a trigger through [`legaia_engine_battle::encounter::EncounterSession::trigger_with`].
        // REF: FUN_801D9E1C (ported in legaia_engine_battle::region_encounter)
        if self.terrain.region_tracker.is_some() {
            // The region roll owns the rate AND the formation pick; the
            // session owns nothing but the frames between trigger and battle.
            // A host that dropped the session after scene entry (the New Game
            // reset is one) must not therefore silently swallow every roll:
            // the region tracker consumes RNG, latches its anti-repeat and
            // re-seeds its counter on a trigger, so a discarded roll is a
            // fight that happened and then didn't. Re-install the bare
            // bracket instead.
            if self.encounters.session.is_none() {
                self.install_encounter_bracket();
            }
            let idle = self.encounters.session.as_ref().is_none_or(|s| {
                matches!(
                    s.phase(),
                    legaia_engine_battle::encounter::EncounterPhase::Idle
                )
            });
            if !idle {
                return false;
            }
            let Some(slot) = self.player_actor_slot else {
                return false;
            };
            let (wx, wz) = match self.actors.get(slot as usize) {
                Some(a) => (a.move_state.world_x, a.move_state.world_z),
                None => return false,
            };
            // Take the tracker out so the RNG closure can borrow `self`
            // (same borrow-window pattern as `live_world_map_tick`).
            let mut tracker = self.terrain.region_tracker.take().expect("is_some checked");
            tracker.set_modifiers(self.encounter_rate_modifiers());
            // Re-run the scene's condition walk against the live story-flag
            // bank before rolling. Retail does this every step, and it is not
            // cosmetic: a scene's regions are several story-state variants
            // laid end to end, so the group choice decides which rates,
            // formations and backdrop the step can see at all.
            tracker.select_group(|flag| self.encounter_condition_flag_test(flag));
            // The reader's battle-setup half runs on every step that lands in
            // a region, before - and regardless of - the roll.
            if let Some(region) = tracker.table().region_at_world(wx, wz) {
                let setup =
                    legaia_engine_battle::region_encounter::region_battle_setup(&region.setup);
                self.store_region_battle_setup(setup);
            }
            // The Incense window (`_DAT_8007B600`) skips the roll outright
            // while it is non-zero - `lw v0,-0x4a00(v0)` / `bne v0,zero` at
            // `0x801DA174`, after the setup half and before the rate scale,
            // so the step counter does not move either.
            if self.locomotion.walk_regen_window != 0 || self.encounter_roll_held() {
                self.terrain.region_tracker = Some(tracker);
                return false;
            }
            let roll = tracker.on_step(wx, wz, || self.next_rand());
            // Per-step roll diagnostics (trace level; off in normal runs):
            // which tile the step landed on and how far the region counter
            // has drained. A step outside every region logs no counter
            // movement - the signature of "walking where nothing rolls".
            log::trace!(
                "field step at ({wx}, {wz}): region counter {}",
                tracker.counter()
            );
            self.encounters.step_counter = tracker.counter();
            self.terrain.region_tracker = Some(tracker);
            return match roll {
                Some(r) => {
                    let er = legaia_engine_battle::encounter::EncounterRoll {
                        formation_id: r.formation_id as u16,
                        row_index: r.formation_id as usize,
                        roll_q8: 0,
                    };
                    log::info!(
                        "encounter: region roll in '{}' at ({wx}, {wz}) -> formation row {}",
                        self.active_scene_label,
                        r.formation_id
                    );
                    match self.encounters.session.as_mut() {
                        Some(s) => s.trigger_with(er),
                        None => {
                            // Unreachable: the bracket is installed above.
                            log::error!(
                                "encounter: region roll dropped - no session sink after the \
                                 bracket install"
                            );
                            false
                        }
                    }
                }
                None => false,
            };
        }
        // Same Incense and engaged gates on the mean-rate stand-in (see the
        // region arm).
        if self.locomotion.walk_regen_window != 0 || self.encounter_roll_held() {
            return false;
        }
        let rng = self.next_rng();
        let modifiers = self.encounter_rate_modifiers();
        match self.encounters.session.as_mut() {
            Some(session) => {
                session.tracker_mut().set_rate_modifiers(modifiers);
                session.on_step(rng)
            }
            None => false,
        }
    }

    /// Per-frame tick of the encounter session timers. Drives the
    /// `Transition` and `Grace` countdowns, and - while the session is in
    /// `Transition` - the field-to-battle intro state machine.
    pub fn tick_encounter(&mut self) {
        if let Some(session) = self.encounters.session.as_mut() {
            session.tick_frame();
        }
        self.tick_battle_intro();
    }

    /// The battle's **first monster id** - retail's `DAT_8007BD0C`, slot 0 of
    /// the resolved formation cell.
    ///
    /// Three sources, in the order retail's own value becomes available:
    ///
    /// 1. the rolled formation row's own slot 0. This is the only source that
    ///    answers while the encounter sits in its `Transition` phase, which is
    ///    when the intro is armed: the world is still in Field mode there and
    ///    [`World::battle_monster_slots`] returns empty outside
    ///    `SceneMode::Battle`;
    /// 2. the live actor table, for an in-battle re-arm (a second wave, a
    ///    scripted re-entry) where the formation row is not the authority;
    ///    and
    /// 3. the formation id itself - a **row index**, not a monster id, and
    ///    therefore wrong for the selector. It is the last resort only
    ///    because the emitter must produce some style.
    pub fn battle_intro_slot0(&self, formation_id: u16) -> u8 {
        self.tables
            .formation_table
            .formation(formation_id)
            .and_then(|d| d.slots.first())
            .map(|s| s.monster_id as u8)
            .or_else(|| {
                self.battle_monster_slots()
                    .first()
                    .map(|&(_, id, _)| id as u8)
            })
            .unwrap_or(formation_id as u8)
    }

    /// All three intro-style selector inputs for `formation_id` - the retail
    /// globals `DAT_8007BD60`, `DAT_8007BD0C` and `DAT_80084540`.
    ///
    /// `battle_flags` is `DAT_8007BD60`, of which the selector reads only bit
    /// `0x80`; the entity SM's confirm state ORs that bit in when the rolled
    /// row's `record[+0]` is non-zero (`FUN_801DA51C` at
    /// `0x801DA5F8..0x801DA61C`), which is what makes the scripted / boss arm
    /// reachable at all. `scene_index` is `DAT_80084540`, the loaded scene's
    /// raw CDNAME define ([`crate::world::BattleState::map_id`]) - not
    /// `Scene::start`, which is two below it in the extraction frame. None of
    /// the three is a host choice, so both hosts and the transition length
    /// read them from here.
    pub fn battle_intro_style_inputs(
        &self,
        formation_id: u16,
    ) -> legaia_render_kernels::battle_intro_styles::IntroStyleInputs {
        legaia_render_kernels::battle_intro_styles::IntroStyleInputs {
            battle_flags: self
                .tables
                .formation_table
                .formation(formation_id)
                .map(|d| d.per_battle_flags())
                .unwrap_or(0),
            formation_slot0: self.battle_intro_slot0(formation_id),
            scene_index: self.battle.map_id,
        }
    }

    /// One frame of the field-to-battle transition entity, `FUN_801CF5BC`.
    ///
    /// The encounter session's `Transition` phase *is* this transition: retail
    /// runs the intro overlay (PROT 0979 `field_battle_intro`) between the
    /// encounter trigger and the battle scene coming up, and its entity's
    /// `+0x22` phase counter sequences the same handoff the engine performs at
    /// [`World::enter_battle_from_formation`]. The entity is armed on the frame
    /// the session enters `Transition` and dropped when it leaves.
    ///
    /// Two of the kernel's effects have engine-side consumers. The phase-2
    /// `LoadBattleBgm` is the point retail starts the battle track - *during*
    /// the spin, not after it. Routing it here is what moves the swap off
    /// [`World::enter_battle_from_formation`]; the swap is idempotent, so the
    /// later call is a no-op once this one has run. The phase-0 `SetAudioCue`
    /// is the battle-start sound: retail stores the cue id (`0x1F` plain,
    /// `0x4D` for a flagged/boss row) straight into slot 0 of the pending SFX
    /// ring `_DAT_8007B6D8`, and the engine's carrier for that ring is
    /// [`crate::world::AudioState::battle_sfx_cues`] - the queue both hosts drain into their SFX
    /// scheduler every frame - so the cue is pushed there. Because retail's
    /// store is a slot-0 **overwrite** (no ring-counter bump), only the last
    /// `SetAudioCue` of the tick is pushed. The remaining effects (mesh
    /// assembly, the bundle read, the load waits) are recorded in
    /// [`crate::world::BattleState::intro_effects`] for a host that owns those reads.
    ///
    /// The kernel's own load-wait response is reported idle: the engine has
    /// already resolved the formation by the time the session reaches
    /// `Transition`, so nothing is in flight to hold a phase.
    ///
    /// PORT: FUN_801CF5BC (live wiring; the kernel is
    /// `legaia_render_kernels::battle_intro_transition::tick_transition`)
    fn tick_battle_intro(&mut self) {
        use legaia_engine_battle::encounter::EncounterPhase;
        use legaia_render_kernels::battle_intro_transition::{
            TransitionEffect, TransitionGlobals, TransitionResponses, tick_transition,
        };

        let phase = self.encounters.session.as_ref().map(|s| s.phase());
        let Some(EncounterPhase::Transition {
            frames_remaining,
            roll,
        }) = phase
        else {
            self.battle.intro = None;
            self.battle.intro_effects.clear();
            self.battle.intro_mode_handoff = false;
            return;
        };
        // First frame of the transition: the style the intro's init picks
        // fixes `DAT_801D2458` (132, or 252 for the swirl), and the session
        // armed before it was known. Rebase the countdown onto it. A session
        // whose length a caller set to something else (a test or tool
        // shortening the bracket) keeps its own.
        if self.battle.intro.is_none() {
            use legaia_render_kernels::battle_intro_styles::{
                INTRO_DURATION_FRAMES, INTRO_DURATION_FRAMES_SWIRL, intro_duration_frames,
                select_intro_style,
            };
            let style =
                select_intro_style(&self.battle_intro_style_inputs(roll.formation_id)).style;
            let frames = intro_duration_frames(style) as u16;
            if let Some(session) = self.encounters.session.as_mut()
                && [INTRO_DURATION_FRAMES, INTRO_DURATION_FRAMES_SWIRL]
                    .contains(&i32::from(session.transition_frames))
            {
                session.set_transition_length(frames);
            }
        }
        let frames_remaining = match self.encounters.session.as_ref().map(|s| s.phase()) {
            Some(EncounterPhase::Transition {
                frames_remaining, ..
            }) => frames_remaining,
            _ => frames_remaining,
        };
        let total = self
            .encounters
            .session
            .as_ref()
            .map(|s| s.transition_frames)
            .unwrap_or(0);
        let mut entity = self.battle.intro.take().unwrap_or_default();
        // `+0x1A` counts display frames against `DAT_801D2458`; the session's
        // own countdown is the same clock read the other way round.
        entity.elapsed = total.saturating_sub(frames_remaining) as i16;
        let globals = TransitionGlobals {
            // `_DAT_8007B880` - the battle sound set the field script left
            // (op-`0x35` sub-op `7`; zeroed on every scene entry), not the
            // formation: nothing on the encounter path writes the word. `-1`
            // is a script's "no battle track" choice, and the only arm that
            // reads the flags byte for the cue.
            battle_id: self.audio.battle_sound_set,
            // `DAT_8007B64B`: the region record's alternate-default-bundle
            // bit, read by phase 2 only for sound set `0`.
            alt_default_bundle: u8::from(
                self.encounters
                    .region_setup
                    .and_then(|s| s.keep_backdrop_object_1)
                    .unwrap_or(false),
            ),
            // `DAT_8007BD0C` - the formation cell's slot 0, which the
            // completion arm compares against `0xA6`.
            formation_slot0: self
                .tables
                .formation_table
                .formation(roll.formation_id)
                .and_then(|d| d.slots.first())
                .map_or(0, |s| s.monster_id as u8),
            total_duration: i32::from(total),
            // `DAT_8007BD60`. Bit `0x80` is the only bit this kernel reads,
            // and it is a property of the rolled formation row: the entity
            // SM's confirm state raises it when the row's `record[+0]` is
            // non-zero (`FUN_801DA51C` at `0x801DA5F8..0x801DA61C`). The
            // scripted / boss rows are exactly those rows, and the raised bit
            // makes the flagged battle-start cue (`0x4D`) overwrite the plain
            // one (`0x1F`) in ring slot 0.
            battle_flags: self
                .tables
                .formation_table
                .formation(roll.formation_id)
                .map(|d| d.per_battle_flags())
                .unwrap_or(0),
            // `DAT_8007B648 == 0x80` is retail's "battle-mesh assembly
            // finished" byte, which phase 1 spins on. The engine assembles
            // battle meshes synchronously at battle entry, so the answer is
            // always "done" and the phase passes on its first tick.
            assembly_state: 0x80,
            // `DAT_80070536` - the field-BGM sound-source actor's bound voice
            // id (`0x8007052C + 0xA`), runtime-written when the field track
            // attaches. The engine's BGM director owns a single sequencer
            // slot, so its id is 0. Structurally unread on this path: the
            // `ReapplyBgmLevel` effect only fires for a non-negative battle
            // id, and the phase-0 tick passes the sentinel (above).
            bgm_voice_id: 0,
            // `_DAT_8007B910` - the live audio level. The engine models no
            // field-mode duck, so the level sits where the MAN loader rests
            // it: the configured level `_DAT_8008457C` (`0xD7` from a cold
            // boot, a loaded save's own word after a load;
            // `docs/subsystems/battle-action.md` § audio duck).
            audio_level: self.audio.levels.configured_level,
            ..Default::default()
        };
        let tick = tick_transition(&mut entity, &globals, &TransitionResponses::default());
        self.battle.intro = Some(entity);
        let mut battle_start_cue = None;
        for effect in &tick.effects {
            match effect {
                TransitionEffect::LoadBattleBgm { .. } => self.swap_to_battle_bgm(),
                // Retail's slot-0 overwrite: the last cue written this tick
                // is the one the ring drainer plays.
                TransitionEffect::SetAudioCue { cue } => battle_start_cue = Some(*cue),
                // `SsSeqSetVol(field_bgm_voice, level, 100)` - re-applies the
                // live audio level to the field track as the spin starts.
                // The engine's BGM director already plays at its configured
                // level and the world models no live duck in field mode, so
                // there is nothing to re-apply - a no-op here, disclosed
                // rather than faked. (Unreachable on this path anyway: the
                // phase-0 sentinel skips it, as retail's `blez` arm does.)
                TransitionEffect::ReapplyBgmLevel { .. } => {}
                _ => {}
            }
        }
        if let Some(cue) = battle_start_cue {
            // The battle-start sound (`0x1F` plain / `0x4D` flagged). `kind`
            // is the resolved ring id, which for sub-`0x64` cues is the
            // static SFX descriptor index both hosts' schedulers key their
            // banks by; slot fields are HUD context only - no actor is
            // attacking during the spin.
            self.audio
                .battle_sfx_cues
                .push(legaia_engine_battle::battle_events::BattleSfxCue {
                    kind: cue,
                    timing_frames: 0,
                    actor_slot: 0,
                    target_slot: 0,
                });
        }
        // The completion arm zeroes the sound set after the fight whose
        // first monster is `0xA6` (`0x801CF934..0x801CF94C`): the Gaza fight
        // selects `-1` for its own score, and the next fight must not
        // inherit it.
        if tick.cleared_battle_id {
            self.audio.battle_sound_set = 0;
        }
        self.battle.intro_effects = tick.effects;
        // The master mode hand-off (`_DAT_8007B83C = 0x14` at `0x801CF8F8`).
        // Latching, not level-triggered: retail writes the word once, at the
        // end of the spin, and never unwrites it inside the transition.
        self.battle.intro_mode_handoff |= tick.entered_battle_mode;
    }

    /// Whether the battle-intro spin is still holding the master mode word
    /// back from `BATTLE MODE`.
    ///
    /// The engine seats the battle **scene** synchronously at the trigger
    /// (`World::enter_battle_from_formation`), where retail seats it at the
    /// end of the spin, so `World::mode` reads `Battle` for the whole
    /// transition. The mode *word* must not follow it that early:
    /// `FUN_801CF5BC` writes `_DAT_8007B83C = 0x14` only once the intro clock
    /// has passed its full duration **and** `TransitionEntity::ready == 3`
    /// (`legaia_render_kernels::battle_intro_transition::TransitionTick::entered_battle_mode`).
    /// [`legaia_engine_mode::mode::ModeSeat::adopt_world_mode`] consults this, so both
    /// hosts take the retail edge rather than the trigger frame.
    ///
    /// False outside a transition, so nothing else is gated by it.
    pub fn battle_mode_word_held(&self) -> bool {
        matches!(
            self.encounters.session.as_ref().map(|s| s.phase()),
            Some(legaia_engine_battle::encounter::EncounterPhase::Transition { .. })
        ) && !self.battle.intro_mode_handoff
    }

    /// `true` from the frame a battle is committed (a latched scripted fight,
    /// or the encounter session's intro transition) until it opens: the
    /// window in which no field script context may step.
    ///
    /// Retail's battle intro is its own slot-A overlay (PROT 0979,
    /// `field_battle_intro`, base `0x801CE818`, own content `0x4000`), loaded
    /// over the field overlay's head - which holds the field frame pump
    /// `FUN_801D1344` that drives the script contexts. So between the
    /// commit and the battle nothing field-side runs, and a scene's
    /// post-battle beat only starts on the return. The Rim Elm bee beat
    /// depends on it: `P1[21]` raises `0x5C0` and fires `3E FF 03`, and the
    /// scene system script `P1[0]`, whose loop tests `0x5C0`, spawns the
    /// outcome record `P2[29]` (a flag-1 test at its head) - on the
    /// post-battle pass, where flag 1 holds this fight's outcome.
    // REF: FUN_801D1344, FUN_801CF5BC
    pub fn field_scripts_held_for_battle(&self) -> bool {
        self.carriers.pending_battle.is_some()
            || matches!(
                self.encounters.session.as_ref().map(|s| s.phase()),
                Some(legaia_engine_battle::encounter::EncounterPhase::Transition { .. })
            )
    }

    /// `true` while a step must not roll a random encounter: retail's region
    /// reader `FUN_801D9E1C` returns without scaling the rate or draining the
    /// counter when the player carries the engaged bit
    /// (`*(_DAT_8007C364)+0x10 & 0x80000`), while the dialogue-pacing
    /// countdown `_DAT_8007B6B4` is running, or while the kind-0 warp timer
    /// `_DAT_8007B6B0` is up (`0x801DA130..0x801DA164` - after the region's
    /// battle-setup half, ahead of the Incense test).
    ///
    /// The engaged bit is raised by a talk or touch and by the script runner
    /// `FUN_80039B7C` on every frame it steps a context
    /// (`0x80039DB8..0x80039DD4`), so a door record carrying the party out,
    /// a scripted walk and an open box all hold the roll; the engine's
    /// readings of the three are [`Self::dialogue_owns_input`],
    /// [`Self::script_context_engages_player`] and
    /// [`Self::field_warp_in_flight`]. A step onto an overworld door
    /// therefore never also starts a fight.
    ///
    /// REF: FUN_801D9E1C (`0x801DA130..0x801DA164`), FUN_80039B7C
    pub fn encounter_roll_held(&self) -> bool {
        self.dialogue_owns_input()
            || self.script_context_engages_player()
            || self.field_warp_in_flight()
            || self
                .player_actor_slot
                .and_then(|s| self.actors.get(usize::from(s)))
                .is_some_and(|a| a.move_state.flags & 0x0008_0000 != 0)
    }

    /// `true` from the step a random or scripted fight is committed until it
    /// opens. Retail's region reader raises the player's engaged bit on the
    /// trigger itself (`player+0x10 |= 0x80000`, `0x801DA2C0..0x801DA2D8`),
    /// so the pad controller stops walking the player for the whole battle
    /// intro: a step that rolled a fight cannot carry on onto a door.
    ///
    /// REF: FUN_801D9E1C (`0x801DA2C0..0x801DA2D8`)
    pub fn encounter_owns_player(&self) -> bool {
        self.carriers.pending_battle.is_some()
            || matches!(
                self.encounters.session.as_ref().map(|s| s.phase()),
                Some(
                    legaia_engine_battle::encounter::EncounterPhase::Transition { .. }
                        | legaia_engine_battle::encounter::EncounterPhase::Triggered(_)
                )
            )
    }

    /// Return the resolved [`legaia_engine_battle::monster_catalog::FormationDef`] for the
    /// currently-triggered encounter, if any. Engines call this after the
    /// session reports `Triggered` to drain the roll and resolve into a
    /// concrete monster set; the session advances to `Battling` as a
    /// side-effect.
    pub fn drain_encounter_formation(
        &mut self,
    ) -> Option<legaia_engine_battle::encounter::EncounterRoll> {
        self.encounters
            .session
            .as_mut()
            .and_then(|s| s.drain_triggered())
    }

    /// Mark that the active battle finished. Engines call this from the
    /// post-battle resolution path so the session enters its grace window
    /// (suppresses encounters for `grace_frames` frames).
    pub fn end_encounter_battle(&mut self) {
        if let Some(session) = self.encounters.session.as_mut() {
            session.end_battle();
        }
    }
}
