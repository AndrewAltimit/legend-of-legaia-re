//! Extracted from `window.rs` (mechanical split; behavior-preserving).

use super::*;

/// One on-screen sparring-tutorial box with its stage rect `(x, y, w, h)`.
pub(super) type TutorialStageBox<'a> = (
    (i32, i32, i32, i32),
    &'a legaia_engine_core::battle_flow::ActiveTutorialBox,
);

/// Project the simulation's arts-input phase onto the presentation
/// crate's. The two enums are deliberately separate types -
/// `legaia-engine-ui` is a leaf that does not link `engine-core` - so
/// every host that draws the input screen carries this three-line map.
fn arts_input_screen(
    p: legaia_engine_core::arts_command_input::ArtsInputScreen,
) -> legaia_engine_render::arts_input::ArtsInputScreen {
    use legaia_engine_core::arts_command_input::ArtsInputScreen as Sim;
    use legaia_engine_render::arts_input::ArtsInputScreen as Ui;
    match p {
        Sim::Entering => Ui::Entering,
        Sim::Review => Ui::Review,
        Sim::Targeting => Ui::Targeting,
    }
}

/// The chip cluster a host projects for one frame: the owned `(label,
/// enabled)` chips in seat order, the cursor index, and the cluster's phase
/// (`engine-core::battle_hud::BattleCommandChips`, with the phase mapped
/// onto the leaf crate's own enum).
pub(super) type CommandChips = (
    Vec<(String, bool)>,
    usize,
    legaia_engine_render::battle_command_ui::ChipPhase,
);

impl PlayWindowApp {
    /// Keep the rendered dialog panel ([`Self::active_dialog`]) in sync with
    /// the world's pending dialog request.
    ///
    /// The world owns dismissal: the field VM's op-`0x4C` dialog-advance hook
    /// and the overworld talk-to handler both clear `World::dialog.current` on
    /// a confirm/cancel press. This method only mirrors that state into a
    /// visible, typed-out box - it opens a panel from the scene's MES the frame
    /// a request appears, ticks its typewriter reveal, and drops the panel the
    /// frame the world clears the request. It never clears `current_dialog`
    /// itself, so it can't race the world's dismiss.
    pub(super) fn sync_dialog_panel(&mut self) {
        // When the inline-script field-VM runner owns dialogue, it manages its
        // own box (rendered from `world.dialog.inline`); don't also open the
        // simplified panel.
        if self.session.host.world.toggles.use_vm_dialogue {
            self.active_dialog = None;
            return;
        }
        if self.session.host.world.dialog.current.is_none() {
            self.active_dialog = None;
            return;
        }
        if self.active_dialog.is_none()
            && let Some(mut panel) = self.session.host.open_pending_dialog()
        {
            panel.set_glyphs_per_frame(2);
            self.active_dialog = Some(panel);
        }
        if let Some(panel) = self.active_dialog.as_mut() {
            panel.tick();
        }
    }

    /// Commit the `4C E1` text balloon's font measurement while the app is
    /// still `&mut` - the tick-phase half of the balloon's measure/commit
    /// round-trip (`World::commit_text_balloon_width`).
    ///
    /// Retail measures the line inside the spawner (`FUN_8003C764` via
    /// `FUN_80035F04`) because its font metrics share its address space; the
    /// engine's font atlas is host-side, so the record's `x` stays `None`
    /// until a host measures. Doing it here, in the same mutation phase as
    /// [`Self::sync_dialog_panel`], is what lets [`Self::build_hud`] and
    /// [`Self::dialog_chrome_sprite_draws`] stay `&self`: by the time the
    /// draw passes run, the committed geometry is already on the record
    /// (`TextBalloon::pen` / `frame_rect`). The commit is idempotent, so the
    /// cheap `x.is_none()` guard is an optimisation, not a correctness gate.
    pub(super) fn sync_text_balloon(&mut self) {
        let world = &mut self.session.host.world;
        if world
            .cutscene
            .text_balloon
            .as_ref()
            .is_none_or(|b| b.x.is_some() || b.killed)
        {
            return;
        }
        let width = match world.cutscene.text_balloon.as_ref() {
            Some(b) => legaia_engine_render::text_balloon_text_width(&self.font, &b.text),
            None => return,
        };
        world.commit_text_balloon_width(width);
    }

    /// Is the field party HUD allowed to be on screen at all this frame?
    ///
    /// Retail's `_DAT_8007B868` suppress global, asked of the shared kernel
    /// [`legaia_engine_core::world_map_panel_host::field_hud_suppressed`].
    /// This host answers only the term the world cannot: a window-side panel
    /// with no `World` state behind it. The enumeration used to live here in
    /// full and a second copy lived on the browser play page, and the copies
    /// had drifted - the page's lacked all three of the host terms below, and
    /// this one gated the badge column (`FUN_801d095c`) differently again.
    pub(super) fn field_party_hud_suppressed(&self) -> bool {
        legaia_engine_core::world_map_panel_host::field_hud_suppressed(
            &self.session.host.world,
            self.boot_ui.is_active()
                || self.menu_runtime.is_open()
                || self.cutscene.is_some()
                || self.active_dialog.is_some(),
        )
    }

    /// Where the player projects on the 240-line stage this frame.
    ///
    /// Retail hands the projection the player object's position with `0x80`
    /// subtracted from Y (`addiu v0,v0,-0x80` at `0x801D0F8C`) - the torso,
    /// not the feet - and the HUD drops to its low row when the result sits
    /// above stage `y = 0x30`. `None` is retail's own staged-load path, which
    /// also forces the low row.
    fn field_hud_projected_player_y(&self) -> Option<i16> {
        let (sw, sh) = self.win.surface_size();
        if sh == 0 {
            return None;
        }
        let world = &self.session.host.world;
        let slot = world.player_actor_slot.map(usize::from).unwrap_or(0);
        let p = world
            .actors
            .get(slot)
            .filter(|a| a.active || a.tmd_binding.is_some())?;
        // The projection the 3D pass draws with, into the stage rect whose
        // NDC `y` is exactly the 240-line stage row.
        let (_, aspect) = scene_viewport_for(sw, sh);
        let in_world_map = world.mode == SceneMode::WorldMap;
        let cam = self.compute_scene_camera(aspect, in_world_map, None);
        let v = cam
            * glam::Vec4::new(
                p.move_state.world_x as f32,
                p.move_state.world_y as f32 - 128.0,
                p.move_state.world_z as f32,
                1.0,
            );
        if v.w <= 0.0 {
            return None;
        }
        // wgpu NDC has +Y up; the stage is 240 lines with +Y down.
        let stage_y: f32 = (1.0 - v.y / v.w) * 120.0;
        Some(stage_y.clamp(-4096.0, 4096.0) as i16)
    }

    /// Advance the field party HUD's countdown one frame.
    ///
    /// Runs in the mutation phase beside the other per-frame syncs; the draw
    /// pass reads the decision back off the driver.
    pub(super) fn tick_field_party_hud(&mut self) {
        // A scene change is retail's rearm condition (its own arm includes
        // "the staged scene load is still pending"), and without it the
        // stationary compare would run against the previous scene's
        // coordinates and pop the HUD up mid-transition.
        let scene = self.session.host.scene.as_ref().map(|s| s.name.clone());
        if scene != self.field_party_hud_scene {
            self.field_party_hud_scene = scene;
            self.field_party_hud.rearm();
        }
        // Retail's player-engaged rearm term: a script or conversation that
        // holds the player restarts the idle countdown every frame.
        if legaia_engine_core::world_map_panel_host::field_hud_rearm_held(&self.session.host.world)
        {
            self.field_party_hud.rearm();
        }
        // Capture harness: phase-align the countdown to the retail state
        // being compared (`LEGAIA_HUD_COUNTDOWN`).
        if let Some(sc) = self.screenshot.as_ref()
            && let Some(n) = sc.hud_countdown
        {
            let near = i32::from(self.session.host.world.mode == SceneMode::WorldMap);
            let idle = legaia_engine_vm::world_map_panel_actors::hud_idle_frames(near, false);
            if legaia_engine_core::world_map_panel_host::hud_phase_hold(
                self.tick_no,
                sc.capture_tick,
                n,
                idle,
            ) {
                self.field_party_hud.rearm();
            }
        }
        let suppressed = self.field_party_hud_suppressed();
        let projected_y = if suppressed {
            None
        } else {
            self.field_hud_projected_player_y()
        };
        let world = &self.session.host.world;
        // View mode `_DAT_800845C4`: `0` is the near field camera (0x28-frame
        // idle before the HUD returns), `1` the far overworld one (0xA0).
        let view_mode = i32::from(world.mode == SceneMode::WorldMap);
        let player_pos = world
            .player_actor_slot
            .map(usize::from)
            .and_then(|s| world.actors.get(s))
            .map(|a| (a.move_state.world_x, a.move_state.world_z));
        // The suppress mask is the PACKED d-pad, so the raw word has to be
        // converted or nothing ever suppresses. The word is the one the
        // world was handed this tick (`World::input`), as on the browser
        // page - not the window's held keys, which keep the HUD hidden while
        // the world itself was fed a neutral pad (a shop, the narration
        // crawl, a locked cutscene).
        let pad = legaia_engine_core::world_map_panel_host::packed_pad(world.input.pad());
        self.field_party_hud
            .tick(suppressed, view_mode, pad, player_pos, 1, projected_y);
    }

    /// The field party HUD's two draw halves for this frame, or empty when
    /// the kernel's decision is anything but `Draw`.
    pub(super) fn field_party_hud_draws(
        &self,
        w: u32,
        h: u32,
    ) -> legaia_engine_render::BattleHudDraws {
        use legaia_engine_render::field_party_hud as fp;
        // Ask the suppress gate on the DRAW path too, not only on the tick.
        // Retail evaluates it once because `FUN_801D0D38` is both halves in
        // one function; the port splits them, and a split decision is only
        // as fresh as the last frame on which the host reached the tick. Any
        // arm that short-circuits the frame path without short-circuiting the
        // draw then paints the kernel's last pre-suppression answer - which
        // is exactly how this readout came to sit under the pause menu.
        if self.field_party_hud_suppressed() {
            return Default::default();
        }
        let Some(legaia_engine_vm::world_map_panel_actors::HudDecision::Draw { y }) =
            self.field_party_hud.decision()
        else {
            return Default::default();
        };
        let rows = legaia_engine_core::world_map_panel_host::field_party_hud_members(
            &self.session.host.world,
        );
        let members: Vec<fp::FieldHudMember<'_>> = rows
            .iter()
            .map(|m| fp::FieldHudMember {
                name: &m.name,
                level: m.level,
                hp: m.hp,
                hp_max: m.hp_max,
                mp: m.mp,
                mp_max: m.mp_max,
                alive: m.alive,
            })
            .collect();
        let (origin, scale) = self.save_select_stage(w, h);
        fp::field_party_hud_draws_for(
            &self.font,
            &fp::FieldPartyHudFrame {
                members: &members,
                y: i32::from(y),
                chrome: self.save_menu.as_ref().map(|a| &a.rects),
                scrim_src: self.save_menu.as_ref().and_then(|a| a.solid),
                solid_src: self.battle_hud_solid_src(),
                origin,
                scale: scale as i32,
            },
        )
    }

    /// One minigame status row (dance / fishing / slots / Baka Fighter) at
    /// its pen on the shared 320x240 stage, scaled onto the surface. The
    /// browser play page composes the same rows at the same pens
    /// (`PEN_STATUS` / `PEN_PROMPT` in `play_minigames.rs`) through the stage
    /// transform; this host used to lay them out in raw surface pixels, a
    /// third the size and pinned to the window corner instead of the stage.
    fn stage_status_row(
        &self,
        text: &str,
        pen: (i32, i32),
        color: [f32; 4],
        w: u32,
        h: u32,
    ) -> Vec<TextDraw> {
        let mut d = text_draws_for(&self.font.layout_ascii(text), pen, color);
        let (stage_origin, stage_scale) = self.save_select_stage(w, h);
        legaia_engine_render::scale_stage_text_draws(&mut d, stage_origin, stage_scale);
        d
    }

    /// The engine's minigame status rows (`engine-core::minigame_status`),
    /// through the shared draw kernel and scaled onto the surface through
    /// the one stage transform the page applies to the same rows.
    fn stage_status_rows(
        &self,
        rows: &[legaia_engine_core::minigame_status::StatusRow],
        w: u32,
        h: u32,
    ) -> Vec<TextDraw> {
        let mut d = legaia_engine_render::ui_text_lines::status_row_draws_for(
            &self.font,
            rows.iter().map(|r| (r.text.as_str(), r.pen, r.bright)),
        );
        let (stage_origin, stage_scale) = self.save_select_stage(w, h);
        legaia_engine_render::scale_stage_text_draws(&mut d, stage_origin, stage_scale);
        d
    }

