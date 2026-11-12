//! Publisher-logos boot phase.
//!
//! Runs before the title screen off the four TIMs in PROT 0895
//! (`init.pak`). Retail plays them from the overlay's own sequencer - an
//! actor tick on a 13-arm state machine - and the order it plays is
//! **SCEA → Contrail → PROKION**, not the order the TIMs sit in the
//! file. The fourth TIM (WARNING) is uploaded to VRAM and owns a sprite
//! descriptor, but no site in PROT 0895 emits its quad.
//!
//! The session is renderer-free: it owns timing/state only. Engines
//! query `current_logo()` + `alpha()` each frame and draw the quads
//! [`LOGO_QUADS`] names for that logo. When
//! [`PublisherLogosSession::is_done`] returns true, the caller
//! transitions to the title screen.
//!
//! Retail sources, all in `boot_init_pak` (PROT 0895) at the slot-A base
//! `0x801CE818`: sequencer `FUN_801CEFD4`, quad emitter `FUN_801CFBB8`,
//! the SCEA and PROKION pair drawers `FUN_801D0868` / `FUN_801D08F0`,
//! and the six-record sprite-descriptor table at `0x801F369C` (file
//! `+0x24E84`, immediately after the fourth TIM). Written up in
//! `docs/subsystems/boot.md`.

/// Total number of publisher-logo TIMs in `init.pak`.
pub const LOGO_COUNT: usize = 4;

/// Atlas / TIM index, in **file** order - the order
/// [`build_atlas_from_init_pak`] stacks them.
pub const LOGO_PROKION: usize = 0;
/// See [`LOGO_PROKION`].
pub const LOGO_CONTRAIL: usize = 1;
/// See [`LOGO_PROKION`].
pub const LOGO_SCEA: usize = 2;
/// See [`LOGO_PROKION`].
pub const LOGO_WARNING: usize = 3;

/// The screen retail runs the boot pass in.
///
/// `FUN_801CE9C0` calls `FUN_8001DAF8(0x400)`, the 640×480 wide mode,
/// and only the sequencer's last-but-one state switches back to 320
/// (`FUN_8001DAF8(0x140)`). Every `dst` rect in [`LOGO_QUADS`] is in
/// this space.
pub const STAGE: (u32, u32) = (640, 480);

/// One on-screen quad of one logo.
///
/// `src` is a rect in the logo's own decoded-TIM pixel space (the `u/v`
/// and `w/h` bytes of its descriptor record); `dst` is where retail puts
/// it in the [`STAGE`] framebuffer. `dst` follows the emitter's own
/// arithmetic: a descriptor carries a centre plus half-extents `w >> 1`
/// / `h >> 1`, so an odd source dimension loses its last row or column -
/// which is why the destination sizes below are not always the source
/// sizes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LogoQuad {
    /// `(x, y, w, h)` in the decoded TIM.
    pub src: (u32, u32, u32, u32),
    /// `(x, y, w, h)` in the 640×480 stage.
    pub dst: (i32, i32, u32, u32),
}

/// Retail per-logo quad layout, indexed by atlas index
/// (`LOGO_PROKION` … `LOGO_WARNING`).
///
/// PROKION and SCEA are **vertically packed** in their TIMs: retail
/// draws the top half and the bottom half as two quads side by side,
/// centred on the stage's `x = 320`. Contrail draws whole. Reading each
/// descriptor's `tpage`/`clut` back through the VRAM rects
/// `FUN_801CE9C0` writes is what assigns a record to a logo -
/// descriptors 0/5 carry PROKION's `tpage 0x9A` + `clut 0x7ED4`, 2/3
/// SCEA's `0x0A` / `0x7F14`, 4 Contrail's `0x9C` / `0x7F54` and 1
/// WARNING's `0x0B` / `0x7E80`.
///
/// WARNING's entry is **empty on purpose**: descriptor 1 exists and its
/// TIM is uploaded, but none of the five `FUN_801CFBB8` call sites in
/// PROT 0895 passes descriptor id 1, so the overlay never draws it.
// PORT: FUN_801cfbb8 (the POLY_GT4 logo-quad emitter)
// PORT: FUN_801d0868 (the SCEA pair draw)
// PORT: FUN_801d08f0 (the PROKION pair draw)
pub const LOGO_QUADS: [&[LogoQuad]; LOGO_COUNT] = [
    // PROKION - descriptors 0 and 5, drawn by FUN_801D08F0 at centres
    // (232, 228) and (408, 228) with half-extents (88, 63).
    &[
        LogoQuad {
            src: (0, 0, 176, 127),
            dst: (144, 165, 176, 126),
        },
        LogoQuad {
            src: (0, 128, 176, 127),
            dst: (320, 165, 176, 126),
        },
    ],
    // Contrail - descriptor 4, drawn straight from the sequencer at
    // centre (320, 232) with half-extents (92, 127).
    &[LogoQuad {
        src: (0, 0, 184, 254),
        dst: (228, 105, 184, 254),
    }],
    // SCEA - descriptors 2 and 3, drawn by FUN_801D0868 at centres
    // (320 - w/2, 224) and (320 + w/2, 224) with half-extents (126, 32).
    &[
        LogoQuad {
            src: (0, 0, 253, 64),
            dst: (68, 192, 252, 64),
        },
        LogoQuad {
            src: (0, 64, 252, 64),
            dst: (320, 192, 252, 64),
        },
    ],
    // WARNING - uploaded, descriptor 1, never drawn by PROT 0895.
    &[],
];

