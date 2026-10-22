//! The slot machine rules-engine shell: start, step, the composed frame and
//! the state / bonus / marquee JSON.

use super::*;

#[wasm_bindgen]
impl LegaiaMinigames {
    // ----------------------------------------------------------------- slots

    /// Start a slot session on the disc's payout table with `balance` coins in
    /// the machine, on the overlay's own literal seed
    /// ([`legaia_engine_minigames::slot_machine::SLOT_RNG_SEED`]) - the one the play
    /// hosts rack every cabinet with. Returns `false` when the payout table
    /// didn't decode.
    pub fn slot_start(&mut self, balance: i32) -> bool {
        let seed = legaia_engine_minigames::slot_machine::SLOT_RNG_SEED;
        let Some(payouts) = self.slot_payouts.clone() else {
            return false;
        };
        let paylines = self
            .slot_scene
            .as_ref()
            .map(|s| s.paylines.clone())
            .unwrap_or_default();
        self.slot = Some(SlotMachine::new(payouts, seed, balance).with_paylines(paylines));
        self.slot_clock = Default::default();
        true
    }

    /// One frame of the machine on this frame's packed retail pad-edge word
    /// (`0x40` Cross, `0x80` Square, `0x20` Circle, `0x10` Triangle, `0x100`
    /// Select, `0x1000` / `0x4000` Up / Down, `0x04` / `0x01` L1 / L2): the
    /// shared [`SlotMachine::frame`] the play hosts' world tick runs. Cross
    /// spins at idle, Square / Cross / Circle stop reels 0 / 1 / 2, Cross
    /// finishes a win's timed tally at once, Triangle / Select open the
    /// cash-out flow.
    ///
    /// This page used to run its own path: a resolved spin banked whole on
    /// the frame it evaluated (retail counts it in over state 4's timed
    /// tally, `11` or `1` coins every other frame), and one key stopped the
    /// reels left to right. Both were rules the cabinet does not have; the
    /// page's one-key convenience is now an input macro in its script that
    /// picks which cabinet button to press.
    ///
    /// Returns the coins credited this frame - one tally transfer, or the
    /// rest of the count on a Cross press.
    /// The machine's static-table cues (`< 0x200`) are keyed here; its
    /// runtime-bank cues are this page's own `slotPlay` path, drained and
    /// dropped.
    pub fn slot_step(&mut self, packed: u32) -> i32 {
        let Some(m) = self.slot.as_mut() else {
            return 0;
        };
        let was_payout = m.phase() == SlotPhase::Payout;
        let owed = m.payout_left();
        let _ = m.frame(packed);
        let credited = if was_payout {
            owed - m.payout_left()
        } else {
            0
        };
        let cues: Vec<i16> = m
            .take_sounds()
            .ring
            .into_iter()
            .map(|(_, id)| id)
            .filter(|&id| (0..0x200).contains(&id))
            .collect();
        let frame = m.marquee();
        let reach = m.anticipation();
        if let Some(c) = self.slot_cabinet.as_ref() {
            self.slot_dots = self.slot_clock.frame(&frame, reach, &c.scene.messages);
        }
        for id in cues {
            self.minigame_sfx_cue(id as u16);
        }
        credited
    }

    /// Whether the machine's resident set decoded, so the page draws the
    /// machine as [`Self::slot_frame_rgba`] - the play hosts' cabinet scene,
    /// rasterised - rather than its own canvas composition.
    pub fn slot_cabinet_ready(&self) -> bool {
        self.slot_cabinet.is_some()
    }

