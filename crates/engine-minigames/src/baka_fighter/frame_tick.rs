//! The per-frame step of a [`BakaFight`]: presentation, cameo, rules and result-banner ticks.

use super::*;

impl BakaFight {
    /// `FUN_801D4DF8`'s call on the booking arm for `slot`: the pair spawns
    /// at the fighter's position offset by the current action's strike TRS -
    /// the landed sub-keyframe, or slot `0` when `reset_keyframe` (the
    /// draw's calls). A fight without strike tables has no offsets and
    /// spawns nothing.
    fn spawn_impact(&mut self, slot: usize, reset_keyframe: bool) {
        let Some(st) = self.strike.as_ref() else {
            return;
        };
        let Some(attack) = self.f[slot].chosen else {
            return;
        };
        let action = StrikeTable::action_of(attack);
        let live = self.f[slot].clock.landed.map_or(-1, |i| i as i32);
        let k = crate::baka_fighter_chrome::impact_keyframe_index(live, reset_keyframe);
        let Some(off) = usize::try_from(k)
            .ok()
            .and_then(|k| st[slot].offsets.get(action)?.get(k).copied())
        else {
            return;
        };
        let p = self.fighter_position(slot);
        let pos = (p[0] as i16, p[1] as i16, p[2] as i16);
        // The block's facing word `+0x28`: clear for the player's seat, set
        // for the opponent's (the round setup's `0x801CFFF8` / `0x801CFFFC`).
        let facing = slot & 1 == 1;
        let spawns = crate::baka_fighter_chrome::impact_effect_pair(
            slot,
            pos,
            (off[0], off[1], off[2]),
            facing,
            self.special_latch,
        );
        self.impact.spawn_pair(&spawns);
    }

    /// Step every live afterimage one frame (retail's actor pool runs the
    /// `0x801D7684` callback once per frame per actor).
    fn tick_afterimages(&mut self, frame_step: i32) {
        use legaia_minigame_formats::baka_opponents::ACTION_SPECIAL;
        self.afterimage_frames.clear();
        let strike = self.strike.as_ref();
        let chosen = [self.f[0].chosen, self.f[1].chosen];
        let mut out = Vec::new();
        self.afterimages.retain_mut(|a| {
            let clip = strike
                .and_then(|st| st[a.owner].clips[ACTION_SPECIAL])
                .map(|c| (c.n, c.frames));
            // With no clip header nothing can expire the ghosts; they go with
            // the special's own exchange instead.
            if clip.is_none() && chosen[a.owner] != Some(BakaAttack::Special) {
                return false;
            }
            let f = a.tick(frame_step, clip);
            if f.retire {
                return false;
            }
            out.push((a.owner, f));
            true
        });
        self.afterimage_frames = out;
    }

    /// One frame of the whole cabinet on this frame's **packed** pad words
    /// (`edge` = `_DAT_8007B874`, `held` = `_DAT_8007B850`) - the per-frame
    /// step every host runs, so the play hosts' world tick and the standalone
    /// minigames page drive one ladder, one tally and one input rule.
    ///
    /// - The front end (attract card, player select) reads the edge itself.
    /// - After a match the cabinet runs its tally, the "NEXT GAME / PAY OUT"
    ///   sheet, the next rung or the game-over / all-clear sequence off the
    ///   edge, with `pot` - the host's winnings accumulator - as the coins at
    ///   risk; any face button fast-forwards the tally.
    /// - In a duel, the frame's throw is retail's last-write-wins test read
    ///   back to front (Cross, then Circle, then Square), and the held word
    ///   feeds the round setup's cameo test.
    ///
    /// The returned outcome is what the host's accumulator does: add
    /// [`BakaFrameOutcome::paid`], clear it on a forfeit, and leave the
    /// cabinet on [`BakaFrameOutcome::exit`].
    pub fn frame(&mut self, edge: u16, held: u16, pot: u32) -> BakaFrameOutcome {
        use legaia_engine_system::pad::{PACK_CIRCLE, PACK_CROSS, PACK_SQUARE, PACK_TRIANGLE};
        if self.cabinet.front_end() {
            self.set_cabinet_pad(edge);
            self.tick(1);
            return BakaFrameOutcome::default();
        }
        if self.match_over() {
            let face = edge & (PACK_TRIANGLE | PACK_CIRCLE | PACK_CROSS | PACK_SQUARE) != 0;
            self.cabinet.set_pot(pot);
            self.set_cabinet_pad(edge);
            self.tick_with_input(1, face);
            return BakaFrameOutcome {
                paid: self.take_tally_gold().max(0) as u32,
                forfeit: self.cabinet_frame.forfeit.is_some(),
                exit: self.cabinet.exit_done(),
            };
        }
        let attack = if edge & PACK_CROSS != 0 {
            Some(BakaAttack::C)
        } else if edge & PACK_CIRCLE != 0 {
            Some(BakaAttack::B)
        } else if edge & PACK_SQUARE != 0 {
            Some(BakaAttack::A)
        } else {
            None
        };
        if let Some(attack) = attack {
            self.choose(0, attack);
        }
        self.set_held_pad(held);
        self.tick(1);
        BakaFrameOutcome::default()
    }

