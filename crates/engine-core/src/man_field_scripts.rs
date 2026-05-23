//! Opcode-aware walk of a scene MAN's partition-1 field-VM scripts.
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
use legaia_engine_vm::field_disasm::{InsnInfo, LinearWalker, YieldKind};

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
