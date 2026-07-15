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
            InsnInfo::InventoryCmp {
                kind:
                    InventoryCmpKind::Compare {
                        skip_delta,
                        skip_target,
                        ..
                    }
                    | InventoryCmpKind::PartyBank {
                        skip_delta,
                        skip_target,
                        ..
                    },
                ..
            } => jumps.push(RelJump {
                base: skip_target.wrapping_sub(*skip_delta as usize),
                target: *skip_target,
            }),
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

/// One inline scene destination decoded from a `0x3F` named-scene-change op:
/// the disc-sourced answer to "which scenes can this scene warp to". Deduped
/// by `(scene_name, index)`. Field-for-field the same shape as the engine's
/// `man_field_scripts::SceneDestination`, so the engine-side scan can
/// delegate here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SceneDestination {
    /// Destination CDNAME scene label (e.g. `"town0c"`, `"jouina"`).
    pub scene_name: String,
    /// The `i16` index operand the op carries (a story/entry id; *not* the
    /// `0x3E` door-warp `map_id` - distinct id space, observed past 100).
    pub index: i16,
    /// Entry tile X byte at the destination (`& 0x7F` tile, `& 0x80` half-tile).
    pub entry_x: u8,
    /// Entry tile Z byte at the destination (same encoding as `entry_x`).
    pub entry_z: u8,
}

/// The **partition-1 table pass**: recover the `0x3F` destinations reachable
/// from a scene's partition-1 scripts plus the trailing destination-table
/// blob some controllers append *after* their last partition-1 record.
///
/// The blob sits past the tight per-record ceiling (in `map01` it trails the
/// last partition-1 record, well past the next-section bound), so each record
/// is bounded by the **next partition-1 record start** (man-end for the last
/// record) and walked with the *recovering* [`field_disasm::LinearWalker`].
/// The clean-name gate + `(name, index)` dedup absorb the over-walk: a record
/// viewed from an earlier start re-sees the same ops, and desync junk past
/// the table fails the gate.
///
/// This pass **under-reports doors carried only by partition-2 records**: the
/// recovering over-walk can be desynced when it crosses a P2 record's
/// SJIS-name header, so a P2-only `0x3F` is missed. The retail class is the
/// town/dungeon **exit door** (a P2 door-choreography record) - `town01`'s
/// exit to the `map01` overworld, `retockin`→`retona`, `geremi`→`map02` /
/// `tower`. Use [`scene_destinations`] for the full (P1 ∪ P2) set; this pass
/// stays public so the asymmetry is pinnable.
pub fn partition1_destinations(man_file: &ManFile, man: &[u8]) -> Vec<SceneDestination> {
    let n1 = man_file.header.partition_counts[1].max(0) as usize;
    let mut starts: Vec<usize> = (0..n1)
        .filter_map(|i| man_file.actor_placement_record_offset(i, man.len()))
        .collect();
    starts.sort_unstable();
    let mut out: Vec<SceneDestination> = Vec::new();
    for (k, &start) in starts.iter().enumerate() {
        let end = starts.get(k + 1).copied().unwrap_or(man.len());
        let pc0 = {
            let locals = *man.get(start).unwrap_or(&0) as usize;
            1 + locals * 2 + 4
        };
        if start + pc0 >= end {
            continue;
        }
        let body = &man[start..end];
        for insn in field_disasm::LinearWalker::new(body, pc0).flatten() {
            let InsnInfo::SceneChange {
                index,
                entry_x,
                entry_z,
                ..
            } = insn.info
            else {
                continue;
            };
            let Some(scene_name) = field_disasm::scene_change_name(body, &insn) else {
                continue;
            };
            if out
                .iter()
                .any(|d| d.index == index && d.scene_name == scene_name)
            {
                continue;
            }
            out.push(SceneDestination {
                scene_name,
                index,
                entry_x,
                entry_z,
            });
        }
    }
    out
}

