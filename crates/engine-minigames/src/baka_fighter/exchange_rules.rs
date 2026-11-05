//! The exchange rules on [`BakaFight`]: move commit, AI pick, the resolver, damage and round bookkeeping.

use super::*;
use legaia_minigame_formats::baka_opponents::ROUND_WIN_TARGET;

impl BakaFight {
    /// The combat tick's commit, for either side: the clock restarts, and a
    /// special halves the round's clip rate and spawns its afterimage
    /// (`0x801D4538..0x801D4634`).
    pub(super) fn commit(&mut self, slot: usize, attack: BakaAttack) {
        self.f[slot].clock.commit();
        self.f[slot].hold_reset = true;
        // The clip store: `+0x5C` = the attack's display id, `+0x68 = 0`
        // (`0x801D44D8` / `0x801D44DC`), held on its last frame only for the
        // special (`ori v0,v0,0x8` at `0x801D4640`).
        self.motion[slot].play(
            StrikeTable::action_of(attack),
            attack == BakaAttack::Special,
        );
        self.special_fx(slot, attack);
    }

    /// What a special's commit does beyond the clip store: the latch, the
    /// camera glide, the slowed rate and the afterimage.
    fn special_fx(&mut self, slot: usize, attack: BakaAttack) {
        if attack == BakaAttack::Special {
            self.special_latch = true;
        }
        if attack == BakaAttack::Special && self.strike.is_some() {
            // The special's camera glide (`0x801D4644..0x801D4740`): the
            // player's row of the per-fighter table, the opponent's fixed
            // record.
            let record = if slot == 0 {
                self.special_cameras.get(self.cfg[0].roster_id).copied()
            } else {
                Some(crate::baka_duel::OPPONENT_SPECIAL_GLIDE)
            };
            if let Some(r) = record {
                self.camera.arm_glide(&r);
            }
        }
        if attack == BakaAttack::Special
            && let Some(st) = self.strike.as_ref()
        {
            self.rate_divisor = crate::baka_fighter_chrome::SPECIAL_RATE_DIVISOR;
            let speed = st[slot].speed[legaia_minigame_formats::baka_opponents::ACTION_SPECIAL];
            self.afterimages
                .push(crate::baka_fighter_chrome::AfterimageActor::spawn(
                    slot, speed,
                ));
        }
    }

    /// Commit an attack for `slot` this exchange. Returns `false` (ignored)
    /// while the fighter can't act - see [`Self::can_choose`] - and always
    /// for [`BakaAttack::Special`]: type 4 is not a button. Retail's combat
    /// tick throws it on its own, as the finisher against a foe already at
    /// 0 HP (see [`Self::finish_or_end_round`]).
    pub fn choose(&mut self, slot: usize, attack: BakaAttack) -> bool {
        if attack == BakaAttack::Special || !self.can_choose(slot) {
            return false;
        }
        if self.strike.is_some() {
            // The commit is the combat tick's, on the tick that reads the edge.
            self.pad_attack[slot] = Some(attack);
            return true;
        }
        self.f[slot].chosen = Some(attack);
        self.commit(slot, attack);
        true
    }

    /// One frame of the duel on the disc tables, in the pool walk's order:
    /// the player's combat tick, the round SM, the opponent's combat tick
    /// (the actor list of `minigame_baka_fighter_retail_fight`).
    pub(super) fn duel_frame(&mut self, frame_step: i32) {
        self.combat_tick(0, frame_step);
        self.round_sm(frame_step);
        self.combat_tick(1, frame_step);
        self.clips_stepped = true;
    }

