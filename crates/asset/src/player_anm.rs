//! Player-character animation bundle - the per-scene ANM blob the retail
//! engine allocates at `DAT_8007B7C8` (see [Ghidra dump of `FUN_8001F05C`
//! case 6](../../../ghidra/scripts/funcs/8001f05c.txt)).
//!
//! REF: FUN_8001B964 - the runtime per-actor animated-mesh renderer that
//! consumes this layout. The clean-room engine reproduces it via wgpu plus
//! the ported [`BoneTransform::decode`] (`FUN_8001BE80`), so the GTE/OT draw
//! loop of `FUN_8001B964` itself is not ported; this file owns the data side.
//!
//! Each town scene's first PROT slot ships a multi-section
//! [`parse_player_lzs`]-shaped container; one of
//! the sections (typically section 2) is tagged with asset type byte
//! [`SCENE_ANM_TYPE_BYTE`] (= `0x05`, [`crate::AssetType::Move`]). Despite
//! the "MOVE" label in the dispatcher table, the content is a canonical
//! [`legaia_anm`]-shaped container with `marker_1 = 0x080C` records. The
//! dispatcher's `case 6` (which allocates `DAT_8007B7C8` via the
//! `anm_malloc_err` string) actually routes type-`0x05` data, not
//! type-`0x06` - a quirk between the runtime case index and the asset-type
//! byte.
//!
//! Confirmed corpus (byte-equality against live `DAT_8007B7C8` in the
//! [`v0_1_pre_battle_tetsu`](../../scripts/scenarios.toml) field-mode save):
//!
//! | PROT entry | CDNAME | Section | Records | Decoded bytes |
//! |---|---|---|---|---|
//! | `0004` | town01 | 2 | 69 | 96 448 |
//! | `0013` | town0b | 2 | 69 | 91 784 |
//! | `0183` | balden | 2 | 72 | 71 604 |
//! | `0408` | bubu1  | 2 | 70 | 87 844 |
//! | `1203` | other5 | 2 | 30 | 87 684 (battle variant) |
//!
//! The detector at [`find_in_entry`] walks the player-LZS container shape
//! and reports every type-[`SCENE_ANM_TYPE_BYTE`] section that decompresses
//! cleanly into a valid ANM container.
//!
//! ## Disc-vs-runtime layout
//!
//! Disc form (LZS-decompressed):
//!
//! ```text
//! +0x00  u32  count
//! +0x04  u32  byte_offsets[count]   ; ABSOLUTE byte offsets into the buffer
//!        ...  records start at byte_offsets[i]
//! ```
//!
//! The offsets are absolute (i.e. an offset of `0x118` means record 0 starts
//! at byte `0x118` of the buffer, exactly at the end of the offset table for
//! a 69-record bundle). This matches the existing [`legaia_anm::parse`]
//! convention.
//!
//! Each record's first 8 bytes are the canonical
//! [`legaia_anm::RecordHeader`] (`a`, `b`, `marker_1`, `flag`). Across the
//! corpus, `marker_1` is `0x080C` for every record, and the record-body
//! size obeys:
//!
//! ```text
//!     record_size = 16 + 8 * (a & 0xFF) * b
//! ```
//!
//! where `a & 0xFF` is the **bone count** (number of animated objects in the
//! TMD) and `b` is the **frame count**. The body sits **immediately after
//! the 8-byte header**, with an 8-byte zero-padding trailer at the end of
//! the record (so the record always rounds to a 16-byte boundary). The
//! runtime layout, traced through `FUN_8001B964` (per-actor animated
//! renderer) → `FUN_8001BE80` (per-(bone, frame) decoder):
//!
//! ```text
//! +0x00..+0x08    header (a, b, marker_1, flag)
//! +0x08..+end-8  frame_count frames; per frame:
//!                    bone_count × 8 bytes
//!                  each 8-byte entry is one bone's TR for that frame
//! +end-8..+end    8 zero bytes (record-boundary padding)
//! ```
//!
//! The first frame (`+0x08..+0x08 + bone_count*8`) holds the **rest-pose**
//! assembly transform - frame 0 bone 0 has all-zero bytes only when the
//! root joint sits at origin. The runtime pointer `actor[+0x4C]` points
//! at the record's byte 0, and `*pbVar6 == nobj` is a sanity check
//! (header byte 0 = `a & 0xFF` = bone count = the TMD's nobj).
//!
//! ### Per-(bone, frame) 8-byte encoding
//!
//! Decoded by `FUN_8001BE80` (signed 12-bit unpacking + PsyQ rotation
//! composition via `FUN_8004638C` / `FUN_8004629C` / `FUN_800461A4`):
//!
//! ```text
//!   byte 0   = low8(T0)
//!   byte 1   = low8(T1)
//!   byte 2   = (high4(T1) << 4) | high4(T0)
//!   byte 3   = low8(T2)
//!   byte 4   = ???                | high4(T2)   ; high nibble unused
//!   byte 5   = u8 rot-X (left-shifted by 4 to make a 12-bit PSX angle)
//!   byte 6   = u8 rot-Y
//!   byte 7   = u8 rot-Z
//! ```
//!
//! `T0..T2` are signed 12-bit translation values (sign-extended to i32 via
//! `if (v & 0x800) v |= 0xFFFFF000`). The translation is pushed through the
//! GTE's MVMVA with the actor's rotation matrix - i.e., it's the joint's
//! offset in actor-local space, transformed to world space before being
//! loaded into the GTE TR registers. The three rotation angles compose
//! into the GTE rotation matrix in the order Z, Y, X (post-multiplication),
//! producing the per-bone orientation that `FUN_8002735C` then renders the
//! object with.
//!
//! Runtime form (what `FUN_8001F05C` case 6 allocates at `DAT_8007B7C8`):
//!
//! ```text
//! +0x00  u32  zero
//! +0x04  u32  self_ptr - 0x0C
//! +0x08  u32  end_ptr
//! +0x0C  u32  total_size
//! +0x10  -- disc-form starts here, byte-for-byte --
//! ```
//!
//! [`parse`] returns the disc form (no preamble).