/// Recover the full inline scene-destination set from a scene's MAN:
/// the [`partition1_destinations`] table pass **plus a partition-2 pass**
/// that walks each P2 record from its true `pc0` ([`p2_pc0`]'s SJIS-name +
/// condition-list header) with a clean fall-through decode - the same walk
/// as [`scene_change_sites`], which is what sees the P2-only doors the P1
/// over-walk desyncs past (the town/dungeon exit doors: `town01`→`map01`,
/// `retockin`→`retona`, …). Results are merged in first-seen order (P1 pass
/// first, then P2 additions), deduped by `(scene_name, index)`, so the
/// output is a superset of the P1 pass and existing consumers see the same
/// destinations plus the previously missing ones.
pub fn scene_destinations(man_file: &ManFile, man: &[u8]) -> Vec<SceneDestination> {
    let mut out = partition1_destinations(man_file, man);
    let starts = record_starts(man_file);
    let dro = man_file.data_region_offset;
    for &off in &man_file.partitions[2] {
        let start = dro + off as usize;
        if start >= man.len() {
            continue;
        }
        let Some(pc0) = p2_pc0(man, start) else {
            continue;
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
                ..
            } = insn.info
                && let Some(scene_name) = field_disasm::scene_change_name(man, &insn)
                && !out
                    .iter()
                    .any(|d| d.index == index && d.scene_name == scene_name)
            {
                out.push(SceneDestination {
                    scene_name,
                    index,
                    entry_x,
                    entry_z,
                });
            }
            pc += insn.size;
        }
    }
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

/// One text-span rewrite inside a decompressed MAN's **record script** region:
/// replace the `old_len` bytes at `offset` with `new_bytes` (any length). This
/// is the generalization of [`DestEdit`] from a door's destination name to an
/// arbitrary interior run - the dialog-segment text a translation grows or
/// shrinks. `offset` addresses the first byte of the run (for a `0x1F`-lead
/// dialog segment, the byte after the `0x1F`); the `0x1F` lead and `0x00`
/// terminator stay put and bound the run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextEdit {
    /// First byte of the run being replaced (absolute, decompressed-MAN coords).
    pub offset: usize,
    /// Length of the run being replaced.
    pub old_len: usize,
    /// Replacement bytes (the new run; may be longer or shorter than `old_len`).
    pub new_bytes: Vec<u8>,
}

/// Apply a set of interior text-run edits to a decompressed MAN, returning the
/// rebuilt buffer with every crossing internal reference relocated (partition
/// record-offset tables, `u24_at_28`, intra-record relative jumps - the same
/// [`rebuild_man`] machinery the door editor uses). This is the generalized
/// dialog rewriter: it lets a localized line grow past its USA byte span while
/// the surrounding script stays valid, so the budget becomes the MAN's own
/// footprint rather than each string's fixed span.
///
/// Every edit must land in the **record region** (before section 0), inside a
/// partition record's script body; an edit that touches the section chain, or a
/// record carrying an absolute-reference op (`0x4E` abs-jump / `0x45 0xC0`
/// camera-apply), is refused (a byte shift can't safely relocate those). The
/// caller recompresses the result, rewrites the external descriptor size word
/// ([`crate::scene_asset_table::encode_size_word`]), and should confirm the
/// rewrite with [`text_edits_preserve_scripts`] as a round-trip backstop.
pub fn apply_text_edits(man: &[u8], edits: &[TextEdit]) -> Result<Vec<u8>, ManEditError> {
    use std::collections::BTreeSet;
    let mf = man_section::parse(man).map_err(|_| ManEditError::Parse)?;
    // Section 0 begins right after the record region; every editable run must
    // lie strictly before it (dialog is field-VM script = partition records,
    // never a data section, whose length prefixes this pass does not fix).
    let sec0_abs = mf.data_region_offset + mf.header.u24_at_28 as usize;

    let mut splices: Vec<Splice> = Vec::new();
    let mut jump_fixups: Vec<RelJump> = Vec::new();
    let mut scanned: BTreeSet<usize> = BTreeSet::new();
    for e in edits {
        if e.offset + e.old_len > sec0_abs {
            // Touches (or crosses into) the section chain - out of scope.
            return Err(ManEditError::RecordNotFound { op_pc: e.offset });
        }
        let (rstart, pc0, rend) = record_for(&mf, man, e.offset)
            .ok_or(ManEditError::RecordNotFound { op_pc: e.offset })?;
        // `record_for` also indexes section starts; a record-region offset must
        // resolve to a partition record (its start is before section 0).
        if rstart >= sec0_abs || e.offset < rstart + pc0 || e.offset + e.old_len > rend {
            return Err(ManEditError::RecordNotFound { op_pc: e.offset });
        }
        // Scan each edited record's control flow exactly once (rejects abs refs).
        if scanned.insert(rstart) {
            jump_fixups.extend(scan_record_refs(man, rstart, pc0, rend, e.offset)?);
        }
        splices.push(Splice {
            block_start: e.offset,
            old_len: e.old_len,
            delta: e.new_bytes.len() as i64 - e.old_len as i64,
            tail: e.offset + e.old_len,
            new_bytes: e.new_bytes.clone(),
        });
    }
    if splices.is_empty() {
        return Ok(man.to_vec());
    }
    rebuild_man(man, &mf, splices, &jump_fixups)
}

