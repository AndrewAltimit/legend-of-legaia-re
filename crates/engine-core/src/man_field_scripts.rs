//! Opcode-aware walk of a scene MAN's field-VM scripts.
//!
//! [`walk_partition1_scripts`] surveys partition 1 (the encounter hunt);
//! [`walk_partition_gflag_sites`] is the partition-agnostic companion that
//! collects global-flag writes (used for the opening prologue's partition-2
//! `GFLAG_SET 26` hand-off arm), both via the same [`LinearWalker`] decode.
//!
//! The record **header** is partition-specific. Partitions 0/1 use the
//! `[u8 N][N*2 locals][4-byte header]` prefix below. Partition 2 (the
//! cutscene-timeline records) instead opens with a Shift-JIS name and three
//! condition-list gates - see [`partition2_record_script_offset`] and
//! [`partition_record_span`], decoded from the dispatcher `FUN_8003BDE0`.
//!
//! Partition 1 of a scene MAN (the "actor-placement / scripts" partition)
//! holds one field-VM script per record:
//!
//! - record `0` is the scene-entry **system script** — the one
//!   [`crate::scene::Scene::field_man_entry_script`] resolves and
//!   `enter_field_scene` loads via `load_field_script_at`;
//! - records `1..` are per-actor **interaction scripts**, dispatched when
//!   the player interacts with the placed actor.
//!
//! Each record opens with the same `[u8 N][N*2 locals][4-byte header]`
//! prefix as the entry script, so the first opcode sits `1 + N*2 + 4`
//! bytes in (see [`legaia_asset::man_section::ManFile::scene_entry_script`]).
//!
//! This module pairs the MAN partition walk with the field-VM disassembler
//! ([`legaia_engine_vm::field_disasm`]) so callers get a faithful,
//! opcode-aware instruction stream per record instead of a byte scan. The
//! distinction matters for the scripted-encounter hunt: a naive search for
//! a "yield" byte (`0x37` / `0x41`) hits every yield opcode **and** every
//! operand / SJIS byte that happens to equal `0x37` / `0x41`. Walking the
//! opcode stream means an [`ArmSite`] is reported only at a real `Yield`
//! instruction boundary, and the inline record bytes are decoded with
//! [`EncounterRecord::parse`] — the same `+0x3` count / `+0x4` ids layout
//! the retail reader at `0x801DA620` consumes.
//!
//! ## What this can and cannot conclude
//!
//! Per [`crate::field::step`]'s own commentary there is **no dedicated
//! encounter opcode**: the arm ops (`0x37`/`0x41`, `0x38`, `0x43`, `0x47`,
//! `0x4C`) all share the yield-and-forward shape, and the *discriminator*
//! is the consuming entity-SM state, not the opcode. So a single
//! [`ArmSite`] whose inline window decodes as a valid `[count][ids]` record
//! is a *candidate*, not a proof. The value here is empirical: it surfaces
//! whether any P1 script carries an inline `[count=1][id=0x4F]` Tetsu
//! literal at a real yield boundary — which adjudicates the inline-literal
//! hypothesis against the indexed-formation-table hypothesis
//! (see [`crate::encounter_record::RIM_ELM_TRAINING_FORMATION_ID`]).

use legaia_asset::man_section::ManFile;
use legaia_engine_vm::field_disasm::{FlagKind, InsnInfo, LinearWalker, YieldKind};

use crate::encounter_record::EncounterRecord;

/// One field-VM `Yield` instruction in a partition-1 script, annotated with
/// the inline encounter-record decode of its trailing operand window.
#[derive(Debug, Clone)]
pub struct ArmSite {
    /// Absolute byte offset of the yield opcode in the MAN buffer.
    pub abs_pc: usize,
    /// Byte offset relative to the record's `script_start`.
    pub rel_pc: usize,
    /// The yield opcode (`0x37` / `0x41` standard, `0x47` wide).
    pub opcode: u8,
    /// `0x37`/`0x41` (standard) vs `0x47` (wide) yield encoding.
    pub wide: bool,
    /// The 8-byte window the retail reader would consume at this site
    /// (`man[abs_pc..abs_pc+8]`, zero-padded if the buffer ends early).
    pub window: [u8; 8],
    /// The inline record decoded from `window` (`+0x3` count, `+0x4` ids),
    /// when it parses as a valid `0..=4`-monster formation.
    pub record: Option<EncounterRecord>,
}

