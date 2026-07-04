//! Typed accessors over the PSX `GPU` section.
//!
//! Mednafen's PSX module stores the full 1 MiB of VRAM under the `GPU`
//! section as the sub-entry `&GPURAM[0][0]`, alongside the CLUT/texture
//! caches, draw-area / clip rectangles, texture-page registers, and the
//! `BlitterFIFO` state. This module exposes the entries Legaia RE work
//! actually needs - the raw VRAM blob and the small set of control
//! registers that determine *how* the runtime is sampling that VRAM at
//! the moment of capture.
//!
//! VRAM is 1024x512 16-bit pixels (BGR555 + STP). The byte layout is
//! linear: pixel `(x, y)` lives at `bytes[(y * 1024 + x) * 2 .. + 2]`,
//! little-endian. This matches `legaia_tim::Vram::pixel(x, y)`.

use crate::container::SaveState;

/// Width of the PSX GPU VRAM in pixels.
pub const VRAM_WIDTH: usize = 1024;
/// Height of the PSX GPU VRAM in pixels.
pub const VRAM_HEIGHT: usize = 512;
/// Size of the PSX GPU VRAM in bytes (1 MiB).
pub const VRAM_BYTES: usize = VRAM_WIDTH * VRAM_HEIGHT * 2;

/// Snapshot of GPU control state that determines how the runtime samples
/// VRAM at the moment of capture. All fields are populated lazily - read
/// what you need.
#[derive(Debug, Clone, Default)]
pub struct GpuRegs {
    /// Drawing area clip rect (inclusive low / exclusive high). Set by
    /// `GP0(0xE3)` / `GP0(0xE4)`.
    pub clip: Option<(u32, u32, u32, u32)>,
    /// Drawing offset added to every vertex. Set by `GP0(0xE5)`.
    pub draw_offset: Option<(i32, i32)>,
    /// `(mask_x, mask_y, off_x, off_y)` for the texture-window register
    /// (`GP0(0xE2)`).
    pub tex_window: Option<(u8, u8, u8, u8)>,
    /// `tww` / `twh` / `twx` / `twy` as stored by mednafen (same values
    /// as `tex_window` but split for clarity).
    pub tex_window_raw: Option<(u8, u8, u8, u8)>,
    /// Selected texture-page origin `(x, y)` in VRAM pixels. The mednafen
    /// state stores `TexPageX` / `TexPageY` as `u32` words.
    pub tex_page: Option<(u32, u32)>,
    /// Texture mode (color depth selector). 0=4bpp, 1=8bpp, 2=15bpp.
    pub tex_mode: Option<u32>,
    /// Display framebuffer origin `(x, y)`. Set by `GP1(0x05)`.
    pub display_fb: Option<(u32, u32)>,
    /// `(width, height)` of the currently-active display window. Mednafen
    /// stores horizontal / vertical start+end which we keep as-is.
    pub display_h_range: Option<(u32, u32)>,
    pub display_v_range: Option<(u32, u32)>,
    /// `1` if display is off (GP1(0x03) bit 0).
    pub display_off: Option<bool>,
    /// Display mode bits exactly as stored by mednafen.
    pub display_mode_raw: Option<u32>,
}

impl GpuRegs {
    /// Decode the on-screen display resolution `(width, height)` in pixels
    /// from the `GP1(0x08)` display-mode bits (`display_mode_raw`).
    ///
    /// Horizontal: bit6 (HR2) forces 368; otherwise bits0-1 (HR1) select
    /// `256 / 320 / 512 / 640`. Vertical: 480 only when both the interlace
    /// bit (5) and the vertical-resolution bit (2) are set, else 240. This
    /// is the standard PSX GPU decode; Legaia runs 320x240.
    pub fn display_resolution(&self) -> Option<(u32, u32)> {
        let m = self.display_mode_raw?;
        let width = if m & 0x40 != 0 {
            368
        } else {
            [256u32, 320, 512, 640][(m & 0x3) as usize]
        };
        let height = if (m & 0x20 != 0) && (m & 0x04 != 0) {
            480
        } else {
            240
        };
        Some((width, height))
    }

