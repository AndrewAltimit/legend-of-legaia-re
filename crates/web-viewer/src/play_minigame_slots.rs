//! The casino **slot machine** on the play page: the in-world session's
//! read-outs and the art the page draws it with.
//!
//! The rules run in the engine (`World::tick_slot_machine`: Cross spins,
//! stops each reel, collects; the balance seeds from and cashes out into
//! `World::minigames.casino_coins`). This module hands the page what the
//! standalone minigames page's renderer (`site/_content/minigames.html`,
//! `slotRender`) reads, in the same JSON shapes, over the **world's** session:
//! the 3D scene graph and art pack decode through the shared presentation
//! bundle ([`crate::play_minigames`]), while the live reel positions, strips,
//! phase and bonus latch come off the engine session directly. So the play
//! page and the standalone page draw one machine from one decoder, and the
//! reel that pays is the reel that draws.
//!
//! Every export here is prefixed `play_mg_slot_` and is a thin delegate;
//! nothing about the machine's presentation is decided in this file.

use legaia_engine_core::slot_machine::{REEL_COUNT, STRIP_LEN, SlotMachine, SlotPhase};
use legaia_engine_ui::TextDraw;
use wasm_bindgen::prelude::*;

use crate::runtime::LegaiaRuntime;

/// Slot presentation edges the page cannot read off the session alone.
#[derive(Default)]
pub(crate) struct SlotUi {
    /// Last tick's phase, for the payout edge.
    prev_phase: Option<SlotPhase>,
    /// Coins a spin banked this tick (the page's payout-caption cue); `0`
    /// on every other tick.
    pub(crate) credited: i32,
    /// The credited spin was a bonus-round spin (the marquee tally caption).
    pub(crate) credited_bonus: bool,
    /// Frame counter for the marquee's attract sweep + blink.
    pub(crate) tick: u32,
    /// The marquee's legend / blink counters for the shared cabinet builder
    /// - the native window's `slot_marquee_clock` twin.
    pub(crate) clock: legaia_engine_ui::ui_slot_cabinet::SlotMarqueeClock,
    /// This frame's composed dot buffer.
    pub(crate) dots: Vec<u8>,
}

impl LegaiaRuntime {
    /// The live slot session on the scene host's world.
    fn slot_session(&self) -> Option<&SlotMachine> {
        self.scene_host
            .host()?
            .world
            .minigames
            .slot_machine
            .as_ref()
    }

    pub(crate) fn enter_slot_ui(&mut self) {
        self.minigame_ui.slot = SlotUi::default();
    }

    /// Per-tick: the payout edge (`Payout` -> `Idle` on the collect press)
    /// becomes the page's credit cue, so the marquee can hold the tally and
    /// slide the payout figure in the way retail's caption does.
    pub(crate) fn tick_slot_ui(&mut self) {
        let (phase, last) = match self.slot_session() {
            Some(m) => (Some(m.phase()), m.last_result()),
            None => (None, None),
        };
        let ui = &mut self.minigame_ui.slot;
        ui.tick = ui.tick.wrapping_add(1);
        ui.credited = 0;
        ui.credited_bonus = false;
        if ui.prev_phase == Some(SlotPhase::Payout)
            && phase == Some(SlotPhase::Idle)
            && let Some(r) = last
            && r.payout > 0
        {
            ui.credited = r.payout;
            ui.credited_bonus = r.bonus_spin;
        }
        ui.prev_phase = phase;
        // The marquee's dot buffer for the shared cabinet builder, off the
        // same `SlotMachine::marquee` state the native window composes from.
        let marquee = self.slot_session().map(|m| (m.marquee(), m.anticipation()));
        let messages = self
            .minigame_art()
            .and_then(|a| a.slot_cabinet.as_ref())
            .map(|c| c.scene.messages.clone());
        let ui = &mut self.minigame_ui.slot;
        ui.dots = match (marquee, messages) {
            (Some((f, reach)), Some(msgs)) => ui.clock.frame(&f, reach, &msgs),
            _ => Vec::new(),
        };
    }

