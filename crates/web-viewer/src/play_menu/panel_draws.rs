//! `play_menu_draws_json` and the root-level panel builders: context lock,
//! top-level list, status, config, arts editor and list order.

use super::*;

#[wasm_bindgen]
impl LegaiaRuntime {
    /// Build the two draw lists for the current menu state, in surface pixels.
    /// Shape:
    /// ```text
    /// { "open": true,
    ///   "sprites": [ { "dst":[x,y,w,h], "src":[x,y,w,h], "color":[r,g,b,a] } ],
    ///   "texts":   [ ... ] }
    /// ```
    /// `sprites` sample the chrome atlas, `texts` the font atlas. `open` is
    /// `false` (and the lists empty) when no menu is up.
    pub fn play_menu_draws_json(&self, surface_w: u32, surface_h: u32) -> String {
        let (Some(menu), Some(assets)) = (self.play_menu.as_ref(), self.menu_assets.as_ref())
        else {
            return r#"{"open":false,"sprites":[],"texts":[]}"#.to_string();
        };
        // Nothing to draw before the wipe spawns the menu: the field shows.
        if !self.pause_menu_spawned() {
            return r#"{"open":false,"sprites":[],"texts":[]}"#.to_string();
        }
        let (origin, scale) = stage_transform(surface_w.max(1), surface_h.max(1));
        let mut sprites: Vec<SpriteDraw> = Vec::new();
        let mut texts: Vec<TextDraw> = Vec::new();
        // The save screen's subtractive darkening (`ui::SaveScreenDarken`):
        // the sprite index of its `B - F` quad and the grey it subtracts.
        let mut darken: Option<(usize, u8)> = None;

        // The kind-0x0D entry pair replaces the root list rather than
        // overlaying it: both sub-screens open with `05 00` (close every
        // window) before opening their own, so the command rows are not on
        // screen while either is up. Browser twin of the native window's
        // `context_locked_screen_draws`.
        let context_screen = self.build_context_locked(assets, &mut texts, origin, scale);
        match &menu.sub {
            _ if context_screen => {}
            None => self.build_top_level(assets, menu, &mut sprites, &mut texts, origin, scale),
            Some(PlaySub::Session(sub)) => match sub.as_ref() {
                FieldMenuSubsession::Save(s) => {
                    darken = self.build_save_select(
                        assets,
                        s,
                        menu,
                        &mut sprites,
                        &mut texts,
                        origin,
                        scale,
                    );
                }
                FieldMenuSubsession::Status(s) => {
                    self.build_status(assets, s, &mut sprites, &mut texts, origin, scale)
                }
                FieldMenuSubsession::Config(s) => {
                    self.build_config(assets, s, &mut sprites, &mut texts, origin, scale)
                }
                FieldMenuSubsession::Items(s) => {
                    self.build_items(assets, s, &mut sprites, &mut texts, origin, scale)
                }
                FieldMenuSubsession::Spells(s) => {
                    self.build_spells(assets, s, &mut sprites, &mut texts, origin, scale)
                }
                FieldMenuSubsession::Equip {
                    session, char_slot, ..
                } => self.build_equip(
                    assets,
                    session,
                    *char_slot,
                    &mut sprites,
                    &mut texts,
                    origin,
                    scale,
                ),
                FieldMenuSubsession::Arts(s) => {
                    self.build_arts_editor(assets, s, &mut sprites, &mut texts, origin, scale)
                }
                FieldMenuSubsession::ListOrder(s) => {
                    self.build_list_order(assets, s, &mut sprites, &mut texts, origin, scale)
                }
            },
        }

        // Window 7 - the spell level-up notice - overlays whichever menu
        // screen is current while the shared runtime holds the beat. Both
        // hosts route it through the one shared composition, so neither can
        // pick a different painter or a different stand-in cursor.
        if let Some(notice) = self.menu.spell_level_notice() {
            texts.extend(spell_level_notice_draws(
                &assets.menu_ctx(origin, scale),
                &notice.line,
            ));
        }
        // Window 8 - the art-learned notice, through the same shared
        // composition the native window calls.
        if let Some(notice) = self.menu.art_learned_notice() {
            texts.extend(art_learned_notice_draws(
                &assets.menu_ctx(origin, scale),
                &notice.lines,
            ));
        }

        serde_json::json!({
            "open": true,
            "sprites": sprites.iter().map(quad_json).collect::<Vec<_>>(),
            "texts": texts.iter().map(quad_json).collect::<Vec<_>>(),
            // The page's 2D canvas has no `B - F` composite: it blits the
            // sprites before `at`, subtracts `level` from every channel of
            // what is drawn, and blits the rest past the quad itself.
            "darken": darken.map(|(at, level)| serde_json::json!({"at": at, "level": level})),
        })
        .to_string()
    }
}