use anyhow::{Result, bail};
use serde::Serialize;

use crate::{DecodeMode, decode, parse_player_lzs};

/// Asset type byte the per-scene player ANM bundle ships under. Counter-
/// intuitively this is the [`crate::AssetType::Move`] tag (`0x05`), not
/// [`crate::AssetType::Anm`] (`0x06`) - the dispatcher's `case 6` (which
/// allocates `DAT_8007B7C8`) actually routes type-0x05 data, not type-0x06.
pub const SCENE_ANM_TYPE_BYTE: u8 = 0x05;

/// ANM record `marker_1` - the first halfword of every record header
/// (at byte `+4` of the record, per [`legaia_anm::RecordHeader`]).
pub const ANM_MARKER_1: u16 = 0x080C;

/// Header size in bytes (a, b, marker_1, flag - matches
/// [`legaia_anm::RECORD_HEADER_SIZE`]).
pub const RECORD_HEADER_SIZE: usize = 8;

/// Size of the trailing zero-padding block after the body. Empirically
/// `8` across every record in the corpus, padding the record to a 16-byte
/// boundary.
pub const RECORD_TRAILER_SIZE: usize = 8;

/// Bytes per (bone, frame) entry. Empirically `8` across every record in
/// the corpus; size formula `16 + 8 * (a & 0xFF) * b` falls out of this
/// (8-byte header + body + 8-byte trailer).
pub const BONE_FRAME_BYTES: usize = 8;

/// One decoded player-ANM record (one animation clip).
#[derive(Debug, Clone, Serialize)]
pub struct PlayerAnmRecord {
    /// Header `a` field - the **bone count** lives in `a & 0xFF`. The high
    /// byte appears to be a sub-format flag (set to `0x01` for records 9+
    /// in the field-form bundle and for every record in the battle-form
    /// bundle).
    pub a: u16,
    /// Header `b` field - **frame count** of this animation clip.
    pub b: u16,
    /// Canonical marker (`0x080C` for every record in the corpus).
    pub marker_1: u16,
    /// Sub-format selector (`0x02` / `0x04` in the field corpus, plus
    /// `0x0201` / `0x0401` / `0x0402` in the battle-form bundle).
    pub flag: u16,
    /// Computed bone count = `a & 0xFF`.
    pub bone_count: u16,
    /// Computed frame count = `b`.
    pub frame_count: u16,
}

