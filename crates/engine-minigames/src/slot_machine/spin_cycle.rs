//! The spin cycle on [`SlotMachine`]: charge and spin-up, the per-frame reel tick, reel stops, win evaluation, payout tally and marquee.

use super::*;
use legaia_minigame_formats::minigame_slot_scene::{
    LANDING_LINE_BY_JITTER, MarqueeFrame, MarqueePlacement, PAYLINE_ROW_OFFSETS,
    compose_marquee_frame,
};
use legaia_minigame_formats::slot_payout;

impl SlotMachine {
    /// Charge the bet and start a spin (state `1` → `2`): subtract the flat
    /// spin cost (3 coins, or 1 during feature modes 4..=6 - a bonus "free
    /// spin" still costs 1), accrue the net take (`+6` / `+1`), run the
    /// per-spin feature roll, ramp the reels, and arm the spin timer.
    /// Returns `false` (no-op) when not idle or the balance is under the
    /// 3-coin gate.
    // PORT: FUN_801cf0d8 states 1-2 (bet charge + spin-up)
    pub fn spin(&mut self) -> bool {
        if !self.can_spin() {
            return false;
        }
        let feature_spin = (4..=6).contains(&self.feature_mode);
        self.balance = (self.balance - self.spin_cost()).max(0);
        self.net_take += if feature_spin {
            NET_TAKE_FEATURE_SPIN
        } else {
            NET_TAKE_NORMAL_SPIN
        };
        let roll = feature_roll(
            &mut self.rand,
            self.net_take,
            self.feature_mode,
            self.spin_up_pressed,
        );
        // State 1 clears the latch the moment the roll has read it
        // (`sw zero,0x3790(v0)` at `0x801CF56C`, right after
        // `jal 0x801D258C`): one spin-up press widens exactly one roll.
        self.spin_up_pressed = false;
        self.jitter = roll.jitter;
        self.normal_target = roll.normal_target;
        if let Some(mode) = roll.entered_mode {
            self.feature_mode = mode;
        }
        self.stopped = [None; REEL_COUNT];
        // The claimed values are cleared with the reel flags at the bet charge,
        // which is what resets the marquee tally to `0 x 0 x 0`.
        self.claimed = [0; REEL_COUNT];
        self.reel_vel = SPIN_VELOCITY;
        // A bonus spin (and the first spin after one) runs long, so the display
        // strip has room to rotate onto the other source strip before the
        // payline row comes round - see BONUS_SPIN_UP_FRAMES.
        self.spin_timer = self.next_spin_up_frames();
        self.bonus_just_ended = false;
        self.last_result = None;
        // State 2 clears the latch and its one-shot guard on entry
        // (`0x801CF600` / `0x801CF608`).
        self.anticipation = 0;
        self.anticipation_sounded = false;
        self.win_line = -1;
        self.payout_left = 0;
        self.phase = SlotPhase::Spinning;
        // The bet charge keys the reel motor and, in the two reach modes the
        // roll just entered, stores their sting into ring slot 2.
        self.sounds.voice_on.push(spin_voice(0));
        match self.feature_mode {
            1 => self.sounds.ring.push((2, CUE_FEATURE_1)),
            2 => self.sounds.ring.push((2, CUE_FEATURE_2)),
            _ => {}
        }
        true
    }

    /// The spin-up's input latch: a face-button edge (`_DAT_8007B874 & 0xF0`)
    /// on a spin-up frame whose timer has not run out raises the latch
    /// `DAT_801D3790`, which widens every feature-entry denominator of the
    /// **next** spin's roll (`FUN_801D258C`) by `rand % 100 + 200` - so a
    /// player who mashes buttons while the reels spin up makes the next
    /// spin's reach / hot modes rarer (`1/700` becomes `1/900..=1/999`).
    ///
    /// Call once per frame after [`Self::tick`] with this frame's edge; a no-op
    /// outside [`SlotPhase::Spinning`] - retail's test lives in state `2`
    /// alone. One frame differs: retail tests after the decrement that ends
    /// the spin-up, so an edge on that last frame still latches; the port's
    /// tick has already left `Spinning` by then.
    ///
    /// PORT: FUN_801cf0d8 (`0x801CF6D4`..`0x801CF704`, state 2's latch)
    pub fn latch_spin_up(&mut self, face_edge: bool) {
        if face_edge && self.phase == SlotPhase::Spinning && self.spin_timer != 0 {
            self.spin_up_pressed = true;
        }
    }

