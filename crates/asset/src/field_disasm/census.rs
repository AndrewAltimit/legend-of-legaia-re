//! Disc-wide **opcode census** over field-VM bytecode.
//!
//! The question this answers is "does any shipped scene carry op X", for an
//! *arbitrary* opcode - including the sub-dispatched arms of `0x4C`
//! (`MENU_CTRL`), `0x43`, `0x45`, `0x49` and `0x34`, which is where the rare
//! arms live. The three censuses that came before it
//! (`--system-flag-census`, `--motion-flag-census`, `--op49-window-census`)
//! each report one *family*, so an arm outside those families had no
//! instrument at all.
//!
//! ## Why not a byte scan
//!
//! A raw byte scan for `4C CF` is not a substitute and never will be: a
//! field-VM record embeds Shift-JIS message text, and every opcode byte value
//! occurs in prose. The census therefore decodes: it walks each record from
//! its **first-opcode offset** with [`LinearWalker`] and keys the tally on a
//! decoded instruction boundary.
//!
//! ## Coherence: `clean` versus `total`
//!
//! The walk is an over-approximating linear disassembly. Once it hits a
//! decode error inside a record - a truncated operand, an unsized sub-op, or a
//! byte that is not an opcode at all - every later boundary in that record is
//! a guess, because the walk resumes one byte on and may be off by any amount.
//! So each tally is kept twice:
//!
//! * `total` counts every decoded occurrence;
//! * `clean` counts only occurrences decoded **before** that record's first
//!   decode error.
//!
//! A `clean` count is the defensible number. A `total`-only hit is a lead,
//! not a carrier.
//!
//! ## What a count of zero means
//!
//! Zero `clean` occurrences disc-wide means **no shipped scene reaches that
//! arm through the field VM's own bytecode**. It does not mean the handler is
//! dead: an arm can also be entered from a cross-context dispatch whose
//! carrier the walk mis-sizes, from a `.PCH` prescript this census does cover,
//! or - the real residue - from an event-script record whose record table the
//! carrier walk does not enumerate. What zero *does* settle is the question a
//! replay fixture asks: there is no scene to drive, so a "no ladder drives it"
//! runtime-reach row is not waiting on a fixture.

use std::collections::{BTreeMap, BTreeSet};

use super::{DisasmError, Insn, InsnInfo, LinearWalker};
use crate::man_section::{ManFile, RECORD_PARTITIONS};

/// A census key: a top-level opcode plus, for the sub-dispatched opcodes, the
/// sub-selector byte the dispatcher switches on.
///
/// For `0x4C` the sub is the **whole** `op0` byte (outer nibble in the high
/// half, sub-arm in the low half), because that is how the retail dispatcher
/// indexes: `FUN_801DE840` reads `op0`, selects one of sixteen outer
/// dispatchers through the jump table at `0x801CEE60`, and each of those
/// indexes its own sixteen-entry table by `op0 & 0x0F`. So `[4C CF]` is outer
/// nibble `0xC`, sub-arm `0xF`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct OpKey {
    /// Top-level opcode byte, with the `0x80` cross-context prefix cleared.
    pub opcode: u8,
    /// Sub-selector byte, for the sub-dispatched opcodes only.
    pub sub: Option<u8>,
}

impl OpKey {
    /// `"4C CF"` / `"39"` - the form the reach rows and the threads cite.
    pub fn label(&self) -> String {
        match self.sub {
            Some(s) => format!("{:02X} {:02X}", self.opcode, s),
            None => format!("{:02X}", self.opcode),
        }
    }
}

impl std::fmt::Display for OpKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.label())
    }
}

/// The census key of one decoded instruction.
///
/// Sub-dispatched opcodes are broken out; everything else keys on the opcode
/// alone. The sub value comes from the decoded payload rather than from a
/// re-read of the raw byte, so it is the byte the dispatcher would switch on
/// even where the decoder normalises (`0x4C` nibble-5/6/7 arms carry typed
/// payloads, and `op0` is preserved alongside).
pub fn op_key(insn: &Insn) -> OpKey {
    let sub = match &insn.info {
        InsnInfo::MenuCtrl { op0, .. } => Some(*op0),
        InsnInfo::ActorCtrl { sub_op, .. } => Some(*sub_op),
        InsnInfo::Camera { op0, .. } => Some(*op0),
        InsnInfo::Effect { op0, .. } => Some(*op0),
        InsnInfo::StateResume { sub_op, .. } => Some(*sub_op),
        _ => None,
    };
    OpKey {
        opcode: insn.opcode,
        sub,
    }
}

