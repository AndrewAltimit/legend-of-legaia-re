//! The redraw's simulation half: the sim ticks one redraw drains and the
//! per-tick steps (scripted input, the modal arms that own a tick, the field
//! pad, the world tick and its frame tail).

use super::super::*;
use super::redraw::tick_menu_runtime_session;
use legaia_engine_session::TickOwner;

impl PlayWindowApp {
    /// Drain this redraw's sim ticks (the shared `frame_step::SimStepper`
    /// rule, or one tick under a `--screenshot` capture), ending a finished
    /// windowed movie first.
    pub(super) fn run_frame_ticks(&mut self) {
        let dt = self.win.advance_tick(100);
        // The shared frame-step rule (`frame_step::SimStepper`): whole 1/60 s
        // ticks, at most four a frame, and a backlog past four dropped rather
        // than carried - the browser page drains through the same kernel.
        let ticks = self.sim_stepper.drain(dt.as_secs_f64());
        // A `--screenshot` capture is tick-locked: one tick per redraw,
        // whatever the wall clock did, so the redraw it samples lands on the
        // tick its headless seed counts to. (The fog pool's render step,
        // which ages the pool the next tick's spawns read and so moves the
        // world `rand()` stream, used to run here per redraw as well; it is a
        // per-tick session step now - `BootSession::step_field_fog`.)
        let ticks = if self.screenshot.is_some() { 1 } else { ticks };
        self.poll_gamepad();
        self.run_sim_ticks(ticks);
    }

    /// The `--simple-dialogue` panel's option picker, on this tick's bound pad
    /// word (`OwnedDialogPanel::step_picker_pad`): a rebound key or a
    /// controller drives it as a default key does. Returns `pad` with the
    /// bits the open picker owns cleared, so the confirm that commits a
    /// choice does not also reach the world's dialog dismiss. The pager
    /// cues its picker (move `0x21`, commit `0x20`).
    ///
    /// A bit the picker owned stays hidden until it is released: the confirm
    /// that commits closes the menu, and the same held button must not
    /// arrive at the world one tick later as a fresh press that dismisses
    /// the reply.
    fn step_simple_dialog_picker(&mut self, pad: u16) -> u16 {
        let pressed = pad & !self.prev_pad;
        self.picker_swallow &= pad;
        if let Some(panel) = self.active_dialog.as_mut() {
            let step = panel.step_picker_pad(pressed);
            if step.moved {
                self.fire_menu_cue(legaia_engine_dialog::dialog::PICKER_MOVE_CUE as u16);
            }
            if step.chosen.is_some() {
                self.fire_menu_cue(legaia_engine_dialog::dialog::PICKER_CONFIRM_CUE as u16);
            }
            self.picker_swallow |= step.consumed & pad;
        }
        pad & !self.picker_swallow
    }

    /// Read the controllers once for this display frame (`window::gamepad`):
    /// adopt the folded word as [`Self::gamepad_held`] and hand every press
    /// since the last poll to the tap latch, the keyboard's sub-frame rule.
    /// A change to the combined word is a replay transition, as a key's is.
    pub(in crate::window) fn poll_gamepad(&mut self) {
        let Some(gp) = self.gamepad.as_mut() else {
            return;
        };
        let poll = gp.poll();
        let before = self.pad | self.gamepad_held;
        self.gamepad_held = poll.edges.held;
        self.pad_taps.press(poll.edges.pressed | poll.taps);
        let after = self.pad | self.gamepad_held;
        if after != before
            && let Some(log) = self.record_log.as_mut()
        {
            log.record_transition(self.session.frames, after);
        }
    }