/// A single decoded player-ANM bundle (one type-0x05 section's worth of
/// records).
#[derive(Debug, Clone, Serialize)]
pub struct PlayerAnmBundle {
    /// `count` from the container header.
    pub record_count: u32,
    /// Absolute byte offsets of each record's start (one entry per record).
    pub record_offsets: Vec<u32>,
    /// Per-record sizes in bytes (`record_offsets[i+1] - record_offsets[i]`,
    /// or `decoded.len() - record_offsets[last]` for the final record).
    pub record_sizes: Vec<u32>,
    /// LZS-decoded bytes of the whole bundle (container header + offset
    /// table + records).
    pub decoded: Vec<u8>,
}

impl PlayerAnmBundle {
    /// Byte slice of record `index` (header + prologue + frames). Empty if
    /// `index` is past `record_count`.
    pub fn record_bytes(&self, index: usize) -> &[u8] {
        if index >= self.record_offsets.len() {
            return &[];
        }
        let start = self.record_offsets[index] as usize;
        let size = self.record_sizes[index] as usize;
        let end = start + size;
        if end > self.decoded.len() {
            return &[];
        }
        &self.decoded[start..end]
    }

    /// Read the `marker_1` halfword (at byte +4 of the record's header).
    pub fn record_marker_1(&self, index: usize) -> Option<u16> {
        let r = self.record_bytes(index);
        if r.len() < RECORD_HEADER_SIZE {
            return None;
        }
        Some(u16::from_le_bytes([r[4], r[5]]))
    }

    /// Decode record `index`'s header + sizes. Errors if the record's
    /// size doesn't satisfy the `16 + 8 * (a & 0xFF) * b` invariant
    /// (8-byte header + body + 8-byte trailer).
    pub fn record(&self, index: usize) -> Result<PlayerAnmRecord> {
        let r = self.record_bytes(index);
        if r.len() < RECORD_HEADER_SIZE + RECORD_TRAILER_SIZE {
            bail!(
                "record {index} too small for header + trailer ({} < {})",
                r.len(),
                RECORD_HEADER_SIZE + RECORD_TRAILER_SIZE
            );
        }
        let a = u16::from_le_bytes([r[0], r[1]]);
        let b = u16::from_le_bytes([r[2], r[3]]);
        let marker_1 = u16::from_le_bytes([r[4], r[5]]);
        let flag = u16::from_le_bytes([r[6], r[7]]);
        let bone_count = a & 0xFF;
        let frame_count = b;
        let expected = RECORD_HEADER_SIZE
            + (bone_count as usize) * (frame_count as usize) * BONE_FRAME_BYTES
            + RECORD_TRAILER_SIZE;
        if r.len() != expected {
            bail!(
                "record {index} size mismatch: a=0x{a:04X} (bone_count={bone_count}), b={frame_count}, \
                 expected size = 8 + 8 * {bone_count} * {frame_count} + 8 = {expected}, \
                 actual = {}",
                r.len()
            );
        }
        Ok(PlayerAnmRecord {
            a,
            b,
            marker_1,
            flag,
            bone_count,
            frame_count,
        })
    }

    /// Like [`Self::record`] but tolerating records whose payload runs
    /// **longer** than the `16 + 8 * bones * frames` invariant. The dance-hall
    /// scene bundle (PROT 1229, the `other7` MOVE section) carries several
    /// choreography records with extra frame data past the header's count -
    /// the retail clip driver clamps its cursor at `frame_count * 16 - 1`
    /// (`FUN_800204F8`), so only the header's frames ever play and the tail is
    /// unread. Still errors on records **shorter** than the invariant (frames
    /// past the buffer would be garbage).
    pub fn record_lenient(&self, index: usize) -> Result<PlayerAnmRecord> {
        match self.record(index) {
            Ok(r) => Ok(r),
            Err(e) => {
                let r = self.record_bytes(index);
                if r.len() < RECORD_HEADER_SIZE + RECORD_TRAILER_SIZE {
                    return Err(e);
                }
                let a = u16::from_le_bytes([r[0], r[1]]);
                let b = u16::from_le_bytes([r[2], r[3]]);
                let bone_count = a & 0xFF;
                let expected = RECORD_HEADER_SIZE
                    + (bone_count as usize) * (b as usize) * BONE_FRAME_BYTES
                    + RECORD_TRAILER_SIZE;
                if r.len() < expected {
                    return Err(e);
                }
                Ok(PlayerAnmRecord {
                    a,
                    b,
                    marker_1: u16::from_le_bytes([r[4], r[5]]),
                    flag: u16::from_le_bytes([r[6], r[7]]),
                    bone_count,
                    frame_count: b,
                })
            }
        }
    }