    /// Advance the duel one frame: decay cooldowns, let the CPU pick, charge
    /// a held special, resolve the exchange, and book damage / rounds / the
    /// match, mirroring the retail resolution arm.
    ///
    /// PORT: FUN_801d3468 (round / match resolution state machine)
    pub fn tick(&mut self, frame_step: i32) {
        self.tick_with_input(frame_step, false);
    }

    /// [`Self::tick`] with the tally's fast-forward input: `face_button` is
    /// this frame's edge-triggered face-button test (`_DAT_8007b874 & 0xf0`),
    /// which snaps the end-of-match tally to its end state.
    pub fn tick_with_input(&mut self, frame_step: i32, face_button: bool) {
        self.tick_rules(frame_step, face_button);
        self.tick_presentation(frame_step);
    }

    /// The fighters' display clips and the arena camera, one frame: what the
    /// combat tick's clip store / idle reset and the clip selector do to each
    /// fighter actor (`FUN_801D3F44`, `FUN_800204F8`), and the camera spin /
    /// glide ([`crate::baka_duel::DuelCamera`]). Nothing here feeds the
    /// rules - the exchange books off the [`StrikeClock`].
    fn tick_presentation(&mut self, frame_step: i32) {
        use crate::baka_cabinet::{ST_CHOICE, ST_CHOICE_SECRET, ST_TALLY_OUT};
        use crate::baka_duel::MOTION_WIN;
        let st = self.cabinet.state();
        if crate::baka_cabinet::front_end(st) {
            // State `0x0A` writes the select camera (`0x801CF8F4..0x801CF924`)
            // as it spawns the lineup; the attract arms draw no 3D at all.
            if crate::baka_cabinet::lineup_live(st) {
                if self.select_clock == 0 {
                    self.camera.select_screen();
                }
                self.select_clock += frame_step;
            } else {
                self.select_clock = 0;
            }
            return;
        }
        if matches!(self.phase, MatchPhase::MatchOver(0))
            && matches!(st, ST_TALLY_OUT | ST_CHOICE | ST_CHOICE_SECRET)
            && !self.motion[0].pinned
        {
            // The tally's end (state `0x66` leaving, `0x801D0A38..0x801D0AB0`,
            // and the secret variant `0x6D` at `0x801D0FBC..0x801D1024`): the
            // result close-up, the player's win flourish held, and Noa one
            // step toward the camera.
            self.camera.result_close_up(st == ST_CHOICE_SECRET);
            self.motion[0].play(MOTION_WIN, true);
            self.motion[0].pinned = true;
            if self.cfg[0].roster_id == crate::baka_duel::RESULT_STEP_FIGHTER {
                self.stand_z[0] += crate::baka_duel::RESULT_STEP_Z;
            }
        }
        if std::mem::take(&mut self.setup_pending)
            && crate::baka_fighter_chrome::cameo_spawns(self.held_pad)
        {
            // The round setup's spawn of prototype `0x801D7624` (phase and
            // cursor zeroed by the allocator).
            self.cameo = Some(CameoActor {
                phase: 0,
                cursor: 0,
                clip: crate::baka_fighter_chrome::CAMEO_CLIP_WALK,
            });
        }
        self.tick_cameo(frame_step);
        if let Some(st) = self.strike.as_ref() {
            for (m, t) in self.motion.iter_mut().zip(st.iter()) {
                let r = m
                    .record
                    .min(legaia_minigame_formats::baka_opponents::ACTIONS_PER_FIGHTER - 1);
                m.step(t.speed[r], self.rate_divisor, t.clips[r], frame_step);
            }
        }
        self.camera.tick(frame_step);
    }

