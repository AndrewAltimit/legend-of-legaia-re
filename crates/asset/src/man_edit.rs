//! Variable-length editing of a decompressed scene **MAN** buffer.
//!
//! The field-VM `0x3F` named-scene-change ("door / exit") destinations are
//! **partition-2 records** addressed through the MAN's partition record-offset
//! table (runtime-pinned: the controller sets the VM bytecode base to
//! `man_base + data_region + partition2[slot]` and runs the record). Their
//! inline destination *name* is variable length, so re-pointing a door at a
//! scene with a differently-sized name **changes the record's byte length** and
//! shifts everything after it. This module rebuilds the MAN buffer applying such
//! resizes and fixing every internal offset so the structure stays valid:
//!
//! 1. **Partition record-offset tables** (`MAN[0x2B..]`, u24LE entries relative
//!    to `data_region_offset`): every entry whose record starts after the edit
//!    is bumped by the byte delta. This table *is* the door dispatch index, so
//!    fixing it keeps each destination addressable.
//! 2. **`u24_at_28`** (header section-0 offset, relative to `data_region`): the
//!    section chain sits after the records, so it shifts by the total delta.
//! 3. **Intra-record relative jumps** (`field_disasm` `0x26/0x42/0x4D/0x4E`
//!    /`0x70`) inside an edited record whose source/target straddle the edit:
//!    the stored u16 delta is recomputed. A jump wholly on one side is
//!    unaffected (its endpoints shift together). The delta field sits exactly at
//!    the jump's relative base (`target - delta`), so the rewrite is op-agnostic.
//!
//! `data_region_offset` itself is derived (`0x2B + 3*total_records`) and does
//! not move. The **decompressed size** is stored only in the external
//! scene-bundle descriptor (`scene_asset_table`), which the caller rewrites with
//! [`crate::scene_asset_table::encode_size_word`] after recompressing.
//!
//! ## Safety
//!
//! Across the retail corpus there are **no absolute-reference ops**
//! (`0x45 0xC0` camera-apply, `0x4E` abs-jump) at/after any destination op, so
//! [`apply_dest_edits`] **errors out** (rather than risk a wrong fixup) if it
//! finds one in an edited record - the caller then leaves that scene unchanged.
//! [`validate`] re-parses + re-walks the rebuilt MAN as a final backstop.

use crate::field_disasm::{self, CameraKind, InsnInfo, InventoryCmpKind};
use crate::man_section::{self, ManFile, RECORDS_BEGIN_OFFSET, U24_AT_28_OFFSET};

/// One destination rewrite: replace the `0x3F` op at `op_pc` with a new
/// destination descriptor. All fields come from the destination being pointed
/// at (so `index`/`entry`/`dir` travel with the `name`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DestEdit {
    /// Absolute offset of the `0x3F` opcode (lead byte) in the decompressed MAN.
    pub op_pc: usize,
    /// New `i16` destination index (the dest-scene id passed to the warp packet).
    pub index: i16,
    /// New destination scene-name bytes (raw, no terminator; 1..=255 long).
    pub name: Vec<u8>,
    /// New destination entry-tile X byte.
    pub entry_x: u8,
    /// New destination entry-tile Z byte.
    pub entry_z: u8,
    /// New facing/depth selector byte.
    pub dir: u8,
}

/// Errors [`apply_dest_edits`] can return; the caller treats any of these as
/// "leave this scene unchanged".
#[derive(Debug, PartialEq, Eq)]
pub enum ManEditError {
    /// `op_pc` doesn't decode as a `0x3F` SceneChange op.
    NotSceneChange { op_pc: usize },
    /// Two edits target the same / overlapping operand block.
    OverlappingEdits,
    /// `op_pc` couldn't be mapped to a partition record (so its record bounds /
    /// `pc0` are unknown, and intra-record jumps can't be checked).
    RecordNotFound { op_pc: usize },
    /// An edited record contains an absolute-reference op (`0x45 0xC0` /
    /// `0x4E` abs-jump) at/after the edit - too risky to relocate.
    AbsoluteRef { op_pc: usize, ref_pc: usize },
    /// A new name is empty or longer than a u8 length field allows.
    BadName { len: usize },
    /// The MAN failed to parse.
    Parse,
}

