//! "Scene event-scripts only" detector - sister of
//! [`crate::scene_scripted_asset_table`] for the case where the prescript
//! exists but no canonical 7-asset table follows at the next sector boundary.
//!
//! ### Layout
//!
//! ```text
//! +0x00              u16  count             ; 3..=4096
//! +0x02              u16  offsets[count]    ; offsets[0] = 2 + count*2,
//!                                          ; monotonically non-decreasing,
//!                                          ; all <= file size
//! +offsets[i]        stager record         ; [i16 model_sel][u16 flags][move-VM
//!                                          ; bytecode]; the bulk lead with
//!                                          ; `model_sel = -1` (= the old
//!                                          ; "0xFFFF 0x0000" lead, a transform
//!                                          ; node). See "Format meaning" below.
//! ...                                       ; bulk asset payload after the
//!                                          ; prescript (format unidentified;
//!                                          ; varies per scene)
//! ```
//!
//! ### Distinguishing from sister detectors
//!
//! * [`crate::scene_scripted_asset_table`] runs first; it claims the cases
//!   where a canonical `[u32 count=7][7 × descriptor]` asset table sits at
//!   the next 0x800-aligned offset past the prescript.
//! * This detector catches the rest - the prescript is present, but the
//!   payload after it isn't a canonical asset table. The post-prescript
//!   bulk is some other asset bundle layout (per-scene secondary header,
//!   format not yet reversed).
//!
//! The frame-opener rate is what makes this detector zero-false-positive
//! on its own. Files matching the prescript shape by coincidence (random
//! `[u16 count][u16 offsets]`-shaped data) carry no `0xFFFF 0x0000` opener
//! at the record positions; real scene-event-script bundles carry it on
//! the majority of records (50–92 %).
//!
//! ### Format meaning - a per-scene MOVE-VM stager table (summon-stager format)
//!
//! The prescript records were long assumed to be field-VM (`FUN_801DE840`)
//! event scripts. They are not (field-VM disassembly yields a 65–88 % error
//! rate). They are **move-VM (`FUN_80023070`) records in the summon-stager
//! format** - byte-identical in shape to the per-summon stager parts
//! ([`crate::summon_overlay`]):
//!
//! ```text
//! +0x00   i16  model_sel   ; -1 = transform/pivot node (the dominant kind; this
//!                          ;   is the "0xFFFF" of the old "0xFFFF 0x0000" lead),
//!                          ;   0x4000/0x4001 = render-mode node, >=0 = library mesh
//! +0x02   u16  flags       ; (the "0x0000" of the old lead)
//! +0x04   ..   move-VM bytecode   ; ticked by FUN_80023070; op 0x08 = Halt (the
//!                          ;   "0x0008 terminator"), all opcodes in 0x00..=0x46
//! ```
//!
//! **The full runtime chain (pinned from disc + resident-overworld RAM):**
//! 1. The scripted bundle is loaded into `_DAT_8007b8d0` (the field scratch slot
//!    `_DAT_1f8003ec + 0x12800`).
//! 2. The per-scene **field VM `FUN_801DE840`** (the scene MAN script) calls the
//!    **installer `FUN_800252EC(id)`**, which resolves
//!    `record = _DAT_8007b8d0 + (offsets[id] & ~1)` from the `[count][offsets]`
//!    table and calls the part-stager `FUN_80021B04`.
//! 3. `FUN_80021B04` seats an actor (`actor[+0x48] = record`, `actor[+0x70] = 2`
//!    move-VM PC, tick fn `FUN_80021DF4`) and kicks the move VM.
//! 4. `FUN_80021DF4` (per frame) → `FUN_80023070` runs the record's move-VM
//!    bytecode (from `record+4`) until the Halt op (`0x08`).
//!
//! So the "consuming command VM" is the **move VM** (already decoded: 71 ops
//! `0x00..=0x46`, operand widths in `legaia_engine_vm::move_vm`), and the
//! records parse with the same [`crate::summon_overlay::SummonPart`] shape -
//! see [`move_stager_records`]. This is the *per-scene* sibling of the summon
//! stagers (same record format, same `FUN_80021B04` + move-VM consumer). It is
//! NOT field-VM, NOT a bespoke command VM, and NOT vestigial: live
//! kingdom-overworld RAM shows these records resident at `_DAT_8007b8d0` with
//! actors (`actor[+0x48]`) executing them through the move VM.
//!
//! The earlier "operand widths aren't a fixed per-opcode table / ~56 % cap"
//! finding is explained: decoding was attempted (a) with the *wrong VM* and
//! (b) from offset 0 (the `[model_sel][flags]` header), not from `record+4`;
//! the move VM's widths *are* fixed per opcode, but a *linear* disasm still
//! desyncs at the move VM's jump ops (`0x18/0x19/0x1A/0x1B`) - decode with the
//! move VM (which follows jumps), not a linear pass.
//!
//! Record `0`'s lead is a different shape (a small dispatch/default table, not a
//! stager record). The genuine per-scene field-VM (`FUN_801DE840`) *scripts*
//! live in the scene's MAN sub-asset (`FUN_8003A1E4`; see [`crate::man_section`]
//! and `docs/subsystems/script-vm.md`); this prescript is the move-VM *stager*
//! table the MAN scripts spawn from. [`record_words`] still surfaces the raw
//! 16-bit word stream; [`move_stager_records`] surfaces the structured records.

