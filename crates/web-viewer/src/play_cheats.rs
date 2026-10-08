//! The play page's **Cheats** panel: thin wasm bindings over
//! [`legaia_engine_core::cheats::PlayerCheat`] - the one cheat list both
//! hosts expose, applied through `World::apply_cheat` (the native
//! `play-window`'s `--cheat-*` flags build the same values) - and over
//! [`legaia_engine_core::cheat_applier::apply_text`] for pasted codes (the
//! native `--cheat-file`).

use crate::runtime::LegaiaRuntime;
use legaia_engine_core::cheats::PlayerCheat;
use wasm_bindgen::prelude::*;

/// A [`PlayerCheat`] from the page's `(key, number, text)` triple.
fn cheat_from_key(key: &str, n: f64, text: &str) -> Option<PlayerCheat> {
    let byte = n.clamp(0.0, 255.0) as u8;
    Some(match key {
        "level" => PlayerCheat::PartyLevel(byte),
        "restore" => PlayerCheat::Restore,
        "max-ap" => PlayerCheat::MaxAp,
        "seru" => PlayerCheat::GrantSeru(byte),
        "arts" => PlayerCheat::LearnAllArts,
        "max-items" => PlayerCheat::MaxItems,
        "gold" => PlayerCheat::Gold(n as i64),
        "coins" => PlayerCheat::Coins(n.max(0.0) as u64),
        "item" => PlayerCheat::GiveItem {
            query: text.to_string(),
            qty: byte,
        },
        "encounters" => PlayerCheat::RandomEncounters(n != 0.0),
        _ => return None,
    })
}

impl LegaiaRuntime {
    fn apply_player_cheat(&mut self, cheat: &PlayerCheat) -> String {
        let Some(h) = self.scene_host.host_mut() else {
            return "no disc loaded".to_string();
        };
        let templates = h.new_game_defaults.as_ref().map(|d| d.party.clone());
        h.world.apply_cheat(cheat, templates.as_ref())
    }
}

#[wasm_bindgen]
impl LegaiaRuntime {
    /// Apply one cheat from the shared list
    /// ([`legaia_engine_core::cheats::PLAYER_CHEAT_LIST`]) by key, with a
    /// numeric operand `n` (level, Seru level, gold, coins, quantity; `1` /
    /// `0` for the encounters switch) and a text operand `text` (the item
    /// query). Returns the one-line outcome the native window logs for the
    /// same cheat. Unknown keys return a reason.
    pub fn cheat_apply(&mut self, key: &str, n: f64, text: &str) -> String {
        let Some(cheat) = cheat_from_key(key, n, text) else {
            return format!("unknown cheat '{key}'");
        };
        self.apply_player_cheat(&cheat)
    }

    /// Paste-in cheat codes (GameShark lines or a Mednafen `.cht`), applied
    /// through the engine's RAM-cell registry - the native `--cheat-file`
    /// path. `strict` honours conditional codes. Returns the summary line.
    pub fn cheat_apply_codes(&mut self, text: &str, strict: bool) -> String {
        let Some(h) = self.scene_host.host_mut() else {
            return "no disc loaded".to_string();
        };
        use legaia_engine_core::cheat_applier::{CheatTextFormat, apply_text};
        match apply_text(&mut h.world, text, CheatTextFormat::sniff(text), strict) {
            Ok(report) => report.summary(),
            Err(e) => format!("codes not understood: {e}"),
        }
    }

    /// [`Self::cheat_apply`] `"level"` (kept for cached pages).
    pub fn cheat_set_party_level(&mut self, level: u8) -> String {
        self.apply_player_cheat(&PlayerCheat::PartyLevel(level))
    }

    /// Set the gold purse (clamped to the retail cap). Returns the purse.
    pub fn cheat_set_gold(&mut self, gold: f64) -> i32 {
        self.apply_player_cheat(&PlayerCheat::Gold(gold as i64));
        self.scene_host.host().map_or(0, |h| h.world.party.money)
    }

    /// Set the casino coin bank (clamped). Returns the bank.
    pub fn cheat_set_coins(&mut self, coins: f64) -> u32 {
        self.apply_player_cheat(&PlayerCheat::Coins(coins.max(0.0) as u64));
        self.scene_host
            .host()
            .map_or(0, |h| h.world.minigames.casino_coins)
    }

    /// [`Self::cheat_apply`] `"item"` (kept for cached pages).
    pub fn cheat_give_item(&mut self, query: &str, qty: u8) -> String {
        self.apply_player_cheat(&PlayerCheat::GiveItem {
            query: query.to_string(),
            qty,
        })
    }

    /// [`Self::cheat_apply`] `"restore"` (kept for cached pages).
    pub fn cheat_restore_party(&mut self) {
        self.apply_player_cheat(&PlayerCheat::Restore);
    }

    /// [`Self::cheat_apply`] `"max-ap"` (kept for cached pages).
    pub fn cheat_max_ap(&mut self) -> String {
        self.apply_player_cheat(&PlayerCheat::MaxAp)
    }

    /// [`Self::cheat_apply`] `"seru"` (kept for cached pages).
    pub fn cheat_grant_seru(&mut self, level: u8) -> String {
        self.apply_player_cheat(&PlayerCheat::GrantSeru(level))
    }

    /// [`Self::cheat_apply`] `"arts"` (kept for cached pages).
    pub fn cheat_learn_all_arts(&mut self) -> String {
        self.apply_player_cheat(&PlayerCheat::LearnAllArts)
    }

    /// [`Self::cheat_apply`] `"max-items"` (kept for cached pages).
    pub fn cheat_max_items(&mut self) -> String {
        self.apply_player_cheat(&PlayerCheat::MaxItems)
    }

    /// Snapshot for the panel: `{party:[{name,level,hp,hp_max,mp,mp_max,ap}],
    /// gold, coins, items:[[id,name],...]}`.
    pub fn cheat_state_json(&self) -> String {
        let Some(h) = self.scene_host.host() else {
            return "null".to_string();
        };
        let w = &h.world;
        let party: Vec<serde_json::Value> = (0..usize::from(w.party.party_count.min(3)))
            .filter_map(|m| {
                let r = w.party_roster_slot(m);
                let rec = w.party.roster.members.get(r)?;
                let hms = rec.hp_mp_sp();
                Some(serde_json::json!({
                    "name": w.party_name(r),
                    "level": rec.level().max(1),
                    "hp": hms.hp_cur, "hp_max": hms.hp_max,
                    "mp": hms.mp_cur, "mp_max": hms.mp_max,
                    "ap": hms.sp_cur,
                }))
            })
            .collect();
        let items: Vec<serde_json::Value> = w
            .item_name_pairs()
            .into_iter()
            .map(|(id, n)| serde_json::json!([id, n]))
            .collect();
        serde_json::json!({
            "party": party,
            "gold": w.party.money,
            "coins": w.minigames.casino_coins,
            "items": items,
        })
        .to_string()
    }
}