    /// Whether the spin-up press latch (`DAT_801D3790`) is up for the next
    /// spin.
    pub fn spin_up_pressed(&self) -> bool {
        self.spin_up_pressed
    }

    /// The source strip the display strip is currently being refilled from: the
    /// bonus numerals during a bonus round, the reel symbols otherwise
    /// (`FUN_801cf0d8` render tail: `DAT_801d3cac == 6 ? DAT_801d3fd0 :
    /// DAT_801d3e90`).
    fn active_source(&self) -> &[[u8; STRIP_LEN]; REEL_COUNT] {
        if self.in_bonus_round() {
            &self.bonus_strips
        } else {
            &self.symbol_strips
        }
    }

    /// Advance one frame: reels advance by their velocities (wrapping mod
    /// `0x1400`), the spin timer counts down into the stopping state, and each
    /// reel copies **one row** of the display strip from the active source.
    ///
    /// That one-row copy is the whole reel-swap mechanism. Retail never rewrites
    /// a strip wholesale: the render tail of `FUN_801cf0d8` refills the row
    /// [`DISPLAY_REFRESH_LEAD`] ahead of the payline, every frame, from whichever
    /// source strip the feature mode names - so when a bonus round opens, the
    /// numbers *rotate into* the reels from off-screen as they turn, and rotate
    /// back out again when it ends. Nothing is swapped in one go, and a stopped
    /// reel keeps its row (the refill runs ahead of the payline, never on it).
    // PORT: FUN_801cf0d8 tail (reel advance + display-strip row refill) + state 2
    pub fn tick(&mut self) {
        // The caption's slide-in clock. It only runs while a caption is up, and
        // the composer reads `min(frame - PAYOUT_SLIDE_ROWS, 0)` off it, so
        // without this advance the caption would sit one row short forever.
        if self.phase == SlotPhase::Payout {
            self.tick_payout();
        }
        for reel in 0..REEL_COUNT {
            if self.stopped[reel].is_none() {
                self.reel_pos[reel] =
                    (self.reel_pos[reel] + self.reel_vel[reel]).rem_euclid(REEL_WRAP);
            }
        }
        if self.phase == SlotPhase::Spinning {
            self.spin_timer -= 1;
            if self.spin_timer <= 0 {
                self.phase = SlotPhase::Stopping;
            }
        }
        // State 3 zeroes the latch every frame (the store sits in the delay
        // slot of the Stop-0 test at `0x801CF71C`, so it runs whatever the
        // pad says), then re-runs the scanner while exactly two stops are in
        // (the `DAT_801d3d2c == 2` test at `0x801CF7EC`).
        if self.phase == SlotPhase::Stopping {
            self.anticipation = if self.reels_stopped() == 2 {
                self.anticipation_scan()
            } else {
                0
            };
            // The first sighting per spin stings (ring slot 2) and swaps the
            // motor voice onto the reach loop, behind `DAT_801d3ca8`.
            if self.anticipation != 0 && !self.anticipation_sounded {
                self.anticipation_sounded = true;
                self.sounds.ring.push((2, CUE_REACH));
                self.sounds.voice_on.push(spin_voice(1));
            }
        }
        // The display-strip refill, after the advance - as in retail's tail.
        let source = *self.active_source();
        let rows: [usize; REEL_COUNT] =
            core::array::from_fn(|r| (self.payline_row(r) + DISPLAY_REFRESH_LEAD) % STRIP_LEN);
        for ((display, src), &row) in self.strips.iter_mut().zip(source.iter()).zip(rows.iter()) {
            display[row] = src[row];
        }
        self.frame = self.frame.wrapping_add(1);
    }

