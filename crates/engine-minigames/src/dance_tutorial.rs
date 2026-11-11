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
    /// clears the score and the groove gauge every frame and calls each
    /// button as the chart asks for it. Advances once the frames danced reach
    /// [`PRACTICE_PROGRESS_GATE`]. A missed press puts a scold up instead,
    /// on a window that **counts up** and wraps past [`SCOLD_FRAMES`].
    Warmup,
    /// The practice step (`0xD`): the player dances with a fresh triangle
    /// stock, and the step advances on the same gate. Captions the last
    /// triangle spend (praise or a timing scold) while its window, which
    /// this step alone counts down, is open.
    Practice,
    /// A jump-table slot with no body (`0x12`), or a state outside the table -
    /// the handler does nothing but re-enter the actor dispatcher.
    Idle,
}

// PORT: FUN_801d0750 (the tutorial script's jump-table shape)
// Wired: [`DanceTutorial::step`] dispatches on this per frame, and the play
// window hosts a tutorial run beside the how-to dance session
// (`window/minigames.rs` -> `window/hud.rs`), drawing placeholder captions at
// the retail line positions (the caption strings themselves are overlay
// rodata and stay unread).
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
// Wired: [`DanceTutorial::step`]'s prompt arm, on the play window's tutorial
// path (see [`tutorial_step`]).
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
// Wired: [`DanceTutorial::step`]'s talk arm, on the play window's tutorial
// path (see [`tutorial_step`]).
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
///
/// Named `tutorial_countdown_frame` and not `countdown_frame` on purpose: the
/// Baka Fighter round chrome carries an unrelated free `countdown_frame`
/// (`crates/engine-minigames/src/baka_fighter_chrome.rs`) that a live host does
/// call, and the reachability audit resolves a bare call against every free
/// function of that name. Sharing the name made this disclosure read as stale.
// PORT: FUN_801d0750 cases 0x0C / 0x11 (the countdown steps)
// Wired: [`DanceTutorial::step`]'s countdown arm, on the play window's
// tutorial path (see [`tutorial_step`]).
pub fn tutorial_countdown_frame(remaining: i32, frame_step: i32) -> (i32, bool) {
    let next = remaining - frame_step;
    (next, next < 0)
}

/// Frames of dancing a free-dance step waits for (`DAT_801D5150`, which the
/// hall's state `10` raises by the frame step: `slti v0,v0,0x384`).
pub const PRACTICE_PROGRESS_GATE: i32 = 900;
/// Third argument of the music fades (`li a2,0x3c` ahead of `FUN_80062004`).
pub const MUSIC_FADE_ARG: u32 = 0x3C;
/// The hall state the script parks the dance on (`DAT_801D5334 = 2`): the
/// load gate's wait, from which the count-in runs again.
pub const HALL_STATE_RESTART: u32 = 2;
/// Triangles the practice hand-off restocks (`DAT_801D534C = 3`).
pub const PRACTICE_STOCK: u32 = 3;

/// Where a caption's text is on the user's disc: the overlay's own rodata,
/// never read here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptionSource {
    /// The VA of a pointer word in the script's line tables.
    Table(u32),
    /// The VA of the string itself (the prompt's two option rows).
    Inline(u32),
}

/// One `FUN_80036888` call of a frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TutorialLine {
    pub source: CaptionSource,
    pub x: i16,
    pub y: i16,
}

/// What a frame asks of the hall's music.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TutorialMusic {
    /// `FUN_80062004(seq, volume << shift >> 16, MUSIC_FADE_ARG)`: the song
    /// down to a half (`shift == 15`) or a quarter (`14`) of the BGM volume.
    Fade { shift: u8 },
    /// `FUN_800266E0` on the hall's BGM source: the song stops.
    Release,
}

