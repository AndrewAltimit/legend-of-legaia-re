//! Disc-gated: the slot-B module layout, over all 64 band entries.
//!
//! `legaia_asset::slot_b_module` names the bytes of a band image: a head jump
//! table, a frame-matched code partition, and the spawn-record band whose
//! extents come from the module's own pointer-forming instructions. Two things
//! rest on those claims being right - `asset account` credits them, and
//! `scripts/ci/disc-coverage.py`'s `spawn_record_band` shape takes them out of
//! the code denominator - so a wrong claim would silently retire real code from
//! the dump worklist.
//!
//! What is asserted, off the disc, for every entry `0903..=0966`:
//!
//! 1. no claimed record overlaps any frame-matched function body - the
//!    invariant that keeps the shape from demoting code;
//! 2. records are ascending, disjoint and at least a header long, and every
//!    one opens with a `model_sel` `FUN_80021B04` dispatches;
//! 3. every claimed record's start is an address some `jal` into
//!    `FUN_80021B04` / `FUN_80050ED4` hands over in `$a2` - the consumer
//!    evidence, re-derived here from the raw words rather than taken from the
//!    parser;
//! 4. the band is non-vacuous: at least 60 of the 64 entries carry a bounded
//!    record, and the whole band claims at least 60 KB;
//! 5. PROT 0943 and 0961 put a framed function **above** their first record -
//!    the interleave that makes "everything past the last function" the wrong
//!    rule; and
//! 6. `byte_account` credits the records structurally on a band entry.
//!
//! Skips (and passes) without `LEGAIA_DISC_BIN` / `extracted/`.

use legaia_asset::slot_b_module::{self, SLOT_B_LINK_BASE, SLOT_B_PROT_FIRST, SLOT_B_PROT_LAST};
use std::path::PathBuf;

/// The two spawn helpers that take the record pointer in `$a2`.
const SPAWN: [u32; 2] = [0x8002_1B04, 0x8005_0ED4];

/// The band's two record-less entries, for the same reason
/// `cast_module_data_rows_real.rs` names them: PROT 0926 is the 1-sector null
/// stub, and PROT 0952's two spawn sites load `$a2` out of a saved register no
/// static window can see.
const RECORDLESS: [u32; 2] = [926, 952];

fn extracted_dir() -> Option<PathBuf> {
    std::env::var_os("LEGAIA_DISC_BIN")?;
    for base in ["extracted", "../../extracted"] {
        let p = PathBuf::from(base);
        if p.join("PROT.DAT").is_file() {
            return Some(p);
        }
    }
    None
}

fn read_entry(dir: &std::path::Path, idx: u32) -> Vec<u8> {
    let mut archive =
        legaia_prot::archive::Archive::open(&dir.join("PROT.DAT")).expect("open PROT.DAT");
    let entry = archive
        .entries
        .get(idx as usize)
        .cloned()
        .unwrap_or_else(|| panic!("PROT {idx} entry"));
    let mut bytes = Vec::new();
    archive
        .read_entry(&entry, &mut bytes)
        .unwrap_or_else(|e| panic!("read PROT {idx}: {e:#}"));
    bytes
}

fn word_at(b: &[u8], off: usize) -> u32 {
    u32::from_le_bytes(b[off..off + 4].try_into().unwrap())
}

/// Every `$a2` a spawn-helper `jal` in this image is handed, resolved from the
/// raw words. Deliberately a second implementation of the parser's rule, so
/// the test is not asserting the parser against itself.
fn spawn_a2_values(b: &[u8]) -> Vec<u32> {
    let mut out = Vec::new();
    let calls: Vec<u32> = SPAWN
        .iter()
        .map(|a| 0x0C00_0000 | ((a >> 2) & 0x03FF_FFFF))
        .collect();
    let mut off = 0usize;
    while off + 4 <= b.len() {
        if calls.contains(&word_at(b, off)) {
            let mut a2: Option<u32> = None;
            let mut p = off.saturating_sub(22 * 4);
            while p + 4 <= off {
                let w = word_at(b, p);
                let (op, rs, rt, imm) = (w >> 26, (w >> 21) & 31, (w >> 16) & 31, w & 0xFFFF);
                if op == 3 {
                    a2 = None;
                } else if rt == 6 {
                    let s = if imm & 0x8000 != 0 {
                        imm as i32 - 0x1_0000
                    } else {
                        imm as i32
                    };
                    a2 = match op {
                        0x0F => Some(imm << 16),
                        0x09 if rs == 6 => a2.map(|v| (v as i32).wrapping_add(s) as u32),
                        0x09 if rs == 0 => Some(s as u32),
                        _ => None,
                    };
                }
                p += 4;
            }
            if let Some(v) = a2 {
                out.push(v);
            }
        }
        off += 4;
    }
    out
}

