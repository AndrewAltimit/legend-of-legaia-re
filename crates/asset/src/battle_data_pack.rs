//! TMD-slot walker for the per-character player battle files
//! `data\battle\PLAYER1..4` (extraction PROT entries 0863..0866 = the retail
//! `battle_data` CDNAME block; Vahn / Noa / Gala / Terra. The extraction
//! filename labels `0863/0864_edstati3` are the +2 label shift - see
//! `docs/formats/cdname.md`).
//!
//! ### Layout (see `docs/formats/battle-data-pack.md` for the full format)
//!
//! The file head is `[u32 desc_off][u32 clut_a_off][u32 clut_b_off]
//! [u32 budget]` + the `record[0]` LZS stream (the battle-palette chain,
//! parsed by [`crate::battle_char_palette`]). At `desc_off` sits a chained
//! 12-byte descriptor table `[u32 id][u32 offset][u32 size]`
//! (`offset[i+1] == offset[i] + size[i]`; `id = 0` marks section
//! boundaries / default-variant slots; an all-zero entry terminates), and
//! the slot region at `data_base` (0x8000 in all four retail files) holds
//! per-slot `[u32 dec_size][LZS]` streams decoding to:
//!
//! ```text
//!   +0x00  u32 magic_or_count  ; 0x14 (=20) in every observed slot
//!   +0x04  u32 sub_obj0_end    ; nested-section end offset (often 0)
//!   +0x08  u32 sub_obj1_end    ; nested-section end offset (often 0)
//!   +0x0C  u32 tmd_body_end    ; offset where the Legaia TMD body ends
//!   +0x10..0x20                ; per-texture info (layout TBD)
//!   +0x20  Legaia TMD          ; magic 0x80000002
//!   +tmd_body_end              ; texture/CLUT pool (layout partially TBD)
//! ```
//!
//! ### Framing note (pending realignment)
//!
//! This walker predates the runtime pin of the descriptor table and reads it
//! through a 4-byte-shifted frame: entry 0's `id` as a "record count"
//! (`record_count`), entry 0's `offset` (always 0) as a "reserved" word, and
//! each `(id, data_offset)` pair with the *previous* entry's `size` as
//! `on_disc_size`. The `(id, data_offset)` pairs come out correct, so every
//! slot decodes with the right id attached - except entry 0 itself, which
//! surfaces as the `offset = 0` "filler" record with id 0 (its real id is
//! the value reported as `record_count`). The off-by-one `on_disc_size` is
//! harmless because the LZS decode is output-bounded.
//!
//! ### What this is NOT
//!
//! Not the monster stat archive (extraction 0867, fixed `0x14000`-stride
//! slots - [`crate::monster_archive`]); the historical "16 MB battle_data
//! container at 0865" reading analyzed extraction 0865's *extended* TOC
//! window, which over-reads across 0866 into the archive. It also does NOT
//! carry the row-479 town NPC palettes (byte-match corpus negative; those
//! are scene-pack TIMs - see `docs/formats/npc-palette.md`).

use anyhow::{Result, bail};
use serde::Serialize;

/// Maximum plausible record count we'll accept from the header.
const MAX_RECORDS: u32 = 256;

/// Alignment for the LZS-data section base (sector boundary).
const DATA_BASE_ALIGN: usize = 0x800;

/// One record in the trailer table.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct Record {
    /// Index in the record table (0..count).
    pub index: usize,
    /// Compressed size on disc (bytes), including the u32 dec_size prefix.
    pub on_disc_size: u32,
    /// Slot id (0..0x7F observed). `0` marks an empty/filler slot.
    pub id: u32,
    /// Byte offset from `data_base` where the compressed entry lives.
    pub data_offset: u32,
}

impl Record {
    /// Absolute file offset of the compressed entry.
    pub fn file_offset(&self, data_base: usize) -> usize {
        data_base + self.data_offset as usize
    }
}

/// Parsed pack header (record table + data base).
#[derive(Debug, Clone, Serialize)]
pub struct BattleDataPack {
    /// Byte offset where the trailer record table starts.
    pub table_offset: usize,
    /// Total record count declared in the header.
    pub record_count: u32,
    /// Records that have non-zero size. Records past the first zero-size
    /// entry are treated as table padding and excluded.
    pub records: Vec<Record>,
    /// Byte offset where the compressed-data section begins.
    pub data_base: usize,
}