impl std::fmt::Display for ManEditError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotSceneChange { op_pc } => {
                write!(f, "op at 0x{op_pc:X} is not a 0x3F scene-change")
            }
            Self::OverlappingEdits => write!(f, "overlapping destination edits"),
            Self::RecordNotFound { op_pc } => {
                write!(f, "no partition record contains op 0x{op_pc:X}")
            }
            Self::AbsoluteRef { op_pc, ref_pc } => write!(
                f,
                "edited record (op 0x{op_pc:X}) has an absolute ref at 0x{ref_pc:X}"
            ),
            Self::BadName { len } => write!(f, "bad destination name length {len}"),
            Self::Parse => write!(f, "MAN failed to parse"),
        }
    }
}

impl std::error::Error for ManEditError {}

/// Sorted list of `(start, old_len)` record spans across all three partitions,
/// in absolute MAN coordinates. Used to find the record containing an edit and
/// to bound a record's clean walk.
fn record_starts(mf: &ManFile) -> Vec<usize> {
    let dro = mf.data_region_offset;
    let mut starts: Vec<usize> = Vec::new();
    for part in &mf.partitions {
        for &off in part {
            starts.push(dro + off as usize);
        }
    }
    // Section starts also bound the last record's walk.
    for s in &mf.sections {
        starts.push(s.offset);
    }
    starts.sort_unstable();
    starts.dedup();
    starts
}

/// Partition-2 named-record first-opcode offset (relative to the record start).
/// `[u8 name_len][name_len*2 SJIS][u8 c0][c0][u8 c1][c1*2][u8 c2][c2*2]`. Mirror
/// of `man_field_scripts::partition2_record_script_offset`.
fn p2_pc0(man: &[u8], start: usize) -> Option<usize> {
    let name_len = *man.get(start)? as usize;
    let mut cur = 1 + name_len * 2;
    let c0 = *man.get(start + cur)? as usize;
    cur += 1 + c0;
    let c1 = *man.get(start + cur)? as usize;
    cur += 1 + c1 * 2;
    let c2 = *man.get(start + cur)? as usize;
    cur += 1 + c2 * 2;
    Some(cur)
}

/// Which partition (0/1/2) a record start belongs to, for picking the `pc0`
/// header shape.
fn partition_of(mf: &ManFile, start: usize) -> Option<usize> {
    let dro = mf.data_region_offset;
    for (p, part) in mf.partitions.iter().enumerate() {
        if part.iter().any(|&o| dro + o as usize == start) {
            return Some(p);
        }
    }
    None
}

/// `(record_start, pc0, record_end)` for the record containing `op_pc`.
fn record_for(mf: &ManFile, man: &[u8], op_pc: usize) -> Option<(usize, usize, usize)> {
    let starts = record_starts(mf);
    let start = *starts.iter().rev().find(|&&s| s <= op_pc)?;
    let end = starts
        .iter()
        .copied()
        .find(|&s| s > start)
        .unwrap_or(man.len());
    let p = partition_of(mf, start)?;
    let pc0 = if p == 2 {
        p2_pc0(man, start)?
    } else {
        let locals = *man.get(start)? as usize;
        1 + locals * 2 + 4
    };
    Some((start, pc0, end))
}

/// A relative jump found in an edited record: the byte offset of its u16 LE
/// delta field (= its relative base) and its absolute target.
struct RelJump {
    base: usize,
    target: usize,
}

/// Collect the relative jumps + detect absolute refs in `[start+pc0, end)` via a
/// clean fall-through decode. Returns `Err` (the op_pc for context) on an
/// absolute ref. A decode error ends the clean walk (the rest is data).
fn scan_record_refs(
    man: &[u8],
    start: usize,
    pc0: usize,
    end: usize,
    op_pc: usize,
) -> Result<Vec<RelJump>, ManEditError> {
    let mut jumps = Vec::new();
    let mut pc = start + pc0;
    while pc < end {
        let Ok(insn) = field_disasm::decode(man, pc) else {
            break;
        };
        if insn.size == 0 || insn.pc >= end {
            break;
        }
        match &insn.info {
            InsnInfo::JmpRel { delta, target } => jumps.push(RelJump {
                base: target.wrapping_sub(*delta as usize),
                target: *target,
            }),
            InsnInfo::CondJmp { delta, target, .. } => jumps.push(RelJump {
                base: target.wrapping_sub(*delta as usize),
                target: *target,
            }),
            InsnInfo::BBoxTest {
                skip_delta,
                skip_target,
                ..
            } => jumps.push(RelJump {
                base: skip_target.wrapping_sub(*skip_delta as usize),
                target: *skip_target,
            }),
            InsnInfo::SystemFlag {
                delta: Some(d),
                target: Some(t),
                ..
            } => jumps.push(RelJump {
                base: t.wrapping_sub(*d as usize),
                target: *t,
            }),
            InsnInfo::InventoryCmp { kind, .. } => match kind {
                InventoryCmpKind::Compare {
                    skip_delta,
                    skip_target,
                    ..
                }
                | InventoryCmpKind::PartyBank {
                    skip_delta,
                    skip_target,
                    ..
                } => jumps.push(RelJump {
                    base: skip_target.wrapping_sub(*skip_delta as usize),
                    target: *skip_target,
                }),
                InventoryCmpKind::AbsJump { .. } => {
                    return Err(ManEditError::AbsoluteRef {
                        op_pc,
                        ref_pc: insn.pc,
                    });
                }
                _ => {}
            },
            InsnInfo::Camera {
                kind: CameraKind::Apply { .. },
                ..
            } => {
                return Err(ManEditError::AbsoluteRef {
                    op_pc,
                    ref_pc: insn.pc,
                });
            }
            _ => {}
        }
        pc += insn.size;
    }
    Ok(jumps)
}

