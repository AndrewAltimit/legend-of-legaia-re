//! The actor render binding `FUN_80020F88` against `bind_actor_render`: the
//! refresh and re-pick arms off the scene's placement records, the bound
//! check, the node request with its failure, the seeds on the node and the
//! mesh-chain follow-through, with the allocator and the chain builder
//! scripted.

use crate::common::{Rng, scus_cpu};
use legaia_engine_scene::actor_bind::{
    ActorBinding, ActorChainAction, ActorNodeAction, FLAG_NO_CHAIN, FLAG_NODE_READY,
    FLAG_REFRESH, FLAG_REPICK_KIND, PlacementRecord, RENDER_NODE_BYTES, RENDER_NODE_SEED,
    bind_actor_render, on_render_node_alloc_failure,
};
use psx_retail_exec::{A0, A1, Stop, V0};
use std::cell::RefCell;
use std::rc::Rc;

const BIND: u32 = 0x8002_0F88;
const ALLOC: u32 = 0x8001_7888;
const CHAIN_BUILD: u32 = 0x8002_4D78;
const CHAIN_FREE: u32 = 0x8002_04A4;
const PRINTF: u32 = 0x8005_67A8;
const RECORDS_PTR: u32 = 0x1F80_03EC;
const MESH_PREFIX: u32 = 0x8007_B6F8;
const POOL_LEN: u32 = 0x8007_BB38;
const STATUS: u32 = 0x8007_B828;
const ACTOR: u32 = 0x8010_0000;
const NODE: u32 = 0x8010_1000;
const OLD_NODE: u32 = 0x8010_2000;
const RECORDS: u32 = 0x8011_0000;
const N_RECORDS: usize = 64;

#[derive(Default)]
struct Script {
    alloc_ok: bool,
    chain_ok: bool,
    allocs: Vec<u32>,
    builds: u32,
    frees: u32,
    prints: u32,
}