    pub(super) fn build_hud(&self, w: u32, h: u32) -> Vec<TextDraw> {
        let Some(atlas) = &self.font_atlas else {
            return Vec::new();
        };
        let _ = atlas;
        // Boot UI is fullscreen - when active, suppress every other HUD layer
        // and just render the active panel (title screen / save-select).
        if self.boot_ui.is_active() {
            return self.boot_ui_draws(w, h);
        }
        let white = [1.0f32, 1.0, 1.0, 1.0];
        let dim = [0.7f32, 0.85, 1.0, 1.0];
        let mut out: Vec<TextDraw> = Vec::new();
        // The shell's own diagnostic rows. Retail draws nothing here, and
        // this seat is exactly where its party readout goes
        // ([`Self::field_party_hud_draws`]), so they are off unless asked
        // for: `F1` toggles at runtime and `LEGAIA_DIAG_HUD` starts them on.
        // Everything below this block is game surface and is NOT gated.
        if self.diag_rows {
            let scene_name = self
                .session
                .host
                .scene
                .as_ref()
                .map(|s| s.name.as_str())
                .unwrap_or("(none)");
            let line1 = format!(
                "scene {}  frame {}  meshes {}",
                scene_name,
                self.session.host.world.frame,
                self.meshes.len()
            );
            let layout1 = self.font.layout_ascii(&line1);
            out.extend(text_draws_for(&layout1, (8, 8), white));
            let audio_str = if self.session.audio.is_none() {
                "no audio"
            } else if self.options_state.muted {
                "audio MUTED (F2)"
            } else {
                "audio on (F2 mutes)"
            };
            // Human-readable name for the playing track: global-pool ids join
            // the music_01 bank / debug sound-test order the curated
            // `legaia_gamedata` music table is keyed on.
            let bgm_str = self
                .session
                .bgm
                .as_ref()
                .and_then(|b| b.last_started)
                .map(
                    |id| match legaia_engine_core::music_labels::label_for_bgm_id(id) {
                        Some(label) => format!("  bgm {id}: {label}"),
                        None => format!("  bgm {id}"),
                    },
                )
                .unwrap_or_default();
            // Dynamic-lighting enhancement state (opt-in, non-retail; `I`
            // toggles; `Y` toggles the point-light/shadow sub-layer).
            let light_str = match (self.dynamic_lighting, self.dyn_shadows) {
                (true, true) => "  light+shadows ON (I/Y)",
                (true, false) => "  light ON (I) shadows off (Y)",
                (false, _) => "",
            };
            // Camera-distance preset (`T` cycles) + precise-movement toggle
            // (`R`) - the compass/zoom state, appended to the status line.
            let cam_str = format!("  cam {} (T)", self.session.camera.distance.label());
            let precise_str = if self.options_state.precise_movement {
                "  precise-move ON (R)"
            } else {
                ""
            };
            // Camera-occlusion fade is default-on; flag the non-default state
            // (`F4` toggles) so a "why is the wall solid again" session sees it.
            let occl_str = if self.occlusion_fade {
                ""
            } else {
                "  occl-fade off (F4)"
            };
            let line2 = format!(
                "t {:.1}s  {}{}{}{}{}{}  arrows=dpad Z=X drag=orbit",
                self.win.elapsed_secs(),
                audio_str,
                bgm_str,
                light_str,
                cam_str,
                precise_str,
                occl_str
            );
            let layout2 = self.font.layout_ascii(&line2);
            out.extend(text_draws_for(&layout2, (8, 26), dim));
            if let Some(ctrl) = &self.session.host.world.world_map.ctrl {
                let mode_str = if ctrl.is_top_view() {
                    "top-view"
                } else {
                    "walk"
                };
                let line3 = format!(
                    "world-map {} | cam ({},{}) az {} zoom {}",
                    mode_str, ctrl.camera_x, ctrl.camera_z, ctrl.azimuth, ctrl.zoom
                );
                let layout3 = self.font.layout_ascii(&line3);
                out.extend(text_draws_for(&layout3, (8, 44), white));
            }
        }
        // Dance minigame HUD: the running score / groove gauge / active lane,
        // the arrow the current beat calls for, and the last press judgement.
        // The three arrows are the retail pad bits (Square/Circle/Triangle).
        //
        // Withheld while the pre-song count-in is up - the shared phase
        // predicate answers that for both hosts.
        if self.session.host.world.mode == SceneMode::Dance
            && self.session.host.world.minigames.dance_status_visible()
            && let Some(g) = &self.session.host.world.minigames.dance
        {
            // Score / gauge / lane, the called arrow with the last
            // judgement, and the beat track (`FUN_801d2524`'s displayed
            // combo slot and scrolling notes) - the engine's rows, shared
            // with the browser page.
            out.extend(self.stage_status_rows(
                &legaia_engine_core::minigame_status::dance_status_rows(
                    g,
                    self.session.host.world.minigames.dance_last_judge.as_ref(),
                ),
                w,
                h,
            ));

            // The retail-coordinate HUD frame: the HUD driver's per-frame
            // list (`DanceGame::hud_draws`, FUN_801d231c) laid out at its
            // 320x240 stage positions and upscaled with the stage transform.
            // The rival-HUD gate stands in for `_DAT_8007B6D0`: raised in the
            // two versus modes, so the rivals' score boxes, gauges and beat
            // tracks draw there and nowhere else.
            {
                // Which rows, at which seats, in which pen is the engine's
                // decision (`DanceGame::hud_frame_rows`); this host only lays
                // the strings out. It used to be written out longhand here,
                // which is why the browser play page - same `DanceGame`, same
                // run - drew a plain status line instead of the frame.
                let rival_hud = g.rival_hud_visible();
                let (stage_origin, stage_scale) = self.save_select_stage(w, h);
                let mut stage_draws: Vec<TextDraw> = Vec::new();
                // With the hall's HUD page resident the frame draws as
                // retail's own quads in the screen-prim pass
                // (`dance_hud_prims`); these rows are the fallback without it.
                let rows = if self.session.host.world.minigames.dance_hud_art_staged {
                    Vec::new()
                } else {
                    g.hud_frame_rows(rival_hud)
                };
                for r in rows {
                    let ly = self.font.layout_ascii(&r.text);
                    stage_draws.extend(text_draws_for(
                        &ly,
                        (r.x, r.y),
                        if r.dim { dim } else { white },
                    ));
                }
                legaia_engine_render::scale_stage_text_draws(
                    &mut stage_draws,
                    stage_origin,
                    stage_scale,
                );
                out.extend(stage_draws);
                // The sprite-part layer: `FUN_801d387c`'s emit dispatch over
                // the run's own part pool (the sequence-clear banner + stars
                // the rules engine spawns), faded by its `+0x78` prologue.
                out.extend(legaia_engine_render::minigame_fx::dance_sprite_part_draws(
                    &self.font,
                    &minigame_fx::dance_sprite_part_views(&g.sprite_part_emits()),
                    stage_origin,
                    stage_scale,
                ));
            }

            // Disco King tutorial captions (the how-to run) and the
            // pre-song count-in banner, both through the shared
            // `legaia_engine_ui::ui_dance` builders the browser play page
            // draws them with - the caption strings are overlay rodata the
            // port does not read, so the seats are retail's and the
            // letterforms are placeholders.
            if let Some(tf) = self
                .session
                .host
                .world
                .minigames
                .dance_tutorial_frame
                .as_ref()
            {
                let (stage_origin, stage_scale) = self.save_select_stage(w, h);
                out.extend(legaia_engine_render::ui_dance::dance_tutorial_draws_for(
                    &self.font,
                    legaia_engine_render::ui_dance::DanceTutorialView {
                        captions: &tf.captions,
                        options: tf.options,
                        cursor_pos: tf.cursor_pos,
                        feedback: tf.feedback,
                    },
                    stage_origin,
                    stage_scale,
                ));
            }
        }
        // Dance pre-song count-in banner (`1 2 3 READY... GO!`): the
        // envelope's two sliding halves / held centre, faded by its
        // brightness ramp. The world owns the phase; this only projects it.
        //
        // Placeholder letterforms ONLY while the hall's own HUD page is not
        // resident. With it staged the banner is retail's textured sprite,
        // emitted as screen-space primitives in `redraw`'s prim list through
        // the same `ui_dance::dance_countin_prims` the browser play page
        // calls - so the two hosts cannot end up drawing different halves of
        // this one banner, which is exactly what happened to the battle
        // numerals.
        if !self.session.host.world.minigames.dance_hud_art_staged
            && let Some(env) = self
                .session
                .host
                .world
                .minigames
                .dance_countin_banner
                .as_ref()
        {
            let (stage_origin, stage_scale) = self.save_select_stage(w, h);
            out.extend(legaia_engine_render::ui_dance::dance_countin_draws_for(
                &self.font,
                legaia_engine_render::ui_dance::DanceCountInView {
                    x_offset: env.x_offset,
                    brightness: env.brightness,
                    hold: env.hold,
                },
                stage_origin,
                stage_scale,
            ));
        }
        // The shared minigame effect pool's live parts (fishing splash,
        // wander ripples, celebration bursts), in stage space. The pool is
        // `World::minigames.fx` and the builder is the one the browser hosts
        // draw it with - the dance's sequence banner is NOT here, because
        // the run spawns it into its own pool above.
        {
            let (stage_origin, stage_scale) = self.save_select_stage(w, h);
            out.extend(legaia_engine_render::minigame_fx::fx_part_draws(
                &self.font,
                &minigame_fx::fx_part_views(&self.session.host.world.minigames.fx),
                stage_origin,
                stage_scale,
            ));
        }
        // Fishing minigame HUD: the phase-specific line (cast-power bar while
        // casting; tension + strength while fighting; the catch result when
        // done) plus the running point total.
        if self.session.host.world.mode == SceneMode::Fishing
            && let Some(s) = &self.session.host.world.minigames.fishing
        {
            // The phase line + key hint are one engine derivation both play
            // hosts print (`PondSession::status_rows`).
            let (line, hint) = s.status_rows("Circle", "Cross", "Square");
            out.extend(self.stage_status_row(&line, (8, 62), white, w, h));
            let hint = format!("{hint}  (Triangle = menu, Start = quit, P = prizes)");
            out.extend(self.stage_status_row(&hint, (8, 80), dim, w, h));

            // The overlay's developer readout (FUN_801d2050): the wander
            // actor's tile pair + settled height, shown only when the
            // dev-menu session (the engine's `_DAT_8007B9B0` print-flag
            // stand-in) is up AND the held pad carries the modifier bit -
            // the same two-sided gate retail applies.
            if let Some(wd) = &self.session.host.world.minigames.fishing_venue.wander {
                use legaia_engine_core::fishing_actors::{
                    debug_readout_visible, debug_tile, tracked_point_separation,
                };
                let held = self.pad.rotate_right(8);
                if debug_readout_visible(self.dev_menu.is_some(), held) {
                    // Separation of the actor from the venue anchor it
                    // spawned at, in sub-cells (the overlay's tracked-point
                    // pair, with an integer sqrt for the SCUS normalise
                    // helper).
                    let sep = tracked_point_separation((0x400, 0x400), (wd.x, wd.z), |v| {
                        (v.max(0) as f64).sqrt() as i32
                    });
                    let line = format!(
                        "tile ({}, {})  y {}  facing {:#x}  sep {sep}",
                        debug_tile(wd.x),
                        debug_tile(wd.z),
                        wd.y,
                        wd.facing
                    );
                    out.extend(self.stage_status_row(&line, (8, 116), dim, w, h));
                }
            }

            // The retail persistent HUD rows (best-catch, capped point total,
            // lure label, lures remaining) at their traced stage-pixel pens,
            // through the ported layout + its draw-list consumer. The lure
            // index is the session's - the entry's ownership gate already
            // re-pointed it at an owned lure.
            let inventory = &self.session.host.world.party.inventory;
            let lure = s.lure;
            let lures_left = *inventory
                .get(&(legaia_engine_core::fishing::lure_item_id(lure) as u8))
                .unwrap_or(&0) as i32;
            let mut items = legaia_engine_render::persistent_hud_draws(
                s.record.points,
                s.record.best_points,
                lure,
                lures_left,
            );
            // The catch HUD, drawn over the persistent rows while a cast is
            // out: the length / extent / cast-power readouts, plus the depth
            // and tension gauge block once the fish is on - one engine
            // derivation (`PondSession::catch_hud`). The cast line-projection
            // term `DAT_801d9178` has no engine analogue and stays zero.
            let c = s.catch_hud();
            if c.visible {
                items.extend(legaia_engine_render::catch_hud_draws(
                    &legaia_engine_render::CatchHudState {
                        record: c.record,
                        line_extent: 0,
                        cast_power: c.cast_power,
                        depth: c.depth,
                        tension: c.tension,
                        gauges_visible: c.gauges_visible,
                    },
                ));
            }
            // This frame's live one-shot banners (hook / reel-in / miss /
            // auxiliary / strike splash), serviced in the redraw handler.
            items.extend(self.fishing_banner_draws.iter().copied());
            // No fishing sprite page is uploaded, so the glyph ids resolve
            // to nothing; the number / caption rows are font-atlas text and
            // render as-is. The gauge fills (the cast-power and depth /
            // tension bars) stretch the font atlas's solid texel - the page
            // fills the same resolved frames from its `bars` payload, and a
            // `None` here left the native gauges empty.
            let hud_atlas = legaia_engine_render::FishingHudAtlas {
                solid_src: self.battle_hud_solid_src(),
                glyph_src: &|_| None,
                bar_thickness: 8,
            };
            let mut draws = legaia_engine_render::fishing_hud_draws_for(
                &self.font,
                &items,
                &legaia_engine_render::FishingCaptions::placeholder(),
                &hud_atlas,
                (0, 0),
            );
            let (stage_origin, stage_scale) = self.save_select_stage(w, h);
            legaia_engine_render::scale_stage_text_draws(&mut draws, stage_origin, stage_scale);
            out.extend(draws);
            // The venue's hub menu / help pages / tackle list (Triangle or
            // Select on the idle shore), laid out by the engine
            // (`World::fishing_hub_lines`) and drawn through the one
            // composition the browser play page uses.
            let hub = self.session.host.world.fishing_hub_lines();
            if !hub.is_empty() {
                let mut hub_draws = legaia_engine_render::ui_fishing_hub::fishing_hub_draws_for(
                    &self.font,
                    hub.iter()
                        .map(|l| (&l.text[..], i32::from(l.x), i32::from(l.y), l.marked)),
                    white,
                );
                legaia_engine_render::scale_stage_text_draws(
                    &mut hub_draws,
                    stage_origin,
                    stage_scale,
                );
                out.extend(hub_draws);
            }
        }
        // Fishing point-exchange list: the venue's prize rows with the retail
        // gating (row 0 hidden until affordable, greyed unavailable rows,
        // one-time prizes latched after purchase).
        if self.session.host.world.mode == SceneMode::Fishing
            && let Some(ex) = &self.session.host.world.minigames.fishing_exchange
        {
            let world = &self.session.host.world;
            // The venue sub-screen's panel frame (FUN_801d74b0): the retail
            // menu-picker rect, centre-x converted to a left edge with the
            // two-left / six-down skin bias, swaying on the overlay's idle
            // sway triple (FUN_801d03b0). The list is anchored inside it.
            let sway = world.minigames.fishing_venue.sway_offset;
            let panel = legaia_engine_core::fishing_chrome::centred_panel(0xA0, 0x50, 0x68, 0x50);
            let (px, py) = panel
                .map(|p| (p.x as i32 + sway.0 as i32, p.y as i32 + sway.1 as i32))
                .unwrap_or((8, 98));
            let names: Vec<String> = ex
                .rows
                .iter()
                .map(|r| {
                    r.name
                        .clone()
                        .unwrap_or_else(|| format!("item {:#04x}", r.item_id))
                })
                .collect();
            // The screen itself is `legaia_engine_ui::ui_fishing_exchange`,
            // shared with the browser play page: the row layout, the ink
            // rule and the one-time tag are decided once. This host supplies
            // the pen (the swaying panel above), its own key legend, and the
            // live bag reads the view needs.
            use legaia_engine_render::ui_fishing_exchange as fx;
            let rows: Vec<fx::ExchangeRowView<'_>> = ex
                .rows
                .iter()
                .enumerate()
                .map(|(i, r)| {
                    let owned = *world.party.inventory.get(&r.item_id).unwrap_or(&0) as u32;
                    fx::ExchangeRowView {
                        name: names[i].as_str(),
                        price: r.price,
                        owned,
                        available: ex.is_available(
                            i,
                            world.minigames.fishing_points,
                            owned,
                            world.minigames.fishing_prizes_purchased,
                        ),
                        one_time: r.is_one_time(),
                        latched: ex.is_latched(i, world.minigames.fishing_prizes_purchased),
                    }
                })
                .collect();
            let view = fx::ExchangeView {
                venue: ex.venue as u8,
                points: world.minigames.fishing_points,
                cursor: ex.cursor,
                first_visible: ex.first_visible(world.minigames.fishing_points),
                rows: &rows,
            };
            // The pen is the retail panel rect, a 320x240 stage position, so
            // the rows scale onto the surface through the same stage the
            // HUD rows and the browser play page use. Drawn in raw surface
            // pixels they sat in the window's top-left at a third of the
            // page's size.
            let mut ex_draws = fx::exchange_screen_draws_for(
                &self.font,
                &view,
                "   (Enter = trade, Left/Right = venue, P = close)",
                (px, py),
                white,
                dim,
            );
            let (stage_origin, stage_scale) = self.save_select_stage(w, h);
            legaia_engine_render::scale_stage_text_draws(&mut ex_draws, stage_origin, stage_scale);
            out.extend(ex_draws);
        }
        // Slot-machine minigame HUD: the three payline symbols, the balance /
        // bet readout, and the phase-specific prompt.
        if self.session.host.world.mode == SceneMode::SlotMachine
            && let Some(m) = &self.session.host.world.minigames.slot_machine
        {
            out.extend(self.stage_status_rows(
                &legaia_engine_core::minigame_status::slot_status_rows(m),
                w,
                h,
            ));
        }
        // Baka Fighter minigame HUD: HP bars as numbers, round pips, the
        // last-exchange readout, and the input prompt.
        if self.session.host.world.mode == SceneMode::BakaFighter
            && let Some(f) = &self.session.host.world.minigames.baka_fighter
        {
            out.extend(self.stage_status_rows(
                &legaia_engine_core::minigame_status::baka_status_rows(f),
                w,
                h,
            ));

            // The duel's three number drawers, at their ported cell layouts:
            // the one-glyph round digit, the 8 px right-aligned score field,
            // and the 0x10 px "GET COIN" numeral strip for the prize. The
            // placement is `baka_fighter_chrome::hud_digit_placements` and
            // the glyph quads are `ui_baka_strips` - both shared with the
            // browser play page, which printed a summary line here instead.
            // The HUD widget descriptors these cells patch (`DAT_801d7160`)
            // index a sprite page no host uploads, so each cell draws as a
            // font glyph at its ported x: the layout is retail's, the glyph
            // source is not.
            // The attract card and the player select draw no duel HUD
            // (`baka_cabinet::draws_hud` is false across the front end).
            let placed = if f.cabinet().front_end() {
                Vec::new()
            } else {
                legaia_engine_core::baka_fighter_chrome::hud_digit_placements(
                    f.round() as i32,
                    f.tally().map(|t| (t.total(), t.gold_remaining())),
                )
            };
            // The placements are 320x240 stage cells, like the rows above:
            // drawn raw they sat top-left at a fraction of the page's size.
            let mut cells = legaia_engine_render::ui_baka_strips::baka_digit_strip_draws_for(
                &self.font,
                &placed,
                legaia_engine_render::ui_text_lines::STATUS_ROW_DIM_INK,
            );
            let (stage_origin, stage_scale) = self.save_select_stage(w, h);
            legaia_engine_render::scale_stage_text_draws(&mut cells, stage_origin, stage_scale);
            out.extend(cells);

            // The round chrome's resolved draws (`BakaChrome` - the intro
            // title, round banner and countdown timelines): each widget at
            // its stage position, faded by its brightness. A glyph draw
            // shows its paged cell index; the stamped cell rect
            // (`glyph_u`-paged `u` + the record's `v/w/h`) rides alongside
            // as the future atlas source.
            // Labels come from the shared kernels
            // (`baka_fighter_chrome::chrome_labels`, `ui_baka_strips`), the
            // same the browser play page draws.
            let sheet = f.cabinet().choice_sheet();
            let cabinet_cells = f.cabinet_cells();
            if !self.baka_chrome_frame.is_empty() || !cabinet_cells.is_empty() {
                let (stage_origin, stage_scale) = self.save_select_stage(w, h);
                // With the duel VRAM's HUD pages resident the widgets draw as
                // retail quads in the prim pass (`baka_hud_prims`); these
                // labels are the fallback without them.
                let quads = self.baka_hud_art_drawn();
                let draws: Vec<_> = self.baka_chrome_frame.iter().map(|(d, _)| *d).collect();
                let mut cd = if quads {
                    Vec::new()
                } else {
                    legaia_engine_render::ui_baka_strips::baka_widget_label_draws_for(
                        &self.font,
                        &legaia_engine_core::baka_fighter_chrome::chrome_labels(&draws),
                        [1.0; 4],
                    )
                };
                // The cabinet's own widgets (`FUN_801CF388`): the attract
                // prompt, the "PLAYER SELECT" banner and the "NEXT GAME /
                // PAY OUT" sheet, plus the sheet's pot numeral off the live
                // accumulator.
                use legaia_engine_core::baka_cabinet as bcab;
                if !quads {
                    cd.extend(
                        legaia_engine_render::ui_baka_strips::baka_widget_label_draws_for(
                            &self.font,
                            &bcab::choice_sheet_labels(&cabinet_cells),
                            [1.0; 4],
                        ),
                    );
                }
                if sheet.is_some() {
                    cd.extend(
                        legaia_engine_render::ui_baka_strips::baka_digit_strip_draws_for(
                            &self.font,
                            &bcab::choice_pot_placements(
                                self.session.host.world.minigames.winnings,
                            ),
                            [1.0; 4],
                        ),
                    );
                }
                legaia_engine_render::scale_stage_text_draws(&mut cd, stage_origin, stage_scale);
                out.extend(cd);
            }
        }
        // Muscle Dome leg: the battle HUD's text half - the status plate and
        // the command cluster's labels - off the same builders a battle
        // draws, fed by the dome session's command flow (the sprite half
        // rides `battle_chrome_sprite_draws`). The browser play page makes
        // the same two calls.
        if self.session.host.world.mode == SceneMode::MuscleDome && self.dome_battle_chrome_up() {
            out.extend(self.battle_hud_frame_draws(w, h).text);
            if let Some((chips, cursor, phase)) = self.battle_command_menu_chips() {
                use legaia_engine_render::battle_command_ui as bcu;
                let (origin, scale) = self.save_select_stage(w, h);
                let views = bcu::command_chip_views(&chips);
                out.extend(bcu::battle_command_chip_text(
                    &self.font,
                    &bcu::BattleCommandMenuFrame {
                        chips: &views,
                        cursor: Some(cursor),
                        phase,
                    },
                    origin,
                    scale,
                ));
            }
        }
        // Muscle Dome rows (`minigame_status`): the Ra-Seru list stand-in and
        // the between-turn / decided prompts. They also carry why the retail
        // "Turns Left / HP Left" strip is not among them.
        if self.session.host.world.mode == SceneMode::MuscleDome {
            out.extend(self.stage_status_rows(
                &legaia_engine_core::minigame_status::muscle_status_rows(&self.session.host.world),
                w,
                h,
            ));
        }
        // Shop / inn / prize / coin-counter overlay group, scaled through the
        // one stage transform both hosts share. Built by its own `&self`
        // method so the sprite pass can size the frame around it.
        let mut stage = self.shop_overlay_stage_draws();
        if !stage.is_empty() {
            // The shared kernel by its own name rather than through the
            // `save_select_stage` wrapper, so `check-ui-host-drift.py` can
            // pin this composition against the browser page's: this is the
            // only such call in `build_hud`, and losing it fails the gate.
            let (stage_origin, stage_scale) =
                legaia_engine_render::pause_menu::stage_transform(w, h);
            legaia_engine_render::scale_stage_text_draws(&mut stage, stage_origin, stage_scale);
            out.extend(stage);
        }
        // Battle-event log: the engine's own typed battle stream
        // (`Pose(...)`, `RecomputeBattleOrder`, per-strike `slot N -M HP`)
        // rendered along the right edge, most recent at the bottom. It is a
        // **diagnostic** surface - retail draws no such column, and painting
        // it over the dialog box is what made a live battle unreadable - so
        // it rides the shared `LEGAIA_DIAG_HUD` toggle with the rest of the
        // debug readout and is off by default. The ring itself keeps
        // filling either way, so a probe can turn it on mid-session.
        if !self.battle_event_log.is_empty() && legaia_engine_render::diag_hud_enabled() {
            let log_color = [1.0f32, 0.95, 0.7, 1.0];
            let line_height = 14;
            let bottom_y = 280;
            let n = self.battle_event_log.len();
            for (i, line) in self.battle_event_log.iter().enumerate() {
                let layout = self.font.layout_ascii(line);
                let y = bottom_y - ((n - 1 - i) as i32) * line_height;
                out.extend(text_draws_for(&layout, (220, y), log_color));
            }
        }
        // Battle HUD: party + monster HP plus, when the battle is
        // player-driven, the live command menu / target cursor. Only drawn in
        // SceneMode::Battle; harmless when the live loop is off (it just never
        // enters battle).
        if self.session.host.world.mode == SceneMode::Battle {
            use legaia_engine_core::battle_input::CommandPhase;
            use legaia_engine_core::target_picker::{CursorRow, PickerState};
            let bw = &self.session.host.world;
            // Greyed-out row tint, used by the target lists in the Arts /
            // Magic / Item submenus below for a K.O.'d target.
            let down_color = [0.6f32, 0.6, 0.6, 1.0];

            // The retail party strip (one full-width lozenge per live member
            // across the stage bottom), the top-left plaque and the floating
            // popups all come from the shared builder. Its text half lands
            // here; its chrome sprites ride `battle_chrome_sprite_draws` in
            // the system-UI atlas slot. Numerals carry the ported retail
            // readout-tint law (`hp_bar_color_index` / `mp_bar_color_index`,
            // FUN_800349EC / FUN_80035EA8). Rows are fed from the `BattleHud`
            // model, refreshed each tick by `sync_battle_hud_rows`.
            out.extend(self.battle_hud_frame_draws(w, h).text);

            // Encounter-transition banner: centred "ENCOUNTER!" over the
            // formation label, shown for the opening frames of the battle.
            // Armed once per Field -> Battle edge by `sync_battle_render`,
            // aged in `drain_and_log_battle_events`. A port invention with no
            // retail counterpart - retail's Field -> Battle edge draws no
            // banner at all - so it is gated off by default and only appears
            // under `LEGAIA_DIAG_HUD` (`encounter_banner_enabled`).
            if let Some((_, label)) = self
                .encounter_banner
                .as_ref()
                .filter(|_| legaia_engine_core::battle_hud::encounter_banner_enabled())
            {
                let head_w = self.font.layout_ascii("ENCOUNTER!").advance_x as i32;
                let pen = ((w as i32 - head_w) / 2, h as i32 / 4);
                out.extend(encounter_banner_draws_for(&self.font, label, pen));
            }

            // Player-driven submenus (opened from the Arts / Magic / Item
            // commands). Each parks both the SM and the command session while
            // open, so it takes priority over the command menu.
            //
            // While an in-battle dialogue box owns the frame (the tutorial
            // text; the battle tick parks the SM and the camera holds the
            // dialogue close-up), the menus are hidden - retail shows no
            // command chrome under the tutorial box.
            let dialogue_up = bw.dialogue_owns_input();
            if dialogue_up {
                // Dialogue box up: no menu chrome.
            } else if bw.arts_input_view().is_some() {
                // Retail-model arts entry: the whole screen is baked art,
                // drawn in the sprite layer by
                // `arts_input_chrome_sprite_draws`, so it puts up no text.
                // The Begin | Reselect pick is the party's commit confirm
                // (the command-chip cluster), not a line of this screen.
            } else if let Some(arts) = &bw.battle.arts_menu {
                use legaia_engine_core::battle_arts::ArtsPhase;
                let menu_x = 8i32;
                let mut my = 210i32;
                match &arts.phase {
                    ArtsPhase::Select { cursor } => {
                        let header = format!("P{} - arts:", arts.actor + 1);
                        out.extend(text_draws_for(
                            &self.font.layout_ascii(&header),
                            (menu_x, my),
                            white,
                        ));
                        my += 16;
                        if arts.arts.is_empty() {
                            out.extend(text_draws_for(
                                &self.font.layout_ascii("  (no saved arts)"),
                                (menu_x + 8, my),
                                down_color,
                            ));
                        }
                        for (i, row) in arts.arts.iter().enumerate() {
                            let sel = i as u8 == *cursor;
                            let marker = if sel { ">" } else { " " };
                            let line = match (row.miracle, row.super_art) {
                                (Some(name), _) => {
                                    format!("{} {} x{} *{}*", marker, row.name, row.hits(), name)
                                }
                                (None, Some(name)) => {
                                    format!("{} {} x{} <{}>", marker, row.name, row.hits(), name)
                                }
                                (None, None) => format!("{} {} x{}", marker, row.name, row.hits()),
                            };
                            let color = if sel { white } else { dim };
                            out.extend(text_draws_for(
                                &self.font.layout_ascii(&line),
                                (menu_x + 8, my),
                                color,
                            ));
                            my += 14;
                        }
                    }
                    ArtsPhase::Targeting { picker, .. } => {
                        // Enemy cursor: the retail dedup name strip
                        // (FUN_801D9D3C rows + layout). Ally / sweep states
                        // keep the text line.
                        if let Some(strip) = self.enemy_target_strip_draws(picker, w, h) {
                            out.extend(strip);
                        } else {
                            let line = match picker.state() {
                                PickerState::Cursor {
                                    row: CursorRow::Ally,
                                    slot,
                                } => format!("art -> target P{}", slot + 1),
                                _ => "art -> select target".to_string(),
                            };
                            out.extend(text_draws_for(
                                &self.font.layout_ascii(&line),
                                (menu_x, my),
                                white,
                            ));
                        }
                        my += 14;
                        out.extend(text_draws_for(
                            &self
                                .font
                                .layout_ascii("Left/Right=move  Cross=confirm  Circle=back"),
                            (menu_x, my),
                            dim,
                        ));
                    }
                    _ => {}
                }
            } else if let Some(spell) = &bw.battle.spell_menu {
                use legaia_engine_core::battle_magic::SpellPhase;
                let menu_x = 8i32;
                let mut my = 210i32;
                match &spell.phase {
                    SpellPhase::Select { cursor } => {
                        let header = format!("P{} - magic:", spell.actor + 1);
                        out.extend(text_draws_for(
                            &self.font.layout_ascii(&header),
                            (menu_x, my),
                            white,
                        ));
                        my += 16;
                        if spell.spells.is_empty() {
                            out.extend(text_draws_for(
                                &self.font.layout_ascii("  (no spells)"),
                                (menu_x + 8, my),
                                down_color,
                            ));
                        }
                        for (i, row) in spell.spells.iter().enumerate() {
                            let sel = i as u8 == *cursor;
                            let marker = if sel { ">" } else { " " };
                            let line = format!("{} {} {:>2}MP", marker, row.name, row.mp_cost);
                            let color = if !row.affordable {
                                down_color
                            } else if sel {
                                white
                            } else {
                                dim
                            };
                            out.extend(text_draws_for(
                                &self.font.layout_ascii(&line),
                                (menu_x + 8, my),
                                color,
                            ));
                            my += 14;
                        }
                    }
                    SpellPhase::Targeting { picker, .. } => {
                        if let Some(strip) = self.enemy_target_strip_draws(picker, w, h) {
                            out.extend(strip);
                        } else {
                            let line = match picker.state() {
                                PickerState::Cursor {
                                    row: CursorRow::Ally,
                                    slot,
                                } => format!("cast -> target P{}", slot + 1),
                                _ => "cast -> select target".to_string(),
                            };
                            out.extend(text_draws_for(
                                &self.font.layout_ascii(&line),
                                (menu_x, my),
                                white,
                            ));
                        }
                        my += 14;
                        out.extend(text_draws_for(
                            &self
                                .font
                                .layout_ascii("Left/Right=move  Cross=confirm  Circle=back"),
                            (menu_x, my),
                            dim,
                        ));
                    }
                    _ => {}
                }
            } else if bw.battle.item_menu.is_some() {
                // Retail's item window (state 0x3C): the packet-pinned list
                // + description windows with breadcrumbs and the hand
                // cursor. Text half here; the window chrome + hand ride the
                // sprite layer (`battle_chrome_sprite_draws`).
                if let Some(model) = self.battle_item_menu_model() {
                    let (origin, scale) = self.save_select_stage(w, h);
                    out.extend(with_battle_item_frame(&model, |frame| {
                        legaia_engine_render::battle_item_ui::battle_item_window_text(
                            &self.font, frame, origin, scale,
                        )
                    }));
                }
            } else if let Some(cmd) = &bw.battle.command {
                let menu_x = 8i32;
                let mut my = 210i32;
                match &cmd.phase {
                    CommandPhase::RoundPrompt { .. }
                    | CommandPhase::Menu { .. }
                    | CommandPhase::AttackMode { .. }
                    | CommandPhase::CommitConfirm { .. } => {
                        // Retail's command surfaces are clusters of framed
                        // chips around a D-pad glyph, not lists: the
                        // round-open `Begin | Run` pair, the packet-pinned
                        // four-arm diamond at `(228, 70)`, and the
                        // `Auto | Command` pair that re-uses the diamond's
                        // own left / right arms. Labels ride the shared
                        // builder's left-aligned interior pen, and a
                        // command that cannot be chosen keeps its chip and
                        // draws a single `-`. The plates themselves go out
                        // in the sprite layer
                        // (`battle_chrome_sprite_draws`).
                        if let Some((chips, cursor, phase)) = self.battle_command_menu_chips() {
                            use legaia_engine_render::battle_command_ui as bcu;
                            let (origin, scale) = self.save_select_stage(w, h);
                            let views = bcu::command_chip_views(&chips);
                            out.extend(bcu::battle_command_chip_text(
                                &self.font,
                                &bcu::BattleCommandMenuFrame {
                                    chips: &views,
                                    cursor: Some(cursor),
                                    phase,
                                },
                                origin,
                                scale,
                            ));
                        }
                    }
                    CommandPhase::Targeting { command, picker } => {
                        if let Some(strip) = self.enemy_target_strip_draws(picker, w, h) {
                            out.extend(strip);
                        } else {
                            let line = match picker.state() {
                                PickerState::Cursor {
                                    row: CursorRow::Ally,
                                    slot,
                                } => format!("{} -> target P{}", command.label(), slot + 1),
                                _ => format!("{} -> select target", command.label()),
                            };
                            out.extend(text_draws_for(
                                &self.font.layout_ascii(&line),
                                (menu_x, my),
                                white,
                            ));
                        }
                        my += 14;
                        let hint = "Left/Right=move  Cross=confirm  Circle=back";
                        out.extend(text_draws_for(
                            &self.font.layout_ascii(hint),
                            (menu_x, my),
                            dim,
                        ));
                    }
                    _ => {}
                }
            }

            // Sparring-tutorial prompt box. `FUN_801F747C` measures the prompt
            // and registers a text actor with a full rect, so this is a sized
            // window, not loose text: the shared builder lays the rows out at
            // the rect origin and the sprite layer
            // (`battle_tutorial_chrome_sprite_draws`) frames it.
            //
            // Unlike the rest of this battle HUD - which is authored in
            // surface pixels - the tutorial rect is in retail's 320x240 stage
            // space, so it goes through the stage transform the dialog box and
            // window chrome use. Drawn last inside the battle block so it sits
            // over the menus, which is where retail's message box lands too.
            //
            // Every box of the on-screen group draws, not just the front
            // one: a retail hook dispatch registers all its boxes at once
            // (the lesson's top-anchored intro and its bottom-anchored
            // explainer share the frame at `Begin | Run`).
            let (stage_origin, stage_scale) = self.save_select_stage(w, h);
            for (rect, tbox) in self.battle_tutorial_stage_boxes() {
                let mut draws = legaia_engine_render::battle_tutorial_text_draws_for(
                    &self.font, &tbox.text, rect,
                );
                // Without the system-UI atlas there is no frame and no advance
                // hand, so keep a plain confirm hint as the only affordance a
                // waiting box would otherwise have.
                if tbox.waits_for_input && self.save_menu.is_none() {
                    let lines = tbox.text.lines().count() as i32;
                    draws.extend(text_draws_for(
                        &self.font.layout_ascii("Cross=continue"),
                        (rect.0, rect.1 + lines * 14),
                        dim,
                    ));
                }
                legaia_engine_render::scale_stage_text_draws(&mut draws, stage_origin, stage_scale);
                out.extend(draws);
            }
            // Koru's timed-fight strip (`Turns Left / HP Left`): a stage-space
            // text actor like the tutorial box, framed in the same layer by
            // `battle_tutorial_chrome_sprite_draws`.
            if let Some(strip) =
                legaia_engine_core::timed_fight::timed_fight_strip(&self.session.host.world)
            {
                let mut draws = legaia_engine_render::timed_fight_strip_text_draws(
                    &self.font,
                    &legaia_engine_render::TimedFightStripView {
                        label: &strip.label,
                        turns_left: strip.turns_left,
                        hp_left: strip.hp_left,
                    },
                );
                legaia_engine_render::scale_stage_text_draws(&mut draws, stage_origin, stage_scale);
                out.extend(draws);
            }
        }
        // Level-up + Seru-capture messages. Both take retail's own
        // top-of-screen banner - the widget the `noa_levelup_banner` capture
        // pinned - rather than a loose pen in the corner.
        //
        // Two draw paths, mutually exclusive by mode: inside battle
        // `battle_hud_draws_for` emits the banner (and yields the plaque's
        // seat to it); outside, the same builders run here, because the port
        // raises both messages a mode-tick after the fight has already
        // returned to the field.
        // Without the system-UI atlas there is no frame to put a message in,
        // so a chrome-less host keeps the original loose pens.
        let banner_message = self
            .battle_banner_message()
            .filter(|_| self.save_menu.is_some());
        match &banner_message {
            // In battle `battle_hud_draws_for` already emitted both halves.
            Some(_) if self.session.host.world.mode == SceneMode::Battle => {}
            Some(message) => {
                let (stage_origin, stage_scale) = self.save_select_stage(w, h);
                let mut rows =
                    legaia_engine_render::battle_hud_chrome::message_banner_text_draws_for(
                        &self.font, message,
                    );
                legaia_engine_render::scale_stage_text_draws(&mut rows, stage_origin, stage_scale);
                out.extend(rows);
            }
            None => {
                if let Some(banner) = &self.session.host.world.party.current_level_up_banner {
                    out.extend(level_up_draws_for(
                        &self.font,
                        banner.char_id,
                        banner.new_level,
                        banner.hp_gained,
                        banner.mp_gained,
                        LEVEL_UP_BANNER_PEN,
                    ));
                }
                if let Some(banner) = &self.session.host.world.party.current_capture_banner
                    && let Some(text) = banner.current_banner()
                {
                    out.extend(capture_banner_draws_for(
                        &self.font,
                        &text,
                        CAPTURE_BANNER_PEN,
                    ));
                }
            }
        }
        // Opening-cutscene narration: the retail bottom-up subtitle CRAWL
        // (`FUN_80037174`) - every visible line centred at its current window
        // Y, scrolling upward - and the static title card (`map01`'s
        // "twilight of humanity" beat). Both are laid out in retail's 320x240
        // stage by the shared `cutscene_text_stage_draws` and upscaled with
        // the stage transform the rest of the stage-space text uses, so the
        // glyphs are stage-sized and the rows sit 16 stage lines apart on
        // every window size (scaling only the row Y drew 1x glyphs at ~3x the
        // pitch).
        {
            let world = &self.session.host.world;
            let lines = world
                .cutscene
                .narration
                .as_ref()
                .map(|n| n.visible_lines())
                .unwrap_or_default();
            let crawl: Vec<(&str, i32)> = lines.iter().map(|l| (l.text, l.y)).collect();
            let card: Vec<&str> = world
                .cutscene
                .card
                .iter()
                .flatten()
                .map(String::as_str)
                .collect();
            if !crawl.is_empty() || !card.is_empty() {
                let mut draws = legaia_engine_render::cutscene_text_stage_draws(
                    &self.font,
                    &crawl,
                    &card,
                    [1.0, 1.0, 1.0, 1.0],
                );
                let (stage_origin, stage_scale) = self.save_select_stage(w, h);
                legaia_engine_render::scale_stage_text_draws(&mut draws, stage_origin, stage_scale);
                out.extend(draws);
            }
        }
        // Name-entry overlay: the opening `town01` lead-character naming
        // prompt, laid out in stage pixels at the retail-traced geometry
        // and upscaled with the same stage transform the window chrome
        // uses (`name_entry_chrome_sprite_draws`) so text and frames stay
        // locked together.
        if let Some(entry) = &self.session.host.world.party.name_entry {
            let view = self.name_entry_view(entry);
            let mut draws = legaia_engine_render::name_entry_draws_for(&self.font, &view);
            let (stage_origin, stage_scale) = self.save_select_stage(w, h);
            legaia_engine_render::scale_stage_text_draws(&mut draws, stage_origin, stage_scale);
            out.extend(draws);
        }
        // Dialog box text: the active NPC / event message (simplified
        // panel, cutscene-timeline segment, or the inline-script
        // field-VM runner - `dialog_snapshot` picks whichever is
        // live). Laid out in stage pixels inside the retail box rect
        // computed by `dialog_stage_layout`, then upscaled with the
        // same stage transform the window chrome uses so text and
        // frame stay locked together. The chrome itself is emitted in
        // the sprite layer (`dialog_chrome_sprite_draws`).
        if let Some(snap) = self.dialog_snapshot() {
            let lay = Self::dialog_stage_layout(&snap);
            let (stage_origin, stage_scale) = self.save_select_stage(w, h);
            let has_chrome = self.save_menu.is_some();
            let (bx, by, _, _) = lay.main;
            // Main text: the pager's row window - one row per
            // 0x7C-separated line at the retail 15-px pitch from the box
            // origin, offset by the window's scroll and clipped to the rows
            // band (`FUN_801D84D0` draws `FUN_80036888(row, 0, 0, ctx+0x12,
            // ctx+0x14 + (scroll >> 4) + i*0xF)` in the staged CLUT-7 menu
            // white). The shared builder is the browser page's too.
            let mut draws: Vec<TextDraw> = legaia_engine_render::dialog_reading_box_text_draws_for(
                &self.font,
                &snap.page,
                (bx, by),
                snap.scroll_px,
                snap.box_rows,
            );
            // Option-picker labels: retail draws them CLUT-7 white at
            // `box_x + 0x10`, 15-px pitch from the box origin row; the
            // pointing-hand sprite (drawn in the chrome layer) marks the
            // selection. Keep a text `>` marker only when the chrome
            // atlas is missing.
            if let Some((px, py, _, _)) = lay.picker {
                for (i, opt) in snap.options.iter().enumerate() {
                    let selected = i == snap.cursor;
                    let label = if has_chrome {
                        opt.clone()
                    } else {
                        format!("{}{}", if selected { "> " } else { "  " }, opt)
                    };
                    let row_layout = self.font.layout_ascii(&label);
                    let pen = (px + 0x10, py + i as i32 * 0xF);
                    let color = if selected || has_chrome {
                        legaia_engine_render::MENU_TEXT_WHITE
                    } else {
                        [0.8, 0.85, 1.0, 1.0]
                    };
                    draws.extend(text_draws_for(&row_layout, pen, color));
                }
            }
            legaia_engine_render::scale_stage_text_draws(&mut draws, stage_origin, stage_scale);
            out.extend(draws);
        }
        // The `4C E1` text balloon: the single line at the retail pen -
        // centred on the full 320-px screen, not on its own frame (retail's
        // `FUN_80036888(text, 0, 0, x, y)`; a wide line overhangs the frame,
        // and both halves are retail). `text_balloon_drawing` gates out the
        // startup band; the pen is `Some` because `sync_text_balloon`
        // committed the measurement in the tick phase. The chrome frame is
        // emitted in the sprite layer (`dialog_chrome_sprite_draws`), like
        // the reading box's.
        if let Some(text) = self.session.host.world.text_balloon_drawing()
            && let Some(pen) = self
                .session
                .host
                .world
                .cutscene
                .text_balloon
                .as_ref()
                .and_then(|b| b.pen())
        {
            let (stage_origin, stage_scale) = self.save_select_stage(w, h);
            let mut draws =
                legaia_engine_render::text_balloon_text_draws_for(&self.font, text, pen);
            legaia_engine_render::scale_stage_text_draws(&mut draws, stage_origin, stage_scale);
            out.extend(draws);
        }
        // The tile board's quit prompt (walk-SM state 5): the title and the
        // two rows, read off the field overlay's own strings.
        if let Some(lay) = self.tile_board_prompt_layout()
            && let Some(lines) = self.session.host.tile_board_prompt_lines()
        {
            let (stage_origin, stage_scale) = self.save_select_stage(w, h);
            let mut draws =
                legaia_engine_render::tile_board_prompt_text_draws_for(&self.font, &lay, &lines);
            legaia_engine_render::scale_stage_text_draws(&mut draws, stage_origin, stage_scale);
            out.extend(draws);
        }
        // The Incense wear-off notice (`FUN_801F1E48`): one line of the field
        // overlay's own text.
        if let Some(line) = self.session.host.incense_notice_line() {
            let (stage_origin, stage_scale) = self.save_select_stage(w, h);
            let mut draws = legaia_engine_render::incense_notice_text_draws_for(
                &self.font,
                legaia_engine_core::incense_notice::notice_text_pen(),
                &line,
            );
            legaia_engine_render::scale_stage_text_draws(&mut draws, stage_origin, stage_scale);
            out.extend(draws);
        }
        // Opt-in developer menu: its row list draws over everything else.
        //
        // Through the canonical 320x240 stage, like every other retail screen
        // the window composes. `DEV_MENU_PEN` / `DEV_RECORDS_PEN` are retail
        // framebuffer coords, so drawing them raw pinned the whole overlay
        // into the top-left ninth of a 960x720 window while the browser play
        // page - which has always scaled them - filled the stage.
        if !self.dev_menu_draws.is_empty() {
            let (stage_origin, stage_scale) = self.save_select_stage(w, h);
            let mut dev = self.dev_menu_draws.clone();
            legaia_engine_render::scale_stage_text_draws(&mut dev, stage_origin, stage_scale);
            out.extend(dev);
        }
        out
    }

