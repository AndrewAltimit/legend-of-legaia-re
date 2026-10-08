//! MAN partition-1 script-record walking + arm-site / script-record types.
//!
//! Extracted verbatim from `man_field_scripts.rs`.

use super::*;

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
    /// formation - `count == 1` and `monster_ids[0] == 0x4F`.
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
            // Degenerate / empty record body - record it with no sites.
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
