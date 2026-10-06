//! Extracted from `window.rs` (mechanical split; behavior-preserving).

use super::*;
use legaia_engine_audio::AudioSink;
// `AudioBgmDirector::stop` is a `BgmDirector` trait item; the title/boot
// hand-offs below stop the score through it.
use legaia_engine_core::scene::BgmDirector as _;

/// What [`PlayWindowApp::service_title_attract`] wants done with the title
/// screen's attract hand-off this frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TitleAttractAction {
    /// Nothing pending - run the ordinary title tick.
    Idle,
    /// The countdown fired: start this `fmv_id` and freeze the title.
    Start(i16),
    /// The movie is up; the title stays frozen.
    Playing,
    /// The movie drained and the session is back on the menu.
    Finished,
    /// The player aborted the movie with a pad press; the session is back on
    /// the menu and the caller has to tear the decoder down.
    Aborted,
}

/// Build the window's title session: [`legaia_engine_core::title::TitleSession::for_front_end`],
/// the constructor the browser page's title uses too. Continue follows
/// `continue_enabled`; the attract hand-off is armed (this host plays
/// retail's `fmv_id 0` through its windowed MDEC path).
pub(super) fn title_session(continue_enabled: bool) -> legaia_engine_core::title::TitleSession {
    legaia_engine_core::title::TitleSession::for_front_end(continue_enabled)
}

/// Does any port of this host's save rack hold a save? Port 1 is the save
/// directory, port 2 the mounted `--card` image. Scanning the directory
/// alone greys Continue out for a player whose only save is on the card they
/// mounted, which is the one thing `--card` exists for.
pub(super) fn rack_has_save(
    save_dir: &std::path::Path,
    card: Option<&legaia_save::emu::MountedCard>,
) -> bool {
    scan_save_dir(save_dir).iter().any(|s| s.present)
        || card.is_some_and(|c| {
            legaia_engine_core::save_select::card_block_snapshots(c)
                .iter()
                .any(|s| s.present)
        })
}

impl PlayWindowApp {
    /// Drive [`legaia_engine_core::save_screen::SaveScreenFlow`] for whichever
    /// save screen is open this frame, and return the edge the session should
    /// see.
    ///
    /// Two responsibilities, both the kernel's decision and neither this
    /// host's: *when* a port must be read (the flow asks, once per port), and
    /// what a pad edge means on the block grid. All this shell adds is the
    /// bytes - `disk_port_blocks` turns a port number into fifteen block
    /// snapshots, the same shape the browser's card rack hands over.
    fn service_save_flow(&mut self, edge: u16) -> u16 {
        use legaia_engine_core::field_menu_dispatch::FieldMenuSubsession;
        let session = match &self.boot_ui {
            BootUiState::SaveSelect(s) => Some(s),
            BootUiState::FieldMenu {
                sub: Some(FieldMenuSubsession::Save(s)),
            } => Some(s),
            _ => None,
        };
        let Some(session) = session else { return edge };
        let pending = self.save_flow.pending_read(session);
        let blocks = pending
            .map(|port| disk_port_blocks_with_card(&self.save_dir, self.card.as_ref(), port));
        // Re-borrow: `disk_port_blocks` needed `&self.save_dir` while the
        // session above borrowed `self.boot_ui`.
        let session = match &self.boot_ui {
            BootUiState::SaveSelect(s) => s,
            BootUiState::FieldMenu {
                sub: Some(FieldMenuSubsession::Save(s)),
            } => s,
            _ => return edge,
        };
        let mut flow = std::mem::take(&mut self.save_flow);
        if let (Some(port), Some(blocks)) = (pending, blocks) {
            flow.install_blocks(port, blocks);
        }
        let edge = flow.before_tick(session, edge);
        self.save_flow = flow;
        edge
    }

    /// Move the bytes a finished save screen asked for.
    ///
    /// [`SaveCommit`](legaia_engine_core::save_screen::SaveCommit) is in rack
    /// coordinates: `port` is the card the player picked off the pill row,
    /// `cell` the block they picked out of its 5x3 grid. This shell mounts
    /// its save directory as the card in port 1, where cell `i` is
    /// `slot_{i:02}` - a plain index, not the `i + 1` a real card's block 0
    /// directory forces. Any other port is unmounted and there is nothing to
    /// move.
    ///
    /// A **Save** records the loaded scene as the file's resume point
    /// ([`legaia_engine_shell::boot::BootSession::current_resume`]). A
    /// **Load** resumes through [`legaia_engine_shell::boot::BootSession::resume_save`],
    /// the landing order the browser page's `play_resume_save` shares
    /// (`legaia_engine_core::resume::land_save`): the save's own scene - retail
    /// resumes a save where it was written - else the scene already running,
    /// never a New Game. The world is hydrated after the entry, so the field
    /// VM's first tick sees the saved story state in the saved scene. Returns
    /// `true` when a scene was entered (the caller's screen state is stale
    /// then).
    fn apply_save_commit(&mut self, commit: legaia_engine_core::save_screen::SaveCommit) -> bool {
        use legaia_engine_core::save_screen::SaveCommitKind;
        // Port 2 is a mounted memory-card image: its Load reads the block's
        // SC bytes and its Save writes them through the shared card kernel.
        if commit.port == 1 {
            return self.apply_card_save_commit(commit);
        }
        if commit.port != 0 {
            log::warn!(
                "save screen: port {} holds no card; nothing written",
                commit.port + 1
            );
            return false;
        }
        let slot = commit.cell;
        match commit.kind {
            SaveCommitKind::Load => match read_slot_save(&self.save_dir, slot) {
                Ok((sf, resume)) => {
                    let landing =
                        self.session
                            .resume_save(sf, &resume.scene, &self.field_live_opts);
                    log::info!(
                        "save screen: loaded slot {slot}, landed {} ({:?})",
                        landing.kind(),
                        landing.scene()
                    );
                    if landing.entered_scene() {
                        // The host swapped scenes under the renderer: rebuild
                        // the render-side scene state so the landed scene's
                        // geometry replaces the old one.
                        self.rebuild_scene_render_state();
                        return true;
                    }
                }
                Err(e) => {
                    // A log line is not an answer to the player: raise the
                    // shared refusal notice the card path and the browser
                    // rack raise for the same failure.
                    log::warn!("save screen: load slot {slot} failed: {e:#}");
                    self.save_flow
                        .refuse(legaia_engine_core::save_screen::SaveRefusal::CardReadFailed);
                }
            },
            SaveCommitKind::Save => {
                let resume = self.session.current_resume();
                let sf = self.session.host.world.save_full();
                match write_slot_save(&self.save_dir, slot, &sf, &resume) {
                    Ok(p) => log::info!(
                        "save screen: saved slot {slot} to {} (scene '{}', '{}')",
                        p.display(),
                        resume.scene,
                        resume.location
                    ),
                    Err(e) => {
                        log::warn!("save screen: save slot {slot} failed: {e:#}");
                        self.save_flow
                            .refuse(legaia_engine_core::save_screen::SaveRefusal::CardWriteFailed);
                    }
                }
            }
        }
        false
    }

