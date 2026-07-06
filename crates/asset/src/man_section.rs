//! Per-scene MAN (asset type `0x03`) multi-section header walker.
//!
//! PORT: FUN_8003AEB0, FUN_8003A1E4, FUN_8003A110
//!
//! (`FUN_8003A110` is the section-0 carve: it reads the three stride bytes that
//! follow the section-0 pointer and walks `count * stride + 1` past each
//! sub-table to derive the formation / condition / region table bases - the
//! exact layout the [`EncounterSection`] parse + `region_records` reproduce.)
//!
//! The MAN sub-asset is the third descriptor in every scene's
//! [`scene_asset_table`](crate::scene_asset_table) bundle. After the asset
//! dispatcher LZS-decompresses it into a heap buffer addressed by
//! `_DAT_8007B898`, `FUN_8003AEB0` walks a fixed-shape multi-section header
//! and installs section pointers into the encounter control block
//! (`_DAT_801C6EA4`) and three sibling globals. Per
//! [`docs/formats/encounter.md`](../../../docs/formats/encounter.md),
//! `ctrl[+0x20]` lands on **section 0**, which `FUN_8003A110` then carves
//! up into formation/condition/region sub-tables.
//!
//! ## On-disc layout
//!
//! ```text
//! 0x00..0x02   u16   status_flags                 ; copied to the function's
//!                                                ; return value (carries the
//!                                                ; `& 0x400` "world-map
//!                                                ; wireframe" hint that the
//!                                                ; bulk-terrain emit
//!                                                ; mechanism consumes)
//! 0x01         u8    low bit -> DAT_8007B6A8     ; secondary scene flag
//! 0x02..0x22   16 x s16  depth_lut               ; written negated into the
//!                                                ; GTE scratchpad at
//!                                                ; 0x1F800314+0x48, the
//!                                                ; per-scene perspective /
//!                                                ; fog depth-sample table
//! 0x22..0x24   s16   N0                          ; count of partition-0
//!                                                ; records in the 3-byte
//!                                                ; record table
//! 0x24..0x26   s16   N1                          ; partition-1 (consumed
//!                                                ; by FUN_8003A1E4 as the
//!                                                ; per-scene NPC / actor
//!                                                ; placement list)
//! 0x26..0x28   s16   N2                          ; partition-2
//! 0x28..0x2B   u24LE u24_at_28                   ; in-table byte offset of
//!                                                ; section 0's length
//!                                                ; prefix within the data
//!                                                ; region that begins
//!                                                ; after the 3-byte record
//!                                                ; table
//! 0x2B..0x2B+3*(N0+N1+N2)  3-byte records       ; concatenated [P0..P1..P2]
//!                                                ; partitions; each record
//!                                                ; is a u24LE byte offset
//!                                                ; into the data region
//!                                                ; (used by FUN_8003A1E4
//!                                                ; for actor placement)
//! 0x2B + 3*N + u24_at_28                        ; <-- section 0 begins here
//!
//! sections [0..4] are linked-list length-prefixed:
//!   [u24LE byte_length][byte_length bytes]
//! the chain reader advances `next = section + 3 + byte_length`. Section 5
//! is universally a 3-byte zero terminator across all 80 retail scene
//! bundles.
//! ```
//!
//! The five sections install into different globals (per FUN_8003AEB0):
//!
//! | Index | Install target                | Role (where known)              |
//! |-------|-------------------------------|----------------------------------|
//! | 0     | `_DAT_801C6EA4[+0x20]`         | Encounter / formation tables. The first three bytes after this pointer are the strides FUN_8003A110 uses to carve `+0x20/+0x24/+0x28` into formation / condition / region table bases. See [`docs/formats/encounter.md`](../../../docs/formats/encounter.md). |
//! | 1     | `_DAT_801C6EA4[+0x00]`         | **Motion-VM script table** - the per-actor `FUN_80038158` bytecode streams `FUN_8003A9D4` installs at actor `+0x80` at scene entry (player = id `0xF8`, world-map entity = `0xFB`, else field-actor `+0x50` match). Decoder: [`crate::man_motion`]. The pointer is advanced past its 3-byte length prefix immediately after walking. |
//! | 2     | `_DAT_801C6EA0`                | (Open) - same advance-by-3 treatment as section 1. |
//! | 3     | `_DAT_801C6EA4[+0x04]`         | Zone / camera-region records (18-byte, count-prefixed; queried per tile by `FUN_801DBA20`) - same advance-by-3 treatment. |
//! | 4     | `DAT_80073ED8`                 | (Open) - advances by 4 (skipping length + 1 byte); the byte at `+3` is copied into `DAT_80073EDC` and a zero terminator there detaches the pointer (`DAT_80073ED8 = NULL`). |
//! | 5     | `DAT_80073EE0`                 | Universally a zero-length terminator in the retail corpus; reserved-but-unused / sentinel. |
//!
//! Cracking sections 1..4 in full is downstream of having this header
//! walker. This module provides the byte-exact section locator so engines
//! and scripts can lift each section's bytes by name without re-reading
//! the disassembly.

use serde::Serialize;

/// Number of sections the chain walk produces (encounter + 4 siblings +
/// terminator).
pub const SECTION_COUNT: usize = 6;

/// Number of partitions in the 3-byte record table (consumed by
/// FUN_8003A1E4 with `param_1` indexing into partition 1).
pub const RECORD_PARTITIONS: usize = 3;

/// Number of s16 entries in the depth LUT at MAN[0x02..0x22].
pub const DEPTH_LUT_LEN: usize = 16;

/// Byte offset where partition counts start.
pub const PARTITION_COUNTS_OFFSET: usize = 0x22;

