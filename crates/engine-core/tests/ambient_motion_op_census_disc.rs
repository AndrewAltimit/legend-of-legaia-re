//! Disc-gated: which **scripted-motion VM** opcodes the disc actually
//! authors, per scene MAN.
//!
//! The port of `FUN_80038158` runs the whole 32-slot op table
//! (`legaia_engine_vm::ambient_motion_ops`), but four of its arms reach no
//! consumer on either host - `0x0E` (model re-bind), `0x13` (`MoveImage`),
//! `0x15` / `0x16` (the pitch / roll tweens). Wiring a consumer for an arm no
//! authored stream ever executes is speculation dressed as parity, so this
//! census answers the prior question from the bytes: does the disc carry the
//! op at all, and in which scenes?
//!
//! It walks every CDNAME scene's MAN tail-**section 1** streams (the carrier
//! `legaia_asset::man_motion` decodes, every variant of every record) and
//! counts each opcode by the section's own width table. Printed as a table
//! with `--nocapture`; the assertions pin the arms whose absence or presence
//! a wiring decision rests on.
//!
//! Skips + passes when `LEGAIA_DISC_BIN` / extracted assets are missing
//! (CLAUDE.md disc-gated convention).

use legaia_engine_core::man_field_scripts::scene_man_carriers;
use legaia_engine_core::scene::{ProtIndex, Scene};
use std::collections::{BTreeMap, BTreeSet};
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

/// `op -> (total sites, scenes carrying it)`.
type Census = BTreeMap<u8, (usize, BTreeSet<String>)>;

/// How the walk left each variant - the honesty check on a zero count. A
/// census that stops early on an unknown byte under-reports every op after
/// it, so the two tallies are printed beside the table.
#[derive(Default)]
struct WalkStats {
    variants: usize,
    ran_to_end: usize,
    stopped_on_unknown_op: usize,
}

fn census() -> Option<(Census, WalkStats)> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    let extracted = extracted_dir().or_else(|| {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        None
    })?;
    let index = ProtIndex::open_extracted(&extracted).expect("open ProtIndex");
    let mut out: Census = BTreeMap::new();
    let mut stats = WalkStats::default();
    for name in index.cdname_scene_names() {
        let Ok(scene) = Scene::load(&index, &name) else {
            continue;
        };
        for carrier in scene_man_carriers(&index, &scene) {
            let man = &carrier.payload;
            let Ok(man_file) = legaia_asset::man_section::parse(man) else {
                continue;
            };
            for rec in legaia_asset::man_motion::motion_records(man, &man_file) {
                for var in legaia_asset::man_motion::stream_variants(man, &rec) {
                    stats.variants += 1;
                    let mut pc = var.code_offset;
                    let mut clean = true;
                    while pc < var.code_end && pc < man.len() {
                        let op = man[pc];
                        let Some(w) = legaia_asset::man_motion::op_width(op) else {
                            clean = false;
                            break;
                        };
                        let e = out.entry(op).or_default();
                        e.0 += 1;
                        e.1.insert(name.clone());
                        pc += w;
                    }
                    if clean {
                        stats.ran_to_end += 1;
                    } else {
                        stats.stopped_on_unknown_op += 1;
                    }
                }
            }
        }
    }
    Some((out, stats))
}

#[test]
fn scripted_motion_op_census_or_skip() {
    let Some((c, stats)) = census() else { return };
    eprintln!(
        "[motion ops] {} variants walked: {} ran to the record end, {} stopped on a byte the width table does not name",
        stats.variants, stats.ran_to_end, stats.stopped_on_unknown_op
    );
    eprintln!("[motion ops] op   sites  scenes");
    for (op, (sites, scenes)) in &c {
        eprintln!("[motion ops] {op:#04x} {sites:6} {:6}", scenes.len());
    }
    let count = |op: u8| c.get(&op).map(|e| e.0).unwrap_or(0);
    let scenes = |op: u8| c.get(&op).map(|e| e.1.len()).unwrap_or(0);
    // The census must have found the corpus at all: op `0x08` (system-flag
    // clear) is the arm the sibling `motion_flag_census_disc` pins by scene,
    // so a zero here means this walk is broken, not that the disc is empty.
    assert!(count(0x08) > 0, "motion-VM flag-clear arm found nowhere");
    // The four consumer-gap arms. Whatever these numbers are, they are the
    // denominator any "wire it on both hosts" decision has to quote.
    for op in [0x0E, 0x13, 0x15, 0x16] {
        let names: Vec<&str> = c
            .get(&op)
            .map(|e| e.1.iter().map(String::as_str).collect())
            .unwrap_or_default();
        eprintln!(
            "[motion ops] consumer-gap op {op:#04x}: {} sites over {} scenes {names:?}",
            count(op),
            scenes(op)
        );
    }
}