    /// State 4, one frame (`0x801CF888..0x801CF940`): advance the caption
    /// timer; while coins are owed, transfer a chunk into the balance on
    /// every odd frame - `11` while more than `20` remain, else `1` - with
    /// one tally tick into ring slot 0 per transfer; once nothing is owed
    /// and the timer reaches [`PAYOUT_HOLD_FRAMES`], drop back to idle with
    /// the winning line cleared.
    // PORT: FUN_801cf0d8 state 4 (the timed tally)
    fn tick_payout(&mut self) {
        self.caption_frame += 1;
        if self.payout_left > 0 {
            if self.frame & 1 == 0 {
                return;
            }
            let chunk = if self.payout_left > 20 { 11 } else { 1 };
            self.payout_left -= chunk;
            self.balance = (self.balance + chunk).min(BALANCE_CAP);
            self.sounds.ring.push((0, CUE_PAYOUT_TICK));
        } else if self.caption_frame >= PAYOUT_HOLD_FRAMES {
            self.phase = SlotPhase::Idle;
            self.win_line = -1;
            self.caption_frame = 0;
        }
    }

    /// Stop reel `reel` (a Stop input in state `3`): plan the stop for the
    /// active feature mode, run the landing search from the live row, snap the
    /// reel, and **claim** it - latch the landed payline value + 1 into
    /// [`SlotMachine::claimed`], which is what fills that column of the marquee
    /// tally. Once all three reels are stopped the spin is evaluated and the
    /// machine moves to [`SlotPhase::Payout`]. Returns `false` when stopping
    /// isn't allowed or the reel is already stopped.
    // PORT: FUN_801cf0d8 state 3 (per-reel stop) + FUN_801d0554 (snap + claim)
    pub fn stop_reel(&mut self, reel: usize) -> bool {
        if self.phase != SlotPhase::Stopping || reel >= REEL_COUNT || self.stopped[reel].is_some() {
            return false;
        }
        // The guaranteed-hit mode drives later reels to the first reel's landed
        // symbol so the line connects. (The bonus round does NOT: its stop plan
        // has no target at all - the reel lands where you stopped it.)
        // This spin's landing line (`DAT_801d4134` row of the `0x801d3630`
        // table): a forced target lands on that payline, not on the middle row.
        let line = LANDING_LINE_BY_JITTER[self.jitter.rem_euclid(5) as usize];
        let line_offset = |r: usize| PAYLINE_ROW_OFFSETS[line][r];
        let guarantee = self.stopped.iter().enumerate().find_map(|(r, s)| {
            s.map(|row| {
                let at = (row as i32 + line_offset(r)).rem_euclid(STRIP_LEN as i32);
                self.strips[r][at as usize]
            })
        });
        let (depth, target) = stop_plan(
            &mut self.rng,
            self.feature_mode,
            self.normal_target,
            guarantee,
        );
        let from_row = self.payline_row(reel);
        let row = land_row(
            &self.strips[reel],
            from_row,
            depth,
            target,
            line_offset(reel),
        );
        self.reel_pos[reel] = (row as i32) << 8;
        self.reel_vel[reel] = 0;
        self.stopped[reel] = Some(row);
        // `DAT_801d3d20[reel] = display[reel][payline_row] + 1`, the frame the
        // reel locks. The +1 is retail's, and it is what makes the tally's
        // "unclaimed" state (`0`) distinguishable from a landed value of `0`.
        self.claimed[reel] = self.strips[reel][row] as i32 + 1;
        self.sounds.ring.push((0, CUE_REEL_STOP));
        if self.reels_stopped() == REEL_COUNT {
            // The third stop leaves the scanner's two-stop window; retail's
            // next state-3 frame zeroes the latch before the payout state.
            self.anticipation = 0;
            let result = self.evaluate_spin();
            self.last_result = Some(result);
            self.phase = SlotPhase::Payout;
            // Raise the marquee's payout caption on a paying spin. Retail gates
            // the caption on the figure being non-zero, so a losing spin leaves
            // the matrix to the tally / attract strip.
            self.caption_payout = result.payout;
            // The payout state's timer starts at 0 on a win and at
            // `0x6B` on a loss (`0x801CF850..0x801CF868`); the motor voice
            // is released (`FUN_800653C8(0x13)` at `0x801CF878`).
            self.caption_frame = if result.payout != 0 {
                0
            } else {
                LOSING_PAYOUT_TIMER
            };
            self.payout_left = result.payout;
            self.win_line = result.line.map_or(-1, |l| l as i32);
            self.sounds.voice_off.push(SPIN_VOICE);
        }
        true
    }