    /// Run `ticks` sim ticks as one redraw's drain: end a finished windowed
    /// movie, hand a key tapped since the last redraw to the first tick, and
    /// step [`Self::sim_tick`] per tick. The redraw computes `ticks` off the
    /// wall clock; the headless driver (`window::headless`) passes its own.
    pub(in crate::window) fn run_sim_ticks(&mut self, ticks: u32) {
        // In-flow windowed cutscene: when the field VM's FMV-trigger
        // op flips the world into SceneMode::Cutscene and the STR has
        // decoded, suspend world ticks and play the video in-window.
        // Once its frames drain, resume the field (`finish_cutscene`).
        if self
            .cutscene
            .as_ref()
            .is_some_and(|c| c.skipped || c.idx >= c.frames.len())
        {
            // Stop the cutscene audio and give the score back whatever the
            // movie took from it - nothing, when it ducked nothing.
            self.end_movie_audio();
            self.session.host.world.finish_cutscene();
            // Retail does NOT resume the trigger scene after a mid-game FMV -
            // the master dispatch writes a next-scene CDNAME label
            // (`town01` -> fmv 1 -> `town0b`). The shared kernel performs the
            // transfer; without it the window put the player back where the
            // movie started.
            self.apply_fmv_handoff();
            self.cutscene = None;
        }
        let run_ticks = if self.cutscene.is_some() { 0 } else { ticks };
        // A key tapped between two redraws is set and cleared before any
        // tick samples `pad`; the latch hands it to this frame's first tick
        // as one held tick (`PadTapLatch`, the browser page's `pulse`).
        // The keyboard's held word, plus the controller's (`poll_gamepad`).
        let key_pad = self.pad;
        let held_pad = key_pad | self.gamepad_held;
        let first_tick_pad = self.pad_taps.take_frame_word(held_pad);
        // Retail's movie abort on an in-flow movie (`fmv_skip_edge_hit`: the
        // attract id only, on a face button / Select), which the browser
        // page's FMV lane tests on every frame of a field cutscene too. The
        // movie holds the ticks, so the edge is the frame's own; the movie
        // ends on the next redraw.
        if run_ticks == 0
            && let Some(c) = self.cutscene.as_mut()
        {
            let edge = first_tick_pad & !self.prev_pad;
            if let Some(fmv_id) = self.session.host.world.active_fmv()
                && legaia_engine_system::movie::cutscene::fmv_skip_edge_hit(fmv_id, edge)
            {
                c.skipped = true;
            }
            self.prev_pad = held_pad;
        }
        let mut first_tick = true;
        for _ in 0..run_ticks {
            // A phase-gated capture stops ticking the frame its phase is
            // reached, so the frame drawn is that one and not up to three
            // ticks past it.
            if self.capture_phase_met() {
                break;
            }
            self.pad = if std::mem::take(&mut first_tick) {
                first_tick_pad
            } else {
                held_pad
            };
            self.tick_no += 1;
            // A tick the field sat frozen under (a shop, the naming prompt,
            // the pause menu) returns before its tail, so it moves no NPC
            // clip: the players step in the tail's `drain_anim_cues`.
            self.sim_tick();
        }
        // The tap was one tick held; the word the key events maintain is
        // the keyboard's held set again (the controller's word stays in
        // `gamepad_held`). The scripted harnesses own the pad word and press
        // through `handle_key`, so their presses are not taps.
        if run_ticks > 0 && self.screenshot.is_none() {
            self.pad = key_pad;
        }
        if self.screenshot.is_some() {
            self.pad_taps.clear();
        }
    }

