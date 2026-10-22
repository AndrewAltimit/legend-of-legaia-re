//! The boot front end both play hosts run: the publisher logos, the title
//! card, its attract movie, and the title's hand-off.
//!
//! Retail's cold boot is `init.pak`'s logo sequencer (`FUN_801CEFD4`), whose
//! phase-3 arm branches on the entry word to `CARD INIT` (the front end) or
//! `CONFIG INIT` (the debug menu); the front end is the title overlay's
//! `FUN_801DD35C`, whose `AttractIdle` arm hands the screen to `fmv_id 0` and
//! whose confirm arms leave for the field launch (NEW GAME) or the card
//! manager (CONTINUE). [`FrontEnd`] is that chain as one state machine over
//! the engine's own sessions
//! ([`PublisherLogosSession`], [`TitleSession`]): which pad bits skip the
//! logos, the mode-table hand-off after them, the Continue-enable scan's
//! place, the title-row counter carried on the [`ModeSeat`], the order of the
//! attract hand-off against the title tick, and the skip edge that aborts
//! the movie.
//!
//! What stays a host's: the pixels (each host draws the sessions through the
//! shared `engine-ui` / `engine-screens` builders), the movie lane the
//! attract plays on ([`AttractLane`]), the save scan behind Continue's enable
//! bit (a closure, because the two hosts' card racks are different stores),
//! and acting on the [`TitleOutcome`]: New Game through
//! `BootSession::start_new_game` (the page's `play_new_game`), Continue into
//! the save-select (`SaveSelectSession::title_continue`), Options into the
//! pause menu's Options row.
//!
//! The headless soak harness drives the same object, which is how the
//! title -> New Game -> naming chain and the title -> Continue chain are
//! exercised without a window.

use legaia_engine_mode::mode::{GameMode, ModeSeat};
use legaia_engine_system::input::PadButton;
use legaia_engine_title::publisher_logos::PublisherLogosSession;
use legaia_engine_title::title::{TitleEvent, TitleInput, TitleOutcome, TitlePhase, TitleSession};

/// The title's pad decode off a just-pressed PSX pad word.
pub fn title_input(edge: u16) -> TitleInput {
    let hit = |b: PadButton| edge & b.mask() != 0;
    TitleInput {
        up: hit(PadButton::Up),
        down: hit(PadButton::Down),
        cross: hit(PadButton::Cross),
        start: hit(PadButton::Start),
        circle: hit(PadButton::Circle),
    }
}

/// Whether `edge` skips the publisher logos: Start or Cross.
pub fn logos_skip_edge(edge: u16) -> bool {
    edge & (PadButton::Start.mask() | PadButton::Cross.mask()) != 0
}

/// A title session as a shipped front end opens one
/// ([`TitleSession::for_front_end_at`]): Continue live exactly when
/// `any_save`, the menu on the seat's row counter (`_DAT_8007B820`, which
/// retail never resets), the attract countdown armed.
pub fn open_title(seat: &ModeSeat, any_save: bool) -> TitleSession {
    TitleSession::for_front_end_at(any_save, seat.title_row())
}

/// The movie lane a host plays the title's attract movie on.
///
/// The native window decodes it through its MDEC path; the browser page arms
/// it on its FMV lane. Both answer the same two questions.
pub trait AttractLane {
    /// The player pressed the movie's skip edge: end it.
    fn request_skip(&mut self);
    /// Whether the movie still owns the screen. Called once per frame while
    /// the attract plays; `false` returns the title to its menu.
    fn running(&mut self) -> bool;
}

/// Which screen the front end shows.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone)]
pub enum FrontEndScreen {
    /// The publisher logos (PROT 0895's sequencer).
    Logos(PublisherLogosSession),
    /// The title card.
    Title(TitleSession),
}

