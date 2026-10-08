//! Opening-cutscene narration presenter - the retail subtitle **roller**.
//!
//! The opening prologue scenes (`opdeene` / `opstati` / `opurud` / `map01`)
//! play their on-screen narration from inline ASCII text pages embedded in the
//! scene MAN's cutscene-timeline script (parsed by
//! [`legaia_asset::cutscene_text`]). Retail routes the introducing op
//! (`0xCC 0xF8 0x80 N`, field-VM `0x4C` outer-nibble-8 sub-0) to a dedicated
//! on-screen-text actor whose handler is `FUN_80037174` - a bottom-up **text
//! crawl**, not a one-line caption.
//!
//! [`CutsceneNarration`] is that handler, state for state, read off the
//! disassembly (`see ghidra/scripts/funcs/80037174.txt`):
//!
//! - **Geometry comes from the scene, not from a table.** The roller reads a
//!   config block through the dialog-context pointer `*0x801C6EA4`: `+0x4C`
//!   window top, `+0x4E` line-slot count `n`, `+0x50` scroll divisor, `+0x52`
//!   release line count ([`RollerSeed`]). The scene reset `FUN_8003A024`
//!   stores `0x40 / 8 / 4 / 0` (`0x8003A0BC..0x8003A0DC`), and the timeline's
//!   `CC F8 E8 w0 w1 w2 w3` op (`w3 == 0`) overwrites the first three,
//!   defaulting each zero word the same way (`0x801E348C..0x801E34BC` in the
//!   field VM). On the disc the seed op immediately precedes a crawl block.
//! - **The line pitch is fixed at 16.** Slot `i` draws at
//!   `y = top - subscroll + 16*i` (`addiu s3,s3,0x10` at `0x8003760C`), and
//!   the sub-scroll `+0x9E` wraps at `0x10` (`0x80037288`).
//! - **The clock is the adaptive frame step, not a frame count.** Every frame
//!   the accumulator `+0x50` gains the frame step `DAT_1F800393`
//!   (`0x8003723C..0x80037250`, skipped while the pause bit `+0x10 & 0x80000`
//!   is set); when it reaches the divisor it resets to zero and the crawl
//!   climbs one pixel. The remainder is discarded, so the speed is
//!   `1 px per ceil(divisor / step)` frames.
//! - **`n + 1` slot states** at `actor+0x80..` (`0xFF` empty, `0xFE` blank
//!   line, `1` text), seeded all-empty (`0x800371F0..0x80037210`). A full
//!   16-pixel climb shifts them up one (`0x800372CC..0x800372FC`), counting a
//!   retired page `+0x6A` when slot 0 held one, and admits page `+0x9C` into
//!   slot `n` - `0xFE` when its first byte is the terminator, `0xFF` once the
//!   pages run out.
//! - **Release** (`0x800373BC..0x80037428`): with `+0x52` non-zero and slot 1
//!   occupied, the roller pauses itself (`+0x10 |= 0x80000`) and clears
//!   `+0x52` when the lines retired so far (`+0x6A`, plus one if slot 0 is
//!   occupied) equal it.
//! - **Clip window** (`0x80037610..0x800376C4`): `y` from `top + 4` to
//!   `min(top + 16*n - 1, 0xE8)`, `x` from `0` to `0x13F`; lines scroll into
//!   and out of it a pixel at a time.
//! - **Completion** (`0x800376D8..0x80037728`): once `+0x6A` reaches the
//!   page count the roller clears the parent's `+0x10 & 0x400` and kills
//!   itself (`+0x10 |= 8`).
//!
//! What the port does not own is the frame step. `FUN_80016B6C` derives it
//! from the measured frame cost and floors it at `DAT_8007B9D8`; the
//! cold-boot `opdeene` capture (`s1_newgame_field`) holds that floor at `3`
//! and a live roller's accumulator at `3` after one frame, so the opening
//! runs at a frame step of 3 - [`OPENING_FRAME_STEP`]. The floor's writer is
//! the scene itself: `opdeene`'s prescript record 16 opens with move-VM ext
//! sub-op `0x2F` operand `3` (the arm at `0x801D45D4` in PROT 0897 stores it
//! to `0x8007B9D8`), and the world installs the roller at the world's cadence,
//! so the constant is the measured value, not an override. At the seed's divisor
//! of 4 that is 1 px per two frames, 10 px/s, which is the realtime-video
//! figure the previous capture-pinned model had fitted with a frame count.
//!
//! The `PORT:` tag for the roller is on [`CutsceneNarration`] itself, not
//! here - a module-scope tag resolves to a file-scoped anchor
//! ([`reach-triage.md`](../../../docs/tooling/reach-triage.md)).

