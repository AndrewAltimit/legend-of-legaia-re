//! Opening-cutscene narration presenter - the retail subtitle **roller**.
//!
//! The opening prologue scenes (`opdeene` / `opstati` / `opurud`) play their
//! on-screen narration from inline ASCII text pages embedded in the scene
//! MAN's cutscene-timeline script (parsed by [`legaia_asset::cutscene_text`]).
//! Retail routes the introducing op (`0xCC 0xF8 0x80 N`, field-VM `0x4C`
//! outer-nibble-8 sub-0) to a dedicated on-screen-text actor whose handler is
//! `FUN_80037174` - a bottom-up **text crawl**, not a one-line caption:
//!
//! - one roller actor owns all `N` pages of a block; the *parent* timeline
//!   script halt-suspends at the op until the roller finishes;
//! - lines enter at the bottom of a clipped window and scroll upward one
//!   pixel per `frames_per_pixel` frames; each full `line_step` of climb
//!   admits the next page at the bottom;
//! - several lines are visible concurrently (up to 8 in `opdeene`), each
//!   drawn centered with all glyphs at once (no typewriter);
//! - the block completes when every page has scrolled out of the window,
//!   which un-halts the parent script.
//!
//! Geometry + speed are per-scene ([`RollerParams::for_scene`]), pinned from
//! a PCSX-Redux cold-boot pixel capture of the retail opening (0.5 px/frame;
//! `opurud` 1.0; `opdeene` window ~y64..188 at 18 px spacing, the other
//! scenes ~y128..203 at 16 px).
//!
//! [`CutsceneNarration`] is that roller as a small state machine - installed
//! on the world by [`crate::world::World::open_cutscene_narration`] (from the
//! timeline stepper when its PC reaches a narration block), ticked each frame
//! from [`crate::world::World::tick`], and rendered by the host from
//! [`Self::visible_lines`].
// PORT: FUN_80037174
// REF: FUN_8003BDE0

/// Frames per 1-pixel scroll step (retail default; 0.5 px/frame measured
/// across the opening in the cold-boot pixel capture).
pub const DEFAULT_FRAMES_PER_PIXEL: u32 = 2;
/// Pixel height of one text row (the roller's line step; `opdeene` uses 18).
pub const LINE_STEP_PX: i32 = 16;

/// Per-scene roller geometry / speed, pinned from the PCSX-Redux cold-boot
/// pixel capture of the retail opening (per-frame text-band tracking):
/// lines enter at `enter_y`, crawl up 1 pixel per `frames_per_pixel` frames,
/// and retire when they reach `exit_y`. `opdeene` runs the tall window
/// (up to 8 lines, 18 px spacing, exit mid-upper screen); the other opening
/// scenes run a short mid-screen window; `opurud` scrolls at double speed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RollerParams {
    /// Y where a new line enters (PSX framebuffer pixels, top of the row).
    pub enter_y: i32,
    /// Y at which a line retires (scrolls out of the window).
    pub exit_y: i32,
    /// Vertical spacing between consecutive lines.
    pub line_step: i32,
    /// Frames per 1-pixel scroll step.
    pub frames_per_pixel: u32,
}

impl RollerParams {
    /// The default short mid-screen window (opstati / map01 measurements).
    pub const DEFAULT: Self = Self {
        enter_y: 203,
        exit_y: 128,
        line_step: LINE_STEP_PX,
        frames_per_pixel: DEFAULT_FRAMES_PER_PIXEL,
    };

