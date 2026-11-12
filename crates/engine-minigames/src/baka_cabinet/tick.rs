//! The cabinet's per-frame state dispatcher and its duel / choice / dev-menu arms.

use super::*;

impl BakaCabinet {
    /// One frame of the cabinet.
    pub fn tick(&mut self, input: &CabinetInput) -> CabinetFrame {
        let step = input.frame_step;
        let edge = input.pad_edge;
        let mut f = CabinetFrame::default();

        // Prologue: two accumulators run regardless of state.
        self.blink += step;
        self.frames += 1;

        f.draw_arena = draws_arena(self.state);
        f.draw_hud = draws_hud(self.state);

        let entry = self.state;
        match self.state {
            ST_BOOT => {
                self.suppress_scene = false;
                self.all_clear = 0;
                self.high_score = 0;
                self.scene_timer = 0;
                self.state = ST_ATTRACT;
            }
            ST_ATTRACT => {
                // `0x801CF6E4..0x801CF7AC`: the blink phase `DAT_801DC128`
                // advances first and its bit 4 picks the prompt's level; the
                // title card is called with the clock, which advances after.
                self.scene_timer += step;
                f.widgets.push(SheetCell {
                    widget: WIDGET_PRESS_START,
                    x: PRESS_START_PEN.0,
                    y: PRESS_START_PEN.1,
                    brightness: if self.scene_timer & 0x10 != 0 {
                        0x40
                    } else {
                        0x80
                    },
                });
                f.title_card = Some(self.intro_timer);
                self.intro_timer += step;
                if edge & CABINET_START != 0 {
                    f.cues.push(crate::baka_fighter::BAKA_CUE_CONFIRM);
                    self.scene_timer = 0;
                    self.state = ST_ATTRACT_OUT;
                }
            }
            ST_ATTRACT_OUT => {
                // `0x801CF808..0x801CF864`: for the first `0x1E` steps the
                // prompt flashes at `0x100` over the card; the card's clock
                // does not advance here, only the blink phase does.
                if self.scene_timer < 0x1E {
                    f.widgets.push(SheetCell {
                        widget: WIDGET_PRESS_START,
                        x: PRESS_START_PEN.0,
                        y: PRESS_START_PEN.1,
                        brightness: 0x100,
                    });
                    f.title_card = Some(self.intro_timer);
                }
                self.scene_timer += step;
                if self.scene_timer >= 0x3D {
                    self.state = ST_SELECT_SETUP;
                }
            }
            ST_SELECT_SETUP => {
                // `0x801CF88C..0x801CFA60`: the three party fighters spawn
                // (their lineup is `baka_duel_scene::SELECT_LINEUP`), the
                // stage counter re-seeds to `2`, and the banner actor spawns.
                self.high_score = 0;
                self.match_phase = 0;
                self.round = 0;
                self.perfect_matches = 0;
                self.stage = 2;
                self.stage_display = 0;
                self.state = ST_SELECT;
                f.widgets.push(player_select_banner());
            }
            ST_SELECT => {
                let mask = edge | input.pad_edge_alt;
                let (c, moved) = cursor_step(
                    self.select_cursor,
                    SELECT_OPTIONS,
                    mask,
                    CABINET_LEFT,
                    CABINET_RIGHT,
                );
                self.select_cursor = c;
                if moved {
                    f.cues.push(crate::baka_fighter::BAKA_CUE_CURSOR);
                }
                f.widgets.push(player_select_banner());
                if edge & CABINET_CONFIRM != 0 {
                    f.cues.push(crate::baka_fighter::BAKA_CUE_CONFIRM);
                    self.state = ST_SELECT_DONE;
                }
                // The tail clears both unconditionally, whether or not the
                // player confirmed this frame.
                self.max_streak = 0;
                self.secret = SecretOpponent::None;
            }
            ST_SELECT_DONE => {
                self.match_phase = 1;
                self.state_timer = 0;
                self.state = ST_SELECT_WIPE;
            }
            ST_SELECT_WIPE => {
                if self.state_timer >= 0x1F {
                    self.state_timer = 0;
                    self.state = ST_SELECT_POSE;
                }
                self.state_timer += step;
            }
            ST_SELECT_POSE => {
                // `0x801CFBBC..0x801CFCB4`: the chosen fighter's actor spawns
                // with the cursor as its roster id (`+0x5A`) and stands at
                // `-(record[cursor] +0x44 + 200)`; the round SM spawns too.
                f.install_player = Some(self.select_cursor);
                self.state = ST_OPPONENT_INSTALL;
            }
            ST_OPPONENT_INSTALL => {
                // The tally's rows clear for the new rung, the match-bonus
                // row with them (`0x801CFCE4..0x801CFCF4`).
                self.bonus = 0;
                // Every match starts at round `0` (`0x801CFDB0`), so a secret
                // rung's first round is the one its intro pose waits for.
                self.round = 0;
                let (roster, mesh) = rung_fold(self.stage, self.secret);
                f.install_opponent = Some((roster, mesh));
                self.match_phase = 2;
                self.state = ST_ROUND_SETUP;
            }
            ST_ROUND_SETUP => {
                self.state_timer = 0;
                // The banner clock `DAT_801DC128` restarts with every round
                // (`0x801D0188`).
                self.scene_timer = 0;
                self.state = ST_ROUND_BANNER_A;
            }
            // Each banner arm calls `FUN_801D5C7C(DAT_801DC128)` and then
            // steps the clock (`0x801D01D8`, `0x801D0290`, `0x801D0344`,
            // `0x801D0468`).
            ST_ROUND_BANNER_A => {
                f.round_banner = Some(self.scene_timer);
                self.scene_timer += step;
                self.state = ST_ROUND_BANNER_B;
            }
            ST_ROUND_BANNER_B => {
                f.round_banner = Some(self.scene_timer);
                self.scene_timer += step;
                if !input.camera_hold {
                    self.state = ST_ROUND_BANNER_C;
                }
            }
            // The spin's arm (`0x801D0324..0x801D0454`) holds until the yaw
            // passes a full turn, so the HUD (drawn from `0x36` on, the
            // epilogue's `s7` test at `0x801D218C`) waits for the camera.
            ST_ROUND_BANNER_C => {
                self.suppress_scene = false;
                f.round_banner = Some(self.scene_timer);
                self.scene_timer += step;
                if !input.camera_hold {
                    self.state = ST_ROUND_BANNER_D;
                }
            }
            ST_ROUND_BANNER_D => {
                f.round_banner = Some(self.scene_timer);
                self.scene_timer += step;
                if self.scene_timer >= 0x79 {
                    self.scene_timer = 0;
                    self.state = ST_ROUND_READY;
                }
            }
            ST_ROUND_READY => {
                self.state = ST_ROUND_GO;
            }
            ST_ROUND_GO => {
                // The "FIGHT!" flash's line, on the arm's first frame
                // (`0x801D04D4..0x801D04F0`).
                if self.scene_timer == 0 {
                    f.xa = Some(crate::baka_fighter_chrome::FIGHT_FLASH_XA);
                }
                self.scene_timer += step;
                if self.scene_timer >= 0x31 {
                    self.state = ST_DUEL;
                    self.state_timer = 0;
                }
            }
            ST_DUEL => self.tick_duel(input, &mut f),
            ST_PERFECT => {
                // The wipe holds at `0xF0` for its last draw, but the word
                // itself is zeroed for the tally (`0x801D095C..0x801D0960`).
                self.state_timer += step * 3;
                if self.state_timer >= 0xF1 {
                    self.state_timer = 0;
                    self.state = ST_TALLY;
                }
            }
            ST_TALLY => {
                // The hold counts only once the drain `FUN_801D239C` reports
                // every row empty (`bnez v0` at `0x801D09C0`).
                if !input.tally_draining {
                    self.state_timer += step;
                }
                if self.state_timer >= 0x79 {
                    self.state_timer = 0xF0;
                    self.state = ST_TALLY_OUT;
                    if self.secret != SecretOpponent::None {
                        self.state_timer = 0;
                        self.state = ST_CHOICE_SECRET;
                        self.suppress_scene = true;
                        self.stage_display += 1;
                    }
                }
            }
            ST_TALLY_OUT => {
                self.suppress_scene = false;
                self.state_timer -= step * 3;
                if self.state_timer < 0 {
                    self.state_timer = 0;
                    self.stage_display += 1;
                    self.state = ST_CHOICE;
                    if self.all_clear != 0 {
                        self.clear_fade_in = 0;
                        self.clear_fade_out = 0;
                        self.state = ST_CLEAR_A;
                    }
                }
            }
            ST_CHOICE => self.tick_choice(input, &mut f),
            ST_CHOICE_SECRET => {
                // The scene stays suppressed here: the tally's secret branch
                // raised it and the spin arm `0x35` is what drops it
                // (`0x801D033C`), so the walls, the floor light and the HUD
                // stay off through the challenger's announcement and intro.
                // The arm's first frame seeds the word at `1`, then tests it
                // and steps it, the hand-off frame included
                // (`0x801D1094..0x801D1168`).
                if self.state_timer == 0 {
                    self.state_timer = 1;
                }
                if self.state_timer >= 0xB5 {
                    self.match_phase = 1;
                    self.state = ST_NEXT_RUNG;
                }
                self.state_timer += step;
            }
            ST_NEXT_RUNG => {
                self.match_phase = 1;
                self.state = ST_OPPONENT_INSTALL;
            }
            ST_LOSE => {
                self.state_timer += step * 2;
                if self.state_timer >= 0xF1 {
                    self.match_phase = 0;
                    self.suppress_scene = true;
                    self.state_timer = 0;
                    self.lose_timer = 0;
                    self.state = ST_GAME_OVER;
                }
            }
            ST_GAME_OVER => {
                if self.state_timer == 0 {
                    self.state_timer = 1;
                    // The game-over line (`0x801D1248..0x801D1268`).
                    f.xa = Some(crate::baka_fighter_chrome::GAME_OVER_XA);
                    // The pot is zeroed here, not on the way out: a mid-run
                    // defeat forfeits every coin the run had accumulated.
                    if self.pot != 0 {
                        f.forfeit = Some(self.pot);
                    }
                    self.pot = 0;
                }
                self.lose_timer += step;
                if self.lose_timer >= 0x79 {
                    self.match_phase = 0;
                    self.exit_timer = 0;
                    self.state = ST_EXIT;
                }
            }
            ST_PAUSE_2 => {
                let (c, moved) = cursor_step(self.menu_cursor, 2, edge, CABINET_UP, CABINET_DOWN);
                self.menu_cursor = c;
                if moved {
                    f.cues.push(crate::baka_fighter::BAKA_CUE_CURSOR);
                }
                if edge & CABINET_CANCEL != 0 {
                    f.cues.push(crate::baka_fighter::BAKA_CUE_CANCEL);
                    self.state = ST_DUEL;
                } else if edge & CABINET_CONFIRM != 0 {
                    f.cues.push(crate::baka_fighter::BAKA_CUE_CONFIRM);
                    if self.menu_cursor == 0 {
                        self.state = ST_DUEL;
                    } else {
                        self.match_phase = 0;
                        self.state = ST_BOOT;
                    }
                }
            }
            ST_PAUSE_3 => {
                let (c, moved) =
                    cursor_step_mod(self.menu_cursor, 3, edge, CABINET_UP, CABINET_DOWN);
                self.menu_cursor = c;
                if moved {
                    f.cues.push(crate::baka_fighter::BAKA_CUE_CURSOR);
                }
                if edge & CABINET_CANCEL != 0 {
                    f.cues.push(crate::baka_fighter::BAKA_CUE_CANCEL);
                    self.state = ST_DUEL;
                } else if edge & CABINET_CONFIRM != 0 {
                    f.cues.push(crate::baka_fighter::BAKA_CUE_CONFIRM);
                    match self.menu_cursor {
                        2 => self.state = ST_HOWTO,
                        1 => self.state = ST_EXIT,
                        _ => self.state = ST_DUEL,
                    }
                }
            }
            ST_HOWTO => {
                if edge & CABINET_ANY_FACE != 0 {
                    f.cues.push(crate::baka_fighter::BAKA_CUE_CONFIRM);
                    self.state = ST_PAUSE_3;
                }
            }
            ST_DEV_MENU => self.tick_dev_menu(input, &mut f),
            ST_CLEAR_A => {
                self.suppress_scene = false;
                self.clear_fade_in += step * 4;
                if self.clear_fade_in >= 0x101 {
                    self.clear_fade_in = 0x100;
                    self.state_timer = 0;
                    self.state = ST_CLEAR_B;
                }
            }
            ST_CLEAR_B => {
                self.state_timer += step * 4;
                if self.state_timer >= 0x100 {
                    self.state_timer = 0xFF;
                    self.state = ST_CLEAR_C;
                    // `0x801D1AB4..0x801D1AC0`: a run whose displayed stage
                    // reads `0xE` (the score gate inserted both secret
                    // opponents) ends on no fighter.
                    f.spawn_clear_fighter =
                        self.stage_display != crate::baka_fighter_chrome::CLEAR_FINAL_STAGE;
                }
                f.clear_fade = Some(self.state_timer.clamp(0, 0xFF) as u8);
            }
            ST_CLEAR_C => {
                self.clear_fade_in = (self.clear_fade_in - step * 2).max(0);
                self.clear_fade_out += step * 2;
                if self.clear_fade_out >= 0x101 {
                    self.clear_fade_out = 0x100;
                    self.state = ST_CLEAR_D;
                }
                f.clear_fade = Some(0xFF);
            }
            ST_CLEAR_D => {
                self.state_timer -= step;
                if self.state_timer < 0x50 {
                    self.state_timer = 0x50;
                    self.state = ST_CLEAR_E;
                }
                f.clear_fade = Some(self.state_timer.clamp(0, 0xFF) as u8);
            }
            ST_CLEAR_E => {
                self.suppress_scene = false;
                self.clear_fade_out = (self.clear_fade_out - step * 2).max(0);
                self.state_timer += step;
                // The last fade draws at `0xFF`; the word itself is zeroed
                // with the hand-off (`0x801D1CAC..0x801D1CD0`).
                let mut level = self.state_timer;
                if self.state_timer >= 0x100 {
                    level = 0xFF;
                    self.state_timer = 0;
                    self.match_phase = 0;
                    self.exit_timer = 0;
                    self.state = ST_EXIT;
                }
                f.clear_fade = Some(level.clamp(0, 0xFF) as u8);
            }
            ST_DEV_EDITOR => {
                self.state = ST_DEV_EDITOR_RUN;
            }
            ST_DEV_EDITOR_RUN => {
                if edge & CABINET_DEV_DUMP != 0 {
                    f.action_dump = Some(action_table_dump(&self.action_tables));
                }
                if edge & CABINET_DEV_EXIT != 0 {
                    self.match_phase = 1;
                    self.state = ST_SELECT_POSE;
                }
            }
            ST_EXIT => {
                self.exit_timer += step;
                if self.exit_timer >= 0x3D && self.pot != 0 {
                    // `FUN_80026018` - the mode-24 return warp, which is what
                    // moves the accumulator into the casino coin bank.
                    f.payout = Some(self.pot);
                    self.pot = 0;
                }
            }
            _ => {}
        }

        // The five all-clear beats each end their arm on the all-clear
        // announcer `FUN_801D21FC(0)` (`0x801D1B5C` for `0xFA` / `0xFB`,
        // `0x801D1BD0` / `0x801D1C24` / `0x801D1C90` for `0xFC` / `0xFD` /
        // `0xFE`), after the arm has moved its fades. No other state calls
        // it - the round banner states `0x33..0x36` call only the banner.
        if (ST_CLEAR_A..=ST_CLEAR_E).contains(&entry) {
            let cd = crate::baka_fighter_chrome::countdown_frame(
                &mut self.clear_announcer,
                step,
                false,
                self.clear_fade_out,
                self.clear_fade_in,
                self.stage_display,
            );
            if cd.xa.is_some() {
                f.xa = cd.xa;
            }
            f.widgets.extend(cd.draws.iter().map(|d| SheetCell {
                widget: d.widget,
                x: i32::from(d.x),
                y: i32::from(d.y),
                brightness: d.brightness,
            }));
        }

        // Epilogue. Retail re-reads the state word here, *after* the body, and
        // the exit state short-circuits the whole epilogue - so the frame a
        // state transitions into `0x1F4` draws nothing either, whatever draw
        // flags its body raised.
        if self.state == ST_EXIT {
            f.draw_arena = false;
            f.draw_hud = false;
            f.state = self.state;
            return f;
        }
        self.shake = shake_decay(self.shake, step);
        if f.draw_hud && !self.suppress_scene {
            let hud = hud_frame(&HudInput {
                hp: input.hp,
                round_wins: [input.player_round_wins, input.opponent_round_wins],
                win_target: input.win_target,
                combo_taken: input.combo_taken,
                chosen_type: input.chosen_type,
                type_hold: input.type_hold,
                stage_display: self.stage_display + 1,
                high_score: self.high_score,
                latch: self.max_streak,
                blink: self.blink,
            });
            self.max_streak = hud.max_streak;
            f.hud = Some(hud);
        }
        if self.suppress_scene {
            f.draw_arena = false;
            f.draw_hud = false;
        }
        f.state = self.state;
        f
    }

