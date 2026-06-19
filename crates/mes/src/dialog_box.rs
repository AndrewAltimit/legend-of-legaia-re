//! Multi-segment **box packing** for field-VM inline dialogue.
//!
//! A field NPC's interaction text is a flat pool of `0x1F`-lead glyph lines (see
//! [`crate::picker`] for the option-menu side). The per-actor dialog state
//! machine `FUN_80039B7C` and the window pager `FUN_801D84D0` group consecutive
//! lines into a window of **`_DAT_801F2740 = 3`** text rows. This module decodes
//! that grouping.
//!
//! ## Grammar (pinned on real town01 disc bytes)
//!
//! Each line is `0x1F <glyphs> 0x00`. Lines are packed into a box back-to-back -
//! the byte after one line's `0x00` terminator being another `0x1F` means "same
//! box, next row". A box holds up to [`LINES_PER_BOX`] rows; the box ends at a
//! **post-page control byte** the pager reads in state `0x19`:
//!
//! | byte (`& 0x7F`) | meaning | [`Dispatch`] |
//! |---|---|---|
//! | `0x24` | advance to the next page, same conversation | [`Dispatch::NextPage`] |
//! | `0x48` | open a fresh box | [`Dispatch::NewBox`] |
//! | `0x2A` | resize the box | [`Dispatch::Resize`] |
//! | `0x25` | end the conversation | [`Dispatch::End`] |
//! | `0x4C 0xFF` | terminate / close | [`Dispatch::Terminate`] |
//! | `0x27`/`0x28`/`0x29` | open a 2/3/4-option menu | [`Dispatch::Picker`] |
//!
//! This is the same dispatch table the picker continuation byte uses
//! (`FUN_801D84D0`); the box-packing side just reaches it after up to three
//! plain lines instead of after the option list.
//!
//! ## `0xC?` two-byte escapes
//!
//! When the SM advances past a line it has shown (`FUN_80039B7C` state `0x2`,
//! the `for (; 0x1e < *pbVar4; ...)` loop), it masks `(*pbVar4 & 0xF0) == 0xC0`
//! and skips the following data byte as part of the same token. So a line body
//! containing e.g. `0xC1 0x00` (a character-name substitution whose argument is
//! `0x00`) is **not** truncated at that `0x00` - the escape's argument byte can
//! fall in the `0x00..=0x1E` terminator range without ending the line. The line
//! ends only at a terminator byte that is *not* a `0xC?` escape argument. The
//! standard [`Interpreter`] already decodes every `0xC0..=0xCF` byte as a
//! 2-byte token, so [`line_end`] reuses it and inherits the correct stride.
//!
//! REF: FUN_801D84D0  (window pager: `_DAT_801F2740` = 3-row capacity, the
//!                     state-`0x19` post-page dispatch table)

use std::ops::Range;

use crate::interp::{Interpreter, MesEvent};

/// Text rows a single dialog box holds before the pager pauses. Retail
/// `_DAT_801F2740`, pinned at both box-init arms (`case 6` / `case 9`) of the
/// window pager `FUN_801D84D0`.
pub const LINES_PER_BOX: usize = 3;

/// What the pager does after a box's rows are shown - decoded from the control
/// byte that follows the box's last line terminator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dispatch {
    /// `0x24` - advance to the next page of the same conversation (the window
    /// stays open; the next up-to-3 lines follow).
    NextPage,
    /// `0x48` - open a fresh box.
    NewBox,
    /// `0x2A` - box-geometry resize, then continue.
    Resize,
    /// `0x25` - end the conversation.
    End,
    /// `0x4C 0xFF` - terminate / close the window.
    Terminate,
    /// `0x27`/`0x28`/`0x29` - open a 2/3/4-option menu (the count is carried).
    Picker(usize),
    /// The box filled to [`LINES_PER_BOX`] and the next byte is another `0x1F`
    /// lead with no explicit control byte between - an implicit new page.
    ImplicitNextPage,
    /// Ran off the end of the buffer with no dispatch byte.
    EndOfBuffer,
    /// A control byte not in the pager's post-page dispatch set.
    Unknown(u8),
}

impl Dispatch {
    /// `true` when more dialogue follows in the same conversation branch - the
    /// pager should page-break and continue, not end.
    pub fn continues(self) -> bool {
        matches!(
            self,
            Dispatch::NextPage | Dispatch::NewBox | Dispatch::Resize | Dispatch::ImplicitNextPage
        )
    }
}

/// One decoded dialog box: up to [`LINES_PER_BOX`] line glyph-byte ranges plus
/// the control byte that ended it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DialogBox {
    /// Index of the box's first `0x1F` lead in the source buffer.
    pub lead: usize,
    /// Glyph-byte ranges (after each line's `0x1F` lead, up to its `0x00`
    /// terminator), in display order.
    pub lines: Vec<Range<usize>>,
    /// Index of the dispatch control byte (one past the last line's terminator),
    /// or the buffer length if the box ran to the end.
    pub dispatch_at: usize,
    /// What the pager does after this box.
    pub dispatch: Dispatch,
}

