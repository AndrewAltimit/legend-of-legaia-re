//! Extracted from `window.rs` (mechanical split; behavior-preserving).

use super::*;

impl PlayWindowApp {
    /// Tick the boot-UI state machine (when active) using the latest
    /// pad bitmask. Returns `true` if the boot UI is still active and
    /// the scene tick should be skipped this frame.
    pub(super) fn tick_boot_ui(&mut self) -> bool {
        // Build edge-triggered "newly pressed" mask so menu navigation
        // doesn't auto-repeat on held keys.
        let pressed = self.pad & !self.prev_pad;
        let cross = pressed & 0x4000 != 0;
        let circle = pressed & 0x2000 != 0;
        let triangle = pressed & 0x1000 != 0;
        let start = pressed & 0x0008 != 0;
        let up = pressed & 0x0010 != 0;
        let down = pressed & 0x0040 != 0;
        let left = pressed & 0x0080 != 0;
        let right = pressed & 0x0020 != 0;

        match &mut self.boot_ui {
            BootUiState::Inactive => false,
            BootUiState::PublisherLogos(session) => {
                // Start (or Cross) skips the boot sequence.
                if start || cross {
                    session.request_skip();
                }
                session.tick();
                if session.is_done() {
                    // Hand off to the title screen with the
                    // continue-enabled flag set per save-slot scan.
                    let snapshots = scan_save_dir(&self.save_dir);
                    let any_present = snapshots.iter().any(|s| s.present);
                    self.boot_ui = if any_present {
                        BootUiState::Title(legaia_engine_core::title::TitleSession::new())
                    } else {
                        BootUiState::Title(
                            legaia_engine_core::title::TitleSession::without_save_data(),
                        )
                    };
                }
                true
            }
            BootUiState::Title(session) => {
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
                            log::info!("title: New Game");
                        }
                        TitleEvent::ContinueSelected => {
                            log::info!("title: Continue");
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
                            self.session.begin_new_game();
                            let cutscene = legaia_asset::new_game::OPENING_CUTSCENE_SCENE;
                            match self
                                .session
                                .enter_field_live(cutscene, &self.field_live_opts)
                            {
                                Ok(mode) => {
                                    // The cutscene -> Rim Elm handoff is now armed
                                    // inside `enter_field_scene` by walking opdeene's
                                    // MAN cutscene-timeline for the real `GFLAG_SET 26`
                                    // write (World::arm_prologue_handoff_from_man), so
                                    // no blind arm is needed here. The confirm-gated
                                    // transition still fires in the field tick below
                                    // (World::take_prologue_handoff).
                                    log::info!(
                                        "new game: seeded party_count={}, entered opening cutscene \
                                         '{cutscene}' (mode={mode:?})",
                                        self.session.host.world.party_count,
                                    );
                                    // The host swapped to the prologue scene:
                                    // rebuild the render-side scene state so its
                                    // geometry replaces the boot scene's.
                                    self.rebuild_scene_render_state();
                                }
                                Err(e) => log::warn!(
                                    "new game: enter opening cutscene '{cutscene}' failed ({e:#}); \
                                     staying on the pre-booted scene"
                                ),
                            }
                            self.boot_ui = BootUiState::Inactive;
                        }
                        TitleOutcome::Continue => {
                            // Open the save-select panel against `save_dir`.
                            let snapshots = scan_save_dir(&self.save_dir);
                            self.boot_ui = BootUiState::SaveSelect(
                                legaia_engine_core::save_select::SaveSelectSession::new(
                                    legaia_engine_core::save_select::SaveSelectMode::Load,
                                    snapshots,
                                ),
                            );
                        }
                        TitleOutcome::Options => {
                            self.boot_ui = BootUiState::Options(
                                legaia_engine_core::options::OptionsSession::new(
                                    self.options_state.clone(),
                                ),
                            );
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
                    match outcome {
                        SelectOutcome::Loaded(slot) => {
                            // Hydrate the world from the slot file.
                            let runtime = legaia_engine_core::menu_runtime::MenuRuntime::new(
                                self.save_dir.clone(),
                            );
                            match runtime.load_from_slot(&mut self.session.host.world, slot) {
                                Ok(p) => log::info!("loaded slot {} from {}", slot, p.display()),
                                Err(e) => log::warn!("load slot {slot} failed: {e:#}"),
                            }
                            self.boot_ui = BootUiState::Inactive;
                        }
                        SelectOutcome::Cancelled => {
                            // Back to title.
                            self.boot_ui =
                                BootUiState::Title(legaia_engine_core::title::TitleSession::new());
                        }
                        SelectOutcome::Saved(_) | SelectOutcome::Deleted(_) => {
                            // Save-select in Load mode shouldn't emit these,
                            // but degrade gracefully.
                            self.boot_ui = BootUiState::Inactive;
                        }
                    }
                }
                true
            }
            BootUiState::Options(session) => {
                use legaia_engine_core::options::{OptionsInput, OptionsOutcome};
                let input = OptionsInput {
                    up,
                    down,
                    left,
                    right,
                    cross,
                    circle,
                    start,
                };
                let _ = session.tick(input);
                if let Some(OptionsOutcome::Closed) = session.outcome() {
                    // Value edits commit inside the session's popup (retail
                    // writes the config word at popup confirm and never
                    // reverts); lift + persist the final state.
                    self.options_state = session.state().clone();
                    self.persist_and_apply_options();
                    // After options, route back to Title so the player can
                    // pick New Game / Continue (matches retail flow).
                    self.boot_ui =
                        BootUiState::Title(legaia_engine_core::title::TitleSession::new());
                }
                true
            }
            BootUiState::FieldMenu { sub } => {
                use legaia_engine_core::field_menu::{FieldMenuInput, FieldMenuOutcome};
                use legaia_engine_core::field_menu_dispatch::{
                    FieldMenuSubsession, apply_arts_outcome, apply_equip_outcome,
                    apply_inventory_outcome, apply_spell_outcome,
                };
                // The menu session is hosted by the BootSession (so headless
                // drivers share it); if it vanished out from under the UI
                // arm, drop back to the scene.
                if self.session.field_menu.is_none() {
                    self.boot_ui = BootUiState::Inactive;
                    return true;
                }
                if let Some(active_sub) = sub.as_mut() {
                    // Engine extension: Triangle on the Status screen swaps
                    // it for the Tactical Arts chain editor (retail's seven
                    // rows carry no Arts row). Consume the edge so the same
                    // press does not also drive the screen it replaced.
                    let opened_arts = legaia_engine_core::field_menu_dispatch::try_open_arts_editor(
                        active_sub,
                        pressed,
                        &self.session.host.world,
                    );
                    // A sub-session is open - route input + check for done.
                    if !opened_arts {
                        active_sub.tick_pad_edge(pressed);
                    }
                    if active_sub.is_done() {
                        // Drain into world side-effects + handle save.
                        let finished = sub.take().expect("sub was Some");
                        match finished {
                            FieldMenuSubsession::Items(s) => {
                                apply_inventory_outcome(&s.inner, &mut self.session.host.world);
                            }
                            FieldMenuSubsession::Equip { session, char_slot } => {
                                let _ = apply_equip_outcome(
                                    &session,
                                    char_slot,
                                    &mut self.session.host.world,
                                );
                            }
                            FieldMenuSubsession::Spells(s) => {
                                apply_spell_outcome(&s, &mut self.session.host.world);
                            }
                            FieldMenuSubsession::Arts(editor) => {
                                // Persist the edit back into the world's saved
                                // chains so the next battle's Arts rows reflect
                                // it: lift the live library, apply the editor
                                // outcome, store it back (World::chain_library
                                // <-> store_chain_library bridge over
                                // World::saved_chains).
                                let mut library = self.session.host.world.chain_library();
                                if apply_arts_outcome(editor, &mut library).is_ok() {
                                    self.session.host.world.store_chain_library(&library);
                                }
                            }
                            FieldMenuSubsession::Status(_) => {}
                            FieldMenuSubsession::Save(s) => {
                                use legaia_engine_core::save_select::SelectOutcome;
                                let runtime = legaia_engine_core::menu_runtime::MenuRuntime::new(
                                    self.save_dir.clone(),
                                );
                                match s.outcome() {
                                    Some(SelectOutcome::Saved(slot)) => {
                                        match runtime
                                            .save_to_slot(&mut self.session.host.world, slot)
                                        {
                                            Ok(p) => log::info!(
                                                "field menu: saved slot {} to {}",
                                                slot,
                                                p.display()
                                            ),
                                            Err(e) => log::warn!(
                                                "field menu: save slot {slot} failed: {e:#}"
                                            ),
                                        }
                                    }
                                    // The retail Load row: picking a slot
                                    // replaces the running world with the
                                    // saved one.
                                    Some(SelectOutcome::Loaded(slot)) => {
                                        match runtime
                                            .load_from_slot(&mut self.session.host.world, slot)
                                        {
                                            Ok(p) => log::info!(
                                                "field menu: loaded slot {} from {}",
                                                slot,
                                                p.display()
                                            ),
                                            Err(e) => log::warn!(
                                                "field menu: load slot {slot} failed: {e:#}"
                                            ),
                                        }
                                    }
                                    _ => {}
                                }
                            }
                            FieldMenuSubsession::Config(o) => {
                                // Edits committed inside the session's value
                                // popup (retail semantics); lift + persist.
                                self.options_state = o.state().clone();
                                self.persist_and_apply_options();
                            }
                        }
                        if let Some(menu) = self.session.field_menu.as_mut() {
                            let _ = menu.resume(false);
                        }
                    }
                    return true;
                }
                let input = FieldMenuInput {
                    up,
                    down,
                    cross,
                    circle,
                    start,
                };
                // After Cross on a row the menu phase becomes Suspended.
                // Build the matching sub-session and route control there.
                let suspended_row = match self.session.field_menu.as_mut() {
                    Some(menu) => {
                        let _ = menu.tick(input);
                        match menu.phase() {
                            legaia_engine_core::field_menu::FieldMenuPhase::Suspended { row } => {
                                Some(row)
                            }
                            _ => None,
                        }
                    }
                    None => None,
                };
                if let Some(row) = suspended_row {
                    let snapshots = scan_save_dir(&self.save_dir);
                    // Build sub-sessions from the DISC tables the boot path
                    // already installed on the world (spell table, equipment
                    // bonus table) plus the live saved-chain library - not
                    // throwaway vanilla()/new() placeholders, which ignored
                    // any randomizer/disc data and dropped Arts edits.
                    let world = &self.session.host.world;
                    let chain_library = world.chain_library();
                    *sub = Some(FieldMenuSubsession::build(
                        row,
                        world,
                        &self.options_state,
                        &snapshots,
                        &chain_library,
                        &world.spell_catalog,
                        &world.equipment_table,
                    ));
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
                        }
                    }
                }
                true
            }
            BootUiState::GameOver(session) => {
                use legaia_engine_core::game_over::{GameOverInput, GameOverOutcome};
                let input = GameOverInput { up, down, cross };
                let _ = session.tick(input);
                if let Some(outcome) = session.outcome() {
                    match outcome {
                        GameOverOutcome::Continue => {
                            let snapshots = scan_save_dir(&self.save_dir);
                            self.boot_ui = BootUiState::SaveSelect(
                                legaia_engine_core::save_select::SaveSelectSession::new(
                                    legaia_engine_core::save_select::SaveSelectMode::Load,
                                    snapshots,
                                ),
                            );
                        }
                        GameOverOutcome::Retry | GameOverOutcome::Quit => {
                            // Retry → drop to scene; Quit → back to title.
                            self.boot_ui = match outcome {
                                GameOverOutcome::Quit => BootUiState::Title(
                                    legaia_engine_core::title::TitleSession::new(),
                                ),
                                _ => BootUiState::Inactive,
                            };
                        }
                    }
                }
                true
            }
        }
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
                let (phase_id, cursor) = match s.phase() {
                    TitlePhase::FadeIn { .. } => (0, 0),
                    TitlePhase::PressStart { .. } => (1, 0),
                    TitlePhase::MainMenu { cursor } => (2, cursor),
                    TitlePhase::Done(_) => return Vec::new(),
                };
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
                use legaia_engine_core::save_select::SelectPhase;
                let rows: Vec<legaia_engine_render::SaveSelectRow<'_>> = s
                    .slots()
                    .iter()
                    .map(|snap| legaia_engine_render::SaveSelectRow {
                        label: &snap.label,
                        present: snap.present,
                        party_lv: snap.party_lv,
                        play_time_seconds: snap.play_time_seconds,
                        money: snap.money,
                        location: &snap.location,
                    })
                    .collect();
                let (stage_origin, stage_scale) = self.save_select_stage(surface_w, surface_h);
                let cursor = match s.phase() {
                    SelectPhase::Browsing { cursor } => cursor as usize,
                    SelectPhase::NowChecking { slot, .. }
                    | SelectPhase::SlotPreview { slot }
                    | SelectPhase::ConfirmOverwrite { slot, .. }
                    | SelectPhase::ConfirmDelete { slot, .. } => slot as usize,
                    SelectPhase::Done(_) => return Vec::new(),
                };
                // Always emit the base save-select chrome text (the
                // mode's title word) so it stays visible in every
                // phase. Skip the ASCII `>` cursor when the
                // sprite-based pointing-finger cursor is being emitted
                // alongside (i.e. when the save-menu atlas is loaded).
                // The confirm prompt is NOT the flat inline Yes/No:
                // retail raises it as its own centred messagebox,
                // emitted by `save_select_phase_text_draws` (text) +
                // `save_select_chrome_sprite_draws` (panels).
                let emit_text_cursor = self.save_menu.is_none();
                let mut out = legaia_engine_render::save_select_draws_for(
                    &self.font,
                    save_select_title_word(s),
                    &rows,
                    cursor,
                    None,
                    stage_origin,
                    stage_scale,
                    emit_text_cursor,
                );
                // Phase-specific overlays (NowChecking dialog text,
                // slot-info panel text / captions, confirm messagebox)
                // - shared with the field-menu Load / Save sub-screens.
                out.extend(save_select_phase_text_draws(
                    &self.font,
                    s,
                    stage_origin,
                    stage_scale,
                    self.save_menu.is_some(),
                ));
                out
            }
            BootUiState::Options(s) => {
                let rows = s.state().rows();
                let row_views: Vec<legaia_engine_render::OptionsRowView<'_>> = rows
                    .iter()
                    .map(|r| legaia_engine_render::OptionsRowView {
                        label: r.label,
                        value: r.value,
                        teal: r.teal,
                        advance: r.advance,
                    })
                    .collect();
                // The boot-UI options panel draws at a fixed pen rather
                // than the menu-overlay window rects; anchor the value
                // popup off the same pen (value column + 6).
                let popup = s.popup().map(|p| legaia_engine_render::OptionsPopupDraw {
                    rect: legaia_engine_core::options::options_popup_content_rect(
                        80,
                        96 + 146,
                        128,
                        p.row,
                        p.choices.len(),
                    ),
                    choices: p.choices,
                    cursor: p.cursor,
                });
                legaia_engine_render::options_draws_for(
                    &self.font,
                    &row_views,
                    s.cursor(),
                    popup.as_ref(),
                    (96, 80),
                )
            }
            BootUiState::FieldMenu { sub } => {
                use legaia_engine_core::field_menu_dispatch::FieldMenuSubsession;
                // The field pause menu and its sub-screens lay glyphs out
                // in 320x240 stage pixels. Route them through the shared
                // boot-UI stage so they upscale + center exactly like the
                // title art and save chrome (and stay locked to the
                // `menu_window_chrome_draws_for` frame). The Save
                // sub-session is the exception: it pre-scales to surface
                // coords (it reuses the load-screen chrome stage), so it
                // must not be scaled twice.
                let is_save_sub = matches!(sub, Some(FieldMenuSubsession::Save(_)));
                let mut draws = if let Some(active_sub) = sub {
                    // Render the active sub-session's overlay. Each branch
                    // builds the matching plain-data view + calls the
                    // shipped `*_draws_for` helper.
                    self.field_menu_sub_draws(active_sub)
                } else {
                    // The menu session lives on the BootSession (the
                    // headless host of the CARD/menu mode); the window
                    // only renders it. Command rows fill the id-50 list
                    // window, money/play-time the id-49 corner box, and
                    // the party overview the id-51 right panel (the
                    // pinned top-level window set).
                    let Some(menu) = self.session.field_menu.as_ref() else {
                        return Vec::new();
                    };
                    use legaia_asset::menu_windows::window_ids;
                    let view = menu.view();
                    let row_views: Vec<legaia_engine_render::FieldMenuRowView<'_>> = view
                        .rows
                        .iter()
                        .map(|r| legaia_engine_render::FieldMenuRowView {
                            label: r.label,
                            enabled: r.enabled,
                        })
                        .collect();
                    let mut d = legaia_engine_render::field_menu_draws_for(
                        &self.font,
                        &row_views,
                        view.cursor,
                        view.money,
                        view.play_time_seconds,
                        self.menu_window_pen(window_ids::TOP_COMMAND_LIST),
                        self.menu_window_pen(window_ids::TOP_MONEY_TIME),
                    );
                    let snaps = legaia_engine_core::field_menu_dispatch::status_snapshots(
                        &self.session.host.world,
                    );
                    let party: Vec<legaia_engine_render::FieldMenuPartyView<'_>> = snaps
                        .iter()
                        .map(|s| legaia_engine_render::FieldMenuPartyView {
                            name: &s.name,
                            level: s.level,
                            hp: s.hp,
                            hp_max: s.hp_max,
                            mp: s.mp,
                            mp_max: s.mp_max,
                            ap: s.ap as u16,
                        })
                        .collect();
                    d.extend(legaia_engine_render::field_menu_info_draws_for(
                        &self.font,
                        &party,
                        self.menu_window_pen(window_ids::TOP_INFO_PANEL),
                    ));
                    d
                };
                if !is_save_sub {
                    let (origin, scale) = self.save_select_stage(surface_w, surface_h);
                    legaia_engine_render::scale_stage_text_draws(&mut draws, origin, scale);
                }
                draws
            }
            BootUiState::GameOver(s) => legaia_engine_render::game_over_draws_for(
                &self.font,
                s.cursor(),
                s.continue_enabled,
                (96, 100),
            ),
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
        let world = &mut self.session.host.world;
        let events = world.drain_field_events();
        for ev in events {
            match ev {
                FieldEvent::ActorSpawned { slot, .. } => {
                    let has_tmd = world
                        .actors
                        .get(slot as usize)
                        .is_some_and(|a| a.tmd_ref.is_some());
                    if has_tmd {
                        self.pending_dynamic_mesh_slots.push(slot);
                    }
                }
                // `apply == 0` Camera Configure beats snap the live camera
                // globals immediately in retail. Queue them for the cutscene
                // camera interp so a snap+glide beat pair committed in ONE
                // tick (no yield between the ops) still glides FROM the
                // snapped pose - the merged `camera_state` alone only shows
                // the last beat's targets. See `pending_camera_snaps`.
                FieldEvent::CameraConfigure {
                    params,
                    apply_trigger: 0,
                    ..
                } => self.pending_camera_snaps.push(params),
                _ => {}
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

    pub(super) fn try_start_windowed_cutscene(&mut self) {
        use legaia_engine_shell::cutscene_av::{decode_str_av_from_disc, decode_str_video_only};
        let Some(fmv_id) = self.session.host.world.active_fmv() else {
            return;
        };
        let Some(rel) = self.session.host.world.active_fmv_str_filename() else {
            log::info!("cutscene: fmv_id={fmv_id} (cut/unmapped slot); skipping");
            self.session.host.world.finish_cutscene();
            return;
        };

        let decoded: Option<(Vec<legaia_mdec::VideoFrame>, std::time::Duration, _)> = if let Some(
            disc_path,
        ) =
            self.disc_path.as_ref()
        {
            match resolve_iso_file(disc_path, Path::new(&rel)) {
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
        };

        match decoded {
            Some((frames, frame_period, audio)) => {
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
            None => {
                // Drain the trigger so the field resumes next frame.
                self.session.host.world.finish_cutscene();
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
            // cursor (the A/V-sync master clock) starts with the picture. Pause
            // the scene sequencer so the cutscene track isn't layered over BGM.
            if let (Some(out), Some(track)) = (audio_out.as_ref(), c.pending_audio.take()) {
                out.set_sequencer_paused(true);
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
