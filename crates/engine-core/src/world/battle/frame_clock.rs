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

    /// Install a seeded step while the action SM sits in its state, and the
    /// default outside it. Called after every action-SM step.
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
            });
        let want = if self.battle_ctx.action_state == state && reached {
            step
        } else {
            DEFAULT_BATTLE_FRAME_STEP
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
        let _discarded = self.next_rand();
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
        if self.mode != legaia_engine_mode::mode::SceneMode::Battle
            || !self.toggles.battle_sm_frame_cadence
            || self.battle.anim_pass_vsyncs == 0
        {
            return;
        }
        // What the previous pass's draw stored, for this frame's readout
        // (`BattleState::readout_trios`).
        self.battle.readout_trios = (0..self.actors.len())
            .map(|i| self.battle_display_trio(i))
            .collect();
        let commits_before: Vec<u32> = self.actors.iter().map(|a| a.battle_clip_commits).collect();
        self.tick_battle_animations();
        // Every commit the advance made is the committing actor's own.
        let mut rewalk: Vec<usize> = commits_before
            .into_iter()
            .enumerate()
            .filter(|&(k, before)| {
                self.actors.get(k).map(|a| a.battle_clip_commits) != Some(before)
            })
            .map(|(k, _)| k)
            .collect();
        if self.battle.anim_slot_reached {
            let events_before = self.pending_battle_events.len();
            self.tick_battle_locomotion_dt(self.battle.anim_pass_vsyncs);
            self.tick_battle_hit_events();
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
        // The draw: every body pair from the pose the pass left, for the
        // next pass's SM and camera to read - a seat the root-motion pass
        // has not seeded yet included (the draw stores every drawn actor's).
        self.seed_battle_seats();
        self.refresh_battle_body_pairs();
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
    /// (`camera` `.3` to `.8` across the corpus).
    pub fn battle_camera_state(&self) -> u8 {
        let live = self.battle_ctx.action_state;
        if matches!(live, 0x32..=0x37) {
            return live;
        }
        let ran = self.battle.sm_ran_state.unwrap_or(live);
        if self.toggles.battle_sm_frame_cadence || ran == 0x0C {
            ran
        } else {
            live
        }
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
            self.battle_frame_step().max(1)
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
        for _ in 0..delta {
            let phase = self.casting.module_phase;
            live = tick(self);
            if !live || self.casting.module_phase != phase {
                if !cadence && live && self.casting.module_phase != phase {
                    self.casting.module_handover_waits = true;
                }
                break;
            }
        }
        live
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
        let step = u64::from(clock.step.max(1));
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

    #[test]
    fn a_seeded_step_holds_only_inside_its_state() {
        let mut w = World::new();
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
}
