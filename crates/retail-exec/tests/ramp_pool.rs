//! The SCUS property-ramp pool at `0x801C66A0` against `RampScheduler`, as a
//! stream of installs and frame ticks: the pool reset `FUN_8003CDA8`, the
//! first-free installer `FUN_8003C5F0` with its overflow counter
//! `DAT_80073ED0`, and the ticker `FUN_80036D80` over all four store widths
//! (the packed-RGB lanes included) at every frame step. Every destination the
//! ramps write, and which slots stand busy, has to match after every
//! operation.
//!
//! Each ramp gets a destination of its own and no owner: the ticker walks its
//! list in install order and the port its slots in index order, which only
//! differ for two live ramps on one destination; the owner kill is the host's
//! `free_owner` in the port.

use crate::common::{Rng, scus_cpu};
use legaia_engine_motion_vm::ambient_motion::{Ramp, RampKind, RampScheduler};
use psx_retail_exec::{Stop, V0};

const RESET: u32 = 0x8003_CDA8;
const INSTALL: u32 = 0x8003_C5F0;
const TICK: u32 = 0x8003_6D80;
const ALLOC: u32 = 0x8002_0DE0;
const POOL: u32 = 0x801C_66A0;
const OVERFLOW: u32 = 0x8007_3ED0;
const FRAME_STEP: u32 = 0x1F80_0393;
const DESTS: u32 = 0x8014_0000;
const STACK_TOP: u32 = 0x801F_FF00;

fn kind_code(k: RampKind) -> u32 {
    match k {
        RampKind::U8 => 1,
        RampKind::U16 => 2,
        RampKind::Rgb => 3,
        RampKind::U32 => 4,
    }
}

#[test]
fn the_ramp_pool_writes_what_retail_writes() {
    differential!(scus(RESET), scus(INSTALL), scus(TICK));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.stub(ALLOC, 0);
    let mut rng = Rng(0x5241_4D50);
    let (mut installs, mut overflows, mut finished, mut rgb) = (0, 0, 0, 0);
    for session in 0..40 {
        assert_eq!(cpu.call(RESET, &[]), Stop::Returned);
        cpu.mem.wr32(OVERFLOW, 0);
        let mut port = RampScheduler::new();
        port.reset_pool();
        // One word per destination, each written by one ramp at a time.
        let mut dests = vec![0u32; 256];
        let mut free: Vec<u32> = (0..256).collect();
        for (i, d) in dests.iter_mut().enumerate() {
            *d = rng.next();
            cpu.mem.wr32(DESTS + i as u32 * 4, *d);
        }
        let mut live: Vec<u32> = Vec::new();
        for op in 0..1500 {
            // Odd sessions install faster than ramps retire and fill the pool.
            let install = if session % 2 == 1 {
                rng.below(5) != 0
            } else {
                rng.below(3) == 0
            };
            if install && !free.is_empty() {
                let d = free.swap_remove(rng.below(free.len() as u32) as usize);
                let kind = [RampKind::U8, RampKind::U16, RampKind::Rgb, RampKind::U32]
                    [rng.below(4) as usize];
                let total = [1i32, 2, 5, 30, 0x100][rng.below(5) as usize];
                let ramp = Ramp {
                    dest: DESTS + d * 4,
                    owner: 0,
                    start: rng.next() as i32 >> rng.below(16),
                    end: rng.next() as i32 >> rng.below(16),
                    total,
                    remaining: total,
                    kind,
                };
                cpu.mem.wr32(STACK_TOP + 0x10, ramp.end as u32);
                cpu.mem.wr32(STACK_TOP + 0x14, total as u32);
                cpu.regs.r[V0] = 0xDEAD_BEEF;
                let stop = cpu.call(INSTALL, &[0, ramp.dest, kind_code(kind), ramp.start as u32]);
                assert_eq!(stop, Stop::Returned);
                let ok = port.install(ramp).is_some();
                assert_eq!(
                    cpu.regs.r[V0],
                    u32::from(ok),
                    "session {session} op {op} install"
                );
                if ok {
                    live.push(d);
                    installs += 1;
                    rgb += u32::from(kind == RampKind::Rgb);
                } else {
                    free.push(d);
                    overflows += 1;
                }
            } else {
                let step = [1u8, 1, 2, 3, 4][rng.below(5) as usize];
                cpu.mem.wr8(FRAME_STEP, step);
                assert_eq!(cpu.call(TICK, &[]), Stop::Returned);
                for w in port.tick(step) {
                    let i = ((w.dest - DESTS) / 4) as usize;
                    let v = w.value as u32;
                    dests[i] = match w.kind {
                        RampKind::U8 => dests[i] & !0xFF | v & 0xFF,
                        RampKind::U16 => dests[i] & !0xFFFF | v & 0xFFFF,
                        RampKind::Rgb | RampKind::U32 => v,
                    };
                    if w.finished {
                        finished += 1;
                        let d = i as u32;
                        live.retain(|x| *x != d);
                        free.push(d);
                    }
                }
            }
            for (i, d) in dests.iter().enumerate() {
                assert_eq!(
                    cpu.mem.rd32(DESTS + i as u32 * 4),
                    *d,
                    "session {session} op {op} dest {i}"
                );
            }
            let busy = (1..64)
                .filter(|k| cpu.mem.rd16(POOL + k * 0x20 + 0x1E) != 0)
                .count();
            assert_eq!(busy, port.active(), "session {session} op {op}");
            assert_eq!(
                cpu.mem.rd32(OVERFLOW),
                port.overflow,
                "session {session} op {op}"
            );
        }
    }
    assert!(
        installs > 5000 && overflows > 100 && finished > 5000 && rgb > 1000,
        "{installs} {overflows} {finished} {rgb}"
    );
}
