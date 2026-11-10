//! The Items command window, phase 1 of `FUN_801D7C00` (PROT 0899, the
//! pause menu's Items sub-menu `5`), frame by frame against
//! `PauseItemsSession` in `Command` focus, with the three-row picker
//! `FUN_801D688C(&DAT_801E46C0, 3, 1)` live beneath it.
//!
//! The window VM, the window-list tick, the arrange kernel and the cue
//! enqueue are stubbed and the menu cue `FUN_80035BD0` counted; the busy word
//! is held clear and the pad's edge and repeat words carry one random word a
//! frame (Triangle masked off: the port also closes on it, an engine
//! convenience retail's configured cancel mask does not carry). The bag is a
//! dense run of occupied slots, or empty. Each frame compares the command
//! cursor while the window holds, and the ending: Use -> sub-screen `6`,
//! Throw Out -> `7` (cue `0x20` each), Arrange -> phase `2`, cancel ->
//! sub-screen `1`; a confirm on an empty bag buzzes (cue `0x23`) and stays.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_menus::inventory_use::{InventoryContext, InventoryUseSession, TargetRow};
use legaia_engine_menus::items::ItemCatalog;
use legaia_engine_menus::pause_screens::{PauseItemRow, PauseItemsFocus, PauseItemsSession};
use legaia_engine_system::pad::{PadButton, retail_packed};
use psx_retail_exec::{A0, Stop};
use std::cell::RefCell;
use std::rc::Rc;

const OVERLAY_BASE: u32 = 0x801C_E818;
const COMMAND: u32 = 0x801D_7C00;
const VM: u32 = 0x801D_6628;
const ARRANGE: u32 = 0x801D_64A8;
const CUE: u32 = 0x8003_5BD0;
const SCREEN: u32 = 0x801E_46A4;
const STEP: u32 = 0x801E_46AC;
const CURSOR: u32 = 0x801E_46C0;
const BUSY: u32 = 0x8007_BB80;
const EDGE: u32 = 0x8007_B874;
const REPEAT: u32 = 0x8007_BB84;
const WINDOW_LO: u32 = 0x8007_B5EA;
const WINDOW_HI: u32 = 0x8007_B5EC;
const BAG: u32 = 0x8008_5958;
const LIST_CONFIRM_MASK: u32 = 0x801E_F0F0;
const LIST_CANCEL_MASK: u32 = 0x801E_F0F4;

fn session(ids: &[u8]) -> PauseItemsSession {
    let rows: Vec<PauseItemRow> = ids
        .iter()
        .enumerate()
        .map(|(i, &id)| PauseItemRow {
            id,
            slot: i as u8,
            name: String::new(),
            count: 1,
            desc: String::new(),
            passive: None,
            detail: None,
            scope_icon: None,
            use_bits: 0,
            throw_bits: 0,
        })
        .collect();
    let inner = InventoryUseSession::new(
        ItemCatalog::vanilla(),
        ids.to_vec(),
        vec![TargetRow::new(0, "Vahn")],
        InventoryContext::Field,
    );
    PauseItemsSession::new(inner, rows)
}

#[test]
fn the_items_command_window_routes_as_retail_does() {
    differential!(prot(899, COMMAND));
    let (Some(mut cpu), Some(d)) = (scus_cpu(), disc()) else {
        return;
    };
    cpu.mem.load(
        OVERLAY_BASE,
        &d.read_entry_footprint(899).expect("PROT 0899"),
    );
    for va in [VM, ARRANGE, 0x8003_1D00, 0x8003_5B50] {
        cpu.stub(va, 0);
    }
    let cues = Rc::new(RefCell::new(Vec::<u32>::new()));
    let c = cues.clone();
    cpu.hook(CUE, move |r, _| c.borrow_mut().push(r.r[A0]));
    cpu.mem.wr32(LIST_CONFIRM_MASK, 0x40);
    cpu.mem.wr32(LIST_CANCEL_MASK, 0x20);
    cpu.mem.wr32(BUSY, 0);
    let mut rng = Rng(0x434F_4D4D);
    let mut endings = [0usize; 5];
    for run in 0..2000 {
        let n = if rng.below(5) == 0 {
            0
        } else {
            1 + rng.below(4) as usize
        };
        let ids: Vec<u8> = (0..n).map(|_| 1 + rng.below(0xFE) as u8).collect();
        for slot in 0..0x20u32 {
            let (id, count) = ids.get(slot as usize).map_or((0, 0), |&id| (id, 1));
            cpu.mem.wr8(BAG + 2 * slot, id);
            cpu.mem.wr8(BAG + 2 * slot + 1, count);
        }
        cpu.mem.wr16(WINDOW_LO, 0);
        cpu.mem.wr16(WINDOW_HI, 0x20);
        let mut port = session(&ids);
        for _ in 0..rng.below(3) {
            port.input_pad_edge(PadButton::Down.mask());
        }
        assert_eq!(port.focus, PauseItemsFocus::Command);
        cpu.mem.wr32(STEP, 1);
        cpu.mem.wr32(SCREEN, 5);
        cpu.mem.wr32(CURSOR, u32::from(port.command_cursor));
        cpu.mem.wr32(EDGE, 0);
        cpu.mem.wr32(REPEAT, 0);
        cues.borrow_mut().clear();

        for frame in 0..40 {
            let raw: u16 = match rng.below(8) {
                0..=2 => 0,
                3 => PadButton::Up.mask(),
                4 => PadButton::Down.mask(),
                5 => PadButton::Cross.mask(),
                6 => PadButton::Circle.mask(),
                _ => rng.next() as u16,
            } & !PadButton::Triangle.mask();
            let word = u32::from(retail_packed(raw));
            cpu.mem.wr32(EDGE, word);
            cpu.mem.wr32(REPEAT, word);
            cues.borrow_mut().clear();
            assert_eq!(cpu.call(COMMAND, &[]), Stop::Returned);
            port.input_pad_edge(raw);
            let ctx = format!("run {run} frame {frame} raw {raw:#x} packed {word:#x} rows {n}");
            let screen = cpu.mem.rd32(SCREEN);
            let step = cpu.mem.rd32(STEP);
            if port.is_done() {
                assert_eq!(screen, 1, "{ctx}: cancel");
                endings[0] += 1;
                break;
            }
            match port.focus {
                PauseItemsFocus::List => {
                    assert_eq!((screen, cues.borrow()[0]), (6, 0x20), "{ctx}: Use");
                    endings[1] += 1;
                    break;
                }
                PauseItemsFocus::ThrowOutList => {
                    assert_eq!((screen, cues.borrow()[0]), (7, 0x20), "{ctx}: Throw Out");
                    endings[2] += 1;
                    break;
                }
                PauseItemsFocus::Command => {}
                other => panic!("{ctx}: {other:?}"),
            }
            assert_eq!(screen, 5, "{ctx}: still on the command window");
            if port.take_arrange().is_some() {
                assert_eq!(step, 2, "{ctx}: Arrange");
                endings[3] += 1;
                break;
            }
            assert_eq!(step, 1, "{ctx}");
            assert_eq!(
                cpu.mem.rd32(CURSOR) & 0xFFF,
                u32::from(port.command_cursor),
                "{ctx}: cursor"
            );
            if cues.borrow().contains(&0x23) {
                assert!(port.bag_empty(), "{ctx}: the empty-bag buzz");
                endings[4] += 1;
            }
        }
    }
    assert!(endings.iter().all(|&n| n > 50), "{endings:?}");
}
