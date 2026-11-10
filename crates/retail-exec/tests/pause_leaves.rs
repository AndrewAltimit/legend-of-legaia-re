//! Three pause-menu decisions of the menu overlay (PROT 0899) driven through
//! their screens: the root picker `FUN_801D6B20` (sub-screen `0x01`) against
//! `root_menu_confirm_route` / `root_menu_cancel_route` with the list kernel
//! live, the Use list's phase-2 dispatch `FUN_801D7E50` against
//! `use_route_for_effect`, and the notify-window renderer `FUN_801DCD58`
//! against `notify_window_operands` / `patch_notify_template` over the
//! overlay's own template, its markup scan `FUN_8003CBF8` live.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_menus::pause_screens::{
    NOTIFY_TEMPLATE_VA, ROOT_MENU_CONTEXT_LOCKED, ROOT_MENU_SAVE_ALLOW_FLAG, RootMenuRoute,
    UseRoute, notify_template_from_menu_overlay, notify_window_operands, patch_notify_template,
    root_menu_cancel_route, root_menu_confirm_route, use_route_for_effect,
};
use psx_retail_exec::{A0, A2, A3, Cpu, SP, Stop};
use std::cell::RefCell;
use std::rc::Rc;

const OVERLAY_BASE: u32 = 0x801C_E818;
const ROOT: u32 = 0x801D_6B20;
const USE_LIST: u32 = 0x801D_7E50;
const NOTIFY: u32 = 0x801D_CD58;
const VM: u32 = 0x801D_6628;
const CUE: u32 = 0x8003_5BD0;
const WINDOW_TICK: u32 = 0x8003_1D00;
const TEXT: u32 = 0x8003_CC98;
const HAND: u32 = 0x8002_B994;

const SCREEN: u32 = 0x801E_46A4;
const STEP: u32 = 0x801E_46AC;
const ROOT_CURSOR: u32 = 0x801E_46BC;
const BUSY: u32 = 0x8007_BB80;
const EDGE: u32 = 0x8007_B874;
const REPEAT: u32 = 0x8007_BB84;
const LIST_CONFIRM_MASK: u32 = 0x801E_F0F0;
const LIST_CANCEL_MASK: u32 = 0x801E_F0F4;
const ENTRY_CONTEXT: u32 = 0x8007_B450;
const CONTEXT_BYTE: u32 = 0x8012_0000;
const STAGED: u32 = 0x8007_BB88;
const LIST_RESULT: u32 = 0x8007_BB94;
const BAG: u32 = 0x8008_5958;
const ITEMS: u32 = 0x8007_4368;
const EFFECTS: u32 = 0x8007_52C0;
const SELECTOR: u32 = 0x8007_BB70;
const BASE: u32 = 0x8007_BB78;
const WINDOW: u32 = 0x8012_1000;

type Cues = Rc<RefCell<Vec<u32>>>;

fn rig() -> Option<(Cpu, Vec<u8>, Cues)> {
    let (mut cpu, d) = (scus_cpu()?, disc()?);
    let overlay = d.read_entry_footprint(899).expect("PROT 0899");
    cpu.mem.load(OVERLAY_BASE, &overlay);
    let cues = Rc::new(RefCell::new(Vec::new()));
    let c = cues.clone();
    cpu.hook(CUE, move |r, _| c.borrow_mut().push(r.r[A0]));
    cpu.stub(VM, 0);
    cpu.stub(WINDOW_TICK, 0);
    cpu.stub(0x8003_5B50, 0);
    Some((cpu, overlay, cues))
}

#[test]
fn the_root_picker_routes_and_gates_as_retail_does() {
    differential!(prot(899, ROOT));
    let Some((mut cpu, _, cues)) = rig() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem.wr32(LIST_CONFIRM_MASK, 0x40);
    cpu.mem.wr32(LIST_CANCEL_MASK, 0x20);
    cpu.mem.wr32(BUSY, 0);
    cpu.mem.wr32(REPEAT, 0);
    let mut rng = Rng(0x524F_4F54);
    let (mut buzzes, mut routes, mut cancels) = (0, 0, 0);
    for case in 0..20000 {
        let row = rng.below(7);
        let kind = match rng.below(4) {
            0 => None,
            1 => Some(ROOT_MENU_CONTEXT_LOCKED),
            _ => Some(rng.below(16) as u8),
        };
        match kind {
            None => cpu.mem.wr32(ENTRY_CONTEXT, 0),
            Some(k) => {
                cpu.mem.wr32(ENTRY_CONTEXT, CONTEXT_BYTE);
                cpu.mem.wr8(CONTEXT_BYTE, k);
            }
        }
        let save_allowed = rng.below(2) == 0;
        cpu.mem
            .wr8(ROOT_MENU_SAVE_ALLOW_FLAG, u8::from(save_allowed));
        let cancel = rng.below(4) == 0;
        cpu.mem.wr32(EDGE, if cancel { 0x20 } else { 0x40 });
        cpu.mem.wr32(ROOT_CURSOR, row);
        cpu.mem.wr32(STEP, 1);
        cpu.mem.wr32(SCREEN, 1);
        cues.borrow_mut().clear();
        assert_eq!(cpu.call(ROOT, &[]), Stop::Returned, "case {case}");
        let screen = cpu.mem.rd32(SCREEN);
        let what = format!("case {case} row {row} {kind:?} save {save_allowed} cancel {cancel}");
        if cancel {
            assert_eq!(screen, u32::from(root_menu_cancel_route(kind)), "{what}");
            cancels += 1;
            continue;
        }
        match root_menu_confirm_route(row as u16, kind, save_allowed) {
            RootMenuRoute::Sub(id) => {
                assert_eq!(screen, u32::from(id), "{what}");
                let cue: &[u32] = if id >= 0x18 { &[0x20] } else { &[] };
                assert_eq!(*cues.borrow(), cue, "{what}");
                routes += 1;
            }
            RootMenuRoute::Buzz => {
                assert_eq!(screen, 1, "{what}");
                assert_eq!(*cues.borrow(), [0x23], "{what}");
                buzzes += 1;
            }
            RootMenuRoute::None => panic!("{what}"),
        }
    }
    assert!(
        buzzes > 1000 && routes > 5000 && cancels > 2000,
        "{buzzes} {routes} {cancels}"
    );
}

