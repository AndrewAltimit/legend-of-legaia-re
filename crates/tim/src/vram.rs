//! Software emulation of the PSX 1MB VRAM (1024×512 16-bit pixels).
//!
//! Used by the renderer to do faithful PSX texture lookups: each TMD
//! primitive carries a CBA (CLUT base address) and TSB (texture sub-base /
//! "tpage") that index into VRAM, not into any individual TIM. To resolve
//! them we need every TIM in the scene placed at its canonical fb_x/fb_y
//! position — which is exactly what the PSX BIOS does at boot when the
//! game DMAs each TIM to its `fb_x/fb_y` slot.
//!
//! [`Vram::upload_tim`] writes both the image block and the CLUT block at
//! the positions stored in the TIM header. Overlapping uploads use last-
//! wins (matches the real hardware: later DMA writes overwrite earlier ones).
//!
//! Pixels are stored in raw 16-bit form (BGR555 + STP). Decoding to RGBA
//! happens in the fragment shader so per-prim bit-depth + CLUT lookup
//! decisions stay on the GPU.

use crate::Tim;

/// PSX VRAM is 1024 pixels wide × 512 pixels tall, 16 bits per pixel.
pub const VRAM_WIDTH: usize = 1024;
pub const VRAM_HEIGHT: usize = 512;
pub const VRAM_PIXELS: usize = VRAM_WIDTH * VRAM_HEIGHT;

/// Software 1MB VRAM. Each cell is one 16-bit framebuffer word. Indexed
/// row-major: `pixels[y * VRAM_WIDTH + x]`.
#[derive(Clone)]
pub struct Vram {
    pixels: Vec<u16>,
}

impl Default for Vram {
    fn default() -> Self {
        Self::new()
    }
}

impl Vram {
    /// Fresh VRAM, all zeros.
    pub fn new() -> Self {
        Self {
            pixels: vec![0u16; VRAM_PIXELS],
        }
    }

    /// Raw pixel buffer in row-major order. Bytes are little-endian u16.
    pub fn as_u16(&self) -> &[u16] {
        &self.pixels
    }

    /// Same data, viewed as bytes — useful for GPU upload (R16Uint).
    pub fn as_bytes(&self) -> &[u8] {
        bytemuck::cast_slice(&self.pixels)
    }

    /// Upload a TIM's image block (and CLUT, if present) at the positions
    /// stored in the TIM header. Out-of-bounds writes are clipped.
    pub fn upload_tim(&mut self, tim: &Tim) {
        if let Some(clut) = tim.clut.as_ref() {
            self.write_words(clut.fb_x, clut.fb_y, clut.w, clut.h, &clut.entries);
        }
        // Image block: data is `fb_w * h` 16-bit words.
        let img = &tim.image;
        let n_words = img.fb_w as usize * img.h as usize;
        let mut words = Vec::with_capacity(n_words);
        for i in 0..n_words {
            let off = i * 2;
            if off + 2 > img.data.len() {
                break;
            }
            words.push(u16::from_le_bytes([img.data[off], img.data[off + 1]]));
        }
        self.write_words(img.fb_x, img.fb_y, img.fb_w, img.h, &words);
    }

    /// Write `w * h` 16-bit words into VRAM starting at `(x, y)`.
    /// Pixels falling outside `[0..VRAM_WIDTH) × [0..VRAM_HEIGHT)` are skipped.
    fn write_words(&mut self, x: u16, y: u16, w: u16, h: u16, src: &[u16]) {
        let x0 = x as usize;
        let y0 = y as usize;
        for row in 0..h as usize {
            let dy = y0 + row;
            if dy >= VRAM_HEIGHT {
                break;
            }
            for col in 0..w as usize {
                let dx = x0 + col;
                if dx >= VRAM_WIDTH {
                    break;
                }
                let src_idx = row * w as usize + col;
                if src_idx >= src.len() {
                    return;
                }
                self.pixels[dy * VRAM_WIDTH + dx] = src[src_idx];
            }
        }
    }