/// What one [`FrontEnd::step`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FrontEndStep {
    /// The logos still own the screen.
    Logos,
    /// The logos finished and the mode table raised the title card this
    /// step. `mode` is what the seat's hand-off returned: `CardInit` on a
    /// shipped boot; anything else is the debug-menu route, which the port
    /// has no screen for and lands on the title without Continue. The host
    /// starts the title theme.
    TitleOpened(GameMode),
    /// The title ticked; these are its events.
    Title(Vec<TitleEvent>),
    /// The attract countdown fired this step: the host starts `fmv_id` on
    /// its movie lane. The title is frozen until the lane ends.
    AttractStart(i16),
    /// The attract movie owns the screen.
    AttractPlaying,
    /// The attract movie ended this step (played out or skipped) and the
    /// title is back on its menu. The host ends the movie's audio.
    AttractEnded,
    /// The player confirmed a row. The title stays in place (a host may
    /// keep it as the save-select backdrop) until the host replaces it.
    Picked(TitleOutcome),
}

/// The boot front end ([module docs](self)).
#[derive(Debug, Clone)]
pub struct FrontEnd {
    screen: FrontEndScreen,
}

impl FrontEnd {
    /// A cold boot: the publisher logos when the disc's `init.pak` built an
    /// atlas, else straight to the title with `any_save`'s Continue bit -
    /// the same scan the logos' hand-off makes when they do play.
    pub fn cold_boot(seat: &ModeSeat, has_logos: bool, any_save: bool) -> Self {
        if has_logos {
            Self::logos()
        } else {
            Self::title(seat, any_save)
        }
    }

    /// The front end on its publisher-logo stage.
    pub fn logos() -> Self {
        Self {
            screen: FrontEndScreen::Logos(PublisherLogosSession::new()),
        }
    }

    /// The front end on its title card ([`open_title`]): a cold boot with no
    /// logos, a return from Options or a backed-out Continue, the post-wipe
    /// title.
    pub fn title(seat: &ModeSeat, any_save: bool) -> Self {
        Self {
            screen: FrontEndScreen::Title(open_title(seat, any_save)),
        }
    }

    /// The current screen.
    pub fn screen(&self) -> &FrontEndScreen {
        &self.screen
    }

    /// The logo session, while the logos play.
    pub fn logos_session(&self) -> Option<&PublisherLogosSession> {
        match &self.screen {
            FrontEndScreen::Logos(s) => Some(s),
            FrontEndScreen::Title(_) => None,
        }
    }

    /// The title session, once the card is up.
    pub fn title_session(&self) -> Option<&TitleSession> {
        match &self.screen {
            FrontEndScreen::Title(s) => Some(s),
            FrontEndScreen::Logos(_) => None,
        }
    }

    /// The title session, mutably.
    pub fn title_session_mut(&mut self) -> Option<&mut TitleSession> {
        match &mut self.screen {
            FrontEndScreen::Title(s) => Some(s),
            FrontEndScreen::Logos(_) => None,
        }
    }

    /// Release the title session (a Continue parks it as the save-select's
    /// backdrop).
    pub fn into_title(self) -> Option<TitleSession> {
        match self.screen {
            FrontEndScreen::Title(s) => Some(s),
            FrontEndScreen::Logos(_) => None,
        }
    }

    /// The outcome the title resolved to, if it has.
    pub fn outcome(&self) -> Option<TitleOutcome> {
        self.title_session().and_then(TitleSession::outcome)
    }