    /// The per-fighter combat tick.
    ///
    /// The opponent reads no pad and picks only in answer to the player's
    /// commit (`DAT_801DC124`, `0x801D41B4..0x801D42EC`): that answer clears
    /// its lock, marks its strike consumed and passes the commit gate
    /// whatever its clip is doing, so a player who holds back is never
    /// attacked. A commit needs the clip idle or the seat's strike booked; the
    /// clip's end returns the seat to idle, clearing its type, its lock and
    /// its streak (`0x801D411C..0x801D415C`).
    ///
    /// PORT: FUN_801d3f44
    fn combat_tick(&mut self, slot: usize, frame_step: i32) {
        let other = slot ^ 1;
        let pad = std::mem::take(&mut self.pad_attack[slot]);
        let live = self.cabinet.state() == crate::baka_cabinet::ST_DUEL;
        if !live || self.f[slot].committed {
            self.step_clip(slot, frame_step, false);
            return;
        }
        if self.motion[slot].ended {
            self.motion[slot].record = 0;
            self.motion[slot].hold = false;
            self.motion[slot].ended = false;
            let fs = &mut self.f[slot];
            fs.chosen = None;
            fs.locked = false;
            fs.combo = 0;
        }
        let mut pad = pad;
        let mut answered = false;
        if slot == 1 && self.ai_controlled[1] {
            pad = None;
            if self.player_commit >= 0 {
                self.f[1].locked = false;
                self.f[1].clock.state = StrikeState::Consumed;
                pad = Some(self.ai_pick(1));
                answered = true;
                self.player_commit = -1;
            }
        }
        if self.f[slot].locked {
            pad = None;
        }
        // The keyframe lookup, on the cursor's last step over whatever clip
        // the seat stands in (`0x801D4304..0x801D435C`).
        if let Some(st) = self.strike.as_ref() {
            let clock = &mut self.f[slot].clock;
            if clock.state != StrikeState::Landed {
                let frames = st[slot]
                    .frames
                    .get(self.motion[slot].record)
                    .map_or(&[][..], |f| &f[..]);
                let hit = keyframe_in_range(frames, clock.prev, self.motion[slot].cursor);
                if let Some(i) = hit
                    && clock.landed != Some(i)
                {
                    clock.landed = Some(i);
                    clock.state = StrikeState::Landed;
                }
            }
        }
        let mut next = None;
        if !self.special_latch {
            next = pad;
            if self.f[slot].hp != 0 && self.f[other].hp == 0 {
                next = Some(BakaAttack::Special);
            }
        }
        let fs = &mut self.f[slot];
        if matches!(fs.chosen.map(BakaAttack::type_id), Some(1..=3)) {
            fs.hold += frame_step;
        } else {
            fs.hold = 0;
        }
        if let Some(attack) = next
            && !self.f[slot].committed
            && !self.f[other].committed
            && (self.f[slot].clock.state == StrikeState::Consumed
                || self.motion[slot].record == 0
                || answered)
        {
            self.f[slot].clock.state = StrikeState::Armed;
            self.f[other].clock.state = StrikeState::Armed;
            self.f[slot].clock.landed = None;
            self.f[slot].hold = 0;
            self.f[slot].chosen = Some(attack);
            let action = StrikeTable::action_of(attack);
            let hold = self.motion[slot].hold || attack == BakaAttack::Special;
            self.motion[slot].play(action, hold);
            if slot == 0 {
                self.player_commit = if attack == BakaAttack::Special {
                    -1
                } else {
                    action as i32
                };
            }
            self.special_fx(slot, attack);
        }
        self.step_clip(slot, frame_step, true);
    }

    /// The clip selector's advance at the record's speed, with the pre-step
    /// cursor stored for the next lookup (`0x801D47E0..0x801D47EC`). The rate
    /// `+0x6A` is written only by the tick's logic half (`0x801D4780..
    /// 0x801D47CC`): the other half steps at whatever it left, so a knocked-down
    /// seat plays its knockdown at the rate of the clip it was in.
    fn step_clip(&mut self, slot: usize, frame_step: i32, rate: bool) {
        let Some(st) = self.strike.as_ref() else {
            return;
        };
        let m = &mut self.motion[slot];
        let r = m
            .record
            .min(legaia_minigame_formats::baka_opponents::ACTIONS_PER_FIGHTER - 1);
        let clip = st[slot].clips[r].unwrap_or(FALLBACK_CLIP);
        if rate {
            self.clip_rate[slot] = st[slot].speed[r].wrapping_mul(self.rate_divisor);
        }
        self.f[slot].clock.prev = m.cursor;
        // The stored rate is already `speed * divisor`, and `step` takes it `>> 3`.
        m.step(self.clip_rate[slot], 1, Some(clip), frame_step);
        self.f[slot].clock.cursor = m.cursor;
    }

