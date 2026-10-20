//! The cabinet side of a fight: rung and player installs, the cabinet tick and the select-screen lineup.

use super::*;
use legaia_minigame_formats::baka_opponents::ROUND_WIN_TARGET;

impl BakaFight {
    /// Seat the next rung's opponent, the port side of the cabinet's
    /// `ST_OPPONENT_INSTALL` (`0x1E`): the roster record the rung fold names
    /// replaces slot 1, and both fighters start a fresh match (retail's round
    /// setup `0x32` refills HP to `0xC80` and clears the bracket). The pot is
    /// untouched - it is the global accumulator, not the fight's.
    fn install_rung(&mut self, roster: usize) {
        if let Some(t) = self.tables.clone()
            && let (Some(opp), Some(act)) = (t.0.get(roster), t.1.get(roster))
        {
            self.cfg[1] = FighterConfig::from_tables(opp, act);
            if let Some(st) = self.strike.as_mut() {
                st[1] = StrikeTable::from_actions(act);
                if let Some(h) = self.roster_clips.as_ref().and_then(|r| r.get(roster)) {
                    st[1].clips = *h;
                }
            }
        }
        if let Some(opp) = self.tables.as_ref().and_then(|t| t.0.get(roster)) {
            self.stand_off[1] = i32::from(opp.stand_off);
        }
        self.f = [FighterState::new(), FighterState::new()];
        self.motion = Default::default();
        self.stand_z = [0; 2];
        self.camera.round_setup();
        self.setup_pending = true;
        self.round = 0;
        self.rate_divisor = STRIKE_RATE_DIVISOR;
        self.special_latch = false;
        self.afterimages.clear();
        self.afterimage_frames.clear();
        self.settle_timer = 0;
        self.phase = MatchPhase::Fighting;
        self.last_exchange = None;
        self.tally = None;
        self.score_rows = [0; 3];
    }

    /// Step the cabinet shell one frame off the duel's own live state.
    ///
    /// REF: FUN_801cf388 - retail's dispatcher reads exactly these globals
    /// each frame (`DAT_801DBFF0` / `DAT_801DC098` round wins,
    /// `DAT_801DBED0` the target, the per-slot HP and hits-taken counters for
    /// the HUD pass), so the port feeds them across rather than duplicating
    /// them inside the cabinet.
    pub(super) fn tick_cabinet(&mut self, frame_step: i32) {
        let input = crate::baka_cabinet::CabinetInput {
            frame_step,
            // The host's packed edge, but only once the match is decided and
            // the cabinet has left the duel state (see `set_cabinet_pad`): the
            // duel band's one read is the pause edge `0x110`, which overlaps
            // the cameo's held Triangle, and the port's cabinet sits in the
            // duel state for up to `0xB5` frames after the deciding exchange.
            pad_edge: if self.cabinet.front_end()
                || matches!(self.phase, MatchPhase::MatchOver(_))
                    && self.cabinet.state() != crate::baka_cabinet::ST_DUEL
            {
                std::mem::take(&mut self.cabinet_pad)
            } else {
                self.cabinet_pad = 0;
                0
            },
            pad_edge_alt: 0,
            pad_held: 0,
            dev_menu_enabled: false,
            player_round_wins: self.f[0].round_wins,
            opponent_round_wins: self.f[1].round_wins,
            win_target: ROUND_WIN_TARGET,
            rung_prize: self.cfg[1].gold_reward,
            hp: [self.f[0].hp, self.f[1].hp],
            combo_taken: [self.f[0].combo, self.f[1].combo],
            chosen_type: self
                .f
                .each_ref()
                .map(|fs| fs.chosen.map_or(0, |a| i32::from(a.type_id()))),
            type_hold: [self.f[0].hold, self.f[1].hold],
            // `FUN_801D3468` advances the round timer only once the round is
            // decided (a finisher flag or both HPs at zero) - in the port,
            // once the exchange that decided it has left the fight phase.
            round_clock: !matches!(self.phase, MatchPhase::Fighting),
        };
        self.cabinet_frame = self.cabinet.tick(&input);
        self.cues.append(&mut self.cabinet_frame.cues);
        if let Some(roster) = self
            .cabinet_frame
            .install_player
            .and_then(|r| usize::try_from(r).ok())
        {
            self.install_player(roster);
        }
        if let Some((roster, _mesh)) = self.cabinet_frame.install_opponent
            && let Ok(roster) = usize::try_from(roster)
        {
            self.install_rung(roster);
        }
        // The intro title card belongs to the cabinet, not to the duel: both
        // `jal 0x801D59D4` sites in PROT 0976 are attract arms of this same
        // state machine, and those arms are what advance its clock. Hand the
        // clock to the chrome while the cabinet is on one of them, so the card
        // runs for any host that enters at the cabinet's own boot rather than
        // only for one that armed a private timeline. The arms call the card
        // only on the frames they draw it (the fade-out stops after `0x1E`),
        // so the chrome takes exactly what this frame called.
        if self.cabinet.front_end() || self.cabinet_frame.title_card.is_some() {
            self.chrome.set_intro_clock(self.cabinet_frame.title_card);
        }
    }

    /// Seat the player-select pick in slot 0 - the port side of state `0x0E`,
    /// which spawns the chosen fighter's actor with the select cursor as its
    /// roster id (`sh a0,0x5a(t1)` at `0x801CFC50`) and reads that record's
    /// stand-off (`0x801CFC40`). The pick brings its own stats, action table,
    /// strike clips and special camera; the opponent install that follows
    /// starts the match.
    fn install_player(&mut self, roster: usize) {
        let Some(t) = self.tables.clone() else {
            return;
        };
        let (Some(rec), Some(act)) = (t.0.get(roster), t.1.get(roster)) else {
            return;
        };
        self.cfg[0] = FighterConfig::from_tables(rec, act);
        if let Some(st) = self.strike.as_mut() {
            st[0] = StrikeTable::from_actions(act);
            if let Some(h) = self.roster_clips.as_ref().and_then(|r| r.get(roster)) {
                st[0].clips = *h;
            }
        }
        self.stand_off[0] = i32::from(rec.stand_off);
    }

    /// The player-select lineup while it is on stage: the select cursor
    /// (`DAT_801DBF70`) and the frames the three idle clips have run. `None`
    /// outside states `0x0A` / `0x0B`.
    pub fn select_lineup(&self) -> Option<(usize, i32)> {
        crate::baka_cabinet::lineup_live(self.cabinet.state()).then(|| {
            (
                usize::try_from(self.cabinet.select_cursor()).unwrap_or(0),
                self.select_clock,
            )
        })
    }

    /// The cabinet's own widget draws this frame - the attract prompt, the
    /// "PLAYER SELECT" banner and the "NEXT GAME / PAY OUT" sheet - as one
    /// list every host draws the same way.
    pub fn cabinet_cells(&self) -> Vec<crate::baka_cabinet::SheetCell> {
        let mut out = self.cabinet_frame.widgets.clone();
        if let Some(sheet) = self.cabinet.choice_sheet() {
            out.extend_from_slice(&sheet);
        }
        out
    }
}

/// The roster id the cabinet serves first on every visit: the rung fold of
/// the stage counter `FUN_801CF00C` seeds (`DAT_801DC10C = 2`, so roster
/// `2 + 3 = 5`). Every later rung comes from the cabinet's own stage advance
/// through [`crate::baka_cabinet::rung_fold`].
pub fn first_rung_roster() -> usize {
    let cab = crate::baka_cabinet::BakaCabinet::new();
    crate::baka_cabinet::rung_fold(cab.stage(), cab.secret_opponent()).0 as usize
}