use crate::summon_overlay::SummonPart;
use serde::Serialize;

/// Maximum sane prescript record count. Real hits sit in `8..=71`; 4096
/// gives generous headroom while still rejecting random bytes whose
/// `u16[0]` happens to fall in range.
const MAX_PRESCRIPT_COUNT: u16 = 4096;

/// Minimum record count. Below this the prescript shape can match
/// arbitrary `[count][offsets]`-style data by chance.
const MIN_PRESCRIPT_COUNT: u16 = 3;

/// Lead of a transform-node stager record - `0xFFFF 0x0000` little-endian, i.e.
/// `model_sel = -1` (transform/pivot node) + `flags = 0`. (Historically
/// mislabelled "the field VM's frame divider opcode"; it is the summon-stager
/// record lead - see the module-level "Format meaning" note.)
const FRAME_OPENER: u32 = 0x0000_FFFF;

/// The word-aligned record terminator (`0x0008`), present on 99.9 % of framed
/// records across the retail corpus.
pub const RECORD_TERMINATOR: u16 = 0x0008;

/// Required minimum fraction of records that open with [`FRAME_OPENER`].
/// Real scene-event-script bundles hit 45–92 %; this threshold separates
/// them cleanly from the few coincidence-prescript files (`< 14 %`).
const FRAME_OPENER_RATE_MIN: f32 = 0.45;

/// Detection result.
#[derive(Debug, Clone, Serialize)]
pub struct SceneEventScripts {
    /// Number of records in the prescript offset table.
    pub records: u16,
    /// Byte offset of the last record (start of the last script).
    pub last_record_offset: u16,
    /// How many records open with the `0xFFFF 0x0000` record header sentinel.
    pub frame_opener_count: u16,
    /// Fraction of records opening with the header sentinel.
    pub frame_opener_rate: f32,
}

/// Try to detect a scene-event-scripts prescript. Returns `None` if the
/// prescript shape is wrong or the frame-opener rate is too low.
pub fn detect(buf: &[u8]) -> Option<SceneEventScripts> {
    let offsets = detect_prescript(buf)?;
    let count = offsets.len() as u16;

    // Count how many records open with the field-VM frame sentinel.
    let mut openers: u16 = 0;
    for (i, &off) in offsets.iter().enumerate() {
        // Guard: each record needs at least 4 bytes to test for the magic.
        let off = off as usize;
        if off + 4 > buf.len() {
            continue;
        }
        // Don't read past the next record's start (or buffer end).
        let next_end = if i + 1 < offsets.len() {
            offsets[i + 1] as usize
        } else {
            buf.len()
        };
        if off + 4 > next_end {
            continue;
        }
        let lead = u32::from_le_bytes(buf[off..off + 4].try_into().ok()?);
        if lead == FRAME_OPENER {
            openers += 1;
        }
    }

    let rate = openers as f32 / count as f32;
    if rate < FRAME_OPENER_RATE_MIN {
        return None;
    }

    Some(SceneEventScripts {
        records: count,
        last_record_offset: *offsets.last()?,
        frame_opener_count: openers,
        frame_opener_rate: rate,
    })
}

/// Walk the offset table and produce per-record `(start, end)` byte ranges
/// in the buffer. Returns `None` when the prescript itself is malformed (same
/// gate as [`detect`] minus the frame-opener-rate check, since callers may
/// want ranges even on borderline-rate files).
///
/// `end` is the next record's `start`, or `buf.len()` for the final record.
/// Use the returned slice with `&buf[start..end]` to extract one script.
pub fn record_ranges(buf: &[u8]) -> Option<Vec<(usize, usize)>> {
    let offsets = detect_prescript(buf)?;
    let mut out = Vec::with_capacity(offsets.len());
    for (i, &off) in offsets.iter().enumerate() {
        let start = off as usize;
        let end = if i + 1 < offsets.len() {
            offsets[i + 1] as usize
        } else {
            buf.len()
        };
        if start > end || end > buf.len() {
            return None;
        }
        out.push((start, end));
    }
    Some(out)
}