    /// The port-2 half of [`Self::apply_save_commit`]: a Load out of the
    /// mounted memory-card image, resumed into the save's own scene the same
    /// way a port-1 Load is, or a Save into it.
    ///
    /// A Save goes through the kernel the browser rack writes with
    /// ([`legaia_engine_core::card_write::write_save_into_card`]) - the block's
    /// save number, payload, engine ext tail, resume point, identity and
    /// directory claim are decided there - and the card is then written back
    /// to the image file `--card` mounted. Either step failing raises the
    /// shared refusal notice rather than closing on a log line.
    fn apply_card_save_commit(
        &mut self,
        commit: legaia_engine_core::save_screen::SaveCommit,
    ) -> bool {
        use legaia_engine_core::save_screen::SaveCommitKind;
        let cell = commit.cell;
        use legaia_engine_core::save_screen::SaveRefusal;
        if self.card.is_none() {
            log::warn!("save screen: port 2 holds no card; nothing read");
            self.save_flow.refuse(SaveRefusal::CardReadFailed);
            return false;
        }
        if matches!(commit.kind, SaveCommitKind::Save) {
            let sf = self.session.host.world.save_full();
            let resume = self.session.current_resume();
            let index = self.session.host.index.clone();
            let Some(card) = self.card.as_mut() else {
                return false;
            };
            let block = cell + 1;
            let wrote = legaia_engine_core::card_write::write_save_into_card(
                card,
                block,
                &sf,
                &resume,
                Some(&index),
            )
            .map_err(anyhow::Error::msg)
            .and_then(|w| card.persist().map(|_| w));
            match wrote {
                Ok(w) => log::info!(
                    "save screen: saved card block {block} as save {} (scene '{}'{})",
                    w.save_slot,
                    resume.scene,
                    if w.ext_written {
                        ""
                    } else {
                        ", engine ext withheld"
                    }
                ),
                Err(e) => {
                    log::warn!("save screen: card save into block {block} failed: {e:#}");
                    self.save_flow.refuse(SaveRefusal::CardWriteFailed);
                }
            }
            return false;
        }
        let Some(card) = self.card.as_ref() else {
            return false;
        };
        let Some((sf, resume)) = card.save_at(cell) else {
            log::warn!("save screen: card block {} holds no save", cell + 1);
            self.save_flow.refuse(SaveRefusal::CardReadFailed);
            return false;
        };
        let landing = self
            .session
            .resume_save(sf, &resume.scene, &self.field_live_opts);
        log::info!(
            "save screen: loaded card block {}, landed {} ({:?})",
            cell + 1,
            landing.kind(),
            landing.scene()
        );
        if landing.entered_scene() {
            self.rebuild_scene_render_state();
            return true;
        }
        false
    }

    /// Fire the pause menu's own blip for this frame's pad edges. Which cue,
    /// if any, is the engine's one rule
    /// ([`legaia_engine_core::menu_cues::menu_edge_blip`]) - the browser play
    /// page asks the same function through `play_menu_edge_blip`.
    /// `start_closes_menu` is whether this frame's Start closes the menu:
    /// on the root row list it does, inside a sub-screen it does not.
    pub(super) fn fire_menu_cues(&mut self, pressed: u16, start_closes_menu: bool) {
        if let Some(blip) =
            legaia_engine_core::menu_cues::menu_edge_blip(pressed, start_closes_menu)
        {
            self.fire_menu_cue(u16::from(blip.cue()));
        }
    }

    /// Queue one menu cue on the director (no-op with audio off).
    pub(super) fn fire_menu_cue(&mut self, cue: u16) {
        if let Some(bgm) = self.session.bgm.as_mut() {
            bgm.enqueue_sfx(cue, 0, 0, 0);
        }
    }

    /// Advance the SFX scheduler while a boot-UI arm owns the frame. The
    /// scene tick is skipped then, and with it `drain_and_log_battle_events`
    /// (the only other place the scheduler ticks), so a menu cue queued on
    /// the pause menu would otherwise wait for the menu to close to sound.
    pub(super) fn tick_menu_sfx(&mut self) {
        if let Some(bgm) = self.session.bgm.as_mut() {
            for (id, voice) in bgm.tick_sfx_frame() {
                log::debug!("menu SFX cue {id:#04x} fired on voice {voice}");
            }
        }
    }

    /// Score the title screen: start the global-pool title theme
    /// ([`legaia_engine_core::music_labels::TITLE_THEME_BGM_ID`]). Called on
    /// every entry to [`BootUiState::Title`]; the director suppresses a
    /// same-id restart, so re-entering from the save-select / options
    /// panels never drops the playhead. A `false` return (no disc bank,
    /// audio off) simply leaves the title silent.
    fn start_title_bgm(&mut self) {
        let id = legaia_engine_core::music_labels::TITLE_THEME_BGM_ID;
        if !self.session.start_global_bgm(id) {
            log::debug!("title: theme bgm {id} unavailable; title stays silent");
        }
    }