/// One `0x3F` named-scene-change ("door") site located in a decompressed MAN by
/// the clean partition walk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SceneChangeSite {
    /// Absolute offset of the `0x3F` opcode in the decompressed MAN.
    pub op_pc: usize,
    /// Partition (0/1/2) of the record carrying it (almost always 2).
    pub partition: usize,
    /// Destination-scene `i16` index (the id passed to the warp packet).
    pub index: i16,
    /// Destination CDNAME scene label (clean-gated; e.g. `"map01"`).
    pub name: String,
    /// Destination entry-tile X byte.
    pub entry_x: u8,
    /// Destination entry-tile Z byte.
    pub entry_z: u8,
    /// Facing/depth selector byte.
    pub dir: u8,
}

/// Enumerate every `0x3F` named-scene-change site in a decompressed MAN by
/// walking each partition record from its true `pc0` with a **clean
/// fall-through** decode (stop at the first desync - the rest is data). This is
/// the correct door enumeration: the destinations are partition-2 records, so a
/// partition-1 recovering walk mis-attributes them. Only ops whose inline name
/// passes the clean-CDNAME-label gate are returned; sites are unique by `op_pc`.
pub fn scene_change_sites(man: &[u8]) -> Vec<SceneChangeSite> {
    let Ok(mf) = man_section::parse(man) else {
        return Vec::new();
    };
    let starts = record_starts(&mf);
    let dro = mf.data_region_offset;
    let mut out: Vec<SceneChangeSite> = Vec::new();
    for (p, part) in mf.partitions.iter().enumerate() {
        for &off in part {
            let start = dro + off as usize;
            if start >= man.len() {
                continue;
            }
            let pc0 = if p == 2 {
                match p2_pc0(man, start) {
                    Some(v) => v,
                    None => continue,
                }
            } else {
                let locals = *man.get(start).unwrap_or(&0) as usize;
                1 + locals * 2 + 4
            };
            let end = starts
                .iter()
                .copied()
                .find(|&s| s > start)
                .unwrap_or(man.len());
            if start + pc0 >= end {
                continue;
            }
            let mut pc = start + pc0;
            while pc < end {
                let Ok(insn) = field_disasm::decode(man, pc) else {
                    break;
                };
                if insn.size == 0 || insn.pc >= end {
                    break;
                }
                if let InsnInfo::SceneChange {
                    index,
                    entry_x,
                    entry_z,
                    dir,
                    ..
                } = insn.info
                    && let Some(name) = field_disasm::scene_change_name(man, &insn)
                    && !out.iter().any(|s| s.op_pc == insn.pc)
                {
                    out.push(SceneChangeSite {
                        op_pc: insn.pc,
                        partition: p,
                        index,
                        name,
                        entry_x,
                        entry_z,
                        dir,
                    });
                }
                pc += insn.size;
            }
        }
    }
    out.sort_by_key(|s| s.op_pc);
    out
}

/// One field-VM `0x23 MOVE_TO` ("teleport to grid tile") op located in a
/// decompressed MAN. Intra-town doors are these: a script repositions the
/// player to an interior sub-area tile. Distinguishing door warps from NPC /
/// cutscene movement is the caller's job (see `crate::house_door` in
/// `legaia-rando`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MoveToSite {
    /// Absolute offset of the `0x23` opcode in the decompressed MAN.
    pub op_pc: usize,
    /// Partition (0/1/2) of the carrying record.
    pub partition: usize,
    /// Record index within the partition.
    pub record: usize,
    /// First operand byte (X tile encoding: `tile = b & 0x7F`,
    /// `+half = b & 0x80`).
    pub xb: u8,
    /// Second operand byte (Z tile encoding, same shape as `xb`).
    pub zb: u8,
}

