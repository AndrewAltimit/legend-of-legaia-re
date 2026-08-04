//! Party wipe -> the title screen.
//!
//! **Retail's game over is not a screen and not a menu.** It is two stores
//! and a hand-off, and the disc carries the same pair at four sites:
//!
//! ```text
//!   game_mode      (_DAT_8007B83C) = 0x16    ; 22 = CARD INIT
//!   title context  (_DAT_8007BB00) = 1       ; "run the CARD overlay as the title screen"
//! ```
//!
//! | Site | Where the pair lives | Reached by |
//! |---|---|---|
//! | `FUN_8003AEB0` `0x8003B5D0` / `0x8003B5E0` | inline in MAIN INIT's back-from-battle arm | a real party wipe |
//! | `FUN_8003C7EC` `0x8003C7F8` / `0x8003C808` | the standalone helper twin | field-VM op `4C EA` (scripted loss) |
//! | `FUN_801D84B4` `0x801D84B8` / `0x801D84CC` | a 7-instruction leaf, whole body | field/cutscene "back to title" |
//! | `0x801CF050` / `0x801CF048` | the STR overlay's attract exit | the demo movie ending |
//!
//! The wipe arm is selected by two branches in `FUN_8003AEB0`:
//! `0x8003B57C` `beq v0, zero, 0x8003B598` takes the wipe path when the
//! party-survived latch `DAT_8007BD60 & 0x80` is **clear**, and
//! `0x8003B5BC` `bne v1, zero, 0x8003B5F8` skips the hand-off again when
//! story-flag index 0 (the scripted-loss latch) is set. Falling through
//! both lands on the `0x16` store. The battle itself never forks: the
//! battle-exit mode selector `FUN_80046A20` writes mode 2 on every ending
//! and never reads the wipe cause `_DAT_8007BD2C`.
//!
//! On the far side the title overlay reads the context word at
//! `0x801DD968` (`lw a0, -0x4500(v0)`) and, when it is non-zero, enters at
//! sub-mode `0x11` `AttractDelay` (`0x801DD97C`) instead of its usual
//! `0x02`: a fade, then sub-mode `0x10` `AttractIdle` - the title screen
//! with its own NEW GAME / CONTINUE rows (cursor word `_DAT_8007B820`,
//! masked `& 1`, so retail's post-wipe choice is the *title's* two rows,
//! not a game-over panel's).
//!
//! There is therefore no Continue / Retry / Quit vocabulary anywhere on
//! this path, and no artwork: the only `GAME OVER` string on the disc is
//! in PROT 0902, the mode-18/19 overlay, which no retail path reaches
//! (see [`crate::mode::GameMode::GameOverInit`] and
//! `docs/subsystems/battle.md` § party wipe + the game-over overlay).
//! The port's former three-row chooser was an engine invention standing in
//! for a destination that was, at the time it was written, unpinned. It is
//! pinned now, so the invention is gone.
//!
//! What is left is the hand-off itself: a short hold on the frozen frame -
//! retail spends it streaming the menu overlay off the disc - and then the
//! title. Both hosts construct a [`GameOverSession`] off
//! [`crate::world::World::game_over`] (native `BootUiState::GameOver`, the
//! browser's post-battle overlay) and route [`GameOverOutcome::ReturnToTitle`]
//! into the same title session their boot path uses.

// REF: FUN_8003C7EC - the standalone form of the hand-off, and
// REF: FUN_801D84B4 - its 7-instruction twin, whose whole body is the pair.
// Tagged `REF:` rather than `PORT:` because `world::vm_hosts` carries the
// `PORT:` for `FUN_8003C7EC` (the field-VM `4C EA` arm) and the catalog
// counts occurrences - one address cannot have two port sites. This module
// is the state those two stores put the engine into, not the store site.

/// Frames the hand-off holds before the title takes over.
///
/// Retail's own number for the transition is the title overlay's
/// `AttractDelay` fade: sub-mode `0x11` drains the screen-fade level
/// `_DAT_8007BAB4` by `8 * frame_scalar` per frame (`0x801DDAEC`) and the
/// level is clamped to `0xFF` where it is consumed (`0x801DD38C` ->
/// `FUN_80024EE4(0, 1, level * 0x10101)`), so a full-black entry needs
/// `ceil(0xFF / 8)` = 32 drain frames before the title is at full ink.
///
/// It is the fade's length, not a timing claim about the whole transition:
/// retail also spends an unmeasured disc read on the menu overlay between
/// the wipe store and the overlay's first tick, and the port has no disc
/// read to spend. 32 frames is the part of the window that is pinned.
pub const TITLE_HANDOFF_FRAMES: u16 = 0xFFu16.div_ceil(8);