#[test]
fn the_use_list_routes_a_pick_as_retail_does() {
    differential!(prot(899, USE_LIST));
    let Some((mut cpu, _, _)) = rig() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem.wr32(BUSY, 0);
    let mut rng = Rng(0x5553_4552);
    let mut seen = [0u32; 6];
    for case in 0..20000 {
        let slot = rng.below(256);
        let id = rng.below(256);
        let sub = rng.below(130);
        cpu.mem.wr8(BAG + 2 * slot, id as u8);
        cpu.mem.wr8(ITEMS + 12 * id + 1, sub as u8);
        let class = [0x80u8, 0x81, 0x82, 0x00, 0x05, 0x7F, 0x83, 0xFF][rng.below(8) as usize];
        let flags = rng.next() as u8;
        cpu.mem.wr8(EFFECTS + 4 * sub, class);
        cpu.mem.wr8(EFFECTS + 4 * sub + 2, flags);
        cpu.mem.wr32(STAGED, slot);
        let result = [2u32, 2, 3, 1][rng.below(4) as usize];
        cpu.mem.wr32(LIST_RESULT, result);
        cpu.mem.wr32(STEP, 2);
        cpu.mem.wr32(SCREEN, 6);
        assert_eq!(cpu.call(USE_LIST, &[]), Stop::Returned, "case {case}");
        let screen = cpu.mem.rd32(SCREEN);
        let what = format!("case {case} class {class:#x} flags {flags:#x} result {result}");
        let want = match result {
            3 => 5,
            2 => match use_route_for_effect(class, flags) {
                UseRoute::ApplyAll => 9,
                UseRoute::ApplySingle => 0xA,
                UseRoute::DoorOfLight => 0xB,
                UseRoute::DoorOfWind => 0xC,
                UseRoute::Incense => 0xD,
            },
            _ => 6,
        };
        assert_eq!(screen, want, "{what}");
        if (9..=0xD).contains(&want) {
            seen[want as usize - 9] += 1;
        } else if want == 5 {
            seen[5] += 1;
        }
    }
    assert!(seen.iter().all(|&n| n > 500), "{seen:?}");
}

#[test]
fn the_notify_window_patches_and_places_what_retail_does() {
    differential!(prot(899, NOTIFY));
    let Some((mut cpu, overlay, _)) = rig() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let pristine = notify_template_from_menu_overlay(&overlay).expect("template");
    let log = Rc::new(RefCell::new(Vec::new()));
    let l = log.clone();
    cpu.hook(TEXT, move |r, m| {
        let y = m.rd32(r.r[SP] + 0x10);
        l.borrow_mut()
            .push(("text", r.r[A0], r.r[A3] as i16, y as i16));
    });
    let l = log.clone();
    cpu.hook(HAND, move |r, _| {
        l.borrow_mut()
            .push(("hand", r.r[A0], r.r[A2] as i16, r.r[A3] as i16));
    });
    let mut rng = Rng(0x4E4F_5449);
    for case in 0..5000 {
        cpu.mem.load(NOTIFY_TEMPLATE_VA, &pristine);
        let selector = match rng.below(3) {
            0 => rng.below(4) as i16,
            1 => -1,
            _ => rng.next() as i16,
        };
        let base = rng.next() as u8;
        cpu.mem.wr16(SELECTOR, selector as u16);
        cpu.mem.wr8(BASE, base);
        let (wx, wy) = match rng.below(3) {
            0 => (rng.below(320) as i16, rng.below(240) as i16),
            _ => (rng.next() as i16, rng.next() as i16),
        };
        cpu.mem.wr16(WINDOW + 0xA, wx as u16);
        cpu.mem.wr16(WINDOW + 0xC, wy as u16);
        log.borrow_mut().clear();
        assert_eq!(cpu.call(NOTIFY, &[WINDOW]), Stop::Returned);
        let mut port = pristine.clone();
        patch_notify_template(&mut port, selector, base);
        let n = notify_window_operands((wx, wy), selector, base);
        let what = format!("case {case} selector {selector} base {base} ({wx}, {wy})");
        assert_eq!(cpu.mem.read(NOTIFY_TEMPLATE_VA, port.len()), port, "{what}");
        assert_eq!(
            *log.borrow(),
            [
                ("text", NOTIFY_TEMPLATE_VA, n.text_pen.0, n.text_pen.1),
                ("hand", 1, n.cursor_pen.0, n.cursor_pen.1),
            ],
            "{what}"
        );
    }
}
