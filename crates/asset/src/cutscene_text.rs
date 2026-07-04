//! Inline cutscene-narration text embedded in a field-VM cutscene-timeline
//! record.
//!
//! The opening prologue scene (`opdeene`, the in-engine 3D "Genesis" sequence)
//! carries its on-screen narration as **inline ASCII text pages**, not as a
//! `MES` text id. The pages live directly in the cutscene-timeline field-VM
//! script (the scene MAN's partition-2 record that also raises the
//! `town01` hand-off `GFLAG_SET 26`), interleaved with the camera-configure
//! (op `0x45`), effect-spawn (op `0x34`), render-config (op `0x46`) and
//! `MoveTo` (op `0x23`) instructions that stage the scene.
//!
//! ## Wire format
//!
//! A narration **block** is introduced by a field-VM op `0x4C` in its
//! outer-nibble-8 form with the cross-context extended target `0xF8`
//! (`[0xCC 0xF8 0x80 N]`, where `0xCC = 0x80 | 0x4C` is the extended-bit
//! opcode and `N` is the page count). Immediately after the op come exactly
//! `N` **pages**, each encoded as:
//!
//! ```text
//! 0x1F  <printable ASCII bytes...>  0x00
//! ```
//!
//! `0x1F` (ASCII Unit Separator) marks the start of a page; `0x00` terminates
//! it. The bytes between are plain 7-bit ASCII (the US English narration). A
//! page typically corresponds to one rendered subtitle line.
//!
//! For `opdeene` the timeline carries two blocks: a 14-page creation prologue
//! and an 8-page Seru-history block. The page count in the introducing op
//! matches the number of `0x1F`-delimited pages that follow, which both
//! validates the parse and lets a consumer pace the subtitle reveal.
//!
//! ## Provenance
//!
//! The narration display op is the field-VM `0x4C` outer-nibble-8 dispatcher
//! (see `FUN_801DE840` in the field/event VM overlay; the menu-control opcode
//! `MenuCtrl` `Nibble8` in the engine's field disassembler). The pages are
//! consumed as inline data after the op, terminating when the page count is
//! exhausted (the next byte is the following field-VM opcode, e.g. op `0x46`
//! render-config).
//!
//! This parser is clean-room: it locates the introducing op and the
//! `0x1F`/`0x00` page framing structurally and decodes the runtime disc bytes.
//! No narration text is baked into the source.

/// One inline narration page (a single subtitle line). `text` is the decoded
/// ASCII between the `0x1F` start marker and the `0x00` terminator.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NarrationPage {
    /// Byte offset of the `0x1F` start marker, relative to the record body
    /// the parse was handed.
    pub offset: usize,
    /// Decoded ASCII text of the page.
    pub text: String,
}

/// How a narration block presents on screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NarrationKind {
    /// The scrolling subtitle crawl (`op0 = 0x80`): lines enter at the bottom
    /// of a clipped window and roll upward; the parent script suspends until
    /// every page has scrolled off (retail roller `FUN_80037174`).
    Crawl,
    /// A static title card (`op0 = 0x89`): the pages show simultaneously,
    /// centered mid-screen, while the parent script continues; a later card
    /// block whose pages are blank clears it (the `map01` fly-in's
    /// "twilight of humanity" card).
    Card,
}

/// A narration block: the page count declared by the introducing op plus the
/// pages that follow it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NarrationBlock {
    /// Byte offset of the introducing `0x4C` op, relative to the record body.
    pub op_offset: usize,
    /// Page count declared in the introducing op (`N`). Card blocks carry no
    /// declared count; theirs is set to the decoded page count.
    pub declared_pages: u8,
    /// The decoded pages that follow the op.
    pub pages: Vec<NarrationPage>,
    /// Presentation form (crawl vs static card).
    pub kind: NarrationKind,
}

impl NarrationBlock {
    /// `true` when the number of decoded pages matches the count the
    /// introducing op declared - the parse-validation invariant.
    pub fn count_matches(&self) -> bool {
        self.pages.len() == usize::from(self.declared_pages)
    }

    /// Byte span `[start, end)` (relative to the parsed body) this block
    /// occupies: the introducing op through the last page's `0x00`
    /// terminator. A block whose pages did not frame cleanly spans just its
    /// introducing op (4 bytes crawl / 5 bytes card).
    ///
    /// A consumer that drives the cutscene timeline through a field-VM
    /// (rather than rendering the narration itself) uses this to skip the
    /// inline pages: the page bytes are data, not field-VM opcodes, so
    /// stepping the VM into them would mis-decode. The narration text is
    /// presented separately from the decoded [`pages`](Self::pages).
    pub fn byte_span(&self) -> (usize, usize) {
        let end = match self.pages.last() {
            // A page is `[0x1F][text bytes][0x00]`; text is 7-bit ASCII, so
            // its byte length equals its `char` length. End is one past the
            // `0x00`: offset + 1 (start marker) + text + 1 (terminator).
            Some(last) => last.offset + 2 + last.text.len(),
            None => {
                self.op_offset
                    + match self.kind {
                        NarrationKind::Crawl => 4,
                        NarrationKind::Card => 5,
                    }
            }
        };
        (self.op_offset, end)
    }
}