/// Absolute control-flow targets an instruction references (relative jumps +
/// the absolute camera-apply / abs-jump). Empty for straight-line ops.
fn control_targets(insn: &InsnInfo) -> Vec<usize> {
    match insn {
        InsnInfo::JmpRel { target, .. } | InsnInfo::CondJmp { target, .. } => vec![*target],
        InsnInfo::BBoxTest { skip_target, .. } => vec![*skip_target],
        InsnInfo::SystemFlag {
            target: Some(t), ..
        } => vec![*t],
        InsnInfo::InventoryCmp {
            kind:
                InventoryCmpKind::Compare { skip_target, .. }
                | InventoryCmpKind::PartyBank { skip_target, .. },
            ..
        } => vec![*skip_target],
        InsnInfo::Camera {
            kind: CameraKind::Apply { abs_target },
            ..
        } => vec![*abs_target],
        _ => vec![],
    }
}

/// Per-record normalized instruction signature: for each instruction in a clean
/// fall-through walk of `[start+pc0, end)`, its opcode and the **ordinals**
/// (indices within this walk) of its control-flow targets, or `None` for a
/// target that doesn't land on a walked instruction boundary.
fn record_signature(
    man: &[u8],
    start: usize,
    pc0: usize,
    end: usize,
) -> Vec<(u8, Vec<Option<usize>>)> {
    let mut insns: Vec<field_disasm::Insn> = Vec::new();
    let mut pc = start + pc0;
    while pc < end {
        let Ok(insn) = field_disasm::decode(man, pc) else {
            break;
        };
        if insn.size == 0 || insn.pc >= end {
            break;
        }
        pc += insn.size;
        insns.push(insn);
    }
    let ordinal_of = |target: usize| insns.iter().position(|i| i.pc == target);
    insns
        .iter()
        .map(|i| {
            (
                i.opcode,
                control_targets(&i.info)
                    .iter()
                    .map(|&t| ordinal_of(t))
                    .collect(),
            )
        })
        .collect()
}

/// Round-trip backstop for [`apply_text_edits`]: confirm the `rebuilt` MAN is
/// the **same program** as `original`, relocated. It re-parses, and for every
/// partition record walks both buffers from the record's `pc0` and requires an
/// identical instruction stream - same opcodes in order, and every control-flow
/// target resolving to the same instruction ordinal. A mis-relocated jump (a
/// straddling delta the fixup missed) diverges the ordinal and is caught; a
/// target that no longer lands on an instruction boundary reads as `None` on
/// one side. Returns `false` on any divergence, so the caller can drop the
/// growth and fall back to same-size abbreviation for that scene.
pub fn text_edits_preserve_scripts(original: &[u8], rebuilt: &[u8]) -> bool {
    let (Ok(a), Ok(b)) = (man_section::parse(original), man_section::parse(rebuilt)) else {
        return false;
    };
    if a.header.partition_counts != b.header.partition_counts {
        return false;
    }
    let starts_a = record_starts(&a);
    let starts_b = record_starts(&b);
    let dro_a = a.data_region_offset;
    let dro_b = b.data_region_offset;
    for (p, part) in a.partitions.iter().enumerate() {
        for (ri, &off_a) in part.iter().enumerate() {
            let start_a = dro_a + off_a as usize;
            let start_b = dro_b + b.partitions[p][ri] as usize;
            if start_a >= original.len() || start_b >= rebuilt.len() {
                return false;
            }
            let pc0 = |man: &[u8], start: usize| -> Option<usize> {
                if p == 2 {
                    p2_pc0(man, start)
                } else {
                    man.get(start).map(|&l| 1 + l as usize * 2 + 4)
                }
            };
            let (Some(pc0a), Some(pc0b)) = (pc0(original, start_a), pc0(rebuilt, start_b)) else {
                return false;
            };
            let end_a = starts_a
                .iter()
                .copied()
                .find(|&s| s > start_a)
                .unwrap_or(original.len());
            let end_b = starts_b
                .iter()
                .copied()
                .find(|&s| s > start_b)
                .unwrap_or(rebuilt.len());
            if record_signature(original, start_a, pc0a, end_a)
                != record_signature(rebuilt, start_b, pc0b, end_b)
            {
                return false;
            }
        }
    }
    true
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
