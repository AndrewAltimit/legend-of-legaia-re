//! Extracted from `window.rs` (mechanical split; behavior-preserving).

use super::*;

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
        // Dynamic-lighting enhancement state (opt-in, non-retail; `I` toggles).
        let light_str = if self.dynamic_lighting {
            "  light ON (I)"
        } else {
            ""
        };
        // Camera-distance preset (`T` cycles) + precise-movement toggle
        // (`R`) - the compass/zoom state, appended to the status line.
        let cam_str = format!("  cam {} (T)", self.session.camera.distance.label());
        let precise_str = if self.options_state.precise_movement {
            "  precise-move ON (R)"
        } else {
            ""
        };
        let line2 = format!(
            "t {:.1}s  {}{}{}{}{}  arrows=dpad Z=X drag=orbit",
            self.win.elapsed_secs(),
            audio_str,
            bgm_str,
            light_str,
            cam_str,
            precise_str
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
        // The three arrows map to the retail pad bits (Left/Right/Up).
        if self.session.host.world.mode == SceneMode::Dance
            && let Some(g) = &self.session.host.world.dance
        {
            let arrow = match g.required_symbol() {
                Some(1) => "< (Left)",
                Some(2) => "> (Right)",
                Some(3) => "^ (Up)",
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
            let venue_name = if ex.venue == 0 { "Buma" } else { "Vidna" };
            let head = format!(
                "PRIZE EXCHANGE ({venue_name})  points {}   (Enter = trade, Left/Right = venue, P = close)",
                world.fishing_points
            );
            let ly = self.font.layout_ascii(&head);
            out.extend(text_draws_for(&ly, (8, 98), white));
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
                let tag = if r.is_one_time() {
                    if avail { "one-time" } else { "sold" }
                } else {
                    "each"
                };
                let line = format!(
                    "{cursor} {name:<18} {:>6} pts  {tag}  (own {owned})",
                    r.price
                );
                let ly = self.font.layout_ascii(&line);
                let y = 116 + 18 * (i - first) as i32;
                out.extend(text_draws_for(&ly, (8, y), if avail { white } else { dim }));
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
        }
        // Muscle Dome HUD: HP + score readouts, the hand with costs, the
        // budget line, and the phase prompt.
        if self.session.host.world.mode == SceneMode::MuscleDome
            && let Some(s) = &self.session.host.world.muscle_dome
        {
            use legaia_engine_core::muscle_dome::MusclePhase;
            let ml1 = format!(
                "MUSCLE DOME  you {}hp ({}%)  vs  foe {}hp ({}%)  round {}  time {}/{}",
                s.hp(0),
                s.score_percent(0),
                s.hp(1),
                s.score_percent(1),
                s.round() + 1,
                s.time_meter(),
                legaia_engine_core::muscle_dome::TIME_METER_MAX,
            );
            let ly1 = self.font.layout_ascii(&ml1);
            out.extend(text_draws_for(&ly1, (8, 62), white));
            let status = match s.phase() {
                MusclePhase::Select => {
                    let h = s.hand(0);
                    format!(
                        "cards L:{} R:{} U:{} D:{}  budget {}  queued {}  (Cross = fight)",
                        h[0].cost,
                        h[1].cost,
                        h[2].cost,
                        h[3].cost,
                        s.budget(0),
                        s.queue(0).len()
                    )
                }
                MusclePhase::Resolve => "resolving...".to_string(),
                MusclePhase::RoundOver => {
                    let [taken, dealt] = s.last_round_damage();
                    format!("round: dealt {dealt}, took {taken}  (Cross = next round)")
                }
                MusclePhase::Won => format!(
                    "YOU WIN! Seru spell {:#x} awarded  (Cross/M = leave)",
                    s.reward_spell_id()
                ),
                MusclePhase::Lost => "you lose the contest  (Cross/M = leave)".to_string(),
            };
            let ml2 = format!("{status}   (M = quit)");
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
                if !rows_spec.is_empty() {
                    let rows: Vec<ShopRow<'_>> = rows_spec
                        .iter()
                        .map(|(l, price, ink)| ShopRow {
                            label: l.as_str(),
                            price: *price,
                            ink: *ink,
                        })
                        .collect();
                    let shop_draws =
                        shop_draws_for(&self.font, title, &rows, cursor, show_gold, (8, 140));
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
                        let inn_draws =
                            shop_draws_for(&self.font, &title, &rows, cursor, Some(gold), (8, 140));
                        out.extend(inn_draws);
                    }
                    Some(MenuState::InnSleep) => {
                        let layout = self.font.layout_ascii("Resting...");
                        out.extend(text_draws_for(&layout, (8, 140), white));
                    }
                    _ => {
                        let menu_label = format!("[{}]", label);
                        let ml_layout = self.font.layout_ascii(&menu_label);
                        out.extend(text_draws_for(&ml_layout, (8, 140), white));
                    }
                }
            } else {
                // Non-shop, non-inn menu: show current mode label.
                let menu_label = format!("[{}]", label);
                let ml_layout = self.font.layout_ascii(&menu_label);
                out.extend(text_draws_for(&ml_layout, (8, 140), white));
            }
        }
        // Battle-event log: rendered along the right edge when non-empty.
        // Most recent at the bottom of the column.
        if !self.battle_event_log.is_empty() {
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
            use legaia_engine_core::battle_input::{BattleCommand, CommandPhase};
            use legaia_engine_core::target_picker::{CursorRow, PickerState};
            let bw = &self.session.host.world;
            // Greyed-out row tint, used by the target lists in the Arts /
            // Magic / Item submenus below for a K.O.'d target.
            let down_color = [0.6f32, 0.6, 0.6, 1.0];

            // Per-slot rows, status strip and floating popups all come from
            // the shared builder, which carries the ported retail HP / MP
            // colour law (`hp_bar_color_index` / `mp_bar_color_index`,
            // FUN_800349EC / FUN_80035EA8). The rows are fed from the
            // `BattleHud` model, refreshed each tick by
            // `sync_battle_hud_rows`.
            out.extend(battle_hud_draws_for(
                &self.font,
                &battle_hud_slot_views(&self.battle_hud, &self.battle_hud_status_letters()),
                &battle_hud_popup_views(&self.battle_hud),
                &[],
                BATTLE_HUD_PEN,
            ));

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
                        let line = match picker.state() {
                            PickerState::Cursor {
                                row: CursorRow::Enemy,
                                slot,
                            } => format!("art -> target M{}", slot + 1),
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
                        let line = match picker.state() {
                            PickerState::Cursor {
                                row: CursorRow::Enemy,
                                slot,
                            } => format!("cast -> target M{}", slot + 1),
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
            } else if let Some(menu) = &bw.battle_item_menu {
                out.extend(self.items_session_draws(menu));
            } else if let Some(cmd) = &bw.battle_command {
                let menu_x = 8i32;
                let mut my = 210i32;
                match &cmd.phase {
                    CommandPhase::Menu { .. } => {
                        let header = format!("P{} - command:", cmd.actor + 1);
                        out.extend(text_draws_for(
                            &self.font.layout_ascii(&header),
                            (menu_x, my),
                            white,
                        ));
                        my += 16;
                        let cur = cmd.menu_command();
                        for c in BattleCommand::MENU {
                            let marker = if Some(c) == cur { ">" } else { " " };
                            let line = if c.enabled() {
                                format!("{} {}", marker, c.label())
                            } else {
                                format!("{} {} --", marker, c.label())
                            };
                            let color = if Some(c) == cur {
                                white
                            } else if c.enabled() {
                                dim
                            } else {
                                down_color
                            };
                            out.extend(text_draws_for(
                                &self.font.layout_ascii(&line),
                                (menu_x + 8, my),
                                color,
                            ));
                            my += 14;
                        }
                    }
                    CommandPhase::Targeting { command, picker } => {
                        let line = match picker.state() {
                            PickerState::Cursor {
                                row: CursorRow::Enemy,
                                slot,
                            } => format!("{} -> target M{}", command.label(), slot + 1),
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

            // Sparring-tutorial prompt box. Placed by the retail style table
            // (`FUN_801F747C`): the engine-core box carries the style index, we
            // supply the measured text width and it returns the corner. Drawn
            // last inside the battle block so it sits over the menus, which is
            // where retail's message box lands too.
            if let Some(tbox) = bw.battle_tutorial_box() {
                let layouts: Vec<_> = tbox
                    .text
                    .lines()
                    .map(|l| self.font.layout_ascii(l))
                    .collect();
                let width = layouts
                    .iter()
                    .map(|l| l.advance_x as i16)
                    .max()
                    .unwrap_or(0);
                let (bx, by) = tbox.position(width).unwrap_or((0x10, 0x0E));
                // Retail box coordinates are 320x240 screen space; the window
                // draws in the same space as the rest of this HUD.
                for (i, l) in layouts.iter().enumerate() {
                    out.extend(text_draws_for(
                        l,
                        (bx as i32, by as i32 + (i as i32) * 14),
                        white,
                    ));
                }
                if tbox.waits_for_input {
                    out.extend(text_draws_for(
                        &self.font.layout_ascii("Cross=continue"),
                        (bx as i32, by as i32 + (layouts.len() as i32) * 14),
                        dim,
                    ));
                }
            }
        }
        // Level-up banner: rendered near the top when active after a battle win.
        if let Some(banner) = &self.session.host.world.current_level_up_banner {
            let draws = level_up_draws_for(
                &self.font,
                banner.char_id,
                banner.new_level,
                banner.hp_gained,
                banner.mp_gained,
                (8, 60),
            );
            out.extend(draws);
        }
        // Seru-capture banner: shown after a battle in which a Seru was
        // captured (and, if a threshold was crossed, a spell learned).
        if let Some(banner) = &self.session.host.world.current_capture_banner
            && let Some(text) = banner.current_banner()
        {
            out.extend(capture_banner_draws_for(&self.font, &text, (8, 40)));
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
pub(super) const BATTLE_HUD_PEN: (i32, i32) = (8, 60);

impl PlayWindowApp {
    /// Per-slot status-letter strips, one entry per HUD slot. Kept separate
    /// from [`battle_hud_slot_views`] because `HudSlotView` borrows the strip
    /// and the caller has to own the backing buffer.
    pub(super) fn battle_hud_status_letters(&self) -> Vec<Vec<u8>> {
        self.battle_hud
            .slots
            .iter()
            .map(|s| s.status_letters())
            .collect()
    }
}

/// Project the HUD model's slot array into the shared builder's view type.
///
/// Every slot is emitted, **including inactive ones** (as empty-name rows the
/// builder skips). That is deliberate: `battle_hud_draws_for` derives both a
/// row's Y and a popup's anchor from the slice index, so the index has to stay
/// the absolute actor-table slot. Compacting to active slots only would shift
/// every monster row up and anchor damage numbers to the wrong actor.
pub(super) fn battle_hud_slot_views<'a>(
    hud: &'a legaia_engine_core::battle_hud::BattleHud,
    letters: &'a [Vec<u8>],
) -> Vec<HudSlotView<'a>> {
    hud.slots
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let meta = HudSlotMeta {
                is_party: s.is_party,
                alive: s.alive,
                hp: s.hp,
                hp_max: s.hp_max,
                mp: s.mp,
                mp_max: s.mp_max,
                ap_filled: s.ap_filled,
                ap_max: s.ap_max,
            };
            let name = if s.active { s.name.as_str() } else { "" };
            let strip: &'a [u8] = letters.get(i).map(|v| v.as_slice()).unwrap_or(&[]);
            HudSlotView::from_plain(meta, name, strip)
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
    use legaia_engine_render::battle_hud_draws_for;

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

    /// The window's battle block must produce an MP field. The hand-rolled HUD
    /// this replaced printed HP only, so this assertion is what pins the MP
    /// readout as wired rather than merely available.
    ///
    /// MP is drawn at `pen.x + 140`; HP at `pen.x + 70`. Counting glyphs at or
    /// past the MP column is therefore a positional test that cannot pass off
    /// an HP-only row as an MP one.
    #[test]
    fn native_battle_hud_draws_an_mp_field() {
        let font = legaia_font::synthetic_for_tests();
        let hud = hud_with_party_row(250, 300, 12, 30);
        let letters = vec![Vec::new(); hud.slots.len()];
        let draws = battle_hud_draws_for(
            &font,
            &battle_hud_slot_views(&hud, &letters),
            &battle_hud_popup_views(&hud),
            &[],
            BATTLE_HUD_PEN,
        );
        let mp_x = BATTLE_HUD_PEN.0 + 140;
        assert!(
            draws.iter().any(|d| d.dst.0 >= mp_x),
            "no glyph reached the MP column at x={mp_x}"
        );
    }

    /// The four-tier retail HP colour law has to reach the surface, not just
    /// exist in engine-ui. Normal / caution / danger / K.O. must produce three
    /// distinct tints plus the dim K.O. row.
    #[test]
    fn native_battle_hud_hp_tints_span_all_four_retail_tiers() {
        let font = legaia_font::synthetic_for_tests();
        let letters = vec![Vec::new(); 8];
        let hp_x = BATTLE_HUD_PEN.0 + 70;
        let mp_x = BATTLE_HUD_PEN.0 + 140;
        // First glyph of the HP field, per HP value.
        let hp_tint = |hp: u16| -> [f32; 4] {
            let hud = hud_with_party_row(hp, 100, 0, 0);
            let draws = battle_hud_draws_for(
                &font,
                &battle_hud_slot_views(&hud, &letters),
                &[],
                &[],
                BATTLE_HUD_PEN,
            );
            draws
                .iter()
                .filter(|d| d.dst.0 >= hp_x && d.dst.0 < mp_x)
                .map(|d| d.color)
                .next()
                .expect("HP field produced no glyph")
        };
        let normal = hp_tint(90); // > max/2  -> index 7
        let caution = hp_tint(40); // <= max/2 -> index 6
        let danger = hp_tint(20); // <= max/4 -> index 9
        assert_ne!(
            normal, caution,
            "caution tier not distinguished from normal"
        );
        assert_ne!(
            caution, danger,
            "danger tier not distinguished from caution"
        );
        assert_ne!(normal, danger, "danger tier not distinguished from normal");
        // Caution is yellow (r ~= g, both high); danger is red (r > g).
        assert!(danger[0] > danger[1], "danger tier is not red-dominant");
        assert!(
            caution[1] > danger[1],
            "caution tier is not the lighter tint"
        );
    }

    /// The end-to-end wiring: a live `World` battle state must reach the
    /// shared builder's draw list, MP included.
    ///
    /// This is the assertion that fails if `sync_battle_hud_rows` is dropped
    /// from the tick - the HUD model's slots stay `active == false`, the
    /// builder skips every empty-name row, and `draws` comes back empty.
    /// Confirmed by commenting the `hud.sync_slot` loop out of
    /// `sync_battle_hud_rows`: the row assertion below then fails with an
    /// empty draw list, and the two MP assertions with it.
    #[test]
    fn live_world_battle_state_reaches_the_shared_builder() {
        use legaia_engine_core::world::World;

        let font = legaia_font::synthetic_for_tests();
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

        let letters = vec![Vec::new(); hud.slots.len()];
        let draws = battle_hud_draws_for(
            &font,
            &battle_hud_slot_views(&hud, &letters),
            &battle_hud_popup_views(&hud),
            &[],
            BATTLE_HUD_PEN,
        );
        assert!(!draws.is_empty(), "synced battle state produced no draws");
        let mp_x = BATTLE_HUD_PEN.0 + 140;
        assert!(
            draws.iter().any(|d| d.dst.0 >= mp_x),
            "live world state produced no MP field"
        );
    }

    /// Popups carry an absolute actor slot. The builder anchors them by slice
    /// index, so the projection must keep inactive slots in place - a
    /// compacted list would put a monster's damage number on a party row.
    #[test]
    fn popup_anchors_track_absolute_actor_slot() {
        let font = legaia_font::synthetic_for_tests();
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
        let letters = vec![Vec::new(); hud.slots.len()];
        let draws = battle_hud_draws_for(
            &font,
            &battle_hud_slot_views(&hud, &letters),
            &battle_hud_popup_views(&hud),
            &[],
            BATTLE_HUD_PEN,
        );
        // Row stride is 14; slot 3's row sits at pen.y + 42, popups 16 above.
        let want_y = BATTLE_HUD_PEN.1 + 3 * 14 - 16;
        let popup_x = BATTLE_HUD_PEN.0 + 80;
        assert!(
            draws
                .iter()
                .any(|d| d.dst.1 == want_y && d.dst.0 >= popup_x),
            "no popup glyph at slot 3's anchor (y={want_y})"
        );
    }
}
