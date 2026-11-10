//! Three field-overlay (PROT 0897) spawners against their ports, each a call
//! to the pool allocator `FUN_80020DE0` and a handful of stores on the record
//! it returns: the shutter blackout `FUN_801DE754` (`ShutterBars::spawn`, op
//! `43 0C`), the screen frame `FUN_801DE7BC` (`ScreenFrame::spawn`, ops
//! `43 0D` / `43 0F`) and the countdown HUD's install / remove `FUN_801D596C`
//! (`countdown_actor_spawn`, op `4C D3`).
//!
//! The allocator answers a record of junk bytes, or null, and checks the
//! template and the list head it is handed; the whole record is compared, so
//! a store the port does not know about shows.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_core::world::ScreenFrame;
use legaia_engine_field::escape_timer::{
    COUNTDOWN_HUD_POS, COUNTDOWN_HUD_POS_B, COUNTDOWN_HUD_SCALE, CountdownSpawn,
    countdown_actor_spawn,
};
use legaia_engine_field::field_actor_timers::ShutterBars;
use psx_retail_exec::{A0, A1, Cpu, Stop, V0};
use std::cell::Cell;
use std::rc::Rc;

const OVERLAY_BASE: u32 = 0x801C_E818;
const SHUTTER: u32 = 0x801D_E754;
const FRAME: u32 = 0x801D_E7BC;
const COUNTDOWN: u32 = 0x801D_596C;
const ALLOC: u32 = 0x8002_0DE0;
const FIND: u32 = 0x8003_CF04;
const HUD_TICK: u32 = 0x801D_2EBC;
const ARMED: u32 = 0x8008_45B8;
const POOL_LISTS: u32 = 0x8007_C348;
const ACTOR: u32 = 0x8013_0000;
const LIVE: u32 = 0x8013_1000;
const STACK_TOP: u32 = 0x801F_FF00;
const RECORD: usize = 0x100;

fn cpu() -> Option<Cpu> {
    let mut cpu = scus_cpu()?;
    cpu.mem
        .load(OVERLAY_BASE, &disc()?.read_entry(897).expect("PROT 0897"));
    Some(cpu)
}

/// The allocator answering `ACTOR` (filled with `junk`) or null, checking the
/// template and the list head.
fn allocator(cpu: &mut Cpu, template: u32, head: u32, ok: Rc<Cell<bool>>, junk: Rc<Cell<u8>>) {
    cpu.hook(ALLOC, move |r, m| {
        assert_eq!((r.r[A0], r.r[A1]), (template, head), "allocator arguments");
        if ok.get() {
            m.load(ACTOR, &[junk.get(); RECORD]);
        }
        r.r[V0] = if ok.get() { ACTOR } else { 0 };
    });
}

fn put16(rec: &mut [u8], at: usize, v: u16) {
    rec[at..at + 2].copy_from_slice(&v.to_le_bytes());
}

#[test]
fn the_shutter_spawn_matches_retail() {
    differential!(prot(897, SHUTTER));
    let Some(mut cpu) = cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let head = 0x8014_0000;
    cpu.mem.wr32(POOL_LISTS + 4, head);
    let (ok, junk) = (Rc::new(Cell::new(true)), Rc::new(Cell::new(0)));
    allocator(&mut cpu, 0x801F_2858, head, ok.clone(), junk.clone());
    let mut rng = Rng(0x5348_5554);
    let mut refused = 0;
    for run in 0..3000 {
        let d: [u8; 3] = std::array::from_fn(|_| rng.next() as u8);
        ok.set(rng.below(8) != 0);
        junk.set(rng.next() as u8);
        cpu.mem.load(ACTOR, &[0xEE; RECORD]);
        let args = d.map(u32::from);
        assert_eq!(cpu.call(SHUTTER, &args), Stop::Returned, "run {run}");
        let got = cpu.mem.read(ACTOR, RECORD);
        if !ok.get() {
            assert_eq!(got, [0xEE; RECORD], "run {run}: a refused spawn stores");
            refused += 1;
            continue;
        }
        let bars = ShutterBars::spawn(i16::from(d[0]), i16::from(d[1]), i16::from(d[2]));
        let mut want = vec![junk.get(); RECORD];
        put16(&mut want, 0x54, bars.phase as u16);
        put16(&mut want, 0x9E, bars.clock);
        for (i, at) in [0xB8, 0xBA, 0xBC].into_iter().enumerate() {
            put16(&mut want, at, bars.durations[i] as u16);
        }
        assert_eq!(got, want, "run {run} {d:?}");
    }
    assert!(refused > 200, "{refused}");
}

