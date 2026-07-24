//! The Noa dance minigame's **setumei (how-to) tutorial script** - the Disco
//! King actor's own per-frame state machine (`FUN_801D0750`).
//!
//! The tutorial is not a separate mode: it is one actor handler running beside
//! the ordinary dance session. Its whole state is the `i16` at the actor's
//! `+0x9C`, and every frame the handler switches on it, prints that step's
//! caption lines, and - for the talky steps - waits for a face button before
//! advancing. Two steps instead run a countdown, and one is the practice step
//! that watches the live session's score.
//!
//! The caption **strings** are overlay rodata (Sony text) and are not modeled;
//! [`TutorialStep::captions`] gives the line count and the screen positions
//! retail draws them at, so a host resolves the text from the user's own disc.
//!
//! Dispatch is a 19-entry jump table (`0x801CEEE8`, guarded by an unsigned
//! `state < 0x13`); anything outside it falls straight through to the shared
//! actor dispatcher without touching the state. Three tails share the store:
//! the common `break` path stores `state + 1`, one case stores a literal
//! (`0` -> `5`, the "no thanks" branch), and one stores `state + 2`.
//!
//! See [`docs/subsystems/minigame-dance.md`](../../../docs/subsystems/minigame-dance.md);
//! dump `overlay_dance_801d0750.txt`.

/// Screen x of a caption line (retail's `s1`).
pub const CAPTION_X: i16 = 8;
/// Screen x of the two menu options on the opening prompt (`s1 + 0x40`).
pub const CAPTION_OPTION_X: i16 = 0x48;
/// Screen y of the first caption line (retail's `s2`).
pub const CAPTION_Y0: i16 = 0x78;
/// Vertical pitch between caption lines.
pub const CAPTION_PITCH: i16 = 0x10;
/// Screen x of the option cursor (`FUN_8002C488`'s first argument, `8 | 0x20`).
pub const CURSOR_X: i16 = 0x28;
/// Cursor sprite id (`FUN_8002C488`'s third argument).
pub const CURSOR_SPRITE: u16 = 0x4E;
/// Screen y of the first option row the cursor can sit on
/// (`CAPTION_Y0 + 0x20`).
pub const CURSOR_Y0: i16 = CAPTION_Y0 + 0x20;

/// Pad mask the talky steps advance on - the four face buttons
/// (`_DAT_8007B874 & 0xF0`).
pub const PAD_ADVANCE: u16 = 0xF0;
/// Pad bit that moves the opening prompt's cursor up (`0x1000`).
pub const PAD_CURSOR_PREV: u16 = 0x1000;
/// Pad bit that moves it down (`0x4000`).
pub const PAD_CURSOR_NEXT: u16 = 0x4000;
/// Cue id the cursor move fires (`_DAT_8007B6D8 = 0x21`).
pub const CUE_CURSOR_MOVE: u16 = 0x21;
/// Cue id a confirmed advance fires (`_DAT_8007B6D8 = 0x20`).
pub const CUE_CONFIRM: u16 = 0x20;

/// Score the practice steps wait for before moving on (`DAT_801D5150`
/// compared `< 900` / `899 <`).
pub const PRACTICE_SCORE_GATE: i32 = 900;
/// Frames the two hand-off steps park on their countdown
/// (`DAT_801D6080 = 0x3C`).
pub const HANDOFF_FRAMES: i32 = 0x3C;
/// Frames the scold caption stays up before the step re-arms
/// (`DAT_801D5140` wraps past `0x20`).
pub const SCOLD_FRAMES: i32 = 0x20;

/// Highest state the jump table covers. `state >= TUTORIAL_STATES` is the
/// unsigned default: no caption, no advance.
pub const TUTORIAL_STATES: i16 = 0x13;

