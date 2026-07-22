//! Cross-region corpus alignment: compare a target disc (usually the retail
//! NTSC/USA build the importer patches) against a second disc (an official PAL
//! localization) to answer three questions the language-pack pipeline needs
//! before it can lift an official translation:
//!
//! 1. **Structural parity.** Do the two discs carry the same PROT entries in
//!    the same order, so entry `i` is the same logical asset on both? (Legaia's
//!    PAL discs are 1:1 with USA at the PROT-TOC level - same entry count, same
//!    scene-block boundaries - so a USA disc coordinate names the same scene on
//!    a PAL disc.)
//! 2. **Positional alignment.** Within each scene-bundle MAN (and each raw
//!    event-script carrier), do the `0x1F`-segment dialog lines appear in the
//!    same order and count on both discs, so the Nth line on the target pairs
//!    with the Nth line on the other? Segment byte *offsets* never match (a
//!    localized string has a different length, so the decompressed MAN repacks),
//!    but the line *order* is the script's, not the text's.
//! 3. **Same-size fit.** For each positionally paired line, does the official
//!    localized text fit inside the target line's byte budget (the same-size
//!    in-place constraint the importer enforces against the target disc)?
//!
//! It also censuses the high-range glyph bytes the other disc uses (`0x7F..`),
//! which on a PAL build are the accented-Latin tiles the NTSC font lacks - the
//! concrete input to a font-patch spec.
//!
//! No text is emitted: the report is counts, offsets and byte values only, so
//! it embeds no game strings.

use std::collections::BTreeMap;

use crate::disc::DiscPatcher;

use super::export::SceneManText;
use super::segments;

/// Per-domain (MAN dialog / raw carrier) alignment tally.
#[derive(Debug, Default, Clone)]
pub struct DomainStats {
    /// Entries carrying at least one qualifying segment on the target disc.
    pub entries_a: usize,
    /// ... on the other disc.
    pub entries_b: usize,
    /// Entries carrying segments on **both** discs (the alignable set).
    pub entries_both: usize,
    /// Total qualifying segments on the target / other disc.
    pub total_segs_a: usize,
    pub total_segs_b: usize,
    /// Entries where both discs carry segments and the **count matches** (so the
    /// lines pair 1:1 by order).
    pub count_matched_entries: usize,
    /// Segments living inside a count-matched entry (the confidently pairable
    /// population).
    pub paired_segments: usize,
    /// Of the paired segments, how many the other disc's line fits within the
    /// target line's byte budget (same-size in place).
    pub fit: usize,
    /// ... and how many overflow it.
    pub overflow: usize,
    /// Total and max overflow in bytes across the overflowing paired segments.
    pub overflow_bytes_total: u64,
    pub overflow_bytes_max: usize,

    // --- Order-based pairing (robust to scanner marginal disagreements) ---
    /// Lower bound on order-pairable segments: sum over both-present entries of
    /// `min(count_a, count_b)`. Pairing the first `min` lines of each entry in
    /// order is far more representative than requiring whole-entry count
    /// equality (one marginal disagreement in a 300-line scene fails the exact
    /// gate but leaves nearly every line pairable).
    pub order_pairable: usize,
    /// Sum of `|count_a - count_b|` over both-present entries - the lines that
    /// need reconciliation (scanner-marginal short runs / coincidental hits).
    pub order_delta: usize,
    /// Of the order-paired segments, how many the other line fits the target
    /// budget, and how many overflow.
    pub order_fit: usize,
    pub order_overflow: usize,
    pub order_overflow_bytes_total: u64,
    pub order_overflow_bytes_max: usize,
}

impl DomainStats {
    fn record_entry(&mut self, a: &[usize], b: &[usize]) {
        // `a` / `b` are the per-segment byte lengths on each disc, in order.
        if !a.is_empty() {
            self.entries_a += 1;
        }
        if !b.is_empty() {
            self.entries_b += 1;
        }
        self.total_segs_a += a.len();
        self.total_segs_b += b.len();
        if a.is_empty() || b.is_empty() {
            return;
        }
        self.entries_both += 1;

        // Order-based pairing: pair the first min(a,b) lines by position.
        self.order_pairable += a.len().min(b.len());
        self.order_delta += a.len().abs_diff(b.len());
        for (&la, &lb) in a.iter().zip(b) {
            self.order_fit_one(la, lb);
        }

        // Exact-count pairing (strict; a single scanner disagreement fails the
        // whole entry - kept as a conservative lower bound).
        if a.len() != b.len() {
            return;
        }
        self.count_matched_entries += 1;
        for (&la, &lb) in a.iter().zip(b) {
            self.paired_segments += 1;
            if lb <= la {
                self.fit += 1;
            } else {
                self.overflow += 1;
                let over = lb - la;
                self.overflow_bytes_total += over as u64;
                self.overflow_bytes_max = self.overflow_bytes_max.max(over);
            }
        }
    }

    fn order_fit_one(&mut self, la: usize, lb: usize) {
        if lb <= la {
            self.order_fit += 1;
        } else {
            self.order_overflow += 1;
            let over = lb - la;
            self.order_overflow_bytes_total += over as u64;
            self.order_overflow_bytes_max = self.order_overflow_bytes_max.max(over);
        }
    }