    /// Roller geometry for a scene label (capture-pinned per-scene values;
    /// unknown scenes get [`Self::DEFAULT`]).
    pub fn for_scene(label: &str) -> Self {
        match label {
            // Tall window: enter ~y188, fade out at y64..67, 18 px spacing.
            "opdeene" => Self {
                enter_y: 188,
                exit_y: 64,
                line_step: 18,
                frames_per_pixel: 2,
            },
            // Double-speed short window: enter ~y187, vanish y128.
            "opurud" => Self {
                enter_y: 187,
                exit_y: 128,
                line_step: 16,
                frames_per_pixel: 1,
            },
            _ => Self::DEFAULT,
        }
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
#[derive(Clone, Debug)]
pub struct CutsceneNarration {
    /// The subtitle pages, in entry order.
    pages: Vec<String>,
    /// Window geometry + speed (see [`RollerParams`]).
    params: RollerParams,
    /// Frame accumulator toward the next pixel step.
    clock: u32,
    /// Live lines: `(page index, current Y)`, oldest (highest) first.
    active: Vec<(usize, i32)>,
    /// Next page index to enter at the window bottom.
    next_page: usize,
    /// Pixels the newest line has climbed since entering (the next page
    /// enters after a full `line_step`).
    entered_px: i32,
    /// Set once every page has scrolled out (or the block was
    /// force-finished).
    complete: bool,
}

impl CutsceneNarration {
    /// Build a roller over `pages` with the default window. A roller with no
    /// pages is immediately [`complete`](Self::is_complete).
    pub fn new(pages: Vec<String>) -> Self {
        Self::with_params(pages, RollerParams::DEFAULT)
    }

    /// Build a roller with explicit per-scene geometry / speed.
    pub fn with_params(pages: Vec<String>, params: RollerParams) -> Self {
        let complete = pages.is_empty();
        Self {
            pages,
            params,
            clock: 0,
            active: Vec::new(),
            next_page: 0,
            // Primed so the first line enters on the first pixel step.
            entered_px: params.line_step.max(1) - 1,
            complete,
        }
    }

    /// Total number of pages in the block.
    pub fn page_count(&self) -> usize {
        self.pages.len()
    }

    /// Index of the next page to enter at the window bottom.
    pub fn current_index(&self) -> usize {
        self.next_page
    }

    /// `true` once every page has scrolled out (or the block was
    /// force-finished).
    pub fn is_complete(&self) -> bool {
        self.complete
    }

    /// The window geometry this roller runs.
    pub fn params(&self) -> RollerParams {
        self.params
    }

    /// The currently visible lines, top (oldest) first, each with its
    /// PSX-space Y. Empty once complete.
    pub fn visible_lines(&self) -> Vec<NarrationLine<'_>> {
        if self.complete {
            return Vec::new();
        }
        self.active
            .iter()
            .map(|&(page, y)| NarrationLine {
                y,
                text: self.pages[page].as_str(),
            })
            .collect()
    }

    /// The text of the newest (bottom-most) visible line, or `None` when
    /// nothing is on screen. Compatibility accessor for single-line hosts;
    /// prefer [`Self::visible_lines`].
    pub fn current_text(&self) -> Option<&str> {
        if self.complete {
            return None;
        }
        self.active
            .last()
            .map(|&(page, _)| self.pages[page].as_str())
    }

    /// Advance the roller by `frame_delta` frames: every
    /// `frames_per_pixel` frames each visible line climbs 1 pixel; a line
    /// reaching `exit_y` retires; a new page enters at `enter_y` once the
    /// newest line has climbed one full `line_step`. Returns `true` while
    /// the block is still on screen, `false` once complete (so the host can
    /// release the suspended timeline).
    pub fn tick(&mut self, frame_delta: u32) -> bool {
        if self.complete {
            return false;
        }
        self.clock = self.clock.saturating_add(frame_delta);
        let fpp = self.params.frames_per_pixel.max(1);
        while self.clock >= fpp && !self.complete {
            self.clock -= fpp;
            self.pixel_step();
        }
        !self.complete
    }

    /// One pixel of crawl: climb every line, retire top-outs, admit the next
    /// page on a full line step.
    fn pixel_step(&mut self) {
        for line in &mut self.active {
            line.1 -= 1;
        }
        let exit_y = self.params.exit_y;
        self.active.retain(|&(_, y)| y > exit_y);
        self.entered_px += 1;
        if self.next_page < self.pages.len() && self.entered_px >= self.params.line_step {
            self.entered_px = 0;
            self.active.push((self.next_page, self.params.enter_y));
            self.next_page += 1;
        }
        if self.next_page >= self.pages.len() && self.active.is_empty() {
            self.complete = true;
        }
    }

    /// Force one immediate line step (a debug / skip accelerator; retail has
    /// no per-line skip - the whole opening is skipped via the hand-off
    /// packet instead). Returns `true` while the block is still on screen.
    pub fn skip_page(&mut self) -> bool {
        if self.complete {
            return false;
        }
        for _ in 0..self.params.line_step.max(1) {
            self.pixel_step();
            if self.complete {
                break;
            }
        }
        self.clock = 0;
        !self.complete
    }

    /// Force-finish the block (retail config op `0x4C 0x88` mode 3): every
    /// remaining page is retired and the roller reports complete.
    pub fn force_finish(&mut self) {
        self.active.clear();
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

    #[test]
    fn empty_narration_is_complete_immediately() {
        let mut n = CutsceneNarration::new(vec![]);
        assert!(n.is_complete());
        assert!(n.visible_lines().is_empty());
        assert_eq!(n.current_text(), None);
        assert!(!n.tick(1));
    }

    #[test]
    fn first_line_enters_at_the_window_bottom() {
        let mut n = CutsceneNarration::new(pages(3));
        assert!(
            n.visible_lines().is_empty(),
            "nothing before the first step"
        );
        assert!(n.tick(DEFAULT_FRAMES_PER_PIXEL));
        let lines = n.visible_lines();
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].text, "line 0");
        assert_eq!(lines[0].y, RollerParams::DEFAULT.enter_y);
    }