    /// One sim tick, after the pad word and `tick_no` are set: the scripted
    /// input, then the arms that own a whole tick (the boot UI, the
    /// name-entry overlay, the prologue hand-off, the pause-menu open), the
    /// field pad, the suspended-menu arm and the field's frame tail. Returns
    /// whether the tick ran the whole tail (`false` when an arm owned it).
    ///
    /// The arms stay inline here, as early returns, because they are this
    /// host's per-tick control flow: `scripts/ci/check-ui-host-drift.py`
    /// reads this body (with the step helpers below spliced in) as the
    /// native frame path and checks each arm's skipped kernels.
    fn sim_tick(&mut self) -> bool {
        crate::window::run::fire_deferred_battle(&mut self.session, self.tick_no);
        self.inject_scripted_tick_input();
        // Party wipe: the world raises `game_over` when a battle resolves to
        // `BattleEndCause::PartyWipe`. The probe starts the return-to-title
        // hold, which owns the frame from here (the arm below skips the
        // scene tick while any boot UI is active).
        self.poll_game_over();
        let pressed_edge = self.pad & !self.prev_pad;
        // The tick's owner, in the order both play hosts share
        // (`BootSession::claim_tick`): the window's boot UI (title, save
        // select, options, the spawned pause menu, the party-wipe hold); the
        // naming prompt (stepped on this tick's edge by the session); the
        // prologue intro skip (retail `FUN_801D1344`; the session has taken
        // the hand-off); the pause menu's spawn hand-over and its press
        // (`press_field_menu`: a script's op-`0x49` save point, a Start edge
        // where `World::field_menu_open_allowed` says so, the deny buzz; a
        // shop / prize overlay owns the pad, so no press reaches the menu
        // while one is up); a menu-overlay screen that suspends the field;
        // the field. Each arm adds only what the window holds: its screens,
        // the scene re-entry and its render rebuild, the open blip, and the
        // party readout's decision kernel, whose suppression predicate names
        // each of these states and so must be stepped on a tick that skips
        // the tail.
        let claims = legaia_engine_session::TickClaims {
            host_screen: self.boot_ui.is_active(),
            overlay_open: self.menu_runtime.is_open(),
            overlay_suspends: self.menu_runtime.suspends_field(),
            pause_menu_ready: true,
        };
        let owner = self.session.claim_tick(claims, pressed_edge);
        match owner {
            TickOwner::HostScreen => {
                let _ = self.tick_boot_ui();
                // The scene tick (and its SFX drain) is skipped, so the menu
                // cues queued this frame fire here.
                self.tick_menu_sfx();
                // The field party HUD's decision kernel is stepped in the
                // scene tick too, and its suppression predicate names this
                // very state - so step it here as well, or the kernel keeps
                // its last pre-menu `Draw` and the readout stays painted
                // under the pause menu.
                self.tick_field_party_hud();
                self.prev_pad = self.pad;
                return false;
            }
            TickOwner::NameEntry(_) => {
                self.tick_field_party_hud();
                self.prev_pad = self.pad;
                return false;
            }
            TickOwner::PrologueSkip(target) => {
                match self.session.enter_field_live(target, &self.field_live_opts) {
                    Ok(mode) => {
                        log::info!("prologue handoff: entered '{target}' (mode={mode:?})");
                        // `enter_field_scene` installs `town01`'s opening
                        // timeline (the name-entry overlay opens at its pinned
                        // op-`0x49`); the scene swapped under the window, so
                        // rebuild the render-side scene state.
                        self.rebuild_scene_render_state();
                    }
                    Err(e) => {
                        log::warn!("prologue handoff: enter '{target}' failed ({e:#})")
                    }
                }
                self.tick_field_party_hud();
                self.prev_pad = self.pad;
                return false;
            }
            TickOwner::PauseMenuSpawned => {
                // The wipe has spawned the menu: the boot-UI arm takes the
                // frame from the next tick. Retail's mode-`0x17` handler runs
                // the cue drainer under the menu, so the SFX scheduler steps.
                self.boot_ui = BootUiState::FieldMenu { sub: None };
                self.tick_menu_sfx();
                self.tick_field_party_hud();
                self.prev_pad = self.pad;
                return false;
            }
            TickOwner::PauseMenuOpened(scripted) => {
                // The open blips as a confirm, as on the browser page; a
                // scripted press blips nothing - retail's cue `0x20` belongs
                // to the pad controller, not to the actor it spawns.
                if !scripted {
                    self.fire_menu_cue(
                        legaia_engine_session::audio_director::RETAIL_MENU_CONFIRM_CUE,
                    );
                }
                self.tick_menu_sfx();
                self.tick_field_party_hud();
                self.prev_pad = self.pad;
                return false;
            }
            TickOwner::OverlayScreen | TickOwner::Field(_) => {}
        }
        // The field's pad: neutral while the narration crawl / title card
        // owns it or an overlay screen is up (`TickOwner::Field(true)`; the
        // prologue's 3D keeps playing under the crawl in retail, so the tick
        // and its whole tail still run). Under a suspending screen the field
        // is frozen below.
        let neutral = !matches!(owner, TickOwner::Field(false));
        let field_pad = if neutral { 0 } else { self.pad };
        let field_pad = self.step_simple_dialog_picker(field_pad);
        let field_suspended = owner == TickOwner::OverlayScreen;
        let field_pad = self.resolve_tick_pad(field_pad);
        self.session.host.world.set_pad(field_pad);
        if field_suspended {
            // A shop / prize exchange: the field is frozen, tail included -
            // the world tick, the effect scene-graphs, the ocean and CLUT
            // cyclers, the event drains, the NPC clips and the party
            // readout's kernel. Retail runs the counter at game mode 0x17
            // with the field overlay swapped out for the menu overlay, so
            // none of that code is resident; the browser page skips its
            // whole `tick_frame` under a shop for the same reason. What
            // still runs is what the page runs: the menu session on this
            // tick's edges, the unpark on close, and (like the pause-menu
            // arm above) the SFX scheduler step.
            if let Some(cue) = tick_menu_runtime_session(
                &mut self.menu_runtime,
                &mut self.session.host.world,
                pressed_edge,
            ) {
                self.fire_menu_cue(u16::from(cue));
            }
            self.tick_menu_sfx();
            self.prev_pad = self.pad;
            if let Some(log) = self.record_log.as_mut() {
                log.observe_frame(self.session.frames);
            }
            return false;
        }
        self.tick_field_tail(pressed_edge);
        true
    }

