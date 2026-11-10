//! The Throw Out confirm, phase 3 of `FUN_801D8734` (PROT 0899, the pause
//! menu's Items sub-menu `0xA`), frame by frame against `PauseItemsSession`
//! in `ThrowOutConfirm` focus, with the Yes / No picker `FUN_801D688C` live
//! beneath it.
//!
//! The window VM, the window-list tick and the cues are stubbed (cue
//! `0x37`, the discard, is counted), the busy word held clear and the pad's
//! edge and repeat words carry one random word a frame. The bag is a dense
//! run of occupied slots inside the active window and the confirm opens on
//! a random row, seeded on No as phase 2 seeds it. Each frame compares the
//! Yes / No cursor while the confirm is open, and the ending: on Yes the
//! picked slot zeroed (both bytes), the discard cue, and the way back - to
//! the list (phase `1`) while anything is left in the bag, else to the
//! command window (screen `5`); on No or cancel the list with the bag
//! untouched.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_menus::inventory_use::{InventoryContext, InventoryUseSession, TargetRow};
use legaia_engine_menus::items::ItemCatalog;
use legaia_engine_menus::pause_screens::{PauseItemRow, PauseItemsFocus, PauseItemsSession};
use legaia_engine_system::pad::{PadButton, retail_packed};
use psx_retail_exec::{A0, Stop};
use std::cell::RefCell;
use std::rc::Rc;

