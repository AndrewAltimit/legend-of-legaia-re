//! What a movie does to the score - one policy both hosts consult.
//!
//! # Retail
//!
//! The movie path itself never touches the sequencer. Master mode `0x1A`'s
//! entry (`FUN_80025FB4`), its dispatch (`FUN_801CEA3C`), the play loop
//! (`FUN_801CF098`) and the main loop's mode-change arm (`FUN_80015E90`,
//! `0x800161B8..0x80016200`, and the pass `FUN_80016230` it calls) issue no
//! BGM-slot call and no `SsSeq*` call; the play loop's only sound calls open
//! and close the SPU CD input (`FUN_800643C4` / `FUN_80062A0C`). The
//! sequencer ticks from the root-counter callback (the word at `0x8007A910`
//! names `FUN_80062F98`), so a mid-game movie leaves the score in whatever
//! state the script put it: the nine field-VM triggers are preceded by their
//! scripts' own op-`0x35` words, which differ scene to scene. Whether a track
//! the script left running is *audible* under the movie's XA has not been
//! captured.
//!
//! The one movie that does act on the score is the title attract. Its
//! underflow arm in the title tick releases the BGM slot - `FUN_800266E0` +
//! `FUN_80026520` on `0x8007052C` at `0x801DDD7C` / `0x801DDD84`, behind the
//! entry word `_DAT_8007BB00 != 0`, which the boot image always raises - and
//! the return runs `CARD INIT` (`FUN_8002574C`), which streams the title
//! theme again (raw TOC `0x41F` into category 1 at `0x800258CC..0x80025934`)
//! and re-attaches the slot (`FUN_80026478` at `0x80025948`). So the theme
//! comes back from its first beat, not from where the attract cut it.
//!
//! # The port
//!
//! Hosts decode a movie's XA onto the same mixer as the BGM, and do not wait
//! on a capture to decide whether the two layer. [`MovieScore`] therefore
//! ducks the score under a movie that stages audio - and only then - and on
//! the movie's end gives back exactly what it took: a track the script had
//! already paused stays paused, a movie with no audio track leaves the score
//! alone, and nothing is reopened twice. The duck is the sequencer gate the
//! directors already treat as their pause latch, so there is one latch, not
//! one per host. The attract follows retail: the score is released when the
//! movie starts and the title theme restarts when it ends.

/// Which caller armed the movie. Retail treats the two differently: only the
/// title attract releases the score.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MovieOrigin {
    /// The title screen's idle countdown (`fmv_id 0`).
    Attract,
    /// A field-VM `0x4C 0xE2` trigger.
    Cutscene,
}

/// What a host does to its audio when a movie ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MovieEnd {
    /// Reopen the sequencer gate: this movie closed it.
    pub reopen_gate: bool,
    /// Start the title theme from its first beat: the attract released it.
    pub restart_title_theme: bool,
}

/// The movie-audio policy both hosts consult (see the module docs).
///
/// A host calls [`Self::on_movie_start`] when it arms a movie,
/// [`Self::on_movie_audio`] when it puts the movie's XA track on the mixer,
/// and [`Self::on_movie_end`] on **every** way a movie can end - played out,
/// skipped, aborted, cut slot, undecodable, never installed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MovieScore {
    origin: Option<MovieOrigin>,
    ducked: bool,
}

impl MovieScore {
    /// A fresh policy with no movie in flight.
    pub const fn new() -> Self {
        Self {
            origin: None,
            ducked: false,
        }
    }

    /// A movie was armed. Returns `true` when the host has to **stop** the
    /// score now (`BgmDirector::stop`): the title attract's release of the
    /// BGM slot.
    ///
    /// A duck a previous movie still holds is kept (its gate is still
    /// closed), so the end of this one gives it back.
    // REF: FUN_801DD35C (`0x801DDD64..0x801DDD88`, the attract release)
    pub fn on_movie_start(&mut self, origin: MovieOrigin) -> bool {
        self.origin = Some(origin);
        origin == MovieOrigin::Attract
    }