    /// The round SM's body in the duel state: the booking arms and what they
    /// leave. The types stand after a booking, so an attack with a second
    /// strike lands again on the same exchange, until a clip ends or someone
    /// commits.
    ///
    /// PORT: FUN_801d3468
    fn round_sm(&mut self, frame_step: i32) {
        for r in &mut self.rumble {
            if *r > 0 {
                *r -= frame_step << 4;
            }
            *r = (*r).max(0);
        }
        if self.cabinet.state() != crate::baka_cabinet::ST_DUEL {
            return;
        }
        match self.resolve(frame_step) {
            ExchangeOutcome::FighterWins(w) if self.f[w].clock.state == StrikeState::Landed => {
                let l = w ^ 1;
                self.spawn_impact(w, false);
                let (damage, critical, special_round_win) = self.apply_damage(l);
                self.rumble = if w == 0 {
                    [0, COOLDOWN_RESET]
                } else {
                    [COOLDOWN_RESET; 2]
                };
                self.f[0].crit_pending = false;
                self.f[1].crit_pending = false;
                self.f[w].combo = 0;
                self.roll_comeback_crit(l);
                self.last_exchange = Some(ExchangeReport {
                    winner: w,
                    draw: false,
                    damage,
                    critical,
                    special_round_win,
                });
                if special_round_win {
                    self.end_round(w, true);
                } else if self.f[l].hp == 0 && !self.has_finisher(w) {
                    self.end_round(w, false);
                }
            }
            ExchangeOutcome::Draw
                if self.f[0].clock.state == StrikeState::Landed
                    || self.f[1].clock.state == StrikeState::Landed =>
            {
                self.spawn_impact(0, true);
                self.spawn_impact(1, true);
                let (d0, c0, _) = self.apply_damage(0);
                let (d1, c1, _) = self.apply_damage(1);
                self.rumble = [COOLDOWN_RESET; 2];
                self.f[0].combo = 0;
                self.f[1].combo = 0;
                self.f[0].crit_pending = false;
                self.f[1].crit_pending = false;
                self.roll_comeback_crit(0);
                self.roll_comeback_crit(1);
                self.last_exchange = Some(ExchangeReport {
                    winner: 1,
                    draw: true,
                    damage: d0.max(d1),
                    critical: c0 || c1,
                    special_round_win: false,
                });
                match (self.f[0].hp == 0, self.f[1].hp == 0) {
                    // Double KO replays the round: no round win is credited.
                    (true, true) => self.phase = MatchPhase::RoundOver(0),
                    (true, false) if !self.has_finisher(1) => self.end_round(1, false),
                    (false, true) if !self.has_finisher(0) => self.end_round(0, false),
                    _ => {}
                }
            }
            _ => {}
        }
    }

    /// A seat's standing fighter has a special with strikes to land.
    fn has_finisher(&self, slot: usize) -> bool {
        self.f[slot].hp > 0
            && self.strike.as_ref().is_some_and(|st| {
                !st[slot].frames[legaia_minigame_formats::baka_opponents::ACTION_SPECIAL].is_empty()
            })
    }

