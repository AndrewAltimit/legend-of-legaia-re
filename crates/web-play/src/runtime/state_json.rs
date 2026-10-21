//! The HUD-facing JSON read-backs: engine state line, BGM, field-menu model,
//! mode word and the live dialogue box.

use super::*;

#[wasm_bindgen]
impl LegaiaRuntime {
    /// One-line engine state for the HUD:
    /// ```text
    /// { "scene": "town01", "frame": 421, "mode": "Field",
    ///   "actors": 12, "npcs": 9,
    ///   "player": { "x": 2688, "y": -256, "z": 2432, "facing": 2048,
    ///               "walking": true },
    ///   "dialog": { "text": "...", "options": ["Yes", "No"], "cursor": 0 },
    ///   "bgm": { "requested": 2019, "playing": 2019 } }
    /// ```
    /// `dialog` is `null` when no box is up. `bgm.requested` is the id the
    /// simulation's last op-`0x35` selected and `bgm.playing` is the id the
    /// audio output holds - equal on a healthy frame, and the one externally
    /// visible signal that a music change resolved without reaching the
    /// sequencer.
    pub fn state_json(&self) -> String {
        let Some(h) = self.scene_host.host() else {
            return serde_json::json!({
                "scene": serde_json::Value::Null,
                "frame": self.world.frame,
                "mode": format!("{:?}", self.world.mode),
                "actors": 0,
                "npcs": 0,
                "player": serde_json::Value::Null,
                "dialog": serde_json::Value::Null,
                "bgm": self.bgm_value(),
            })
            .to_string();
        };
        let w = &h.world;
        let player = w
            .player_actor_slot
            .and_then(|s| w.actors.get(s as usize))
            .map(|a| {
                serde_json::json!({
                    "x": a.move_state.world_x,
                    "y": a.move_state.world_y,
                    "z": a.move_state.world_z,
                    "facing": a.move_state.render_26,
                    "walking": w.locomotion.player_anim.as_ref().is_some_and(|f| f.walking),
                })
            })
            .unwrap_or(serde_json::Value::Null);
        serde_json::json!({
            "scene": h.scene.as_ref().map(|s| s.name.clone()),
            "frame": w.frame,
            "mode": format!("{:?}", w.mode),
            "actors": w.actors.iter().filter(|a| a.active).count(),
            "npcs": self.actors.npcs.as_ref().map(|n| n.pack.entries.len()).unwrap_or(0),
            "player": player,
            "dialog": self.dialog_value(),
            "bgm": self.bgm_value(),
        })
        .to_string()
    }

    /// `{ "requested": id|null, "playing": id|null }` - what the simulation's
    /// last op-`0x35` selected versus what the audio output is actually
    /// sounding.
    ///
    /// The two are the same value on a healthy frame, and their *drift* is the
    /// only external symptom of a BGM change that resolved but never reached
    /// the sequencer. `playing` is `null` whenever audio is not up, which is
    /// the ordinary state before the first user gesture.
    fn bgm_value(&self) -> serde_json::Value {
        let requested = self
            .scene_host
            .host()
            .and_then(|h| h.world.audio.current_bgm)
            .map(serde_json::Value::from)
            .unwrap_or(serde_json::Value::Null);
        let playing = self
            .scene_host
            .director()
            .and_then(|d| d.last_started)
            .map(serde_json::Value::from)
            .unwrap_or(serde_json::Value::Null);
        serde_json::json!({ "requested": requested, "playing": playing })
    }