    /// The movie's audio track went onto the mixer. `gate_closed` is the
    /// sequencer gate as it stands. Returns `true` when the host has to close
    /// it - only when it is open, so a score the script already paused is
    /// not claimed by the movie and is not reopened by its end.
    pub fn on_movie_audio(&mut self, gate_closed: bool) -> bool {
        if self.ducked || gate_closed {
            return false;
        }
        self.ducked = true;
        true
    }

    /// The movie ended, however it ended. Consumes the in-flight state, so a
    /// second call (two teardown paths meeting on one frame) asks for
    /// nothing.
    // REF: FUN_8002574C (`0x800258CC..0x80025948`, the title theme restart)
    pub fn on_movie_end(&mut self) -> MovieEnd {
        let end = MovieEnd {
            reopen_gate: self.ducked,
            restart_title_theme: self.origin == Some(MovieOrigin::Attract),
        };
        *self = Self::new();
        end
    }

    /// Whether a movie is armed and not yet ended.
    pub fn in_flight(&self) -> bool {
        self.origin.is_some()
    }

    /// Whether this movie is holding the sequencer gate closed.
    pub fn ducked(&self) -> bool {
        self.ducked
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A cutscene movie with audio over a playing score: duck, then give it
    /// back once.
    #[test]
    fn a_movie_with_audio_ducks_a_playing_score_and_returns_it_once() {
        let mut m = MovieScore::new();
        assert!(
            !m.on_movie_start(MovieOrigin::Cutscene),
            "a cutscene keeps the score"
        );
        assert!(
            m.on_movie_audio(false),
            "an open gate is closed under the movie"
        );
        assert!(!m.on_movie_audio(true), "the duck is taken once");
        let end = m.on_movie_end();
        assert!(end.reopen_gate);
        assert!(!end.restart_title_theme);
        assert_eq!(
            m.on_movie_end(),
            MovieEnd::default(),
            "a second end asks nothing"
        );
    }

    /// The script paused the score before the trigger: the movie does not
    /// claim the pause, so its end does not undo it.
    #[test]
    fn a_script_paused_score_stays_paused_across_a_movie() {
        let mut m = MovieScore::new();
        m.on_movie_start(MovieOrigin::Cutscene);
        assert!(!m.on_movie_audio(true));
        assert!(!m.on_movie_end().reopen_gate);
    }

    /// A movie that never staged audio (no disc XA, a cut slot, a page that
    /// never installed it) touches nothing, played or not.
    #[test]
    fn a_movie_without_audio_leaves_the_score_alone() {
        let mut m = MovieScore::new();
        m.on_movie_start(MovieOrigin::Cutscene);
        assert_eq!(m.on_movie_end(), MovieEnd::default());
    }

    /// The attract releases the score at its start and restarts the title
    /// theme at its end, whether or not the movie played.
    #[test]
    fn the_attract_releases_the_score_and_restarts_the_theme() {
        let mut m = MovieScore::new();
        assert!(m.on_movie_start(MovieOrigin::Attract));
        // The stop reopened the gate; the attract's XA then ducks nothing
        // audible, and the end reopens it for the restarted theme.
        assert!(m.on_movie_audio(false));
        let end = m.on_movie_end();
        assert!(end.reopen_gate && end.restart_title_theme);

        let mut unplayed = MovieScore::new();
        unplayed.on_movie_start(MovieOrigin::Attract);
        let end = unplayed.on_movie_end();
        assert!(!end.reopen_gate && end.restart_title_theme);
    }

    /// Arming a movie over one whose end never ran keeps the gate the first
    /// one closed, so the gate is still given back exactly once.
    #[test]
    fn a_movie_armed_over_an_unended_one_keeps_its_duck() {
        let mut m = MovieScore::new();
        m.on_movie_start(MovieOrigin::Cutscene);
        assert!(m.on_movie_audio(false));
        m.on_movie_start(MovieOrigin::Cutscene);
        assert!(m.ducked() && m.in_flight());
        assert!(!m.on_movie_audio(true));
        assert!(m.on_movie_end().reopen_gate);
    }
}
