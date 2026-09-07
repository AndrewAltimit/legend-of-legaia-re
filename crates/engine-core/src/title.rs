//! Title screen state machine.
//!
//! Drives the boot-time UI: title fade-in → "Press Start" → main menu
//! (New Game / Continue / Options) → hand off to either field boot or
//! save-select. Engines render via the existing renderer text overlay;
//! audio host fires the title-music BGM through the BGM director.
//!
//! ## States
//!
//! - [`TitlePhase::FadeIn`] - opening fade from black. No input
//!   accepted; advances on a frame counter.
//! - [`TitlePhase::PressStart`] - "Press START" prompt with cursor blink.
//!   Start (or Cross) advances to the main menu.
//! - [`TitlePhase::MainMenu`] - three-row menu (New Game / Continue /
//!   Options). Up/Down move the cursor; Cross confirms.
//! - [`TitlePhase::Done`] - the player chose; engine inspects
//!   [`TitleSession::outcome`].
//!
//! Engines run [`TitleSession::tick`] each frame and react to the
//! returned [`TitleEvent`]s (CursorMoved, MenuConfirmed, etc.). The
//! session is intentionally renderer-free - it knows only abstract phase
//! state, not pixel coordinates.
//!
//! ## What is retail's and what is the port's
//!
//! The menu half runs retail's own law: [`TitleSession::tick`] packs its
//! booleans into the repacked pad word and steps
//! [`legaia_engine_vm::title_overlay::TitleMenuState`], the port of the
//! title tick's `AttractIdle` (`0x10`) block. That gets both hosts the
//! retail cursor step + wrap, the `Start | L1 | Cross` confirm mask, the
//! `0x21` / `0x20` cue pair, and the attract countdown's input-freeze
//! band, from one kernel.
//!
//! Three things around it are the port's own and are named as such:
//!
//! - [`TitlePhase::FadeIn`] / [`TitlePhase::PressStart`] are the port's
//!   staging. `Init` writes sub-mode `0x02` at `0x801DD920` and overwrites
//!   it with `0x11` only when the entry word `_DAT_8007BB00` is non-zero
//!   (`0x801DD97C`), so `0x02 -> 0x14` is the default graph and
//!   `0x11 -> 0x10` the re-entry one; which of the two a player sees is a
//!   mode-graph question this session does not answer. What both share,
//!   and what is ported here, is the menu law.
//! - `continue_enabled` skipping the CONTINUE row has no retail
//!   counterpart - retail always lets the row be picked and lets the
//!   save screen say "No data". It is a port guard, applied on top of the
//!   retail step.
//! - The attract countdown is **off by default**
//!   ([`TitleSession::attract_enabled`]). The kernel is ported and
//!   tested, but retail's fire arm writes master game mode `0x1A` (the
//!   opening movie) and neither shipped host has an attract-movie
//!   destination in its boot UI, so leaving it on would freeze input for
//!   the last sixteen frames of every idle period and then do nothing.
//!   With it off the session is bit-identical to a session with no
//!   countdown at all.

/// Phase of the title state machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TitlePhase {
    FadeIn { frames_remaining: u16 },
    PressStart { blink_phase: u16 },
    MainMenu { cursor: u8 },
    Done(TitleOutcome),
}

/// Final outcome of the title session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TitleOutcome {
    NewGame,
    /// Player picked Continue - engines drop into save-select.
    Continue,
    /// Player opened the Options panel - engines push the menu.
    Options,
}

/// Per-frame input bundle.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TitleInput {
    pub up: bool,
    pub down: bool,
    pub cross: bool,
    pub start: bool,
    pub circle: bool,
}

/// Events emitted per `tick` call. Engines fold these into HUD blips
/// and audio-cue triggers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TitleEvent {
    /// Title fade-in completed; engines start the BGM ramp.
    FadeInDone,
    /// Player pressed Start at the prompt; menu opens.
    StartPressed,
    /// Cursor moved in the main menu.
    CursorMoved { row: u8 },
    /// Player confirmed a menu row.
    MenuConfirmed { row: u8 },
    /// Player picked New Game.
    NewGameSelected,
    /// Player picked Continue.
    ContinueSelected,
    /// Player picked Options.
    OptionsSelected,
    /// The attract countdown underflowed - retail's hand-off to master
    /// game mode `0x1A` (the opening movie). Only ever emitted with
    /// [`TitleSession::attract_enabled`] set; see the module docs for why
    /// it is off by default.
    AttractTimeout,
}

