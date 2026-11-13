//! The battle frame clock's `World` half: the per-tick frame id and step
//! reads over `legaia_engine_battle_loop::frame_clock::BattleFrameClock`.

use super::*;

impl World {
    /// The battle frame the current tick falls in
    /// ([`BattleFrameClock::frame_of`](legaia_engine_battle_loop::frame_clock::BattleFrameClock::frame_of) at `display_frames`).
    pub fn battle_frame_id(&self) -> u64 {
        self.battle.frame_clock.frame_of(self.clock.display_frames)
    }

    /// Run the battle on frames of `vsyncs` from this tick on: the
    /// root-motion carry's frames and the camera's steps. A replay sets it
    /// from the step retail ran at; play keeps [`DEFAULT_BATTLE_FRAME_STEP`].
    pub fn set_battle_frame_step(&mut self, vsyncs: u8) {
        let tick = self.clock.display_frames;
        self.battle.frame_clock.set_step(tick, vsyncs);
        if let Some(cam) = self.battle.camera.as_mut() {
            cam.set_frame_step(vsyncs);
        }
    }

    /// The step of a battle frame nothing else names: the frame driver's law
    /// over a measured cost, read on the last pass
    /// ([`legaia_engine_battle_loop::frame_clock::FrameCostRing`]).
    pub fn ordinary_battle_frame_step(&self) -> u8 {
        if self.toggles.battle_party_cost_step {
            self.battle.frame_cost_step
        } else {
            DEFAULT_BATTLE_FRAME_STEP
        }
    }

    /// One pass's push into the cost ring: the frame's measured cost, or what an
    /// arm table's step leaves there, then the step that reads. An ordinary
    /// frame's cost is its party and action state's measured base plus the
    /// draw load the frame carries ([`Self::battle_frame_load`],
    /// [`frame_load_cost`](legaia_engine_battle_loop::frame_clock::frame_load_cost)).
    ///
    /// Returns what the frame's own length follows
    /// ([`frame_span`](legaia_engine_battle_loop::frame_clock::frame_span)): the
    /// cost an ordinary frame pushed, and on an arm the cost of the state's work
    /// under a loading clip's polls. An arm table's step is its frames' own
    /// measurement and no cost, so an arm no clip loads returns `0`, a frame
    /// inside its step.
    fn push_frame_cost(&mut self, arm_step: Option<u8>) -> u16 {
        use legaia_engine_battle_loop::frame_clock::{cost_of_step, measured_frame_cost_at};
        let state = self.battle_ctx.action_state;
        let passes = match self.battle.frame_cost_state {
            Some((s, n)) if s == state => n.saturating_add(1),
            _ => 0,
        };
        self.battle.frame_cost_state = Some((state, passes));
        let xa_passes = self.battle_xa_passes_since_cue();
        let work = measured_frame_cost_at(self.party.party_count, state, passes)
            .saturating_add(self.battle_frame_load_cost());
        // An XA clip's read span: the frame runs on the CD interrupt path
        // (`FUN_8005DAB0`) once the stream is up.
        let loaded = xa_passes.map(|n| {
            legaia_engine_battle_loop::frame_clock::xa_stream_pass_cost(
                n,
                work,
                self.toggles.battle_cd_timing,
                self.toggles.battle_frame_span_law && self.audio.battle_xa_poll_load,
            )
        });
        let cost = arm_step.map_or(loaded.unwrap_or(work), cost_of_step);
        let cost = match self.battle.drawn_table_cost {
            Some(gpu) if self.toggles.battle_gpu_cost_step => cost.max(gpu),
            _ => cost,
        };
        self.battle.frame_cost_step = self.battle.frame_cost_ring.push(cost);
        // A capture-band module's own line leaves its arms their table's
        // length: retail's frames under one run anywhere from inside the step
        // (Death Game, four vsyncs a step-`4` frame) to eight (Kiss of Death),
        // which no one share of a table cost reads.
        match arm_step {
            None => cost,
            Some(_) if self.audio.battle_xa_poll_load => loaded.unwrap_or(0),
            Some(_) => 0,
        }
    }

    /// Note the GPU's wait over the battle frame the host just drew, in
    /// hblanks (draw plus DMA of its table,
    /// `legaia_engine_system::gpu_draw_cost`). The next pass's cost is the
    /// larger of its CPU term and this while
    /// `WorldToggles::battle_gpu_cost_step` is up: retail's entry is
    /// `max(cpu, dma + draw of the previous pass's table)`, the table the GPU
    /// draws while the pass runs (`DrawOTag` at `0x80017288`, `DrawSync(0)`
    /// at `0x800174D8`). A no-op outside a battle.
    pub fn note_drawn_table_cost(&mut self, hblanks: u16) {
        if self.mode == legaia_engine_mode::mode::SceneMode::Battle {
            self.battle.drawn_table_cost = Some(hblanks);
        }
    }

    /// Open the window a clip just started loads the frame for
    /// ([`crate::world::AudioState::battle_xa_load_frames`]): its span, plus
    /// PCSX-Redux's read tail.
    pub(in crate::world) fn arm_battle_xa_load_window(&mut self) {
        use legaia_engine_battle_loop::frame_clock::{CdTiming, XA_READ_TAIL_VSYNCS};
        let tail = match self.toggles.battle_cd_timing {
            CdTiming::PcsxRedux if self.toggles.battle_frame_span_law => XA_READ_TAIL_VSYNCS,
            _ => 0,
        };
        self.audio.battle_xa_load_frames = self.audio.battle_xa_busy_frames.saturating_add(tail);
        // The drive word `0x8007BC20` holds `2` from the start (`0x8003D65C`)
        // until the stop `FUN_8003EE00` clears it once the read is done
        // (`0x8003EE2C`), not for the span alone: a `--watch` recording of
        // Medusa's Glare clears it `559` vsyncs after the start of its
        // `539`-vsync line, and the shout fallback `0x290` plays inside that.
        if tail != 0 {
            self.audio.battle_xa_busy_frames = self.audio.battle_xa_load_frames;
        }
    }

    /// The passes since a loading battle CD-XA clip's cue, `None` while none
    /// plays ([`legaia_engine_battle_loop::frame_clock::xa_stream_pass_cost`]).
    fn battle_xa_passes_since_cue(&self) -> Option<u64> {
        if self.audio.battle_xa_load_frames == 0 || !self.audio.battle_xa_frame_load {
            return None;
        }
        Some(
            self.audio
                .battle_xa_cue_frame
                .map_or(u64::MAX, |f| self.battle_frame_id().saturating_sub(f)),
        )
    }

    /// What a battle frame draws past its state's base: the live move-VM
    /// parts of every effect scene (the cast's, the move's, the effect-script
    /// records', the Mystic Shield's and the stage's) and the after-image
    /// ghosts the draw tick's walk `FUN_80049348` re-draws a body for
    /// ([`legaia_engine_battle::battle_afterimage::plan_ghosts`], the plan
    /// both hosts draw from). `(parts, ghosts)`.
    pub fn battle_frame_load(&self) -> (u16, u16) {
        use legaia_engine_battle::battle_afterimage as ai;
        let c = &self.casting;
        let scene_parts = |s: &legaia_engine_effects::summon::SummonScene| {
            s.parts.iter().filter(|p| !p.finished).count()
        };
        let parts = c.active_summon.as_ref().map_or(0, scene_parts)
            + c.active_move_fx.as_ref().map_or(0, scene_parts)
            + c.active_action_fx.iter().map(scene_parts).sum::<usize>()
            + c.mystic_shield_fx.as_ref().map_or(0, scene_parts)
            + c.stage_fx.as_ref().map_or(0, scene_parts);
        let per_frame = self.ghost_ring_slots_per_frame();
        let mut ghosts = 0;
        for a in &self.actors {
            // A hidden body (the tint pass's colour word at zero) skips the
            // walk with the rest of its draw (`0x800481CC`).
            if a.battle_pose_history.is_empty()
                || (a.battle.render_color == 0 && a.battle.render_flag != 0)
            {
                continue;
            }
            let hist = &a.battle_pose_history;
            ghosts += ai::plan_ghosts(
                a.battle.anim_rate.get(),
                a.battle_monster_id.is_some(),
                [0; 3],
                |d| {
                    d < ai::HISTORY_DEPTH
                        && hist.get(per_frame * d).is_some_and(|f| f.ghost_eligible)
                },
            )
            .len();
        }
        (
            u16::try_from(parts).unwrap_or(u16::MAX),
            u16::try_from(ghosts).unwrap_or(u16::MAX),
        )
    }

    /// What a battle frame's draw load costs over its state's base, in
    /// hblanks: the live move-VM parts
    /// ([`EFFECT_PART_COST`](legaia_engine_battle_loop::frame_clock::EFFECT_PART_COST)),
    /// the effect pool's live children
    /// ([`effect_pool_cost`](legaia_engine_battle_loop::frame_clock::effect_pool_cost)),
    /// and each planned after-image ghost at its body's polygon load
    /// ([`ghost_draw_cost`](legaia_engine_battle_loop::frame_clock::ghost_draw_cost)):
    /// a ghost re-runs the body's whole draw (`FUN_80049348` calls
    /// `FUN_80048A08` per ghost), so a boss's ghost costs what its mesh does.
    pub fn battle_frame_load_cost(&self) -> u16 {
        use legaia_engine_battle::battle_afterimage as ai;
        use legaia_engine_battle_loop::frame_clock::{
            EFFECT_PART_COST, effect_pool_cost, ghost_draw_cost,
        };
        let (parts, _) = self.battle_frame_load();
        let pool = self
            .effect_pool
            .children
            .iter()
            .filter(|c| c.frame_count != 0)
            .count();
        let mut cost = parts
            .saturating_mul(EFFECT_PART_COST)
            .saturating_add(effect_pool_cost(u16::try_from(pool).unwrap_or(u16::MAX)));
        let per_frame = self.ghost_ring_slots_per_frame();
        for a in &self.actors {
            if a.battle_pose_history.is_empty()
                || (a.battle.render_color == 0 && a.battle.render_flag != 0)
            {
                continue;
            }
            let hist = &a.battle_pose_history;
            let n = ai::plan_ghosts(
                a.battle.anim_rate.get(),
                a.battle_monster_id.is_some(),
                [0; 3],
                |d| {
                    d < ai::HISTORY_DEPTH
                        && hist.get(per_frame * d).is_some_and(|f| f.ghost_eligible)
                },
            )
            .len();
            let triangles = a
                .battle_body_triangles
                .filter(|&(id, _)| Some(id) == a.battle_monster_id)
                .map(|(_, t)| t)
                .filter(|&t| t > 0);
            let each = ghost_draw_cost(triangles);
            cost = cost.saturating_add(each.saturating_mul(u16::try_from(n).unwrap_or(u16::MAX)));
        }
        cost
    }