/// Retail's fixed line pitch (`addiu s3,s3,0x10`; the sub-scroll wraps at
/// `0x10`).
pub const LINE_PITCH: i32 = 16;

/// The frame step (`DAT_1F800393`) the opening scenes run at: the floor
/// `DAT_8007B9D8` reads `3` in the cold-boot `opdeene` capture, and a live
/// roller's accumulator reads `3` one frame after a reset.
pub const OPENING_FRAME_STEP: u16 = 3;

/// Bottom of the clip window never passes this line (`slti v0,a3,0xe9`).
pub const CLIP_BOTTOM_MAX: i32 = 0xE8;

/// Slot state: empty (`0xFF`).
const SLOT_EMPTY: u8 = 0xFF;
/// Slot state: a page with no text - consumes a page, draws nothing (`0xFE`).
const SLOT_BLANK: u8 = 0xFE;
/// Slot state: a drawn page.
const SLOT_TEXT: u8 = 1;

/// The crawl config block the roller reads through `*0x801C6EA4`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RollerSeed {
    /// `+0x4C` - window top (slot 0's line base).
    pub top: i16,
    /// `+0x4E` - line-slot count `n`; the roller keeps `n + 1` slot states.
    pub slots: i16,
    /// `+0x50` - accumulator threshold for one pixel of climb.
    pub divisor: i16,
    /// `+0x52` - release line count; `0` = never.
    pub release: i16,
}

impl RollerSeed {
    /// What the scene reset `FUN_8003A024` stores.
    pub const SCENE_RESET: Self = Self {
        top: 0x40,
        slots: 8,
        divisor: 4,
        release: 0,
    };

    /// Apply the timeline's `CC F8 E8` seed words `w0..w2` (`w3 == 0`
    /// sub-mode): each zero word takes the op's default (`0x40` / `8` / `4`),
    /// and `+0x52` is left as it was.
    pub fn with_config_words(self, w0: i16, w1: i16, w2: i16) -> Self {
        Self {
            top: if w0 == 0 { 0x40 } else { w0 },
            slots: if w1 == 0 { 8 } else { w1 },
            divisor: if w2 == 0 { 4 } else { w2 },
            release: self.release,
        }
    }

    /// The seed op ending exactly at `op_offset` in a timeline body, if one
    /// does: `[CC F8 E8][w0][w1][w2][w3]` with `w3 == 0` (the geometry
    /// sub-mode). Every crawl block on the disc is either preceded by one or
    /// runs on the seed a previous block in the same scene left.
    pub fn config_op_before(self, body: &[u8], op_offset: usize) -> Option<Self> {
        let start = op_offset.checked_sub(11)?;
        let op = body.get(start..op_offset)?;
        if op[..3] != [0xCC, 0xF8, 0xE8] {
            return None;
        }
        let w = |k: usize| i16::from_le_bytes([op[3 + 2 * k], op[4 + 2 * k]]);
        (w(3) == 0).then(|| self.with_config_words(w(0), w(1), w(2)))
    }

    /// The clip window's vertical span, `(top, bottom)` inclusive.
    pub fn clip_window(&self) -> (i32, i32) {
        let top = i32::from(self.top);
        let bottom = (top + LINE_PITCH * i32::from(self.slots) - 1).min(CLIP_BOTTOM_MAX);
        (top + 4, bottom)
    }
}

