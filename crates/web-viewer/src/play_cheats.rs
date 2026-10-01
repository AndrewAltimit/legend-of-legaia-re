//! The play page's **Cheats** panel: thin wasm bindings over
//! [`legaia_engine_core::cheats`], where every mutation lives (the native
//! `play-window`'s `--cheat-*` flags call the same methods).

use crate::runtime::LegaiaRuntime;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
impl LegaiaRuntime {
    /// Raise every present party member to `level` with the retail stat
    /// growth. Returns a one-line summary (empty roster -> a reason).
    pub fn cheat_set_party_level(&mut self, level: u8) -> String {
        let Some(h) = self.scene_host.as_mut() else {
            return "no disc loaded".to_string();
        };
        let got = h.world.cheat_set_party_level(level);
        if got.is_empty() {
            return "no party members to level".to_string();
        }
        let parts: Vec<String> = got
            .iter()
            .map(|&(slot, lv)| format!("{} Lv {lv}", h.world.party_name(slot as usize)))
            .collect();
        parts.join(", ")
    }

    /// Set the gold purse (clamped to the retail cap). Returns the purse.
    pub fn cheat_set_gold(&mut self, gold: f64) -> i32 {
        self.scene_host
            .as_mut()
            .map_or(0, |h| h.world.cheat_set_gold(gold as i64))
    }

    /// Set the casino coin bank (clamped). Returns the bank.
    pub fn cheat_set_coins(&mut self, coins: f64) -> u32 {
        self.scene_host
            .as_mut()
            .map_or(0, |h| h.world.cheat_set_coins(coins.max(0.0) as u64))
    }

    /// Add `qty` of the item `query` names (an id, `0x`-hex id, or item
    /// name). Returns a one-line summary for the page's status line.
    pub fn cheat_give_item(&mut self, query: &str, qty: u8) -> String {
        let Some(h) = self.scene_host.as_mut() else {
            return "no disc loaded".to_string();
        };
        let pairs = h.world.item_name_pairs();
        let Some(id) = legaia_engine_core::cheats::resolve_item(
            query,
            pairs.iter().map(|(i, n)| (*i, n.as_str())),
        ) else {
            return format!("no single item matches '{query}'");
        };
        let name = pairs
            .iter()
            .find(|(i, _)| *i == id)
            .map_or_else(|| format!("item {id:#04x}"), |(_, n)| n.clone());
        match h.world.cheat_give_item(id, qty) {
            Some(g) if g.granted == 0 && qty > 0 => {
                format!("{name}: bag full or stack at 99 (holding {})", g.held)
            }
            Some(g) => format!("{name} +{} (holding {})", g.granted, g.held),
            None => format!("no single item matches '{query}'"),
        }
    }

    /// Restore every party member's HP / MP (works mid-battle too).
    pub fn cheat_restore_party(&mut self) {
        if let Some(h) = self.scene_host.as_mut() {
            h.world.cheat_restore_party();
        }
    }

    /// Snapshot for the panel: `{party:[{name,level,hp,hp_max,mp,mp_max}],
    /// gold, coins, items:[[id,name],...]}`.
    pub fn cheat_state_json(&self) -> String {
        let Some(h) = self.scene_host.as_ref() else {
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
                    "level": rec.level().max(rec.magic_rank()).max(1),
                    "hp": hms.hp_cur, "hp_max": hms.hp_max,
                    "mp": hms.mp_cur, "mp_max": hms.mp_max,
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