/// One script body's provenance, so a hit can be traced back to bytes.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct ScriptSite {
    /// Which carrier the body came from - a MAN partition record, an event
    /// script record, or a `.PCH` prescript.
    pub carrier: ScriptCarrier,
    /// Record index inside that carrier.
    pub record: usize,
}

/// The kinds of field-VM bytecode carrier the census walks.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ScriptCarrier {
    /// A MAN record partition (`0` objects, `1` actor placements,
    /// `2` cutscene-timeline named records).
    ManPartition(usize),
    /// A raw event-script carrier record (`scene_event_scripts`).
    EventScript,
    /// A `.PCH` walk-on prescript record.
    Prescript,
}

impl std::fmt::Display for ScriptCarrier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ScriptCarrier::ManPartition(p) => write!(f, "man.p{p}"),
            ScriptCarrier::EventScript => f.write_str("event"),
            ScriptCarrier::Prescript => f.write_str("pch"),
        }
    }
}

/// Per-opcode tallies plus the walk's own health figures.
#[derive(Clone, Debug, Default)]
pub struct OpCensus {
    /// Occurrences decoded before the carrying record's first decode error.
    pub clean: BTreeMap<OpKey, usize>,
    /// Every decoded occurrence, coherent or not.
    pub total: BTreeMap<OpKey, usize>,
    /// Record bodies walked.
    pub records: usize,
    /// Record bodies that desynced at least once.
    pub desynced_records: usize,
    /// Decode errors across every record.
    pub decode_errors: usize,
    /// Script bytes walked (the sum of the bounded bodies, from `pc0` on).
    pub script_bytes: usize,
}

impl OpCensus {
    /// Walk one bounded record body starting at `pc0` and fold it in.
    ///
    /// `body` is the record's bounded span (so the walk cannot spill into the
    /// next record); `pc0` is the first-opcode offset inside it.
    pub fn tally_record(&mut self, body: &[u8], pc0: usize) {
        self.records += 1;
        self.script_bytes += body.len().saturating_sub(pc0);
        let mut desynced = false;
        for step in LinearWalker::new(body, pc0) {
            match step {
                Ok(insn) => {
                    let key = op_key(&insn);
                    *self.total.entry(key).or_insert(0) += 1;
                    if !desynced {
                        *self.clean.entry(key).or_insert(0) += 1;
                    }
                }
                Err((_pc, err)) => {
                    // `EndOfStream` is the walk finishing, not a desync.
                    if !matches!(err, DisasmError::EndOfStream { .. }) {
                        self.decode_errors += 1;
                        if !desynced {
                            self.desynced_records += 1;
                        }
                        desynced = true;
                    }
                }
            }
        }
    }

    /// Fold another census into this one.
    pub fn merge(&mut self, other: &OpCensus) {
        for (k, v) in &other.clean {
            *self.clean.entry(*k).or_insert(0) += v;
        }
        for (k, v) in &other.total {
            *self.total.entry(*k).or_insert(0) += v;
        }
        self.records += other.records;
        self.desynced_records += other.desynced_records;
        self.decode_errors += other.decode_errors;
        self.script_bytes += other.script_bytes;
    }

    /// `clean` count for one key (0 when absent).
    pub fn clean_count(&self, key: OpKey) -> usize {
        self.clean.get(&key).copied().unwrap_or(0)
    }

    /// Every key that occurs at all, in key order.
    pub fn keys(&self) -> BTreeSet<OpKey> {
        self.clean
            .keys()
            .chain(self.total.keys())
            .copied()
            .collect()
    }
}

