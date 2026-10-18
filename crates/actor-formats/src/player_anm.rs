//! Player-character animation bundle - the per-scene ANM blob the retail
//! engine allocates at `DAT_8007B7C8` (see [Ghidra dump of `FUN_8001F05C`
//! case 6](../../../ghidra/scripts/funcs/8001f05c.txt)).
//!
//! REF: FUN_8001B964 - the runtime per-actor animated-mesh renderer that
//! consumes this layout. The engine reproduces it via wgpu plus
//! the ported [`BoneTransform::decode`] (`FUN_8001BE80`), so the GTE/OT draw
//! loop of `FUN_8001B964` itself is not ported; this file owns the data side.
//!
//! Each town scene's first PROT slot ships a multi-section
//! [`parse_player_lzs`]-shaped container; one of
//! the sections (typically section 2) is tagged with asset type byte
//! [`SCENE_ANM_TYPE_BYTE`] (= `0x05`, [`legaia_container_formats::AssetType::Move`]). Despite
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

use anyhow::{Context, Result, bail};
use serde::Serialize;

use legaia_container_formats::{DecodeMode, decode, parse_player_lzs};

/// Asset type byte the per-scene player ANM bundle ships under. Counter-
/// intuitively this is the [`legaia_container_formats::AssetType::Move`] tag (`0x05`), not
/// [`legaia_container_formats::AssetType::Anm`] (`0x06`) - the dispatcher's `case 6` (which
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

impl PlayerAnmRecord {
    /// The clip's sub-frame **blend gate**: bit 0 of `a`'s high byte (clip
    /// byte `+1`). The frame blender `FUN_8001BE80` interpolates toward the
    /// next entry only when it is set (`lbu v0,0x1(a2); andi v0,v0,0x1` at
    /// `0x8001BF70` and again at `0x8001C0EC`, `a2` = the bound clip
    /// `actor+0x4C`). It is the same bit the clip tick `FUN_800204F8` reads
    /// to select its scaled step (`lbu v0,0x1(a1)` at `0x800205B4`, `a1` =
    /// `actor+0x4C` again) - one header bit gates both.
    pub fn blends(&self) -> bool {
        (self.a >> 8) & 1 != 0
    }
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

    /// The transform retail's frame blender poses bone `bone_index` of record
    /// `record_index` with, for the frame cursor `cursor` (`actor+0x68`, in
    /// 1/16-frame units).
    ///
    /// PORT: FUN_8001BE80 (the two-frame sampler: the next-entry pick at
    /// `0x8001BEAC..0x8001BF00` plus the blend)
    ///
    /// - The current frame is `(i16)cursor >> 4` (`sll 0x10; sra 0x14`).
    /// - The **next** entry is the same part one frame later while the frame
    ///   is below `frame_count - 1` (`entry + a2[0]*8`). On the last frame it
    ///   is the entry itself when `hold_at_end` - the actor's clamp bit
    ///   `+0x62 & 8` - is set, else the part's frame-0 entry
    ///   (`*(actor+0x4C) + part*8 + 8`), the loop wrap.
    /// - When the record's [`PlayerAnmRecord::blends`] gate is clear the
    ///   current entry is returned as decoded; when set, the two entries go
    ///   through [`blend_bone_transform`] with `frac = cursor & 0xF`.
    ///
    /// Retail reads out of range for a cursor past the clip; the clip tick
    /// never leaves one there, and this clamps the frame into the clip instead.
    /// `None` when the record or bone does not resolve.
    pub fn sample_bone(
        &self,
        record_index: usize,
        cursor: i16,
        hold_at_end: bool,
        bone_index: usize,
    ) -> Option<BoneTransform> {
        let rec = self.record(record_index).ok()?;
        let frames = rec.frame_count as usize;
        if frames == 0 {
            return None;
        }
        let frame = ((cursor >> 4).max(0) as usize).min(frames - 1);
        let cur = self.bone_transform(record_index, frame, bone_index)?;
        let frac = (cursor as u16 & 0xF) as i32;
        if !rec.blends() || frac == 0 {
            return Some(cur);
        }
        let next_frame = if frame < frames - 1 {
            frame + 1
        } else if hold_at_end {
            frame
        } else {
            0
        };
        let next = self.bone_transform(record_index, next_frame, bone_index)?;
        Some(blend_bone_transform(cur, next, frac))
    }

