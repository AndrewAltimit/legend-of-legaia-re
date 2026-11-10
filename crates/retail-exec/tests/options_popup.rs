//! The Options screen of the menu overlay (PROT 0899): the wrapper
//! `FUN_801DD330` against the argument tuple the port takes from it, and the
//! picker `FUN_801DA9F8`'s popup-open arm against `options_popup_content_rect`:
//! the popup `y` / `h` it stamps into window descriptor 47, over every display
//! row and settings-window `y`, through the overlay's own layout and option
//! tables. The port's per-setting choice counts and row advances are checked
//! against the tables the arm reads.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_menus::options::{
    OPTIONS_DISPLAY_ROWS, OPTIONS_SUBSCREEN_INIT, OPTIONS_SUBSCREEN_RETURN,
    OPTIONS_SUBSCREEN_ROW_SPAN, options_popup_content_rect,
};
use psx_retail_exec::{A0, A1, A2, A3, Cpu, Stop};
use std::cell::RefCell;
use std::rc::Rc;

const OVERLAY_BASE: u32 = 0x801C_E818;
const WRAPPER: u32 = 0x801D_D330;
const PICKER: u32 = 0x801D_A9F8;
const VM: u32 = 0x801D_6628;
const CUE: u32 = 0x8003_5BD0;
const WINDOW_TICK: u32 = 0x8003_1D00;
const STEP: u32 = 0x801E_46AC;
const CURSOR: u32 = 0x801E_46C0;
const BUSY: u32 = 0x8007_BB80;
const EDGE: u32 = 0x8007_B874;
const REPEAT: u32 = 0x8007_BB84;
const LIST_CONFIRM_MASK: u32 = 0x801E_F0F0;
const LIST_CANCEL_MASK: u32 = 0x801E_F0F4;
/// The display layout table: `[option id, advance]` halfwords per row.
const LAYOUT: u32 = 0x801E_4404;
/// The option records: `[value ptr][count][..][id][..]`, 8 bytes each.
const OPTIONS: u32 = 0x801E_44B8;
/// Window descriptor table; 48's `y` is the settings window's, 47's `y` /
/// `h` the popup's.
const SETTINGS_Y: u32 = 0x801E_4A3E;
const POPUP_Y: u32 = 0x801E_4A2E;
const POPUP_H: u32 = 0x801E_4A32;

fn rig() -> Option<(Cpu, Rc<RefCell<Vec<u32>>>)> {
    let (mut cpu, d) = (scus_cpu()?, disc()?);
    cpu.mem.load(
        OVERLAY_BASE,
        &d.read_entry_footprint(899).expect("PROT 0899"),
    );
    let cues = Rc::new(RefCell::new(Vec::new()));
    let c = cues.clone();
    cpu.hook(CUE, move |r, _| c.borrow_mut().push(r.r[A0]));
    cpu.stub(VM, 0);
    cpu.stub(WINDOW_TICK, 0);
    cpu.stub(0x8003_5B50, 0);
    Some((cpu, cues))
}

#[test]
fn the_options_wrapper_passes_what_the_port_takes() {
    differential!(prot(899, WRAPPER));
    let Some((mut cpu, _)) = rig() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let args = Rc::new(RefCell::new(None));
    let a = args.clone();
    cpu.hook(PICKER, move |r, _| {
        *a.borrow_mut() = Some((r.r[A0], r.r[A1], r.r[A2], r.r[A3]))
    });
    assert_eq!(cpu.call(WRAPPER, &[]), Stop::Returned);
    let (lo, hi) = OPTIONS_SUBSCREEN_ROW_SPAN;
    assert_eq!(
        *args.borrow(),
        Some((
            u32::from(lo),
            u32::from(hi),
            OPTIONS_SUBSCREEN_INIT,
            u32::from(OPTIONS_SUBSCREEN_RETURN)
        ))
    );
}

#[test]
fn the_options_popup_opens_where_retail_opens_it() {
    differential!(prot(899, PICKER));
    let Some((mut cpu, cues)) = rig() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem.wr32(LIST_CONFIRM_MASK, 0x40);
    cpu.mem.wr32(LIST_CANCEL_MASK, 0x20);
    cpu.mem.wr32(BUSY, 0);
    cpu.mem.wr32(REPEAT, 0);
    // Each display row's option record, as the arm finds it.
    let mut records = Vec::new();
    for (row, def) in OPTIONS_DISPLAY_ROWS.iter().enumerate() {
        let id = cpu.mem.rd16(LAYOUT + 4 * row as u32) as i16;
        assert_eq!(
            cpu.mem.rd16(LAYOUT + 4 * row as u32 + 2) as i16 as i32,
            def.advance,
            "row {row} advance"
        );
        let mut rec = OPTIONS;
        let found = loop {
            if cpu.mem.rd32(rec) == 0 {
                break None;
            }
            if i16::from(cpu.mem.rd8(rec + 6)) == id {
                break Some(rec);
            }
            rec += 8;
        };
        let count = found.map(|r| cpu.mem.rd8(r + 4));
        if let (Some(setting), Some(n)) = (def.setting, count) {
            assert_eq!(setting.choices().len(), usize::from(n), "row {row} choices");
        }
        records.push(count);
    }
    let mut rng = Rng(0x4F50_5453);
    let (mut opened, mut flipped) = (0, 0);
    for case in 0..4000 {
        let row = rng.below(10) as usize;
        let Some(count) = records[row].filter(|&n| n != 0) else {
            continue;
        };
        let settings_y = [rng.below(240) as i16, rng.next() as i16][rng.below(2) as usize];
        cpu.mem.wr16(SETTINGS_Y, settings_y as u16);
        cpu.mem.wr16(POPUP_Y, 0xAAAA);
        cpu.mem.wr16(POPUP_H, 0xAAAA);
        cpu.mem.wr32(CURSOR, row as u32);
        cpu.mem.wr32(STEP, 2);
        cpu.mem.wr32(EDGE, 0x40);
        cues.borrow_mut().clear();
        assert_eq!(
            cpu.call(PICKER, &[0, 9, 0x30, 1]),
            Stop::Returned,
            "case {case}"
        );
        assert_eq!(
            cpu.mem.rd32(STEP),
            3,
            "case {case} row {row}: the popup opened"
        );
        let (_, y, _, h) =
            options_popup_content_rect(i32::from(settings_y), 0, 0, row, usize::from(count));
        let what = format!("case {case} row {row} settings y {settings_y} count {count}");
        assert_eq!(cpu.mem.rd16(POPUP_Y) as i16, y as i16, "{what}");
        assert_eq!(cpu.mem.rd16(POPUP_H) as i16, h as i16, "{what}");
        opened += 1;
        flipped += u32::from(y < i32::from(settings_y) + 0x16);
    }
    assert!(opened > 1000 && flipped > 200, "{opened} {flipped}");
}
