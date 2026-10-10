//! Legaia ANM (asset type 0x06) - animation pack container.
//!
//! ## Format (in RAM, post-load)
//!
//! ```text
//! u32 count                        // number of animation records
//! u32 byte_offset[count]           // each is a byte offset relative to
//!                                  //   payload base (i.e. relative to
//!                                  //   the count word)
//! ...record bytes, packed back-to-back...
//! ```
//!
//! The byte offsets in the table are *relative to the payload base* -
//! same as the count u32 itself. Record `i` lives at
//! `payload[byte_offset[i] .. byte_offset[i+1]]`, with the last record
//! extending to the payload end.
//!
//! ## Wrapper preamble (only when extracted from a RAM dump)
//!
//! When the dispatcher (`FUN_8001f05c` case 6) loads ANM data, the
//! malloc'd buffer at `DAT_8007b7c8` carries a 16-byte allocator preamble
//! before the payload:
//!
//! ```text
//! +0x00  back_ptr        (RAM ptr - usually base - 0xC or similar)
//! +0x04  forward_ptr     (RAM ptr to next allocation)
//! +0x08  forward_ptr_2   (RAM ptr - sometimes 0)
//! +0x0C  expanded_size   (u32 - total allocated bytes including the
//!                         preamble's worth of slack? exact convention
//!                         TBC; in observed dumps it's `payload_size`)
//! +0x10  -- payload starts here --
//! ```
//!
//! [`parse`] takes the *payload* (no preamble). Use [`peel_preamble`] to
//! strip the wrapper from a RAM-extracted blob first.
//!
//! ## Per-record content
//!
//! Each record begins with an 8-byte common header observed across two
//! independent captures (title screen, town):
//!
//! ```text
//! u16 a       // varies (e.g. 0x0A, 0x06, 0x02)
//! u16 b       // varies (e.g. 0x1E, 0x14, 0x28) - likely frame count
//! u16 marker1 // = 0x080C in every record observed
//! u16 marker2 // = 0x0002 (or 0x0004) - sub-format selector
//! ```
//!
//! For records consumed via animation opcode `0x06` (the bulk of retail
//! ANM data), the body after the header is a per-bone **keyframe table**,
//! not opcode bytecode. The per-frame interpreter is the canonical actor
//! tick `FUN_80021DF4` in `SCUS_942.54` (block at `0x80022ec4..0x80023040`),
//! which walks the table indexed by a bone count sourced from the actor's
//! mesh context. Layout:
//!
//! ```text
//! +0..+8                     header (a, b, marker1, marker2)
//! +8..+(8 + 8*N)             per-bone OUTPUT slots - written by the tick
//!                             (8 bytes per bone: packed pos+rot deltas)
//! +(8 + 8*N)..+(8 + 32*N)    per-bone KEYFRAME data - read by the tick
//!                             (24 bytes per bone = 12 little-endian i16
//!                              shorts: src_pos.xyz, dst_pos.xyz,
//!                              src_rot.xyz, dst_rot.xyz)
//! ```
//!
//! The tick reads the 12 shorts, multiplies the `(dst - src)` deltas by
//! `actor[+0x22]` (the per-actor interpolation factor - driven from the
//! field-VM frame counter), and writes the resulting 8 packed bytes back
//! into the OUTPUT slots. See [`KeyframeReader`] for the typed accessor.
//!
//! ## How records are consumed at runtime
//!
//! `FUN_80024cfc` in `SCUS_942.54` is the only static-binary reader. It
//! does (in pseudocode):
//!
//! ```text
//! play_anm_by_id(id, actor, ?) {
//!     base = DAT_8007b7c8;                  // ANM payload base
//!     iVar3 = mem32[base + id*4 + 4];       // table[id] (skips count u32)
//!     actor->anim_ptr (+0x4c) = base + iVar3;
//!     actor->anim_op_code (+0x56) = 0xb;
//!     actor->anim_timer (+0x68) = 100;
//! }
//! ```
//!
//! The overlay (`FUN_801D6704` in the title overlay) loops `id ∈ 0..count`
//! and calls this for every record, registering an animation per actor.

use anyhow::{Result, bail};
use serde::Serialize;

pub mod player;
pub use player::{AnimPlayer, PoseFrame};

/// Size of the optional 16-byte allocator preamble that wraps a RAM-loaded
/// ANM buffer. Not present in on-disc ANM blobs.
pub const PREAMBLE_SIZE: usize = 0x10;

/// Offset of `expanded_size` within the preamble (u32 LE).
pub const PREAMBLE_EXPANDED_SIZE_OFFSET: usize = 0x0C;