/// Byte offset of the u24LE section-0 offset.
pub const U24_AT_28_OFFSET: usize = 0x28;

/// Byte offset where the 3-byte record table begins.
pub const RECORDS_BEGIN_OFFSET: usize = 0x2B;

/// Bit `0x400` of [`ManHeader::status_flags`] - hint consumed by the
/// world-map bulk-terrain emit path.
pub const STATUS_FLAG_WORLD_MAP_BULK_TERRAIN: u16 = 0x0400;

/// Decoded MAN multi-section file.
#[derive(Debug, Clone, Serialize)]
pub struct ManFile {
    /// Fixed-shape 0x2B-byte header.
    pub header: ManHeader,
    /// Three partitions of 3-byte u24LE record offsets. Each offset is a
    /// byte position into the data region that begins right after the
    /// concatenated record table.
    pub partitions: [Vec<u32>; RECORD_PARTITIONS],
    /// Byte offset (in the MAN buffer) where the data region begins. This
    /// equals `RECORDS_BEGIN_OFFSET + 3 * total_records`.
    pub data_region_offset: usize,
    /// Byte offsets of each section's length-prefix header.
    ///
    /// `sections[0..=4]` index real sections; `sections[5]` is the
    /// terminator position. A section with `length == 0` is the
    /// chain-terminator sentinel and has no payload.
    pub sections: [SectionRef; SECTION_COUNT],
}

/// Header at MAN offset 0.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct ManHeader {
    /// `MAN[0x00..0x02]` little-endian.
    pub status_flags: u16,
    /// `MAN[0x01] & 1`, copied to the per-scene `DAT_8007B6A8` flag.
    pub low_flag: bool,
    /// `MAN[0x02..0x22]` as 16 s16 values. Written negated to the GTE
    /// scratchpad at scene-init; the unnegated values are returned here.
    pub depth_lut: [i16; DEPTH_LUT_LEN],
    /// `MAN[0x22..0x24]`, `[0x24..0x26]`, `[0x26..0x28]`.
    pub partition_counts: [i16; RECORD_PARTITIONS],
    /// `MAN[0x28..0x2B]` as a 24-bit little-endian unsigned int.
    pub u24_at_28: u32,
}

impl ManHeader {
    /// Total number of records across all three partitions.
    pub fn total_records(&self) -> usize {
        self.partition_counts
            .iter()
            .map(|&c| c.max(0) as usize)
            .sum()
    }

    /// `true` when the world-map bulk-terrain flag is set.
    pub fn world_map_bulk_terrain(&self) -> bool {
        (self.status_flags & STATUS_FLAG_WORLD_MAP_BULK_TERRAIN) != 0
    }
}

/// One entry in the MAN section chain.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct SectionRef {
    /// Byte offset of the length prefix in the MAN buffer (the same value
    /// FUN_8003AEB0 installs into the section pointer).
    pub offset: usize,
    /// Decoded u24LE length read from `MAN[offset..offset+3]`. `0`
    /// means the section is the chain terminator.
    pub length: u32,
}

impl SectionRef {
    /// Byte offset where the section body starts (right after the 3-byte
    /// length prefix). Mirrors the runtime advance applied by
    /// FUN_8003AEB0 after the walk (`section += 3`).
    pub fn body_offset(&self) -> usize {
        self.offset + 3
    }

    /// Byte offset one past the last payload byte.
    pub fn end_offset(&self) -> usize {
        self.offset + 3 + self.length as usize
    }

    /// `true` when this section is the universal terminator (length == 0).
    pub fn is_terminator(&self) -> bool {
        self.length == 0
    }

    /// Slice the section body out of the MAN buffer (or `None` if the
    /// section runs past the buffer end).
    pub fn body<'a>(&self, man: &'a [u8]) -> Option<&'a [u8]> {
        man.get(self.body_offset()..self.end_offset())
    }
}

/// Errors the MAN walker can produce.
#[derive(Debug)]
pub enum ManError {
    BufferTooSmall {
        needed: usize,
        have: usize,
    },
    NegativePartitionCount {
        idx: usize,
        value: i16,
    },
    RecordTablePastEnd {
        records_end: usize,
        man_len: usize,
    },
    SectionStartsPastEnd {
        idx: usize,
        offset: usize,
        man_len: usize,
    },
    SectionPayloadPastEnd {
        idx: usize,
        offset: usize,
        length: u32,
        man_len: usize,
    },
}

impl std::fmt::Display for ManError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BufferTooSmall { needed, have } => write!(
                f,
                "MAN buffer too small: need at least {needed} bytes, have {have}"
            ),
            Self::NegativePartitionCount { idx, value } => {
                write!(f, "partition count {idx} is negative ({value})")
            }
            Self::RecordTablePastEnd {
                records_end,
                man_len,
            } => write!(
                f,
                "record table runs past MAN end (records_end=0x{records_end:X}, len={man_len})"
            ),
            Self::SectionStartsPastEnd {
                idx,
                offset,
                man_len,
            } => write!(
                f,
                "section {idx} starts past MAN end (offset=0x{offset:X}, len={man_len})"
            ),
            Self::SectionPayloadPastEnd {
                idx,
                offset,
                length,
                man_len,
            } => write!(
                f,
                "section {idx} payload runs past MAN end (offset=0x{offset:X}, length=0x{length:X}, len={man_len})"
            ),
        }
    }
}

impl std::error::Error for ManError {}