    #[test]
    fn lines_crawl_upward_one_pixel_per_step() {
        let mut n = CutsceneNarration::new(pages(3));
        n.tick(DEFAULT_FRAMES_PER_PIXEL);
        let y0 = n.visible_lines()[0].y;
        n.tick(DEFAULT_FRAMES_PER_PIXEL);
        let y1 = n.visible_lines()[0].y;
        assert_eq!(
            y1,
            y0 - 1,
            "1 pixel up per {DEFAULT_FRAMES_PER_PIXEL} frames"
        );
    }

    #[test]
    fn a_full_line_period_brings_in_the_next_page() {
        let p = RollerParams::DEFAULT;
        let mut n = CutsceneNarration::new(pages(3));
        n.tick(p.frames_per_pixel); // line 0 enters
        n.tick(p.frames_per_pixel * p.line_step as u32);
        let lines = n.visible_lines();
        assert_eq!(lines.len(), 2, "two lines visible");
        assert_eq!(lines[0].text, "line 0");
        assert_eq!(lines[1].text, "line 1");
        assert_eq!(lines[1].y - lines[0].y, p.line_step);
    }

    #[test]
    fn completes_when_every_page_scrolls_out() {
        let p = RollerParams::DEFAULT;
        let mut n = CutsceneNarration::new(pages(2));
        // Entry cadence (2 line steps) + a full window traversal each.
        let window_px = (p.enter_y - p.exit_y) as u32;
        let steps = (2 * p.line_step as u32 + window_px + 4) * p.frames_per_pixel;
        assert!(!n.tick(steps), "roller completes after the crawl");
        assert!(n.is_complete());
        assert!(n.visible_lines().is_empty());
    }

    #[test]
    fn window_bounds_visible_line_count() {
        let p = RollerParams::DEFAULT;
        let mut n = CutsceneNarration::new(pages(40));
        n.tick(p.frames_per_pixel * p.line_step as u32 * 12);
        assert!(!n.is_complete());
        let max_lines = ((p.enter_y - p.exit_y) / p.line_step) as usize + 1;
        assert!(n.visible_lines().len() <= max_lines);
    }

    #[test]
    fn opdeene_params_run_the_tall_window() {
        let p = RollerParams::for_scene("opdeene");
        assert_eq!(p.line_step, 18);
        assert_eq!(p.exit_y, 64);
        let mut n = CutsceneNarration::with_params(pages(22), p);
        // Saturate: up to 7-8 lines visible concurrently (retail capture).
        n.tick(p.frames_per_pixel * p.line_step as u32 * 10);
        assert!(n.visible_lines().len() >= 6);
    }

    #[test]
    fn skip_page_forces_a_line_step() {
        let mut n = CutsceneNarration::new(pages(2));
        assert!(n.skip_page());
        assert_eq!(n.visible_lines().len(), 1);
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
        n.tick(DEFAULT_FRAMES_PER_PIXEL * 20);
        n.force_finish();
        assert!(n.is_complete());
        assert!(n.visible_lines().is_empty());
        assert!(!n.tick(1));
    }
}