    /// Stand the frame-cost ring where a retail capture's stood (its sixteen
    /// `0x80084098` entries and the cursor `gp+0x440`) and run ordinary frames
    /// on the ring from here on, the harness's pin to the default step
    /// (`LEGAIA_BATTLE_PARTY_STEP=0`) lifted: what a replay that reached the
    /// capture on the default step runs once it stands there. A harness seat.
    pub fn seed_frame_cost_ring(&mut self, entries: [u16; 16], cursor: u8) {
        use legaia_engine_battle_loop::frame_clock::FrameCostRing;
        let ring = FrameCostRing::from_entries(entries, cursor);
        self.battle.frame_cost_ring = ring;
        self.battle.frame_cost_step = ring.step();
        self.toggles.battle_party_cost_step = true;
    }

    /// Seed a replay's battle frame step: `step` vsyncs a frame on every tick
    /// the action SM sits in `state`, the default everywhere else
    /// ([`legaia_engine_vm::frame_clock::FrameClock::battle_frame_step_seed`]).
    pub fn seed_battle_frame_step(&mut self, step: u8, state: u8) {
        self.clock.battle_frame_step_seed = Some((step.max(1), state));
    }

    /// Stretch the battle frame the current tick falls in to `vsyncs`,
    /// keeping its id, so the next pass waits that long from the frame's
    /// opening tick and runs with `vsyncs` as its step; the frame that pass
    /// opens returns to the current step
    /// (`World::settle_battle_frame_stretch`). One retail frame whose cost
    /// ran a step past the rest, at a point the replay picks: retail's step
    /// is the frame's measured cost (`FUN_80016B6C`), so a capture's clocks
    /// can sit on a lattice the replay's fixed step never lands on. A no-op
    /// on the per-vsync cadence, or while a stretch is pending.
    pub fn stretch_battle_frame(&mut self, vsyncs: u8) {
        if !self.toggles.battle_sm_frame_cadence || self.battle.frame_stretch.is_some() {
            return;
        }
        let tick = self.clock.display_frames;
        let clock = self.battle.frame_clock;
        let start = tick - clock.ticks_into_frame(tick);
        let id = clock.frame_of(tick);
        self.battle.frame_clock.retime_frame(start, vsyncs);
        self.battle.frame_stretch = Some((id, clock.step));
        if let Some(cam) = self.battle.camera.as_mut() {
            cam.set_frame_step(vsyncs);
        }
    }

    /// Return a stretched clock ([`Self::stretch_battle_frame`]) to its
    /// step once the pass after the stretched frame has run: that pass's
    /// own frame takes the old step, under the same id.
    pub(in crate::world) fn settle_battle_frame_stretch(&mut self) {
        let Some((id, step)) = self.battle.frame_stretch else {
            return;
        };
        let tick = self.clock.display_frames;
        let clock = self.battle.frame_clock;
        let now = clock.frame_of(tick);
        if now <= id || self.battle.sm_pass_frame != Some(now) {
            return;
        }
        let start = tick - clock.ticks_into_frame(tick);
        self.battle.frame_clock.retime_frame(start, step);
        self.battle.frame_stretch = None;
        if let Some(cam) = self.battle.camera.as_mut() {
            cam.set_frame_step(step);
        }
    }

    /// Whether the action state is the one a replay's seeded step speaks for.
    /// A seed on the magic band's sustained state `0x2B` also opens on `0x2A`:
    /// the pass after the clip commit is the `0x2A` pass storing `0x2B`, and it
    /// runs on the step the capture's ring holds for it.
    fn battle_frame_step_seed_holds(&self, state: u8) -> bool {
        self.battle_ctx.action_state == state
            || (state == 0x2B && self.battle_ctx.action_state == 0x2A)
    }

    /// Install a seeded step while the action SM sits in its state, and hand
    /// the frame back to the ordinary step on the pass that leaves it. Called
    /// after every action-SM step.
    pub(in crate::world) fn apply_battle_frame_step_seed(&mut self) {
        let Some((step, state)) = self.clock.battle_frame_step_seed else {
            return;
        };
        // Scoped to the state: the ring a capture holds speaks for its own
        // state's frames, not for the states around it.
        let reached = self
            .battle
            .frame_step_seed_from
            .is_none_or(|(clip, cursor)| {
                self.actors
                    .get(usize::from(self.battle_ctx.active_actor))
                    .is_some_and(|a| {
                        (a.battle.current_anim == clip || a.battle_reaction_entry == Some(clip))
                            && a.battle_animation
                                .as_ref()
                                .is_some_and(|p| p.cursor_sixteenths() as i32 >= cursor)
                    })
            })
            && self.battle.frame_step_seed_after_accum.is_none_or(|hold| {
                self.battle
                    .camera
                    .as_ref()
                    .is_some_and(|c| c.close_up_accum() >= hold)
            });
        let in_state = self.battle_frame_step_seed_holds(state);
        // Outside its state the seed hands the frame back once, on the pass
        // that leaves it; from there the arm tables and the ring own the
        // step (`Self::apply_cast_arm_frame_step`). Taking the ordinary step
        // on every pass outside it overrode a summon band's arm steps for
        // the whole cast (`theeder_summon_mid_cast`, seeded on `0x33`).
        let left = !in_state && std::mem::take(&mut self.battle.frame_step_seed_in_state);
        self.battle.frame_step_seed_in_state = in_state;
        if !in_state && !left {
            return;
        }
        let want = if in_state && reached {
            step
        } else {
            self.ordinary_battle_frame_step()
        };
        if self.battle.frame_clock.step != want {
            if self.toggles.battle_sm_frame_cadence {
                // The pass this tick ran opened the frame the new step
                // spans.
                let tick = self.clock.display_frames;
                self.battle.frame_clock.retime_frame(tick, want);
                if let Some(cam) = self.battle.camera.as_mut() {
                    cam.set_frame_step(want);
                }
            } else {
                self.set_battle_frame_step(want);
            }
        }
    }

    /// Run a summon module's arm on the frame step retail runs it at
    /// ([`legaia_engine_cast::cast_module_frame_step`]): a frame that draws
    /// a whole-screen effect costs retail four vsyncs, so its part ticks, its
    /// scrollers and its per-pass seats all take that step. Called beside
    /// [`Self::apply_battle_frame_step_seed`], which a replay's own step
    /// outranks, and only on retail's cadence: the per-vsync cadence has no
    /// frames to stretch.
    pub(in crate::world) fn apply_cast_arm_frame_step(&mut self) {
        use legaia_engine_battle_vm::battle_action::ActionState as S;
        if !self.toggles.battle_sm_frame_cadence {
            self.battle.pending_frame_step = None;
            return;
        }
        // A recording's own steps outrank a capture's seeded one: the track
        // holds every frame the seed's one state does. Before the film begins
        // the fight runs on its own rules.
        if self
            .battle
            .retail_step_track
            .as_ref()
            .is_some_and(|t| t.armed())
        {
            // A harness seat runs the recording's own steps whole: no overrun.
            self.battle.pending_frame_step = None;
            self.battle
                .frame_clock
                .retime_span(self.clock.display_frames, 0);
            self.apply_retail_step_track();
            return;
        }
        if self
            .clock
            .battle_frame_step_seed
            .is_some_and(|(_, state)| self.battle_frame_step_seed_holds(state))
        {
            self.battle.pending_frame_step = None;
            self.battle
                .frame_clock
                .retime_span(self.clock.display_frames, 0);
            return;
        }
        let state = self.battle_ctx.action_state;
        let band = state == S::SummonSustain.as_byte() || state == S::SummonReturn.as_byte();
        let measured = self
            .casting
            .summon_stager
            .as_ref()
            .filter(|_| band)
            .and_then(|st| self.cast_module_for(st.spell_id))
            .and_then(|entry| {
                legaia_engine_cast::cast_module_frame_step::cast_arm_frame_step(
                    entry,
                    self.casting.module_phase,
                    self.toggles.battle_cd_timing,
                )
            });
        let measured = measured.or_else(|| {
            let spell = self
                .casting
                .capture_spell
                .filter(|_| state == S::MagicCapturePhase2.as_byte())?;
            legaia_engine_cast::cast_module_frame_step::capture_arm_frame_step(
                spell,
                self.casting.module_phase,
                self.toggles.battle_cd_timing,
            )
        });
        // `FUN_80016B6C` closes a pass by storing the step this frame's cost
        // reads, waits `VSync` on the one the pass ran at (`0x8001719C`) and
        // only then copies the new one across (`0x800171D8`): the frame this
        // pass opened runs out on the step it ran at, and a cost pushed now
        // first times the next pass, which also drains its timers by it
        // (`battle-stage-camera.md`, the frame step). An arm table's step is
        // the cost retail's frames ran under the arm, pushed like any other.
        let span_cost = self.push_frame_cost(measured);
        // An arm table names the cost of the arm's own frames, which the push
        // above stored: the ring's step after it is the larger of the arm's and
        // what the passes before left, so a cheaper arm after a dear one runs
        // the ring's step until the dear entries leave it (Kiss of Death's arm
        // 5 runs `3` for sixteen passes after arm 4's clamped frames, then `2`).
        // The frame this pass opened runs out on the step the pass ran at and
        // spans the blanks its cost runs past (`VSync(n)`, `frame_span`): a
        // dear first pass on a cheap step, and every frame a clip's polls load
        // (`XA_POLL_COST_RATIO_MILLI`), arm or not.
        let step = self.battle.frame_clock.step;
        let law = self.toggles.battle_frame_span_law;
        let extra = if !self.toggles.battle_party_cost_step
            || (law && self.battle.frame_stretch.is_some())
        {
            0
        } else if law {
            legaia_engine_battle_loop::frame_clock::frame_span(span_cost, step)
                .saturating_sub(step.max(1))
        } else if measured.is_none()
            && self.toggles.battle_cd_timing
                == legaia_engine_battle_loop::frame_clock::CdTiming::PcsxRedux
            && self.battle_xa_passes_since_cue().is_some_and(|n| n >= 2)
        {
            // Off the law: a loaded ordinary frame runs one vsync past its step.
            legaia_engine_battle_loop::frame_clock::XA_STREAM_SPAN_EXTRA
        } else {
            0
        };
        let tick = self.clock.display_frames;
        self.battle.frame_clock.retime_span(tick, extra);
        let ordinary = self.ordinary_battle_frame_step();
        let want = match (measured, self.battle.cast_arm_step_on) {
            (Some(_), _) => self.battle.frame_cost_step,
            (None, true) => ordinary,
            (None, false)
                if (ordinary != DEFAULT_BATTLE_FRAME_STEP
                    || (self.toggles.battle_party_cost_step
                        && self.battle.frame_clock.step != ordinary))
                    && self.battle.frame_stretch.is_none() =>
            {
                ordinary
            }
            (None, false) => return,
        };
        self.battle.cast_arm_step_on = measured.is_some();
        // The ring's step is the next pass's, an arm's as an ordinary frame's:
        // retail waits this pass's frame out on the step the pass ran at
        // (`VSync(0x1F800392)` at `0x800171C0`) and only then copies the new
        // one in (`0x800171D8`), so the step this pass's cost raised opens the
        // next frame. A dear arm's first pass therefore runs on the step the
        // frames before it left and the arm's own step from its second pass
        // on: Nova's arm 3 runs `1` then `4`, as its recording reads.
        self.battle.pending_frame_step = Some(want);
        self.battle.pending_frame_step_from = self.battle_frame_id();
    }