    /// Stop the leftmost still-spinning reel (host convenience for a single
    /// stop button; retail maps three pad bits to the three reels).
    pub fn stop_next_reel(&mut self) -> bool {
        (0..REEL_COUNT).any(|r| self.stopped[r].is_none() && self.stop_reel(r))
    }

    /// Evaluate the stopped spin (`FUN_801d13e8`): outside a bonus round,
    /// check all five paylines all-three-equal on the display strips, keep
    /// the highest-value line, pay `payout_table[symbol]`, and trigger the
    /// bonus round on the jackpot symbols. During a bonus round every spin
    /// pays the **product of the three payline numbers**, unconditionally - no
    /// equality check, no payout table - and the payout is subtracted from the
    /// net-take counter.
    // PORT: FUN_801d13e8 (win evaluation + payout lookup + bonus trigger)
    pub(super) fn evaluate_spin(&mut self) -> SpinResult {
        let rows: [usize; REEL_COUNT] = core::array::from_fn(|r| self.stopped[r].unwrap_or(0));
        let bonus_spin = self.in_bonus_round() && self.bonus_spins > 0;
        if bonus_spin {
            // The bonus round's arithmetic, whole:
            //
            //   payout = (v0 - 0xf) * (v1 - 0xf) * (v2 - 0xf)
            //
            // over the three payline values of the display strip - which, in a
            // bonus round, are bonus-strip values `0x10..=0x19`, so each factor
            // is the numeral 1..=10 drawn on that reel. 1 (1x1x1) to 1000
            // (10x10x10) coins. There is no all-equal gate and no payout-table
            // lookup: every bonus spin pays, and it pays what it shows.
            //
            // The claimed values the tally prints are `value + 1` off the SAME
            // rows, so `tally_product() == payout` by construction, not by
            // agreement between two copies.
            let numbers: [u32; REEL_COUNT] = core::array::from_fn(|r| {
                slot_payout::bonus_number_for_value(self.strips[r][rows[r]])
            });
            let payout = numbers.iter().product::<u32>() as i32;
            let all_equal =
                (1..REEL_COUNT).all(|r| self.strips[r][rows[r]] == self.strips[0][rows[0]]);
            self.net_take -= payout;
            self.bonus_spins -= 1;
            if self.bonus_spins <= 0 {
                self.feature_mode = 0;
                // Latch the "just ended" flag so the next spin runs long enough
                // for the symbols to rotate back onto the payline.
                self.bonus_just_ended = true;
            }
            return SpinResult {
                // Retail forces the winning line to the centre (`DAT_801d3c8c = 1`)
                // - the bonus round pays the middle row and lights its lamp.
                line: Some(1),
                symbol: all_equal.then(|| self.strips[0][rows[0]]),
                payout,
                bonus_triggered: false,
                bonus_spin: true,
            };
        }
        // Five paylines - three horizontal and two diagonal. The per-reel row
        // offsets are `legaia_minigame_formats::minigame_slot_scene::PAYLINE_ROW_OFFSETS`,
        // read off the retail evaluator's absolute row reads. All five always
        // play; the winning line index is also the medallion the machine lights.
        let mut best: Option<(usize, u8, i32)> = None;
        for (line, offs) in legaia_minigame_formats::minigame_slot_scene::PAYLINE_ROW_OFFSETS
            .iter()
            .enumerate()
        {
            let sym = |r: usize| {
                let row =
                    (rows[r] as isize + offs[r] as isize).rem_euclid(STRIP_LEN as isize) as usize;
                self.strips[r][row]
            };
            let (a, b, c) = (sym(0), sym(1), sym(2));
            if a == b && b == c {
                let value = self.payouts.payout(a).unwrap_or(0) as i32;
                if best.map(|(_, _, v)| value > v).unwrap_or(true) {
                    best = Some((line, a, value));
                }
            }
        }
        let mut result = SpinResult {
            line: best.map(|(l, _, _)| l),
            symbol: best.map(|(_, s, _)| s),
            payout: best.map(|(_, _, v)| v).unwrap_or(0),
            bonus_triggered: false,
            bonus_spin,
        };
        if let Some((_, sym, _)) = best {
            if let Some(rounds) = slot_payout::bonus_rounds_for(sym) {
                // The jackpot symbols kick off the bonus round: 3 rounds for the
                // red "punch" (id 9), 1 for the blue "kick" (id 8). From the next
                // spin the display strip starts rotating onto the numerals.
                self.feature_mode = FEATURE_MODE_BONUS;
                self.bonus_spins = rounds as i32;
                result.bonus_triggered = true;
            } else if self.feature_mode != 0 && self.feature_mode != 4 {
                // A resolved normal-mode win clears a tease/hot feature.
                self.feature_mode = 0;
            }
        }
        result
    }

