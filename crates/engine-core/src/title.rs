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
//!   staging. Retail's own entry is `Init` -> `0x11` `AttractDelay` ->
//!   `0x10` `AttractIdle`: the sub-mode **word** is `0x801F0204`
//!   (`0x801DD920` / `0x801DD97C` are the *instruction* addresses of the
//!   two stores), and the `0x02` arm is unreachable on retail because
//!   `init.pak` raises the entry word `_DAT_8007BB00` at `0x801CEB84`
//!   and the tick's shared epilogue rewrites `0x02` to `0x10` again at
//!   `0x801DFEF8`. The "`0x02 -> 0x14` is the default graph" reading is
//!   falsified; the executable graph is
//!   [`legaia_engine_vm::title_overlay::TitleTickState`].
//! - `continue_enabled` skipping the CONTINUE row has no retail
//!   counterpart - retail always lets the row be picked and lets the
//!   save screen say "No data". It is a port guard, applied on top of the
//!   retail step.
//! - The attract countdown is **opt-in per host**
//!   ([`TitleSession::attract_enabled`]), because a host with no movie
//!   destination would freeze input for the last sixteen frames of every
//!   idle period and then do nothing. Both shipped hosts now set it: the
//!   native window plays `fmv_id 0` through its windowed MDEC path, and
//!   the browser play page enters the same [`TitlePhase::Attract`] and
//!   finishes it immediately (the play page has no STR/MDEC playback -
//!   the deviation its `play_cutscene` module already documents). With
//!   the flag off the session is bit-identical to a session with no
//!   countdown at all.

