//! `LegaiaMinigames` WASM bindings for site/minigames.html - the browser twin
//! of the play-window's minigame entry points (`window/minigames.rs`).
//!
//! Three of the game's side-games are ported rules engines in
//! `legaia-engine-core`, each driven entirely by a table baked into its own
//! runtime overlay:
//!
//! | Game | Overlay | Table |
//! |---|---|---|
//! | Noa's dance | PROT 0980 | step chart (`legaia_asset::dance_chart`) |
//! | Baka Fighter | PROT 0976 | roster + action tables (`legaia_asset::baka_opponents`) |
//! | Casino slots | PROT 0975 | per-symbol payout table (`legaia_asset::slot_payout`) |
//!
//! This module is the thin JSON shell over those engines. The load path is
//! identical to the play-window's and to the disc-gated `*_minigame_real`
//! tests: read the raw PROT entry out of the user's own disc, lift it to its
//! statically-recovered loaded form via [`static_overlay::as_loaded`], and hand
//! the bytes to the table parser. No table is shipped with the site - every
//! number a game plays with is read out of the disc the visitor supplied, in
//! their browser, and never leaves it.
//!
//! The engines are the rules; the JS page is the presentation. Everything with
//! a `// PORT:` provenance tag lives in `legaia-engine-core`, not here.

use super::*;

use legaia_asset::minigame_art::{self, SlotHudWidget};
use legaia_asset::minigame_sfx::{self, SfxCueBank};
use legaia_asset::static_overlay;
use legaia_engine_core::baka_fighter::{BakaAttack, BakaFight, MatchPhase};
use legaia_engine_core::dance::{DanceDir, DanceGame, Judge};
use legaia_engine_core::slot_machine::{SlotMachine, SlotPhase};
use legaia_tim::Tim;

/// The three side-games playable in the browser, plus the disc they read.
#[wasm_bindgen]
pub struct LegaiaMinigames {
    /// Extracted `PROT.DAT` bytes (the games only ever need PROT entries).
    prot: Vec<u8>,
    /// PROT TOC.
    entries: Vec<disc::EntryMeta>,

    /// Live dance run.
    dance: Option<DanceGame>,
    /// Live Baka Fighter duel.
    baka: Option<BakaFight>,
    /// Parsed Baka roster + action tables (cached; the roster picker reads them
    /// before a fight starts).
    baka_tables: Option<(
        Vec<legaia_asset::baka_opponents::BakaOpponent>,
        Vec<legaia_asset::baka_opponents::BakaActionSet>,
    )>,
    /// Live slot-machine session.
    slot: Option<SlotMachine>,
    /// Parsed slot payout table (cached; the paytable panel reads it before a
    /// session starts).
    slot_payouts: Option<legaia_asset::slot_payout::SlotPayoutTable>,
    /// The slot machine's five-TIM art pack (PROT 1200) - the reel symbols,
    /// cabinet, digit font and banners the retail machine draws with.
    slot_art: Option<Vec<Tim>>,
    /// The 3 HUD widget descriptors out of the slot overlay (PROT 0975).
    slot_hud: Option<Vec<SlotHudWidget>>,
    /// The slot machine's own SFX cue bank (descriptors from PROT 1199, samples
    /// from the PROT 1198 VAB).
    slot_sfx: Option<SfxCueBank>,
    /// The Baka Fighter roster's fighter names (roster record `+0x00`).
    baka_names: Option<Vec<String>>,
}

impl Default for LegaiaMinigames {
    fn default() -> Self {
        Self::new()
    }
}

/// Read one overlay's **as-loaded** image out of the PROT bytes, exactly as the
/// SCUS loader would: the entry's full on-disc footprint (the web `parse_prot_toc`
/// already honours the extended window), lifted through the static-overlay map so
/// an LZS-form overlay decompresses to its runtime size.
fn overlay_image(prot: &[u8], entries: &[disc::EntryMeta], prot_index: u32) -> Option<Vec<u8>> {
    let rec = static_overlay::overlay_map().by_prot_index(prot_index)?;
    let meta = entries.iter().find(|e| e.index == prot_index)?;
    let off = meta.byte_offset as usize;
    let end = off.checked_add(meta.size_bytes as usize)?;
    let raw = prot.get(off..end.min(prot.len()))?;
    static_overlay::as_loaded(raw, rec).ok()
}