impl MoveToSite {
    /// Decoded destination tile `(x, z)` (the `& 0x7F` of each operand).
    pub fn tile(&self) -> (u8, u8) {
        (self.xb & 0x7F, self.zb & 0x7F)
    }
}

/// Enumerate every `0x23 MOVE_TO` site in a decompressed MAN via the same clean
/// partition-walk as [`scene_change_sites`]. Sites are unique by `op_pc`,
/// sorted. (The `(0x7F, 0x7F)` sentinel "here" target appears throughout and is
/// not a door - callers filter it.)
pub fn move_to_sites(man: &[u8]) -> Vec<MoveToSite> {
    let Ok(mf) = man_section::parse(man) else {
        return Vec::new();
    };
    let starts = record_starts(&mf);
    let dro = mf.data_region_offset;
    let mut out: Vec<MoveToSite> = Vec::new();
    for (p, part) in mf.partitions.iter().enumerate() {
        for (ri, &off) in part.iter().enumerate() {
            let start = dro + off as usize;
            if start >= man.len() {
                continue;
            }
            let pc0 = if p == 2 {
                match p2_pc0(man, start) {
                    Some(v) => v,
                    None => continue,
                }
            } else {
                let locals = *man.get(start).unwrap_or(&0) as usize;
                1 + locals * 2 + 4
            };
            let end = starts
                .iter()
                .copied()
                .find(|&s| s > start)
                .unwrap_or(man.len());
            if start + pc0 >= end {
                continue;
            }
            let mut pc = start + pc0;
            while pc < end {
                let Ok(insn) = field_disasm::decode(man, pc) else {
                    break;
                };
                if insn.size == 0 || insn.pc >= end {
                    break;
                }
                if let InsnInfo::MoveTo { xb, zb } = insn.info
                    && !out.iter().any(|s| s.op_pc == insn.pc)
                {
                    out.push(MoveToSite {
                        op_pc: insn.pc,
                        partition: p,
                        record: ri,
                        xb,
                        zb,
                    });
                }
                pc += insn.size;
            }
        }
    }
    out.sort_by_key(|s| s.op_pc);
    out
}

/// Internal: one resolved edit in old-buffer coordinates.
struct Splice {
    /// Start of the operand block being replaced (`op_pc + header_size`).
    block_start: usize,
    /// Length of the old operand block (`6 + old_name_len`).
    old_len: usize,
    /// New operand bytes (`6 + new_name_len`).
    new_bytes: Vec<u8>,
    /// Byte delta (`new_len - old_len`).
    delta: i64,
    /// First old offset that shifts (`block_start + old_len`).
    tail: usize,
}

/// Apply destination-name edits to a decompressed MAN, returning the rebuilt
/// buffer. See the module docs for the relocation it performs. Does **not**
/// touch the external descriptor size - the caller rewrites that after
/// recompressing (and should call [`validate`] on the result).
pub fn apply_dest_edits(man: &[u8], edits: &[DestEdit]) -> Result<Vec<u8>, ManEditError> {
    let mf = man_section::parse(man).map_err(|_| ManEditError::Parse)?;

    // Resolve each edit to a splice + collect intra-record jump fixups.
    let mut splices: Vec<Splice> = Vec::new();
    let mut jump_fixups: Vec<RelJump> = Vec::new();
    for e in edits {
        if e.name.is_empty() || e.name.len() > u8::MAX as usize {
            return Err(ManEditError::BadName { len: e.name.len() });
        }
        let insn = field_disasm::decode(man, e.op_pc)
            .map_err(|_| ManEditError::NotSceneChange { op_pc: e.op_pc })?;
        let InsnInfo::SceneChange { name_len, .. } = insn.info else {
            return Err(ManEditError::NotSceneChange { op_pc: e.op_pc });
        };
        let hs = if insn.extended.is_some() { 2 } else { 1 };
        let block_start = e.op_pc + hs;
        let old_len = 6 + name_len as usize;

        let mut new_bytes = Vec::with_capacity(6 + e.name.len());
        new_bytes.extend_from_slice(&e.index.to_le_bytes());
        new_bytes.push(e.name.len() as u8);
        new_bytes.extend_from_slice(&e.name);
        new_bytes.push(e.entry_x);
        new_bytes.push(e.entry_z);
        new_bytes.push(e.dir);
        debug_assert_eq!(new_bytes.len(), 6 + e.name.len());

        let (rstart, pc0, rend) =
            record_for(&mf, man, e.op_pc).ok_or(ManEditError::RecordNotFound { op_pc: e.op_pc })?;
        jump_fixups.extend(scan_record_refs(man, rstart, pc0, rend, e.op_pc)?);

        splices.push(Splice {
            block_start,
            old_len,
            delta: e.name.len() as i64 - name_len as i64,
            tail: block_start + old_len,
            new_bytes,
        });
    }

    rebuild_man(man, &mf, splices, &jump_fixups)
}