/// The byte span of one MAN partition record as a field-VM script:
/// `(start, pc0, len)` - absolute MAN offset, first-opcode offset relative to
/// it, and the bounded body length.
///
/// The header shape is **partition-specific** and all three differ, which is
/// why a single formula silently drops a whole record class from a census:
///
/// * partition 0 (object records - doors, chests, signs; bound by the `.MAP`
///   tile triggers `FUN_8003A55C` reads) opens
///   `[u8 n][n*2 SJIS name][u8 attr]`, so `pc0 = 1 + n*2 + 1`;
/// * partition 1 (actor placements) opens `[u8 N][N*2 locals][4-byte
///   placement header]`, so `pc0 = 1 + N*2 + 4`;
/// * partition 2 (cutscene-timeline named records) opens the name field plus
///   three condition blocks the record dispatcher `FUN_8003BDE0` walks -
///   see [`partition2_script_offset`].
///
/// Returns `None` when the partition / index is out of range, the record
/// offset lands past the buffer, or the header already overruns its bound.
// REF: FUN_8003BDE0
pub fn partition_record_span(
    man_file: &ManFile,
    man: &[u8],
    partition: usize,
    index: usize,
) -> Option<(usize, usize, usize)> {
    let off = *man_file.partitions.get(partition)?.get(index)? as usize;
    let start = man_file.data_region_offset.checked_add(off)?;
    if start >= man.len() {
        return None;
    }
    let end = record_end_bound(man_file, man.len(), start);
    let body = man.get(start..end)?;
    let n = *body.first().unwrap_or(&0) as usize;
    let pc0 = match partition {
        0 => 1 + n * 2 + 1,
        1 => 1 + n * 2 + 4,
        _ => partition2_script_offset(body)?,
    };
    if start + pc0 >= end {
        return None;
    }
    Some((start, pc0, end - start))
}

/// First-opcode offset of a **partition-2 named record**, relative to the
/// record start.
///
/// The header is `[u8 name_len][name_len*2 SJIS]` then three condition blocks
/// the dispatcher skips: `[u8 C0][C0 bytes]`, `[u8 C1][C1 u16]`,
/// `[u8 C2][C2 u16]`. `None` when a count byte lies past the body.
// REF: FUN_8003BDE0
pub fn partition2_script_offset(body: &[u8]) -> Option<usize> {
    let name_len = *body.first()? as usize;
    let mut cur = 1 + name_len * 2;
    let c0 = *body.get(cur)? as usize;
    cur += 1 + c0;
    let c1 = *body.get(cur)? as usize;
    cur += 1 + c1 * 2;
    let c2 = *body.get(cur)? as usize;
    cur += 1 + c2 * 2;
    Some(cur)
}

/// Tightest upper byte bound for a record body starting at `start`: the
/// smallest record offset or section-header offset strictly greater than
/// `start`, clamped to the MAN length. Without it a record's walk spills into
/// the next record's bytes and desyncs everything after.
pub fn record_end_bound(man_file: &ManFile, man_len: usize, start: usize) -> usize {
    let mut bound = man_len;
    let data = man_file.data_region_offset;
    for partition in &man_file.partitions {
        for &off in partition {
            let abs = data + off as usize;
            if abs > start && abs < bound {
                bound = abs;
            }
        }
    }
    for section in &man_file.sections {
        if section.offset > start && section.offset < bound {
            bound = section.offset;
        }
    }
    bound.min(man_len)
}

/// Walk every record of every partition of one MAN and fold it into `census`,
/// recording which sites carry each key.
///
/// `sites` (when given) accumulates, per key, the carrier + record index of
/// every **clean** occurrence, so a hit can be reduced back to bytes.
pub fn tally_man(
    man_file: &ManFile,
    man: &[u8],
    census: &mut OpCensus,
    mut sites: Option<&mut BTreeMap<OpKey, BTreeSet<ScriptSite>>>,
) {
    for partition in 0..RECORD_PARTITIONS {
        let count = man_file
            .header
            .partition_counts
            .get(partition)
            .copied()
            .unwrap_or(0)
            .max(0) as usize;
        for record in 0..count {
            let Some((start, pc0, len)) = partition_record_span(man_file, man, partition, record)
            else {
                continue;
            };
            let body = &man[start..start + len];
            let mut one = OpCensus::default();
            one.tally_record(body, pc0);
            if let Some(sites) = sites.as_deref_mut() {
                for key in one.clean.keys() {
                    sites.entry(*key).or_default().insert(ScriptSite {
                        carrier: ScriptCarrier::ManPartition(partition),
                        record,
                    });
                }
            }
            census.merge(&one);
        }
    }
}

