//! Dialog **option-picker** decoding for field-VM inline interaction scripts.
//!
//! A field NPC's interaction text is a run of `0x1F`-lead glyph segments
//! (the message lines) interleaved with control bytes that the per-actor
//! dialog window pager (`FUN_801D84D0`) and the inline-script control handler
//! (`FUN_80038050`) consume. Three of those control bytes open a **multiple-
//! choice menu**:
//!
//! | open byte (`& 0x7F`) | options |
//! |---|---|
//! | `0x27` | 2 (`Yes`/`No`-style) |
//! | `0x28` | 3 |
//! | `0x29` | 4 |
//!
//! ## Control-region layout (relative to the open byte at index `O`)
//!
//! ```text
//! [ .. 0x1F prompt segment .. 0x00 ]   <- the box text shown above the menu
//! O                                     <- open byte (0x27 / 0x28 / 0x29)
//! O+1 .. O+N*2                          <- N option entries, 2 bytes each (i16 LE)
//! O+N*2+1                               <- continuation byte
//! [ optional 0x4C 0xFF terminate ]
//! N * [ 0x1F label segment 0x00 ]       <- the on-screen option labels
//! ```
//!
//! The pager render loop (`overlay_dialog FUN_801D84D0`, lines ~2166-2185)
//! draws each label as a standalone `0x1F`-lead glyph segment located **after**
//! the continuation byte (measured by `FUN_8003CA38`, drawn by `FUN_80036888`).
//! The N×2-byte region between the open byte and the continuation is **not** the
//! labels - it is the per-option **jump table**.
//!
//! ## Option entries are signed relative jumps
//!
//! Each 2-byte entry is a **signed 16-bit little-endian relative displacement**.
//! When the player confirms a choice, the inline-script control handler
//! `FUN_80038050` reads the chosen index from the cursor at `DAT_801C6EA4+0xC`
//! and sets the actor's script PC (`actor[+0x9E]`) to:
//!
//! ```text
//! new_pc = (O + 1 + index*2) + i16_LE(entry[index])
//! ```
//!
//! i.e. the displacement is relative to the **start of that option's own
//! 2-byte entry**. This was pinned empirically: across the four story-branch
//! re-emissions of the `izumi` book menu, all four option entries shift by an
//! identical per-emission delta (-518, -564, -549), the signature of relative
//! addressing to a moving site; and `FUN_80038050` confirms the arithmetic.
//!
//! The continuation byte (`O+N*2+1`) follows the same post-page dispatch table
//! as a normal full box (`0x24` continue / `0x25` end / `0x48` new box /
//! `0x4C 0xFF` terminate); it selects what happens after the menu closes,
//! independent of which option was chosen (that branch is the relative jump).

use crate::interp::{Interpreter, MesEvent};

/// One menu option: its on-screen label glyph bytes plus the signed relative
/// jump the inline-script control handler applies when this option is chosen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PickerOption {
    /// Signed little-endian relative jump displacement from the start of this
    /// option's own 2-byte entry (see [`Picker::jump_target`]).
    pub rel_jump: i16,
    /// Decoded glyph bytes of the on-screen label (no `0x1F` lead, no `0x00`
    /// terminator). Render through [`legaia_font`](../legaia_font/index.html).
    pub label: Vec<u8>,
}

/// A decoded option-picker control region.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Picker {
    /// Index of the open byte (`0x27`/`0x28`/`0x29`, possibly `| 0x80`) in the
    /// source inline buffer.
    pub open: usize,
    /// Raw open byte as stored.
    pub open_byte: u8,
    /// Number of options (`2`, `3`, or `4`).
    pub n: usize,
    /// The continuation byte at `open + n*2 + 1` (raw, before `& 0x7F`).
    pub continuation: u8,
    /// The N options, in on-screen order.
    pub options: Vec<PickerOption>,
    /// One-past-the-end index of the last label segment in the source buffer
    /// (where decoding the picker region finished).
    pub end: usize,
}

impl Picker {
    /// Absolute PC the inline-script control handler (`FUN_80038050`) jumps to
    /// when option `index` is confirmed: `(open + 1 + index*2) + rel_jump`.
    ///
    /// Returns `None` if the result would be negative (a backward jump past the
    /// buffer start, which never occurs for a valid script).
    pub fn jump_target(&self, index: usize) -> Option<usize> {
        let opt = self.options.get(index)?;
        let base = self.open as i64 + 1 + (index as i64) * 2;
        let t = base + opt.rel_jump as i64;
        (t >= 0).then_some(t as usize)
    }
}

