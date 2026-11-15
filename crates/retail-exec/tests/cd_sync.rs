//! The game's wait on the CD drive `FUN_8003F2B8` against `cd_sync_wait`:
//! the blocking arm (`mode 0`) and the single poll every cast arm makes
//! (`mode 1`, and any other non-zero mode), over scripted `CdSync` answers.
//! The library's `CdSync` (`0x8005BE8C`), `VSync` (`0x8005FB84`) and the dev
//! print (`0x8001A068`) are stood in for and their calls recorded: what the
//! routine returns, how many statuses it read, how many vsyncs it spent, and
//! what it printed under the debug word `0x8007B6D0`.

use crate::common::{Rng, scus_cpu};
use legaia_engine_battle_vm::battle_cd_drive::{CD_SYNC_COMPLETE, cd_sync_wait};
use psx_retail_exec::{A0, A1, Stop, V0};
use std::cell::RefCell;
use std::rc::Rc;

const WAIT: u32 = 0x8003_F2B8;
const CD_SYNC: u32 = 0x8005_BE8C;
const VSYNC: u32 = 0x8005_FB84;
const DEV_PRINT: u32 = 0x8001_A068;
const RESULT_BUF: u32 = 0x8007_BC10;
const DEBUG_WORD: u32 = 0x8007_B6D0;
const CHECK_FMT: u32 = 0x8001_1174;

#[derive(Default)]
struct Lib {
    script: Vec<u32>,
    reads: usize,
    vsyncs: usize,
    prints: Vec<u32>,
}

#[test]
fn the_drive_wait_answers_as_retail_does() {
    differential!(scus(WAIT));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let lib: Rc<RefCell<Lib>> = Rc::default();
    {
        let l = lib.clone();
        cpu.hook(CD_SYNC, move |r, _| {
            assert_eq!((r.r[A0], r.r[A1]), (1, RESULT_BUF));
            let mut l = l.borrow_mut();
            let i = l.reads;
            l.reads += 1;
            r.r[V0] = l.script[i];
        });
        let l = lib.clone();
        cpu.hook(VSYNC, move |r, _| {
            assert_eq!(r.r[A0], 0);
            l.borrow_mut().vsyncs += 1;
        });
        let l = lib.clone();
        cpu.hook(DEV_PRINT, move |r, _| {
            assert_eq!(r.r[A0], CHECK_FMT);
            l.borrow_mut().prints.push(r.r[A1]);
        });
    }
    let mut rng = Rng(0x4344_5359);
    let (mut blocking, mut busy, mut done, mut printed) = (0, 0, 0, 0);
    for n in 0..20_000 {
        let mode = match rng.below(4) {
            0 => 0,
            1 => 1,
            2 => rng.below(8),
            _ => rng.next(),
        };
        // `CdSync` answers: running (0), complete (2), disk error (5) and
        // anything else, closed by a complete so the blocking arm returns.
        let mut script: Vec<u32> = (0..rng.below(12))
            .map(|_| match rng.below(6) {
                0 => CD_SYNC_COMPLETE,
                1 => 5,
                2 => rng.next(),
                _ => 0,
            })
            .collect();
        script.push(CD_SYNC_COMPLETE);
        let debug_word = match rng.below(3) {
            0 => 0,
            1 => 1,
            _ => rng.next(),
        };
        cpu.mem.wr32(DEBUG_WORD, debug_word);
        *lib.borrow_mut() = Lib {
            script: script.clone(),
            ..Lib::default()
        };
        assert_eq!(cpu.call(WAIT, &[mode]), Stop::Returned, "n {n}");
        let retail = cpu.regs.r[V0];

        let (mut reads, mut vsyncs, mut prints) = (0, 0, Vec::new());
        let port = cd_sync_wait(
            mode,
            debug_word & 1 != 0,
            || {
                reads += 1;
                script[reads - 1]
            },
            || vsyncs += 1,
            |s| prints.push(s),
        );
        let l = lib.borrow();
        assert_eq!(retail, port, "n {n} mode {mode:#x} {script:x?}");
        assert_eq!(
            (l.reads, l.vsyncs, &l.prints),
            (reads, vsyncs, &prints),
            "n {n} mode {mode:#x} {script:x?} debug {debug_word:#x}"
        );
        if mode == 0 {
            blocking += 1;
        } else if port == 0 {
            done += 1;
        } else {
            busy += 1;
            printed += usize::from(!prints.is_empty());
        }
    }
    assert!(
        blocking > 3000 && busy > 5000 && done > 1000 && printed > 2000,
        "{blocking} {busy} {done} {printed}"
    );
}