impl LegaiaRuntime {
    /// Top-level command list + money/time box + party info panel, with gold
    /// window chrome + the cursor / icon sprites. Mirrors the native window's
    /// `BootUiState::FieldMenu { sub: None }` path.
    /// Windows 6 and 5 - the pair the pause menu draws when the op-`0x49`
    /// entry context's kind byte is `0x0D`. Browser twin of the native
    /// window's `context_locked_screen_draws`, off the same session phases,
    /// the same disc-parsed rects and the same
    /// [`legaia_engine_menus::pause_screens::ContextLockedLabels`] - so
    /// neither host can invent a label the other does not have.
    ///
    /// Returns `true` when one of the two drew, which is what suppresses
    /// the root list for the frame.
    fn build_context_locked(
        &self,
        assets: &PlayMenuAssets,
        texts: &mut Vec<TextDraw>,
        origin: (i32, i32),
        scale: u32,
    ) -> bool {
        let Some(world) = self.menu_world() else {
            return false;
        };
        let ctx = assets.menu_ctx(origin, scale);
        let labels = &world.menu.context_labels;
        let Some(root) = self.root_menu() else {
            return false;
        };
        let out = if root.notice_is_up() {
            let lines: Vec<&str> = labels.notice_lines.iter().map(String::as_str).collect();
            pause_screen_draws(&ctx, PauseScreen::ContextNotice { lines: &lines })
        } else if let Some(cursor_row) = root.ready_confirm_cursor() {
            pause_screen_draws(
                &ctx,
                PauseScreen::ContextReady {
                    headings: [
                        labels.ready_headings[0].as_str(),
                        labels.ready_headings[1].as_str(),
                    ],
                    choices: [labels.choices[0].as_str(), labels.choices[1].as_str()],
                    cursor: u32::from(cursor_row),
                },
            )
        } else {
            return false;
        };
        if out.is_empty() {
            return false;
        }
        texts.extend(out.texts);
        true
    }

    /// Top-level command list + money/time box + party info panel.
    ///
    /// Money, play time, cursor and per-row ink all come off the shared
    /// session's view - the same projection the native window renders. The
    /// page used to substitute `world.frame / 60` for the clock and a
    /// literal `true` for every row's ink; the first made the H:MM:SS box a
    /// per-page-load frame counter (and wrote that number into any save
    /// taken from the browser), and the second drew a blocked row white.
    fn build_top_level(
        &self,
        assets: &PlayMenuAssets,
        menu: &PlayMenu,
        sprites: &mut Vec<SpriteDraw>,
        texts: &mut Vec<TextDraw>,
        origin: (i32, i32),
        scale: u32,
    ) {
        let Some(world) = self.menu_world() else {
            return;
        };
        let Some(root) = self.root_menu() else {
            return;
        };
        let view = root.view();
        let rows: Vec<FieldMenuRowView<'_>> = view
            .rows
            .iter()
            .map(|r| FieldMenuRowView {
                label: r.label,
                enabled: r.enabled,
            })
            .collect();
        let snaps = status_snapshots(world);
        let party: Vec<FieldMenuPartyView<'_>> = snaps
            .iter()
            .map(|s| FieldMenuPartyView {
                name: &s.name,
                level: s.level,
                hp: s.hp,
                hp_max: s.hp_max,
                mp: s.mp,
                mp_max: s.mp_max,
                ap: s.ap as u16,
                // Record `+0x12E`: non-zero re-inks the HP number gold
                // (`FUN_800349EC`'s ailment arm).
                status: s.status_flags,
            })
            .collect();
        let party_ap: Vec<u16> = snaps.iter().map(|s| s.ap as u16).collect();
        let out = pause_screen_draws(
            &assets.menu_ctx(origin, scale),
            PauseScreen::TopLevel(TopLevelView {
                rows: &rows,
                cursor: view.cursor,
                money: view.money,
                play_time_seconds: view.play_time_seconds,
                party: &party,
                party_ap: &party_ap,
            }),
        );
        sprites.extend(out.sprites);
        texts.extend(out.texts);
        // A refused save commit lands the player back here, so this is where
        // the notice is owed. Same two builders the native window draws.
        if let Some(reason) = menu.save_flow.refusal() {
            if let Some(rects) = assets.chrome_rects() {
                sprites.extend(ui::save_refusal_panel_draws_for(rects, origin, scale));
            }
            texts.extend(ui::save_refusal_text_draws_for(
                assets.font_ref(),
                reason.message(),
                origin,
                scale,
            ));
        }
    }