/// Parse a MAN buffer.
pub fn parse(man: &[u8]) -> Result<ManFile, ManError> {
    if man.len() < RECORDS_BEGIN_OFFSET {
        return Err(ManError::BufferTooSmall {
            needed: RECORDS_BEGIN_OFFSET,
            have: man.len(),
        });
    }

    let header = ManHeader {
        status_flags: u16::from_le_bytes([man[0], man[1]]),
        low_flag: (man[1] & 1) != 0,
        depth_lut: {
            let mut out = [0i16; DEPTH_LUT_LEN];
            for (i, slot) in out.iter_mut().enumerate() {
                let p = 2 + i * 2;
                *slot = i16::from_le_bytes([man[p], man[p + 1]]);
            }
            out
        },
        partition_counts: {
            let mut out = [0i16; RECORD_PARTITIONS];
            for (i, slot) in out.iter_mut().enumerate() {
                let p = PARTITION_COUNTS_OFFSET + i * 2;
                *slot = i16::from_le_bytes([man[p], man[p + 1]]);
            }
            out
        },
        u24_at_28: u24_le(man, U24_AT_28_OFFSET),
    };

    // Reject negative partition counts. The runtime treats them as
    // unsigned in practice (the read is a `lhu` then a sign-extending
    // shift, but every retail MAN is non-negative), but a parser exposed
    // to untrusted bytes should refuse them rather than `as usize`-cast.
    for (idx, &c) in header.partition_counts.iter().enumerate() {
        if c < 0 {
            return Err(ManError::NegativePartitionCount { idx, value: c });
        }
    }

    let total = header.total_records();
    let records_end = RECORDS_BEGIN_OFFSET + total * 3;
    if records_end > man.len() {
        return Err(ManError::RecordTablePastEnd {
            records_end,
            man_len: man.len(),
        });
    }

    // Concatenated record bytes are partitioned in [P0..P1..P2] order.
    let mut partitions: [Vec<u32>; RECORD_PARTITIONS] = Default::default();
    let mut cursor = RECORDS_BEGIN_OFFSET;
    for (idx, slot) in partitions.iter_mut().enumerate() {
        let n = header.partition_counts[idx] as usize;
        slot.reserve(n);
        for _ in 0..n {
            slot.push(u24_le(man, cursor));
            cursor += 3;
        }
    }
    debug_assert_eq!(cursor, records_end);

    let data_region_offset = records_end;

    // Section 0's offset is `data_region + u24_at_28`. Sections 1..=5
    // chain via length prefix from there.
    let mut sections = [SectionRef {
        offset: 0,
        length: 0,
    }; SECTION_COUNT];

    let s0_offset = data_region_offset.saturating_add(header.u24_at_28 as usize);
    if s0_offset + 3 > man.len() {
        return Err(ManError::SectionStartsPastEnd {
            idx: 0,
            offset: s0_offset,
            man_len: man.len(),
        });
    }
    sections[0] = SectionRef {
        offset: s0_offset,
        length: u24_le(man, s0_offset),
    };
    if sections[0].end_offset() > man.len() {
        return Err(ManError::SectionPayloadPastEnd {
            idx: 0,
            offset: sections[0].offset,
            length: sections[0].length,
            man_len: man.len(),
        });
    }

    for i in 1..SECTION_COUNT {
        let prev = sections[i - 1];
        let next_offset = prev.end_offset();
        // Even the terminator (length == 0) has a real 3-byte slot for
        // its zero length; require it to fit.
        if next_offset + 3 > man.len() {
            return Err(ManError::SectionStartsPastEnd {
                idx: i,
                offset: next_offset,
                man_len: man.len(),
            });
        }
        let length = u24_le(man, next_offset);
        sections[i] = SectionRef {
            offset: next_offset,
            length,
        };
        if sections[i].end_offset() > man.len() {
            return Err(ManError::SectionPayloadPastEnd {
                idx: i,
                offset: next_offset,
                length,
                man_len: man.len(),
            });
        }
    }

    Ok(ManFile {
        header,
        partitions,
        data_region_offset,
        sections,
    })
}

impl ManFile {
    /// Section 0 - the encounter section. `ctrl[+0x20]` in the retail
    /// control block.
    pub fn encounter_section(&self) -> &SectionRef {
        &self.sections[0]
    }