/// Neutral brightness in the retail emitter.
///
/// The logo quads are **opaque** `POLY_GT4`s (GP0 code `0x3C`); the fade
/// is the PSX texture blend `texel * colour / 128` applied to a vertex
/// colour of `(0xFF, 0xFF, 0xFF) * level >> 8`. So `level` runs `0`
/// (black) to `0x80` (unmodified texel), and no alpha channel is
/// involved at all.
pub const LEVEL_FULL: u16 = 0x80;

/// One logo's slot in the retail play order, in frames at 60 Hz.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LogoStep {
    /// Atlas index (`LOGO_PROKION` … `LOGO_WARNING`).
    pub logo: usize,
    /// Black frames ahead of the fade-in: the sequencer's settles, which
    /// draw nothing.
    pub before: u16,
    /// Frames ramping the brightness level `0` → [`LEVEL_FULL`].
    pub fade_in: u16,
    /// Frames at [`LEVEL_FULL`].
    pub hold: u16,
    /// Frames ramping back down to `0`.
    pub fade_out: u16,
}

impl LogoStep {
    /// Total frames this step occupies.
    pub const fn frames(&self) -> u16 {
        self.before + self.fade_in + self.hold + self.fade_out
    }
}

/// Retail play order and per-logo pacing, read off the sequencer's own
/// step sizes (`FUN_801CEFD4`, jump table `0x801CE8E8`).
///
/// - SCEA: state 3 adds `8` per frame to `0x80` (16 frames), state 4
///   counts `0x83` frames, state 5 adds `8` to `0x100` while the level
///   tracks `0x80 - t/2` (32 frames).
/// - Contrail: state 7 runs one counter `0` → `0x441` at `8` per frame
///   with the level clamped at `0x80`, so 16 frames of ramp and 121 of
///   hold; state 8 walks the same counter back down from `0x80`, and draws
///   its last frame at `0`, so PROKION opens on two black frames.
/// - PROKION: state 9 runs `0` → `0x351` at `8` per frame, again clamped
///   (16 frames of ramp, 92 at full with state 10's first). Its exit, state 10, is **not** a fade to black - it
///   holds PROKION at full while a full-screen blend quad
///   (`FUN_801D0460`) ramps the screen to white over `0x101` at
///   `4 × frame_delta` per tick. The port ramps the logo down over that
///   same 65-frame span instead of compositing the blend quad.
/// - The settles draw nothing: states 0..2 ahead of SCEA (two `0x11`-tick
///   counts either side of one frame, 35 frames), state 6 between SCEA and
///   Contrail (17), and states 11 and 12 after PROKION ([`TAIL_FRAMES`]).
///   Each step's `before` counts its black frames as the session shows them,
///   ticked then drawn: the frame that rolls into a step is its first.
pub const RETAIL_SEQUENCE: [LogoStep; 3] = [
    LogoStep {
        logo: LOGO_SCEA,
        before: 35,
        fade_in: 16,
        hold: 131,
        fade_out: 32,
    },
    LogoStep {
        logo: LOGO_CONTRAIL,
        before: 17,
        fade_in: 16,
        hold: 121,
        fade_out: 16,
    },
    LogoStep {
        logo: LOGO_PROKION,
        before: 1,
        fade_in: 16,
        hold: 92,
        fade_out: 65,
    },
];