/// Decode `body` from `pc0` and return the byte offsets at which `key`
/// occurs **before** the record's first decode error - the `clean` hits, with
/// the coordinate a reader needs to go look at the bytes.
///
/// A clean hit is a lead, not a verdict: a linear walk can stay error-free
/// through message text and re-sync on a byte that is not an opcode. Reading
/// the decoded neighbourhood of a rare op's hits is the last check, and this
/// is what makes that cheap.
pub fn clean_hit_offsets(body: &[u8], pc0: usize, key: OpKey) -> Vec<usize> {
    let mut out = Vec::new();
    for step in LinearWalker::new(body, pc0) {
        match step {
            Ok(insn) => {
                if op_key(&insn) == key {
                    out.push(insn.pc);
                }
            }
            Err((_pc, err)) => {
                if !matches!(err, DisasmError::EndOfStream { .. }) {
                    break;
                }
            }
        }
    }
    out
}

/// Every `(partition, record, script_start, pc0, body_len)` a MAN carries, in
/// partition-then-record order. The walk order [`tally_man`] uses.
pub fn man_script_spans(
    man_file: &ManFile,
    man: &[u8],
) -> Vec<(usize, usize, usize, usize, usize)> {
    let mut out = Vec::new();
    for partition in 0..RECORD_PARTITIONS {
        let count = man_file
            .header
            .partition_counts
            .get(partition)
            .copied()
            .unwrap_or(0)
            .max(0) as usize;
        for record in 0..count {
            if let Some((start, pc0, len)) = partition_record_span(man_file, man, partition, record)
            {
                out.push((partition, record, start, pc0, len));
            }
        }
    }
    out
}

#[cfg(test)]
mod census_tests {
    use super::*;

    /// The key of a `0x4C` instruction is the whole `op0` byte, so the outer
    /// nibble and the sub-arm are both visible - the thread rows cite arms as
    /// `[4C CF]`, not as "nibble C".
    #[test]
    fn menu_ctrl_keys_on_the_whole_op0_byte() {
        // `4C CF 10 20` - the script focus override (nibble C, sub F).
        let code = [0x4Cu8, 0xCF, 0x10, 0x20];
        let insn = super::super::decode(&code, 0).expect("decode 4C CF");
        assert_eq!(insn.size, 4, "4C CF is a four-byte instruction");
        let key = op_key(&insn);
        assert_eq!(key.opcode, 0x4C);
        assert_eq!(key.sub, Some(0xCF));
        assert_eq!(key.label(), "4C CF");
    }

    /// A record that desyncs stops contributing to `clean` but keeps
    /// contributing to `total` - the distinction the census's zero rests on.
    #[test]
    fn a_desync_splits_clean_from_total() {
        // `21` Nop, then `0x00` (not an opcode) -> error, then `21` Nop again.
        let code = [0x21u8, 0x00, 0x21];
        let mut c = OpCensus::default();
        c.tally_record(&code, 0);
        let nop = OpKey {
            opcode: 0x21,
            sub: None,
        };
        assert_eq!(c.clean_count(nop), 1, "only the pre-error Nop is clean");
        assert_eq!(c.total.get(&nop).copied(), Some(2));
        assert_eq!(c.desynced_records, 1);
        assert_eq!(c.records, 1);
    }

    /// `partition2_script_offset` on the documented worked example: a
    /// six-character name with all three condition blocks empty puts the
    /// first opcode at `0x10`.
    #[test]
    fn partition2_header_walk_matches_the_worked_example() {
        let mut body = vec![6u8];
        body.extend_from_slice(&[0x41; 12]); // 6 chars * 2 bytes
        body.extend_from_slice(&[0, 0, 0]); // C0 / C1 / C2 all empty
        body.push(0x34); // first opcode
        assert_eq!(partition2_script_offset(&body), Some(0x10));
    }
}