/// Title screen state machine.
#[derive(Debug, Clone)]
pub struct TitleSession {
    phase: TitlePhase,
    /// Frames the [`TitlePhase::FadeIn`] phase lasts. Default 90 (1.5s).
    pub fade_in_frames: u16,
    /// Cursor blink period in frames. Default 30 (0.5s).
    pub blink_period: u16,
    /// Number of menu rows. Default 2 (New Game / Continue). Retail
    /// only carries those two rows; Options is reached through the
    /// in-game field menu, not from the title screen.
    rows: u8,
    /// Set to `false` if no save data is present - disables the Continue
    /// row in the menu.
    pub continue_enabled: bool,
    /// Retail's title-menu state: the row counter, the attract countdown
    /// and the last cue. Stepped once per frame in [`Self::tick`].
    menu: legaia_engine_vm::title_overlay::TitleMenuState,
    /// Whether the attract countdown may fire. Off by default - see the
    /// module docs.
    pub attract_enabled: bool,
}

impl TitleSession {
    pub fn new() -> Self {
        Self {
            phase: TitlePhase::FadeIn {
                frames_remaining: 90,
            },
            fade_in_frames: 90,
            blink_period: 30,
            rows: legaia_engine_vm::title_overlay::TITLE_MENU_ROWS,
            continue_enabled: true,
            menu: legaia_engine_vm::title_overlay::TitleMenuState::new(),
            attract_enabled: false,
        }
    }

    /// The attract countdown's current value, in frames. Retail seeds it
    /// with `0x5DC` and re-arms it on any held pad bit.
    pub fn attract_countdown(&self) -> i32 {
        self.menu.countdown
    }

    /// The SFX cue the last [`Self::tick`] stored, if any - retail's
    /// `0x8007B6D8` halfword (`0x21` cursor move, `0x20` confirm).
    pub fn last_sfx_cue(&self) -> Option<u16> {
        self.menu.sfx
    }

    /// Construct a session with `Continue` disabled (no save data).
    pub fn without_save_data() -> Self {
        let mut s = Self::new();
        s.continue_enabled = false;
        s
    }

    pub fn phase(&self) -> TitlePhase {
        self.phase
    }

    pub fn is_done(&self) -> bool {
        matches!(self.phase, TitlePhase::Done(_))
    }

    pub fn outcome(&self) -> Option<TitleOutcome> {
        match self.phase {
            TitlePhase::Done(o) => Some(o),
            _ => None,
        }
    }

    /// Force-skip to the [`TitlePhase::PressStart`] phase. Used by
    /// engines that pre-load assets while the fade-in animates and
    /// want to drop the player directly at the prompt.
    pub fn skip_fade_in(&mut self) {
        self.phase = TitlePhase::PressStart { blink_phase: 0 };
    }