const OVERLAY_BASE: u32 = 0x801C_E818;
const THROW_OUT: u32 = 0x801D_8734;
const VM: u32 = 0x801D_6628;
const WINDOW_OF: u32 = 0x8003_5334;
const CUE: u32 = 0x8003_5BD0;
const SCREEN: u32 = 0x801E_46A4;
const STEP: u32 = 0x801E_46AC;
const CURSOR: u32 = 0x801E_46D0;
const BUSY: u32 = 0x8007_BB80;
const EDGE: u32 = 0x8007_B874;
const REPEAT: u32 = 0x8007_BB84;
const PAYLOAD: u32 = 0x8007_BB88;
const LIST_STATE: u32 = 0x8007_BB94;
const WINDOW_LO: u32 = 0x8007_B5EA;
const WINDOW_HI: u32 = 0x8007_B5EC;
const BAG: u32 = 0x8008_5958;
const LIST_CONFIRM_MASK: u32 = 0x801E_F0F0;
const LIST_CANCEL_MASK: u32 = 0x801E_F0F4;
/// A stand-in list window for the scroll fix-up's lookup.
const WINDOW: u32 = 0x8013_0000;

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
fn the_throw_out_confirm_ends_as_retail_does() {
    differential!(prot(899, THROW_OUT));
    let (Some(mut cpu), Some(d)) = (scus_cpu(), disc()) else {
        return;
    };
    cpu.mem.load(
        OVERLAY_BASE,
        &d.read_entry_footprint(899).expect("PROT 0899"),
    );
    cpu.stub(VM, 0);
    for va in [0x8003_1D00, 0x8003_5B50] {
        cpu.stub(va, 0);
    }
    let cues = Rc::new(RefCell::new(Vec::<u32>::new()));
    let c = cues.clone();
    cpu.hook(CUE, move |r, _| c.borrow_mut().push(r.r[A0]));
    cpu.stub(WINDOW_OF, WINDOW);
    cpu.mem.wr32(WINDOW + 0x18, WINDOW + 0x100);
    cpu.mem.wr16(WINDOW + 0x102, 12);
    cpu.mem.wr32(LIST_CONFIRM_MASK, 0x40);
    cpu.mem.wr32(LIST_CANCEL_MASK, 0x20);
    cpu.mem.wr32(BUSY, 0);
    let mut rng = Rng(0x5448_524F);
    let mut endings = [0usize; 3];
    for run in 0..1500 {
        let n = 1 + rng.below(4) as usize;
        let ids: Vec<u8> = (0..n).map(|_| 1 + rng.below(0xFE) as u8).collect();
        let pick = rng.below(n as u32) as usize;
        for slot in 0..0x20u32 {
            let (id, count) = ids.get(slot as usize).map_or((0, 0), |&id| (id, 1));
            cpu.mem.wr8(BAG + 2 * slot, id);
            cpu.mem.wr8(BAG + 2 * slot + 1, count);
        }
        cpu.mem.wr16(WINDOW_LO, 0);
        cpu.mem.wr16(WINDOW_HI, 0x20);
        // Phase 2 has just opened the confirm on No.
        cpu.mem.wr32(STEP, 3);
        cpu.mem.wr32(SCREEN, 0xA);
        cpu.mem.wr32(CURSOR, 1);
        cpu.mem.wr32(PAYLOAD, pick as u32);
        cpu.mem.wr32(LIST_STATE, 7);
        cpu.mem.wr32(EDGE, 0);
        cpu.mem.wr32(REPEAT, 0);
        cues.borrow_mut().clear();

        let mut port = session(&ids).with_throw_out_row_order((0..n as u8).collect());
        port.input_pad_edge(PadButton::Down.mask());
        port.input_pad_edge(PadButton::Cross.mask());
        assert_eq!(port.focus, PauseItemsFocus::ThrowOutList);
        for _ in 0..pick {
            port.input_pad_edge(PadButton::Down.mask());
        }
        port.input_pad_edge(PadButton::Cross.mask());
        assert_eq!(port.focus, PauseItemsFocus::ThrowOutConfirm);
        assert_eq!(port.list_cursor(), pick);

        for frame in 0..40 {
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
            assert_eq!(cpu.call(THROW_OUT, &[]), Stop::Returned);
            port.input_pad_edge(raw);
            let ctx = format!(
                "run {run} frame {frame} raw {raw:#x} packed {word:#x} rows {n} pick {pick}"
            );
            let slot_bytes = (
                cpu.mem.rd8(BAG + 2 * pick as u32),
                cpu.mem.rd8(BAG + 2 * pick as u32 + 1),
            );
            match port.focus {
                PauseItemsFocus::ThrowOutConfirm => {
                    assert_eq!(cpu.mem.rd32(STEP), 3, "{ctx}: still open");
                    assert_eq!(
                        cpu.mem.rd32(CURSOR) & 0xFFF,
                        u32::from(port.confirm_cursor),
                        "{ctx}"
                    );
                    continue;
                }
                _ if !port.inner.thrown_slots.is_empty() => {
                    assert_eq!(port.inner.thrown_slots, vec![pick as u8], "{ctx}");
                    assert_eq!(slot_bytes, (0, 0), "{ctx}: the slot is emptied");
                    assert_eq!(*cues.borrow(), vec![0x37], "{ctx}: the discard cue");
                    if port.focus == PauseItemsFocus::Command {
                        assert_eq!(n, 1, "{ctx}");
                        assert_eq!(cpu.mem.rd32(SCREEN), 5, "{ctx}: command window");
                        assert_eq!(cpu.mem.rd32(LIST_STATE), 0, "{ctx}");
                        endings[1] += 1;
                    } else {
                        assert_eq!(port.focus, PauseItemsFocus::ThrowOutList, "{ctx}");
                        assert_eq!(cpu.mem.rd32(STEP), 1, "{ctx}: back to the list");
                        endings[0] += 1;
                    }
                }
                PauseItemsFocus::ThrowOutList => {
                    assert_eq!(cpu.mem.rd32(STEP), 1, "{ctx}: back to the list");
                    assert_eq!(slot_bytes, (ids[pick], 1), "{ctx}: the bag untouched");
                    assert!(cues.borrow().is_empty(), "{ctx}");
                    endings[2] += 1;
                }
                other => panic!("{ctx}: {other:?}"),
            }
            break;
        }
    }
    assert!(endings.iter().all(|&n| n > 50), "{endings:?}");
}
