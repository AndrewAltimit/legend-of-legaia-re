//! Census of the field-VM `0x4C` (`MENU_CTRL`) **outer-nibble-5** sub-ops over
//! the disc's scene MAN field-VM scripts.
//!
//! Why nibble 5 specifically: retail's n5 arm dispatches five sub-ops off a
//! 5-entry table (overlay 0897 file `+0x718`, VA `0x801CEF30`), and the port
//! decodes four of them - sub `2` is missing, so `[4C, 0x52, item_id]` falls
//! into `op_4c_n5`'s `_ => Halt` arm. A halt at PC is a stall, not a skip, so
//! whether any scene issues it decides whether the hole is a latent softlock
//! or a documented-absent encoding.
//!
//! Same instrument as `scan_4c_d8`: sites are taken at real instruction
//! boundaries through the field-VM disassembler, with a walker-independent raw
//! byte-pair count printed alongside as the cross-check.
//!
//! Run with:
//!   cargo run --release -p legaia-engine-core --example scan_4c_n5

use std::collections::BTreeMap;
use std::path::PathBuf;

use legaia_engine_core::man_field_scripts::{
    CLEAN_RESYNC_INSNS, partition_record_span, scene_man_carriers,
};
use legaia_engine_core::scene::{ProtIndex, Scene};
use legaia_engine_vm::field_disasm::{InsnInfo, LinearWalker};

fn main() -> anyhow::Result<()> {
    let extracted = PathBuf::from("extracted");
    let index = ProtIndex::open_extracted(&extracted)?;
    let names = index.cdname_scene_names();

    // sub-op -> decoded site count
    let mut decoded: BTreeMap<u8, usize> = BTreeMap::new();
    // sub-op -> raw `4C 5s` byte-pair count
    let mut raw: BTreeMap<u8, usize> = BTreeMap::new();
    // scenes carrying a decoded sub-2 site
    let mut sub2_scenes: BTreeMap<String, Vec<(u32, usize)>> = BTreeMap::new();
    let mut carriers = 0usize;
    let mut records = 0usize;

    for name in &names {
        let Ok(scene) = Scene::load(&index, name) else {
            continue;
        };
        for carrier in scene_man_carriers(&index, &scene) {
            carriers += 1;
            let man = &carrier.payload;
            for o in 0..man.len().saturating_sub(1) {
                if man[o] == 0x4C && (man[o + 1] & 0xF0) == 0x50 {
                    *raw.entry(man[o + 1] & 0x0F).or_default() += 1;
                }
            }
            let Ok(man_file) = legaia_asset::man_section::parse(man) else {
                continue;
            };
            for partition in 0..3 {
                let count = (*man_file
                    .header
                    .partition_counts
                    .get(partition)
                    .unwrap_or(&0))
                .max(0) as usize;
                for record in 0..count {
                    let Some((start, pc0, len)) =
                        partition_record_span(&man_file, man, partition, record)
                    else {
                        continue;
                    };
                    records += 1;
                    let body = &man[start..start + len];
                    let mut ok_run = CLEAN_RESYNC_INSNS;
                    for insn in LinearWalker::new(body, pc0) {
                        let Ok(insn) = insn else {
                            ok_run = 0;
                            continue;
                        };
                        let clean = ok_run >= CLEAN_RESYNC_INSNS;
                        ok_run += 1;
                        let InsnInfo::MenuCtrl { op0, .. } = insn.info else {
                            continue;
                        };
                        if op0 & 0xF0 != 0x50 {
                            continue;
                        }
                        *decoded.entry(op0 & 0x0F).or_default() += 1;
                        if op0 & 0x0F == 2 && clean {
                            sub2_scenes
                                .entry(name.clone())
                                .or_default()
                                .push((carrier.entry_idx, start + insn.pc));
                        }
                    }
                }
            }
        }
    }

    println!(
        "{} CDNAME scenes, {carriers} MAN carriers, {records} records walked",
        names.len()
    );
    println!("\n4C 5s sub-op census (decoded at instruction boundaries):");
    for sub in 0u8..16 {
        let d = decoded.get(&sub).copied().unwrap_or(0);
        let r = raw.get(&sub).copied().unwrap_or(0);
        if d == 0 && r == 0 {
            continue;
        }
        println!("  sub {sub:#x}: decoded={d:6}  raw_bytepairs={r:6}");
    }
    println!("\nsub-2 (`[4C, 52, item_id]`, the undecoded arm) clean sites:");
    if sub2_scenes.is_empty() {
        println!("  NONE");
    }
    for (name, list) in &sub2_scenes {
        for (entry, pc) in list {
            println!("  {name} entry={entry} pc=0x{pc:X}");
        }
    }
    Ok(())
}