/// One decompressed entry.
#[derive(Debug, Clone)]
pub struct DecodedEntry {
    /// Record this entry was decoded from.
    pub record: Record,
    /// Decompressed bytes.
    pub bytes: Vec<u8>,
    /// Byte range of the embedded Legaia TMD (if found).
    pub tmd_range: Option<std::ops::Range<usize>>,
}

/// Cheap presence check: does `buf` look like a battle_data pack?
pub fn is_battle_data_pack(buf: &[u8]) -> bool {
    detect_header(buf).is_some()
}

/// Locate the trailer record table without parsing every record. The
/// streaming preamble runs from offset 0 to `chunk0_size + 4`, terminator
/// included; the count u32 lives right after the terminator, at
/// `chunk0_size + 4` (the terminator itself sits at `chunk0_size + 0`).
fn detect_header(buf: &[u8]) -> Option<(usize, u32)> {
    if buf.len() < 32 {
        return None;
    }
    let chunk0_header = read_u32_le(buf, 0)?;
    let type_byte = (chunk0_header >> 24) & 0xFF;
    let chunk0_size = (chunk0_header & 0x00FF_FFFF) as usize;
    // First chunk must be a TIM-typed dispatcher chunk by the streaming
    // convention; battle_data's streaming preamble uses type=0.
    if type_byte != 0 {
        return None;
    }
    // The record table sits right after the chunk payload + terminator.
    // chunk_payload spans [4, 4 + chunk0_size); terminator at +chunk0_size,
    // and the count u32 starts at +chunk0_size + 4. (For 0865 this is
    // exactly 0x6C68 + 4 = 0x6C6C ... wait, the count is at 0x6C68 in 0865
    // because the streaming "size = chunk0_size = 0x6C68" includes the
    // chunk's own header byte. Empirically the count lives at chunk0_size
    // exactly, not chunk0_size + 4.)
    //
    // Read both candidate positions and prefer the one with a sane count.
    for cand in [chunk0_size, chunk0_size + 4] {
        if cand + 8 > buf.len() {
            continue;
        }
        let count = read_u32_le(buf, cand)?;
        let reserved = read_u32_le(buf, cand + 4)?;
        if reserved != 0 {
            continue;
        }
        if count == 0 || count > MAX_RECORDS {
            continue;
        }
        // Sanity check: first record's offset should be small and size plausible.
        let table = cand + 8;
        if table + 12 > buf.len() {
            continue;
        }
        let sz0 = read_u32_le(buf, table)?;
        let id0 = read_u32_le(buf, table + 4)?;
        let off0 = read_u32_le(buf, table + 8)?;
        if sz0 == 0 || sz0 > 0x40_0000 {
            continue;
        }
        if id0 > 0xFF {
            continue;
        }
        if off0 == 0 || off0 > 0x100_0000 {
            continue;
        }
        return Some((cand, count));
    }
    None
}

/// Parse the trailer record table. Returns `None` when the buffer doesn't
/// match the battle-data pack shape.
pub fn detect(buf: &[u8]) -> Option<BattleDataPack> {
    let (count_off, count) = detect_header(buf)?;
    let table_offset = count_off + 8;
    let mut records = Vec::with_capacity(count as usize);
    let mut max_extent = table_offset + (count as usize) * 12;
    for i in 0..count as usize {
        let p = table_offset + i * 12;
        if p + 12 > buf.len() {
            break;
        }
        let on_disc_size = read_u32_le(buf, p)?;
        let id = read_u32_le(buf, p + 4)?;
        let data_offset = read_u32_le(buf, p + 8)?;
        if on_disc_size == 0 {
            // Table tail is zero-padded; stop here.
            break;
        }
        records.push(Record {
            index: i,
            on_disc_size,
            id,
            data_offset,
        });
        // Track the largest table coverage so we can align data_base above it.
        max_extent = max_extent.max(p + 12);
    }
    if records.is_empty() {
        return None;
    }
    // Align data_base up to the next DATA_BASE_ALIGN boundary past the table.
    let want_base = max_extent.div_ceil(DATA_BASE_ALIGN) * DATA_BASE_ALIGN;
    // Self-correct data_base: pick the first sector boundary at or past
    // `want_base` where every record's `dec_size` u32 prefix reads as a
    // plausible decompressed size (1 .. 4 MiB). Slots with offset=0 are
    // sentinel/filler entries that point back into the table region and
    // are excluded from this check.
    let mut data_base = want_base;
    let mut chose = false;
    let probe_limit = (want_base + 0x4_0000).min(buf.len().saturating_sub(8));
    while data_base <= probe_limit {
        let mut all_ok = true;
        for r in &records {
            if r.data_offset == 0 {
                // Sentinel/filler slot - retail rec 42 has offset 0 and
                // points back into the table region. Skip the dec_size
                // sanity check; decode_record will reject it later if
                // the caller tries to decode this slot.
                continue;
            }
            let start = data_base + r.data_offset as usize;
            if start + 4 > buf.len() {
                all_ok = false;
                break;
            }
            let Some(dec_size) = read_u32_le(buf, start) else {
                all_ok = false;
                break;
            };
            if dec_size == 0 || dec_size > 0x40_0000 {
                all_ok = false;
                break;
            }
        }
        if all_ok {
            chose = true;
            break;
        }
        data_base += DATA_BASE_ALIGN;
    }
    if !chose {
        return None;
    }
    Some(BattleDataPack {
        table_offset,
        record_count: count,
        records,
        data_base,
    })
}