/// Ticks after the one that ends PROKION's step, to the one the sequencer
/// raises its done bit on (`_DAT_801F3EB0 |= 2`): state 11's one frame and
/// state 12's `0x11`-tick settle, black throughout.
pub const TAIL_FRAMES: u16 = 17;

/// One logo's atlas placement: source rect `(x, y, w, h)` in atlas
/// pixels.
pub type LogoRect = (u32, u32, u32, u32);

/// Pre-decoded publisher-logo atlas - vertically stacked RGBA pixels +
/// per-logo source rects. Build once from PROT 0895 bytes via
/// [`build_atlas_from_init_pak`], hand to engine-render's
/// `upload_sprite_atlas`, then sample the rect for the current logo
/// each frame.
#[derive(Debug, Clone)]
pub struct LogosAtlas {
    /// Stacked RGBA bytes - `4 * width * height`.
    pub rgba: Vec<u8>,
    pub width: u32,
    pub height: u32,
    /// Source rects in **file** order (`PROKION, Contrail, SCEA,
    /// WARNING`), indexed by the `LOGO_*` constants - not the order
    /// [`RETAIL_SEQUENCE`] plays them in.
    pub rects: [LogoRect; LOGO_COUNT],
}

/// Build a [`LogosAtlas`] by parsing PROT 0895 (`init.pak`) bytes and
/// decoding each of the four TIMs.
///
/// Atlas layout: vertically stacked, widest logo's width. Each logo is
/// flush-left within its row.
///
/// This is the port's stand-in for the **upload** half of the mode-16
/// `READ INIT` body ([`read_init`]): retail rewrites each TIM's CLUT and
/// pixel destination rects ([`LOGO_VRAM`]) and hands the TIM to
/// `FUN_800198E0`; sampling an atlas rect replaces the VRAM page.
pub fn build_atlas_from_init_pak(prot_0895_bytes: &[u8]) -> anyhow::Result<LogosAtlas> {
    let pak = legaia_ui_formats::init_pak::parse(prot_0895_bytes)?;
    let mut tims = Vec::with_capacity(LOGO_COUNT);
    for logo in &pak.logos {
        let tim = psx_tim::parse(logo.bytes)?;
        let rgba = psx_tim::decode_rgba8(&tim, 0)?;
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

/// Where the mode-16 `READ INIT` body places one logo TIM in VRAM: it
/// rewrites the TIM's CLUT block and pixel block destinations (the `+4` /
/// `+6` halfwords of each) before the upload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LogoVram {
    /// The CLUT's VRAM `(x, y)`.
    pub clut: (u16, u16),
    /// The pixel block's VRAM `(x, y)`, in 16-bit VRAM words.
    pub pixels: (u16, u16),
    /// 8bpp (a 256-colour CLUT) rather than 4bpp.
    pub bpp8: bool,
}

impl LogoVram {
    /// The `tpage` word a sprite sampling this logo names.
    pub const fn tpage(&self) -> u16 {
        (self.pixels.0 / 64) | ((self.pixels.1 / 256) << 4) | if self.bpp8 { 0x80 } else { 0 }
    }

    /// The `clut` word a sprite sampling this logo names.
    pub const fn clut_word(&self) -> u16 {
        (self.clut.0 / 16) | (self.clut.1 << 6)
    }
}

/// [`LogoVram`] per logo, indexed by atlas index. Reading a sprite
/// descriptor's `tpage` / `clut` back through these is what binds the
/// descriptor to a logo ([`LOGO_QUADS`]).
pub const LOGO_VRAM: [LogoVram; LOGO_COUNT] = [
    // PROKION
    LogoVram {
        clut: (320, 507),
        pixels: (640, 256),
        bpp8: true,
    },
    // Contrail
    LogoVram {
        clut: (320, 509),
        pixels: (768, 256),
        bpp8: true,
    },
    // SCEA
    LogoVram {
        clut: (320, 508),
        pixels: (640, 0),
        bpp8: false,
    },
    // WARNING
    LogoVram {
        clut: (0, 506),
        pixels: (704, 0),
        bpp8: false,
    },
];

/// The order the body uploads the logos: SCEA and WARNING as their rects are
/// written, PROKION and Contrail once the display env is up.
pub const LOGO_UPLOAD_ORDER: [usize; LOGO_COUNT] =
    [LOGO_SCEA, LOGO_WARNING, LOGO_PROKION, LOGO_CONTRAIL];

/// The VRAM rect the body clears to black before the uploads (the `RECT` at
/// file `+0x08`).
pub const READ_INIT_CLEAR: (i16, i16, i16, i16) = (0, 0, 640, 500);

/// The 16-entry CLUT row the body loads after the uploads (the `RECT` at file
/// `+0x00`). Only its first two entries are set, `0x0000` and `0x5294`: the
/// rest of the 32-byte source is the routine's uninitialised stack.
pub const READ_INIT_CLUT_ROW: (i16, i16, i16, i16) = (0, 500, 16, 1);
/// See [`READ_INIT_CLUT_ROW`].
pub const READ_INIT_CLUT_HEAD: [u16; 2] = [0x0000, 0x5294];

/// The display mode the body selects (`FUN_8001DAF8`): the 640-wide one.
pub const READ_INIT_DISPLAY: u16 = 0x400;

/// The game mode the body stores at its end, `READ` (`0x11`).
pub const READ_INIT_NEXT_MODE: u16 = 0x11;

/// The sequencer's done word (`0x801F3EB0`) bit that ends the logos.
pub const LOGOS_DONE_BIT: u32 = 2;

/// The words the body's skip arms read.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ReadInitFlags {
    /// `_DAT_8007B98C` - the build's debug-mode word.
    pub debug_word: u32,
    /// `_DAT_8007B8C2` - the loader-path selector (non-zero is retail).
    pub build_mode: i16,
    /// `_DAT_8007B850` - the held pad word.
    pub pad: u32,
    /// `_DAT_8007B868` - the dev / dual-mode word.
    pub dev_word: u32,
}

