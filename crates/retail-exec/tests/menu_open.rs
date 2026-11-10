//! The menu overlay's open sequence `FUN_801DAD6C` (PROT 0899) against
//! `engine-menus`' `menu_open_step`: the step word, the effect each frame
//! asks for (read off the calls the routine makes and the words it stores)
//! and the unconditional tail.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_menus::menu_open_sequence::{MenuOpenEffect, menu_open_step};
use psx_retail_exec::{A0, A1, Stop};
use std::cell::RefCell;
use std::rc::Rc;

const OVERLAY_BASE: u32 = 0x801C_E818;
const OPEN: u32 = 0x801D_AD6C;
const ACTOR_VM: u32 = 0x801D_6628;
const UPLOAD: u32 = 0x8002_0DE0;
const TAIL: u32 = 0x8003_1D00;

const STEP: u32 = 0x801E_46AC;
const DONE_FLAG: u32 = 0x801E_46A4;
const BUSY: u32 = 0x8007_BB80;
const STAGED_SRC: u32 = 0x801C_6EA0;
const STAGED_DST: u32 = 0x8007_B44C;
const UPLOAD_ARG: u32 = 0x8007_C34C;

#[test]
fn the_open_sequence_steps_as_retail_steps() {
    differential!(prot(899, OPEN));
    let (Some(mut cpu), Some(d)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem.load(
        OVERLAY_BASE,
        &d.read_entry_footprint(899).expect("PROT 0899"),
    );
    let calls: Rc<RefCell<Vec<(u32, u32, u32)>>> = Rc::default();
    for entry in [ACTOR_VM, UPLOAD, TAIL] {
        let c = calls.clone();
        cpu.hook(entry, move |r, _| {
            c.borrow_mut().push((entry, r.r[A0], r.r[A1]))
        });
    }
    let mut rng = Rng(0x4F50_454E);
    let mut seen = [0u32; 4];
    for run in 0..4_000 {
        let step = match rng.below(4) {
            0 => 5 + rng.below(100),
            _ => rng.below(5),
        };
        let busy = rng.below(2);
        let (src, arg) = (rng.next(), rng.next());
        cpu.mem.wr32(STEP, step);
        cpu.mem.wr32(BUSY, busy * (1 + rng.below(9)));
        cpu.mem.wr32(DONE_FLAG, 0x77);
        cpu.mem.wr32(STAGED_SRC, src);
        cpu.mem.wr32(STAGED_DST, 0x1111);
        cpu.mem.wr32(UPLOAD_ARG, arg);
        calls.borrow_mut().clear();
        assert_eq!(cpu.call(OPEN, &[]), Stop::Returned);
        let mut port = step;
        let effect = menu_open_step(&mut port, busy != 0);
        let what = format!("run {run}: step {step} busy {busy}");
        assert_eq!(cpu.mem.rd32(STEP), port, "{what}: {effect:?}");
        let got = calls.borrow().clone();
        // The window-chrome stage closes every frame.
        assert_eq!(got.last().map(|c| c.0), Some(TAIL), "{what}");
        let body: Vec<(u32, u32, u32)> = got[..got.len() - 1].to_vec();
        let (want, staged, flag) = match effect {
            MenuOpenEffect::StartActorScript => (vec![(ACTOR_VM, 0x801E_4A78)], src, 0x77),
            MenuOpenEffect::RunDeferredUpload => (vec![(UPLOAD, 0x8007_06BC)], 0x1111, 0x77),
            MenuOpenEffect::Idle => (vec![], 0x1111, 0x77),
            MenuOpenEffect::Finish => (vec![], 0x1111, 0),
        };
        assert_eq!(
            body.iter().map(|c| (c.0, c.1)).collect::<Vec<_>>(),
            want,
            "{what}: {effect:?}"
        );
        if effect == MenuOpenEffect::RunDeferredUpload {
            assert_eq!(body[0].2, arg, "{what}: the upload's second argument");
        }
        assert_eq!(cpu.mem.rd32(STAGED_DST), staged, "{what}: {effect:?}");
        assert_eq!(cpu.mem.rd32(DONE_FLAG), flag, "{what}: {effect:?}");
        seen[match effect {
            MenuOpenEffect::StartActorScript => 0,
            MenuOpenEffect::RunDeferredUpload => 1,
            MenuOpenEffect::Idle => 2,
            MenuOpenEffect::Finish => 3,
        }] += 1;
    }
    assert!(seen.iter().all(|&n| n > 200), "{seen:?}");
}