    /// Install the step the last pass left ([`BattleState::pending_frame_step`])
    /// on the tick the next pass opens its frame.
    pub(in crate::world) fn take_pending_frame_step(&mut self) {
        // The pass that left the step has its frame: the effect scene graphs'
        // claims of that same frame (`World::summon_fx_pass_vsyncs`,
        // `World::action_fx_pass_vsyncs`, run in the frame tail after the
        // pass) leave it for the next one.
        // A replay's stretched frame (`Self::stretch_battle_frame`) runs the
        // pass after it on the stretched step and settles back to its own:
        // the ring's step waits for the stretch to settle, or it would
        // overwrite the step the stretch exists to give that pass and the
        // replay's countdown would never reach retail's lattice.
        if self.battle.pending_frame_step.is_none()
            || self.battle_frame_id() == self.battle.pending_frame_step_from
            || self.battle.frame_stretch.is_some()
        {
            return;
        }
        let Some(want) = self.battle.pending_frame_step.take() else {
            return;
        };
        if self.battle.frame_clock.step != want {
            let tick = self.clock.display_frames;
            self.battle.frame_clock.retime_frame(tick, want);
            if let Some(cam) = self.battle.camera.as_mut() {
                cam.set_frame_step(want);
            }
        }
    }

    /// Start a film's retail step track on the pass just run
    /// ([`legaia_engine_battle_loop::frame_clock::RetailStepTrack::arm`]):
    /// the recorder calls it on the tick its recording begins, the cast's
    /// begin or the capture's gate. A harness seat.
    pub fn arm_retail_step_track(&mut self) {
        use legaia_engine_battle_vm::battle_action::ActionState as S;
        let state = self.battle_ctx.action_state;
        let band = state == S::SummonSustain.as_byte()
            || state == S::SummonReturn.as_byte()
            || state == S::MagicCapturePhase2.as_byte();
        let phase = if band { self.casting.module_phase } else { 0 };
        let Some(track) = self
            .battle
            .retail_step_track
            .as_mut()
            .filter(|t| !t.armed())
        else {
            return;
        };
        // The recording's first frame opens on the state the arming pass
        // left: its step is the next pass's, which both drains by it and
        // spans it. It goes in as the step that pass's frame claims
        // (`Self::take_pending_frame_step`); the recorder arms ahead of the
        // tick after the arming pass, so any later claim is a later frame.
        // A key the film never saw leaves the engine's own step in place.
        if let Some(want) = track.arm(state, phase) {
            self.battle.pending_frame_step = Some(want);
            self.battle.pending_frame_step_from = u64::MAX;
        }
    }

    /// The step of the battle frame this tick belongs to, as a module arm
    /// that runs inside the pass reads it: the step the frame claimed when
    /// it opened ([`Self::take_pending_frame_step`]). A film's retail step
    /// track hands its step on through that same claim
    /// ([`Self::apply_retail_step_track`]), so the module reads the
    /// recording's step for the frame without looking ahead.
    pub(in crate::world) fn module_frame_step(&self) -> u8 {
        self.battle_frame_step().max(1)
    }

    /// A film's retail step for the frame the next pass opens
    /// ([`legaia_engine_battle_loop::frame_clock::RetailStepTrack`]), the
    /// step that pass drains by and the frame it spans. It goes in as the
    /// pending step the next frame claims, the way an arm table's does: a
    /// pass's step and its frame's length are one value. Re-timed onto this
    /// pass's own frame instead, the frame lasted the next pass's step while
    /// the next pass drained by this one's, so a seated film either kept
    /// retail's wall timing and drained a frame late, or drained on time
    /// and moved every step change's frame boundary by a pass.
    fn apply_retail_step_track(&mut self) {
        use legaia_engine_battle_vm::battle_action::ActionState as S;
        let Some(track) = self.battle.retail_step_track.as_mut() else {
            return;
        };
        let state = self.battle_ctx.action_state;
        let band = state == S::SummonSustain.as_byte()
            || state == S::SummonReturn.as_byte()
            || state == S::MagicCapturePhase2.as_byte();
        let phase = if band { self.casting.module_phase } else { 0 };
        let Some(want) = track.next_step(state, phase) else {
            return;
        };
        self.battle.pending_frame_step = Some(want);
        self.battle.pending_frame_step_from = self.battle_frame_id();
    }

    /// Start the capture band's CD wait on the state the SM just entered.
    ///
    /// `0x6E` polls the read `0x28` queued (`jal 0x8003DE7C` at `0x801E4F08`)
    /// and `0x6F` the module's own (`jal 0x8003F2B8`), both on the disc in
    /// retail and answered at once by the engine's resident image. The polls
    /// are not idle: each `0x6E` pass walks the framing `FUN_801D5854(seat, 6)`
    /// (`0x801E4F00`), and each `0x6F` pass pulls `ctx[+0x6D0]` in by
    /// `step * 16`, so arm 0's long shot opens from another pose and every
    /// shot reading `ctx[+0x6D0]` lands at another depth without them.
    pub(in crate::world) fn arm_capture_read_wait(&mut self, to: u8) {
        use legaia_engine_battle_vm::battle_action::ActionState as S;
        /// Vsyncs each poll held over the harvest-state casts: `14..=17` and
        /// `4..=6`, a read that does not move with the module or the fight.
        const BRANCH_READ_VSYNCS: u64 = 14;
        const FADE_READ_VSYNCS: u64 = 4;
        let vsyncs = if to == S::MagicCaptureBranch.as_byte() {
            BRANCH_READ_VSYNCS
        } else if to == S::MagicCaptureFade.as_byte() {
            FADE_READ_VSYNCS
        } else {
            return;
        };
        self.battle.capture_read_until = self.clock.display_frames + vsyncs;
    }

    /// Whether the capture band's read has not answered yet
    /// ([`Self::arm_capture_read_wait`]).
    pub(in crate::world) fn capture_read_pending(&self) -> bool {
        use legaia_engine_battle_vm::battle_action::ActionState as S;
        let state = self.battle_ctx.action_state;
        (state == S::MagicCaptureBranch.as_byte() || state == S::MagicCaptureFade.as_byte())
            && self.clock.display_frames < self.battle.capture_read_until
    }

    /// The battle frame driver's discarded `rand()` draw: `FUN_80046A20`
    /// calls the generator on every pass (`jal 0x80056798` at `0x80046D2C`,
    /// after the four-slot loop that every path of falls through to
    /// `0x80046D10`) and overwrites `v0` at `0x80046D34` without reading it.
    /// A pass is one battle frame, so the engine, which ticks once a vsync,
    /// draws on the first tick of each [`BattleFrameClock`](legaia_engine_battle_loop::frame_clock::BattleFrameClock) frame. The
    /// call sits ahead of the side-band pass (`jal 0x80056208` at
    /// `0x80046D60`) and both state machines, and nothing on the pass gates
    /// it: the Begin prompt, a dialogue box and the results sequencer all
    /// draw. Gated by [`crate::world::WorldToggles::battle_pass_rand_draw`].
    ///
    // PORT: FUN_80046A20 (the discarded rand() draw, 0x80046D2C)
    pub(in crate::world) fn tick_battle_pass_draw(&mut self) {
        if !self.toggles.battle_pass_rand_draw || self.game_over_hold {
            return;
        }
        let frame = self.battle_frame_id();
        if self.battle.pass_draw_frame == Some(frame) {
            return;
        }
        self.battle.pass_draw_frame = Some(frame);
        self.seat_retail_rand_word();
        let _discarded = self.next_rand();
    }

    /// A film's retail `rand()` word for the pass opening here
    /// ([`legaia_engine_battle_loop::frame_clock::RetailRandTrack`]), keyed as
    /// the pairing keys a frame: the action state, and the module phase inside
    /// the bands. A harness seat; with no track the stream runs on.
    fn seat_retail_rand_word(&mut self) {
        use legaia_engine_battle_vm::battle_action::ActionState as S;
        let state = self.battle_ctx.action_state;
        let band = state == S::SummonSustain.as_byte()
            || state == S::SummonReturn.as_byte()
            || state == S::MagicCapturePhase2.as_byte();
        let phase = if band { self.casting.module_phase } else { 0 };
        if let Some(word) = self
            .battle
            .retail_rand_track
            .as_mut()
            .and_then(|t| t.pass_word(state, phase, band))
        {
            self.rng_state = word;
        }
    }

    /// The kept object 1's spin: on every pass `FUN_80046A20` adds half the
    /// frame step to the backdrop draw's slot-1 Y angle while battle init's
    /// keep-object-1 byte is up (`lbu v0,0x333(gp)` = `0x8007B64B`, then
    /// `0x800891D2 += *0x1F800393 >> 1`, `0x80046D34..0x80046D5C`). The byte
    /// is the one that keeps the object in the draw list, so the object the
    /// region kept is the one that turns - nilboa's horizon mist ribbon
    /// drifts round the arena at one unit a frame. A pass is one battle
    /// frame, so the engine winds on the first tick of each
    /// [`BattleFrameClock`](legaia_engine_battle_loop::frame_clock::BattleFrameClock) frame. The store sits right after the pass's
    /// discarded `rand()` draw and nothing gates it but the byte.
    ///
    /// The angle has two references on the whole disc, this writer and the
    /// backdrop draw's read, and no reset: it accumulates from boot, and a
    /// stage that drops object 1 draws whatever sits in slot 1 at the angle
    /// the last keeping fight left.
    ///
    // PORT: FUN_80046A20 (the slot-1 backdrop spin, 0x80046D34..0x80046D5C)
    pub(in crate::world) fn tick_battle_backdrop_spin(&mut self) {
        let frame = self.battle_frame_id();
        if self.battle.backdrop_spin_frame == Some(frame) {
            return;
        }
        self.battle.backdrop_spin_frame = Some(frame);
        let keep = self
            .encounters
            .region_setup
            .and_then(|s| s.keep_backdrop_object_1)
            .unwrap_or(false);
        if keep {
            let step = u16::from(self.battle.frame_clock.step >> 1);
            self.battle.backdrop_slot_1_yaw = self.battle.backdrop_slot_1_yaw.wrapping_add(step);
        }
    }

    /// The backdrop draw's slot-1 Y angle this frame, in 4096ths of a turn
    /// ([`Self::tick_battle_backdrop_spin`]). Both battle hosts turn their
    /// slot-1 backdrop draws by it through
    /// [`legaia_engine_scene_host::scene_host::backdrop_slot_1_basis`].
    pub fn battle_backdrop_slot_1_yaw(&self) -> u16 {
        self.battle.backdrop_slot_1_yaw
    }

    /// Stamp the slot-1 angle directly, for a replay of a retail capture
    /// whose RAM holds it (`0x800891D2`): the angle is time since boot spent
    /// in keeping fights, which no seed replays.
    pub fn seed_battle_backdrop_slot_1_yaw(&mut self, yaw: u16) {
        self.battle.backdrop_slot_1_yaw = yaw;
    }