impl ReadInitFlags {
    /// What a retail boot reads: no debug word, the retail loader path, no
    /// pad held, no dev word.
    pub const RETAIL: Self = Self {
        debug_word: 0,
        build_mode: 1,
        pad: 0,
        dev_word: 0,
    };
}

/// What the body leaves for the frames after it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReadInit {
    /// `_DAT_8007BB00`, the front-end entry word the title overlay reads.
    pub entry_word: u32,
    /// `0x801F3EB0`, the sequencer's done word: [`LOGOS_DONE_BIT`] set means
    /// the logos are skipped.
    pub done: u32,
    /// Whether the body raised the sequencer's load permit `0x801F3EA4`
    /// (bit 0), which it does exactly when the logos are skipped.
    pub permit: bool,
}

/// The mode-16 `READ INIT` body's decisions.
///
/// It raises the entry word and clears the done word, then: with the debug
/// word set on a dev build (`_DAT_8007B8C2 == 0`) it drops the entry word and
/// skips the logos - unless pad bit 0 is held, which puts both back; with the
/// dev word set it drops the entry word and skips the logos. A skip raises
/// the load permit so the loader runs at once.
///
/// The rest of the body is setup the host owns, with the values above: the
/// [`READ_INIT_DISPLAY`] mode, the [`READ_INIT_CLEAR`] clear, the
/// [`LOGO_VRAM`] uploads in [`LOGO_UPLOAD_ORDER`], the
/// [`READ_INIT_CLUT_ROW`] load, the pad init, the two boot actors and the
/// [`READ_INIT_NEXT_MODE`] store.
// PORT: FUN_801ce9c0 (mode-16 READ INIT body - the publisher-logo upload)
pub fn read_init(f: ReadInitFlags) -> ReadInit {
    let mut entry_word = 1;
    let mut done = 0;
    if f.debug_word != 0 && f.build_mode == 0 {
        entry_word = 0;
        done |= LOGOS_DONE_BIT;
        if f.pad & 1 != 0 {
            done = 0;
            entry_word = 1;
        }
    }
    if f.dev_word != 0 {
        entry_word = 0;
        done |= LOGOS_DONE_BIT;
    }
    ReadInit {
        entry_word,
        done,
        permit: done & LOGOS_DONE_BIT != 0,
    }
}

/// Phase within a single logo, sized by that logo's [`LogoStep`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogoPhase {
    /// A settle ahead of the logo: nothing drawn, over `LogoStep::before`
    /// frames.
    Black,
    /// Black → [`LEVEL_FULL`], over `LogoStep::fade_in` frames.
    FadeIn,
    /// Held at [`LEVEL_FULL`], over `LogoStep::hold` frames.
    Hold,
    /// [`LEVEL_FULL`] → black, over `LogoStep::fade_out` frames.
    FadeOut,
}