#[test]
fn every_band_image_partitions_into_code_and_spawn_records() {
    let Some(dir) = extracted_dir() else {
        eprintln!("[skip] slot-B module layout: no LEGAIA_DISC_BIN / extracted/");
        return;
    };

    let mut with_records = 0usize;
    let mut total_record_bytes = 0usize;
    let mut total_records = 0usize;

    for entry in SLOT_B_PROT_FIRST..=SLOT_B_PROT_LAST {
        let bytes = read_entry(&dir, entry);
        assert!(!bytes.is_empty(), "PROT {entry} is empty");
        let layout = slot_b_module::parse(&bytes);
        assert_eq!(layout.link_base, SLOT_B_LINK_BASE);

        let a2s = spawn_a2_values(&bytes);
        let mut prev_end = 0usize;
        for r in &layout.records {
            // (1) never inside a function body.
            for f in &layout.functions {
                assert!(
                    r.end <= f.start || r.start >= f.end,
                    "PROT {entry}: record {:#x}..{:#x} overlaps framed function \
                     {:#x}..{:#x}",
                    r.start,
                    r.end,
                    f.start,
                    f.end
                );
            }
            // (2) ascending, disjoint, at least a header, in range.
            assert!(r.start >= prev_end, "PROT {entry}: records out of order");
            assert!(r.end >= r.start + 4 && r.end <= bytes.len());
            assert!(
                r.model_sel == -1
                    || (0..0x100).contains(&r.model_sel)
                    || r.model_sel == 0x4000
                    || r.model_sel == 0x4001,
                "PROT {entry}: record {:#x} model_sel {:#x} is not a value \
                 FUN_80021B04 dispatches",
                r.start,
                r.model_sel
            );
            // (3) the consumer names this address.
            let va = SLOT_B_LINK_BASE + r.start as u32;
            assert!(
                a2s.contains(&va),
                "PROT {entry}: record {va:#010X} is claimed but no spawn call \
                 in the image hands that address over in $a2"
            );
            prev_end = r.end;
            total_record_bytes += r.end - r.start;
            total_records += 1;
        }

        if layout.records.is_empty() {
            assert!(
                RECORDLESS.contains(&entry),
                "PROT {entry}: no bounded spawn record, and it is not one of \
                 the two entries documented as record-less"
            );
        } else {
            with_records += 1;
        }

        // The head table, when there is one, sits below the first function.
        if let (Some(h), Some(f)) = (layout.head_table.clone(), layout.functions.first()) {
            assert!(h.end <= f.start, "PROT {entry}: head table runs into code");
        }
    }

    // (4) non-vacuous.
    assert!(
        with_records >= 60,
        "only {with_records} of 64 band entries carry a bounded spawn record"
    );
    assert!(
        total_record_bytes >= 60_000,
        "the whole band claims only {total_record_bytes} record bytes"
    );
    assert!(
        total_records >= 700,
        "only {total_records} records over the band"
    );
}

#[test]
fn two_images_interleave_records_with_code() {
    let Some(dir) = extracted_dir() else {
        eprintln!("[skip] slot-B interleave: no LEGAIA_DISC_BIN / extracted/");
        return;
    };
    // PROT 0943 (cast_curse) and PROT 0961 (cast_dead_end_crisis) both resume
    // code ABOVE their first record, which is why a claim is cut at the next
    // framed function's prologue instead of running to the image end.
    for entry in [943u32, 961] {
        let bytes = read_entry(&dir, entry);
        let layout = slot_b_module::parse(&bytes);
        let first = layout.records.first().expect("records").start;
        assert!(
            layout.functions.iter().any(|f| f.start > first),
            "PROT {entry}: expected a framed function above the first record \
             at {first:#x}"
        );
    }
}

#[test]
fn byte_account_credits_the_records_structurally() {
    let Some(dir) = extracted_dir() else {
        eprintln!("[skip] slot-B byte accounting: no LEGAIA_DISC_BIN / extracted/");
        return;
    };
    // PROT 0923 (summon_gilium) carries the band's largest record run.
    let entry = 923u32;
    let bytes = read_entry(&dir, entry);
    let opts = legaia_asset::byte_account::AccountOptions {
        label: format!("PROT {entry}"),
        prot_index: Some(entry),
        depth: 0,
        rescan: false,
        ..Default::default()
    };
    let acc = legaia_asset::byte_account::account(&bytes, &opts);
    assert_eq!(acc.walker, legaia_asset::byte_account::Walker::SlotBModule);
    let record_bytes: usize = acc
        .by_owner
        .iter()
        .filter(|o| o.owner == legaia_asset::byte_account::OWNER_RECORD)
        .map(|o| o.bytes)
        .sum();
    assert!(
        record_bytes >= 5_000,
        "PROT {entry}: only {record_bytes} bytes credited to `record`"
    );
    assert!(
        acc.structural >= record_bytes,
        "structural must include the record claims"
    );
}
