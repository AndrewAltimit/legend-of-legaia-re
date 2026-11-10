//! The field BGM re-attach `FUN_80019898` (field-VM op `0x35` sub-op `8`)
//! against `scene_host::bgm_reattach_volume`: the slot it re-attaches, the
//! voice it reads off the slot and the level it hands both channels, over the
//! volume global's whole word.

use crate::common::{Rng, scus_cpu};
use legaia_engine_scene_host::scene_host::bgm_reattach_volume;
use psx_retail_exec::{A0, A1, A2, Stop};
use std::cell::RefCell;
use std::rc::Rc;

const REATTACH: u32 = 0x8001_9898;
const ATTACH: u32 = 0x8002_6478;
const SET_VOL: u32 = 0x8006_4890;
const BGM_SLOT: u32 = 0x8007_057C;
const VOLUME: u32 = 0x8007_B6EC;

#[test]
fn the_bgm_reattach_level_is_retails() {
    differential!(scus(REATTACH));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let calls = Rc::new(RefCell::new(Vec::new()));
    let c = calls.clone();
    cpu.hook(ATTACH, move |r, _| c.borrow_mut().push((0, r.r[A0], 0, 0)));
    let c = calls.clone();
    cpu.hook(SET_VOL, move |r, _| {
        c.borrow_mut().push((1, r.r[A0], r.r[A1], r.r[A2]));
    });
    let mut rng = Rng(0x4247_4D38);
    for case in 0..40_000u32 {
        let raw = match case {
            0..=0x400 => case as i32 - 0x200,
            _ => rng.next() as i32,
        };
        let voice = rng.next() as i16;
        cpu.mem.wr32(VOLUME, raw as u32);
        cpu.mem.wr16(BGM_SLOT + 0xA, voice as u16);
        calls.borrow_mut().clear();
        assert_eq!(cpu.call(REATTACH, &[]), Stop::Returned);
        let level = i32::from(bgm_reattach_volume(raw)) as u32;
        assert_eq!(
            *calls.borrow(),
            vec![
                (0, BGM_SLOT, 0, 0),
                (1, i32::from(voice) as u32, level, level)
            ],
            "raw {raw:#x}"
        );
    }
}