/// The dance hall's words the script reads and writes, by address.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TutorialHall {
    /// `DAT_801D5138`: holds the song timer still while the lesson runs.
    pub song_hold: u32,
    /// `DAT_801D5830`: the load gate. Set, the hall's state `2` waits and the
    /// award routine takes no press.
    pub load_gate: u32,
    /// `DAT_801D5334`: the hall's state.
    pub hall_state: u32,
    /// `DAT_801D53CC`: the human's score.
    pub score: u32,
    /// `DAT_801D544C`: the human's groove gauge.
    pub gauge: i32,
    /// `DAT_801D5150`: frames danced since the last clear.
    pub progress: i32,
    /// `DAT_801D5158`: the count-in's clock.
    pub count_in: u32,
    /// `DAT_801D5140`: the window the award routine arms on the human's miss.
    pub miss_window: i32,
    /// `DAT_801D5144`: the window a spent triangle arms.
    pub feedback: i32,
    /// `DAT_801D512C`: the beat track's combo flash.
    pub track_flash: i32,
    /// `DAT_801D5170`: the bar count.
    pub bars: u32,
    /// `DAT_801D5828`: the bar's closing-beat mark.
    pub bar_tail: u32,
    /// `DAT_801D581C`: the beat phase.
    pub phase: u32,
    /// `DAT_801D5820`: the song timer.
    pub song: u32,
    /// `DAT_801D5824`: the bar phase.
    pub bar_phase: u32,
    /// `DAT_801D534C`: the human's triangle stock.
    pub stock: u32,
    /// `DAT_801D570C`: whether the human's last triangle landed.
    pub landed: u32,
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

/// What one tutorial frame asks the host to do (the drawn/audible half of a
/// [`DanceTutorial::step`] call; the stores already happened).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TutorialFrame {
    /// Cue id to fire into the SFX scheduler (`_DAT_8007B6D8`).
    pub cue: Option<u16>,
    /// Every `FUN_80036888` call of the frame, in call order.
    pub lines: Vec<TutorialLine>,
    /// Positions of the frame's table lines on a talk or prompt step. The
    /// strings are overlay rodata the port does not read; the host draws its
    /// own text here.
    pub captions: Vec<(i16, i16)>,
    /// The opening prompt's two option rows, when it is up.
    pub options: Option<[(i16, i16); 2]>,
    /// The option cursor's position, when the prompt is up.
    pub cursor_pos: Option<(i16, i16)>,
    /// A free-dance step's caption: `Some(true)` = the praise line,
    /// `Some(false)` = a scold.
    pub feedback: Option<bool>,
    /// What the frame asks of the song.
    pub music: Option<TutorialMusic>,
    /// The frame parked the hall on [`HALL_STATE_RESTART`]: the count-in
    /// runs again once the load gate drops.
    pub restart: bool,
    /// The script ran off the jump table - the tutorial is over.
    pub done: bool,
}

/// The line table each talk state reads (`lw a0,N(s0)`), by state.
const TALK_TABLES: [(i16, u32); 13] = [
    (1, 0x801D_497C),
    (2, 0x801D_4988),
    (3, 0x801D_4990),
    (4, 0x801D_499C),
    (5, 0x801D_49A8),
    (6, 0x801D_4A24),
    (8, 0x801D_49C0),
    (9, 0x801D_49CC),
    (0xA, 0x801D_49D8),
    (0xB, 0x801D_49E4),
    (0xE, 0x801D_4A0C),
    (0xF, 0x801D_4A18),
    (0x10, 0x801D_4A24),
];
/// The prompt's two lines, then its two option strings.
const PROMPT_TABLE: u32 = 0x801D_4974;
const PROMPT_OPTIONS: [u32; 2] = [0x801C_EEDC, 0x801C_EEE0];
/// The warm-up's two button prompts (Square, then Circle) and its scold pair.
const WARMUP_PROMPTS: [u32; 2] = [0x801D_49B0, 0x801D_49B4];
const WARMUP_SCOLD: u32 = 0x801D_49B8;
/// The practice step's combo call, praise line and scold pair.
const PRACTICE_CALL: u32 = 0x801D_49F4;
const PRACTICE_PRAISE: u32 = 0x801D_49F8;
const PRACTICE_SCOLD: u32 = 0x801D_49FC;

