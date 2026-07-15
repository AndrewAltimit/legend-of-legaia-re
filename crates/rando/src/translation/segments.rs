//! `0x1F`-lead dialog-segment scanner shared by translation export + import.
//!
//! Field dialogue has no dedicated container on the disc: an NPC's text is a
//! pool of `0x1F <token stream> 0x00` segments inline in its scene's
//! interaction scripts (see `docs/formats/mes.md` § multi-segment box
//! packing, `docs/formats/dialog-font.md`, and
//! `legaia_asset::cutscene_text` for the narration flavour). The same framing
//! carries picker labels, chest flavor text and cutscene narration, in two
//! byte domains:
//!
//! - **decompressed scene-bundle MANs** (the `scene_asset_table` type-3
//!   slot, LZS on disc), and
//! - **raw PROT carriers** (the v12 `.PCH` event-script prescripts and the
//!   streaming-MAN dungeon scenes), where the same segments sit uncompressed.
//!
//! The scanner walks a buffer for that framing and applies a conservative
//! text-quality gate so binary coincidences (offset tables, compressed
//! streams) don't masquerade as dialog. The gate is deliberately strict:
//! a rejected real line just doesn't get exported (it stays vanilla), while
//! an accepted false positive would hand a translator a write into data.
//! Compressed-stream hits are structurally excluded: LZS literal runs are
//! chaperoned by `0xFF` control bytes every <= 8 bytes, and any segment
//! containing `0xFF` is rejected.

use super::markup;

/// One qualifying dialog segment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Segment {
    /// Byte offset of the first **text** byte (the byte after the `0x1F`
    /// lead) within the scanned buffer.
    pub text_off: usize,
    /// Text byte length (up to, not including, the `0x00` terminator).
    pub len: usize,
}

/// 2-byte escape opcodes allowed *inside* exported dialog text: the
/// substitution set (`0xC1..=0xC5`, `0xC7`), spacing (`0xCE` + alias `0x5E`)
/// and color (`0xCF`). The wide-glyph opcodes (`0xC0`, `0xC6`,
/// `0xC8..=0xCD`) and the `0xFF` alias never occur in retail Latin dialog
/// and are treated as junk indicators.
fn allowed_escape(b: u8) -> bool {
    matches!(b, 0x5E | 0xC1..=0xC5 | 0xC7 | 0xCE | 0xCF)
}

/// Punctuation the quality gate accepts as normal prose.
fn is_prose_punct(b: u8) -> bool {
    matches!(
        b,
        b',' | b'.'
            | b'\''
            | b'!'
            | b'?'
            | b'-'
            | b':'
            | b';'
            | b'"'
            | b'('
            | b')'
            | b'&'
            | b'%'
            | b'+'
            | b'*'
            | b'['
            | b']'
            | b'|'
            | b'~'
            | b'$'
            | b'#'
            | b'@'
            | b'<'
            | b'>'
            | b'='
            | b'_'
    )
}

/// Tokenize `text` (the bytes between `0x1F` and the terminator) and judge
/// whether it reads as retail dialog. See module docs for the rationale.
pub fn qualifies(text: &[u8]) -> bool {
    qualifies_ext(text, false)
}

/// [`qualifies`] with an `allow_high` flag. When `true`, single-byte glyphs in
/// the high range `0x7F..=0xFF` (that are not 2-byte opcodes) are accepted as
/// printable and counted as word-like - the shape of the PAL localizations,
/// whose accented Latin glyphs (CP437-style: `é`=`0x82`, `ü`=`0x81`, `ê`=`0x88`,
/// ...) live above `0x7E`. The retail NTSC (USA) build never uses high glyph
/// tiles, so scanning it with `allow_high` on is a no-op; scanning a PAL build
/// with it off drops every accented line. Cross-region alignment must use the
/// same flag on both discs (see [`super::diff`]).
pub fn qualifies_ext(text: &[u8], allow_high: bool) -> bool {
    if text.is_empty() {
        return false;
    }
    let mut glyphs: Vec<u8> = Vec::with_capacity(text.len());
    let mut i = 0;
    while i < text.len() {
        let b = text[i];
        if markup::is_two_byte_op(b) {
            if !allowed_escape(b) || i + 1 >= text.len() {
                return false;
            }
            i += 2;
            continue;
        }
        glyphs.push(b);
        i += 1;
    }
    // A glyph is legal if it is printable ASCII, or (when `allow_high`) a
    // high-range glyph tile. Retail Latin dialog never uses high tiles, and
    // their presence marks binary data - except in a PAL build, where they are
    // the accented letters.
    let is_glyph = |g: u8| (0x20..=0x7E).contains(&g) || (allow_high && g >= 0x7F);
    if glyphs.iter().any(|&g| !is_glyph(g)) {
        return false;
    }
    let n = glyphs.len();
    let is_letter = |g: u8| g.is_ascii_alphabetic() || (allow_high && g >= 0x7F);
    let letters = glyphs.iter().filter(|&&g| is_letter(g)).count();
    if letters == 0 {
        return false;
    }
    let good = glyphs
        .iter()
        .filter(|&&g| g == b' ' || g.is_ascii_alphanumeric() || is_prose_punct(g) || is_letter(g))
        .count();
    if glyphs.contains(&b' ') {
        // Prose: at least three letters and >= 90% word-like glyphs.
        letters >= 3 && (good * 10) >= n * 9
    } else {
        // Space-less runs are accepted only as clean single words ("Yes",
        // "No", "Cancel"). Mixed letter/digit runs without spaces are the
        // signature of offset tables that happen to land in glyph range.
        letters == n && (2..=16).contains(&n)
    }
}

