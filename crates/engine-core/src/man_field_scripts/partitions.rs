//! MAN partition record-offset helpers + narration / g-flag site collection.
//!
//! Extracted verbatim from `man_field_scripts.rs`.

use std::collections::BTreeMap;

use crate::scene::{ProtIndex, Scene};

use super::*;

/// Which flag bank a [`GFlagSite`] touches.
///
/// The two banks are distinct id spaces, so census consumers must not merge
/// them: a scratchpad bit `26` and a system flag `26` are unrelated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlagBank {
    /// The 32-bit scratchpad story-flag word `_DAT_1F800394` (the engine's
    /// [`crate::world::World::story_flags`]); reached by opcodes `0x2E`
    /// (`SET`) / `0x2F` (`CLEAR`). Flag numbers are bit indices `0..31`.
    Scratchpad,
    /// The wide SYSTEM-flag bitmap reached by the `0x50..=0x7F` op family
    /// (`0x5x` SET, `0x6x` CLEAR, `0x7x` TEST). The flag number is a `u16`
    /// (`(lead & 0x8F) << 8 | operand`); the engine's bit helpers live at
    /// [`crate::world::World::system_flag_set`] /
    /// [`crate::world::World::system_flag_test`]. This is the id space of the
    /// overworld progress gates (e.g. `0x193` / `0x482` / `0x2FC`).
    System,
}

/// One field-VM **flag write / test** found while walking a MAN partition's
/// records as field-VM scripts. Covers both the scratchpad global-flag ops
/// (`GFLAG_SET` `0x2E` / `GFLAG_CLEAR` `0x2F`) and the wide SYSTEM-flag ops
/// (`0x50..=0x7F`), annotated with the bank + flag number it touches.
///
/// The opening prologue's `opdeene` cutscene-timeline record ends with a
/// scratchpad `GFLAG_SET 26`, the write the `town01` hand-off gate
/// (`FUN_801D1344`) waits on - see [`crate::world::PROLOGUE_HANDOFF_FLAG`].
/// SYSTEM-flag setters (the overworld progress gates) typically live in a
/// *different* scene's MAN than the one that gates on them, which is what the
/// disc-wide [`system_flag_census`] surfaces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GFlagSite {
    /// Absolute byte offset of the flag opcode in the MAN buffer.
    pub abs_pc: usize,
    /// Partition the carrying record lives in (`0..3`).
    pub partition: usize,
    /// Record index within the partition.
    pub record: usize,
    /// The opcode byte (scratchpad `0x2E`/`0x2F`, or a `0x50..=0x7F` system op).
    pub opcode: u8,
    /// `true` iff this is a SET op (scratchpad `0x2E` or system `0x5x`).
    /// `false` for CLEAR **and** TEST - use [`GFlagSite::kind`] to tell those
    /// apart. Kept for the prologue-arm consumers that only care about SET.
    pub set: bool,
    /// SET / CLEAR / TEST discriminator (carries TEST, which `set` cannot).
    pub kind: FlagKind,
    /// Which bank the op targets.
    pub bank: FlagBank,
    /// Low byte of the flag number. For [`FlagBank::Scratchpad`] this is the
    /// full bit index (`0..31`); for [`FlagBank::System`] it is truncated -
    /// use [`GFlagSite::flag`] for the full number.
    pub bit: u8,
    /// The full flag number: scratchpad bit index, or the `u16` system flag id.
    pub flag: u16,
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
/// collect its flag write/test sites: the scratchpad global-flag ops
/// (`GFLAG_SET` `0x2E` / `GFLAG_CLEAR` `0x2F`) **and** the wide SYSTEM-flag
/// ops (`0x50..=0x7F`, SET/CLEAR/TEST). Each site is tagged with its
/// [`FlagBank`] and full flag number, so callers can tell a scratchpad bit
/// from a system flag that share a low byte.
///
/// This is the partition-agnostic companion to [`walk_partition1_scripts`]:
/// the encounter hunt cares about partition 1's yield sites, the opening
/// prologue cares about partition 2's cutscene-timeline `GFLAG_SET`, and the
/// overworld progress-gate hunt cares about SYSTEM-flag setters across every
/// partition. All share the same `[u8 N][N*2 locals][4-byte header]` record
/// prefix and the same opcode-aware [`LinearWalker`] decode, so a site is
/// reported only at a real instruction boundary - not at an operand / SJIS
/// byte that happens to equal a flag opcode.
///
/// Prologue-arm consumers filter on `s.set && s.bit == 26` over the scratchpad
/// bank; TEST sites (`set == false`) and system-bank sites are ignored by that
/// filter, so the extra sites are additive.
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
            match insn.info {
                // Scratchpad global flag (`0x2E` set / `0x2F` clear). The VM
                // has no scratchpad TEST op reaching this variant, but guard
                // anyway so `set`/`kind` stay coherent.
                InsnInfo::GFlag { kind, bit } => out.push(GFlagSite {
                    abs_pc: script_start + insn.pc,
                    partition,
                    record: index,
                    opcode: insn.opcode,
                    set: kind == FlagKind::Set,
                    kind,
                    bank: FlagBank::Scratchpad,
                    bit,
                    flag: u16::from(bit),
                }),
                // Wide SYSTEM-flag bank (`0x5x` set / `0x6x` clear / `0x7x`
                // test). `idx` is the full `u16` flag number; `bit` keeps the
                // low byte for the scratchpad-shaped consumers.
                InsnInfo::SystemFlag { kind, idx, .. } => out.push(GFlagSite {
                    abs_pc: script_start + insn.pc,
                    partition,
                    record: index,
                    opcode: insn.opcode,
                    set: kind == FlagKind::Set,
                    kind,
                    bank: FlagBank::System,
                    bit: (idx & 0xFF) as u8,
                    flag: idx,
                }),
                _ => {}
            }
        }
    }
    out
}

