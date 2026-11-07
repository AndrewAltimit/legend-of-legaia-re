//! The SFX cue ring over whole runs of frames: the enqueue trio `FUN_80035B50` /
//! `FUN_80035BAC` / `FUN_80035BD0`, the aging loop of `FUN_8001698C`
//! (`0x80016AF4..0x80016B58`) and the drainer `FUN_80016B6C`, against the port's
//! `SfxScheduler` ring and the `SfxBank` voice choice the session keys a drained
//! cue through.
//!
//! The drainer's two libsnd calls are hooked and logged: `FUN_800653C8(voice)`
//! (key off) and `FUN_80065034(voice, vab, program, tone, note, ...)` (key on).
//! The port's side keys a synthetic bank whose every `(program, tone)` names its
//! own sample address, so the SPU note trace says which tone each voice took.

use crate::common::{Rng, scus_cpu};
use legaia_engine_audio::spu::Spu;
use legaia_engine_audio::vab_bind::{UploadedVag, VabBank, VabProgram};
use legaia_engine_audio::{SfxBank, SfxScheduler};
use psx_retail_exec::{A0, A2, A3, Cpu, Stop};
use std::cell::RefCell;
use std::rc::Rc;

const DRAIN: u32 = 0x8001_6B6C;
/// `0x80016EA4`: the ring loop's exit, where the frame tail begins.
const DRAIN_LOOP_EXIT: u32 = 0x8001_6EA4;
const AGE_LOOP: u32 = 0x8001_6AF4;
const PUSH: u32 = 0x8003_5B50;
const SET_DELAY: u32 = 0x8003_5BAC;
const REPLACE: u32 = 0x8003_5BD0;
const KEY_OFF: u32 = 0x8006_53C8;
const KEY_ON: u32 = 0x8006_5034;
/// The ring loop's head, once per slot: the back-edge at `0x80016E9C` lands
/// here, and the slot's first instruction is `lui v0,0x8008`.
const DRAIN_SLOT: u32 = 0x8001_6BFC;

const GP: u32 = 0x8007_B318;
const RING_IDS: u32 = 0x8007_B6D8;
const RING_TIMERS: u32 = 0x8007_C338;
const MIXER: u32 = 0x8009_1508;
const RUNTIME_BANK: u32 = 0x8011_0000;
const STACK_TOP: u32 = 0x801F_FF00;

/// One libsnd call the drainer made, or one SPU event the port's bank made.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ev {
    /// The drainer starts on a ring slot (`0x80016BF8`, the loop head).
    Cue,
    Off(u8),
    /// `(voice, program, tone)`.
    On(u8, u8, u8),
}

const PROGRAMS: usize = 16;
const TONES: usize = 32;

/// A bank whose tone `(p, t)` plays sample `p * TONES + t`, at a unique address.
fn tagged_bank() -> VabBank {
    let tone = |vag: i16| psx_vab::VagAtr {
        prior: 0,
        mode: 0,
        vol: 127,
        pan: 64,
        center: 60,
        shift: 0,
        min: 0,
        max: 127,
        vibw: 0,
        vibt: 0,
        porw: 0,
        port: 0,
        pbmin: 0,
        pbmax: 0,
        reserved1: 0,
        reserved2: 0,
        adsr1: 0,
        adsr2: 0,
        prog: 0,
        vag,
        reserved3: [0; 4],
    };
    VabBank {
        master_vol: 127,
        samples: (0..PROGRAMS * TONES)
            .map(|i| {
                Some(UploadedVag {
                    addr: 0x1000 + i as u32 * 0x10,
                    size: 0x10,
                })
            })
            .collect(),
        programs: (0..PROGRAMS)
            .map(|p| VabProgram {
                mvol: 0x7F,
                mpan: 0x40,
                tones: (0..TONES)
                    .map(|t| tone((p * TONES + t + 1) as i16))
                    .collect(),
                key_tones: TONES as u8,
            })
            .collect(),
    }
}

fn rig(log: &Rc<RefCell<Vec<Ev>>>) -> Option<Cpu> {
    let mut cpu = scus_cpu()?;
    let l = log.clone();
    cpu.hook(KEY_OFF, move |r, _| {
        l.borrow_mut().push(Ev::Off(r.r[A0] as u8))
    });
    let l = log.clone();
    // A hook stands in for a routine and returns through `ra`, so this one runs
    // the slot's first instruction itself and resumes after it. The drainer
    // saved its own `ra` on entry (`0x80016B80`) and restores it from there.
    cpu.hook(DRAIN_SLOT, move |r, _| {
        l.borrow_mut().push(Ev::Cue);
        r.r[psx_retail_exec::V0] = 0x8008_0000;
        r.r[psx_retail_exec::RA] = DRAIN_SLOT + 4;
    });
    let l = log.clone();
    // The routine goes on past the ring loop into the frame's vsync pacing,
    // which rewrites the frame step; the differential stops at the loop's exit.
    cpu.hook(DRAIN_LOOP_EXIT, |r, _| {
        r.r[psx_retail_exec::RA] = psx_retail_exec::RETURN_SENTINEL
    });
    cpu.hook(KEY_ON, move |r, _| {
        l.borrow_mut()
            .push(Ev::On(r.r[A0] as u8, r.r[A2] as u8, r.r[A3] as u8));
    });
    Some(cpu)
}