    /// Body bytes of section 0 (encounter section), or `None` if the
    /// MAN buffer is truncated. Suitable for feeding to
    /// [`parse_encounter_section`].
    pub fn encounter_section_body<'a>(&self, man: &'a [u8]) -> Option<&'a [u8]> {
        self.encounter_section().body(man)
    }

    /// Sibling sections (1..=4). Index 0 of the returned slice is
    /// section 1, index 1 is section 2, etc. Section 5 (terminator)
    /// is excluded.
    pub fn sibling_sections(&self) -> &[SectionRef] {
        &self.sections[1..5]
    }

    /// Section 5 (the terminator). Universally a zero-length sentinel
    /// across the retail corpus.
    pub fn terminator(&self) -> &SectionRef {
        &self.sections[5]
    }

    /// Resolve one partition-1 record (the "actor-placement" partition)
    /// to an absolute MAN byte offset. Mirrors
    /// `FUN_8003A1E4`'s `(iVar11 + param_1)` indexing where `iVar11 = N0`
    /// and `param_1` is the loop index `1..N1`.
    ///
    /// Returns `None` when `index >= N1` or when the resolved offset
    /// runs past the MAN buffer.
    pub fn actor_placement_record_offset(&self, index: usize, man_len: usize) -> Option<usize> {
        let n1 = self.header.partition_counts[1].max(0) as usize;
        if index >= n1 {
            return None;
        }
        let off = self.partitions[1][index] as usize;
        let abs = self.data_region_offset.checked_add(off)?;
        if abs >= man_len {
            return None;
        }
        Some(abs)
    }

    /// Resolve the field-VM **scene-entry system script** (context channel
    /// `0xFB`) within the MAN buffer.
    ///
    /// PORT: FUN_8003ab2c
    ///
    /// `FUN_8003ab2c` is the per-frame field-VM driver. At scene entry it
    /// builds the system script from partition 1's first record: the script
    /// block begins at `data_region_offset + partitions[1][0]` and opens
    /// with a `[u8 local_count N][N*2 bytes][4-byte record header]` prefix,
    /// so the first opcode is `1 + N*2 + 4` bytes in (the original computes
    /// `pcVar12 + 4 - pcVar11` after walking the `N` two-byte local
    /// entries).
    ///
    /// Returns `(script_start, pc0)` where `script_start` is the script
    /// block's byte offset in `man` and `pc0` is the first opcode's offset
    /// **relative to `script_start`** (the VM's `buffer_base` is
    /// `script_start`, matching the retail `iVar2[+0x90] = pcVar11`). Returns
    /// `None` when partition 1 is empty or the offsets run past the buffer.
    pub fn scene_entry_script(&self, man: &[u8]) -> Option<(usize, usize)> {
        let script_start = self.actor_placement_record_offset(0, man.len())?;
        let n = *man.get(script_start)? as usize;
        let pc0 = 1 + n * 2 + 4;
        if script_start.checked_add(pc0)? >= man.len() {
            return None;
        }
        Some((script_start, pc0))
    }

    /// Decode every NPC / actor placement in partition 1 (`FUN_8003A1E4`).
    ///
    /// PORT: FUN_8003A1E4
    ///
    /// The scene-init routine `FUN_8003AEB0` runs `FUN_8003A1E4` over
    /// partition-1 records `1..N1` (record `0` is the scene-entry controller
    /// whose script is the [`Self::scene_entry_script`]; it is not a placed
    /// entity). Each placed record shares the same prefix shape as the entry
    /// script - `[u8 local_count N][N × 2 bytes][4-byte placement header][script]`
    /// - and the 4-byte header is the actor's spawn data:
    ///
    /// | byte | meaning |
    /// |---|---|
    /// | +0 | model index. `< 0xF0` indexes the kingdom-TMD pool from base `DAT_8007b6f8`; `>= 0xF0` selects a special model from `_DAT_8007b824` (and sets the actor's `0x1000000` flag). |
    /// | +1 | move/action count (installed into actor `+0x5c`). |
    /// | +2 | tile X: `(b & 0x7F)` tile column; bit 7 shifts the spawn a half-tile. |
    /// | +3 | tile Z: same encoding for the row. |
    ///
    /// World position is `(b & 0x7F) * 128 + (if bit7 { 128 } else { 64 })` per
    /// axis (the actor sits at tile centre, or the next half-tile when bit 7 is
    /// set). The actor's field-VM script starts at `record + 1 + 2*N + 4` with
    /// the record base as its buffer; that script is what later installs the
    /// entity's encounter record (`actor[+0x94]`, initialised to `-1` here) or
    /// portal behaviour, so the placement gives **position + model + script
    /// pointer**, not the entity's kind. Returns one entry per readable record,
    /// in partition order.
    pub fn actor_placements(&self, man: &[u8]) -> Vec<ActorPlacement> {
        let n1 = self.header.partition_counts[1].max(0) as usize;
        (1..n1)
            .filter_map(|index| self.actor_placement(man, index))
            .collect()
    }

    /// Decode a single partition-1 placement record, or `None` when the index
    /// is out of range / the 4-byte placement header runs past the buffer.
    /// `index` is partition-1-relative (index `0` is the scene-entry
    /// controller; see [`Self::actor_placements`]).
    pub fn actor_placement(&self, man: &[u8], index: usize) -> Option<ActorPlacement> {
        let record_offset = self.actor_placement_record_offset(index, man.len())?;
        let local_count = *man.get(record_offset)? as usize;
        let header = record_offset.checked_add(1 + local_count * 2)?;
        let model_index = *man.get(header)?;
        let anim_id = *man.get(header + 1)?;
        let bx = *man.get(header + 2)?;
        let bz = *man.get(header + 3)?;
        let pos = |b: u8| -> i16 {
            let tile = ((b & 0x7F) as i16) * 0x80;
            tile + if b & 0x80 != 0 { 0x80 } else { 0x40 }
        };
        Some(ActorPlacement {
            index,
            record_offset,
            local_count,
            model_index,
            special_model: model_index >= 0xF0,
            anim_id,
            tile_x: bx & 0x7F,
            tile_z: bz & 0x7F,
            world_x: pos(bx),
            world_z: pos(bz),
            script_pc0: 1 + local_count * 2 + 4,
        })
    }
}

/// One placed NPC / actor decoded from the MAN partition-1 list
/// (`FUN_8003A1E4`). See [`ManFile::actor_placements`] for the byte layout.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ActorPlacement {
    /// Partition-1 record index (`1..N1`; index 0 is the scene controller).
    pub index: usize,
    /// Absolute byte offset of the record in the MAN buffer. This is also the
    /// actor's script buffer base (retail `actor[+0x90]`).
    pub record_offset: usize,
    /// `N` two-byte local entries preceding the placement header.
    pub local_count: usize,
    /// Raw model byte (`>= 0xF0` selects a special model; see
    /// [`Self::special_model`]).
    pub model_index: u8,
    /// `true` when `model_index >= 0xF0` (special-model base + `0x1000000`
    /// actor flag).
    pub special_model: bool,
    /// Animation id installed into actor `+0x5C`: **scene-bundle ANM record
    /// index + 1** (`0` = no animation). Runtime-pinned: every animated town01
    /// actor's `+0x5C` halfword equals its live anim-record pointer's bundle
    /// index + 1, and the disc byte seeds it (walkers drift +/-1 as scripts
    /// switch clips). For special models (`>= 0xF0`, the party/savepoint
    /// global-pool head) the id indexes the PROT 0874 section-1 locomotion
    /// bundle instead (Noa placement carries id 9 = locomotion record 8 =
    /// Noa's idle; Gala id 16 = record 15).
    pub anim_id: u8,
    /// Tile column (`world_x >> 7`).
    pub tile_x: u8,
    /// Tile row (`world_z >> 7`).
    pub tile_z: u8,
    /// Spawn world X (tile centre, or next half-tile when the X bit-7 is set).
    pub world_x: i16,
    /// Spawn world Z.
    pub world_z: i16,
    /// Byte offset (relative to `record_offset`) of the actor's first field-VM
    /// opcode (`1 + 2*local_count + 4`).
    pub script_pc0: usize,
}