    /// Status sub-screen: the main panel + the three satellite windows + the
    /// Status tab, with the LV/HP/MP + AP-gauge + element icon sprites.
    fn build_status(
        &self,
        assets: &PlayMenuAssets,
        s: &StatusScreenSession,
        sprites: &mut Vec<SpriteDraw>,
        texts: &mut Vec<TextDraw>,
        origin: (i32, i32),
        scale: u32,
    ) {
        let Some(snap) = s.current() else {
            return;
        };
        let stat_rows: Vec<StatusStatRow<'_>> = snap
            .stats
            .iter()
            .zip(snap.stat_labels.iter())
            .map(|((live, growth), l)| StatusStatRow {
                label: l,
                value: *live as u32,
                growth: *growth as u32,
            })
            .collect();
        let equip_rows: Vec<(&str, &str)> = snap
            .equip
            .iter()
            .map(|e| (e.label, e.item_name.as_str()))
            .collect();
        let panel = StatusPanelView {
            name: &snap.name,
            level: snap.level,
            xp: snap.xp,
            xp_to_next: snap.xp_to_next,
            hp: snap.hp,
            hp_max: snap.hp_max,
            mp: snap.mp,
            mp_max: snap.mp_max,
            ap: snap.ap,
            ap_max: snap.ap_max,
            stat_rows: &stat_rows,
            equip_rows: &equip_rows,
            status: snap.status_flags,
        };
        let names: Vec<&str> = s.snapshots().iter().map(|m| m.name.as_str()).collect();
        let satellite = StatusSatelliteView {
            party_names: &names,
            cursor: s.cursor() as usize,
            name: &snap.name,
            level: snap.level,
        };
        let out = pause_screen_draws(
            &assets.menu_ctx(origin, scale),
            PauseScreen::Status(StatusScreenView {
                panel: &panel,
                satellite: &satellite,
                ap: snap.ap as u16,
                atr_char: snap.slot as usize,
            }),
        );
        sprites.extend(out.sprites);
        texts.extend(out.texts);
    }

    /// Options sub-screen: the settings rows + value popup + the hand cursor,
    /// with the options window frame + tab.
    fn build_config(
        &self,
        assets: &PlayMenuAssets,
        s: &OptionsSession,
        sprites: &mut Vec<SpriteDraw>,
        texts: &mut Vec<TextDraw>,
        origin: (i32, i32),
        scale: u32,
    ) {
        // The screen's model is the engine's (`OptionsSession::screen_model`,
        // the native window's call too); this host only borrows it into the
        // renderer's views and places the popup.
        let m = s.screen_model();
        let row_views: Vec<ui::OptionsRowView<'_>> = m
            .rows
            .iter()
            .map(|r| ui::OptionsRowView {
                label: r.label,
                value: r.value,
                teal: r.teal,
                advance: r.advance,
            })
            .collect();
        let popup = m.popup.map(|p| ui::OptionsPopupDraw {
            rect: self.options_popup_rect(assets, &p),
            choices: p.choices,
            cursor: p.cursor,
        });
        let rebind_pairs: Vec<(&str, &str)> = m
            .rebind
            .iter()
            .flat_map(|k| k.rows.iter())
            .map(|(b, k)| (b.as_str(), k.as_str()))
            .collect();
        let rebind = m.rebind.as_ref().map(|k| KeyRebindView {
            rows: &rebind_pairs,
            cursor: k.cursor,
            awaiting: k.awaiting,
        });
        let out = pause_screen_draws(
            &assets.menu_ctx(origin, scale),
            PauseScreen::Options(OptionsScreenView {
                rows: &row_views,
                cursor: m.cursor,
                popup,
                row_y_off: m.row_y_off,
                rebind,
            }),
        );
        sprites.extend(out.sprites);
        texts.extend(out.texts);
    }

