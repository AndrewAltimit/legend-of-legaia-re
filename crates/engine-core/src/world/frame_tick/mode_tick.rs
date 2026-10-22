//! `World::tick` and the per-mode dispatch it runs.

use super::*;
use legaia_engine_mode::mode::SceneMode;

impl World {
    /// Per-frame world tick. Drives whichever scene-mode VMs are live.
    /// Returns the battle-step outcome when in [`SceneMode::Battle`], else
    /// `None`.
    ///
    /// Order of operations:
    ///  1. Effect pool tick - the faithful retail walker, run on the
    ///     ~60 Hz retail-frame sub-clock regardless of mode.
    ///  2. Per-actor move-VM tick - only for actors with bytecode loaded.
    ///  3. Per-actor physics tick (`FUN_80021DF4`) - drains timer,
    ///     advances motion, kicks the move-buffer cursor on
    ///     [`TickEvent::MoveVmKick`]. Runs over every active actor.
    ///  4. Per-actor keyframe / anim-player tick.
    ///  5. Mode-specific VM:
    ///     - `Battle`     → battle-action state machine step.
    ///     - `Field`      → field-VM step (or no-op if no bytecode loaded).
    ///     - `Cutscene`   → field-VM step (cutscenes use the same script VM).
    ///     - `Title`      → no further VM.
    ///
    /// This is the engine's counterpart of the retail master frame driver
    /// `FUN_80016444`: retail runs five `FUN_8002519C` **tick passes** over
    /// the actor-list heads `_DAT_8007C34C..0x36C` (pass 3/4 swapped by the
    /// `_DAT_1F800394 & 0x10` mirror bit), then five `FUN_8001D140` **render
    /// passes** (the scratchpad-SP trampoline into `FUN_8001ADA4`), with the
    /// display flip (`FUN_8001D058` → `FUN_80026CE4`) before the render
    /// passes in STR mode (0x15) and after them otherwise, plus dev error
    /// prints and the dev mode-transition writer `FUN_800179C0`. The engine
    /// splits that frame: the tick passes are the per-actor loops below,
    /// the render passes live in the host renderer (wgpu), the flip is the
    /// swapchain present, and the dev prints are not ported. Divergence:
    /// engine actors live in one pool with an `active` flag, not five
    /// linked lists - pass ORDER is preserved by the sequencing below.
    // PORT: FUN_80016444 (frame-pass sequencing; render/flip halves are the
    //                     host renderer's, dev prints not ported)
    pub fn tick(&mut self) -> Option<StepOutcome> {
        // A pinned encounter entry ([`EncounterState::rng_hold`]).
        if let Some(seed) = self.encounters.rng_hold {
            if self.mode == SceneMode::Battle {
                self.encounters.rng_hold = None;
            } else {
                self.rng_state = seed;
            }
        }
        // The move-VM strip set on screen is one tick's
        // (`MoveVmGlobals::strip_frame`).
        self.move_vm.begin_strip_tick();
        let outcome = self.tick_modes();
        // The battle camera observes the frame this tick produced, in every
        // mode (outside battle it drops its state) - once, here, for every
        // host (`crate::battle_cam_inputs`).
        self.tick_battle_camera();
        // The near-camera ghost pass reads the pose this tick settled
        // (`FUN_80046A20` calls `FUN_8004DC68` after its camera update).
        self.tick_battle_camera_ghost();
        self.tick_battle_anim_pass();
        // The volumetric ground-fog enhancement steps on the tick this frame
        // settled, after every actor moved (`legaia_render_kernels::fog_volume`).
        self.tick_fog_volume();
        outcome
    }