fn u24_le(buf: &[u8], pos: usize) -> u32 {
    buf[pos] as u32 | ((buf[pos + 1] as u32) << 8) | ((buf[pos + 2] as u32) << 16)
}

// ============================================================================
// Encounter section (section 0) interior layout.
//
// `FUN_8003A110` reads the 4-byte header at the start of section 0's body,
// then walks three count-prefixed record arrays:
//
//   +0x00  u8  formation_stride
//   +0x01  u8  condition_stride
//   +0x02  u8  region_stride
//   +0x03  u8  formation_count
//   +0x04  formation_count * formation_stride bytes
//   +0x04 + formation_count * formation_stride
//          u8  condition_count
//          condition_count * condition_stride bytes
//   ...    u8  region_count
//          region_count * region_stride bytes
//
// Per the existing docs/formats/encounter.md mapping, each formation
// record carries an `encounter_record` shape at `+0x3..` (count + ids),
// each region record carries a 4-byte AABB + rate + formation range.
// This parser surfaces the strides and slices so callers can apply the
// per-row decoders without re-walking the bytes.

/// Decoded section-0 interior.
#[derive(Debug, Clone, Serialize)]
pub struct EncounterSection {
    pub formation_stride: u8,
    pub condition_stride: u8,
    pub region_stride: u8,
    pub formation_count: u8,
    pub condition_count: u8,
    pub region_count: u8,

    /// Byte range (relative to the section *body* slice passed in) where
    /// the formation array lives.
    pub formation_range: (usize, usize),
    /// Same for the condition array.
    pub condition_range: (usize, usize),
    /// Same for the region array.
    pub region_range: (usize, usize),
}

impl EncounterSection {
    /// Number of bytes consumed by the parser (header + all three arrays).
    pub fn total_bytes(&self) -> usize {
        self.region_range.1
    }
}

/// Errors for the section-0 interior walk.
#[derive(Debug)]
pub enum EncounterSectionError {
    HeaderTooSmall(usize),
    FormationArrayPastEnd { needed: usize, have: usize },
    ConditionArrayPastEnd { needed: usize, have: usize },
    RegionArrayPastEnd { needed: usize, have: usize },
}

impl std::fmt::Display for EncounterSectionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::HeaderTooSmall(have) => write!(
                f,
                "section-0 body too small: need at least 4 bytes for header, have {have}"
            ),
            Self::FormationArrayPastEnd { needed, have } => write!(
                f,
                "formation array runs past section-0 body (need {needed}, have {have})"
            ),
            Self::ConditionArrayPastEnd { needed, have } => write!(
                f,
                "condition array runs past section-0 body (need {needed}, have {have})"
            ),
            Self::RegionArrayPastEnd { needed, have } => write!(
                f,
                "region array runs past section-0 body (need {needed}, have {have})"
            ),
        }
    }
}

impl std::error::Error for EncounterSectionError {}

/// Parse the section-0 interior. `body` is the slice returned by
/// [`SectionRef::body`] for section 0.
pub fn parse_encounter_section(body: &[u8]) -> Result<EncounterSection, EncounterSectionError> {
    if body.len() < 4 {
        return Err(EncounterSectionError::HeaderTooSmall(body.len()));
    }
    let formation_stride = body[0];
    let condition_stride = body[1];
    let region_stride = body[2];
    let formation_count = body[3];

    let f_start = 4usize;
    let f_end = f_start + formation_count as usize * formation_stride as usize;
    if f_end > body.len() {
        return Err(EncounterSectionError::FormationArrayPastEnd {
            needed: f_end,
            have: body.len(),
        });
    }

    let cond_count_pos = f_end;
    if cond_count_pos + 1 > body.len() {
        return Err(EncounterSectionError::ConditionArrayPastEnd {
            needed: cond_count_pos + 1,
            have: body.len(),
        });
    }
    let condition_count = body[cond_count_pos];
    let c_start = cond_count_pos + 1;
    let c_end = c_start + condition_count as usize * condition_stride as usize;
    if c_end > body.len() {
        return Err(EncounterSectionError::ConditionArrayPastEnd {
            needed: c_end,
            have: body.len(),
        });
    }

    let reg_count_pos = c_end;
    if reg_count_pos + 1 > body.len() {
        return Err(EncounterSectionError::RegionArrayPastEnd {
            needed: reg_count_pos + 1,
            have: body.len(),
        });
    }
    let region_count = body[reg_count_pos];
    let r_start = reg_count_pos + 1;
    let r_end = r_start + region_count as usize * region_stride as usize;
    if r_end > body.len() {
        return Err(EncounterSectionError::RegionArrayPastEnd {
            needed: r_end,
            have: body.len(),
        });
    }

    Ok(EncounterSection {
        formation_stride,
        condition_stride,
        region_stride,
        formation_count,
        condition_count,
        region_count,
        formation_range: (f_start, f_end),
        condition_range: (c_start, c_end),
        region_range: (r_start, r_end),
    })
}