    /// The CPU move pick: random attack or a backward step of the scripted
    /// pattern, exactly as the retail picker rolls it.
    ///
    /// PORT: FUN_801d487c (opponent AI move picker)
    pub(super) fn ai_pick(&mut self, slot: usize) -> BakaAttack {
        let roll = self.rng.next_u15() as i32;
        let r6 = roll % 6;
        let pattern_len = self.cfg[slot].ai_pattern.len();
        if r6 < 3 {
            if self.f[slot].ai_cursor == 0 {
                return BakaAttack::from_type_id((r6 % 3) as u8 + 1).unwrap();
            }
        } else if self.f[slot].ai_cursor == 0 {
            // Seed the cursor to the pattern length; the pattern then plays
            // back-to-front to exhaustion.
            if pattern_len > 0 && self.cfg[slot].ai_pattern[0] != 0 {
                self.f[slot].ai_cursor = pattern_len;
            }
            if self.f[slot].ai_cursor == 0 {
                return BakaAttack::from_type_id((r6 % 3) as u8 + 1).unwrap();
            }
        }
        self.f[slot].ai_cursor -= 1;
        let sym = self.cfg[slot].ai_pattern[self.f[slot].ai_cursor];
        BakaAttack::from_type_id((sym - 1) % 3 + 1).unwrap()
    }

    /// Resolve the current exchange.
    ///
    /// PORT: FUN_801d3a14 (exchange win-condition: settle timer, special
    /// priority, committed gates, and the 2>1 / 3>2 / 1>3 beats relation)
    pub(super) fn resolve(&mut self, frame_step: i32) -> ExchangeOutcome {
        // Settle timer: while it hasn't elapsed the exchange stays open.
        if self.settle_timer - frame_step >= 0 {
            self.settle_timer -= frame_step;
            return ExchangeOutcome::Undecided;
        }
        self.settle_timer = 0;
        let p1 = self.f[0].chosen.map(BakaAttack::type_id).unwrap_or(0);
        let p2 = self.f[1].chosen.map(BakaAttack::type_id).unwrap_or(0);
        // The special is an unbeatable win, fighter 0 checked first - the
        // retail resolver's first two tests (`0x801D3A54` / `0x801D3A5C`: a
        // type of 4 on either side returns that side before any other
        // check). Only the auto-finisher ever throws it, and the keyframe
        // gate in the tick is what paces it.
        if p1 == 4 {
            return ExchangeOutcome::FighterWins(0);
        }
        if p2 == 4 {
            return ExchangeOutcome::FighterWins(1);
        }
        if p1 == 0 && p2 == 0 {
            return ExchangeOutcome::Undecided;
        }
        if self.f[0].committed || self.f[1].committed {
            return ExchangeOutcome::Undecided;
        }
        if p1 == p2 {
            return ExchangeOutcome::Draw;
        }
        // Beats relation from the dump: 2 beats 1, 3 beats 2, 1 beats 3.
        match (p1, p2) {
            (1, 2) | (2, 3) | (3, 1) => ExchangeOutcome::FighterWins(1),
            (2, 1) | (3, 2) | (1, 3) => ExchangeOutcome::FighterWins(0),
            // One side idle (type 0): an attack never lands on a non-attacker.
            _ => ExchangeOutcome::Undecided,
        }
    }