/// The extended-bit field-VM opcode `0x80 | 0x4C` that opens a narration op.
const OP_MENU_CTRL_EXT: u8 = 0xCC;
/// Cross-context extended target byte used by the narration op.
const EXT_TARGET_CUTSCENE: u8 = 0xF8;
/// Outer-nibble-8 selector for the narration crawl op.
const OP0_NARRATION: u8 = 0x80;
/// Outer-nibble-8 selector for the static title-card op.
const OP0_TITLE_CARD: u8 = 0x89;
/// Bytes a title-card op may interpose between itself and its first page
/// (position/params, e.g. the `map01` card's `4F`-shaped placement word).
const CARD_PAGE_SCAN: usize = 8;
/// Page start marker (ASCII Unit Separator).
const PAGE_START: u8 = 0x1F;
/// Page terminator.
const PAGE_END: u8 = 0x00;

/// Decode the `0x1F`/`0x00`-framed page that starts at `body[start]` (which
/// must be the `0x1F` marker). Returns the decoded page and the index just
/// past its `0x00` terminator, or `None` if the run is not a valid page
/// (non-ASCII byte, or no terminator before the buffer ends).
fn read_page(body: &[u8], start: usize) -> Option<(NarrationPage, usize)> {
    if body.get(start) != Some(&PAGE_START) {
        return None;
    }
    let mut j = start + 1;
    let mut text = String::new();
    while let Some(&b) = body.get(j) {
        if b == PAGE_END {
            // Reject an empty page; the narration never emits a bare 1F 00.
            if text.is_empty() {
                return None;
            }
            return Some((
                NarrationPage {
                    offset: start,
                    text,
                },
                j + 1,
            ));
        }
        // Pages are plain 7-bit printable ASCII.
        if !(0x20..0x7F).contains(&b) {
            return None;
        }
        text.push(b as char);
        j += 1;
    }
    None
}

/// Parse every inline narration block in a cutscene-timeline record `body`.
///
/// Scans for the introducing op (`[0xCC 0xF8 0x80 N]`) immediately followed
/// by a `0x1F` page marker, then reads up to `N` pages. Blocks whose op is
/// present but whose pages do not frame cleanly are still returned (with
/// whatever pages decoded), so [`NarrationBlock::count_matches`] can flag a
/// malformed block rather than silently dropping it.
pub fn parse_narration(body: &[u8]) -> Vec<NarrationBlock> {
    let mut blocks = Vec::new();
    let mut i = 0usize;
    while i + 4 < body.len() {
        if body[i] != OP_MENU_CTRL_EXT || body[i + 1] != EXT_TARGET_CUTSCENE {
            i += 1;
            continue;
        }
        // Crawl block: `[CC F8 80 N]` immediately followed by a page.
        if body[i + 2] == OP0_NARRATION && body[i + 4] == PAGE_START {
            let declared_pages = body[i + 3];
            let op_offset = i;
            let mut cursor = i + 4;
            let mut pages = Vec::new();
            while pages.len() < usize::from(declared_pages) {
                match read_page(body, cursor) {
                    Some((page, next)) => {
                        pages.push(page);
                        cursor = next;
                    }
                    None => break,
                }
            }
            // Advance past the bytes this block consumed before scanning on.
            i = cursor.max(op_offset + 4);
            blocks.push(NarrationBlock {
                op_offset,
                declared_pages,
                pages,
                kind: NarrationKind::Crawl,
            });
            continue;
        }
        // Title-card block: `[CC F8 89 b1 b2]`, pages within a short scan
        // window (the op may interpose a placement word), read greedily (no
        // declared count).
        if body[i + 2] == OP0_TITLE_CARD {
            let op_offset = i;
            let mut cursor = i + 5;
            let scan_end = (cursor + CARD_PAGE_SCAN).min(body.len());
            while cursor < scan_end && body.get(cursor) != Some(&PAGE_START) {
                cursor += 1;
            }
            let mut pages = Vec::new();
            while let Some((page, next)) = read_page(body, cursor) {
                pages.push(page);
                cursor = next;
            }
            if !pages.is_empty() {
                let declared_pages = pages.len() as u8;
                i = cursor;
                blocks.push(NarrationBlock {
                    op_offset,
                    declared_pages,
                    pages,
                    kind: NarrationKind::Card,
                });
                continue;
            }
        }
        i += 1;
    }
    blocks
}