#[test]
fn the_bind_resolves_what_the_port_resolves() {
    differential!(scus(BIND));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let script: Rc<RefCell<Script>> = Rc::default();
    {
        let s = script.clone();
        cpu.hook(ALLOC, move |r, _| {
            let mut s = s.borrow_mut();
            s.allocs.push(r.r[A1]);
            r.r[V0] = if s.alloc_ok { NODE } else { 0 };
        });
    }
    {
        let s = script.clone();
        cpu.hook(CHAIN_BUILD, move |r, _| {
            let mut s = s.borrow_mut();
            assert_eq!(r.r[A0], ACTOR);
            s.builds += 1;
            r.r[V0] = u32::from(s.chain_ok);
        });
    }
    {
        let s = script.clone();
        cpu.hook(CHAIN_FREE, move |r, _| {
            assert_eq!(r.r[A0], ACTOR);
            s.borrow_mut().frees += 1;
        });
    }
    {
        let s = script.clone();
        cpu.hook(PRINTF, move |_, _| s.borrow_mut().prints += 1);
    }
    let mut rng = Rng(0x4249_4E44);
    let (mut refreshed, mut repicked, mut failed, mut built, mut reseeded, mut none, mut oob) =
        (0, 0, 0, 0, 0, 0, 0);
    for round in 0..6_000 {
        let records: Vec<PlacementRecord> = (0..N_RECORDS)
            .map(|_| PlacementRecord {
                mesh_id: rng.below(32) as u16,
                flags: rng.next() as u16,
                sub_id: rng.next() as u8,
            })
            .collect();
        for (k, r) in records.iter().enumerate() {
            let mut rec: Vec<u8> = (0..0x20).map(|_| rng.next() as u8).collect();
            rec[0x10..0x12].copy_from_slice(&r.mesh_id.to_le_bytes());
            rec[0x12..0x14].copy_from_slice(&r.flags.to_le_bytes());
            rec[0x1E] = r.sub_id;
            cpu.mem.load(RECORDS + 0x20 * k as u32, &rec);
        }
        let prefix = rng.below(16) as u16;
        let pool_len = rng.below(80);
        let flags = (rng.next() & !(FLAG_REFRESH | FLAG_REPICK_KIND | FLAG_NO_CHAIN | FLAG_NODE_READY))
            | if rng.below(2) == 0 { FLAG_REFRESH } else { 0 }
            | if rng.below(2) == 0 { FLAG_REPICK_KIND } else { 0 }
            | if rng.below(3) == 0 { FLAG_NO_CHAIN } else { 0 }
            | if rng.below(2) == 0 { FLAG_NODE_READY } else { 0 };
        let actor = ActorBinding {
            flags,
            record_flags: rng.next() as u16,
            kind: rng.below(10) as u16,
            sub_id: rng.next() as u16,
            slot: rng.below(N_RECORDS as u32) as u16,
            mesh_index: rng.below(N_RECORDS as u32) as u16,
        };
        let mut rec: Vec<u8> = (0..0x100).map(|_| rng.next() as u8).collect();
        rec[0x10..0x14].copy_from_slice(&actor.flags.to_le_bytes());
        rec[0x44..0x48].copy_from_slice(&OLD_NODE.to_le_bytes());
        rec[0x52..0x54].copy_from_slice(&actor.record_flags.to_le_bytes());
        rec[0x56..0x58].copy_from_slice(&actor.kind.to_le_bytes());
        rec[0x58..0x5A].copy_from_slice(&actor.sub_id.to_le_bytes());
        rec[0x60..0x62].copy_from_slice(&actor.slot.to_le_bytes());
        rec[0x64..0x66].copy_from_slice(&actor.mesh_index.to_le_bytes());
        cpu.mem.load(ACTOR, &rec);
        cpu.mem.load(NODE, &[0xA5; 0xA0]);
        cpu.mem.load(OLD_NODE, &[0xA5; 0xA0]);
        cpu.mem.wr32(RECORDS_PTR, RECORDS);
        cpu.mem.wr16(MESH_PREFIX, prefix);
        cpu.mem.wr32(POOL_LEN, pool_len);
        cpu.mem.wr32(STATUS, 0);
        *script.borrow_mut() = Script {
            alloc_ok: rng.below(5) != 0,
            chain_ok: rng.below(3) != 0,
            ..Script::default()
        };
        assert_eq!(cpu.call(BIND, &[ACTOR]), Stop::Returned, "round {round}");
        let ret = cpu.regs.r[V0] as i32;
        let s = script.borrow();

        let port = bind_actor_render(actor, &records, prefix, pool_len);
        let mut want = port.actor;
        let mut status = 0u32;
        let mut node = None;
        let ctx = format!("round {round}: {actor:x?} -> {port:x?}");
        match port.render_node {
            ActorNodeAction::None => {
                assert!(s.allocs.is_empty(), "{ctx}");
                assert_eq!(ret, 0, "{ctx}");
                none += 1;
            }
            ActorNodeAction::Reseed => {
                assert!(s.allocs.is_empty(), "{ctx}");
                node = Some(OLD_NODE);
                reseeded += 1;
            }
            ActorNodeAction::Allocate => {
                assert_eq!(s.allocs, [RENDER_NODE_BYTES as u32], "{ctx}");
                if s.alloc_ok {
                    node = Some(NODE);
                } else {
                    assert_eq!(on_render_node_alloc_failure(&mut want, &mut status), ret, "{ctx}");
                    failed += 1;
                }
            }
        }
        assert_eq!(cpu.mem.rd32(STATUS), status, "{ctx}: status word");
        let got = ActorBinding {
            flags: cpu.mem.rd32(ACTOR + 0x10),
            record_flags: cpu.mem.rd16(ACTOR + 0x52),
            kind: cpu.mem.rd16(ACTOR + 0x56),
            sub_id: cpu.mem.rd16(ACTOR + 0x58),
            slot: cpu.mem.rd16(ACTOR + 0x60),
            mesh_index: cpu.mem.rd16(ACTOR + 0x64),
        };
        // The chain builder's failure frees the actor; its success clears a
        // bit the arm has just tested clear.
        let chained = node.is_some() && port.chain == ActorChainAction::Build;
        assert_eq!(s.builds, u32::from(chained), "{ctx}: chain build");
        assert_eq!(s.frees, u32::from(chained && !s.chain_ok), "{ctx}: chain free");
        assert_eq!(got, want, "{ctx}: actor");
        assert_eq!(s.prints, u32::from(port.mesh_index_out_of_range), "{ctx}: bound check");
        // The node's seeds, and the rest of both blocks left alone.
        let mut want_rec = rec.clone();
        for (off, bytes) in [
            (0x10usize, want.flags.to_le_bytes().to_vec()),
            (0x52, want.record_flags.to_le_bytes().to_vec()),
            (0x56, want.kind.to_le_bytes().to_vec()),
            (0x58, want.sub_id.to_le_bytes().to_vec()),
            (0x64, want.mesh_index.to_le_bytes().to_vec()),
        ] {
            want_rec[off..off + bytes.len()].copy_from_slice(&bytes);
        }
        if port.render_node == ActorNodeAction::Allocate {
            let p = if s.alloc_ok { NODE } else { 0 };
            want_rec[0x44..0x48].copy_from_slice(&p.to_le_bytes());
        }
        if node.is_some() {
            want_rec[0x7C..0x80].copy_from_slice(&0u32.to_le_bytes());
        }
        assert!(cpu.mem.read(ACTOR, 0x100) == want_rec, "{ctx}: actor record");
        for at in [NODE, OLD_NODE] {
            let mut want_node = vec![0xA5u8; 0xA0];
            if node == Some(at) {
                for (off, v) in RENDER_NODE_SEED {
                    want_node[off..off + 2].copy_from_slice(&v.to_le_bytes());
                }
            }
            assert!(cpu.mem.read(at, 0xA0) == want_node, "{ctx}: node {at:#x}");
        }
        refreshed += u32::from(flags & FLAG_REFRESH != 0);
        repicked += u32::from(flags & FLAG_REPICK_KIND != 0);
        built += u32::from(chained);
        oob += u32::from(port.mesh_index_out_of_range);
    }
    assert!(
        refreshed > 2_000
            && repicked > 2_000
            && failed > 100
            && built > 500
            && reseeded > 500
            && none > 500
            && oob > 500,
        "{refreshed} {repicked} {failed} {built} {reseeded} {none} {oob}"
    );
}