    /// Tick the boot-UI state machine (when active) using the latest
    /// pad bitmask. Returns `true` if the boot UI is still active and
    /// the scene tick should be skipped this frame.
    pub(super) fn tick_boot_ui(&mut self) -> bool {
        // Build edge-triggered "newly pressed" mask so menu navigation
        // doesn't auto-repeat on held keys.
        let pressed = self.pad & !self.prev_pad;
        // Save-screen driver first: answer the card read the flow is waiting
        // on and let it step the block-grid cursor / gate an empty-block Load
        // BEFORE the session sees this edge. It runs ahead of the match
        // because the match borrows `self.boot_ui` for the rest of the tick.
        let pressed = self.service_save_flow(pressed);
        // A refusal notice owns the pad while it is up: the edge that
        // dismisses it must not also drive the menu behind it.
        let pressed = if self.save_flow.tick_refusal(pressed) {
            0
        } else {
            pressed
        };
        // Read-only copy for the commit below, taken for the same reason.
        let save_flow = self.save_flow.clone();
        let cross = pressed & 0x4000 != 0;
        let circle = pressed & 0x2000 != 0;
        let triangle = pressed & 0x1000 != 0;
        let start = pressed & 0x0008 != 0;
        let up = pressed & 0x0010 != 0;
        let down = pressed & 0x0040 != 0;
        let left = pressed & 0x0080 != 0;
        let right = pressed & 0x0020 != 0;
        // Read before the match: it borrows `self.boot_ui`, so the title
        // arm cannot call back into `self`.
        let cutscene_live = self.cutscene.is_some();
        // Same reason: the Key Config screen consumes one key name per
        // physical press, and the match holds `self.boot_ui` for the rest of
        // the tick. Taken unconditionally - a key latched while no rebind
        // screen is open is stale by the next frame and must not survive to
        // be bound later.
        let pending_key = self.pending_key_name.take();
        let mut start_attract: Option<i16> = None;
        // The player aborted the attract movie this tick; the decoder is torn
        // down below, once the match has released `self.boot_ui`.
        let mut abort_attract = false;
        // The attract came back to the menu this tick, played or not.
        let mut attract_finished = false;
        // The pause menu's blips, off the raw edges before any screen
        // consumes them - the browser page asks the same engine rule. Start
        // blips as a cancel only where it closes the menu: a sub-screen
        // owns the pad and gets Start as an ordinary edge. Ahead of the
        // match because the match holds `self.boot_ui` for the rest of the
        // tick.
        if let BootUiState::FieldMenu { sub } = &self.boot_ui {
            let start_closes_menu = sub.is_none();
            self.fire_menu_cues(pressed, start_closes_menu);
        }

        let boot_ui_active = match &mut self.boot_ui {
            BootUiState::Inactive => false,
            BootUiState::PublisherLogos(session) => {
                // Start (or Cross) skips the boot sequence.
                if start || cross {
                    session.request_skip();
                }
                session.tick();
                if session.is_done() {
                    // The hand-off is the mode table's, not this host's:
                    // `init.pak`'s phase-3 arm runs the core-state reset and
                    // branches on the entry word - the front end (`CARD INIT`)
                    // when it is raised, the debug menu (`CONFIG INIT`) when it
                    // is not. The seat performs that branch and this arm raises
                    // the screen the mode it returns owns.
                    use legaia_engine_core::mode::GameMode;
                    let next = self.session.mode_seat.boot_handoff();
                    match next {
                        GameMode::CardInit => {
                            let any_present = rack_has_save(&self.save_dir, self.card.as_ref());
                            self.boot_ui = BootUiState::Title(title_session(any_present));
                            self.start_title_bgm();
                        }
                        // The dev route. The port has no debug-menu screen, so
                        // it lands on the title anyway - logged rather than
                        // silently folded, because the two are different modes.
                        other => {
                            log::info!("boot hand-off went to {other:?}; no engine screen owns it");
                            self.boot_ui = BootUiState::Title(title_session(false));
                            self.start_title_bgm();
                        }
                    }
                }
                true
            }
            BootUiState::Title(session) => 'title: {
                // The attract hand-off runs ahead of the tick: while the
                // movie owns the screen the title is frozen, exactly as
                // retail's master mode 0x1A takes the front-end off the
                // dispatcher until the STR overlay unloads.
                let attract = Self::service_title_attract(session, cutscene_live, pressed);
                if let TitleAttractAction::Start(id) = attract {
                    start_attract = Some(id);
                }
                if attract == TitleAttractAction::Aborted {
                    abort_attract = true;
                }
                if attract == TitleAttractAction::Finished {
                    attract_finished = true;
                }
                if matches!(
                    attract,
                    TitleAttractAction::Start(_)
                        | TitleAttractAction::Playing
                        | TitleAttractAction::Aborted
                ) {
                    break 'title true;
                }
                use legaia_engine_core::title::{TitleEvent, TitleInput, TitleOutcome};
                let input = TitleInput {
                    up,
                    down,
                    cross,
                    start,
                    circle,
                };
                let events = session.tick(input);
                for ev in &events {
                    match ev {
                        TitleEvent::NewGameSelected => {
                            // The retail sub-mode the row moved to: NEW GAME
                            // is 0x16 (LaunchFade), CONTINUE is 0x18
                            // (ContinueFadeIn). The browser page reads the
                            // same value through `boot_title_submode`.
                            log::info!(
                                "title: New Game (retail sub-mode 0x{:02X})",
                                session.retail_submode()
                            );
                        }
                        TitleEvent::ContinueSelected => {
                            log::info!(
                                "title: Continue (retail sub-mode 0x{:02X})",
                                session.retail_submode()
                            );
                        }
                        TitleEvent::AttractTimeout => {
                            log::info!(
                                "title: attract countdown fired (retail sub-mode 0x{:02X})",
                                session.retail_submode()
                            );
                        }
                        TitleEvent::OptionsSelected => {
                            // The selection event is informational; the Options
                            // panel opens when the title session resolves to
                            // `TitleOutcome::Options` below.
                            log::info!("title: Options");
                        }
                        _ => {}
                    }
                }
                if let Some(outcome) = session.outcome() {
                    match outcome {
                        TitleOutcome::NewGame => {
                            // Mirror the retail NEW GAME → field-launch
                            // (master mode 2 → mode 3): establish a fresh slate
                            // and seed the starting party (Vahn) from the disc's
                            // SCUS template, then enter the prologue cutscene
                            // scene `opdeene` (the front-end launcher's opening
                            // scene id, verified live), which hands off to the
                            // interactive `town01`. See docs/subsystems/boot.md
                            // "New Game boot chain".
                            // One call on both hosts: the seeded slate, the
                            // title theme stopped, and the shared opening
                            // order (`resume::enter_new_game`: `opdeene`,
                            // else `town01`). The page's twin is
                            // `play_new_game`.
                            match self.session.start_new_game(&self.field_live_opts) {
                                Some(scene) => {
                                    // The cutscene -> Rim Elm handoff is armed
                                    // inside `enter_field_scene` off opdeene's
                                    // MAN timeline
                                    // (World::arm_prologue_handoff_from_man).
                                    log::info!(
                                        "new game: seeded party_count={}, entered '{scene}'",
                                        self.session.host.world.party.party_count,
                                    );
                                    // The host swapped to the prologue scene:
                                    // rebuild the render-side scene state so its
                                    // geometry replaces the boot scene's.
                                    self.rebuild_scene_render_state();
                                }
                                None => log::warn!(
                                    "new game: no opening scene would enter; staying on the \
                                     pre-booted scene"
                                ),
                            }
                            self.boot_ui = BootUiState::Inactive;
                        }
                        TitleOutcome::Continue => {
                            // Open the save-select panel against the shell's
                            // rack: retail's two card ports, port 1 mounted
                            // with `save_dir`. The rack kind is what puts the
                            // session in the two-stage flow - no host flips
                            // that flag by hand.
                            self.save_flow.reset();
                            self.boot_ui = BootUiState::SaveSelect(
                                legaia_engine_core::save_select::SaveSelectSession::for_rack(
                                    legaia_engine_core::save_select::SaveSelectMode::Load,
                                    &disk_save_rack_with_card(&self.save_dir, self.card.as_ref()),
                                ),
                            );
                        }
                        TitleOutcome::Options => {
                            // The retail options screen is the pause menu's
                            // framed Options sub-screen; the title reaches it
                            // through the same menu runtime the Start press
                            // does, as the browser play page's title does
                            // (`play_menu_open_row("Options")`). A refused
                            // open leaves the title up.
                            if !self.open_menu_row_from_title(
                                legaia_engine_core::field_menu::FieldMenuRow::Options,
                            ) {
                                self.boot_ui = BootUiState::Title(title_session(rack_has_save(
                                    &self.save_dir,
                                    self.card.as_ref(),
                                )));
                                self.start_title_bgm();
                            }
                        }
                    }
                }
                true
            }
            BootUiState::SaveSelect(session) => {
                use legaia_engine_core::save_select::{SelectInput, SelectOutcome};
                let input = SelectInput {
                    up,
                    down,
                    left,
                    right,
                    cross,
                    circle,
                    triangle,
                };
                let _ = session.tick(input);
                if let Some(outcome) = session.outcome() {
                    // `commit` pairs the port off the outcome with the block
                    // cell off the grid; the flat pill-slot reading is what
                    // this used to do and is wrong for a two-stage rack.
                    let commit = save_flow.commit(session);
                    match outcome {
                        SelectOutcome::Cancelled => {
                            // Back to title (the theme is already up; the
                            // director suppresses the same-id restart).
                            self.boot_ui = BootUiState::Title(title_session(rack_has_save(
                                &self.save_dir,
                                self.card.as_ref(),
                            )));
                            self.start_title_bgm();
                        }
                        _ => {
                            if let Some(c) = commit {
                                // A Load re-enters the save's own scene; the
                                // pre-booted `--scene` is only the fallback
                                // for a file with no resume point.
                                let _ = self.apply_save_commit(c);
                            }
                            // Hand the score from the title theme to the
                            // loaded scene: stop, then re-play the world's
                            // own op-0x35 track when it is a global-pool id.
                            if let Some(bgm) = self.session.bgm.as_mut() {
                                bgm.stop();
                            }
                            self.session.restore_field_bgm();
                            self.boot_ui = BootUiState::Inactive;
                        }
                    }
                }
                true
            }
            BootUiState::FieldMenu { sub } => {
                use legaia_engine_core::field_menu::FieldMenuOutcome;
                use legaia_engine_core::field_menu_dispatch::{
                    FieldMenuSubsession, SubsessionHandoff, finish_subsession,
                    tick_open_subsession, tick_root_list,
                };
                // The menu session is hosted by the BootSession (so headless
                // drivers share it); if it vanished out from under the UI
                // arm, drop back to the scene.
                if self.session.field_menu.is_none() {
                    self.boot_ui = BootUiState::Inactive;
                    return true;
                }
                // Window 7 (spell level-up notice) owns the pad while armed:
                // retail's cast sub-screens stall on the confirm | cancel
                // masks after the widget-VM `[open window 7]` script
                // (`0x801E4D50` / `0x801E4D78`), and nothing else on the pad
                // moves. The press both dismisses and is consumed.
                if self
                    .menu_runtime
                    .dismiss_spell_level_notice(cross, circle, triangle)
                {
                    return true;
                }
                // Window 8 (art learned) - the Items use sub-screen's same
                // confirm | cancel stall after `[open window 8]`.
                if self
                    .menu_runtime
                    .dismiss_art_learned_notice(cross, circle, triangle)
                {
                    return true;
                }
                // A bind committed inside the Options sub-session's Key
                // Config screen this tick. Held in a local because
                // `self.mapping` cannot be written while `sub` borrows
                // `self.boot_ui`.
                let rebound_in_menu;
                if let Some(active_sub) = sub.as_mut() {
                    // A sub-session is open: the shared step (with the
                    // Status screen's Triangle -> Arts editor extension)
                    // routes the edge and hands back a committed rebind.
                    rebound_in_menu = tick_open_subsession(
                        active_sub,
                        pressed,
                        pending_key,
                        &self.session.host.world,
                    );
                    if active_sub.is_done() {
                        // Fold the outcome into the world through the one
                        // router every host shares; only the save rack and
                        // the options file are this host's.
                        let finished = sub.take().expect("sub was Some");
                        let mut done = finish_subsession(finished, &mut self.session.host.world);
                        // Windows 7 / 8: the runtime holds the beat and this
                        // arm's pre-empts above hold the pad.
                        self.menu_runtime.arm_finished_notices(&mut done);
                        match done.handoff {
                            SubsessionHandoff::Applied => {}
                            // The retail Load / Save rows, committed through
                            // the shared flow: the outcome names the card
                            // port, the grid names the block.
                            SubsessionHandoff::Save(s) => {
                                if let Some(c) = save_flow.commit(&s)
                                    && self.apply_save_commit(c)
                                {
                                    // The Load re-entered the save's scene:
                                    // the menu was opened over a scene that
                                    // is gone. Drop it and let the fresh
                                    // scene's own mode stand rather than
                                    // the one the menu suspended.
                                    let mode = self.session.host.world.mode;
                                    self.session.close_field_menu();
                                    self.session.host.world.mode = mode;
                                    self.session.mode_seat.adopt_scene_mode(mode);
                                    self.boot_ui = BootUiState::Inactive;
                                    return true;
                                }
                            }
                            SubsessionHandoff::Options(state) => {
                                self.options_state = state;
                                self.persist_and_apply_options();
                            }
                        }
                        // A title-opened menu has no root screen to return
                        // to: the sub-screen's exit closes it entirely.
                        if let Some(menu) = self.session.field_menu.as_mut() {
                            let _ = menu.resume(self.menu_from_title);
                        }
                    }
                    // Adopt + persist the rebind now that the `self.boot_ui`
                    // borrow is dead: the live table is what the very next
                    // key event resolves through, and the file is the same
                    // `legaia-input.toml` the CLI editor writes.
                    if let Some(mapping) = rebound_in_menu {
                        self.mapping = mapping;
                        self.persist_bindings();
                    }
                    return true;
                }
                // After Cross on a row the menu phase becomes Suspended.
                // Build the matching sub-session and route control there.
                let suspended_row = self
                    .session
                    .field_menu
                    .as_mut()
                    .and_then(|menu| tick_root_list(menu, pressed));
                if let Some(row) = suspended_row {
                    // The shell's save rack: retail's two card ports, port 1
                    // mounted with `save_dir`. Its kind is what puts a Load /
                    // Save sub-session in the two-stage flow.
                    let rack = disk_save_rack_with_card(&self.save_dir, self.card.as_ref());
                    self.save_flow.reset();
                    // Build sub-sessions from the DISC tables the boot path
                    // already installed on the world (spell table, equipment
                    // bonus table) plus the live saved-chain library - not
                    // throwaway vanilla()/new() placeholders, which ignored
                    // any randomizer/disc data and dropped Arts edits.
                    let world = &self.session.host.world;
                    let chain_library = world.chain_library();
                    let mut built = FieldMenuSubsession::build(
                        row,
                        world,
                        &self.options_state,
                        &rack,
                        &chain_library,
                        &world.tables.spell_catalog,
                        &world.tables.equipment_table,
                    );
                    // The Options row grows its engine-only Key Config row
                    // only where a host has a binding table to edit; the
                    // browser play page arms the same row off its own stored
                    // table.
                    built.arm_key_rebind(self.mapping.clone());
                    *sub = Some(built);
                }
                let outcome = self.session.field_menu.as_ref().and_then(|m| m.outcome());
                if let Some(outcome) = outcome {
                    match outcome {
                        FieldMenuOutcome::Closed | FieldMenuOutcome::Confirmed(_) => {
                            // Closed = player backed out; Confirmed = a
                            // sub-session signaled "close menu entirely" via
                            // resume(true). Either way restore the suspended
                            // scene mode and drop straight to the scene.
                            self.session.close_field_menu();
                            self.boot_ui = BootUiState::Inactive;
                            if std::mem::take(&mut self.menu_from_title) {
                                self.boot_ui = BootUiState::Title(title_session(rack_has_save(
                                    &self.save_dir,
                                    self.card.as_ref(),
                                )));
                                self.start_title_bgm();
                            }
                        }
                    }
                }
                true
            }
            BootUiState::GameOver(session) => {
                use legaia_engine_core::game_over::GameOverOutcome;
                // No input arm: retail's wipe path offers the player nothing.
                // It stores `game_mode = 0x16` + `_DAT_8007BB00 = 1`
                // (`FUN_8003AEB0` `0x8003B5D0` / `0x8003B5E0`) and the title
                // overlay takes the screen. The hold below stands in for the
                // menu-overlay stream retail spends that window on.
                session.tick();
                if let Some(GameOverOutcome::ReturnToTitle) = session.outcome() {
                    // The hold froze the world mid-battle (`finish_battle`
                    // deferred the field restore so the final battle frame
                    // stayed up). Complete the deferred restore before the
                    // title takes the screen, so Continue / New Game starts
                    // from a field-shaped world.
                    self.session.host.world.resolve_game_over_hold();
                    // The wipe left the battle track running; the title
                    // overlay owns the screen now, so swap the score with
                    // it. Stop first so a failed title-theme start leaves
                    // silence rather than the stale battle BGM.
                    if let Some(bgm) = self.session.bgm.as_mut() {
                        bgm.stop();
                    }
                    self.boot_ui = BootUiState::Title(title_session(rack_has_save(
                        &self.save_dir,
                        self.card.as_ref(),
                    )));
                    self.start_title_bgm();
                }
                true
            }
        };
        // The title arm cannot reach back into `self`, so the attract's
        // decode + stage happens here, once the match's borrow is dead.
        if let Some(fmv_id) = start_attract {
            self.start_title_attract(fmv_id);
        }
        // The player aborted the attract: drop the decoder and give the score
        // back, the same teardown the redraw loop's drain runs when the movie
        // ends on its own. Without it the picture would keep drawing over a
        // title the session has already returned to.
        if abort_attract {
            self.cutscene = None;
            self.end_movie_audio();
        }
        // An attract that never staged a picture (a cut or undecodable slot)
        // comes back as `Finished` without the redraw drain having run: end
        // it here. A drained one already ended, and a second end asks for
        // nothing.
        if attract_finished {
            self.end_movie_audio();
        }
        boot_ui_active
    }

    /// End the movie in flight on the audio side, however it ended: stop its
    /// XA, reopen the sequencer gate only if this movie closed it, and bring
    /// the title theme back from its first beat after the attract - the
    /// shared [`legaia_engine_core::movie_audio::MovieScore`] decides each.
    pub(super) fn end_movie_audio(&mut self) {
        if let Some(out) = self.session.audio.as_ref() {
            out.stop_xa();
        }
        let end = self.movie_score.on_movie_end();
        if end.reopen_gate
            && let Some(out) = self.session.audio.as_ref()
        {
            out.set_sequencer_paused(false);
        }
        if end.restart_title_theme {
            self.start_title_bgm();
        }
    }

    /// Arm the movie-audio policy for a movie this host is about to play,
    /// and release the score when it says to (the title attract).
    fn begin_movie_audio(&mut self, origin: legaia_engine_core::movie_audio::MovieOrigin) {
        if self.movie_score.on_movie_start(origin)
            && let Some(bgm) = self.session.bgm.as_mut()
        {
            bgm.stop();
        }
    }

    /// Write the live binding table back to `legaia-input.toml` - the file
    /// [`legaia_engine_core::input::Mapping::load_or_default`] reads at
    /// startup and `legaia-engine config set --binding` edits from the
    /// command line. The pause menu's Key Config screen is a third editor of
    /// the same file, not a second store.
    pub(super) fn persist_bindings(&self) {
        if self.scripted_run() {
            return;
        }
        let path = std::path::PathBuf::from(INPUT_CONFIG_FILE);
        if let Err(e) = self.mapping.save(&path) {
            log::warn!("bindings: save to {} failed: {e:#}", path.display());
        }
    }

    /// Play the title screen's attract movie in-window.
    ///
    /// Retail's `AttractIdle` arm hands the screen to `fmv_id 0`
    /// (`_DAT_8007BA78 = 0` at `0x801DDCE8`, master mode `0x1A` at
    /// `0x801DDCF0`); this decodes that movie through the same kernel the
    /// field-VM cutscene path uses and stages it as the in-window video.
    /// Retail releases the title theme as the attract starts and `CARD INIT`
    /// streams it again on the way back, so the theme is stopped here and
    /// restarted from its first beat when the movie ends
    /// ([`legaia_engine_core::movie_audio`]).
    ///
    /// A slot that will not decode leaves `self.cutscene` empty, which the
    /// next `tick_boot_ui` reads as "the movie drained" and returns the
    /// session to the menu - the same place a played-out movie lands.
    // REF: FUN_801DD35C
    fn start_title_attract(&mut self, fmv_id: i16) {
        self.begin_movie_audio(legaia_engine_core::movie_audio::MovieOrigin::Attract);
        let Some(rel) = legaia_engine_core::cutscene::fmv_index_to_str_filename(fmv_id) else {
            log::info!("title attract: fmv_id={fmv_id} (cut/unmapped slot); skipping");
            return;
        };
        match self.decode_fmv(fmv_id, rel) {
            Some(decoded) => self.stage_windowed_cutscene(decoded),
            None => log::info!("title attract: fmv_id={fmv_id} did not decode; staying on title"),
        }
    }

    /// Open the pause menu straight onto one row's sub-screen, for the
    /// title's Options row: the native twin of the browser play page's
    /// `play_menu_open_row`. The row is reached through the shared picker's
    /// own confirm routing (cursor steps + Cross), so a row its gate blocks
    /// stays blocked. `false` when the open or the row is refused, with the
    /// menu closed again.
    pub(super) fn open_menu_row_from_title(
        &mut self,
        row: legaia_engine_core::field_menu::FieldMenuRow,
    ) -> bool {
        use legaia_engine_core::field_menu::{FieldMenuInput, FieldMenuPhase};
        use legaia_engine_core::field_menu_dispatch::FieldMenuSubsession;
        self.session.open_field_menu();
        let Some(menu) = self.session.field_menu.as_mut() else {
            return false;
        };
        if !menu.row_is_available(row) {
            self.session.close_field_menu();
            return false;
        }
        for _ in 0..row.index() {
            let _ = menu.tick(FieldMenuInput {
                down: true,
                ..FieldMenuInput::default()
            });
        }
        let _ = menu.tick(FieldMenuInput {
            cross: true,
            ..FieldMenuInput::default()
        });
        if menu.phase() != (FieldMenuPhase::Suspended { row }) {
            self.session.close_field_menu();
            return false;
        }
        let rack = disk_save_rack_with_card(&self.save_dir, self.card.as_ref());
        self.save_flow.reset();
        let world = &self.session.host.world;
        let chain_library = world.chain_library();
        let mut built = FieldMenuSubsession::build(
            row,
            world,
            &self.options_state,
            &rack,
            &chain_library,
            &world.tables.spell_catalog,
            &world.tables.equipment_table,
        );
        built.arm_key_rebind(self.mapping.clone());
        self.boot_ui = BootUiState::FieldMenu { sub: Some(built) };
        self.menu_from_title = true;
        true
    }

    /// Build text draws for the active boot UI (when applicable).
    pub(super) fn boot_ui_draws(&self, surface_w: u32, surface_h: u32) -> Vec<TextDraw> {
        match &self.boot_ui {
            BootUiState::Inactive => Vec::new(),
            BootUiState::PublisherLogos(_) => {
                // The publisher logos are drawn via the sprite overlay
                // (see `publisher_logo_sprite_draw`); no font text.
                Vec::new()
            }
            BootUiState::Title(s) => {
                use legaia_engine_core::title::TitlePhase;
                // Which screen the front end draws is retail's own selector
                // word `state[+0x204]`, not this host's enum: both hosts ask
                // `title_text_phase` about `TitleSession::retail_submode`, so
                // one table decides it (see `ui::title_draw_list`). The
                // session's phase is still what supplies the *cursor row* and
                // the blink, which the sub-mode word does not carry.
                let (menu_open, cursor) = match s.phase() {
                    TitlePhase::MainMenu { cursor } => (true, cursor),
                    // The attract movie owns the screen; the window
                    // renders its frames, not the title's text.
                    TitlePhase::Attract { .. } => return Vec::new(),
                    TitlePhase::Done(_) => return Vec::new(),
                    TitlePhase::FadeIn { .. } => return Vec::new(),
                    TitlePhase::PressStart { .. } => (false, 0),
                };
                let phase_id =
                    legaia_engine_render::title_text_phase(s.retail_submode(), menu_open);
                // When the title-screen atlas is uploaded, the
                // main-menu rows render through the sprite path,
                // sampling NEW GAME / CONTINUE sub-rects from the
                // title TIM directly (retail-faithful). Suppress
                // the dialog-font fallback for phase 2 so the rows
                // aren't double-drawn. Earlier phases (fade /
                // press-start) still use the dialog font for their
                // prompt text.
                if phase_id == 2 && self.title_screen.is_some() {
                    return Vec::new();
                }
                let blink_on = match s.phase() {
                    TitlePhase::PressStart { blink_phase } => blink_phase < s.blink_period / 2,
                    _ => true,
                };
                // When the PROT 0888 title atlas is loaded, anchor the
                // menu text to the same centred + integer-scaled 256×256
                // stage `title_screen_sprite_draws` uses, so the menu
                // sits between the wordmark band (ends at src y=140)
                // and the press-start / copyright bands (start at src
                // y=178). Without an atlas we keep the legacy
                // (96, 100) pen so the no-disc fallback still renders.
                let atlas_present = self.title_screen.is_some();
                let pen = if atlas_present {
                    let atlas_w: u32 = 256;
                    let atlas_h: u32 = 256;
                    let scale = (surface_w / atlas_w.max(1))
                        .min(surface_h / atlas_h.max(1))
                        .clamp(1, 4) as i32;
                    let stage_x0 = (surface_w as i32 - (atlas_w as i32) * scale) / 2;
                    let stage_y0 = (surface_h as i32 - (atlas_h as i32) * scale) / 2;
                    // src-y=148 sits between the wordmark and the
                    // press-start/copyright bands; src-x=104 centres
                    // a ~6-glyph menu row inside the 256-wide stage.
                    (stage_x0 + 104 * scale, stage_y0 + 148 * scale)
                } else {
                    (96, 100)
                };
                legaia_engine_render::title_draws_for(
                    &self.font,
                    phase_id,
                    cursor,
                    s.continue_enabled,
                    blink_on,
                    atlas_present,
                    pen,
                )
            }
            BootUiState::SaveSelect(s) => {
                // The whole screen's text half - the mode's title word and
                // pill rows, the phase overlays (NowChecking dialog, slot-info
                // text / captions, confirm messagebox) - through the one
                // composition the pause menu's Load / Save rows share.
                self.save_select_overlay(s, surface_w, surface_h).texts
            }
            BootUiState::FieldMenu { sub } => {
                use legaia_engine_core::field_menu_dispatch::FieldMenuSubsession;
                // The kind-0x0D pair replaces the root list rather than
                // overlaying it: both sub-screens hand the widget VM a
                // script that opens with `05 00` (close every window) and
                // then opens their own one, so the command rows are not on
                // screen while either is up.
                let context_screen = self.context_locked_screen_draws(surface_w, surface_h);
                let mut draws = if !context_screen.is_empty() {
                    context_screen.texts
                } else if let Some(FieldMenuSubsession::Save(s)) = sub {
                    // The Save sub-session renders through the save-select
                    // stage it shares with the boot Continue -> Load screen,
                    // which pre-scales to surface coords.
                    self.field_save_sub_draws(s, surface_w, surface_h)
                } else if let Some(active_sub) = sub {
                    self.field_menu_sub_draws(active_sub, surface_w, surface_h)
                        .texts
                } else {
                    // Command rows fill the id-50 list window,
                    // money/play-time the id-49 corner box, and the party
                    // overview the id-51 right panel (the pinned top-level
                    // window set).
                    self.field_menu_root_draws(surface_w, surface_h).texts
                };
                // Window 7 - the spell level-up notice - overlays whichever
                // menu screen is current while `MenuRuntime` holds the beat.
                draws.extend(self.magic_level_notice_draws(surface_w, surface_h));
                draws.extend(self.art_learned_notice_window_draws(surface_w, surface_h));
                draws
            }
            // The wipe hand-off draws nothing: retail's next frame after the
            // `game_mode = 0x16` store is the title overlay fading in, and
            // the browser host is silent here for the same reason. The panel
            // that used to be built here was an engine invention.
            BootUiState::GameOver(_) => Vec::new(),
        }
    }

    /// Drain world field events and route them to whichever subsystem
    /// owns them. Currently:
    /// - [`FieldEvent::ActorSpawned`]: when the actor carries a non-`None`
    ///   `Actor::tmd_ref` (the `0x4C 0xD8` synchronous-spawn path), queue
    ///   the slot in [`Self::pending_dynamic_mesh_slots`] so the next
    ///   render pass uploads its mesh. ActorSpawned events without a
    ///   `tmd_ref` (the `0x4C 0x80` halt-acquire-gated bytecode-only
    ///   path) are dropped silently here - those actors have no visual
    ///   in this renderer until their bytecode runs.
    /// - All other events: not relevant to the play-window renderer yet,
    ///   surfaced via the HUD log instead by callers that want them.
    pub(super) fn drain_and_route_field_events(&mut self) {
        use legaia_engine_core::field_events::FieldEvent;
        // Route any BGM event the frame's own steps raised after the session
        // tick's routing pass - a dance song ending on its own
        // (`World::exit_dance` -> `restore_minigame_bgm`), a minigame swap -
        // before the drain below drops every non-spawn event. Without this the
        // hall's track restart was thrown away here and the chart loop played
        // on over the field. The browser page makes the same second pass
        // (`route_bgm_wasm` ahead of `drain_and_route_field_events_web`).
        if let Some(bgm) = self.session.bgm.as_mut()
            && let Err(e) = self.session.host.route_bgm_events(bgm)
        {
            log::error!("late BGM route: {e:#}");
        }
        let world = &mut self.session.host.world;
        let events = world.drain_field_events();
        // `ActorSpawned` is the only variant this drain answers, and there is
        // deliberately no `CameraConfigure` arm: `Camera::route_camera_events`
        // has already consumed every one off the world queue during the
        // session tick and does not restore it, so an arm here could never
        // fire. The `apply == 0` snap beats are banked on the camera instead
        // (`Camera::take_camera_snap_beats`, replayed by
        // `frame_step::CutsceneGlide::advance`).
        for ev in events {
            if let FieldEvent::ActorSpawned { slot, .. } = ev {
                let has_tmd = world
                    .actors
                    .get(slot as usize)
                    .is_some_and(|a| a.tmd_ref.is_some());
                if has_tmd {
                    self.pending_dynamic_mesh_slots.push(slot);
                }
            }
        }
    }

    /// Start windowed cutscene playback when the world has flipped into
    /// [`SceneMode::Cutscene`] (a field-VM FMV-trigger op fired). Resolves the
    /// active FMV's `MV*.STR` and decodes it: from the disc image (raw 2352-
    /// byte sectors, so the interleaved XA audio plays in sync) when booting
    /// from a disc, otherwise the video-only Form-1 extract under the extracted
    /// root. A cut/missing slot, an unresolvable path, or a decode that yields
    /// no frames drains the trigger immediately via `finish_cutscene` (no-op),
    /// matching the headless `play` loop. Leaves `self.cutscene = None` when
    /// nothing starts.
    /// Narrow a whole-`MVn.STR`-file sector span to just the segment a given
    /// `fmv_id` plays, using the FMV dispatch table decoded from the cutscene
    /// overlay (PROT 0970). One `MVn.STR` can carry several cutscenes by frame
    /// range (e.g. `MV3.STR` -> fmv 1 / 2 / ...), so without this an `fmv_id`
    /// that seeks into the file would play from the wrong frame. Returns
    /// `(start_lba, sector_count)`; falls back to the whole file
    /// (`(file_lba, file_sectors)`) when the table / entry is unavailable.
    pub(super) fn fmv_segment_window(
        &self,
        fmv_id: i16,
        file_lba: u32,
        file_sectors: u32,
    ) -> (u32, u32) {
        use legaia_asset::fmv_dispatch::{FmvTable, STR_OVERLAY_PROT_INDEX};
        let table = self
            .session
            .host
            .index
            .entry_bytes(STR_OVERLAY_PROT_INDEX)
            .ok()
            .and_then(|b| FmvTable::from_str_overlay(&b[..]));
        legaia_engine_shell::cutscene_av::fmv_segment_window(
            table.as_ref().and_then(|t| t.entry(fmv_id)),
            file_lba,
            file_sectors,
        )
    }

    /// Decode one `fmv_id`'s `MV*.STR` into frames + timing + audio, from the
    /// disc image when one booted this session (raw 2352-byte sectors, so the
    /// interleaved XA plays in sync) and otherwise from the video-only Form-1
    /// extract. `None` when the slot is cut, the path does not resolve, or the
    /// decode yields no frames.
    ///
    /// Shared by the in-flow field-VM cutscene
    /// ([`Self::try_start_windowed_cutscene`]) and the title attract
    /// ([`Self::service_title_attract`]) so both hosts of the FMV path decode
    /// through one kernel.
    pub(super) fn decode_fmv(
        &self,
        fmv_id: i16,
        rel: &str,
    ) -> Option<(
        Vec<legaia_mdec::VideoFrame>,
        std::time::Duration,
        Option<legaia_engine_shell::cutscene_av::CutsceneAudio>,
    )> {
        use legaia_engine_shell::cutscene_av::{decode_str_av_from_disc, decode_str_video_only};
        if let Some(disc_path) = self.disc_path.as_ref() {
            match resolve_iso_file(disc_path, Path::new(rel)) {
                Ok((lba, size)) => {
                    let total = size.div_ceil(legaia_iso::raw::USER_DATA_SIZE as u32);
                    // Narrow to the fmv_id's frame-range segment (multi-cutscene
                    // files like MV3.STR carry several fmv_ids by frame range).
                    let (lba, count) = self.fmv_segment_window(fmv_id, lba, total);
                    match decode_str_av_from_disc(disc_path, lba, count) {
                        Ok(av) if !av.frames.is_empty() => {
                            log::info!(
                                "cutscene: playing fmv_id={fmv_id} {rel} from disc \
                                 ({} frames, {:.2} fps, audio: {})",
                                av.frames.len(),
                                av.timing.fps,
                                if av.audio.is_some() { "yes" } else { "no" }
                            );
                            Some((av.frames, av.timing.frame_period(), av.audio))
                        }
                        Ok(_) => {
                            log::warn!("cutscene: fmv_id={fmv_id} {rel} decoded no frames");
                            None
                        }
                        Err(e) => {
                            log::warn!(
                                "cutscene: fmv_id={fmv_id} {rel} disc decode failed ({e:#})"
                            );
                            None
                        }
                    }
                }
                Err(e) => {
                    log::warn!("cutscene: fmv_id={fmv_id} {rel} not on disc ({e:#})");
                    None
                }
            }
        } else if let Some(root) = self.extracted_root.as_ref() {
            let path = root.join(rel);
            match decode_str_video_only(&path) {
                Ok((frames, timing)) if !frames.is_empty() => {
                    log::info!(
                        "cutscene: playing fmv_id={fmv_id} {rel} ({} frames, {:.2} fps, no audio)",
                        frames.len(),
                        timing.fps
                    );
                    Some((frames, timing.frame_period(), None))
                }
                Ok(_) => {
                    log::warn!("cutscene: fmv_id={fmv_id} {rel} decoded no frames; skipping");
                    None
                }
                Err(e) => {
                    log::warn!(
                        "cutscene: fmv_id={fmv_id} {} decode failed ({e:#}); skipping",
                        path.display()
                    );
                    None
                }
            }
        } else {
            log::info!("cutscene: fmv_id={fmv_id} (no disc / extracted root); skipping");
            None
        }
    }

    /// Stage decoded frames as the in-window movie.
    fn stage_windowed_cutscene(
        &mut self,
        decoded: (
            Vec<legaia_mdec::VideoFrame>,
            std::time::Duration,
            Option<legaia_engine_shell::cutscene_av::CutsceneAudio>,
        ),
    ) {
        let (frames, frame_period, audio) = decoded;
        self.cutscene = Some(WindowedCutscene {
            frames,
            idx: 0,
            uploaded: None,
            frame_period,
            clock: None,
            pending_audio: audio,
            has_audio: false,
        });
    }

    /// The title screen's attract hand-off, native side.
    ///
    /// Retail's `AttractIdle` (`0x10`) arm zeroes the FMV index and writes
    /// master game mode `0x1A`, i.e. hands the screen to `fmv_id 0`
    /// (`MV1.STR`) and comes back to the title afterwards
    /// (`legaia_engine_vm::title_overlay::ATTRACT_FMV_ID`;
    /// `cutscene_trigger::TITLE_TICK_INLINE`). This runs the same movie
    /// through the window's own MDEC path and calls `finish_attract` once its
    /// frames drain - the drain itself is the redraw handler's, which clears
    /// `self.cutscene`.
    ///
    /// Returns `true` while the attract owns the screen, so the caller skips
    /// the rest of the title tick.
    // REF: FUN_801DD35C
    ///
    /// `pressed` is this tick's just-pressed pad word. Retail lets the player
    /// abort the attract movie (`fmv_id 0` is the one skippable movie -
    /// `legaia_engine_core::cutscene::fmv_skip_edge_hit`), and this host used
    /// to play it to its last frame whatever the player pressed: the abort
    /// existed only in the browser. An abort returns
    /// [`TitleAttractAction::Aborted`] so the caller can tear the decoder
    /// down; the session is finished here either way.
    fn service_title_attract(
        session: &mut legaia_engine_core::title::TitleSession,
        cutscene_live: bool,
        pressed: u16,
    ) -> TitleAttractAction {
        if let Some(fmv_id) = session.attract_pending() {
            session.mark_attract_started();
            return TitleAttractAction::Start(fmv_id);
        }
        if session.attract_playing() && !cutscene_live {
            session.finish_attract();
            return TitleAttractAction::Finished;
        }
        if session.attract_playing() {
            // The attract slot is the skippable movie; ask the shared test
            // rather than spelling the button set out a second time.
            if legaia_engine_core::cutscene::fmv_skip_edge_hit(
                legaia_engine_vm::title_overlay::ATTRACT_FMV_ID,
                pressed,
            ) {
                session.finish_attract();
                return TitleAttractAction::Aborted;
            }
            return TitleAttractAction::Playing;
        }
        TitleAttractAction::Idle
    }

    pub(super) fn try_start_windowed_cutscene(&mut self) {
        let Some(fmv_id) = self.session.host.world.active_fmv() else {
            return;
        };
        self.begin_movie_audio(legaia_engine_core::movie_audio::MovieOrigin::Cutscene);
        let Some(rel) = self.session.host.world.active_fmv_str_filename() else {
            log::info!("cutscene: fmv_id={fmv_id} (cut/unmapped slot); skipping");
            self.session.host.world.finish_cutscene();
            self.end_movie_audio();
            return;
        };
        match self.decode_fmv(fmv_id, rel) {
            Some(decoded) => self.stage_windowed_cutscene(decoded),
            None => {
                // Drain the trigger so the field resumes next frame.
                self.session.host.world.finish_cutscene();
                self.end_movie_audio();
            }
        }
    }

    /// Render the active cutscene's current frame, paced to the stream's
    /// detected frame rate. The visible frame is `elapsed / frame_period`, so
    /// playback runs at the movie's real ~15 fps regardless of the display
    /// refresh rate (frames are held, or dropped if the host falls behind).
    /// `idx` tracks the due frame so the drain check at the top of the redraw
    /// handler resumes the field once the full duration has elapsed.
    pub(super) fn render_windowed_cutscene(&mut self) {
        // Clone the audio handle before borrowing the renderer / cutscene so
        // staging the track and reading its cursor don't alias `self`.
        let audio_out = self.session.audio.clone();
        let Some(renderer) = self.win.renderer.as_ref() else {
            return;
        };
        if let Some(c) = self.cutscene.as_mut() {
            // Stage the interleaved audio on the first render so the audio
            // cursor (the A/V-sync master clock) starts with the picture, and
            // duck the score under it - only if it is sounding, so a track
            // the script paused stays the script's (`movie_audio`).
            if let (Some(out), Some(track)) = (audio_out.as_ref(), c.pending_audio.take()) {
                if self.movie_score.on_movie_audio(out.sequencer_paused()) {
                    out.set_sequencer_paused(true);
                }
                out.play_xa(track.pcm, track.sample_rate, track.channels, false, 0x4000);
                c.has_audio = true;
            }
            let now = std::time::Instant::now();
            let start = *c.clock.get_or_insert(now);
            let elapsed = now.duration_since(start).as_secs_f64();
            // A/V sync: drive the visible frame off the audio cursor while a
            // track is playing, else off wall-clock. `idx` reaching the frame
            // count signals end-of-playback to the drain check.
            let audio_secs = if c.has_audio {
                audio_out.as_ref().and_then(|o| o.xa_cursor_secs())
            } else {
                None
            };
            let due = legaia_engine_shell::cutscene_av::due_video_frame(
                audio_secs,
                elapsed,
                c.frame_period.as_secs_f64(),
            );
            c.idx = due;
            let show = due.min(c.frames.len().saturating_sub(1));
            if let Some(f) = c.frames.get(show) {
                match renderer.upload_texture(&f.rgba, f.width, f.height) {
                    Ok(tex) => c.uploaded = Some(tex),
                    Err(e) => log::warn!("cutscene upload: {e}"),
                }
            }
            match c.uploaded.as_ref() {
                Some(tex) => {
                    let _ = renderer.render(RenderTarget::Texture(tex));
                }
                None => {
                    let _ = renderer.render(RenderTarget::Clear);
                }
            }
        }
    }
}

impl BootUiState {
    pub(super) fn is_active(&self) -> bool {
        !matches!(self, BootUiState::Inactive)
    }
}