    /// Step the front end one retail frame on `edge`, the frame's
    /// just-pressed pad bits.
    ///
    /// `any_save` is asked only when the logos hand the screen to the title
    /// - the Continue-enable scan runs at that moment, not when the logos
    ///   started. `lane` is asked only while the attract movie plays.
    ///
    /// The title frame's order: a playing attract owns the frame (its skip
    /// edge, then whether the lane still runs) and the title does not tick;
    /// otherwise the title ticks, the row counter lands on the seat, and a
    /// countdown that fired this tick hands the movie to the lane the same
    /// frame - retail's `AttractIdle` arm writes the movie id and master
    /// mode `0x1A` in the frame its countdown underflows.
    // REF: FUN_801CEFD4, FUN_801DD35C
    pub fn step(
        &mut self,
        seat: &mut ModeSeat,
        edge: u16,
        any_save: impl FnOnce() -> bool,
        lane: &mut dyn AttractLane,
    ) -> FrontEndStep {
        match &mut self.screen {
            FrontEndScreen::Logos(logos) => {
                if logos_skip_edge(edge) {
                    logos.request_skip();
                }
                logos.tick();
                if !logos.is_done() {
                    return FrontEndStep::Logos;
                }
                // `init.pak`'s phase-3 arm: the core-state reset, then the
                // branch on the entry word (`ModeSeat::boot_handoff`).
                let mode = seat.boot_handoff();
                let any = match mode {
                    GameMode::CardInit => any_save(),
                    other => {
                        log::info!(
                            "front end: boot hand-off went to {other:?}; no engine screen owns \
                             it, opening the title"
                        );
                        false
                    }
                };
                self.screen = FrontEndScreen::Title(open_title(seat, any));
                FrontEndStep::TitleOpened(mode)
            }
            FrontEndScreen::Title(title) => {
                if let Some(step) = service_attract(title, edge, lane) {
                    return step;
                }
                let events = title.tick(title_input(edge));
                seat.set_title_row(title.row_counter());
                for ev in &events {
                    log_title_event(*ev, title.retail_submode());
                }
                if let Some(fmv_id) = title.attract_pending() {
                    title.mark_attract_started();
                    return FrontEndStep::AttractStart(fmv_id);
                }
                match title.outcome() {
                    Some(o) => FrontEndStep::Picked(o),
                    None => FrontEndStep::Title(events),
                }
            }
        }
    }
}

impl<S: legaia_engine_audio::AudioSink> crate::BootSession<S> {
    /// Step `front_end` against this session's mode seat
    /// ([`FrontEnd::step`]).
    pub fn step_front_end(
        &mut self,
        front_end: &mut FrontEnd,
        edge: u16,
        any_save: impl FnOnce() -> bool,
        lane: &mut dyn AttractLane,
    ) -> FrontEndStep {
        front_end.step(&mut self.mode_seat, edge, any_save, lane)
    }

    /// The title card on this session's seat ([`FrontEnd::title`]).
    pub fn front_end_title(&self, any_save: bool) -> FrontEnd {
        FrontEnd::title(&self.mode_seat, any_save)
    }

    /// Score the title card: the global-pool title theme
    /// ([`legaia_engine_system::music_labels::TITLE_THEME_BGM_ID`]). The
    /// director suppresses a same-id restart, so re-opening the title from
    /// Options or a backed-out Continue never drops the playhead. `false`
    /// when the bank or the director is missing (the title stays silent).
    pub fn start_title_bgm(&mut self) -> bool {
        self.start_global_bgm(legaia_engine_system::music_labels::TITLE_THEME_BGM_ID)
    }
}

impl FrontEnd {
    /// The attract half of a title frame on its own: while the movie plays,
    /// its skip edge and whether the lane still runs; `None` when no attract
    /// is playing. [`Self::step`] runs it first every title frame. A host
    /// whose lane can settle the movie the frame it is armed (the browser
    /// page's lane finishes an unplayable movie at once) calls it again
    /// right after arming, so that movie never outlives its frame.
    pub fn service_attract(
        &mut self,
        edge: u16,
        lane: &mut dyn AttractLane,
    ) -> Option<FrontEndStep> {
        match &mut self.screen {
            FrontEndScreen::Title(title) => service_attract(title, edge, lane),
            FrontEndScreen::Logos(_) => None,
        }
    }
}

