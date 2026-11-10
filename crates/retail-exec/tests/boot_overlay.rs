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

/// The scene-bundle loader `FUN_8001EEF0`, whose port is its sector-rounding
/// tail `bytes_to_sectors`.
const BUNDLE_LOAD: u32 = 0x8001_EEF0;

#[test]
fn the_bundle_load_rounds_to_sectors_as_retail_does() {
    differential!(scus(BUNDLE_LOAD));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    // `_DAT_8007B868` set takes the path loader `FUN_8003E6BC`, whose byte
    // count reaches the rounding tail untouched (`0x8001F01C..0x8001F024`).
    cpu.mem.wr32(0x8007_B868, 1);
    let bytes = Rc::new(std::cell::Cell::new(0i32));
    {
        let bytes = bytes.clone();
        cpu.hook(0x8003_E6BC, move |r, _| r.r[V0] = bytes.get() as u32);
    }
    let mut rng = Rng(0x5345_4354);
    let mut negative = 0;
    for round in 0..20_000 {
        let n = match rng.below(5) {
            0 => -(rng.below(0x4000) as i32),
            1 => [
                0,
                1,
                0x7FF,
                0x800,
                0x801,
                -1,
                -0x7FF,
                -0x800,
                -0x801,
                i32::MIN,
            ][rng.below(10) as usize],
            2 => rng.next() as i32,
            _ => rng.below(0x40_0000) as i32,
        };
        negative += i32::from(n < 0);
        bytes.set(n);
        cpu.mem.wr32(0x8007_B730, rng.next());
        assert_eq!(
            cpu.call(BUNDLE_LOAD, &[0, 0, 0, 0]),
            Stop::Returned,
            "round {round}"
        );
        assert_eq!(
            cpu.regs.r[V0] as i32,
            legaia_overlay_images::boot_overlay::bytes_to_sectors(n),
            "round {round}: {n:#x}"
        );
        assert_eq!(cpu.mem.rd32(0x8007_B730), 0);
    }
    assert!(negative > 4_000, "{negative}");
}

/// The CARD-mode init `FUN_8002574C`, whose port is its loader constant
/// `CARD_TIM_EXTRACTION_INDEX`; the TIM uploads it walks are held to
/// `container-formats::pack` over the entry the constant names.
const CARD_INIT: u32 = 0x8002_574C;

#[test]
fn the_card_init_loads_the_ports_entry() {
    differential!(scus(CARD_INIT));
    let (Some(mut cpu), Some(disc)) = (scus_cpu(), crate::common::disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    use legaia_overlay_images::boot_overlay::CARD_TIM_EXTRACTION_INDEX;
    const BUF: u32 = 0x8010_0000;
    // Everything the init calls returns at once but the three it is about.
    cpu.stub_outside = Some((CARD_INIT, CARD_INIT + 0x300));
    cpu.mem.wr32(0x8007_BB00, 1); // gp[0x7E8]: the by-index path.
    cpu.mem.wr16(0x8007_B8C2, 1);
    cpu.stub(0x8001_7888, BUF);
    let loads: Rc<RefCell<Vec<(u32, u32)>>> = Rc::default();
    {
        let loads = loads.clone();
        cpu.hook(0x8003_EB98, move |r, m| {
            loads.borrow_mut().push((r.r[A0], r.r[A1]));
            let entry = disc
                .read_entry((r.r[A0] - RAW_TO_EXTRACTION) as usize)
                .expect("entry");
            m.load(r.r[A1], &entry);
        });
    }
    let uploads: Rc<RefCell<Vec<u32>>> = Rc::default();
    {
        let uploads = uploads.clone();
        cpu.hook(0x8001_98E0, move |r, _| uploads.borrow_mut().push(r.r[A0]));
    }
    assert_eq!(cpu.call(CARD_INIT, &[]), Stop::Returned);
    assert_eq!(
        *loads.borrow(),
        vec![(CARD_TIM_EXTRACTION_INDEX + RAW_TO_EXTRACTION, BUF)]
    );
    let pack = disc
        .read_entry(CARD_TIM_EXTRACTION_INDEX as usize)
        .expect("PROT 0892");
    let want: Vec<u32> = legaia_container_formats::pack::parse_pack(&pack)
        .expect("pack")
        .iter()
        .map(|e| BUF + e.byte_offset as u32)
        .collect();
    assert!(!want.is_empty());
    assert_eq!(*uploads.borrow(), want);
}
