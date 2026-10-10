//! The redraw's simulation half: the sim ticks one redraw drains and the
//! per-tick steps (scripted input, the modal arms that own a tick, the field
//! pad, the world tick and its frame tail), split out of `handle_redraw`.

use super::super::*;
use super::redraw::tick_menu_runtime_session;

impl PlayWindowApp {
    /// Drain this redraw's sim ticks (the shared `frame_step::SimStepper`
    /// rule, or one tick under a `--screenshot` capture), ending a finished
    /// windowed movie first. Returns the ticks that ran the field's whole
    /// tail - the NPC clip playheads in the draw pass advance by this.
    pub(super) fn run_frame_ticks(&mut self) -> u32 {
        let dt = self.win.advance_tick(100);
        // The shared frame-step rule (`frame_step::SimStepper`): whole 1/60 s
        // ticks, at most four a frame, and a backlog past four dropped rather
        // than carried - the browser page drains through the same kernel.
        let ticks = self.sim_stepper.drain(dt.as_secs_f64());
        // A `--screenshot` capture is tick-locked: one tick per redraw,
        // whatever the wall clock did. The draw pass is not inert - the fog
        // pool's render step ages the pool the next tick's spawns read, and
        // those spawns draw the world `rand()` stream - so a wall-paced
        // capture ran a fogged scene on a stream that moved with machine
        // load, and the retail comparison's frame landed on a different
        // fight from its headless seed (`BootSession::fog_render_tick`).
        let ticks = if self.screenshot.is_some() { 1 } else { ticks };
        // In-flow windowed cutscene: when the field VM's FMV-trigger
        // op flips the world into SceneMode::Cutscene and the STR has
        // decoded, suspend world ticks and play the video in-window.
        // Once its frames drain, resume the field (`finish_cutscene`).
        if self
            .cutscene
            .as_ref()
            .is_some_and(|c| c.idx >= c.frames.len())
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
        let held_pad = self.pad;
        let first_tick_pad = self.pad_taps.take_frame_word(held_pad);
        let mut first_tick = true;
        // Ticks this frame that ran the field's whole tail. The NPC clip
        // playheads in the draw pass advance by this, not by `run_ticks`, so
        // a tick the field sat frozen under (a shop, the naming prompt, the
        // pause menu) moves no clip - the browser page, which skips its whole
        // `tick_frame` on those frames, freezes them the same way.
        let mut field_tail_ticks = 0;
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
            if self.sim_tick() {
                field_tail_ticks += 1;
            }
        }
        // The tap was one tick held; the word the key events maintain is
        // the held set again. The scripted harnesses own the pad word and
        // press through `handle_key`, so their presses are not taps.
        if run_ticks > 0 && self.screenshot.is_none() {
            self.pad = held_pad;
        }
        if self.screenshot.is_some() {
            self.pad_taps.clear();
        }
        field_tail_ticks
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
        self.inject_scripted_tick_input();
        // Party wipe: the world raises `game_over` when a battle
        // resolves to `BattleEndCause::PartyWipe`. Consume the flag and
        // start the return-to-title hand-off, which owns the frame from
        // here (the arm below skips the scene tick while any boot UI is
        // active). Retail's wipe arm stores mode 22 CARD INIT with the
        // title context word set, so the destination is the title screen
        // and nothing is asked of the player on the way.
        if self.session.host.world.game_over && !self.boot_ui.is_active() {
            self.session.host.world.game_over = false;
            self.boot_ui =
                BootUiState::GameOver(legaia_engine_core::game_over::GameOverSession::new());
            log::info!("play-window: party wipe -> title screen");
        }
        // When the boot UI is active, route input there and skip
        // the scene tick - the player hasn't entered the world
        // yet (or has paused into save-select).
        if self.boot_ui.is_active() {
            let _ = self.tick_boot_ui();
            // The scene tick (and its SFX drain) is skipped below, so
            // the menu cues queued this frame fire here.
            self.tick_menu_sfx();
            // The field party HUD's decision kernel is stepped in the
            // scene tick too, and its suppression predicate names this
            // very state - so step it here as well, or the kernel keeps
            // its last pre-menu `Draw` and the readout stays painted
            // under the pause menu (the browser page has no early-out
            // and never showed it).
            self.tick_field_party_hud();
            self.prev_pad = self.pad;
            return false;
        }
        // Start in field opens the pause menu. Edge-detect so a
        // held key doesn't auto-reopen.
        let pressed_edge = self.pad & !self.prev_pad;
        // Name-entry overlay is modal: while it's open the field is
        // frozen and every pad edge routes into the entry SM (one
        // cell / glyph per press). Mirrors the opening `town01`
        // naming prompt, which suspends the field VM.
        // The routing is the engine's (`World::step_name_entry_frame`, the
        // kernel `BootSession::tick` runs for every other driver): the
        // edge drives the entry SM and the frame counter advances so the
        // caret blinks. This arm adds only the window's frame-tail skip.
        if self.session.step_name_entry_frame(pressed_edge) {
            // Same reason as the boot-UI arm above: the party readout's
            // decision kernel is stepped in the fall-through path and its
            // suppression predicate names this state, so an arm that
            // skips the step paints the readout under the overlay from
            // the kernel's last pre-overlay answer.
            self.tick_field_party_hud();
            self.prev_pad = self.pad;
            return false;
        }
        // Prologue intro-skip (retail FUN_801D1344): while the opening
        // chain plays with the trigger bit armed, a confirm press
        // (Cross) skips the WHOLE remaining opening to `town01` -
        // available mid-narration too (the crawl is timer-driven; retail
        // has no per-line skip).
        if let Some(target) = self
            .session
            .host
            .world
            .take_prologue_handoff(pressed_edge & 0x4000 != 0)
        {
            match self.session.enter_field_live(target, &self.field_live_opts) {
                Ok(mode) => {
                    log::info!("prologue handoff: entered '{target}' (mode={mode:?})");
                    // `enter_field_scene` installs `town01`'s opening
                    // cutscene timeline (gated on the prologue hand-off):
                    // the establishing camera + Vahn's scripted walk-out
                    // play, and the name-entry overlay opens when the
                    // timeline reaches its pinned op-`0x49` STATE_RESUME
                    // (P2[3] body `0x02c6`) - the faithful in-script
                    // trigger, not a blind host call at the hand-off.
                    //
                    // The host swapped scenes (opdeene -> town01):
                    // rebuild the render-side scene state so Rim Elm's
                    // geometry replaces the prologue's.
                    self.rebuild_scene_render_state();
                }
                Err(e) => {
                    log::warn!("prologue handoff: enter '{target}' failed ({e:#})")
                }
            }
            // The hand-off swapped the scene under the window, which is
            // the kernel's own rearm condition - step it here so the
            // readout rearms on this frame rather than one frame late.
            self.tick_field_party_hud();
            self.prev_pad = self.pad;
            return false;
        }
        // While the opening narration crawl / title card is on screen the
        // pad is frozen (the timeline owns the scene) and Start opens
        // nothing, but the frame is otherwise an ordinary one: the scene
        // ticks and the whole tail below runs. This arm used to `continue`
        // straight after the scene tick, so under the crawl the effect
        // scene-graphs, the scripted CLUT / VRAM effects, the field-event
        // drain, the NPC rebind, the balloon sync and the play clock all
        // stood still while the browser page ran them - and the prologue's
        // 3D keeps playing under the crawl in retail.
        let narration = self.session.host.world.cutscene_narration_active()
            || self.session.host.world.cutscene.card.is_some();
        // Start opens the pause menu wherever retail's locomotion
        // controller runs, which is the field **and the overworld**.
        // The guard used to be `!menu_runtime.is_open()` alone, so Start
        // mid-battle opened the menu and froze the fight - the boot-UI
        // arm above skips the scene tick, so nothing advanced until the
        // player backed out.
        //
        // It then spelled the mode test out locally as
        // `mode == SceneMode::Field`, on the premise that "on the world
        // map the controller has its own" Start handler. That premise is
        // false: `FUN_801E76D4` is the top-view debug renderer, and the
        // overworld runs the ordinary `FUN_801D01B0` chain. A local copy
        // of the test is exactly how the overworld lost the pause menu,
        // so this asks the engine instead
        // ([`World::field_menu_open_allowed`]) and every host that opens
        // the menu asks the same question.
        // The press itself - a script's op-`0x49` save point / ready
        // check, a Start edge, the deny buzz - is answered by the one
        // engine rule every host asks (`BootSession::press_field_menu`),
        // which already holds the narration / title-card refusal this
        // arm used to spell out locally. A shop / prize overlay owns the
        // pad, so no press reaches the menu while one is up.
        //
        // The open can be REFUSED - a dialogue engagement owns the player,
        // as retail's engaged-bit branch does - and then the boot-UI arm
        // is not taken, or the window would route input and draws to a
        // menu that is not there while the scene tick stayed skipped.
        // The menu spawns partway through the field's wipe to black
        // (`BootSession::pause_wipe`): until then the field keeps drawing
        // under the wipe and ticking (with the world in `Menu`, so the
        // player stands still), and the boot-UI arm takes the frame from
        // the tick the menu exists.
        if !self.boot_ui.is_active()
            && self.session.field_menu_is_open()
            && self.session.pause_wipe().menu_spawned()
        {
            self.boot_ui = BootUiState::FieldMenu { sub: None };
            self.tick_field_party_hud();
            self.prev_pad = self.pad;
            return false;
        }
        let press = if self.menu_runtime.is_open() || self.session.field_menu_is_open() {
            legaia_engine_session::PauseMenuPress::None
        } else {
            self.session.press_field_menu(pressed_edge & 0x0008 != 0)
        };
        if let legaia_engine_session::PauseMenuPress::Opened { scripted } = press {
            // Start: the BootSession-hosted pause menu (the retail CARD
            // pair, game_mode 0x17 - the world holds SceneMode::Menu
            // while it is open), with the window's input + draws routed
            // to it via the boot-UI arm. The open blips as a confirm, as
            // on the browser page; a scripted press blips nothing -
            // retail's cue `0x20` belongs to the pad controller, not to
            // the actor it spawns.
            if !scripted {
                self.fire_menu_cue(crate::bgm::RETAIL_MENU_CONFIRM_CUE);
            }
            self.tick_menu_sfx();
            // The boot-UI arm waits for the wipe to spawn the menu
            // (above); this frame the field holds still under it.
            self.tick_field_party_hud();
            self.prev_pad = self.pad;
            return false;
        }
        // Route this frame's pad into the engine before the
        // tick so World::tick's mode dispatch (world-map
        // controller, field-VM dialog-advance poll) sees real
        // input. Edge detection lives in World.input. While a
        // menu-runtime overlay (shop / inn) is up the pad drives
        // the menu, not the field, so feed the field a neutral pad
        // (the player must not walk while shopping).
        let field_pad = if narration || self.menu_runtime.is_open() {
            0
        } else {
            self.pad
        };
        // A shop / prize exchange is a menu-overlay session in retail
        // (the field overlay is swapped out under it), so the world does
        // not tick at all while one is up - the browser page's freeze.
        let field_suspended = self.menu_runtime.suspends_field();
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
            if self.tick_no == legaia_parity::retail_compare_script::SCRIPT_RESUME_TICK
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
        // key), not world state. The browser page pushes the same set
        // through the same call.
        self.options_state
            .apply_to_world(&mut self.session.host.world);
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
            legaia_parity::retail_compare_battle::apply_bar_seeds(
                &mut self.session.host.world,
                &sc.battle_bars,
            );
        }
        match self.screenshot.as_ref() {
            Some(sc) if let Some(drive) = sc.battle_drive => {
                let world = &mut self.session.host.world;
                // Retail's battle tick waits out the camera's entry
                // sweep; a drive into a running fight waits with it, as
                // the headless seed does.
                let sweeping = !matches!(
                    drive,
                    legaia_parity::retail_compare_battle::BattleDrive::Opening { .. }
                ) && world.mode == SceneMode::Battle
                    && !legaia_parity::retail_compare_battle::entry_sweep_reached(world, 0xFF);
                if sweeping {
                    0
                } else {
                    if world.mode == SceneMode::Battle && !sc.battle_drive_primed.replace(true) {
                        drive.prime(world);
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
    /// and its scene / battle edges, the minigame side channels, the menu
    /// overlay and battle FX requests, the frame-tail effect passes, the menu
    /// session step and the event drains.
    fn tick_field_tail(&mut self, pressed_edge: u16) {
        self.tick_session_step();
        self.tick_side_channels();
        let shop_opened_this_tick = self.take_menu_overlay_requests();
        self.take_battle_fx_requests();
        self.tick_frame_tail_effects();
        // A shop or prize counter opened on this tick (the take above)
        // takes its first step now, on no edge: the press this tick
        // carries already went to the field - typically the Cross that
        // closed the merchant's line and ran the script into op 0x49 -
        // and handing it to the screen too committed the picker's first
        // row on the same press. Retail's screen comes up on a later
        // frame (the menu overlay swaps in), and the browser page hands a
        // shop only the edges of the frames after it opened. An inn
        // session runs here every tick on the tick's own edge.
        let menu_edge = legaia_engine_core::menu_runtime::MenuRuntime::session_edge(
            shop_opened_this_tick,
            pressed_edge,
        );
        if let Some(cue) = tick_menu_runtime_session(
            &mut self.menu_runtime,
            &mut self.session.host.world,
            menu_edge,
        ) {
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
            Ok(legaia_engine_core::scene::SceneTickEvent::SceneEntered { name }) => {
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

    /// Minigame side channels, the play clock, the dev menu and the dance
    /// auto-end.
    fn tick_side_channels(&mut self) {
        // Baka Fighter duel: drain the exchange-hit SFX cue the rules
        // kernel queued this tick and enqueue it into the SFX scheduler
        // (the per-frame `tick_sfx_frame` below fires it against the
        // resident class-2 sound bank).
        self.drain_baka_sfx_cues();
        // Minigame side-channels: the dance count-in + tutorial + effect
        // spawns, the fishing venue actors (wander / line / floor solve /
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
        // Dance minigame auto-end: `tick_dance` restores the scene
        // mode when the song timer runs out but leaves the game
        // installed for one frame. Detect that (mode no longer
        // Dance while a game is still present), log the final grade,
        // and clear it. `exit_dance` gives the hall its own track back
        // itself (`restore_minigame_bgm` queues the start this host's
        // BGM routing plays), as on the browser page; a second start
        // here restarted the field track on top of it.
        if let Some(g) = self.session.host.world.finish_dance_if_over() {
            log::info!(
                "dance: song finished - score {} (pass={})",
                g.score(),
                g.passed()
            );
        }
    }

    /// A field-VM shop or casino prize counter opened this tick: hand the
    /// player into it. Returns whether one opened.
    fn take_menu_overlay_requests(&mut self) -> bool {
        // A field-VM shop (op `0x49` sub-0) or casino prize counter
        // (sub-7) opened this tick: hand the player into it through the
        // shared drain (`MenuRuntime::open_field_overlay_requests`, the
        // browser page's too). The field VM stays suspended (op-0x49
        // Armed) until the player leaves.
        let opened = self
            .menu_runtime
            .open_field_overlay_requests(&mut self.session.host.world);
        // The Trade row's names come from the boot SCUS - a host read.
        if opened && self.session.host.world.seru_trade_enabled() {
            self.ensure_seru_names();
        }
        opened
    }

    /// The battle's summon-creature and move-FX spawn requests (with the
    /// move's sound cue).
    fn take_battle_fx_requests(&mut self) {
        // Production cast-band trigger: a player summon cast
        // (spell id 0x81..=0xA0) requests a summon spawn. The
        // faithful render is the cast's own body drawn through the
        // enemy animation pipeline (the namesake battle_data creature
        // for 0x81..=0x95, the cast's summon.dat record above that),
        // so spawn it as a battle creature rather than the move-VM
        // scene-graph stand-in (`summon::summon_spawn_asset`).
        if let Some((spell_id, _origin)) = self.session.host.world.take_pending_summon_spawn() {
            self.spawn_summon_creature(spell_id);
        }
        // Production move-FX trigger, spawn to sound: the shared
        // `battle_fx::spawn_pending_move_fx` (the browser page's step
        // calls it too) seats the requested scene graph and resolves the
        // move's cue through the retail dispatch decode; a ring value
        // rides the same per-frame SFX scheduler the art-strike cues do.
        if let Some(ring_value) =
            legaia_engine_session::battle_fx::spawn_pending_move_fx(&mut self.session.host.world)
        {
            if let Some(bgm) = self.session.bgm.as_mut() {
                bgm.enqueue_sfx(ring_value, 0, 0, 0);
            }
            log::debug!("battle move-FX cue -> SFX {ring_value:#06x}");
        }
    }

    /// The frame-tail effect passes: the move-VM scene-graphs, the battle
    /// face / CLUT / stage-shell steps, the ocean shimmer, the scripted
    /// CLUT-cell effects and the battle VRAM residency check.
    fn tick_frame_tail_effects(&mut self) {
        // Advance the three move-VM effect scene-graphs - an active
        // Seru-magic summon (the cast above, or the `G` debug spawn), a
        // battle move-FX (`H`), and the field op-`0x34` sub-3 prescript
        // stagers - through the shared frame-tail kernel the browser page
        // calls too. Each self-gates when nothing is live.
        self.session.host.world.tick_effect_scene_graphs();
        // In battle, re-stamp the party's eye/mouth face frames
        // from the playing clips' facial tracks (the retail
        // per-frame facial animator). The clips themselves are
        // advanced by `World::tick`'s Battle arm, which every host
        // reaches - ticking them again here would run them at 2x.
        if self.session.host.world.mode == SceneMode::Battle {
            self.tick_battle_face_stamps();
            self.tick_battle_status_clut();
            self.tick_battle_effect_clut();
            self.tick_battle_stage_shell();
        }
        // World-map ocean shimmer: cycle the 13-frame CLUT animation
        // (self-gates to None off the world map).
        self.advance_ocean_animation();
        // Scripted CLUT-cell effects (field-VM 4C 61 one-shots +
        // cross-fades): drain the world's banked game ticks against the
        // CPU VRAM (self-gates when none are live).
        self.apply_world_clut_fx();
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
