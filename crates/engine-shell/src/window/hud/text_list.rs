//! The HUD text list for one frame ([`PlayWindowApp::build_hud`]).
//!
//! The minigame status rows (`engine-minigame-scenes::minigame_status`) are **not** in
//! this list: they are caption data, shown in the title bar
//! (`PlayWindowApp::sync_minigame_caption_title`) where no game chrome can
//! cover them.

use super::*;
use legaia_app_theme::{chrome, spacing};

/// The window chrome's colours, from the palette the launcher and Studio
/// share (`legaia_app_theme::chrome`).
const HEADING: [f32; 4] = chrome::HEADING.to_f32();
const KEY: [f32; 4] = chrome::KEY.to_f32();
const TEXT: [f32; 4] = chrome::TEXT.to_f32();

impl PlayWindowApp {
    /// One line of this window's own text at a pen on the shared 320x240
    /// stage, scaled onto the surface through the stage transform.
    pub(super) fn stage_status_row(
        &self,
        text: &str,
        pen: (i32, i32),
        color: [f32; 4],
        w: u32,
        h: u32,
    ) -> Vec<TextDraw> {
        let mut d = text_draws_for(&self.font.layout_ascii(text), pen, color);
        let (stage_origin, stage_scale) = self.save_select_stage(w, h);
        legaia_engine_ui::scale_stage_text_draws(&mut d, stage_origin, stage_scale);
        d
    }

    /// The window's own chrome, drawn over everything else in the text
    /// layer: the "press Escape again" line while a first `Escape` waits for
    /// its confirmation (`handle_keyboard`), the last hotkey's notice line
    /// ("Ground fog: off"), and the hotkey help panel (`F12` / `?`). None of
    /// it is game surface - retail has no quit key and no host toggles.
    pub(in crate::window) fn window_chrome_draws(&self, w: u32, h: u32) -> Vec<TextDraw> {
        use crate::window::app_state::{NOTICE_DURATION, QUIT_CONFIRM_WINDOW};
        if self.font_atlas.is_none() {
            return Vec::new();
        }
        let mut stage: Vec<TextDraw> = Vec::new();
        let mut rows: Vec<(&str, [f32; 4])> = Vec::new();
        // The timed lines are a player's: a `--screenshot` capture leaves
        // them out unless `--screenshot-chrome` asks for them (a scripted key
        // press would otherwise put its notice in the frame).
        let timed = self.screenshot.as_ref().is_none_or(|s| s.chrome);
        if timed
            && self
                .quit_prompt
                .is_some_and(|t| t.elapsed() <= QUIT_CONFIRM_WINDOW)
        {
            rows.push(("Press Esc again to quit", HEADING));
        }
        if timed
            && let Some((text, at)) = &self.notice
            && at.elapsed() <= NOTICE_DURATION
        {
            rows.push((text.as_str(), KEY));
        }
        // The start-up pointer at the help panel, under the panel's own
        // gold; never with the panel up.
        if !self.hotkey_help && self.startup_hint.visible(std::time::Instant::now()) {
            rows.push((crate::window::app_state::STARTUP_HINT_TEXT, HEADING));
        }
        // Bottom-left of the stage, stacking upward: the top-left is the
        // field party readout's seat.
        let mut y = legaia_engine_ui::BOOT_UI_STAGE_H as i32 - spacing::CHROME_INSET;
        for (text, color) in rows.into_iter().rev() {
            let layout = self.font.layout_ascii(text);
            y -= layout.advance_y as i32 + spacing::CHROME_LINE_GAP;
            self.push_scrim(
                &mut stage,
                (6, y - 1),
                (layout.advance_x + 4, layout.advance_y + 2),
                0.7,
            );
            stage.extend(text_draws_for(&layout, (8, y), color));
        }
        if self.hotkey_help {
            stage.extend(self.hotkey_help_stage_draws());
        }
        let (stage_origin, stage_scale) = self.save_select_stage(w, h);
        legaia_engine_ui::scale_stage_text_draws(&mut stage, stage_origin, stage_scale);
        stage
    }

    /// A translucent dark plate behind window text (the font atlas's solid
    /// texel, tinted), so the line reads over a bright scene. Stage pixels.
    fn push_scrim(&self, out: &mut Vec<TextDraw>, at: (i32, i32), size: (u32, u32), alpha: f32) {
        if let Some(src) = self.battle_hud_solid_src() {
            out.push(TextDraw {
                dst: (at.0, at.1, size.0, size.1),
                src,
                color: [0.0, 0.0, 0.0, alpha],
            });
        }
    }