    /// The whole machine as screen primitives through the shared
    /// `ui_slot_cabinet::slot_cabinet_prims` - the native window's
    /// `slot_cabinet_screen_prims` twin. Empty outside a session.
    pub(crate) fn slot_cabinet_prims(&self) -> Vec<legaia_engine_ui::screen_prim::ScreenPrim> {
        use legaia_engine_ui::ui_slot_cabinet as usc;
        let (Some(m), Some(c)) = (
            self.slot_session(),
            self.minigame_art().and_then(|a| a.slot_cabinet.as_ref()),
        ) else {
            return Vec::new();
        };
        // The cash-out flow over the machine (the picker, the prompt, the
        // fade) - or, on a rules page, in its place.
        let menu = usc::slot_menu_prims(c, m.screen(), m.fade_level());
        if matches!(
            m.screen(),
            legaia_engine_core::slot_machine::SlotScreen::Instructions { .. }
        ) {
            return menu;
        }
        let strips = m.strips();
        let clear;
        let dots: &[u8] = if self.minigame_ui.slot.dots.is_empty() {
            clear = legaia_asset::minigame_slot_scene::clear_dots();
            &clear
        } else {
            &self.minigame_ui.slot.dots
        };
        let mut prims = usc::slot_cabinet_prims(&usc::SlotCabinetInput {
            scene: &c.scene,
            cabinet: c.cabinet.as_ref(),
            hud: &c.hud,
            reel_pos: core::array::from_fn(|r| m.reel_pos(r)),
            strips: [&strips[0], &strips[1], &strips[2]],
            stop_open: core::array::from_fn(|r| m.reel_stop_open(r)),
            winning_line: m.winning_line_word(),
            dots,
            blink: self.minigame_ui.slot.clock.blink,
            balance: m.balance(),
        });
        // The paylines over it: the machine's own ported pass + projection
        // (`SlotMachine::payline_segments`) through the shared
        // `ui_slot_paylines` builder - the native window's
        // `slot_payline_screen_prims` twin. The 2D-canvas fallback strokes
        // them itself, so they join the prim pass only with the cabinet.
        use legaia_engine_ui::ui_slot_paylines as usp;
        let segments: Vec<usp::PaylineSegment> = m
            .payline_segments()
            .iter()
            .map(|l| usp::PaylineSegment {
                a: [l.a.0, l.a.1],
                b: [l.b.0, l.b.1],
                rgb: [l.prim.color.0, l.prim.color.1, l.prim.color.2],
                semi: l.prim.code & 0x02 != 0,
            })
            .collect();
        prims.extend(usp::payline_screen_prims(&segments));
        prims.extend(menu);
        prims
    }

    /// The slot HUD rows, the engine's (`minigame_status::slot_status_rows`)
    /// through the shared draw kernel the native window calls. Like the
    /// window, only while the machine itself is not drawn: its own marquee,
    /// lamps and coin readout carry all of it.
    pub(crate) fn slot_status_draws(&self, font: &legaia_font::Font) -> Vec<TextDraw> {
        let Some(m) = self.slot_session() else {
            return Vec::new();
        };
        if let Some(c) = self.minigame_art().and_then(|a| a.slot_cabinet.as_ref()) {
            // The machine draws itself; the rules pages' text is the one
            // text draw it owes (`ui_slot_cabinet::slot_rules_text_draws_for`,
            // the native window's twin).
            return c
                .rules
                .as_ref()
                .map(|r| {
                    legaia_engine_ui::ui_slot_cabinet::slot_rules_text_draws_for(
                        font,
                        r,
                        m.screen(),
                        m.fade_level(),
                    )
                })
                .unwrap_or_default();
        }
        let rows = legaia_engine_core::minigame_status::slot_status_rows(m);
        legaia_engine_ui::ui_text_lines::status_row_draws_for(
            font,
            rows.iter().map(|r| (r.text.as_str(), r.pen, r.bright)),
        )
    }
}

#[wasm_bindgen]
impl LegaiaRuntime {
    /// Whether the machine's resident set decoded, so the page draws the
    /// cabinet through the shared screen-prim pass (and its VRAM through
    /// [`Self::play_mg_slot_vram`]) instead of its 2D-canvas composition.
    pub fn play_mg_slot_cabinet_ready(&self) -> bool {
        self.minigame_art()
            .is_some_and(|a| a.slot_cabinet.is_some())
    }