fn service_attract(
    title: &mut TitleSession,
    edge: u16,
    lane: &mut dyn AttractLane,
) -> Option<FrontEndStep> {
    if !title.attract_playing() {
        return None;
    }
    let fmv_id = match title.phase() {
        TitlePhase::Attract { fmv_id, .. } => fmv_id,
        _ => legaia_engine_title::title_overlay::ATTRACT_FMV_ID,
    };
    // Retail's abort: a face button / Select on `fmv_id 0`
    // (`FUN_801CF098`'s `_DAT_8007B850 & 0x1F0` test).
    if legaia_engine_system::movie::cutscene::fmv_skip_edge_hit(fmv_id, edge) {
        lane.request_skip();
    }
    if lane.running() {
        return Some(FrontEndStep::AttractPlaying);
    }
    title.finish_attract();
    Some(FrontEndStep::AttractEnded)
}

/// The one log line per title transition both hosts print. The retail
/// sub-mode is what a capture reads at `FUN_801DD35C`'s state word: NEW GAME
/// moves to `0x16` (`LaunchFade`), CONTINUE to `0x18` (`ContinueFadeIn`).
fn log_title_event(ev: TitleEvent, submode: u8) {
    match ev {
        TitleEvent::NewGameSelected => {
            log::info!("title: New Game (retail sub-mode 0x{submode:02X})")
        }
        TitleEvent::ContinueSelected => {
            log::info!("title: Continue (retail sub-mode 0x{submode:02X})")
        }
        TitleEvent::AttractTimeout => {
            log::info!("title: attract countdown fired (retail sub-mode 0x{submode:02X})")
        }
        TitleEvent::OptionsSelected => log::info!("title: Options"),
        _ => {}
    }
}

/// A lane with no movie: the attract ends the frame it starts, which is
/// what a host without STR playback (or a headless driver) does.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoMovieLane;