/// Minimum number of **prose** segments (see [`is_prose`]) a PROT entry must
/// carry before its raw `0x1F`-segment hits are trusted as real dialog.
///
/// The `0x1F <printable> <printable> 0x00` framing is short enough to occur by
/// coincidence throughout binary asset banks (sequenced music, VAB sample
/// banks, the battle-character mesh/animation packs, monster archives), so a
/// bare `qualifies` hit is not proof the bytes are text - writing a
/// "translation" over such a hit corrupts the binary asset and freezes the
/// game (a garbled SEQ hangs the sound driver on New Game; a garbled PROT-1204
/// battle-form pack freezes the in-battle menu). Real dialog carriers - the
/// v12 event-script prescripts and streaming-MAN dungeon scenes - are instead
/// prose-dense: they carry dozens to thousands of multi-word English lines.
///
/// Across the retail disc the two populations separate with a wide margin: the
/// binary banks top out at **2** coincidental prose hits per entry, while the
/// smallest genuine dialog carrier has **8**. Requiring at least this many
/// prose segments keeps every real carrier and rejects every binary bank.
pub const MIN_CARRIER_PROSE: usize = 3;

/// Minimum ASCII-letter count for a segment to read as prose.
const MIN_PROSE_LETTERS: usize = 6;

/// `true` when `text` (the bytes between `0x1F` and the terminator) reads as a
/// multi-word English line: it contains an interior space (two word-like runs)
/// and at least [`MIN_PROSE_LETTERS`] ASCII letters. Used only to *corroborate*
/// that a PROT entry is a real text carrier (see [`is_dialog_carrier`]); the
/// per-segment [`qualifies`] gate still governs which segments are exported.
pub fn is_prose(text: &[u8]) -> bool {
    // Walk glyphs, skipping 2-byte escape tokens.
    let mut letters = 0usize;
    let mut has_interior_space = false;
    let mut seen_letter_before_space = false;
    let mut i = 0;
    while i < text.len() {
        let b = text[i];
        if markup::is_two_byte_op(b) {
            i += 2;
            continue;
        }
        if b.is_ascii_alphabetic() {
            letters += 1;
            seen_letter_before_space = true;
        } else if b == b' ' && seen_letter_before_space {
            // A space that follows at least one letter and precedes more text
            // marks a word boundary inside the line.
            if text[i + 1..]
                .iter()
                .any(|&c| c.is_ascii_alphabetic() && !markup::is_two_byte_op(c))
            {
                has_interior_space = true;
            }
        }
        i += 1;
    }
    has_interior_space && letters >= MIN_PROSE_LETTERS
}

/// `true` when `buf` (a whole PROT entry) carries at least
/// [`MIN_CARRIER_PROSE`] prose segments - i.e. it is a genuine dialog carrier
/// rather than a binary asset bank in which the `0x1F <text> 0x00` framing
/// occurs by coincidence. Both export and import gate raw-segment writes on
/// this so a pack can never overwrite binary asset data.
pub fn is_dialog_carrier(buf: &[u8]) -> bool {
    scan(buf)
        .iter()
        .filter(|s| is_prose(&buf[s.text_off..s.text_off + s.len]))
        .count()
        >= MIN_CARRIER_PROSE
}

/// Scan a buffer for qualifying `0x1F <text> 0x00` segments. On a qualifying
/// hit the cursor resumes past the terminator; otherwise it advances one
/// byte, so overlapping candidates are still found.
pub fn scan(buf: &[u8]) -> Vec<Segment> {
    scan_ext(buf, false)
}