    /// Field pause-menu model: the party (battle order) + inventory + gold the
    /// page's menu overlay renders when the player presses Start. Shape:
    /// ```text
    /// { "gold": 240,
    ///   "party": [{ "name": "Vahn", "level": 1, "hp": 60, "hp_max": 60,
    ///               "mp": 8, "mp_max": 8 }, ...],
    ///   "items": [{ "id": 32, "name": "Healing Leaf", "count": 3 }, ...] }
    /// ```
    /// `null` before a disc scene is entered. Item labels come from the SCUS
    /// item-name table ([`Self::load_disc`]); a PROT.DAT-only load falls back to
    /// the raw id. The retail pause menu is a native-only draw path (glyph atlas
    /// + window-descriptor table); this feeds the browser's HTML overlay
    /// equivalent so Start still surfaces the party / items on the play page.
    pub fn field_menu_model_json(&self) -> String {
        let Some(h) = self.scene_host.host() else {
            return "null".to_string();
        };
        let w = &h.world;
        let order: Vec<usize> = if w.party.active_party.is_empty() {
            (0..w.party.roster.members.len()).collect()
        } else {
            w.party.active_party.iter().map(|&s| s as usize).collect()
        };
        let party: Vec<serde_json::Value> = order
            .iter()
            .filter_map(|&slot| {
                let m = w.party.roster.members.get(slot)?;
                // A never-populated roster slot decodes to an all-zero record
                // (empty name) - skip it so the menu shows only real members.
                let name = m.name();
                if name.trim().is_empty() {
                    return None;
                }
                let hms = m.hp_mp_sp();
                Some(serde_json::json!({
                    "name": name,
                    "level": m.level(),
                    "hp": hms.hp_cur, "hp_max": hms.hp_max,
                    "mp": hms.mp_cur, "mp_max": hms.mp_max,
                }))
            })
            .collect();
        let items: Vec<serde_json::Value> = MenuRuntime::inventory_items(w)
            .into_iter()
            .map(|(id, count)| {
                let name = self
                    .item_names
                    .as_ref()
                    .and_then(|t| t.name(id))
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| format!("Item {id:#04x}"));
                serde_json::json!({ "id": id, "name": name, "count": count })
            })
            .collect();
        serde_json::json!({ "gold": w.party.money, "party": party, "items": items }).to_string()
    }

    /// The live retail mode word (`_DAT_8007B83C`), its table name, and the
    /// front-end entry word beside it, as
    /// `{"word": <u32>, "name": "<MODE>", "entry_word": <u32>}`.
    ///
    /// The page's own read of the seat this host holds - which is what makes
    /// a browser mode trace possible at all. Before the seat existed the
    /// front end ran with no mode word, so the two hosts could not be
    /// compared on the one register retail's whole dispatch keys off.
    ///
    /// `word` is the **mode table index** the native oracle samples
    /// (`ModeSeat::game_mode`). It used to be the seat's `entry_word`
    /// (`_DAT_8007BB00`, the front-end flag `init.pak`'s hand-off reads),
    /// which is a different global: the JSON paired that flag's value with
    /// the mode's *name*, so a reader comparing the two hosts' words was
    /// comparing two different registers and could not see a divergence.
    ///
    /// `edges` is the count of mode changes the seat has taken, and it is
    /// the field that makes the two hosts comparable at the **INIT** modes.
    /// An INIT mode lasts one frame *inside* `ModeSeat::enter` - the call
    /// resolves the staging plan and hands the word to the mode's RUN
    /// sibling before returning - so no sampler outside the call ever
    /// observes the word sitting on one. What it leaves behind is the extra
    /// edge, so a host that reaches `MAIN MODE` by entering `MAIN INIT` and
    /// one that reaches it by adopting the world's scene mode walk the same
    /// words and differ here.
    pub fn mode_state_json(&self) -> String {
        serde_json::json!({
            "word": self.mode_word(),
            "name": self.scene_host.seat().mode_name(),
            "entry_word": self.scene_host.seat().entry_word(),
            "edges": self.scene_host.seat().edges(),
        })
        .to_string()
    }
}

impl LegaiaRuntime {
    /// Decode the live dialogue box (the field VM's inline-script runner) into
    /// the JSON the HUD prints. Glyph bytes are ASCII-compatible from `0x20`.
    fn dialog_value(&self) -> serde_json::Value {
        let Some(h) = self.scene_host.host() else {
            return serde_json::Value::Null;
        };
        let Some(id) = h.world.dialog.inline.as_ref() else {
            return serde_json::Value::Null;
        };
        let ascii = |bytes: &[u8]| -> String {
            bytes
                .iter()
                .map(|&b| {
                    if (0x20..=0x7E).contains(&b) {
                        b as char
                    } else {
                        ' '
                    }
                })
                .collect::<String>()
                .trim_end()
                .to_string()
        };
        let text = ascii(&id.page_bytes());
        if text.trim().is_empty() {
            return serde_json::Value::Null;
        }
        let options: Vec<String> = id
            .menu_active()
            .then(|| id.picker())
            .flatten()
            .map(|_| id.picker_labels().iter().map(|l| ascii(l)).collect())
            .unwrap_or_default();
        serde_json::json!({
            "text": text,
            "options": options,
            "cursor": id.picker_cursor(),
        })
    }
}
