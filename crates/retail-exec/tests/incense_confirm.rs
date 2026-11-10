//! The Incense confirm `FUN_801D8D94` (PROT 0899, the pause menu's Use
//! sub-menu `0xD`) frame by frame against `SpecialUseSession` on
//! `UseRoute::Incense`, with the list kernel `FUN_801D688C` live beneath it.
//! The window VM, the window-list tick and the cues are stubbed; the busy
//! word is held clear and the pad's edge and repeat words carry one random
//! word a frame. Each frame compares the Yes / No cursor while the confirm
//! is open, and the ending: the item take `FUN_80042310(0x8A, 1)` and the
//! effect applier `FUN_800402F4` on Yes, the return to the Use list (screen
//! word `6`) on No or cancel.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_menus::pause_screens::{
    SpecialUseOutcome, SpecialUsePhase, SpecialUseSession, UseRoute,
};
use legaia_engine_system::pad::{PadButton, retail_packed};
use psx_retail_exec::{A0, A1, Stop};
use std::cell::RefCell;
use std::rc::Rc;

const OVERLAY_BASE: u32 = 0x801C_E818;
const INCENSE: u32 = 0x801D_8D94;
const VM: u32 = 0x801D_6628;
const TAKE_ITEM: u32 = 0x8004_2310;
const APPLY_EFFECT: u32 = 0x8004_02F4;
const SCREEN: u32 = 0x801E_46A4;
const STEP: u32 = 0x801E_46AC;
const CURSOR: u32 = 0x801E_46D0;
const BUSY: u32 = 0x8007_BB80;
const EDGE: u32 = 0x8007_B874;
const REPEAT: u32 = 0x8007_BB84;
const LIST_CONFIRM_MASK: u32 = 0x801E_F0F0;
const LIST_CANCEL_MASK: u32 = 0x801E_F0F4;

#[test]
fn the_incense_confirm_ends_as_retail_does() {
    differential!(prot(899, INCENSE));
    let (Some(mut cpu), Some(d)) = (scus_cpu(), disc()) else {
        return;
    };
    cpu.mem.load(
        OVERLAY_BASE,
        &d.read_entry_footprint(899).expect("PROT 0899"),
    );
    cpu.stub(VM, 0);
    for va in [0x8003_1D00, 0x8003_5B50, 0x8003_5BD0] {
        cpu.stub(va, 0);
    }
    let taken = Rc::new(RefCell::new(Vec::<(u32, u32)>::new()));
    let t = taken.clone();
    cpu.hook(TAKE_ITEM, move |r, _| {
        t.borrow_mut().push((r.r[A0], r.r[A1]))
    });
    let applied = Rc::new(RefCell::new(0usize));
    let a = applied.clone();
    cpu.hook(APPLY_EFFECT, move |_, _| *a.borrow_mut() += 1);
    cpu.mem.wr32(LIST_CONFIRM_MASK, 0x40);
    cpu.mem.wr32(LIST_CANCEL_MASK, 0x20);
    cpu.mem.wr32(BUSY, 0);
    let mut rng = Rng(0x494E_4345);
    let mut endings = [0usize; 2];
    for run in 0..1500 {
        cpu.mem.wr32(STEP, 0);
        cpu.mem.wr32(SCREEN, 0xD);
        cpu.mem.wr32(CURSOR, 0x55);
        cpu.mem.wr32(EDGE, 0);
        cpu.mem.wr32(REPEAT, 0);
        taken.borrow_mut().clear();
        *applied.borrow_mut() = 0;
        // The opening frame seeds the cursor on Yes.
        assert_eq!(cpu.call(INCENSE, &[]), Stop::Returned);
        assert_eq!(cpu.mem.rd32(STEP), 1, "run {run}: opened");
        let mut port = SpecialUseSession::new(UseRoute::Incense, Vec::new());
        for frame in 0..40 {
            // The port takes the raw libpad word; retail's menu reads the
            // packed word the pad pump publishes from it.
            let raw: u16 = match rng.below(8) {
                0..=2 => 0,
                3 => PadButton::Up.mask(),
                4 => PadButton::Down.mask(),
                5 => PadButton::Cross.mask(),
                6 => PadButton::Circle.mask(),
                _ => rng.next() as u16,
            };
            let word = u32::from(retail_packed(raw));
            cpu.mem.wr32(EDGE, word);
            cpu.mem.wr32(REPEAT, word);
            assert_eq!(cpu.call(INCENSE, &[]), Stop::Returned);
            port.input_pad_edge(raw);
            let ctx = format!("run {run} frame {frame} raw {raw:#x} packed {word:#x}");
            match &port.phase {
                SpecialUsePhase::Confirm => {
                    assert_eq!(cpu.mem.rd32(STEP), 1, "{ctx}: still open");
                    assert_eq!(cpu.mem.rd32(CURSOR) & 0xFFF, port.cursor as u32, "{ctx}");
                    assert!(taken.borrow().is_empty(), "{ctx}");
                }
                SpecialUsePhase::Done(SpecialUseOutcome::EncounterSuppress) => {
                    assert_eq!(*taken.borrow(), vec![(0x8A, 1)], "{ctx}: one Incense");
                    assert_eq!(*applied.borrow(), 1, "{ctx}: its effect");
                    assert_eq!(cpu.mem.rd32(STEP), 2, "{ctx}");
                    endings[0] += 1;
                    break;
                }
                SpecialUsePhase::Done(SpecialUseOutcome::Cancelled) => {
                    assert_eq!(cpu.mem.rd32(SCREEN), 6, "{ctx}: back to the Use list");
                    assert!(taken.borrow().is_empty(), "{ctx}");
                    endings[1] += 1;
                    break;
                }
                other => panic!("{ctx}: {other:?}"),
            }
        }
    }
    assert!(endings.iter().all(|&n| n > 300), "{endings:?}");
}