/// Phase of the hand-off.
///
/// Two phases, no input: retail offers the player nothing here. The pad is
/// deliberately not plumbed in - a session that accepted a button would be
/// the invention coming back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameOverPhase {
    /// Holding on the frozen frame. Retail spends this window loading the
    /// menu overlay (mode 22 `CARD INIT`) off the disc.
    Hold { frames_remaining: u16 },
    /// Hold drained; the host owes the title screen.
    Done,
}

/// The only thing a party wipe can resolve to.
///
/// A single variant on purpose: retail's wipe path has exactly one exit
/// store (`game_mode = 0x16`), and one store cannot express a choice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameOverOutcome {
    /// Mode 22 `CARD INIT` with `_DAT_8007BB00 = 1` - the title screen.
    ReturnToTitle,
}

/// The party-wipe hand-off, driven one [`GameOverSession::tick`] per frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameOverSession {
    phase: GameOverPhase,
}

impl GameOverSession {
    /// Start the hand-off with the retail-derived hold
    /// ([`TITLE_HANDOFF_FRAMES`]).
    pub fn new() -> Self {
        Self::with_hold(TITLE_HANDOFF_FRAMES)
    }

    /// Start the hand-off with an explicit hold, in frames. `0` resolves on
    /// the first tick.
    pub fn with_hold(frames: u16) -> Self {
        Self {
            phase: GameOverPhase::Hold {
                frames_remaining: frames,
            },
        }
    }

    pub fn phase(&self) -> GameOverPhase {
        self.phase
    }

    /// Frames left on the hold; `0` once it is done.
    pub fn frames_remaining(&self) -> u16 {
        match self.phase {
            GameOverPhase::Hold { frames_remaining } => frames_remaining,
            GameOverPhase::Done => 0,
        }
    }

    pub fn is_done(&self) -> bool {
        self.phase == GameOverPhase::Done
    }

    /// The resolved destination, or `None` while the hold is still running.
    /// Sticky once set.
    pub fn outcome(&self) -> Option<GameOverOutcome> {
        match self.phase {
            GameOverPhase::Done => Some(GameOverOutcome::ReturnToTitle),
            GameOverPhase::Hold { .. } => None,
        }
    }

    /// Advance one frame. Takes no input - see [`GameOverPhase`].
    pub fn tick(&mut self) {
        if let GameOverPhase::Hold { frames_remaining } = self.phase {
            self.phase = match frames_remaining.checked_sub(1) {
                Some(n) if n > 0 => GameOverPhase::Hold {
                    frames_remaining: n,
                },
                _ => GameOverPhase::Done,
            };
        }
    }
}

impl Default for GameOverSession {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hold_drains_to_done() {
        let mut s = GameOverSession::with_hold(3);
        for _ in 0..2 {
            s.tick();
            assert!(!s.is_done());
            assert_eq!(s.outcome(), None);
        }
        s.tick();
        assert!(s.is_done());
    }

    /// The one thing this module promises: a wipe has exactly one
    /// destination. If a second variant ever appears, whoever added it owes
    /// the disassembly for a second exit store.
    #[test]
    fn the_only_outcome_is_the_title() {
        let mut s = GameOverSession::with_hold(1);
        s.tick();
        assert_eq!(s.outcome(), Some(GameOverOutcome::ReturnToTitle));
    }

    /// A zero hold is legal and resolves on the first tick, so a host that
    /// wants retail's "no visible pause at all" reading can ask for it.
    #[test]
    fn zero_hold_resolves_immediately() {
        let mut s = GameOverSession::with_hold(0);
        assert!(!s.is_done());
        s.tick();
        assert_eq!(s.outcome(), Some(GameOverOutcome::ReturnToTitle));
    }

    /// 32, not 31: the drain runs *while* the level is `>= 1`, so a level of
    /// `0xFF` still needs a 32nd frame to take the remainder off. Truncating
    /// division is the easy way to be one frame short here.
    #[test]
    fn default_hold_is_the_traced_title_fade() {
        assert_eq!(GameOverSession::new().frames_remaining(), 32);
        assert_eq!(TITLE_HANDOFF_FRAMES, 32);
        assert!(u32::from(TITLE_HANDOFF_FRAMES) * 8 >= 0xFF);
    }
}
