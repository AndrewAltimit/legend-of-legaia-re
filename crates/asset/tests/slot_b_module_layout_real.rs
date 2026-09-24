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
//! 5. PROT 0943 and 0961 put a framed function **above** their first record,
//!    which is what makes "everything past the last function" the wrong rule.
//!    Those bodies are the images' inherited residue (0943's own bytes end at
//!    file `+0x1037`, 0961's at `+0x1918`; the donors are PROT 0942 and
//!    0960), so the assertion is about the partition this parser sees, not
//!    about a second code region either image owns; and
//! 6. `byte_account` credits the records structurally on a band entry.
//!
//! Skips (and passes) without `LEGAIA_DISC_BIN` / `extracted/`.

use legaia_asset::slot_b_module::{self, SLOT_B_LINK_BASE, SLOT_B_PROT_FIRST, SLOT_B_PROT_LAST};
use std::path::PathBuf;

/// The two spawn helpers that take the record pointer in `$a2`.
const SPAWN: [u32; 2] = [0x8002_1B04, 0x8005_0ED4];

/// The band's two record-less entries, for the same reason
/// `cast_module_data_rows_real.rs` names them: PROT 0926 is the 1-sector null
/// stub, and PROT 0952's two spawn sites sit in its inherited tail (file
/// `+0x11E8..+0x1800`, PROT 0951's bytes) and resolve to `0x801F8348` /
/// `0x801F836C`, two of PROT 0951's records past the end of 0952's image.
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

/// The value `$a2` holds after running the words `[from, to)` forward from an
/// unknown register file: `lui` / `addiu` / register copies are tracked, a
/// call clobbers the caller-saved registers, anything else writing a register
/// forgets it.
fn a2_after(b: &[u8], from: usize, to: usize, skip: usize) -> Option<u32> {
    let mut regs: [Option<u32>; 32] = [None; 32];
    regs[0] = Some(0);
    let mut p = from;
    while p + 4 <= to {
        let w = word_at(b, p);
        p += 4;
        if p - 4 == skip {
            continue;
        }
        let (op, rs, rt, rd) = (w >> 26, (w >> 21) & 31, (w >> 16) & 31, (w >> 11) & 31);
        let s = (w & 0xFFFF) as i16 as i32;
        if op == 3 || (op == 0 && w & 0x3F == 0x09) {
            for r in [
                1usize, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 24, 25, 31,
            ] {
                regs[r] = None;
            }
            continue;
        }
        match op {
            0x0F => regs[rt as usize] = Some((w & 0xFFFF) << 16),
            0x09 => {
                regs[rt as usize] = regs[rs as usize].map(|v| (v as i32).wrapping_add(s) as u32)
            }
            0x00 if matches!(w & 0x3F, 0x21 | 0x25) && (rs == 0 || rt == 0) => {
                regs[rd as usize] = regs[(rs | rt) as usize];
            }
            0x00 if !matches!(w & 0x3F, 0x08 | 0x18..=0x1B | 0x11 | 0x13) => {
                regs[rd as usize] = None
            }
            0x08 | 0x0A..=0x0E | 0x20..=0x26 | 0x10 | 0x12 => regs[rt as usize] = None,
            _ => {}
        }
        regs[0] = Some(0);
    }
    regs[6]
}