/// Flatten every block's pages into a single ordered subtitle script. Useful
/// for a consumer that just wants the narration lines in display order.
pub fn narration_pages(body: &[u8]) -> Vec<NarrationPage> {
    parse_narration(body)
        .into_iter()
        .flat_map(|b| b.pages)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a synthetic cutscene-timeline fragment: a render-config-shaped
    /// prefix, then a 2-page narration block, then a trailing opcode byte.
    fn synthetic_block(pages: &[&str]) -> Vec<u8> {
        let mut v = vec![0x46, 0x24, 0x00, 0x00]; // some preceding op bytes
        v.push(OP_MENU_CTRL_EXT);
        v.push(EXT_TARGET_CUTSCENE);
        v.push(OP0_NARRATION);
        v.push(pages.len() as u8);
        for p in pages {
            v.push(PAGE_START);
            v.extend_from_slice(p.as_bytes());
            v.push(PAGE_END);
        }
        v.push(0x46); // following opcode (not a 1F page)
        v
    }

    #[test]
    fn parses_a_single_block_and_matches_count() {
        let body = synthetic_block(&["Hello world,", "this is a test."]);
        let blocks = parse_narration(&body);
        assert_eq!(blocks.len(), 1);
        let b = &blocks[0];
        assert_eq!(b.declared_pages, 2);
        assert!(b.count_matches());
        assert_eq!(b.pages.len(), 2);
        assert_eq!(b.pages[0].text, "Hello world,");
        assert_eq!(b.pages[1].text, "this is a test.");
        // op_offset points at the introducing 0xCC.
        assert_eq!(body[b.op_offset], OP_MENU_CTRL_EXT);
    }

    #[test]
    fn parses_two_blocks_in_order() {
        let mut body = synthetic_block(&["a one", "a two", "a three"]);
        body.extend_from_slice(&synthetic_block(&["b one", "b two"]));
        let blocks = parse_narration(&body);
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].declared_pages, 3);
        assert_eq!(blocks[1].declared_pages, 2);
        assert!(blocks.iter().all(NarrationBlock::count_matches));
        let flat = narration_pages(&body);
        assert_eq!(flat.len(), 5);
        assert_eq!(flat[0].text, "a one");
        assert_eq!(flat[4].text, "b two");
    }

    #[test]
    fn ignores_a_bare_op_with_no_pages() {
        // Introducing op declares 2 pages but no 0x1F follows.
        let body = vec![
            OP_MENU_CTRL_EXT,
            EXT_TARGET_CUTSCENE,
            OP0_NARRATION,
            0x02,
            0x46,
            0x00,
            0x00,
            0x00,
        ];
        // The `body[i+4] == 0x1F` guard means this is not recognised as a
        // narration op at all.
        assert!(parse_narration(&body).is_empty());
    }

    #[test]
    fn rejects_non_ascii_page_bytes() {
        let mut body = vec![OP_MENU_CTRL_EXT, EXT_TARGET_CUTSCENE, OP0_NARRATION, 0x01];
        body.push(PAGE_START);
        body.extend_from_slice(&[0x48, 0x80, 0x49]); // 0x80 is not printable ASCII
        body.push(PAGE_END);
        let blocks = parse_narration(&body);
        // The op is recognised (0x1F follows) but the page fails to decode.
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].pages.len(), 0);
        assert!(!blocks[0].count_matches());
    }

    #[test]
    fn empty_input_yields_no_blocks() {
        assert!(parse_narration(&[]).is_empty());
        assert!(narration_pages(&[]).is_empty());
    }

    #[test]
    fn byte_span_covers_op_through_last_terminator() {
        // 4-byte prefix, then a 2-page block, then a trailing 0x46 op.
        let body = synthetic_block(&["Hello world,", "this is a test."]);
        let blocks = parse_narration(&body);
        assert_eq!(blocks.len(), 1);
        let (start, end) = blocks[0].byte_span();
        // Span starts at the introducing 0xCC (after the 4-byte prefix).
        assert_eq!(start, 4);
        assert_eq!(body[start], OP_MENU_CTRL_EXT);
        // Span ends one past the last page's 0x00 terminator - i.e. exactly
        // at the trailing 0x46 op.
        assert_eq!(body[end], 0x46);
        assert_eq!(body[end - 1], PAGE_END);
        // Filling the span with field-VM NOPs (0x21) is offset-preserving:
        // every byte inside is replaced, the trailing op is untouched.
        let mut filled = body.clone();
        for b in &mut filled[start..end] {
            *b = 0x21;
        }
        assert!(filled[start..end].iter().all(|&b| b == 0x21));
        assert_eq!(filled[end], 0x46);
        assert_eq!(filled[..start], body[..start]);
    }

    #[test]
    fn byte_span_of_pageless_block_is_the_op_header() {
        // Op present, declares 1 page, but the page fails to decode.
        let mut body = vec![OP_MENU_CTRL_EXT, EXT_TARGET_CUTSCENE, OP0_NARRATION, 0x01];
        body.push(PAGE_START);
        body.extend_from_slice(&[0x48, 0x80, 0x49]); // 0x80 not printable
        body.push(PAGE_END);
        let blocks = parse_narration(&body);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].byte_span(), (0, 4));
    }
}