impl Default for RollerSeed {
    fn default() -> Self {
        Self::SCENE_RESET
    }
}

/// One visible roller line: its current PSX-space Y (top of the 16px row) and
/// the page text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NarrationLine<'a> {
    /// Top Y of the line's row in PSX framebuffer pixels.
    pub y: i32,
    /// The page text (drawn centered).
    pub text: &'a str,
}

/// A running narration block: the roller over an ordered list of subtitle
/// pages.
///
/// PORT: FUN_80037174
/// REF: FUN_8003BDE0
#[derive(Clone, Debug)]
pub struct CutsceneNarration {
    /// The subtitle pages, in entry order.
    pages: Vec<String>,
    /// The config block (see [`RollerSeed`]).
    seed: RollerSeed,
    /// `DAT_1F800393` - vsyncs per retail frame, and the accumulator gain.
    frame_step: u16,
    /// Vsyncs banked toward the next retail frame.
    vsyncs: u32,
    /// `+0x54` - `false` until the first frame's init has run.
    started: bool,
    /// `+0x50` - the scroll accumulator.
    accum: u16,
    /// `+0x9E` - pixels climbed within the current 16-pixel line.
    subscroll: i16,
    /// `+0x6A` - pages retired off the top slot.
    retired: usize,
    /// `+0x9C` - next page to admit at the bottom slot.
    next_page: usize,
    /// `+0x80..` - `n + 1` slot states.
    slots: Vec<u8>,
    /// `+0x10 & 0x80000` - the pause bit (release, or the config op's pause).
    paused: bool,
    /// `+0x10 & 8` - the block has finished (or was force-finished).
    complete: bool,
}

impl CutsceneNarration {
    /// Build a roller over `pages` with the scene-reset seed at the opening's
    /// frame step. A roller with no pages is immediately
    /// [`complete`](Self::is_complete).
    pub fn new(pages: Vec<String>) -> Self {
        Self::with_seed(pages, RollerSeed::SCENE_RESET, OPENING_FRAME_STEP)
    }

    /// Build a roller with an explicit seed and frame step.
    pub fn with_seed(pages: Vec<String>, seed: RollerSeed, frame_step: u16) -> Self {
        let complete = pages.is_empty();
        Self {
            pages,
            seed,
            frame_step: frame_step.max(1),
            vsyncs: 0,
            started: false,
            accum: 0,
            subscroll: 0,
            retired: 0,
            next_page: 0,
            slots: Vec::new(),
            paused: false,
            complete,
        }
    }

    /// Total number of pages in the block.
    pub fn page_count(&self) -> usize {
        self.pages.len()
    }

    /// Index of the next page to enter at the window bottom (`+0x9C`).
    pub fn current_index(&self) -> usize {
        self.next_page
    }

    /// Pages retired off the top of the window (`+0x6A`).
    pub fn retired(&self) -> usize {
        self.retired
    }

    /// `true` once every page has scrolled out (or the block was
    /// force-finished).
    pub fn is_complete(&self) -> bool {
        self.complete
    }

    /// The config block this roller runs.
    pub fn seed(&self) -> RollerSeed {
        self.seed
    }

    /// The retail clip window, `(top, bottom)` inclusive, in PSX pixels.
    pub fn clip_window(&self) -> (i32, i32) {
        self.seed.clip_window()
    }

    /// `true` while the pause bit is set.
    pub fn is_paused(&self) -> bool {
        self.paused
    }

    /// The config op's pause (`w3 == 1`, `w0 == 0`).
    pub fn pause(&mut self) {
        self.paused = true;
    }

    /// The config op's resume (`w3 == 2`): clear the pause bit.
    pub fn resume(&mut self) {
        self.paused = false;
    }

    /// The config op's release trigger (`w3 == 1`, `w0 != 0`): write `+0x52`.
    pub fn set_release(&mut self, lines: i16) {
        self.seed.release = lines;
    }

