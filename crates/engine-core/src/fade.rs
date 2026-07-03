//! Screen-fade primitive state - clean-room port of the retail fade-state
//! loader (`FUN_80020B00`, `see ghidra/scripts/funcs/80020b00.txt`).
//!
//! Retail stages full-screen fades as pool actors: `FUN_80024E80` allocates an
//! actor and calls the loader with a 13-`i16` template describing the ramp.
//! The loader converts the template into a 10.6 fixed-point state:
//!
//! ```text
//! state[0..2]  = start RGB << 6          ; current colour (10.6 fixed)
//! state[4..6]  = end RGB << 6
//! state[8..10] = ((end - start) * 0x40) / duration   ; per-frame delta
//! state[0x10]  = duration (frames)
//! ```
//!
//! so the displayed colour each frame is `current >> 6`, advancing linearly
//! and landing exactly on `end` after `duration` frames. The battle-action SM
//! stages the summon backdrop fade (state `0x33`) and the successful-escape
//! white-out (state `0x66`, template at `DAT_801C9070`) through this.

/// The 13-`i16` fade template `FUN_80020B00` consumes (`param_2` field
/// indices in brackets).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FadeTemplate {
    /// `[0]` - fade kind/id, copied verbatim onto the state.
    pub kind: i16,
    /// `[1]` - ramp duration in frames (the per-frame delta divisor).
    pub duration: i16,
    /// `[3..=5]` - start RGB.
    pub start_rgb: [i16; 3],
    /// `[7..=9]` - end RGB.
    pub end_rgb: [i16; 3],
    /// `[10]` / `[11]` / `[12]` - mode words copied verbatim onto the state
    /// (consumed by the pool actor's draw handler; semantics not yet pinned).
    pub mode: [i16; 3],
}

/// The successful-escape white-out template the battle-action SM writes at
/// `DAT_801C9070` before spawning the fade (state `0x66`): kind `2`, a
/// `0x40`-frame ramp from black `(0,0,0)` to white `(0xFF,0xFF,0xFF)`,
/// mode words `(0, -1, 0)`.
///
/// REF: FUN_801E295C (case 0x66 template write)
pub fn escape_fade_template() -> FadeTemplate {
    FadeTemplate {
        kind: 2,
        duration: 0x40,
        start_rgb: [0, 0, 0],
        end_rgb: [0xFF, 0xFF, 0xFF],
        mode: [0, -1i16, 0],
    }
}

/// Live fade state, the engine mapping of the retail actor's `+0x7C` block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FadeState {
    /// Fade kind (`state[0xc..]` as i32 in retail; template `[0]`).
    pub kind: i32,
    /// Current RGB, 10.6 fixed point.
    current_q6: [i16; 3],
    /// Target RGB, 10.6 fixed point.
    end_q6: [i16; 3],
    /// Per-frame delta, 10.6 fixed point.
    delta_q6: [i16; 3],
    /// Ramp duration in frames.
    pub duration: i16,
    /// Frames stepped so far.
    elapsed: i16,
    /// Mode words (template `[10..=12]`).
    pub mode: [i16; 3],
}

impl FadeState {
    /// Load a template into a live fade state, mirroring `FUN_80020B00`'s
    /// arithmetic exactly: start/end RGB promoted to 10.6 fixed point and the
    /// per-frame delta `((end - start) * 0x40) / duration` (i32 divide
    /// truncated to i16, as the retail store does).
    ///
    /// PORT: FUN_80020B00
    pub fn load(t: &FadeTemplate) -> FadeState {
        let duration = t.duration.max(1); // retail templates are never 0
        let mut current_q6 = [0i16; 3];
        let mut end_q6 = [0i16; 3];
        let mut delta_q6 = [0i16; 3];
        for c in 0..3 {
            current_q6[c] = t.start_rgb[c] << 6;
            end_q6[c] = t.end_rgb[c] << 6;
            delta_q6[c] =
                (((t.end_rgb[c] as i32 - t.start_rgb[c] as i32) * 0x40) / duration as i32) as i16;
        }
        FadeState {
            kind: t.kind as i32,
            current_q6,
            end_q6,
            delta_q6,
            duration,
            elapsed: 0,
            mode: t.mode,
        }
    }