/// What one tutorial state is, structurally.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TutorialStep {
    /// The opening `Do you wanna learn how to dance?` prompt: two caption
    /// lines plus two option rows, an up/down cursor, and a face-button
    /// confirm that branches on which option the cursor sits on.
    Prompt,
    /// A caption-only step: draw `lines` lines and advance to `state + advance`
    /// on any face button.
    Talk { lines: u8, advance: i16 },
    /// A frame countdown (`DAT_801D6080`), advancing when it goes negative.
    /// The pad is ignored.
    Countdown,
    /// The first free-dance step (`7`): the player dances while the script
    /// clears the groove gauge every frame and captions the run off the
    /// dancer's level. Advances on the same [`PRACTICE_SCORE_GATE`] as
    /// [`Practice`](TutorialStep::Practice). Its scold timer **counts up** and
    /// wraps at [`SCOLD_FRAMES`], where the practice step's counts down.
    Warmup,
    /// The practice step (`0xD`): the player dances, and the step advances
    /// once the session score passes [`PRACTICE_SCORE_GATE`]. Draws the praise
    /// / scold captions off the last triangle spend instead of a fixed line
    /// list.
    Practice,
    /// A jump-table slot with no body (`0x12`), or a state outside the table -
    /// the handler does nothing but re-enter the actor dispatcher.
    Idle,
}

// PORT: FUN_801d0750 (the tutorial script's jump-table shape)
// NOT WIRED: the tutorial is a *dance-hall actor* handler. The port models the
// dance as a rules session ([`crate::dance::DanceGame`]) with no Disco King
// actor, no caption renderer bound to the overlay's own string table, and no
// pre-song phase for the prompt to sit in front of. Wiring it needs a dance
// presentation host that owns the hall's actors and can resolve the overlay
// string rows.
/// Classify a tutorial state.
///
/// The line counts and advances are read straight off the switch arms:
/// state `0` is the prompt; `5` skips a step (`state + 2`, hopping the
/// alternate acknowledgement at `6`); `7` and `0xD` are the two free-dance
/// steps; `0xC` and `0x11` are countdowns; `0x12` is a jump-table slot with no
/// body; everything else is a caption step advancing by one.
pub fn tutorial_step(state: i16) -> TutorialStep {
    if !(0..TUTORIAL_STATES).contains(&state) {
        return TutorialStep::Idle;
    }
    match state {
        0 => TutorialStep::Prompt,
        7 => TutorialStep::Warmup,
        0xC | 0x11 => TutorialStep::Countdown,
        0xD => TutorialStep::Practice,
        // `5` is the last line before the first free dance, and the one arm
        // that stores `state + 2`: it hops the acknowledgement at `6`, which
        // the script only reaches through the prompt's "no thanks" branch.
        5 => TutorialStep::Talk {
            lines: 2,
            advance: 2,
        },
        1 | 3 | 4 | 6 | 8 | 9 | 0xA | 0xE | 0xF | 0x10 => TutorialStep::Talk {
            lines: 3,
            advance: 1,
        },
        0x12 => TutorialStep::Idle,
        _ => TutorialStep::Talk {
            lines: 2,
            advance: 1,
        },
    }
}

/// Screen position of caption line `i` of an ordinary step.
pub fn caption_pos(i: u8) -> (i16, i16) {
    (CAPTION_X, CAPTION_Y0 + CAPTION_PITCH * i as i16)
}

/// Screen position of option row `i` of the opening prompt - the two rows sit
/// below the two prompt lines and are indented to [`CAPTION_OPTION_X`].
pub fn option_pos(i: u8) -> (i16, i16) {
    (
        CAPTION_OPTION_X,
        CAPTION_Y0 + CAPTION_PITCH * (2 + i as i16),
    )
}

/// Where the option cursor is drawn for cursor row `cursor`.
pub fn cursor_pos(cursor: u32) -> (i16, i16) {
    (CURSOR_X, CURSOR_Y0 + CAPTION_PITCH * (cursor & 1) as i16)
}