    /// Collect the latched payout into the balance (state `4` tally, capped
    /// at [`BALANCE_CAP`]) and return to idle. Returns the credited amount.
    // PORT: FUN_801cf0d8 state 4 (payout tally into DAT_801d4114)
    pub fn collect(&mut self) -> i32 {
        if self.phase != SlotPhase::Payout {
            return 0;
        }
        // The rest of the tally at once; what the timed tally already moved
        // is in the balance.
        let credit = self.payout_left;
        self.balance = (self.balance + credit).min(BALANCE_CAP);
        self.payout_left = 0;
        self.phase = SlotPhase::Idle;
        self.win_line = -1;
        // The caption comes down with the tally it was captioning.
        self.caption_payout = 0;
        self.caption_frame = 0;
        credit
    }

    /// The marquee's per-frame inputs, read off this machine's live state.
    ///
    /// Every field but the caption pair is a global the machine already keeps:
    /// the feature mode, the reel state word, the bonus-round counter and the
    /// per-reel claimed values are the same storage the payout arithmetic uses,
    /// so the marquee cannot disagree with what the machine actually paid.
    pub fn marquee(&self) -> MarqueeFrame {
        MarqueeFrame {
            payout: self.caption_payout,
            payout_frame: self.caption_frame,
            feature_mode: self.feature_mode,
            reel_state: match self.phase {
                SlotPhase::Idle => 1,
                SlotPhase::Spinning => 2,
                SlotPhase::Stopping => 3,
                SlotPhase::Payout => 4,
                SlotPhase::Menu | SlotPhase::NoCoins | SlotPhase::Leaving => self.retail_state(),
                SlotPhase::CashedOut => 100,
            },
            bonus_rounds: self.bonus_spins,
            claimed: self.claimed,
        }
    }

    /// What the dot matrix shows this frame, as blit placements. Pair with
    /// [`legaia_minigame_formats::minigame_slot_scene::render_marquee`] and a parsed message
    /// bank to rasterise it.
    pub fn marquee_placements(&self) -> Vec<MarqueePlacement> {
        compose_marquee_frame(&self.marquee())
    }

    /// Commit the cash-out (state `100`): the machine is done and the final
    /// balance is returned for assignment into the casino coin bank
    /// (`_DAT_800845A4 = DAT_801d4114` - an assignment, not a delta).
    // PORT: FUN_801cf0d8 state 100 (cash-out commit)
    pub fn cash_out(&mut self) -> i32 {
        self.phase = SlotPhase::CashedOut;
        self.balance
    }
}