/// [`scan`] with the [`qualifies_ext`] `allow_high` flag, so a PAL build's
/// accented dialog qualifies. See [`super::diff`].
pub fn scan_ext(buf: &[u8], allow_high: bool) -> Vec<Segment> {
    let mut segs = Vec::new();
    let mut i = 0;
    while i < buf.len() {
        if buf[i] != 0x1F {
            i += 1;
            continue;
        }
        match walk_to_terminator(buf, i + 1) {
            Some(term) if buf[term] == 0x00 && qualifies_ext(&buf[i + 1..term], allow_high) => {
                segs.push(Segment {
                    text_off: i + 1,
                    len: term - (i + 1),
                });
                i = term + 1;
            }
            _ => i += 1,
        }
    }
    segs
}

/// Token-walk from `start` to the first terminator byte (`<= 0x1E`),
/// honouring 2-byte token strides. `None` when the buffer ends first.
pub fn walk_to_terminator(buf: &[u8], start: usize) -> Option<usize> {
    let mut j = start;
    while j < buf.len() {
        let c = buf[j];
        if c <= 0x1E {
            return Some(j);
        }
        j += if markup::is_two_byte_op(c) { 2 } else { 1 };
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seg(text: &[u8]) -> Vec<u8> {
        let mut v = vec![0x1F];
        v.extend_from_slice(text);
        v.push(0x00);
        v
    }

    #[test]
    fn finds_prose_segments() {
        let mut buf = vec![0xE3, 0x12, 0x00];
        buf.extend(seg(b"Clean water flows from"));
        buf.extend([0x41, 0x99]);
        buf.extend(seg(b"Do you wish to drink this water?"));
        let s = scan(&buf);
        assert_eq!(s.len(), 2);
        assert_eq!(
            &buf[s[0].text_off..s[0].text_off + s[0].len],
            b"Clean water flows from"
        );
        assert_eq!(s[1].len, 32);
    }

    #[test]
    fn accepts_short_labels_and_escapes() {
        let mut buf = seg(b"Yes");
        buf.extend(seg(b"No"));
        buf.extend(seg(&[
            b'H', b'i', b' ', 0xC1, 0x00, b'!', b' ', b'h', b'o', b'w', b'?',
        ]));
        assert_eq!(scan(&buf).len(), 3);
    }

    #[test]
    fn rejects_binary_noise() {
        // High glyphs, wide-glyph escapes, 0xFF chaperones, letter-poor runs.
        for junk in [
            &[0x5D, 0xE2, 0x71, 0x50, 0x6D][..],             // ']{e2}qPm'
            &[b'S', 0xFF, 0x6D, b'a', b'l', b'l'][..],       // compressed-stream hit
            &[b'p', 0xCB, 0xD3, b'a'][..],                   // wide-glyph escape
            &[b'2', b'/', b'D', b'/', b'V', b'/', b'h'][..], // offset-table pattern
            &[b'w'][..],                                     // single letter
        ] {
            let buf = seg(junk);
            assert_eq!(scan(&buf), Vec::new(), "junk {junk:02x?} must not qualify");
        }
    }

    #[test]
    fn requires_nul_terminator() {
        // Terminated by 0x05 instead of 0x00 -> not a dialog segment.
        let buf = [0x1F, b'H', b'e', b'l', b'l', b'o', 0x05];
        assert!(scan(&buf).is_empty());
    }

    #[test]
    fn walk_honours_two_byte_strides() {
        // The escape argument 0x00 must not read as a terminator.
        let buf = [0x1F, 0xC1, 0x00, b'H', b'i', 0x00];
        let s = scan(&buf);
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].len, 4);
    }

    #[test]
    fn prose_needs_an_interior_space_and_letters() {
        assert!(is_prose(b"Do you wish to drink this?"));
        assert!(is_prose(b"Clean water flows"));
        // Single word, no interior space -> not prose.
        assert!(!is_prose(b"Cancel"));
        // Two-letter false-positive run -> not prose.
        assert!(!is_prose(b"Wx"));
        // A trailing space after one word (no following word) is not interior.
        assert!(!is_prose(b"Yes "));
        // Too few letters even with a space.
        assert!(!is_prose(b"a b c"));
    }

    #[test]
    fn dialog_carrier_needs_several_prose_lines() {
        // A binary bank: a couple of coincidental short 0x1F segments, no prose.
        let mut bank = Vec::new();
        bank.extend(seg(b"Wx"));
        bank.extend([0x03, 0x11, 0x9A, 0x40]);
        bank.extend(seg(b"aQ"));
        assert!(!is_dialog_carrier(&bank));

        // A real carrier: several multi-word English lines.
        let mut scene = Vec::new();
        scene.extend(seg(b"Do you wish to read it?"));
        scene.extend(seg(b"A slightly soiled diary."));
        scene.extend(seg(b"Yes")); // short label rides along, now trusted
        scene.extend(seg(b"There is a chest here."));
        assert!(is_dialog_carrier(&scene));
    }
}