/// Parse - errors instead of returning `None`.
pub fn parse(buf: &[u8]) -> Result<BattleDataPack> {
    detect(buf).ok_or_else(|| anyhow::anyhow!("not a battle_data pack"))
}

/// Decompress one record. Returns the decoded bytes plus the Legaia-TMD
/// byte range inside them (when locatable).
pub fn decode_record(buf: &[u8], pack: &BattleDataPack, idx: usize) -> Result<DecodedEntry> {
    let record = *pack
        .records
        .get(idx)
        .ok_or_else(|| anyhow::anyhow!("record index {} out of bounds", idx))?;
    let file_off = record.file_offset(pack.data_base);
    if file_off + 4 > buf.len() {
        bail!("record {} dec_size prefix past buffer end", idx);
    }
    let dec_size =
        read_u32_le(buf, file_off).ok_or_else(|| anyhow::anyhow!("dec_size read failed"))? as usize;
    if dec_size == 0 || dec_size > 0x40_0000 {
        bail!("record {} has implausible dec_size 0x{:x}", idx, dec_size);
    }
    // `on_disc_size` declares the slot's *allocation* footprint, not the
    // LZS stream length. The retail decoder reads tokens until enough
    // output bytes (`dec_size`) are produced; records often run their
    // LZS source past `on_disc_size` into the next slot's region.
    // Hand the decompressor everything from the dec_size prefix to the
    // end of file - it stops based on output count, not input count.
    let lzs_input = &buf[file_off + 4..];
    let bytes = legaia_lzs::decompress(lzs_input, dec_size)?;
    let tmd_range = locate_embedded_tmd(&bytes);
    Ok(DecodedEntry {
        record,
        bytes,
        tmd_range,
    })
}

/// Decompress every record. Records whose decode fails are skipped with
/// the error attached to the result via `Result<...>` so a caller can
/// surface them in a CLI listing.
pub fn decode_all(buf: &[u8], pack: &BattleDataPack) -> Vec<Result<DecodedEntry>> {
    (0..pack.records.len())
        .map(|i| decode_record(buf, pack, i))
        .collect()
}

/// Find the Legaia TMD inside a decoded entry.
///
/// Empirically two record-shape variants appear in retail 0865:
///   - Simple: TMD at offset 0x20 (after a 32-byte header whose u32[3]
///     holds the TMD body end).
///   - Nested: u32[1] / u32[2] hold non-zero sub-object end offsets and
///     the TMD shifts later in the buffer.
///
/// Try the canonical 0x20 position first; if that doesn't validate,
/// fall back to scanning every word-aligned offset for the magic and
/// keep the first one that passes structural checks.
fn locate_embedded_tmd(decoded: &[u8]) -> Option<std::ops::Range<usize>> {
    if decoded.len() < 0x20 + 12 {
        return None;
    }
    // Fast path: canonical 0x20 offset with u32[3] = body end.
    if let Some(rng) = try_tmd_at(decoded, 0x20, Some(12)) {
        return Some(rng);
    }
    // Fallback: word-aligned magic scan. Body end is unknown - rely on
    // legaia_tmd::parse to validate and report extent later.
    let mut off = 4;
    while off + 12 <= decoded.len() {
        if let Some(rng) = try_tmd_at(decoded, off, None) {
            return Some(rng);
        }
        off += 4;
    }
    None
}

