//! The partition-2 record dispatcher `FUN_8003BDE0` - the record locate, the
//! name and C0 skip, the C1-any / C2-all story-flag gates and the script entry
//! it installs - against `partition_record_span` and
//! `World::p2_record_gates_pass`, over every partition-2 record of every scene
//! MAN on the disc, with the gates' flags drawn up and down.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_core::world::World;
use legaia_engine_field::man_field_scripts::{partition_record_span, partition2_record_gates};
use psx_retail_exec::{Stop, V0};

const DISPATCH: u32 = 0x8003_BDE0;
const ALLOC: u32 = 0x8002_0DE0;
const MOTION_PAUSE: u32 = 0x8003_C9AC;
const MAN_PTR: u32 = 0x8007_B898;
const MAN: u32 = 0x8010_0000;
const FLAGS: u32 = 0x8008_5758;
const CTX: u32 = 0x8015_0000;
const RUNNER: u32 = 0x801C_6EA4;
const RUNNER_BLOCK: u32 = 0x8016_0000;

fn man_payloads(buf: &[u8]) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    if let Some(table) = legaia_scene_formats::scene_asset_table::detect(buf) {
        for d in table.descriptors.iter().filter(|d| d.type_byte == 0x03) {
            let start = d.data_offset as usize;
            if start < buf.len()
                && let Ok((decoded, _)) =
                    psx_lzs::decompress_tracked(&buf[start..], d.size as usize)
            {
                out.push(decoded);
            }
        }
    }
    out
}

#[test]
fn every_disc_record_spawns_where_retail_spawns_it() {
    let (Some(mut cpu), Some(disc)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.stub(ALLOC, CTX);
    cpu.stub(MOTION_PAUSE, 0);
    cpu.mem.wr32(RUNNER, RUNNER_BLOCK);
    cpu.mem.wr32(MAN_PTR, MAN);
    let mut rng = Rng(0x5350_4157);
    let (mut failures, mut records, mut spawned, mut blocked) = (Vec::new(), 0, 0, 0);
    let mut w = World::new();
    // The scenes end before the battle data.
    for entry in 0..863 {
        let Ok(buf) = disc.read_entry(entry) else {
            continue;
        };
        for man in man_payloads(&buf) {
            let Ok(file) = legaia_scene_formats::man_section::parse(&man) else {
                continue;
            };
            let n2 = file.partitions.get(2).map_or(0, |p| p.len());
            if n2 == 0 {
                continue;
            }
            cpu.mem.load(MAN, &man);
            for index in 0..n2 {
                let Some((c1, c2)) = partition2_record_gates(&file, &man, index) else {
                    continue;
                };
                let span = partition_record_span(&file, &man, 2, index);
                for _ in 0..4 {
                    records += 1;
                    // Each listed flag up or down, over a random bank.
                    let mut bank = [0u8; 0x200];
                    for b in bank.iter_mut() {
                        *b = if rng.below(4) == 0 {
                            rng.below(256) as u8
                        } else {
                            0
                        };
                    }
                    let up = rng.below(3);
                    for &f in c1.iter().chain(&c2) {
                        let bit = 0x80u8 >> (f & 7);
                        let at = usize::from(f >> 3);
                        if at < bank.len() {
                            match up {
                                0 => bank[at] &= !bit,
                                1 => bank[at] |= bit,
                                _ if rng.below(2) == 0 => bank[at] |= bit,
                                _ => bank[at] &= !bit,
                            }
                        }
                    }
                    cpu.mem.load(FLAGS, &bank);
                    w.flags.system_flags = bank.to_vec();
                    for b in 0..0xA0 {
                        cpu.mem.wr8(CTX + b, 0);
                    }
                    cpu.regs.r[V0] = 0;
                    let stop = cpu.call(DISPATCH, &[0, 0, index as u32, 1]);
                    assert_eq!(stop, Stop::Returned, "entry {entry} record {index}");
                    let retail = cpu.calls.contains(&ALLOC).then(|| cpu.mem.rd16(CTX + 0x9E));
                    let port = w
                        .p2_record_gates_pass(&c1, &c2)
                        .then(|| span.map_or(0, |(_, pc0, _)| pc0 as u16));
                    spawned += u32::from(retail.is_some());
                    blocked += u32::from(retail.is_none());
                    if retail != port && failures.len() < 8 {
                        failures.push(format!(
                            "entry {entry} record {index} c1 {c1:x?} c2 {c2:x?}: retail {retail:?} port {port:?}"
                        ));
                    }
                }
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert!(
        records > 1000 && spawned > 200 && blocked > 200,
        "{records} {spawned} {blocked}"
    );
}