/// True when `record` opens with the `0xFFFF 0x0000` header sentinel.
pub fn record_is_framed(record: &[u8]) -> bool {
    record.len() >= 4 && u32::from_le_bytes(record[0..4].try_into().unwrap()) == FRAME_OPENER
}

/// Decode the raw 16-bit word stream of one framed record: the little-endian
/// `u16`s after the `0xFFFF 0x0000` header, up to and **excluding** the
/// [`RECORD_TERMINATOR`] (`0x0008`). Returns `None` if the record does not
/// open with the header sentinel.
///
/// This is deliberately a *structural* view: it does not split the stream into
/// `(opcode, operands)` because the per-opcode operand widths - and the
/// consuming command VM - are not yet identified (see the module note). The
/// low byte of each word is the opcode; the high byte is `0` for the bulk of
/// words and non-zero only on packed coordinate operands.
pub fn record_words(record: &[u8]) -> Option<Vec<u16>> {
    if !record_is_framed(record) {
        return None;
    }
    let mut out = Vec::new();
    let mut i = 4;
    while i + 2 <= record.len() {
        let w = u16::from_le_bytes(record[i..i + 2].try_into().unwrap());
        if w == RECORD_TERMINATOR {
            break;
        }
        out.push(w);
        i += 2;
    }
    Some(out)
}

/// Parse the prescript records as **summon-stager-format move-VM records**.
///
/// Each `[u16 count][u16 offsets]`-indexed record is
/// `[i16 model_sel][u16 flags][move-VM bytecode]` - the same shape the per-summon
/// stagers use ([`crate::summon_overlay::SummonPart`]). At runtime the field VM
/// (`FUN_801DE840`) installs a record by id via `FUN_800252EC`
/// (`record = bundle_base + offsets[id]`) → the part-stager `FUN_80021B04`
/// (`actor[+0x48] = record`) → the move VM `FUN_80023070` runs `record+4`.
///
/// Returns one [`SummonPart`] per prescript record (`bytecode` = `record+4`
/// up to the next record's start). Returns `None` if the prescript shape is
/// invalid. Record `0` is included as-is (its lead is a small dispatch/default
/// table rather than a stager record - classify via [`SummonPart::kind`]).
pub fn move_stager_records(buf: &[u8]) -> Option<Vec<SummonPart>> {
    let ranges = record_ranges(buf)?;
    let mut out = Vec::with_capacity(ranges.len());
    for (start, end) in ranges {
        // Need the 4-byte [model_sel][flags] header to classify the record.
        if start + 4 > end {
            continue;
        }
        let model_sel = i16::from_le_bytes(buf[start..start + 2].try_into().ok()?);
        let flags = u16::from_le_bytes(buf[start + 2..start + 4].try_into().ok()?);
        out.push(SummonPart {
            record_off: start,
            model_sel,
            flags,
            bytecode: (start + 4)..end,
        });
    }
    Some(out)
}