impl DialogBox {
    /// Where the next box begins, given this box's dispatch - or `None` when the
    /// conversation ends here (`End`/`Terminate`/`Picker`/`EndOfBuffer`/unknown).
    /// `NextPage`/`Resize` consume their 1-byte control; `Terminate` is 2 bytes
    /// but ends the branch; `ImplicitNextPage`/`NewBox`'s next lead is already at
    /// `dispatch_at`.
    pub fn next_box_pc(&self) -> Option<usize> {
        match self.dispatch {
            Dispatch::NextPage | Dispatch::Resize | Dispatch::NewBox => Some(self.dispatch_at + 1),
            Dispatch::ImplicitNextPage => Some(self.dispatch_at),
            Dispatch::End
            | Dispatch::Terminate
            | Dispatch::Picker(_)
            | Dispatch::EndOfBuffer
            | Dispatch::Unknown(_) => None,
        }
    }
}

/// End index of the `0x1F`-lead line whose glyph run starts at `from` (one past
/// the lead). Returns the index of the `0x00` terminator (where the standard MES
/// [`Interpreter`] halts), honoring `0xC0..=0xCF` 2-byte escapes so a `0x00`
/// argument byte doesn't end the line early. If the line never terminates the
/// buffer length is returned.
pub fn line_end(buf: &[u8], from: usize) -> usize {
    let mut interp = Interpreter::new_at(buf, from);
    loop {
        match interp.next_event() {
            // `pc()` after EndOfMessage sits one past the 0x00; back up to the
            // terminator index itself.
            Some(MesEvent::EndOfMessage(_)) => break interp.pc().saturating_sub(1),
            None => break interp.pc(),
            _ => {}
        }
    }
}

/// Classify the post-page control byte at `idx`, reading one byte ahead for the
/// `0x4C 0xFF` terminate marker.
fn classify_dispatch(buf: &[u8], idx: usize) -> Dispatch {
    let Some(&b) = buf.get(idx) else {
        return Dispatch::EndOfBuffer;
    };
    match b & 0x7f {
        0x1F => Dispatch::ImplicitNextPage, // another lead with no control byte
        0x24 => Dispatch::NextPage,
        0x48 => Dispatch::NewBox,
        0x2A => Dispatch::Resize,
        0x25 => Dispatch::End,
        0x27 => Dispatch::Picker(2),
        0x28 => Dispatch::Picker(3),
        0x29 => Dispatch::Picker(4),
        0x4C if buf.get(idx + 1).copied() == Some(0xFF) => Dispatch::Terminate,
        _ => Dispatch::Unknown(b),
    }
}

/// Pack one dialog box starting at `pc`. `pc` should point at a `0x1F` lead;
/// returns `None` if it doesn't. Collects up to [`LINES_PER_BOX`] consecutive
/// `0x1F` lines and decodes the control byte that follows the last one.
///
/// Ports the line-grouping + box-advance of the per-actor dialog SM: the
/// state-`0x2` loop in `FUN_80039B7C` that walks a shown line (`for (; 0x1e <
/// *pbVar4; ...)`, skipping `0xC?` 2-byte escapes) and stops at the next yield
/// byte. `World::step_inline_dialogue`'s port of `FUN_80039B7C` drives the
/// per-segment VM stepping; this is the box-packing half it doesn't cover.
// PORT: FUN_80039B7C
pub fn pack_box(buf: &[u8], pc: usize) -> Option<DialogBox> {
    if buf.get(pc) != Some(&0x1F) {
        return None;
    }
    let lead = pc;
    let mut lines = Vec::with_capacity(LINES_PER_BOX);
    let mut cur = pc;
    loop {
        // cur points at a 0x1F lead.
        let glyph_start = cur + 1;
        let term = line_end(buf, glyph_start);
        lines.push(glyph_start..term);
        // Position just past this line's 0x00 terminator.
        let after = (term + 1).min(buf.len());
        if lines.len() >= LINES_PER_BOX {
            // Box is full - the dispatch byte is whatever sits here.
            return Some(DialogBox {
                lead,
                lines,
                dispatch_at: after,
                dispatch: classify_dispatch(buf, after),
            });
        }
        if buf.get(after) == Some(&0x1F) {
            // Same box, next row.
            cur = after;
            continue;
        }
        return Some(DialogBox {
            lead,
            lines,
            dispatch_at: after,
            dispatch: classify_dispatch(buf, after),
        });
    }
}