    /// One-frame tick.
    pub fn tick(&mut self, input: TitleInput) -> Vec<TitleEvent> {
        let mut events = Vec::new();
        let phase = self.phase;
        match phase {
            TitlePhase::FadeIn { frames_remaining } => {
                if frames_remaining > 0 {
                    self.phase = TitlePhase::FadeIn {
                        frames_remaining: frames_remaining - 1,
                    };
                } else {
                    self.phase = TitlePhase::PressStart { blink_phase: 0 };
                    events.push(TitleEvent::FadeInDone);
                }
            }
            TitlePhase::PressStart { blink_phase } => {
                self.phase = TitlePhase::PressStart {
                    blink_phase: (blink_phase + 1) % self.blink_period,
                };
                if input.start || input.cross {
                    let cursor = if self.continue_enabled { 1 } else { 0 };
                    self.phase = TitlePhase::MainMenu { cursor };
                    events.push(TitleEvent::StartPressed);
                }
            }
            TitlePhase::MainMenu { cursor } => {
                // Cancel is the port's own row-return; retail's `0x10`
                // block has no cancel arm at all.
                if input.circle {
                    self.phase = TitlePhase::PressStart { blink_phase: 0 };
                    return events;
                }
                self.menu.row_counter = cursor as i32;
                let (edge, held) = Self::pad_words(input);
                let stepped = self.menu.step(
                    edge,
                    held,
                    1,
                    legaia_engine_vm::title_overlay::TITLE_MENU_ROWS,
                );
                for e in stepped {
                    use legaia_engine_vm::title_overlay::TitleMenuEvent as Vm;
                    match e {
                        Vm::CursorMoved { row } => {
                            // The port's one deviation from the retail
                            // step: a row the host greyed out is skipped.
                            let row = self.skip_disabled(cursor, row);
                            self.menu.row_counter = row as i32;
                            if row != cursor {
                                self.phase = TitlePhase::MainMenu { cursor: row };
                                events.push(TitleEvent::CursorMoved { row });
                            }
                        }
                        Vm::Confirmed { row } => {
                            let outcome = match row {
                                0 => TitleOutcome::NewGame,
                                1 => TitleOutcome::Continue,
                                2 => TitleOutcome::Options,
                                _ => TitleOutcome::NewGame,
                            };
                            self.phase = TitlePhase::Done(outcome);
                            events.push(TitleEvent::MenuConfirmed { row });
                            events.push(match outcome {
                                TitleOutcome::NewGame => TitleEvent::NewGameSelected,
                                TitleOutcome::Continue => TitleEvent::ContinueSelected,
                                TitleOutcome::Options => TitleEvent::OptionsSelected,
                            });
                        }
                        Vm::AttractFired => {
                            if self.attract_enabled {
                                events.push(TitleEvent::AttractTimeout);
                            }
                            // Nothing to hand the screen to, so re-arm
                            // rather than sit under the input freeze.
                            self.menu.countdown =
                                legaia_engine_vm::title_overlay::COUNTDOWN_RESET_VALUE as i32;
                        }
                    }
                }
            }
            TitlePhase::Done(_) => {}
        }
        events
    }

    /// Pack a [`TitleInput`] into the two pad words the ported menu
    /// kernel reads: the just-pressed word the cursor and confirm test,
    /// and the held word the attract countdown re-arms from.
    ///
    /// The bit layout is Legaia's repacked pad word (`FUN_8001822C`):
    /// `0x1000` Up, `0x4000` Down, `0x40` Cross, `0x04` L1, `0x20`
    /// Circle, `0x800` Start.
    fn pad_words(input: TitleInput) -> (u16, u16) {
        let mut w = 0u16;
        if input.up {
            w |= legaia_engine_vm::title_overlay::PADMASK_CURSOR_PREV;
        }
        if input.down {
            w |= legaia_engine_vm::title_overlay::PADMASK_CURSOR_NEXT;
        }
        if input.cross {
            w |= 0x0040;
        }
        if input.start {
            w |= 0x0800;
        }
        if input.circle {
            w |= 0x0020;
        }
        (w, w)
    }

    /// Walk past a row the host disabled. Port-only; retail lets every
    /// row be picked.
    fn skip_disabled(&self, from: u8, to: u8) -> u8 {
        if self.continue_enabled || to != 1 {
            return to;
        }
        from
    }

    #[allow(dead_code)]
    fn step_cursor(&self, from: u8, dir: i8) -> u8 {
        let n = self.rows as i16;
        let mut cursor = from as i16;
        for _ in 0..self.rows {
            cursor = (cursor + dir as i16).rem_euclid(n);
            if !self.continue_enabled && cursor == 1 {
                continue;
            }
            return cursor as u8;
        }
        from
    }
}

impl Default for TitleSession {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fade_in_completes() {
        let mut s = TitleSession::new();
        s.fade_in_frames = 3;
        s.phase = TitlePhase::FadeIn {
            frames_remaining: 3,
        };
        let mut events = Vec::new();
        for _ in 0..4 {
            events.extend(s.tick(TitleInput::default()));
        }
        assert!(matches!(s.phase(), TitlePhase::PressStart { .. }));
        assert!(events.iter().any(|e| matches!(e, TitleEvent::FadeInDone)));
    }