/// Per-formation decoded fields.
///
/// Mirrors the [`EncounterRecord`](crate)-shaped slot the formation
/// record carries at `+0x3..`. Bytes `+0..+3` are passed through as
/// `header_bytes` for callers that want to peek at them (the random-
/// encounter trigger path stores extra state there).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct FormationRecord {
    pub header_bytes: [u8; 3],
    pub monster_count: u8,
    pub monster_ids: [u8; 4],
    /// Bytes after `+4 + monster_count` (stride-dependent; the runtime
    /// reader copies only `monster_ids[0..monster_count]` into the
    /// formation cell - trailing bytes are stride padding / other
    /// per-formation state).
    pub trailing_byte_count: u8,
}

impl FormationRecord {
    /// Parse a single formation record. Requires the slice to be exactly
    /// `formation_stride` bytes long; returns `None` if the slice is too
    /// short for `(4 + monster_count)` or claims more than 4 monsters.
    pub fn parse(record: &[u8]) -> Option<Self> {
        if record.len() < 4 {
            return None;
        }
        let monster_count = record[3];
        if monster_count as usize > 4 {
            return None;
        }
        let payload_end = 4 + monster_count as usize;
        if payload_end > record.len() {
            return None;
        }
        let mut monster_ids = [0u8; 4];
        let n = monster_count as usize;
        monster_ids[..n].copy_from_slice(&record[4..4 + n]);
        Some(Self {
            header_bytes: [record[0], record[1], record[2]],
            monster_count,
            monster_ids,
            trailing_byte_count: (record.len() - payload_end) as u8,
        })
    }
}

/// Per-region decoded fields. Mirrors the random-encounter trigger reader
/// at `FUN_801D9E1C` (see [`docs/formats/encounter.md`](../../../docs/formats/encounter.md)).
///
/// Only the well-understood prefix is decoded; bytes after `+0x08` are
/// stride-dependent extras kept as a raw tail.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct RegionRecord {
    pub aabb_x_min: u8,
    pub aabb_y_min: u8,
    pub aabb_x_max: u8,
    pub aabb_y_max: u8,
    /// Per-step rate increment.
    pub rate_increment: u8,
    /// `region[+5]` - role currently unknown (rate? terrain class?).
    pub reserved_5: u8,
    /// First formation index this region rolls into.
    pub formation_range_base: u8,
    /// Number of formations in the roll range.
    pub formation_range_count: u8,
}

impl RegionRecord {
    /// Parse a single region record (at least 8 bytes).
    pub fn parse(record: &[u8]) -> Option<Self> {
        if record.len() < 8 {
            return None;
        }
        Some(Self {
            aabb_x_min: record[0],
            aabb_y_min: record[1],
            aabb_x_max: record[2],
            aabb_y_max: record[3],
            rate_increment: record[4],
            reserved_5: record[5],
            formation_range_base: record[6],
            formation_range_count: record[7],
        })
    }
}

/// Iterator over the formation records inside a parsed encounter
/// section's `formation_range`.
pub fn formation_records<'a>(
    body: &'a [u8],
    sec: &EncounterSection,
) -> impl Iterator<Item = Option<FormationRecord>> + 'a {
    let stride = sec.formation_stride as usize;
    let (start, end) = sec.formation_range;
    let slice = &body[start..end];
    (0..sec.formation_count as usize).map(move |i| {
        let p = i * stride;
        FormationRecord::parse(&slice[p..p + stride])
    })
}

