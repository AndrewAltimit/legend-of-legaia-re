//! The trading cards on the play page: the collection summary for the
//! page's UI, opening and closing the card screen, and the card notices.
//!
//! The screen is the engine's (`World::open_card_menu` and siblings, ticked
//! under `SceneMode::Expansion` by the world tick off the pad word the page
//! routes) and so is its drawing, through
//! `legaia_engine_screens::expansion_modes` - the composition the native
//! window draws, monster portraits included (the scene host renders them off
//! the page's disc image). This module is the twin of the native window's
//! backslash key and `--mode cards` flag.

use crate::runtime::LegaiaRuntime;
use legaia_engine_mode::mode::SceneMode;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
impl LegaiaRuntime {
    /// The active pack's card state as `{"available", "open", "cards",
    /// "distinct", "set_size", "unseen", "boosters"}`; `available` is false
    /// before a scene is loaded or without pack cards.
    pub fn play_cards_json(&self) -> String {
        let Some(host) = self.scene_host.host() else {
            return r#"{"available":false,"open":false}"#.to_string();
        };
        let w = &host.world;
        let c = &w.expansion.quests.progress.cards;
        let (distinct, set_size) = w
            .card_book()
            .map_or((0, 0), |b| b.completion(&c.collection));
        serde_json::json!({
            "available": w.cards_available(),
            "open": w.card_screen_open(),
            "cards": c.collection.total(),
            "distinct": distinct,
            "set_size": set_size,
            "unseen": c.unseen(),
            "boosters": c.total_boosters(),
        })
        .to_string()
    }

    /// Open a card screen over the field or the overworld: `"menu"`,
    /// `"binder"`, or `"dealer"` / `"dealer:<id>"`. `false` when it could not
    /// open (no scene, no pack cards, another screen up, unknown dealer).
    pub fn play_cards_open(&mut self, screen: &str) -> bool {
        let Some(host) = self.scene_host.host_mut() else {
            return false;
        };
        let w = &mut host.world;
        if !matches!(w.mode, SceneMode::Field | SceneMode::WorldMap) {
            return false;
        }
        match screen {
            "menu" => w.open_card_menu(),
            "binder" => w.open_card_binder(),
            s => match s.strip_prefix("dealer") {
                Some(d) => w.open_card_dealer(d.trim_start_matches(':')),
                None => false,
            },
        }
    }

    /// Close the card screen.
    pub fn play_cards_close(&mut self) {
        if let Some(host) = self.scene_host.host_mut() {
            host.world.close_card_screen();
        }
    }

    /// Take the card notices raised since the last call, as a JSON list of
    /// lines (`["Got card: Mist Wisp (new!)", ...]`).
    pub fn play_card_notices_json(&mut self) -> String {
        let lines: Vec<String> = self
            .scene_host
            .host_mut()
            .map(|h| h.world.take_card_notices())
            .unwrap_or_default()
            .iter()
            .map(|n| n.line())
            .collect();
        serde_json::to_string(&lines).unwrap_or_else(|_| "[]".into())
    }
}
