//! Glyph placement: atlas cell lookup, pen-advance layout and word wrap.

use super::*;

impl Font {
    /// Atlas top-left pixel coordinate for character `c`. Returns `None` for
    /// characters that don't have a glyph.
    pub fn glyph_origin(c: u8) -> Option<(u32, u32)> {
        if c < FIRST_CHAR {
            return None;
        }
        let col = (c & 0x0F) as u32;
        let row = ((c - FIRST_CHAR) >> 4) as u32;
        if row >= ROWS {
            return None;
        }
        Some((col * GLYPH_W, row * GLYPH_H))
    }

    /// Lay out `text` starting at `(0, 0)`. The caller translates by the
    /// pen position when uploading to the GPU.
    ///
    /// Newline byte (`0x7C`) advances the cursor down one line and resets X.
    /// Escape sequences (`0xCE`, `0xCF`) are recognized but their argument
    /// byte is consumed without rendering - the runtime substitution table
    /// is host-side state we don't model here.
    /// PORT: FUN_80035F04
    ///
    /// `FUN_80035F04` (SCUS `0x80035f04`) is retail's MES line-width measurement
    /// pass: it walks an MES message, handles `0x7C` newline by tracking the
    /// max-so-far pen advance, consumes the `0xCE` / `0xCF` 2-byte inline
    /// escapes without advancing, accumulates per-glyph widths from the font
    /// width table at `DAT_80073F1C` (the same proportional table this loader
    /// reads from `dialog_font_widths.csv`), and returns the max line width
    /// across all newline-separated lines. [`Layout::advance_x`] is the
    /// engine-side equivalent return value; this `layout` method is a superset
    /// that also emits the per-glyph positions for the renderer.
    pub fn layout(&self, text: &[u8]) -> Layout {
        self.layout_padded(text, 0)
    }

    /// [`Self::layout`] with retail's per-call glyph padding `DAT_800740E8`
    /// added to every printable byte's advance (the common tail of
    /// `FUN_80036888`'s byte loop, `0x80036B9C`: `widths[c] + pad + 1`). The
    /// control bytes and a `0xCE` sprite escape's own advance take no pad.
    /// Menus and battle draw at `0`; the field dialog pager `FUN_801D84D0`
    /// stores [`measure::DIALOG_GLYPH_PAD`] before each row (`0x801D97D8`).
    pub fn layout_padded(&self, text: &[u8], glyph_pad: u32) -> Layout {
        let mut glyphs = Vec::new();
        let mut pen_x: i32 = 0;
        let mut pen_y: i32 = 0;
        let mut max_x: i32 = 0;
        let mut i = 0;
        while i < text.len() {
            let c = text[i];
            i += 1;
            match c {
                0 => break,
                NEWLINE => {
                    if pen_x > max_x {
                        max_x = pen_x;
                    }
                    pen_x = 0;
                    // Saturating: a pathologically long all-newline input must
                    // not overflow the i32 pen cursor (panic in debug builds).
                    pen_y = pen_y.saturating_add(LINE_HEIGHT as i32);
                }
                // 0xCE = inline-escape (variable / string substitution),
                // 0xCF = color-change. Both consume the next byte. The
                // runtime substitution is host-side - we just skip the
                // operand here so the pen doesn't pretend to render it.
                // A sprite escape draws its icon at the pen and advances by
                // the table's width (`FUN_80036888`); without the sprites
                // attached, or for a numeric escape, it lays out as nothing.
                0xCE if i < text.len() => {
                    let op = text[i];
                    i += 1;
                    if let Some(c) = self.escape_cell(op) {
                        glyphs.push(LaidGlyph {
                            byte: ESCAPE_GLYPH_BYTE,
                            dst_x: pen_x,
                            dst_y: pen_y + i32::from(c.y_offset),
                            width: c.w,
                            height: c.h,
                            atlas_x: c.atlas_x,
                            atlas_y: c.atlas_y,
                        });
                        pen_x = pen_x.saturating_add(i32::from(c.advance));
                    }
                }
                0xCF if i < text.len() => {
                    i += 1;
                }
                0xCE | 0xCF => {}
                FIRST_CHAR..=0xFF => {
                    if let Some((ax, ay)) = Self::glyph_origin(c) {
                        glyphs.push(LaidGlyph {
                            byte: c,
                            dst_x: pen_x,
                            dst_y: pen_y,
                            width: GLYPH_W,
                            height: GLYPH_H,
                            atlas_x: ax,
                            atlas_y: ay,
                        });
                    }
                    pen_x = pen_x.saturating_add((self.advance_of(c) + glyph_pad) as i32);
                }
                _ => {
                    // Unprintable / out-of-range bytes (0x01..0x1F minus
                    // 0x7C). No glyph, no advance.
                }
            }
        }
        if pen_x > max_x {
            max_x = pen_x;
        }
        Layout {
            glyphs,
            advance_x: max_x.max(0) as u32,
            advance_y: (pen_y.max(0) as u32).saturating_add(LINE_HEIGHT),
        }
    }

