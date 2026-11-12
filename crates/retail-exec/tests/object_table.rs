//! The object-table stamp `FUN_80024D78` over every Legaia TMD the raw PROT
//! entries carry, seated in the global TMD pool `DAT_8007C018` at the slot the
//! actor's `+0x64` names, against what the port renders from: the actor's
//! `tmd_ref` resolved through `World::global_tmd` and
//! `ActorAllocatorHost::rebuild_object_table`. Retail's `+0x44` table is the
//! object count followed by one pointer per object header (`+0xC + 0x1C * i`);
//! the port walks the parsed `objects` in the same order, so the count and
//! each header's place are what the two must share. The `+0x10` renderable
//! bit is the port's `active`.

use crate::common::{Rng, disc, scus_cpu};
use legaia_container_formats::tmd_scan::scan_buffer;
use legaia_engine_core::world::World;
use legaia_engine_scene::model_bank::GlobalTmd;
use legaia_engine_vm::actor_alloc::{ActorAllocatorHost, SpawnPosition};
use psx_retail_exec::{Stop, V0};
use std::sync::Arc;

const STAMP: u32 = 0x8002_4D78;
const POOL: u32 = 0x8007_C018;
const TMD: u32 = 0x8010_0000;
const ACTOR: u32 = 0x8013_0000;
const TABLE: u32 = 0x8013_1000;

#[test]
fn the_object_table_is_the_objects_the_port_walks() {
    differential!(scus(STAMP));
    let (Some(mut cpu), Some(d)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut rng = Rng(0x4F42_4A54);
    let (mut tmds, mut objects) = (0usize, 0usize);
    for entry in 0..1233 {
        let Ok(buf) = d.read_entry(entry) else {
            continue;
        };
        for hit in scan_buffer(&buf).into_iter().take(64) {
            let bytes = &buf[hit.offset..hit.offset + hit.byte_len];
            if bytes.len() > 0xF_0000 {
                continue;
            }
            let Ok(tmd) = legaia_tmd::parse(bytes) else {
                continue;
            };
            let ctx = format!("entry {entry} offset {:#x}", hit.offset);
            let slot = rng.below(0x40) as i16;
            cpu.mem.load(TMD, bytes);
            cpu.mem.wr32(POOL + 4 * slot as u32, TMD);
            let noise: Vec<u8> = (0..0x100).map(|_| rng.next() as u8).collect();
            cpu.mem.load(ACTOR, &noise);
            cpu.mem.wr16(ACTOR + 0x64, slot as u16);
            cpu.mem.wr32(ACTOR + 0x44, TABLE);
            let flags = cpu.mem.rd32(ACTOR + 0x10);
            assert_eq!(cpu.call(STAMP, &[ACTOR]), Stop::Returned, "{ctx}");
            assert_eq!(cpu.regs.r[V0], 1, "{ctx}");
            assert_eq!(cpu.mem.rd32(ACTOR + 0x10), flags | 0x0800_0000, "{ctx}");
            let n = cpu.mem.rd32(TABLE) as usize;
            let headers: Vec<u32> = (0..n)
                .map(|k| cpu.mem.rd32(TABLE + 4 + 4 * k as u32) - TMD)
                .collect();

            let mut w = World::new();
            w.set_global_tmd(
                slot as usize,
                Arc::new(GlobalTmd {
                    tmd,
                    raw: bytes.to_vec(),
                }),
            );
            let a = w
                .spawn_at_position(SpawnPosition::default(), 0, 0)
                .expect("a slot");
            w.actors[a as usize].active = false;
            w.actors[a as usize].tmd_ref = w.global_tmd(slot).cloned();
            assert!(w.rebuild_object_table(a), "{ctx}");
            assert!(w.actors[a as usize].active, "{ctx}: renderable");
            let walked = &w.actors[a as usize].tmd_ref.as_ref().unwrap().tmd.objects;
            assert_eq!(n, walked.len(), "{ctx}: object count");
            // Each object the port walks is parsed from the header retail
            // points at: its primitive pool sits where that header's
            // `primitive_top` says.
            for (k, o) in walked.iter().enumerate() {
                assert_eq!(headers[k], 12 + 0x1C * k as u32, "{ctx}: object {k}");
                let prim_top = u32::from_le_bytes(
                    bytes[headers[k] as usize + 0x10..headers[k] as usize + 0x14]
                        .try_into()
                        .unwrap(),
                );
                assert_eq!(
                    (prim_top & !3) + 12,
                    o.primitives_byte_offset as u32,
                    "{ctx}: object {k} primitives"
                );
                objects += 1;
            }
            tmds += 1;
        }
    }
    assert!(
        tmds > 400 && objects > 1000,
        "{tmds} TMDs, {objects} objects"
    );
}