    /// The shop / inn / prize / coin-counter overlay group, in the retail
    /// 320x240 **stage** rather than in surface pixels.
    ///
    /// Held apart from `build_hud`'s surface-pixel rows because every builder
    /// below places in stage coordinates; drawing it straight into the HUD
    /// list left the whole shop UI at a third of its size in a 960x720 window
    /// while the browser play page scaled the same builders' output. The
    /// pinned `SHOP_OVERLAY_PEN` / `play_shop::SHOP_PEN` pair could not see
    /// that, because the split was in the transform and not the pen.
    ///
    /// It is a `&self` method, and that is the point: the window's chrome is
    /// a separate `&self` sprite pass, so the frame around this panel can
    /// only be sized by a builder both passes can call. See
    /// [`Self::shop_overlay_chrome_sprite_draws`].
    pub(super) fn shop_overlay_stage_draws(&self) -> Vec<TextDraw> {
        let mut stage: Vec<TextDraw> = Vec::new();
        let white = [1.0f32, 1.0, 1.0, 1.0];
        // Casino coin counter (op-0x49 sub-6): the submode screen's digit
        // entry, drawn off the world's live counter cells whenever the
        // screen is open on the coin slot. Not a menu-runtime state - the
        // field VM owns the park.
        stage.extend(self.coin_counter_window_draws());
        // The field floor window (op-0x49 sub-op 4, handler slot 0x23 - the Uru Mais
        // warp pads): laid out by the engine off the live picker state, the
        // legend read off the field overlay, drawn through the shared line
        // composition the browser play page uses.
        let mut floor = self.session.host.flag_window_lines();
        // The code lock (op-0x49 sub-op 2, handler slot 0x21 - doman's
        // password door): header off the field overlay, one letter per
        // entered symbol, through the same line composition.
        floor.extend(self.session.host.code_lock_lines());
        stage.extend(legaia_engine_render::ui_text_lines::text_line_draws_for(
            &self.font,
            floor
                .iter()
                .map(|l| (&l.text[..], i32::from(l.x), i32::from(l.y), l.marked)),
            white,
            legaia_engine_render::ui_text_lines::FLOOR_WINDOW_MARKED_INK,
        ));
        // Shop / inn overlay: rendered at the bottom of the screen when the menu
        // runtime is in any shop, inn, or confirmation state.
        if self.menu_runtime.is_open() {
            let label = self.menu_runtime.current_label();
            // Casino prize exchange: the session runs outside the MenuState
            // graph, so it is checked before the shop states. Windows
            // 43/44/45/46 through the shared engine-ui composition.
            if let Some(session) = &self.menu_runtime.prize_session {
                stage.extend(self.prize_window_draws(session));
            }
            if let Some(shop) = &self.menu_runtime.shop_session {
                let state = MenuState::from_byte(self.menu_runtime.ctx_state());
                let cursor = self.menu_runtime.cursor() as usize;
                let gold = self.session.host.world.party.money;
                // The seru-trade screens carry dynamic, owned-string labels, so
                // render them directly (the generic `(title, rows)` path below
                // only handles `'static` labels).
                let trade_state = matches!(
                    state,
                    Some(MenuState::ShopTrade) | Some(MenuState::ShopTradeConfirm)
                );
                if trade_state {
                    self.draw_shop_trade(&mut stage, state, cursor);
                }
                // Row labels are owned so item names can be resolved from the
                // disc item table; the ink is the retail `_DAT_8007B454` pen
                // from the menu-overlay window kernels.
                let bag = MenuRuntime::inventory_items(&self.session.host.world);
                let item_label = |id: u8| -> String { self.session.host.world.menu.item_label(id) };
                let held_of = |id: u8| -> i16 {
                    bag.iter()
                        .find(|(i, _)| *i == id)
                        .map(|(_, q)| *q as i16)
                        .unwrap_or(0)
                };
                let (title, rows_spec, show_gold): (_, Vec<(String, Option<u32>, u8)>, _) =
                    match state {
                        _ if trade_state => (label, Vec::new(), None),
                        // Top picker: Buy / Sell / (Trade) / Exit - labels and
                        // retail's bag-scan ink from
                        // `menu_runtime::shop_root_labels`, the page's call too.
                        Some(MenuState::ShopMenu) => {
                            let rows = legaia_engine_core::menu_runtime::shop_root_labels(
                                self.session.host.world.seru_trade_enabled(),
                                !bag.is_empty(),
                            )
                            .into_iter()
                            .map(|(l, i)| (l.to_string(), None, i))
                            .collect();
                            (label, rows, Some(gold))
                        }
                        Some(MenuState::ShopBuy) => {
                            let rows = shop
                                .inventory
                                .items
                                .iter()
                                .enumerate()
                                .map(|(row, item)| {
                                    // FUN_80032A44's buy-row arm: the hoisted band keeps
                                    // its featured pen even when dim.
                                    let ink = legaia_engine_core::shop::shop_buy_row_ink(
                                        row < shop.inventory.featured_rows,
                                        held_of(item.item_id),
                                        gold,
                                        item.price as i32,
                                        false,
                                    );
                                    (item_label(item.item_id), Some(item.price), ink)
                                })
                                .collect();
                            (label, rows, Some(gold))
                        }
                        // Retail's sell list is the price-gated slot walk,
                        // not the id-sorted bag: an unsellable row dims and
                        // sorts last (`MenuRuntime::sell_list_rows`). Twin of
                        // the browser page's arm in `web-viewer::play_shop`.
                        Some(MenuState::ShopSell) => {
                            let rows =
                                legaia_engine_core::menu_runtime::MenuRuntime::sell_list_rows(
                                    &self.session.host.world,
                                )
                                .iter()
                                .map(|r| {
                                    (
                                        format!("{} x{}", item_label(r.id), r.count),
                                        None,
                                        if r.dim {
                                            legaia_engine_render::SHOP_INK_GREY
                                        } else {
                                            legaia_engine_render::SHOP_INK_NORMAL
                                        },
                                    )
                                })
                                .collect();
                            (label, rows, Some(gold))
                        }
                        Some(MenuState::ShopQuantity) => {
                            // Retail's quantity screen has no list: one number
                            // steps in place inside window 35 / 37 while the
                            // list it came from stays parked behind it, so this
                            // screen contributes a title and no rows. The
                            // window is drawn in `shop_windows`. Twin of the
                            // browser page's arm in `web-viewer::play_shop`.
                            (label, Vec::new(), None)
                        }
                        Some(MenuState::ShopConfirm) => {
                            let rows = vec![
                                (
                                    "Yes".to_string(),
                                    None,
                                    legaia_engine_render::SHOP_INK_NORMAL,
                                ),
                                (
                                    "No".to_string(),
                                    None,
                                    legaia_engine_render::SHOP_INK_NORMAL,
                                ),
                            ];
                            (label, rows, Some(gold))
                        }
                        _ => (label, Vec::new(), None),
                    };
                // The retail descriptor windows for this phase - vendor
                // plate, purse, item info, sell quantity - each painted by
                // dispatching on its descriptor's `renderer_va`
                // (`window/shop_windows.rs`). Empty without a disc table.
                // The purse window is the retail gold readout, so the
                // engine panel below drops its own footer whenever it draws.
                let retail_windows = self.shop_window_draws(shop, state, cursor);
                let show_gold = if retail_windows.is_empty() {
                    show_gold
                } else {
                    None
                };
                stage.extend(retail_windows);
                // The equipment-buy recipient flow's windows (36 / 25 / 41)
                // ride over the parked buy list while the picker owns the
                // pad - the same compositing order the browser play page
                // uses in `play_overlay_draws_json`.
                stage.extend(self.recipient_window_draws());
                if !rows_spec.is_empty() {
                    let rows: Vec<ShopRow<'_>> = rows_spec
                        .iter()
                        .map(|(l, price, ink)| ShopRow {
                            label: l.as_str(),
                            price: *price,
                            ink: *ink,
                        })
                        .collect();
                    let shop_draws = shop_draws_for(
                        &self.font,
                        title,
                        &rows,
                        cursor,
                        show_gold,
                        SHOP_OVERLAY_PEN,
                    );
                    stage.extend(shop_draws);
                }
            } else if self.menu_runtime.inn_session.is_some() {
                // Inn overlay: cost prompt with Yes / No cursor.
                let state = MenuState::from_byte(self.menu_runtime.ctx_state());
                let cursor = self.menu_runtime.cursor() as usize;
                let cost = self
                    .menu_runtime
                    .inn_session
                    .as_ref()
                    .map(|s| s.cost)
                    .unwrap_or(0);
                let gold = self.session.host.world.party.money;
                match state {
                    Some(MenuState::InnConfirm) => {
                        let title = format!("INN  Rest for {}G?", cost);
                        let rows = vec![ShopRow::new("Yes", None), ShopRow::new("No", None)];
                        let inn_draws = shop_draws_for(
                            &self.font,
                            &title,
                            &rows,
                            cursor,
                            Some(gold),
                            SHOP_OVERLAY_PEN,
                        );
                        stage.extend(inn_draws);
                    }
                    Some(MenuState::InnSleep) => {
                        let layout = self.font.layout_ascii("Resting...");
                        stage.extend(text_draws_for(&layout, SHOP_OVERLAY_PEN, white));
                    }
                    _ => {
                        let menu_label = format!("[{}]", label);
                        let ml_layout = self.font.layout_ascii(&menu_label);
                        stage.extend(text_draws_for(&ml_layout, SHOP_OVERLAY_PEN, white));
                    }
                }
            } else {
                // Non-shop, non-inn menu: show current mode label.
                let menu_label = format!("[{}]", label);
                let ml_layout = self.font.layout_ascii(&menu_label);
                stage.extend(text_draws_for(&ml_layout, SHOP_OVERLAY_PEN, white));
            }
        }
        stage
    }

