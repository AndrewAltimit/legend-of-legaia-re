//! Publisher-logos boot phase.
//!
//! Runs before the title screen. Displays the four TIMs from PROT 0895
//! (`init.pak`) in sequence: PROKION → Contrail → SCEA → WARNING. Each
//! logo fades in, holds, fades out before advancing to the next.
//!
//! The session is renderer-free: it owns timing/state only. Engines
//! query `current_logo()` + `alpha()` each frame to drive the actual
//! blit. When [`PublisherLogosSession::is_done`] returns true, the
//! caller transitions to the title screen.

/// Per-logo timing (in frames @ 60 Hz). Tunable; retail timings TBD —
/// these match a typical PSX boot pacing.
const FADE_IN_FRAMES: u16 = 30;
const HOLD_FRAMES: u16 = 90;
const FADE_OUT_FRAMES: u16 = 30;
const FRAMES_PER_LOGO: u16 = FADE_IN_FRAMES + HOLD_FRAMES + FADE_OUT_FRAMES;

/// Total number of publisher logos shown during boot.
pub const LOGO_COUNT: usize = 4;

/// Per-logo `(cols, rows)` grid that describes how each TIM is sliced
/// into strips for on-screen layout.
///
/// PROKION (176×256) and SCEA (256×128) are stored as vertically-packed
/// sprite atlases in VRAM — retail boot draws `cols * rows` GPU quads
/// to unfold them. Source strips are stored in **column-major** order
/// (top to bottom in the bitmap = column 0 top to column 0 bottom, then
/// column 1 top to column 1 bottom, …); the output grid is row-major.
///
/// Without unfolding, blitting the whole TIM as one quad shows the
/// packed layout (e.g. PROKION as `PROK` over `KION` instead of
/// `PROK ☉ KION` side-by-side; SCEA as 4 rows of wrapped text instead
/// of a 2-line "Sony Computer Entertainment America / Presents" splash).
///
/// Indexed by logo order `[PROKION, Contrail, SCEA, WARNING]`:
/// - PROKION:  2 cols × 1 row  → 2 strips of 176×128, unfolds to 352×128
/// - Contrail: 1 col  × 1 row  → full TIM, no slicing
/// - SCEA:     2 cols × 2 rows → 4 strips of 256×32, unfolds to 512×64
/// - WARNING:  1 col  × 1 row  → full TIM, no slicing
pub const STRIP_GRID: [(u32, u32); LOGO_COUNT] = [(2, 1), (1, 1), (2, 2), (1, 1)];

/// One logo's atlas placement: source rect `(x, y, w, h)` in atlas
/// pixels.
pub type LogoRect = (u32, u32, u32, u32);

/// Pre-decoded publisher-logo atlas — vertically stacked RGBA pixels +
/// per-logo source rects. Build once from PROT 0895 bytes via
/// [`build_atlas_from_init_pak`], hand to engine-render's
/// `upload_sprite_atlas`, then sample the rect for the current logo
/// each frame.
#[derive(Debug, Clone)]
pub struct LogosAtlas {
    /// Stacked RGBA bytes — `4 * width * height`.
    pub rgba: Vec<u8>,
    pub width: u32,
    pub height: u32,
    /// Source rects in the same order the session walks (`PROKION,
    /// Contrail, SCEA, WARNING`).
    pub rects: [LogoRect; LOGO_COUNT],
}

/// Build a [`LogosAtlas`] by parsing PROT 0895 (`init.pak`) bytes and
/// decoding each of the four TIMs.
///
/// Atlas layout: vertically stacked, widest logo's width. Each logo is
/// flush-left within its row.
pub fn build_atlas_from_init_pak(prot_0895_bytes: &[u8]) -> anyhow::Result<LogosAtlas> {
    let pak = legaia_asset::init_pak::parse(prot_0895_bytes)?;
    let mut tims = Vec::with_capacity(LOGO_COUNT);
    for logo in &pak.logos {
        let tim = legaia_tim::parse(logo.bytes)?;
        let rgba = legaia_tim::decode_rgba8(&tim, 0)?;
        let w = tim.pixel_width() as u32;
        let h = tim.image.h as u32;
        if rgba.len() != (w * h * 4) as usize {
            anyhow::bail!(
                "publisher-logo TIM decode size mismatch: rgba={} w*h*4={}",
                rgba.len(),
                w * h * 4
            );
        }
        tims.push((rgba, w, h));
    }

    let atlas_w = tims.iter().map(|(_, w, _)| *w).max().unwrap_or(0);
    let atlas_h: u32 = tims.iter().map(|(_, _, h)| *h).sum();
    let mut atlas = vec![0u8; (atlas_w * atlas_h * 4) as usize];

    let mut rects: [LogoRect; LOGO_COUNT] = [(0, 0, 0, 0); LOGO_COUNT];
    let mut y_cursor: u32 = 0;
    for (i, (rgba, w, h)) in tims.iter().enumerate() {
        // Copy row-by-row into the atlas at (0, y_cursor).
        for row in 0..*h {
            let src_off = (row * w * 4) as usize;
            let dst_off = (((y_cursor + row) * atlas_w) * 4) as usize;
            let bytes_per_row = (*w * 4) as usize;
            atlas[dst_off..dst_off + bytes_per_row]
                .copy_from_slice(&rgba[src_off..src_off + bytes_per_row]);
        }
        rects[i] = (0, y_cursor, *w, *h);
        y_cursor += h;
    }

    Ok(LogosAtlas {
        rgba: atlas,
        width: atlas_w,
        height: atlas_h,
        rects,
    })
}