/// One frame of the opening prompt's cursor + confirm handling.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PromptFrame {
    /// The cursor row after this frame's pad, already masked to `0 ..= 1`.
    pub cursor: u32,
    /// Cue the frame fires, if any.
    pub cue: Option<u16>,
    /// The state to store, if the frame confirms.
    pub next_state: Option<i16>,
}

// PORT: FUN_801d0750 case 0 (the prompt's cursor + confirm)
// NOT WIRED: same missing host as [`tutorial_step`] - there is no Disco King
// actor for the prompt to live on and no pre-song phase in the port's dance
// session.
/// Step the opening prompt.
///
/// `pad` is the retail pad word `_DAT_8007B874`. Left ([`PAD_CURSOR_PREV`])
/// decrements and Right ([`PAD_CURSOR_NEXT`]) increments - retail moves this
/// two-row menu on the **horizontal** axis, not the vertical one - each firing
/// [`CUE_CURSOR_MOVE`]; both may fire in one frame, cancelling out but still
/// leaving the cue armed. The row is then masked to one bit, so it wraps
/// rather than clamping.
///
/// A face button ([`PAD_ADVANCE`]) fires [`CUE_CONFIRM`] and confirms: row `0`
/// (yes) advances to state `1`, row `1` (no) jumps straight to state `5`.
pub fn prompt_frame(cursor: u32, pad: u16) -> PromptFrame {
    let mut c = cursor as i64;
    let mut cue = None;
    if pad & PAD_CURSOR_PREV != 0 {
        c -= 1;
        cue = Some(CUE_CURSOR_MOVE);
    }
    if pad & PAD_CURSOR_NEXT != 0 {
        c += 1;
        cue = Some(CUE_CURSOR_MOVE);
    }
    let cursor = (c as u32) & 1;
    if pad & PAD_ADVANCE == 0 {
        return PromptFrame {
            cursor,
            cue,
            next_state: None,
        };
    }
    PromptFrame {
        cursor,
        cue: Some(CUE_CONFIRM),
        next_state: Some(if cursor == 0 { 1 } else { 5 }),
    }
}

/// One frame of an ordinary caption step: the state to store (if any) and the
/// cue to fire.
///
/// Returns `None` while no face button is held - retail leaves the state alone
/// and falls through to the actor dispatcher.
// PORT: FUN_801d0750 (the shared caption-step advance tail)
// NOT WIRED: same missing host as [`tutorial_step`].
pub fn talk_advance(state: i16, pad: u16) -> Option<(i16, u16)> {
    let TutorialStep::Talk { advance, .. } = tutorial_step(state) else {
        return None;
    };
    if pad & PAD_ADVANCE == 0 {
        return None;
    }
    Some((state + advance, CUE_CONFIRM))
}

/// One frame of a countdown step.
///
/// `remaining` is `DAT_801D6080`, decremented by the frame step
/// `DAT_1F800393`. The step advances only once the counter goes **negative**,
/// so a countdown seeded at [`HANDOFF_FRAMES`] runs one frame longer than the
/// seed suggests. Returns the new counter and whether the state advances.
// PORT: FUN_801d0750 cases 0x0C / 0x11 (the countdown steps)
// NOT WIRED: same missing host as [`tutorial_step`].
pub fn countdown_frame(remaining: i32, frame_step: i32) -> (i32, bool) {
    let next = remaining - frame_step;
    (next, next < 0)
}

/// One frame of the practice step (`0xD`).
///
/// `feedback` is the praise / scold window `DAT_801D5144`, `combo_hit` the
/// `DAT_801D570C` latch saying the triangle landed on the combo slot, and
/// `score` the live session score `DAT_801D5150`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PracticeFrame {
    /// `Some(true)` = the praise line, `Some(false)` = the timing scold,
    /// `None` = no feedback caption this frame.
    pub caption_praise: Option<bool>,
    /// The feedback window after this frame's decay, floored at zero.
    pub feedback: i32,
    /// `true` once the score passes [`PRACTICE_SCORE_GATE`] and the step
    /// hands off.
    pub advance: bool,
}