    /// The gold 9-slice frame around the shop / inn panel above.
    ///
    /// The browser play page has framed this panel since it gained the menu
    /// chrome atlas; the native window drew the rows bare, because its text
    /// pass and its sprite pass are different borrows and nothing sized the
    /// frame for the sprite one. Both hosts now take the rect from
    /// `legaia_engine_ui::shop_panel_frame_rect`, off the same pen and the
    /// same row count.
    pub(super) fn shop_overlay_chrome_sprite_draws(
        &self,
        surface_w: u32,
        surface_h: u32,
    ) -> Vec<legaia_engine_render::SpriteDraw> {
        let Some(menu) = self.save_menu.as_ref() else {
            return Vec::new();
        };
        let draws = self.shop_overlay_stage_draws();
        if draws.is_empty() {
            return Vec::new();
        }
        let rows = legaia_engine_render::shop_panel_rows(&draws);
        let (stage_origin, stage_scale) =
            legaia_engine_render::pause_menu::stage_transform(surface_w, surface_h);
        legaia_engine_render::menu_window_chrome_draws_for(
            &menu.rects,
            legaia_engine_render::shop_panel_frame_rect(SHOP_OVERLAY_PEN, rows),
            stage_origin,
            stage_scale,
        )
    }

    /// Snapshot the live dialog source (simplified panel, cutscene
    /// timeline, or inline field-VM runner) into plain strings the
    /// text and chrome layers both consume. `None` when no box is
    /// open this frame.
    /// The tile board's quit-prompt geometry while its walk SM sits in the
    /// prompt state, `None` otherwise.
    fn tile_board_prompt_layout(&self) -> Option<legaia_engine_render::TileBoardPromptLayout> {
        self.session.host.world.tile_board_prompt_cursor()?;
        let (frame, rows, cursor_x) = legaia_engine_core::tile_board::prompt_layout();
        Some(legaia_engine_render::TileBoardPromptLayout {
            frame,
            rows,
            cursor_x,
        })
    }