/// Phase of the title state machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TitlePhase {
    FadeIn {
        frames_remaining: u16,
    },
    PressStart {
        blink_phase: u16,
    },
    MainMenu {
        cursor: u8,
    },
    /// The attract countdown underflowed and the screen belongs to the
    /// opening movie. Retail's `AttractIdle` arm zeroes `_DAT_8007BA78`
    /// and writes master game mode `0x1A` (`0x801DDCE8` / `0x801DDCF0`),
    /// so `fmv_id` is always `0`. `playing` is set once a host has picked
    /// the movie up; [`TitleSession::finish_attract`] returns to the menu
    /// the way retail's re-entry does (`Init` with the entry word at
    /// `2`, which still takes the `0x11` -> `0x10` arm).
    Attract {
        fmv_id: i16,
        playing: bool,
    },
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
    /// Whether the attract countdown may fire. Off by default and set by
    /// each host for itself; both shipped hosts set it. See the module
    /// docs for why it is not on by default.
    pub attract_enabled: bool,
    /// Retail's own front-end tick state, stepped alongside the session so
    /// the host can ask which sub-mode of `FUN_801DD35C` the title is in.
    ///
    /// It is seeded the way retail seeds it - by **raising the entry word**
    /// `_DAT_8007BB00` and running `Init`, not by writing a sub-mode - so
    /// the boot lands in `0x11` `AttractDelay` and then `0x10`
    /// `AttractIdle`, which is what a cold-boot capture sees.
    tick: legaia_engine_vm::title_overlay::TitleTickState,
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
            tick: Self::cold_boot_tick(),
        }
    }

    /// Run retail's front-end entry: `Init` with the boot entry word raised,
    /// then the `AttractDelay` hand-off, leaving the state in `AttractIdle`.
    ///
    /// This is the whole point of keeping the tick state around - the entry
    /// sub-mode is *derived* from `_DAT_8007BB00` by executing `Init`
    /// (`0x801DD820`), never written by hand, so the port cannot drift into
    /// the `0x02` graph retail's `init.pak` makes unreachable.
    fn cold_boot_tick() -> legaia_engine_vm::title_overlay::TitleTickState {
        use legaia_engine_vm::title_overlay::{
            ATTRACT_DELAY_SEED, TitleCardStatus, TitleTickPad, TitleTickState,
        };
        let mut tick = TitleTickState::cold_boot();
        // Init -> AttractDelay, then the hold the SCUS stager seeded
        // (`0x100` at `0x8002579C`, spent at 8 a frame) -> AttractIdle.
        for _ in 0..=(2 + ATTRACT_DELAY_SEED / 8) {
            let _ = tick.step(TitleTickPad::from_edge(0), TitleCardStatus::default());
        }
        tick
    }

    /// The retail sub-mode of `FUN_801DD35C` this session's title is in.
    ///
    /// A cold boot reports `0x10` (`AttractIdle`) - never `0x02`, whose
    /// handler `init.pak`'s entry word makes unreachable. Confirming NEW GAME
    /// moves it to `0x16` (`LaunchFade`) and CONTINUE to `0x18`
    /// (`ContinueFadeIn`), the two rows' real retail destinations.
    pub fn retail_submode(&self) -> u8 {
        self.tick.submode
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
                            // The retail graph picks the row's destination,
                            // not this session: `AttractIdle`'s confirm arm
                            // sends row 0 to `0x16` `LaunchFade`
                            // (`0x801DDC3C`) and row 1 to `0x18`
                            // `ContinueFadeIn` (`0x801DDC5C`).
                            self.tick.row_counter = row as i32;
                            let _ = self.tick.step(
                                legaia_engine_vm::title_overlay::TitleTickPad::from_edge(
                                    legaia_engine_vm::title_overlay::PADMASK_START_L1_CROSS,
                                ),
                                legaia_engine_vm::title_overlay::TitleCardStatus::default(),
                            );
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
                                // Retail's arm hands the screen to the
                                // opening movie: `_DAT_8007BA78 = 0` then
                                // master game mode `0x1A`.
                                self.phase = TitlePhase::Attract {
                                    fmv_id: legaia_engine_vm::title_overlay::ATTRACT_FMV_ID,
                                    playing: false,
                                };
                            }
                            // Re-arm either way; with the flag off there
                            // is nothing to hand the screen to and the
                            // session must not sit under the input freeze.
                            self.menu.countdown =
                                legaia_engine_vm::title_overlay::COUNTDOWN_RESET_VALUE as i32;
                        }
                    }
                }
            }
            // The movie owns the screen; the session freezes until the
            // host calls `finish_attract`.
            TitlePhase::Attract { .. } => {}
            TitlePhase::Done(_) => {}
        }
        events
    }

    /// The `fmv_id` waiting for a host to pick up, or `None` when the
    /// session is not in [`TitlePhase::Attract`] or a host already took
    /// it. Always [`ATTRACT_FMV_ID`] on retail.
    ///
    /// [`ATTRACT_FMV_ID`]: legaia_engine_vm::title_overlay::ATTRACT_FMV_ID
    pub fn attract_pending(&self) -> Option<i16> {
        match self.phase {
            TitlePhase::Attract {
                fmv_id,
                playing: false,
            } => Some(fmv_id),
            _ => None,
        }
    }

    /// Whether a host has picked the attract movie up and not yet
    /// finished it. Hosts poll this to know when their own playback
    /// drained and it is time to call [`Self::finish_attract`].
    pub fn attract_playing(&self) -> bool {
        matches!(self.phase, TitlePhase::Attract { playing: true, .. })
    }

    /// Claim the pending attract movie. A host calls this once it has
    /// started (or decided it cannot start) playback, so the session
    /// stops re-offering the same `fmv_id` every frame.
    pub fn mark_attract_started(&mut self) {
        if let TitlePhase::Attract { fmv_id, .. } = self.phase {
            self.phase = TitlePhase::Attract {
                fmv_id,
                playing: true,
            };
        }
    }

    /// Return from the attract movie to the menu, the way retail does:
    /// the front-end re-enters through `Init` with the entry word at
    /// `2`, which still takes the `0x11` -> `0x10` arm, so the player
    /// lands back on the live menu with the countdown re-armed and the
    /// cursor on row 0.
    pub fn finish_attract(&mut self) {
        if matches!(self.phase, TitlePhase::Attract { .. }) {
            self.menu = legaia_engine_vm::title_overlay::TitleMenuState::new();
            // Retail re-enters the front-end through `Init` with the entry
            // word still non-zero, so the sub-mode walks `0x11` -> `0x10`
            // again rather than resuming where the movie interrupted it.
            self.tick = Self::cold_boot_tick();
            self.phase = TitlePhase::MainMenu { cursor: 0 };
        }
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