fn try_tmd_at(
    decoded: &[u8],
    tmd_offset: usize,
    header_body_end_offset: Option<usize>,
) -> Option<std::ops::Range<usize>> {
    const TMD_MAGIC: u32 = 0x8000_0002;
    let magic = read_u32_le(decoded, tmd_offset)?;
    if magic != TMD_MAGIC {
        return None;
    }
    let flags = read_u32_le(decoded, tmd_offset + 4)?;
    if flags != 0 {
        return None;
    }
    let nobj = read_u32_le(decoded, tmd_offset + 8)?;
    if nobj == 0 || nobj > 64 {
        return None;
    }
    if let Some(off) = header_body_end_offset {
        let tmd_end = read_u32_le(decoded, off)? as usize;
        if tmd_end > tmd_offset && tmd_end <= decoded.len() {
            return Some(tmd_offset..tmd_end);
        }
    }
    // Without a header-supplied end, conservatively report the TMD as
    // running from `tmd_offset` to end-of-buffer. Callers needing the
    // exact extent should hand the range to `legaia_tmd::parse`.
    Some(tmd_offset..decoded.len())
}

fn read_u32_le(buf: &[u8], off: usize) -> Option<u32> {
    Some(u32::from_le_bytes(buf.get(off..off + 4)?.try_into().ok()?))
}

/// Probe the post-TMD region of a decoded entry for a CLUT-shaped run of
/// halfwords (16 valid RGB1555 colors with the high-transparency bit
/// clear in at least 12 of them). Returns the byte range of the first
/// such run, or `None` if no run matches.
///
/// The retail post-TMD layout interleaves CLUTs and 4bpp texture pixel
/// data without standard TIM image-block headers; this probe is a cheap
/// "is there a palette here" heuristic that engine integrations can use
/// to surface raw bytes for upload to a VRAM row. Not a replacement for
/// a full layout reverse - the post-TMD descriptor table at u32[3..0x20]
/// of the entry header points at specific palette positions, but that
/// table's semantics aren't yet pinned.
pub fn probe_first_clut_run(decoded: &[u8]) -> Option<std::ops::Range<usize>> {
    let tmd_range = locate_embedded_tmd(decoded)?;
    let mut off = tmd_range.end;
    // Align to 2 (halfword) - retail post-TMD regions are u16-aligned.
    if !off.is_multiple_of(2) {
        off += 1;
    }
    while off + 32 <= decoded.len() {
        if looks_clut_like(&decoded[off..off + 32]) {
            return Some(off..off + 32);
        }
        off += 2;
    }
    None
}

fn looks_clut_like(bytes: &[u8]) -> bool {
    debug_assert!(bytes.len() == 32);
    let mut nonzero = 0;
    let mut high_clear = 0;
    let mut distinct = std::collections::HashSet::new();
    for i in 0..16 {
        let h = u16::from_le_bytes([bytes[i * 2], bytes[i * 2 + 1]]);
        if h != 0 {
            nonzero += 1;
        }
        if h & 0x8000 == 0 {
            high_clear += 1;
        }
        distinct.insert(h);
    }
    nonzero >= 12 && high_clear >= 12 && distinct.len() >= 8
}

/// VRAM width in pixels (= halfword units) used by `find_clut_in_vram`.
/// Matches `legaia_tim::Vram::WIDTH` and `legaia_mednafen::gpu::VRAM_WIDTH`
/// without taking a runtime dep.
pub const VRAM_WIDTH: usize = 1024;
/// VRAM height in pixels.
pub const VRAM_HEIGHT: usize = 512;
/// VRAM byte size (1024 * 512 * 2).
pub const VRAM_BYTES: usize = VRAM_WIDTH * VRAM_HEIGHT * 2;
/// One CLUT-row width in halfwords (4bpp palette = 16 entries).
pub const CLUT_ROW_HALFWORDS: usize = 16;
/// One CLUT-row width in bytes.
pub const CLUT_ROW_BYTES: usize = CLUT_ROW_HALFWORDS * 2;

/// One match between a CLUT-shaped run inside a decoded `battle_data` record
/// and a 32-byte window in PSX VRAM. Used by `find_clut_in_vram` to build a
/// corpus of `(record, VRAM coord)` pairs from which the post-TMD descriptor
/// at `u32[3..0x20]` can be reverse-engineered.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct ClutVramMatch {
    /// Byte offset within the decoded record where the matching 32 bytes start.
    pub record_byte_offset: usize,
    /// VRAM coordinate of the match. `fb_x` is the pixel column (= halfword
    /// column) and `fb_y` is the pixel row.
    pub fb_x: u16,
    pub fb_y: u16,
}