/// Rebuild a decompressed MAN applying `splices` (each replaces `old_len` bytes at
/// `block_start` with `new_bytes`, byte delta `delta`) and fixing the three classes
/// of internal offset: the partition record-offset tables, the section-0 offset
/// (`u24_at_28`), and the intra-record relative-jump deltas in `jump_fixups`. The
/// relocation is identical whether the splices come from destination-name resizes
/// ([`apply_dest_edits`]) or raw byte insertions ([`apply_insertions`]). Errors on
/// overlapping splices.
fn rebuild_man(
    man: &[u8],
    mf: &ManFile,
    mut splices: Vec<Splice>,
    jump_fixups: &[RelJump],
) -> Result<Vec<u8>, ManEditError> {
    splices.sort_by_key(|s| s.block_start);
    for w in splices.windows(2) {
        if w[0].block_start + w[0].old_len > w[1].block_start {
            return Err(ManEditError::OverlappingEdits);
        }
    }

    // Cumulative byte shift applied to an old offset `x`: sum of deltas of every
    // splice whose tail is <= x.
    let map_off = |x: usize| -> usize {
        let shift: i64 = splices
            .iter()
            .filter(|s| s.tail <= x)
            .map(|s| s.delta)
            .sum();
        (x as i64 + shift) as usize
    };

    // ---- build the rebuilt buffer ----
    let total_delta: i64 = splices.iter().map(|s| s.delta).sum();
    let new_len = (man.len() as i64 + total_delta) as usize;
    let mut out = Vec::with_capacity(new_len);
    let mut cur = 0usize;
    for s in &splices {
        out.extend_from_slice(&man[cur..s.block_start]);
        out.extend_from_slice(&s.new_bytes);
        cur = s.block_start + s.old_len;
    }
    out.extend_from_slice(&man[cur..]);
    debug_assert_eq!(out.len(), new_len);

    // ---- fixup #1: partition record-offset tables ----
    // Each entry is a u24LE *data-region-relative* offset. Bump it by the shift
    // applied to its absolute record start. Table sits in the header region
    // (before data_region), so its byte position is unchanged.
    let dro = mf.data_region_offset;
    let mut cursor = RECORDS_BEGIN_OFFSET;
    for part in &mf.partitions {
        for &off in part {
            let abs = dro + off as usize;
            let new_off = (map_off(abs) as i64 - dro as i64) as u32;
            write_u24_le(&mut out, cursor, new_off);
            cursor += 3;
        }
    }

    // ---- fixup #2: u24_at_28 (section-0 offset, data-region-relative) ----
    let sec0_abs = dro + mf.header.u24_at_28 as usize;
    let new_sec0 = (map_off(sec0_abs) as i64 - dro as i64) as u32;
    write_u24_le(&mut out, U24_AT_28_OFFSET, new_sec0);

    // ---- fixup #3: intra-record relative-jump deltas ----
    // Only jumps whose endpoints straddle a splice change; the rest recompute to
    // the same delta. The delta field lives at the jump's relative `base`.
    for j in jump_fixups {
        let new_base = map_off(j.base);
        let new_target = map_off(j.target);
        let new_delta = (new_target as i64 - new_base as i64) as u16;
        write_u16_le(&mut out, new_base, new_delta);
    }

    Ok(out)
}

/// One byte-block insertion into a decompressed MAN: splice `bytes` in **before**
/// the instruction currently at `offset` (an instruction boundary inside a record's
/// script body), growing that record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Insertion {
    /// Absolute offset in the decompressed MAN to insert before. Must be at/after
    /// the containing record's first opcode (`pc0`) and at/before its end.
    pub offset: usize,
    /// The bytecode to splice in. Assumed position-independent at `offset` (its own
    /// relative jumps are self-contained); the caller emits such a block (see
    /// `legaia_rando::starting_bag`).
    pub bytes: Vec<u8>,
}

