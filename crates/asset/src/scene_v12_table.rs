//! Scene "v12" header + event-script bundle.
//!
//! ### What it is
//!
//! 97 PROT entries on the disc - **one per game scene** - share a strict 16-byte
//! header at offset 0 plus a [scene event-scripts] prescript at the next
//! 0x800-aligned offset. The on-disc header carries:
//!
//! * three constant magic words at fixed offsets (`0x0012` and `0x0014` twice);
//! * three runtime-fixup slots that are zero on disc and filled by the loader;
//! * a per-scene `param` field that counts a small inline records table at `+0x14`;
//! * a per-scene `N` field that is algebraically related to the records-table
//!   size (`N = 4*param + 22`).
//!
//! The dense 30 KB - 387 KB payload past the header is **not opaque** - it's a
//! standard [`scene_event_scripts`](crate::scene_event_scripts) prescript at
//! file offset 0x800, exactly like the 100 sister entries that carry the same
//! prescript at offset 0. Each scene has both: the offset-0 form (often called
//! the "script-only" form) and the offset-0x800 form (this format, with the
//! v12 header prefix).
//!
//! ### On-disc layout
//!
//! ```text
//! +0x000   u16  end_records + 6  ; ` = N + 4`, header field
//! +0x002   u16  0x0012           ; constant magic
//! +0x004   u16  0x0000           ; constant
//! +0x006   u16  0x0014           ; constant magic (= byte offset of records)
//! +0x008   u16  param            ; record-table entry count (1..=192 in retail)
//! +0x00A   u16  end_records + 2  ; ` = N`, header field; runtime fixup slot
//! +0x00C   u16  0x0000           ; constant
//! +0x00E   u16  end_records + 4  ; ` = N + 2`, header field; runtime fixup slot
//! +0x010   u32  0                ; padding
//! +0x014   param × 4 bytes       ; inline record table
//! +end_records (= 0x14 + 4*param)
//!         (zero pad to 0x800)
//! +0x800   u16  script_count     ; scene event-scripts prescript
//! +0x802   script_count × u16    ;   offset table (relative to +0x800)
//! +0x800 + offsets[i]            ; per-record word-aligned command bytes
//! ```
//!
//! Note: the prescript records are the **word-aligned** per-scene actor/event
//! structure described in [`crate::scene_event_scripts`], **not** field-VM
//! (`FUN_801DE840`) bytecode — it disassembles as field-VM with a 65–88 %
//! error rate. The real per-scene field-VM scripts live in the MAN sub-asset
//! (see [`crate::man_section`]).
//!
//! ### Why the algebraic ties matter
//!
//! `u16[0] = N+4`, `u16[5] = N`, `u16[7] = N+2` are not random fields: they're
//! **runtime pointer slots** sitting immediately past the end of the inline
//! records (`end_records = 0x14 + 4 * param`). On disc those bytes are zero;
//! at scene load the runtime writes a computed pointer into each slot. The
//! on-disc bytes `[N+4, ?, ?, ?, ?, N, ?, N+2]` are the slot **offsets** that
//! tell the loader where to write, not the pointer values.
//!
//! The strict header check is therefore: three constant magic words, plus
//! `u16[0] = u16[5] + 4` and `u16[7] = u16[5] + 2`. Across the entire 1234
//! entry corpus this matches 97 entries with zero false positives.
//!
//! ### Inline records at `+0x14`
//!
//! The `param` records each pack `[u8 b0][u8 b1][u8 b2][u8 = 0x01]`. Semantics
//! are scene-specific and only partially understood:
//! * `b3 = 0x01` on every record across all 97 entries (probably "live" flag).
//! * `b2` groups records into 1..N categories. Drake (`map01`) has 8 distinct
//!   `b2` values across 12 records; Karisto (`map03`) groups 12 of its 23
//!   records under a single `(b1=0x2F, b2=0x05)` triple - a strong "scene
//!   region" / "transition table" smell.
//! * `b0`, `b1` carry scene-local identifiers (sub-index, region-id, or
//!   target-script index) - the exact mapping is consumer-dependent.
//!
//! The parser surfaces the bytes raw; downstream code can interpret them as it
//! pins down each consumer.
//!
//! ### Event-script prescript at `+0x800`
//!
//! Identical shape to [`scene_event_scripts`]: `[u16 count][u16 offsets[count]]`
//! followed by per-record field-VM bytecode. The field VM is `FUN_801DE840`
//! (see `docs/subsystems/script-vm.md`); the per-record `0xFFFF 0x0000`
//! sentinel is the frame-divider opcode.
//!
//! Across the 97 v12 entries, 75 hit a frame-opener rate >= 50 %. The 22
//! lower-rate entries (e.g. `0029_town0c.BIN`, `0037_izumi.BIN`) carry event
//! scripts that open with a different opcode for the first record - the rest
//! still open with the sentinel, but the average dips below 50 %.
//!
//! ### Where the docs live
//!
//! Format-level reference: [`docs/formats/scene-v12-table.md`](../../../docs/formats/scene-v12-table.md).
//! Cross-format context: [`docs/formats/scene-bundles.md`](../../../docs/formats/scene-bundles.md).