#[test]
fn the_screen_frame_spawn_matches_retail() {
    differential!(prot(897, FRAME));
    let Some(mut cpu) = cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let head = 0x8014_0000;
    cpu.mem.wr32(POOL_LISTS + 4, head);
    let (ok, junk) = (Rc::new(Cell::new(true)), Rc::new(Cell::new(0)));
    allocator(&mut cpu, 0x801F_2870, head, ok.clone(), junk.clone());
    let mut rng = Rng(0x4652_414D);
    let mut refused = 0;
    for run in 0..3000 {
        let b: [u8; 4] = std::array::from_fn(|_| rng.next() as u8);
        // The two ops' own OT entries, and any other byte.
        let ot = [3u8, 0, rng.next() as u8][rng.below(3) as usize];
        ok.set(rng.below(8) != 0);
        junk.set(rng.next() as u8);
        cpu.mem.load(ACTOR, &[0xEE; RECORD]);
        // The fifth argument: `0x38` past the routine's own frame of `0x28`.
        cpu.mem.wr32(STACK_TOP + 0x10, u32::from(ot));
        let args = b.map(u32::from);
        assert_eq!(cpu.call(FRAME, &args), Stop::Returned, "run {run}");
        let got = cpu.mem.read(ACTOR, RECORD);
        if !ok.get() {
            assert_eq!(cpu.regs.r[V0], 0, "run {run}");
            assert_eq!(got, [0xEE; RECORD], "run {run}: a refused spawn stores");
            refused += 1;
            continue;
        }
        assert_eq!(cpu.regs.r[V0], ACTOR, "run {run}");
        let frame = ScreenFrame::spawn(b[0], [b[1], b[2], b[3]], ot);
        let mut want = vec![junk.get(); RECORD];
        put16(&mut want, 0x72, u16::from(frame.width));
        for (i, at) in [0xB8, 0xBA, 0xBC].into_iter().enumerate() {
            put16(&mut want, at, u16::from(frame.rgb[i]));
        }
        put16(&mut want, 0x50, u16::from(frame.ot));
        assert_eq!(got, want, "run {run} {b:?} ot {ot}");
    }
    assert!(refused > 200, "{refused}");
}

#[test]
fn the_countdown_install_matches_retail() {
    differential!(prot(897, COUNTDOWN));
    let Some(mut cpu) = cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let head = 0x8014_0000;
    cpu.mem.wr32(POOL_LISTS + 4, head);
    let (ok, junk) = (Rc::new(Cell::new(true)), Rc::new(Cell::new(0)));
    allocator(&mut cpu, 0x801F_22DC, head, ok.clone(), junk.clone());
    let live = Rc::new(Cell::new(false));
    let l = live.clone();
    cpu.hook(FIND, move |r, _| {
        assert_eq!((r.r[A0], r.r[A1]), (head, HUD_TICK), "lookup arguments");
        r.r[V0] = if l.get() { LIVE } else { 0 };
    });
    let mut rng = Rng(0x434E_5444);
    let (mut retired, mut spawned, mut kept, mut refused) = (0, 0, 0, 0);
    for run in 0..4000 {
        let armed = match rng.below(3) {
            0 => 0,
            1 => rng.next() as i32,
            _ => 1 + rng.below(10_000) as i32,
        };
        live.set(rng.below(2) == 0);
        ok.set(rng.below(8) != 0);
        junk.set(rng.next() as u8);
        let live_flags = rng.next() & !8;
        cpu.mem.wr32(ARMED, armed as u32);
        cpu.mem.wr32(LIVE + 0x10, live_flags);
        cpu.mem.load(ACTOR, &[0xEE; RECORD]);
        assert_eq!(cpu.call(COUNTDOWN, &[]), Stop::Returned, "run {run}");
        let got = cpu.mem.read(ACTOR, RECORD);
        let ctx = format!("run {run} armed {armed} live {}", live.get());
        let port = countdown_actor_spawn(armed, live.get());
        let retire = port == CountdownSpawn::Retire;
        assert_eq!(
            cpu.mem.rd32(LIVE + 0x10),
            live_flags | if retire { 8 } else { 0 },
            "the live actor's kill bit: {ctx}"
        );
        match port {
            CountdownSpawn::Spawn(hud) if ok.get() => {
                assert_eq!(cpu.regs.r[V0], ACTOR, "{ctx}");
                let mut want = vec![junk.get(); RECORD];
                put16(&mut want, 0x14, COUNTDOWN_HUD_POS.0 as u16);
                put16(&mut want, 0x16, COUNTDOWN_HUD_POS.1 as u16);
                put16(&mut want, 0x24, COUNTDOWN_HUD_POS_B.0 as u16);
                put16(&mut want, 0x26, COUNTDOWN_HUD_POS_B.1 as u16);
                put16(&mut want, 0x54, u16::from(hud.phase));
                put16(&mut want, 0x68, hud.hold as u16);
                put16(&mut want, 0x6A, COUNTDOWN_HUD_SCALE as u16);
                assert_eq!(got, want, "{ctx}");
                spawned += 1;
            }
            other => {
                assert_eq!(cpu.regs.r[V0], 0, "{ctx}");
                assert_eq!(got, [0xEE; RECORD], "no spawn: {ctx}");
                retired += u32::from(retire);
                kept += u32::from(other == CountdownSpawn::Leave && armed != 0);
                refused += u32::from(matches!(other, CountdownSpawn::Spawn(_)));
            }
        }
    }
    assert!(
        retired > 200 && spawned > 200 && kept > 200 && refused > 20,
        "{retired} {spawned} {kept} {refused}"
    );
}
