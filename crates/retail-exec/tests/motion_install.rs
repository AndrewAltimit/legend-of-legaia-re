//! The motion-script installer `FUN_8003A9D4` against
//! `man_motion::motion_records`, over every scene MAN on the disc: the walk of
//! MAN tail section 1 and, for every binding of every record, the stream
//! pointer it installs at actor `+0x80` and the byte at `+0x8A` - on the
//! player (`0xF8`), the world-map entity (`0xFB`, the first `0x8007C34C`
//! node whose tick is `FUN_801DA51C`) or the field actor whose placement
//! index `+0x50` matches. Every actor the scene could bind is seated, so an
//! install on the wrong one shows.

use crate::common::{disc, scus_cpu};
use legaia_scene_formats::man_motion::motion_records;
use legaia_scene_formats::man_section;
use psx_retail_exec::Stop;
use std::collections::BTreeMap;

const INSTALL: u32 = 0x8003_A9D4;
const CTRL_PTR: u32 = 0x801C_6EA4;
const CTRL: u32 = 0x8016_0000;
const PLAYER_PTR: u32 = 0x8007_C364;
const ENTITY_LIST: u32 = 0x8007_C34C;
const FIELD_LIST: u32 = 0x8007_C354;
const WORLD_MAP_TICK: u32 = 0x801D_A51C;
const MAN: u32 = 0x8010_0000;
const NODES: u32 = 0x8017_0000;
const NODE: u32 = 0x100;
const PLACEMENTS: u32 = 64;

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

/// Node `i`'s address: `0` the player, `1` a world-map node with another
/// tick, `2` the world-map entity, `3..` field actors with placement `i - 3`.
fn node(i: u32) -> u32 {
    NODES + i * NODE
}

#[test]
fn every_scene_installs_its_motion_scripts_where_retail_does() {
    differential!(scus(INSTALL));
    let (Some(mut cpu), Some(disc)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem.wr32(CTRL_PTR, CTRL);
    let (mut scenes, mut installs, mut fails) = (0, 0, Vec::new());
    for entry in 0..863 {
        let Ok(buf) = disc.read_entry(entry) else {
            continue;
        };
        for man in man_payloads(&buf) {
            let Ok(file) = man_section::parse(&man) else {
                continue;
            };
            let sec = &file.sections[1];
            if sec.is_terminator() {
                continue;
            }
            cpu.mem.load(MAN, &man);
            cpu.mem.wr32(CTRL, MAN + sec.body_offset() as u32);
            // Seat every actor with poison in the two fields.
            let total = 3 + PLACEMENTS;
            for i in 0..total {
                let n = node(i);
                cpu.mem.wr32(n + 0x80, 0xDEAD_BEEF);
                cpu.mem.wr8(n + 0x8A, 0xEE);
            }
            cpu.mem.wr32(PLAYER_PTR, node(0));
            cpu.mem.wr32(ENTITY_LIST, ENTITY_LIST + 0x100);
            cpu.mem.wr32(ENTITY_LIST + 0x100, node(1));
            cpu.mem.wr32(node(1), node(2));
            cpu.mem.wr32(node(1) + 0xC, 0x801D_0000);
            cpu.mem.wr32(node(2), 0);
            cpu.mem.wr32(node(2) + 0xC, WORLD_MAP_TICK);
            cpu.mem.wr32(FIELD_LIST, FIELD_LIST + 0x200);
            cpu.mem.wr32(FIELD_LIST + 0x200, node(3));
            for p in 0..PLACEMENTS {
                let n = node(3 + p);
                cpu.mem
                    .wr32(n, if p + 1 < PLACEMENTS { node(4 + p) } else { 0 });
                cpu.mem.wr16(n + 0x50, p as u16);
            }
            assert_eq!(cpu.call(INSTALL, &[]), Stop::Returned);
            let retail: Vec<(u32, u8)> = (0..total)
                .map(|i| (cpu.mem.rd32(node(i) + 0x80), cpu.mem.rd8(node(i) + 0x8A)))
                .collect();

            let mut port: BTreeMap<u32, (u32, u8)> = BTreeMap::new();
            for rec in motion_records(&man, &file) {
                for b in &rec.bindings {
                    let target = match b.actor_id {
                        0xF8 => Some(0),
                        0xFB => Some(2),
                        id if u32::from(id) < PLACEMENTS => Some(3 + u32::from(id)),
                        _ => None,
                    };
                    if let Some(t) = target {
                        port.insert(t, (MAN + rec.stream_offset as u32, b.enable));
                        installs += 1;
                    }
                }
            }
            let port: Vec<(u32, u8)> = (0..total)
                .map(|i| port.get(&i).copied().unwrap_or((0xDEAD_BEEF, 0xEE)))
                .collect();
            scenes += 1;
            if retail != port && fails.len() < 6 {
                let diffs: Vec<String> = retail
                    .iter()
                    .zip(&port)
                    .enumerate()
                    .filter(|(_, (a, b))| a != b)
                    .take(4)
                    .map(|(i, (a, b))| format!("node {i}: retail {a:x?} port {b:x?}"))
                    .collect();
                fails.push(format!("PROT {entry}: {diffs:?}"));
            }
        }
    }
    assert!(fails.is_empty(), "{}", fails.join("\n"));
    assert!(scenes > 50 && installs > 100, "{scenes} {installs}");
}
