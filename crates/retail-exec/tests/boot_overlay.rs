//! Two boot-time loaders whose ports are `overlay-images::boot_overlay`:
//!
//! - `FUN_80025BA0`, the slot-B default overlay pick, against
//!   `slot_b_default_overlay` over every (flag, suppression, resident word):
//!   the loader call `FUN_8003EC70(param, 0)` or none, the flag's mirror
//!   `gp[0x778]` and the suppression word cleared on the way out.
//! - `FUN_8003E360`, the effect-data load, against `effect_data_source`: the
//!   flag-set branch resolves the raw TOC index (`FUN_8003E8A8(0x3D5, 1)`)
//!   and reads it to the streaming buffer `+ EFFECT_DATA_BUFFER_OFFSET`; the
//!   flag-clear branch opens a host file through the debug-station trap
//!   `FUN_800608F0`.

use crate::common::{Rng, scus_cpu};
use legaia_overlay_images::boot_overlay::{
    EFFECT_DATA_BUFFER_OFFSET, EFFECT_DATA_EXTRACTION_INDEX, EffectDataSource, RAW_TO_EXTRACTION,
    effect_data_source, overlay_param_to_extraction, slot_b_default_overlay,
};
use psx_retail_exec::{A0, A1, Stop, V0};
use std::cell::RefCell;
use std::rc::Rc;

const SLOT_B: u32 = 0x8002_5BA0;
const EFFECT_LOAD: u32 = 0x8003_E360;
const LOADER: u32 = 0x8003_EC70;
const FLAG: u32 = 0x8007_B6A8;
const MIRROR: u32 = 0x8007_BA90;
const SUPPRESS: u32 = 0x8007_BAA4;
const RESIDENT: u32 = 0x8007_BC4C;

#[test]
fn the_slot_b_pick_is_retails() {
    differential!(scus(SLOT_B));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let loads: Rc<RefCell<Vec<(u32, u32)>>> = Rc::default();
    {
        let loads = loads.clone();
        cpu.hook(LOADER, move |r, _| {
            loads.borrow_mut().push((r.r[A0], r.r[A1]))
        });
    }
    let mut rng = Rng(0x534C_4F42);
    let mut seen = [0u32; 3];
    for round in 0..4_000 {
        let flag = if rng.below(2) == 0 {
            0
        } else {
            1 + rng.below(255) as u8
        };
        let suppressed = rng.below(4) == 0;
        let resident = [5u32, 6, 0, rng.next()][rng.below(4) as usize];
        cpu.mem.wr8(FLAG, flag);
        cpu.mem
            .wr32(SUPPRESS, if suppressed { 1 + rng.below(9) } else { 0 });
        cpu.mem.wr32(RESIDENT, resident);
        cpu.mem.wr32(MIRROR, rng.next());
        loads.borrow_mut().clear();
        assert_eq!(cpu.call(SLOT_B, &[]), Stop::Returned, "round {round}");
        let port = slot_b_default_overlay(
            flag != 0,
            suppressed,
            matches!(resident, 5 | 6).then_some(resident),
        );
        let want: Vec<(u32, u32)> = port
            .filter(|c| c.needs_load)
            .map(|c| (c.param, 0))
            .into_iter()
            .collect();
        assert_eq!(*loads.borrow(), want, "round {round}");
        if let Some(c) = port {
            assert_eq!(c.extraction_index, overlay_param_to_extraction(c.param));
            seen[usize::from(c.needs_load)] += 1;
        } else {
            seen[2] += 1;
        }
        assert_eq!(cpu.mem.rd32(MIRROR), u32::from(flag != 0), "round {round}");
        assert_eq!(cpu.mem.rd32(SUPPRESS), 0, "round {round}");
    }
    assert!(seen.iter().all(|&n| n > 300), "{seen:?}");
}

#[test]
fn the_effect_data_load_takes_the_ports_branch() {
    differential!(scus(EFFECT_LOAD));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    const DEV_FLAG: u32 = 0x8007_B8C2;
    const BUFFER_PTR: u32 = 0x8007_B85C;
    const BUFFER: u32 = 0x8010_0000;
    const RESOLVE: u32 = 0x8003_E8A8;
    const READ: u32 = 0x8003_E800;
    const HOST_OPEN: u32 = 0x8006_08F0;
    let calls: Rc<RefCell<Vec<(u32, u32, u32)>>> = Rc::default();
    for (va, ret) in [(RESOLVE, 0x1234u32), (READ, 0), (HOST_OPEN, u32::MAX)] {
        let calls = calls.clone();
        cpu.hook(va, move |r, _| {
            calls.borrow_mut().push((va, r.r[A0], r.r[A1]));
            r.r[V0] = ret;
        });
    }
    cpu.stub(0x8005_6758, 0);
    cpu.mem.wr32(BUFFER_PTR, BUFFER);
    for flag in [0u16, 1, 0x8000, 0xFFFF] {
        cpu.mem.wr16(DEV_FLAG, flag);
        calls.borrow_mut().clear();
        assert_eq!(cpu.call(EFFECT_LOAD, &[]), Stop::Returned, "flag {flag:#x}");
        let got = calls.borrow().clone();
        match effect_data_source(flag != 0) {
            EffectDataSource::ProtEntry(index) => {
                assert_eq!(index, EFFECT_DATA_EXTRACTION_INDEX);
                assert_eq!(
                    got,
                    vec![
                        (RESOLVE, index + RAW_TO_EXTRACTION, 1),
                        (READ, BUFFER + EFFECT_DATA_BUFFER_OFFSET, 0x1234),
                    ],
                    "flag {flag:#x}"
                );
            }
            EffectDataSource::HostFile => {
                assert_eq!(got.len(), 1, "flag {flag:#x}: {got:x?}");
                assert_eq!(got[0].0, HOST_OPEN, "flag {flag:#x}");
            }
        }
    }
}
