//! MAN partition record-offset helpers + narration / g-flag site collection.
//!
//! Extracted verbatim from `man_field_scripts.rs`.

use super::*;

/// One field-VM **global-flag write** (`GFLAG_SET` / `GFLAG_CLEAR`, opcodes
/// `0x2E` / `0x2F`) found while walking a MAN partition's records as
/// field-VM scripts, annotated with the scratchpad flag bit it touches.
///
/// The global-flag bank is `_DAT_1F800394` (the engine's
/// [`crate::world::World::story_flags`]); op `0x2E` sets `1 << bit`, op
/// `0x2F` clears it. The opening prologue's `opdeene` cutscene-timeline
/// record ends with `GFLAG_SET 26`, the write the `town01` hand-off gate
/// (`FUN_801D1344`) waits on - see
/// [`crate::world::PROLOGUE_HANDOFF_FLAG`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GFlagSite {
    /// Absolute byte offset of the `GFLAG` opcode in the MAN buffer.
    pub abs_pc: usize,
    /// Partition the carrying record lives in (`0..3`).
    pub partition: usize,
    /// Record index within the partition.
    pub record: usize,
    /// The opcode byte (`0x2E` set, `0x2F` clear).
    pub opcode: u8,
    /// `true` for `GFLAG_SET` (`0x2E`), `false` for `GFLAG_CLEAR` (`0x2F`).
    pub set: bool,
    /// Scratchpad flag bit the op touches (`0..31`).
    pub bit: u8,
}

/// The script `script_start` of `partition`'s record `index`, computed from
/// the partition's u24 record-offset table against the MAN data region.
/// `None` when the partition or index is out of range or the offset lands
/// past the buffer.
pub(crate) fn partition_record_offset(
    man_file: &ManFile,
    man_len: usize,
    partition: usize,
    index: usize,
) -> Option<usize> {
    let off = *man_file.partitions.get(partition)?.get(index)? as usize;
    let abs = man_file.data_region_offset.checked_add(off)?;
    (abs < man_len).then_some(abs)
}

/// First-opcode offset of a **partition-2 named-record** (the cutscene-timeline
/// records), relative to the record start in `body`.
///
/// Partition-2 records are not the partition-1 `[u8 N][N*2 locals][4-byte
/// header]` shape - they open with a Shift-JIS **name** and three
/// condition-list gates that the dispatcher `FUN_8003BDE0` walks before the
/// script proper:
///
/// ```text
/// [u8 name_len]                 ; name length in CHARACTERS
/// [name_len * 2 bytes]          ; SJIS name (no separate terminator)
/// [u8 C0][C0 bytes]             ; cond-block 0 (byte-granular; skipped)
/// [u8 C1][C1 * u16]             ; cond-block 1 (story-flag OR gate)
/// [u8 C2][C2 * u16]             ; cond-block 2 (story-flag AND gate)
/// <script…>                     ; first field-VM opcode
/// ```
///
/// So the entry offset is `1 + name_len*2 + (1+C0) + (1+C1*2) + (1+C2*2)`.
/// Returns `None` if a count byte lies past the record body. For `opdeene`'s
/// record 18 (`name_len=6` "Opening", all three blocks empty) this is `0x10`,
/// the `0x34` EFFECT op that opens the prologue timeline.
// REF: FUN_8003BDE0
pub(crate) fn partition2_record_script_offset(body: &[u8]) -> Option<usize> {
    let name_len = *body.first()? as usize;
    let mut cur = 1 + name_len * 2; // name field (chars * 2, no terminator)
    let c0 = *body.get(cur)? as usize;
    cur += 1 + c0; // cond-block 0: 1 byte per unit
    let c1 = *body.get(cur)? as usize;
    cur += 1 + c1 * 2; // cond-block 1: u16 per unit
    let c2 = *body.get(cur)? as usize;
    cur += 1 + c2 * 2; // cond-block 2: u16 per unit
    Some(cur)
}

/// The C1 / C2 story-flag gate lists of a **partition-2 named-record**
/// (see [`partition2_record_script_offset`] for the header shape).
///
/// Retail's record dispatcher `FUN_8003BDE0` tests each listed flag against
/// the story-flag bitmap at `DAT_80085758` (`bit = byte[flag >> 3] &
/// (0x80 >> (flag & 7))`): **C1 blocks the spawn if ANY listed flag is set**
/// (the one-shot mechanism - e.g. `town01`'s opening record lists `0x225`,
/// set once the opening has played); **C2 requires ALL listed flags set**.
/// Returns `None` when the header overruns the record body.
// REF: FUN_8003BDE0
pub fn partition2_record_gates(
    man_file: &ManFile,
    man: &[u8],
    index: usize,
) -> Option<(Vec<u16>, Vec<u16>)> {
    let script_start = partition_record_offset(man_file, man.len(), 2, index)?;
    let end = record_end_bound(man_file, man.len(), script_start);
    let body = man.get(script_start..end)?;
    let name_len = *body.first()? as usize;
    let mut cur = 1 + name_len * 2;
    let c0 = *body.get(cur)? as usize;
    cur += 1 + c0;
    let read_u16_list = |body: &[u8], cur: &mut usize| -> Option<Vec<u16>> {
        let n = *body.get(*cur)? as usize;
        *cur += 1;
        let mut out = Vec::with_capacity(n);
        for _ in 0..n {
            let lo = *body.get(*cur)?;
            let hi = *body.get(*cur + 1)?;
            *cur += 2;
            out.push(u16::from_le_bytes([lo, hi]));
        }
        Some(out)
    };
    let c1 = read_u16_list(body, &mut cur)?;
    let c2 = read_u16_list(body, &mut cur)?;
    Some((c1, c2))
}