    pub(super) fn dialog_snapshot(&self) -> Option<DialogSnapshot> {
        let to_ascii = |bytes: &[u8]| -> String {
            bytes
                .iter()
                .map(|&b| {
                    if (0x20..=0x7E).contains(&b) {
                        b as char
                    } else {
                        '?'
                    }
                })
                .collect()
        };
        let from_panel = |panel: &legaia_engine_core::dialog::OwnedDialogPanel,
                          require_text: bool|
         -> Option<DialogSnapshot> {
            let page = to_ascii(&panel.page_bytes());
            if require_text && page.is_empty() {
                return None;
            }
            let (options, cursor) = if panel.menu_active() {
                match panel.picker() {
                    Some(p) => (
                        p.options.iter().map(|o| to_ascii(&o.label)).collect(),
                        panel.picker_cursor(),
                    ),
                    None => (Vec::new(), 0),
                }
            } else {
                (Vec::new(), 0)
            };
            Some(DialogSnapshot {
                page,
                scroll_px: panel.scroll_px(),
                box_rows: panel.box_rows(),
                options,
                cursor,
                picker_rect: panel.picker_rect(),
                picker_hand: panel.picker_hand_drawn(),
                // The advance hand shows at a page break AND on the final
                // fully-typed page (retail waits for a confirm on both).
                waiting: panel.is_waiting_for_input() || panel.is_done(),
            })
        };
        if let Some(panel) = self.active_dialog.as_ref() {
            return from_panel(panel, false);
        }
        if let Some(panel) = self.session.host.world.script_dialog_panel()
            && let Some(snap) = from_panel(panel, true)
        {
            return Some(snap);
        }
        if let Some(id) = self.session.host.world.dialog.inline.as_ref()
            && let Some(panel) = id.panel.as_ref()
        {
            return from_panel(panel, true);
        }
        None
    }

    /// Compute the stage-pixel box rects for a dialog snapshot,
    /// mirroring the pager's traced geometry (`FUN_801D84D0`):
    ///
    /// - Main (reading) box: `(0x26, 0x10, 0xF4, lines*0xF - 3)` - the
    ///   per-frame `FUN_8002C69C` call passes `(ctx+0x12, ctx+0x14,
    ///   0xF4, lines*0xF + 5 - 8)`, and the live context in the
    ///   `v0_1_tetsu_dialogue_accept` capture holds `ctx+0x12 = 0x26`,
    ///   `ctx+0x14 = 0x10` (framebuffer cross-checked: drawn footprint
    ///   `x 30..289, y 8..65` = this rect inflated by the skin border).
    ///   Retail anchors the reading box at the TOP of the stage - with
    ///   or without an option picker.
    /// - Picker box: the engine's slide rect
    ///   ([`legaia_engine_core::dialog::OwnedDialogPanel::picker_rect`]) -
    ///   the box on its way in from off screen, then at rest: `0x2A` at the
    ///   top right `(0xD8, 0x4A, 0x58, 0x1A)`, the N-option lists at
    ///   `(0x26, 0x94 + ((4-n)*0xF)/2, 0xF4, 0x38 - (4-n)*0xF)`. `None`
    ///   (no box) on the press's sentinel call.
    ///
    /// Rects are the retail centre rects; the border skin the chrome
    /// pass draws extends ~8 px beyond them on every side
    /// (`dialog_window_chrome_draws_for`).
    pub(super) fn dialog_stage_layout(snap: &DialogSnapshot) -> DialogStageLayout {
        // Retail's standard reading box is ALWAYS 3 rows tall
        // (`_DAT_801F2740 = 3` in both box-init arms) regardless of how
        // much text has typed in; only over-long simplified pages grow
        // it to a 4th row.
        let lines = legaia_engine_render::dialog_reading_box_lines(&snap.page, snap.box_rows);
        let main_w = 0xF4;
        let main_h = lines * 0xF - 3;
        let picker = if snap.options.is_empty() {
            None
        } else {
            snap.picker_rect
        };
        DialogStageLayout {
            main: (0x26, 0x10, main_w, main_h),
            picker,
        }
    }

    /// Build the **arts command-input** chrome sprites - the four
    /// direction chips + D-pad glyph, the input bar with its committed
    /// pennants, and the AP plate - while a party member owns the pad in
    /// the retail-model entry session. Empty otherwise.
    ///
    /// Everything is composed by the shared
    /// [`legaia_engine_render::arts_input`] builders off the same baked
    /// system-UI atlas the menu chrome samples, so this host and the
    /// browser play page draw one geometry.
    pub(super) fn arts_input_chrome_sprite_draws(
        &self,
        surface_w: u32,
        surface_h: u32,
    ) -> Vec<legaia_engine_render::SpriteDraw> {
        use legaia_engine_render::arts_input as ai;
        let Some(assets) = self.save_menu.as_ref() else {
            return Vec::new();
        };
        if self.session.host.world.mode == legaia_engine_core::world::SceneMode::MuscleDome
            && !self.dome_battle_chrome_up()
        {
            return Vec::new();
        }
        let Some(view) = self.session.host.world.arts_input_view() else {
            return Vec::new();
        };
        let (stage_origin, stage_scale) = self.save_select_stage(surface_w, surface_h);
        let frame = ai::ArtsInputFrame {
            buffer: view.pennants,
            spent: view.pennant_spent,
            chip_costs: view.costs,
            chip_icons: view.chip_icons,
            pool: view.pool,
            pool_max: view.pool_max,
            plate_value: view.plate_value,
            list_page: view.list_page,
            phase: arts_input_screen(view.phase),
        };
        let mut out = ai::arts_input_chrome_draws(
            &ai::ArtsInputAtlasRects::BAKED,
            &frame,
            stage_origin,
            stage_scale,
        );
        // The Rot stamp over each direction the caster's rotted limbs refuse,
        // on top of the chips (retail draws it right after the D-pad glyph).
        // The browser page makes the same call.
        if let Some(rot) = assets.rects.battle.and_then(|b| b.rot_stamp) {
            out.extend(ai::arts_input_rot_stamp_draws(
                rot,
                &frame,
                view.status,
                stage_origin,
                stage_scale,
            ));
        }
        // The AP plate is the status screen's own AP-gauge widget, so it
        // reuses the pieces the atlas already carries.
        out.extend(ai::arts_input_ap_plate_draws(
            &ai::ApPlateRects {
                cap: assets.rects.gauge_cap,
                trough: assets.rects.gauge_trough,
                fill: assets.rects.gauge_fill,
                box_: assets.rects.gauge_box,
                digits: assets.rects.gauge_digits,
            },
            &frame,
            stage_origin,
            stage_scale,
        ));
        out
    }

    /// Build the dialog-window chrome sprites (gradient fill + gold
    /// 9-slice frame + hand cursors) for the active dialog box, if
    /// any. Sampled from the resident system-UI atlas; composited in
    /// the same sprite slot as the menu chrome, under the text layer.
    pub(super) fn dialog_chrome_sprite_draws(
        &self,
        surface_w: u32,
        surface_h: u32,
    ) -> Vec<legaia_engine_render::SpriteDraw> {
        let Some(assets) = self.save_menu.as_ref() else {
            return Vec::new();
        };
        if self.boot_ui.is_active() {
            return Vec::new();
        }
        let (stage_origin, stage_scale) = self.save_select_stage(surface_w, surface_h);
        // The `4C E1` balloon's frame: the fixed `0x58 x 0x90` window
        // (`FUN_8002C69C(0x58, y, 0x90, 0xB)`), in the same chrome skin as
        // the reading box. Drawn whether or not a reading box is also up -
        // retail's balloon is its own actor and outlives any dialog
        // engagement that spawned it.
        let mut out: Vec<legaia_engine_render::SpriteDraw> = Vec::new();
        if self.session.host.world.text_balloon_drawing().is_some()
            && let Some(rect) = self
                .session
                .host
                .world
                .cutscene
                .text_balloon
                .as_ref()
                .map(|b| b.frame_rect())
        {
            out.extend(legaia_engine_render::text_balloon_chrome_draws_for(
                &assets.rects,
                rect,
                stage_origin,
                stage_scale,
            ));
        }
        if let Some(lay) = self.tile_board_prompt_layout()
            && let Some(cursor) = self.session.host.world.tile_board_prompt_cursor()
        {
            out.extend(legaia_engine_render::tile_board_prompt_sprites_for(
                &assets.rects,
                &lay,
                cursor,
                stage_origin,
                stage_scale,
            ));
        }
        if self.session.host.world.incense_notice_shown() {
            out.extend(legaia_engine_render::incense_notice_sprites_for(
                &assets.rects,
                legaia_engine_core::incense_notice::notice_frame_rect(),
                stage_origin,
                stage_scale,
            ));
        }
        let Some(snap) = self.dialog_snapshot() else {
            return out;
        };
        let lay = Self::dialog_stage_layout(&snap);
        out.extend(legaia_engine_render::dialog_window_chrome_draws_for(
            &assets.rects,
            lay.main,
            stage_origin,
            stage_scale,
        ));
        if let Some(prect) = lay.picker {
            out.extend(legaia_engine_render::dialog_window_chrome_draws_for(
                &assets.rects,
                prect,
                stage_origin,
                stage_scale,
            ));
            // Pointing-hand cursor on the selected option row
            // (FUN_8002B994 kind 0 at box_x-6, box_y + cursor*0xF), drawn
            // only once the slide rests (count 0).
            if snap.picker_hand {
                out.push(legaia_engine_render::dialog_option_hand_sprite(
                    &assets.rects,
                    (prect.0, prect.1),
                    snap.cursor,
                    stage_origin,
                    stage_scale,
                ));
            }
        } else if snap.waiting {
            // Page-advance hand at the lower-right rim while the pager
            // waits for confirm (FUN_8002B994 kind 1).
            out.push(legaia_engine_render::dialog_advance_hand_sprite(
                &assets.rects,
                lay.main,
                stage_origin,
                stage_scale,
            ));
        }
        out
    }

    /// The live sparring-tutorial prompt's box rect in 320x240 stage pixels,
    /// or `None` when no box is up.
    ///
    /// The width is measured in this host's font (retail measures it with
    /// `FUN_80035F04`) and the engine applies the emitter's placement +
    /// sizing arithmetic. Shared by the text layer and the chrome layer so
    /// the frame and the rows cannot disagree.
    pub(super) fn battle_tutorial_stage_rect(&self) -> Option<(i32, i32, i32, i32)> {
        let tbox = self.session.host.world.battle_tutorial_box()?;
        let width = legaia_engine_render::battle_tutorial_text_width(&self.font, &tbox.text);
        let (x, y, w, h) = tbox.rect(width)?;
        Some((x as i32, y as i32, w as i32, h as i32))
    }

    /// Every tutorial box on screen with its stage rect - the front group of
    /// the world's box queue (one retail hook dispatch registers all of its
    /// boxes together, so the group draws together).
    pub(super) fn battle_tutorial_stage_boxes(&self) -> Vec<TutorialStageBox<'_>> {
        self.session
            .host
            .world
            .battle_tutorial_boxes_on_screen()
            .filter_map(|tbox| {
                let width =
                    legaia_engine_render::battle_tutorial_text_width(&self.font, &tbox.text);
                let (x, y, w, h) = tbox.rect(width)?;
                Some(((x as i32, y as i32, w as i32, h as i32), tbox))
            })
            .collect()
    }

    /// Sparring-tutorial prompt-box chrome: the same gradient fill + gold
    /// 9-slice frame the dialog reading box wears, at the rect the retail
    /// emitter registers the prompt's text actor with. Sampled from the
    /// resident system-UI atlas; composited in the shared chrome sprite slot,
    /// under the text layer.
    pub(super) fn battle_tutorial_chrome_sprite_draws(
        &self,
        surface_w: u32,
        surface_h: u32,
    ) -> Vec<legaia_engine_render::SpriteDraw> {
        let Some(assets) = self.save_menu.as_ref() else {
            return Vec::new();
        };
        if self.boot_ui.is_active() {
            return Vec::new();
        }
        let (stage_origin, stage_scale) = self.save_select_stage(surface_w, surface_h);
        let mut out = Vec::new();
        for (rect, tbox) in self.battle_tutorial_stage_boxes() {
            out.extend(legaia_engine_render::battle_tutorial_chrome_draws_for(
                &assets.rects,
                rect,
                tbox.waits_for_input,
                stage_origin,
                stage_scale,
            ));
        }
        // Koru's timed-fight strip wears the same skin (both are text actors
        // registered with an explicit rect and style word `0x44`).
        if legaia_engine_core::timed_fight::timed_fight_strip(&self.session.host.world).is_some() {
            out.extend(legaia_engine_render::timed_fight_strip_chrome_draws(
                &assets.rects,
                stage_origin,
                stage_scale,
            ));
        }
        out
    }

    /// Project the live name-entry session into the renderer-agnostic view
    /// the engine-ui builders consume (grid vs control cursor split via the
    /// session's own control mapping).
    pub(super) fn name_entry_view<'a>(
        &self,
        entry: &'a legaia_engine_core::name_entry::NameEntry,
    ) -> legaia_engine_render::NameEntryView<'a> {
        use legaia_engine_core::name_entry::GRID;
        let (grid_cursor, control_cursor) = entry.cursor_cells();
        legaia_engine_render::NameEntryView {
            grid_rows: &GRID,
            name: &entry.name,
            default_name: &entry.default_name,
            grid_cursor,
            control_cursor,
            confirming: entry.state == legaia_engine_core::name_entry::NameEntryState::Confirm,
            confirm_yes: entry.confirm_yes,
            caret_on: legaia_engine_core::name_entry::caret_on(self.session.host.world.frame),
        }
    }

    /// Build the name-entry window chrome + hand cursor sprites (the two
    /// filigree 9-slice windows at the retail-traced footprints). Sampled
    /// from the resident system-UI atlas; composited in the same sprite
    /// slot as the dialog chrome, under the text layer.
    pub(super) fn name_entry_chrome_sprite_draws(
        &self,
        surface_w: u32,
        surface_h: u32,
    ) -> Vec<legaia_engine_render::SpriteDraw> {
        let Some(assets) = self.save_menu.as_ref() else {
            return Vec::new();
        };
        let Some(entry) = self.session.host.world.party.name_entry.as_ref() else {
            return Vec::new();
        };
        let view = self.name_entry_view(entry);
        let (stage_origin, stage_scale) = self.save_select_stage(surface_w, surface_h);
        legaia_engine_render::name_entry_chrome_sprite_draws_for(
            &assets.rects,
            &view,
            stage_origin,
            stage_scale,
        )
    }
}

