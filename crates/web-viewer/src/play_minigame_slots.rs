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

use crate::play_minigames::{DIM, PEN_PROMPT, PEN_STATUS, WHITE, row};
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
}

impl LegaiaRuntime {
    /// The live slot session on the scene host's world.
    fn slot_session(&self) -> Option<&SlotMachine> {
        self.scene_host
            .as_ref()?
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
    }

    /// The native window's two slot HUD lines (`window/hud.rs`), with the
    /// page's exit binding named.
    pub(crate) fn slot_status_draws(&self, font: &legaia_font::Font) -> Vec<TextDraw> {
        let Some(m) = self.slot_session() else {
            return Vec::new();
        };
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
        let l1 = format!("SLOTS  {reels}  coins {}{feature}", m.balance());
        let prompt = match m.phase() {
            SlotPhase::Idle if !m.can_spin() => "not enough coins".to_string(),
            SlotPhase::Idle => format!("Cross = spin ({} coins)", m.spin_cost()),
            SlotPhase::Spinning => "spinning...".to_string(),
            SlotPhase::Stopping => "Square/Cross/Circle = stop reels 1/2/3".to_string(),
            SlotPhase::Payout => match m.last_result() {
                Some(r) if r.payout > 0 => {
                    format!("WIN +{} coins!  (Cross = collect)", r.payout)
                }
                _ => "no win  (Cross = continue)".to_string(),
            },
            SlotPhase::CashedOut => "cashed out".to_string(),
        };
        let l2 = format!("{prompt}   (Start = cash out + leave)");
        let mut out = row(font, &l1, PEN_STATUS, WHITE);
        out.extend(row(font, &l2, PEN_PROMPT, DIM));
        out
    }
}

#[wasm_bindgen]
impl LegaiaRuntime {
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