/// Read one PROT entry's **raw** on-disc bytes (no static-overlay lift - this is
/// for plain data entries like the art packs, not code overlays).
fn entry_bytes<'a>(
    prot: &'a [u8],
    entries: &[disc::EntryMeta],
    prot_index: u32,
) -> Option<&'a [u8]> {
    let meta = entries.iter().find(|e| e.index == prot_index)?;
    let off = meta.byte_offset as usize;
    let end = off.checked_add(meta.size_bytes as usize)?;
    prot.get(off..end.min(prot.len()))
}

/// JSON-escape a string for the hand-rolled object writers below.
fn jstr(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

#[wasm_bindgen]
impl LegaiaMinigames {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        #[cfg(target_arch = "wasm32")]
        console_error_panic_hook::set_once();
        Self {
            prot: Vec::new(),
            entries: Vec::new(),
            dance: None,
            baka: None,
            baka_tables: None,
            slot: None,
            slot_payouts: None,
            slot_art: None,
            slot_hud: None,
            slot_sfx: None,
            baka_names: None,
        }
    }

    /// Load a full Mode2/2352 disc image (or a raw `PROT.DAT`), parse the TOC,
    /// and pre-decode every minigame table that resolves. Returns a JSON status
    /// object naming which games came up:
    ///
    /// ```json
    /// { "entries": 1290,
    ///   "dance":  { "ok": true, "rows": 3, "beats": 32 },
    ///   "baka":   { "ok": true, "fighters": 17 },
    ///   "slot":   { "ok": true, "payouts": [.., ..] } }
    /// ```
    ///
    /// A game whose overlay or table doesn't resolve reports `{"ok":false}` with
    /// a reason rather than throwing - a regional / modded disc can still play
    /// the others.
    pub fn load_disc(&mut self, bytes: Vec<u8>) -> Result<String, JsValue> {
        let prot = if disc::is_mode2_2352_disc(&bytes) {
            disc::extract_prot_dat(&bytes).ok_or_else(|| {
                JsValue::from_str("minigames: PROT.DAT not found in this disc image")
            })?
        } else {
            bytes
        };
        let entries = disc::parse_prot_toc(&prot)
            .ok_or_else(|| JsValue::from_str("minigames: PROT.DAT TOC parse failed"))?;
        #[cfg(target_arch = "wasm32")]
        console_log(&format!(
            "Minigames: PROT.DAT loaded ({} entries)",
            entries.len()
        ));
        self.prot = prot;
        self.entries = entries;
        self.dance = None;
        self.baka = None;
        self.slot = None;

        // --- dance step chart (PROT 0980) ---
        let dance_json = match self.dance_chart() {
            Some(c) => format!(
                r#"{{"ok":true,"rows":{},"beats":{}}}"#,
                c.rows.len(),
                legaia_asset::dance_chart::BEATS_PER_ROW
            ),
            None => format!(
                r#"{{"ok":false,"why":{}}}"#,
                jstr("dance overlay (PROT 0980) or its step chart did not decode")
            ),
        };

        // --- baka roster + action tables (PROT 0976) ---
        self.baka_tables = overlay_image(
            &self.prot,
            &self.entries,
            legaia_asset::baka_opponents::BAKA_OVERLAY_PROT_INDEX as u32,
        )
        .and_then(|img| {
            let opponents = legaia_asset::baka_opponents::parse(&img)?;
            let actions = legaia_asset::baka_opponents::parse_actions(&img)?;
            Some((opponents, actions))
        });
        // The roster carries each fighter's name in the 32 bytes ahead of the
        // stat block the table parser starts at.
        self.baka_names = overlay_image(
            &self.prot,
            &self.entries,
            legaia_asset::baka_opponents::BAKA_OVERLAY_PROT_INDEX as u32,
        )
        .and_then(|img| minigame_art::baka_roster_names(&img).ok());
        let baka_json = match &self.baka_tables {
            Some((o, _)) => format!(
                r#"{{"ok":true,"fighters":{},"named":{}}}"#,
                o.len(),
                self.baka_names.is_some()
            ),
            None => format!(
                r#"{{"ok":false,"why":{}}}"#,
                jstr("Baka Fighter overlay (PROT 0976) or its roster tables did not decode")
            ),
        };

        // --- slot payout table (PROT 0975) ---
        self.slot_payouts = overlay_image(
            &self.prot,
            &self.entries,
            legaia_asset::slot_payout::SLOT_OVERLAY_PROT_INDEX as u32,
        )
        .and_then(|img| legaia_asset::slot_payout::parse(&img));

        // --- slot art pack (PROT 1200) + HUD descriptors (raw PROT 0975) ---
        // The overlay init `FUN_801CEC94` loads the art from raw TOC 0x4B2; the
        // HUD widget table is rodata inside the overlay entry itself.
        self.slot_art = entry_bytes(
            &self.prot,
            &self.entries,
            minigame_art::SLOT_ART_PROT_INDEX as u32,
        )
        .and_then(|raw| minigame_art::parse_art_pack(raw).ok());
        self.slot_hud = entry_bytes(
            &self.prot,
            &self.entries,
            legaia_asset::slot_payout::SLOT_OVERLAY_PROT_INDEX as u32,
        )
        .and_then(|raw| minigame_art::parse_slot_hud(raw).ok());

        // --- slot SFX cue bank (descriptors PROT 1199 + samples PROT 1198) ---
        // The reel-stop click, payout tick and reach sting are all runtime-bank
        // cues (id >= 0x200), so they need the overlay's own efect.dat block.
        self.slot_sfx = match (
            entry_bytes(
                &self.prot,
                &self.entries,
                minigame_sfx::SLOT_SFX_BANK_PROT_INDEX as u32,
            ),
            entry_bytes(
                &self.prot,
                &self.entries,
                minigame_sfx::SLOT_SFX_VAB_PROT_INDEX as u32,
            ),
        ) {
            (Some(bank), Some(vab)) => SfxCueBank::new(bank, vab).ok(),
            _ => None,
        };

        let slot_json = match &self.slot_payouts {
            Some(t) => format!(
                r#"{{"ok":true,"payouts":[{}],"art":{},"sfx":{}}}"#,
                t.payouts
                    .iter()
                    .map(|p| p.to_string())
                    .collect::<Vec<_>>()
                    .join(","),
                self.slot_art.is_some() && self.slot_hud.is_some(),
                self.slot_sfx.as_ref().map(|b| b.cues().len()).unwrap_or(0),
            ),
            None => format!(
                r#"{{"ok":false,"why":{}}}"#,
                jstr("slot-machine overlay (PROT 0975) or its payout table did not decode")
            ),
        };

        Ok(format!(
            r#"{{"entries":{},"dance":{dance_json},"baka":{baka_json},"slot":{slot_json}}}"#,
            self.entries.len()
        ))
    }

    // ---------------------------------------------------------------- dance

    /// Start a dance run on the disc's baked step chart. `long_song` picks the
    /// long song-length limit. Returns `false` when the chart didn't decode.
    pub fn dance_start(&mut self, long_song: bool) -> bool {
        let Some(chart) = self.dance_chart() else {
            return false;
        };
        self.dance = Some(DanceGame::new(chart, long_song));
        true
    }

    /// Advance the beat clock by `frames` frames (the retail clock steps
    /// `frame_delta * 10` phase units per frame).
    pub fn dance_tick(&mut self, frames: u32) {
        if let Some(g) = self.dance.as_mut() {
            g.advance(frames);
        }
    }

    /// Judge a directional press. `dir` is the chart symbol (`1` / `2` / `3`).
    /// Returns `"miss"` / `"hit"` / `"sequence"` (`"none"` with no live run).
    pub fn dance_press(&mut self, dir: u8) -> String {
        let Some(g) = self.dance.as_mut() else {
            return "none".to_string();
        };
        let d = match dir {
            1 => DanceDir::A,
            2 => DanceDir::B,
            3 => DanceDir::C,
            _ => return "none".to_string(),
        };
        match g.judge_press(d) {
            Judge::Miss => "miss".to_string(),
            Judge::Hit { .. } => "hit".to_string(),
            Judge::Sequence { .. } => "sequence".to_string(),
        }
    }

    /// Live dance state.
    ///
    /// ```json
    /// { "live": true, "score": 0, "gauge": 0, "lane": 0, "beat": 3,
    ///   "phase": 40, "period": 281, "window": 210, "accuracy": 3200, "dead_zone": false,
    ///   "judged": 2, "displayed": 3, "song_timer": 900, "song_len": 16860,
    ///   "over": false, "passed": false }
    /// ```
    ///
    /// **`judged` is the step to press.** Retail splits the chart lookup
    /// (`FUN_801d1820`) into two halves: the hit judge (`FUN_801d1960`) matches
    /// a press against the raw chart cell (`judged`), while the display /
    /// auto-feed half substitutes the held-sequence symbol `3` on every 4th
    /// beat (`displayed`). Both are surfaced; only `judged` scores. `0` = the
    /// beat carries no step, `null` = the dead zone between beats.
    pub fn dance_state_json(&self) -> String {
        let Some(g) = self.dance.as_ref() else {
            return r#"{"live":false}"#.to_string();
        };
        let opt = |s: Option<u8>| match s {
            Some(v) => v.to_string(),
            None => "null".to_string(),
        };
        format!(
            concat!(
                r#"{{"live":true,"score":{},"gauge":{},"lane":{},"beat":{},"phase":{},"#,
                r#""period":{},"window":{},"accuracy":{},"dead_zone":{},"judged":{},"displayed":{},"#,
                r#""song_timer":{},"song_len":{},"over":{},"passed":{}}}"#
            ),
            g.score(),
            g.gauge(),
            g.lane(),
            g.beat_index(),
            g.intra_beat_phase(),
            legaia_engine_core::dance::BEAT_PERIOD,
            legaia_engine_core::dance::BEAT_WINDOW,
            g.accuracy_weight(),
            g.in_dead_zone(),
            opt(g.judged_symbol()),
            opt(g.required_symbol()),
            g.song_timer(),
            g.song_len(),
            g.song_over(),
            g.passed(),
        )
    }

    /// The whole decoded step chart, for the page's scrolling note lane:
    /// `{"rows":[[u8; 32], ...]}` (one row per difficulty lane).
    pub fn dance_chart_json(&self) -> String {
        let Some(c) = self.dance_chart() else {
            return r#"{"rows":[]}"#.to_string();
        };
        let rows = c
            .rows
            .iter()
            .map(|r| {
                let cells = r
                    .iter()
                    .map(|b| b.to_string())
                    .collect::<Vec<_>>()
                    .join(",");
                format!("[{cells}]")
            })
            .collect::<Vec<_>>()
            .join(",");
        format!(r#"{{"rows":[{rows}]}}"#)
    }

    // ---------------------------------------------------------- baka fighter

    /// The parsed roster, for the opponent picker. The disc carries no names for
    /// these fighters - only their numbers - so each row is the record's own
    /// stat block:
    ///
    /// ```json
    /// [ { "id": 1, "gold": 30, "damage_mod": 20, "crit_chance": 10,
    ///     "atk_tiers": [..], "def_tiers": [..], "pattern": [2,1,3],
    ///     "power": [..] }, ... ]
    /// ```
    pub fn baka_roster_json(&self) -> String {
        let Some((opponents, actions)) = self.baka_tables.as_ref() else {
            return "[]".to_string();
        };
        let rows = opponents
            .iter()
            .map(|o| {
                let list = |v: &[i32]| {
                    v.iter()
                        .map(|x| x.to_string())
                        .collect::<Vec<_>>()
                        .join(",")
                };
                let power = actions
                    .get(o.index)
                    .map(|a| {
                        (1..=4u8)
                            .map(|t| a.attack_power(t).unwrap_or(0).to_string())
                            .collect::<Vec<_>>()
                            .join(",")
                    })
                    .unwrap_or_default();
                format!(
                    concat!(
                        r#"{{"id":{},"gold":{},"damage_mod":{},"crit_chance":{},"#,
                        r#""atk_tiers":[{}],"def_tiers":[{}],"pattern":[{}],"power":[{}]}}"#
                    ),
                    o.index,
                    o.gold_reward,
                    o.damage_mod,
                    o.crit_chance,
                    list(&o.atk_tiers),
                    list(&o.def_tiers),
                    o.ai_pattern
                        .iter()
                        .map(|b| b.to_string())
                        .collect::<Vec<_>>()
                        .join(","),
                    power,
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        format!("[{rows}]")
    }

    /// Start a best-of-3 duel: the visitor fights as roster fighter 0 (the
    /// player-side default) against `opponent`. Returns `false` when the tables
    /// didn't decode or the roster id is out of range.
    pub fn baka_start(&mut self, opponent: usize, seed: u32) -> bool {
        let Some((opponents, actions)) = self.baka_tables.as_ref() else {
            return false;
        };
        match BakaFight::from_tables(opponents, actions, 0, opponent, seed) {
            Some(f) => {
                self.baka = Some(f);
                true
            }
            None => false,
        }
    }

    /// Advance the duel one frame's worth of `frame_step` (the retail SM's
    /// per-frame delta; `1` is a normal frame).
    pub fn baka_tick(&mut self, frame_step: i32) {
        if let Some(f) = self.baka.as_mut() {
            f.tick(frame_step);
        }
    }

    /// Commit the visitor's attack this exchange: `1`/`2`/`3` are the three
    /// rock-paper-scissors throws, `4` the special. Returns `false` when the
    /// fighter can't act yet (cooldown, or a choice is already pending).
    pub fn baka_choose(&mut self, attack: u8) -> bool {
        let Some(f) = self.baka.as_mut() else {
            return false;
        };
        let Some(a) = BakaAttack::from_type_id(attack) else {
            return false;
        };
        f.choose(0, a)
    }

    /// Live duel state.
    ///
    /// ```json
    /// { "live": true, "phase": "fighting"|"round_over"|"match_over",
    ///   "round": 0, "hp": [3200, 2900], "hp_start": 3200,
    ///   "wins": [0, 1], "combo": [0, 2], "chosen": [2, null],
    ///   "can_choose": true, "gold": 30, "winner": null,
    ///   "last": { "winner": 0, "draw": false, "damage": 512,
    ///             "critical": false, "special": false } }
    /// ```
    pub fn baka_state_json(&self) -> String {
        let Some(f) = self.baka.as_ref() else {
            return r#"{"live":false}"#.to_string();
        };
        let phase = match f.phase() {
            MatchPhase::Fighting => "fighting",
            MatchPhase::RoundOver(_) => "round_over",
            MatchPhase::MatchOver(_) => "match_over",
        };
        let chosen = |s: usize| match f.chosen(s) {
            Some(a) => a.type_id().to_string(),
            None => "null".to_string(),
        };
        let last = match f.last_exchange() {
            Some(e) => format!(
                r#"{{"winner":{},"draw":{},"damage":{},"critical":{},"special":{}}}"#,
                e.winner, e.draw, e.damage, e.critical, e.special_round_win
            ),
            None => "null".to_string(),
        };
        let winner = match f.winner() {
            Some(w) => w.to_string(),
            None => "null".to_string(),
        };
        format!(
            concat!(
                r#"{{"live":true,"phase":{},"round":{},"hp":[{},{}],"hp_start":{},"#,
                r#""wins":[{},{}],"combo":[{},{}],"chosen":[{},{}],"can_choose":{},"#,
                r#""gold":{},"winner":{},"last":{}}}"#
            ),
            jstr(phase),
            f.round(),
            f.hp(0),
            f.hp(1),
            legaia_engine_core::baka_fighter::HP_START,
            f.round_wins(0),
            f.round_wins(1),
            f.combo(0),
            f.combo(1),
            chosen(0),
            chosen(1),
            f.can_choose(0),
            f.gold_reward(),
            winner,
            last,
        )
    }

    // ----------------------------------------------------------------- slots

    /// Start a slot session on the disc's payout table with `balance` coins in
    /// the machine. Returns `false` when the payout table didn't decode.
    pub fn slot_start(&mut self, seed: u32, balance: i32) -> bool {
        let Some(payouts) = self.slot_payouts.clone() else {
            return false;
        };
        self.slot = Some(SlotMachine::new(payouts, seed, balance));
        true
    }

    /// Charge the bet and start a spin. `false` when the machine isn't idle or
    /// the balance is under the 3-coin gate.
    pub fn slot_spin(&mut self) -> bool {
        self.slot.as_mut().is_some_and(|m| m.spin())
    }

    /// Advance the reels one frame.
    pub fn slot_tick(&mut self) {
        if let Some(m) = self.slot.as_mut() {
            m.tick();
        }
    }

    /// Stop the leftmost still-spinning reel. `false` when stopping isn't
    /// allowed yet (the reels are still spinning up).
    pub fn slot_stop(&mut self) -> bool {
        self.slot.as_mut().is_some_and(|m| m.stop_next_reel())
    }

    /// Tally the latched payout into the balance and return to idle. Returns
    /// the credited coins.
    pub fn slot_collect(&mut self) -> i32 {
        self.slot.as_mut().map(|m| m.collect()).unwrap_or(0)
    }

    /// Live machine state. `window` is the 3x3 grid of symbol ids actually on
    /// screen (`window[reel][0..3]` = top / payline / bottom row), read off the
    /// live reel positions so the page can render a spinning machine.
    ///
    /// ```json
    /// { "live": true, "phase": "idle"|"spinning"|"stopping"|"payout"|"cashed_out",
    ///   "balance": 97, "cost": 3, "can_spin": true, "can_stop": false,
    ///   "stopped": 0, "feature_mode": 0, "bonus_spins": 0, "net_take": 6,
    ///   "window": [[4,7,1],[2,2,9],[0,3,3]],
    ///   "payouts": [..],
    ///   "last": { "line": 0, "symbol": 7, "payout": 30,
    ///             "bonus_triggered": false, "bonus_spin": false } }
    /// ```
    pub fn slot_state_json(&self) -> String {
        let Some(m) = self.slot.as_ref() else {
            return r#"{"live":false}"#.to_string();
        };
        let phase = match m.phase() {
            SlotPhase::Idle => "idle",
            SlotPhase::Spinning => "spinning",
            SlotPhase::Stopping => "stopping",
            SlotPhase::Payout => "payout",
            SlotPhase::CashedOut => "cashed_out",
        };
        // The visible 3x3: each reel's payline row plus the row above / below,
        // which are exactly the rows the win eval reads for the three paylines.
        let strips = m.strips();
        let len = legaia_engine_core::slot_machine::STRIP_LEN as isize;
        let window = (0..legaia_engine_core::slot_machine::REEL_COUNT)
            .map(|r| {
                let row = m.payline_row(r) as isize;
                let cell = |off: isize| strips[r][(row + off).rem_euclid(len) as usize].to_string();
                format!("[{},{},{}]", cell(-1), cell(0), cell(1))
            })
            .collect::<Vec<_>>()
            .join(",");
        let last = match m.last_result() {
            Some(r) => format!(
                r#"{{"line":{},"symbol":{},"payout":{},"bonus_triggered":{},"bonus_spin":{}}}"#,
                r.line.map(|l| l.to_string()).unwrap_or("null".into()),
                r.symbol.map(|s| s.to_string()).unwrap_or("null".into()),
                r.payout,
                r.bonus_triggered,
                r.bonus_spin,
            ),
            None => "null".to_string(),
        };
        let payouts = self
            .slot_payouts
            .as_ref()
            .map(|t| {
                t.payouts
                    .iter()
                    .map(|p| p.to_string())
                    .collect::<Vec<_>>()
                    .join(",")
            })
            .unwrap_or_default();
        format!(
            concat!(
                r#"{{"live":true,"phase":{},"balance":{},"cost":{},"can_spin":{},"#,
                r#""can_stop":{},"stopped":{},"feature_mode":{},"bonus_spins":{},"#,
                r#""net_take":{},"window":[{}],"payouts":[{}],"last":{}}}"#
            ),
            jstr(phase),
            m.balance(),
            m.spin_cost(),
            m.can_spin(),
            m.can_stop(),
            m.reels_stopped(),
            m.feature_mode(),
            m.bonus_spins(),
            m.net_take(),
            window,
            payouts,
            last,
        )
    }

    // ------------------------------------------------------------- slot art
    //
    // Everything below decodes the *retail* slot machine's own textures out of
    // the visitor's disc (PROT entry 1200, the pack `FUN_801CEC94` loads). No
    // pixel ships with the site.

    /// Whether the slot machine's art pack decoded off this disc. When `false`
    /// the page must fall back to symbol *ids*, not to invented artwork.
    pub fn slot_art_ready(&self) -> bool {
        self.slot_art.is_some()
    }

    /// One reel symbol (`0..=9`) as a 64x64 RGBA8 buffer, at the exact cell and
    /// **per-symbol CLUT** the retail reel renderer `FUN_801d0fa8` samples
    /// (`U = (sym & 3) * 0x40`, `V = (sym & 0xC) * 0x10`, CLUT `0x7A80 + sym`).
    ///
    /// The palette is load-bearing: symbols 0/1/2 are one piece of artwork
    /// recoloured three ways, and so are 4/5. Empty when the art didn't decode.
    pub fn slot_symbol_rgba(&self, sym: usize) -> Vec<u8> {
        self.slot_art
            .as_ref()
            .and_then(|art| minigame_art::slot_symbol(art, sym).ok())
            .map(|s| s.rgba)
            .unwrap_or_default()
    }

    /// The coin readout's font strip - the `"COIN"` label (`x = 0..64`) followed
    /// by digits `0..=9` at `x = 64 + d * 16` - as a 224x16 RGBA8 buffer
    /// (`FUN_801d2914`, CLUT `0x7A8D`).
    pub fn slot_digits_rgba(&self) -> Vec<u8> {
        self.slot_art
            .as_ref()
            .and_then(|art| minigame_art::slot_digit_strip(art).ok())
            .map(|s| s.rgba)
            .unwrap_or_default()
    }

    /// One of the 3 HUD widgets the retail rasteriser `FUN_801d2cc0` draws from
    /// the descriptor table `DAT_801d347c`, decoded through *its own* texpage +
    /// CLUT: `0` = the cabinet panel, `1` = the "COIN" label, `2` = the cash-out
    /// cursor. RGBA8; pair with [`Self::slot_hud_json`] for the dimensions.
    pub fn slot_hud_rgba(&self, index: usize) -> Vec<u8> {
        let (Some(art), Some(hud)) = (self.slot_art.as_ref(), self.slot_hud.as_ref()) else {
            return Vec::new();
        };
        hud.get(index)
            .and_then(|w| minigame_art::slot_hud_sprite(art, w).ok())
            .map(|s| s.rgba)
            .unwrap_or_default()
    }

    /// The 3 HUD widget descriptors, as parsed off the disc:
    ///
    /// ```json
    /// [ { "u": 0, "v": 16, "w": 127, "h": 239,
    ///     "page": 4, "palette": 0, "texpage": [640, 0], "clut": [0, 494] }, ... ]
    /// ```
    ///
    /// `page` is the index into the art pack the record's texpage resolves to,
    /// and `palette` the CLUT column - so a caller can re-decode the same traced
    /// rect through a different palette. That is not academic: the retail
    /// rasteriser `FUN_801d2cc0` lets the *call site* override the record's CLUT
    /// (the id's high field swaps in `0x7D0F`), so a widget's on-screen colour
    /// is not always the one its descriptor names.
    pub fn slot_hud_json(&self) -> String {
        let (Some(hud), Some(art)) = (self.slot_hud.as_ref(), self.slot_art.as_ref()) else {
            return "[]".to_string();
        };
        let rows = hud
            .iter()
            .map(|w| {
                let page = art
                    .iter()
                    .position(|t| t.image.fb_x == w.texpage.x() && t.image.fb_y == w.texpage.y())
                    .map(|p| p.to_string())
                    .unwrap_or("null".into());
                format!(
                    concat!(
                        r#"{{"u":{},"v":{},"w":{},"h":{},"page":{},"palette":{},"#,
                        r#""texpage":[{},{}],"clut":[{},{}]}}"#
                    ),
                    w.u,
                    w.v,
                    w.w,
                    w.h,
                    page,
                    w.clut.palette_index(),
                    w.texpage.x(),
                    w.texpage.y(),
                    w.clut.x(),
                    w.clut.y(),
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        format!("[{rows}]")
    }

    /// A whole 256x256 art page decoded through one of its 16 palettes, as
    /// RGBA8. The escape hatch for the machine's *chrome* - the marquee panel,
    /// the mascot heads, the reel-stop button medallions - whose on-screen rects
    /// this port crops from the decoded page rather than tracing to an emitter
    /// (unlike the reel symbols / digits / cabinet / cursor above, which are
    /// traced). Page indices follow the pack order documented on
    /// [`legaia_asset::minigame_art`]: `2` = chrome, `3` = banner text,
    /// `4` = cabinet.
    pub fn slot_page_rgba(&self, page: usize, palette: usize) -> Vec<u8> {
        self.slot_art
            .as_ref()
            .and_then(|art| minigame_art::slot_page(art, page, palette).ok())
            .map(|s| s.rgba)
            .unwrap_or_default()
    }

    // ------------------------------------------------------------- slot sound
    //
    // The machine's own cues, decoded off the disc: the reel-stop click, the
    // payout tick, the reach sting. These are runtime-bank ids (>= 0x200), so
    // they resolve through the slot overlay's `efect.dat` descriptor block
    // (PROT 1199) into the VAB it loads alongside it (PROT 1198). Nothing here
    // is a substitute sound - if a cue does not resolve, the page stays silent.

    /// The cue ids this disc's slot bank actually defines, with the VAB voice
    /// each one keys:
    ///
    /// ```json
    /// [ { "id": 522, "program": 1, "tone": 6, "note": 66, "rate": 46616 }, ... ]
    /// ```
    ///
    /// `id` is decimal (`522` = `0x20A`, the reel-stop click).
    pub fn slot_sfx_json(&self) -> String {
        let Some(bank) = self.slot_sfx.as_ref() else {
            return "[]".to_string();
        };
        let rows = bank
            .cues()
            .iter()
            .map(|c| {
                let rate = bank.decode(c.id).map(|(_, r)| r).unwrap_or(0);
                format!(
                    r#"{{"id":{},"program":{},"tone":{},"note":{},"rate":{}}}"#,
                    c.id, c.program, c.tone, c.note, rate
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        format!("[{rows}]")
    }

    /// Decode one cue to mono PCM (`i16`). Empty when the cue isn't in the bank.
    pub fn slot_sfx_pcm(&self, cue: u16) -> Vec<i16> {
        self.slot_sfx
            .as_ref()
            .and_then(|b| b.decode(cue).ok())
            .map(|(pcm, _)| pcm)
            .unwrap_or_default()
    }

    /// The rate [`Self::slot_sfx_pcm`]'s samples must be played back at - the
    /// cue's note against the VAG's own centre note *is* the pitch, so this
    /// carries it. `0` when the cue isn't in the bank.
    pub fn slot_sfx_rate(&self, cue: u16) -> u32 {
        self.slot_sfx
            .as_ref()
            .and_then(|b| b.decode(cue).ok())
            .map(|(_, rate)| rate)
            .unwrap_or(0)
    }

    /// The retail cue ids, so the page never has to hard-code a number:
    /// `{"reel_stop":522,"payout_tick":521,"reach":512,"reach1":513,"reach2":514}`.
    pub fn slot_sfx_cue_ids(&self) -> String {
        format!(
            r#"{{"reel_stop":{},"payout_tick":{},"reach":{},"reach1":{},"reach2":{}}}"#,
            minigame_sfx::CUE_SLOT_REEL_STOP,
            minigame_sfx::CUE_SLOT_PAYOUT_TICK,
            minigame_sfx::CUE_SLOT_REACH,
            minigame_sfx::CUE_SLOT_REACH_1,
            minigame_sfx::CUE_SLOT_REACH_2,
        )
    }

    // ------------------------------------------------------- baka: names + ladder

    /// The 17 fighter names, in roster order, read out of the roster records
    /// (`+0x00`, 32-byte ASCII). Empty when the overlay didn't decode.
    pub fn baka_names_json(&self) -> String {
        let Some(names) = self.baka_names.as_ref() else {
            return "[]".to_string();
        };
        format!(
            "[{}]",
            names.iter().map(|n| jstr(n)).collect::<Vec<_>>().join(",")
        )
    }

    /// The ladder the cabinet actually serves, as `[{stage, roster}]`.
    ///
    /// The stage counter starts at **2** and `roster = stage + 3`, so the first
    /// lap is roster ids `5..=16` - across which the prize gold is strictly
    /// monotonic. Roster `3` and `4` are only reachable after the all-clear
    /// wraps the counter, which is why the roster's gold column looks out of
    /// order if you read it straight down.
    pub fn baka_ladder_json(&self) -> String {
        let rows = minigame_art::baka_ladder()
            .into_iter()
            .map(|(stage, roster)| format!(r#"{{"stage":{stage},"roster":{roster}}}"#))
            .collect::<Vec<_>>()
            .join(",");
        format!("[{rows}]")
    }
}

impl LegaiaMinigames {
    /// Decode the dance step chart out of the dance overlay (PROT 0980).
    fn dance_chart(&self) -> Option<legaia_asset::dance_chart::DanceChart> {
        let img = overlay_image(
            &self.prot,
            &self.entries,
            legaia_asset::dance_chart::DANCE_OVERLAY_PROT_INDEX as u32,
        )?;
        legaia_asset::dance_chart::parse(&img)
    }
}