/// The Disco King tutorial actor as one advancing object: the actor's own
/// words, stepped once a frame over the hall's ([`TutorialHall`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DanceTutorial {
    /// The actor's `+0x9C` state word.
    pub state: i16,
    /// The opening prompt's cursor row (`DAT_801D607C`).
    pub cursor: u32,
    /// The countdown counter (`DAT_801D6080`).
    pub countdown: i32,
    /// The hall's words between frames. [`Self::step_game`] refreshes the
    /// ones a [`crate::dance::DanceGame`] holds.
    pub hall: TutorialHall,
}

impl Default for DanceTutorial {
    fn default() -> Self {
        Self::new()
    }
}

impl DanceTutorial {
    /// A tutorial parked on the opening prompt (state `0`).
    pub fn new() -> Self {
        DanceTutorial {
            state: 0,
            cursor: 0,
            countdown: 0,
            hall: TutorialHall::default(),
        }
    }

    /// One frame of the tutorial script over a live run: the hall's words the
    /// run holds are read off it and stored back, the chart probes are the
    /// run's, and a restart sends its floor back to the count-in.
    pub fn step_game(
        &mut self,
        game: &mut crate::dance::DanceGame,
        pad: u16,
        frame_step: u8,
    ) -> TutorialFrame {
        let mut hall = game.tutorial_hall(self.hall);
        let frame = {
            let g = &*game;
            self.step(&mut hall, pad, frame_step, |lane, symbol| {
                g.probe_cell(lane, symbol)
            })
        };
        game.apply_tutorial_hall(&hall);
        if frame.restart {
            game.restart_for_count_in();
        }
        self.hall = hall;
        frame
    }