    /// Advance the ramp one frame (the linear integrator the loader's
    /// state layout implies: `current += delta`, latching exactly on the
    /// target at the end of the ramp). Returns `true` while the fade is
    /// still running, `false` once it has completed. The retail pool
    /// actor's per-frame tick isn't dumped yet, so the latch-at-end is the
    /// engine's well-defined endpoint rather than a verified retail detail.
    pub fn step(&mut self) -> bool {
        if self.elapsed >= self.duration {
            return false;
        }
        self.elapsed += 1;
        if self.elapsed >= self.duration {
            self.current_q6 = self.end_q6;
            return false;
        }
        for c in 0..3 {
            self.current_q6[c] = self.current_q6[c].wrapping_add(self.delta_q6[c]);
        }
        true
    }

    /// The current display colour (`current >> 6`, clamped to a byte).
    pub fn rgb(&self) -> [u8; 3] {
        [
            (self.current_q6[0] >> 6).clamp(0, 255) as u8,
            (self.current_q6[1] >> 6).clamp(0, 255) as u8,
            (self.current_q6[2] >> 6).clamp(0, 255) as u8,
        ]
    }

    /// `true` once the ramp has run its full duration.
    pub fn finished(&self) -> bool {
        self.elapsed >= self.duration
    }

    /// Ramp progress in `0.0..=1.0` (for hosts that drive an overlay alpha).
    pub fn progress(&self) -> f32 {
        self.elapsed as f32 / self.duration.max(1) as f32
    }
}

/// Field-VM colour-fade overlay (op `0x34` sub-0, `FUN_801E1FB0`).
///
/// The field/cutscene fade path: a full-screen wash of one colour whose
/// *coverage* ramps over a short window. Unlike [`FadeState`] (which ramps the
/// RGB channels for the battle escape white-out), this holds a fixed colour
/// and ramps how much of the screen it covers - the shape the opening
/// prologue's white flash needs (`34 05 FF FF FF 00 00` = a white overlay that
/// fades to reveal the scene).
///
/// ## Approximate by design
///
/// The retail fade actor's per-frame draw handler is not dumped, so the exact
/// coverage curve + PSX blend mode are not pinned. This models the documented
/// setup (`FUN_801E1FB0`: colour = operand RGB, direction from `op0 & 1`,
/// zero-colour = clear) as a linear coverage ramp; the host draws it with a
/// semi-transparent wash. When the draw handler is dumped this can be made
/// byte-exact.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ColorFade {
    /// Overlay colour (operand RGB).
    pub rgb: [u8; 3],
    /// Ramp length in frames.
    pub frames: u16,
    /// Frames stepped so far.
    elapsed: u16,
    /// When `true`, coverage ramps 1.0 → 0.0 (a fade-*from*-colour reveal, the
    /// opening white flash); when `false`, 0.0 → 1.0 (a fade-*to*-colour).
    pub reveal: bool,
}

impl ColorFade {
    /// Default ramp length for a field colour fade (frames). Retail flashes
    /// are brief; the exact count is per-op and not pinned, so this is a
    /// reasonable opening-flash duration (~0.5 s at 60 fps).
    pub const DEFAULT_FRAMES: u16 = 32;

    /// Build a colour fade from an op-`0x34` sub-0 setup: `op0`'s low bit
    /// selects direction (`& 1` = reveal / fade-from-colour, the opening
    /// flash form), the RGB is the wash colour. A zero colour is *not* a fade;
    /// callers clear the overlay instead (mirrors retail's "all colour bytes
    /// zero → clear `_DAT_8007B62C`").
    ///
    /// REF: FUN_801E1FB0
    pub fn from_op34(op0: u8, rgb: [u8; 3]) -> ColorFade {
        ColorFade {
            rgb,
            frames: Self::DEFAULT_FRAMES,
            elapsed: 0,
            reveal: op0 & 1 != 0,
        }
    }

