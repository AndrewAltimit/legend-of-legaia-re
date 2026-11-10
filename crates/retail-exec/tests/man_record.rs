//! The MAN record resolver `FUN_8003C8F0(id, partition)` against
//! `flat_record_span`, over every record of every scene MAN on the disc, each
//! partition's own index space folded onto the flat one the way the resolver
//! folds it, and past the end, where retail hands back its default record.

use crate::common::disc;
use crate::common::scus_cpu;
use legaia_engine_field::man_field_scripts::flat_record_span;
use psx_retail_exec::{Stop, V0};

const RESOLVE: u32 = 0x8003_C8F0;
const MAN_PTR: u32 = 0x8007_B898;
const MAN: u32 = 0x8010_0000;
/// What the resolver returns for a partition past `2` or an id past the table.
const DEFAULT_RECORD: u32 = 0x8007_3EE8;

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
fn every_disc_record_resolves_where_retail_resolves_it() {
    differential!(scus(RESOLVE));
    let (Some(mut cpu), Some(disc)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem.wr32(MAN_PTR, MAN);
    let (mut mans, mut resolved, mut past) = (0, 0, 0);
    for entry in 0..863 {
        let Ok(buf) = disc.read_entry(entry) else {
            continue;
        };
        for man in man_payloads(&buf) {
            let Ok(file) = legaia_scene_formats::man_section::parse(&man) else {
                continue;
            };
            mans += 1;
            cpu.mem.load(MAN, &man);
            let counts: Vec<usize> = file.partitions.iter().map(Vec::len).collect();
            let total: usize = counts.iter().sum();
            for partition in 0..4usize {
                let base: usize = counts.iter().take(partition).sum();
                let n = counts.get(partition).copied().unwrap_or(0);
                for id in 0..n + 3 {
                    cpu.regs.r[V0] = 0xDEAD_BEEF;
                    assert_eq!(
                        cpu.call(RESOLVE, &[id as u32, partition as u32]),
                        Stop::Returned
                    );
                    let r = cpu.regs.r[V0];
                    let flat = base + id;
                    if partition >= 3 || flat >= total {
                        assert_eq!(r, DEFAULT_RECORD, "entry {entry} p{partition} id {id}");
                        past += 1;
                        continue;
                    }
                    // The resolver reads the flat table on through the next
                    // partition's ids, as the port's flat index does.
                    if let Some((start, _, _)) = flat_record_span(&file, &man, flat) {
                        assert_eq!(
                            r,
                            MAN + start as u32,
                            "entry {entry} p{partition} id {id} flat {flat}"
                        );
                        resolved += 1;
                    }
                }
            }
        }
    }
    assert!(
        mans > 80 && resolved > 5000 && past > 300,
        "{mans} {resolved} {past}"
    );
}