    fn tick_modes(&mut self) -> Option<StepOutcome> {
        self.frame += 1;
        // Does retail run the master frame driver on a frame in this mode?
        //
        // Every per-frame mode handler except one calls `FUN_80016444`, and
        // that call is what advances the actor pool at all - the five
        // `FUN_8002519C` tick passes over `_DAT_8007C34C..0x36C`. The exception
        // is mode 23 CARD, the mode the in-field pause menu runs under
        // (`_DAT_8007B83C = 0x17` in every menu-open capture): its handler
        // `FUN_80025F74` calls `FUN_80017978` instead, and that body is 18
        // instructions with three `jal`s, none of them `0x80016444`
        // (`0x80017978..0x800179BC`). So while the menu owns the frame retail
        // advances **no** actor, effect, move VM or animation; the CARD actor's
        // own `+0x0C` handler is the entire frame.
        //
        // Resolved through the ported mode table rather than spelled out as a
        // `SceneMode::Menu` literal here, so the rule and its provenance cannot
        // drift apart: [`legaia_engine_mode::mode::runs_master_frame_driver`] reads it off
        // the same `per_frame_stage` rows the mode-table oracles use.
        //
        // What still runs under the menu is retail's too, and is *outside* the
        // master driver: the CARD handler's frame-begin pass `FUN_8001698C`
        // (which is where the timed sound-source auto-release `FUN_800267FC`
        // lives) and its frame-end pass `FUN_80016B6C` (the cadence resolver
        // and the SFX cue ring) both run unconditionally - `FUN_80017978`
        // returns `move v0,zero`, so the handler's abort branch never fires.
        //
        // **Which consumers this gate actually moves**, stated plainly because
        // it is easy to over-claim. Neither shipped host reaches it with the
        // pause menu open: the native window `continue`s past `session.tick()`
        // while its boot-UI owns the frame, and the browser page's sim loop is
        // gated `if (advance && !menuOpen && ...)`. Both therefore freeze
        // *more* than retail does - they stop the frame-begin / frame-end
        // passes too, which retail keeps running under CARD. This gate is what
        // makes the correct split available: a host can now tick the world
        // under the menu and get retail's behaviour instead of choosing between
        // "everything runs" and "nothing runs". Today it is exercised by the
        // headless `World::tick` consumers (the replay / determinism / mode
        // trace drivers, and any future owner of `ModeDriver`).
        // REF: FUN_80025f74, FUN_80017978, FUN_80016444
        let runs_master_driver = legaia_engine_mode::mode::GameMode::for_scene_mode(self.mode)
            .map(legaia_engine_mode::mode::runs_master_frame_driver)
            .unwrap_or(true);
        // Bridge the vsync-rate pad to the game-tick-rate actor pool for the
        // op-0x49 submode screens: the hosts publish a pad word every tick and
        // the dispatcher runs every `frame_step` ticks, so without this the
        // half of the edges that land on a skipped pass are lost outright.
        // See `SubmodeScreen::pad_edge_latch`.
        self.latch_submode_pad_edge();
        // Age the post-battle spoils panel (armed by `finish_battle`) and the
        // no-encounters-here hint (armed by `arm_live_loop`).
        self.battle.spoils_frames = self.battle.spoils_frames.saturating_sub(1);
        self.encounters.scene_hint_frames = self.encounters.scene_hint_frames.saturating_sub(1);
        // ------------------------------------------------------------------
        // The simulation clock's denomination.
        //
        // **One `World::tick` is exactly one retail display frame (vsync).**
        // Both hosts drive it that way through one fixed-timestep kernel
        // (`legaia_engine_field::frame_step::SimStepper`, `TICK_SECS = 1/60`, backlog
        // capped at four ticks). So `SIM_HZ == RETAIL_FPS`, the retail-frame sub-clock is
        // an identity - `field_frame_step` is `1` on every tick and
        // `field_frames == frame` - and the gates below are statements of
        // which consumers are retail-frame paced rather than rate changes.
        //
        // Retail's frame has TWO clocks and `DAT_1F800393` relates them.
        // `FUN_80016B6C` resolves that byte once per frame
        // (`0x80017044..0x800171D8`: sample the frame time with `VSync(1)`
        // through `FUN_800173BC`, pick 1..4, raise it to the per-mode floor
        // `DAT_8007B9D8`, then `VSync(n)`-wait), so one pass of the master
        // driver `FUN_80016444` spans `DAT_1F800393` vsyncs - the **game
        // tick**. Every duration and every velocity inside that pass is
        // denominated in vsyncs and scaled by the byte, which makes the
        // wall-clock rates cadence-invariant:
        //
        //  * player locomotion `FUN_801D01B0` - called **once per game tick**
        //    from the field frame pump `FUN_801D1344` (`jal 0x801D01B0` at
        //    `0x801D16F4`, with no cadence gate of its own), and its travel
        //    budget for the call is
        //    `((base_step * player[+0x72]) >> 12) * DAT_1F800393`
        //    (`0x801D0564..0x801D05C4`: `mult s4,v0`; `sra s4,t1,0xc`;
        //    `lbu v1,0x7f(a1)` with `a1 = 0x1F800314`; `mult s4,v1`).
        //  * field-NPC motion `FUN_8003774C` - the same shape
        //    (`0x80037868 lbu s2,0x393(s2)`, then `mult ...,s2` into every
        //    glide leg).
        //  * the frame pump's own countdowns - the dialogue-pacing timer
        //    `_DAT_8007B6B4` (`0x801D1618..0x801D1630`) and the field-control
        //    byte `+0x62` (`0x801D1670..0x801D1690`) each subtract
        //    `DAT_1F800393`, not `1`.
        //
        // So retail's player advances `base_step` units per **vsync** whatever
        // the cadence does: at the field floor of 2 it runs the controller
        // 30x a second for `2 * base_step` each. The engine takes the
        // finer-grained half of that identity - the controller once per vsync
        // with the scalar at 1 - which lands on the same `base_step * 60`
        // units per second and merely emits twice as many intermediate poses.
        // Neither half is a place to insert a rate.
        //
        // The `SIM_HZ = 100` this replaces was a premise no host ever met. It
        // withheld 2 of every 5 retail frames from the *gated* consumers (the
        // narration crawl, the cutscene timeline, the effect pool, the escape
        // timer, NPC motion, the CLUT / ambient game-tick banks, the timed
        // sound release) - 36 Hz against retail's 60 - while the *ungated*
        // ones (locomotion, the per-actor field channels, the prop and
        // tile-board layers) ran at the correct 60. The two errors cancelled
        // in the one place anyone measured: `opening_chain_wall_time.rs`
        // divided observed ticks by 100 while the timeline emitted 0.6 retail
        // frames per tick, so the *seconds* came out right and the *unit*
        // stayed wrong.
        //
        // REF: FUN_80016B6C (cadence resolver), FUN_80016444 (master driver),
        //      FUN_801D1344 (field frame pump), FUN_801D01B0, FUN_8003774C
        const RETAIL_FPS: u32 = 60;
        const SIM_HZ: u32 = RETAIL_FPS;
        // A host that ever wants to oversample has to re-derive every
        // consumer below, not just relax this constant.
        const _: () = assert!(
            SIM_HZ == RETAIL_FPS,
            "one sim tick is one retail display frame"
        );
        self.clock.display_frame_step = 1;
        self.clock.display_frames += 1;
        // Kept advancing as the cheap "a world frame ran" witness (the mode
        // driver's frame-begin-skip test probes it); the fixed-point phase it
        // used to carry is gone with the 1:1 denomination.
        self.clock.sim_ticks = self.clock.sim_ticks.wrapping_add(1);
        // Retail game-tick clock for the scripted CLUT-cell effects: one game
        // tick spans `frame_step` vsyncs (the adaptive `DAT_1F800393` factor
        // written by `FUN_80016B6C`; see [`legaia_engine_vm::frame_clock::FrameClock::frame_step`]). Count the sim
        // ticks that map to a retail vsync and bank a game tick every
        // `frame_step` of them; [`Self::step_clut_fx`] drains the bank
        // against the host's VRAM. Only accumulates while effects are live
        // (capped so an undrained host can't wind up a backlog).
        if self.clock.display_frame_step == 1
            && !(self.ambient.clut_fx.is_empty() && self.ambient.clut_blend_fx.is_empty())
        {
            self.ambient.clut_vsync_accum += 1;
            if self.ambient.clut_vsync_accum >= self.clock.frame_step.max(1) {
                self.ambient.clut_vsync_accum = 0;
                self.ambient.clut_pending_game_ticks =
                    (self.ambient.clut_pending_game_ticks + 1).min(600);
            }
        }
        // Same game-tick law for the ambient move-VM effect parts (jou's
        // CLUT-cell cyclers / lightning director); drained by the host's
        // `step_ambient_fx` against its VRAM.
        if self.clock.display_frame_step == 1 && !self.ambient.fx.is_empty() {
            self.ambient.vsync_accum += 1;
            if self.ambient.vsync_accum >= self.clock.frame_step.max(1) {
                self.ambient.vsync_accum = 0;
                self.ambient.pending_game_ticks = (self.ambient.pending_game_ticks + 1).min(600);
            }
        }
        // The modelled CD drive under a field XA one-shot: one vsync of its
        // read span elapses per world tick (`World::push_field_xa_cue`).
        self.tick_field_xa_busy();
        // Retail's frame-begin driver services the timed sound-source
        // auto-release before anything else in the frame (`FUN_800267FC`,
        // called at `0x800169FC`). Its accumulator advances by the frame step,
        // so drive it on the sim ticks that map to a retail vsync.
        if self.clock.display_frame_step == 1 {
            let step = self.clock.frame_step.max(1);
            // The teardown gates (`record[+8]` active, `_DAT_8007B868`) live
            // in the libsnd voice binding the engine replaces, so the engine
            // arm is "release when it fires" unconditionally.
            if let legaia_engine_system::sound_state::SoundReleaseTick::Fired { .. } =
                self.audio.sound_release.tick(step, true, false)
            {
                self.audio.pending_sound_release = true;
                // What the expiry does is the field-BGM detach, inline: the
                // released record is the BGM slot `0x8007052C`, and the arm
                // at `0x80026828..0x8002686C` is `FUN_800266E0`'s body - pan
                // reset `FUN_8002657C(0, slot)`, `FUN_80064370(slot[+0xA])`,
                // `DAT_8007B708 = 0` - behind the same `_DAT_8007B868` gate.
                // `FUN_800266E0` is BGM sub-op 2's primitive, so the expiry
                // reaches both hosts' BGM routing as that pause.
                //
                // Except inside a sub-op 9 -> 0xA swap: there the slot still
                // holds the *outgoing* track (the poller `FUN_800243F0`
                // stalls its install on the commit), and that is what the
                // release stops - a cutscene's `9 · 5 · 0xA` fades the old
                // score out under the new one's load. The port started the
                // incoming track at sub-op 9, so pausing here would silence
                // the new score for good (the commit then releases the paused
                // source).
                if !self.audio.start_pending_commit {
                    self.pending_field_events.push(
                        legaia_engine_field::field_events::FieldEvent::Bgm {
                            text_id: 0,
                            sub_op: 2,
                        },
                    );
                }
            }
        }
        // Step the active full-screen fade. A template with a hold countdown
        // is dropped once the ramp lands (hosts stop drawing the overlay); one
        // whose hold word is `-1` - the battle-end / escape template - keeps
        // its end colour up until the battle teardown clears it
        // (`FadeState::holds_at_end`, `finish_battle`).
        //
        // In battle on retail's cadence the fade actors (`FUN_80025000`,
        // whose ramp `FUN_80020C14` drains `DAT_1F800393` a call) tick with
        // the battle pass, a frame step at a time
        // (`World::battle_anim_pass_vsyncs`); between passes they hold.
        let pass_vsyncs = if self.mode == SceneMode::Battle {
            self.battle_anim_pass_vsyncs()
        } else {
            1
        };
        self.battle.anim_pass_vsyncs = pass_vsyncs;
        if pass_vsyncs > 0 {
            if let Some(fade) = &mut self.presentation.fade
                && !fade.step_vsyncs(pass_vsyncs)
                && !fade.holds_at_end()
            {
                self.presentation.fade = None;
            }
            self.presentation
                .module_fades
                .retain_mut(|f| f.step_vsyncs(pass_vsyncs) || f.holds_at_end());
            self.step_cast_hook_fades(pass_vsyncs);
        }
        // Step the scripted global multiply tint (op `0x4C 0x12`). A ramp
        // that lands on a non-neutral target HOLDS there (a screen faded to
        // black stays black until a new op replaces it); one that lands on
        // the neutral identity is dropped so the render path returns to
        // untouched. The op-`0x34` sub-0 screen effect is **not** a second
        // channel here - it is a pool colour tween emitting
        // `FUN_80024EE4` pushes, stepped by `tick_handler_actors`.
        if let Some(t) = self.presentation.tint.as_mut() {
            t.step();
            if t.is_identity() {
                self.presentation.tint = None;
            }
        }
        // Step an op-`4C 13` clear-colour ramp (its `FUN_8003C5F0` slot jobs).
        if let Some(r) = self.presentation.clear_ramp.as_mut() {
            r.elapsed = r.elapsed.saturating_add(1);
            self.presentation.clear_rgb = r.value();
            if r.elapsed >= r.total {
                self.presentation.clear_ramp = None;
            }
        }
        // Consume a pending FMV transition the field VM signalled last frame
        // (op `0x4C 0xE2`). Retail's main mode dispatcher reads the
        // next-game-mode global one frame after the op writes it, so the flip
        // into the cutscene mode lands here, at the top of the following tick.
        self.maybe_enter_pending_cutscene();
        // Effect-pool walker on the retail-frame sub-clock: retail runs the
        // per-frame walker once per rendered frame with its vsync catch-up
        // factor - one sweep per vsync. Under the 1:1 denomination above that
        // is one sweep per sim tick; the gate names the clock the walker's
        // wait counters are denominated in rather than thinning them.
        // REF: FUN_801E0088
        //
        // In battle on retail's cadence the walker's sweeps run with the
        // battle pass, one per vsync the pass spans, so an effect stands
        // where the pass left it until the next one (as on retail's screen).
        if self.clock.display_frame_step == 1 && runs_master_driver {
            for _ in 0..pass_vsyncs {
                self.tick_effects();
            }
        }
        if runs_master_driver {
            self.tick_move_vms();
        }
        // Actor pool on the retail **game-tick** clock. Retail resolves one
        // `DAT_1F800393` per frame (`FUN_80016B6C`) and runs the per-actor
        // dispatcher once per game tick, so with the field floor of 2
        // (installed by the scene loader `FUN_801D6704`) the pool advances
        // every second vsync - and the tick that fires carries `frame_step`
        // into the dispatcher's scalars instead of `1`.
        //
        // The pairing is the whole point, and neither half is correct alone:
        // every retail duration accumulates `DAT_1F800393` rather than `1`
        // (`t = min(t + dt, d)`), which makes durations **cadence-invariant** -
        // a 600-frame move arrives after 600 vsyncs at any cadence. Gating
        // without the scalars would halve wall-clock speed; scaling without
        // the gate would double it. Together they leave every duration where
        // it was and only drop the *sample rate*: retail emits a pose every
        // `frame_step` vsyncs, so the engine draws proportionally fewer
        // intermediate poses over the same wall-clock span.
        //
        // REF: FUN_80016B6C (cadence resolver), FUN_801D6704 (field floor)
        if self.clock.display_frame_step == 1 && runs_master_driver {
            self.clock.actor_vsync_accum += 1;
        }
        let cadence = self.clock.frame_step.max(1);
        let actor_tick_fired = self.clock.actor_vsync_accum >= cadence && runs_master_driver;
        self.clock.game_tick_fired = actor_tick_fired;
        // The script contexts stepped below time themselves on this clock
        // (`FrameClock::script_frame_delta`).
        self.clock.game_tick_live = self.clock.display_frame_step == 1 && runs_master_driver;
        if actor_tick_fired {
            self.clock.actor_vsync_accum = 0;
            self.tick_actor_physics();
            // The `jalr node[+0x0C]` arm of the same walk: run the ported
            // per-frame handler kernels (the colour tween, the field VM
            // clone's clip-fraction fade, the `4C 86` reflection pairs) and
            // drop the actors that raised the kill bit - this pass's own
            // expiries plus any marked by the scene-transition sweep or a
            // `FUN_8003CF40` retire since the last tick.
            // REF: FUN_8002519C
            self.tick_handler_actors(cadence);
            self.tick_actors();
            // Actor-VM glides (op 0x09 `MotionAt` -> `start_motion`): one
            // motion-VM pursue step per game tick toward the recorded target.
            self.tick_actor_motions();
        }
        // Drain the scripted countdown the field VM armed with `0x4C 0xD3`.
        // Retail's scheduler runs once per display frame off the play clock,
        // so drive it on the same retail-frame sub-clock the other 60 Hz
        // consumers use.
        if self.clock.display_frame_step == 1 {
            self.tick_escape_timer();
        }
        // Tick art-learned banner countdown - clear when it reaches zero.
        if let Some(banner) = &mut self.party.current_art_banner {
            if banner.frames_remaining > 0 {
                banner.frames_remaining -= 1;
            } else {
                self.party.current_art_banner = None;
            }
        }
        // Tick level-up banner countdown; when it expires the next member who
        // levelled in the same fight takes the slot (see
        // `World::party.pending_level_up_banners`).
        if let Some(banner) = &mut self.party.current_level_up_banner {
            if banner.frames_remaining > 0 {
                banner.frames_remaining -= 1;
            } else {
                self.party.current_level_up_banner =
                    self.party.pending_level_up_banners.pop_front();
            }
        }
        // Advance the post-battle Seru-capture banner; clear when it finishes.
        if let Some(banner) = &mut self.party.current_capture_banner {
            banner.tick_frame();
            if banner.is_done() {
                self.party.current_capture_banner = None;
            }
        }
        // Advance the opening-cutscene narration roller. The crawl is
        // timer-driven only (retail `FUN_80037174` has no per-line confirm
        // skip; the player skips the WHOLE opening through the hand-off
        // packet instead - see `take_prologue_handoff`). Clear it once every
        // page has scrolled off so the suspended cutscene timeline resumes.
        // The roller counts vsyncs and runs one handler pass per
        // `frame_step`-vsync retail frame it was opened at (`cutscene_narration`).
        if let Some(narration) = &mut self.cutscene.narration
            && !narration.tick(self.clock.display_frame_step as u32)
        {
            self.cutscene.narration = None;
        }
        // Fade the "It was the Seru." caption image (opdeene's baked-TIM
        // caption, `Self::cutscene_caption`). It is target-visible in the
        // FIRST gap after a narration crawl block has shown (a block opened,
        // `seq >= 1`, and has since scrolled out - narration inactive), and
        // fades out on the next block or scene end (the image is cleared on
        // scene entry). At the retail-video-pinned crawl rate the blocks run
        // back-to-back, so the first real gap lands after the LAST crawl -
        // the caption fades in over the held villager tableau, which is
        // where the retail capture shows it. The smooth alpha ramp stands in
        // for the TIM's two-CLUT fade steps.
        //
        // The timeline's post-crawl hold can run long, so `in_gap` alone
        // would freeze the caption on screen. Bound it to a retail-like ~2 s
        // beat: once it has been fully shown for `CAPTION_HOLD_FRAMES`, fade
        // it back out and keep it hidden (the counter never resets within
        // the scene, so the caption shows exactly once).
        if self.cutscene.caption.is_some() {
            const CAPTION_FADE_STEP: f32 = 0.06;
            const CAPTION_HOLD_FRAMES: u32 = 180;
            let in_gap = self.cutscene.narration_seq >= 1 && !self.cutscene_narration_active();
            if in_gap && self.cutscene.caption_alpha >= 1.0 {
                self.cutscene.caption_shown_frames =
                    self.cutscene.caption_shown_frames.saturating_add(1);
            }
            let hold_elapsed = self.cutscene.caption_shown_frames >= CAPTION_HOLD_FRAMES;
            let target = if in_gap && !hold_elapsed { 1.0 } else { 0.0 };
            if self.cutscene.caption_alpha < target {
                self.cutscene.caption_alpha =
                    (self.cutscene.caption_alpha + CAPTION_FADE_STEP).min(target);
            } else if self.cutscene.caption_alpha > target {
                self.cutscene.caption_alpha =
                    (self.cutscene.caption_alpha - CAPTION_FADE_STEP).max(target);
            }
        }
        // Tick the live `4C E1` text balloon (FUN_801DA7F0 handler; see
        // `legaia_engine_dialog::text_balloon`). The player-engaged flag (`_DAT_8007C364
        // +0x10 & 0x80000`) is host-substituted by "a dialog engagement is
        // live"; the cadence is the 60 fps sub-clock step, matching the
        // narration roller above.
        let balloon_engaged = self.dialogue_owns_input();
        if runs_master_driver && let Some(balloon) = self.cutscene.text_balloon.as_mut() {
            let engaged = balloon_engaged;
            let cadence = self.clock.display_frame_step as i16;
            if balloon.tick(engaged, cadence)
                == legaia_engine_dialog::text_balloon::BalloonTick::Killed
            {
                self.cutscene.text_balloon = None;
            }
        }
        // Run every live camera-register zone ramp (op `0x43` sub-3..6). Same
        // position in the frame as the balloon above and for the same reason:
        // both are `+0x0C` handlers on retail's one effect-actor list, and
        // this is the one tick path all three hosts reach. Which is also why
        // both sit behind `runs_master_driver`: an actor-list handler is
        // reached only through `FUN_8002519C`, and the CARD-mode frame does not
        // walk the lists at all.
        if runs_master_driver {
            self.tick_register_ramps();
        }
        // The camera vertical-offset easing `FUN_801DA390`: one call a frame,
        // walking `_DAT_8007BCAC` toward `scene_ctrl[+0x4A] - player[+0x16]`.
        // Same gate as the ramps above - retail runs it off the field frame
        // pump, which the CARD-mode frame does not reach.
        if runs_master_driver {
            self.tick_camera_offset_ease();
        }
        // The three frame-delta timer templates (`0x801F2858` bars,
        // `0x801F2840` eased moves, `0x801F27EC` floor-ladder rungs) ride the
        // same gate for the same reason: all three are `+0x0C` handlers on
        // that one effect-actor list.
        if runs_master_driver {
            let delta = self.clock.display_frame_step.min(u16::from(u8::MAX)) as u8;
            self.tick_field_timer_actors(delta);
            // The element channel rides the same gate for the same reason -
            // the ambient emitter is a `+0x0C` handler on that one
            // effect-actor list, and its own template
            // (`0x801F271C`) sits in the very table the three above come from.
            // Self-gates to a no-op on an empty channel.
            let mut rng = crate::world::WorldRng::new(self.rng_state);
            self.tick_cutscene_elements(delta, || rng.step());
            self.rng_state = rng.state();
            // The fog pool's render step (`FUN_8003F348`, run from the field
            // render pass) sees `DAT_1F800393` as its `dt`; the hosts call
            // it from their draw path, so the ticks accumulate until then.
            self.fog.pending_dt = self.fog.pending_dt.saturating_add(u32::from(delta));
        }
        // The three-actor-talk controller's per-frame flag poll: when the
        // scene script drops the talk lock (system flag 0xD), the controller
        // despawns and the story party un-collapses. Same all-hosts tick
        // position as the ramps above.
        if runs_master_driver {
            self.tick_three_actor_talk();
        }
        // Menu-staged Door uses (Door of Wind warp / Door of Light
        // escape): hand the staged record to the pause-menu session, whose
        // travel art ends in the named scene transition the scene host
        // already drains.
        self.drain_staged_menu_warp();
        // The pause-menu session's post-menu half: the ramp-down to the
        // travel-art hand-off, then the art itself, on every host.
        self.tick_pause_session();
        // A minigame the player can enter must be one the player can leave.
        self.poll_minigame_escape();
        // Age the minigame effect-part pool. Here rather than in a host's own
        // frame step: a pool a host owns ages only on that host, and the
        // fishing splash spent its whole life native-only for exactly that
        // reason (see [`legaia_engine_minigames::minigame_fx`]). Unconditional, like the ramps
        // above - a pool with no live part costs a length test.
        self.minigames.fx.tick(1);
        match self.mode {
            SceneMode::Battle => {
                // The frame driver's discarded `rand()` draw, once a battle
                // frame (`FUN_80046A20` `0x80046D2C`).
                self.tick_battle_pass_draw();
                // The kept object 1's spin, the next store on the same pass
                // (`0x80046D34`).
                self.tick_battle_backdrop_spin();
                // Battle animation advance. This is SIMULATION, not
                // presentation: its staged-clip end edge retires `ADVANCE_DONE`
                // and converges the anim id pair - the pacing gate whose
                // failure parks the attack chain at `AttackChain` (`0x1E`)
                // forever. It ran only from the native window's redraw, so the
                // browser and headless hosts never advanced it at all; it must
                // sit here, where every host reaches it. Retail advances the
                // anim system in the frame driver's tick passes (`FUN_8002519C`)
                // ahead of the render passes, which is this position.
                //
                // Ahead of the dialogue gates below, and outside
                // `live_battle_tick`, deliberately: that function early-returns
                // while a command session or a submenu is open, and a command
                // session stays open for as long as the player deliberates.
                // Driving the anims from inside it would freeze every actor's
                // idle loop for that whole window and un-freeze it on confirm.
                // REF: FUN_8002519C
                //
                // On retail's cadence the pass runs once a battle frame and
                // advances by the frame step (`World::battle_anim_pass_vsyncs`);
                // the ticks between hold every actor where the pass left it.
                //
                // On retail's cadence the anim tick runs after the frame
                // driver's pass instead (`World::tick_battle_anim_pass`).
                self.battle.anim_slot_reached = false;
                if self.battle.anim_pass_vsyncs > 0 && !self.toggles.battle_sm_frame_cadence {
                    self.tick_battle_animations();
                }
                // The Arts announcement banner's slide clock. Retail steps it
                // from the battle DRAW tick (`FUN_800480D8`); the engine steps
                // it here so every host advances it, and reads the quads back
                // at draw time (`World::battle_arts_banner_quads`).
                // REF: FUN_800480D8
                self.tick_arts_banner(cadence);
                // In-battle dialogue box (the tutorial text the engage script
                // opened across the transition): the box owns the frame -
                // retail parks the battle under it (the camera holds the
                // dialogue close-up while the text is up) and a confirm /
                // cancel press advances / dismisses it. Drive whichever
                // dialogue channel is live: the inline-script runner carried
                // across the Field -> Battle transition (only Field ticks it
                // otherwise, so it would stick mid-line forever), or the
                // simplified `current_dialog` box on the field / overworld
                // dismiss idiom (`op4c_n5_sub4_dialog_advance` /
                // `tick_world_map_npc_dialog`).
                let out = if self.dialog.inline.is_some() {
                    self.drive_inline_dialogue();
                    None
                } else if self.dialog.current.is_some() {
                    if self.input.just_pressed(input::PadButton::Cross)
                        || self.input.just_pressed(input::PadButton::Circle)
                    {
                        self.dialog.current = None;
                        self.pending_field_events
                            .push(legaia_engine_field::field_events::FieldEvent::DialogDismissed);
                    }
                    None
                } else {
                    // A battle that was ENTERED must be DRIVEN. Retail's
                    // action SM (`FUN_801E295C`) has no "loop enabled"
                    // concept - once the battle scene is up it always runs
                    // the full per-frame driver until a wipe resolves it.
                    // This arm used to be gated on
                    // [`crate::world::WorldToggles::live_gameplay_loop`], falling back to a bare
                    // [`Self::step_battle`] that applies no damage, arms no
                    // turn and never calls [`Self::finish_battle`] - while
                    // battle *entry* (a field carrier's `3E FF` scripted
                    // fight, a world-map region encounter) was never gated
                    // at all. The result was an unresolvable battle: the
                    // ungated entry paths could strand a default session in
                    // `SceneMode::Battle` forever. The Field arm's random
                    // encounter *roll* stays opt-in below; driving a battle
                    // the engine is already in does not.
                    // REF: FUN_801E295C (the retail action SM this drives)
                    self.live_battle_tick()
                };
                // The stage colour pass (`FUN_80050120`'s ambient and
                // backdrop ramps) runs after the action SM in the frame
                // driver (`jal 0x801E295C` at `0x80047048`, `jal 0x80050120`
                // at `0x8004712C`), so the summon close-up's `ctx[+0x243]`
                // dims the stage on the very pass that raises it - in step
                // with the close-up accumulator the same pass advances.
                // Ahead of the SM it trailed the accumulator by a frame.
                self.tick_battle_ambient();
                out
            }
            SceneMode::Field => {
                // The Field arm as a whole is the engine's counterpart of
                // the retail player master frame handler - the field
                // overlay's per-frame driver that wraps everything below:
                // engaged-flag gating, locomotion (FUN_801D01B0), the
                // vertical settle (FUN_801D1BA0), touch/walk-on dispatch
                // (FUN_801DE234/801DE3E0 through FUN_801D1EC4), the camera
                // update (FUN_801DB510/801DAA50) and the intro-skip packet
                // (pad 0x100 while `_DAT_1F800394 & 0x4000000` ->
                // `FUN_8001FD44("town01", 3)`, ported as
                // `World::take_prologue_handoff`). Leg-for-leg mapping in
                // the comments below; the retail body is
                // `overlay_cutscene_dialogue_801d1344.txt`.
                // PORT: FUN_801d1344 (frame-pump orchestration; legs are
                //                     individually ported + cited below)
                //
                // Per-tick: one Cross/Circle edge feeds at most one of the
                // script's 0x4C dialog poll or the interaction probe.
                self.dialog.input_consumed = false;
                // A committed battle suspends every field script context
                // until the fight returns (`Self::field_scripts_held_for_battle`).
                let scripts_held = self.field_scripts_held_for_battle();
                // Retail-frame paced (see `step_spawned_record_contexts`).
                if !scripts_held {
                    self.step_spawned_record_contexts();
                }
                // No placement channel steps here: retail runs a placement's
                // own script only while a touch holds it engaged
                // (`World::pre_run_field_channel_prologues` has the
                // `+0x10 & 0x100` writers), which the engine plays as the
                // interaction timeline.
                let scripts_held = scripts_held || self.field_scripts_held_for_battle();
                // The scene system script (ctx `0xFB`) gets a whole retail
                // frame slice, not one instruction: see
                // [`Self::step_field_frame_slice`] for the three stop
                // conditions and what one-op-per-tick cost.
                if !scripts_held {
                    self.step_field_frame_slice();
                }
                // A placement the system script engaged (`B1 <id> 08`) runs
                // its interaction once the frame is free.
                self.drain_placement_engagements();
                // Field script actors the VM just spawned or is running: the
                // op-0x43 scripted arcs (arc helper `FUN_801D5C08` + release
                // watcher `FUN_801D5D60`) and the op-0x34 sub-1 attached
                // lights' tear-down + keyframe script (`FUN_801E4470` /
                // `FUN_801E3E00`). Pool actors, one visit per frame.
                // REF: FUN_801d5d60, FUN_801e3e00
                self.tick_field_script_arcs();
                self.tick_field_attached_lights();
                // Field-NPC walk legs (cutscene walk-to-tile pokes,
                // actor-VM glides) - one motion-VM step per RETAIL
                // frame, writing back into `field_npc_positions` so collision /
                // interact probes follow the live NPC. The step decode takes
                // `dt = _DAT_1f800393` at 1, so one call credits one retail
                // display frame of glide (`field_npc_walk_step_speed`) and the
                // call rate has to be the retail 60 Hz frame clock. Retail
                // reaches the same wall speed from the other side: it visits
                // `FUN_8003774C` once per game tick and multiplies each leg by
                // `DAT_1F800393` (`0x80037868 lbu s2,0x393(s2)`).
                // REF: FUN_8003774C
                if self.clock.display_frame_step == 1 {
                    self.tick_field_npc_motions();
                }
                // Ambient facing channels (`FUN_80038158` ops 0x04 / 0x0D):
                // the idle turn-in-place a standing town NPC runs between
                // walk legs. Part of the actor pool, so it advances on the
                // actor game tick, not per rendered frame - which is what
                // keeps op 0x0D in lockstep with its ramp scheduler.
                // REF: FUN_80038158, FUN_80036D80
                if actor_tick_fired {
                    self.tick_field_npc_ambient();
                    // The same driver's height arm (`FUN_8003BC08`), after
                    // the tick moved anyone: the glide-class NPCs' Y.
                    self.tick_field_npc_heights();
                }
                self.tick_tile_board();
                // Rebuild the tile-actor draw list from the current board +
                // player cell (retail's per-frame board render pass).
                self.refresh_tile_board_draw_list();
                self.step_field_locomotion();
                // Walk-regen: drain the accumulator the step above just fed
                // and apply the three accessory-gated restore bumps. Its
                // Incense zero edge raises the wear-off notice, which runs
                // here too.
                self.tick_field_walk_regen();
                self.tick_incense_notice();
                // Vertical settle + ledge-hop trigger. Retail runs this as a
                // separate per-frame controller after the walk commits, so
                // it reads the step-delta pair the walk just wrote.
                // PORT: FUN_801d1ba0
                if let Some(pslot) = self.player_actor_slot {
                    self.step_field_vertical(pslot as usize);
                }
                // The system channel's tick follows the player's: its
                // per-tick store of the idle clip base lands after the settle
                // has read the base.
                self.tick_field_system_channel_clip_reset();
                // Motion detection: diff every tracked actor's position
                // against last frame's. Runs after EVERY mover in the frame
                // (timeline, channels, field VM, NPC motion legs, locomotion)
                // so the walk clip is selected by whether an actor moved, not
                // by which subsystem moved it - the script paths commit a
                // position and raise no flag of their own.
                self.tick_player_scale_ramp();
                self.tick_npc_heading_ramps();
                self.npcs.looks.tick(self.move_vm.ramp_ratio.max(1));
                self.detect_field_actor_motion();
                // Locomotion animation: idle vs walk off the movement flag
                // the step above just set, folded into the player's
                // `pose_frame` for the host's posed-mesh rebuild.
                self.tick_field_player_anim();
                // Placed-prop layer: advance the prop clips, step an
                // in-flight prop record run (a door swing / cupboard search
                // through the field VM), and start a run for a movement
                // touch the locomotion just posted (the retail bit-4
                // auto-post of FUN_801D5B5C).
                self.tick_prop_interactions();
                // Interaction probe (retail FUN_801cf9f4): talk to an adjacent
                // NPC / dismiss its box on the action button. Runs before the
                // carrier tick so a dialogue-accept engage launches the battle
                // the same frame.
                self.tick_field_interaction_probe();
                self.tick_field_carriers();
                // Faithful dialogue path (opt-in): drive a just-opened field
                // dialogue through the field VM so branch handlers execute.
                self.drive_inline_dialogue();
                // Interaction teardown: put an addressed NPC's authored facing
                // back once no dialogue channel owns the frame any more
                // (retail's `+0x5A` -> `+0x26` restore on the dialog SM's exit
                // path). Placed after the runner start above, so the frame the
                // talk begins already counts as engaged and the save survives.
                // REF: FUN_80039B7C
                // A placement's own context resumed as the interaction
                // timeline (a boss stager) is the same engaged SM: its
                // restore waits for the timeline's `0x21`.
                if !self.dialogue_owns_input()
                    && self.dialog.active_inline_prologue.is_none()
                    && !self.interaction_timeline_holds_talk_facing()
                {
                    self.release_talk_facing();
                }
                // Screen-effect widgets (mask / sprite / panel / letterbox,
                // the ending-scene op-0x43 family) tick after the script step
                // that may have spawned them this frame.
                self.tick_screen_fx();
                if self.toggles.live_gameplay_loop {
                    self.live_field_tick();
                } else {
                    // `--no-live-loop` gates the encounter *roll* only: a
                    // battle something else armed (a scripted carrier's
                    // transition) is still clocked and drained, so the
                    // intro plays and the fight opens.
                    self.tick_encounter();
                    if let Some(roll) = self.drain_encounter_formation() {
                        self.begin_encounter_battle(roll);
                    }
                }
                None
            }
            SceneMode::Cutscene => {
                // An in-engine choreography cutscene (no STR FMV) is just a
                // field scene that suppresses field/battle dispatch, so the
                // field VM keeps stepping. While an STR FMV is playing
                // ([`active_fmv`] set), the field VM is suspended - retail
                // hands the frame to the cutscene/MDEC overlay - and the host
                // drives playback, calling [`finish_cutscene`] when it ends.
                if self.cutscene.active_fmv.is_none() {
                    self.step_spawned_record_contexts();
                    self.step_field_frame_slice();
                    self.tick_field_script_arcs();
                    self.tick_field_attached_lights();
                    self.tick_screen_fx();
                }
                None
            }
            SceneMode::WorldMap if self.world_map.map_display.is_some() => {
                // The map display (retail game mode `0x0D`) owns the frame:
                // its handler `FUN_80025F2C` runs the display tick and the
                // render tick, and none of the walk frame.
                if let Some(ctrl) = self.world_map.ctrl.as_mut() {
                    // The walk controller's one-tick reports do not outlive
                    // the frame that raised them.
                    ctrl.map_display_requested = false;
                    ctrl.entry_fade_draw = None;
                }
                self.tick_map_display();
                None
            }
            SceneMode::WorldMap => {
                // The opening chain's `map01` fly-in leg runs its cutscene
                // record over the world map (Mist title card + crawl + the
                // terminal SceneChange into Rim Elm), and a free-roam overworld
                // walk-on **beat** record (a Drake mist-wall force-walk band, a
                // gate-1 non-portal partition-2 record spawned by
                // `SceneHost::dispatch_walk_on_trigger` in WorldMap mode) is the
                // same single-context cutscene timeline. Step whichever is
                // installed; `step_world_map_locomotion` stands the overworld
                // player down while it plays (the force-walk lock).
                // Overworld helper spawns (an op-0x44 issued by a world-map
                // record) execute concurrently, same as the field arm - both
                // are retail-frame paced.
                //
                // One Cross/Circle edge per tick, as in the field arm: the
                // script's `0x4C` dialog poll and the talk-to probe in
                // `tick_world_map` share it.
                self.dialog.input_consumed = false;
                self.step_spawned_record_contexts();
                // The scene system script (ctx `0xFB`, MAN `P1[0]`). The
                // overworld is a mode-3 field-run scene and retail runs its
                // entry script like any field's: `map01`'s sets the visible
                // tile window (`46 24 EE F4 12 20`, `(-18, -12, 18, 32)` -
                // the window every library `map01` state holds) and raises
                // the ambient-particle gate (`4C 30`) that puts fog over the
                // continent. Same frame slice as the field arm.
                self.step_field_frame_slice();
                // Cross-context walk legs a world-map record starts
                // (`C7 <id> ..`) run on the same per-actor walk kernel as the
                // field's (`FUN_8003774C`, from the actor driver every mode-3
                // scene runs). Without the step an overworld beat that walks
                // a placement and then waits on its halt bit (`B3 <id> 0A` -
                // urudre2's hand-off onto `map01`) parks for good.
                // REF: FUN_8003774C
                if self.clock.display_frame_step == 1 {
                    self.tick_field_npc_motions();
                }
                // The same driver's scripted-motion half (`FUN_80038158`,
                // `jal` at `0x8003BD98`): each placed actor's tail-section-1
                // stream, its variant re-selected every tick on a story
                // flag. The overworld's own streams write story flags:
                // `map02` P1[0] raises `0x5A4` (P2[14]) once the Karisto cart
                // has stood east long enough, and the cart actor's `0x5A4`
                // variant flies it back and raises `0x467` / drops `0x466`,
                // the flags `station` P1[15] sells the ride on. Without the
                // tick the cart never returned.
                // The driver's height arm (`0x8003BC3C..0x8003BCF4`) runs on
                // the overworld too - `FUN_8003BC08` has no world-map gate.
                // Without it every placement snaps to the floor sample, so
                // `map03`'s floating castle (P1[1]: `31 1D` + `4C 42 84 03`,
                // `+0x16 = -(+0x8E)` = 900 up) drifts along the ground.
                // REF: FUN_8003BC08, FUN_80038158
                if actor_tick_fired {
                    self.tick_field_npc_ambient();
                    self.tick_field_npc_heights();
                }
                // The per-actor anim tick: the overworld's MAN actors play
                // the kingdom bundle's slot-4 clips through `FUN_800204F8`
                // exactly as a town's do (a live `map01` actor list holds
                // four clip-bound actors resolving into that bank).
                self.tick_actor_anims();
                // Clock a committed overworld encounter's field-to-battle
                // transition (the intro overlay rides this phase) and open
                // the fight when it elapses - the world-map twin of the
                // field drain in `live_field_tick`.
                self.tick_encounter();
                if let Some(roll) = self.drain_encounter_formation() {
                    self.enter_world_map_battle(roll);
                }
                self.tick_world_map();
                None
            }
            SceneMode::Dance => {
                self.tick_dance();
                None
            }
            SceneMode::Fishing => {
                self.tick_fishing();
                None
            }
            SceneMode::SlotMachine => {
                self.tick_slot_machine();
                None
            }
            SceneMode::BakaFighter => {
                self.tick_baka_fighter();
                None
            }
            SceneMode::MuscleDome => {
                self.tick_muscle_dome();
                None
            }
            // The pause menu owns the frame (retail CARD mode 0x17): field /
            // battle dispatch is suspended; the hosting session drives the
            // menu state machine and restores the suspended mode on close.
            SceneMode::Menu => None,
            SceneMode::Title => None,
        }
    }
}