/// Bytes per offset-table entry (u32 LE).
pub const TABLE_ENTRY_SIZE: usize = 4;

/// Sane upper bound on `count` (paranoid; observed counts are 24..69).
pub const MAX_REASONABLE_COUNT: usize = 4096;

/// Byte length of the common per-record header.
pub const RECORD_HEADER_SIZE: usize = 8;

/// Constant marker u16 at record header `+4..+6`. Observed identical
/// across all 93 records in two independent captures (title + town).
pub const RECORD_MARKER_1: u16 = 0x080C;

/// Observed values of the flag/variant u16 at record header `+6..+8`.
/// Not constant - looks like a sub-format selector. Title+town corpus:
/// `0x0002` (78%) and `0x0004` (22%).
pub const RECORD_FLAG_VALUES: &[u16] = &[0x0002, 0x0004];

/// Optional allocator preamble that wraps a RAM-extracted ANM buffer.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct Preamble {
    pub back_ptr: u32,
    pub forward_ptr: u32,
    pub forward_ptr_2: u32,
    pub expanded_size: u32,
}

impl Preamble {
    /// Read a preamble from the first 16 bytes of `buf`.
    pub fn from_bytes(buf: &[u8]) -> Result<Self> {
        if buf.len() < PREAMBLE_SIZE {
            bail!(
                "buffer too small for ANM preamble: {} < {}",
                buf.len(),
                PREAMBLE_SIZE
            );
        }
        Ok(Self {
            back_ptr: u32::from_le_bytes(buf[0..4].try_into().unwrap()),
            forward_ptr: u32::from_le_bytes(buf[4..8].try_into().unwrap()),
            forward_ptr_2: u32::from_le_bytes(buf[8..12].try_into().unwrap()),
            expanded_size: u32::from_le_bytes(buf[12..16].try_into().unwrap()),
        })
    }
}

/// Strip the optional 16-byte allocator preamble from a RAM-extracted blob,
/// trimming to `expanded_size`. Returns the payload slice ready for [`parse`].
pub fn peel_preamble(buf: &[u8]) -> Result<&[u8]> {
    let pre = Preamble::from_bytes(buf)?;
    let n = pre.expanded_size as usize;
    let end = PREAMBLE_SIZE
        .checked_add(n)
        .ok_or_else(|| anyhow::anyhow!("preamble expanded_size overflows"))?;
    if end > buf.len() {
        bail!(
            "preamble claims {} payload bytes ({} total) but buffer is {} bytes",
            n,
            end,
            buf.len()
        );
    }
    Ok(&buf[PREAMBLE_SIZE..end])
}

/// Parsed ANM payload. Holds the count, offset table, and per-record byte
/// ranges within the payload buffer.
#[derive(Debug, Clone, Serialize)]
pub struct AnmPack {
    /// Total payload size in bytes (from `parse`'s input).
    pub payload_size: usize,
    /// Number of records as declared by the count u32 at offset 0.
    pub count: usize,
    /// Per-record byte ranges within the payload.
    pub records: Vec<RecordRange>,
}

/// One record's byte range within the payload.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct RecordRange {
    /// Index in the offset table.
    pub index: usize,
    /// Byte offset (from payload start) of the first byte of this record.
    pub offset: usize,
    /// Size in bytes of this record (computed from `offset[i+1] - offset[i]`,
    /// or `payload_size - offset[last]` for the final record).
    pub size: usize,
}

/// Parse an ANM payload (count + offset table + records).
pub fn parse(payload: &[u8]) -> Result<AnmPack> {
    if payload.len() < 4 {
        bail!("payload too small ({} < 4)", payload.len());
    }
    let count = u32::from_le_bytes(payload[0..4].try_into().unwrap()) as usize;
    if count == 0 {
        return Ok(AnmPack {
            payload_size: payload.len(),
            count: 0,
            records: Vec::new(),
        });
    }
    if count > MAX_REASONABLE_COUNT {
        bail!("implausible ANM count {}", count);
    }
    let table_end = 4 + count * TABLE_ENTRY_SIZE;
    if table_end > payload.len() {
        bail!(
            "ANM offset table ({} entries, ends at byte {}) overruns payload ({} bytes)",
            count,
            table_end,
            payload.len()
        );
    }

    let mut offsets: Vec<usize> = Vec::with_capacity(count);
    for i in 0..count {
        let p = 4 + i * TABLE_ENTRY_SIZE;
        let v = u32::from_le_bytes(payload[p..p + 4].try_into().unwrap()) as usize;
        if v < table_end {
            bail!(
                "offset[{}] = 0x{:X} points into the offset table (table ends at 0x{:X})",
                i,
                v,
                table_end
            );
        }
        if v > payload.len() {
            bail!(
                "offset[{}] = 0x{:X} past payload end ({} bytes)",
                i,
                v,
                payload.len()
            );
        }
        if let Some(prev) = offsets.last()
            && v < *prev
        {
            bail!(
                "offsets not monotonic: offset[{}] = 0x{:X} < offset[{}] = 0x{:X}",
                i,
                v,
                i - 1,
                prev
            );
        }
        offsets.push(v);
    }

    let mut records = Vec::with_capacity(count);
    for i in 0..count {
        let start = offsets[i];
        let end = if i + 1 < count {
            offsets[i + 1]
        } else {
            payload.len()
        };
        records.push(RecordRange {
            index: i,
            offset: start,
            size: end - start,
        });
    }

    Ok(AnmPack {
        payload_size: payload.len(),
        count,
        records,
    })
}

