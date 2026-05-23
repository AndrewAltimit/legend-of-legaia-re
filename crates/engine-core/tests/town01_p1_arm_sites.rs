//! Disc-gated: opcode-aware survey of town01's MAN partition-1 field-VM
//! scripts, hunting the Rim Elm Tetsu training-fight arm.
//!
//! Replaces the naive `0x37`/`0x41` byte-scan (which matches every yield
//! opcode AND every operand / SJIS byte that equals those values) with a
//! real linear opcode walk: an arm site is reported only at a decoded
//! `Yield` instruction boundary, and the inline window is decoded with the
//! retail `+0x3` count / `+0x4` ids layout.
//!
//! Skip-passes without disc data / extracted assets (CLAUDE.md convention).

use legaia_engine_core::man_field_scripts::walk_partition1_scripts;
use legaia_engine_core::scene::{ProtIndex, Scene};
use std::path::PathBuf;

fn extracted_dir() -> Option<PathBuf> {
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

#[test]
fn town01_partition1_scripts_walk_and_report_arm_sites() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing — run `legaia-extract` first");
        return;
    };

    let index = ProtIndex::open_extracted(&extracted).expect("open ProtIndex");
    let scene = Scene::load(&index, "town01").expect("load town01");
    let bundle = legaia_engine_core::scene_bundle::find_bundle(&scene)
        .expect("town01 has a scene_asset_table bundle");
    let entry_bytes = index
        .entry_bytes_extended(bundle.entry_idx())
        .expect("entry bytes");
    let man = legaia_engine_core::scene_bundle::extract_man_payload(&bundle, &entry_bytes)
        .expect("man extract")
        .expect("town01 MAN payload");
    let man_file = legaia_asset::man_section::parse(&man).expect("man parse");

    let records = walk_partition1_scripts(&man_file, &man);
    eprintln!(
        "town01 MAN: {} partition-1 records (counts {:?})",
        records.len(),
        man_file.header.partition_counts
    );

    let mut total_arm_sites = 0usize;
    let mut total_decoded_records = 0usize;
    let mut tetsu_sites: Vec<(usize, usize)> = Vec::new(); // (record_idx, abs_pc)

    for rec in &records {
        let candidates: Vec<_> = rec.encounter_arm_candidates().collect();
        total_arm_sites += rec.arm_sites.len();
        total_decoded_records += candidates.len();
        if rec.arm_sites.is_empty() && rec.insn_count == 0 {
            continue;
        }
        // Only print records that carry at least one decodable inline record
        // or are the system entry script — keeps the log readable.
        if !candidates.is_empty() || rec.index == 0 {
            eprintln!(
                "  P1[{:3}] start=0x{:05X} pc0={} body={:5}b insns={:4} errs={:3} yields={} candidates={}",
                rec.index,
                rec.script_start,
                rec.pc0,
                rec.body_len,
                rec.insn_count,
                rec.decode_errors,
                rec.arm_sites.len(),
                candidates.len(),
            );
        }
        for site in &rec.arm_sites {
            if let Some(record) = site.record {
                let tag = if site.matches_tetsu() {
                    "  <<< TETSU"
                } else {
                    ""
                };
                eprintln!(
                    "      yield 0x{:02X}{} @ abs=0x{:05X} (rel {:#06X})  window={:02X?}  -> count={} ids={:02X?}{}",
                    site.opcode,
                    if site.wide { "(wide)" } else { "" },
                    site.abs_pc,
                    site.rel_pc,
                    site.window,
                    record.count,
                    &record.monster_ids[..record.count as usize],
                    tag,
                );
                if site.matches_tetsu() {
                    tetsu_sites.push((rec.index, site.abs_pc));
                }
            }
        }
    }

    eprintln!(
        "[summary] {} P1 records, {} yield sites, {} decode as valid formation records, {} match Tetsu (count=1 id=0x4F)",
        records.len(),
        total_arm_sites,
        total_decoded_records,
        tetsu_sites.len(),
    );
    if !tetsu_sites.is_empty() {
        eprintln!("[tetsu] inline-literal sites: {tetsu_sites:?}");
    } else {
        eprintln!(
            "[tetsu] no inline [count=1][id=0x4F] literal at any P1 yield site \
             — supports the indexed-formation-table install path (formation index {})",
            legaia_engine_core::encounter_record::RIM_ELM_TRAINING_FORMATION_ID
        );
    }

    // Structural invariants (stable regardless of which install path holds):
    // partition 1 is non-empty and the system entry script (record 0) walks.
    assert!(!records.is_empty(), "town01 has partition-1 records");
    assert_eq!(records[0].index, 0, "record 0 is the system entry script");

    // The system entry script (record 0) is genuine executable field-VM
    // bytecode: it decodes near-cleanly. The per-actor interaction records
    // [1..], by contrast, desync hard because they are dominated by embedded
    // MES dialog text (the "candidate" windows above are all ASCII strings).
    // That asymmetry is itself evidence the encounter arm is NOT script-borne.
    assert!(
        records[0].insn_count > 50,
        "the system entry script decodes to a substantial instruction stream"
    );
    assert!(
        records[0].decode_errors * 10 < records[0].insn_count,
        "the entry script decodes near-cleanly (errs {} vs insns {})",
        records[0].decode_errors,
        records[0].insn_count
    );

    // The pivotal finding: NO partition-1 yield site carries an inline
    // `[count=1][id=0x4F]` Tetsu literal. The opcode walk lands on every
    // 0x37/0x41 byte (the count=0 dialog-noise candidates prove that), so a
    // real inline literal would surface here. Its absence falsifies the
    // inline-literal install hypothesis and corroborates the indexed
    // formation-table path (carrier `+0x94` selects formation index
    // RIM_ELM_TRAINING_FORMATION_ID into the MAN encounter section).
    assert!(
        tetsu_sites.is_empty(),
        "no inline Tetsu literal in town01 P1 scripts (found {tetsu_sites:?}) \
         — the training fight installs via the indexed formation table"
    );
}
