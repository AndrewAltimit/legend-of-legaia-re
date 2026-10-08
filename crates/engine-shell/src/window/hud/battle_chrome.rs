//! The battle HUD chrome the play window draws: the frame and badge rects,
//! the banner message, the item / command menu chips, the Muscle Dome chrome
//! gate and the enemy target strip.
//! Split out of `hud.rs`; no logic change.

use super::*;

impl PlayWindowApp {
    /// The solid-white font-atlas texel the battle HUD's filled rects sample
    /// (`font_solid_src`). Scanned once per process - the window's font never
    /// changes after startup.
    pub(in crate::window) fn battle_hud_solid_src(&self) -> Option<(u32, u32, u32, u32)> {
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
    pub(in crate::window) fn battle_hud_frame_draws(
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
                plaque_dy: bh::battle_action_plaque_dy(w_ref),
                target_plaque_dy: bh::battle_target_plaque_dy(w_ref),
                bar_dy: bh::battle_readout_bar_dy(w_ref),
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
    pub(in crate::window) fn battle_badge_rects(
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
    pub(in crate::window) fn battle_banner_message(&self) -> Option<String> {
        self.save_menu.as_ref()?;
        legaia_engine_core::battle_hud::battle_banner_message(&self.session.host.world)
    }

    /// The engine-core battle-item-window projection (shared with the
    /// browser play page - `World::battle_item_menu_model` owns the gating
    /// and text resolution; this window only borrows it into the builder's
    /// frame via [`with_battle_item_frame`]).
    pub(in crate::window) fn battle_item_menu_model(
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
    pub(in crate::window) fn dome_battle_chrome_up(&self) -> bool {
        self.session.host.world.mode == SceneMode::MuscleDome
            && self.session.host.world.minigames.muscle_dome.is_some()
            && !self.session.host.world.minigames.muscle_hub.covers_leg()
    }

    pub(in crate::window) fn battle_command_menu_chips(&self) -> Option<CommandChips> {
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
    pub(in crate::window) fn battle_chrome_sprite_draws(
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
    pub(in crate::window) fn enemy_target_strip_draws(
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