/// One SYSTEM-flag site recovered by [`system_flag_census`], carrying the
/// scene it lives in plus the partition/record/op that touches the flag.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlagCensusSite {
    /// CDNAME scene name whose MAN carries the op.
    pub scene_name: String,
    /// Partition the carrying record lives in (`0..3`).
    pub partition: usize,
    /// Record index within the partition.
    pub record: usize,
    /// The opcode byte (a `0x50..=0x7F` system op).
    pub opcode: u8,
    /// SET / CLEAR / TEST discriminator.
    pub kind: FlagKind,
}

/// Disc-wide SYSTEM-flag census: walk every scene's MAN across all three
/// partitions and map each SYSTEM flag number to the list of sites (scene +
/// partition + record + op + kind) that set / clear / test it.
///
/// This is the tool the overworld progress-gate RE needs: a gate like
/// `system_flag_test(0x193)` lives in one scene, but the *setter* that opens
/// it almost always lives in a different scene's MAN. Only the SYSTEM bank
/// (`0x50..=0x7F` ops) is reported - the scratchpad bank is a separate 32-bit
/// id space with its own tooling ([`walk_partition_gflag_sites`]).
///
/// Scenes that fail to load or have no MAN are skipped silently (the census is
/// best-effort over the whole CDNAME scene set). The returned map is sorted by
/// flag number; each flag's site list preserves scene / partition / record
/// discovery order.
pub fn system_flag_census<I, S>(index: &ProtIndex, scenes: I) -> BTreeMap<u16, Vec<FlagCensusSite>>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut out: BTreeMap<u16, Vec<FlagCensusSite>> = BTreeMap::new();
    for name in scenes {
        let name = name.as_ref();
        let Ok(scene) = Scene::load(index, name) else {
            continue;
        };
        let Ok(Some(man)) = scene.field_man_payload(index) else {
            continue;
        };
        let Ok(man_file) = legaia_asset::man_section::parse(&man) else {
            continue;
        };
        for partition in 0..3 {
            for site in walk_partition_gflag_sites(&man_file, &man, partition) {
                if site.bank != FlagBank::System {
                    continue;
                }
                out.entry(site.flag).or_default().push(FlagCensusSite {
                    scene_name: name.to_string(),
                    partition: site.partition,
                    record: site.record,
                    opcode: site.opcode,
                    kind: site.kind,
                });
            }
        }
    }
    out
}
