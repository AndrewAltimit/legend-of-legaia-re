//! The battle overlay's draw lists: spoils chrome, post-battle overlay, arts
//! input strip, HUD frame, banner, command chips, chrome sprites and the
//! composed `battle_overlay_draws`.

use super::*;

impl LegaiaRuntime {
    /// Out-of-battle battle presentation, in **surface pixels**: the
    /// post-battle spoils panel and the game-over panel. Both sit outside
    /// [`SceneMode::Battle`], which is why they are not part of
    /// [`Self::battle_overlay_draws`], and both use the same shared
    /// `engine-ui` builder + world model as the native window
    /// (`window/battle.rs`, `window/boot_cutscene.rs`).
    ///
    /// The "this scene rolls no encounters" hint does **not** belong here.
    /// A non-empty overlay clears the canvas before it blits, so a passive
    /// hint routed through this list would wipe whatever the frame had
    /// already painted for the first seconds of a town. The page reads
    /// [`Self::scene_rolls_encounters`] and prints its own notice instead.
    /// The post-battle report's window chrome (gold nine-slice over the blue
    /// fill), one frame per window the shared builder describes. Rides the
    /// page's system-UI sprite array with the rest of the menu chrome.
    pub(crate) fn battle_spoils_chrome_sprite_draws(
        &self,
        assets: &crate::play_menu::PlayMenuAssets,
        surface_w: u32,
        surface_h: u32,
    ) -> Vec<ui::SpriteDraw> {
        if self.game_over.is_some() {
            return Vec::new();
        }
        let Some(w) = self.scene_host.host().map(|h| &h.world) else {
            return Vec::new();
        };
        let Some(rects) = assets.chrome_rects() else {
            return Vec::new();
        };
        // The loss window (screen element `0x42`) shares the report frame.
        if let Some(defeat) = w.battle_defeat_banner() {
            let (origin, scale) =
                crate::play_menu::stage_transform(surface_w.max(1), surface_h.max(1));
            return ui::battle_defeat_windows(defeat.line.as_deref(), defeat.slide_y)
                .iter()
                .flat_map(|win| ui::menu_window_chrome_draws_for(rects, win.rect, origin, scale))
                .collect();
        }
        let Some(banner) = w.battle_spoils_banner() else {
            return Vec::new();
        };
        let leader = w.battle_spoils_leader();
        let view = ui::BattleSpoilsView {
            xp: banner.xp,
            gold: banner.gold,
            level_ups: &banner.level_ups,
            drops: &banner.drops,
            leader: &leader,
            subject: banner.subject,
            slide: banner.slide,
        };
        let (origin, scale) = crate::play_menu::stage_transform(surface_w.max(1), surface_h.max(1));
        ui::battle_spoils_windows(&view)
            .iter()
            .flat_map(|win| ui::menu_window_chrome_draws_for(rects, win.rect, origin, scale))
            .collect()
    }

    pub(crate) fn post_battle_overlay_draws(
        &self,
        assets: &crate::play_menu::PlayMenuAssets,
        surface_w: u32,
        surface_h: u32,
    ) -> Vec<TextDraw> {
        let Some(w) = self.scene_host.host().map(|h| &h.world) else {
            return Vec::new();
        };
        let font = assets.font_ref();
        let mut out: Vec<TextDraw> = Vec::new();

        // Party wipe owns the frame, and draws nothing on it: retail's next
        // frame after the wipe store is the title overlay fading in, so the
        // hand-off holds the frozen battle frame and adds no chrome. The
        // native host's `boot_ui_draws` arm is silent for the same reason.
        if self.game_over.is_some() {
            return out;
        }

        if let Some(defeat) = w.battle_defeat_banner() {
            let windows = ui::battle_defeat_windows(defeat.line.as_deref(), defeat.slide_y);
            let (origin, scale) =
                crate::play_menu::stage_transform(surface_w.max(1), surface_h.max(1));
            let mut draws = ui::battle_result_line_draws_for(font, &windows);
            ui::scale_stage_text_draws(&mut draws, origin, scale);
            out.extend(draws);
        } else if let Some(banner) = w.battle_spoils_banner() {
            let leader = w.battle_spoils_leader();
            let view = ui::BattleSpoilsView {
                xp: banner.xp,
                gold: banner.gold,
                level_ups: &banner.level_ups,
                drops: &banner.drops,
                leader: &leader,
                subject: banner.subject,
                slide: banner.slide,
            };
            let windows = ui::battle_spoils_windows(&view);
            let (origin, scale) =
                crate::play_menu::stage_transform(surface_w.max(1), surface_h.max(1));
            let mut draws = ui::battle_spoils_draws_for(font, &view, &windows);
            ui::scale_stage_text_draws(&mut draws, origin, scale);
            out.extend(draws);
        }

        out
    }

