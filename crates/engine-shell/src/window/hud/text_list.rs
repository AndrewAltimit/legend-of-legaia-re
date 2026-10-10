//! The HUD text list for one frame ([`PlayWindowApp::build_hud`]) and the
//! minigame status-row helpers it lays out through.

use super::*;

impl PlayWindowApp {
    /// One minigame status row (dance / fishing / slots / Baka Fighter) at
    /// its pen on the shared 320x240 stage, scaled onto the surface. The
    /// browser play page composes the same rows at the same pens
    /// (`PEN_STATUS` / `PEN_PROMPT` in `play_minigames.rs`) through the stage
    /// transform; this host used to lay them out in raw surface pixels, a
    /// third the size and pinned to the window corner instead of the stage.
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
        legaia_engine_render::scale_stage_text_draws(&mut d, stage_origin, stage_scale);
        d
    }

    /// The engine's minigame status rows (`engine-core::minigame_status`),
    /// through the shared draw kernel and scaled onto the surface through
    /// the one stage transform the page applies to the same rows.
    pub(super) fn stage_status_rows(
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
                    |id| match legaia_engine_core::music_labels::label_for_bgm_id(id) {
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
                    if self.dyn_shadows { "+shadows" } else { "" },
                    if self.dyn_shadows {
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
            let occl_str = if self.occlusion_fade {
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
        // `GO!` after READY (`FUN_801cf470` states 4 / 5), same either/or.
        if !self.session.host.world.minigames.dance_hud_art_staged
            && let Some(go) = self.session.host.world.minigames.dance_countin_go
        {
            let (stage_origin, stage_scale) = self.save_select_stage(w, h);
            out.extend(legaia_engine_render::ui_dance::dance_go_draws_for(
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
            out.extend(self.stage_status_row(&hint, (8, 80), dim, w, h));
            // The session affordances on their own row - with the reel hint
            // they overran the stage on one line. The browser play page
            // prints the same three on the same row with its own keys.
            out.extend(self.stage_status_row(
                "Triangle = menu, Start = quit, P = prizes",
                (8, 98),
                dim,
                w,
                h,
            ));

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

            let items = self.fishing_hud_items();
            // The sprite half (plates, digits, gauge caps, banners) is drawn
            // as screen primitives over the venue's HUD page when the sprite
            // table decoded (`fishing_hud_screen_prims`); this consumer then
            // keeps only the captions, the lure count and the gauge fills.
            let sprites_drawn = self.session.host.world.minigames.fishing_sprites.is_some();
            let hud_atlas = legaia_engine_render::FishingHudAtlas {
                solid_src: self.battle_hud_solid_src(),
                glyph_src: &|_| None,
                bar_thickness: 8,
                sprites_drawn,
            };
            // The lure row's captions off the disc when the overlay's text
            // resolved, the engine placeholders otherwise.
            let captions = match self.session.host.world.minigames.fishing_captions.as_ref() {
                Some(t) => legaia_engine_render::FishingCaptions::from_disc(
                    &t.lure_names,
                    &t.lures_left,
                    &t.suffix,
                )
                .with_fish_names(&t.species_names),
                None => legaia_engine_render::FishingCaptions::placeholder(),
            };
            let mut draws = legaia_engine_render::fishing_hud_draws_for(
                &self.font,
                &items,
                &captions,
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
        // Only while the machine itself is not on screen: once its art is
        // resident the cabinet's own marquee, lamps and coin readout carry
        // all of this.
        if self.session.host.world.mode == SceneMode::SlotMachine
            && self.slot_gpu.is_none()
            && let Some(m) = &self.session.host.world.minigames.slot_machine
        {
            out.extend(self.stage_status_rows(
                &legaia_engine_core::minigame_status::slot_status_rows(m),
                w,
                h,
            ));
        }
        // The machine's rules pages are its one text draw.
        if self.slot_gpu.is_some() {
            out.extend(self.slot_rules_text_draws(w, h));
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
        // Level-up + Seru-capture messages: retail's framed top-of-screen
        // banner outside battle, or the loose pens on a chrome-less run -
        // the shared frame decides (`legaia_engine_screens::banner_stage_draws`).
        // Stage pixels, scaled like every other stage-space text.
        if !screens.banner_texts.is_empty() {
            let (stage_origin, stage_scale) = self.save_select_stage(w, h);
            let mut rows = screens.banner_texts.clone();
            legaia_engine_render::scale_stage_text_draws(&mut rows, stage_origin, stage_scale);
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
            // Option-picker labels, through the builder the page shares.
            if let Some((px, py, _, _)) = lay.picker {
                draws.extend(legaia_engine_render::dialog_picker_label_draws_for(
                    &self.font,
                    &snap.options,
                    snap.cursor,
                    (px, py),
                    has_chrome,
                ));
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
}