impl ArmSite {
    /// `true` when the inline window decodes as the lone Rim Elm Tetsu
    /// formation — `count == 1` and `monster_ids[0] == 0x4F`.
    pub fn matches_tetsu(&self) -> bool {
        matches!(
            self.record,
            Some(EncounterRecord { count: 1, monster_ids })
                if monster_ids[0] == crate::encounter_record::RIM_ELM_TRAINING_OPPONENT_ID
        )
    }
}

/// Per-record disassembly summary for one partition-1 field-VM script.
#[derive(Debug, Clone)]
pub struct ManScriptRecord {
    /// Partition-1 record index (`0` = scene-entry system script).
    pub index: usize,
    /// Absolute byte offset of the record's script block in the MAN buffer.
    pub script_start: usize,
    /// First-opcode offset relative to `script_start` (`1 + N*2 + 4`).
    pub pc0: usize,
    /// Number of bytes from `script_start` to the record's bounded end.
    pub body_len: usize,
    /// Number of instructions a linear walk decoded.
    pub insn_count: usize,
    /// Number of bytes the linear walk could not decode (recovered by
    /// advancing one byte).
    pub decode_errors: usize,
    /// Yield sites found in this record, with inline-record decodes.
    pub arm_sites: Vec<ArmSite>,
}

impl ManScriptRecord {
    /// Yield sites whose inline window decodes as a valid formation record.
    pub fn encounter_arm_candidates(&self) -> impl Iterator<Item = &ArmSite> {
        self.arm_sites.iter().filter(|s| s.record.is_some())
    }
}

/// Compute the tightest upper byte bound for a record body that starts at
/// `start`: the smallest record offset (across all three partitions) or
/// section start that is strictly greater than `start`, clamped to the MAN
/// length. This stops a record's walk from spilling into the next record's
/// or the encounter section's bytes.
fn record_end_bound(man_file: &ManFile, man_len: usize, start: usize) -> usize {
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
    // The encounter section (and its siblings) live in the same data region;
    // their length-prefix offsets are a hard ceiling for script bytes.
    for section in &man_file.sections {
        if section.offset > start && section.offset < bound {
            bound = section.offset;
        }
    }
    bound.min(man_len)
}