use serde::Serialize;

/// Magic word at `u16[1]`.
const W1_MAGIC: u16 = 0x0012;
/// Magic word at `u16[3]`. Equal to the byte offset of the inline records.
const W3_MAGIC: u16 = 0x0014;

/// Byte offset where the inline records table begins.
pub const RECORDS_OFFSET: usize = 0x14;

/// Byte offset where the event-script prescript begins.
pub const PRESCRIPT_OFFSET: usize = 0x800;

/// Minimum sane `param` (inline record count). `0724_noaru.BIN` is the
/// retail-corpus minimum at `param = 0` (empty inline-records table, full
/// event-script payload at `+0x800`). Below 0 is impossible.
const MIN_PARAM: u16 = 0;

/// Maximum sane `param`. Real entries top out at 192 (`0084_suimon.BIN`); a
/// generous cap leaves room for unseen variants while rejecting random
/// buffers.
const MAX_PARAM: u16 = 1024;

/// The fixed algebraic relationship between `N` and `param`:
/// `N = 4 * param + GAP`. Verified across all 97 corpus entries (Drake
/// `N=70, param=12`, Sebucus `N=46, param=6`, Karisto `N=114, param=23`,
/// down to `0127_rikuroa2.BIN` `N=26, param=1`).
///
/// `N` is the byte distance from the start of the file to the first runtime
/// fixup slot (just past the inline records).
const N_GAP: u16 = 22;

/// One 4-byte inline record from `+0x14`. Bytes are surfaced raw - downstream
/// consumers know the per-scene semantics. `flag` is `0x01` across the entire
/// corpus; it's surfaced explicitly so anomalies stand out.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
pub struct ParamRecord {
    /// Byte at offset `+0`. Scene-local identifier (sub-index / region-id).
    pub b0: u8,
    /// Byte at offset `+1`. Scene-local identifier (region-id / target-id).
    pub b1: u8,
    /// Byte at offset `+2`. Categorises records into 1..N groups within the
    /// scene; semantically resembles a "scene kind" or "region-class" enum.
    pub b2: u8,
    /// Byte at offset `+3`. Always `0x01` in the retail corpus - probably a
    /// "record is live" flag.
    pub flag: u8,
}

/// One record range inside the event-script prescript at `+0x800`. Offsets
/// are absolute byte offsets within the v12 file. Use
/// [`SceneV12Table::script_payload`] to obtain the bytecode slice.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
pub struct ScriptRecord {
    /// Absolute byte offset within the file (start of the record bytecode).
    pub start: usize,
    /// Absolute byte offset of the **end** of the record (exclusive). For the
    /// last record this is the file end (or buffer end).
    pub end: usize,
    /// True when the record opens with the field-VM frame-divider sentinel
    /// `0xFFFF 0x0000`.
    pub frame_opener: bool,
}

