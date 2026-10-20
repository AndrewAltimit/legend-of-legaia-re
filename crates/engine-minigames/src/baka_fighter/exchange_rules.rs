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
        self.f[slot].chosen = Some(attack);
        self.commit(slot, attack);
        true
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
        if self.f[winner].clock.state == StrikeState::Landed {
            self.f[winner].clock.state = StrikeState::Consumed;
        }
        let mut special_round_win = false;
        if winner_type == 4 && full_hit {
            self.f[winner].round_wins += 1;
            self.f[loser].committed = true;
            special_round_win = true;
        }

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
        // The result banners rise first (retail's banner tail runs ahead of
        // the round setup), so the round banner asked for here waits out
        // their hold rather than drawing over them.
        self.tick_result_banner();
        self.chrome
            .start_round_banner(crate::baka_fighter_chrome::ROUND_BANNER_SPRITE);
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