/// Every `$a2` a spawn-helper `jal` in this image is handed, resolved from the
/// raw words by a forward register simulation from the enclosing routine's
/// prologue - deliberately a second
/// implementation of the parser's backward walk, so the test is not asserting
/// the parser against itself. Both paths into a call count: the fall-through
/// one through the call's own delay slot, and every `j` that lands on the call
/// (or up to three words above it) through that `j`'s delay slot.
fn spawn_a2_values(b: &[u8]) -> Vec<u32> {
    // Simulate from the enclosing routine's prologue (`addiu sp,sp,-N`), or
    // from the image start when there is none above the word.
    let entry = |at: usize| {
        let mut p = at;
        while p >= 4 {
            p -= 4;
            let w = word_at(b, p);
            if w >> 16 == 0x27BD && w & 0x8000 != 0 {
                return p;
            }
        }
        0
    };
    let mut out = Vec::new();
    let calls: Vec<u32> = SPAWN
        .iter()
        .map(|a| 0x0C00_0000 | ((a >> 2) & 0x03FF_FFFF))
        .collect();
    let mut off = 0usize;
    while off + 8 <= b.len() {
        if calls.contains(&word_at(b, off)) {
            out.extend(a2_after(b, entry(off), off + 8, off));
            let mut j = 0usize;
            while j + 8 <= b.len() {
                let w = word_at(b, j);
                if w >> 26 == 0x02 {
                    let va = ((SLOT_B_LINK_BASE + j as u32 + 4) & 0xF000_0000)
                        | ((w & 0x03FF_FFFF) << 2);
                    let t = va.wrapping_sub(SLOT_B_LINK_BASE) as usize;
                    if t <= off && off - t <= 12 {
                        out.extend(a2_after(b, entry(j), j + 8, j));
                    }
                }
                j += 4;
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

/// Record for record against `scripts/ghidra-analysis/slot_b_band.py`'s
/// `spawn_record_band`, the mirror `disc-coverage.py` and
/// `attribute-dump-extents.py` read. Both sides resolve the spawn pointer by
/// the same widened rule (delay slot, register copies, `switch` arms into a
/// shared call), and a divergence here would make the two instruments
/// disagree about which bytes are records.
#[test]
fn rust_and_python_spawn_records_agree() {
    let Some(dir) = extracted_dir() else {
        eprintln!("[skip] slot-B record parity: no LEGAIA_DISC_BIN / extracted/");
        return;
    };
    let Some(script_dir) = ["scripts/ghidra-analysis", "../../scripts/ghidra-analysis"]
        .into_iter()
        .map(PathBuf::from)
        .find(|p| p.join("slot_b_band.py").is_file())
    else {
        eprintln!("[skip] scripts/ghidra-analysis not found from this cwd");
        return;
    };
    let prot = dir.join("PROT");
    let prog = format!(
        r#"
import glob, os, sys, json
sys.path.insert(0, {script_dir:?})
import slot_b_band
out = {{}}
for idx in range({first}, {last} + 1):
    hits = sorted(glob.glob(os.path.join({prot:?}, "%04d_*" % idx)))
    if not hits:
        continue
    data = open(hits[0], "rb").read()
    out[str(idx)] = [len(data), [[lo, hi] for lo, hi in slot_b_band.spawn_record_band(data, slot_b_band.SLOT_B_LINK_BASE)]]
print(json.dumps(out))
"#,
        script_dir = script_dir.to_string_lossy(),
        prot = prot.to_string_lossy(),
        first = SLOT_B_PROT_FIRST,
        last = SLOT_B_PROT_LAST,
    );
    let out = std::process::Command::new("python3")
        .arg("-c")
        .arg(&prog)
        .output()
        .expect("run python3");
    if !out.status.success() {
        eprintln!(
            "[skip] python side failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        return;
    }
    type PyBand = std::collections::BTreeMap<String, (usize, Vec<(u32, u32)>)>;
    let py: PyBand = serde_json::from_str(String::from_utf8_lossy(&out.stdout).trim())
        .expect("parse python records");
    let mut compared = 0usize;
    let mut records = 0usize;
    for entry in SLOT_B_PROT_FIRST..=SLOT_B_PROT_LAST {
        let Some((len, band)) = py.get(&entry.to_string()) else {
            continue;
        };
        let bytes = read_entry(&dir, entry);
        assert_eq!(
            bytes.len(),
            *len,
            "PROT {entry}: file and archive lengths differ"
        );
        let layout = slot_b_module::parse(&bytes);
        let mut rust: Vec<(u32, u32)> = layout
            .records
            .iter()
            .chain(&layout.chained_records)
            .map(|r| {
                (
                    SLOT_B_LINK_BASE + r.start as u32,
                    SLOT_B_LINK_BASE + r.end as u32,
                )
            })
            .collect();
        rust.sort_unstable();
        assert_eq!(
            &rust, band,
            "PROT {entry}: Rust and Python spawn records differ"
        );
        compared += 1;
        records += rust.len();
    }
    eprintln!("[ok] slot-B record parity: {compared} images, {records} records identical");
    assert!(compared >= 60, "only {compared} band images compared");
}