impl ScriptRecord {
    /// Length of the record's bytecode in bytes.
    pub fn len(&self) -> usize {
        self.end - self.start
    }

    /// Returns `true` when the bytecode is empty.
    pub fn is_empty(&self) -> bool {
        self.start == self.end
    }
}

/// Parsed scene v12 table.
#[derive(Debug, Clone, Serialize)]
pub struct SceneV12Table {
    /// `N` from the header (`u16[5]`). Equal to `4 * param + 22` for every
    /// retail entry - surfaced for direct inspection.
    pub n: u16,
    /// Per-scene parameter (= record count of the inline table at `+0x14`).
    pub param: u16,
    /// Inline records at `+0x14`. Length equals [`Self::param`].
    pub records: Vec<ParamRecord>,
    /// Event-script records parsed from the prescript at [`PRESCRIPT_OFFSET`].
    /// Empty if the prescript is malformed (treated as a non-fatal warning -
    /// the v12 header itself still validated).
    pub scripts: Vec<ScriptRecord>,
    /// How many script records open with the field-VM frame sentinel
    /// (`0xFFFF 0x0000`). Surfaced for downstream "is this still a sane
    /// field-VM script bundle?" checks.
    pub frame_opener_count: u16,
}

impl SceneV12Table {
    /// First runtime fixup slot offset (`u16[0]` = `N + 4`).
    pub fn table_a_base(&self) -> u16 {
        self.n + 4
    }

    /// Second runtime fixup slot offset (`u16[7]` = `N + 2`).
    pub fn table_b_base(&self) -> u16 {
        self.n + 2
    }

    /// Byte offset just past the inline records (= `N - 2`).
    pub fn end_records(&self) -> usize {
        RECORDS_OFFSET + 4 * self.param as usize
    }

    /// Frame-opener rate across the event scripts. `0.0` if there are no
    /// scripts (malformed prescript).
    pub fn frame_opener_rate(&self) -> f32 {
        if self.scripts.is_empty() {
            0.0
        } else {
            self.frame_opener_count as f32 / self.scripts.len() as f32
        }
    }

    /// Slice the `i`-th script record's bytecode out of the original buffer.
    pub fn script_payload<'a>(&self, buf: &'a [u8], i: usize) -> Option<&'a [u8]> {
        let r = self.scripts.get(i)?;
        buf.get(r.start..r.end)
    }
}

/// Try to detect a v12 scene table at the buffer head. Validates header
/// algebra strictly; the prescript at `+0x800` is parsed best-effort and
/// surfaced through [`SceneV12Table::scripts`] (an empty `scripts` vec means
/// the prescript was malformed but the v12 header itself was valid).
pub fn detect(buf: &[u8]) -> Option<SceneV12Table> {
    if buf.len() < 16 {
        return None;
    }
    let n_plus_4 = read_u16_le(buf, 0)?;
    let w1 = read_u16_le(buf, 2)?;
    let w2 = read_u16_le(buf, 4)?;
    let w3 = read_u16_le(buf, 6)?;
    let param = read_u16_le(buf, 8)?;
    let n = read_u16_le(buf, 10)?;
    let w6 = read_u16_le(buf, 12)?;
    let n_plus_2 = read_u16_le(buf, 14)?;

    if w1 != W1_MAGIC || w2 != 0 || w3 != W3_MAGIC || w6 != 0 {
        return None;
    }
    if !(MIN_PARAM..=MAX_PARAM).contains(&param) {
        return None;
    }
    if n_plus_4 != n.checked_add(4)? || n_plus_2 != n.checked_add(2)? {
        return None;
    }
    // Algebraic tie between N and param (N = 4 * param + 22). This is the
    // cleanest no-overlap signature: every retail v12 entry satisfies it.
    if n != param.checked_mul(4)?.checked_add(N_GAP)? {
        return None;
    }

    // Bounds-check the inline records.
    let end_records = RECORDS_OFFSET + 4 * param as usize;
    if end_records > buf.len() {
        return None;
    }
    let mut records = Vec::with_capacity(param as usize);
    for i in 0..(param as usize) {
        let p = RECORDS_OFFSET + i * 4;
        records.push(ParamRecord {
            b0: buf[p],
            b1: buf[p + 1],
            b2: buf[p + 2],
            flag: buf[p + 3],
        });
    }

    // Parse the prescript at +0x800. Best-effort: if it's malformed (or the
    // file is too short to even reach the prescript), we still return the
    // v12 header parse with an empty `scripts` vec.
    let (scripts, frame_opener_count) = parse_prescript(buf);

    Some(SceneV12Table {
        n,
        param,
        records,
        scripts,
        frame_opener_count,
    })
}