    /// Advance one frame. Returns `true` while still running, `false` once the
    /// ramp completes (the host then drops the overlay).
    pub fn step(&mut self) -> bool {
        if self.elapsed >= self.frames {
            return false;
        }
        self.elapsed += 1;
        self.elapsed < self.frames
    }

    /// Screen coverage in `0.0..=1.0` this frame: `reveal` ramps down from
    /// full, otherwise up from empty.
    pub fn coverage(&self) -> f32 {
        let p = self.elapsed as f32 / self.frames.max(1) as f32;
        if self.reveal { 1.0 - p } else { p }
    }

    /// The wash colour.
    pub fn rgb(&self) -> [u8; 3] {
        self.rgb
    }

    /// `true` once the ramp has completed.
    pub fn finished(&self) -> bool {
        self.elapsed >= self.frames
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn color_fade_reveal_ramps_coverage_down() {
        // op0 = 5 (low bit set) = reveal: coverage starts full, ends empty.
        let mut f = ColorFade::from_op34(0x05, [0xFF, 0xFF, 0xFF]);
        assert!(f.reveal);
        assert_eq!(f.rgb(), [0xFF, 0xFF, 0xFF]);
        assert_eq!(f.coverage(), 1.0);
        let mut frames = 0;
        while f.step() {
            frames += 1;
        }
        assert_eq!(frames + 1, ColorFade::DEFAULT_FRAMES as i32);
        assert!(f.finished());
        assert_eq!(f.coverage(), 0.0, "reveal lands fully transparent");
    }

    #[test]
    fn color_fade_cover_ramps_coverage_up() {
        // op0 = 4 (low bit clear) = fade-to-colour.
        let mut f = ColorFade::from_op34(0x04, [0, 0, 0]);
        assert!(!f.reveal);
        assert_eq!(f.coverage(), 0.0);
        while f.step() {}
        assert_eq!(f.coverage(), 1.0, "cover lands fully opaque");
    }

    #[test]
    fn loader_matches_the_retail_fixed_point_layout() {
        // start 0x20, end 0xFF over 0x40 frames: delta = (0xDF * 0x40)/0x40
        // = 0xDF in 10.6 - i.e. (end-start)/duration per displayed unit.
        let t = FadeTemplate {
            kind: 2,
            duration: 0x40,
            start_rgb: [0x20, 0x20, 0x20],
            end_rgb: [0xFF, 0xFF, 0xFF],
            mode: [0, 0, 0],
        };
        let f = FadeState::load(&t);
        assert_eq!(f.kind, 2);
        assert_eq!(f.rgb(), [0x20, 0x20, 0x20]);
        assert_eq!(f.delta_q6[0], ((0xFF - 0x20) * 0x40) / 0x40);
    }

    #[test]
    fn escape_fade_ramps_black_to_white_over_0x40_frames() {
        let mut f = FadeState::load(&escape_fade_template());
        assert_eq!(f.rgb(), [0, 0, 0]);
        assert_eq!(f.duration, 0x40);
        let mut frames = 0;
        while f.step() {
            frames += 1;
        }
        assert_eq!(frames + 1, 0x40, "ramp runs the template duration");
        assert!(f.finished());
        assert_eq!(f.rgb(), [0xFF, 0xFF, 0xFF], "lands exactly on white");
    }

    #[test]
    fn midpoint_is_linear() {
        let mut f = FadeState::load(&escape_fade_template());
        for _ in 0..0x20 {
            f.step();
        }
        let [r, ..] = f.rgb();
        // 0xFF*0x40/0x40 per frame in q6: after 32 frames ≈ 127.
        assert!((126..=128).contains(&r), "halfway ≈ mid grey, got {r}");
    }
}
