//! Disc-gated proof that the `scene_event_scripts` / `scene_v12_table`
//! prescript records are a WORD-ALIGNED (16-bit) per-scene actor/event command
//! structure - NOT field-VM (`FUN_801DE840`) bytecode.
//!
//! This falsifies the long-standing "the prescript records are field-VM event
//! scripts" claim (formerly in `scene_event_scripts`, `scene_v12_table`,
//! `scene_scripted_asset_table`, and several docs). The evidence is empirical
//! and contrastive:
//!
//! - **Prescript records** disassemble as field-VM with a HIGH decode-error
//!   rate (the bytes are word-aligned: low byte = opcode, high byte usually 0;
//!   the field VM's valid opcode floor is 0x22, but the records are dominated
//!   by opcodes 0x01..0x21). Their framed records terminate with a `0x0008`
//!   word and open with the `0xFFFF 0x0000` header sentinel.
//! - **MAN partition-1 actor scripts** (the genuine per-scene field-VM scripts
//!   that `FUN_8003A1E4` runs through `FUN_801DE840`, and that the engine
//!   executes) disassemble with a LOW error rate. This is the contrast that
//!   makes the falsification non-vacuous.
//!
//! Skips silently when `extracted/PROT/` is missing.

use legaia_asset::field_disasm::decode;
use legaia_asset::{man_section, scene_asset_table, scene_event_scripts};
use std::path::PathBuf;

fn extracted_prot() -> Option<PathBuf> {
    [
        PathBuf::from("extracted/PROT"),
        PathBuf::from("../../extracted/PROT"),
    ]
    .into_iter()
    .find(|p| p.is_dir())
}

fn bin_entries(prot: &PathBuf) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(prot)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("BIN"))
        .collect();
    v.sort();
    v
}

/// Walk `body` as field-VM bytecode linearly and return the fraction of decode
/// steps that errored. On a decode error we advance one byte and count it as
/// an error step (matching the disassembler CLI's resume behaviour).
fn field_vm_error_rate(body: &[u8]) -> f32 {
    let mut pc = 0usize;
    let mut steps = 0usize;
    let mut errors = 0usize;
    while pc < body.len() {
        steps += 1;
        match decode(body, pc) {
            Ok(insn) => pc += insn.size.max(1),
            Err(_) => {
                errors += 1;
                pc += 1;
            }
        }
    }
    if steps == 0 {
        return 0.0;
    }
    errors as f32 / steps as f32
}