impl ClutVramMatch {
    /// VRAM byte offset of the match (= `(fb_y * 1024 + fb_x) * 2`).
    pub fn vram_byte_offset(&self) -> usize {
        ((self.fb_y as usize) * VRAM_WIDTH + (self.fb_x as usize)) * 2
    }
}

/// Slide a halfword-aligned 32-byte window across `decoded` (skipping any
/// known TMD body), keep windows that pass [`looks_clut_like`], and look
/// each one up in `vram_bytes` as an exact byte match.
///
/// Returns one [`ClutVramMatch`] per `(record_offset, vram_position)` pair.
/// A single CLUT-shaped window may match multiple VRAM positions (the
/// runtime can upload the same palette to several CLUT rows when several
/// 4bpp prims share it). Callers that want a unique mapping should
/// post-filter on the VRAM coord range they expect.
///
/// `vram_bytes` must be exactly [`VRAM_BYTES`] long (1 MiB). The search is
/// bounded to halfword-aligned VRAM offsets within rows that have at least
/// 16 contiguous halfwords past the candidate `x` (i.e. `fb_x + 16 <=
/// 1024`) so a CLUT row never wraps to the next row.
///
/// **Why this matters:** the post-TMD region of each battle_data record
/// holds the character's textures + CLUTs but has no standard TIM headers,
/// so we can't infer `(fb_x, fb_y)` from the bytes themselves. By byte-
/// matching CLUT-shaped runs against retail VRAM captured mid-scene, we
/// build a corpus of `(record_idx, header_u32s, fb_xy)` pairs - enough to
/// reverse-engineer the descriptor encoding at the record header's
/// `u32[3..0x20]`.
pub fn find_clut_in_vram(decoded: &DecodedEntry, vram_bytes: &[u8]) -> Vec<ClutVramMatch> {
    if vram_bytes.len() != VRAM_BYTES {
        return Vec::new();
    }
    let buf = &decoded.bytes;
    // Don't bother scanning inside the embedded TMD body - the CLUT-like
    // heuristic gets too many false positives on UV / normal data inside
    // the TMD's primitive groups. Start at the byte right after the TMD
    // (or at offset 0x20 if no TMD was located).
    let start = match decoded.tmd_range.as_ref() {
        Some(rng) => rng.end,
        None => 0x20.min(buf.len()),
    };
    let aligned_start = (start + 1) & !1;

    // Precompute a row-by-row halfword view of VRAM so the substring
    // search can be done per row (each candidate CLUT row spans 16
    // halfwords in a single VRAM row).
    let mut hits = Vec::new();
    let mut off = aligned_start;
    while off + CLUT_ROW_BYTES <= buf.len() {
        let window = &buf[off..off + CLUT_ROW_BYTES];
        if looks_clut_like(window) {
            // Search every VRAM row for the exact byte sequence. We scan
            // each row independently so a 32-byte match can't straddle
            // two VRAM rows (the PSX never stores a CLUT row across the
            // 1024-pixel boundary).
            for row in 0..VRAM_HEIGHT {
                let row_off = row * VRAM_WIDTH * 2;
                let row_bytes = &vram_bytes[row_off..row_off + VRAM_WIDTH * 2];
                let mut start_col = 0;
                while start_col + CLUT_ROW_BYTES <= row_bytes.len() {
                    if let Some(rel) = find_at_step2(&row_bytes[start_col..], window) {
                        let col = start_col + rel;
                        debug_assert!(col.is_multiple_of(2));
                        let fb_x = (col / 2) as u16;
                        let fb_y = row as u16;
                        hits.push(ClutVramMatch {
                            record_byte_offset: off,
                            fb_x,
                            fb_y,
                        });
                        start_col = col + 2;
                    } else {
                        break;
                    }
                }
            }
        }
        off += 2;
    }
    hits
}

/// Find `needle` inside `haystack` starting at an even offset (halfword
/// alignment). Returns the byte offset of the first hit, or `None`.
fn find_at_step2(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || needle.len() > haystack.len() {
        return None;
    }
    let last = haystack.len() - needle.len();
    let mut i = 0;
    while i <= last {
        if haystack[i..i + needle.len()] == *needle {
            return Some(i);
        }
        i += 2;
    }
    None
}