    /// The VRAM sub-rectangle `(x, y, w, h)` the runtime is scanning out to
    /// the TV: the display framebuffer origin (`GP1(0x05)`) sized by the
    /// decoded resolution, clamped to the VRAM bounds. `None` when either
    /// the framebuffer origin or the display mode is absent.
    pub fn display_crop_rect(&self) -> Option<(u32, u32, u32, u32)> {
        let (fx, fy) = self.display_fb?;
        let (mut w, mut h) = self.display_resolution()?;
        w = w.min(VRAM_WIDTH as u32 - fx.min(VRAM_WIDTH as u32));
        h = h.min(VRAM_HEIGHT as u32 - fy.min(VRAM_HEIGHT as u32));
        Some((fx, fy, w, h))
    }
}

/// Extract an `(x, y, w, h)` sub-rectangle from a full-VRAM RGBA8 buffer
/// (`VRAM_WIDTH x VRAM_HEIGHT`, 4 bytes/pixel) into a tightly-packed
/// `w * h * 4` RGBA8 buffer. Rows are copied verbatim; the rect must lie
/// within the VRAM bounds.
pub fn crop_rgba(rgba: &[u8], rect: (u32, u32, u32, u32)) -> Vec<u8> {
    let (x, y, w, h) = rect;
    let (x, y, w, h) = (x as usize, y as usize, w as usize, h as usize);
    assert_eq!(
        rgba.len(),
        VRAM_WIDTH * VRAM_HEIGHT * 4,
        "crop_rgba: not full VRAM"
    );
    assert!(
        x + w <= VRAM_WIDTH && y + h <= VRAM_HEIGHT,
        "crop_rgba: rect out of bounds"
    );
    let mut out = Vec::with_capacity(w * h * 4);
    for row in 0..h {
        let start = ((y + row) * VRAM_WIDTH + x) * 4;
        out.extend_from_slice(&rgba[start..start + w * 4]);
    }
    out
}

/// Top-level helper over the `GPU` section.
#[derive(Debug, Clone, Copy)]
pub struct PsxGpu<'a> {
    save: &'a SaveState,
}

impl<'a> PsxGpu<'a> {
    pub fn new(save: &'a SaveState) -> Self {
        Self { save }
    }