    /// Whether this tick opens a battle frame the action SM has not yet run
    /// its pass on, and if so claim it. Retail runs `FUN_801E295C` once a
    /// battle frame: the frame driver `FUN_80046A20` (`jal 0x801E295C` at
    /// `0x80047048`, then the separation pass `jal 0x80051078`) is an actor
    /// node the master frame driver ticks once a main-loop pass, and the
    /// frame-end pass `FUN_80016B6C` paces that loop with `VSync(step)`
    /// (`jal 0x8005FB84` at `0x800171C0`, `a0` the frame step byte
    /// `0x1F800392`). The engine ticks once a vsync, so the pass runs on the
    /// first tick of each [`BattleFrameClock`](legaia_engine_battle_loop::frame_clock::BattleFrameClock) frame and its timers drain by
    /// the step ([`Self::battle_frame_step`], the SM's `frame_dt`).
    pub(in crate::world) fn battle_sm_pass_due(&mut self) -> bool {
        if !self.toggles.battle_sm_frame_cadence {
            return true;
        }
        self.settle_battle_frame_stretch();
        let frame = self.battle_frame_id();
        if self.battle.sm_pass_frame == Some(frame) {
            return false;
        }
        self.battle.sm_pass_frame = Some(frame);
        self.take_pending_frame_step();
        true
    }

    /// Whether the next world tick runs an action-SM pass on retail's
    /// cadence: it opens a battle frame no pass has claimed yet
    /// ([`Self::battle_sm_pass_due`]). `false` on the per-vsync cadence,
    /// where every tick runs one.
    pub fn battle_sm_pass_due_next_tick(&self) -> bool {
        self.toggles.battle_sm_frame_cadence
            && self.battle.sm_pass_frame
                != Some(
                    self.battle
                        .frame_clock
                        .frame_of(self.clock.display_frames + 1),
                )
    }

    /// How many vsyncs of the actor anim tick this tick runs, and claim the
    /// frame. Retail's anim tick `FUN_80047430` is an actor-list node of the
    /// same main-loop pass as the battle frame driver, and it advances each
    /// cursor by the frame step: `(DAT_1F800393 * +0x21D * clip rate) >> 1`
    /// (`lbu v1,0x393(v1)` at `0x80047734`, the `>> 2` twin at
    /// `0x80047700`). On retail's cadence the engine therefore runs the
    /// anim family (the cursor advance, its root-motion drive and its
    /// hit-event walk) on the first tick of each [`BattleFrameClock`](legaia_engine_battle_loop::frame_clock::BattleFrameClock) frame,
    /// once per vsync the frame spans, and holds every actor still on the
    /// ticks between: a pose, a hit and an effect record land on the frame
    /// the pass ran, never between two. On the per-vsync cadence it is `1`.
    pub(in crate::world) fn battle_anim_pass_vsyncs(&mut self) -> u8 {
        if !self.toggles.battle_sm_frame_cadence {
            return 1;
        }
        let frame = self.battle_frame_id();
        if self.battle.anim_pass_frame == Some(frame) {
            return 0;
        }
        self.battle.anim_pass_frame = Some(frame);
        self.take_pending_frame_step();
        self.battle_frame_step().max(1)
    }

    /// How many vsyncs the effect-script table parts' part tick spans this
    /// tick, and claim the frame. Retail's part tick `FUN_80021DF4` is an
    /// actor-list node of the same main-loop pass as the battle frame
    /// driver, so it runs once a battle frame and scales its wait drain and
    /// channel integration by `DAT_1F800393 * DAT_1F80037D`
    /// (`0x80021E40..0x80021E4C`) and the envelope tail `FUN_80020740` by
    /// `DAT_1F800393`. On retail's cadence in battle that is the first tick
    /// of each [`BattleFrameClock`](legaia_engine_battle_loop::frame_clock::BattleFrameClock) frame, with the frame step, and `0` on
    /// the ticks between; elsewhere it is `1`.
    pub(in crate::world) fn action_fx_pass_vsyncs(&mut self) -> u8 {
        if self.mode != legaia_engine_mode::mode::SceneMode::Battle
            || !self.toggles.battle_sm_frame_cadence
        {
            return 1;
        }
        let frame = self.battle_frame_id();
        if self.battle.action_fx_pass_frame == Some(frame) {
            return 0;
        }
        self.battle.action_fx_pass_frame = Some(frame);
        self.take_pending_frame_step();
        self.battle_frame_step().max(1)
    }

    /// The same pass for the cast's summon parts
    /// ([`Self::tick_summon`]): the slot-B module's parts are list-1 nodes
    /// of the battle pass like the table-form ones, so on retail's cadence
    /// they take one part tick a battle frame, scaled by the step. The
    /// motion block's velocity terms truncate per tick
    /// (`(v << 12) * delta >> 18`), so a part ticked every vsync at the
    /// one-vsync delta never moves on a drift under `8`: PROT 0930's arm-6
    /// glows (`+0x3C` / `+0x3E` of `0..10`) stood in the centre.
    pub(in crate::world) fn summon_fx_pass_vsyncs(&mut self) -> u8 {
        if self.mode != legaia_engine_mode::mode::SceneMode::Battle
            || !self.toggles.battle_sm_frame_cadence
        {
            return 1;
        }
        let frame = self.battle_frame_id();
        if self.battle.summon_fx_pass_frame == Some(frame) {
            return 0;
        }
        self.battle.summon_fx_pass_frame = Some(frame);
        self.take_pending_frame_step();
        self.battle_frame_step().max(1)
    }

    /// The actor anim tick's pass on retail's cadence, run after the frame
    /// driver's pass and its camera: the cursor advance
    /// ([`Self::tick_battle_animations`]), then - when the pass reached the
    /// anim slot - its root motion and its hit walk, whose events fold here,
    /// then the draw's body-pair store.
    ///
    /// Retail ticks the battle frame driver `FUN_80046A20` (the action SM,
    /// the separation pass, the camera walker) ahead of the battle actors'
    /// anim tick `FUN_80047430` within one pass, and the battle draw
    /// callback's pose decoder `FUN_8004998C` stores the body pairs after
    /// both. Two captures pin the order through the close-up accumulator
    /// `ctx[+0x87C]`, which the SM's framing prologue raises by
    /// `8 * step` a pass (`0x801D5900..0x801D5914`) and a clip commit
    /// zeroes beside the node cursor (`FUN_8004AD80`, `sw zero,0x87c` at
    /// `0x8004BF74`, `sh zero,0x68` at `0x8004B068`):
    /// `rim_elm_gimard_victory` holds accumulator `16` beside cursor `16`
    /// one pass after a step-`2` event-path cut (`0x80047A3C`, after the
    /// advance), and `theeder_summon_mid_cast` holds `72` beside `36`
    /// three step-`3` passes after a commit. Framing ahead of the anim tick
    /// reads both; framing after it would read `32` and `96`.
    ///
    /// A no-op on the per-vsync cadence, where the anim tick keeps its seat
    /// ahead of the SM.
    pub(in crate::world) fn tick_battle_anim_pass(&mut self) {
        let cast_tick_due = std::mem::take(&mut self.battle.cast_tick_due);
        if self.mode != legaia_engine_mode::mode::SceneMode::Battle
            || !self.toggles.battle_sm_frame_cadence
            || self.battle.anim_pass_vsyncs == 0
        {
            return;
        }
        // The node tick's head: the HP / MP readout ramp, once a pass
        // (`FUN_80047430` `0x800474E8..0x80047638`, ahead of the cursor
        // advance and the hit walk).
        // PORT: FUN_80047430 (the readout ramp's cadence)
        self.tick_battle_hp_bars();
        // What the previous pass's draw stored, for this frame's readout
        // (`BattleState::readout_trios`).
        self.battle.readout_trios = (0..self.actors.len())
            .map(|i| self.battle_display_trio(i))
            .collect();
        // Each actor's hit walk runs inside its own slice of the anim walk
        // (`World::tick_battle_animations_walk`), ahead of the pass's root
        // motion, as in the node tick (`0x8004787C..0x80047A44` ahead of
        // `0x80047D20`).
        let hits = self.battle.anim_slot_reached;
        let events_before = self.pending_battle_events.len();
        if hits {
            self.begin_battle_hit_pass();
        }
        let own = self.tick_battle_animations_walk(hits);
        // Every commit an actor's own slice made is walked again.
        let mut rewalk: Vec<usize> = own
            .into_iter()
            .enumerate()
            .filter(|&(_, moved)| moved)
            .map(|(k, _)| k)
            .collect();
        if hits {
            self.tick_battle_locomotion_dt(self.battle.anim_pass_vsyncs);
            self.fold_new_battle_events(events_before);
            for k in std::mem::take(&mut self.battle.hit_commit_rewalk) {
                if !rewalk.contains(&k) {
                    rewalk.push(k);
                }
            }
        }
        // A commit inside an actor's own anim tick is followed, in the same
        // tick, by a second hit / effect-script / cue walk of the clip it
        // installed, at that clip's first frame (`beq s4,zero,0x80047c3c`,
        // then `jal 0x801ec3e4` / `jal 0x801dea50` / `jal 0x800508dc` at
        // `0x80047BF0..0x80047C34`, the cursor `FUN_8004AD80` just zeroed).
        // So a struck monster's reaction throws its frame-0 effects - the
        // melee ray burst - on the pass the blow lands, not a pass later.
        // The hit walk of the new clip is not repeated here: the pass's hit
        // walk already reads the clips the advance committed.
        // PORT: FUN_80047430 (`0x80047BC4..0x80047C38`, the post-commit walk)
        rewalk.sort_unstable();
        for k in rewalk {
            let Some(frame) = self
                .actors
                .get(k)
                .and_then(|a| a.battle_animation.as_ref())
                .map(|p| p.current_frame())
            else {
                continue;
            };
            self.step_actor_effect_script(k, frame);
            self.step_actor_anim_cues(k, frame);
        }
        // A commit this pass zeroed `ctx[+0x87C]` and its ramps on the spot
        // (`FUN_8004AD80`, `0x8004BF68..0x8004BF78`); the camera holds the
        // retail words, so it takes the reset now rather than at its next
        // step.
        let (commits, installs) = (
            self.battle_ctx.active_clip_commits,
            self.battle_ctx.active_clip_installs,
        );
        if let Some(cam) = self.battle.camera.as_mut() {
            cam.observe_active_commits(commits);
            cam.observe_active_installs(installs);
        }
        // The draw's preamble, ahead of any body: the cast tick's census
        // and its effect-child drive (`jal 0x801E09F8` at `0x80048130`).
        // What a landing stages here commits on the victim's next node
        // tick, a pass on.
        // PORT: FUN_800480D8 (`0x800480FC..0x80048130`, the cast tick's seat in the pass)
        if cast_tick_due {
            self.tick_battle_cast_census();
            self.battle.cast_census_drawn = true;
            let n = self.battle.anim_pass_vsyncs;
            self.battle.in_draw_cast_tick = true;
            self.tick_battle_cast_drive(n);
            self.battle.in_draw_cast_tick = false;
        }
        // The draw: every body pair from the pose the pass left, for the
        // next pass's SM and camera to read - a seat the root-motion pass
        // has not seeded yet included (the draw stores every drawn actor's).
        self.seed_battle_seats();
        self.refresh_battle_body_pairs();
        // ... and the render pass's own `rand()` draws (the jittered rings).
        self.deal_battle_draw_rands();
        // The same draw sheds the burning bodies' puffs (the tail of
        // `FUN_8004998C`, `0x8004A5FC..0x8004A8D8`) at the pose and ground
        // the pass left, on the frame driver's accumulator `ctx[+0x328]`,
        // which gained `step << 3` this frame (`0x8004713C..0x80047160`).
        let vsyncs = self.battle.anim_pass_vsyncs;
        self.emit_battle_burn_sprites(8 * u16::from(vsyncs));
    }