/// One record's parsed common header.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct RecordHeader {
    /// First u16 - varies (3..14 observed). Possibly a record kind / opcode.
    pub a: u16,
    /// Second u16 - varies (typically a small count, 0..40). Likely the
    /// frame count or duration of the animation.
    pub b: u16,
    /// Fixed format marker - `RECORD_MARKER_1` in every observed record.
    pub marker_1: u16,
    /// Variant/flag selector - `0x0002` or `0x0004` observed.
    pub flag: u16,
    /// `true` iff `marker_1 == RECORD_MARKER_1`.
    pub marker_ok: bool,
    /// `true` iff `flag` is one of `RECORD_FLAG_VALUES`.
    pub flag_known: bool,
}

impl RecordHeader {
    pub fn from_bytes(buf: &[u8]) -> Result<Self> {
        if buf.len() < RECORD_HEADER_SIZE {
            bail!(
                "record too small for header: {} < {}",
                buf.len(),
                RECORD_HEADER_SIZE
            );
        }
        let a = u16::from_le_bytes(buf[0..2].try_into().unwrap());
        let b = u16::from_le_bytes(buf[2..4].try_into().unwrap());
        let marker_1 = u16::from_le_bytes(buf[4..6].try_into().unwrap());
        let flag = u16::from_le_bytes(buf[6..8].try_into().unwrap());
        Ok(Self {
            a,
            b,
            marker_1,
            flag,
            marker_ok: marker_1 == RECORD_MARKER_1,
            flag_known: RECORD_FLAG_VALUES.contains(&flag),
        })
    }
}

/// Borrow a single record's bytes from the payload.
pub fn record_bytes<'a>(payload: &'a [u8], rec: &RecordRange) -> &'a [u8] {
    &payload[rec.offset..rec.offset + rec.size]
}

/// Per-bone keyframe entry - the 24-byte block the actor tick reads for
/// each bone. Twelve `i16` shorts: source pose (pos.xyz + rot.xyz) plus
/// target pose. The runtime computes `(dst - src) * factor` to drive the
/// interpolation between the two poses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct BoneKeyframe {
    pub src_pos: [i16; 3],
    pub dst_pos: [i16; 3],
    pub src_rot: [i16; 3],
    pub dst_rot: [i16; 3],
}

impl BoneKeyframe {
    /// Size of one keyframe entry in bytes.
    pub const SIZE: usize = 24;

    /// Parse one keyframe entry from a 24-byte slice.
    pub fn from_bytes(buf: &[u8]) -> Result<Self> {
        if buf.len() < Self::SIZE {
            bail!(
                "buffer too small for bone keyframe: {} < {}",
                buf.len(),
                Self::SIZE
            );
        }
        let read = |off: usize| i16::from_le_bytes([buf[off], buf[off + 1]]);
        Ok(Self {
            src_pos: [read(0), read(2), read(4)],
            dst_pos: [read(6), read(8), read(10)],
            src_rot: [read(12), read(14), read(16)],
            dst_rot: [read(18), read(20), read(22)],
        })
    }

    /// Linear interpolation between source and target pose using a
    /// `factor` in `[0, 256)` (the actor tick uses this convention; the
    /// math is `src + ((dst - src) * factor) / 256`). Returns
    /// `(pos, rot)` as two `[i16; 3]` triples.
    pub fn interpolate(&self, factor_u8: u8) -> ([i16; 3], [i16; 3]) {
        let lerp = |s: i16, d: i16| -> i16 {
            let delta = d as i32 - s as i32;
            let scaled = (delta * factor_u8 as i32) >> 8;
            (s as i32 + scaled).clamp(i16::MIN as i32, i16::MAX as i32) as i16
        };
        let pos = [
            lerp(self.src_pos[0], self.dst_pos[0]),
            lerp(self.src_pos[1], self.dst_pos[1]),
            lerp(self.src_pos[2], self.dst_pos[2]),
        ];
        let rot = [
            lerp(self.src_rot[0], self.dst_rot[0]),
            lerp(self.src_rot[1], self.dst_rot[1]),
            lerp(self.src_rot[2], self.dst_rot[2]),
        ];
        (pos, rot)
    }
}