    /// One frame of the cameo's actor: the animator's pose, the clip
    /// selector's advance at the forced step `+0x6A = 8`, then the phase
    /// advance by the frame step; the actor retires once the pose raises its
    /// retire bit.
    ///
    /// REF: FUN_801D6310
    fn tick_cameo(&mut self, frame_step: i32) {
        let Some(c) = self.cameo.as_mut() else {
            return;
        };
        let Some(pose) = crate::baka_fighter_chrome::cameo_pose(c.phase) else {
            self.cameo = None;
            return;
        };
        if pose.retire {
            self.cameo = None;
            return;
        }
        if pose.clip != c.clip {
            c.clip = pose.clip;
            c.cursor = 0;
        }
        c.cursor += CAMEO_STEP * frame_step;
        c.phase = c.phase.wrapping_add(frame_step as i16);
    }

    /// The rules half of [`Self::tick_with_input`], then the round-result
    /// banner tail the resolution SM runs after it.
    fn tick_rules(&mut self, frame_step: i32, face_button: bool) {
        self.tick_rules_body(frame_step, face_button);
        self.tick_result_banner();
    }

    /// The round-result banners (`FUN_801D3468`'s tail, `0x801D3864..`):
    /// once per round - the sub-state `DAT_801DBF84` - the first frame a
    /// fighter's HP is down, the chrome raises the banner the two HPs and
    /// the untouched flag name ([`crate::baka_fighter_chrome::RoundResult`]).
    /// They stand for the duel state's round-over hold
    /// ([`crate::baka_fighter_chrome::RESULT_HOLD_FRAMES`]) and the next
    /// round's banner waits for them, as retail draws it from the round
    /// setup after the hold; on the deciding round they stand through the
    /// win flourish / loss / GAME OVER screens and come down when the
    /// cabinet moves on to its tally. Retail's lifetime is the spawn
    /// template's (`0x801DB9C4`), which the port does not run, so the hold
    /// is the cabinet's state timer. The next round's start re-arms the latch
    /// and the untouched flag.
    ///
    /// PORT: FUN_801d3468 (`0x801D3864..0x801D39F0`)
    pub(super) fn tick_result_banner(&mut self) {
        use crate::baka_cabinet::{ST_DUEL, ST_GAME_OVER, ST_LOSE, ST_PERFECT};
        use crate::baka_fighter_chrome::RoundResult;
        // Past the decided match (the tally, the sheet, the next rung) the
        // cabinet's own screens own the frame.
        if matches!(self.phase, MatchPhase::MatchOver(_))
            && !matches!(
                self.cabinet.state(),
                ST_DUEL | ST_PERFECT | ST_LOSE | ST_GAME_OVER
            )
        {
            self.chrome.clear_result();
            return;
        }
        if self.result_latch || self.cabinet.front_end() {
            return;
        }
        let result = match (self.f[0].hp <= 0, self.f[1].hp <= 0) {
            (true, true) => RoundResult::Draw,
            (true, false) => RoundResult::Lose,
            (false, true) if self.player_untouched => RoundResult::Perfect,
            (false, true) => RoundResult::Win,
            (false, false) => return,
        };
        self.chrome.raise_result(result);
        self.result_latch = true;
    }