    /// Apply exchange damage to `loser`. Returns `(damage, critical,
    /// special_round_win)`.
    ///
    /// PORT: FUN_801d3b18 (damage application: HP-tiered ATK/DEF, combo bonus,
    /// crit override, special full-hit round win)
    pub(super) fn apply_damage(&mut self, loser: usize) -> (i32, bool, bool) {
        let winner = loser ^ 1;
        if loser == 0 {
            // `sw zero,-0x40DC` in both arms that damage slot 0
            // (`0x801D375C`, `0x801D37B8`).
            self.player_untouched = false;
        }
        // The retail ring write (`_DAT_8007b6d8 = 9`) sits at the top of
        // FUN_801D3B18, before the damage arithmetic - so a double-KO draw
        // (which applies damage twice) queues the cue twice, as it does here.
        self.cues.push(BAKA_CUE_HIT);
        self.f[loser].hits_taken += 1;
        let winner_type = self.f[winner].chosen.map(BakaAttack::type_id).unwrap_or(0);

        // Special full-hit: the special scores the immediate round win only
        // when it lands on the action's final sub-keyframe - the winner's landed
        // sub-keyframe (block `+0x98`) is the special record's last
        // (`record[+0x1C] - 1`, `0x801D3C00..0x801D3C0C`). The damage kernel
        // also hands the winner's strike back to the lookup (`+0x0C = 2`,
        // `0x801D3EB0`), which is what lets the special's next strike land.
        let full_hit = self.strike.as_ref().is_some_and(|st| {
            let n =
                st[winner].frames[legaia_minigame_formats::baka_opponents::ACTION_SPECIAL].len();
            n > 0 && self.f[winner].clock.landed == Some(n - 1)
        });
        if self.strike.is_some() {
            self.f[winner].clock.state = StrikeState::Consumed;
            self.f[loser].locked = true;
            // Both actors step back from the blow, and the camera focus
            // follows (`0x801D3EB4..0x801D3F28`).
            let push = if loser == 0 { -0x20 } else { 0x20 };
            self.push_x += push;
            self.push_ease += push;
        }
        let mut special_round_win = false;
        if winner_type == 4 && full_hit {
            self.f[winner].round_wins += 1;
            self.f[loser].committed = true;
            special_round_win = true;
        }
        // The clip store on the struck side, held on its last frame: `base + 6`,
        // one further for the third attack, two for the special's last strike
        // (`0x801D3BA0..0x801D3CA0`). The kernel leaves `+0x62 & 0x100` alone, so
        // a blow landing on the frame the seat's last clip ended is lost to
        // that clip's idle reset on the seat's next tick.
        let ended = self.motion[loser].ended;
        self.motion[loser].play(
            match (winner_type, special_round_win) {
                (_, true) => crate::baka_duel::MOTION_KNOCKDOWN,
                (3, _) => crate::baka_duel::MOTION_HIT_THIRD,
                _ => crate::baka_duel::MOTION_HIT,
            },
            true,
        );
        self.motion[loser].ended = ended;
        self.motion[loser].down = special_round_win;

        let def_tier = self.cfg[loser].def_tier(self.f[loser].hp);
        let atk_tier = self.cfg[winner].atk_tier(self.f[winner].hp);
        let power = self.cfg[winner]
            .attack_power
            .get(winner_type as usize)
            .copied()
            .unwrap_or(0);
        let mod_ = self.cfg[loser].damage_mod;
        let combo = self.f[loser].combo;
        let hit = power + power * atk_tier / 100;
        let guard = mod_ + mod_ * def_tier / 100;
        let mut dmg = (hit * (200 - guard) * 0x20) / 100 + (combo - 1) * COMBO_DAMAGE_STEP;
        let critical = self.f[winner].crit_pending;
        if critical {
            dmg = power << 7;
        }
        // The special carries no HP payload. Its action record's `+0x18` power
        // is 0 for all 17 fighters (disc corpus), so retail's kernel would
        // compute `(combo-1)*0x40` = **-0x40** on a fresh combo - but retail
        // never applies it: type 4 is the auto-finisher, gated on the
        // opponent's HP already being 0 (`overlay_baka_fighter_801d3f44.txt`),
        // and the kernel's HP write is `hp > 0`-gated
        // (`overlay_baka_fighter_801d3b18.txt` `0x801d3e58..0x801d3e68`:
        // `blez` skips the `subu`). A special that wins against a foe still
        // standing (the other fighter's special, or a test driving the
        // resolver) would have the raw arithmetic *heal* the loser by 64;
        // the faithful HP delta for a special-won exchange is zero - its
        // payoff is the exchange / round win, never HP.
        if winner_type == 4 {
            dmg = 0;
        }
        if self.f[loser].hp > 0 {
            self.f[loser].hp -= dmg;
        }
        if self.f[loser].hp < 1 {
            self.f[loser].hp = 0;
        }
        self.f[loser].combo += 1;
        (dmg, critical, special_round_win)
    }

