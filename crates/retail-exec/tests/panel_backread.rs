//! The slot-B "FIELD BACK READ" loader tick `FUN_801F6B24` in PROT 0978,
//! whole, on its contest path (`_DAT_8007BAC0 != 0`), phase by phase against
//! `backread_tick`, `BackreadStep`, `backread_slice_rect` and
//! `backread_texture_variant`: the phase it leaves, its return value, the
//! PROT entry it opens, the sector it seeks to and the rect each strip is
//! uploaded at.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_battle_vm::panel_backread_loader::{
    BACKREAD_PHASE_GLOBAL, BACKREAD_PROT_BASE, BACKREAD_SLICE_BYTES, BackreadStep,
    backread_slice_rect, backread_texture_variant, backread_tick,
};
use psx_retail_exec::{A0, A1, Stop, V0};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

const SLOT_B_BASE: u32 = 0x801F_69D8;
const TICK: u32 = 0x801F_6B24;
const OPEN: u32 = 0x8003_E8A8;
const SEEK: u32 = 0x8003_E964;
const READ: u32 = 0x8003_E800;
const POLL: u32 = 0x8003_DE7C;
const LOAD_IMAGE: u32 = 0x8005_83C8;
const HP_LIVE: u32 = 0x8008_480E;
const HP_MAX: u32 = 0x8008_4824;
const CONTEST: u32 = 0x8007_BAC0;
const DISC_PATH: u32 = 0x8007_B8C2;
const BUFFERS: [u32; 2] = [0x8007_B728, 0x8007_B72C];

#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct Calls {
    opened: Option<u32>,
    seek: Option<u32>,
    /// `(buffer, sectors)`.
    read: Option<(u32, u32)>,
    /// `(x, y, w, h)` and the source buffer.
    upload: Option<((i16, i16, i16, i16), u32)>,
    polled: bool,
}

#[test]
fn the_back_read_loader_steps_as_retail_steps_it() {
    differential!(prot(978, TICK));
    let (Some(mut cpu), Some(d)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem
        .load(SLOT_B_BASE, &d.read_entry(978).expect("PROT 0978"));
    let calls = Rc::new(RefCell::new(Calls::default()));
    let busy = Rc::new(Cell::new(0u32));
    let c = calls.clone();
    cpu.hook(OPEN, move |r, _| {
        assert_eq!(r.r[A1], 1);
        c.borrow_mut().opened = Some(r.r[A0]);
    });
    let c = calls.clone();
    cpu.hook(SEEK, move |r, _| c.borrow_mut().seek = Some(r.r[A0]));
    let c = calls.clone();
    cpu.hook(READ, move |r, _| {
        c.borrow_mut().read = Some((r.r[A0], r.r[A1]))
    });
    let c = calls.clone();
    cpu.hook(LOAD_IMAGE, move |r, m| {
        let p = r.r[A0];
        let mut h = |o| m.rd16(p + o) as i16;
        c.borrow_mut().upload = Some(((h(0), h(2), h(4), h(6)), r.r[A1]));
    });
    let (c, b) = (calls.clone(), busy.clone());
    cpu.hook(POLL, move |r, _| {
        assert_eq!(r.r[A0], 1);
        c.borrow_mut().polled = true;
        r.r[V0] = b.get();
    });
    for print in [0x8001_A068, 0x8005_67A8] {
        cpu.stub(print, 0);
    }
    cpu.mem.wr32(CONTEST, 1);
    cpu.mem.wr16(DISC_PATH, 1);
    cpu.mem.wr32(BUFFERS[0], 0x8010_0000);
    cpu.mem.wr32(BUFFERS[1], 0x8014_0000);
    let staged = [0x8010_0000 + 0x2_8000, 0x8014_0000 + 0x2_8000u32];
    let sectors = BACKREAD_SLICE_BYTES / 0x800;

    let mut rng = Rng(0x4241_434B);
    let (mut done, mut waits) = (0, 0);
    for run in 0..400 {
        let (hp, max) = (rng.next() as u16, rng.next() as u16);
        let variant = backread_texture_variant(hp, max);
        cpu.mem.wr16(HP_LIVE, hp);
        cpu.mem.wr16(HP_MAX, max);
        // A run in four starts off the table; the rest walk it from the
        // phase the caller seats.
        let mut phase: i32 = if run % 4 == 0 {
            [-1, 0, 1, 12, 40][rng.below(5) as usize]
        } else {
            2
        };
        cpu.mem.wr32(BACKREAD_PHASE_GLOBAL, phase as u32);
        let mut last_read = None;
        for tick in 0..200 {
            let poll_busy = rng.below(3) == 0;
            busy.set(if poll_busy { 1 + rng.below(9) } else { 0 });
            *calls.borrow_mut() = Calls::default();
            assert_eq!(cpu.call(TICK, &[]), Stop::Returned);
            let got = calls.borrow().clone();
            let ctx = format!("run {run} tick {tick} phase {phase} hp {hp}/{max}");

            let step = BackreadStep::for_phase(phase);
            let (next, loading) = backread_tick(phase, !poll_busy);
            assert_eq!(
                (cpu.mem.rd32(BACKREAD_PHASE_GLOBAL) as i32, cpu.regs.r[V0]),
                (next, u32::from(loading)),
                "{ctx}"
            );
            // The strip a step uploads and the slice it reads next.
            let (upload, read) = match step {
                BackreadStep::Transfer { slice } => (slice.checked_sub(1), Some(slice)),
                BackreadStep::FinalUpload { slice } => (Some(slice), None),
                _ => (None, None),
            };
            assert_eq!(
                got.upload.map(|(rect, _)| rect),
                upload.map(backread_slice_rect),
                "{ctx}: the upload's rect"
            );
            if let Some((_, src)) = got.upload {
                assert_eq!(Some(src), last_read, "{ctx}: the strip read last");
            }
            assert_eq!(
                got.opened,
                read.map(|_| BACKREAD_PROT_BASE + variant),
                "{ctx}: the entry"
            );
            // The first slice is read from where the open leaves the head.
            assert_eq!(
                got.seek,
                read.filter(|&s| s > 0).map(|s| s * sectors),
                "{ctx}: the seek"
            );
            assert_eq!(
                got.read,
                read.map(|s| (staged[s as usize % 2], sectors)),
                "{ctx}: the read"
            );
            assert_eq!(got.polled, step == BackreadStep::Poll, "{ctx}: the poll");
            if let Some((buf, _)) = got.read {
                last_read = Some(buf);
            }
            waits += u32::from(step == BackreadStep::Poll && poll_busy);
            if !loading {
                done += 1;
                break;
            }
            if next == phase && step != BackreadStep::Poll {
                // A phase with no arm stands still for good.
                assert!(matches!(
                    step,
                    BackreadStep::Stall | BackreadStep::OutOfRange
                ));
                break;
            }
            phase = next;
        }
    }
    assert!(done > 250 && waits > 300, "done {done} waits {waits}");
}