/// Boot-time publisher logos state machine.
///
/// Walks [`RETAIL_SEQUENCE`], so `step_idx` indexes the play order and
/// [`PublisherLogosSession::current_logo`] returns the **atlas** index
/// of whichever logo that step shows.
// PORT: FUN_801cefd4 (PROT 0895 publisher-logo sequencer)
#[derive(Debug, Clone)]
pub struct PublisherLogosSession {
    step_idx: u8,
    frames_in_logo: u16,
    /// Black ticks left after the last step, once it has ended.
    tail: u16,
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
            step_idx: 0,
            frames_in_logo: 0,
            tail: TAIL_FRAMES,
            done: false,
            skip_requested: false,
        }
    }

    /// The session the `READ INIT` body leaves: [`Self::new`], or already
    /// done when the body skipped the logos.
    pub fn after_read_init(init: &ReadInit) -> Self {
        let mut s = Self::new();
        s.done = init.done & LOGOS_DONE_BIT != 0;
        s
    }

    /// The [`RETAIL_SEQUENCE`] step now playing, or `None` when done.
    pub fn current_step(&self) -> Option<&'static LogoStep> {
        if self.done {
            None
        } else {
            RETAIL_SEQUENCE.get(self.step_idx as usize)
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
        let Some(step) = self.current_step() else {
            // The settles after the last logo: black until the done bit.
            self.tail = self.tail.saturating_sub(1);
            if self.tail == 0 {
                self.done = true;
            }
            return None;
        };
        let phase = self.phase();
        self.frames_in_logo += 1;
        if self.frames_in_logo >= step.frames() {
            self.frames_in_logo = 0;
            self.step_idx += 1;
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

    /// **Atlas** index of the logo currently displayed. Returns
    /// [`LOGO_COUNT`] when the session is done.
    pub fn current_logo(&self) -> usize {
        self.current_step().map_or(LOGO_COUNT, |s| s.logo)
    }

    /// The quads to draw for the current logo, in the [`STAGE`] space.
    pub fn current_quads(&self) -> &'static [LogoQuad] {
        match self.current_step() {
            Some(s) => LOGO_QUADS[s.logo],
            None => &[],
        }
    }

    pub fn phase(&self) -> LogoPhase {
        let Some(step) = self.current_step() else {
            return LogoPhase::FadeOut;
        };
        let Some(f) = self.frames_in_logo.checked_sub(step.before) else {
            return LogoPhase::Black;
        };
        if f < step.fade_in {
            LogoPhase::FadeIn
        } else if f < step.fade_in + step.hold {
            LogoPhase::Hold
        } else {
            LogoPhase::FadeOut
        }
    }

    /// Retail brightness level for this frame, `0 ..= `[`LEVEL_FULL`].
    ///
    /// This is the value the emitter multiplies the descriptor's vertex
    /// colour by; `LEVEL_FULL` leaves the texel unmodified.
    pub fn retail_level(&self) -> u16 {
        let Some(step) = self.current_step() else {
            return 0;
        };
        // `checked_div` covers the zero-length ramps: a zero `fade_in`
        // is never entered (the phase test is `frames < 0`) and a zero
        // `fade_out` cuts straight to black.
        let f = self.frames_in_logo.saturating_sub(step.before);
        match self.phase() {
            LogoPhase::Black => 0,
            LogoPhase::FadeIn => (LEVEL_FULL * f)
                .checked_div(step.fade_in)
                .unwrap_or(LEVEL_FULL),
            LogoPhase::Hold => LEVEL_FULL,
            LogoPhase::FadeOut => {
                let into_fadeout = f.saturating_sub(step.fade_in + step.hold);
                let drop = (LEVEL_FULL * into_fadeout)
                    .checked_div(step.fade_out)
                    .unwrap_or(LEVEL_FULL);
                LEVEL_FULL - drop.min(LEVEL_FULL)
            }
        }
    }

    /// Opacity in `[0.0, 1.0]` for the current logo this frame -
    /// [`retail_level`](Self::retail_level) normalised by
    /// [`LEVEL_FULL`]. `0.0` = fully black, `1.0` = fully visible.
    pub fn alpha(&self) -> f32 {
        if self.done {
            return 0.0;
        }
        self.retail_level() as f32 / LEVEL_FULL as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plays_the_retail_order_scea_contrail_prokion() {
        let mut s = PublisherLogosSession::new();
        let mut seen = Vec::new();
        // Long enough to outrun the whole sequence.
        for _ in 0..2000 {
            if s.is_done() {
                break;
            }
            let logo = s.current_logo();
            // The tail's settle shows no logo.
            if logo != LOGO_COUNT && seen.last() != Some(&logo) {
                seen.push(logo);
            }
            s.tick();
        }
        assert!(s.is_done());
        assert_eq!(seen, vec![LOGO_SCEA, LOGO_CONTRAIL, LOGO_PROKION]);
        assert_eq!(s.current_logo(), LOGO_COUNT);
    }

    #[test]
    fn each_step_lasts_its_retail_frame_count() {
        for (i, step) in RETAIL_SEQUENCE.iter().enumerate() {
            let mut s = PublisherLogosSession::new();
            // Run out every step before this one.
            for prev in &RETAIL_SEQUENCE[..i] {
                for _ in 0..prev.frames() {
                    s.tick();
                }
            }
            assert_eq!(s.current_logo(), step.logo);
            for _ in 0..(step.frames() - 1) {
                s.tick();
                assert_eq!(s.current_logo(), step.logo);
            }
            s.tick();
            assert_ne!(s.current_logo(), step.logo);
        }
    }

    #[test]
    fn level_ramps_between_zero_and_neutral() {
        let mut s = PublisherLogosSession::new();
        let step = RETAIL_SEQUENCE[0];
        assert_eq!(s.retail_level(), 0);
        assert_eq!(s.phase(), LogoPhase::Black);
        for _ in 0..step.before {
            s.tick();
            assert_eq!(s.alpha(), 0.0);
        }
        assert_eq!(s.phase(), LogoPhase::FadeIn);
        for _ in 0..step.fade_in {
            s.tick();
        }
        assert_eq!(s.phase(), LogoPhase::Hold);
        assert_eq!(s.retail_level(), LEVEL_FULL);
        assert_eq!(s.alpha(), 1.0);
        for _ in 0..step.hold {
            s.tick();
        }
        assert_eq!(s.phase(), LogoPhase::FadeOut);
        // The first fade-out frame is still at full.
        assert_eq!(s.retail_level(), LEVEL_FULL);
        for _ in 0..(step.fade_out - 1) {
            s.tick();
        }
        assert!(s.retail_level() < LEVEL_FULL);
    }

    #[test]
    fn quads_are_inside_the_retail_stage() {
        for (idx, quads) in LOGO_QUADS.iter().enumerate() {
            // WARNING has no draw site in PROT 0895.
            if idx == LOGO_WARNING {
                assert!(quads.is_empty());
                continue;
            }
            assert!(!quads.is_empty());
            for q in quads.iter() {
                let (x, y, w, h) = q.dst;
                assert!(x >= 0 && y >= 0, "quad {q:?} starts off-stage");
                assert!(
                    x as u32 + w <= STAGE.0 && y as u32 + h <= STAGE.1,
                    "quad {q:?} runs past the {STAGE:?} stage"
                );
                // The emitter's half-extent truncation: dst dimensions
                // are the source dimensions with the odd bit dropped.
                assert_eq!(w, q.src.2 & !1);
                assert_eq!(h, q.src.3 & !1);
            }
        }
    }

    #[test]
    fn packed_logos_split_their_tim_in_half_across_the_stage_centre() {
        for idx in [LOGO_PROKION, LOGO_SCEA] {
            let quads = LOGO_QUADS[idx];
            assert_eq!(quads.len(), 2, "logo {idx} should draw two quads");
            // The second strip starts one strip-height down in the TIM.
            assert_eq!(quads[0].src.1, 0);
            assert!(quads[1].src.1 >= quads[0].src.3);
            // Left quad ends where the right one starts, on x = 320.
            let (x0, _, w0, _) = quads[0].dst;
            assert_eq!(x0 + w0 as i32, STAGE.0 as i32 / 2);
            assert_eq!(quads[1].dst.0, STAGE.0 as i32 / 2);
        }
    }

    #[test]
    fn request_skip_ends_on_next_tick() {
        let mut s = PublisherLogosSession::new();
        for _ in 0..(RETAIL_SEQUENCE[0].frames() + 30) {
            s.tick();
        }
        assert_eq!(s.current_logo(), RETAIL_SEQUENCE[1].logo);
        s.request_skip();
        assert!(!s.is_done()); // not done yet
        s.tick();
        assert!(s.is_done());
        assert_eq!(s.current_logo(), LOGO_COUNT);
        assert!(s.current_quads().is_empty());
    }
}