    /// The capture harnesses' per-tick input: `--key-script` keys through the
    /// real keyboard arms, the scripted talk / shop arms, and (under a
    /// `--screenshot` run) the scripted pad word that overrides the keyboard.
    fn inject_scripted_tick_input(&mut self) {
        // Scripted keyboard harness (`--key-script`): deliver this tick's
        // keys through the real keyboard arms, press then release, before
        // the pad injection below. Order matters both ways round: the key
        // arms run first so a minigame entry is open for the rest of the
        // tick, and the pad write lands after so a key that also binds to
        // a pad button cannot leave a bit latched into `set_pad`.
        let scripted_keys = self
            .screenshot
            .as_ref()
            .map(|sc| {
                sc.key_script
                    .get(&self.tick_no)
                    .cloned()
                    .unwrap_or_default()
            })
            .unwrap_or_default();
        // A scripted key that also *binds* to a pad button has to survive
        // the neutral-pad write below, or `--key-script` can only ever
        // arm window toggles: `handle_key` sets the bit and the release
        // on the very next line clears it again, and the write then
        // stamps the whole word to zero. Collect those bits and re-apply
        // them as this tick's pad word - one tick held, one tick clear,
        // which is exactly the edge `--pad-script` delivers.
        let mut scripted_key_pad = 0u16;
        for code in scripted_keys {
            if let Some(button) = self.mapping.pad_button_for_key(keycode_to_name(code)) {
                scripted_key_pad |= button.mask();
            }
            self.handle_key(code, ElementState::Pressed);
            self.handle_key(code, ElementState::Released);
        }
        // Screenshot harness: inject the scripted one-tick pad edge for
        // this tick (overriding keyboard). Ticks with no script entry get
        // a neutral pad so the previous press releases (edge resets).
        if let Some((t, slot)) = self.screenshot.as_ref().and_then(|sc| sc.talk_at)
            && t == self.tick_no
        {
            self.session.host.world.trigger_field_interact(0xFF, slot);
        }
        if let Some((t, idx)) = self.screenshot.as_ref().and_then(|sc| sc.shop_at)
            && t == self.tick_no
        {
            let _ = self.session.host.world.debug_arm_scene_shop(idx);
        }
        if self.screenshot.is_some() {
            let scripted_pad = self
                .screenshot
                .as_ref()
                .and_then(|sc| sc.pad_script.get(&self.tick_no).copied())
                .unwrap_or(0);
            // A script-gated capture resumes the retail record when the
            // entry did not start it, and pages its dialog boxes - the
            // same drive the headless seed runs.
            if self.tick_no == legaia_engine_capture::script_gate::SCRIPT_RESUME_TICK
                && let Some(sc) = self.screenshot.as_ref()
                && !sc
                    .script_gate
                    .as_ref()
                    .is_some_and(|g| g.resumed_early.get())
            {
                for &idx in &sc.seat_latches {
                    self.session.host.world.system_flag_set(idx);
                }
            }
            let gate = self
                .screenshot
                .as_ref()
                .and_then(|sc| sc.script_gate.clone());
            let gate_pad = match &gate {
                Some(g) => {
                    g.drive_resume(&mut self.session.host, self.tick_no);
                    // `g` is this tick's copy: carry the early-resume mark
                    // back to the capture's own gate.
                    if g.resumed_early.get()
                        && let Some(own) = self
                            .screenshot
                            .as_ref()
                            .and_then(|sc| sc.script_gate.as_ref())
                    {
                        own.resumed_early.set(true);
                    }
                    g.advance_pad(&self.session.host.world, self.tick_no)
                }
                None => 0,
            };
            self.pad = scripted_pad | scripted_key_pad | gate_pad;
        }
    }