    /// Convenience: lay out a string whose `char`s up to `U+00FF` are the
    /// dialog font's bytes (Latin-1): a `char` `U+0080..=U+00FF` is the one
    /// glyph byte of that value - the form the disc string readers hand back
    /// an accent cell in - and anything past `U+00FF` is laid out as its
    /// UTF-8 bytes. Use [`Font::layout`] directly when you have a pre-encoded
    /// dialog string.
    pub fn layout_ascii(&self, text: &str) -> Layout {
        let mut bytes = Vec::with_capacity(text.len());
        for c in text.chars() {
            match u8::try_from(u32::from(c)) {
                Ok(b) => bytes.push(b),
                Err(_) => bytes.extend_from_slice(c.encode_utf8(&mut [0; 4]).as_bytes()),
            }
        }
        self.layout(&bytes)
    }

    /// Lay out `text` with word-wrap at `box_width_px` pixels. Words
    /// (runs of non-space bytes) that would overflow the current line are
    /// pushed to the next line. Existing newlines (`0x7C`) are honoured.
    /// Single words longer than the box width are emitted as-is on their
    /// own line - no mid-word breaking.
    ///
    /// This is an engine convenience, not a retail behaviour: no retail
    /// text surface wraps. Every retail line break is an authored `0x7C`
    /// or a new `0x1F` dialog line, and `FUN_80036044` (once read as a wrap
    /// pass) returns the string's typewriter glyph count, not a layout. See
    /// `docs/formats/dialog-font.md` ("Line width and wrapping").
    pub fn layout_wrapped(&self, text: &[u8], box_width_px: u32) -> Layout {
        let wrapped = self.wrap_bytes(text, box_width_px);
        self.layout(&wrapped)
    }

    /// Insert `0x7C` newlines into `text` at word boundaries so each line
    /// fits within `box_width_px` when laid out. Returns a new byte buffer
    /// suitable for passing to [`Font::layout`].
    pub fn wrap_bytes(&self, text: &[u8], box_width_px: u32) -> Vec<u8> {
        let mut out = Vec::with_capacity(text.len() + 8);
        let mut line_w: u32 = 0;
        let mut i = 0;
        while i < text.len() {
            let c = text[i];
            if c == 0 {
                break;
            }
            if c == NEWLINE {
                out.push(c);
                line_w = 0;
                i += 1;
                continue;
            }
            // Inline-escape opcodes consume their operand byte without
            // emitting a glyph; mirror Font::layout's behaviour so the
            // wrapped width tracks rendering width.
            if (c == 0xCE || c == 0xCF) && i + 1 < text.len() {
                out.push(c);
                out.push(text[i + 1]);
                i += 2;
                continue;
            }
            if c == b' ' {
                if line_w.saturating_add(self.advance_of(c)) > box_width_px {
                    // Soft-break instead of emitting a trailing space.
                    out.push(NEWLINE);
                    line_w = 0;
                    i += 1;
                    continue;
                }
                out.push(c);
                line_w = line_w.saturating_add(self.advance_of(c));
                i += 1;
                continue;
            }
            // Find the end of the current word.
            let mut j = i;
            let mut word_w: u32 = 0;
            while j < text.len() {
                let cj = text[j];
                if cj == 0 || cj == b' ' || cj == NEWLINE {
                    break;
                }
                if (cj == 0xCE || cj == 0xCF) && j + 1 < text.len() {
                    j += 2;
                    continue;
                }
                word_w = word_w.saturating_add(self.advance_of(cj));
                j += 1;
            }
            if line_w > 0 && line_w.saturating_add(word_w) > box_width_px {
                // Trim trailing space if the previous emit was one.
                if matches!(out.last(), Some(&b' ')) {
                    out.pop();
                }
                out.push(NEWLINE);
                line_w = 0;
            }
            out.extend_from_slice(&text[i..j]);
            line_w = line_w.saturating_add(word_w);
            i = j;
        }
        out
    }
}