    /// The **arts command-input** chrome for the browser play page:
    /// `(sprites, texts)`. The sprites are the shared composition
    /// (`legaia_engine_screens::battle_screens::arts_input_chrome_draws`, the
    /// native window's call too), placed by the stage `origin` / `scale`; the
    /// screen is baked art, so the text half is always empty - the Begin |
    /// Reselect pick is the party's commit confirm, drawn by the command-chip
    /// cluster.
    pub(crate) fn arts_input_stage_draws(
        &self,
        _font: &legaia_font::Font,
        chrome: Option<&ui::SaveMenuAtlasRects>,
        origin: (i32, i32),
        scale: u32,
    ) -> (Vec<ui::SpriteDraw>, Vec<TextDraw>) {
        let Some(world) = self.scene_host.host().map(|h| &h.world) else {
            return (Vec::new(), Vec::new());
        };
        (
            legaia_engine_screens::battle_screens::arts_input_chrome_draws(
                world, chrome, origin, scale,
            ),
            Vec::new(),
        )
    }

    /// The shared battle screens' inputs, borrowed from this page's holders;
    /// `None` while no scene is up.
    pub(crate) fn battle_screen_inputs<'a>(
        &'a self,
        assets: &'a crate::play_menu::PlayMenuAssets,
    ) -> Option<legaia_engine_screens::battle_screens::BattleScreenInputs<'a>> {
        let font = assets.font_ref();
        Some(legaia_engine_screens::battle_screens::BattleScreenInputs {
            world: &self.scene_host.host()?.world,
            hud: &self.battle_hud,
            font,
            chrome: assets.chrome_rects(),
            badges: assets.battle_badges(),
            solid_src: ui::font_solid_src(font),
        })
    }

    /// One battle-HUD frame from the shared builder
    /// (`legaia_engine_screens::battle_screens::battle_hud_frame_draws`, the
    /// native window's call too): the party strip, the top-left plaque and
    /// the popups. Empty while no scene is up.
    ///
    /// Both halves come from one call so the page's two draw arrays cannot
    /// drift - the text half joins the surface-space battle text, the sprite
    /// half the system-UI atlas `sprites` array.
    pub(crate) fn battle_hud_frame_draws(
        &self,
        assets: &crate::play_menu::PlayMenuAssets,
        surface_w: u32,
        surface_h: u32,
    ) -> ui::BattleHudDraws {
        match self.battle_screen_inputs(assets) {
            Some(inputs) => legaia_engine_screens::battle_screens::battle_hud_frame_draws(
                &inputs, surface_w, surface_h,
            ),
            None => ui::BattleHudDraws {
                text: Vec::new(),
                sprites: Vec::new(),
            },
        }
    }

    /// The live battle command surface projected into the shared chip-cluster
    /// view: the owned `(label, enabled)` chips of whichever phase is up, the
    /// cursor index, and the phase (which names the seats). `None` when no
    /// command surface owns the frame.
    ///
    /// The projection itself is `engine-core::battle_hud::battle_command_chips`,
    /// shared with the native window, and where the ring's element chip
    /// becomes the member's Ra-Seru name or `-` off the disc. One projector
    /// feeds both halves of the cluster (the plate sprites and the labels),
    /// so the page's two draw arrays cannot disagree about whether the menu
    /// is up.
    /// The command cluster's open glide this frame: each chip's offset from
    /// its seat and whether the D-pad glyph draws
    /// (`battle_hud::battle_chip_offsets` / `battle_chip_dpad_visible`), at
    /// rest without a world. The native window reads the same pair.
    pub(crate) fn battle_chip_glide(&self) -> ([(i32, i32); 4], bool) {
        use legaia_engine_core::battle_hud as bh;
        match self.scene_host.host().map(|h| &h.world) {
            Some(w) => (bh::battle_chip_offsets(w), bh::battle_chip_dpad_visible(w)),
            None => ([(0, 0); 4], true),
        }
    }

    /// The command menu's chips this frame (the shared projection,
    /// `legaia_engine_screens::battle_command`).
    pub(crate) fn battle_command_menu_chips(&self) -> Option<CommandChips> {
        legaia_engine_screens::battle_command::battle_command_menu_chips(
            &self.scene_host.host()?.world,
        )
    }

    /// The battle HUD's chrome sprites, the item window's frames and the
    /// command chips, or outside battle the field-side message banner's frame,
    /// for the page's system-UI atlas array. The composition is
    /// `legaia_engine_screens::battle_screens::battle_chrome_sprite_draws`,
    /// the native window's call too.
    pub(crate) fn battle_chrome_sprite_draws(
        &self,
        assets: &crate::play_menu::PlayMenuAssets,
        surface_w: u32,
        surface_h: u32,
    ) -> Vec<ui::SpriteDraw> {
        self.battle_screen_inputs(assets)
            .map(|inputs| {
                legaia_engine_screens::battle_screens::battle_chrome_sprite_draws(
                    &inputs,
                    surface_w.max(1),
                    surface_h.max(1),
                )
            })
            .unwrap_or_default()
    }

    pub(crate) fn battle_overlay_draws(
        &self,
        assets: &crate::play_menu::PlayMenuAssets,
        surface_w: u32,
        surface_h: u32,
    ) -> Vec<TextDraw> {
        use legaia_engine_battle::target_picker::{CursorRow, PickerState};
        use legaia_engine_menus::battle_input::CommandPhase;

        let Some(bw) = self.scene_host.host().map(|h| &h.world) else {
            return Vec::new();
        };
        // Muscle Dome leg: the battle HUD's text half - the status plate and
        // the command cluster's labels - off the same builders a battle
        // draws, fed by the dome session's command flow. The native window
        // makes the same two calls.
        if bw.mode == SceneMode::MuscleDome {
            if !self.dome_battle_chrome_up() {
                return Vec::new();
            }
            let mut out = self
                .battle_hud_frame_draws(assets, surface_w, surface_h)
                .text;
            if let Some((chips, cursor, phase)) = self.battle_command_menu_chips() {
                use legaia_engine_ui::battle_command_ui as bcu;
                let (origin, scale) =
                    crate::play_menu::stage_transform(surface_w.max(1), surface_h.max(1));
                let views = bcu::command_chip_views(&chips);
                out.extend(bcu::battle_command_chip_text(
                    assets.font_ref(),
                    &bcu::BattleCommandMenuFrame {
                        chips: &views,
                        cursor,
                        phase,
                        offsets: self.battle_chip_glide().0,
                        dpad: self.battle_chip_glide().1,
                    },
                    origin,
                    scale,
                ));
            }
            return out;
        }
        if bw.mode != SceneMode::Battle {
            return Vec::new();
        }
        let font = assets.font_ref();
        let white = [1.0f32, 1.0, 1.0, 1.0];
        let dim = [0.7f32, 0.85, 1.0, 1.0];
        // Greyed-out row tint (K.O.'d targets, unaffordable spells).
        let down_color = [0.6f32, 0.6, 0.6, 1.0];
        let mut out: Vec<TextDraw> = Vec::new();

        // The retail party strip (one full-width lozenge per live member
        // across the stage bottom), the top-left plaque and the floating
        // popups all come from the shared builder. Its text half lands here;
        // its chrome sprites go out of `battle_chrome_sprite_draws` into the
        // page's sprite array. Numerals carry the ported retail readout-tint
        // law (`hp_bar_color_index` / `mp_bar_color_index`, FUN_800349EC /
        // FUN_80035EA8). Rows are fed from the `BattleHud` model, refreshed
        // each tick by the shared `sync_battle_hud_rows` fold.
        out.extend(
            self.battle_hud_frame_draws(assets, surface_w, surface_h)
                .text,
        );

        // Floating value readout: the numeral a landed hit throws, over the
        // struck actor. With a battle render up, the page draws retail's own
        // 24x24 cells off the resident effect atlas through the screen-prim
        // pass (`battle_value_readout_prims`) and this font fallback stays
        // empty; it only draws before the battle VRAM exists.
        out.extend(self.battle_value_readout_draws(font, surface_w, surface_h));

        // Encounter-transition banner: centred "ENCOUNTER!" over the
        // formation label, shown for the opening frames of the battle. A port
        // invention with no retail counterpart - retail's Field -> Battle edge
        // draws no banner at all - so it is gated off by default and only
        // appears under `LEGAIA_DIAG_HUD` (`encounter_banner_enabled`).
        if let Some((_, label)) = self
            .encounter_banner
            .as_ref()
            .filter(|_| encounter_banner_enabled())
        {
            let head_w = font.layout_ascii("ENCOUNTER!").advance_x as i32;
            let pen = ((surface_w as i32 - head_w) / 2, surface_h as i32 / 4);
            out.extend(ui::encounter_banner_draws_for(font, label, pen));
        }

        // Player-driven submenus (opened from the Arts / Magic / Item
        // commands). Each parks both the SM and the command session while
        // open, so it takes priority over the command menu. While an
        // in-battle dialogue box owns the frame (the tutorial text), the
        // menus are hidden - retail shows no command chrome under it.
        let dialogue_up = bw.dialogue_owns_input();
        if dialogue_up {
            // Dialogue box up: no menu chrome.
        } else if let Some(arts) = &bw.battle.arts_menu {
            use legaia_engine_battle::battle_arts::ArtsPhase;
            let mut my = MENU_Y;
            match &arts.phase {
                ArtsPhase::Select { cursor } => {
                    let header = format!("P{} - arts:", arts.actor + 1);
                    out.extend(ui::text_draws_for(
                        &font.layout_ascii(&header),
                        (MENU_X, my),
                        white,
                    ));
                    my += 16;
                    if arts.arts.is_empty() {
                        out.extend(ui::text_draws_for(
                            &font.layout_ascii("  (no saved arts)"),
                            (MENU_X + 8, my),
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
                        out.extend(ui::text_draws_for(
                            &font.layout_ascii(&line),
                            (MENU_X + 8, my),
                            color,
                        ));
                        my += 14;
                    }
                }
                ArtsPhase::Targeting { picker, .. } => {
                    // Enemy cursor: the retail dedup name strip
                    // (FUN_801D9D3C rows + layout). Ally / sweep states keep
                    // the text line.
                    if let Some(strip) =
                        legaia_engine_screens::battle_screens::enemy_target_strip_draws(picker)
                    {
                        out.extend(strip);
                    } else {
                        let line = match picker.state() {
                            PickerState::Cursor {
                                row: CursorRow::Ally,
                                slot,
                            } => format!("art -> target P{}", slot + 1),
                            _ => "art -> select target".to_string(),
                        };
                        out.extend(ui::text_draws_for(
                            &font.layout_ascii(&line),
                            (MENU_X, my),
                            white,
                        ));
                    }
                    my += 14;
                    out.extend(ui::text_draws_for(
                        &font.layout_ascii("Left/Right=move  Cross=confirm  Circle=back"),
                        (MENU_X, my),
                        dim,
                    ));
                }
                _ => {}
            }
        } else if let Some(spell) = &bw.battle.spell_menu {
            use legaia_engine_battle::battle_magic::SpellPhase;
            let mut my = MENU_Y;
            match &spell.phase {
                SpellPhase::Select { cursor } => {
                    let header = format!("P{} - magic:", spell.actor + 1);
                    out.extend(ui::text_draws_for(
                        &font.layout_ascii(&header),
                        (MENU_X, my),
                        white,
                    ));
                    my += 16;
                    if spell.spells.is_empty() {
                        out.extend(ui::text_draws_for(
                            &font.layout_ascii("  (no spells)"),
                            (MENU_X + 8, my),
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
                        out.extend(ui::text_draws_for(
                            &font.layout_ascii(&line),
                            (MENU_X + 8, my),
                            color,
                        ));
                        my += 14;
                    }
                }
                SpellPhase::Targeting { picker, .. } => {
                    if let Some(strip) =
                        legaia_engine_screens::battle_screens::enemy_target_strip_draws(picker)
                    {
                        out.extend(strip);
                    } else {
                        let line = match picker.state() {
                            PickerState::Cursor {
                                row: CursorRow::Ally,
                                slot,
                            } => format!("cast -> target P{}", slot + 1),
                            _ => "cast -> select target".to_string(),
                        };
                        out.extend(ui::text_draws_for(
                            &font.layout_ascii(&line),
                            (MENU_X, my),
                            white,
                        ));
                    }
                    my += 14;
                    out.extend(ui::text_draws_for(
                        &font.layout_ascii("Left/Right=move  Cross=confirm  Circle=back"),
                        (MENU_X, my),
                        dim,
                    ));
                }
                _ => {}
            }
        } else if bw.battle.item_menu.is_some() {
            // Retail's item window (state 0x3C): the packet-pinned list +
            // description windows with breadcrumbs and the hand cursor.
            // Same engine-core projection + engine-ui builder the native
            // window uses; the chrome sprites ride
            // `battle_chrome_sprite_draws`.
            if let Some(model) = bw.battle_item_menu_model() {
                let (origin, scale) =
                    crate::play_menu::stage_transform(surface_w.max(1), surface_h.max(1));
                out.extend(legaia_engine_screens::with_battle_item_frame(
                    &model,
                    |frame| {
                        legaia_engine_ui::battle_item_ui::battle_item_window_text(
                            font, frame, origin, scale,
                        )
                    },
                ));
            }
        } else if let Some(cmd) = &bw.battle.command {
            let mut my = MENU_Y;
            match &cmd.phase {
                CommandPhase::RoundPrompt { .. }
                | CommandPhase::Menu { .. }
                | CommandPhase::AttackMode { .. }
                | CommandPhase::CommitConfirm { .. } => {
                    // Retail's command surfaces are chip clusters, not
                    // lists: the round-open `Begin | Run` pair, the
                    // packet-pinned four-arm diamond, and the
                    // `Auto | Command` pair on the diamond's own arms.
                    // Same shared builder + same projector the native
                    // window uses, so the two hosts cannot drift; the
                    // plates go out in `battle_chrome_sprite_draws`.
                    if let Some((chips, cursor, phase)) = self.battle_command_menu_chips() {
                        use legaia_engine_ui::battle_command_ui as bcu;
                        let (origin, scale) =
                            crate::play_menu::stage_transform(surface_w.max(1), surface_h.max(1));
                        let views = bcu::command_chip_views(&chips);
                        out.extend(bcu::battle_command_chip_text(
                            font,
                            &bcu::BattleCommandMenuFrame {
                                chips: &views,
                                cursor,
                                phase,
                                offsets: self.battle_chip_glide().0,
                                dpad: self.battle_chip_glide().1,
                            },
                            origin,
                            scale,
                        ));
                    }
                }
                CommandPhase::Targeting { command, picker } => {
                    if let Some(strip) =
                        legaia_engine_screens::battle_screens::enemy_target_strip_draws(picker)
                    {
                        out.extend(strip);
                    } else {
                        let line = match picker.state() {
                            PickerState::Cursor {
                                row: CursorRow::Ally,
                                slot,
                            } => format!("{} -> target P{}", command.label(), slot + 1),
                            _ => format!("{} -> select target", command.label()),
                        };
                        out.extend(ui::text_draws_for(
                            &font.layout_ascii(&line),
                            (MENU_X, my),
                            white,
                        ));
                    }
                    my += 14;
                    let hint = "Left/Right=move  Cross=confirm  Circle=back";
                    out.extend(ui::text_draws_for(
                        &font.layout_ascii(hint),
                        (MENU_X, my),
                        dim,
                    ));
                }
                _ => {}
            }
        }

        // The sparring-tutorial prompt box is NOT part of this list: its rect
        // is in retail's 320x240 stage space while everything above is in
        // surface pixels, and it is a framed window rather than loose text.
        // It is built by `battle_tutorial_stage_draws` /
        // `battle_tutorial_chrome_draws` and folded into the stage-scaled
        // group in `play_overlay_draws_json`.
        out
    }
}