    /// The field pad word this tick hands the world: the options' simulation
    /// knobs re-asserted, a capture's battle bars seeded, and a
    /// `LEGAIA_BATTLE_DRIVE` capture's own pad path over the player's.
    fn resolve_tick_pad(&mut self, field_pad: u16) -> u16 {
        // Re-assert the options' simulation knobs each tick (precise
        // movement, the Field Move default, the reduce-flashing guard,
        // battle Select Attack): scene / New Game transitions can reseed
        // world state, and the knobs are host policy (options file + `R`
        // key), not world state. The browser page runs the same session
        // step on each of its ticks. A save load's option words land first
        // (retail restores them with the card), so this push carries them.
        if self
            .session
            .adopt_loaded_options(&mut self.options_state, &mut self.options_pins)
        {
            self.persist_and_apply_options();
        }
        self.session.apply_option_knobs(&self.options_state, None);
        // `set_pad` also latches the run button off the same word, so
        // there is nothing host-side to keep in sync.
        // A `LEGAIA_BATTLE_DRIVE` capture walks the fight's pad path
        // itself, arming the drive's world seed on the first battle tick.
        // The capture's mid-fight bars, on the first battle tick - the
        // point the headless seed puts them on.
        if let Some(sc) = self.screenshot.as_ref()
            && self.session.host.world.mode == SceneMode::Battle
            && !sc.battle_bars.is_empty()
            && !sc.battle_bars_seeded.replace(true)
        {
            legaia_engine_capture::bar_seeds::apply_bar_seeds(
                &mut self.session.host.world,
                &sc.battle_bars,
            );
        }
        if let Some(sc) = self.screenshot.as_ref()
            && self.session.host.world.mode == SceneMode::Battle
            && sc.mystic_shield.replace(false)
        {
            self.session.host.world.raise_mystic_shield();
        }
        // A replayed cast's gate holds the summon head's CD poll for a pass
        // (`PhaseGate::steer`), as the headless seed does.
        if let Some(gate) = self.screenshot.as_ref().and_then(|sc| sc.phase_gate) {
            gate.steer(&mut self.session.host.world);
        }
        match self.screenshot.as_ref() {
            Some(sc) if let Some(drive) = sc.battle_drive => {
                let world = &mut self.session.host.world;
                // Retail's battle tick waits out the camera's entry
                // sweep; a drive into a running fight waits with it, as
                // the headless seed does.
                let sweeping = !matches!(
                    drive,
                    legaia_engine_capture::battle_drive::BattleDrive::Opening { .. }
                ) && world.mode == SceneMode::Battle
                    && !legaia_engine_capture::battle_drive::entry_sweep_reached(world, 0xFF);
                if sweeping {
                    0
                } else {
                    if world.mode == SceneMode::Battle && !sc.battle_drive_primed.replace(true) {
                        drive.prime(world);
                        legaia_engine_capture::melee_hits::seed_melee_hits(
                            world,
                            &sc.battle_melee_hits,
                        );
                    }
                    drive.steer(world);
                    // A reached phase is held with no input until it is
                    // sampled (`BattleDrive::hold_ticks`).
                    if drive.reached(world) {
                        0
                    } else {
                        drive.pad_word_at(world, self.tick_no)
                    }
                }
            }
            _ => field_pad,
        }
    }

