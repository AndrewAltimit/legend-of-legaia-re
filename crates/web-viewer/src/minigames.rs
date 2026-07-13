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

use legaia_asset::static_overlay;
use legaia_engine_core::baka_fighter::{BakaAttack, BakaFight, MatchPhase};
use legaia_engine_core::dance::{DanceDir, DanceGame, Judge};
use legaia_engine_core::slot_machine::{SlotMachine, SlotPhase};

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
        let baka_json = match &self.baka_tables {
            Some((o, _)) => format!(r#"{{"ok":true,"fighters":{}}}"#, o.len()),
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
        let slot_json = match &self.slot_payouts {
            Some(t) => format!(
                r#"{{"ok":true,"payouts":[{}]}}"#,
                t.payouts
                    .iter()
                    .map(|p| p.to_string())
                    .collect::<Vec<_>>()
                    .join(",")
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