/// Walk the prescript at `+0x800`. Best-effort: returns an empty vec if the
/// prescript is malformed. The walker tolerates `count` from 1 upwards, since
/// a few retail v12 entries (e.g. `0779_koin1b.BIN`) have only 2 records -
/// below the `scene_event_scripts` standalone-detector threshold of 3.
fn parse_prescript(buf: &[u8]) -> (Vec<ScriptRecord>, u16) {
    if buf.len() < PRESCRIPT_OFFSET + 4 {
        return (Vec::new(), 0);
    }
    let pre = &buf[PRESCRIPT_OFFSET..];
    let count = match pre.get(0..2) {
        Some(b) => u16::from_le_bytes(b.try_into().unwrap()),
        None => return (Vec::new(), 0),
    };
    // Tolerate small counts; reject obviously corrupt huge ones.
    if !(1..=4096).contains(&count) {
        return (Vec::new(), 0);
    }
    let table_end = 2usize + 2 * count as usize;
    if pre.len() < table_end {
        return (Vec::new(), 0);
    }
    let mut offsets = Vec::with_capacity(count as usize);
    let mut prev: u16 = 0;
    for i in 0..(count as usize) {
        let p = 2 + i * 2;
        let o = u16::from_le_bytes(pre[p..p + 2].try_into().unwrap());
        if (o as usize) > pre.len() || o < prev {
            return (Vec::new(), 0);
        }
        offsets.push(o);
        prev = o;
    }
    if (offsets[0] as usize) != table_end {
        return (Vec::new(), 0);
    }

    let mut out = Vec::with_capacity(count as usize);
    let mut openers: u16 = 0;
    for i in 0..(count as usize) {
        let start_rel = offsets[i] as usize;
        let end_rel = if i + 1 < (count as usize) {
            offsets[i + 1] as usize
        } else {
            pre.len()
        };
        let start = PRESCRIPT_OFFSET + start_rel;
        let end = PRESCRIPT_OFFSET + end_rel;
        let frame_opener = start + 4 <= end
            && start + 4 <= buf.len()
            && buf[start..start + 4] == [0xFF, 0xFF, 0x00, 0x00];
        if frame_opener {
            openers += 1;
        }
        out.push(ScriptRecord {
            start,
            end,
            frame_opener,
        });
    }
    (out, openers)
}