/// Validate the leading `[u16 count][u16 offsets[count]]` prescript shape.
/// Returns the offsets vector on success.
fn detect_prescript(buf: &[u8]) -> Option<Vec<u16>> {
    if buf.len() < 4 {
        return None;
    }
    let count = u16::from_le_bytes(buf[0..2].try_into().ok()?);
    if !(MIN_PRESCRIPT_COUNT..=MAX_PRESCRIPT_COUNT).contains(&count) {
        return None;
    }
    let table_end = 2usize.checked_add((count as usize) * 2)?;
    if table_end > buf.len() {
        return None;
    }
    let first = u16::from_le_bytes(buf[2..4].try_into().ok()?);
    if first as usize != table_end {
        return None;
    }

    let mut offsets = Vec::with_capacity(count as usize);
    let mut prev: u16 = 0;
    for i in 0..(count as usize) {
        let p = 2 + i * 2;
        let o = u16::from_le_bytes(buf[p..p + 2].try_into().ok()?);
        if (o as usize) > buf.len() || o < prev {
            return None;
        }
        offsets.push(o);
        prev = o;
    }
    Some(offsets)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a buffer with `count` records, `n_openers` of which begin with
    /// the frame sentinel. All records are 8 bytes long.
    fn synth(count: u16, n_openers: u16) -> Vec<u8> {
        assert!(n_openers <= count);
        let table_end = 2 + (count as usize) * 2;
        let record_size = 8usize;
        let mut buf = Vec::new();
        // Header.
        buf.extend_from_slice(&count.to_le_bytes());
        for i in 0..(count as usize) {
            let off = (table_end + i * record_size) as u16;
            buf.extend_from_slice(&off.to_le_bytes());
        }
        // Records: first `n_openers` start with FFFF 0000; rest with 0xAA filler.
        for i in 0..(count as u32) {
            if (i as u16) < n_openers {
                buf.extend_from_slice(&FRAME_OPENER.to_le_bytes());
                buf.extend_from_slice(&[0xAA, 0xBB, 0xCC, 0xDD]);
            } else {
                buf.extend_from_slice(&[0xAA; 8]);
            }
        }
        buf
    }

    #[test]
    fn detects_all_records_open_with_sentinel() {
        let buf = synth(10, 10);
        let r = detect(&buf).expect("should detect");
        assert_eq!(r.records, 10);
        assert_eq!(r.frame_opener_count, 10);
        assert!((r.frame_opener_rate - 1.0).abs() < 1e-6);
    }

    #[test]
    fn detects_at_50_pct_threshold() {
        let buf = synth(10, 5);
        let r = detect(&buf).expect("50% should still detect (above 45% threshold)");
        assert_eq!(r.frame_opener_count, 5);
        assert!((r.frame_opener_rate - 0.5).abs() < 1e-6);
    }

    #[test]
    fn rejects_below_45_pct() {
        // 4/10 = 40% is below the 45% threshold
        let buf = synth(10, 4);
        assert!(detect(&buf).is_none());
    }

    #[test]
    fn rejects_zero_openers() {
        let buf = synth(10, 0);
        assert!(detect(&buf).is_none());
    }

    #[test]
    fn record_words_reads_to_terminator() {
        // Mirrors the real record `FF FF 00 00 25 00 29 00 25 00 2A 00 08 00`.
        let record = [
            0xFF, 0xFF, 0x00, 0x00, // header sentinel
            0x25, 0x00, 0x29, 0x00, // cmd(0x25, 0x29)
            0x25, 0x00, 0x2A, 0x00, // cmd(0x25, 0x2A)
            0x08, 0x00, // terminator (excluded)
            0xDE, 0xAD, // trailing bytes past the terminator (ignored)
        ];
        assert!(record_is_framed(&record));
        let words = record_words(&record).expect("framed");
        assert_eq!(words, vec![0x0025, 0x0029, 0x0025, 0x002A]);
    }

    #[test]
    fn record_words_rejects_unframed_record() {
        // Record 0 (the fixed 96x8 table) carries no header sentinel.
        let record = [0x00, 0x02, 0x3E, 0x03, 0x03, 0x00, 0x00, 0x00];
        assert!(!record_is_framed(&record));
        assert!(record_words(&record).is_none());
    }

    #[test]
    fn rejects_when_first_offset_doesnt_match_table_end() {
        let mut buf = synth(10, 10);
        // Stomp first offset.
        buf[2] = 0xFF;
        buf[3] = 0;
        assert!(detect(&buf).is_none());
    }

    #[test]
    fn rejects_count_below_minimum() {
        // count = 2 is below MIN_PRESCRIPT_COUNT.
        let buf = synth(2, 2);
        assert!(detect(&buf).is_none());
    }

    #[test]
    fn rejects_offsets_out_of_bounds() {
        let mut buf = synth(10, 10);
        // Make last offset way past buffer end.
        let last_idx = 2 + 9 * 2;
        buf[last_idx] = 0xFF;
        buf[last_idx + 1] = 0xFF;
        assert!(detect(&buf).is_none());
    }

    #[test]
    fn rejects_random_bytes() {
        let buf: Vec<u8> = (0..0x1000u32).map(|i| (i & 0xFF) as u8).collect();
        assert!(detect(&buf).is_none());
    }

    #[test]
    fn record_ranges_walks_every_record() {
        let buf = synth(4, 4);
        let ranges = record_ranges(&buf).expect("prescript valid");
        assert_eq!(ranges.len(), 4);
        // Records are 8 bytes each; first starts at table_end = 2 + 4*2 = 10.
        assert_eq!(ranges[0], (10, 18));
        assert_eq!(ranges[1], (18, 26));
        assert_eq!(ranges[2], (26, 34));
        // Last record extends to buffer end.
        assert_eq!(ranges[3].0, 34);
        assert_eq!(ranges[3].1, buf.len());
    }

    #[test]
    fn record_ranges_returns_none_on_malformed_prescript() {
        let buf: Vec<u8> = (0..0x1000u32).map(|i| (i & 0xFF) as u8).collect();
        assert!(record_ranges(&buf).is_none());
    }
}