/// Walks the per-bone keyframe table inside an animation opcode-`6`
/// record body. The bone count is sourced from the actor's mesh context
/// at runtime; pass it explicitly here.
///
/// Layout:
///
/// ```text
/// header[8] | output[8 * N] | keyframes[24 * N]
/// ```
///
/// where `N = bone_count`. The keyframe table starts at `8 + 8 * N` and
/// must end within the record. `KeyframeReader::parse` validates that.
pub struct KeyframeReader<'a> {
    /// Backing record bytes (full record including header).
    record: &'a [u8],
    /// Bone count - supplied by the caller (the actor tick reads this
    /// from the mesh context).
    bone_count: usize,
    /// Byte offset where the keyframe table begins.
    keyframe_table_offset: usize,
}

impl<'a> KeyframeReader<'a> {
    /// Validate the record can host a keyframe table for `bone_count`
    /// bones and return a reader. Errors if the record is too small.
    pub fn parse(record: &'a [u8], bone_count: usize) -> Result<Self> {
        if record.len() < RECORD_HEADER_SIZE {
            bail!("record too small for header");
        }
        // Use checked arithmetic throughout: `bone_count` is supplied by
        // the caller (and ultimately derived from attacker-controlled mesh
        // metadata in offline tooling), so a huge value must produce an
        // `Err`, not an unchecked-multiply / unchecked-add overflow panic.
        let overflow = || anyhow::anyhow!("keyframe table size overflows");
        let output_bytes = 8usize.checked_mul(bone_count).ok_or_else(overflow)?;
        let kf_offset = RECORD_HEADER_SIZE
            .checked_add(output_bytes)
            .ok_or_else(overflow)?;
        let kf_bytes = BoneKeyframe::SIZE
            .checked_mul(bone_count)
            .ok_or_else(overflow)?;
        let kf_end = kf_offset.checked_add(kf_bytes).ok_or_else(overflow)?;
        if kf_end > record.len() {
            bail!(
                "record ({} bytes) too small for keyframe table at +0x{:X} for {} bones (need {} bytes)",
                record.len(),
                kf_offset,
                bone_count,
                kf_end
            );
        }
        Ok(Self {
            record,
            bone_count,
            keyframe_table_offset: kf_offset,
        })
    }

    /// Number of bones the keyframe table covers.
    pub fn bone_count(&self) -> usize {
        self.bone_count
    }

    /// Borrow the per-bone keyframe entry. Returns `None` if `bone` is
    /// out of range.
    pub fn keyframe(&self, bone: usize) -> Option<BoneKeyframe> {
        if bone >= self.bone_count {
            return None;
        }
        let off = self.keyframe_table_offset + bone * BoneKeyframe::SIZE;
        BoneKeyframe::from_bytes(&self.record[off..off + BoneKeyframe::SIZE]).ok()
    }

    /// Iterate every per-bone keyframe entry in order. Yields `bone_count`
    /// items; entries that fail to parse (shouldn't happen post-`parse`)
    /// are silently skipped.
    pub fn iter(&self) -> impl Iterator<Item = BoneKeyframe> + '_ {
        (0..self.bone_count).filter_map(move |i| self.keyframe(i))
    }

    /// Heuristic: a record's body length is consistent with a keyframe
    /// table when `record_len == 8 + 32 * bone_count` exactly. Useful to
    /// reject misclassified records before we hand them to the tick.
    pub fn fits_exactly(record_len: usize, bone_count: usize) -> bool {
        record_len == RECORD_HEADER_SIZE + 32 * bone_count
    }

    /// Search the plausible bone-count range that makes the record fit
    /// exactly. Returns the count or `None` if no count satisfies the
    /// equation. Useful when classifying records without a mesh context
    /// (the runtime always knows `N`, but offline tooling doesn't).
    pub fn infer_bone_count(record_len: usize) -> Option<usize> {
        if record_len < RECORD_HEADER_SIZE {
            return None;
        }
        let body = record_len - RECORD_HEADER_SIZE;
        if body.is_multiple_of(32) {
            Some(body / 32)
        } else {
            None
        }
    }
}