    /// The whole machine for this frame as a `w x h` RGBA8 image: the shared
    /// `slot_cabinet` builder (cabinet, reels, furniture, dot matrix, coin
    /// HUD) plus the `slot_paylines` quads, rasterised on the CPU over the
    /// art pack's VRAM (`screen_prim_raster`) - the same primitive list the
    /// native window and the play page draw on their GPUs. Empty without a
    /// session or the resident set.
    pub fn slot_frame_rgba(&self, w: u32, h: u32) -> Vec<u8> {
        use legaia_engine_ui::{slot_cabinet as usc, slot_paylines as usp};
        let (Some(m), Some(c)) = (self.slot.as_ref(), self.slot_cabinet.as_ref()) else {
            return Vec::new();
        };
        // The cash-out flow over the machine - or, on a rules page, in its
        // place - and the rules text, as the play hosts compose them.
        let menu = usc::slot_menu_prims(c, m.screen(), m.fade_level());
        let text = match (self.slot_font.as_ref(), c.rules.as_ref()) {
            (Some(f), Some(r)) => usc::slot_rules_text_draws_for(f, r, m.screen(), m.fade_level()),
            _ => Vec::new(),
        };
        if matches!(m.screen(), slot_scene::SlotScreen::Instructions { .. }) {
            let mut px = legaia_render_kernels::screen_prim_raster::rasterize_rgba(
                &menu,
                c.vram.as_u16(),
                w,
                h,
            );
            if let Some(f) = self.slot_font.as_ref() {
                blit_text(&mut px, w, h, f, &text);
            }
            return px;
        }
        let strips = m.strips();
        let clear;
        let dots: &[u8] = if self.slot_dots.is_empty() {
            clear = slot_scene::clear_dots();
            &clear
        } else {
            &self.slot_dots
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
            blink: self.slot_clock.blink,
            balance: m.balance(),
        });
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
        legaia_render_kernels::screen_prim_raster::rasterize_rgba(&prims, c.vram.as_u16(), w, h)
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
            SlotPhase::Menu => "menu",
            SlotPhase::NoCoins => "no_coins",
            SlotPhase::Leaving => "leaving",
            SlotPhase::CashedOut => "cashed_out",
        };
        // The visible 3x3: each reel's payline row plus the row above / below,
        // which are exactly the rows the win eval reads for the three paylines.
        let strips = m.strips();
        let len = legaia_engine_minigames::slot_machine::STRIP_LEN as isize;
        let window = (0..legaia_engine_minigames::slot_machine::REEL_COUNT)
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
                r#""can_stop":{},"stop_open":[{},{},{}],"stopped":{},"feature_mode":{},"bonus_spins":{},"#,
                r#""net_take":{},"window":[{}],"payouts":[{}],"last":{}}}"#
            ),
            jstr(phase),
            m.balance(),
            m.spin_cost(),
            m.can_spin(),
            m.can_stop(),
            m.can_stop() && m.reel_stop_open(0),
            m.can_stop() && m.reel_stop_open(1),
            m.can_stop() && m.reel_stop_open(2),
            m.reels_stopped(),
            m.feature_mode(),
            m.bonus_spins(),
            m.net_take(),
            window,
            payouts,
            last,
        )
    }

    /// The **bonus game**: the two jackpot triggers, and - when a round is live -
    /// the numbers on the reels and the **claimed-column tally** the machine
    /// prints across its marquee.
    ///
    /// A matching line of the **blue "kick"** symbol (id 8) earns 1 bonus round;
    /// the **red "punch"** symbol (id 9) earns 3 - the counts and symbol ids are
    /// pinned in the disassembly (`FUN_801d13e8`) and the colours in the PROT
    /// 1200 reel art. A bonus round swaps the reels onto the machine's *second*
    /// strip - the numerals `1..=10`, their own artwork on art page 1 - and pays
    /// the **product of the three numbers you stop on** (`1..=1000`).
    ///
    /// ```json
    /// { "kick_symbol": 8, "kick_rounds": 1, "punch_symbol": 9, "punch_rounds": 3,
    ///   "min": 1, "max": 1000, "active": true, "rounds_left": 2,
    ///   "numbers": [9, 5, 3], "tally": [9, 5, 0], "claimed": [true, true, false],
    ///   "complete": false, "product": 0 }
    /// ```
    ///
    /// * `numbers` - the number **live on each reel's payline** right now, so the
    ///   page can draw the wheels while they spin.
    /// * `tally` - the machine's own claimed-column latch (`DAT_801d3d20`): `0`
    ///   for a column whose reel is still spinning, its landed number once that
    ///   stop is taken. This is the `0 x 0 x 0` -> `9 x 5 x 0` strip.
    /// * `product` - the tally's product, i.e. the coins the round pays; `0`
    ///   until all three columns are claimed (`complete`).
    ///
    /// The tally and the payout are **the same state**, not two copies: the
    /// evaluator multiplies the very rows the tally latched. A page that renders
    /// `tally` cannot show a line that disagrees with what the spin paid.
    pub fn slot_bonus_json(&self) -> String {
        use legaia_engine_minigames::slot_machine::REEL_COUNT;
        use legaia_minigame_formats::slot_payout as sp;
        let head = format!(
            r#""kick_symbol":{},"kick_rounds":{},"punch_symbol":{},"punch_rounds":{},"min":{},"max":{}"#,
            sp::KICK_SYMBOL_ID,
            sp::KICK_BONUS_ROUNDS,
            sp::PUNCH_SYMBOL_ID,
            sp::PUNCH_BONUS_ROUNDS,
            sp::BONUS_PAYOUT_MIN,
            sp::BONUS_PAYOUT_MAX,
        );
        let Some(m) = self.slot.as_ref() else {
            return format!(
                r#"{{{head},"active":false,"rounds_left":0,"numbers":[],"tally":[],"claimed":[],"complete":false,"product":0}}"#
            );
        };
        let join = |v: &[String]| v.join(",");
        // The live payline numbers - the display strip's value biased by 0xF,
        // which is the same byte the reel renderer draws the numeral from.
        let numbers = (0..REEL_COUNT)
            .map(|r| sp::bonus_number_for_value(m.payline_symbol(r)).to_string())
            .collect::<Vec<_>>();
        let tally = m.tally();
        let claimed = (0..REEL_COUNT)
            .map(|r| (m.claimed(r) > sp::BONUS_VALUE_BIAS as i32).to_string())
            .collect::<Vec<_>>();
        let tally_s = tally.iter().map(|n| n.to_string()).collect::<Vec<_>>();
        format!(
            concat!(
                r#"{{{head},"active":{active},"rounds_left":{rounds},"numbers":[{numbers}],"#,
                r#""tally":[{tally}],"claimed":[{claimed}],"complete":{complete},"product":{product}}}"#
            ),
            head = head,
            active = m.in_bonus_round(),
            rounds = m.bonus_spins().max(0),
            numbers = join(&numbers),
            tally = join(&tally_s),
            claimed = join(&claimed),
            complete = m.tally_complete(),
            product = m.tally_product(),
        )
    }

    /// The **marquee message bank's roles** - which of the 21 dot-matrix bitmaps
    /// in [`Self::slot_scene_json`] is which glyph, and the dot columns the
    /// machine blits them at.
    ///
    /// The tally strip and the payout caption are not chrome the page invents:
    /// they are `FUN_801cfff0` composing the *same* 78x13 dot matrix that
    /// scrolls the attract legend in the normal game. This hands over the ids and
    /// columns it uses, so the page draws the retail glyphs at the retail
    /// positions rather than a font of its own.
    ///
    /// ```json
    /// { "number_base": 6, "number_max": 10, "times": 17, "coins": 20,
    ///   "pip_on": 18, "pip_off": 19,
    ///   "tally_cols": [0, 32, 64], "times_cols": [16, 48], "pip_cols": [0, 32, 64],
    ///   "payout_digit_cols": [0, 13, 26, 39], "payout_coins_col": 52,
    ///   "payout_slide_rows": 13 }
    /// ```
    ///
    /// `number_base + n` is the bitmap for the numeral `n`, `0..=10` - eleven
    /// records, because a bonus reel can land on **10** and retail gives it a
    /// glyph of its own rather than two digit cells.
    pub fn slot_marquee_json(&self) -> String {
        let cols = |c: &[usize]| {
            c.iter()
                .map(|v| v.to_string())
                .collect::<Vec<_>>()
                .join(",")
        };
        format!(
            concat!(
                r#"{{"number_base":{nb},"number_max":{nm},"times":{times},"coins":{coins},"#,
                r#""pip_on":{pon},"pip_off":{poff},"tally_cols":[{tc}],"times_cols":[{xc}],"#,
                r#""pip_cols":[{pc}],"payout_digit_cols":[{pdc}],"payout_coins_col":{pcc},"#,
                r#""payout_slide_rows":{psr}}}"#
            ),
            nb = slot_scene::MSG_NUMBER_BASE,
            nm = slot_scene::MSG_NUMBER_MAX,
            times = slot_scene::MSG_TIMES,
            coins = slot_scene::MSG_COINS,
            pon = slot_scene::MSG_ROUND_PIP_ON,
            poff = slot_scene::MSG_ROUND_PIP_OFF,
            tc = cols(&slot_scene::TALLY_NUMBER_COLS),
            xc = cols(&slot_scene::TALLY_TIMES_COLS),
            pc = cols(&slot_scene::ROUND_PIP_COLS),
            pdc = cols(&slot_scene::PAYOUT_DIGIT_COLS),
            pcc = slot_scene::PAYOUT_COINS_COL,
            psr = slot_scene::PAYOUT_SLIDE_ROWS,
        )
    }
}