/// Walk every partition-1 record of `man_file` as a field-VM script and
/// return a per-record disassembly summary.
///
/// `man` is the decompressed MAN buffer the offsets index into.
pub fn walk_partition1_scripts(man_file: &ManFile, man: &[u8]) -> Vec<ManScriptRecord> {
    let n1 = man_file.header.partition_counts[1].max(0) as usize;
    let mut out = Vec::with_capacity(n1);
    for index in 0..n1 {
        let Some(script_start) = man_file.actor_placement_record_offset(index, man.len()) else {
            continue;
        };
        let n = *man.get(script_start).unwrap_or(&0) as usize;
        let pc0 = 1 + n * 2 + 4;
        let end = record_end_bound(man_file, man.len(), script_start);
        if script_start + pc0 >= end {
            // Degenerate / empty record body — record it with no sites.
            out.push(ManScriptRecord {
                index,
                script_start,
                pc0,
                body_len: end.saturating_sub(script_start),
                insn_count: 0,
                decode_errors: 0,
                arm_sites: Vec::new(),
            });
            continue;
        }
        let body = &man[script_start..end];
        let mut insn_count = 0usize;
        let mut decode_errors = 0usize;
        let mut arm_sites = Vec::new();
        for r in LinearWalker::new(body, pc0) {
            match r {
                Ok(insn) => {
                    insn_count += 1;
                    if let InsnInfo::Yield { kind } = insn.info {
                        let abs_pc = script_start + insn.pc;
                        let mut window = [0u8; 8];
                        for (i, slot) in window.iter_mut().enumerate() {
                            if let Some(&b) = man.get(abs_pc + i) {
                                *slot = b;
                            }
                        }
                        arm_sites.push(ArmSite {
                            abs_pc,
                            rel_pc: insn.pc,
                            opcode: insn.opcode,
                            wide: matches!(kind, YieldKind::Wide),
                            window,
                            record: EncounterRecord::parse(&window),
                        });
                    }
                }
                Err(_) => decode_errors += 1,
            }
        }
        out.push(ManScriptRecord {
            index,
            script_start,
            pc0,
            body_len: end - script_start,
            insn_count,
            decode_errors,
            arm_sites,
        });
    }
    out
}

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
fn partition_record_offset(
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
fn partition2_record_script_offset(body: &[u8]) -> Option<usize> {
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

/// The byte span of `partition`'s record `index` as a field-VM script:
/// `(script_start, pc0, body_len)`, where `script_start` is the absolute
/// MAN offset of the record, `pc0` the first-opcode offset relative to it,
/// and `body_len` the bounded body length (clamped so the walk does not spill
/// into the next record or a sibling section).
///
/// The header shape is partition-specific: partition 2 (the cutscene-timeline
/// records) uses the named-record header decoded by
/// [`partition2_record_script_offset`] (`FUN_8003BDE0`); the other partitions
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

#[cfg(test)]
mod tests {
    use super::*;
    use legaia_asset::man_section::{ManFile, ManHeader};

    /// Build a minimal one-partition-1-record MAN whose single record is a
    /// field-VM script: `[N=0][4-byte header][0x37 yield with inline
    /// count=1 id=0x4F][...]`. Exercises the record-walk + arm-site decode
    /// without disc data.
    fn synthetic_man_with_tetsu_arm() -> (ManFile, Vec<u8>) {
        // data_region_offset is arbitrary for the synthetic test; pick a
        // small value and lay the record body right after it.
        let data_region_offset = 0x40usize;
        let p1_0 = 0u32; // record 0 sits at the start of the data region.
        let script_start = data_region_offset + p1_0 as usize;

        // Record prefix: N=0 -> pc0 = 1 + 0 + 4 = 5.
        // Then a 0x37 yield whose inline window is [0x37][s0][s1][count=1][0x4F].
        let mut man = vec![0u8; script_start];
        man.push(0x00); // N = 0
        man.extend_from_slice(&[0xAA, 0xBB, 0xCC, 0xDD]); // 4-byte header
        // pc0 = 5: the yield opcode + inline record.
        man.push(0x37); // +0 yield opcode
        man.push(0x11); // +1 reserved
        man.push(0x22); // +2 reserved
        man.push(0x01); // +3 count = 1
        man.push(0x4F); // +4 monster id = Tetsu
        man.push(0x00); // +5 padding so the window has 8 bytes
        man.push(0x00);
        man.push(0x00);

        let header = ManHeader {
            status_flags: 0,
            low_flag: false,
            depth_lut: [0; 16],
            partition_counts: [0, 1, 0],
            u24_at_28: 0,
        };
        let man_file = ManFile {
            header,
            partitions: [vec![], vec![p1_0], vec![]],
            data_region_offset,
            // Sections all point past the script so they don't bound it.
            sections: std::array::from_fn(|_| legaia_asset::man_section::SectionRef {
                offset: man.len(),
                length: 0,
            }),
        };
        (man_file, man)
    }

    #[test]
    fn walks_partition1_and_decodes_inline_tetsu_arm() {
        let (man_file, man) = synthetic_man_with_tetsu_arm();
        let records = walk_partition1_scripts(&man_file, &man);
        assert_eq!(records.len(), 1);
        let rec = &records[0];
        assert_eq!(rec.index, 0);
        assert_eq!(rec.pc0, 5);
        assert_eq!(rec.arm_sites.len(), 1, "one yield site");
        let site = &rec.arm_sites[0];
        assert_eq!(site.opcode, 0x37);
        assert!(!site.wide);
        let record = site.record.expect("inline window decodes");
        assert_eq!(record.count, 1);
        assert_eq!(record.monster_ids[0], 0x4F);
        assert!(site.matches_tetsu());
    }

    /// Build a minimal one-partition-2-record MAN whose single record is a
    /// field-VM script ending in `GFLAG_SET 26` (op `0x2E`, operand `0x1A`) -
    /// the opening prologue's `town01` hand-off arm.
    fn synthetic_man_with_gflag_set_26() -> (ManFile, Vec<u8>) {
        let data_region_offset = 0x40usize;
        let p2_0 = 0u32;
        let script_start = data_region_offset + p2_0 as usize;

        // Record prefix: N=0 -> pc0 = 5. Then GFLAG_SET 26.
        let mut man = vec![0u8; script_start];
        man.push(0x00); // N = 0
        man.extend_from_slice(&[0xAA, 0xBB, 0xCC, 0xDD]); // 4-byte header
        man.push(0x2E); // GFLAG_SET
        man.push(0x1A); // bit 26
        man.push(0x48); // a trailing no-op so the walk has a clean boundary

        let header = ManHeader {
            status_flags: 0,
            low_flag: false,
            depth_lut: [0; 16],
            partition_counts: [0, 0, 1],
            u24_at_28: 0,
        };
        let man_file = ManFile {
            header,
            partitions: [vec![], vec![], vec![p2_0]],
            data_region_offset,
            sections: std::array::from_fn(|_| legaia_asset::man_section::SectionRef {
                offset: man.len(),
                length: 0,
            }),
        };
        (man_file, man)
    }

    #[test]
    fn walks_partition2_and_finds_gflag_set_26() {
        let (man_file, man) = synthetic_man_with_gflag_set_26();
        let sites = walk_partition_gflag_sites(&man_file, &man, 2);
        assert_eq!(sites.len(), 1, "one GFLAG site");
        let site = sites[0];
        assert_eq!(site.partition, 2);
        assert_eq!(site.record, 0);
        assert_eq!(site.opcode, 0x2E);
        assert!(site.set);
        assert_eq!(site.bit, 26);
        // The other partitions carry no records, hence no sites.
        assert!(walk_partition_gflag_sites(&man_file, &man, 0).is_empty());
        assert!(walk_partition_gflag_sites(&man_file, &man, 1).is_empty());
    }

    #[test]
    fn partition2_named_record_script_offset_matches_the_formula() {
        // name_len=6 (12 SJIS bytes), all three cond-blocks empty -> 0x10,
        // the opdeene record-18 shape.
        let mut body = vec![0x06];
        body.extend_from_slice(&[0xAA; 12]); // 6 SJIS chars
        body.extend_from_slice(&[0x00, 0x00, 0x00]); // C0=C1=C2=0
        body.push(0x34); // first opcode
        assert_eq!(partition2_record_script_offset(&body), Some(0x10));

        // Non-empty blocks: name_len=2 (4 bytes), C0=3 (3 bytes), C1=1 (2
        // bytes), C2=2 (4 bytes) -> 1 + 4 + (1+3) + (1+2) + (1+4) = 17.
        let mut body = vec![0x02, 0xAA, 0xAA, 0xAA, 0xAA];
        body.push(0x03); // C0 = 3
        body.extend_from_slice(&[0x11, 0x22, 0x33]);
        body.push(0x01); // C1 = 1 u16
        body.extend_from_slice(&[0x44, 0x55]);
        body.push(0x02); // C2 = 2 u16
        body.extend_from_slice(&[0x66, 0x77, 0x88, 0x99]);
        body.push(0x21); // first opcode
        assert_eq!(partition2_record_script_offset(&body), Some(17));
        assert_eq!(body[17], 0x21);

        // A count byte past the end returns None rather than panicking.
        assert_eq!(partition2_record_script_offset(&[0x06]), None);
    }

    #[test]
    fn empty_partition1_yields_no_records() {
        let header = ManHeader {
            status_flags: 0,
            low_flag: false,
            depth_lut: [0; 16],
            partition_counts: [0, 0, 0],
            u24_at_28: 0,
        };
        let man_file = ManFile {
            header,
            partitions: [vec![], vec![], vec![]],
            data_region_offset: 0x2B,
            sections: std::array::from_fn(|_| legaia_asset::man_section::SectionRef {
                offset: 0x2B,
                length: 0,
            }),
        };
        let man = vec![0u8; 0x80];
        assert!(walk_partition1_scripts(&man_file, &man).is_empty());
    }
}
