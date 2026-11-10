//! The side-band transfer sequencer `FUN_801F17F8` against
//! `StreamSlotSm::step`, frame by frame over armed requests of both files,
//! a busy or idle drive, a stale handle left from the last transfer and the
//! stage byte at every value: the libcd calls retail makes (busy poll
//! `FUN_8003DE7C`, open `FUN_800558FC`, seek `FUN_80055A5C`, read
//! `FUN_800559EC`, close `FUN_80055AC8`) logged with their arguments beside
//! the step the port asks its host for, and the request, stage and handle
//! words compared after every frame.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_battle_vm::battle_stream_slot::{SLOT_BYTES, StreamSlotSm, StreamStep};
use psx_retail_exec::{A0, A1, A2, A3, Stop, V0};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

const SEQUENCER: u32 = 0x801F_17F8;
const OVERLAY_BASE: u32 = 0x801C_E818;
const CTX_PTR: u32 = 0x8007_BD24;
const CTX: u32 = 0x8012_0000;
const BUSY: u32 = 0x8003_DE7C;
const OPEN: u32 = 0x8005_58FC;
const SEEK: u32 = 0x8005_5A5C;
const READ: u32 = 0x8005_59EC;
const CLOSE: u32 = 0x8005_5AC8;
const APPLIER: u32 = 0x801F_12D0;
const HANDLE: u32 = 0x8017_0000;

#[derive(Debug, Clone, PartialEq)]
enum Cd {
    Open(u32),
    Seek(u32),
    Read(u32),
    Close,
}

#[test]
fn the_transfer_sequencer_is_retails() {
    differential!(prot(898, SEQUENCER));
    let (Some(mut cpu), Some(disc)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem
        .load(OVERLAY_BASE, &disc.read_entry(898).expect("PROT 0898"));
    cpu.mem.wr32(CTX_PTR, CTX);
    let busy = Rc::new(Cell::new(false));
    let log = Rc::new(RefCell::new(Vec::<Cd>::new()));
    {
        let busy = busy.clone();
        cpu.hook(BUSY, move |r, _| r.r[V0] = u32::from(busy.get()));
    }
    {
        let log = log.clone();
        cpu.hook(OPEN, move |r, _| {
            log.borrow_mut().push(Cd::Open(r.r[A3]));
            r.r[V0] = HANDLE;
        });
    }
    {
        let log = log.clone();
        cpu.hook(SEEK, move |r, _| {
            assert_eq!(r.r[A0], HANDLE);
            assert_eq!(r.r[A2], 0);
            log.borrow_mut().push(Cd::Seek(r.r[A1]));
        });
    }
    {
        let log = log.clone();
        cpu.hook(READ, move |r, _| {
            assert_eq!(r.r[A0], HANDLE);
            log.borrow_mut().push(Cd::Read(r.r[A2]));
        });
    }
    {
        let log = log.clone();
        cpu.hook(CLOSE, move |_, _| log.borrow_mut().push(Cd::Close));
    }
    cpu.stub(APPLIER, 0);
    let mut rng = Rng(0x5354_524D);
    let (mut failures, mut transfers, mut finished) = (Vec::new(), 0, 0);
    for run in 0..2_000 {
        let mut sm = StreamSlotSm {
            request: [0u8, 1, 2, 0x50, 0x80, 0x81, 0x9F, 0xFF, rng.next() as u8]
                [rng.below(9) as usize],
            stage: [0u8, 0, 0, 1, 2, 3, 7][rng.below(7) as usize],
            handle_open: rng.below(2) == 0,
        };
        let applier = rng.below(2) as u8;
        cpu.mem.wr8(CTX + 0x26B, sm.request);
        cpu.mem.wr8(CTX + 0x26C, sm.stage);
        cpu.mem
            .wr32(CTX + 0x314, if sm.handle_open { HANDLE } else { 0 });
        cpu.mem.wr8(CTX + 0x276, applier);
        for frame in 0..8 {
            busy.set(rng.below(3) == 0);
            log.borrow_mut().clear();
            cpu.calls.clear();
            assert_eq!(cpu.call(SEQUENCER, &[]), Stop::Returned, "run {run}");
            assert_eq!(cpu.calls.contains(&APPLIER), applier != 0);
            let step = sm.step(busy.get());
            let expected = match step {
                StreamStep::ReleasePrevious => vec![Cd::Close],
                StreamStep::Transfer { file, offset } => {
                    transfers += 1;
                    vec![
                        Cd::Open(u32::from(file.cdname())),
                        Cd::Seek(offset),
                        Cd::Read(SLOT_BYTES),
                    ]
                }
                StreamStep::Finish => {
                    finished += 1;
                    vec![Cd::Close]
                }
                StreamStep::Idle | StreamStep::WaitCd => vec![],
            };
            let retail = (
                cpu.mem.rd8(CTX + 0x26B),
                cpu.mem.rd8(CTX + 0x26C),
                cpu.mem.rd32(CTX + 0x314) != 0,
                log.borrow().clone(),
            );
            let port = (sm.request, sm.stage, sm.handle_open, expected);
            if retail != port && failures.len() < 8 {
                failures.push(format!(
                    "run {run} frame {frame} busy {}: retail {retail:?} port {port:?} ({step:?})",
                    busy.get()
                ));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert!(transfers > 300 && finished > 300, "{transfers} {finished}");
}