    /// Read one 16-bit pixel at `(x, y)`. Returns 0 outside VRAM.
    pub fn pixel(&self, x: usize, y: usize) -> u16 {
        if x >= VRAM_WIDTH || y >= VRAM_HEIGHT {
            return 0;
        }
        self.pixels[y * VRAM_WIDTH + x]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse;

    /// Build a 4-pixel 16bpp TIM at fb_x=64, fb_y=128 — easiest case to verify
    /// (no CLUT, image data goes straight into VRAM as-is).
    fn tim_16bpp_at(fb_x: u16, fb_y: u16) -> Vec<u8> {
        let mut buf = Vec::new();
        buf.extend_from_slice(&0x10u32.to_le_bytes());
        buf.extend_from_slice(&0x02u32.to_le_bytes()); // pmode 2 = 16bpp, no CLUT
        // Image: 4 pixels × 1 row at 16bpp = fb_w=4 words
        // bs_len = 12 + 4 * 1 * 2 = 20
        buf.extend_from_slice(&20u32.to_le_bytes());
        buf.extend_from_slice(&fb_x.to_le_bytes());
        buf.extend_from_slice(&fb_y.to_le_bytes());
        buf.extend_from_slice(&4u16.to_le_bytes()); // fb_w
        buf.extend_from_slice(&1u16.to_le_bytes()); // h
        // Four 16-bit pixels: 0xAAAA, 0xBBBB, 0xCCCC, 0xDDDD
        buf.extend_from_slice(&0xAAAAu16.to_le_bytes());
        buf.extend_from_slice(&0xBBBBu16.to_le_bytes());
        buf.extend_from_slice(&0xCCCCu16.to_le_bytes());
        buf.extend_from_slice(&0xDDDDu16.to_le_bytes());
        buf
    }

    #[test]
    fn upload_places_pixels_at_fbxy() {
        let mut vram = Vram::new();
        let buf = tim_16bpp_at(64, 128);
        let tim = parse(&buf).unwrap();
        vram.upload_tim(&tim);
        assert_eq!(vram.pixel(64, 128), 0xAAAA);
        assert_eq!(vram.pixel(65, 128), 0xBBBB);
        assert_eq!(vram.pixel(66, 128), 0xCCCC);
        assert_eq!(vram.pixel(67, 128), 0xDDDD);
        // Untouched cells remain zero.
        assert_eq!(vram.pixel(0, 0), 0);
        assert_eq!(vram.pixel(63, 128), 0);
        assert_eq!(vram.pixel(68, 128), 0);
    }

    #[test]
    fn last_upload_wins_on_overlap() {
        let mut vram = Vram::new();
        vram.upload_tim(&parse(&tim_16bpp_at(0, 0)).unwrap());
        // Same address, different data
        let mut buf = tim_16bpp_at(0, 0);
        // Patch the first pixel of the image block (offset 8 + 4 = 12 then +12 for image header)
        // Image block starts at offset 8 (after header), pixel data starts at 8+12 = 20.
        buf[20] = 0x11;
        buf[21] = 0x11;
        vram.upload_tim(&parse(&buf).unwrap());
        assert_eq!(vram.pixel(0, 0), 0x1111);
    }

    #[test]
    fn upload_clips_at_vram_edge() {
        // fb_x = 1023 leaves only one column inside VRAM
        let mut vram = Vram::new();
        let buf = tim_16bpp_at(1023, 0);
        vram.upload_tim(&parse(&buf).unwrap());
        assert_eq!(vram.pixel(1023, 0), 0xAAAA);
        // Other cells of the source were clipped, so this row stays zero past 1023.
        // (No way to read beyond VRAM_WIDTH; just verify nothing crashed.)
    }

    #[test]
    fn as_bytes_round_trips_le() {
        let mut vram = Vram::new();
        let buf = tim_16bpp_at(0, 0);
        vram.upload_tim(&parse(&buf).unwrap());
        let bytes = vram.as_bytes();
        // First 8 bytes = first 4 pixels = 0xAAAA, 0xBBBB, 0xCCCC, 0xDDDD (LE)
        assert_eq!(
            &bytes[0..8],
            &[0xAA, 0xAA, 0xBB, 0xBB, 0xCC, 0xCC, 0xDD, 0xDD]
        );
    }
}