impl AttractLane for NoMovieLane {
    fn request_skip(&mut self) {}
    fn running(&mut self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const START: u16 = 0x0008;
    const CROSS: u16 = 0x4000;
    const DOWN: u16 = 0x0040;

    /// Drive the front end with `pad` until `stop` holds or `limit` frames.
    fn drive(
        fe: &mut FrontEnd,
        seat: &mut ModeSeat,
        limit: u32,
        mut pad: impl FnMut(u32) -> u16,
        mut stop: impl FnMut(&FrontEndStep) -> bool,
    ) -> Option<FrontEndStep> {
        for f in 0..limit {
            let step = fe.step(seat, pad(f), || true, &mut NoMovieLane);
            if stop(&step) {
                return Some(step);
            }
        }
        None
    }

    #[test]
    fn logos_skip_on_start_and_hand_off_through_the_seat() {
        let mut seat = ModeSeat::new_at_boot();
        let mut fe = FrontEnd::logos();
        let opened = drive(
            &mut fe,
            &mut seat,
            600,
            |f| if f == 3 { START } else { 0 },
            |s| matches!(s, FrontEndStep::TitleOpened(_)),
        );
        assert_eq!(
            opened,
            Some(FrontEndStep::TitleOpened(GameMode::CardInit)),
            "a Start edge skips the logos to the title, through CARD INIT"
        );
        assert!(fe.title_session().is_some());
    }

    #[test]
    fn logos_play_out_without_input() {
        let mut seat = ModeSeat::new_at_boot();
        let mut fe = FrontEnd::logos();
        let opened = drive(
            &mut fe,
            &mut seat,
            20_000,
            |_| 0,
            |s| matches!(s, FrontEndStep::TitleOpened(_)),
        );
        assert!(opened.is_some(), "the logo sequencer ends on its own");
    }

    /// Alternate Start and Cross until the title resolves: New Game is row 0.
    #[test]
    fn title_new_game_off_start_then_cross() {
        let mut seat = ModeSeat::new_at_boot();
        let mut fe = FrontEnd::title(&seat, true);
        let picked = drive(
            &mut fe,
            &mut seat,
            2_000,
            |f| match f % 8 {
                0 => START,
                4 => CROSS,
                _ => 0,
            },
            |s| matches!(s, FrontEndStep::Picked(_)),
        );
        assert_eq!(picked, Some(FrontEndStep::Picked(TitleOutcome::NewGame)));
        assert_eq!(fe.outcome(), Some(TitleOutcome::NewGame));
    }

    /// Down onto CONTINUE, then Cross; the row counter lands on the seat.
    #[test]
    fn title_continue_carries_the_row_to_the_seat() {
        let mut seat = ModeSeat::new_at_boot();
        let mut fe = FrontEnd::title(&seat, true);
        let mut menu_open = false;
        let mut pressed_down = false;
        let mut picked = None;
        for f in 0..2_000u32 {
            let pad = if !menu_open {
                if f % 8 == 0 { START } else { 0 }
            } else if !pressed_down {
                pressed_down = true;
                DOWN
            } else if f % 8 == 0 {
                CROSS
            } else {
                0
            };
            let step = fe.step(&mut seat, pad, || true, &mut NoMovieLane);
            if let Some(TitlePhase::MainMenu { .. }) = fe.title_session().map(|t| t.phase()) {
                menu_open = true;
            }
            if let FrontEndStep::Picked(o) = step {
                picked = Some(o);
                break;
            }
        }
        assert_eq!(picked, Some(TitleOutcome::Continue));
        assert_eq!(seat.title_row(), 1);
        // A later title opens on the row the seat carries.
        let again = FrontEnd::title(&seat, true);
        assert_eq!(again.title_session().unwrap().row_counter(), 1);
    }

    /// With no save, Continue is greyed and the cursor folds to New Game.
    #[test]
    fn no_save_title_opens_on_new_game() {
        let mut seat = ModeSeat::new_at_boot();
        seat.set_title_row(1);
        let fe = FrontEnd::title(&seat, false);
        assert_eq!(fe.title_session().unwrap().row_counter(), 0);
    }

    /// A lane that plays for `frames` frames and records skips.
    struct CountLane {
        frames: u32,
        skipped: bool,
    }

    impl AttractLane for CountLane {
        fn request_skip(&mut self) {
            self.skipped = true;
        }
        fn running(&mut self) -> bool {
            if self.skipped || self.frames == 0 {
                return false;
            }
            self.frames -= 1;
            true
        }
    }

    #[test]
    fn attract_starts_the_frame_it_fires_and_freezes_the_title() {
        let mut seat = ModeSeat::new_at_boot();
        let mut fe = FrontEnd::title(&seat, true);
        let mut lane = CountLane {
            frames: 5,
            skipped: false,
        };
        let mut started = None;
        for f in 0..10_000u32 {
            match fe.step(&mut seat, 0, || true, &mut lane) {
                FrontEndStep::AttractStart(id) => {
                    started = Some((f, id));
                    break;
                }
                FrontEndStep::Title(_) => {}
                other => panic!("unexpected {other:?}"),
            }
        }
        let (_, id) = started.expect("the idle countdown fires");
        assert_eq!(id, legaia_engine_title::title_overlay::ATTRACT_FMV_ID);
        let mut playing = 0;
        loop {
            match fe.step(&mut seat, 0, || true, &mut lane) {
                FrontEndStep::AttractPlaying => playing += 1,
                FrontEndStep::AttractEnded => break,
                other => panic!("unexpected {other:?}"),
            }
        }
        assert_eq!(playing, 5);
        assert!(!fe.title_session().unwrap().attract_playing());
    }

    #[test]
    fn attract_skip_edge_ends_the_movie() {
        let mut seat = ModeSeat::new_at_boot();
        let mut fe = FrontEnd::title(&seat, true);
        let mut lane = CountLane {
            frames: 1_000,
            skipped: false,
        };
        while !matches!(
            fe.step(&mut seat, 0, || true, &mut lane),
            FrontEndStep::AttractStart(_)
        ) {}
        assert_eq!(
            fe.step(&mut seat, 0, || true, &mut lane),
            FrontEndStep::AttractPlaying
        );
        assert_eq!(
            fe.step(&mut seat, CROSS, || true, &mut lane),
            FrontEndStep::AttractEnded
        );
        assert!(lane.skipped);
    }
}