    /// The duel state's own body: the round bracket plus the pause-menu edge.
    ///
    /// Retail's duel state only *reads* the round timer `DAT_801DBF88`
    /// against `0xB5` (`0x801D0620`); the fight resolution SM `FUN_801D3468`
    /// advances it, by the frame step, only on the frames its round is
    /// decided ([`CabinetInput::round_clock`]). So the hold is counted from
    /// the deciding exchange, not from the duel's start. The port's cabinet
    /// advances the word itself on exactly those frames.
    fn tick_duel(&mut self, input: &CabinetInput, f: &mut CabinetFrame) {
        let step = input.frame_step;
        // The menu cursor is re-zeroed every duel frame, not on entry to the
        // menu - so a re-opened pause menu always starts on its first row.
        self.menu_cursor = 0;
        let mut pause_ready = input.pause_ready;
        if self.state_timer < 0xB5 {
            if input.round_clock {
                self.state_timer += step;
            }
        } else {
            // Round over. The arm drops the pause gate (`0x801D0638`), so
            // the menu cannot open on this frame.
            pause_ready = false;
            self.state_timer = 0;
            self.round = (self.round + 1).min(9);
            self.state = ST_ROUND_SETUP;

            if input.player_round_wins == input.win_target {
                // Only `DAT_801DBEE8` is written here. The pot
                // (`_DAT_80084440`) is fed by the score tally's drain
                // (`FUN_801D239C`, `0x801D28AC..0x801D28BC`) and by nothing
                // in this dispatcher - its one store to the word is the
                // game-over zero.
                self.prize = input.rung_prize as i32;
                // A win over an ordinary rung may insert a secret one
                // (`0x801D072C..0x801D0790`); a win over a secret rung clears
                // the override, and the second secret's ends the run in the
                // all-clear (`all_clear = 2`, `0x801D0794..0x801D07A4`). The
                // stage steps whenever no override is left standing, and
                // reaching `0xE` is the other all-clear.
                if self.secret == SecretOpponent::None {
                    self.secret = secret_opponent_gate(self.stage, self.high_score);
                } else {
                    if self.secret == SecretOpponent::Second {
                        self.all_clear = 2;
                    }
                    self.secret = SecretOpponent::None;
                }
                let adv = advance_stage(self.stage, self.secret);
                self.stage = adv.stage;
                if adv.all_clear {
                    self.all_clear = 1;
                }
                self.bonus = match_bonus(input.opponent_round_wins);
                // `state = 0x65` sits in the **delay slot** of the
                // "did the opponent take a round" branch, so it runs either
                // way - a match win always reaches the flourish. The branch
                // (`bnez` at `0x801D0868`) skips the shutout tally
                // `DAT_801DBF28` when the opponent took a round.
                self.state = ST_PERFECT;
                if input.opponent_round_wins == 0 {
                    self.perfect_matches += 1;
                }
            } else if input.opponent_round_wins == input.win_target {
                self.state = ST_LOSE;
                self.state_timer = 0;
            }
        }

        if pause_ready && input.pad_edge & CABINET_DUEL_MENU != 0 {
            f.cues.push(crate::baka_fighter::BAKA_CUE_CONFIRM);
            self.menu_cursor = 0;
            self.state = if input.dev_menu_enabled {
                ST_DEV_MENU
            } else {
                self.state_timer = 0;
                ST_PAUSE_3
            };
        }
    }