#[test]
fn prescript_records_are_word_aligned_not_field_vm() {
    let Some(prot) = extracted_prot() else {
        eprintln!("[skip] extracted/PROT/ missing");
        return;
    };

    let mut entries_scanned = 0usize;
    let mut framed_records = 0usize;
    let mut terminated_records = 0usize; // end in a 0x0008 word
    let mut body_words = 0usize;
    let mut hi_byte_zero = 0usize;
    // Per-record field-VM error rates over the record body (post-header).
    let mut high_error_records = 0usize;

    for path in bin_entries(&prot) {
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        let Some(es) = scene_event_scripts::detect(&bytes) else {
            continue;
        };
        // Only consider the high-confidence prescripts.
        if es.frame_opener_rate < 0.5 {
            continue;
        }
        let Some(ranges) = scene_event_scripts::record_ranges(&bytes) else {
            continue;
        };
        entries_scanned += 1;

        for (start, end) in ranges {
            let record = &bytes[start..end];
            if !scene_event_scripts::record_is_framed(record) {
                continue; // record 0 / unframed shapes
            }
            framed_records += 1;

            // Word-stream invariants.
            let words = scene_event_scripts::record_words(record).unwrap();
            // The terminator is excluded from `record_words`; a properly framed
            // record has a 0x0008 word somewhere after the header.
            let has_terminator = record[4..].chunks_exact(2).any(|w| {
                u16::from_le_bytes([w[0], w[1]]) == scene_event_scripts::RECORD_TERMINATOR
            });
            if has_terminator {
                terminated_records += 1;
            }
            for &w in &words {
                body_words += 1;
                if (w >> 8) == 0 {
                    hi_byte_zero += 1;
                }
            }

            // Field-VM disassembly of the record BODY (skip the 4-byte header).
            let body = &record[4..];
            if !body.is_empty() && field_vm_error_rate(body) > 0.4 {
                high_error_records += 1;
            }
        }
    }

    if entries_scanned == 0 {
        eprintln!("[skip] no scene-event-script prescripts in corpus");
        return;
    }

    // Corpus-scale assertions (figures from the retail USA disc):
    //  78 entries, ~1561 framed records, 99.9% terminate in 0x0008,
    //  82.9% of body words have a zero high byte.
    assert!(
        entries_scanned >= 50,
        "expected the prescript shape across most scenes, got {entries_scanned} entries"
    );
    assert!(
        framed_records >= 500,
        "expected many framed records, got {framed_records}"
    );

    let term_rate = terminated_records as f32 / framed_records as f32;
    assert!(
        term_rate > 0.95,
        "framed records should terminate in a 0x0008 word; rate = {term_rate:.3}"
    );

    let hi0_rate = hi_byte_zero as f32 / body_words as f32;
    assert!(
        hi0_rate > 0.70,
        "body words should be 16-bit word-aligned (high byte 0); rate = {hi0_rate:.3}"
    );

    // The decisive falsification: the vast majority of framed records do NOT
    // decode as field-VM bytecode (high error rate under the real disassembler).
    let high_err_rate = high_error_records as f32 / framed_records as f32;
    assert!(
        high_err_rate > 0.80,
        "framed records should NOT decode as field-VM (high disasm-error rate); \
         only {high_err_rate:.3} of records exceeded the 0.4 error threshold"
    );
}

#[test]
fn man_actor_scripts_are_genuine_field_vm() {
    // The contrast: real field-VM scripts (the ones the engine executes) live
    // in the scene MAN, and DO disassemble cleanly (low error rate).
    let Some(prot) = extracted_prot() else {
        eprintln!("[skip] extracted/PROT/ missing");
        return;
    };

    let mut checked = 0usize;
    let mut clean = 0usize; // entry scripts with a low field-VM error rate

    for path in bin_entries(&prot) {
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        let Some(table) = scene_asset_table::detect(&bytes) else {
            continue;
        };
        let Some(man_desc) = table.descriptors.iter().find(|d| d.type_byte == 0x03) else {
            continue;
        };
        let start = man_desc.data_offset as usize;
        if start >= bytes.len() {
            continue;
        }
        let Ok((man_bytes, _)) =
            legaia_lzs::decompress_tracked(&bytes[start..], man_desc.size as usize)
        else {
            continue;
        };
        if man_bytes.len() as u32 != man_desc.size {
            continue;
        }
        let Ok(man) = man_section::parse(&man_bytes) else {
            continue;
        };
        // `scene_entry_script` returns `(script_start, pc0)` where `pc0` is the
        // first opcode's offset RELATIVE to `script_start` (after the
        // `[u8 N][N*2 locals][4-byte header]` prefix). The field-VM body begins
        // at `script_start + pc0`.
        let Some((script_start, pc0)) = man.scene_entry_script(&man_bytes) else {
            continue;
        };
        let body_start = script_start + pc0;
        if body_start + 32 > man_bytes.len() {
            continue;
        }
        // Bound the linear walk to a window so a run-off into the next record's
        // data doesn't dominate the rate. Real field-VM entry scripts decode
        // cleanly (~8% error on the retail town MANs).
        let end = (body_start + 200).min(man_bytes.len());
        let body = &man_bytes[body_start..end];
        checked += 1;
        if field_vm_error_rate(body) < 0.30 {
            clean += 1;
        }
    }

    if checked == 0 {
        eprintln!("[skip] no MAN entry scripts recovered (needs extended-footprint MAN bytes)");
        return;
    }

    let clean_rate = clean as f32 / checked as f32;
    assert!(
        clean_rate > 0.5,
        "MAN entry scripts should decode cleanly as field-VM; only {clean_rate:.3} \
         of {checked} were under the 0.30 error threshold"
    );
}