    /// The action state the battle camera frames. Retail's framing calls
    /// (`FUN_801D5854`, the cast-effect driver) sit inside the state body, so
    /// a pass arms the shot of the state it **ran** and the state it stores
    /// is framed by the next pass: the seed `0x0C` arms case 6 on the `0x800`
    /// rung and only then stores `0x14` and `0x200` (`0x801E2CF8`,
    /// `0x801E2D50`, `0x801E2F18..0x801E2F20`). On retail's cadence the
    /// camera therefore follows the state the last pass ran.
    ///
    /// On the per-vsync cadence it follows the live state, except on the
    /// tick the seed ran: that pass's framing is the seed's `0x800`-rung
    /// case 6, and framed on the live `0x14` it is the `0x200` rung's, a
    /// shot retail arms only on the pass after. Framed as the live state,
    /// the seed's shot never reached the camera, and every capture on the
    /// walker pass after the seed glided toward the wrong endpoint
    /// (`super_queue_replace_*`: yaw endpoint `2561` against retail's
    /// `4062`). Every other state is still framed live there: the camera
    /// steps once per two vsyncs while the SM takes a pass per vsync, so
    /// following the ran state through every transition moves which arm the
    /// step lands on, and the casts that walk `0x0A..0x29` in one-vsync
    /// passes measured worse for it (`flute_lippian_midcast` `image` `.740`
    /// to `.705`).
    ///
    /// The summon band `0x32..=0x37` is framed by the pass that enters it:
    /// `0x29`'s summon-route arm stores the `0x600` depth and falls through
    /// into the entered state's case-6 call (`0x801E4604`, `0x801E4748`), and
    /// the band's own states hand the camera to the close-up and the module.
    /// Framed one pass late there, every summon capture lost its shot
    /// (`camera` `.3` to `.8` across the corpus). Inside the band the rule is
    /// the general one again for `0x32`'s exit: its body walks case 6 on the
    /// caster (`0x801E4984`) ahead of the drive test that stores `0x33`
    /// (`0x801E4994`), so the pass that stores it is framed by case 6 and the
    /// close-up's first shot is the next pass's, on an accumulator that has
    /// gained a step (`party_basic_attack_vs_gobu_gobu` films: retail's TR z
    /// runs `2963`, `2877` on case 6, then `2681` with the accumulator at
    /// `16`, where a close-up armed on the storing pass cut to `2097`).
    pub fn battle_camera_state(&self) -> u8 {
        let live = self.battle_ctx.action_state;
        let ran = self.battle.sm_ran_state.unwrap_or(live);
        if matches!(live, 0x32..=0x37) {
            if live == 0x33 && ran == 0x32 && self.toggles.battle_sm_frame_cadence {
                return ran;
            }
            return live;
        }
        if self.toggles.battle_sm_frame_cadence || ran == 0x0C {
            ran
        } else {
            live
        }
    }

    /// Whether this module tick is the one retail's pass maps onto: the
    /// frame driver enters the module once a battle frame, while the module
    /// here runs on every display tick ([`Self::module_pass`]), so what an
    /// arm does once a pass (a spawn call, a `rand()` draw) runs on the tick
    /// that opens a frame, and on the first tick of a multi-tick pass.
    pub(in crate::world) fn module_tick_opens_pass(&self) -> bool {
        !self.casting.module_pass_repeat
            && self
                .battle
                .frame_clock
                .ticks_into_frame(self.clock.display_frames)
                == 0
    }

    /// Whether this module tick is the last of its pass: on the battle-frame
    /// cadence a pass runs its `delta` one-vsync ticks back to back
    /// ([`Self::module_pass`]), and only the last has drained, drifted and
    /// walked what retail's one `delta`-scaled call has when it makes its
    /// calls. Off the cadence a pass is one tick, the frame-opening one.
    pub(in crate::world) fn module_tick_closes_pass(&self) -> bool {
        if !self.toggles.battle_sm_frame_cadence {
            return self.module_tick_opens_pass();
        }
        self.casting.module_cam.countdown.1 == 0
            && self
                .battle
                .frame_clock
                .ticks_into_frame(self.clock.display_frames)
                == 0
    }

    /// One SM pass's call into a paged cast module (`FUN_801F1ED4` /
    /// `FUN_801F2160`), at retail's frame delta.
    ///
    /// Retail enters the module once a pass and every counted arm drains its
    /// countdown by `scalar * delta`, drifts its pose by `k * delta` and
    /// re-tests its threshold once (`cast_module_camera`, the countdown
    /// kernel); an arm that finishes stores the next phase `ctx[+0x279]` and
    /// returns, so the next arm runs on the next pass. The port's module ticks
    /// are written in one-vsync units (`MODULE_DRAIN_PER_TICK`, a drift of
    /// `k`), so a pass runs `delta` of them - the same drain, drift and spin
    /// as one `delta`-scaled call - and stops on the tick an arm hands over,
    /// so the next arm starts on the next pass as retail's does.
    ///
    /// On the per-vsync cadence the pass is one tick, but an arm that hands
    /// over still leaves the next arm to the next **battle frame**: the
    /// module is entered once a frame, so an arm that finishes on one pass
    /// (Big Wave's arm 0 cut, then arm 4's pan, `0x801F6B08..0x801F6C34`,
    /// `0x801F6E90..0x801F6F2C`) is followed by the next arm a frame later,
    /// never a vsync later. The band holds on the ticks between; the arms
    /// keep their one-vsync drains, so an arm's span is unchanged.
    pub(in crate::world) fn module_pass(&mut self, tick: fn(&mut World) -> bool) -> bool {
        let cadence = self.toggles.battle_sm_frame_cadence;
        let delta = if cadence {
            // The frame this pass opens ([`Self::module_frame_step`]).
            self.module_frame_step()
        } else {
            if self.casting.module_handover_waits {
                if self
                    .battle
                    .frame_clock
                    .ticks_into_frame(self.clock.display_frames)
                    != 0
                {
                    return true;
                }
                self.casting.module_handover_waits = false;
            }
            1
        };
        let mut live = false;
        for i in 0..delta {
            let phase = self.casting.module_phase;
            self.casting.module_pass_repeat = i != 0;
            self.casting.module_tick_drift = None;
            self.casting.module_tick_after_drift = None;
            // The one-vsync ticks of the pass still owed after this one: an
            // arm leaving on this tick has drained `delta` ticks' worth by
            // retail's clock (the whole pass's product in one `subu`), and
            // its re-arm adds to that residue.
            self.casting.module_cam.countdown.1 = delta - 1 - i;
            live = tick(self);
            if !live || self.casting.module_phase != phase {
                if !cadence && live && self.casting.module_phase != phase {
                    self.casting.module_handover_waits = true;
                }
                // Retail's passing pass drifts the camera globals by
                // `k * delta` before it stores the next phase (Gola Gola's
                // arm `0xB`, `yaw += 4 p`), where the port's ticks stop on
                // the one that hands over: the rest of the pass's drift.
                if live && self.casting.module_phase != phase {
                    self.drift_rest_of_pass(delta - 1 - i);
                    self.repeat_module_seat_moves(phase, delta - 1 - i);
                }
                break;
            }
        }
        live
    }

    /// Re-apply the handing-over tick's camera drift for the `ticks` of its
    /// pass the port does not run ([`Self::module_pass`]).
    ///
    /// A director in retail's drift-first order (PROT 0959, 0960) drifts the
    /// globals by the whole pass's `k * delta` and only then makes its pass
    /// stores and its builder call, so the rest of the drift goes in ahead of
    /// them: the stores and the shot the tick made are made again over it. A
    /// cut then lands on its own endpoint, where drifting after it left
    /// Plasma Strike's arm-7, 9 and `0x0B` cuts a step-`2` or `3` drift past
    /// retail's (`yaw 496`, TR z `1075` against `512` / `819` on arm 8 of the
    /// `--retail-steps` film).
    fn drift_rest_of_pass(&mut self, ticks: u8) {
        let Some(d) = self.casting.module_tick_drift else {
            return;
        };
        let after = self.casting.module_tick_after_drift.take();
        let Some(cam) = self.battle.camera.as_mut() else {
            return;
        };
        for _ in 0..ticks {
            cam.drift_module(d.pitch, d.yaw, d.tr_y, d.tr_z);
            if d.tr_x != 0 {
                cam.drift_module_tr_x(d.tr_x);
            }
        }
        if ticks == 0 {
            return;
        }
        if let Some((set, shot)) = after {
            if let Some(set) = set {
                cam.set_module_pose(
                    set.yaw.map(f32::from),
                    set.tr_y.map(|v| v as f32),
                    set.tr_z.map(|v| v as f32),
                    set.focus_xz.map(|[x, z]| [f32::from(x), f32::from(z)]),
                );
            }
            if let Some(shot) = shot {
                let (pose, raw_z) = shot.pose();
                cam.arm_module_shot(pose, raw_z, u32::from(shot.frames));
            }
        }
    }

    /// The SM's `frame_dt` (`DAT_1F800393`): the frame step when the SM runs
    /// once a battle frame, one vsync when it runs every tick.
    pub(in crate::world) fn battle_sm_frame_dt(&self) -> i16 {
        if self.toggles.battle_sm_frame_cadence {
            i16::from(self.battle_frame_step())
        } else {
            1
        }
    }

    /// The battle frame step in vsyncs ([`Self::set_battle_frame_step`]).
    pub fn battle_frame_step(&self) -> u8 {
        self.battle.frame_clock.step
    }