/// Plain-string view of the live dialog panel shared by the text and
/// chrome layers (see `PlayWindowApp::dialog_snapshot`).
pub(super) struct DialogSnapshot {
    /// Current typed-out page, `|` (0x7C) separating rows.
    pub page: String,
    /// Whole pixels the rows draw above their slots (the pager window's
    /// scroll, `<= 0`).
    pub scroll_px: i32,
    /// The pager window's height in rows (`None` for the plain-MES panel,
    /// whose box grows with its page); also the text clip band.
    pub box_rows: Option<usize>,
    /// Decoded option labels when a picker menu is open (empty
    /// otherwise).
    pub options: Vec<String>,
    /// Selected option row.
    pub cursor: usize,
    /// The picker box's rect this frame, from the engine's slide
    /// (`OwnedDialogPanel::picker_rect`); `None` before it starts.
    pub picker_rect: Option<(i32, i32, i32, i32)>,
    /// The option hand is drawn (the slide rests).
    pub picker_hand: bool,
    /// The panel is waiting for a confirm press (page fully typed).
    pub waiting: bool,
}

/// Stage-pixel dialog box layout (see
/// `PlayWindowApp::dialog_stage_layout`).
pub(super) struct DialogStageLayout {
    /// Main reading-box rect `(x, y, w, h)`.
    pub main: (i32, i32, i32, i32),
    /// Option-picker box rect when a menu is open.
    pub picker: Option<(i32, i32, i32, i32)>,
}

/// Top-left anchor of the battle HUD's slot-row block, in surface pixels.
///
/// Numerically equal to [`LEVEL_UP_BANNER_PEN`] and deliberately a separate
/// constant: nothing ties the battle HUD's anchor to the post-battle banner's,
/// and collapsing them would invent a coupling neither host has.
pub(super) const BATTLE_HUD_PEN: (i32, i32) = (8, 60);

/// Pen the field shop / inn overlay draws at (`shop_draws_for`'s `pen`), and
/// the anchor its plain-text stand-in lines share.
///
/// Duplicated on the browser play page as `play_shop::SHOP_PEN`; the two are
/// pinned equal by `scripts/ci/check-ui-host-drift.py`, which is the only
/// thing that keeps a move on one host from silently leaving the other behind.
pub(super) const SHOP_OVERLAY_PEN: (i32, i32) = (8, 140);

/// Pen the post-battle level-up banner draws at (`level_up_draws_for`).
/// Web twin: `play_shop::LEVEL_UP_PEN`.
pub(super) const LEVEL_UP_BANNER_PEN: (i32, i32) = (8, 60);

/// Pen the monster-capture banner draws at (`capture_banner_draws_for`).
/// Web twin: `play_shop::CAPTURE_PEN`.
pub(super) const CAPTURE_BANNER_PEN: (i32, i32) = (8, 40);

impl PlayWindowApp {
    /// The solid-white font-atlas texel the battle HUD's filled rects sample
    /// (`font_solid_src`). Scanned once per process - the window's font never
    /// changes after startup.
    pub(super) fn battle_hud_solid_src(&self) -> Option<(u32, u32, u32, u32)> {
        use std::sync::OnceLock;
        static SOLID: OnceLock<Option<(u32, u32, u32, u32)>> = OnceLock::new();
        *SOLID.get_or_init(|| legaia_engine_render::font_solid_src(&self.font))
    }

    /// One battle-HUD frame from the shared builder: the party strip, the
    /// top-left plaque and the popups.
    ///
    /// Both halves come from one call so the two host draw slots cannot
    /// drift: the text half goes into the glyph layer, the sprite half into
    /// the system-UI atlas layer through `battle_chrome_sprite_draws`.
    pub(super) fn battle_hud_frame_draws(
        &self,
        w: u32,
        h: u32,
    ) -> legaia_engine_render::BattleHudDraws {
        use legaia_engine_core::battle_hud as bh;
        // The result screen draws neither the party card nor the pill
        // (retail `noa_levelup_banner`: the two framed windows over the
        // bare battle scene) - the readout comes down with the last action.
        if self.session.host.world.battle_result_screen_active() {
            return legaia_engine_render::BattleHudDraws {
                text: Vec::new(),
                sprites: Vec::new(),
            };
        }
        let slots = battle_hud_slot_views(&self.battle_hud);
        let popups = battle_hud_popup_views(&self.battle_hud);
        let w_ref = &self.session.host.world;
        // Every per-phase decision is the engine's (`battle_hud`'s
        // predicates carry retail's sub-draw script + action-SM rule), so
        // this host and the browser page cannot disagree about which
        // surface is up.
        let plaque = bh::battle_active_actor(w_ref);
        let third_tab = bh::battle_breadcrumb_third_tab(w_ref);
        let target_plaque = bh::battle_target_plaque(w_ref);
        let target_select = bh::battle_target_select_plaque(w_ref);
        let move_name = bh::battle_move_name(w_ref);
        let message_bar = bh::battle_message_bar(w_ref);
        let commit_log = bh::battle_commit_log(w_ref);
        // The battle-intro enemy-name banner (retail flow `0x0A`), laid out
        // with this window's font - the same builder the browser page calls.
        let intro_names = bh::battle_intro_names(w_ref, &self.font);
        // A battle-stage module's boss-name banner (the Cort arrival), the
        // same builder the browser page calls.
        let stage_banner = bh::battle_stage_banner(w_ref, &self.font);
        let badges = self.battle_badge_rects();
        let banner = self.battle_banner_message();
        battle_hud_draws_for(
            &self.font,
            &legaia_engine_render::BattleHudFrame {
                slots: &slots,
                popups: &popups,
                log: &[],
                solid_src: self.battle_hud_solid_src(),
                surface: (w, h),
                chrome: self.save_menu.as_ref().map(|a| &a.rects),
                // The actor-name plaque shares its top-left seat with the
                // item window's Begin | <name> | Item breadcrumb trail, and
                // retail parks the plaque while that window is up (the
                // battle_item_window capture shows the crumbs alone), so the
                // frame draws one or the other, never both.
                plaque: plaque
                    .as_ref()
                    .filter(|_| w_ref.battle.item_menu.is_none())
                    .map(|(_, n)| n.as_str()),
                plaque_badge: bh::battle_plaque_element_badge(w_ref),
                banner: banner.as_deref(),
                // The sparring-tutorial prompt is a box the host draws
                // itself, and its rect starts on the plaque's own content
                // pen - so while it is up the plaque must not draw, or the
                // two text runs land on the same pixels.
                plaque_seat_taken: self.battle_tutorial_stage_rect().is_some()
                    || w_ref.dialog.current.is_some()
                    || w_ref.dialog.inline.is_some()
                    // Koru's timed-fight strip: retail draws it OVER the
                    // plaque (text actor key 1 walks before the plaque's
                    // 0x23 on the same ordering-table slot), so the plaque
                    // sits under the strip's fill. This host composites
                    // every text run above every sprite and cannot put the
                    // plaque's name under the strip's frame - it parks it.
                    || legaia_engine_core::timed_fight::timed_fight_strip(w_ref).is_some(),
                badges: badges.as_ref(),
                // The same box, tested against the party surfaces' own rows:
                // a bottom-anchored prompt lands on the active-actor bar
                // (188..208) and inside the roster panels (164..212), so the
                // builder parks whichever one it covers.
                host_box: self.battle_tutorial_stage_rect(),
                active_slot: bh::battle_readout_bar_slot(w_ref),
                panels_parked: !bh::battle_panels_visible(w_ref),
                begin_tab: bh::battle_begin_tab_visible(w_ref),
                third_tab: third_tab.as_deref(),
                move_name: move_name.as_deref(),
                target_plaque: target_plaque.as_ref().map(|(n, b)| (n.as_str(), *b)),
                target_select: target_select.as_ref().map(|(n, b)| (n.as_str(), *b)),
                message_bar: message_bar.as_deref(),
                ap_plate_value: bh::battle_ring_ap_plate_value(w_ref),
                commit_log: &commit_log,
                intro_names: &intro_names,
                stage_banner: stage_banner.as_ref().map(|(l, x, y)| (l.as_str(), *x, *y)),
                diag: legaia_engine_render::diag_hud_enabled(),
            },
            BATTLE_HUD_PEN,
        )
    }

    /// The badge cells the battle HUD blits, projected out of the baked
    /// atlas. `None` before the atlas is resident; a `None` *cell* inside it
    /// means that badge's palette source was outside the slice the atlas was
    /// built from, and the HUD falls back to its labelled tag.
    pub(super) fn battle_badge_rects(
        &self,
    ) -> Option<legaia_engine_render::battle_hud_chrome::BattleBadgeRects> {
        self.save_menu.as_ref().map(|a| a.badges)
    }

    /// The message holding retail's top-of-screen banner this frame, if any:
    /// the engine's shared read
    /// ([`legaia_engine_core::battle_hud::battle_banner_message`] - the absorb
    /// / magic-level element line, then level-up, then Seru capture), which the
    /// browser page draws too.
    ///
    /// `None` without the system-UI atlas: there is no frame to put a
    /// message in, so a chrome-less host keeps the loose pens instead.
    pub(super) fn battle_banner_message(&self) -> Option<String> {
        self.save_menu.as_ref()?;
        legaia_engine_core::battle_hud::battle_banner_message(&self.session.host.world)
    }

    /// The engine-core battle-item-window projection (shared with the
    /// browser play page - `World::battle_item_menu_model` owns the gating
    /// and text resolution; this window only borrows it into the builder's
    /// frame via [`with_battle_item_frame`]).
    pub(super) fn battle_item_menu_model(
        &self,
    ) -> Option<legaia_engine_core::inventory_use::BattleItemMenuModel> {
        self.session.host.world.battle_item_menu_model()
    }

    /// The live battle command surface projected into the shared chip-cluster
    /// view: the owned `(label, enabled)` chips of whichever phase is up, the
    /// cursor index, and the phase (which names the seats). `None` when no
    /// command surface owns the frame.
    ///
    /// The projection itself is `engine-core::battle_hud::battle_command_chips`,
    /// shared with the browser page, and where the ring's element chip
    /// becomes the member's Ra-Seru name or `-` off the disc.
    /// Whether a Muscle Dome leg's battle chrome is on screen: the leg's
    /// selection is up and no hub screen (the first visit, the leg-open
    /// ROUND card) covers it. Retail runs those hub arms before the round
    /// driver raises its command cluster.
    pub(super) fn dome_battle_chrome_up(&self) -> bool {
        self.session.host.world.mode == SceneMode::MuscleDome
            && self.session.host.world.minigames.muscle_dome.is_some()
            && !self.session.host.world.minigames.muscle_hub.covers_leg()
    }

    pub(super) fn battle_command_menu_chips(&self) -> Option<CommandChips> {
        use legaia_engine_core::battle_hud::{CommandChipPhase, battle_command_chips};
        use legaia_engine_render::battle_command_ui::ChipPhase;
        let chips = battle_command_chips(&self.session.host.world)?;
        // The two enums are separate types because `engine-ui` is a leaf
        // that does not link `engine-core`; the browser page carries the
        // same four-line map.
        let phase = match chips.phase {
            CommandChipPhase::RoundPrompt => ChipPhase::RoundPrompt,
            CommandChipPhase::CommandRing => ChipPhase::CommandRing,
            CommandChipPhase::AttackMode => ChipPhase::AttackMode,
            CommandChipPhase::CommitConfirm => ChipPhase::CommitConfirm,
        };
        Some((chips.chips, chips.cursor, phase))
    }

    /// The battle HUD's chrome sprites (strip + plaque lozenges, gold `HP` /
    /// green `MP` label cells) for the system-UI atlas slot, plus the
    /// command menu's chip plates + D-pad glyph when a menu is up. Empty
    /// before the atlas is resident.
    ///
    /// Outside battle this narrows to one surface: the frame of the
    /// **message banner** carrying a level-up / Seru-capture line, which the
    /// port raises after the fight has already handed the frame back to the
    /// field. Its text half rides the glyph layer in `hud_draws`.
    pub(super) fn battle_chrome_sprite_draws(
        &self,
        surface_w: u32,
        surface_h: u32,
    ) -> Vec<legaia_engine_render::SpriteDraw> {
        let Some(assets) = self.save_menu.as_ref() else {
            return Vec::new();
        };
        if self.boot_ui.is_active() {
            return Vec::new();
        }
        if self.session.host.world.mode == legaia_engine_core::world::SceneMode::MuscleDome
            && !self.dome_battle_chrome_up()
        {
            return Vec::new();
        }
        if !matches!(
            self.session.host.world.mode,
            legaia_engine_core::world::SceneMode::Battle
                | legaia_engine_core::world::SceneMode::MuscleDome
        ) {
            let Some(message) = self.battle_banner_message() else {
                return Vec::new();
            };
            let (origin, scale) = self.save_select_stage(surface_w, surface_h);
            use legaia_engine_render::battle_hud_chrome as bhc;
            return bhc::message_banner_chrome_draws_for(
                &assets.rects,
                bhc::message_banner_content(&self.font, &message),
                origin,
                scale,
            );
        }
        let mut out = self.battle_hud_frame_draws(surface_w, surface_h).sprites;
        // The battle item window's chrome (both packet-pinned 9-slice
        // windows, the breadcrumb tabs and the hand cursor) rides the same
        // atlas slot as the rest of the menu chrome.
        if let Some(model) = self.battle_item_menu_model() {
            let (origin, scale) = self.save_select_stage(surface_w, surface_h);
            out.extend(with_battle_item_frame(&model, |frame| {
                legaia_engine_render::battle_item_ui::battle_item_window_sprites(
                    &self.font,
                    &assets.rects,
                    frame,
                    origin,
                    scale,
                )
            }));
        }
        // The command chips sample the same blue plate 3-slice the party
        // bar does, so they ride this list rather than a second slot.
        if let (Some(rects), Some((chips, cursor, phase))) =
            (assets.rects.battle, self.battle_command_menu_chips())
        {
            use legaia_engine_render::battle_command_ui as bcu;
            let (origin, scale) = self.save_select_stage(surface_w, surface_h);
            let views = bcu::command_chip_views(&chips);
            // The plates, and on top of them every mark the ring wears -
            // the red cross-outs the special word raises (the Rim Elm
            // ambush / monster `0xAF`) and the Rot / Curse marks over the
            // arms the acting member's status refuses: one builder, one
            // engine read, the same call the browser page makes.
            out.extend(bcu::battle_command_menu_sprites(
                &rects,
                &bcu::BattleCommandMenuFrame {
                    chips: &views,
                    cursor: Some(cursor),
                    phase,
                },
                legaia_engine_core::battle_hud::battle_ring_marks(&self.session.host.world),
                origin,
                scale,
            ));
        }
        out
    }

    /// The enemy-row half of a target picker's on-screen text.
    ///
    /// Retail draws **one** plaque for the target cursor - placement record
    /// `0x29`, seated by `FUN_801D5854`'s target arm at `0xE8 - w/2`, row
    /// `162` - and that plaque rides the shared battle-HUD builder
    /// (`BattleHudFrame::target_select`, filled from
    /// `battle_hud::battle_target_select_plaque`) so it lands on the same
    /// pixels on both hosts. This returns an empty draw list while the cursor
    /// sits on the enemy row, so the caller does not add its text fallback;
    /// `None` on the ally / sweep states, which keep their text line.
    ///
    /// The earlier dedup-name strip (the `FUN_801D9D3C` intro-banner layout
    /// at stage row 166) is retired here: that routine is the battle-intro
    /// banner's composer, and its strip overprinted the commit log's target
    /// column.
    pub(super) fn enemy_target_strip_draws(
        &self,
        picker: &legaia_engine_core::target_picker::TargetPickerSession,
        _w: u32,
        _h: u32,
    ) -> Option<Vec<TextDraw>> {
        use legaia_engine_core::target_picker::{CursorRow, PickerState};
        matches!(
            picker.state(),
            PickerState::Cursor {
                row: CursorRow::Enemy,
                ..
            }
        )
        .then(Vec::new)
    }
}