    #[test]
    fn skip_fade_in_drops_to_prompt() {
        let mut s = TitleSession::new();
        s.skip_fade_in();
        assert!(matches!(s.phase(), TitlePhase::PressStart { .. }));
    }

    #[test]
    fn start_press_opens_menu_with_continue_enabled() {
        let mut s = TitleSession::new();
        s.skip_fade_in();
        let events = s.tick(TitleInput {
            start: true,
            ..Default::default()
        });
        match s.phase() {
            TitlePhase::MainMenu { cursor } => assert_eq!(cursor, 1),
            _ => panic!("expected MainMenu"),
        }
        assert!(events.contains(&TitleEvent::StartPressed));
    }

    #[test]
    fn no_save_data_starts_at_new_game() {
        let mut s = TitleSession::without_save_data();
        s.skip_fade_in();
        s.tick(TitleInput {
            start: true,
            ..Default::default()
        });
        match s.phase() {
            TitlePhase::MainMenu { cursor } => assert_eq!(cursor, 0),
            _ => panic!(),
        }
    }

    #[test]
    fn cursor_skips_continue_when_disabled() {
        // With only two rows (NewGame / Continue) and Continue disabled,
        // pressing Down wraps right back to NewGame - that's the
        // intended UX (don't land on a greyed-out row).
        let mut s = TitleSession::without_save_data();
        s.skip_fade_in();
        s.tick(TitleInput {
            start: true,
            ..Default::default()
        });
        s.tick(TitleInput {
            down: true,
            ..Default::default()
        });
        match s.phase() {
            TitlePhase::MainMenu { cursor } => assert_eq!(cursor, 0),
            _ => panic!(),
        }
    }

    #[test]
    fn confirm_emits_menu_confirmed_and_specific() {
        let mut s = TitleSession::new();
        s.skip_fade_in();
        s.tick(TitleInput {
            start: true,
            ..Default::default()
        });
        // Cursor at 1 = Continue.
        let events = s.tick(TitleInput {
            cross: true,
            ..Default::default()
        });
        assert!(events.contains(&TitleEvent::MenuConfirmed { row: 1 }));
        assert!(events.contains(&TitleEvent::ContinueSelected));
        assert_eq!(s.outcome(), Some(TitleOutcome::Continue));
    }

    #[test]
    fn circle_returns_to_press_start() {
        let mut s = TitleSession::new();
        s.skip_fade_in();
        s.tick(TitleInput {
            start: true,
            ..Default::default()
        });
        s.tick(TitleInput {
            circle: true,
            ..Default::default()
        });
        assert!(matches!(s.phase(), TitlePhase::PressStart { .. }));
    }

    #[test]
    fn cursor_wraps_around() {
        // Two-row menu (NewGame / Continue). Start press lands cursor
        // on Continue (1); Up goes to NewGame (0); Up again wraps back
        // to Continue (1).
        let mut s = TitleSession::new();
        s.skip_fade_in();
        s.tick(TitleInput {
            start: true,
            ..Default::default()
        });
        s.tick(TitleInput {
            up: true,
            ..Default::default()
        });
        match s.phase() {
            TitlePhase::MainMenu { cursor } => assert_eq!(cursor, 0),
            _ => panic!(),
        }
        s.tick(TitleInput {
            up: true,
            ..Default::default()
        });
        match s.phase() {
            TitlePhase::MainMenu { cursor } => assert_eq!(cursor, 1),
            _ => panic!(),
        }
    }

    #[test]
    fn outcome_new_game() {
        let mut s = TitleSession::new();
        s.skip_fade_in();
        s.tick(TitleInput {
            start: true,
            ..Default::default()
        });
        s.tick(TitleInput {
            up: true,
            ..Default::default()
        });
        s.tick(TitleInput {
            cross: true,
            ..Default::default()
        });
        assert_eq!(s.outcome(), Some(TitleOutcome::NewGame));
    }
}