    /// 1 MiB of VRAM bytes. Returns `None` if the save state doesn't
    /// expose the `&GPURAM[0][0]` entry (some non-PSX mednafen modules).
    pub fn vram_bytes(&self) -> Option<&'a [u8]> {
        let bytes = self.save.entry_bytes("GPU", "&GPURAM[0][0]")?;
        if bytes.len() != VRAM_BYTES {
            return None;
        }
        Some(bytes)
    }

    /// VRAM pixel `(x, y)` as a 16-bit word. `None` if VRAM isn't
    /// exposed or coords are out of range.
    pub fn vram_pixel(&self, x: u32, y: u32) -> Option<u16> {
        let bytes = self.vram_bytes()?;
        if x as usize >= VRAM_WIDTH || y as usize >= VRAM_HEIGHT {
            return None;
        }
        let off = ((y as usize) * VRAM_WIDTH + x as usize) * 2;
        Some(u16::from_le_bytes([bytes[off], bytes[off + 1]]))
    }

    /// Pull a single u32 entry by name.
    fn u32_entry(&self, name: &str) -> Option<u32> {
        let bytes = self.save.entry_bytes("GPU", name)?;
        if bytes.len() < 4 {
            return None;
        }
        Some(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    /// Pull a single u8 entry by name.
    fn u8_entry(&self, name: &str) -> Option<u8> {
        let bytes = self.save.entry_bytes("GPU", name)?;
        bytes.first().copied()
    }

    /// Read the GPU control-register snapshot.
    pub fn regs(&self) -> GpuRegs {
        let mut out = GpuRegs::default();
        if let (Some(x0), Some(y0), Some(x1), Some(y1)) = (
            self.u32_entry("ClipX0"),
            self.u32_entry("ClipY0"),
            self.u32_entry("ClipX1"),
            self.u32_entry("ClipY1"),
        ) {
            out.clip = Some((x0, y0, x1, y1));
        }
        if let (Some(ox), Some(oy)) = (self.u32_entry("OffsX"), self.u32_entry("OffsY")) {
            out.draw_offset = Some((ox as i32, oy as i32));
        }
        if let (Some(tww), Some(twh), Some(twx), Some(twy)) = (
            self.u8_entry("tww"),
            self.u8_entry("twh"),
            self.u8_entry("twx"),
            self.u8_entry("twy"),
        ) {
            out.tex_window = Some((tww, twh, twx, twy));
            out.tex_window_raw = Some((tww, twh, twx, twy));
        }
        if let (Some(tx), Some(ty)) = (self.u32_entry("TexPageX"), self.u32_entry("TexPageY")) {
            out.tex_page = Some((tx, ty));
        }
        out.tex_mode = self.u32_entry("TexMode");
        if let (Some(fx), Some(fy)) = (
            self.u32_entry("DisplayFB_XStart"),
            self.u32_entry("DisplayFB_YStart"),
        ) {
            out.display_fb = Some((fx, fy));
        }
        if let (Some(hs), Some(he)) = (self.u32_entry("HorizStart"), self.u32_entry("HorizEnd")) {
            out.display_h_range = Some((hs, he));
        }
        if let (Some(vs), Some(ve)) = (self.u32_entry("VertStart"), self.u32_entry("VertEnd")) {
            out.display_v_range = Some((vs, ve));
        }
        out.display_off = self.u8_entry("DisplayOff").map(|b| b != 0);
        out.display_mode_raw = self.u32_entry("DisplayMode");
        out
    }
}

/// Convert one PSX BGR555+STP word into an `(r, g, b, a)` RGBA8 tuple.
/// STP=0 with all-zero color → fully transparent; everything else is
/// opaque. Mirrors the asset-viewer's BGR555 decode.
pub fn bgr555_to_rgba8(word: u16) -> [u8; 4] {
    let r5 = (word & 0x1F) as u8;
    let g5 = ((word >> 5) & 0x1F) as u8;
    let b5 = ((word >> 10) & 0x1F) as u8;
    let stp = (word >> 15) & 1;
    let r = (r5 << 3) | (r5 >> 2);
    let g = (g5 << 3) | (g5 >> 2);
    let b = (b5 << 3) | (b5 >> 2);
    let a = if word == 0 && stp == 0 { 0 } else { 0xFF };
    [r, g, b, a]
}

/// Convert a full 1 MiB VRAM blob to a `1024 * 512 * 4`-byte RGBA8 buffer.
/// Same encoding rule as [`bgr555_to_rgba8`].
pub fn vram_to_rgba8(bytes: &[u8]) -> Vec<u8> {
    assert_eq!(bytes.len(), VRAM_BYTES, "vram_to_rgba8: wrong byte count");
    let mut rgba = Vec::with_capacity(VRAM_WIDTH * VRAM_HEIGHT * 4);
    for chunk in bytes.chunks_exact(2) {
        let w = u16::from_le_bytes([chunk[0], chunk[1]]);
        rgba.extend_from_slice(&bgr555_to_rgba8(w));
    }
    rgba
}

/// Count VRAM rows with at least one non-zero pixel - a cheap signal of
/// "how much texture data the runtime has uploaded so far".
pub fn nonzero_rows(bytes: &[u8]) -> usize {
    bytes
        .chunks_exact(VRAM_WIDTH * 2)
        .filter(|row| row.chunks_exact(2).any(|c| c != [0u8, 0u8]))
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::container::{MDFN_HEADER_LEN, MDFN_MAGIC, SECTION_NAME_LEN};

    fn build_save_with_gpu(entries: &[(&str, Vec<u8>)]) -> Vec<u8> {
        let mut body = Vec::new();
        for (name, value) in entries {
            body.push(name.len() as u8);
            body.extend_from_slice(name.as_bytes());
            body.extend_from_slice(&(value.len() as u32).to_le_bytes());
            body.extend_from_slice(value);
        }
        let mut name_buf = [0u8; SECTION_NAME_LEN];
        name_buf[..3].copy_from_slice(b"GPU");
        let mut payload = Vec::new();
        payload.extend_from_slice(MDFN_MAGIC);
        payload.extend_from_slice(&[0u8; MDFN_HEADER_LEN - MDFN_MAGIC.len()]);
        payload.extend_from_slice(&name_buf);
        payload.extend_from_slice(&(body.len() as u32).to_le_bytes());
        payload.extend_from_slice(&body);
        payload
    }

    #[test]
    fn bgr555_decodes_corners() {
        // Pure red in BGR555 → r5=0x1F, g5=0, b5=0, stp=0
        let red = 0x001F;
        assert_eq!(bgr555_to_rgba8(red), [0xFF, 0, 0, 0xFF]);
        // Pure black, stp=0 → transparent
        assert_eq!(bgr555_to_rgba8(0x0000), [0, 0, 0, 0]);
        // Black with stp=1 → opaque black
        assert_eq!(bgr555_to_rgba8(0x8000), [0, 0, 0, 0xFF]);
    }

    #[test]
    fn vram_bytes_returns_full_buffer() {
        let mut vram = vec![0u8; VRAM_BYTES];
        // Write a recognisable pixel at (3, 0) - little-endian 0xBEEF.
        vram[6] = 0xEF;
        vram[7] = 0xBE;
        let payload = build_save_with_gpu(&[("&GPURAM[0][0]", vram)]);
        let save = SaveState::from_decompressed(payload).unwrap();
        let gpu = PsxGpu::new(&save);
        let bytes = gpu.vram_bytes().unwrap();
        assert_eq!(bytes.len(), VRAM_BYTES);
        assert_eq!(gpu.vram_pixel(3, 0), Some(0xBEEF));
        assert_eq!(gpu.vram_pixel(0, 0), Some(0));
    }

    #[test]
    fn vram_pixel_rejects_out_of_range() {
        let vram = vec![0u8; VRAM_BYTES];
        let payload = build_save_with_gpu(&[("&GPURAM[0][0]", vram)]);
        let save = SaveState::from_decompressed(payload).unwrap();
        let gpu = PsxGpu::new(&save);
        assert_eq!(gpu.vram_pixel(VRAM_WIDTH as u32, 0), None);
        assert_eq!(gpu.vram_pixel(0, VRAM_HEIGHT as u32), None);
    }

    #[test]
    fn regs_reads_known_subset() {
        let payload = build_save_with_gpu(&[
            ("ClipX0", 8u32.to_le_bytes().to_vec()),
            ("ClipY0", 16u32.to_le_bytes().to_vec()),
            ("ClipX1", 320u32.to_le_bytes().to_vec()),
            ("ClipY1", 240u32.to_le_bytes().to_vec()),
            ("OffsX", 4u32.to_le_bytes().to_vec()),
            ("OffsY", 0u32.to_le_bytes().to_vec()),
            ("tww", vec![0x10]),
            ("twh", vec![0x20]),
            ("twx", vec![0x01]),
            ("twy", vec![0x02]),
            ("TexPageX", 640u32.to_le_bytes().to_vec()),
            ("TexPageY", 256u32.to_le_bytes().to_vec()),
            ("TexMode", 2u32.to_le_bytes().to_vec()),
            ("DisplayFB_XStart", 0u32.to_le_bytes().to_vec()),
            ("DisplayFB_YStart", 0u32.to_le_bytes().to_vec()),
            ("HorizStart", 0x260u32.to_le_bytes().to_vec()),
            ("HorizEnd", 0xC56u32.to_le_bytes().to_vec()),
            ("VertStart", 16u32.to_le_bytes().to_vec()),
            ("VertEnd", 256u32.to_le_bytes().to_vec()),
            ("DisplayOff", vec![0]),
            ("DisplayMode", 0u32.to_le_bytes().to_vec()),
        ]);
        let save = SaveState::from_decompressed(payload).unwrap();
        let regs = PsxGpu::new(&save).regs();
        assert_eq!(regs.clip, Some((8, 16, 320, 240)));
        assert_eq!(regs.draw_offset, Some((4, 0)));
        assert_eq!(regs.tex_window, Some((0x10, 0x20, 0x01, 0x02)));
        assert_eq!(regs.tex_page, Some((640, 256)));
        assert_eq!(regs.tex_mode, Some(2));
        assert_eq!(regs.display_fb, Some((0, 0)));
        assert_eq!(regs.display_off, Some(false));
    }

    #[test]
    fn display_resolution_decodes_modes() {
        let mk = |mode: u32| GpuRegs {
            display_mode_raw: Some(mode),
            display_fb: Some((0, 4)),
            ..Default::default()
        };
        // HR1=1 (320), no interlace → 320x240 (Legaia).
        assert_eq!(mk(0x01).display_resolution(), Some((320, 240)));
        assert_eq!(mk(0x00).display_resolution(), Some((256, 240)));
        assert_eq!(mk(0x02).display_resolution(), Some((512, 240)));
        assert_eq!(mk(0x03).display_resolution(), Some((640, 240)));
        // HR2 (bit6) forces 368 regardless of HR1.
        assert_eq!(mk(0x43).display_resolution(), Some((368, 240)));
        // interlace (bit5) + vres480 (bit2) → 480 tall.
        assert_eq!(mk(0x25).display_resolution(), Some((320, 480)));
        // No display_mode → None.
        assert_eq!(GpuRegs::default().display_resolution(), None);
    }

    #[test]
    fn display_crop_rect_uses_fb_origin() {
        let mut r = GpuRegs {
            display_mode_raw: Some(0x01), // 320x240
            display_fb: Some((0, 244)),
            ..Default::default()
        };
        assert_eq!(r.display_crop_rect(), Some((0, 244, 320, 240)));
        // Origin near the VRAM edge clamps the height.
        r.display_fb = Some((0, 400));
        assert_eq!(r.display_crop_rect(), Some((0, 400, 320, 112)));
    }

    #[test]
    fn crop_rgba_extracts_subrect() {
        let mut rgba = vec![0u8; VRAM_WIDTH * VRAM_HEIGHT * 4];
        // Mark pixel (5, 7) red.
        let off = (7 * VRAM_WIDTH + 5) * 4;
        rgba[off] = 0xFF;
        rgba[off + 3] = 0xFF;
        let cropped = crop_rgba(&rgba, (4, 6, 3, 3));
        assert_eq!(cropped.len(), 3 * 3 * 4);
        // (5,7) maps to local (col 1, row 1) in a 3-wide crop → pixel index
        // row*width + col, byte index *4.
        let (row, col, w) = (1usize, 1usize, 3usize);
        let local = (row * w + col) * 4;
        assert_eq!(&cropped[local..local + 4], &[0xFF, 0, 0, 0xFF]);
    }

    #[test]
    fn nonzero_rows_counts_active_rows() {
        let mut vram = vec![0u8; VRAM_BYTES];
        // Row 100 has one non-zero pixel at x=200.
        let off = (100 * VRAM_WIDTH + 200) * 2;
        vram[off] = 0xFF;
        // Row 300 has all-zero.
        // Row 400 has stp=1 black (nonzero word).
        let off2 = 400 * VRAM_WIDTH * 2;
        vram[off2 + 1] = 0x80;
        assert_eq!(nonzero_rows(&vram), 2);
    }
}
