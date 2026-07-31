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
//! (`offset[i+1] == offset[i] + size[i]`; sizes are sector-aligned;
//! `id = 0` marks section boundaries / default-variant slots; an all-zero
//! entry terminates), and the slot region at `data_base` (0x8000 in all
//! four retail files) holds per-slot `[u32 dec_size][LZS]` streams
//! decoding to:
//!
//! ```text
//!   +0x00  u32 frame_off       ; loader-frame offset (0x14 + 4*attach_objs)
//!   +0x04  u32 swing_rec_a     ; swing action record (sections 2..4; else 0)
//!   +0x08  u32 swing_rec_b     ; second swing record (section 4 only)
//!   +0x0C  u32 tmd_body_end    ; offset where the Legaia TMD body ends
//!   +0x10  s16 attach_objs     ; attach-object record count
//!   +0x12  u16 upload_flag     ; texture-pool-present flag
//!   +0x14  u32 attach_off[]    ; attach-object record offsets
//!   +frame_off  loader frame   ; attach_count + bone ids + embedded TMD
//!   +tmd_body_end              ; texture/CLUT pool
//! ```
//!
//! (Full layout: `docs/formats/battle-data-pack.md` § Decompressed slot
//! layout; swing records: [`crate::battle_char_assembly::swing_battle_animations`].)
//!
//! ### Framing
//!
//! The walker reads the descriptor table in its runtime-pinned frame:
//! entries start at `desc_off` itself (the header's first word doubles as a
//! type-0 streaming chunk header, which is how streaming-format walkers skip
//! the head cleanly). Detection validates the chain invariant
//! (`offset[i+1] == offset[i] + size[i]`, entry 0 at offset 0) plus
//! sector-aligned sizes, which is strict enough to accept all four retail
//! player files - including Terra's 0866, whose table is all-default
//! (`id = 0`) entries - while rejecting random input.
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

/// Maximum plausible record count we'll accept before giving up on
/// finding the table terminator.
const MAX_RECORDS: usize = 256;

/// Alignment for the LZS-data section base and every slot size (sector
/// boundary).
const DATA_BASE_ALIGN: usize = 0x800;

/// One 12-byte `[id, offset, size]` entry of the descriptor table.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct Record {
    /// Index in the descriptor table (0..count).
    pub index: usize,
    /// Slot id (0..0xBA observed). `0` marks a section boundary /
    /// default-variant slot - still a real, decodable slot.
    pub id: u32,
    /// Byte offset from `data_base` where the compressed entry lives.
    /// Entry 0 sits at offset 0; the chain invariant
    /// `offset[i+1] == offset[i] + size[i]` holds across the table.
    pub data_offset: u32,
    /// Slot allocation footprint in bytes (sector-aligned). This is the
    /// slot's *region* size, not the LZS stream length - the stream may
    /// end short of it (zero padding) and the decoder is output-bounded.
    pub size: u32,
}

impl Record {
    /// Absolute file offset of the compressed entry.
    pub fn file_offset(&self, data_base: usize) -> usize {
        data_base + self.data_offset as usize
    }
}

/// Parsed pack header (descriptor table + data base).
#[derive(Debug, Clone, Serialize)]
pub struct BattleDataPack {
    /// Byte offset where the descriptor table starts (= the header's
    /// `desc_off` word).
    pub table_offset: usize,
    /// Every real descriptor entry, in table order. The terminating
    /// all-zero entry is excluded.
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
    detect(buf).is_some()
}

