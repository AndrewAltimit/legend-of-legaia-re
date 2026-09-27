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
//! | `0x25` | end the conversation | [`Dispatch::End`] |
//! | `0x4C 0xFF` | terminate / close | [`Dispatch::Terminate`] |
//! | `0x27`/`0x28`/`0x29` | open a 2/3/4-option menu | [`Dispatch::Picker`] |
//! | `0x2A` | open a 2-option menu, box geometry animated first | [`Dispatch::Picker`] |
//!
//! `0x2A` was once classified here as a box **resize** that continues the
//! conversation - a mis-reading the koin1 Prize Counter falsified: its
//! 3-row prompt ends `2A <j0:i16> <j1:i16>` followed by the `Yes`/`No`
//! labels, i.e. exactly the [`crate::picker`] wire format (which has always
//! decoded `0x2A` as the 2-option opener whose pager entry `0x11 -> 0x12`
//! animates the box geometry before the menu - every inn's Yes/No offer is
//! a `0x2A` menu). Classifying it as a continue made a runner page past the
//! jump table and end the conversation instead of arming the menu.
//!
//! This is the same dispatch table the picker continuation byte uses
//! (`FUN_801D84D0`); the box-packing side just reaches it after up to three
//! plain lines instead of after the option list.
//!
//! ## Why `End`/`Terminate` are grouped apart from `NewBox`
//!
//! The three box-open bytes all route through near-identical reset arms, which
//! makes them look interchangeable in the decompiled C. They are not, and the
//! difference is one word. Verified against the disassembly of PROT 0897 at base
//! `0x801CE818` (jump table `0x801CEBC0`, dispatch chain `0x801D8FDC`):
//!
//! - `0x25` -> state `0` (`0x801D90BC`) -> successor `1`
//! - `0x4C 0xFF` -> state `6` (`0x801D9174`) -> successor `7`
//! - `0x48` -> state `9` (`0x801D920C`) -> successor `0xA`
//!
//! Those three arms are byte-identical over their 0x98-byte extent *except* the
//! `li v0,N` that picks the successor. States `1`, `4` and `7` share handler
//! `0x801D8708`, which returns early when the state is `4` (so `0x24` keeps its
//! rows) and otherwise clears the row buffer - so `0x25` and `0x4C 0xFF` tear the
//! box down and are indistinguishable from each other. State `0xA` is a separate
//! handler (`0x801D92A4`) that runs the box-open animation. Hence
//! `End`/`Terminate` ending a packed run while `NewBox` continues it is the
//! faithful grouping; do not "fix" it by merging the three.
//!
//! What the pager does *not* decide is whether the conversation is over - it
//! clears the rows and returns no status. That call is caller-side
//! (`FUN_80039B7C` / the field VM), so these two names describe box teardown,
//! not a session-level end. See `docs/formats/mes.md` § Post-page dispatch.
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

/// Text rows the dialog window shows at once. Retail `_DAT_801F2740`, pinned at
/// the box-init arms of the window pager `FUN_801D84D0`.
///
/// It is the window's height, not a page length: the pager does **not** pause
/// when a fourth consecutive `0x1F` line follows a full window - the finished
/// third row scrolls up (pager state `0xC`) and the fourth types beneath it
/// with no button press (`0x801D8AAC..0x801D8B34`). A page runs until a
/// control byte (see [`pack_page`]); [`pack_box`] still groups lines in threes
/// for tools that want window-sized chunks, and marks the cut
/// [`Dispatch::ImplicitNextPage`].
pub const LINES_PER_BOX: usize = 3;