    /// Tactical Arts chain editor, inside the generic sub-window frame.
    ///
    /// The engine extension reached by Triangle on the Status screen (see
    /// `field_menu_dispatch::try_open_arts_editor`). The live editor state
    /// is projected by the shared `arts_editor_view`, so the character
    /// name, the pretty-printed sequences and the "+ New" room check are
    /// the same code the native window runs - only the borrow into
    /// `ArtsEditorDrawArgs` is per host.
    fn build_arts_editor(
        &self,
        assets: &PlayMenuAssets,
        editor: &legaia_engine_battle::tactical_arts_editor::ChainEditor,
        sprites: &mut Vec<SpriteDraw>,
        texts: &mut Vec<TextDraw>,
        origin: (i32, i32),
        scale: u32,
    ) {
        let Some(world) = self.menu_world() else {
            return;
        };
        let view = field_menu_dispatch::arts_editor_view(editor, world);
        let saved_rows: Vec<ui::ArtsChainRow<'_>> = view
            .saved
            .iter()
            .map(|(name, pretty)| ui::ArtsChainRow {
                name,
                pretty_sequence: pretty,
            })
            .collect();
        let args = ui::ArtsEditorDrawArgs {
            character_name: &view.character_name,
            phase: match view.phase {
                ArtsEditorPhaseTag::Browsing => ui::ArtsEditorPhase::Browsing,
                ArtsEditorPhaseTag::Editing => ui::ArtsEditorPhase::Editing,
                ArtsEditorPhaseTag::Naming => ui::ArtsEditorPhase::Naming,
            },
            saved: &saved_rows,
            browse_cursor: view.browse_cursor,
            editing_pretty: &view.editing_pretty,
            editing_len: view.editing_len,
            min_len: view.min_len,
            max_len: view.max_len,
            naming_name: &view.naming_name,
            can_add_new: view.can_add_new,
        };
        let out = pause_screen_draws(
            &assets.menu_ctx(origin, scale),
            PauseScreen::Generic(GenericContent::Arts(args)),
        );
        sprites.extend(out.sprites);
        texts.extend(out.texts);
    }

    /// The record screen's list page, opened over the Magic screen's spell
    /// rows (Square). Twin of the native window's arm in
    /// `window::menu_draws`.
    fn build_list_order(
        &self,
        assets: &PlayMenuAssets,
        session: &legaia_engine_menus::list_order::ListOrderSession,
        sprites: &mut Vec<SpriteDraw>,
        texts: &mut Vec<TextDraw>,
        origin: (i32, i32),
        scale: u32,
    ) {
        let rows: Vec<ui::ListOrderRowView<'_>> = session
            .rows()
            .iter()
            .enumerate()
            .map(|(i, r)| ui::ListOrderRowView {
                label: r.label.as_str(),
                latched: session.latched() == Some(i),
            })
            .collect();
        let out = pause_screen_draws(
            &assets.menu_ctx(origin, scale),
            PauseScreen::Generic(GenericContent::ListOrder(ui::ListOrderDrawArgs {
                title: LIST_ORDER_TITLE,
                rows: &rows,
                cursor: session.cursor(),
                scroll_top: session.scroll_top(),
                page_rows: legaia_engine_menus::list_order::LIST_ORDER_PAGE_ROWS,
                reorderable: session.reorderable(),
            })),
        );
        sprites.extend(out.sprites);
        texts.extend(out.texts);
    }
}