    /// Borrow the per-frame slice (`bone_count * 8` bytes) for one frame.
    /// Returns `&[]` on out-of-range record or frame. The body sits
    /// immediately after the 8-byte header, frame-major.
    pub fn frame_bytes(&self, record_index: usize, frame_index: usize) -> &[u8] {
        let r = self.record_bytes(record_index);
        if r.len() < RECORD_HEADER_SIZE + RECORD_TRAILER_SIZE {
            return &[];
        }
        let bone_count = (u16::from_le_bytes([r[0], r[1]]) & 0xFF) as usize;
        let frame_count = u16::from_le_bytes([r[2], r[3]]) as usize;
        if frame_index >= frame_count {
            return &[];
        }
        let frame_bytes = bone_count * BONE_FRAME_BYTES;
        let off = RECORD_HEADER_SIZE + frame_index * frame_bytes;
        if off + frame_bytes > r.len() {
            return &[];
        }
        &r[off..off + frame_bytes]
    }

    /// Borrow the 8-byte (one bone, one frame) entry from record
    /// `record_index`, frame `frame_index`, bone `bone_index`. Returns `&[]`
    /// if any index is out of range.
    pub fn bone_frame_bytes(
        &self,
        record_index: usize,
        frame_index: usize,
        bone_index: usize,
    ) -> &[u8] {
        let f = self.frame_bytes(record_index, frame_index);
        if f.is_empty() {
            return &[];
        }
        let off = bone_index * BONE_FRAME_BYTES;
        if off + BONE_FRAME_BYTES > f.len() {
            return &[];
        }
        &f[off..off + BONE_FRAME_BYTES]
    }

    /// Decode one (bone, frame) 8-byte entry into the (T, R) transform the
    /// retail engine pushes through the GTE - see `FUN_8001BE80`.
    /// Returns `None` if any index is out of range.
    pub fn bone_transform(
        &self,
        record_index: usize,
        frame_index: usize,
        bone_index: usize,
    ) -> Option<BoneTransform> {
        let bf = self.bone_frame_bytes(record_index, frame_index, bone_index);
        if bf.len() != BONE_FRAME_BYTES {
            return None;
        }
        Some(BoneTransform::decode(bf))
    }
}

/// Per-(bone, frame) transform decoded from one 8-byte entry. `t*` are the
/// joint's translation in actor-local space (signed 12-bit, sign-extended
/// to i32). `r_x/y/z` are PSX rotation units (0..4096 = 0..360°), composed
/// in the order Z, Y, X via post-multiplication of the actor's rotation
/// matrix (see `FUN_8001B964`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct BoneTransform {
    pub t_x: i32,
    pub t_y: i32,
    pub t_z: i32,
    pub r_x: i32,
    pub r_y: i32,
    pub r_z: i32,
}

impl BoneTransform {
    /// Decode the 8-byte (bone, frame) entry as the retail engine does.
    /// Panics if `bytes.len() < 8`.
    // PORT: FUN_8001BE80 - the per-(bone, frame) entry decoder: unpacks the
    // three nibble-packed signed 12-bit translations (sign-extend on bit 0x800)
    // and the three u8 rotation angles (each << 4 into a 12-bit PSX angle).
    //
    // `t_z`'s high nibble is the LOW nibble of byte 4 (`andi v0,v0,0xf` at
    // 0x8001BF38); byte 4's high nibble is never read. Prose elsewhere has
    // said "high nibble" - the instructions say otherwise, and
    // `bone_transform_decode_signed_12bit` below is what keeps this honest.
    //
    // Scope limit: retail `FUN_8001BE80` also blends two frames using the
    // 4-bit sub-frame fraction at `actor+0x68` (gated on `*(a2+1) & 1`,
    // angles via `FUN_8001D088`). This decodes a single entry and does no
    // interpolation - see `docs/formats/anm.md`.
    pub fn decode(bytes: &[u8]) -> Self {
        let unpack = |lo: u8, hi4: u8| -> i32 {
            let mut v = (lo as u32) | (((hi4 & 0x0F) as u32) << 8);
            if v & 0x800 != 0 {
                v |= 0xFFFF_F000;
            }
            v as i32
        };
        let t_x = unpack(bytes[0], bytes[2] & 0x0F);
        let t_y = unpack(bytes[1], bytes[2] >> 4);
        let t_z = unpack(bytes[3], bytes[4] & 0x0F);
        let r_x = (bytes[5] as i32) << 4;
        let r_y = (bytes[6] as i32) << 4;
        let r_z = (bytes[7] as i32) << 4;
        BoneTransform {
            t_x,
            t_y,
            t_z,
            r_x,
            r_y,
            r_z,
        }
    }
}