/// Number of options implied by an open byte, or `None` if it isn't one.
fn option_count(open_byte: u8) -> Option<usize> {
    match open_byte & 0x7f {
        0x27 => Some(2),
        0x28 => Some(3),
        0x29 => Some(4),
        _ => None,
    }
}

/// Decode one `0x1F`-lead glyph segment starting at `seg[0] == 0x1F`. Returns
/// `(end_index_after_terminator, glyph_bytes)`, or `None` if `seg` doesn't
/// begin with a lead byte or never terminates.
fn decode_label(buf: &[u8], lead: usize) -> Option<(usize, Vec<u8>)> {
    if buf.get(lead) != Some(&0x1F) {
        return None;
    }
    let mut interp = Interpreter::new_at(buf, lead + 1);
    let mut glyphs = Vec::new();
    loop {
        match interp.next_event() {
            Some(MesEvent::Glyph(g)) | Some(MesEvent::SkipTwo(g)) => glyphs.push(g),
            Some(MesEvent::WideGlyph(_, arg)) => glyphs.push(arg),
            Some(MesEvent::EndOfMessage(_)) => return Some((interp.pc(), glyphs)),
            // Spacing / Substitute / Control stay inside the label.
            Some(_) => {}
            None => return None,
        }
    }
}

/// Attempt to decode a picker whose open byte sits at `open` in `buf`. Returns
/// `None` if the bytes there don't form a structurally-valid picker (wrong open
/// byte, missing continuation, or fewer than N decodable label segments) - this
/// is the genuineness filter that rejects coincidental `0x27`/`0x28`/`0x29`
/// glyph/data bytes.
pub fn parse_picker_at(buf: &[u8], open: usize) -> Option<Picker> {
    let open_byte = *buf.get(open)?;
    let n = option_count(open_byte)?;

    // Just past the N*2-byte jump table sits EITHER a post-page continuation
    // byte (`0x24`/`0x25`/`0x48`/`0x4c`) before the labels, OR - when the menu
    // has no post-page dispatch - the first label's `0x1F` lead directly (the
    // labels start immediately). The izumi book menu uses the continuation-byte
    // form; the Rim Elm spar (`town01`) uses the immediate-labels form (pinned
    // live: open `0x29` at the actor's dialogue buffer, 4 jump entries, then the
    // option labels straight after - see `autorun_tetsu_picker_data.lua`).
    let cont_idx = open + n * 2 + 1;
    let continuation = *buf.get(cont_idx)?;
    let cm = continuation & 0x7f;
    let labels_immediate = cm == 0x1f;
    if !labels_immediate && !matches!(cm, 0x24 | 0x25 | 0x48 | 0x4c) {
        return None;
    }

    // Label region begins past the continuation. A 1-byte continuation
    // (`0x24` continue / `0x25` end / `0x48` new box) is consumed inline; a
    // leading `0x4C 0xFF` terminate marker is also skipped (per FUN_801D84D0);
    // the immediate-labels form starts the labels at `cont_idx` itself.
    let mut ls = cont_idx;
    if matches!(cm, 0x24 | 0x25 | 0x48) {
        ls += 1;
    }
    if buf.get(ls) == Some(&0x4c) && buf.get(ls + 1) == Some(&0xff) {
        ls += 2;
    }

    let mut options = Vec::with_capacity(n);
    let mut cur = ls;
    for i in 0..n {
        let entry = open + 1 + i * 2;
        let lo = *buf.get(entry)?;
        let hi = *buf.get(entry + 1)?;
        let rel_jump = i16::from_le_bytes([lo, hi]);
        let (end, label) = decode_label(buf, cur)?;
        options.push(PickerOption { rel_jump, label });
        cur = end;
    }

    Some(Picker {
        open,
        open_byte,
        n,
        continuation,
        options,
        end: cur,
    })
}