/// Insert byte blocks into a decompressed MAN, returning the rebuilt buffer with the
/// partition table / section offset / intra-record jump deltas relocated (the same
/// fixups as [`apply_dest_edits`], via [`rebuild_man`]).
///
/// Each insertion must sit at an instruction boundary inside a record's script body,
/// and that record must contain no absolute reference (`0x4E` abs-jump, `0x45 0xC0`
/// camera-apply, inventory abs-jump) - those store an absolute target that a shift
/// would invalidate, so the call errors [`ManEditError::AbsoluteRef`] and the caller
/// leaves the scene unchanged (relative jumps shift with their record and are
/// preserved). The caller rewrites the external descriptor size after recompressing
/// and should run [`validate`] / re-walk on the result.
pub fn apply_insertions(man: &[u8], insertions: &[Insertion]) -> Result<Vec<u8>, ManEditError> {
    let mf = man_section::parse(man).map_err(|_| ManEditError::Parse)?;
    let mut splices: Vec<Splice> = Vec::new();
    let mut jump_fixups: Vec<RelJump> = Vec::new();
    for ins in insertions {
        if ins.bytes.is_empty() {
            continue;
        }
        let (rstart, pc0, rend) = record_for(&mf, man, ins.offset)
            .ok_or(ManEditError::RecordNotFound { op_pc: ins.offset })?;
        if ins.offset < rstart + pc0 || ins.offset > rend {
            return Err(ManEditError::RecordNotFound { op_pc: ins.offset });
        }
        // Reuse the record scan to reject absolute refs + collect relative jumps
        // (which a uniform same-record shift leaves with identical deltas, but the
        // fixup re-emits them correctly regardless).
        jump_fixups.extend(scan_record_refs(man, rstart, pc0, rend, ins.offset)?);
        splices.push(Splice {
            block_start: ins.offset,
            old_len: 0,
            delta: ins.bytes.len() as i64,
            tail: ins.offset,
            new_bytes: ins.bytes.clone(),
        });
    }
    if splices.is_empty() {
        return Ok(man.to_vec());
    }
    rebuild_man(man, &mf, splices, &jump_fixups)
}

/// Re-parse the rebuilt MAN and confirm it walks cleanly: the structure parses
/// and each `op_pc` (mapped through the edits) now decodes as a `0x3F`
/// scene-change carrying the intended name. A final backstop the caller uses to
/// skip a scene whose rebuild somehow corrupted the layout.
pub fn validate(rebuilt: &[u8], expected: &[(usize, &[u8])]) -> bool {
    if man_section::parse(rebuilt).is_err() {
        return false;
    }
    for &(op_pc, name) in expected {
        let Ok(insn) = field_disasm::decode(rebuilt, op_pc) else {
            return false;
        };
        if !matches!(insn.info, InsnInfo::SceneChange { .. }) {
            return false;
        }
        match field_disasm::scene_change_name(rebuilt, &insn) {
            Some(n) if n.as_bytes() == name => {}
            _ => return false,
        }
    }
    true
}

/// Map an old MAN offset to its new offset after the given edits (so a caller
/// can compute where an op moved to for [`validate`]).
pub fn map_offset_after(edits: &[DestEdit], man: &[u8], x: usize) -> usize {
    // Reconstruct the per-edit (tail, delta) without re-walking jumps.
    let Ok(mf) = man_section::parse(man) else {
        return x;
    };
    let mut shift = 0i64;
    for e in edits {
        let Ok(insn) = field_disasm::decode(man, e.op_pc) else {
            continue;
        };
        let InsnInfo::SceneChange { name_len, .. } = insn.info else {
            continue;
        };
        let hs = if insn.extended.is_some() { 2 } else { 1 };
        let tail = e.op_pc + hs + 6 + name_len as usize;
        if tail <= x {
            shift += e.name.len() as i64 - name_len as i64;
        }
    }
    let _ = mf;
    (x as i64 + shift) as usize
}

fn write_u24_le(buf: &mut [u8], at: usize, v: u32) {
    buf[at] = (v & 0xFF) as u8;
    buf[at + 1] = ((v >> 8) & 0xFF) as u8;
    buf[at + 2] = ((v >> 16) & 0xFF) as u8;
}

fn write_u16_le(buf: &mut [u8], at: usize, v: u16) {
    buf[at] = (v & 0xFF) as u8;
    buf[at + 1] = ((v >> 8) & 0xFF) as u8;
}

#[cfg(test)]
mod tests;