    /// Roll the comeback critical for a fighter that just took a hit: fires
    /// while `0 < HP < 0x280` on `rand() % 100 < crit_chance`.
    ///
    /// PORT: FUN_801d6660 (critical / lucky-hit roll)
    pub(super) fn roll_comeback_crit(&mut self, slot: usize) {
        let hp = self.f[slot].hp;
        if hp > 0 && hp < CRIT_HP_BAND {
            let roll = self.rng.next_u15() as i32 % 100;
            if roll < self.cfg[slot].crit_chance {
                self.f[slot].crit_pending = true;
            }
        }
    }

    /// Clear the exchange state on both fighters (host pacing simplification:
    /// retail sequences the recovery through the per-action keyframes).
    pub(super) fn end_exchange(&mut self) {
        for s in 0..2 {
            self.f[s].chosen = None;
            self.f[s].committed = false;
        }
    }

    /// A knockout: `winner` has just put the foe at 0 HP.
    ///
    /// With strike data this is retail's auto-finisher gate (own HP `!= 0`,
    /// foe HP `== 0`, round undecided - the slot-0 and slot-1 branches of
    /// `FUN_801d3f44` alike): the winner throws the special on its own, and
    /// the round is credited by its last strike landing, inside the damage
    /// kernel. A fight with no strike data, or a special with no strike
    /// frames to land, has no finisher to play and ends the round here.
    ///
    /// REF: FUN_801d3f44
    pub(super) fn finish_or_end_round(&mut self, winner: usize) {
        let has_finisher = self.f[winner].hp > 0
            && self.strike.as_ref().is_some_and(|st| {
                !st[winner].frames[legaia_minigame_formats::baka_opponents::ACTION_SPECIAL]
                    .is_empty()
            });
        if !has_finisher {
            self.end_round(winner, false);
            return;
        }
        self.end_exchange();
        self.f[winner].chosen = Some(BakaAttack::Special);
        self.commit(winner, BakaAttack::Special);
    }

    /// End the current round with `winner` (KO path credits here; a landed
    /// full special already credited inside the damage kernel).
    pub(super) fn end_round(&mut self, winner: usize, already_credited: bool) {
        if !already_credited {
            self.f[winner].round_wins += 1;
        }
        self.accumulate_round_score(winner);
        // The result banners rise here (retail's banner tail); the next
        // round's banner is the cabinet's, from its round setup on.
        self.tick_result_banner();
        if self.f[winner].round_wins >= ROUND_WIN_TARGET {
            self.phase = MatchPhase::MatchOver(winner);
            if winner == 0 {
                // The retail end-of-match tally screen comes up on a player
                // win and drains the prize into gold.
                self.tally = Some(BakaTally::new([
                    self.score_rows[0],
                    self.score_rows[1],
                    self.score_rows[2],
                    self.cfg[1].gold_reward as i32,
                ]));
            }
        } else {
            self.phase = MatchPhase::RoundOver(winner);
        }
    }

    /// Fold this round's two bonus increments into the score rows, exactly
    /// as `FUN_801d2a28` does: the combo row takes the running-maximum
    /// streak's table entry, the bonus row the winner's end-HP entry.
    ///
    /// The score channel runs only when a host has supplied the two overlay
    /// bonus tables. Without them the port has no score channel at all - the
    /// rows stay empty and the tally carries the coin prize alone, which is
    /// the behaviour every disc-free oracle measures. Retail always has the
    /// tables, so supplying them is what turns the retail channel on.
    ///
    /// REF: FUN_801d2a28
    fn accumulate_round_score(&mut self, winner: usize) {
        // Only the player's round win reaches the routine (result word `3`,
        // `0x801D0670`, set at `0x801D39F0`), and it reads the player's HP.
        if winner != 0 {
            return;
        }
        let Some(tables) = self.score_tables.clone() else {
            return;
        };
        let gain = baka_round_score(
            self.max_combo,
            &tables.combo_bonus,
            self.f[winner].hp,
            &tables.health_bonus,
        );
        self.score_rows[1] += gain.combo_gain;
        self.score_rows[2] += gain.bonus_gain;
    }
}