fn read_u16_le(buf: &[u8], at: usize) -> Option<u16> {
    let bytes = buf.get(at..at + 2)?;
    Some(u16::from_le_bytes(bytes.try_into().unwrap()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a minimal valid v12 header + records + 0x800 prescript shell.
    fn synth(param: u16, records: &[(u8, u8, u8)], script_count: u16) -> Vec<u8> {
        let n = 4 * param + N_GAP;
        let mut buf = Vec::with_capacity(PRESCRIPT_OFFSET + 4 + 2 * script_count as usize);
        // Header
        buf.extend_from_slice(&(n + 4).to_le_bytes());
        buf.extend_from_slice(&W1_MAGIC.to_le_bytes());
        buf.extend_from_slice(&0u16.to_le_bytes());
        buf.extend_from_slice(&W3_MAGIC.to_le_bytes());
        buf.extend_from_slice(&param.to_le_bytes());
        buf.extend_from_slice(&n.to_le_bytes());
        buf.extend_from_slice(&0u16.to_le_bytes());
        buf.extend_from_slice(&(n + 2).to_le_bytes());
        // Padding 0x10..0x14
        buf.extend_from_slice(&0u32.to_le_bytes());
        // Records at 0x14
        for (b0, b1, b2) in records {
            buf.push(*b0);
            buf.push(*b1);
            buf.push(*b2);
            buf.push(0x01);
        }
        // Zero pad to 0x800
        buf.resize(PRESCRIPT_OFFSET, 0);
        // Prescript count
        buf.extend_from_slice(&script_count.to_le_bytes());
        // Offsets: each record 8 bytes, sentinel + filler
        let table_end = 2 + 2 * script_count as usize;
        for i in 0..script_count {
            let off = (table_end + 8 * i as usize) as u16;
            buf.extend_from_slice(&off.to_le_bytes());
        }
        // Records (all open with 0xFFFF 0x0000 sentinel)
        for _ in 0..script_count {
            buf.extend_from_slice(&[0xFF, 0xFF, 0x00, 0x00]);
            buf.extend_from_slice(&[0xAA, 0xBB, 0xCC, 0xDD]);
        }
        buf
    }

    #[test]
    fn detects_synthetic_with_full_chain() {
        let recs = [(0x15, 0x08, 0x02), (0x14, 0x08, 0x02), (0x13, 0x08, 0x02)];
        let buf = synth(3, &recs, 5);
        let t = detect(&buf).expect("should detect");
        assert_eq!(t.param, 3);
        assert_eq!(t.n, 4 * 3 + N_GAP);
        assert_eq!(t.records.len(), 3);
        assert_eq!(t.records[0].b0, 0x15);
        assert_eq!(t.records[0].b1, 0x08);
        assert_eq!(t.records[0].b2, 0x02);
        assert_eq!(t.records[0].flag, 0x01);
        assert_eq!(t.scripts.len(), 5);
        assert_eq!(t.frame_opener_count, 5);
        assert!((t.frame_opener_rate() - 1.0).abs() < 1e-6);
    }

    #[test]
    fn end_records_matches_n_minus_2() {
        let buf = synth(12, &[(0, 0, 0); 12], 0);
        let t = detect(&buf).expect("should detect");
        assert_eq!(t.end_records(), (t.n as usize) - 2);
        assert_eq!(t.end_records(), 0x14 + 4 * 12);
    }

    #[test]
    fn rejects_buffer_smaller_than_header() {
        assert!(detect(&[0u8; 8]).is_none());
        assert!(detect(&[0u8; 15]).is_none());
    }

    #[test]
    fn rejects_wrong_constant_at_w1() {
        let mut buf = synth(12, &[(0, 0, 0); 12], 0);
        buf[2] = 0x13;
        assert!(detect(&buf).is_none());
    }

    #[test]
    fn rejects_wrong_constant_at_w3() {
        let mut buf = synth(12, &[(0, 0, 0); 12], 0);
        buf[6] = 0x15;
        assert!(detect(&buf).is_none());
    }

    #[test]
    fn rejects_n_param_algebraic_mismatch() {
        // Build a buffer where N != 4*param + 22.
        let mut buf = synth(12, &[(0, 0, 0); 12], 0);
        // Corrupt N (offset 10) to a value that breaks N = 4*12 + 22 = 70.
        buf[10] = 0xA0;
        buf[11] = 0x00;
        // Also need to keep N+2 and N+4 consistent or it'd fail earlier.
        buf[0] = 0xA4;
        buf[1] = 0x00;
        buf[14] = 0xA2;
        buf[15] = 0x00;
        assert!(detect(&buf).is_none());
    }

    #[test]
    fn rejects_n_plus_4_mismatch() {
        let mut buf = synth(12, &[(0, 0, 0); 12], 0);
        buf[0] = 0x05;
        buf[1] = 0x00;
        assert!(detect(&buf).is_none());
    }

    #[test]
    fn rejects_n_plus_2_mismatch() {
        let mut buf = synth(12, &[(0, 0, 0); 12], 0);
        buf[14] = 0xFF;
        buf[15] = 0xFF;
        assert!(detect(&buf).is_none());
    }

    #[test]
    fn accepts_with_malformed_prescript() {
        // Header + records valid, but prescript region is garbage. Detector
        // still returns the header parse; `scripts` is empty.
        let mut buf = synth(3, &[(0x15, 0x08, 0x02); 3], 0);
        // Stomp the entire prescript region with 0xff.
        for b in &mut buf[PRESCRIPT_OFFSET..] {
            *b = 0xFF;
        }
        let t = detect(&buf).expect("v12 header itself is still valid");
        assert_eq!(t.scripts.len(), 0);
        assert_eq!(t.frame_opener_count, 0);
    }

    #[test]
    fn accepts_real_world_drake_head_pattern() {
        // 0093_map01.BIN head bytes (param=12, N=70).
        let mut buf = vec![
            0x4A, 0x00, // u16[0] = N+4 = 0x4a
            0x12, 0x00, // u16[1] = 0x0012
            0x00, 0x00, // u16[2] = 0
            0x14, 0x00, // u16[3] = 0x0014
            0x0C, 0x00, // u16[4] = param = 12
            0x46, 0x00, // u16[5] = N = 0x46 (70)
            0x00, 0x00, // u16[6] = 0
            0x48, 0x00, // u16[7] = N+2 = 0x48
            0x00, 0x00, 0x00, 0x00, // padding 0x10..0x14
        ];
        // 12 records
        let recs: [[u8; 4]; 12] = [
            [0x15, 0x08, 0x02, 0x01],
            [0x14, 0x08, 0x02, 0x01],
            [0x13, 0x08, 0x02, 0x01],
            [0x17, 0x2A, 0x0C, 0x01],
            [0x17, 0x68, 0x0B, 0x01],
            [0x17, 0x69, 0x0B, 0x01],
            [0x17, 0x6A, 0x0B, 0x01],
            [0x14, 0x09, 0x0A, 0x01],
            [0x06, 0x5F, 0x09, 0x01],
            [0x14, 0x5E, 0x08, 0x01],
            [0x77, 0x12, 0x01, 0x01],
            [0x72, 0x3E, 0x00, 0x01],
        ];
        for r in &recs {
            buf.extend_from_slice(r);
        }
        // Pad to 0x800 and add a small valid prescript.
        buf.resize(PRESCRIPT_OFFSET, 0);
        // 3 records, all open with frame sentinel.
        buf.extend_from_slice(&3u16.to_le_bytes());
        for i in 0..3 {
            let off = (2 + 3 * 2 + i * 8) as u16;
            buf.extend_from_slice(&off.to_le_bytes());
        }
        for _ in 0..3 {
            buf.extend_from_slice(&[0xFF, 0xFF, 0x00, 0x00, 0xAA, 0xBB, 0xCC, 0xDD]);
        }
        let t = detect(&buf).expect("drake head should detect");
        assert_eq!(t.param, 12);
        assert_eq!(t.n, 0x46);
        assert_eq!(t.records.len(), 12);
        assert_eq!(t.records[3].b2, 0x0C);
        assert_eq!(t.records[3].flag, 0x01);
        assert_eq!(t.scripts.len(), 3);
        assert_eq!(t.frame_opener_count, 3);
        assert_eq!(t.table_a_base(), 0x4A);
        assert_eq!(t.table_b_base(), 0x48);
    }
}