    /// The hotkey help panel: every host hotkey with its key and current
    /// state, read off the shared table so a new hotkey lists itself, laid
    /// out by [`help_panel_layout`]. Stage pixels.
    fn hotkey_help_stage_draws(&self) -> Vec<TextDraw> {
        use legaia_engine_session::hotkeys::{HOST_HOTKEYS, HostHotkey};
        let entries: Vec<(&str, String)> = HOST_HOTKEYS
            .iter()
            .map(|&(hk, _)| {
                let line = if hk == HostHotkey::CheatMaxAp {
                    hk.label().to_string()
                } else {
                    format!("{}: {}", hk.label(), self.hotkey_state(hk))
                };
                (hk.key_label(), line)
            })
            .collect();
        let l = help_panel_layout(&self.font, &entries);
        let mut out = Vec::new();
        self.push_scrim(
            &mut out,
            (l.x0 - 4, l.y0 - 4),
            ((l.width + 8) as u32, l.height as u32),
            0.85,
        );
        let mut put = |s: &str, at: (i32, i32), color: [f32; 4]| {
            out.extend(text_draws_for(&self.font.layout_ascii(s), at, color));
        };
        put("Keys", (l.x0, l.y0), HEADING);
        let mut y = l.y0 + l.line_h + 2;
        put("Esc", (l.x0, y), KEY);
        put(HELP_QUIT, (l.x0 + l.key_w, y), TEXT);
        put("F12", (l.x0 + l.col_w, y), KEY);
        put(HELP_SELF, (l.x0 + l.col_w + l.key_w, y), TEXT);
        y += l.line_h + 2;
        for ((k, line), &(col, row)) in entries.iter().zip(&l.cells) {
            let at = (l.x0 + col * l.col_w, y + row as i32 * l.line_h);
            put(k, at, KEY);
            put(line, (at.0 + l.key_w, at.1), TEXT);
        }
        y += l.rows as i32 * l.line_h + 2;
        put(HELP_FOOTER, (l.x0, y), HEADING);
        out
    }