/// Pack the whole conversation branch starting at `pc` - every box reachable by
/// following [`Dispatch::continues`] dispatches, stopping at the first box that
/// ends the branch (`End`/`Terminate`/`Picker`/`EndOfBuffer`/unknown) or after
/// `max_boxes` (a guard against a malformed stream that never terminates).
pub fn pack_boxes(buf: &[u8], pc: usize, max_boxes: usize) -> Vec<DialogBox> {
    let mut boxes = Vec::new();
    let mut at = pc;
    while boxes.len() < max_boxes {
        let Some(b) = pack_box(buf, at) else { break };
        let next = b.next_box_pc();
        let cont = b.dispatch.continues();
        boxes.push(b);
        match (cont, next) {
            (true, Some(n)) if n > at => at = n,
            _ => break,
        }
    }
    boxes
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(text: &[u8]) -> Vec<u8> {
        let mut v = vec![0x1F];
        v.extend_from_slice(text);
        v.push(0x00);
        v
    }

    #[test]
    fn packs_three_lines_into_one_box() {
        let mut b = Vec::new();
        b.extend(line(b"line one"));
        b.extend(line(b"line two"));
        b.extend(line(b"line three"));
        b.push(0x24); // next page
        b.extend(line(b"page two"));

        let bx = pack_box(&b, 0).expect("box at 0");
        assert_eq!(bx.lines.len(), 3, "three rows pack into one box");
        assert_eq!(&b[bx.lines[0].clone()], b"line one");
        assert_eq!(&b[bx.lines[2].clone()], b"line three");
        assert_eq!(bx.dispatch, Dispatch::NextPage);
        let next = bx.next_box_pc().unwrap();
        assert_eq!(
            b.get(next),
            Some(&0x1F),
            "next box starts at the page-two lead"
        );
    }

    #[test]
    fn box_caps_at_lines_per_box_even_without_control_byte() {
        // Four consecutive lines, no control byte between - the first box must
        // cap at LINES_PER_BOX and report ImplicitNextPage.
        let mut b = Vec::new();
        for t in [&b"a"[..], b"b", b"c", b"d"] {
            b.extend(line(t));
        }
        let bx = pack_box(&b, 0).unwrap();
        assert_eq!(bx.lines.len(), LINES_PER_BOX);
        assert_eq!(bx.dispatch, Dispatch::ImplicitNextPage);
        // The fourth line is the start of the next box.
        let next = bx.next_box_pc().unwrap();
        assert_eq!(&b[next..], &line(b"d")[..]);
    }

    #[test]
    fn two_line_box_then_picker() {
        let mut b = Vec::new();
        b.extend(line(b"Tetsu: ..,"));
        b.extend(line(b"do you want something today?"));
        b.push(0x29); // 4-option picker
        let bx = pack_box(&b, 0).unwrap();
        assert_eq!(bx.lines.len(), 2);
        assert_eq!(bx.dispatch, Dispatch::Picker(4));
        assert!(!bx.dispatch.continues());
        assert_eq!(bx.next_box_pc(), None);
    }

    #[test]
    fn wide_glyph_zero_argument_does_not_truncate_line() {
        // 0xC1 0x00 (character-name substitution with arg 0x00) inside a line:
        // the 0x00 must NOT be read as the line terminator.
        let mut b = vec![0x1F];
        b.extend_from_slice(b"Mist appeared");
        b.extend_from_slice(&[0xC1, 0x00]); // escape with 0x00 arg
        b.extend_from_slice(b", but");
        b.push(0x00); // real terminator
        b.push(0x25); // end
        let bx = pack_box(&b, 0).unwrap();
        assert_eq!(bx.lines.len(), 1);
        // The line glyph range must span past the 0xC1 0x00 to the real 0x00.
        let glyphs = &b[bx.lines[0].clone()];
        assert!(
            glyphs.ends_with(b", but"),
            "line must include the bytes after the 0xC1 0x00 escape, got {glyphs:02X?}"
        );
        assert_eq!(bx.dispatch, Dispatch::End);
    }

    #[test]
    fn pack_boxes_follows_next_page_until_picker() {
        let mut b = Vec::new();
        // page 1 (3 lines) + NextPage
        b.extend(line(b"a"));
        b.extend(line(b"b"));
        b.extend(line(b"c"));
        b.push(0x24);
        // page 2 (2 lines) + picker
        b.extend(line(b"d"));
        b.extend(line(b"e"));
        b.push(0x27);
        let boxes = pack_boxes(&b, 0, 16);
        assert_eq!(boxes.len(), 2);
        assert_eq!(boxes[0].dispatch, Dispatch::NextPage);
        assert_eq!(boxes[1].dispatch, Dispatch::Picker(2));
    }

    #[test]
    fn end_of_buffer_when_no_dispatch() {
        let b = line(b"only line"); // no control byte after
        let bx = pack_box(&b, 0).unwrap();
        assert_eq!(bx.lines.len(), 1);
        assert_eq!(bx.dispatch, Dispatch::EndOfBuffer);
        assert_eq!(bx.next_box_pc(), None);
    }
}
