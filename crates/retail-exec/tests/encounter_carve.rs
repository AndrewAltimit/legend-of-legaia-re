//! The section-0 carve `FUN_8003A110` against
//! `man_section::parse_encounter_section`, over every scene MAN on the disc:
//! the three strides it copies into the encounter control block
//! (`+0x5D..+0x5F`) and the three table pointers it leaves at `+0x20` /
//! `+0x24` / `+0x28` (the formation count byte, the condition count byte, the
//! region count byte), with the rest of the block held to its seed.

use crate::common::{diff_offsets, disc, scus_cpu};
use legaia_scene_formats::man_section::{self, parse_encounter_section};
use psx_retail_exec::Stop;

const CARVE: u32 = 0x8003_A110;
const PRINTF: u32 = 0x8005_67A8;
const CTRL_PTR: u32 = 0x801C_6EA4;
const CTRL: u32 = 0x8016_0000;
const CTRL_LEN: usize = 0x80;
const MAN: u32 = 0x8010_0000;

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
fn every_scene_carves_where_retail_carves() {
    differential!(scus(CARVE));
    let (Some(mut cpu), Some(disc)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.stub(PRINTF, 0);
    cpu.mem.wr32(CTRL_PTR, CTRL);
    let (mut scenes, mut with_regions, mut fails) = (0, 0, Vec::new());
    for entry in 0..863 {
        let Ok(buf) = disc.read_entry(entry) else {
            continue;
        };
        for man in man_payloads(&buf) {
            let Ok(file) = man_section::parse(&man) else {
                continue;
            };
            let sec = file.encounter_section();
            let Some(body) = sec.body(&man) else { continue };
            let Ok(enc) = parse_encounter_section(body) else {
                continue;
            };
            cpu.mem.load(MAN, &man);
            let seed: Vec<u8> = (0..CTRL_LEN).map(|i| (i * 29 + entry) as u8).collect();
            cpu.mem.load(CTRL, &seed);
            cpu.mem.wr32(CTRL + 0x20, MAN + sec.offset as u32);
            let mut want = cpu.mem.read(CTRL, CTRL_LEN);
            assert_eq!(cpu.call(CARVE, &[]), Stop::Returned);
            let retail = cpu.mem.read(CTRL, CTRL_LEN);

            let base = MAN + sec.body_offset() as u32;
            let mut put = |o: usize, v: u32| want[o..o + 4].copy_from_slice(&v.to_le_bytes());
            put(0x20, base + enc.formation_range.0 as u32 - 1);
            put(0x24, base + enc.condition_range.0 as u32 - 1);
            put(0x28, base + enc.region_range.0 as u32 - 1);
            want[0x5D] = enc.formation_stride;
            want[0x5E] = enc.condition_stride;
            want[0x5F] = enc.region_stride;
            scenes += 1;
            with_regions += u32::from(enc.region_count > 0);
            if retail != want && fails.len() < 6 {
                fails.push(format!("PROT {entry}: {:?}", diff_offsets(&retail, &want)));
            }
        }
    }
    assert!(fails.is_empty(), "{}", fails.join("\n"));
    assert!(scenes > 80 && with_regions > 20, "{scenes} {with_regions}");
}