    /// One frame of the tutorial script.
    ///
    /// `pad` is this frame's pad word in the retail `_DAT_8007B874` layout,
    /// `frame_step` the frame delta `DAT_1F800393`, and `probe(lane, symbol)`
    /// the chart probe `FUN_801D1960(0, lane, 0x100 | (symbol - 1))`.
    // PORT: FUN_801d0750
    pub fn step(
        &mut self,
        hall: &mut TutorialHall,
        pad: u16,
        frame_step: u8,
        mut probe: impl FnMut(i32, u8) -> bool,
    ) -> TutorialFrame {
        let dt = i32::from(frame_step);
        let state = self.state;
        let step = tutorial_step(state);
        let mut out = TutorialFrame::default();
        let table = |out: &mut TutorialFrame, va: u32, row: i16| {
            out.lines.push(TutorialLine {
                source: CaptionSource::Table(va),
                x: CAPTION_X,
                y: CAPTION_Y0 + CAPTION_PITCH * row,
            });
        };
        let next = state.wrapping_add(1);
        let fade = Some(TutorialMusic::Fade { shift: 14 });
        match step {
            TutorialStep::Prompt => {
                hall.song_hold = 1;
                table(&mut out, PROMPT_TABLE, 0);
                table(&mut out, PROMPT_TABLE + 4, 1);
                for (i, va) in PROMPT_OPTIONS.into_iter().enumerate() {
                    let (x, y) = option_pos(i as u8);
                    out.lines.push(TutorialLine {
                        source: CaptionSource::Inline(va),
                        x,
                        y,
                    });
                }
                let f = prompt_frame(self.cursor, pad);
                self.cursor = f.cursor;
                out.cue = f.cue;
                out.options = Some([option_pos(0), option_pos(1)]);
                out.cursor_pos = Some(cursor_pos(self.cursor));
                if let Some(to) = f.next_state {
                    self.state = if to == 1 { next } else { to };
                }
            }
            TutorialStep::Talk { lines, .. } => {
                // The two pages that park the hall do it every frame they
                // are up, ahead of their lines.
                if matches!(state, 8 | 9 | 0xE) {
                    hall.load_gate = 1;
                }
                if matches!(state, 8 | 0xE) {
                    hall.hall_state = HALL_STATE_RESTART;
                    out.restart = true;
                }
                let va = TALK_TABLES
                    .iter()
                    .find(|(s, _)| *s == state)
                    .map_or(0, |&(_, va)| va);
                for i in 0..lines {
                    table(&mut out, va + 4 * u32::from(i), i16::from(i));
                }
                if let Some((to, cue)) = talk_advance(state, pad) {
                    out.cue = Some(cue);
                    self.state = to;
                    match state {
                        6 => hall.count_in = 0,
                        0xA => hall.progress = 0,
                        0xB => {
                            out.music = Some(TutorialMusic::Fade { shift: 15 });
                            self.countdown = HANDOFF_FRAMES;
                        }
                        0xE => out.music = fade,
                        0x10 => {
                            out.music = fade;
                            self.countdown = HANDOFF_FRAMES;
                        }
                        _ => {}
                    }
                }
            }
            TutorialStep::Countdown => {
                if state == 0x11 {
                    hall.gauge = 0;
                    hall.score = 0;
                }
                let (left, advance) = tutorial_countdown_frame(self.countdown, dt);
                self.countdown = left;
                if advance {
                    hall.bars = 0;
                    hall.bar_tail = 0;
                    hall.phase = 0;
                    hall.song = 0;
                    hall.count_in = 0;
                    hall.bar_phase = 0;
                    hall.hall_state = HALL_STATE_RESTART;
                    out.restart = true;
                    if state == 0xC {
                        hall.feedback = 0;
                        hall.track_flash = 0;
                        hall.stock = PRACTICE_STOCK;
                        hall.landed = 0;
                    } else {
                        hall.song_hold = 0;
                        hall.load_gate = 0;
                        out.music = Some(TutorialMusic::Release);
                    }
                    self.state = next;
                }
            }
            TutorialStep::Warmup => {
                // The level is read before the same frame clears the gauge.
                let level = hall.gauge / 1000;
                hall.gauge = 0;
                hall.load_gate = 0;
                hall.score = 0;
                if hall.miss_window != 0 {
                    table(&mut out, WARMUP_SCOLD, 0);
                    table(&mut out, WARMUP_SCOLD + 4, 1);
                    out.feedback = Some(false);
                    hall.miss_window = hall.miss_window.wrapping_add(dt);
                    if hall.miss_window > SCOLD_FRAMES {
                        hall.miss_window = 0;
                    }
                } else {
                    for (i, va) in WARMUP_PROMPTS.into_iter().enumerate() {
                        if probe(level, i as u8 + 1) {
                            table(&mut out, va, 0);
                        }
                    }
                }
                if hall.progress >= PRACTICE_PROGRESS_GATE {
                    self.state = next;
                    out.music = fade;
                }
            }
            TutorialStep::Practice => {
                hall.load_gate = 0;
                hall.score = 0;
                if hall.feedback != 0 {
                    if hall.landed == 1 {
                        table(&mut out, PRACTICE_PRAISE, 1);
                        out.feedback = Some(true);
                    } else {
                        table(&mut out, PRACTICE_SCOLD, 1);
                        table(&mut out, PRACTICE_SCOLD + 4, 2);
                        out.feedback = Some(false);
                    }
                    hall.feedback = hall.feedback.wrapping_sub(dt).max(0);
                }
                if hall.progress >= PRACTICE_PROGRESS_GATE {
                    hall.load_gate = 1;
                    self.state = next;
                }
                if hall.track_flash != 0 {
                    table(&mut out, PRACTICE_CALL, 0);
                    hall.track_flash = hall.track_flash.wrapping_sub(2 * dt).max(0);
                }
            }
            TutorialStep::Idle => {
                out.done = true;
            }
        }
        if matches!(step, TutorialStep::Prompt | TutorialStep::Talk { .. }) {
            out.captions = out
                .lines
                .iter()
                .filter(|l| matches!(l.source, CaptionSource::Table(_)))
                .map(|l| (l.x, l.y))
                .collect();
        }
        out
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
        assert_eq!(tutorial_countdown_frame(1, 1), (0, false));
        assert_eq!(tutorial_countdown_frame(0, 1), (-1, true));
        assert_eq!(
            tutorial_countdown_frame(HANDOFF_FRAMES, 2).0,
            HANDOFF_FRAMES - 2
        );
    }