/// The byte span of `partition`'s record `index` as a field-VM script:
/// `(script_start, pc0, body_len)`, where `script_start` is the absolute
/// MAN offset of the record, `pc0` the first-opcode offset relative to it,
/// and `body_len` the bounded body length (clamped so the walk does not spill
/// into the next record or a sibling section).
///
/// The header shape is partition-specific: partition 2 (the cutscene-timeline
/// records) uses the named-record header decoded by
/// `partition2_record_script_offset` (`FUN_8003BDE0`); the other partitions
/// use the `[u8 N][N*2 locals][4-byte header]` prefix (`pc0 = 1 + N*2 + 4`).
///
/// `None` when the partition / index is out of range, the offset lands past
/// the buffer, or the record's header already overruns its bound.
pub fn partition_record_span(
    man_file: &ManFile,
    man: &[u8],
    partition: usize,
    index: usize,
) -> Option<(usize, usize, usize)> {
    let script_start = partition_record_offset(man_file, man.len(), partition, index)?;
    let end = record_end_bound(man_file, man.len(), script_start);
    let body = man.get(script_start..end)?;
    let pc0 = if partition == 2 {
        partition2_record_script_offset(body)?
    } else {
        let n = *body.first().unwrap_or(&0) as usize;
        1 + n * 2 + 4
    };
    if script_start + pc0 >= end {
        return None;
    }
    Some((script_start, pc0, end - script_start))
}

/// Collect every inline cutscene-narration page in `partition`'s records, in
/// record-then-page order.
///
/// Each record's bounded body is handed to
/// [`legaia_asset::cutscene_text::parse_narration`], which finds the narration
/// op + `0x1F`/`0x00` page framing structurally. The opening prologue scene
/// (`opdeene`) carries its narration in the cutscene-timeline partition
/// (partition 2); this returns those subtitle pages as plain text for the
/// runtime presenter ([`crate::cutscene_narration::CutsceneNarration`]).
pub fn collect_partition_narration(
    man_file: &ManFile,
    man: &[u8],
    partition: usize,
) -> Vec<String> {
    let count = man_file
        .header
        .partition_counts
        .get(partition)
        .copied()
        .unwrap_or(0)
        .max(0) as usize;
    let mut pages = Vec::new();
    for index in 0..count {
        let Some((script_start, _pc0, body_len)) =
            partition_record_span(man_file, man, partition, index)
        else {
            continue;
        };
        let body = &man[script_start..script_start + body_len];
        for block in legaia_asset::cutscene_text::parse_narration(body) {
            pages.extend(block.pages.into_iter().map(|p| p.text));
        }
    }
    pages
}

/// Walk every record of `partition` (`0..3`) as a field-VM script and
/// collect its global-flag write sites (`GFLAG_SET` / `GFLAG_CLEAR`).
///
/// This is the partition-agnostic companion to [`walk_partition1_scripts`]:
/// the encounter hunt cares about partition 1's yield sites, the opening
/// prologue cares about partition 2's cutscene-timeline `GFLAG_SET`. Both
/// share the same `[u8 N][N*2 locals][4-byte header]` record prefix and the
/// same opcode-aware [`LinearWalker`] decode, so a `GFLAG` site is reported
/// only at a real instruction boundary - not at an operand / SJIS byte that
/// happens to equal `0x2E`.
pub fn walk_partition_gflag_sites(
    man_file: &ManFile,
    man: &[u8],
    partition: usize,
) -> Vec<GFlagSite> {
    let count = man_file
        .header
        .partition_counts
        .get(partition)
        .copied()
        .unwrap_or(0)
        .max(0) as usize;
    let mut out = Vec::new();
    for index in 0..count {
        let Some(script_start) = partition_record_offset(man_file, man.len(), partition, index)
        else {
            continue;
        };
        let n = *man.get(script_start).unwrap_or(&0) as usize;
        let pc0 = 1 + n * 2 + 4;
        let end = record_end_bound(man_file, man.len(), script_start);
        if script_start + pc0 >= end {
            continue;
        }
        let body = &man[script_start..end];
        for insn in LinearWalker::new(body, pc0).flatten() {
            if let InsnInfo::GFlag { kind, bit } = insn.info
                && kind != FlagKind::Test
            {
                out.push(GFlagSite {
                    abs_pc: script_start + insn.pc,
                    partition,
                    record: index,
                    opcode: insn.opcode,
                    set: kind == FlagKind::Set,
                    bit,
                });
            }
        }
    }
    out
}