/// Frequency of each byte value in a record's bytecode region (i.e. the
/// bytes after the 8-byte common header).
///
/// The per-record bytecode interpreter is not statically reachable in
/// `SCUS_942.54` - `FUN_80024CFC` is the public entry point but it just
/// stows the per-record bytecode pointer in `actor[+0x4C]` and lets a
/// per-frame actor tick consume it (the actual dispatcher hasn't been
/// captured yet). Until then, this histogram surfaces the byte
/// distribution so downstream analysis can spot common opcode bytes
/// without re-deriving the count loop in every consumer.
///
/// Returns a 256-element array where index `b` is the count of byte `b`
/// across the bytecode region.
pub fn record_bytecode_histogram(record: &[u8]) -> [u32; 256] {
    let mut hist = [0u32; 256];
    if record.len() <= RECORD_HEADER_SIZE {
        return hist;
    }
    for &b in &record[RECORD_HEADER_SIZE..] {
        hist[b as usize] += 1;
    }
    hist
}

/// Same as [`record_bytecode_histogram`] but walks every record in `pack`,
/// accumulating into one histogram. Useful for a corpus-wide "what bytes
/// dominate ANM bytecode?" sweep.
pub fn pack_bytecode_histogram(payload: &[u8], pack: &AnmPack) -> [u32; 256] {
    let mut hist = [0u32; 256];
    for rec in &pack.records {
        let bytes = record_bytes(payload, rec);
        if bytes.len() > RECORD_HEADER_SIZE {
            for &b in &bytes[RECORD_HEADER_SIZE..] {
                hist[b as usize] += 1;
            }
        }
    }
    hist
}

/// The top-K most frequent bytes in a histogram, returned as
/// `(byte, count)` pairs sorted by descending count. Ties resolved by
/// ascending byte value.
pub fn top_k(hist: &[u32; 256], k: usize) -> Vec<(u8, u32)> {
    let mut pairs: Vec<(u8, u32)> = (0..256u16)
        .map(|b| (b as u8, hist[b as usize]))
        .filter(|&(_, c)| c > 0)
        .collect();
    pairs.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    pairs.truncate(k);
    pairs
}

/// Record-bytecode bigram (`(byte_n, byte_{n+1})`) histogram. Useful for
/// inferring the **shape** of the per-record opcode stream without the
/// dispatcher (which is overlay-resident).
///
/// If the bytecode is structured as `[op, operand, op, operand, ...]`
/// pairs, the bigram surface concentrates on a handful of `(op, *)`
/// rows. If it's variable-length VLQ-style, the bigram is more diffuse.
/// Either way, surfacing the top bigrams gives a reverse-engineer a
/// strong on-ramp before they capture the dispatcher overlay.
///
/// Returns counts indexed by `[byte_n][byte_{n+1}]`.
pub fn record_bytecode_bigram(record: &[u8]) -> Box<[[u32; 256]; 256]> {
    let mut hist = Box::new([[0u32; 256]; 256]);
    if record.len() <= RECORD_HEADER_SIZE + 1 {
        return hist;
    }
    let body = &record[RECORD_HEADER_SIZE..];
    for w in body.windows(2) {
        hist[w[0] as usize][w[1] as usize] += 1;
    }
    hist
}

/// Same as [`record_bytecode_bigram`] but walks every record in `pack`,
/// accumulating into one bigram histogram. Returns the most frequent K
/// bigrams as `(byte_n, byte_{n+1}, count)` triples.
pub fn pack_bytecode_top_bigrams(payload: &[u8], pack: &AnmPack, k: usize) -> Vec<(u8, u8, u32)> {
    let mut hist = Box::new([[0u32; 256]; 256]);
    for rec in &pack.records {
        let bytes = record_bytes(payload, rec);
        if bytes.len() > RECORD_HEADER_SIZE + 1 {
            let body = &bytes[RECORD_HEADER_SIZE..];
            for w in body.windows(2) {
                hist[w[0] as usize][w[1] as usize] += 1;
            }
        }
    }
    let mut triples: Vec<(u8, u8, u32)> = Vec::new();
    for a in 0..256u16 {
        for b in 0..256u16 {
            let c = hist[a as usize][b as usize];
            if c > 0 {
                triples.push((a as u8, b as u8, c));
            }
        }
    }
    triples.sort_by(|x, y| y.2.cmp(&x.2).then(x.0.cmp(&y.0)).then(x.1.cmp(&y.1)));
    triples.truncate(k);
    triples
}

#[cfg(test)]
mod tests;