/// Iterator over the region records inside a parsed encounter section.
pub fn region_records<'a>(
    body: &'a [u8],
    sec: &EncounterSection,
) -> impl Iterator<Item = Option<RegionRecord>> + 'a {
    let stride = sec.region_stride as usize;
    let (start, end) = sec.region_range;
    let slice = &body[start..end];
    (0..sec.region_count as usize).map(move |i| {
        let p = i * stride;
        RegionRecord::parse(&slice[p..p + stride])
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a minimal MAN buffer with N0=1, N1=2, N2=1 and a custom
    /// section_0 payload.
    fn build_synthetic(
        depth_lut: [i16; 16],
        section_0_body: &[u8],
        section_n_lengths: [u32; 4],
    ) -> Vec<u8> {
        let mut buf = Vec::new();
        buf.push(0xB2);
        buf.push(0x01); // status low bit + bit 0x100 -> 0x01B2
        for v in depth_lut {
            buf.extend_from_slice(&v.to_le_bytes());
        }
        // N0=1, N1=2, N2=1
        buf.extend_from_slice(&1i16.to_le_bytes());
        buf.extend_from_slice(&2i16.to_le_bytes());
        buf.extend_from_slice(&1i16.to_le_bytes());
        // 3-byte u24 at 0x28 == data-region offset where section 0 lands.
        // We'll place section 0 immediately after the record table.
        let data_region_start = 0x2B + 3 * (1 + 2 + 1);
        let u24_at_28 = 0u32; // section 0 right at the start of the data region
        buf.extend_from_slice(&[
            (u24_at_28 & 0xFF) as u8,
            ((u24_at_28 >> 8) & 0xFF) as u8,
            ((u24_at_28 >> 16) & 0xFF) as u8,
        ]);
        // Record table - 4 records of 3 bytes each, all zero offsets
        for _ in 0..4 {
            buf.extend_from_slice(&[0, 0, 0]);
        }
        assert_eq!(buf.len(), data_region_start);

        // Section 0: u24 length + body
        let s0_len = section_0_body.len() as u32;
        buf.extend_from_slice(&[
            (s0_len & 0xFF) as u8,
            ((s0_len >> 8) & 0xFF) as u8,
            ((s0_len >> 16) & 0xFF) as u8,
        ]);
        buf.extend_from_slice(section_0_body);

        // Sections 1..=4: length-prefixed with caller-provided lengths and
        // dummy bytes.
        for &ln in &section_n_lengths {
            buf.extend_from_slice(&[
                (ln & 0xFF) as u8,
                ((ln >> 8) & 0xFF) as u8,
                ((ln >> 16) & 0xFF) as u8,
            ]);
            for i in 0..ln as usize {
                buf.push(0xAA ^ i as u8);
            }
        }
        // Section 5 terminator: 3 zero bytes.
        buf.extend_from_slice(&[0, 0, 0]);
        buf
    }

    #[test]
    fn actor_placement_decodes_position_model_and_script_offset() {
        // Minimal MAN: N0=1, N1=2 (partition-1 record 0 = scene controller,
        // record 1 = one placed actor), N2=0.
        let mut buf = vec![0u8; 0x2B];
        buf[0] = 0xB2;
        buf[1] = 0x01;
        buf[0x22..0x24].copy_from_slice(&1i16.to_le_bytes()); // N0
        buf[0x24..0x26].copy_from_slice(&2i16.to_le_bytes()); // N1
        buf[0x26..0x28].copy_from_slice(&0i16.to_le_bytes()); // N2
        // Section 0 sits 8 bytes into the data region (after the placement record).
        buf[0x28] = 8;
        // Record table: 3 records (N0+N1+N2) of 3 bytes; all point at data
        // region offset 0 (the placement record - record 0 / P1[0] reuse it,
        // they aren't exercised here).
        buf.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0, 0, 0]);
        // data region (offset 0x34): the placement record for P1[1].
        //   local_count=1, 2 local bytes, model=5, actions=2,
        //   tile_x=0x83 (tile 3 + half), tile_z=0x04 (tile 4), script halt 0x21
        buf.extend_from_slice(&[0x01, 0xAA, 0xBB, 0x05, 0x02, 0x83, 0x04, 0x21]);
        // Six zero-length section prefixes (sections 0..=4 + terminator).
        buf.extend_from_slice(&[0u8; 18]);

        let man = parse(&buf).expect("parse");
        assert_eq!(man.header.partition_counts, [1, 2, 0]);
        let placements = man.actor_placements(&buf);
        assert_eq!(
            placements.len(),
            1,
            "record 0 is the controller; only record 1 is a placement"
        );
        let p = &placements[0];
        assert_eq!(p.index, 1);
        assert_eq!(p.local_count, 1);
        assert_eq!(p.model_index, 5);
        assert!(!p.special_model);
        assert_eq!(p.anim_id, 2);
        assert_eq!(p.tile_x, 3);
        assert_eq!(p.tile_z, 4);
        assert_eq!(p.world_x, 3 * 128 + 128, "X bit-7 set shifts a half-tile");
        assert_eq!(p.world_z, 4 * 128 + 64, "Z bit-7 clear -> tile centre");
        assert_eq!(
            p.script_pc0,
            1 + 2 + 4,
            "1 prefix + local_count*2 + 4 header"
        );
        assert_eq!(p.record_offset, man.data_region_offset);
    }

    #[test]
    fn actor_placement_flags_special_model() {
        // A model byte >= 0xF0 marks the special-model (lead-actor) slot.
        let mut buf = vec![0u8; 0x2B];
        buf[0] = 0xB2;
        buf[0x22..0x24].copy_from_slice(&1i16.to_le_bytes()); // N0
        buf[0x24..0x26].copy_from_slice(&2i16.to_le_bytes()); // N1
        buf[0x28] = 6; // section 0 after the 6-byte placement record
        buf.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0, 0, 0]);
        // local_count=0, model=0xF2, actions=0, tile_x=0, tile_z=0
        buf.extend_from_slice(&[0x00, 0xF2, 0x00, 0x00, 0x00, 0x21]);
        buf.extend_from_slice(&[0u8; 18]);

        let man = parse(&buf).expect("parse");
        let p = &man.actor_placements(&buf)[0];
        assert_eq!(p.model_index, 0xF2);
        assert!(p.special_model);
        assert_eq!(p.script_pc0, 1 + 4, "local_count 0 -> script at +5");
    }

    #[test]
    fn parses_synthetic_man() {
        let lut = [
            1, 48, 96, 128, 192, 240, 288, 336, 384, 432, 480, 528, 576, 624, 672, 720,
        ];
        // Build a section-0 body that decodes as 1 formation + 0 conditions
        // + 0 regions: stride=8, count=1, then 8 bytes:
        //   [0,0,0, count=2, ids=4,4, padding 2]
        let section_0_body: Vec<u8> = vec![
            0x08, 0x04, 0x0C, 0x01, // header: f_stride=8, c_stride=4, r_stride=12, f_count=1
            0x00, 0x00, 0x00, 0x02, 0x04, 0x04, 0x00, 0x00, // formation 0
            0x00, // condition_count=0
            0x00, // region_count=0
        ];
        let buf = build_synthetic(lut, &section_0_body, [5, 6, 7, 8]);

        let man = parse(&buf).expect("parse");
        assert_eq!(man.header.status_flags, 0x01B2);
        assert!(man.header.low_flag);
        assert_eq!(man.header.depth_lut, lut);
        assert_eq!(man.header.partition_counts, [1, 2, 1]);
        assert_eq!(man.header.total_records(), 4);
        assert_eq!(man.header.u24_at_28, 0);
        assert_eq!(man.data_region_offset, 0x2B + 3 * 4);

        // Section 0 lands at data region + u24_at_28 = data_region.
        assert_eq!(man.sections[0].offset, man.data_region_offset);
        assert_eq!(man.sections[0].length as usize, section_0_body.len());

        // Sections 1..=4 chain.
        for i in 0..4 {
            let lengths = [5, 6, 7, 8];
            assert_eq!(man.sections[i + 1].length, lengths[i]);
        }
        // Section 5 is the terminator.
        assert!(man.sections[5].is_terminator());

        // Encounter-section body equals what we wrote.
        let body = man.encounter_section_body(&buf).unwrap();
        assert_eq!(body, section_0_body.as_slice());

        // Decode the interior.
        let es = parse_encounter_section(body).unwrap();
        assert_eq!(es.formation_stride, 8);
        assert_eq!(es.condition_stride, 4);
        assert_eq!(es.region_stride, 12);
        assert_eq!(es.formation_count, 1);
        assert_eq!(es.condition_count, 0);
        assert_eq!(es.region_count, 0);

        let formations: Vec<_> = formation_records(body, &es).collect();
        assert_eq!(formations.len(), 1);
        let f = formations[0].expect("formation 0 parses");
        assert_eq!(f.monster_count, 2);
        assert_eq!(f.monster_ids, [4, 4, 0, 0]);
    }

    #[test]
    fn rejects_truncated_man() {
        let buf = vec![0u8; 0x10];
        assert!(matches!(parse(&buf), Err(ManError::BufferTooSmall { .. })));
    }

    #[test]
    fn rejects_negative_partition_count() {
        let lut = [0i16; 16];
        let section_0_body: Vec<u8> = vec![0x00, 0x00, 0x00, 0x00, 0x00, 0x00];
        let mut buf = build_synthetic(lut, &section_0_body, [0, 0, 0, 0]);
        // Patch N0 to -1.
        buf[0x22] = 0xFF;
        buf[0x23] = 0xFF;
        let err = parse(&buf).unwrap_err();
        assert!(matches!(
            err,
            ManError::NegativePartitionCount { idx: 0, value: -1 }
        ));
    }

    #[test]
    fn scene_entry_script_resolves_partition1_first_record() {
        // Hand-build a ManFile whose partition-1 first record points at a
        // crafted script block: [u8 N=2][N*2 local bytes][4-byte header]
        // [opcode...]. The first opcode is at +1 + 2*2 + 4 = +9.
        let data_region_offset = 0x40usize;
        let p1_0 = 0x10u32;
        let script_start = data_region_offset + p1_0 as usize; // 0x50
        let mut man = vec![0u8; 0x80];
        man[script_start] = 2; // local-entry count N
        man[script_start + 9] = 0x25; // first real opcode

        let mf = ManFile {
            header: ManHeader {
                status_flags: 0,
                low_flag: false,
                depth_lut: [0; 16],
                partition_counts: [1, 1, 0],
                u24_at_28: 0,
            },
            partitions: [vec![0], vec![p1_0], vec![]],
            data_region_offset,
            sections: [SectionRef {
                offset: 0,
                length: 0,
            }; SECTION_COUNT],
        };
        let (start, pc0) = mf.scene_entry_script(&man).expect("entry script resolves");
        assert_eq!(start, script_start);
        assert_eq!(pc0, 9);
        assert_eq!(man[start + pc0], 0x25);

        // Empty partition 1 -> no entry script.
        let mut mf_empty = mf.clone();
        mf_empty.partitions[1].clear();
        mf_empty.header.partition_counts[1] = 0;
        assert!(mf_empty.scene_entry_script(&man).is_none());
    }

    #[test]
    fn formation_record_parser_clamps_to_payload() {
        // stride=8 record: [0,0,0, count=1, id=7, 0, 0, 0] - count=1 and
        // 3 trailing pad bytes.
        let rec = [0, 0, 0, 1, 7, 0, 0, 0];
        let f = FormationRecord::parse(&rec).unwrap();
        assert_eq!(f.monster_count, 1);
        assert_eq!(f.monster_ids, [7, 0, 0, 0]);
        assert_eq!(f.trailing_byte_count, 3);
    }

    #[test]
    fn formation_record_rejects_excess_count() {
        let rec = [0, 0, 0, 5, 1, 2, 3, 4, 5];
        assert!(FormationRecord::parse(&rec).is_none());
    }

    #[test]
    fn region_record_parses_aabb_and_range() {
        let rec = [10, 20, 30, 40, 7, 0, 3, 5, 0xCC, 0xCC, 0xCC, 0xCC];
        let r = RegionRecord::parse(&rec).unwrap();
        assert_eq!(r.aabb_x_min, 10);
        assert_eq!(r.aabb_y_min, 20);
        assert_eq!(r.aabb_x_max, 30);
        assert_eq!(r.aabb_y_max, 40);
        assert_eq!(r.rate_increment, 7);
        assert_eq!(r.formation_range_base, 3);
        assert_eq!(r.formation_range_count, 5);
    }

    #[test]
    fn synthetic_actor_placement_record_offset() {
        let lut = [0i16; 16];
        let body = vec![0x08, 0x04, 0x0C, 0x00, 0x00, 0x00];
        let mut buf = build_synthetic(lut, &body, [0, 0, 0, 0]);
        // Patch partition-1 record [0] to point at byte 4 in the data
        // region. Records start at 0x2B and partition 1 begins after
        // N0=1 records (so record 0 of partition 1 is at offset 0x2B + 3 = 0x2E).
        let p1_record0 = 0x2B + 3;
        buf[p1_record0] = 0x04;
        buf[p1_record0 + 1] = 0;
        buf[p1_record0 + 2] = 0;
        let man = parse(&buf).unwrap();
        let abs = man.actor_placement_record_offset(0, buf.len()).unwrap();
        assert_eq!(abs, man.data_region_offset + 4);
    }
}