    /// The tick's world step and the field's frame tail: the session tick
    /// and its scene / battle edges, the minigame side channels, the field
    /// overlay poll, the effect step, the frame-tail effect passes, the menu
    /// session step and the event drains.
    fn tick_field_tail(&mut self, pressed_edge: u16) {
        self.tick_session_step();
        self.tick_side_channels();
        // A shop or prize counter the field VM opened this tick takes its
        // first step inside the poll, on no edge: the press this tick
        // carries already went to the field - typically the Cross that
        // closed the merchant's line and ran the script into op 0x49 - and
        // handing it to the screen too committed the picker's first row on
        // the same press. Retail's screen comes up on a later frame (the
        // menu overlay swaps in).
        let shop_opened_this_tick = self.poll_field_shop();
        self.tick_world_effects();
        self.tick_frame_tail_effects();
        // An open session - an inn prompt, which does not suspend the field -
        // steps here every tick on the tick's own edge, and a closed one
        // unparks its script. Not on the opening tick: the poll stepped it.
        if !shop_opened_this_tick
            && let Some(cue) = tick_menu_runtime_session(
                &mut self.menu_runtime,
                &mut self.session.host.world,
                pressed_edge,
            )
        {
            self.fire_menu_cue(u16::from(cue));
        }
        self.prev_pad = self.pad;
        // Record-mode: advance the log's frame counter so
        // `meta.frames` reflects the recorded duration even
        // when the user closes mid-run with no pad transitions.
        if let Some(log) = self.record_log.as_mut() {
            log.observe_frame(self.session.frames);
        }
        self.tick_event_drains();
    }

    /// The session tick, a scene transition's render rebuild, the battle
    /// drive's hold counter, and the per-tick battle-render / NPC-model edges.
    fn tick_session_step(&mut self) {
        match self.session.tick() {
            // Door transition: the host loaded a new scene under
            // the window (field-VM op 0x3E/0x3F or a walk-touch
            // door). Rebuild the render-side scene state so the
            // new scene's geometry/VRAM replace the old one's -
            // without this the world model swaps under the OLD
            // scene's meshes.
            Ok(legaia_engine_scene_host::scene_host::SceneTickEvent::SceneEntered { name }) => {
                log::info!("play-window: scene transition -> '{name}'");
                self.rebuild_scene_render_state();
            }
            Ok(_) => {}
            Err(e) => log::error!("session tick: {e:#}"),
        }
        if let Some(sc) = self.screenshot.as_ref()
            && let Some(drive) = sc.battle_drive
        {
            let held = if drive.reached(&self.session.host.world) {
                sc.battle_drive_held.get() + 1
            } else {
                0
            };
            sc.battle_drive_held.set(held);
            if sc.display_lag > 0 {
                let world = &self.session.host.world;
                sc.state_since.set(
                    drive
                        .in_action_state(world)
                        .then(|| sc.state_since.get().unwrap_or(self.tick_no)),
                );
            }
        }
        // A script gate on the player's clip latch takes the displayed frame
        // `display_lag` ticks before the gate: any tick since the engine
        // started replaying the gate's record is a frame retail drew too.
        if let Some(sc) = self.screenshot.as_ref()
            && sc.display_lag > 0
            && let Some(g) = sc.script_gate.as_ref()
            && g.latch.is_some()
            && sc.state_since.get().is_none()
            && g.context(&self.session.host.world).is_some()
        {
            sc.state_since.set(Some(self.tick_no));
        }
        // The Field <-> Battle mode edge, latched on the tick that
        // crossed it. The battle load it runs installs gameplay state as
        // well as meshes - the party's idle / action clips, art banks and
        // art records - so it cannot wait for the display frame: this call
        // used to sit only after the tick loop, and the first one to three
        // battle ticks of a catch-up frame ran without them. The browser
        // page latches the edge per sim tick too (`tick_battle_presentation`).
        // Edge-latched, so the call after the loop is a no-op when this one
        // already fired.
        self.sync_battle_render();
        // A scripted mesh re-bind this tick (motion-VM op `0x0E`) needs
        // the swapped mesh uploaded; the world holds the new id and the
        // draw holds the old one.
        self.rebind_live_npc_models();
        // (Placed-prop animation and the touch/interact dispatch are
        // the world's own - `World::tick_prop_interactions`, inside
        // `World::tick`'s field arm - so this loop has no step for them.
        // It used to call an empty `tick_field_prop_anims` shim, which
        // the drift gate then PAIRED with the browser's real NPC-clip
        // kernel: a `{}` body pairs perfectly by name. The browser twin's
        // two drains are done below, inline in this loop.)
    }

