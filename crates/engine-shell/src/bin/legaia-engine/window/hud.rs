//! Extracted from `window.rs` (mechanical split; behavior-preserving).

use super::*;

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
        Sim::BeginMenu { cursor } => Ui::BeginMenu { cursor },
        Sim::Targeting => Ui::Targeting,
    }
}

impl PlayWindowApp {
    /// Keep the rendered dialog panel ([`Self::active_dialog`]) in sync with
    /// the world's pending dialog request.
    ///
    /// The world owns dismissal: the field VM's op-`0x4C` dialog-advance hook
    /// and the overworld talk-to handler both clear `World::current_dialog` on
    /// a confirm/cancel press. This method only mirrors that state into a
    /// visible, typed-out box - it opens a panel from the scene's MES the frame
    /// a request appears, ticks its typewriter reveal, and drops the panel the
    /// frame the world clears the request. It never clears `current_dialog`
    /// itself, so it can't race the world's dismiss.
    pub(super) fn sync_dialog_panel(&mut self) {
        // When the inline-script field-VM runner owns dialogue, it manages its
        // own box (rendered from `world.inline_dialogue`); don't also open the
        // simplified panel.
        if self.session.host.world.use_vm_dialogue {
            self.active_dialog = None;
            return;
        }
        if self.session.host.world.current_dialog.is_none() {
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
        let mut out = text_draws_for(&layout1, (8, 8), white);
        let audio_str = if self.session.audio.is_none() {
            "no audio"
        } else if self.options_state.muted {
            "audio MUTED (V)"
        } else {
            "audio on (V mutes)"
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
        // (`D` toggles) so a "why is the wall solid again" session sees it.
        let occl_str = if self.occlusion_fade {
            ""
        } else {
            "  occl-fade off (D)"
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
        if let Some(ctrl) = &self.session.host.world.world_map_ctrl {
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
        // Dance minigame HUD: the running score / groove gauge / active lane,
        // the arrow the current beat calls for, and the last press judgement.
        // The three arrows are the retail pad bits (Square/Circle/Triangle).
        if self.session.host.world.mode == SceneMode::Dance
            && let Some(g) = &self.session.host.world.dance
        {
            let arrow = match g.required_symbol() {
                Some(1) => "< (Square)",
                Some(2) => "> (Circle)",
                Some(3) => "^ (Triangle)",
                _ => "- (rest)",
            };
            use legaia_engine_core::dance::Judge;
            let judge = match self.session.host.world.dance_last_judge {
                Some(Judge::Sequence { .. }) => "SEQUENCE!",
                Some(Judge::Hit { .. }) => "HIT",
                Some(Judge::Miss) => "miss",
                None => "",
            };
            // The score readout goes through the retail number renderer's
            // decimal split, so leading zeros are blank slots and a score of
            // zero draws nothing at all - the overlay's `-1` sentinel.
            let score_digits: String = legaia_engine_core::dance::dance_number_digits(g.score())
                .iter()
                .map(|d| match d {
                    Some(v) => char::from(b'0' + v),
                    None => ' ',
                })
                .collect();
            let dl1 = format!(
                "DANCE  score {}  gauge {}  lane {}",
                score_digits.trim_start(),
                g.gauge(),
                g.lane()
            );
            let ly1 = self.font.layout_ascii(&dl1);
            out.extend(text_draws_for(&ly1, (8, 62), white));
            let dl2 = format!("press {arrow}   {judge}   (K = quit)");
            let ly2 = self.font.layout_ascii(&dl2);
            out.extend(text_draws_for(&ly2, (8, 80), dim));

            // The beat track. Two things the overlay's track renderer
            // (`FUN_801d2524`) computes, kept distinct here because they are
            // distinct in retail: the **displayed** combo slot uses its own
            // level-widened beat mask and its own narrow flash window, and is
            // NOT the judge's combo slot (`DanceGame::on_combo_slot`, mask 3
            // over the full acceptance window) - the judged cell is not the
            // displayed cell. And the notes scroll one 16-px cell per beat, so
            // note `i`'s pen slides left with the intra-beat fraction.
            use legaia_engine_core::dance::{
                GAUGE_STEP, dance_beat_track_note_x, dance_combo_window_bright,
            };
            let beat = g.beat_index();
            let frac = g.intra_beat_phase();
            let level = g.gauge() / GAUGE_STEP;
            let bright = dance_combo_window_bright(beat, level, frac);
            let track_label = if bright { "COMBO" } else { "beat " };
            let ly3 = self.font.layout_ascii(track_label);
            out.extend(text_draws_for(
                &ly3,
                (8, 98),
                if bright { white } else { dim },
            ));
            // The upcoming eight cells of the human's own chart row, drawn at
            // the ported scroll positions. The x base is this HUD's pen, not
            // the overlay's screen constant; the per-note offset is retail's.
            const TRACK_BASE_X: i32 = 60;
            if let Some(row) = g.chart_row(g.lane()) {
                for i in 0..8u32 {
                    let cell = row[((beat + i) % row.len() as u32) as usize];
                    let glyph = match cell {
                        1 => "<",
                        2 => ">",
                        3 => "^",
                        _ => ".",
                    };
                    let x = dance_beat_track_note_x(TRACK_BASE_X, i, frac);
                    let ly = self.font.layout_ascii(glyph);
                    out.extend(text_draws_for(
                        &ly,
                        (x, 98),
                        if i == 0 && !g.in_dead_zone() {
                            white
                        } else {
                            dim
                        },
                    ));
                }
            }

            // The retail-coordinate HUD frame: the HUD driver's per-frame
            // list (`DanceGame::hud_draws`, FUN_801d231c) laid out at its
            // 320x240 stage positions and upscaled with the stage transform.
            // The rival-HUD gate stands in for `_DAT_8007B6D0`: raised in the
            // two versus modes, so the rivals' score boxes, gauges and beat
            // tracks draw there and nowhere else.
            {
                use legaia_engine_core::dance::{DanceHudDraw, DanceMode};
                let rival_hud = matches!(g.mode(), DanceMode::Qualifier | DanceMode::Finals);
                let (stage_origin, stage_scale) = self.save_select_stage(w, h);
                let mut stage_draws: Vec<TextDraw> = Vec::new();
                for d in g.hud_draws(rival_hud) {
                    match d {
                        DanceHudDraw::Score { x, y, value, .. } => {
                            let digits: String =
                                legaia_engine_core::dance::dance_number_digits(value)
                                    .iter()
                                    .filter_map(|d| d.map(|v| char::from(b'0' + v)))
                                    .collect();
                            let ly = self.font.layout_ascii(&digits);
                            stage_draws.extend(text_draws_for(&ly, (x as i32, y as i32), white));
                        }
                        DanceHudDraw::ScoreBox { x, y } => {
                            // The frame itself is the quad layer's; a dim
                            // bracket marks its slot in the text layer.
                            let ly = self.font.layout_ascii("[");
                            stage_draws.extend(text_draws_for(&ly, (x as i32 - 12, y as i32), dim));
                        }
                        DanceHudDraw::Gauge { x, y, value, .. } => {
                            let lv = value / legaia_engine_core::dance::GAUGE_STEP;
                            let ly = self.font.layout_ascii(&format!("Lv.{lv}"));
                            stage_draws.extend(text_draws_for(&ly, (x as i32, y as i32), dim));
                        }
                        DanceHudDraw::BeatTrack { slot, x, y } => {
                            // The rival tracks draw their own lane's next
                            // cells at the retail anchor; the human's full
                            // track is the pen-space row above.
                            if slot == 0 {
                                continue;
                            }
                            if let Some(row) = g.chart_row(g.dancer_lane(slot)) {
                                let cells: String = (0..8u32)
                                    .map(|i| match row[((beat + i) % row.len() as u32) as usize] {
                                        1 => '<',
                                        2 => '>',
                                        3 => '^',
                                        _ => '.',
                                    })
                                    .collect();
                                let ly = self.font.layout_ascii(&cells);
                                stage_draws.extend(text_draws_for(&ly, (x as i32, y as i32), dim));
                            }
                        }
                    }
                }
                legaia_engine_render::scale_stage_text_draws(
                    &mut stage_draws,
                    stage_origin,
                    stage_scale,
                );
                out.extend(stage_draws);
                // The quad half of the same frame (the FUN_801d2f38 emits,
                // with the digit / gauge glyph-U patches applied): geometry +
                // gouraud colours are computed live; without the dance sprite
                // page resident there is no solid atlas source, so the sink
                // materialises nothing (same degradation as the fishing
                // gauge fills).
                let quads = g.hud_draw_quads(rival_hud);
                out.extend(minigame_fx::dance_quad_draws(
                    &quads,
                    None,
                    stage_origin,
                    stage_scale,
                ));
                // The sprite-part layer: `FUN_801d387c`'s emit dispatch over
                // the run's own part pool (the sequence-clear banner + stars
                // the rules engine spawns), faded by its `+0x78` prologue.
                out.extend(minigame_fx::dance_sprite_part_draws(
                    &g.sprite_part_emits(),
                    &self.font,
                    stage_origin,
                    stage_scale,
                ));
            }

            // Disco King tutorial captions (the how-to run): placeholder
            // text at the retail caption / option / cursor positions - the
            // line strings themselves are overlay rodata the port does not
            // read.
            if let Some(tf) = &self.dance_tutorial_frame {
                let (stage_origin, stage_scale) = self.save_select_stage(w, h);
                let mut tut_draws: Vec<TextDraw> = Vec::new();
                for (i, &(cx, cy)) in tf.captions.iter().enumerate() {
                    let ly = self
                        .font
                        .layout_ascii(&format!("(Disco King, line {})", i + 1));
                    tut_draws.extend(text_draws_for(&ly, (cx as i32, cy as i32), white));
                }
                if let Some(opts) = &tf.options {
                    for (label, &(ox, oy)) in ["Yes", "No thanks"].iter().zip(opts.iter()) {
                        let ly = self.font.layout_ascii(label);
                        tut_draws.extend(text_draws_for(&ly, (ox as i32, oy as i32), white));
                    }
                }
                if let Some((cx, cy)) = tf.cursor_pos {
                    let ly = self.font.layout_ascii(">");
                    tut_draws.extend(text_draws_for(&ly, (cx as i32, cy as i32), white));
                }
                if let Some(praise) = tf.feedback {
                    let ly = self.font.layout_ascii(if praise {
                        "(praise - right on the beat)"
                    } else {
                        "(scold - watch the timing)"
                    });
                    tut_draws.extend(text_draws_for(&ly, (8, 0x68), dim));
                }
                legaia_engine_render::scale_stage_text_draws(
                    &mut tut_draws,
                    stage_origin,
                    stage_scale,
                );
                out.extend(tut_draws);
            }
        }
        // Dance pre-song count-in banner (`1 2 3 READY... GO!`): the
        // envelope's two sliding halves / held centre drawn as placeholder
        // text at the retail x offsets, faded by its brightness ramp.
        if let Some(env) = &self.dance_countin_draw {
            let (stage_origin, stage_scale) = self.save_select_stage(w, h);
            let alpha = (env.brightness.clamp(0, 0xFF) as f32) / 255.0;
            let color = [1.0f32, 1.0, 1.0, alpha];
            let mut cd: Vec<TextDraw> = Vec::new();
            if env.hold {
                let ly = self.font.layout_ascii("READY... GO!");
                cd.extend(text_draws_for(&ly, (0xA0 - 40, 0x40), color));
            } else {
                let left = self.font.layout_ascii("READY");
                cd.extend(text_draws_for(
                    &left,
                    (0xA0 - env.x_offset - 40, 0x40),
                    color,
                ));
                let right = self.font.layout_ascii("GO!");
                cd.extend(text_draws_for(&right, (0xA0 + env.x_offset, 0x40), color));
            }
            legaia_engine_render::scale_stage_text_draws(&mut cd, stage_origin, stage_scale);
            out.extend(cd);
        }
        // The minigame effect pool's live parts (dance banner / stars,
        // fishing splash / ripples / celebration bursts), in stage space.
        {
            let (stage_origin, stage_scale) = self.save_select_stage(w, h);
            let mut fx = self.minigame_fx.stage_draws(&self.font);
            legaia_engine_render::scale_stage_text_draws(&mut fx, stage_origin, stage_scale);
            out.extend(fx);
        }
        // Fishing minigame HUD: the phase-specific line (cast-power bar while
        // casting; tension + strength while fighting; the catch result when
        // done) plus the running point total.
        if self.session.host.world.mode == SceneMode::Fishing
            && let Some(s) = &self.session.host.world.fishing
        {
            use legaia_engine_core::fishing::{FightOutcome, FishingPhase};
            let line = match s.phase() {
                FishingPhase::Casting => {
                    format!("FISHING  cast power {}  (Cross = cast)", s.cast_power())
                }
                FishingPhase::Fighting => {
                    let (tension, strength) = s
                        .fight()
                        .map(|f| (f.tension(), f.strength()))
                        .unwrap_or((0, 0));
                    format!(
                        "FISHING  tension {tension}/{}  strength {strength}  (hold Cross/Circle to reel)",
                        legaia_engine_core::fishing::TENSION_MAX
                    )
                }
                FishingPhase::Done => match s.last_outcome() {
                    Some(FightOutcome::Landed { points }) => {
                        format!("FISHING  landed! +{points} points  (Cross = recast)")
                    }
                    Some(FightOutcome::Snapped) => {
                        "FISHING  the line snapped!  (Cross = recast)".to_string()
                    }
                    _ => "FISHING  (Cross = recast)".to_string(),
                },
            };
            let ly = self.font.layout_ascii(&line);
            out.extend(text_draws_for(&ly, (8, 62), white));
            let ly2 = self.font.layout_ascii("(L = quit, P = prizes)");
            out.extend(text_draws_for(&ly2, (8, 80), dim));

            // The overlay's developer readout (FUN_801d2050): the wander
            // actor's tile pair + settled height, shown only when the
            // dev-menu session (the engine's `_DAT_8007B9B0` print-flag
            // stand-in) is up AND the held pad carries the modifier bit -
            // the same two-sided gate retail applies.
            if let Some(wd) = &self.fish_wander {
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
                    let ly = self.font.layout_ascii(&line);
                    out.extend(text_draws_for(&ly, (8, 116), dim));
                }
            }

            // The retail persistent HUD rows (best-catch, capped point total,
            // rod label, lures remaining) at their traced stage-pixel pens,
            // through the ported layout + its draw-list consumer. The rod
            // index comes from the retail ownership gate, which re-points a
            // stale selection at the next owned lure.
            use legaia_engine_core::fishing::{lure_item_id, select_owned_rod};
            let inventory = &self.session.host.world.inventory;
            let count_of = |id: u32| *inventory.get(&(id as u8)).unwrap_or(&0) as i32;
            let mut rod_index = 0;
            let has_rod = select_owned_rod(&mut rod_index, count_of);
            let mut items = legaia_engine_render::persistent_hud_draws(
                s.record().points,
                s.record().best_points,
                rod_index,
                if has_rod {
                    count_of(lure_item_id(rod_index))
                } else {
                    0
                },
            );
            // The catch HUD, drawn over the persistent rows while a cast is
            // out: the length / extent / cast-power readouts, plus the depth
            // and tension gauge block once the fish is on. `record` is the
            // fight's reel progress - the engine's analogue of the retail line
            // record the land gate compares. Two retail globals have no engine
            // analogue and stay zero: the cast line-projection term
            // (`DAT_801d9178`) and the line depth (`DAT_801d9298`), so the
            // extent readout reads 0 and the depth bar sits empty.
            let fight = s.fight();
            items.extend(legaia_engine_render::catch_hud_draws(
                &legaia_engine_render::CatchHudState {
                    record: fight.map(|f| f.progress()).unwrap_or(0),
                    line_extent: 0,
                    cast_power: s.cast_power(),
                    depth: 0,
                    tension: fight.map(|f| f.tension()).unwrap_or(0),
                    gauges_visible: s.phase() == FishingPhase::Fighting,
                },
            ));
            // This frame's live one-shot banners (hook / reel-in / miss /
            // auxiliary / strike splash), serviced in the redraw handler.
            items.extend(self.fishing_banner_draws.iter().copied());
            // No fishing sprite page is uploaded, so the glyph ids and the
            // gauge fills resolve to nothing; the number / caption rows are
            // font-atlas text and render as-is.
            let hud_atlas = legaia_engine_render::FishingHudAtlas {
                solid_src: None,
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
        }
        // Fishing point-exchange list: the venue's prize rows with the retail
        // gating (row 0 hidden until affordable, greyed unavailable rows,
        // one-time prizes latched after purchase).
        if self.session.host.world.mode == SceneMode::Fishing
            && let Some(ex) = &self.session.host.world.fishing_exchange
        {
            let world = &self.session.host.world;
            // The venue sub-screen's panel frame (FUN_801d74b0): the retail
            // menu-picker rect, centre-x converted to a left edge with the
            // two-left / six-down skin bias, swaying on the overlay's idle
            // sway triple (FUN_801d03b0). The list is anchored inside it.
            let sway = self.fishing_sway_offset;
            let panel = legaia_engine_core::fishing_chrome::centred_panel(0xA0, 0x50, 0x68, 0x50);
            let (px, py) = panel
                .map(|p| (p.x as i32 + sway.0 as i32, p.y as i32 + sway.1 as i32))
                .unwrap_or((8, 98));
            let venue_name = if ex.venue == 0 { "Buma" } else { "Vidna" };
            let head = format!(
                "PRIZE EXCHANGE ({venue_name})  points {}   (Enter = trade, Left/Right = venue, P = close)",
                world.fishing_points
            );
            let ly = self.font.layout_ascii(&head);
            out.extend(text_draws_for(&ly, (px, py), white));
            let first = ex.first_visible(world.fishing_points);
            for (i, r) in ex.rows.iter().enumerate().skip(first) {
                let owned = *world.inventory.get(&r.item_id).unwrap_or(&0) as u32;
                let avail = ex.is_available(
                    i,
                    world.fishing_points,
                    owned,
                    world.fishing_prizes_purchased,
                );
                let cursor = if i == ex.cursor { ">" } else { " " };
                let name = r
                    .name
                    .clone()
                    .unwrap_or_else(|| format!("item {:#04x}", r.item_id));
                // "sold" means the one-time bit is LATCHED, not "you cannot
                // afford it right now". `avail` folds three independent
                // refusals together (price, owned cap, latch), so reading it
                // as the latch printed "sold" beside every unaffordable
                // one-time prize on a fresh save - the row a player has never
                // seen reads as the row they already bought. Ask the latch on
                // its own by re-testing with the two other gates open.
                let sold = r.is_one_time()
                    && !ex.is_available(i, i32::MAX, 0, world.fishing_prizes_purchased);
                let tag = if r.is_one_time() {
                    if sold { "sold" } else { "one-time" }
                } else {
                    "each"
                };
                let line = format!(
                    "{cursor} {name:<18} {:>6} pts  {tag}  (own {owned})",
                    r.price
                );
                let ly = self.font.layout_ascii(&line);
                let y = py + 18 + 18 * (i - first) as i32;
                out.extend(text_draws_for(
                    &ly,
                    (px, y),
                    if avail { white } else { dim },
                ));
            }
        }
        // Slot-machine minigame HUD: the three payline symbols, the balance /
        // bet readout, and the phase-specific prompt.
        if self.session.host.world.mode == SceneMode::SlotMachine
            && let Some(m) = &self.session.host.world.slot_machine
        {
            use legaia_engine_core::slot_machine::SlotPhase;
            let reels = format!(
                "[{}] [{}] [{}]",
                m.payline_symbol(0),
                m.payline_symbol(1),
                m.payline_symbol(2)
            );
            let feature = match m.feature_mode() {
                6 => format!("  BONUS x{}", m.bonus_spins()),
                0 => String::new(),
                mode => format!("  feature {mode}"),
            };
            let sl1 = format!("SLOTS  {reels}  coins {}{feature}", m.balance());
            let ly1 = self.font.layout_ascii(&sl1);
            out.extend(text_draws_for(&ly1, (8, 62), white));
            let prompt = match m.phase() {
                SlotPhase::Idle => "Cross = spin (3 coins)".to_string(),
                SlotPhase::Spinning => "spinning...".to_string(),
                SlotPhase::Stopping => "Cross = stop reel".to_string(),
                SlotPhase::Payout => match m.last_result() {
                    Some(r) if r.payout > 0 => {
                        format!("WIN +{} coins!  (Cross = collect)", r.payout)
                    }
                    _ => "no win  (Cross = continue)".to_string(),
                },
                SlotPhase::CashedOut => "cashed out".to_string(),
            };
            let sl2 = format!("{prompt}   (O = cash out + quit)");
            let ly2 = self.font.layout_ascii(&sl2);
            out.extend(text_draws_for(&ly2, (8, 80), dim));
        }
        // Baka Fighter minigame HUD: HP bars as numbers, round pips, the
        // last-exchange readout, and the input prompt.
        if self.session.host.world.mode == SceneMode::BakaFighter
            && let Some(f) = &self.session.host.world.baka_fighter
        {
            use legaia_engine_core::baka_fighter::MatchPhase;
            let bl1 = format!(
                "BAKA  you {}hp (wins {})  vs  foe {}hp (wins {})  round {}",
                f.hp(0),
                f.round_wins(0),
                f.hp(1),
                f.round_wins(1),
                f.round() + 1
            );
            let ly1 = self.font.layout_ascii(&bl1);
            out.extend(text_draws_for(&ly1, (8, 62), white));
            let status = match f.phase() {
                MatchPhase::MatchOver(0) => {
                    format!(
                        "YOU WIN the match! +{} gold  (Cross/B = leave)",
                        f.gold_reward()
                    )
                }
                MatchPhase::MatchOver(_) => "you lose the match  (Cross/B = leave)".to_string(),
                MatchPhase::RoundOver(0) => "round won!".to_string(),
                MatchPhase::RoundOver(_) => "round lost".to_string(),
                MatchPhase::Fighting => match f.last_exchange() {
                    Some(r) => {
                        let who = if r.draw {
                            "trade".to_string()
                        } else if r.winner == 0 {
                            "you hit".to_string()
                        } else {
                            "foe hits".to_string()
                        };
                        let crit = if r.critical { " CRIT" } else { "" };
                        let sp = if r.special_round_win { " SPECIAL" } else { "" };
                        format!("{who} {}{crit}{sp}", r.damage)
                    }
                    None => "choose your attack".to_string(),
                },
            };
            let bl2 = format!("{status}   Left/Right/Up attack, Down special (B = quit)");
            let ly2 = self.font.layout_ascii(&bl2);
            out.extend(text_draws_for(&ly2, (8, 80), dim));

            // The duel's three number drawers, at their ported cell layouts:
            // the one-glyph round digit, the 8 px right-aligned score field,
            // and the 0x10 px "GET COIN" numeral strip for the prize. The HUD
            // widget descriptors these cells patch (`DAT_801d7160`) index a
            // sprite page the engine does not upload, so each cell is drawn as
            // a font glyph at its ported x offset instead of as a textured
            // quad - the layout is retail's, the glyph source is not.
            use legaia_engine_core::baka_fighter::{
                DigitCell, coin_digit_cells, right_aligned_number_cells, single_digit_cell,
            };
            let mut cell_row = |cells: &[DigitCell], base_x: i32, y: i32| {
                for c in cells {
                    let s = [b'0' + c.digit.min(9)];
                    let text = core::str::from_utf8(&s).unwrap_or("0");
                    let ly = self.font.layout_ascii(text);
                    out.extend(text_draws_for(&ly, (base_x + c.x_offset as i32, y), dim));
                }
            };
            cell_row(&[single_digit_cell((f.round() + 1).min(9) as u8)], 8, 98);
            if let Some(t) = f.tally() {
                cell_row(&right_aligned_number_cells(t.total()), 40, 98);
                cell_row(&coin_digit_cells(t.gold_remaining()), 140, 98);
            }

            // The round chrome's resolved draws (`BakaChrome` - the intro
            // title, round banner and countdown timelines): each widget at
            // its stage position, faded by its brightness. A glyph draw
            // shows its paged cell index; the stamped cell rect
            // (`glyph_u`-paged `u` + the record's `v/w/h`) rides alongside
            // as the future atlas source.
            if !self.baka_chrome_frame.is_empty() {
                let (stage_origin, stage_scale) = self.save_select_stage(w, h);
                let mut cd: Vec<TextDraw> = Vec::new();
                for (d, _cell) in &self.baka_chrome_frame {
                    let alpha = (d.brightness.clamp(0, 0xFF) as f32) / 255.0;
                    let color = [1.0f32, 1.0, 1.0, alpha];
                    let label = match d.glyph {
                        Some(idx) => format!("{}", idx.rem_euclid(10)),
                        None => format!("w{:02x}", d.widget),
                    };
                    let ly = self.font.layout_ascii(&label);
                    cd.extend(text_draws_for(&ly, (d.x as i32, d.y as i32), color));
                }
                legaia_engine_render::scale_stage_text_draws(&mut cd, stage_origin, stage_scale);
                out.extend(cd);
            }
        }
        // Muscle Dome HUD, both lines the host's own.
        //
        // The retail "Turns Left / HP Left" strip is deliberately NOT drawn
        // here: its draw sites gate on formation slot 0 == 0xB6 (Koru), and
        // the dome ladder tops out at 0xAA, so no dome round can ever raise
        // it. A dome leg is an unbounded battle, so line 1 reports the turn
        // reached rather than a countdown to a limit that does not exist.
        if self.session.host.world.mode == SceneMode::MuscleDome
            && let Some(s) = &self.session.host.world.muscle_dome
        {
            use legaia_engine_core::muscle_dome::MusclePhase;
            // Line 0 is the *contest*: which leg of which course this is and
            // what the run has banked. A leg pays nothing; the contest pays
            // coins, so the tally is the number that matters.
            if let Some(c) = &self.session.host.world.muscle_contest {
                let flags = self.session.host.world.muscle_contest_flags();
                let ml0 = format!(
                    "Course {}  Round {}/{}   Coins banked: {}",
                    c.course() + 1,
                    c.round() + 1,
                    c.staged_course_length(&flags),
                    c.tally(),
                );
                let ly0 = self.font.layout_ascii(&ml0);
                out.extend(text_draws_for(&ly0, (8, 44), white));
            }
            let ml1 = format!("      Turn: {}         HP Left: {}", s.turn(), s.hp_left());
            let ly1 = self.font.layout_ascii(&ml1);
            out.extend(text_draws_for(&ly1, (8, 62), white));
            let status = match s.phase() {
                MusclePhase::Select => {
                    let h = s.hand(0);
                    format!(
                        "AP L:{} R:{} U:{} D:{}  budget {}  entered {}  (Cross = fight)",
                        h[0].cost,
                        h[1].cost,
                        h[2].cost,
                        h[3].cost,
                        s.budget(0),
                        s.queue(0).len()
                    )
                }
                MusclePhase::Resolve => "resolving...".to_string(),
                MusclePhase::TurnOver => {
                    let [taken, dealt] = s.last_turn_damage();
                    format!("turn: dealt {dealt}, took {taken}  (Cross = next turn)")
                }
                // The caption names a spell; it awards nothing. The contest's
                // payout lands when the ladder settles.
                MusclePhase::Won => format!(
                    "LEG WON! caption spell {:#x}  (Cross/M = next leg)",
                    s.reward_spell_id()
                ),
                MusclePhase::Lost => "you lose the leg  (Cross/M = leave)".to_string(),
            };
            let ml2 = format!(
                "{status}   you {}hp  foe {}hp  time {}/{}   (M = quit)",
                s.hp(0),
                s.hp(1),
                s.time_meter(),
                legaia_engine_core::muscle_dome::TIME_METER_MAX,
            );
            let ly2 = self.font.layout_ascii(&ml2);
            out.extend(text_draws_for(&ly2, (8, 80), dim));
        }
        // Shop / inn overlay: rendered at the bottom of the screen when the menu
        // runtime is in any shop, inn, or confirmation state.
        if self.menu_runtime.is_open() {
            let label = self.menu_runtime.current_label();
            if let Some(shop) = &self.menu_runtime.shop_session {
                let state = MenuState::from_byte(self.menu_runtime.ctx_state());
                let cursor = self.menu_runtime.cursor() as usize;
                let gold = self.session.host.world.money;
                // The seru-trade screens carry dynamic, owned-string labels, so
                // render them directly (the generic `(title, rows)` path below
                // only handles `'static` labels).
                let trade_state = matches!(
                    state,
                    Some(MenuState::ShopTrade) | Some(MenuState::ShopTradeConfirm)
                );
                if trade_state {
                    self.draw_shop_trade(&mut out, state, cursor);
                }
                // Row labels are owned so item names can be resolved from the
                // disc item table; the ink is the retail `_DAT_8007B454` pen
                // from the menu-overlay window kernels.
                let bag = MenuRuntime::inventory_items(&self.session.host.world);
                let item_label = |id: u8| -> String {
                    self.session
                        .host
                        .world
                        .menu_text
                        .as_ref()
                        .and_then(|t| t.item_name(id))
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| format!("item {id:02}"))
                };
                let held_of = |id: u8| -> i16 {
                    bag.iter()
                        .find(|(i, _)| *i == id)
                        .map(|(_, q)| *q as i16)
                        .unwrap_or(0)
                };
                let (title, rows_spec, show_gold): (_, Vec<(String, Option<u32>, u8)>, _) =
                    match state {
                        _ if trade_state => (label, Vec::new(), None),
                        // Top picker: Buy / Sell / (Trade) / Exit, matching the
                        // runtime's dynamic row layout. The Sell row's ink is
                        // retail's bag-scan verdict.
                        Some(MenuState::ShopMenu) => {
                            let ink = legaia_engine_core::shop::shop_root_command_rows(
                                (0, 0),
                                0x4000,
                                !bag.is_empty(),
                            );
                            let rows = legaia_engine_core::menu_runtime::shop_menu_rows(
                                self.session.host.world.seru_trade_enabled(),
                            )
                            .iter()
                            .map(|s| {
                                let (l, i) = match s {
                                    MenuState::ShopBuy => ("Buy", ink[0].ink),
                                    MenuState::ShopSell => ("Sell", ink[1].ink),
                                    MenuState::ShopTrade => ("Trade Seru", ink[0].ink),
                                    _ => ("Exit", ink[0].ink),
                                };
                                (l.to_string(), None, i)
                            })
                            .collect();
                            (label, rows, Some(gold))
                        }
                        Some(MenuState::ShopBuy) => {
                            let rows = shop
                                .inventory
                                .items
                                .iter()
                                .map(|item| {
                                    let ink = legaia_engine_core::shop::shop_stock_row_ink(
                                        held_of(item.item_id),
                                        0,
                                        gold,
                                        item.price as i32,
                                    );
                                    (item_label(item.item_id), Some(item.price), ink)
                                })
                                .collect();
                            (label, rows, Some(gold))
                        }
                        Some(MenuState::ShopSell) => {
                            let rows = bag
                                .iter()
                                .map(|(id, qty)| {
                                    (
                                        format!("{} x{qty}", item_label(*id)),
                                        None,
                                        legaia_engine_render::SHOP_INK_NORMAL,
                                    )
                                })
                                .collect();
                            (label, rows, Some(gold))
                        }
                        Some(MenuState::ShopQuantity) => {
                            let rows = (1u32..=9)
                                .map(|n| {
                                    (n.to_string(), None, legaia_engine_render::SHOP_INK_NORMAL)
                                })
                                .collect();
                            (label, rows, None)
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
                out.extend(retail_windows);
                // The equipment-buy recipient flow's windows (36 / 25 / 41)
                // ride over the parked buy list while the picker owns the
                // pad - the same compositing order the browser play page
                // uses in `play_overlay_draws_json`.
                out.extend(self.recipient_window_draws());
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
                    out.extend(shop_draws);
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
                let gold = self.session.host.world.money;
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
                        out.extend(inn_draws);
                    }
                    Some(MenuState::InnSleep) => {
                        let layout = self.font.layout_ascii("Resting...");
                        out.extend(text_draws_for(&layout, SHOP_OVERLAY_PEN, white));
                    }
                    _ => {
                        let menu_label = format!("[{}]", label);
                        let ml_layout = self.font.layout_ascii(&menu_label);
                        out.extend(text_draws_for(&ml_layout, SHOP_OVERLAY_PEN, white));
                    }
                }
            } else {
                // Non-shop, non-inn menu: show current mode label.
                let menu_label = format!("[{}]", label);
                let ml_layout = self.font.layout_ascii(&menu_label);
                out.extend(text_draws_for(&ml_layout, SHOP_OVERLAY_PEN, white));
            }
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
            let dialogue_up = bw.current_dialog.is_some() || bw.inline_dialogue.is_some();
            if dialogue_up {
                // Dialogue box up: no menu chrome.
            } else if let Some(view) = bw.arts_input_view() {
                // Retail-model arts entry: the screen is baked art (drawn
                // in the sprite layer by `arts_input_chrome_sprite_draws`),
                // so the only text is the Begin | Reselect pick.
                use legaia_engine_render::arts_input as ai;
                let (origin, scale) = self.save_select_stage(w, h);
                out.extend(ai::arts_input_text_draws(
                    &self.font,
                    &ai::ArtsInputFrame {
                        buffer: view.buffer,
                        spent: view.spent,
                        pool: view.pool,
                        pool_max: view.pool_max,
                        plate_value: view.plate_value,
                        list_page: view.list_page,
                        phase: arts_input_screen(view.phase),
                    },
                    origin,
                    scale,
                ));
            } else if let Some(arts) = &bw.battle_arts_menu {
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
            } else if let Some(spell) = &bw.battle_spell_menu {
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
            } else if bw.battle_item_menu.is_some() {
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
            } else if let Some(cmd) = &bw.battle_command {
                let menu_x = 8i32;
                let mut my = 210i32;
                match &cmd.phase {
                    CommandPhase::RoundPrompt { .. }
                    | CommandPhase::Menu { .. }
                    | CommandPhase::AttackMode { .. } => {
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
                            out.extend(bcu::battle_command_chip_text(
                                &self.font,
                                &bcu::BattleCommandMenuFrame {
                                    chips: &chips,
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
            if let Some(rect) = self.battle_tutorial_stage_rect() {
                let tbox = bw.battle_tutorial_box().expect("rect implies a box");
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
                let (stage_origin, stage_scale) = self.save_select_stage(w, h);
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
                if let Some(banner) = &self.session.host.world.current_level_up_banner {
                    out.extend(level_up_draws_for(
                        &self.font,
                        banner.char_id,
                        banner.new_level,
                        banner.hp_gained,
                        banner.mp_gained,
                        LEVEL_UP_BANNER_PEN,
                    ));
                }
                if let Some(banner) = &self.session.host.world.current_capture_banner
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
        // (`FUN_80037174`) - every visible line drawn centered at its
        // current window Y, scrolling upward. Line Ys are PSX-framebuffer
        // space (240 lines); scale into the surface. Pixel-pinned from the
        // cold-boot retail capture (multi-line, 0.5 px/frame; the earlier
        // one-caption-at-a-time reading measured the separate `4C E1`
        // balloon, not this crawl).
        if let Some(narration) = &self.session.host.world.cutscene_narration {
            let white = [1.0f32, 1.0, 1.0, 1.0];
            let center_x = (w / 2) as i32;
            let scale = h as f32 / 240.0;
            for line in narration.visible_lines() {
                let y = (line.y as f32 * scale) as i32;
                if y < 0 || y > h as i32 - 8 {
                    continue;
                }
                out.extend(legaia_engine_render::cutscene_narration_draws_for(
                    &self.font, line.text, center_x, y, white,
                ));
            }
        }
        // Opening-cutscene static title card (`map01`'s "twilight of
        // humanity" beat): the pages shown together, centered, at the
        // capture-pinned band y=92..130.
        if let Some(card) = &self.session.host.world.cutscene_card {
            let white = [1.0f32, 1.0, 1.0, 1.0];
            let center_x = (w / 2) as i32;
            let scale = h as f32 / 240.0;
            for (i, text) in card.iter().enumerate() {
                let y = ((92 + 16 * i as i32) as f32 * scale) as i32;
                out.extend(legaia_engine_render::cutscene_narration_draws_for(
                    &self.font, text, center_x, y, white,
                ));
            }
        }
        // Name-entry overlay: the opening `town01` lead-character naming
        // prompt, laid out in stage pixels at the retail-traced geometry
        // and upscaled with the same stage transform the window chrome
        // uses (`name_entry_chrome_sprite_draws`) so text and frames stay
        // locked together.
        if let Some(entry) = &self.session.host.world.name_entry {
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
            let mut draws: Vec<TextDraw> = Vec::new();
            let (bx, by, _, _) = lay.main;
            // Main text: one row per 0x7C-separated line at the retail
            // 15-px pitch. The pager draws each reading-box line at the
            // box origin exactly - `FUN_80036888(line, 0, 0, ctx+0x12,
            // ctx+0x14 + i*0xF)` - with the string ink staged CLUT 7
            // (`_DAT_8007B454 = 7` before every line), the (206,206,206)
            // menu white.
            for (i, line) in snap.page.split('|').enumerate() {
                let row_layout = self.font.layout_ascii(line);
                let pen = (bx, by + i as i32 * 0xF);
                draws.extend(text_draws_for(
                    &row_layout,
                    pen,
                    legaia_engine_render::MENU_TEXT_WHITE,
                ));
            }
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
        // Opt-in developer menu: its row list draws over everything else.
        out.extend(self.dev_menu_draws.iter().copied());
        out
    }

    /// Snapshot the live dialog source (simplified panel, cutscene
    /// timeline, or inline field-VM runner) into plain strings the
    /// text and chrome layers both consume. `None` when no box is
    /// open this frame.
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
                options,
                cursor,
                // The advance hand shows at a page break AND on the final
                // fully-typed page (retail waits for a confirm on both).
                waiting: panel.is_waiting_for_input() || panel.is_done(),
            })
        };
        if let Some(panel) = self.active_dialog.as_ref() {
            return from_panel(panel, false);
        }
        if let Some(panel) = self
            .session
            .host
            .world
            .cutscene_timeline
            .as_ref()
            .and_then(|tl| tl.dialog.as_ref())
            && let Some(snap) = from_panel(panel, true)
        {
            return Some(snap);
        }
        if let Some(id) = self.session.host.world.inline_dialogue.as_ref()
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
    /// - Picker box: `x = 0x26`, `y = 0x94 + ((4-n)*0xF)/2`,
    ///   `w = 0xF4`, `h = 0x38 - (4-n)*0xF` (the picker-init arms'
    ///   literal geometry writes).
    ///
    /// Rects are the retail centre rects; the border skin the chrome
    /// pass draws extends ~8 px beyond them on every side
    /// (`dialog_window_chrome_draws_for`).
    pub(super) fn dialog_stage_layout(snap: &DialogSnapshot) -> DialogStageLayout {
        // Retail's standard reading box is ALWAYS 3 rows tall
        // (`_DAT_801F2740 = 3` in both box-init arms) regardless of how
        // much text has typed in; only over-long simplified pages grow
        // it to a 4th row.
        let lines = snap.page.split('|').count().clamp(3, 4) as i32;
        let main_w = 0xF4;
        let main_h = lines * 0xF - 3;
        let picker = if snap.options.is_empty() {
            None
        } else {
            let n = snap.options.len().clamp(2, 4) as i32;
            Some((0x26, 0x94 + ((4 - n) * 0xF) / 2, 0xF4, 0x38 - (4 - n) * 0xF))
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
        let Some(view) = self.session.host.world.arts_input_view() else {
            return Vec::new();
        };
        let (stage_origin, stage_scale) = self.save_select_stage(surface_w, surface_h);
        let frame = ai::ArtsInputFrame {
            buffer: view.buffer,
            spent: view.spent,
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
        let Some(snap) = self.dialog_snapshot() else {
            return Vec::new();
        };
        let lay = Self::dialog_stage_layout(&snap);
        let (stage_origin, stage_scale) = self.save_select_stage(surface_w, surface_h);
        let mut out = legaia_engine_render::dialog_window_chrome_draws_for(
            &assets.rects,
            lay.main,
            stage_origin,
            stage_scale,
        );
        if let Some(prect) = lay.picker {
            out.extend(legaia_engine_render::dialog_window_chrome_draws_for(
                &assets.rects,
                prect,
                stage_origin,
                stage_scale,
            ));
            // Pointing-hand cursor on the selected option row
            // (FUN_8002B994 kind 0 at box_x-6, box_y + cursor*0xF).
            out.push(legaia_engine_render::dialog_option_hand_sprite(
                &assets.rects,
                (prect.0, prect.1),
                snap.cursor,
                stage_origin,
                stage_scale,
            ));
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
        let Some(rect) = self.battle_tutorial_stage_rect() else {
            return Vec::new();
        };
        let waits = self
            .session
            .host
            .world
            .battle_tutorial_box()
            .is_some_and(|b| b.waits_for_input);
        let (stage_origin, stage_scale) = self.save_select_stage(surface_w, surface_h);
        legaia_engine_render::battle_tutorial_chrome_draws_for(
            &assets.rects,
            rect,
            waits,
            stage_origin,
            stage_scale,
        )
    }

    /// Project the live name-entry session into the renderer-agnostic view
    /// the engine-ui builders consume (grid vs control cursor split via the
    /// session's own control mapping).
    pub(super) fn name_entry_view<'a>(
        &self,
        entry: &'a legaia_engine_core::name_entry::NameEntry,
    ) -> legaia_engine_render::NameEntryView<'a> {
        use legaia_engine_core::name_entry::{CHAR_CELLS, Control, GRID, GRID_COLS};
        let (grid_cursor, control_cursor) = if entry.cursor < CHAR_CELLS {
            (
                Some((entry.cursor / GRID_COLS, entry.cursor % GRID_COLS)),
                None,
            )
        } else {
            let idx = match entry.control_at(entry.cursor) {
                Some(Control::Backspace) => Some(0),
                Some(Control::Default) => Some(1),
                Some(Control::End) => Some(2),
                None => None,
            };
            (None, idx)
        };
        legaia_engine_render::NameEntryView {
            grid_rows: &GRID,
            name: &entry.name,
            default_name: &entry.default_name,
            grid_cursor,
            control_cursor,
            confirming: entry.state == legaia_engine_core::name_entry::NameEntryState::Confirm,
            confirm_yes: entry.confirm_yes,
            // Retail blinks the caret at 75% duty from the frame counter's
            // `& 0x18` bits.
            caret_on: (self.session.host.world.frame & 0x18) != 0,
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
        let Some(entry) = self.session.host.world.name_entry.as_ref() else {
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
    /// Decoded option labels when a picker menu is open (empty
    /// otherwise).
    pub options: Vec<String>,
    /// Selected option row.
    pub cursor: usize,
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
        let slots = battle_hud_slot_views(&self.battle_hud);
        let popups = battle_hud_popup_views(&self.battle_hud);
        let w_ref = &self.session.host.world;
        // The arts-input session owns both halves of the park: it names the
        // actor whose full-width bar shows, and its being open is what sends
        // the roster panels off-screen.
        let active = w_ref
            .arts_input_actor()
            .and_then(|slot| {
                legaia_engine_core::battle_hud::battle_active_actor(w_ref)
                    .map(|(_, name)| (slot, name))
            })
            .or_else(|| legaia_engine_core::battle_hud::battle_active_actor(w_ref));
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
                plaque: active
                    .as_ref()
                    .filter(|_| w_ref.battle_item_menu.is_none())
                    .map(|(_, n)| n.as_str()),
                plaque_badge: legaia_engine_core::battle_hud::battle_plaque_element_badge(w_ref),
                banner: banner.as_deref(),
                // The sparring-tutorial prompt is a box the host draws
                // itself, and its rect starts on the plaque's own content
                // pen - so while it is up the plaque must not draw, or the
                // two text runs land on the same pixels.
                plaque_seat_taken: self.battle_tutorial_stage_rect().is_some()
                    || w_ref.current_dialog.is_some()
                    || w_ref.inline_dialogue.is_some(),
                badges: badges.as_ref(),
                // The same box, tested against the party surfaces' own rows:
                // a bottom-anchored prompt lands on the active-actor bar
                // (188..208) and inside the roster panels (164..212), so the
                // builder parks whichever one it covers.
                host_box: self.battle_tutorial_stage_rect(),
                active_slot: active.as_ref().map(|(s, _)| *s),
                // Retail parks the status plate off-screen while a command
                // entry session owns the frame; the port emits no strip.
                input_session_parked: w_ref.arts_input_active(),
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

    /// The message holding retail's top-of-screen banner this frame, if any.
    ///
    /// The port's two battle messages are the level-up and Seru-capture
    /// lines, and retail draws exactly those in this widget -
    /// `noa_levelup_banner` is one of the two save states the banner's
    /// geometry was read out of.
    ///
    /// Not gated on `SceneMode::Battle`, and that is a **port ordering
    /// difference worth naming**: retail raises the level-up message on the
    /// battle result screen, still in battle, while the port grants XP after
    /// the mode has already flipped back to Field - so gating on battle mode
    /// would leave the widget wired and never drawn. The message goes in
    /// retail's own widget wherever the port raises it; the sprite half
    /// follows through [`Self::battle_chrome_sprite_draws`].
    ///
    /// `None` without the system-UI atlas: there is no frame to put a
    /// message in, so a chrome-less host keeps the loose pens instead.
    pub(super) fn battle_banner_message(&self) -> Option<String> {
        self.save_menu.as_ref()?;
        let w = &self.session.host.world;
        if let Some(b) = &w.current_level_up_banner {
            // Name the character, not their roster ordinal: the banner reads
            // to a player, and `P3` is an index only this codebase knows.
            // `char_id` is the ROSTER slot the level-up applier wrote, so it
            // indexes `roster.members` directly (not the battle order).
            let who = w
                .roster
                .members
                .get(b.char_id as usize)
                .map(|r| r.name())
                .filter(|n| !n.is_empty())
                .unwrap_or_else(|| format!("P{}", b.char_id + 1));
            return Some(format!(
                "LEVEL UP!  {who} -> LV {}\nHP +{}  MP +{}",
                b.new_level, b.hp_gained, b.mp_gained
            ));
        }
        w.current_capture_banner
            .as_ref()
            .and_then(|b| b.current_banner())
    }

    /// The live battle command surface projected into the shared chip-cluster
    /// view: one [`legaia_engine_render::battle_command_ui::CommandChipView`]
    /// per chip of whichever phase is up, the cursor index, and the phase
    /// itself (which is what names the seats). `None` when no command surface
    /// owns the frame.
    ///
    /// The three phases are retail's three selection states - the round-open
    /// `Begin | Run` prompt (`0x1E`), the four-arm command ring (`0x28`) and
    /// the `Auto | Command` attack-mode prompt (`0x78`).
    ///
    /// One projector feeds both halves of the cluster - the plate sprites
    /// and the labels - so the two draw slots cannot disagree about whether
    /// the menu is up. The suppression rules mirror the text block's
    /// if-else chain exactly: a dialogue box, an arts-entry session or any
    /// open submenu parks the command chrome, which is what retail does.
    /// The engine-core battle-item-window projection (shared with the
    /// browser play page - `World::battle_item_menu_model` owns the gating
    /// and text resolution; this window only borrows it into the builder's
    /// frame via [`with_battle_item_frame`]).
    pub(super) fn battle_item_menu_model(
        &self,
    ) -> Option<legaia_engine_core::inventory_use::BattleItemMenuModel> {
        self.session.host.world.battle_item_menu_model()
    }

    pub(super) fn battle_command_menu_chips(
        &self,
    ) -> Option<(
        Vec<legaia_engine_render::battle_command_ui::CommandChipView<'static>>,
        usize,
        legaia_engine_render::battle_command_ui::ChipPhase,
    )> {
        use legaia_engine_core::battle_input::{
            AttackMode, BattleCommand, CommandPhase, RoundChoice,
        };
        use legaia_engine_render::battle_command_ui::{ChipPhase, CommandChipView};
        let bw = &self.session.host.world;
        if bw.mode != legaia_engine_core::world::SceneMode::Battle {
            return None;
        }
        if bw.current_dialog.is_some() || bw.inline_dialogue.is_some() {
            return None;
        }
        if bw.arts_input_view().is_some()
            || bw.battle_arts_menu.is_some()
            || bw.battle_spell_menu.is_some()
            || bw.battle_item_menu.is_some()
        {
            return None;
        }
        let cmd = bw.battle_command.as_ref()?;
        let no_escape = bw.battle_no_escape;
        let chip = |label: &'static str, enabled: bool| CommandChipView { label, enabled };
        match cmd.phase {
            CommandPhase::RoundPrompt { cursor } => Some((
                RoundChoice::PROMPT
                    .iter()
                    .map(|c| chip(c.label(), !matches!(c, RoundChoice::Run) || !no_escape))
                    .collect(),
                cursor as usize,
                ChipPhase::RoundPrompt,
            )),
            CommandPhase::Menu { cursor } => Some((
                BattleCommand::MENU
                    .iter()
                    .map(|c| chip(c.label(), c.available(no_escape)))
                    .collect(),
                cursor as usize,
                ChipPhase::CommandRing,
            )),
            CommandPhase::AttackMode { cursor } => Some((
                AttackMode::PROMPT
                    .iter()
                    .map(|m| chip(m.label(), true))
                    .collect(),
                cursor as usize,
                ChipPhase::AttackMode,
            )),
            _ => None,
        }
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
        if self.session.host.world.mode != legaia_engine_core::world::SceneMode::Battle {
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
            out.extend(bcu::battle_command_chip_sprites(
                &bcu::CommandChipAtlas::from_battle_chrome(&rects),
                &bcu::BattleCommandMenuFrame {
                    chips: &chips,
                    cursor: Some(cursor),
                    phase,
                },
                origin,
                scale,
            ));
        }
        out
    }

    /// The retail enemy target-name strip for a picker parked on the enemy
    /// row: rows deduplicated + labelled by the ported `FUN_801D9D3C`
    /// (`battle_enemy_target_rows`), placed by its centre/relax/clamp layout
    /// with this window's font as the measurer, cursor row highlighted.
    /// `None` when the cursor is not on the enemy row (ally / sweep states
    /// keep their text line) or no monster is up.
    pub(super) fn enemy_target_strip_draws(
        &self,
        picker: &legaia_engine_core::target_picker::TargetPickerSession,
        w: u32,
        h: u32,
    ) -> Option<Vec<TextDraw>> {
        use legaia_engine_core::target_picker::{CursorRow, PickerState, layout_enemy_menu_rows};
        let PickerState::Cursor {
            row: CursorRow::Enemy,
            slot,
        } = picker.state()
        else {
            return None;
        };
        let mut rows =
            legaia_engine_core::battle_hud::battle_enemy_target_rows(&self.session.host.world);
        if rows.is_empty() {
            return None;
        }
        layout_enemy_menu_rows(&mut rows, |s| self.font.layout_ascii(s).advance_x as i16);
        let views: Vec<legaia_engine_render::EnemyTargetRowView<'_>> = rows
            .iter()
            .map(|r| legaia_engine_render::EnemyTargetRowView {
                label: &r.label,
                x: r.x,
                selected: slot >= r.first_slot && slot < r.first_slot + r.members,
            })
            .collect();
        // The strip and a bottom-anchored sparring prompt share stage row 166
        // when the prompt runs to three lines, so the strip steps clear of the
        // live box's drawn footprint (`enemy_target_menu_rows_y`).
        Some(legaia_engine_render::enemy_target_menu_draws_at(
            &self.font,
            &views,
            (w, h),
            legaia_engine_render::enemy_target_menu_rows_y(self.battle_tutorial_stage_rect()),
        ))
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

    /// `engine-ui` mirrors `battle_chrome`'s seats as literals (it sits below
    /// `engine-vm` in the crate graph). This window is the one crate that can
    /// see both, so it is where the copy is held honest - a drift here is a
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

    /// The engine-ui anchor mirror is a literal copy of the canonical
    /// `engine-vm` port of retail's `FUN_801D84C0` table (engine-ui sits below
    /// engine-vm in the crate graph, so it cannot import them).
    ///
    /// The anchors are the roster panels' **name pens**, `+5` inside the
    /// panel background `battle_chrome::panel_seats` gives - which is what
    /// ties the overlay table to the packet run.
    #[test]
    fn panel_anchor_mirror_matches_the_engine_vm_port() {
        use legaia_engine_vm::battle_party_panel::panel_anchors;
        assert_eq!(panel_anchors(1), Some((0x72, None)));
        assert_eq!(panel_anchors(2), Some((0x3F, Some(0xA5))));
        assert_eq!(panel_anchors(3), Some((0x0C, Some(0x72))));
        // The inferred third anchor continues the pinned 0x66 stride.
        assert_eq!(0x72 - 0x0C, 0x66);
        assert_eq!(0xA5 - 0x3F, 0x66);
        for (count, ordinal, want) in [
            (1usize, 0usize, 0x72),
            (2, 0, 0x3F),
            (2, 1, 0xA5),
            (3, 0, 0x0C),
            (3, 1, 0x72),
            (3, 2, 0xD8),
        ] {
            assert_eq!(
                legaia_engine_render::party_panel_stage_x(count, ordinal),
                want,
                "engine-ui mirror drifted at ({count}, {ordinal})"
            );
        }
        // Every anchor is a panel seat plus the pinned +5 name inset.
        for size in 1u8..=3 {
            for (i, seat) in legaia_engine_vm::battle_chrome::panel_seats(size)
                .iter()
                .enumerate()
            {
                assert_eq!(
                    *seat as i32 + legaia_engine_vm::battle_chrome::PANEL_TEXT_INSET as i32,
                    legaia_engine_render::party_panel_stage_x(size as usize, i),
                    "anchor {i} of a party of {size} is not its panel seat + 5"
                );
            }
        }
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
        world.party_count = 1;
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
    /// `battle_value_readout_mesh` (see `engine-vm::battle_value_readout`).
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