    /// Percentage of the both-present entries whose segment counts match.
    pub fn count_match_pct(&self) -> f64 {
        pct(self.count_matched_entries, self.entries_both)
    }

    /// Percentage of paired segments that fit the target budget.
    pub fn fit_pct(&self) -> f64 {
        pct(self.fit, self.paired_segments)
    }

    /// Fraction of the larger corpus that is order-pairable (the alignment
    /// confidence: `sum min / max(total_a, total_b)`).
    pub fn order_pairable_pct(&self) -> f64 {
        pct(
            self.order_pairable,
            self.total_segs_a.max(self.total_segs_b),
        )
    }

    /// Percentage of order-paired segments that fit the target budget.
    pub fn order_fit_pct(&self) -> f64 {
        pct(self.order_fit, self.order_pairable)
    }
}

/// Full cross-region report.
#[derive(Debug, Default, Clone)]
pub struct DiffReport {
    pub entries_a: usize,
    pub entries_b: usize,
    /// PROT entries whose on-disc LBA placement (relative start) matches by
    /// index - the structural-parity signal.
    pub entries_lba_aligned: usize,
    pub man: DomainStats,
    pub raw: DomainStats,
    /// High-range glyph byte (`0x7F..=0xFF`, excluding 2-byte opcode bytes) ->
    /// occurrence count across the other disc's paired dialog segments.
    pub high_byte_census: BTreeMap<u8, u64>,
}

fn pct(n: usize, d: usize) -> f64 {
    if d == 0 {
        0.0
    } else {
        100.0 * n as f64 / d as f64
    }
}

/// Segment byte-lengths (in script order) for a scene-bundle MAN, or empty if
/// the entry is not a scene bundle / has no MAN.
fn man_seg_lens(entry: &[u8]) -> Vec<usize> {
    match SceneManText::locate(entry) {
        Some(man) => segments::scan_ext(&man.decoded, true)
            .iter()
            .map(|s| s.len)
            .collect(),
        None => Vec::new(),
    }
}

/// Raw-carrier segment byte-lengths (in order), skipping any segment that falls
/// inside the entry's compressed MAN stream (those bytes are LZS, not text) and
/// gating the whole entry on the dialog-carrier check so binary asset banks are
/// never mistaken for text. Mirrors the export path.
fn raw_seg_lens(entry: &[u8]) -> Vec<usize> {
    if !segments::is_dialog_carrier(entry) {
        return Vec::new();
    }
    let compressed = SceneManText::locate(entry).map(|m| m.compressed_span());
    segments::scan_ext(entry, true)
        .iter()
        .filter(|s| !compressed.as_ref().is_some_and(|c| c.contains(&s.text_off)))
        .map(|s| s.len)
        .collect()
}

/// Census the high-range glyph bytes in a scanned buffer's qualifying segments
/// into `out` (byte -> count). 2-byte opcode argument bytes are skipped so an
/// escape argument does not read as a glyph.
fn census_high_bytes(buf: &[u8], out: &mut BTreeMap<u8, u64>) {
    for s in segments::scan_ext(buf, true) {
        let text = &buf[s.text_off..s.text_off + s.len];
        // Only census real prose lines. The high-byte census's job is to
        // enumerate the *accented-Latin* glyphs a localization adds; a
        // localization's accents live inside multi-word dialog. Restricting to
        // prose drops the shared noise floor of coincidental high-byte runs in
        // binary regions (identical on every disc, including the NTSC target),
        // so an NTSC self-census is empty and a PAL census is just its accents.
        if !segments::is_prose(text) {
            continue;
        }
        let mut i = 0;
        while i < text.len() {
            let b = text[i];
            if super::markup::is_two_byte_op(b) {
                i += 2;
                continue;
            }
            if b >= 0x7F {
                *out.entry(b).or_default() += 1;
            }
            i += 1;
        }
    }
}

/// Diff the target disc `a` against the other disc `b`.
pub fn diff_disc(a: &DiscPatcher, b: &DiscPatcher) -> DiffReport {
    let mut rep = DiffReport {
        entries_a: a.entry_count(),
        entries_b: b.entry_count(),
        ..Default::default()
    };
    let n = a.entry_count().min(b.entry_count());
    for idx in 0..n {
        // Structural: same relative disc placement at this index?
        if let (Some(la), Some(lb), Some(l0a), Some(l0b)) = (
            a.entry_disc_lba(idx),
            b.entry_disc_lba(idx),
            a.entry_disc_lba(0),
            b.entry_disc_lba(0),
        ) && la.wrapping_sub(l0a) == lb.wrapping_sub(l0b)
        {
            rep.entries_lba_aligned += 1;
        }

        let (Ok(ea), Ok(eb)) = (a.read_entry(idx), b.read_entry(idx)) else {
            continue;
        };
        rep.man.record_entry(&man_seg_lens(&ea), &man_seg_lens(&eb));
        rep.raw.record_entry(&raw_seg_lens(&ea), &raw_seg_lens(&eb));

        // High-byte census on the other disc only (the localization's glyphs).
        if let Some(man) = SceneManText::locate(&eb) {
            census_high_bytes(&man.decoded, &mut rep.high_byte_census);
        }
        if segments::is_dialog_carrier(&eb) {
            census_high_bytes(&eb, &mut rep.high_byte_census);
        }
    }
    rep
}
