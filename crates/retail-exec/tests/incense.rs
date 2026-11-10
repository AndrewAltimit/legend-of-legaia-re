//! The Incense window's top-up `FUN_80046870` (the item applier's selector
//! `0x82`) against `incense_window::top_up_cooldown`, over the window's whole
//! word: the `+0x40` step, the `0x100` ceiling and the wrap at the word's top.

use crate::common::{Rng, scus_cpu};
use legaia_engine_system::incense_window::top_up_cooldown;
use psx_retail_exec::Stop;

const TOP_UP: u32 = 0x8004_6870;
const WINDOW: u32 = 0x8007_B600;

#[test]
fn the_incense_top_up_is_retails() {
    differential!(scus(TOP_UP));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut rng = Rng(0x494E_4345);
    let mut capped = 0;
    for case in 0..40_000u32 {
        let v = match case {
            0..=0x200 => case as i32,
            _ if case % 3 == 0 => rng.next() as i32,
            _ => rng.below(0x400) as i32 - 0x100,
        };
        cpu.mem.wr32(WINDOW, v as u32);
        assert_eq!(cpu.call(TOP_UP, &[]), Stop::Returned);
        let retail = cpu.mem.rd32(WINDOW) as i32;
        assert_eq!(retail, top_up_cooldown(v), "{v:#x}");
        capped += u32::from(retail == 0x100);
    }
    assert!(capped > 5000, "{capped}");
}