/// Read the eight u32s of the record header (offsets `0x00..0x20`). When
/// the buffer is too short, missing words are reported as `0`.
pub fn record_header_u32s(decoded: &DecodedEntry) -> [u32; 8] {
    let mut out = [0u32; 8];
    for (i, word) in out.iter_mut().enumerate() {
        let off = i * 4;
        if off + 4 <= decoded.bytes.len() {
            *word = u32::from_le_bytes(
                decoded.bytes[off..off + 4]
                    .try_into()
                    .expect("4-byte slice"),
            );
        }
    }
    out
}

/// One CLUT row a `battle_data` record claims to contribute to VRAM,
/// paired with the record-relative byte offset of the 32 BGR555 bytes
/// and the (fb_x, fb_y) destination. Returned by [`clut_uploads`].
#[derive(Debug, Clone, Copy)]
pub struct ClutUpload {
    /// VRAM column (in halfwords / 16bpp pixels).
    pub fb_x: u16,
    /// VRAM row.
    pub fb_y: u16,
    /// Offset within the decoded record where the 32 BGR555 bytes live.
    pub record_byte_offset: usize,
}
impl ClutUpload {
    /// Slice the 32 CLUT bytes out of a decoded record. Returns `None`
    /// when the entry's `record_byte_offset` extends past the buffer.
    pub fn bytes<'a>(&self, decoded: &'a DecodedEntry) -> Option<&'a [u8]> {
        let end = self.record_byte_offset + CLUT_ROW_BYTES;
        if end > decoded.bytes.len() {
            return None;
        }
        Some(&decoded.bytes[self.record_byte_offset..end])
    }
}

