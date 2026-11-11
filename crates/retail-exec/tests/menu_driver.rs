//! The pause-menu driver `FUN_801DC6B4` (PROT 0899)'s opening state - the
//! entry decode and window 49's rect rewrite - against
//! `legaia_engine_menus::pause_screens::menu_entry_subscreen` and
//! `legaia_engine_ui::pause_menu::money_box_rect`: every entry-context kind
//! byte, a null context, and the casino-coin flag both ways, every call
//! outside the routine stubbed but the flag test `FUN_8003CE64(8)`, which is
//! scripted. The sub-screen it opens on and window 49's `y` / `h` match.

use crate::common::{disc, scus_cpu};
use legaia_engine_menus::pause_screens::menu_entry_subscreen;
use legaia_engine_ui::pause_menu::money_box_rect;
use psx_retail_exec::{Stop, V0};
use std::cell::Cell;
use std::rc::Rc;

const OVERLAY_BASE: u32 = 0x801C_E818;
const DRIVER: u32 = 0x801D_C6B4;
const DRIVER_END: u32 = DRIVER + 856;
const FLAG_TEST: u32 = 0x8003_CE64;
const STATE: u32 = 0x8007_B43C;
const FADE: u32 = 0x801E_46A0;
const SUB_SCREEN: u32 = 0x801E_46A4;
const CONTEXT: u32 = 0x8007_B450;
const WINDOW_49_Y: u32 = 0x801E_4738 + 0x316;
const WINDOW_49_H: u32 = 0x801E_4738 + 0x31A;
const KIND: u32 = 0x8013_0000;

#[test]
fn the_menu_opens_where_the_port_opens_it() {
    differential!(prot(899, DRIVER));
    let (Some(mut cpu), Some(d)) = (scus_cpu(), disc()) else {
        return;
    };
    cpu.mem.load(
        OVERLAY_BASE,
        &d.read_entry_footprint(899).expect("PROT 0899"),
    );
    cpu.stub_outside = Some((DRIVER, DRIVER_END));
    let flag = Rc::new(Cell::new(false));
    let f = flag.clone();
    cpu.hook(FLAG_TEST, move |r, _| r.r[V0] = u32::from(f.get()));
    let x = cpu.mem.rd16(WINDOW_49_Y - 2) as i16 as i32;
    let w = cpu.mem.rd16(WINDOW_49_H - 2) as i16 as i32;
    for kind in (0..=0xFFu16).map(Some).chain([None]) {
        for coins in [false, true] {
            flag.set(coins);
            cpu.mem.wr32(STATE, 0);
            cpu.mem.wr32(FADE, 0);
            match kind {
                Some(k) => {
                    cpu.mem.wr8(KIND, k as u8);
                    cpu.mem.wr32(CONTEXT, KIND);
                }
                None => cpu.mem.wr32(CONTEXT, 0),
            }
            cpu.mem.wr16(WINDOW_49_Y, 0xAAAA);
            cpu.mem.wr16(WINDOW_49_H, 0xAAAA);
            assert_eq!(cpu.call(DRIVER, &[]), Stop::Returned, "kind {kind:?}");
            let want = menu_entry_subscreen(kind.map(|k| k as u8));
            assert_eq!(cpu.mem.rd32(SUB_SCREEN), u32::from(want), "kind {kind:?}");
            let (_, y, _, h) = money_box_rect((x, 178, w, 24), coins.then_some(0));
            assert_eq!(
                (
                    cpu.mem.rd16(WINDOW_49_Y) as i32,
                    cpu.mem.rd16(WINDOW_49_H) as i32
                ),
                (y, h),
                "kind {kind:?} coins {coins}"
            );
        }
    }
}