    /// Move the display clock to the first vsync of the next battle frame
    /// and return the vsyncs it moved: the clock advance a host's
    /// [`World::tick`] makes between two battle passes.
    ///
    /// The per-pass kernels ([`Self::tick_summon`], [`Self::tick_move_fx`],
    /// the action SM on retail's cadence, the battle camera) run once per
    /// [`BattleFrameClock`](legaia_engine_battle_loop::frame_clock::BattleFrameClock) frame and treat every later call inside the same
    /// frame as "between passes", so a test that drives them in a loop must
    /// open a new frame per step or it ticks them once and never again.
    /// From a tick that opens a frame this is the frame step; from one inside
    /// a frame it is the rest of that frame.
    pub fn advance_battle_pass(&mut self) -> u8 {
        let clock = &self.battle.frame_clock;
        let step = clock.span();
        let into = clock.ticks_into_frame(self.clock.display_frames);
        let vsyncs = step - into;
        self.clock.display_frames += vsyncs;
        vsyncs as u8
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `advance_battle_pass` lands on the tick that opens the next frame:
    /// a whole step from a frame's first tick, the remainder from inside
    /// one, and every landing claims a fresh per-pass tick.
    #[test]
    fn advance_battle_pass_opens_the_next_frame() {
        let mut w = World::new();
        w.mode = legaia_engine_mode::mode::SceneMode::Battle;
        w.toggles.battle_sm_frame_cadence = true;
        w.set_battle_frame_step(3);
        let opening = w.clock.display_frames + w.advance_battle_pass() as u64;
        assert_eq!(w.clock.display_frames, opening);
        assert_eq!(w.advance_battle_pass(), 3);
        w.clock.display_frames += 1;
        assert_eq!(w.advance_battle_pass(), 2, "the rest of the frame");
        let mut passes = Vec::new();
        for _ in 0..4 {
            w.advance_battle_pass();
            passes.push(w.summon_fx_pass_vsyncs());
            passes.push(w.summon_fx_pass_vsyncs());
        }
        assert_eq!(passes, [3, 0, 3, 0, 3, 0, 3, 0]);
    }

    /// The table-form parts tick once a battle frame in battle on retail's
    /// cadence, spanning the frame step; every tick at one vsync elsewhere.
    #[test]
    fn the_action_fx_part_tick_is_the_battle_pass() {
        let mut w = World::new();
        w.mode = legaia_engine_mode::mode::SceneMode::Battle;
        w.toggles.battle_sm_frame_cadence = true;
        w.set_battle_frame_step(3);
        let mut ticks = Vec::new();
        for t in 30..40u64 {
            w.clock.display_frames = t;
            ticks.push(w.action_fx_pass_vsyncs());
        }
        let passes: Vec<usize> = (0..ticks.len()).filter(|&i| ticks[i] != 0).collect();
        assert!(passes.windows(2).all(|p| p[1] - p[0] == 3), "{ticks:?}");
        assert!(passes.len() >= 3);
        assert!(ticks.iter().all(|&n| n == 0 || n == 3));
        w.toggles.battle_sm_frame_cadence = false;
        assert_eq!(w.action_fx_pass_vsyncs(), 1);
        assert_eq!(w.action_fx_pass_vsyncs(), 1);
    }

    /// On retail's cadence the action SM takes one pass a battle frame and
    /// drains its timers by the step; on the per-vsync cadence
    /// it takes every tick and drains by one.
    #[test]
    fn the_sm_pass_runs_once_a_battle_frame_on_retail_cadence() {
        let mut w = World::new();
        w.toggles.battle_sm_frame_cadence = true;
        let mut passes = Vec::new();
        for t in 10..20u64 {
            w.clock.display_frames = t;
            if w.battle_sm_pass_due() {
                passes.push(t);
            }
            // A second ask inside the same tick is the same frame.
            assert!(!w.battle_sm_pass_due());
        }
        assert_eq!(passes, [10, 12, 14, 16, 18]);
        assert_eq!(w.battle_sm_frame_dt(), 2);
        w.set_battle_frame_step(3);
        assert_eq!(w.battle_sm_frame_dt(), 3);
        w.toggles.battle_sm_frame_cadence = false;
        assert!(w.battle_sm_pass_due() && w.battle_sm_pass_due());
        assert_eq!(w.battle_sm_frame_dt(), 1);
    }

    /// A stretched frame holds its pass `vsyncs` from the frame's opening
    /// tick, that pass drains the stretched step, and the frame it opens
    /// returns to the old step.
    #[test]
    fn a_stretched_battle_frame_spans_one_longer_pass() {
        let mut w = World::new();
        w.toggles.battle_sm_frame_cadence = true;
        let mut passes = Vec::new();
        for t in 10..22u64 {
            w.clock.display_frames = t;
            if t == 13 {
                w.stretch_battle_frame(3);
            }
            if w.battle_sm_pass_due() {
                passes.push((t, w.battle_sm_frame_dt()));
            }
        }
        assert_eq!(
            passes,
            [(10, 2), (12, 2), (15, 3), (17, 2), (19, 2), (21, 2)]
        );
        assert!(w.battle.frame_stretch.is_none());
        w.toggles.battle_sm_frame_cadence = false;
        w.stretch_battle_frame(3);
        assert!(w.battle.frame_stretch.is_none());
    }

    /// On retail's cadence the camera frames the state the last pass ran -
    /// the seed `0x0C` arms its own shot before it stores `0x14` - except
    /// in the module bands, whose shots the modules arm. On the per-vsync
    /// cadence it frames the live state, but the seed's pass still frames
    /// the seed: its arm precedes the `0x14` store on both cadences.
    #[test]
    fn the_camera_frames_the_state_the_last_pass_ran() {
        let mut w = World::new();
        w.toggles.battle_sm_frame_cadence = false;
        w.battle.sm_ran_state = Some(0x0C);
        w.battle_ctx.action_state = 0x14;
        assert_eq!(w.battle_camera_state(), 0x0C);
        w.battle.sm_ran_state = Some(0x28);
        w.battle_ctx.action_state = 0x29;
        assert_eq!(w.battle_camera_state(), 0x29);
        w.toggles.battle_sm_frame_cadence = true;
        assert_eq!(w.battle_camera_state(), 0x28);
        w.battle.sm_ran_state = Some(0x0C);
        w.battle_ctx.action_state = 0x14;
        assert_eq!(w.battle_camera_state(), 0x0C);
        w.battle_ctx.action_state = 0x35;
        assert_eq!(w.battle_camera_state(), 0x35);
    }

    /// A module pass runs one module tick per vsync of the frame on retail's
    /// cadence, stops on the tick an arm hands over, and is one tick on the
    /// per-vsync cadence.
    #[test]
    fn a_module_pass_runs_the_frame_delta_and_stops_at_a_handover() {
        fn count(w: &mut World) -> bool {
            w.battle.backdrop_slot_1_yaw += 1;
            true
        }
        fn hand_over(w: &mut World) -> bool {
            w.battle.backdrop_slot_1_yaw += 1;
            w.casting.module_phase = w.casting.module_phase.wrapping_add(1);
            true
        }
        let mut w = World::new();
        w.toggles.battle_sm_frame_cadence = true;
        w.set_battle_frame_step(3);
        assert!(w.module_pass(count));
        assert_eq!(w.battle.backdrop_slot_1_yaw, 3);
        w.battle.backdrop_slot_1_yaw = 0;
        w.module_pass(hand_over);
        assert_eq!(
            w.battle.backdrop_slot_1_yaw, 1,
            "the next arm runs next pass"
        );
        w.toggles.battle_sm_frame_cadence = false;
        w.battle.backdrop_slot_1_yaw = 0;
        w.module_pass(count);
        assert_eq!(w.battle.backdrop_slot_1_yaw, 1);
    }

    /// An arm that hands over on a pass's first tick still drifts the camera
    /// for the whole pass, as retail's `k * delta` does before it stores the
    /// next phase.
    #[test]
    fn a_handover_pass_drifts_the_camera_for_its_whole_delta() {
        use legaia_engine_battle_vm::battle_cam_script::{BattleCamPhase, BattleCamera};
        fn hand_over_drifting(w: &mut World) -> bool {
            let d = legaia_engine_cast::cast_module_camera::CaptureDrift {
                yaw: 32,
                ..Default::default()
            };
            w.casting.module_tick_drift = Some(d);
            if let Some(cam) = w.battle.camera.as_mut() {
                cam.drift_module(d.pitch, d.yaw, d.tr_y, d.tr_z);
            }
            w.casting.module_phase = w.casting.module_phase.wrapping_add(1);
            true
        }
        let mut w = World::new();
        w.toggles.battle_sm_frame_cadence = true;
        w.set_battle_frame_step(4);
        w.battle.camera = Some(BattleCamera::new(BattleCamPhase::Menu, 0));
        let before = w.battle.camera.as_ref().unwrap().pose().yaw;
        w.module_pass(hand_over_drifting);
        let after = w.battle.camera.as_ref().unwrap().pose().yaw;
        assert_eq!((after - before).rem_euclid(4096.0), 128.0);
    }

    /// A drift-first director's handing-over pass drifts the globals by the
    /// whole pass and only then cuts (PROT 0960's arms 7, 9 and `0x0B`): the
    /// rest of the drift goes in ahead of the cut, which lands on its own
    /// endpoint.
    #[test]
    fn a_drift_first_handover_cuts_after_the_whole_pass_drift() {
        use legaia_engine_battle_vm::battle_cam_script::{
            BattleCamPhase, BattleCamera, ModuleShot,
        };
        const CUT: ModuleShot = ModuleShot {
            angles: [0, 512, 0],
            tr: [0, 825, 0x200],
            focus: [0, 0, 0],
            frames: 1,
        };
        fn drift_then_cut(w: &mut World) -> bool {
            let d = legaia_engine_cast::cast_module_camera::CaptureDrift {
                yaw: -4,
                tr_z: 64,
                ..Default::default()
            };
            w.casting.module_tick_drift = Some(d);
            if let Some(cam) = w.battle.camera.as_mut() {
                cam.drift_module(d.pitch, d.yaw, d.tr_y, d.tr_z);
                let (pose, raw_z) = CUT.pose();
                cam.arm_module_shot(pose, raw_z, u32::from(CUT.frames));
            }
            w.casting.module_tick_after_drift = Some((None, Some(CUT)));
            w.casting.module_phase = w.casting.module_phase.wrapping_add(1);
            true
        }
        let mut w = World::new();
        w.toggles.battle_sm_frame_cadence = true;
        w.set_battle_frame_step(4);
        w.battle.camera = Some(BattleCamera::new(BattleCamPhase::Menu, 0));
        w.module_pass(drift_then_cut);
        let (want, _) = CUT.pose();
        let got = w.battle.camera.as_ref().unwrap().pose();
        assert_eq!(got.yaw, want.yaw);
        assert_eq!(got.tr[2], want.tr[2]);
    }

    /// An arm that hands over mid-pass moves its seats for the rest of the
    /// pass by its held ticks' every-pass moves (PROT 0958's runs), and a
    /// held pass of another arm owes nothing.
    #[test]
    fn a_handover_pass_moves_the_seats_for_its_whole_delta() {
        use legaia_engine_battle_loop::cast_fx_state::ModulePassMoves;
        fn hand_over(w: &mut World) -> bool {
            w.actors[0].move_state.world_z -= 64;
            w.casting.module_phase = w.casting.module_phase.wrapping_add(1);
            true
        }
        let mut w = World::new();
        w.toggles.battle_sm_frame_cadence = true;
        w.set_battle_frame_step(4);
        w.actors.push(crate::world::Actor::default());
        w.casting.module_phase = 9;
        w.casting.module_held_moves = Some(ModulePassMoves {
            phase: 9,
            seats: [Some((0, -64, 0)), None],
            focus_z: 0,
        });
        w.module_pass(hand_over);
        assert_eq!(w.actors[0].move_state.world_z, -256);
        assert!(w.casting.module_held_moves.is_none());
        // A record from another arm is not this one's.
        w.actors[0].move_state.world_z = 0;
        w.casting.module_held_moves = Some(ModulePassMoves {
            phase: 3,
            seats: [Some((0, -64, 0)), None],
            focus_z: 0,
        });
        w.module_pass(hand_over);
        assert_eq!(w.actors[0].move_state.world_z, -64);
    }

    /// On the per-vsync cadence an arm that hands over leaves the next arm
    /// to the tick that opens the next battle frame: the band holds on the
    /// ticks between without entering the module.
    #[test]
    fn a_per_vsync_handover_waits_for_the_next_battle_frame() {
        fn hand_over(w: &mut World) -> bool {
            w.battle.backdrop_slot_1_yaw += 1;
            w.casting.module_phase = w.casting.module_phase.wrapping_add(1);
            true
        }
        let mut w = World::new();
        w.toggles.battle_sm_frame_cadence = false;
        w.set_battle_frame_step(2);
        let mut entered = Vec::new();
        for _ in 0..6 {
            w.battle.backdrop_slot_1_yaw = 0;
            assert!(w.module_pass(hand_over));
            entered.push((
                w.battle
                    .frame_clock
                    .ticks_into_frame(w.clock.display_frames),
                w.battle.backdrop_slot_1_yaw,
            ));
            w.clock.display_frames += 1;
        }
        // Entered on the first tick, then only on the ticks that open a
        // frame.
        let opened: Vec<u64> = entered
            .iter()
            .skip(1)
            .filter(|e| e.1 == 1)
            .map(|e| e.0)
            .collect();
        assert!(
            !opened.is_empty() && opened.iter().all(|&t| t == 0),
            "{entered:?}"
        );
        assert_eq!(
            entered.iter().filter(|e| e.1 == 1).count(),
            3,
            "{entered:?}"
        );
    }

    /// The anim pass claims each battle frame once, a frame step of vsyncs
    /// wide, and holds on the ticks between; per vsync it is one tick.
    #[test]
    fn the_anim_pass_claims_a_frame_step_of_vsyncs() {
        let mut w = World::new();
        w.toggles.battle_sm_frame_cadence = true;
        let mut got = Vec::new();
        for t in 10..16u64 {
            w.clock.display_frames = t;
            got.push(w.battle_anim_pass_vsyncs());
        }
        assert_eq!(got, [2, 0, 2, 0, 2, 0]);
        w.toggles.battle_sm_frame_cadence = false;
        assert_eq!(w.battle_anim_pass_vsyncs(), 1);
        assert_eq!(w.battle_anim_pass_vsyncs(), 1);
    }

    /// On retail's cadence a step change made on the pass tick does not
    /// open a second pass on the tick after.
    #[test]
    fn a_seeded_step_change_opens_no_second_pass() {
        let mut w = World::new();
        w.toggles.battle_sm_frame_cadence = true;
        w.seed_battle_frame_step(3, 0x33);
        w.clock.display_frames = 10;
        assert!(w.battle_sm_pass_due());
        w.battle_ctx.action_state = 0x33;
        w.apply_battle_frame_step_seed();
        assert_eq!(w.battle_frame_step(), 3);
        let mut passes = vec![10];
        for t in 11..17u64 {
            w.clock.display_frames = t;
            if w.battle_sm_pass_due() {
                passes.push(t);
            }
        }
        assert_eq!(passes, [10, 13, 16]);
    }

    /// Under `battle_gpu_cost_step` a pass costs the larger of its CPU term
    /// and the GPU's wait over the frame the host drew last; off, the drawn
    /// cost is stored and never read.
    #[test]
    fn the_drawn_table_raises_a_pass_only_under_the_gpu_toggle() {
        let mut w = World::new();
        w.mode = legaia_engine_mode::mode::SceneMode::Battle;
        w.party.party_count = 1;
        w.battle_ctx.action_state = 0x1E;
        w.push_frame_cost(None);
        assert_eq!(w.ordinary_battle_frame_step(), 2);
        // A drawn table over the step-3 threshold `0x1FF`.
        w.note_drawn_table_cost(0x240);
        w.push_frame_cost(None);
        assert_eq!(w.ordinary_battle_frame_step(), 2, "the toggle is off");
        w.toggles.battle_gpu_cost_step = true;
        w.push_frame_cost(None);
        assert_eq!(w.ordinary_battle_frame_step(), 3);
        // A table under the CPU term leaves the pass on its CPU cost.
        let mut w = World::new();
        w.mode = legaia_engine_mode::mode::SceneMode::Battle;
        w.toggles.battle_gpu_cost_step = true;
        w.party.party_count = 1;
        w.battle_ctx.action_state = 0x1E;
        w.note_drawn_table_cost(20);
        w.push_frame_cost(None);
        assert_eq!(w.ordinary_battle_frame_step(), 2);
    }

    #[test]
    fn a_full_partys_ordinary_frame_runs_step_three() {
        let mut w = World::new();
        w.party.party_count = 3;
        w.battle_ctx.action_state = 0x1E;
        w.push_frame_cost(None);
        assert_eq!(w.ordinary_battle_frame_step(), 3);
        w.toggles.battle_party_cost_step = false;
        assert_eq!(w.ordinary_battle_frame_step(), DEFAULT_BATTLE_FRAME_STEP);
    }

    /// A lone caster's summon close-up, as on the PCSX-Redux films: six
    /// opening frames over `0xF1` and the ring's sixteen-entry memory drop
    /// the step to `1` forty-two vsyncs into `0x33`, on the pass the ring
    /// reads it, and the pass after it opens the first one-vsync frame (at
    /// forty-four), the `VSync` running on the step `FUN_80016B6C` stored a
    /// pass before. The camera walks that frame on its step.
    #[test]
    fn a_lone_casters_close_up_drops_to_step_one_forty_two_vsyncs_in() {
        use legaia_engine_battle_vm::battle_cam_script::{BattleCamPhase, BattleCamera};
        let mut w = World::new();
        w.toggles.battle_sm_frame_cadence = true;
        w.party.party_count = 1;
        w.battle.camera = Some(BattleCamera::new(BattleCamPhase::Menu, 0));
        w.battle_ctx.action_state = 0x32;
        for _ in 0..16 {
            w.push_frame_cost(None);
        }
        w.battle.frame_clock.retime_frame(0, 2);
        w.battle_ctx.action_state = 0x33;
        let (mut cam_drop, mut frame_drop) = (None, None);
        for t in 0..80u64 {
            w.clock.display_frames = t;
            if w.battle_sm_pass_due() {
                w.apply_cast_arm_frame_step();
                let cam = w.battle.camera.as_ref().map(|c| c.frame_step());
                if cam_drop.is_none() && cam == Some(1) {
                    cam_drop = Some(t);
                }
                if frame_drop.is_none() && w.battle_frame_step() == 1 {
                    frame_drop = Some(t);
                }
            }
        }
        assert_eq!(cam_drop, Some(44));
        assert_eq!(frame_drop, Some(44));
    }

    /// The step a pass's cost raises is the next pass's: the pass that
    /// pushed it keeps its own frame (retail waits it out on
    /// `0x1F800392`, the step it ran at), and the next frame opens on the
    /// new step.
    #[test]
    fn the_rings_step_opens_the_next_frame() {
        let mut w = World::new();
        w.mode = legaia_engine_mode::mode::SceneMode::Battle;
        w.toggles.battle_sm_frame_cadence = true;
        w.toggles.battle_frame_span_law = true;
        w.party.party_count = 3;
        w.battle_ctx.action_state = 0x1E;
        w.clock.display_frames = 10;
        assert!(w.battle_sm_pass_due());
        assert_eq!(w.battle_frame_step(), DEFAULT_BATTLE_FRAME_STEP);
        w.apply_cast_arm_frame_step();
        // The pass at tick 10 keeps its step-2 frame, which its 622 hblanks
        // run on to a third vsync (`VSync(2)` returns at the blank after the
        // work) ...
        assert_eq!(w.battle_frame_step(), 2);
        assert_eq!(w.battle.frame_clock.span(), 3);
        let mut passes = vec![10];
        for t in 11..20u64 {
            w.clock.display_frames = t;
            if w.battle_sm_pass_due() {
                passes.push(t);
                // ... and the frame it hands over opens on step 3.
                assert_eq!(w.battle_frame_step(), 3);
                w.apply_cast_arm_frame_step();
            }
        }
        assert_eq!(passes, [10, 13, 16, 19]);
    }

    /// A frame's live effect parts and after-image ghosts lift its cost: a
    /// lone member's strike frame reads step 2 bare and 3 under a special
    /// clip's two ghosts and a hit burst (`battle_vell_nighto_49_strike`).
    #[test]
    fn a_frames_draw_load_raises_its_step() {
        use legaia_engine_battle_loop::frame_clock::{
            frame_load_cost, measured_frame_cost, step_of_cost,
        };
        let base = measured_frame_cost(1, 0x20);
        assert_eq!(step_of_cost(base), 2);
        assert_eq!(step_of_cost(base + frame_load_cost(39, 2)), 3);
        assert_eq!(step_of_cost(base + frame_load_cost(16, 0)), 2);
        // A full party's strike stays on step 3 under a burst.
        assert_eq!(
            step_of_cost(measured_frame_cost(3, 0x1F) + frame_load_cost(25, 0)),
            3
        );
    }

    #[test]
    fn a_seeded_cost_ring_lifts_the_default_step_pin() {
        let mut w = World::new();
        w.party.party_count = 3;
        w.toggles.battle_party_cost_step = false;
        // `battle_geremi_ogre_strike`'s ring: its oldest entry is a cheap
        // walk-up frame, every newer one a full party's.
        let ring = [
            601, 602, 393, 508, 511, 530, 531, 513, 605, 612, 616, 610, 612, 605, 609, 589,
        ];
        w.seed_frame_cost_ring(ring, 2);
        assert!(w.toggles.battle_party_cost_step);
        assert_eq!(w.ordinary_battle_frame_step(), 3);
        // A newest entry on the clamp is a step-4 pass.
        let mut clamped = ring;
        clamped[1] = 0x2BC;
        w.seed_frame_cost_ring(clamped, 2);
        assert_eq!(w.ordinary_battle_frame_step(), 4);
    }

    #[test]
    fn a_seeded_step_holds_only_inside_its_state() {
        let mut w = World::new();
        w.toggles.battle_party_cost_step = false;
        w.seed_battle_frame_step(3, 0x33);
        w.battle_ctx.action_state = 0x32;
        w.apply_battle_frame_step_seed();
        assert_eq!(w.battle_frame_step(), DEFAULT_BATTLE_FRAME_STEP);
        w.battle_ctx.action_state = 0x33;
        w.apply_battle_frame_step_seed();
        assert_eq!(w.battle_frame_step(), 3);
        w.battle_ctx.action_state = 0x34;
        w.apply_battle_frame_step_seed();
        assert_eq!(w.battle_frame_step(), DEFAULT_BATTLE_FRAME_STEP);
    }

    /// A seed held on the close-up accumulator keeps the ordinary step in
    /// its state until the accumulator reaches the hold.
    #[test]
    fn a_seeded_step_held_on_the_accumulator_waits_for_it() {
        use legaia_engine_battle_vm::battle_cam_script::{BattleCamPhase, BattleCamera};
        let mut w = World::new();
        w.toggles.battle_party_cost_step = false;
        w.seed_battle_frame_step(3, 0x2B);
        w.battle.frame_step_seed_after_accum = Some(16);
        w.battle.camera = Some(BattleCamera::new(BattleCamPhase::Menu, 0));
        w.battle_ctx.action_state = 0x2B;
        w.apply_battle_frame_step_seed();
        assert_eq!(w.battle_frame_step(), DEFAULT_BATTLE_FRAME_STEP);
        w.battle.frame_step_seed_after_accum = Some(0);
        w.apply_battle_frame_step_seed();
        assert_eq!(w.battle_frame_step(), 3);
    }

    #[test]
    fn the_kept_object_spins_half_the_frame_step_a_battle_frame() {
        let mut w = World::default();
        w.seed_battle_backdrop_keep_object_1(true);
        // Default step 2: one unit a two-vsync frame.
        for _ in 0..20 {
            w.tick_battle_backdrop_spin();
            w.clock.display_frames += 1;
        }
        assert_eq!(w.battle_backdrop_slot_1_yaw(), 10);
        // Step 3 still adds `3 >> 1 = 1`, once every three vsyncs.
        w.set_battle_frame_step(3);
        for _ in 0..30 {
            w.tick_battle_backdrop_spin();
            w.clock.display_frames += 1;
        }
        assert_eq!(w.battle_backdrop_slot_1_yaw(), 20);
        // Step 1 adds nothing: `1 >> 1 = 0`.
        w.set_battle_frame_step(1);
        for _ in 0..8 {
            w.tick_battle_backdrop_spin();
            w.clock.display_frames += 1;
        }
        assert_eq!(w.battle_backdrop_slot_1_yaw(), 20);
    }

    #[test]
    fn a_stage_that_drops_object_1_keeps_the_angle_it_was_left() {
        let mut w = World::default();
        w.seed_battle_backdrop_slot_1_yaw(0x1AD3);
        w.seed_battle_backdrop_keep_object_1(false);
        for _ in 0..40 {
            w.tick_battle_backdrop_spin();
            w.clock.display_frames += 1;
        }
        // No keep byte, no winding - and no reset either.
        assert_eq!(w.battle_backdrop_slot_1_yaw(), 0x1AD3);
    }

    #[test]
    fn an_xa_read_span_runs_overrunning_step_four_frames_on_pcsx_redux() {
        use legaia_engine_battle_loop::frame_clock::{CdTiming, XA_STREAM_SPAN_EXTRA};
        let mut w = World::new();
        w.toggles.battle_cd_timing = CdTiming::PcsxRedux;
        w.party.party_count = 1;
        w.audio.battle_xa_busy_frames = 60;
        w.audio.battle_xa_load_frames = 60;
        w.audio.battle_xa_frame_load = true;
        w.push_frame_cost(None);
        assert_eq!(w.battle.frame_cost_step, 4);
        w.battle.frame_clock.step = 4;
        w.apply_cast_arm_frame_step();
        assert_eq!(w.battle.frame_clock.span_extra, XA_STREAM_SPAN_EXTRA);
        w.audio.battle_xa_busy_frames = 0;
        w.audio.battle_xa_load_frames = 0;
        w.apply_cast_arm_frame_step();
        assert_eq!(w.battle.frame_clock.span_extra, 0);
    }

    /// A full party's loaded frame runs its work under the polls: Gala's
    /// Flying Knee frames of `745`..`930` hblanks of work span `9` and `10`
    /// vsyncs, where an item's on a lone member spans `5`.
    #[test]
    fn a_full_partys_loaded_frame_spans_its_work_under_the_polls() {
        use legaia_engine_battle_loop::frame_clock::{CdTiming, frame_span, xa_loaded_frame_cost};
        let mut w = World::new();
        w.toggles.battle_frame_span_law = true;
        w.toggles.battle_cd_timing = CdTiming::PcsxRedux;
        w.party.party_count = 3;
        w.battle_ctx.action_state = 0x20;
        w.audio.battle_xa_busy_frames = 60;
        w.audio.battle_xa_load_frames = 60;
        w.audio.battle_xa_frame_load = true;
        w.audio.battle_xa_poll_load = true;
        w.battle.frame_clock.step = 4;
        w.apply_cast_arm_frame_step();
        let work = legaia_engine_battle_loop::frame_clock::measured_frame_cost(3, 0x20);
        let span = frame_span(xa_loaded_frame_cost(work), 4);
        assert!(span >= 7, "{span}");
        assert_eq!(u64::from(span), w.battle.frame_clock.span());
    }

    /// A dear frame on a cheap step runs on to the blank after its work, and
    /// a frame inside its step does not.
    #[test]
    fn an_ordinary_frame_spans_what_its_cost_runs_past() {
        let mut w = World::new();
        w.toggles.battle_frame_span_law = true;
        w.party.party_count = 3;
        w.battle_ctx.action_state = 0x1E;
        w.battle.frame_clock.step = 2;
        w.apply_cast_arm_frame_step();
        assert_eq!(
            w.battle.frame_clock.span(),
            3,
            "a 622-hblank frame on step 2"
        );
        w.battle.frame_clock.step = 3;
        w.apply_cast_arm_frame_step();
        assert_eq!(w.battle.frame_clock.span(), 3);
    }

    /// A replay's stretched frame keeps its step for the pass after it: the
    /// ring's pending step waits for the stretch to settle.
    #[test]
    fn a_pending_step_waits_out_a_replay_stretch() {
        let mut w = World::new();
        w.mode = legaia_engine_mode::mode::SceneMode::Battle;
        w.toggles.battle_sm_frame_cadence = true;
        w.clock.display_frames = 10;
        assert!(w.battle_sm_pass_due());
        w.battle.pending_frame_step = Some(4);
        w.battle.pending_frame_step_from = w.battle_frame_id();
        w.stretch_battle_frame(3);
        w.clock.display_frames = 13;
        assert!(w.battle_sm_pass_due());
        assert_eq!(
            w.battle.frame_clock.step, 3,
            "the stretched step runs the next pass"
        );
        assert_eq!(w.battle.pending_frame_step, Some(4));
    }

    /// A clip's stream loads the frame from the second pass after its cue:
    /// the Amethyst Heal recording costs `392`, `507`, then the clamp.
    #[test]
    fn a_clips_stream_loads_the_frame_two_passes_after_its_cue() {
        use legaia_engine_battle_loop::frame_clock::{CdTiming, XA_STREAM_FRAME_COST};
        let mut w = World::new();
        w.toggles.battle_cd_timing = CdTiming::PcsxRedux;
        w.party.party_count = 1;
        w.battle_ctx.action_state = 0x3E;
        w.audio.battle_xa_busy_frames = 200;
        w.audio.battle_xa_load_frames = 200;
        w.audio.battle_xa_frame_load = true;
        w.audio.battle_xa_cue_frame = Some(w.battle_frame_id());
        let mut costs = Vec::new();
        for _ in 0..3 {
            w.push_frame_cost(None);
            costs.push(w.battle.frame_cost_ring.newest());
            w.clock.display_frames += w.battle.frame_clock.span();
        }
        assert!(costs[0] < 0x1FF && costs[1] < 0x1FF, "{costs:?}");
        assert_eq!(costs[2], XA_STREAM_FRAME_COST.min(0x2BC));
    }

    /// A capture-band module's own line loads the frame from its cue's pass,
    /// whatever clip cued before it: the band's voiced casts film lower on
    /// the cue funnel's onset ramp (Call Wave `.784` to `.545`).
    #[test]
    fn a_capture_modules_line_loads_the_frame_from_its_cue() {
        use legaia_engine_battle_loop::frame_clock::{CdTiming, XA_STREAM_FRAME_COST};
        for earlier_cue_passes_back in [0u64, 1, 300] {
            let mut w = World::new();
            w.toggles.battle_cd_timing = CdTiming::PcsxRedux;
            w.party.party_count = 3;
            w.battle_ctx.action_state = 0x70;
            let mut spans = vec![0u16; 0x60];
            spans[0x58] = 527;
            w.audio.xa_cue_durations = Some(spans);
            // Another clip cued this many display ticks ago.
            w.audio.battle_xa_cue_frame = Some(w.battle_frame_id());
            w.clock.display_frames += earlier_cue_passes_back;
            w.emit_battle_xa_cue(0x158);
            assert!(w.audio.battle_xa_frame_load, "a band module seeks no file");
            w.push_frame_cost(None);
            assert_eq!(
                w.battle.frame_cost_ring.newest(),
                XA_STREAM_FRAME_COST.min(0x2BC),
                "{earlier_cue_passes_back}"
            );
        }
    }

    /// On a console the clip's stop watch takes a sixteenth of the CPU: the
    /// frames after the cue cost their work over that, inside their step.
    #[test]
    fn a_console_clip_leaves_the_frame_near_its_own_cost() {
        use legaia_engine_cast::cast_module_frame_step::{CdTiming, console_poll_cost};
        let mut w = World::new();
        w.toggles.battle_cd_timing = CdTiming::Console;
        w.party.party_count = 1;
        w.battle_ctx.action_state = 0x3E;
        let base = legaia_engine_battle_loop::frame_clock::measured_frame_cost_at(1, 0x3E, 2);
        w.audio.battle_xa_busy_frames = 200;
        w.audio.battle_xa_load_frames = 200;
        w.audio.battle_xa_frame_load = true;
        w.audio.battle_xa_cue_frame = Some(w.battle_frame_id());
        let mut costs = Vec::new();
        for _ in 0..3 {
            w.push_frame_cost(None);
            costs.push(w.battle.frame_cost_ring.newest());
            w.clock.display_frames += w.battle.frame_clock.span();
        }
        assert_eq!(costs[2], console_poll_cost(base), "{costs:?}");
        assert!(w.battle.frame_cost_step < 4);
        w.apply_cast_arm_frame_step();
        assert_eq!(w.battle.frame_clock.span_extra, 0);
    }

    #[test]
    fn a_cast_modules_voice_leaves_the_frame_its_own_cost() {
        // The summon films run their Done bands near `330` hblanks while the
        // module's clip still reads: the clip does not load the frame.
        let mut w = World::new();
        w.audio.battle_xa_busy_frames = 60;
        w.audio.battle_xa_load_frames = 60;
        w.audio.battle_xa_frame_load = false;
        for _ in 0..16 {
            w.push_frame_cost(None);
        }
        assert!(w.battle.frame_cost_step < 4);
        w.battle.frame_clock.step = w.battle.frame_cost_step;
        w.apply_cast_arm_frame_step();
        assert_eq!(w.battle.frame_clock.span_extra, 0);
    }
}