/// Retail's one-shot arm keys each voice off right before it keys it on, and
/// nothing reaches the chip before the frame's flush: the key-off clears the
/// voice's pending key-on (`FUN_80067480`), so of a cue's passes over one voice
/// only the last keys it. The port's bank resolves a cue's passes that way and
/// its key-on restarts the voice, so the pair reads as the one key-on and an
/// earlier pass of the same cue on that voice drops out. (A sustained run's
/// release of voice 7 ahead of its own key-on stays: the port keys that off
/// too.) Across two cues of one drain the later key-on restarts the voice on
/// both sides, so the cancel is folded inside a cue only.
fn fold_rekeys(evs: &[Ev]) -> Vec<Ev> {
    let mut out: Vec<Ev> = Vec::new();
    let mut cue_start = 0;
    let mut i = 0;
    while i < evs.len() {
        match evs[i] {
            Ev::Cue => cue_start = out.len(),
            Ev::Off(v) if v >= 0x10 => {
                let mut k = cue_start;
                while k < out.len() {
                    if matches!(out[k], Ev::On(w, ..) if w == v) {
                        out.remove(k);
                    } else {
                        k += 1;
                    }
                }
                if !matches!(evs.get(i + 1), Some(Ev::On(w, _, _)) if *w == v) {
                    out.push(evs[i]);
                }
            }
            e => out.push(e),
        }
        i += 1;
    }
    out
}

/// The session's drain: a ring id below `0x100` keys the static table's entry,
/// one at or above `0x200` the runtime bank's row (`sfx_fire.rs`).
fn port_fire(bank: &SfxBank, runtime: &[u8], id: i16, spu: &mut Spu, vab: &VabBank) {
    match u8::try_from(id) {
        Ok(small) => {
            let sustained = bank.get(small).is_some_and(|e| e.sustained);
            bank.play_one_shot(small, spu, vab);
            if sustained {
                bank.latch_sustained_cue(i32::from(id));
            }
        }
        Err(_) => {
            let Some(row) = legaia_engine_core::world::runtime_sfx_descriptor_in(runtime, id)
            else {
                return;
            };
            let sustained = row[3] & 0x20 != 0;
            bank.play_descriptor(&row, spu, vab);
            if sustained {
                bank.latch_sustained_cue(i32::from(id));
            }
        }
    }
}