/// Decode the post-TMD descriptor at `u32[3..0x20]` of a `battle_data`
/// record header into `(fb_x, fb_y, record_offset)` triples ready for
/// synthetic-CLUT VRAM upload. Empty when the descriptor encoding can't
/// be applied with confidence to this record.
///
/// ### Empirical findings (status)
///
/// The descriptor *encoding* has not yet been pinned. The corpus
/// methodology behind this function is to byte-match 32-byte windows
/// from each decoded record against PSX VRAM captured mid-scene with a
/// mednafen save state - the `mednafen-state clut-trace` CLI drives it,
/// and the pure analysis API is [`find_clut_in_vram`]. Across the four
/// retail saves bundled with our analysis fixtures (`mc2` = Rim Elm
/// town01, `mc3` = Izumi town, `mc4` = pre-battle, `mc6` = battle), the
/// corpus shows:
///
/// - The post-TMD pool of each retail PROT 0865 record byte-matches
///   into VRAM as a *texture* contribution at known (fb_x, fb_y) bases:
///   PROT 0865 record 41 maps to `fb_x=864, fb_y≈383..507` in town
///   saves, record 40 to `fb_x=864, fb_y≈426..433`, and the battle-only
///   records 4-9 to `fb_x=768`.
/// - The 32-byte halfword runs that *do* land at standard CLUT
///   coordinates (e.g. row 479 slots 8..14 - the CBAs town01 NPC TMDs
///   sample) are **not present verbatim** in any decoded battle_data
///   record. The same bytes do not appear (even as an 8-byte prefix)
///   in any other raw PROT entry or in `SCUS_942.54`. Their source is
///   external to the battle_data pack - likely a runtime palette
///   generator (the procedural hue cycle observed at row 479 slots 8-14
///   in town saves), or a pre-decoded pool we have not yet located.
///
/// As a result, returning a confident `(fb_x, fb_y)` per record is not
/// possible from the on-disc bytes alone. This function returns an empty
/// vector for now. The shape of the API is stable: when the descriptor
/// encoding (or its runtime resolver) is reverse-engineered, only the
/// body of this function changes and every caller in [`SceneResources`]
/// picks up the upload automatically.
///
/// See [`docs/formats/battle-data-pack.md`] for the full corpus
/// methodology and findings.
pub fn clut_uploads(_decoded: &DecodedEntry) -> Vec<ClutUpload> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a synthetic pack with one record carrying a recognizable
    /// payload through the LZS round-trip. Mirrors the retail layout:
    /// the count u32 lives at the last 4 bytes of the chunk0 payload
    /// (i.e. at offset `chunk0_size`), with the streaming terminator
    /// immediately after.
    fn synth(payload: &[u8]) -> Vec<u8> {
        // Encode the payload as a trivial LZS stream of literals.
        let mut lzs = Vec::new();
        let mut i = 0;
        while i < payload.len() {
            let group = (payload.len() - i).min(8);
            let mut control: u8 = 0;
            for b in 0..group {
                control |= 1 << b;
            }
            lzs.push(control);
            lzs.extend_from_slice(&payload[i..i + group]);
            i += group;
        }
        // Pad with all-literal groups so the decoder always has source.
        for _ in 0..16 {
            lzs.push(0xFF);
            lzs.extend_from_slice(&[0u8; 8]);
        }

        let dec_size = payload.len() as u32;
        let on_disc_size = 4 + lzs.len() as u32;

        let mut buf = Vec::new();
        // Streaming preamble: one type-0 chunk whose payload ends with
        // the (count, reserved) header pair. The detector reads the
        // count at `chunk0_size` and the records at `chunk0_size + 8`.
        // Pick chunk0_payload_size such that the last u32 of payload is
        // the count u32.
        let count_u32_position: usize = 0x40; // arbitrary - aligned multiple of 4
        let chunk0_size: u32 = count_u32_position as u32;
        let chunk_header = chunk0_size; // type=0, size=chunk0_size
        buf.extend_from_slice(&chunk_header.to_le_bytes());
        // Chunk payload: zero-fill up to count_u32_position, then count.
        while buf.len() < count_u32_position {
            buf.push(0);
        }
        // count u32 lives at offset `chunk0_size` = count_u32_position.
        buf.extend_from_slice(&1u32.to_le_bytes()); // count = 1
        // reserved u32 at chunk0_size + 4
        buf.extend_from_slice(&0u32.to_le_bytes());
        // Record table starts at chunk0_size + 8
        // Empirically retail records always start at a non-zero offset
        // (the retail pack leaves the first 0x1800 bytes after data_base
        // unused). Mirror that here so the detector's sanity bounds pass.
        let record_data_offset: u32 = 0x100;
        buf.extend_from_slice(&on_disc_size.to_le_bytes()); // record.on_disc_size
        buf.extend_from_slice(&0x42u32.to_le_bytes()); // record.id
        buf.extend_from_slice(&record_data_offset.to_le_bytes());
        // Pad to next 0x800 boundary - data_base.
        while !buf.len().is_multiple_of(0x800) {
            buf.push(0);
        }
        let data_base = buf.len();
        // Pad to record's data_offset within the data section.
        buf.resize(data_base + record_data_offset as usize, 0);
        // Record entry: u32 dec_size + LZS bytes
        buf.extend_from_slice(&dec_size.to_le_bytes());
        buf.extend_from_slice(&lzs);
        let want_end = data_base + record_data_offset as usize + on_disc_size as usize;
        buf.resize(want_end, 0);
        buf
    }

    #[test]
    fn detects_minimal_synthetic() {
        // Payload: 32-byte header (last u32[3] = 0x40 tmd_body_end) + TMD-shaped magic
        // + small TMD-like body so the locator validates.
        let mut payload = vec![0u8; 0x80];
        // header.u32[3] = tmd body end = 0x60
        payload[12..16].copy_from_slice(&0x60u32.to_le_bytes());
        // TMD at +0x20
        payload[0x20..0x24].copy_from_slice(&0x80000002u32.to_le_bytes());
        payload[0x24..0x28].copy_from_slice(&0u32.to_le_bytes()); // flags
        payload[0x28..0x2C].copy_from_slice(&1u32.to_le_bytes()); // nobj=1
        let buf = synth(&payload);
        let pack = detect(&buf).expect("should detect");
        assert_eq!(pack.records.len(), 1);
        assert_eq!(pack.records[0].id, 0x42);
        let entry = decode_record(&buf, &pack, 0).expect("decode");
        assert_eq!(entry.bytes.len(), payload.len());
        assert_eq!(entry.tmd_range, Some(0x20..0x60));
    }

    #[test]
    fn rejects_random_bytes() {
        let buf = vec![0xAAu8; 0x4000];
        assert!(detect(&buf).is_none());
    }

    #[test]
    fn rejects_short_buffers() {
        for n in [0usize, 1, 16, 31, 100] {
            let buf = vec![0u8; n];
            assert!(detect(&buf).is_none());
        }
    }

    #[test]
    fn rejects_nonzero_chunk0_type() {
        let mut buf = synth(&[0u8; 0x80]);
        // Bump the chunk0 type byte to non-zero.
        let mut h = u32::from_le_bytes(buf[0..4].try_into().unwrap());
        h |= 0x05_00_00_00;
        buf[0..4].copy_from_slice(&h.to_le_bytes());
        assert!(detect(&buf).is_none());
    }

    /// Build a CLUT-shaped 32-byte window: 16 BGR555 halfwords spanning a
    /// hue cycle, all with the high-transparency bit clear.
    fn fake_clut(seed: u16) -> [u8; CLUT_ROW_BYTES] {
        let mut out = [0u8; CLUT_ROW_BYTES];
        for i in 0..16u16 {
            // Build a unique-ish BGR555 value (high bit 0 -> not STP).
            let val = (seed.wrapping_add(i * 17)) & 0x7FFF | (i << 5);
            out[(i as usize) * 2..(i as usize) * 2 + 2].copy_from_slice(&val.to_le_bytes());
        }
        out
    }

    #[test]
    fn find_clut_in_vram_finds_exact_match() {
        // Synth a decoded entry whose post-TMD pool starts at offset 0x40
        // and holds one CLUT-shaped row. Pad with bytes that fail the
        // CLUT shape heuristic so we only get hits from the placed window.
        let clut = fake_clut(0x1234);
        let mut bytes = vec![0u8; 0x80];
        // Pad bytes around the CLUT with all-STP halfwords (high bit set)
        // so the scanner doesn't see neighboring shifted windows as
        // CLUT-shaped.
        for chunk in bytes.chunks_exact_mut(2) {
            chunk.copy_from_slice(&0x8000u16.to_le_bytes());
        }
        bytes[0x40..0x40 + CLUT_ROW_BYTES].copy_from_slice(&clut);
        let entry = DecodedEntry {
            record: Record {
                index: 0,
                on_disc_size: 0,
                id: 0,
                data_offset: 0,
            },
            bytes,
            tmd_range: Some(0x20..0x40),
        };
        // Build a VRAM with the same 32-byte sequence at fb=(100, 50).
        // Surround with all-STP padding so the search doesn't find
        // shifted matches around our placement.
        let mut vram = vec![0u8; VRAM_BYTES];
        for chunk in vram.chunks_exact_mut(2) {
            chunk.copy_from_slice(&0x8000u16.to_le_bytes());
        }
        let off = (50 * VRAM_WIDTH + 100) * 2;
        vram[off..off + CLUT_ROW_BYTES].copy_from_slice(&clut);
        let hits = find_clut_in_vram(&entry, &vram);
        // The shifted-window search produces multiple overlapping hits
        // when the CLUT-shaped run is wider than 32 bytes. The canonical
        // anchor (record offset 0x40 → fb=(100, 50)) must be present.
        let canonical = hits
            .iter()
            .find(|m| m.fb_x == 100 && m.fb_y == 50 && m.record_byte_offset == 0x40);
        assert!(canonical.is_some(), "missing canonical hit; got {:?}", hits);
        assert_eq!(canonical.unwrap().vram_byte_offset(), off);
    }

    #[test]
    fn find_clut_in_vram_skips_short_vram() {
        let entry = DecodedEntry {
            record: Record {
                index: 0,
                on_disc_size: 0,
                id: 0,
                data_offset: 0,
            },
            bytes: vec![0u8; 0x80],
            tmd_range: Some(0x20..0x40),
        };
        // VRAM too small - should return empty.
        assert!(find_clut_in_vram(&entry, &[0u8; 1024]).is_empty());
    }

    #[test]
    fn clut_uploads_returns_empty_until_descriptor_is_pinned() {
        // The current implementation is the documented no-op: the
        // descriptor encoding at `u32[3..0x20]` has not been pinned by
        // the byte-match corpus. This test guards the contract so
        // callers in `SceneResources::build_targeted` keep building.
        let entry = DecodedEntry {
            record: Record {
                index: 0,
                on_disc_size: 0,
                id: 0x42,
                data_offset: 0,
            },
            bytes: vec![0u8; 0x80],
            tmd_range: Some(0x20..0x40),
        };
        assert!(clut_uploads(&entry).is_empty());
    }

    #[test]
    fn record_header_u32s_reads_eight_words() {
        let mut bytes = vec![0u8; 0x40];
        for i in 0..8u32 {
            let off = (i as usize) * 4;
            bytes[off..off + 4].copy_from_slice(&(i * 0x11).to_le_bytes());
        }
        let entry = DecodedEntry {
            record: Record {
                index: 0,
                on_disc_size: 0,
                id: 0,
                data_offset: 0,
            },
            bytes,
            tmd_range: None,
        };
        let h = record_header_u32s(&entry);
        for (i, w) in h.iter().enumerate() {
            assert_eq!(*w, (i as u32) * 0x11);
        }
    }
}
