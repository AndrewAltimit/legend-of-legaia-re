//! Disassemble every placed prop's bind record (the partition-0 field-VM
//! script `FUN_8003A55C` attaches to the actor) for a scene - the raw evidence
//! behind the door-collision / cupboard-interact behaviour notes in
//! `docs/subsystems/field-locomotion.md`.
//!
//! Usage: `cargo run -p legaia-engine-core --example dump_prop_scripts -- town01`
use std::path::PathBuf;

use legaia_asset::field_disasm;
use legaia_asset::man_section::ManFile;
use legaia_engine_core::scene::{ProtIndex, Scene};

/// A partition-0 record's byte span in the MAN (start..next record start).
fn p0_record(man_file: &ManFile, man: &[u8], index: usize) -> Option<(usize, usize)> {
    let start = man_file.data_region_offset + *man_file.partitions[0].get(index)? as usize;
    let mut end = man.len();
    for part in &man_file.partitions {
        for &off in part {
            let a = man_file.data_region_offset + off as usize;
            if a > start && a < end {
                end = a;
            }
        }
    }
    Some((start, end))
}

fn main() -> anyhow::Result<()> {
    let extracted = PathBuf::from("extracted");
    let p = ProtIndex::open_extracted(&extracted)?;
    for name in std::env::args().skip(1) {
        let scene = Scene::load(&p, &name)?;
        let binds = scene.field_object_binds(&p)?.expect("field map + man");
        let man = scene.field_man_payload(&p)?.expect("field man");
        let man_file = legaia_asset::man_section::parse(&man)?;
        let placements = scene.field_object_placements(&p)?.expect("field map");
        println!("=== {name}: {} binds", binds.len());
        let mut seen = std::collections::BTreeSet::new();
        for pl in &placements {
            let anchor = (pl.anchor_col, pl.anchor_row);
            let Some(bind) = binds.get(&anchor) else {
                continue;
            };
            if !seen.insert(bind.record) {
                continue;
            }
            let Some((start, end)) = p0_record(&man_file, &man, bind.record as usize) else {
                continue;
            };
            let body = &man[start..end];
            let n = body[0] as usize;
            let name_sjis: Vec<u8> = body[1..1 + 2 * n].to_vec();
            let pc0 = 1 + 2 * n + 1;
            let anim = body[1 + 2 * n];
            println!(
                "-- P0[{}] anchor=({},{}) obj_id={} mesh={} anim={} name={}",
                bind.record,
                anchor.0,
                anchor.1,
                pl.obj_idx,
                0,
                anim,
                name_sjis
                    .iter()
                    .map(|b| format!("{b:02x}"))
                    .collect::<String>(),
            );
            // Raw bytes.
            print!("   raw:");
            for (i, b) in body.iter().enumerate() {
                if i % 16 == 0 {
                    print!("\n    {i:04x}:");
                }
                print!(" {b:02x}");
            }
            println!();
            // Linear disassembly from pc0.
            let mut pc = pc0;
            let mut steps = 0;
            while pc < body.len() && steps < 200 {
                let b = body[pc];
                if b & 0x7F < 0x20 {
                    println!("    {pc:04x}: [dialog/terminator byte {b:02x}] - stop");
                    break;
                }
                match field_disasm::decode(body, pc) {
                    Ok(insn) => {
                        println!(
                            "    {pc:04x}: {}",
                            field_disasm::format_instruction(&insn, body)
                        );
                        if insn.size == 0 {
                            break;
                        }
                        pc += insn.size;
                    }
                    Err(e) => {
                        println!("    {pc:04x}: <decode error {e}>");
                        break;
                    }
                }
                steps += 1;
            }
        }
    }
    Ok(())
}