/// One full turn in the retail 12-bit angle space (`4096 = 360 deg`).
pub const ANGLE_TURN: i32 = 0x1000;

/// Wraparound-aware interpolation between two 12-bit PSX angles.
///
/// PORT: FUN_8001D088
///
/// NOT WIRED: no engine animation sampler carries a sub-frame fraction, so
/// nothing has a `frac` to pass. Every consumer of this bundle - the field
/// character renderer, the battle pose builder, the web viewer's players -
/// calls [`PlayerAnmBundle::bone_transform`] at a whole frame index and poses
/// the mesh from that decode alone. The prerequisite is the `actor+0x68`
/// blend fraction on the engine's animation state plus a two-frame sampler to
/// feed it; adding one changes every posed frame, so it is an animation-path
/// change measured against the pose oracles rather than a call insertion.
///
/// `frac` is the 4-bit sub-frame fraction the frame blender carries at
/// `actor+0x68` (`0..=15`); the result is
/// `(to + ((from - to) * frac >> 4)) & 0xFFF`, so `frac == 0` yields `to`
/// and `frac == 16` would yield `from`. Both inputs are masked to 12 bits
/// first.
///
/// The part that is not a plain lerp is the **unwrap**, and it is why the
/// bone blender cannot use the translation lerp for angles. Retail brings
/// the pair onto the shortest arc before subtracting, in two guarded steps
/// (`0x8001D090..0x8001D0B8`):
///
/// - if `from - to >= 0x800`, add a full turn to `to`;
/// - then, if `to - from >= 0x800`, add a full turn to `from`.
///
/// Both comparisons are signed `slti` against half a turn, so after the pair
/// the signed difference `from - to` always lies in `(-0x800, 0x800]` - the
/// short way round. Interpolating 0x010 towards 0xFF0 therefore crosses zero
/// (16 units) instead of running 4064 units the long way.
///
/// The two guards are **sequential, not exclusive**: the second re-reads the
/// `to` the first may have just bumped (`subu v0,a1,a0` at `0x8001D0A4`). At
/// exactly half a turn both fire and cancel, leaving the delta at `+0x800`,
/// so that one input resolves forward rather than backward.
///
/// Two side effects of the retail routine are deliberately not modelled: it
/// accumulates `|from - to|` into the counter `_DAT_8007BD28` (the two
/// branches at `0x8001D0DC` add the same magnitude, since `|a-b| == |b-a|`,
/// so the branch is a wash), and it journals the unwrapped `(from, to)` pair
/// into the 8-byte-stride slot table at `0x800891A8`. Both are
/// engine-external bookkeeping over globals this crate does not host.
pub fn lerp_angle_12(from: i32, to: i32, frac: i32) -> u16 {
    let mut from = from & 0xFFF;
    let mut to = to & 0xFFF;
    if from - to >= ANGLE_TURN / 2 {
        to += ANGLE_TURN;
    }
    if to - from >= ANGLE_TURN / 2 {
        from += ANGLE_TURN;
    }
    (((((from - to) * frac) >> 4) + to) as u16) & 0x0FFF
}