// PORT: FUN_801d0750 case 0x0D (the practice step's feedback + score gate)
// NOT WIRED: same missing host as [`tutorial_step`]. The *inputs* all exist on
// [`crate::dance::DanceGame`] (score, and the combo-slot test
// [`crate::dance::DanceGame::on_combo_slot`]); what is missing is the caption
// sink.
/// Step the practice state.
///
/// The feedback window decays by the frame step and is floored at zero; while
/// it is non-zero the step captions the last triangle spend - the praise line
/// when `combo_hit`, otherwise the two-line timing scold. The score gate is
/// `899 < score`, i.e. [`PRACTICE_SCORE_GATE`] or better.
pub fn practice_frame(
    feedback: i32,
    combo_hit: bool,
    score: i32,
    frame_step: i32,
) -> PracticeFrame {
    let (caption_praise, feedback) = if feedback != 0 {
        (Some(combo_hit), (feedback - frame_step).max(0))
    } else {
        (None, feedback)
    };
    PracticeFrame {
        caption_praise,
        feedback,
        advance: score >= PRACTICE_SCORE_GATE,
    }
}

/// One frame of the warm-up step (`7`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WarmupFrame {
    /// `true` while the scold caption pair is up (the window is non-zero).
    pub caption_scold: bool,
    /// The scold window after this frame. It **counts up** by the frame step
    /// and snaps back to zero once it passes [`SCOLD_FRAMES`], so the scold
    /// blinks rather than fading.
    pub scold: i32,
    /// `true` once the score passes [`PRACTICE_SCORE_GATE`].
    pub advance: bool,
}

// PORT: FUN_801d0750 case 0x07 (the warm-up step's scold blink + score gate)
// NOT WIRED: same missing host as [`tutorial_step`].
/// Step the warm-up state. `scold` is `DAT_801D5140`.
///
/// With the window at zero the script instead captions the run off the
/// dancer's level (`DAT_801D544C / 1000`, sampled **before** the same frame
/// clears the gauge), which is a string lookup the port does not model.
pub fn warmup_frame(scold: i32, score: i32, frame_step: i32) -> WarmupFrame {
    let (caption_scold, scold) = if scold != 0 {
        let next = scold + frame_step;
        (true, if next > SCOLD_FRAMES { 0 } else { next })
    } else {
        (false, scold)
    };
    WarmupFrame {
        caption_scold,
        scold,
        advance: score >= PRACTICE_SCORE_GATE,
    }
}

