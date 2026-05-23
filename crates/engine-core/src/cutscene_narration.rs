//! Opening-cutscene narration presenter.
//!
//! The opening prologue scene (`opdeene`) plays its on-screen narration from
//! inline ASCII text pages embedded in the scene MAN's cutscene-timeline
//! script (parsed by [`legaia_asset::cutscene_text`]). This presenter is the
//! runtime that walks those pages on screen: one page at a time, auto-advanced
//! on a per-page frame timer (so the cutscene plays without input) and
//! skippable with a confirm press.
//!
//! It is a small state machine in the same mould as
//! [`crate::name_entry::NameEntry`] - installed on the world by
//! [`crate::world::World::open_cutscene_narration`], its timer advanced each
//! frame from [`crate::world::World::tick`], its current page rendered by the
//! host. When the last page finishes, the presenter reports `complete`, and
//! the host falls through to the prologue hand-off gate
//! ([`crate::world::World::take_prologue_handoff`]) - so the narration plays
//! first and the Rim Elm hand-off follows, mirroring the retail opening order.

/// Default frames each narration page stays on screen before auto-advancing.
/// Pinned to retail: the on-screen text actor's display timer is seeded to
/// `0x78` = 120 frames (≈2.0 s at 60 fps) in `FUN_8003C764` (the text-balloon
/// spawner the `0x4C` narration op routes to). A confirm press advances sooner.
// REF: FUN_8003C764
pub const DEFAULT_PAGE_FRAMES: u32 = 120;

/// A running narration: an ordered list of subtitle pages plus the cursor and
/// per-page timer that walk them.
#[derive(Clone, Debug)]
pub struct CutsceneNarration {
    /// The subtitle pages, in display order.
    pages: Vec<String>,
    /// Index of the page currently on screen.
    current: usize,
    /// Frames the current page has been shown.
    frames_on_page: u32,
    /// Frames a page is shown before it auto-advances.
    page_frames: u32,
    /// Set once the final page's dwell elapses (or it is skipped past).
    complete: bool,
}

impl CutsceneNarration {
    /// Build a presenter over `pages` with the default per-page dwell. A
    /// presenter with no pages is immediately [`complete`](Self::is_complete).
    pub fn new(pages: Vec<String>) -> Self {
        Self::with_page_frames(pages, DEFAULT_PAGE_FRAMES)
    }

    /// Build a presenter with an explicit per-page dwell (frames).
    pub fn with_page_frames(pages: Vec<String>, page_frames: u32) -> Self {
        let complete = pages.is_empty();
        Self {
            pages,
            current: 0,
            frames_on_page: 0,
            page_frames: page_frames.max(1),
            complete,
        }
    }

    /// Total number of pages.
    pub fn page_count(&self) -> usize {
        self.pages.len()
    }

    /// Index of the page currently on screen.
    pub fn current_index(&self) -> usize {
        self.current
    }

    /// The text of the page currently on screen, or `None` once complete.
    pub fn current_text(&self) -> Option<&str> {
        if self.complete {
            None
        } else {
            self.pages.get(self.current).map(String::as_str)
        }
    }

    /// `true` once every page has been shown (or skipped past).
    pub fn is_complete(&self) -> bool {
        self.complete
    }

    /// Advance the per-page timer by `frame_delta` frames, rolling to the next
    /// page when the dwell elapses and marking the presenter complete after
    /// the last page. Returns `true` while the narration is still on screen,
    /// `false` once complete (so the host can release the hand-off gate).
    pub fn tick(&mut self, frame_delta: u32) -> bool {
        if self.complete {
            return false;
        }
        self.frames_on_page = self.frames_on_page.saturating_add(frame_delta);
        while self.frames_on_page >= self.page_frames && !self.complete {
            self.frames_on_page -= self.page_frames;
            self.advance();
        }
        !self.complete
    }

    /// Skip immediately to the next page (a confirm press). Marks the
    /// presenter complete if it was already on the last page. Returns `true`
    /// while the narration is still on screen, `false` once complete.
    pub fn skip_page(&mut self) -> bool {
        if self.complete {
            return false;
        }
        self.frames_on_page = 0;
        self.advance();
        !self.complete
    }

    /// Move the cursor forward one page, marking complete past the last.
    fn advance(&mut self) {
        if self.current + 1 < self.pages.len() {
            self.current += 1;
        } else {
            self.complete = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_narration_is_complete_immediately() {
        let mut n = CutsceneNarration::new(vec![]);
        assert!(n.is_complete());
        assert_eq!(n.current_text(), None);
        assert!(!n.tick(1));
    }

    #[test]
    fn auto_advances_through_pages_on_the_timer() {
        let mut n = CutsceneNarration::with_page_frames(
            vec!["one".into(), "two".into(), "three".into()],
            10,
        );
        assert_eq!(n.current_text(), Some("one"));
        // 9 frames: still on page 0.
        assert!(n.tick(9));
        assert_eq!(n.current_text(), Some("one"));
        // One more frame rolls to page 1.
        assert!(n.tick(1));
        assert_eq!(n.current_text(), Some("two"));
        // Jump 10 frames -> page 2.
        assert!(n.tick(10));
        assert_eq!(n.current_text(), Some("three"));
        // Final page dwell elapses -> complete.
        assert!(!n.tick(10));
        assert!(n.is_complete());
        assert_eq!(n.current_text(), None);
    }

    #[test]
    fn a_large_delta_can_cross_multiple_pages() {
        let mut n =
            CutsceneNarration::with_page_frames(vec!["a".into(), "b".into(), "c".into()], 5);
        // 12 frames at 5/page crosses two boundaries (a->b->c), 2 frames into c.
        assert!(n.tick(12));
        assert_eq!(n.current_text(), Some("c"));
        assert!(!n.is_complete());
    }

    #[test]
    fn skip_page_advances_and_completes_past_the_last() {
        let mut n = CutsceneNarration::new(vec!["a".into(), "b".into()]);
        assert_eq!(n.current_text(), Some("a"));
        assert!(n.skip_page());
        assert_eq!(n.current_text(), Some("b"));
        // Skipping past the last page completes.
        assert!(!n.skip_page());
        assert!(n.is_complete());
        // Skipping again is a no-op.
        assert!(!n.skip_page());
    }
}