/// Find every player-ANM-shaped section in a single PROT entry.
///
/// Walks `bytes` as a [`parse_player_lzs`]-shaped container with the given
/// `descriptor_count` (most scene bundles use 3, 5, 6, or 7). For each
/// type-[`SCENE_ANM_TYPE_BYTE`] descriptor, LZS-decode the section and
/// validate it parses as a canonical ANM container.
pub fn find_in_entry(bytes: &[u8], descriptor_count: usize) -> Vec<PlayerAnmBundle> {
    let Ok(container) = parse_player_lzs(bytes, descriptor_count) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for d in &container.descriptors {
        if d.type_byte != SCENE_ANM_TYPE_BYTE {
            continue;
        }
        let Ok(decoded) = decode(bytes, d, DecodeMode::Lzs) else {
            continue;
        };
        let Ok(bundle) = parse(&decoded) else {
            continue;
        };
        out.push(bundle);
    }
    out
}

/// Parse one fully-decoded player-ANM bundle (the LZS-decompressed bytes of
/// a type-0x05 section). Returns `Err` if the container header / offset
/// table / first record's marker_1 don't validate.
pub fn parse(decoded: &[u8]) -> Result<PlayerAnmBundle> {
    if decoded.len() < 8 {
        bail!("buffer too small for player-ANM container header");
    }
    let count = u32::from_le_bytes(decoded[..4].try_into().unwrap());
    if count == 0 || count > 256 {
        bail!("implausible record count {count}");
    }
    let count_us = count as usize;
    let table_end = 4 + count_us * 4;
    if table_end > decoded.len() {
        bail!(
            "offset table ({count} entries, ends at {table_end}) overruns buffer ({} bytes)",
            decoded.len()
        );
    }
    let mut offsets: Vec<u32> = Vec::with_capacity(count_us);
    for i in 0..count_us {
        let off = u32::from_le_bytes(decoded[4 + i * 4..8 + i * 4].try_into().unwrap());
        offsets.push(off);
    }
    // Offsets are ABSOLUTE byte offsets into `decoded`. They must be
    // monotonically non-decreasing and >= table_end (i.e. point past the
    // offset table).
    let mut prev = 0u32;
    for (i, off) in offsets.iter().enumerate() {
        let abs = *off as usize;
        if abs < table_end {
            bail!(
                "offset[{i}] 0x{off:X} points into the offset table (table ends at 0x{table_end:X})"
            );
        }
        if abs >= decoded.len() {
            bail!(
                "offset[{i}] 0x{off:X} overruns buffer ({} bytes)",
                decoded.len()
            );
        }
        if i > 0 && *off < prev {
            bail!("offsets not monotonic: offset[{i}] = 0x{off:X} < prev 0x{prev:X}");
        }
        prev = *off;
    }
    // Per-record sizes from consecutive offsets.
    let mut sizes: Vec<u32> = Vec::with_capacity(count_us);
    for i in 0..count_us {
        let end = if i + 1 < count_us {
            offsets[i + 1]
        } else {
            decoded.len() as u32
        };
        sizes.push(end - offsets[i]);
    }
    // First record's marker_1 (at byte +4 of the record) must be 0x080C.
    let r0_start = offsets[0] as usize;
    if r0_start + RECORD_HEADER_SIZE > decoded.len() {
        bail!("first record overruns buffer");
    }
    let m = u16::from_le_bytes([decoded[r0_start + 4], decoded[r0_start + 5]]);
    if m != ANM_MARKER_1 {
        bail!(
            "first record marker_1 mismatch: expected 0x{:04X}, got 0x{m:04X}",
            ANM_MARKER_1
        );
    }
    Ok(PlayerAnmBundle {
        record_count: count,
        record_offsets: offsets,
        record_sizes: sizes,
        decoded: decoded.to_vec(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a synthetic bundle that mirrors the real disc layout:
    /// absolute offsets, marker at byte +4 of each record, and
    /// `size = 8 + 8 * bone_count * frame_count + 8` (header + body +
    /// 8-byte zero trailer).
    fn synthetic_two_records() -> Vec<u8> {
        // Two records: (a=2 bones, b=3 frames, size = 8 + 8*2*3 + 8 = 64) twice.
        let count: u32 = 2;
        let table_end = 4 + 4 * count as usize;
        let rec_size = 64;
        let mut buf = Vec::new();
        buf.extend_from_slice(&count.to_le_bytes());
        let off0 = table_end as u32;
        let off1 = off0 + rec_size;
        buf.extend_from_slice(&off0.to_le_bytes());
        buf.extend_from_slice(&off1.to_le_bytes());
        for r in 0..2u16 {
            // header: a=2, b=3, marker=0x080C, flag=0x0002
            buf.extend_from_slice(&2u16.to_le_bytes());
            buf.extend_from_slice(&3u16.to_le_bytes());
            buf.extend_from_slice(&ANM_MARKER_1.to_le_bytes());
            buf.extend_from_slice(&0x0002u16.to_le_bytes());
            // 3 frames × 2 bones × 8 bytes = 48 bytes, tag with record index
            for f in 0..3u8 {
                for bidx in 0..2u8 {
                    let v: u8 = (r as u8) * 100 + f * 10 + bidx;
                    buf.extend_from_slice(&[v, 0, v + 1, 0, v + 2, 0, v + 3, 0]);
                }
            }
            // 8-byte trailer (zero in real records)
            buf.extend_from_slice(&[0u8; 8]);
        }
        buf
    }

    #[test]
    fn parses_synthetic_two_records() {
        let buf = synthetic_two_records();
        let bundle = parse(&buf).expect("synthetic should parse");
        assert_eq!(bundle.record_count, 2);
        assert_eq!(bundle.record_offsets.len(), 2);
        assert_eq!(bundle.record_sizes, vec![64, 64]);
        assert_eq!(bundle.record_marker_1(0), Some(ANM_MARKER_1));
        assert_eq!(bundle.record_marker_1(1), Some(ANM_MARKER_1));
    }

    #[test]
    fn record_decodes_header_and_sizes() {
        let buf = synthetic_two_records();
        let bundle = parse(&buf).unwrap();
        let r0 = bundle.record(0).unwrap();
        assert_eq!(r0.a, 2);
        assert_eq!(r0.b, 3);
        assert_eq!(r0.marker_1, ANM_MARKER_1);
        assert_eq!(r0.flag, 0x0002);
        assert_eq!(r0.bone_count, 2);
        assert_eq!(r0.frame_count, 3);
    }

    #[test]
    fn frame_and_bone_indexing() {
        let buf = synthetic_two_records();
        let bundle = parse(&buf).unwrap();
        // Record 0, frame 0, bone 0: tagged with (0*100 + 0*10 + 0) = 0
        let bf = bundle.bone_frame_bytes(0, 0, 0);
        assert_eq!(bf.len(), 8);
        assert_eq!(bf[0], 0);
        // Record 0, frame 1, bone 0: tagged with (0*100 + 1*10 + 0) = 10
        let bf = bundle.bone_frame_bytes(0, 1, 0);
        assert_eq!(bf.len(), 8);
        assert_eq!(bf[0], 10);
        // Record 1, frame 2, bone 1: tagged with (1*100 + 2*10 + 1) = 121
        let bf = bundle.bone_frame_bytes(1, 2, 1);
        assert_eq!(bf.len(), 8);
        assert_eq!(bf[0], 121);
        // Out-of-range frame returns empty
        assert!(bundle.bone_frame_bytes(0, 99, 0).is_empty());
        // Out-of-range bone returns empty
        assert!(bundle.bone_frame_bytes(0, 0, 99).is_empty());
    }

    #[test]
    fn bone_transform_decode_signed_12bit() {
        // Reproduce a known-good runtime decode: bytes from town01 record 17,
        // frame 0 bone 2: `E6 AB FF FE 0F C0 FD 43` should decode to
        // T=(-26, -85, -2) and R=(0xC00, 0xFD0, 0x430).
        let bf = [0xE6, 0xAB, 0xFF, 0xFE, 0x0F, 0xC0, 0xFD, 0x43];
        let t = BoneTransform::decode(&bf);
        assert_eq!(t.t_x, -26);
        assert_eq!(t.t_y, -85);
        assert_eq!(t.t_z, -2);
        assert_eq!(t.r_x, 0xC00);
        assert_eq!(t.r_y, 0xFD0);
        assert_eq!(t.r_z, 0x430);
    }

    #[test]
    fn rejects_wrong_marker() {
        // Build a buffer where marker at +4 is wrong
        let mut buf = Vec::new();
        buf.extend_from_slice(&1u32.to_le_bytes()); // count
        buf.extend_from_slice(&8u32.to_le_bytes()); // offset (absolute)
        buf.extend_from_slice(&[0xAA, 0xBB, 0xCC, 0xDD, 0xAA, 0xBB]); // wrong marker at +4
        buf.extend_from_slice(&[0u8; 64]);
        assert!(parse(&buf).is_err());
    }

    #[test]
    fn rejects_offset_in_table() {
        let mut buf = Vec::new();
        buf.extend_from_slice(&1u32.to_le_bytes()); // count
        buf.extend_from_slice(&4u32.to_le_bytes()); // offset points into table (< 8)
        buf.extend_from_slice(&[0u8; 64]);
        assert!(parse(&buf).is_err());
    }

    #[test]
    fn record_with_bad_size_errors() {
        // Build a single record that claims a=2, b=3 (expected size 64) but
        // actually leaves only 32 bytes.
        let mut buf = Vec::new();
        buf.extend_from_slice(&1u32.to_le_bytes()); // count
        buf.extend_from_slice(&8u32.to_le_bytes()); // offset = 8 (absolute, table is 4..8)
        // header
        buf.extend_from_slice(&2u16.to_le_bytes());
        buf.extend_from_slice(&3u16.to_le_bytes());
        buf.extend_from_slice(&ANM_MARKER_1.to_le_bytes());
        buf.extend_from_slice(&0x0002u16.to_le_bytes());
        // 16 bytes of body (vs expected 8 + 8*2*3 + 8 = 64 trailing bytes)
        buf.extend_from_slice(&[0u8; 16]);
        let bundle = parse(&buf).unwrap();
        assert!(bundle.record(0).is_err());
    }

    #[test]
    fn lerp_angle_12_endpoints_are_the_two_frames() {
        // frac 0 is the `to` frame verbatim; the mask is applied to both ends.
        assert_eq!(lerp_angle_12(0x400, 0x100, 0), 0x100);
        assert_eq!(lerp_angle_12(0x1400, 0x100, 0), 0x100);
        // 16/16 of the way is `from`, but the caller only ever passes 0..=15,
        // so the last representable step stops one sixteenth short.
        assert_eq!(lerp_angle_12(0x400, 0x100, 15), 0x100 + ((0x300 * 15) >> 4));
    }

    #[test]
    fn lerp_angle_12_takes_the_short_arc_across_zero() {
        // 0xFF0 -> 0x010 is 32 units forward, not 4064 units backward.
        // Half-way must land on 0x000, and never in the 0x800 region.
        assert_eq!(lerp_angle_12(0x010, 0xFF0, 8), 0x000);
        assert_eq!(lerp_angle_12(0xFF0, 0x010, 8), 0x000);
        // Same arc, quarter of the way from each side.
        assert_eq!(lerp_angle_12(0x010, 0xFF0, 4), 0xFF8);
        assert_eq!(lerp_angle_12(0xFF0, 0x010, 4), 0x008);
    }

    #[test]
    fn lerp_angle_12_unwrap_is_symmetric_and_stays_in_range() {
        for from in (0..0x1000).step_by(0x37) {
            for to in (0..0x1000).step_by(0x53) {
                for frac in 0..16 {
                    let v = lerp_angle_12(from, to, frac);
                    assert!(v < 0x1000, "{from:#x} {to:#x} {frac} -> {v:#x}");
                }
                // The unwrap brings the pair onto the short arc, so the step
                // taken per frac tick never exceeds half a turn / 16.
                let a = lerp_angle_12(from, to, 0) as i32;
                let b = lerp_angle_12(from, to, 1) as i32;
                let step = (a - b).rem_euclid(0x1000).min((b - a).rem_euclid(0x1000));
                assert!(step <= 0x800 / 16 + 1, "{from:#x} {to:#x} step {step:#x}");
            }
        }
    }

    #[test]
    fn lerp_angle_12_half_turn_fires_both_guards() {
        // Exactly half a turn is the one input where BOTH guards fire, and
        // they cancel: `from - to == 0x800` bumps `to` to 0x1000, which makes
        // `to - from == 0x800` and bumps `from` to 0x1800, leaving the delta
        // back at +0x800. The walk therefore runs FORWARD, and half-way lands
        // on 0x400 rather than 0xC00. The second guard is re-evaluated on the
        // updated `to` (`subu v0,a1,a0` at 0x8001D0A4), which is what makes
        // this fall out; modelling the two as exclusive gets it backwards.
        assert_eq!(lerp_angle_12(0x800, 0x000, 8), 0x400);
    }
}