    /// The machine's VRAM - the art pack at its own framebuffer
    /// destinations - for the page's renderer while the machine is up.
    pub fn play_mg_slot_vram(&self) -> Vec<u8> {
        self.minigame_art()
            .and_then(|a| a.slot_cabinet.as_ref())
            .map(|c| c.vram.as_bytes().to_vec())
            .unwrap_or_default()
    }

    /// Is the in-world slot session live?
    pub fn play_mg_slot_active(&self) -> bool {
        self.slot_session().is_some()
    }

    /// The live machine's state in the standalone page's `slot_state_json`
    /// shape (`{ live, phase, balance, cost, can_spin, can_stop, stopped,
    /// feature_mode, bonus_spins, net_take, window, last, credited,
    /// credited_bonus, tick }`), read off the **world's** session. `credited`
    /// is the payout the collect press banked this tick (the page's caption
    /// cue), `tick` the marquee clock.
    pub fn play_mg_slot_state_json(&self) -> String {
        let Some(m) = self.slot_session() else {
            return r#"{"live":false}"#.to_string();
        };
        let phase = match m.phase() {
            SlotPhase::Idle => "idle",
            SlotPhase::Spinning => "spinning",
            SlotPhase::Stopping => "stopping",
            SlotPhase::Payout => "payout",
            SlotPhase::Menu => "menu",
            SlotPhase::NoCoins => "no_coins",
            SlotPhase::Leaving => "leaving",
            SlotPhase::CashedOut => "cashed_out",
        };
        let strips = m.strips();
        let len = STRIP_LEN as isize;
        let window: Vec<Vec<u8>> = (0..REEL_COUNT)
            .map(|r| {
                let rowi = m.payline_row(r) as isize;
                [-1isize, 0, 1]
                    .iter()
                    .map(|off| strips[r][(rowi + off).rem_euclid(len) as usize])
                    .collect()
            })
            .collect();
        let last = m.last_result().map(|r| {
            serde_json::json!({
                "line": r.line,
                "symbol": r.symbol,
                "payout": r.payout,
                "bonus_triggered": r.bonus_triggered,
                "bonus_spin": r.bonus_spin,
            })
        });
        let ui = &self.minigame_ui.slot;
        serde_json::json!({
            "live": true,
            "phase": phase,
            "balance": m.balance(),
            "cost": m.spin_cost(),
            "can_spin": m.can_spin(),
            "can_stop": m.can_stop(),
            "stopped": m.reels_stopped(),
            "feature_mode": m.feature_mode(),
            "bonus_spins": m.bonus_spins(),
            "net_take": m.net_take(),
            "window": window,
            "last": last,
            "credited": ui.credited,
            "credited_bonus": ui.credited_bonus,
            "tick": ui.tick,
        })
        .to_string()
    }

    /// The bonus game's state, the standalone page's `slot_bonus_json` shape
    /// over the world's session: the two jackpot triggers, and while a round
    /// is live the reel numbers, the claimed-column tally and its product.
    pub fn play_mg_slot_bonus_json(&self) -> String {
        use legaia_asset::slot_payout as sp;
        let head = serde_json::json!({
            "kick_symbol": sp::KICK_SYMBOL_ID,
            "kick_rounds": sp::KICK_BONUS_ROUNDS,
            "punch_symbol": sp::PUNCH_SYMBOL_ID,
            "punch_rounds": sp::PUNCH_BONUS_ROUNDS,
            "min": sp::BONUS_PAYOUT_MIN,
            "max": sp::BONUS_PAYOUT_MAX,
        });
        let mut out = head;
        let Some(m) = self.slot_session() else {
            out["active"] = false.into();
            out["rounds_left"] = 0.into();
            out["numbers"] = serde_json::json!([]);
            out["tally"] = serde_json::json!([]);
            out["claimed"] = serde_json::json!([]);
            out["complete"] = false.into();
            out["product"] = 0.into();
            return out.to_string();
        };
        let numbers: Vec<u32> = (0..REEL_COUNT)
            .map(|r| sp::bonus_number_for_value(m.payline_symbol(r)) as u32)
            .collect();
        let claimed: Vec<bool> = (0..REEL_COUNT)
            .map(|r| m.claimed(r) > sp::BONUS_VALUE_BIAS as i32)
            .collect();
        out["active"] = m.in_bonus_round().into();
        out["rounds_left"] = m.bonus_spins().max(0).into();
        out["numbers"] = serde_json::json!(numbers);
        out["tally"] = serde_json::json!(m.tally());
        out["claimed"] = serde_json::json!(claimed);
        out["complete"] = m.tally_complete().into();
        out["product"] = m.tally_product().into();
        out.to_string()
    }