/// Borrow an engine-core battle-item-window model as the shared builder's
/// frame, handing it to `f` (the frame borrows row views built here, so
/// this is CPS-shaped).
pub(super) fn with_battle_item_frame<R>(
    model: &legaia_engine_core::inventory_use::BattleItemMenuModel,
    f: impl FnOnce(&legaia_engine_render::battle_item_ui::BattleItemMenuFrame<'_>) -> R,
) -> R {
    use legaia_engine_render::battle_item_ui as bii;
    let rows: Vec<bii::BattleItemRowView<'_>> = model
        .view
        .rows
        .iter()
        .map(|r| bii::BattleItemRowView {
            name: &r.name,
            count: r.count,
            admissible: r.admissible,
        })
        .collect();
    let target_rows: Vec<bii::BattleItemTargetView<'_>> = model
        .targets
        .as_ref()
        .map(|(rows, _)| {
            rows.iter()
                .map(|t| bii::BattleItemTargetView {
                    name: &t.name,
                    hp: t.hp,
                    hp_max: t.hp_max,
                    mp: t.mp,
                    mp_max: t.mp_max,
                    alive: t.alive,
                })
                .collect()
        })
        .unwrap_or_default();
    let frame = bii::BattleItemMenuFrame {
        rows: &rows,
        cursor: model.view.cursor_row,
        description: model.description.as_deref(),
        actor_name: &model.actor_name,
        targets: model
            .targets
            .as_ref()
            .map(|(_, cursor)| (target_rows.as_slice(), *cursor)),
    };
    f(&frame)
}

/// Project the HUD model's slot array into the shared builder's view type.
///
/// Every slot is emitted, **including inactive ones** (as empty-name rows the
/// builder skips). That is deliberate: `battle_hud_draws_for` derives both a
/// row's Y and a popup's anchor from the slice index, so the index has to stay
/// the absolute actor-table slot. Compacting to active slots only would shift
/// every monster row up and anchor damage numbers to the wrong actor.
pub(super) fn battle_hud_slot_views(
    hud: &legaia_engine_core::battle_hud::BattleHud,
) -> Vec<HudSlotView<'_>> {
    hud.slots
        .iter()
        .map(|s| {
            let (hp_fill, mp_fill) = s.gauge_fill_indices();
            let meta = HudSlotMeta {
                is_party: s.is_party,
                alive: s.alive,
                hp: s.hp,
                hp_max: s.hp_max,
                mp: s.mp,
                mp_max: s.mp_max,
                ap_filled: s.ap_filled,
                ap_max: s.ap_max,
                hp_fill,
                mp_fill,
                // The single retail-selected status element
                // (`FUN_8002C2E4`'s ladder over the packed `+0x16E` word)
                // plus the level its no-ailment arm draws.
                status_sprite: s.status_sprite(),
                level: s.level,
            };
            let name = if s.active { s.name.as_str() } else { "" };
            HudSlotView::from_plain(meta, name)
        })
        .collect()
}

/// Project the HUD model's popup queue into the shared builder's view type.
pub(super) fn battle_hud_popup_views(
    hud: &legaia_engine_core::battle_hud::BattleHud,
) -> Vec<HudPopupView> {
    hud.popup_views()
        .into_iter()
        .map(|p| HudPopupView {
            slot: p.slot,
            amount: p.amount,
            is_heal: p.is_heal,
            is_crit: p.is_crit,
            status_letter: p.status_letter,
            alpha: p.alpha,
        })
        .collect()
}

#[cfg(test)]
mod battle_hud_wiring_tests {
    use super::{BATTLE_HUD_PEN, battle_hud_popup_views, battle_hud_slot_views};
    use legaia_engine_core::battle_hud::{BattleHud, DamagePopup, SlotSyncInfo};
    use legaia_engine_render::{BattleHudDraws, BattleHudFrame, battle_hud_draws_for};

    /// A recognisable 1x1 solid src for the filled-rect draws.
    const SOLID: (u32, u32, u32, u32) = (7, 3, 1, 1);
    /// 640x480 = an exact 2x of the 320x240 stage with a zero origin, so a
    /// stage column `c` lands at surface `2 * c` and the pinned retail
    /// columns are readable straight off `dst.0`.
    const SURFACE: (u32, u32) = (640, 480);
    const STAGE_SCALE: i32 = 2;

    /// The numeral strip's seat in the baked atlas
    /// (`save_menu_atlas::ATLAS_RECT_HUD_DIGITS`).
    const BATTLE_MIRROR_DIGITS: (u32, u32, u32, u32) = (0, 244, 80, 12);
    /// The minimum chrome set that puts the numerals on the sprite list, so
    /// a cell's screen seat is readable rather than inferred from a glyph.
    const BATTLE_MIRROR_RECTS: legaia_engine_render::SaveMenuAtlasRects =
        legaia_engine_render::SaveMenuAtlasRects {
            battle: Some(legaia_engine_render::BattleChromeRects {
                panel_bg: (0, 0, 102, 48),
                plate_cap_l: (208, 0, 8, 20),
                plate_body: (192, 0, 16, 20),
                plate_cap_r: (216, 0, 8, 20),
                separator: (96, 64, 8, 16),
                digits: Some(BATTLE_MIRROR_DIGITS),
                cross_out: None,
                rot_stamp: None,
                curse_plate: None,
            }),
            ..blank_rects()
        };

    /// `SaveMenuAtlasRects::default()` is not `const`, so spell the zeroed
    /// base out for [`BATTLE_MIRROR_RECTS`].
    const fn blank_rects() -> legaia_engine_render::SaveMenuAtlasRects {
        const Z: (u32, u32, u32, u32) = (0, 0, 0, 0);
        legaia_engine_render::SaveMenuAtlasRects {
            panel_tl: Z,
            panel_tr: Z,
            panel_bl: Z,
            panel_br: Z,
            panel_top: Z,
            panel_bot: Z,
            panel_left: Z,
            panel_right: Z,
            slot1: Z,
            slot2: Z,
            cursor: Z,
            panel_interior: Z,
            panel_filigree: Z,
            label_lv: Z,
            label_hp: Z,
            label_mp: Z,
            icon_money: Z,
            label_time: Z,
            label_coin: Z,
            gauge_cap: Z,
            gauge_trough: Z,
            gauge_box: Z,
            gauge_tip: Z,
            gauge_digits: Z,
            gauge_100: Z,
            gauge_fill: Z,
            dialog_fill: Z,
            icon_weapon: Z,
            icon_helmet: Z,
            icon_armor: Z,
            icon_boot: Z,
            icon_goods: Z,
            pager_left: Z,
            pager_right: Z,
            tab_cap_l: Z,
            tab_body: Z,
            tab_cap_r: Z,
            atr_icons: [Z; 3],
            load_empty_frame: None,
            load_portrait_by_char: [None; 3],
            battle: None,
        }
    }

    fn hud_with_party_row(hp: u16, hp_max: u16, mp: u16, mp_max: u16) -> BattleHud {
        let mut hud = BattleHud::new();
        hud.sync_slot(
            0,
            SlotSyncInfo {
                name: "Vahn",
                is_party: true,
                alive: true,
                hp,
                hp_max,
                mp,
                mp_max,
                ap: None,
            },
        );
        hud
    }

    fn frame_draws(hud: &BattleHud, diag: bool) -> BattleHudDraws {
        battle_hud_draws_for(
            &legaia_font::synthetic_for_tests(),
            &BattleHudFrame {
                slots: &battle_hud_slot_views(hud),
                popups: &battle_hud_popup_views(hud),
                log: &[],
                solid_src: Some(SOLID),
                surface: SURFACE,
                diag,
                ..Default::default()
            },
            BATTLE_HUD_PEN,
        )
    }

    /// Solid rects of exactly `(w, h)` stage pixels, by stage `(x, y)`.
    fn boxes_of(draws: &[legaia_engine_render::TextDraw], w: i32, h: i32) -> Vec<(i32, i32)> {
        draws
            .iter()
            .filter(|d| {
                d.src == SOLID
                    && d.dst.2 == (w * STAGE_SCALE) as u32
                    && d.dst.3 == (h * STAGE_SCALE) as u32
            })
            .map(|d| (d.dst.0 / STAGE_SCALE, d.dst.1 / STAGE_SCALE))
            .collect()
    }

    fn draws(hud: &BattleHud) -> Vec<legaia_engine_render::TextDraw> {
        frame_draws(hud, false).text
    }

    /// The party arm draws retail's resting surface: one 102x48 roster panel
    /// per live member at `battle_chrome::panel_seats`, and **no gauge bar**.
    ///
    /// The packet run carries no bar primitive in either readout, so a filled
    /// HP or MP bar on a party row is the defect this pins shut.
    #[test]
    fn native_battle_party_is_retail_shaped_and_barless() {
        let hud = hud_with_party_row(250, 300, 12, 30);
        let out = draws(&hud);
        assert_eq!(
            boxes_of(&out, 102, 48),
            vec![(109, 164)],
            "the solo roster panel is not at its packet-pinned seat"
        );
        // Every solid rect on the retail surface is a plate body or one of
        // the 1-px rims the chrome-less fallback draws round it. Anything
        // with interior extents is a gauge bar.
        for d in out.iter().filter(|d| d.src == SOLID) {
            let w = d.dst.2 as i32 / STAGE_SCALE;
            let h = d.dst.3 as i32 / STAGE_SCALE;
            assert!(
                w == 1 || h == 1 || h == 20 || (w, h) == (102, 48),
                "a gauge-bar-shaped rect survives on the retail surface: {:?}",
                d.dst
            );
        }
        // Name glyph at the panel's pinned name pen (+5 inside the panel).
        assert!(
            out.iter().any(|d| d.src != SOLID
                && d.dst.0 == (109 + 5) * STAGE_SCALE
                && d.dst.1 == (164 + 4) * STAGE_SCALE),
            "no name glyph at the panel's pinned name pen"
        );
    }

    /// `engine-ui`'s `party_panel_stage_x` reads the packet-pinned
    /// `engine-vm` kernels on its production path
    /// (`battle_party_panel::panel_anchors`, falling back to
    /// `battle_chrome::panel_seats` + the text inset); only the panel
    /// *backgrounds* still carry a local seat mirror. This test holds the
    /// drawn HUD to `battle_chrome`'s seats end to end - a drift here is a
    /// HUD drawn at coordinates nothing pinned.
    #[test]
    fn engine_ui_seats_mirror_the_packet_pinned_battle_chrome() {
        use legaia_engine_vm::battle_chrome as bc;
        let font = legaia_font::synthetic_for_tests();
        let hud = hud_with_party_row(250, 300, 12, 30);
        // The two party surfaces are mutually exclusive, so each is measured
        // in the frame that owns it: the panels at rest, the bar acting.
        let frame = |active: Option<u8>| -> Vec<legaia_engine_render::TextDraw> {
            battle_hud_draws_for(
                &font,
                &BattleHudFrame {
                    slots: &battle_hud_slot_views(&hud),
                    solid_src: Some(SOLID),
                    surface: SURFACE,
                    active_slot: active,
                    plaque: Some("Vahn"),
                    ..Default::default()
                },
                BATTLE_HUD_PEN,
            )
            .text
        };
        let resting = frame(None);
        let acting = frame(Some(0));

        // Panel seats + row.
        let seats = bc::panel_seats(1);
        assert_eq!(
            boxes_of(&resting, bc::PANEL_BG.2 as i32, bc::PANEL_BG.3 as i32),
            vec![(seats[0] as i32, bc::PANEL_Y as i32)],
            "the mirrored panel seat drifted from battle_chrome"
        );
        assert!(
            boxes_of(&acting, bc::PANEL_BG.2 as i32, bc::PANEL_BG.3 as i32).is_empty(),
            "the roster cluster drew under the active-actor bar"
        );
        // Active-actor bar: plate footprint and name pen.
        let bar_w = (bc::BAR_INTERIOR_W + 2 * bc::PLATE_CAP_W) as i32;
        assert_eq!(
            boxes_of(&acting, bar_w, bc::PLATE_H as i32),
            vec![(bc::BAR_X as i32, bc::BAR_Y as i32)],
            "the mirrored active-actor bar drifted from battle_chrome"
        );
        assert!(
            acting.iter().any(|d| d.src != SOLID
                && d.dst.0 == bc::BAR_NAME.0 as i32 * STAGE_SCALE
                && d.dst.1 == bc::BAR_NAME.1 as i32 * STAGE_SCALE),
            "the mirrored bar name pen drifted from battle_chrome"
        );
        // Plaque: plate sized to the measured name at the pinned seat.
        let plaque = bc::name_plaque(font.layout_ascii("Vahn").advance_x as u16, false);
        assert!(
            boxes_of(
                &acting,
                bc::plate_width(plaque.interior_w) as i32,
                bc::PLATE_H as i32
            )
            .contains(&(bc::PLAQUE_X as i32, bc::PLAQUE_Y as i32)),
            "the mirrored plaque drifted from battle_chrome::name_plaque"
        );
        assert!(
            acting.iter().any(|d| d.src != SOLID
                && d.dst.0 == plaque.text.0 as i32 * STAGE_SCALE
                && d.dst.1 == plaque.text.1 as i32 * STAGE_SCALE),
            "the mirrored plaque text seat drifted from battle_chrome"
        );
    }

    /// Third face of the same mirror: the **command-chip clusters**. Both
    /// pinned clusters and every seat on them have to agree with
    /// `battle_chrome`, or the menu draws chips at coordinates nothing
    /// pinned. This window is again the only crate that can see both sides.
    #[test]
    fn engine_ui_command_chips_mirror_the_packet_pinned_battle_chrome() {
        use legaia_engine_render::battle_command_ui as bcu;
        use legaia_engine_vm::battle_chrome as bc;

        let pairs = [
            (bcu::CLUSTER_COMMAND, bc::CLUSTER_COMMAND),
            (bcu::CLUSTER_TOP_LEVEL, bc::CLUSTER_TOP_LEVEL),
            (bcu::CLUSTER_COMMIT_CONFIRM, bc::CLUSTER_COMMIT_CONFIRM),
        ];
        for (ui, vm) in pairs {
            assert_eq!(ui.centre, (vm.centre.0 as i32, vm.centre.1 as i32));
            assert_eq!(ui.dx, vm.dx as i32);
            assert_eq!(ui.dy, vm.dy as i32);
            assert_eq!(ui.interior_w, vm.interior_w as i32);
            assert_eq!(
                ui.plate_width(),
                bc::plate_width(vm.interior_w) as i32,
                "the mirrored plate width drifted from battle_chrome"
            );
            let seats = [
                (bcu::ChipSeat::Up, bc::ChipSeat::Up),
                (bcu::ChipSeat::Left, bc::ChipSeat::Left),
                (bcu::ChipSeat::Right, bc::ChipSeat::Right),
                (bcu::ChipSeat::Down, bc::ChipSeat::Down),
            ];
            for (us, vs) in seats {
                let (px, py) = vm.plate_origin(vs);
                assert_eq!(
                    ui.plate_origin(us),
                    (px as i32, py as i32),
                    "the mirrored chip plate seat drifted from battle_chrome"
                );
                let (lx, ly) = vm.label_seat(vs);
                assert_eq!(
                    ui.label_seat(us),
                    (lx as i32, ly as i32),
                    "the mirrored chip label pen drifted from battle_chrome"
                );
            }
            let (dx, dy, dw, dh) = vm.dpad_rect();
            assert_eq!(ui.dpad_rect(), (dx as i32, dy as i32, dw as u32, dh as u32));
        }
        // The plate 3-slice and the D-pad cell the chips sample are the
        // same rects `battle_chrome` names.
        let a = bcu::CommandChipAtlas::SHEET;
        assert_eq!(a.plate_cap_l.0 as u16, bc::PLATE_CAP_L_U);
        assert_eq!(a.plate_body.0 as u16, bc::PLATE_BODY_U);
        assert_eq!(a.plate_cap_r.0 as u16, bc::PLATE_CAP_R_U);
        for r in [a.plate_cap_l, a.plate_body, a.plate_cap_r] {
            assert_eq!(r.1 as u16, bc::PLATE_BLUE.v);
            assert_eq!(r.3 as u16, bc::PLATE_H);
        }
        assert_eq!(
            a.dpad,
            (
                bc::DPAD_GLYPH.0 as u32,
                bc::DPAD_GLYPH.1 as u32,
                bc::DPAD_GLYPH.2 as u32,
                bc::DPAD_GLYPH.3 as u32
            ),
            "the command cluster stopped sampling battle_chrome's D-pad cell"
        );
        assert_eq!(bcu::DPAD_DRAW, bc::DPAD_DRAW_W as u32);
        // One chip per ring entry, and every one of them is a pinned diamond
        // arm - there is no invented seating left on this screen.
        assert_eq!(
            bcu::MENU_SEATS.len(),
            legaia_engine_core::battle_input::BattleCommand::MENU.len(),
            "the seating table and the command ring disagree on entry count"
        );
        assert_eq!(
            bcu::MENU_SEATS
                .iter()
                .filter(|s| matches!(s, bcu::CommandSeat::Diamond(_)))
                .count(),
            4,
            "the pinned diamond has four arms and they must all be used"
        );
        // The other two phases seat on pinned arms too: the round prompt on
        // the top-level pair, the attack-mode prompt on the diamond's own
        // left / right.
        assert_eq!(
            bcu::ROUND_PROMPT_SEATS.len(),
            legaia_engine_core::battle_input::RoundChoice::PROMPT.len()
        );
        assert!(
            bcu::ROUND_PROMPT_SEATS
                .iter()
                .all(|s| matches!(s, bcu::CommandSeat::TopLevel(_)))
        );
        assert_eq!(
            bcu::ATTACK_MODE_SEATS.len(),
            legaia_engine_core::battle_input::AttackMode::PROMPT.len()
        );
        assert_eq!(bcu::ATTACK_MODE_SEATS[0], bcu::MENU_SEATS[1]);
        assert_eq!(bcu::ATTACK_MODE_SEATS[1], bcu::MENU_SEATS[2]);
        // The commit confirm seats its two chips on its own pinned pair, in
        // the engine's `Begin`, `Reselect` order.
        assert_eq!(
            bcu::COMMIT_CONFIRM_SEATS.len(),
            legaia_engine_core::battle_input::CommitChoice::PROMPT.len()
        );
        assert_eq!(
            bcu::COMMIT_CONFIRM_SEATS,
            [
                bcu::CommandSeat::Commit(bcu::ChipSeat::Left),
                bcu::CommandSeat::Commit(bcu::ChipSeat::Right),
            ]
        );
    }

