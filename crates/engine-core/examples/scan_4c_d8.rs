//! Census of the field-VM synchronous actor-spawn opcode `0x4C 0xD8` over
//! the disc's **MAN** field-VM scripts.
//!
//! `0x4C` (`MENU_CTRL`) is a field-VM opcode and the field VM's bytecode
//! lives in the scene MAN - the asset-table bundle MAN plus each block's
//! streaming variant carrier. An earlier form of this scan read
//! *event-script* entries, which carry move-VM prescripts and no field-VM
//! bytecode at all; it produced hits only because a one-sector prescript
//! entry read under the old declared-span size ran past itself into the
//! neighbouring bundle. See `docs/subsystems/script-vm-menuctrl.md`.
//!
//! Sites are taken at real instruction boundaries via the field-VM
//! disassembler, so an operand or Shift-JIS byte that happens to read
//! `4C D8` is not counted. The walker-independent raw byte-pair count is
//! printed alongside as a cross-check: the two agree per carrier, which is
//! what makes the decoded number a measurement rather than a walker artifact.
//!
//! Run with:
//!   cargo run --release -p legaia-engine-core --example scan_4c_d8

use std::collections::BTreeMap;
use std::path::PathBuf;

use legaia_engine_core::man_field_scripts::{
    CLEAN_RESYNC_INSNS, partition_record_span, scene_man_carriers,
};
use legaia_engine_core::scene::{ProtIndex, Scene};
use legaia_engine_vm::field_disasm::{InsnInfo, LinearWalker};

/// One decoded `0x4C 0xD8` instruction site in a scene MAN.
struct Site {
    /// PROT extraction index of the MAN carrier the site lives in.
    entry_idx: u32,
    /// `true` for a streaming variant carrier, `false` for the bundle MAN.
    variant: bool,
    partition: usize,
    record: usize,
    /// Byte offset of the opcode within the MAN payload.
    abs_pc: usize,
    /// The walk had a clear run-up, so this is not a resync artifact.
    clean: bool,
    /// Bytes the disassembler consumed (9 for this opcode).
    size: usize,
}

fn main() -> anyhow::Result<()> {
    let extracted = PathBuf::from("extracted");
    let index = ProtIndex::open_extracted(&extracted)?;
    let names = index.cdname_scene_names();

    let mut sites: BTreeMap<String, Vec<Site>> = BTreeMap::new();
    // (scene, entry) -> raw `4C D8` byte pairs in the carrier payload.
    let mut raw_pairs: BTreeMap<(String, u32), usize> = BTreeMap::new();
    let mut carriers = 0usize;
    let mut records = 0usize;

    for name in &names {
        let Ok(scene) = Scene::load(&index, name) else {
            continue;
        };
        for carrier in scene_man_carriers(&index, &scene) {
            carriers += 1;
            let man = &carrier.payload;
            let raw = (0..man.len().saturating_sub(1))
                .filter(|&o| man[o] == 0x4C && man[o + 1] == 0xD8)
                .count();
            if raw > 0 {
                raw_pairs.insert((name.clone(), carrier.entry_idx), raw);
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
                        if let InsnInfo::MenuCtrl { op0: 0xD8, .. } = insn.info {
                            sites.entry(name.clone()).or_default().push(Site {
                                entry_idx: carrier.entry_idx,
                                variant: carrier.is_variant(),
                                partition,
                                record,
                                abs_pc: start + insn.pc,
                                clean,
                                size: insn.size,
                            });
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
    for (name, list) in &sites {
        println!("{} sites in '{name}':", list.len());
        for s in list {
            println!(
                "  {} entry={} P{}[{}] pc=0x{:X} size={} {}",
                if s.variant { "variant" } else { "bundle " },
                s.entry_idx,
                s.partition,
                s.record,
                s.abs_pc,
                s.size,
                if s.clean { "clean" } else { "DESYNCED" },
            );
        }
    }
    println!("\nraw `4C D8` byte pairs per carrier (walker-independent):");
    for ((name, entry), raw) in &raw_pairs {
        let decoded = sites
            .get(name)
            .map(|l| l.iter().filter(|s| s.entry_idx == *entry).count())
            .unwrap_or(0);
        let agree = if *raw == decoded { "agree" } else { "DIFFER" };
        println!("  {name} entry={entry}: raw={raw} decoded={decoded} ({agree})");
    }
    let total: usize = sites.values().map(Vec::len).sum();
    println!(
        "\nscenes carrying >= 1 site: {}; total opcode sites: {total}",
        sites.len()
    );
    Ok(())
}