    /// Every bone of record `record_index` through [`Self::sample_bone`] - the
    /// pose `FUN_8001B964`'s per-part loop builds for one draw. `None` when
    /// the record does not resolve or any bone fails to.
    pub fn sample_pose(
        &self,
        record_index: usize,
        cursor: i16,
        hold_at_end: bool,
    ) -> Option<Vec<BoneTransform>> {
        let rec = self.record(record_index).ok()?;
        (0..rec.bone_count as usize)
            .map(|b| self.sample_bone(record_index, cursor, hold_at_end, b))
            .collect()
    }

    /// Adapt record `index` into the battle-side clip type
    /// ([`crate::monster_archive::MonsterAnimation`]) so consumers built on
    /// that shape - the glTF baker `crate::character_gltf::build_character_glb`
    /// foremost - can carry field / bank clips too.
    ///
    /// Lossless by construction: a [`BoneTransform`]'s translations are
    /// sign-extended 12-bit values (`-2048..=2047`, exactly the
    /// [`crate::monster_archive::PartPose`] `i16` domain) and its rotations
    /// are `u8 << 4` 12-bit PSX angles (`0..=4080`, within the pose's
    /// unsigned 12-bit domain) - both index spaces mean the same thing in
    /// both types (actor-local T, `4096 = 360°` R, Z·Y·X composition).
    /// `action_id` is the record index (truncated to the byte the clip type
    /// carries); `rate` is set to `2` - the nominal retail rate byte, giving
    /// the `7.5 * rate = 15` fps timeline closest to the field animator's
    /// observed ~14 fps playback (the ANM container has no per-record rate
    /// byte of its own). `None` when the record fails its size invariant.
    pub fn record_to_monster_animation(
        &self,
        index: usize,
    ) -> Option<crate::monster_archive::MonsterAnimation> {
        let rec = self.record(index).ok()?;
        let part_count = rec.bone_count as usize;
        let frame_count = rec.frame_count as usize;
        if part_count == 0 || frame_count == 0 {
            return None;
        }
        let mut frames = Vec::with_capacity(frame_count);
        for f in 0..frame_count {
            let mut parts = Vec::with_capacity(part_count);
            for b in 0..part_count {
                let t = self.bone_transform(index, f, b)?;
                parts.push(crate::monster_archive::PartPose {
                    tx: t.t_x as i16,
                    ty: t.t_y as i16,
                    tz: t.t_z as i16,
                    rx: (t.r_x & 0xFFF) as u16,
                    ry: (t.r_y & 0xFFF) as u16,
                    rz: (t.r_z & 0xFFF) as u16,
                });
            }
            frames.push(parts);
        }
        Some(crate::monster_archive::MonsterAnimation {
            variant_track: Default::default(),
            action_id: index as u8,
            rate: 2,
            attach_key: 0,
            solo_flag: 0,
            impact_class: 0,
            part_count,
            frame_count,
            frames,
            // Field ANM records are bone streams only - the battle
            // effect-script region exists only on battle action entries.
            effect_script: Vec::new(),
        })
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
    // This is the entry-decode half. The two-frame half - the next-entry
    // pick and the sub-frame blend gated on `*(a2+1) & 1` - is
    // `PlayerAnmBundle::sample_bone` / `blend_bone_transform` below.
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
/// PORT: FUN_8001D088 - reached through the two-frame sampler
/// [`PlayerAnmBundle::sample_pose`] / [`blend_bone_transform`], which both
/// hosts' field prop pose paths and the NPC / player clip player
/// (`legaia_engine_field::field_anim::FieldClipPlayer`) call.
///
/// `frac` is the 4-bit sub-frame fraction the frame blender carries at
/// `actor+0x68` (`0..=15`); the result is
/// `(to + ((from - to) * frac >> 4)) & 0xFFF`, so `frac == 0` yields `to`
/// and `frac == 16` would yield `from`. Both inputs are masked to 12 bits
/// first. The blender passes the **next** frame's angle as `from` and the
/// current frame's as `to` (`0x8001C108..0x8001C124`).
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
/// Retail also accumulates `|from - to|` into `_DAT_8007BD28` and journals
/// the unwrapped `(from, to)` pair into the 8-byte-stride slot table at
/// `0x800891A8`. Those are **not** bookkeeping: the blender reads both back
/// to decide and run its Euler-flip retry (see [`blend_bone_transform`]).
/// [`lerp_angle_12_journaled`] returns them as values instead of globals.
pub fn lerp_angle_12(from: i32, to: i32, frac: i32) -> u16 {
    lerp_angle_12_journaled(from, to, frac).value
}

/// [`lerp_angle_12`]'s full result: the interpolated angle plus the two side
/// outputs retail leaves in globals - the unwrapped pair it journals at
/// `0x800891A8 + slot*8` and the magnitude it adds to `_DAT_8007BD28`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AngleLerp {
    /// The interpolated 12-bit angle.
    pub value: u16,
    /// `from` after the unwrap (`sh a0,0x0(v0)` at `0x8001D124`).
    pub from_unwrapped: i32,
    /// `to` after the unwrap (`sh a1,0x2(v0)` at `0x8001D128`).
    pub to_unwrapped: i32,
    /// `|from - to|` after the unwrap - what the call adds to the counter.
    /// The two arms at `0x8001D0DC` add `|a1 - a0|` or `|a0 - a1|`, the same
    /// magnitude.
    pub magnitude: i32,
}

/// [`lerp_angle_12`] returning its journal and counter outputs as values.
///
/// PORT: FUN_8001D088
pub fn lerp_angle_12_journaled(from: i32, to: i32, frac: i32) -> AngleLerp {
    let mut from = from & 0xFFF;
    let mut to = to & 0xFFF;
    if from - to >= ANGLE_TURN / 2 {
        to += ANGLE_TURN;
    }
    if to - from >= ANGLE_TURN / 2 {
        from += ANGLE_TURN;
    }
    AngleLerp {
        value: (((((from - to) * frac) >> 4) + to) as u16) & 0x0FFF,
        from_unwrapped: from,
        to_unwrapped: to,
        magnitude: (from - to).abs(),
    }
}

/// The counter threshold past which the blender retries the angle blend
/// against the next frame's equivalent Euler triple (`slti v1,v1,0xc01` at
/// `0x8001C164`: the retry runs when the summed magnitude is `> 0xC00`).
pub const EULER_FLIP_THRESHOLD: i32 = 0xC00;

/// Blend two decoded `(bone, frame)` entries the way retail's frame blender
/// does: `cur` is the entry of the frame the cursor sits on, `next` the entry
/// the next-entry rule picked (see [`PlayerAnmBundle::sample_bone`]), `frac`
/// the cursor's low nibble (`actor+0x68 & 0xF`).
///
/// PORT: FUN_8001BE80 (the blend arm, `0x8001BF84..0x8001C1D0`)
///
/// - **Translations** lerp linearly on the sign-extended 16-bit values:
///   `cur + (((next - cur) * frac) >> 4)` (`mult` / `sra 4` / `addu`, then
///   `sh`, so the result is truncated to 16 bits). The shift is arithmetic,
///   so a negative delta rounds toward minus infinity.
/// - **Angles** go through [`lerp_angle_12`] with `from = next`, `to = cur`,
///   after zeroing the counter `_DAT_8007BD28` (`0x8001C11C`). If the three
///   calls' summed magnitudes exceed [`EULER_FLIP_THRESHOLD`], the blend is
///   redone against the next frame's **equivalent Euler triple** - `(x +
///   0x800, -(y + 0x800), z + 0x800)`, each built from the journaled unwrapped
///   pair (`0x8001C170..0x8001C1CC`). For a Z-Y-X rotation that triple is the
///   same orientation, so the retry takes the shorter path when the three
///   short arcs together run long (a pose passing near gimbal lock).
///
/// `frac == 0` returns `cur` exactly on every lane - the lerp's `to` term
/// survives and the retry cannot change that - so integer cursors pose
/// byte-exactly as [`BoneTransform::decode`] does.
pub fn blend_bone_transform(cur: BoneTransform, next: BoneTransform, frac: i32) -> BoneTransform {
    let frac = frac & 0xF;
    let lerp_t = |a: i32, b: i32| -> i32 {
        let (a, b) = (a as i16 as i32, b as i16 as i32);
        (a + (((b - a) * frac) >> 4)) as i16 as i32
    };
    let x = lerp_angle_12_journaled(next.r_x, cur.r_x, frac);
    let y = lerp_angle_12_journaled(next.r_y, cur.r_y, frac);
    let z = lerp_angle_12_journaled(next.r_z, cur.r_z, frac);
    let counter = x.magnitude + y.magnitude + z.magnitude;
    let (r_x, r_y, r_z) = if counter > EULER_FLIP_THRESHOLD {
        // `lh` of the journaled halfwords, then the per-axis flip. The retry
        // calls re-journal and re-add to the counter, but nothing reads either
        // again before the blender returns.
        let rx = lerp_angle_12(x.from_unwrapped + 0x800, x.to_unwrapped, frac);
        let ry = lerp_angle_12(-(y.from_unwrapped + 0x800), y.to_unwrapped, frac);
        let rz = lerp_angle_12(z.from_unwrapped + 0x800, z.to_unwrapped, frac);
        (rx, ry, rz)
    } else {
        (x.value, y.value, z.value)
    };
    BoneTransform {
        t_x: lerp_t(cur.t_x, next.t_x),
        t_y: lerp_t(cur.t_y, next.t_y),
        t_z: lerp_t(cur.t_z, next.t_z),
        r_x: r_x as i32,
        r_y: r_y as i32,
        r_z: r_z as i32,
    }
}

/// Find every player-ANM-shaped section in a single PROT entry.
///
/// Walks `bytes` as a [`parse_player_lzs`]-shaped container with the given
/// `descriptor_count` (most scene bundles use 3, 5, 6, or 7). For each
/// type-[`SCENE_ANM_TYPE_BYTE`] descriptor, LZS-decode the section and
/// validate it parses as a canonical ANM container.
/// Scale every (bone, frame) translation of record `index` by `r`, in
/// place inside the decoded bundle bytes. Rotations, headers, trailers
/// and every other container byte are preserved (byte 4's unread high
/// nibble included), so the edit is size-neutral by construction.
///
/// Translations are the flat per-frame joint positions the field
/// animator drives through the GTE (`FUN_8001BE80` decode), so a
/// uniform scale about the actor origin rescales the whole posed
/// skeleton while keeping it grounded (the origin is the floor plane).
/// The 12-bit domain clamps at `-2048..=2047`; callers stay well inside
/// it for any plausible figure scale.
pub fn scale_record_translations(decoded: &mut [u8], index: usize, r: f32) -> Result<()> {
    let bundle = parse(decoded).context("parse bundle for translation scale")?;
    let rec = bundle
        .record(index)
        .with_context(|| format!("record {index} for translation scale"))?;
    let start = bundle.record_offsets[index] as usize + RECORD_HEADER_SIZE;
    let entries = rec.bone_count as usize * rec.frame_count as usize;
    let scale = |v: i32| -> i32 { ((v as f32) * r).round().clamp(-2048.0, 2047.0) as i32 };
    for e in 0..entries {
        let off = start + e * BONE_FRAME_BYTES;
        let bytes: &mut [u8] = decoded
            .get_mut(off..off + BONE_FRAME_BYTES)
            .ok_or_else(|| anyhow::anyhow!("record {index} entry {e} out of range"))?;
        let t = BoneTransform::decode(bytes);
        let (tx, ty, tz) = (scale(t.t_x), scale(t.t_y), scale(t.t_z));
        bytes[0] = (tx & 0xFF) as u8;
        bytes[1] = (ty & 0xFF) as u8;
        bytes[2] = (((tx >> 8) & 0x0F) | (((ty >> 8) & 0x0F) << 4)) as u8;
        bytes[3] = (tz & 0xFF) as u8;
        bytes[4] = (bytes[4] & 0xF0) | (((tz >> 8) & 0x0F) as u8);
    }
    Ok(())
}

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

/// Every ANM bundle carried as a type-[`SCENE_ANM_TYPE_BYTE`] chunk of a
/// DATA_FIELD stream ([`legaia_container_formats::parse_streaming`]) - the other way a scene
/// ships its NPC clip bank. A stream's chunks are stored raw (the walker
/// hands each to the dispatcher with `copy_only = 1`), so the chunk body is
/// the bundle itself, no LZS step.
///
/// `rikuroa` is the case that needs it: its scene bank (72 records) is the
/// type-`0x05` chunk of PROT 157, the stream that also carries the scene
/// MAN, and no entry of its block holds a [`parse_player_lzs`] container
/// with an ANM section - so [`find_in_entry`] finds nothing and every NPC
/// clip id the scene names went unresolved. A retail `rikuroa` capture
/// holds the same 72-record table at the scene-bank pointer `FUN_800204F8`
/// reads (`_DAT_8007B888`).
pub fn find_in_stream(bytes: &[u8]) -> Vec<PlayerAnmBundle> {
    let Ok(report) = legaia_container_formats::parse_streaming(bytes, 4096) else {
        return Vec::new();
    };
    report
        .chunks
        .iter()
        .filter(|c| c.type_byte == SCENE_ANM_TYPE_BYTE)
        .filter_map(|c| {
            let start = c.header_offset + 4;
            let body = bytes.get(start..start + c.size as usize)?;
            parse(body).ok()
        })
        .collect()
}

/// The player-LZS descriptor counts a scene entry's container is tried at,
/// in order (town01 resolves at 3; the prologue scenes only at >= 5).
pub const SCENE_BUNDLE_DESCRIPTOR_COUNTS: [usize; 4] = [3, 5, 6, 7];

/// A scene's NPC clip bank out of its PROT entries, with the index of the
/// entry it came from: the first type-`0x05` section of a [`parse_player_lzs`]
/// container (entry-major, [`SCENE_BUNDLE_DESCRIPTOR_COUNTS`] minor), else
/// the first type-`0x05` chunk of a DATA_FIELD stream ([`find_in_stream`]).
/// The stream form is the fallback so a scene the container scan already
/// resolves keeps its bundle.
pub fn find_scene_bundle<'a>(
    entries: impl IntoIterator<Item = (u32, &'a [u8])> + Clone,
) -> Option<(u32, PlayerAnmBundle)> {
    entries
        .clone()
        .into_iter()
        .find_map(|(idx, bytes)| {
            SCENE_BUNDLE_DESCRIPTOR_COUNTS.into_iter().find_map(|desc| {
                find_in_entry(bytes, desc)
                    .into_iter()
                    .next()
                    .map(|b| (idx, b))
            })
        })
        .or_else(|| {
            entries
                .into_iter()
                .find_map(|(idx, bytes)| find_in_stream(bytes).into_iter().next().map(|b| (idx, b)))
        })
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
mod tests;