    fn run(t: &mut DanceTutorial, hall: &mut TutorialHall, pad: u16) -> TutorialFrame {
        t.step(hall, pad, 1, |_, _| false)
    }

    #[test]
    fn the_runner_walks_the_script_end_to_end() {
        let mut t = DanceTutorial::new();
        let mut h = TutorialHall::default();
        // Prompt: yes on row 0 advances to state 1 with the confirm cue.
        let f = run(&mut t, &mut h, PAD_ADVANCE);
        assert_eq!(f.cue, Some(CUE_CONFIRM));
        assert_eq!((t.state, h.song_hold), (1, 1));
        assert!(f.options.is_some() && f.cursor_pos.is_some());
        // A talk step holds without a face button, advances with one.
        assert_eq!(run(&mut t, &mut h, 0).cue, None);
        assert_eq!(t.state, 1);
        run(&mut t, &mut h, PAD_ADVANCE);
        assert_eq!(t.state, 2);
        // The skip arm at 5 hops the acknowledgement.
        t.state = 5;
        run(&mut t, &mut h, PAD_ADVANCE);
        assert_eq!(t.state, 7, "state 5 stores state + 2");
        // Warm-up: clears the score every frame and parks until the frames
        // danced reach the gate, whatever the pad.
        h.score = 500;
        let f = run(&mut t, &mut h, PAD_ADVANCE);
        assert_eq!((t.state, h.score, f.feedback), (7, 0, None));
        h.progress = PRACTICE_PROGRESS_GATE;
        let f = run(&mut t, &mut h, 0);
        assert_eq!(t.state, 8);
        assert_eq!(f.music, Some(TutorialMusic::Fade { shift: 14 }));
        // The level page parks the hall behind the load gate.
        let f = run(&mut t, &mut h, 0);
        assert!(f.restart);
        assert_eq!((h.load_gate, h.hall_state), (1, HALL_STATE_RESTART));
        // Countdown: seeded by the page before it, advances only past zero.
        t.state = 0xB;
        run(&mut t, &mut h, PAD_ADVANCE);
        assert_eq!((t.state, t.countdown), (0xC, HANDOFF_FRAMES));
        for _ in 0..HANDOFF_FRAMES {
            run(&mut t, &mut h, PAD_ADVANCE);
            assert_eq!(t.state, 0xC, "the pad is ignored and the seed holds");
        }
        run(&mut t, &mut h, 0);
        assert_eq!((t.state, h.stock), (0xD, PRACTICE_STOCK));
        // Practice: captions the spend while the window is open and steps
        // the window itself; gates on the frames danced.
        h.progress = 0;
        h.feedback = 10;
        h.landed = 1;
        let f = run(&mut t, &mut h, 0);
        assert_eq!((f.feedback, h.feedback, t.state), (Some(true), 9, 0xD));
        h.progress = PRACTICE_PROGRESS_GATE;
        run(&mut t, &mut h, 0);
        assert_eq!((t.state, h.load_gate), (0xE, 1));
        // Running off the table reports done.
        t.state = 0x12;
        assert!(run(&mut t, &mut h, 0).done);
    }

    #[test]
    fn the_prompt_no_branch_reaches_the_skip_arm() {
        let mut t = DanceTutorial::new();
        let mut h = TutorialHall::default();
        run(&mut t, &mut h, PAD_CURSOR_NEXT);
        assert_eq!(t.cursor, 1);
        run(&mut t, &mut h, PAD_ADVANCE);
        assert_eq!(t.state, 5, "row 1 jumps straight to state 5");
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