/// Scan `buf` for every structurally-valid picker. A candidate open byte must
/// be preceded by a `0x00` (the prompt segment's terminator) and pass
/// [`parse_picker_at`]; this rejects the many coincidental `0x27`/`0x28`/`0x29`
/// bytes that occur inside glyph runs and packed numeric data.
pub fn scan_pickers(buf: &[u8]) -> Vec<Picker> {
    let mut out = Vec::new();
    for i in 1..buf.len() {
        if option_count(buf[i]).is_none() {
            continue;
        }
        if buf[i - 1] != 0x00 {
            continue;
        }
        if let Some(p) = parse_picker_at(buf, i) {
            out.push(p);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a 2-option (`0x27`) picker: prompt "Hi", options "Yes"/"No" with
    /// relative jumps +0x10 / +0x20, continuation `0x24`.
    fn yes_no() -> Vec<u8> {
        let mut b = vec![0x1F, b'H', b'i', 0x00]; // prompt segment
        let open = b.len(); // == 4
        b.push(0x27); // open, N=2
        b.extend_from_slice(&0x10i16.to_le_bytes()); // option 0 jump
        b.extend_from_slice(&0x20i16.to_le_bytes()); // option 1 jump
        b.push(0x24); // continuation (continue)
        b.extend_from_slice(&[0x1F, b'Y', b'e', b's', 0x00]); // label 0
        b.extend_from_slice(&[0x1F, b'N', b'o', 0x00]); // label 1
        assert_eq!(open, 4);
        b
    }

    #[test]
    fn parses_two_option_picker() {
        let b = yes_no();
        let p = parse_picker_at(&b, 4).expect("genuine picker at open=4");
        assert_eq!(p.n, 2);
        assert_eq!(p.continuation, 0x24);
        assert_eq!(p.options[0].label, b"Yes");
        assert_eq!(p.options[1].label, b"No");
        assert_eq!(p.options[0].rel_jump, 0x10);
        assert_eq!(p.options[1].rel_jump, 0x20);
    }

    #[test]
    fn jump_target_is_relative_to_each_entry() {
        let b = yes_no();
        let p = parse_picker_at(&b, 4).unwrap();
        // option 0 entry at open+1 = 5; target = (4 + 1 + 0) + 0x10 = 5 + 16 = 21
        assert_eq!(p.jump_target(0), Some(5 + 0x10));
        // option 1 entry at open+3 = 7; target = (4 + 1 + 2) + 0x20 = 7 + 32 = 39
        assert_eq!(p.jump_target(1), Some(7 + 0x20));
        assert_eq!(p.jump_target(2), None);
    }

    #[test]
    fn scan_finds_the_picker_and_rejects_coincidences() {
        let mut b = yes_no();
        // Append a stray 0x29 inside a glyph run (not preceded by 0x00, no
        // valid continuation) - must not be picked up.
        b.extend_from_slice(&[b'a', 0x29, b'b', b'c']);
        let found = scan_pickers(&b);
        assert_eq!(found.len(), 1, "exactly the one genuine picker");
        assert_eq!(found[0].open, 4);
    }

    #[test]
    fn high_bit_open_byte_is_accepted() {
        let mut b = yes_no();
        b[4] = 0x27 | 0x80; // 0xA7 - masked form the pager also accepts
        let p = parse_picker_at(&b, 4).expect("0xA7 is a 2-option open byte");
        assert_eq!(p.n, 2);
    }

    #[test]
    fn rejects_missing_continuation() {
        // Open byte with no valid continuation byte after the jump table.
        let b = vec![0x00, 0x27, 0x01, 0x00, 0x02, 0x00, 0x99];
        assert!(parse_picker_at(&b, 1).is_none());
    }

    #[test]
    fn parses_immediate_labels_picker() {
        // The Rim Elm Tetsu spar form: open `0x29` + 4 jump entries + the option
        // labels straight after, with NO post-page continuation byte (the cont
        // position is the first label's `0x1F` lead). Pinned live from the spar
        // menu's dialogue buffer (`autorun_tetsu_picker_data.lua`).
        let mut b = vec![0x1F, b'Q', 0x00]; // prompt segment
        let open = b.len(); // == 3
        b.push(0x29); // open, N=4
        for j in [0x10i16, 0x20, 0x30, 0x40] {
            b.extend_from_slice(&j.to_le_bytes());
        }
        // labels immediately - no continuation byte
        for lbl in [&b"A"[..], &b"BB"[..], &b"CCC"[..], &b"D"[..]] {
            b.push(0x1F);
            b.extend_from_slice(lbl);
            b.push(0x00);
        }
        let p = parse_picker_at(&b, open).expect("immediate-labels 4-option picker");
        assert_eq!(p.n, 4);
        assert_eq!(p.continuation, 0x1F);
        assert_eq!(p.options[0].label, b"A");
        assert_eq!(p.options[2].label, b"CCC");
        assert_eq!(p.options[0].rel_jump, 0x10);
        assert_eq!(p.options[3].rel_jump, 0x40);
        // jump_target(0) = (open+1+0) + 0x10
        assert_eq!(p.jump_target(0), Some(open + 1 + 0x10));
        // scan_pickers finds it (the open byte is preceded by the prompt 0x00)
        assert_eq!(scan_pickers(&b).len(), 1);
    }
}
