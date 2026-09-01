//! One-off probe: fully disassemble a scene's MAN partition-2 records.
//!
//! Run with:
//!   cargo run --release -p legaia-engine-core --example dump_p2_records -- town01 12 13 14

use std::path::PathBuf;

use legaia_engine_core::scene::{ProtIndex, Scene};

fn main() -> anyhow::Result<()> {
    let extracted = PathBuf::from("extracted");
    let p = ProtIndex::open_extracted(&extracted)?;
    let mut args = std::env::args().skip(1);
    let name = args.next().expect("scene name");
    let want: Vec<usize> = args.map(|a| a.parse().unwrap()).collect();

    let scene = Scene::load(&p, &name)?;
    let man = scene.field_man_payload(&p)?.expect("no MAN");
    let mf = legaia_asset::man_section::parse(&man)?;

    let n2 = mf.header.partition_counts[2].max(0) as usize;
    for rec in 0..n2 {
        if !want.is_empty() && !want.contains(&rec) {
            continue;
        }
        let Some((start, pc0, len)) =
            legaia_engine_core::man_field_scripts::partition_record_span(&mf, &man, 2, rec)
        else {
            println!("P2[{rec}]: (bad span)");
            continue;
        };
        println!("\n=== P2[{rec}] start={start:#x} pc0={pc0} len={len} ===");
        let body = &man[start..start + len];
        for insn in legaia_asset::field_disasm::LinearWalker::new(body, pc0) {
            match insn {
                Ok(i) => {
                    println!(
                        "{}",
                        legaia_asset::field_disasm::format_instruction(&i, body)
                    );
                }
                Err((pc, e)) => {
                    println!("  (decode stop at {pc:#x}: {e:?})");
                    let tail = &body[pc.min(body.len())..];
                    let hex: Vec<String> = tail.iter().map(|b| format!("{b:02X}")).collect();
                    println!("  raw tail: {}", hex.join(" "));
                    break;
                }
            }
        }
    }
    Ok(())
}
