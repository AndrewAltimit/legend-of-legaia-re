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
        let audio_str = if self.session.audio.is_some() {
            "audio on"
        } else {
            "no audio"
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
        let line2 = format!(
            "t {:.1}s  {}{}  arrows=dpad Z=X",
            self.win.elapsed_secs(),
            audio_str,
            bgm_str
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
            let dl1 = format!(
                "DANCE  score {}  gauge {}  lane {}",
                g.score(),
                g.gauge(),
                g.lane()
            );
            let ly1 = self.font.layout_ascii(&dl1);
            out.extend(text_draws_for(&ly1, (8, 62), white));
            let dl2 = format!("press {arrow}   {judge}   (K = quit)");
            let ly2 = self.font.layout_ascii(&dl2);
            out.extend(text_draws_for(&ly2, (8, 80), dim));
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
            let pts = format!(
                "points {}   best {}   (L = quit, P = prizes)",
                s.record().points,
                s.record().best_points
            );
            let ly2 = self.font.layout_ascii(&pts);
            out.extend(text_draws_for(&ly2, (8, 80), dim));
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
        }
        // Muscle Dome HUD: HP + score readouts, the hand with costs, the
        // budget line, and the phase prompt.
        if self.session.host.world.mode == SceneMode::MuscleDome
            && let Some(s) = &self.session.host.world.muscle_dome
        {
            use legaia_engine_core::muscle_dome::MusclePhase;
            let ml1 = format!(
                "MUSCLE DOME  you {}hp ({}%)  vs  foe {}hp ({}%)  round {}",
                s.hp(0),
                s.score_percent(0),
                s.hp(1),
                s.score_percent(1),
                s.round() + 1
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
                let (title, rows, show_gold) = match state {
                    _ if trade_state => (label, Vec::new(), None),
                    // Top picker: Buy / Sell / (Trade) / Exit, matching the
                    // runtime's dynamic row layout.
                    Some(MenuState::ShopMenu) => {
                        let rows: Vec<ShopRow<'_>> =
                            legaia_engine_core::menu_runtime::shop_menu_rows(
                                self.session.host.world.seru_trade_enabled(),
                            )
                            .iter()
                            .map(|s| ShopRow {
                                label: match s {
                                    MenuState::ShopBuy => "Buy",
                                    MenuState::ShopSell => "Sell",
                                    MenuState::ShopTrade => "Trade Seru",
                                    _ => "Exit",
                                },
                                price: None,
                            })
                            .collect();
                        (label, rows, Some(gold))
                    }
                    Some(MenuState::ShopBuy) => {
                        let rows: Vec<ShopRow<'_>> = shop
                            .inventory
                            .items
                            .iter()
                            .map(|item| ShopRow {
                                label: "Item",
                                price: Some(item.price),
                            })
                            .collect();
                        (label, rows, Some(gold))
                    }
                    Some(MenuState::ShopSell) => {
                        let inv_items = MenuRuntime::inventory_items(&self.session.host.world);
                        let rows: Vec<ShopRow<'_>> = inv_items
                            .iter()
                            .map(|(_id, _qty)| ShopRow {
                                label: "Item",
                                price: None,
                            })
                            .collect();
                        (label, rows, Some(gold))
                    }
                    Some(MenuState::ShopQuantity) => {
                        let rows: Vec<ShopRow<'_>> = (1u32..=9)
                            .map(|_| ShopRow {
                                label: "qty",
                                price: None,
                            })
                            .collect();
                        (label, rows, None)
                    }
                    Some(MenuState::ShopConfirm) => {
                        let rows = vec![
                            ShopRow {
                                label: "Yes",
                                price: None,
                            },
                            ShopRow {
                                label: "No",
                                price: None,
                            },
                        ];
                        (label, rows, Some(gold))
                    }
                    _ => (label, Vec::new(), None),
                };
                if !rows.is_empty() {
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
                        let rows = vec![
                            ShopRow {
                                label: "Yes",
                                price: None,
                            },
                            ShopRow {
                                label: "No",
                                price: None,
                            },
                        ];
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
            let pc = (bw.party_count.clamp(1, 3) as usize).min(bw.actors.len());
            let down_color = [0.6f32, 0.6, 0.6, 1.0];
            let enemy_color = [1.0f32, 0.7, 0.6, 1.0];

            // Per-actor-index row Y, recorded as rows are drawn so popups +
            // status icons anchor to the right slot even though the monster
            // loop skips empty slots.
            let mut row_y: [Option<i32>; 8] = [None; 8];
            let mut y = 60i32;
            // Row label = the occupying character's roster name (the
            // present-party composition maps battle ordinal -> character);
            // "P<n>" when the roster has no record for the slot.
            let party_names = legaia_engine_core::field_menu_dispatch::roster_names(bw);
            for (i, a) in bw.actors.iter().take(pc).enumerate() {
                let name = party_names
                    .get(bw.party_roster_slot(i))
                    .filter(|n| !n.is_empty())
                    .map(|n| format!("{n:<8}"))
                    .unwrap_or_else(|| format!("P{:<7}", i + 1));
                let line = format!("{name}HP {:>4}/{:<4}", a.battle.hp, a.battle.max_hp);
                let color = if a.battle.liveness != 0 {
                    white
                } else {
                    down_color
                };
                out.extend(text_draws_for(
                    &self.font.layout_ascii(&line),
                    (8, y),
                    color,
                ));
                if i < row_y.len() {
                    row_y[i] = Some(y);
                }
                y += 16;
            }
            y += 8;
            for (mi, a) in bw.actors.iter().skip(pc).enumerate() {
                if a.battle.max_hp == 0 {
                    continue;
                }
                let alive = a.battle.liveness != 0;
                let line = format!(
                    "M{}  HP {:>4}/{:<4}{}",
                    mi + 1,
                    a.battle.hp,
                    a.battle.max_hp,
                    if alive { "" } else { "  DOWN" }
                );
                let color = if alive { enemy_color } else { down_color };
                out.extend(text_draws_for(
                    &self.font.layout_ascii(&line),
                    (8, y),
                    color,
                ));
                let actor_idx = pc + mi;
                if actor_idx < row_y.len() {
                    row_y[actor_idx] = Some(y);
                }
                y += 16;
            }

            // Status-effect icon strip per slot (single-letter abbreviations
            // from the live tracker), drawn to the right of the HP row.
            let status_color = [1.0f32, 0.95, 0.4, 1.0];
            for (slot, anchor) in row_y.iter().enumerate() {
                let Some(ry) = anchor else { continue };
                let letters = self.battle_hud.slots[slot].status_letters();
                for (k, letter) in letters.iter().enumerate() {
                    let s = (*letter as char).to_string();
                    out.extend(text_draws_for(
                        &self.font.layout_ascii(&s),
                        (170 + k as i32 * 8, *ry),
                        status_color,
                    ));
                }
            }

            // Floating damage / heal numbers, anchored just above each slot's
            // HP row and fading with the popup's remaining lifetime.
            let dmg_color = [0.5f32, 0.85, 1.0, 1.0];
            let heal_color = [0.5f32, 1.0, 0.5, 1.0];
            let crit_color = [1.0f32, 0.95, 0.4, 1.0];
            for p in self.battle_hud.popup_views() {
                let Some(Some(ry)) = row_y.get(p.slot as usize) else {
                    continue;
                };
                let base = if p.is_heal {
                    heal_color
                } else if p.is_crit {
                    crit_color
                } else {
                    dmg_color
                };
                let color = [base[0], base[1], base[2], base[3] * p.alpha.clamp(0.0, 1.0)];
                let text = if let Some(letter) = p.status_letter {
                    format!("[{}]", letter as char)
                } else if p.is_heal {
                    format!("+{}", p.amount)
                } else {
                    format!("-{}", p.amount)
                };
                out.extend(text_draws_for(
                    &self.font.layout_ascii(&text),
                    (120, *ry - 14),
                    color,
                ));
            }

            // Player-driven submenus (opened from the Arts / Magic / Item
            // commands). Each parks both the SM and the command session while
            // open, so it takes priority over the command menu.
            if let Some(arts) = &bw.battle_arts_menu {
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
        // Name-entry overlay: the opening `town01` lead-character naming prompt.
        if let Some(entry) = &self.session.host.world.name_entry {
            use legaia_engine_core::name_entry::{CHAR_CELLS, GRID, GRID_COLS};
            let (grid_cursor, control_cursor) = if entry.cursor < CHAR_CELLS {
                (
                    Some((entry.cursor / GRID_COLS, entry.cursor % GRID_COLS)),
                    None,
                )
            } else {
                // Map the control-row column to a button index (Back=0/Space=1/End=2)
                // via the cell's resolved action.
                use legaia_engine_core::name_entry::Control;
                let ctrl = entry.control_at(entry.cursor);
                let idx = match ctrl {
                    Some(Control::Backspace) => Some(0),
                    Some(Control::Space) => Some(1),
                    Some(Control::End) => Some(2),
                    _ => None,
                };
                (None, idx)
            };
            let view = legaia_engine_render::NameEntryView {
                grid_rows: &GRID,
                control_labels: &["Back", "Space", "End"],
                name: &entry.name,
                grid_cursor,
                control_cursor,
                confirming: entry.state == legaia_engine_core::name_entry::NameEntryState::Confirm,
                confirm_yes: entry.confirm_yes,
                caret_on: (self.session.host.world.frame / 16).is_multiple_of(2),
            };
            out.extend(legaia_engine_render::name_entry_draws_for(
                &self.font,
                &view,
                (32, 24),
            ));
        }
        // Dialog box: the active NPC / event message, typed out one line near
        // the bottom of the screen (1/8 from the left, ~70% down - the
        // single-line layout the retail field VM emits). The panel mirrors
        // `World::current_dialog`; the world owns dismissal.
        if let Some(panel) = self.active_dialog.as_ref() {
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
            let page = to_ascii(&panel.page_bytes());
            let layout = self.font.layout_ascii(&page);
            let pen = ((w as i32) / 8, (h as i32) * 7 / 10);
            out.extend(text_draws_for(&layout, pen, [1.0, 1.0, 1.0, 1.0]));

            // Multiple-choice menu: draw the decoded option labels under the
            // prompt, one row each, with a `>` cursor on the highlighted option
            // (the picker decoded from the inline interaction script).
            if panel.menu_active()
                && let Some(picker) = panel.picker()
            {
                // The proportional dialog font is a ~14px cell; one row per
                // option below the prompt.
                let line_h = 16i32;
                let cursor = panel.picker_cursor();
                for (i, opt) in picker.options.iter().enumerate() {
                    let selected = i == cursor;
                    let marker = if selected { "> " } else { "  " };
                    let label = format!("{marker}{}", to_ascii(&opt.label));
                    let row_layout = self.font.layout_ascii(&label);
                    let row_pen = (pen.0 + (w as i32) / 16, pen.1 + line_h * (i as i32 + 1));
                    let color = if selected {
                        [1.0, 1.0, 0.6, 1.0]
                    } else {
                        [0.8, 0.85, 1.0, 1.0]
                    };
                    out.extend(text_draws_for(&row_layout, row_pen, color));
                }
            }
        }

        // Cutscene-timeline dialog box: a `0x1F` conversation segment inside a
        // spawned partition-2 record (e.g. the town01 Mei walk-on beat). The
        // world's timeline stepper owns ticking + input; same layout as the
        // other panels.
        if let Some(panel) = self
            .session
            .host
            .world
            .cutscene_timeline
            .as_ref()
            .and_then(|tl| tl.dialog.as_ref())
        {
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
            let page = to_ascii(&panel.page_bytes());
            if !page.is_empty() {
                let layout = self.font.layout_ascii(&page);
                let pen = ((w as i32) / 8, (h as i32) * 7 / 10);
                out.extend(text_draws_for(&layout, pen, [1.0, 1.0, 1.0, 1.0]));
                if panel.menu_active()
                    && let Some(picker) = panel.picker()
                {
                    let line_h = 16i32;
                    let cursor = panel.picker_cursor();
                    for (i, opt) in picker.options.iter().enumerate() {
                        let selected = i == cursor;
                        let marker = if selected { "> " } else { "  " };
                        let label = format!("{marker}{}", to_ascii(&opt.label));
                        let row_layout = self.font.layout_ascii(&label);
                        let row_pen = (pen.0 + (w as i32) / 16, pen.1 + line_h * (i as i32 + 1));
                        let color = if selected {
                            [1.0, 1.0, 0.6, 1.0]
                        } else {
                            [0.8, 0.85, 1.0, 1.0]
                        };
                        out.extend(text_draws_for(&row_layout, row_pen, color));
                    }
                }
            }
        }

        // Inline-script field-VM runner box (the `--vm-dialogue` faithful path).
        // Same layout as the simplified panel, but the source is
        // `world.inline_dialogue`, which the world ticks itself.
        if let Some(id) = self.session.host.world.inline_dialogue.as_ref() {
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
            let page = to_ascii(&id.page_bytes());
            if !page.is_empty() {
                let layout = self.font.layout_ascii(&page);
                let pen = ((w as i32) / 8, (h as i32) * 7 / 10);
                out.extend(text_draws_for(&layout, pen, [1.0, 1.0, 1.0, 1.0]));
                if id.menu_active()
                    && let Some(picker) = id.picker()
                {
                    let line_h = 16i32;
                    let cursor = id.picker_cursor();
                    for (i, opt) in picker.options.iter().enumerate() {
                        let selected = i == cursor;
                        let marker = if selected { "> " } else { "  " };
                        let label = format!("{marker}{}", to_ascii(&opt.label));
                        let row_layout = self.font.layout_ascii(&label);
                        let row_pen = (pen.0 + (w as i32) / 16, pen.1 + line_h * (i as i32 + 1));
                        let color = if selected {
                            [1.0, 1.0, 0.6, 1.0]
                        } else {
                            [0.8, 0.85, 1.0, 1.0]
                        };
                        out.extend(text_draws_for(&row_layout, row_pen, color));
                    }
                }
            }
        }
        out
    }
}