    /// A direction press must commit the chip **drawn on that side of the
    /// screen**. `engine-core` cannot see the seating table (it does not
    /// link `engine-ui`), so it carries the direction → seat map as its own
    /// `match`; this is where that map is held equal to the drawn geometry:
    /// from every starting arm, Up commits the topmost plate, Down the
    /// bottommost, Left the leftmost and Right the rightmost. The committed
    /// arm is read back out of the phase the one press leaves the session
    /// in (retail's direct-commit dispatch - there is no highlight step to
    /// inspect).
    #[test]
    fn direction_presses_land_on_the_chip_drawn_on_that_side() {
        use legaia_engine_core::battle_input::{
            BattleCommand, BattleCommandInput, BattleCommandSession, CommandPhase, Resolution,
        };
        use legaia_engine_core::target_picker::SlotState;
        use legaia_engine_render::battle_command_ui as bcu;

        let party = [SlotState::alive(true, true); 3];
        let monsters = [
            SlotState::alive(true, true),
            SlotState::default(),
            SlotState::default(),
            SlotState::default(),
            SlotState::default(),
        ];
        type Dir = fn(&mut BattleCommandInput);
        type Axis = fn(&bcu::CommandSeat) -> i32;
        let dirs: [(Dir, Axis, bool); 4] = [
            (|e| e.up = true, |s| s.plate_origin().1, false),
            (|e| e.down = true, |s| s.plate_origin().1, true),
            (|e| e.left = true, |s| s.plate_origin().0, false),
            (|e| e.right = true, |s| s.plate_origin().0, true),
        ];
        for from in 0..BattleCommand::MENU.len() {
            for (dir, axis, want_max) in dirs {
                let mut s = BattleCommandSession::new(0, 0);
                s.phase = CommandPhase::Menu { cursor: from as u8 };
                let mut ev = BattleCommandInput::default();
                dir(&mut ev);
                s.input(ev, party, monsters);
                let committed = if s.attack_mode().is_some() {
                    BattleCommand::Attack
                } else {
                    match s.resolved() {
                        Some(Resolution::OpenItemMenu) => BattleCommand::Item,
                        Some(Resolution::OpenSpellMenu) => BattleCommand::Magic,
                        Some(Resolution::SpiritGuard) => BattleCommand::Spirit,
                        other => panic!("the press committed no ring arm: {other:?}"),
                    }
                };
                let to = BattleCommand::MENU
                    .iter()
                    .position(|c| *c == committed)
                    .expect("the committed arm is a ring arm");
                let landed = axis(&bcu::MENU_SEATS[to]);
                let extreme = bcu::MENU_SEATS
                    .iter()
                    .map(axis)
                    .reduce(|a, b| if want_max { a.max(b) } else { a.min(b) })
                    .unwrap();
                assert_eq!(
                    landed, extreme,
                    "from arm {from}, the press did not land on the outermost \
                     drawn chip along its axis (want_max={want_max})"
                );
            }
        }
    }

    /// The sibling half of the mirror check: the numeral fields. Every one is
    /// a right edge the field grows leftward from in 8-px cells, and the
    /// `engine-ui` literals have to name the same edges `battle_chrome` pins
    /// - a drift here is a four-digit HP drawn off the end of its panel.
    #[test]
    fn engine_ui_numeral_edges_mirror_the_packet_pinned_battle_chrome() {
        use legaia_engine_vm::battle_chrome as bc;
        let font = legaia_font::synthetic_for_tests();
        // Widest values every field is laid out against.
        let hud = hud_with_party_row(9999, 9999, 999, 999);
        let cells = |active: Option<u8>| -> Vec<(i32, i32)> {
            battle_hud_draws_for(
                &font,
                &BattleHudFrame {
                    slots: &battle_hud_slot_views(&hud),
                    solid_src: Some(SOLID),
                    surface: SURFACE,
                    chrome: Some(&BATTLE_MIRROR_RECTS),
                    active_slot: active,
                    ..Default::default()
                },
                BATTLE_HUD_PEN,
            )
            .sprites
            .iter()
            .filter(|s| s.src.1 == BATTLE_MIRROR_DIGITS.1 && s.src.2 == bc::DIGIT_W as u32)
            .map(|s| (s.dst.0 / STAGE_SCALE, s.dst.1 / STAGE_SCALE))
            .collect()
        };

        // Panel: the HP row's two four-cell runs, at the pinned right edges.
        let px = bc::panel_seats(1)[0] as i32;
        let panel = cells(None);
        for (right, digits, y) in [
            (bc::panel::CUR_RIGHT, 4, bc::panel::HP_DIGIT_Y),
            (bc::panel::MAX_RIGHT, 4, bc::panel::HP_DIGIT_Y),
            (bc::panel::CUR_RIGHT, 3, bc::panel::MP_DIGIT_Y),
            (bc::panel::MAX_RIGHT, 3, bc::panel::MP_DIGIT_Y),
        ] {
            let left = px + bc::digits_left_of(right, digits) as i32;
            let row = bc::PANEL_Y as i32 + y as i32;
            assert!(
                panel.contains(&(left, row)),
                "no numeral cell at the mirrored panel edge {right} ({digits} digits): {panel:?}"
            );
            assert!(
                panel
                    .iter()
                    .all(|(x, _)| x + bc::DIGIT_W as i32 <= px + bc::PANEL_BG.2 as i32),
                "a panel numeral runs past the 102-px plate: {panel:?}"
            );
        }

        // Bar: four cells per HP field, three per MP field.
        let bar = cells(Some(0));
        let y = bc::BAR_DIGIT_Y as i32;
        for (right, digits) in [
            (bc::BAR_HP_CUR_RIGHT, 4),
            (bc::BAR_HP_MAX_RIGHT, 4),
            (bc::BAR_MP_CUR_RIGHT, 3),
            (bc::BAR_MP_MAX_RIGHT, 3),
        ] {
            let left = bc::digits_left_of(right, digits) as i32;
            assert!(
                bar.contains(&(left, y)),
                "no numeral cell at the mirrored bar edge {right} ({digits} digits): {bar:?}"
            );
        }
    }

    /// Retail draws **no monster gauge at all**
    /// (`docs/subsystems/battle-action.md`), so a monster contributes nothing
    /// to the default surface - and everything it used to contribute has to
    /// still be reachable under `LEGAIA_DIAG_HUD`.
    #[test]
    fn monster_rows_are_diagnostic_only() {
        let mut hud = hud_with_party_row(100, 100, 0, 0);
        hud.sync_slot(
            3,
            SlotSyncInfo {
                name: "Goblin",
                is_party: false,
                alive: true,
                hp: 40,
                hp_max: 100,
                mp: 0,
                mp_max: 0,
                ap: None,
            },
        );
        let monster_row_y = BATTLE_HUD_PEN.1 + 3 * 14;
        assert!(
            !frame_draws(&hud, false)
                .text
                .iter()
                .any(|d| d.dst.1 == monster_row_y),
            "a monster row drew on the default surface"
        );
        assert!(
            frame_draws(&hud, true)
                .text
                .iter()
                .any(|d| d.dst.1 == monster_row_y),
            "the diagnostic surface lost the monster row"
        );
    }

    /// The retail readout-tint law has to reach the **surface**, not just
    /// exist in engine-ui: normal / caution / danger numerals must each take
    /// their own tier's colour.
    ///
    /// Expectations come from `gauge_fill_color`, retail's own law, rather
    /// than from literals. This test used to carry `[1.0, 0.95, 0.4, 1.0]`
    /// ("builder's yellow") and `[1.0, 0.4, 0.4, 1.0]` ("builder's red") -
    /// the port's pre-VRAM approximations - so once the colours were pinned
    /// off a retail frame it failed while asserting nothing retail does.
    /// What it always meant to protect is that the law reaches this host and
    /// separates the tiers; both survive, and neither is spelled here.
    #[test]
    fn native_battle_hud_hp_tints_span_the_retail_tiers() {
        let glyph_colors = |hp: u16| -> Vec<[f32; 4]> {
            let hud = hud_with_party_row(hp, 100, 0, 0);
            draws(&hud)
                .iter()
                .filter(|d| d.src != SOLID)
                .map(|d| d.color)
                .collect()
        };
        // Retail's tier ids: 7 normal, 6 caution, 9 danger.
        let caution = legaia_engine_render::gauge_fill_color(6);
        let danger = legaia_engine_render::gauge_fill_color(9);
        // A law whose tiers collapsed to one colour would satisfy every
        // "contains" below while drawing a single flat readout.
        assert!(
            caution != danger
                && caution != legaia_engine_render::READOUT_NORMAL
                && danger != legaia_engine_render::READOUT_NORMAL,
            "the three tiers must be visually distinct"
        );
        assert!(
            !glyph_colors(90)
                .iter()
                .any(|c| *c == caution || *c == danger),
            "normal tier numerals took a warning tint"
        );
        assert!(
            glyph_colors(40).contains(&caution),
            "caution tier numerals do not take the tier-6 colour"
        );
        assert!(
            glyph_colors(20).contains(&danger),
            "danger tier numerals do not take the tier-9 colour"
        );
    }

    /// The production path: `engine-ui`'s `party_panel_stage_x` (re-exported
    /// by `engine-render`, called by `battle_hud_draws_for`'s roster loop)
    /// reads the canonical `engine-vm` port of retail's `FUN_801D84C0`
    /// anchor table - `panel_anchors` - rather than mirroring it as
    /// literals. Assert the production function returns the kernel's values
    /// for every party size retail writes an anchor for, and that the seats
    /// the table leaves unwritten fall back to the packet-pinned panel seat
    /// plus the +5 name inset.
    #[test]
    fn panel_stage_x_production_path_returns_the_kernel_anchors() {
        use legaia_engine_vm::battle_party_panel::panel_anchors;
        for size in 1usize..=3 {
            let (primary, secondary) =
                panel_anchors(size as u8).expect("party sizes 1..=3 take a build arm");
            assert_eq!(
                legaia_engine_render::party_panel_stage_x(size, 0),
                i32::from(primary),
                "primary anchor for a party of {size}"
            );
            if let Some(sec) = secondary {
                assert_eq!(
                    legaia_engine_render::party_panel_stage_x(size, 1),
                    i32::from(sec),
                    "secondary anchor for a party of {size}"
                );
            }
        }
        // The seat retail writes no anchor for (a full party's third
        // panel): its packet-pinned seat plus the +5 name inset.
        assert_eq!(
            legaia_engine_render::party_panel_stage_x(3, 2),
            i32::from(legaia_engine_vm::battle_chrome::panel_seats(3)[2])
                + i32::from(legaia_engine_vm::battle_chrome::PANEL_TEXT_INSET),
            "unwritten third seat is not seat + inset"
        );
    }

    /// The end-to-end wiring: a live `World` battle state must reach the
    /// shared builder's draw list, MP included.
    ///
    /// This is the assertion that fails if `sync_battle_hud_rows` is dropped
    /// from the tick - the HUD model's slots stay `active == false`, the
    /// builder skips every empty-name row, and `draws` comes back empty.
    #[test]
    fn live_world_battle_state_reaches_the_shared_builder() {
        use legaia_engine_core::world::World;

        let mut world = World::new();
        world.party.party_count = 1;
        world.actors[0].active = true;
        world.actors[0].battle.liveness = 1;
        world.actors[0].battle.hp = 250;
        world.actors[0].battle.max_hp = 300;
        world.actors[0].battle.mp = 12;
        world.set_character_max_mp(0, 30);

        let mut hud = legaia_engine_core::battle_hud::BattleHud::new();
        super::super::battle::sync_battle_hud_rows(&mut hud, &world);
        assert!(hud.slots[0].active, "party slot 0 did not sync");
        assert_eq!(
            hud.slots[0].mp_max, 30,
            "MP ceiling did not reach the model"
        );

        let out = draws(&hud);
        assert!(!out.is_empty(), "synced battle state produced no draws");
        // The MP field only draws for a slot carrying a ceiling, so the live
        // world's MP has to reach the panel's pinned MP row.
        assert!(
            out.iter()
                .any(|d| d.src != SOLID && d.dst.1 == (164 + 34) * STAGE_SCALE),
            "live world state produced no MP field on the panel's MP row"
        );
    }

    /// Popups carry an absolute actor slot. The **diagnostic** readout
    /// anchors them by slice index, so the projection must keep inactive
    /// slots in place - a compacted list would put a monster's damage number
    /// on a party row. The default surface no longer draws them at all:
    /// retail's landed-hit numeral is seated over the struck actor, which
    /// only a host holding the camera can place, so it is the window's own
    /// `battle_value_readout_prims` (see `engine-ui::battle_numerals`).
    #[test]
    fn popup_anchors_track_absolute_actor_slot() {
        let mut hud = hud_with_party_row(100, 100, 0, 0);
        // Slots 1 and 2 stay empty; the monster occupies slot 3.
        hud.sync_slot(
            3,
            SlotSyncInfo {
                name: "Goblin",
                is_party: false,
                alive: true,
                hp: 40,
                hp_max: 100,
                mp: 0,
                mp_max: 0,
                ap: None,
            },
        );
        hud.push_popup(DamagePopup::damage(3, 25));
        let out = frame_draws(&hud, true).text;
        // Row stride is 14; monster slot 3's row sits at pen.y + 42, popups
        // 16 above (monster popups keep the index-anchored surface layout).
        let want_y = BATTLE_HUD_PEN.1 + 3 * 14 - 16;
        let popup_x = BATTLE_HUD_PEN.0 + 80;
        assert!(
            out.iter().any(|d| d.dst.1 == want_y && d.dst.0 >= popup_x),
            "no popup glyph at slot 3's anchor (y={want_y})"
        );
    }

    /// `engine-render`'s HUD tests repeat the badge block's atlas layout as
    /// literals, because that crate sits below `engine-core` and cannot
    /// import the bake. This is the seam that keeps the copy honest - the
    /// same job `engine_ui_command_chips_mirror_the_packet_pinned_battle_chrome`
    /// does for the chip cluster.
    #[test]
    fn badge_atlas_seats_match_the_bake() {
        use legaia_engine_core::save_menu_atlas as sma;
        for i in 0..sma::STATUS_BADGE_COUNT {
            assert_eq!(
                sma::status_badge_atlas_rect(i),
                (
                    48 * (i as u32 % 4),
                    128 + 16 * (i as u32 / 4),
                    sma::STATUS_BADGE_W,
                    sma::STATUS_BADGE_H
                ),
                "status badge {i} atlas seat drifted from the mirrored layout"
            );
        }
        for i in 0..sma::ELEMENT_BADGE_COUNT {
            assert_eq!(
                sma::element_badge_atlas_rect(i),
                (
                    20 * i as u32,
                    176,
                    sma::ELEMENT_BADGE_W,
                    sma::ELEMENT_BADGE_H
                ),
                "element badge {i} atlas seat drifted from the mirrored layout"
            );
        }
        // The badge block must not land on anything the atlas already
        // carries; these are the neighbours it was seated between.
        let (bx, by) = sma::ATLAS_RECT_STATUS_BADGES_ORIGIN;
        assert_eq!((bx, by), (0, 128));
        assert!(
            by >= 128 && by + 3 * sma::STATUS_BADGE_H <= sma::ATLAS_RECT_ELEMENT_BADGES_ORIGIN.1,
            "the status block overruns the element strip"
        );
        const {
            assert!(
                4 * sma::STATUS_BADGE_W <= 200,
                "the status block reaches the arts chip triple at x=200"
            )
        };
        assert!(
            sma::ATLAS_RECT_ELEMENT_BADGES_ORIGIN.1 + sma::ELEMENT_BADGE_H
                <= sma::ATLAS_RECT_FILIGREE.1,
            "the element strip overruns the filigree tile"
        );
    }
}