fn port_events(spu: &mut Spu) -> Vec<Ev> {
    let trace = spu
        .note_trace
        .replace(legaia_engine_audio::note_trace::NoteTrace::new());
    trace
        .map(|t| {
            t.events
                .iter()
                .map(|e| match e.ev {
                    legaia_engine_audio::note_trace::NoteEventKind::On => {
                        let idx = (e.voice_state.addr - 0x1000) / 0x10;
                        Ev::On(
                            e.voice,
                            (idx / TONES as u32) as u8,
                            (idx % TONES as u32) as u8,
                        )
                    }
                    _ => Ev::Off(e.voice),
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Call an interior entry of a routine whose epilogue restores `ra` from
/// `0x10(sp)` before it returns.
fn call_interior(cpu: &mut Cpu, entry: u32) -> Stop {
    cpu.mem
        .wr32(STACK_TOP + 0x10, psx_retail_exec::RETURN_SENTINEL);
    cpu.call(entry, &[])
}

#[test]
fn the_ring_drains_and_keys_as_retail_does() {
    let log = Rc::new(RefCell::new(Vec::new()));
    let Some(mut cpu) = rig(&log) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let scus = crate::common::scus().unwrap();
    let table = legaia_game_tables::sfx_table::SfxTable::from_scus(&scus).expect("sfx table");
    let static_ids = table.entries().len() as u32;
    let vab = tagged_bank();
    let mut reached = [0u32; 5]; // one-shot, sustained, runtime, wrapped cursor, a pass cancelled
    for seed in 0..60u64 {
        let mut rng = Rng(seed);
        log.borrow_mut().clear();
        // Fresh retail state.
        for i in 0..4 {
            cpu.mem.wr16(RING_IDS + i * 2, 0xFFFF);
            cpu.mem.wr32(RING_TIMERS + i * 4, 0);
        }
        for o in [0x158u32, 0x15A] {
            cpu.mem.wr16(GP + o, 0);
        }
        for o in [0x3B8u32, 0x3B4, 0x480, 0x4BC, 0x550, 0x5D0] {
            cpu.mem.wr32(GP + o, 0);
        }
        cpu.mem.wr32(GP + 0x40C, 0xFFFF_FFFF);
        cpu.mem.wr32(0x8007_BABC, 0);
        cpu.mem.wr32(0x8007_BAA0, 0);
        cpu.mem.wr32(0x8007_BA88, 0);
        cpu.mem.wr32(0x1F80_0394, 0);
        cpu.mem.wr32(0x8008_4580, 200);
        for ch in 0..16u32 {
            cpu.mem.wr8(MIXER + ch * 12 + 8, ch as u8);
            cpu.mem.wr8(MIXER + ch * 12 + 0xB, 1);
        }
        let mode: i16 = [3, 0x17, 1, 5][rng.below(4) as usize];
        cpu.mem.wr16(GP + 0x524, mode as u16);
        let step = 1 + rng.below(3) as u8;
        cpu.mem.wr8(0x1F80_0393, step);
        // The runtime bank: a header halfword at `+2`, the rows past it.
        let header = 0x10 + rng.below(2) as u16;
        let mut runtime = vec![0u8; 0x200];
        runtime[2..4].copy_from_slice(&header.to_le_bytes());
        let rows = (header as usize / 2) * 2;
        for r in 0..16 {
            let at = rows + r * 8;
            let count = rng.below(4) as u8;
            let flags = count | if rng.below(3) == 0 { 0x20 } else { 0 };
            runtime[at] = rng.below(PROGRAMS as u32) as u8;
            runtime[at + 1] = rng.below(16) as u8;
            runtime[at + 2] = 0x3C + rng.below(12) as u8;
            runtime[at + 3] = flags;
            runtime[at + 4] = rng.below(8) as u8;
        }
        cpu.mem.load(RUNTIME_BANK, &runtime);
        cpu.mem.wr32(GP + 0x5B8, RUNTIME_BANK);

        let mut sched = SfxScheduler::new();
        sched.set_frame_step(step);
        let bank = SfxBank::from_descriptors(
            table
                .active()
                .map(|(id, d)| (id, d.program, d.tone, d.note, d.flags)),
        );
        bank.set_field_family(mode == 3 || mode == 0x17);
        let mut spu = Spu::new();
        spu.note_trace = Some(legaia_engine_audio::note_trace::NoteTrace::new());

        for frame in 0..40 {
            assert_eq!(call_interior(&mut cpu, AGE_LOOP), Stop::Returned);

            for _ in 0..rng.below(3) {
                let id: i16 = if rng.below(4) == 0 {
                    0x200 + rng.below(16) as i16
                } else {
                    rng.below(static_ids) as i16
                };
                match rng.below(5) {
                    0..=2 => {
                        assert_eq!(cpu.call(PUSH, &[id as u32]), Stop::Returned);
                        sched.push_ring_cue(id);
                    }
                    3 => {
                        let d = rng.below(8) as i16 - 1;
                        assert_eq!(cpu.call(SET_DELAY, &[d as u32]), Stop::Returned);
                        sched.set_ring_cue_delay(d);
                    }
                    _ => {
                        assert_eq!(cpu.call(REPLACE, &[id as u32]), Stop::Returned);
                        sched.replace_ring_cue(id);
                    }
                }
            }
            // The ring before the drain, for the failure message.
            let ring: Vec<(i16, i32)> = (0..4)
                .map(|i| {
                    (
                        cpu.mem.rd16(RING_IDS + i * 2) as i16,
                        cpu.mem.rd32(RING_TIMERS + i * 4) as i32,
                    )
                })
                .collect();
            let port_ring: Vec<(i16, i32)> = sched
                .ring()
                .slots()
                .iter()
                .map(|s| (s.id, s.timer))
                .collect();
            assert_eq!(ring, port_ring, "seed {seed} frame {frame}: ring");
            let cursor = cpu.mem.rd32(GP + 0x4BC);
            assert_eq!(cpu.call(DRAIN, &[]), Stop::Returned);
            let batch = sched.tick_frame();
            for &id in &batch.ring {
                port_fire(&bank, &runtime, id, &mut spu, &vab);
                if id >= 0x200 {
                    reached[2] += 1;
                }
            }
            let raw = std::mem::take(&mut *log.borrow_mut());
            let retail = fold_rekeys(&raw);
            let ons = |e: &[Ev]| e.iter().filter(|e| matches!(e, Ev::On(..))).count();
            if ons(&raw) > ons(&retail) {
                reached[4] += 1;
            }
            let port = port_events(&mut spu);
            if retail
                .iter()
                .any(|e| matches!(e, Ev::On(v, ..) if *v < 0x10))
            {
                reached[1] += 1;
            }
            if retail
                .iter()
                .any(|e| matches!(e, Ev::On(v, ..) if *v >= 0x10))
            {
                reached[0] += 1;
            }
            if cpu.mem.rd32(GP + 0x4BC) < cursor {
                reached[3] += 1;
            }
            assert_eq!(
                retail, port,
                "seed {seed} frame {frame} mode {mode:#x}: ring {ring:?} fired {:?}",
                batch.ring
            );
            assert_eq!(
                cpu.mem.rd32(GP + 0x5D0),
                u32::from(bank.held_sustained()),
                "seed {seed} frame {frame}: held run"
            );
            assert_eq!(
                cpu.mem.rd32(GP + 0x40C) as i32,
                bank.sustained_cue(),
                "seed {seed} frame {frame}: sustained latch"
            );
        }
    }
    assert!(reached.iter().all(|&n| n > 0), "arms reached {reached:?}");
}