    fn tick_rules_body(&mut self, frame_step: i32, face_button: bool) {
        // The HUD renderer latches the running maximum once per frame, so it
        // runs whatever the phase.
        if self.max_combo <= self.f[1].combo {
            self.max_combo = self.f[1].combo;
        }
        let chrome_tick = crate::baka_fighter_chrome::ChromeTick {
            frame_step,
            loading: false,
            match_phase: i32::from(matches!(self.phase, MatchPhase::MatchOver(_))),
            round: self.round as i32,
            round_counter: self.round as i32,
            focus: (0, 1),
        };
        self.chrome_frame = self.chrome.step(&chrome_tick, (&[], &[]));
        self.tick_cabinet(frame_step);
        if self.cabinet.front_end() {
            // Attract and player select: retail spawns the round SM only at
            // `0x0E`, so nothing of the fight runs yet.
            return;
        }
        match self.phase {
            MatchPhase::MatchOver(_) => {
                // The match is decided: the tally screen runs (FUN_801d239c).
                if let Some(t) = self.tally.as_mut() {
                    t.tick(frame_step, face_button);
                    self.cues.extend(t.take_cues());
                }
                return;
            }
            MatchPhase::RoundOver(_) => {
                // The round stays decided - fighters, HP and the result
                // banner as they were - until the cabinet's duel state has
                // run the round timer to `0xB5` and moved to its round setup
                // (`0x801D0620..0x801D063C`, the timer zeroed with it); the
                // next round starts there.
                if self.cabinet.state() == crate::baka_cabinet::ST_DUEL {
                    return;
                }
                self.round += 1;
                self.f[0].reset_round();
                self.f[1].reset_round();
                // The round setup (`0x32`) restores the clip rate the
                // special lowered (`0x801D01A4`), seeds both clips back to
                // the idle (`0x801CFFB0` / `0x801CFFC0`) and snaps the camera.
                self.rate_divisor = STRIKE_RATE_DIVISOR;
                self.special_latch = false;
                // The banner sub-state and the untouched flag re-arm with the
                // round (`DAT_801DBF84`, `DAT_801DBF24`).
                self.result_latch = false;
                self.player_untouched = true;
                self.motion = Default::default();
                self.camera.round_setup();
                self.setup_pending = true;
                self.phase = MatchPhase::Fighting;
                return;
            }
            MatchPhase::Fighting => {}
        }

        // Cooldown decay (`cooldown -= frame_step * 0x10`, floored at 0).
        for s in 0..2 {
            if self.f[s].cooldown > 0 {
                self.f[s].cooldown -= frame_step * COOLDOWN_DECAY;
            }
            if self.f[s].cooldown < 0 {
                self.f[s].cooldown = 0;
            }
        }

        // CPU commits once its cooldown elapses.
        for s in 0..2 {
            if self.ai_controlled[s] && self.can_choose(s) {
                let pick = self.ai_pick(s);
                self.f[s].chosen = Some(pick);
                self.commit(s, pick);
            }
        }
        // The held-type timer the HUD's lit attack icon flashes off
        // (block `+0x8C`), after this frame's commits as in retail's tick.
        for fs in &mut self.f {
            fs.step_hold(frame_step);
        }

        // The strike clocks (retail's per-fighter combat tick, which runs
        // before the resolution SM reads the `+0x0C` words it leaves).
        if let Some(st) = self.strike.as_ref() {
            for (fs, table) in self.f.iter_mut().zip(st.iter()) {
                if let Some(attack) = fs.chosen {
                    let a = StrikeTable::action_of(attack);
                    fs.clock.step(
                        &table.frames[a],
                        table.speed[a],
                        self.rate_divisor,
                        table.clips[a],
                        frame_step,
                    );
                }
            }
        }
        self.tick_afterimages(frame_step);
        // The impact parts ride the actor-pool walk at the same
        // `frame step x DAT_1F80037D` factor as every part tick.
        let delta = (frame_step.max(0) * self.rate_divisor.max(0)).min(i32::from(u16::MAX));
        self.impact.tick(delta as u16);

        let outcome = self.resolve(frame_step);
        // The keyframe gate: the resolution SM books a decided exchange only
        // while the winner's strike has landed (`0x801D36DC` / `0x801D3730`),
        // and a draw while either side's has (`0x801D378C..0x801D37A4`).
        let outcome = match (self.strike.is_some(), outcome) {
            (true, ExchangeOutcome::FighterWins(w))
                if self.f[w].clock.state != StrikeState::Landed =>
            {
                ExchangeOutcome::Undecided
            }
            (true, ExchangeOutcome::Draw)
                if self.f[0].clock.state != StrikeState::Landed
                    && self.f[1].clock.state != StrikeState::Landed =>
            {
                ExchangeOutcome::Undecided
            }
            (_, o) => o,
        };
        match outcome {
            ExchangeOutcome::Undecided => {}
            ExchangeOutcome::FighterWins(w) => {
                let l = w ^ 1;
                // Phase gate: the winner must actually be mid-attack.
                if self.f[w].chosen.is_none() {
                    return;
                }
                // The booking arm's impact pair runs ahead of the damage
                // kernel (`0x801D36F0` / `0x801D3744`).
                self.spawn_impact(w, false);
                let (damage, critical, special_round_win) = self.apply_damage(l);
                // The damage kernel's clip store on the struck side: the hit
                // reaction, or the knockdown when the special's last strike
                // landed, held on its last frame (`0x801D3C60..0x801D3CA0`).
                self.motion[l].play(
                    if special_round_win {
                        crate::baka_duel::MOTION_KNOCKDOWN
                    } else {
                        crate::baka_duel::MOTION_HIT
                    },
                    true,
                );
                self.motion[l].down = special_round_win;
                // Winner's own hit streak clears; crit flags reset; the loser
                // rolls the comeback crit (retail: FUN_801d6660(loser)).
                self.f[w].combo = 0;
                self.f[0].crit_pending = false;
                self.f[1].crit_pending = false;
                self.roll_comeback_crit(l);
                // Cooldowns: fighter-0 win leaves slot 0 free (retail writes
                // 0/200); a fighter-1 win slows both (200/200).
                if w == 0 {
                    self.f[0].cooldown = 0;
                    self.f[1].cooldown = COOLDOWN_RESET;
                } else {
                    self.f[0].cooldown = COOLDOWN_RESET;
                    self.f[1].cooldown = COOLDOWN_RESET;
                }
                // A special that has strikes left keeps playing: retail's
                // clip runs on and its next sub-keyframe lands again (the
                // kernel re-armed it). Only the struck side's exchange ends.
                let special_continues = self.strike.as_ref().is_some_and(|st| {
                    self.f[w].chosen == Some(BakaAttack::Special)
                        && !special_round_win
                        && self.f[w].clock.landed.map_or(0, |i| i + 1)
                            < st[w].frames[legaia_minigame_formats::baka_opponents::ACTION_SPECIAL]
                                .len()
                });
                if special_continues {
                    self.f[l].chosen = None;
                    self.f[l].committed = false;
                } else {
                    self.end_exchange();
                }
                self.last_exchange = Some(ExchangeReport {
                    winner: w,
                    draw: false,
                    damage,
                    critical,
                    special_round_win,
                });
                if special_round_win {
                    self.end_round(w, true);
                } else if self.f[l].hp == 0 && self.f[w].chosen != Some(BakaAttack::Special) {
                    self.finish_or_end_round(w);
                }
            }
            ExchangeOutcome::Draw => {
                // The draw arm spawns both pairs with the keyframe reset
                // (`0x801D37B4` / `0x801D37C0`), then runs both damage calls.
                self.spawn_impact(0, true);
                self.spawn_impact(1, true);
                // Both take damage, both streaks reset, both roll comebacks.
                let (d0, c0, _) = self.apply_damage(0);
                let (d1, c1, _) = self.apply_damage(1);
                for m in &mut self.motion {
                    m.play(crate::baka_duel::MOTION_HIT, true);
                }
                self.f[0].combo = 0;
                self.f[1].combo = 0;
                self.f[0].crit_pending = false;
                self.f[1].crit_pending = false;
                self.roll_comeback_crit(0);
                self.roll_comeback_crit(1);
                self.f[0].cooldown = COOLDOWN_RESET;
                self.f[1].cooldown = COOLDOWN_RESET;
                self.end_exchange();
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
                    (true, false) => self.finish_or_end_round(1),
                    (false, true) => self.finish_or_end_round(0),
                    (false, false) => {}
                }
            }
        }
    }
}