    /// Minigame side channels, the play clock and the dev menu.
    fn tick_side_channels(&mut self) {
        // Minigame side-channels: the minigame and Baka duel cue queues, the
        // dance auto-end + venue, the fishing venue actors (wander / line / floor solve /
        // camera publish / sway), the Baka round chrome, and the shared
        // effect pool. Both it and tick_fishing_banners read the fishing
        // events this world tick raised.
        self.tick_minigame_extras();
        // Fishing: advance the HUD's one-shot banner animations (hook /
        // reel-in / miss / auxiliary / strike splash) and cache their
        // draws - the retail driver tail's own per-frame timer loop.
        self.tick_fishing_banners();
        // Muscle Dome: run the round time meter (climbs through the
        // selection phase, drains outside it).
        self.tick_muscle_time_meter();
        // Opt-in synthetic tile board (`LEGAIA_TILE_BOARD_DEMO=1`): no
        // retail scene script installs one, so this is the visual
        // trigger for the per-cell tile-actor draw pass.
        self.maybe_install_demo_tile_board();
        // Advance the world's play clock off the window's wall clock.
        // `World::advance_play_time` is written to be driven "from the
        // frame loop's wall-clock delta" and no host was driving it, so
        // every consumer of `play_time_seconds` - the save screen's
        // play-time column, the seru-trade gate, the dev Records page -
        // read a value that only ever changed when a save was loaded.
        // Whole seconds only, and by delta rather than absolutely, so a
        // loaded save keeps its accumulated total.
        self.tick_play_clock();
        // Opt-in developer menu (`LEGAIA_DEV_MENU=1`): retail reaches its
        // dev tools from debug branches a player cannot; this is the
        // engine's equivalent entry point.
        self.tick_dev_menu();
    }

    /// The frame-tail effect passes after the scene-graph tick
    /// (`tick_world_effects`): the ocean shimmer, the scripted CLUT-cell
    /// effects and the battle VRAM residency check. The battle face / CLUT /
    /// stage-shell steps run at the end of the battle presentation step
    /// (`drain_and_log_battle_events`), after the HUD fold that arms the
    /// status CLUT latch, as on the page.
    fn tick_frame_tail_effects(&mut self) {
        // World-map ocean shimmer: cycle the 13-frame CLUT animation
        // (self-gates to None off the world map).
        self.advance_ocean_animation();
        // Scripted CLUT-cell effects (field-VM 4C 61 one-shots +
        // cross-fades): drain the world's banked game ticks against the
        // CPU VRAM (self-gates when none are live).
        self.apply_world_clut_fx();
        // The fog pool's render step, once per sim tick through the session
        // step the browser page runs too (`BootSession::step_field_fog`); the
        // draw prep only wraps the quads it left (`take_field_fog_prims`).
        let centre = legaia_engine_world_views::screen_passes::scene_centre(self.scene_aabb);
        let _ = self.session.step_field_fog(centre);
        // Catch any path that re-uploaded VRAM over the battle
        // texture this frame (and restore it).
        self.check_battle_vram_residency();
    }

    /// The tick's event drains: battle events, field events, the dialog
    /// panel, the text balloon, the clip cues and the party HUD countdown.
    fn tick_event_drains(&mut self) {
        // Drain whatever battle events the SM fired this tick,
        // fold their gameplay-state side into the world (HP /
        // status), and ring them into the HUD log.
        self.drain_and_log_battle_events();
        // The audio frame, once every cue of the tick is queued: the duck
        // ramp, the delay scheduler, the matured cues.
        self.tick_sfx();
        // Route field events: ActorSpawned events whose actor
        // carries a `tmd_ref` queue a render-pass mesh upload
        // so spawn-record actors appear in the scene.
        self.drain_and_route_field_events();
        // Mirror the world's dialog request into a rendered,
        // typed-out panel (opened from the scene MES, dropped when
        // the world dismisses the box).
        self.sync_dialog_panel();
        // Commit the `4C E1` balloon's font measurement while `self`
        // is still mutable; the `&self` draw passes read the committed
        // pen/rect off the record.
        self.sync_text_balloon();
        // clip cues (`A2` / `4C 51` for NPCs, `A2 F8` ExecMove for the
        // player), drained every tick in every mode through the shared
        // kernel. This used to run inside the draw pass and only in
        // `SceneMode::Field`, so a cue raised anywhere else waited in the
        // queue for the next field frame.
        self.drain_anim_cues();
        // Advance the field party-status HUD's idle countdown
        // (`FUN_801D0D38`). Its decision is read back in the draw pass.
        self.tick_field_party_hud();
    }
}