impl TutorialStep {
    /// The caption lines this step draws, as `(x, y)` pairs. The prompt's two
    /// option rows are not included - they are [`option_pos`].
    pub fn captions(self) -> Vec<(i16, i16)> {
        let n = match self {
            TutorialStep::Prompt => 2,
            TutorialStep::Talk { lines, .. } => lines,
            // The countdown, free-dance and idle steps have no fixed line
            // list - their captions are conditional on the live session.
            _ => 0,
        };
        (0..n).map(caption_pos).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn states_outside_the_jump_table_are_inert() {
        assert_eq!(tutorial_step(-1), TutorialStep::Idle);
        assert_eq!(tutorial_step(TUTORIAL_STATES), TutorialStep::Idle);
        // The last table slot has no body of its own.
        assert_eq!(tutorial_step(0x12), TutorialStep::Idle);
    }

    #[test]
    fn the_two_free_dance_steps_are_distinct() {
        assert_eq!(tutorial_step(7), TutorialStep::Warmup);
        assert_eq!(tutorial_step(0xD), TutorialStep::Practice);
        // The warm-up scold blinks by counting up and wrapping; the practice
        // scold decays to zero.
        assert_eq!(warmup_frame(SCOLD_FRAMES, 0, 1).scold, 0);
        assert_eq!(warmup_frame(1, 0, 1).scold, 2);
        assert_eq!(practice_frame(1, false, 0, 1).feedback, 0);
    }

    #[test]
    fn the_skip_arm_advances_by_two() {
        assert_eq!(
            tutorial_step(5),
            TutorialStep::Talk {
                lines: 2,
                advance: 2
            }
        );
        assert_eq!(talk_advance(5, PAD_ADVANCE), Some((7, CUE_CONFIRM)));
        assert_eq!(talk_advance(4, PAD_ADVANCE), Some((5, CUE_CONFIRM)));
    }

    #[test]
    fn a_caption_step_holds_until_a_face_button() {
        assert_eq!(talk_advance(1, 0), None);
        assert_eq!(talk_advance(1, PAD_CURSOR_NEXT), None);
        assert_eq!(talk_advance(1, 0x10), Some((2, CUE_CONFIRM)));
    }

    #[test]
    fn countdown_and_free_dance_steps_ignore_the_pad() {
        assert_eq!(talk_advance(0xC, PAD_ADVANCE), None);
        assert_eq!(talk_advance(0xD, PAD_ADVANCE), None);
        assert_eq!(talk_advance(7, PAD_ADVANCE), None);
        assert_eq!(talk_advance(0, PAD_ADVANCE), None);
    }

    #[test]
    fn the_prompt_wraps_its_two_rows_and_branches() {
        let f = prompt_frame(0, PAD_CURSOR_PREV);
        assert_eq!(f.cursor, 1);
        assert_eq!(f.cue, Some(CUE_CURSOR_MOVE));
        assert_eq!(f.next_state, None);

        let yes = prompt_frame(0, PAD_ADVANCE);
        assert_eq!(yes.next_state, Some(1));
        let no = prompt_frame(1, PAD_ADVANCE);
        assert_eq!(no.next_state, Some(5));
        assert_eq!(no.cue, Some(CUE_CONFIRM));
    }

    #[test]
    fn both_cursor_bits_in_one_frame_cancel_but_still_cue() {
        let f = prompt_frame(0, PAD_CURSOR_PREV | PAD_CURSOR_NEXT);
        assert_eq!(f.cursor, 0);
        assert_eq!(f.cue, Some(CUE_CURSOR_MOVE));
    }

    #[test]
    fn the_countdown_advances_only_once_it_goes_negative() {
        assert_eq!(countdown_frame(1, 1), (0, false));
        assert_eq!(countdown_frame(0, 1), (-1, true));
        assert_eq!(countdown_frame(HANDOFF_FRAMES, 2).0, HANDOFF_FRAMES - 2);
    }

    #[test]
    fn practice_captions_only_while_the_feedback_window_is_open() {
        let idle = practice_frame(0, true, 0, 1);
        assert_eq!(idle.caption_praise, None);
        assert!(!idle.advance);

        let praise = practice_frame(10, true, 0, 3);
        assert_eq!(praise.caption_praise, Some(true));
        assert_eq!(praise.feedback, 7);

        let scold = practice_frame(2, false, 0, 5);
        assert_eq!(scold.caption_praise, Some(false));
        assert_eq!(scold.feedback, 0);
    }

    #[test]
    fn practice_hands_off_at_the_score_gate() {
        assert!(!practice_frame(0, false, PRACTICE_SCORE_GATE - 1, 1).advance);
        assert!(practice_frame(0, false, PRACTICE_SCORE_GATE, 1).advance);
    }

    #[test]
    fn cursor_and_option_rows_line_up() {
        assert_eq!(option_pos(0), (CAPTION_OPTION_X, 0x98));
        assert_eq!(option_pos(1), (CAPTION_OPTION_X, 0xA8));
        assert_eq!(cursor_pos(0), (CURSOR_X, 0x98));
        assert_eq!(cursor_pos(1), (CURSOR_X, 0xA8));
        assert_eq!(cursor_pos(3), (CURSOR_X, 0xA8));
    }
}