/// Phase within a single logo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogoPhase {
    /// Black → full opacity (`FADE_IN_FRAMES` frames).
    FadeIn,
    /// Full opacity hold (`HOLD_FRAMES` frames).
    Hold,
    /// Full opacity → black (`FADE_OUT_FRAMES` frames).
    FadeOut,
}

/// Boot-time publisher logos state machine.
#[derive(Debug, Clone)]
pub struct PublisherLogosSession {
    logo_idx: u8,
    frames_in_logo: u16,
    done: bool,
    /// When true, caller has signalled "skip the rest" (Start pressed).
    /// On the next tick we advance straight to `done`.
    skip_requested: bool,
}

impl Default for PublisherLogosSession {
    fn default() -> Self {
        Self::new()
    }
}

impl PublisherLogosSession {
    pub fn new() -> Self {
        Self {
            logo_idx: 0,
            frames_in_logo: 0,
            done: false,
            skip_requested: false,
        }
    }

    /// Advance one frame. Returns the [`LogoPhase`] that just ticked
    /// (or `None` if the session has finished).
    pub fn tick(&mut self) -> Option<LogoPhase> {
        if self.done {
            return None;
        }
        if self.skip_requested {
            self.done = true;
            return None;
        }
        let phase = self.phase();
        self.frames_in_logo += 1;
        if self.frames_in_logo >= FRAMES_PER_LOGO {
            self.frames_in_logo = 0;
            self.logo_idx += 1;
            if (self.logo_idx as usize) >= LOGO_COUNT {
                self.done = true;
            }
        }
        Some(phase)
    }

    /// Request that the session end on the next tick. Caller hooks this
    /// to Start being pressed during the boot sequence.
    pub fn request_skip(&mut self) {
        self.skip_requested = true;
    }

    pub fn is_done(&self) -> bool {
        self.done
    }

    /// Index of the logo currently being displayed (`0..LOGO_COUNT`).
    /// Returns `LOGO_COUNT` when the session is done.
    pub fn current_logo(&self) -> usize {
        if self.done {
            LOGO_COUNT
        } else {
            self.logo_idx as usize
        }
    }

    pub fn phase(&self) -> LogoPhase {
        if self.frames_in_logo < FADE_IN_FRAMES {
            LogoPhase::FadeIn
        } else if self.frames_in_logo < FADE_IN_FRAMES + HOLD_FRAMES {
            LogoPhase::Hold
        } else {
            LogoPhase::FadeOut
        }
    }

    /// Opacity in `[0.0, 1.0]` for the current logo this frame.
    /// `0.0` = fully black, `1.0` = fully visible.
    pub fn alpha(&self) -> f32 {
        if self.done {
            return 0.0;
        }
        match self.phase() {
            LogoPhase::FadeIn => self.frames_in_logo as f32 / FADE_IN_FRAMES as f32,
            LogoPhase::Hold => 1.0,
            LogoPhase::FadeOut => {
                let into_fadeout = self.frames_in_logo - (FADE_IN_FRAMES + HOLD_FRAMES);
                1.0 - (into_fadeout as f32 / FADE_OUT_FRAMES as f32)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn advances_through_all_four_logos() {
        let mut s = PublisherLogosSession::new();
        assert_eq!(s.current_logo(), 0);
        // Tick exactly one logo's worth of frames.
        for _ in 0..FRAMES_PER_LOGO {
            assert!(!s.is_done());
            s.tick();
        }
        assert_eq!(s.current_logo(), 1);

        for _ in 0..FRAMES_PER_LOGO {
            s.tick();
        }
        assert_eq!(s.current_logo(), 2);

        for _ in 0..FRAMES_PER_LOGO {
            s.tick();
        }
        assert_eq!(s.current_logo(), 3);

        for _ in 0..FRAMES_PER_LOGO {
            s.tick();
        }
        assert!(s.is_done());
        assert_eq!(s.current_logo(), LOGO_COUNT);
    }

    #[test]
    fn alpha_curves_match_phase_boundaries() {
        let mut s = PublisherLogosSession::new();
        // FadeIn starts at 0, climbs.
        assert_eq!(s.alpha(), 0.0);
        for _ in 0..(FADE_IN_FRAMES - 1) {
            s.tick();
        }
        // Just before Hold begins.
        let alpha_pre_hold = s.alpha();
        assert!(
            (alpha_pre_hold - (FADE_IN_FRAMES - 1) as f32 / FADE_IN_FRAMES as f32).abs() < 1e-6
        );
        s.tick();
        assert_eq!(s.phase(), LogoPhase::Hold);
        assert_eq!(s.alpha(), 1.0);

        // Skip ahead into FadeOut.
        for _ in 0..HOLD_FRAMES {
            s.tick();
        }
        assert_eq!(s.phase(), LogoPhase::FadeOut);
        // First FadeOut frame: alpha = 1.0 (just entered).
        assert!((s.alpha() - 1.0).abs() < 1e-6);
    }

    #[test]
    fn request_skip_ends_on_next_tick() {
        let mut s = PublisherLogosSession::new();
        // Tick into the middle of logo 1.
        for _ in 0..(FRAMES_PER_LOGO + 30) {
            s.tick();
        }
        assert_eq!(s.current_logo(), 1);
        s.request_skip();
        assert!(!s.is_done()); // not done yet
        s.tick();
        assert!(s.is_done());
        assert_eq!(s.current_logo(), LOGO_COUNT);
    }
}