    /// The HUD text list for one frame. `screens` is this frame's
    /// shop-family overlay ([`Self::shop_overlay_frame`]), built once by the
    /// caller so the chrome sprite pass reads the same composition.
    pub(in crate::window) fn build_hud(
        &self,
        w: u32,
        h: u32,
        screens: &legaia_engine_screens::ShopOverlayFrame,
    ) -> Vec<TextDraw> {
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
                    |id| match legaia_engine_system::music_labels::label_for_bgm_id(id) {
                        Some(label) => format!("  bgm {id}: {label}"),
                        None => format!("  bgm {id}"),
                    },
                )
                .unwrap_or_default();
            // Dynamic-lighting enhancement state (opt-in, non-retail; `I`
            // toggles; `Y` toggles the point-light/shadow sub-layer).
            let light_str = if self.dynamic_lighting {
                format!(
                    "  light {} {} (I/F8){}",
                    self.options_state.lighting_time_of_day,
                    if self.options_state.light_shadows {
                        "+shadows"
                    } else {
                        ""
                    },
                    if self.options_state.light_shadows {
                        ""
                    } else {
                        " shadows off (Y)"
                    }
                )
            } else {
                String::new()
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
            let occl_str = if self.options_state.occlusion_fade {
                ""
            } else {
                "  occl-fade off (F4)"
            };
            // Volumetric ground fog is default-on too (`F9` toggles).
            let fog_str = if self.options_state.volumetric_fog {
                ""
            } else {
                "  ground-fog off (F9)"
            };
            let line2 = format!(
                "t {:.1}s  {}{}{}{}{}{}{}  arrows=dpad Z=X drag=orbit",
                self.win.elapsed_secs(),
                audio_str,
                bgm_str,
                light_str,
                cam_str,
                precise_str,
                occl_str,
                fog_str
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
        // Dance minigame HUD. Withheld while the pre-song count-in is up -
        // the shared phase predicate answers that for both hosts. The score /
        // called-arrow status rows are the title bar's caption, not text here.
        if self.session.host.world.mode == SceneMode::Dance
            && self.session.host.world.minigames.dance_status_visible()
            && let Some(g) = &self.session.host.world.minigames.dance
        {
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
                legaia_engine_ui::scale_stage_text_draws(
                    &mut stage_draws,
                    stage_origin,
                    stage_scale,
                );
                out.extend(stage_draws);
                // The sprite-part layer: `FUN_801d387c`'s emit dispatch over
                // the run's own part pool (the sequence-clear banner + stars
                // the rules engine spawns), faded by its `+0x78` prologue.
                out.extend(legaia_engine_ui::minigame_fx::dance_sprite_part_draws(
                    &self.font,
                    &legaia_engine_screens::minigame_hud::dance_sprite_part_views(
                        &g.sprite_part_emits(),
                    ),
                    stage_origin,
                    stage_scale,
                ));
            }

            // Disco King tutorial captions (the how-to run) and the
            // pre-song count-in banner, both through the shared
            // `legaia_engine_ui::dance` builders the browser play page
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
                out.extend(legaia_engine_ui::dance::dance_tutorial_draws_for(
                    &self.font,
                    legaia_engine_ui::dance::DanceTutorialView {
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
        // the same `dance::dance_countin_prims` the browser play page
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
            out.extend(legaia_engine_ui::dance::dance_countin_draws_for(
                &self.font,
                legaia_engine_ui::dance::DanceCountInView {
                    x_offset: env.x_offset,
                    brightness: env.brightness,
                    hold: env.hold,
                },
                stage_origin,
                stage_scale,
            ));
        }
        // `GO!` after READY (`FUN_801cf470` states 4 / 5), same either/or.
        if !self.session.host.world.minigames.dance_hud_art_staged
            && let Some(go) = self.session.host.world.minigames.dance_countin_go
        {
            let (stage_origin, stage_scale) = self.save_select_stage(w, h);
            out.extend(legaia_engine_ui::dance::dance_go_draws_for(
                &self.font,
                go,
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
            out.extend(legaia_engine_ui::minigame_fx::fx_part_draws(
                &self.font,
                &legaia_engine_screens::minigame_hud::fx_part_views(
                    &self.session.host.world.minigames.fx,
                ),
                stage_origin,
                stage_scale,
            ));
        }
        // Fishing minigame HUD. The phase line, the reel hint and the session
        // affordances are the title bar's caption
        // (`minigame_status::fishing_status_rows`), not text here.
        if self.session.host.world.mode == SceneMode::Fishing
            && self.session.host.world.minigames.fishing.is_some()
        {
            // The overlay's developer readout (FUN_801d2050): the wander
            // actor's tile pair + settled height, shown only when the
            // dev-menu session (the engine's `_DAT_8007B9B0` print-flag
            // stand-in) is up AND the held pad carries the modifier bit -
            // the same two-sided gate retail applies.
            if let Some(wd) = &self.session.host.world.minigames.fishing_venue.wander {
                use legaia_engine_fishing::fishing_actors::{
                    debug_readout_visible, debug_tile, tracked_point_separation,
                };
                let held = (self.pad | self.gamepad_held).rotate_right(8);
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

            let items = self.fishing_hud_items();
            // The sprite half (plates, digits, gauge caps, banners) is drawn
            // as screen primitives over the venue's HUD page when the sprite
            // table decoded (`fishing_hud_screen_prims`); this consumer then
            // keeps only the captions, the lure count and the gauge fills.
            let sprites_drawn = self.session.host.world.minigames.fishing_sprites.is_some();
            let hud_atlas = legaia_engine_ui::FishingHudAtlas {
                solid_src: self.battle_hud_solid_src(),
                glyph_src: &|_| None,
                bar_thickness: 8,
                sprites_drawn,
            };
            // The lure row's captions off the disc when the overlay's text
            // resolved, the engine placeholders otherwise.
            let captions = match self.session.host.world.minigames.fishing_captions.as_ref() {
                Some(t) => legaia_engine_ui::FishingCaptions::from_disc(
                    &t.lure_names,
                    &t.lures_left,
                    &t.suffix,
                )
                .with_fish_names(&t.species_names),
                None => legaia_engine_ui::FishingCaptions::placeholder(),
            };
            let mut draws = legaia_engine_ui::fishing_hud_draws_for(
                &self.font,
                &items,
                &captions,
                &hud_atlas,
                (0, 0),
            );
            let (stage_origin, stage_scale) = self.save_select_stage(w, h);
            legaia_engine_ui::scale_stage_text_draws(&mut draws, stage_origin, stage_scale);
            out.extend(draws);
            // The venue's hub menu / help pages / tackle list (Triangle or
            // Select on the idle shore), laid out by the engine
            // (`World::fishing_hub_lines`) and drawn through the one
            // composition the browser play page uses.
            let hub = self.session.host.world.fishing_hub_lines();
            if !hub.is_empty() {
                let mut hub_draws = legaia_engine_ui::fishing::hub::fishing_hub_draws_for(
                    &self.font,
                    hub.iter()
                        .map(|l| (&l.text[..], i32::from(l.x), i32::from(l.y), l.marked)),
                    white,
                );
                legaia_engine_ui::scale_stage_text_draws(&mut hub_draws, stage_origin, stage_scale);
                out.extend(hub_draws);
            }
        }
        // Fishing point-exchange list: the venue's prize rows with the retail
        // gating (row 0 hidden until affordable, greyed unavailable rows,
        // one-time prizes latched after purchase), composed once for both
        // hosts (`legaia_engine_screens::fishing_exchange_draws`) on the
        // swaying venue panel. This host supplies its key legend; the pen is
        // a 320x240 stage position, so the rows scale onto the surface
        // through the same stage the HUD rows and the browser play page use.
        if self.session.host.world.mode == SceneMode::Fishing {
            let mut ex_draws = legaia_engine_screens::fishing_exchange_draws(
                &self.session.host.world,
                &self.font,
                "   (Enter = trade, Left/Right = venue, P = close)",
                white,
                dim,
            );
            let (stage_origin, stage_scale) = self.save_select_stage(w, h);
            legaia_engine_ui::scale_stage_text_draws(&mut ex_draws, stage_origin, stage_scale);
            out.extend(ex_draws);
        }
        // The slot machine's rules pages are its one text draw. Its status
        // rows (the fallback readout while the machine itself is not on
        // screen) are the title bar's caption.
        if self.slot_gpu.is_some() {
            out.extend(self.slot_rules_text_draws(w, h));
        }
        // Baka Fighter minigame HUD. The HP / round-wins readout and the
        // input prompt are the title bar's caption; what draws here is the
        // game's own chrome.
        if self.session.host.world.mode == SceneMode::BakaFighter
            && let Some(f) = &self.session.host.world.minigames.baka_fighter
        {
            // The tally's number drawers as font glyphs at their ported cell
            // layouts (the 8 px right-aligned score field and the 0x10 px
            // "GET COIN" strip), plus the one-glyph round digit on a frame
            // without the duel HUD's art: placement
            // `minigame_hud::baka_digit_placements`, glyph quads
            // `baka_strips` - both shared with the browser play page. With
            // the art resident the duel HUD's own STAGE cells draw in the
            // prim pass (`baka_hud_prims`).
            let placed = legaia_engine_screens::minigame_hud::baka_digit_placements(
                f,
                self.baka_hud_art_drawn(),
            );
            // The placements are 320x240 stage cells, like the rows above:
            // drawn raw they sat top-left at a fraction of the page's size.
            let mut cells = legaia_engine_ui::baka_strips::baka_digit_strip_draws_for(
                &self.font,
                &placed,
                legaia_engine_ui::text_lines::STATUS_ROW_DIM_INK,
            );
            let (stage_origin, stage_scale) = self.save_select_stage(w, h);
            legaia_engine_ui::scale_stage_text_draws(&mut cells, stage_origin, stage_scale);
            out.extend(cells);

            // The round chrome's resolved draws (`BakaChrome` - the intro
            // title, round banner and countdown timelines): each widget at
            // its stage position, faded by its brightness. A glyph draw
            // shows its paged cell index; the stamped cell rect
            // (`glyph_u`-paged `u` + the record's `v/w/h`) rides alongside
            // as the future atlas source.
            // Labels come from the shared kernels
            // (`baka_fighter_chrome::chrome_labels`, `baka_strips`), the
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
                    legaia_engine_ui::baka_strips::baka_widget_label_draws_for(
                        &self.font,
                        &legaia_engine_minigames::baka_fighter_chrome::chrome_labels(&draws),
                        [1.0; 4],
                    )
                };
                // The cabinet's own widgets (`FUN_801CF388`): the attract
                // prompt, the "PLAYER SELECT" banner and the "NEXT GAME /
                // PAY OUT" sheet, plus the sheet's pot numeral off the live
                // accumulator.
                use legaia_engine_minigames::baka_cabinet as bcab;
                if !quads {
                    cd.extend(legaia_engine_ui::baka_strips::baka_widget_label_draws_for(
                        &self.font,
                        &bcab::choice_sheet_labels(&cabinet_cells),
                        [1.0; 4],
                    ));
                }
                if sheet.is_some() {
                    cd.extend(legaia_engine_ui::baka_strips::baka_digit_strip_draws_for(
                        &self.font,
                        &bcab::choice_pot_placements(self.session.host.world.minigames.winnings),
                        [1.0; 4],
                    ));
                }
                legaia_engine_ui::scale_stage_text_draws(&mut cd, stage_origin, stage_scale);
                out.extend(cd);
            }
        }
        // The Muscle Dome's status rows (`minigame_status::muscle_status_rows`:
        // the Ra-Seru list stand-in and the between-turn prompts) are the
        // title bar's caption, not text here.
        // Shop / inn / prize / coin-counter overlay group (the shared
        // `legaia_engine_screens` frame) plus the field's own stage windows,
        // scaled through the one stage transform both hosts share.
        let mut stage = screens.stage_texts.clone();
        stage.extend(self.field_window_stage_draws());
        if !stage.is_empty() {
            // The shared kernel by its own name rather than through the
            // `save_select_stage` wrapper, so `check-ui-host-drift.py` can
            // pin this composition against the browser page's: this is the
            // only such call in `build_hud`, and losing it fails the gate.
            let (stage_origin, stage_scale) = legaia_engine_ui::pause_menu::stage_transform(w, h);
            legaia_engine_ui::scale_stage_text_draws(&mut stage, stage_origin, stage_scale);
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
        if !self.battle_event_log.is_empty() && legaia_engine_ui::diag_hud_enabled() {
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
        // Battle and Muscle Dome text: the HUD frame's text half, the
        // diagnostic encounter banner and whichever player-driven submenu
        // owns the frame (`battle_screens::battle_text_draws`, the browser
        // page's call too), in surface pixels; then the sparring tutorial's
        // rows and Koru's timed-fight strip, which sit in the 320x240 stage
        // and go through the stage transform the window chrome uses.
        out.extend(legaia_engine_screens::battle_screens::battle_text_draws(
            &self.battle_screen_inputs(),
            self.encounter_banner.as_ref().map(|(_, l)| l.as_str()),
            w,
            h,
        ));
        if self.session.host.world.mode == SceneMode::Battle {
            let (stage_origin, stage_scale) = self.save_select_stage(w, h);
            let mut draws = legaia_engine_screens::battle_screens::battle_tutorial_stage_draws(
                &self.session.host.world,
                &self.font,
                self.save_menu.is_some(),
            );
            legaia_engine_ui::scale_stage_text_draws(&mut draws, stage_origin, stage_scale);
            out.extend(draws);
        }
        // Level-up + Seru-capture messages: retail's framed top-of-screen
        // banner outside battle, or the loose pens on a chrome-less run -
        // the shared frame decides (`legaia_engine_screens::banner_stage_draws`).
        // Stage pixels, scaled like every other stage-space text.
        if !screens.banner_texts.is_empty() {
            let (stage_origin, stage_scale) = self.save_select_stage(w, h);
            let mut rows = screens.banner_texts.clone();
            legaia_engine_ui::scale_stage_text_draws(&mut rows, stage_origin, stage_scale);
            out.extend(rows);
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
            let mut draws = legaia_engine_screens::field_screens::cutscene_narration_stage_draws(
                &self.session.host.world,
                &self.font,
            );
            if !draws.is_empty() {
                let (stage_origin, stage_scale) = self.save_select_stage(w, h);
                legaia_engine_ui::scale_stage_text_draws(&mut draws, stage_origin, stage_scale);
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
            let mut draws = legaia_engine_ui::name_entry_draws_for(&self.font, &view);
            let (stage_origin, stage_scale) = self.save_select_stage(w, h);
            legaia_engine_ui::scale_stage_text_draws(&mut draws, stage_origin, stage_scale);
            out.extend(draws);
        }
        // The field message layer - balloon, map labels, tile-board prompt,
        // Incense notice, dialog reading box - composed once for both hosts
        // (`legaia_engine_screens::field_messages`); its chrome is the sprite
        // layer's (`dialog_chrome_sprite_draws`).
        {
            let mut draws = self.field_message_frame(w, h).stage_texts;
            let (stage_origin, stage_scale) = self.save_select_stage(w, h);
            legaia_engine_ui::scale_stage_text_draws(&mut draws, stage_origin, stage_scale);
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
            legaia_engine_ui::scale_stage_text_draws(&mut dev, stage_origin, stage_scale);
            out.extend(dev);
        }
        out
    }
}

/// The help panel's fixed rows.
const HELP_QUIT: &str = "Quit (press twice)";
const HELP_SELF: &str = "This help (also ?)";
const HELP_FOOTER: &str = "Pad keys: Options > Key Config";

/// Where the hotkey help panel's two columns go, in stage pixels.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::window) struct HelpPanelLayout {
    pub x0: i32,
    pub y0: i32,
    /// The panel's text extent (the scrim adds 4 px each side).
    pub width: i32,
    pub height: i32,
    pub line_h: i32,
    /// Offset of a row's text from its key.
    pub key_w: i32,
    /// Offset of the right column from the left.
    pub col_w: i32,
    /// Each entry's `(column, row)`, in entry order.
    pub cells: Vec<(i32, usize)>,
    /// Rows in the taller column.
    pub rows: usize,
}

/// Lay the hotkey help out on the 320x240 stage.
///
/// Two columns, each in table order. The table's own halves come first,
/// which is what a narrow font gets. When their widest rows land in opposite
/// columns and the pair is wider than the stage (the disc's dialog font,
/// with "Camera distance: farther" and "Free-angle movement: off" both in
/// the second half, ran the panel off the right edge), the widest entries
/// share one column instead - the most even such split that fits. A single
/// column is the last candidate.
pub(in crate::window) fn help_panel_layout(
    font: &legaia_font::Font,
    entries: &[(&str, String)],
) -> HelpPanelLayout {
    help_panel_layout_with(
        &|s: &str| font.layout_ascii(s).advance_x as i32,
        font.layout_ascii("Ag").advance_y as i32 + 1,
        entries,
    )
}

/// [`help_panel_layout`] over any text metric.
fn help_panel_layout_with(
    width: &dyn Fn(&str) -> i32,
    line_h: i32,
    entries: &[(&str, String)],
) -> HelpPanelLayout {
    let stage_w = legaia_engine_ui::BOOT_UI_STAGE_W as i32;
    let stage_h = legaia_engine_ui::BOOT_UI_STAGE_H as i32;
    let key_w = entries
        .iter()
        .map(|(k, _)| width(k))
        .chain([width("Esc"), width("F12")])
        .max()
        .unwrap_or(0)
        + 5;
    let n = entries.len();
    // A candidate is a right-column flag per entry.
    let candidate = |right: &[bool]| {
        let col = |want: bool, extra: &str| {
            entries
                .iter()
                .zip(right)
                .filter(|(_, r)| **r == want)
                .map(|((_, l), _)| width(l))
                .chain([width(extra)])
                .max()
                .unwrap_or(0)
                + key_w
        };
        let left_w = col(false, HELP_QUIT) + 10;
        let right_w = col(true, HELP_SELF);
        let in_right = right.iter().filter(|r| **r).count();
        let rows = in_right.max(n - in_right);
        let body = (left_w + right_w).max(width(HELP_FOOTER));
        let height = (rows as i32 + 3) * line_h + 12;
        (left_w, body, rows, height)
    };
    let even = n.div_ceil(2);
    let mut candidates: Vec<Vec<bool>> = vec![(0..n).map(|i| i >= even).collect()];
    // The `m` widest entries (ties to the earlier entry) in the right
    // column, most even first.
    let mut by_width: Vec<usize> = (0..n).collect();
    by_width.sort_by_key(|&i| (std::cmp::Reverse(width(&entries[i].1)), i));
    let mut ms: Vec<usize> = (1..n).collect();
    ms.sort_by_key(|&m| (m.abs_diff(n - m), std::cmp::Reverse(m)));
    for m in ms {
        let mut right = vec![false; n];
        for &i in &by_width[..m] {
            right[i] = true;
        }
        candidates.push(right);
    }
    // One column: `F12` still sits beside `Esc`.
    candidates.push(vec![false; n]);
    let scored: Vec<_> = candidates.iter().map(|c| (c, candidate(c))).collect();
    let fits = |c: &(i32, i32, usize, i32)| c.1 + 8 <= stage_w && c.3 + 4 <= stage_h;
    let (right, (col_w, body, rows, height)) = scored
        .iter()
        .find(|(_, c)| fits(c))
        .or_else(|| scored.iter().min_by_key(|(_, c)| c.1))
        .map(|(r, c)| (*r, *c))
        .expect("at least the even split");
    let mut next = [0usize; 2];
    let cells = right
        .iter()
        .map(|r| {
            let col = usize::from(*r);
            next[col] += 1;
            (col as i32, next[col] - 1)
        })
        .collect();
    HelpPanelLayout {
        x0: ((stage_w - body) / 2).max(4),
        y0: ((stage_h - height) / 2).max(4),
        width: body,
        height,
        line_h,
        key_w,
        col_w,
        cells,
        rows,
    }
}

#[cfg(test)]
mod help_panel_tests {
    use super::*;
    use legaia_engine_session::hotkeys::{HOST_HOTKEYS, HostHotkey};

    /// Every hotkey in its widest state.
    fn worst_entries() -> Vec<(&'static str, String)> {
        HOST_HOTKEYS
            .iter()
            .map(|&(hk, _)| {
                let state = match hk {
                    HostHotkey::AudioMute => "muted",
                    HostHotkey::TimeOfDay => "night",
                    HostHotkey::CameraDistance => "farther",
                    _ => "off",
                };
                let line = if hk == HostHotkey::CheatMaxAp {
                    hk.label().to_string()
                } else {
                    format!("{}: {state}", hk.label())
                };
                (hk.key_label(), line)
            })
            .collect()
    }

    fn assert_fits(width: &dyn Fn(&str) -> i32, line_h: i32, what: &str) -> HelpPanelLayout {
        let entries = worst_entries();
        let l = help_panel_layout_with(width, line_h, &entries);
        let stage_w = legaia_engine_ui::BOOT_UI_STAGE_W as i32;
        let stage_h = legaia_engine_ui::BOOT_UI_STAGE_H as i32;
        // The scrim reaches 4 px past the text on every side.
        assert!(
            l.x0 - 4 >= 0 && l.x0 + l.width + 4 <= stage_w,
            "{what}: the panel {l:?} leaves the {stage_w} px stage"
        );
        assert!(l.y0 - 4 >= 0 && l.y0 + l.height <= stage_h, "{what}: {l:?}");
        // Every row's text ends inside the panel.
        for ((_, line), &(col, _)) in entries.iter().zip(&l.cells) {
            let x = l.x0 + col * l.col_w + l.key_w;
            assert!(
                x + width(line) <= l.x0 + l.width,
                "{what}: {line:?} runs past the panel"
            );
        }
        l
    }

    /// Disc-free: a proportional metric at the dialog font's typical
    /// advance (6 px a character, 13 px a line - retail advances 4..10).
    /// The panel never leaves the stage; a narrow metric keeps the even
    /// split.
    #[test]
    fn help_panel_fits_the_stage_at_dialog_font_metrics() {
        assert_fits(&|s: &str| 6 * s.len() as i32, 13, "6 px metric");
        let l = assert_fits(&|s: &str| 4 * s.len() as i32, 13, "4 px metric");
        let even = HOST_HOTKEYS.len().div_ceil(2);
        assert!(
            l.cells
                .iter()
                .enumerate()
                .all(|(i, c)| c.0 == i32::from(i >= even))
        );
    }

    /// The disc's own dialog font, which the window letters the panel in
    /// once a disc is known.
    #[test]
    fn help_panel_fits_the_stage_in_the_disc_font() {
        let Some(bin) = std::env::var_os("LEGAIA_DISC_BIN") else {
            eprintln!("skip: LEGAIA_DISC_BIN unset");
            return;
        };
        let font = crate::launcher::disc_font(std::path::Path::new(&bin))
            .expect("the disc's dialog font loads");
        let l = assert_fits(
            &|s: &str| font.layout_ascii(s).advance_x as i32,
            font.layout_ascii("Ag").advance_y as i32 + 1,
            "disc font",
        );
        eprintln!("disc font help layout: {l:?}");
    }
}