    /// The "NEXT GAME / PAY OUT" menu.
    fn tick_choice(&mut self, input: &CabinetInput, f: &mut CabinetFrame) {
        let step = input.frame_step;
        let edge = input.pad_edge;
        if self.state_timer == 0 {
            let (c, moved) = cursor_step(self.menu_cursor, 2, edge, CABINET_LEFT, CABINET_RIGHT);
            self.menu_cursor = c;
            if moved {
                f.cues.push(crate::baka_fighter::BAKA_CUE_CURSOR);
            }
            if edge & CABINET_CONFIRM != 0 {
                f.cues.push(crate::baka_fighter::BAKA_CUE_CONFIRM);
                if self.menu_cursor == 0 {
                    // NEXT GAME: risk the pot on the next rung. The latch's
                    // first frame at `1` starts the announcer line, the same
                    // frame as the confirm (`0x801D0DD8..0x801D0DF8`).
                    self.state_timer = 1;
                    f.xa = Some(crate::baka_fighter_chrome::NEXT_GAME_XA);
                } else {
                    // PAY OUT: bank it and leave.
                    self.match_phase = 0;
                    self.exit_timer = 0;
                    self.state = ST_EXIT;
                }
            }
        }
        // The latch's tail runs on the confirm frame too: the hand-off test
        // first, then the step (`0x801D0F40..0x801D0F94`).
        if self.state_timer >= 0xD3 {
            self.match_phase = 1;
            self.state = ST_NEXT_RUNG;
        }
        if self.state_timer != 0 {
            self.state_timer += step;
        }
    }

    /// The developer menu: five rows, with left/right editing the debug-mode
    /// and stage fields on rows `2` and `3`.
    fn tick_dev_menu(&mut self, input: &CabinetInput, f: &mut CabinetFrame) {
        let edge = input.pad_edge;
        let (c, moved) = cursor_step_mod(
            self.menu_cursor,
            DEV_MENU_ROWS,
            edge,
            CABINET_UP,
            CABINET_DOWN,
        );
        self.menu_cursor = c;
        if moved {
            f.cues.push(crate::baka_fighter::BAKA_CUE_CURSOR);
        }
        if self.menu_cursor == 3 {
            if edge & CABINET_LEFT != 0 {
                self.stage -= 1;
            }
            if edge & CABINET_RIGHT != 0 {
                self.stage += 1;
            }
        }
        if edge & CABINET_CONFIRM != 0 {
            f.cues.push(crate::baka_fighter::BAKA_CUE_CONFIRM);
            match self.menu_cursor {
                0 => self.state = ST_DUEL,
                1 => {
                    self.match_phase = 0;
                    self.state = ST_BOOT;
                }
                4 => {
                    self.match_phase = 0;
                    self.state = ST_DEV_EDITOR;
                }
                _ => {}
            }
        }
    }
}