    /// The live reel positions (`DAT_801d3cc0` fixed-point angles: high
    /// byte = strip row, low byte = the sub-symbol fraction the cylinder
    /// turns by). Empty outside a session.
    pub fn play_mg_slot_reel_pos(&self) -> Vec<i32> {
        match self.slot_session() {
            Some(m) => (0..REEL_COUNT).map(|r| m.reel_pos(r)).collect(),
            None => Vec::new(),
        }
    }

    /// The 20-symbol display strip of `reel`, as the renderer reads it.
    pub fn play_mg_slot_strip(&self, reel: usize) -> Vec<u8> {
        match self.slot_session() {
            Some(m) if reel < REEL_COUNT => m.strips()[reel].to_vec(),
            _ => Vec::new(),
        }
    }

    // ---- art + scene: delegates to the shared presentation bundle ----

    /// Whether the slot art pack (PROT 1200) decoded off this disc.
    pub fn play_mg_slot_art_ready(&self) -> bool {
        self.minigame_art().is_some_and(|a| a.slot_art_ready())
    }

    /// Whether the machine's 3D scene graph (PROT 0975 rodata) decoded.
    pub fn play_mg_slot_scene_ready(&self) -> bool {
        self.minigame_art().is_some_and(|a| a.slot_scene_ready())
    }

    /// The payline line prims (`LegaiaMinigames::slot_payline_prims_json`,
    /// the ported `FUN_801d3380` pass) for `winning_line`.
    pub fn play_mg_slot_payline_prims_json(&self, winning_line: i32) -> String {
        self.minigame_art()
            .map(|a| a.slot_payline_prims_json(winning_line))
            .unwrap_or_else(|| "[]".to_string())
    }

    /// The scene graph + projection (`LegaiaMinigames::slot_scene_json`).
    pub fn play_mg_slot_scene_json(&self) -> String {
        self.minigame_art()
            .map(|a| a.slot_scene_json())
            .unwrap_or_else(|| r#"{"ok":false}"#.to_string())
    }

    /// The marquee message-bank roles (`LegaiaMinigames::slot_marquee_json`).
    pub fn play_mg_slot_marquee_json(&self) -> String {
        self.minigame_art()
            .map(|a| a.slot_marquee_json())
            .unwrap_or_else(|| "null".to_string())
    }

    /// One reel symbol (`0..=9`) as 64x64 RGBA8 through its own CLUT.
    pub fn play_mg_slot_symbol_rgba(&self, sym: usize) -> Vec<u8> {
        self.minigame_art()
            .map(|a| a.slot_symbol_rgba(sym))
            .unwrap_or_default()
    }

    /// One bonus reel numeral (`1..=10`) as 64x64 RGBA8.
    pub fn play_mg_slot_bonus_number_rgba(&self, number: usize) -> Vec<u8> {
        self.minigame_art()
            .map(|a| a.slot_bonus_number_rgba(number))
            .unwrap_or_default()
    }

    /// The coin readout's 224x16 font strip.
    pub fn play_mg_slot_digits_rgba(&self) -> Vec<u8> {
        self.minigame_art()
            .map(|a| a.slot_digits_rgba())
            .unwrap_or_default()
    }

    /// The 127x239 paytable / coin panel (HUD record 0).
    pub fn play_mg_slot_panel_rgba(&self) -> Vec<u8> {
        self.minigame_art()
            .map(|a| a.slot_panel_rgba())
            .unwrap_or_default()
    }

    /// One art page through palette `palette`, RGBA8.
    pub fn play_mg_slot_page_rgba(&self, page: usize, palette: usize) -> Vec<u8> {
        self.minigame_art()
            .map(|a| a.slot_page_rgba(page, palette))
            .unwrap_or_default()
    }

    /// Pixel width of art page `page` (`0` when absent).
    pub fn play_mg_slot_page_width(&self, page: usize) -> usize {
        self.minigame_art()
            .map(|a| a.slot_page_width(page))
            .unwrap_or(0)
    }
}