/// What the pager does after a box's rows are shown - decoded from the control
/// byte that follows the box's last line terminator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dispatch {
    /// `0x24` - advance to the next page of the same conversation (the window
    /// stays open; the next up-to-3 lines follow).
    NextPage,
    /// `0x48` - open a fresh box (pager state `9` -> `0xA`, the open animation).
    NewBox,
    /// `0x25` - box teardown (pager state `0` -> `1`, row buffer cleared).
    /// Named `End` because it ends a packed box run; whether the *conversation*
    /// ends is a caller-side decision, not something this byte carries.
    End,
    /// `0x4C 0xFF` - box teardown (pager state `6` -> `7`). Reaches the same
    /// successor handler as [`Dispatch::End`] and is indistinguishable from it
    /// in the pager; kept separate only because the on-disc encoding differs.
    Terminate,
    /// `0x27`/`0x28`/`0x29` - open a 2/3/4-option menu (the count is
    /// carried). `0x2A` is the 2-option sibling whose pager entry animates
    /// the box geometry first (states `0x11` -> `0x12`; see
    /// [`crate::picker`]).
    Picker(usize),
    /// The box filled to [`LINES_PER_BOX`] and the next byte is another `0x1F`
    /// lead with no explicit control byte between. A [`pack_box`] packing cut,
    /// not a pager pause: retail scrolls the window a row and keeps typing
    /// (see [`LINES_PER_BOX`]); [`pack_page`] never reports it.
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
            Dispatch::NextPage | Dispatch::NewBox | Dispatch::ImplicitNextPage
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
    /// `NextPage` consumes its 1-byte control; `Terminate` is 2 bytes
    /// but ends the branch; `ImplicitNextPage`/`NewBox`'s next lead is already at
    /// `dispatch_at`.
    pub fn next_box_pc(&self) -> Option<usize> {
        match self.dispatch {
            Dispatch::NextPage | Dispatch::NewBox => Some(self.dispatch_at + 1),
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
        0x25 => Dispatch::End,
        0x27 | 0x2A => Dispatch::Picker(2),
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
//
// The engine runtime types whole pages instead ([`pack_page`], in
// `engine-core::dialog` over the `dialog_window` row window). The hosts of
// this window-sized grouping are the disc-gated `field_dialog_boxpack_disc`
// oracle and the `mes boxes` subcommand, which is the view a MAN dialog
// editor needs - which lines share the window at once, and what the pager
// does when a page ends.
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

/// Pack one pager **page** starting at `pc`: every consecutive `0x1F` line up
/// to the first byte that is not another lead, and the control byte there.
///
/// This is the unit the field pager `FUN_801D84D0` types between two waits for
/// a button press. After a row finishes it tests the byte past the row
/// (`(b & 0x7F) < 0x20` at `0x801D8AB4`): another line keeps typing - into the
/// next window slot, or after a one-row scroll once the three slots are full
/// (state `0xC`) - and anything else ends the page (state `0x19`, or `0xF`
/// first). So unlike [`pack_box`] the line count is unbounded and the
/// dispatch is never [`Dispatch::ImplicitNextPage`]. Returns `None` if `pc`
/// is not a `0x1F` lead.
// REF: FUN_801D84D0
pub fn pack_page(buf: &[u8], pc: usize) -> Option<DialogBox> {
    if buf.get(pc) != Some(&0x1F) {
        return None;
    }
    let mut lines = Vec::new();
    let mut cur = pc;
    loop {
        let glyph_start = cur + 1;
        let term = line_end(buf, glyph_start);
        lines.push(glyph_start..term);
        let after = (term + 1).min(buf.len());
        if buf.get(after) == Some(&0x1F) && after > cur {
            cur = after;
            continue;
        }
        return Some(DialogBox {
            lead: pc,
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
    fn a_page_runs_past_three_lines_to_its_control_byte() {
        let mut b = Vec::new();
        for t in [b"a", b"b", b"c", b"d", b"e"] {
            b.extend(line(t));
        }
        b.push(0x24);
        let page = pack_page(&b, 0).unwrap();
        assert_eq!(page.lines.len(), 5);
        assert_eq!(page.dispatch, Dispatch::NextPage);
        assert_eq!(page.dispatch_at, b.len() - 1);
        // The window-sized packing cuts the same run after three lines.
        assert_eq!(
            pack_box(&b, 0).unwrap().dispatch,
            Dispatch::ImplicitNextPage
        );
        assert!(pack_page(&b, 1).is_none());
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
