//! The battle overlay's two-mode effect burst `FUN_801F30C4` (PROT 0898,
//! move-VM op `0x17`) against `legaia_engine_vm::battle_burst::run_burst`.
//!
//! `rand` is scripted so both sides draw one stream; the spawner
//! `FUN_80050ED4` is hooked and logs the record, the scale and the rotation
//! scratch it was handed, and returns a fresh child whose two post-spawn
//! stores (`+0x3E`, `+0x98`) are read back.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_vm::battle_burst::{BurstHost, SpawnRequest, run_burst};
use psx_retail_exec::{A0, A1, A2, A3, Stop, V0};
use std::cell::RefCell;
use std::rc::Rc;

const OVERLAY_BASE: u32 = 0x801C_E818;
const BURST: u32 = 0x801F_30C4;
const RAND: u32 = 0x8005_6798;
const SPAWN: u32 = 0x8005_0ED4;
const COS_PTR: u32 = 0x8007_B7F8;
const SIN_PTR: u32 = 0x8007_B81C;

const PARENT: u32 = 0x8012_0000;
const CHILDREN: u32 = 0x8013_0000;
const CHILD_STRIDE: u32 = 0x100;
const COS: u32 = 0x8011_0000;
const SIN: u32 = 0x8011_4000;

struct Host<'a> {
    draws: std::slice::Iter<'a, i32>,
    sin: &'a [i16],
    cos: &'a [i16],
    yaw: i16,
    scale: u16,
    spawned: Vec<(u32, SpawnRequest)>,
}

impl BurstHost for Host<'_> {
    fn rand(&mut self) -> i32 {
        *self.draws.next().expect("the scripted stream")
    }
    fn sin(&self, index: u32) -> i16 {
        self.sin[index as usize & 0xFFF]
    }
    fn cos(&self, index: u32) -> i16 {
        self.cos[index as usize & 0xFFF]
    }
    fn parent_yaw(&self) -> i16 {
        self.yaw
    }
    fn parent_scale(&self) -> u16 {
        self.scale
    }
    fn spawn(&mut self, record_addr: u32, request: SpawnRequest) -> Option<u32> {
        self.spawned.push((record_addr, request));
        Some(self.spawned.len() as u32)
    }
}

#[test]
fn the_battle_burst_matches_retail() {
    differential!(prot(898, BURST));
    let Some(disc) = disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut cpu = scus_cpu().expect("disc present");
    cpu.mem
        .load(OVERLAY_BASE, &disc.read_entry(898).expect("PROT 0898"));
    let mut rng = Rng(0x4255_5253);
    let cos: Vec<i16> = (0..0x1000).map(|_| (rng.below(0x2001) as i32 - 0x1000) as i16).collect();
    let sin: Vec<i16> = (0..0x1000).map(|_| (rng.below(0x2001) as i32 - 0x1000) as i16).collect();
    for i in 0..0x1000u32 {
        cpu.mem.wr16(COS + 2 * i, cos[i as usize] as u16);
        cpu.mem.wr16(SIN + 2 * i, sin[i as usize] as u16);
    }
    cpu.mem.wr32(COS_PTR, COS);
    cpu.mem.wr32(SIN_PTR, SIN);

    let draws = Rc::new(RefCell::new(Vec::<i32>::new()));
    let cursor = Rc::new(RefCell::new(0usize));
    let (d, c) = (draws.clone(), cursor.clone());
    cpu.hook(RAND, move |r, _| {
        let mut c = c.borrow_mut();
        r.r[V0] = d.borrow()[*c] as u32;
        *c += 1;
    });
    // (pos pointer, rotation scratch, record, scale)
    let spawns = Rc::new(RefCell::new(Vec::<(u32, [i16; 4], u32, u32)>::new()));
    let s = spawns.clone();
    cpu.hook(SPAWN, move |r, m| {
        let rot = [0, 2, 4, 6].map(|o| m.rd16(r.r[A1] + o) as i16);
        let n = s.borrow().len() as u32;
        s.borrow_mut().push((r.r[A0], rot, r.r[A2], r.r[A3]));
        r.r[V0] = CHILDREN + CHILD_STRIDE * n;
    });

    let (mut fired, mut idle) = (0, 0);
    for round in 0..4000 {
        let mode = match rng.below(5) {
            0 => 2 + rng.below(8),
            n => n % 2,
        };
        let rot: [i16; 4] = std::array::from_fn(|_| rng.next() as i16);
        let scale = rng.next() as u16;
        let stream: Vec<i32> = (0..64).map(|_| (rng.next() & 0x7FFF) as i32).collect();
        for (i, v) in rot.iter().enumerate() {
            cpu.mem.wr16(PARENT + 0x24 + 2 * i as u32, *v as u16);
        }
        cpu.mem.wr16(PARENT + 0x72, scale);
        for i in 0..12 {
            cpu.mem.wr16(CHILDREN + CHILD_STRIDE * i + 0x3E, 0xA5A5);
            cpu.mem.wr16(CHILDREN + CHILD_STRIDE * i + 0x98, 0xA5A5);
        }
        *draws.borrow_mut() = stream.clone();
        *cursor.borrow_mut() = 0;
        spawns.borrow_mut().clear();
        assert_eq!(cpu.call(BURST, &[PARENT, mode]), Stop::Returned, "round {round}");

        let mut host = Host {
            draws: stream.iter(),
            sin: &sin,
            cos: &cos,
            yaw: rot[1],
            scale,
            spawned: Vec::new(),
        };
        let out = run_burst(&mut host, mode);
        let used = 64 - host.draws.len();
        let ctx = format!("round {round} mode {mode} rot {rot:?} scale {scale:#x}");
        assert_eq!(*cursor.borrow(), used, "rand draws: {ctx}");
        let got = spawns.borrow().clone();
        assert_eq!(got.len(), out.len(), "spawns: {ctx}");
        for (k, ((pos, srot, record, sc), (rec, req))) in got.iter().zip(&host.spawned).enumerate() {
            assert_eq!(*pos, PARENT + 0x14, "spawn {k}: {ctx}");
            assert_eq!(*record, *rec, "spawn {k} record: {ctx}");
            assert_eq!(
                *srot,
                [rot[0], req.yaw, rot[2], rot[3]],
                "spawn {k} rotation scratch: {ctx}"
            );
            assert_eq!(*sc as i32, req.scale, "spawn {k} scale: {ctx}");
            let child = CHILDREN + CHILD_STRIDE * k as u32;
            assert_eq!(cpu.mem.rd16(child + 0x3E) as i16, req.spread, "spawn {k}: {ctx}");
            assert_eq!(cpu.mem.rd16(child + 0x98) as i16, req.tail, "spawn {k}: {ctx}");
        }
        fired += u32::from(!out.is_empty());
        idle += u32::from(out.is_empty());
    }
    assert!(fired > 2500 && idle > 500, "{fired} {idle}");
}