    /// Every slot's line with its Y, in slot order, as the draw pass walks
    /// them (`0x80037544..0x8003760C`): the page cursor starts at `+0x6A`,
    /// an empty slot neither draws nor consumes a page, a blank slot consumes
    /// one without drawing.
    fn slot_lines(&self) -> Vec<NarrationLine<'_>> {
        let mut out = Vec::new();
        if self.complete || !self.started {
            return out;
        }
        let mut page = self.retired;
        let mut y = i32::from(self.seed.top) - i32::from(self.subscroll);
        for &state in &self.slots {
            match state {
                SLOT_EMPTY => {}
                SLOT_BLANK => page += 1,
                _ => {
                    if let Some(text) = self.pages.get(page) {
                        out.push(NarrationLine { y, text });
                    }
                    page += 1;
                }
            }
            y += LINE_PITCH;
        }
        out
    }

    /// The lines on screen, top first, each with its PSX-space Y. Only lines
    /// whose whole 16-pixel row lies inside [`Self::clip_window`] are
    /// returned, because neither host scissors the text: retail's window
    /// shows the rows at its two edges partially, which a host with a scissor
    /// can reproduce from [`Self::all_lines`].
    pub fn visible_lines(&self) -> Vec<NarrationLine<'_>> {
        let (top, bottom) = self.clip_window();
        self.slot_lines()
            .into_iter()
            .filter(|l| l.y >= top && l.y + LINE_PITCH - 1 <= bottom)
            .collect()
    }

    /// Every drawn line including the rows the clip window cuts.
    pub fn all_lines(&self) -> Vec<NarrationLine<'_>> {
        self.slot_lines()
    }

    /// The text of the newest (bottom-most) visible line, or `None` when
    /// nothing is on screen. Compatibility accessor for single-line hosts;
    /// prefer [`Self::visible_lines`].
    pub fn current_text(&self) -> Option<&str> {
        self.visible_lines().last().map(|l| l.text)
    }

    /// Advance the roller by `vsyncs` display vsyncs. Every
    /// [`Self::frame_step`]-vsync retail frame runs one pass of the handler.
    /// Returns `true` while the block is still on screen, `false` once
    /// complete (so the host can release the suspended timeline).
    pub fn tick(&mut self, vsyncs: u32) -> bool {
        if self.complete {
            return false;
        }
        self.vsyncs = self.vsyncs.saturating_add(vsyncs);
        let step = u32::from(self.frame_step);
        while self.vsyncs >= step && !self.complete {
            self.vsyncs -= step;
            self.frame();
        }
        !self.complete
    }

    /// The frame step this roller runs at.
    pub fn frame_step(&self) -> u16 {
        self.frame_step
    }

    /// One pass of `FUN_80037174`.
    pub fn frame(&mut self) {
        if self.complete {
            return;
        }
        if !self.started {
            // `+0x54 == 0`: reset the counters, prime the sub-scroll at 0xF
            // and the accumulator at the divisor (so the first frame steps),
            // clear `n + 1` slots.
            self.retired = 0;
            self.next_page = 0;
            self.subscroll = 0xF;
            self.accum = self.seed.divisor as u16;
            let n = usize::try_from(self.seed.slots).unwrap_or(0);
            self.slots = vec![SLOT_EMPTY; n + 1];
            self.started = true;
        }
        if !self.paused {
            self.accum = self.accum.wrapping_add(self.frame_step);
        }
        if (self.accum as i16) >= self.seed.divisor {
            self.accum = 0;
            self.subscroll += 1;
            if self.subscroll >= LINE_PITCH as i16 {
                self.subscroll = 0;
                self.line_step();
            }
        }
        if self.retired >= self.pages.len() {
            self.complete = true;
        }
    }

    /// A full 16-pixel climb: retire, shift, admit, then the release test.
    fn line_step(&mut self) {
        if self.slots.first().is_some_and(|&s| s != SLOT_EMPTY) {
            self.retired += 1;
        }
        let last = self.slots.len().saturating_sub(1);
        if !self.slots.is_empty() {
            self.slots.rotate_left(1);
            self.slots[last] = if self.next_page >= self.pages.len() {
                SLOT_EMPTY
            } else {
                let state = if self.pages[self.next_page].is_empty() {
                    SLOT_BLANK
                } else {
                    SLOT_TEXT
                };
                self.next_page += 1;
                state
            };
        }
        let release = self.seed.release;
        if release != 0 && self.slots.get(1).is_some_and(|&s| s != SLOT_EMPTY) {
            let reached = if self.slots[0] == SLOT_EMPTY {
                self.retired as i64
            } else {
                self.retired as i64 + 1
            };
            if reached == i64::from(release) {
                self.seed.release = 0;
                self.paused = true;
            }
        }
    }

    /// Force one immediate line step (a debug / skip accelerator; retail has
    /// no per-line skip - the whole opening is skipped via the hand-off
    /// packet instead). Returns `true` while the block is still on screen.
    pub fn skip_page(&mut self) -> bool {
        if self.complete {
            return false;
        }
        if !self.started {
            self.frame();
        }
        self.subscroll = 0;
        self.line_step();
        self.accum = 0;
        if self.retired >= self.pages.len() {
            self.complete = true;
        }
        !self.complete
    }

    /// Force-finish the block (the config op's `w3 == 3` kill): the roller
    /// reports complete and draws nothing.
    pub fn force_finish(&mut self) {
        self.retired = self.pages.len();
        self.next_page = self.pages.len();
        self.complete = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pages(n: usize) -> Vec<String> {
        (0..n).map(|i| format!("line {i}")).collect()
    }

    fn frames(n: &mut CutsceneNarration, count: u32) {
        for _ in 0..count {
            n.frame();
        }
    }

    #[test]
    fn empty_narration_is_complete_immediately() {
        let mut n = CutsceneNarration::new(vec![]);
        assert!(n.is_complete());
        assert!(n.visible_lines().is_empty());
        assert_eq!(n.current_text(), None);
        assert!(!n.tick(1));
    }

    #[test]
    fn first_frame_admits_page_zero_into_the_bottom_slot() {
        // Primed sub-scroll 0xF + accumulator at the divisor: the very first
        // frame completes a line step.
        let mut n = CutsceneNarration::new(pages(3));
        n.frame();
        assert_eq!(n.current_index(), 1);
        let all = n.all_lines();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].text, "line 0");
        // Slot n = 8 at y = top + 16*8 - 0.
        assert_eq!(all[0].y, 0x40 + 16 * 8);
    }

    #[test]
    fn capture_state_after_ten_steps() {
        // s1_newgame_field: acc 3, +0x9C 1, +0x9E 9, slots [FF x8, 1] at a
        // frame step of 3 and divisor 4 - one step every two frames.
        let mut n = CutsceneNarration::new(pages(14));
        n.frame(); // init + first step (page 0 enters, sub 0)
        frames(&mut n, 18); // nine more steps
        n.frame(); // one frame of accumulation
        assert_eq!(n.accum, 3);
        assert_eq!(n.subscroll, 9);
        assert_eq!(n.current_index(), 1);
        assert_eq!(
            n.slots,
            vec![0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 1]
        );
    }

    #[test]
    fn frame_step_three_divisor_four_climbs_one_pixel_every_two_frames() {
        let mut n = CutsceneNarration::new(pages(3));
        n.frame();
        let y0 = n.all_lines()[0].y;
        n.frame();
        assert_eq!(n.all_lines()[0].y, y0, "3 < 4: no step");
        n.frame();
        assert_eq!(
            n.all_lines()[0].y,
            y0 - 1,
            "6 >= 4: step, remainder dropped"
        );
        // tick() counts vsyncs: two frames = six vsyncs.
        n.tick(6);
        assert_eq!(n.all_lines()[0].y, y0 - 2);
    }

    #[test]
    fn line_pitch_is_sixteen_and_the_window_clips_edge_rows() {
        let mut n = CutsceneNarration::new(pages(40));
        frames(&mut n, 2 * 16 * 12);
        let all = n.all_lines();
        for w in all.windows(2) {
            assert_eq!(w[1].y - w[0].y, LINE_PITCH);
        }
        let (top, bottom) = n.clip_window();
        assert_eq!((top, bottom), (0x44, 0x40 + 128 - 1));
        let vis = n.visible_lines();
        assert!(vis.len() >= 6 && vis.len() < all.len());
        assert!(vis.iter().all(|l| l.y >= top && l.y + 15 <= bottom));
    }

    #[test]
    fn seed_op_before_a_block_sets_the_geometry() {
        // [CC F8 E8][128][5][5][0] then the crawl op.
        let mut body = vec![0u8; 4];
        body.extend([0xCC, 0xF8, 0xE8, 0x80, 0, 5, 0, 5, 0, 0, 0]);
        let op = body.len();
        body.extend([0xCC, 0xF8, 0x80, 3]);
        let seed = RollerSeed::SCENE_RESET.config_op_before(&body, op).unwrap();
        assert_eq!(
            seed,
            RollerSeed {
                top: 128,
                slots: 5,
                divisor: 5,
                release: 0
            }
        );
        assert_eq!(seed.clip_window(), (132, 207));
        // Zero words take the op's defaults.
        let d = RollerSeed {
            top: 1,
            slots: 1,
            divisor: 1,
            release: 7,
        }
        .with_config_words(0, 0, 4);
        assert_eq!(
            d,
            RollerSeed {
                top: 0x40,
                slots: 8,
                divisor: 4,
                release: 7
            }
        );
        // No op directly before: None.
        assert!(RollerSeed::SCENE_RESET.config_op_before(&body, 3).is_none());
    }

    #[test]
    fn completes_when_every_page_retires_off_slot_zero() {
        let mut n = CutsceneNarration::new(pages(2));
        let mut guard = 0;
        while n.tick(3) {
            guard += 1;
            assert!(guard < 10_000);
        }
        assert!(n.is_complete());
        assert_eq!(n.retired(), 2);
        assert!(n.visible_lines().is_empty());
    }

    #[test]
    fn blank_pages_consume_a_slot_without_drawing() {
        let mut n = CutsceneNarration::new(vec!["a".into(), String::new(), "c".into()]);
        frames(&mut n, 1 + 2 * 16 * 2);
        let texts: Vec<&str> = n.all_lines().iter().map(|l| l.text).collect();
        assert_eq!(texts, vec!["a", "c"]);
    }

    #[test]
    fn release_pauses_the_crawl_at_the_line_count() {
        let mut n = CutsceneNarration::new(pages(20));
        n.set_release(3);
        let mut guard = 0;
        while !n.is_paused() {
            n.frame();
            guard += 1;
            assert!(guard < 10_000);
        }
        assert_eq!(n.seed().release, 0, "+0x52 is cleared");
        let before = n.all_lines();
        let snapshot: Vec<i32> = before.iter().map(|l| l.y).collect();
        frames(&mut n, 50);
        let after: Vec<i32> = n.all_lines().iter().map(|l| l.y).collect();
        assert_eq!(snapshot, after, "paused: the accumulator stops");
        n.resume();
        frames(&mut n, 4);
        assert_ne!(n.all_lines()[0].y, snapshot[0]);
    }

    #[test]
    fn skip_page_forces_a_line_step() {
        let mut n = CutsceneNarration::new(pages(2));
        assert!(n.skip_page());
        let mut guard = 0;
        while n.skip_page() {
            guard += 1;
            assert!(guard < 64, "skip converges");
        }
        assert!(n.is_complete());
    }

    #[test]
    fn force_finish_completes_immediately() {
        let mut n = CutsceneNarration::new(pages(5));
        n.tick(60);
        n.force_finish();
        assert!(n.is_complete());
        assert!(n.visible_lines().is_empty());
        assert!(!n.tick(1));
    }
}