/// Parse the descriptor table. Returns `None` when the buffer doesn't
/// match the battle-data pack shape.
pub fn detect(buf: &[u8]) -> Option<BattleDataPack> {
    if buf.len() < 32 {
        return None;
    }
    // Header word 0 = desc_off. It doubles as a type-0 streaming chunk
    // header ((0x00 << 24) | size), so the high byte must be zero.
    let desc_off_word = legaia_bytes::u32_le(buf, 0)?;
    if (desc_off_word >> 24) != 0 {
        return None;
    }
    let table_offset = desc_off_word as usize;
    // The header + record[0] LZS stream precede the table; require room
    // for at least the 0x10-byte header plus one table entry.
    if table_offset < 0x10 || table_offset + 12 > buf.len() {
        return None;
    }
    // Header words 1..3: record[0]'s CLUT offsets + decoded-size budget.
    // All four retail files order them clut_a < clut_b < budget.
    let clut_a = legaia_bytes::u32_le(buf, 4)?;
    let clut_b = legaia_bytes::u32_le(buf, 8)?;
    let budget = legaia_bytes::u32_le(buf, 12)?;
    if clut_a == 0 || clut_a >= clut_b || clut_b >= budget {
        return None;
    }
    // Walk the 12-byte [id, offset, size] entries. The chain invariant
    // (entry 0 at offset 0, each entry starting where the previous ends,
    // sector-aligned sizes) is the structural signature; an all-zero
    // entry terminates the table.
    let mut records = Vec::new();
    let mut expected_offset: u32 = 0;
    let mut terminated = false;
    let mut p = table_offset;
    while p + 12 <= buf.len() && records.len() < MAX_RECORDS {
        let id = legaia_bytes::u32_le(buf, p)?;
        let offset = legaia_bytes::u32_le(buf, p + 4)?;
        let size = legaia_bytes::u32_le(buf, p + 8)?;
        if size == 0 {
            // Only the canonical all-zero terminator is accepted; a
            // zero-size entry with a residual id/offset is a shape
            // mismatch.
            terminated = id == 0 && offset == 0;
            break;
        }
        if id > 0xFF
            || offset != expected_offset
            || size > 0x40_0000
            || !(size as usize).is_multiple_of(DATA_BASE_ALIGN)
        {
            return None;
        }
        records.push(Record {
            index: records.len(),
            id,
            data_offset: offset,
            size,
        });
        expected_offset = offset.checked_add(size)?;
        p += 12;
    }
    if !terminated || records.is_empty() {
        return None;
    }
    // Self-correct data_base: pick the first sector boundary at or past
    // the table end where every record's `dec_size` u32 prefix reads as
    // a plausible decompressed size (1 .. 4 MiB). All four retail files
    // land at 0x8000 (the gap between table end and 0x8000 is
    // zero-padded, which the dec_size check skips past).
    let table_end = p + 12;
    let want_base = table_end.div_ceil(DATA_BASE_ALIGN) * DATA_BASE_ALIGN;
    let mut data_base = want_base;
    let mut chose = false;
    let probe_limit = (want_base + 0x4_0000).min(buf.len().saturating_sub(8));
    while data_base <= probe_limit {
        let mut all_ok = true;
        for r in &records {
            let start = data_base + r.data_offset as usize;
            if start + 4 > buf.len() {
                all_ok = false;
                break;
            }
            let Some(dec_size) = legaia_bytes::u32_le(buf, start) else {
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
    let dec_size = legaia_bytes::u32_le(buf, file_off)
        .ok_or_else(|| anyhow::anyhow!("dec_size read failed"))? as usize;
    if dec_size == 0 || dec_size > 0x40_0000 {
        bail!("record {} has implausible dec_size 0x{:x}", idx, dec_size);
    }
    // `size` declares the slot's *allocation* footprint, not the LZS
    // stream length. The retail decoder reads tokens until enough
    // output bytes (`dec_size`) are produced; hand the decompressor
    // everything from the dec_size prefix to the end of file - it
    // stops based on output count, not input count.
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
/// Empirically two record-shape variants appear in the retail files:
///   - Simple: TMD at offset 0x20 (after a 32-byte header whose u32[3]
///     holds the TMD body end; `frame_off = 0x14`).
///   - Attach-object slots: `frame_off` grows by 4 per attach-object
///     record (`+0x18`/`+0x1C`) and the TMD shifts later in the buffer.
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
    let magic = legaia_bytes::u32_le(decoded, tmd_offset)?;
    if magic != TMD_MAGIC {
        return None;
    }
    let flags = legaia_bytes::u32_le(decoded, tmd_offset + 4)?;
    if flags != 0 {
        return None;
    }
    let nobj = legaia_bytes::u32_le(decoded, tmd_offset + 8)?;
    if nobj == 0 || nobj > 64 {
        return None;
    }
    if let Some(off) = header_body_end_offset {
        let tmd_end = legaia_bytes::u32_le(decoded, off)? as usize;
        if tmd_end > tmd_offset && tmd_end <= decoded.len() {
            return Some(tmd_offset..tmd_end);
        }
    }
    // Without a header-supplied end, conservatively report the TMD as
    // running from `tmd_offset` to end-of-buffer. Callers needing the
    // exact extent should hand the range to `legaia_tmd::parse`.
    Some(tmd_offset..decoded.len())
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
/// known TMD body), keep windows that pass `looks_clut_like`, and look
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
    /// 0x10-byte header, descriptor table at `desc_off` with the
    /// all-zero terminator, sector-aligned slot region past the table.
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
        // Slot allocation footprint: dec_size prefix + stream, rounded up
        // to a sector like retail.
        let slot_size = ((4 + lzs.len()).div_ceil(0x800) * 0x800) as u32;

        let desc_off: u32 = 0x40; // arbitrary - past the 0x10-byte header
        let mut buf = Vec::new();
        buf.extend_from_slice(&desc_off.to_le_bytes());
        // clut_a < clut_b < budget (record[0] palette-chain words).
        buf.extend_from_slice(&0x14u32.to_le_bytes());
        buf.extend_from_slice(&0x20u32.to_le_bytes());
        buf.extend_from_slice(&0x30u32.to_le_bytes());
        // record[0] stream region - zero-fill up to the table.
        buf.resize(desc_off as usize, 0);
        // Descriptor table: one [id, offset, size] entry + terminator.
        buf.extend_from_slice(&0x42u32.to_le_bytes()); // id
        buf.extend_from_slice(&0u32.to_le_bytes()); // offset (entry 0 = 0)
        buf.extend_from_slice(&slot_size.to_le_bytes()); // size
        buf.extend_from_slice(&[0u8; 12]); // all-zero terminator
        // Pad to next 0x800 boundary - data_base.
        while !buf.len().is_multiple_of(0x800) {
            buf.push(0);
        }
        let data_base = buf.len();
        // Slot entry: u32 dec_size + LZS bytes, padded to the slot size.
        buf.extend_from_slice(&dec_size.to_le_bytes());
        buf.extend_from_slice(&lzs);
        buf.resize(data_base + slot_size as usize, 0);
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
                id: 0,
                data_offset: 0,
                size: 0,
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
                id: 0,
                data_offset: 0,
                size: 0,
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
                id: 0x42,
                data_offset: 0,
                size: 0,
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
                id: 0,
                data_offset: 0,
                size: 0,
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
