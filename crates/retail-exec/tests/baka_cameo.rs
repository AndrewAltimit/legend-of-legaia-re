//! The Baka Fighter round-start cameo's animator `FUN_801D6310` in PROT 0976,
//! whole, phase by phase against `cameo_pose`: every actor word it stores,
//! the wink cells it blits, the clip it hands the clip driver and its phase
//! step.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_minigames::baka_fighter_chrome::cameo_pose;
use psx_retail_exec::{A0, A1, Stop};
use std::cell::RefCell;
use std::rc::Rc;

const OVERLAY_BASE: u32 = 0x801C_E818;
const CAMEO: u32 = 0x801D_6310;
const BLIT: u32 = 0x801D_65F8;
const CLIP_DRIVER: u32 = 0x8002_04F8;
const FRAME_STEP: u32 = 0x1F80_0393;
const ACTOR: u32 = 0x8014_0000;

#[test]
fn the_cameo_walks_on_as_retails_does() {
    differential!(prot(976, CAMEO));
    let (Some(mut cpu), Some(d)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem
        .load(OVERLAY_BASE, &d.read_entry(976).expect("PROT 0976"));
    let blits = Rc::new(RefCell::new(Vec::<(u32, u32)>::new()));
    let driven = Rc::new(RefCell::new(Vec::<(u32, i16)>::new()));
    let b = blits.clone();
    cpu.hook(BLIT, move |r, _| b.borrow_mut().push((r.r[A0], r.r[A1])));
    let c = driven.clone();
    cpu.hook(CLIP_DRIVER, move |r, m| {
        c.borrow_mut()
            .push((r.r[A0], m.rd16(r.r[A0] + 0x5C) as i16));
    });
    let mut rng = Rng(0x4341_4D45);
    let mut segments = [0u32; 6];
    for round in 0..6000 {
        let phase = if round < 0x140 {
            round as i16
        } else {
            rng.below(0x140) as i16
        };
        let dt = 1 + rng.below(3) as u8;
        let flags = rng.next() & !0x0020_0008;
        let sprite = rng.next() as u16 & !0x400;
        let status = rng.next() as u16;
        for i in 0..0x30 {
            cpu.mem.wr32(ACTOR + 4 * i, 0xA5A5_A5A5);
        }
        cpu.mem.wr32(ACTOR + 0x10, flags);
        cpu.mem.wr16(ACTOR + 0x22, phase as u16);
        cpu.mem.wr16(ACTOR + 0x52, sprite);
        cpu.mem.wr16(ACTOR + 0x62, status);
        cpu.mem.wr8(FRAME_STEP, dt);
        blits.borrow_mut().clear();
        driven.borrow_mut().clear();
        let before = cpu.mem.read(ACTOR, 0xC0);

        assert_eq!(cpu.call(CAMEO, &[ACTOR]), Stop::Returned);

        let p = cameo_pose(phase).expect("a phase the spawn can reach");
        let mut want = before;
        let mut put16 = |o: usize, v: u16| want[o..o + 2].copy_from_slice(&v.to_le_bytes());
        put16(0x14, p.x as u16);
        put16(0x16, p.y as u16);
        put16(0x18, p.z as u16);
        put16(0x22, (phase as u16).wrapping_add(u16::from(dt)));
        put16(0x26, p.yaw as u16);
        put16(0x52, sprite | 0x400);
        put16(0x5C, p.clip as u16);
        put16(
            0x62,
            if p.hold_last_frame {
                status | 8
            } else {
                status & !8
            },
        );
        put16(0x6A, 8);
        let f = flags | 0x0020_0000 | if p.retire { 8 } else { 0 };
        want[0x10..0x14].copy_from_slice(&f.to_le_bytes());
        let got = cpu.mem.read(ACTOR, 0xC0);
        assert!(
            got == want,
            "phase {phase:#x}: {:?}",
            crate::common::diff_offsets(&got, &want)
        );
        // Cell `0` is blitted every frame and cell `1` over it from the
        // pose on; the clip driver runs on the clip the frame chose.
        let cells: Vec<(u32, u32)> = (0..=p.blit_index as u32).map(|i| (0, i)).collect();
        assert_eq!(*blits.borrow(), cells, "phase {phase:#x}: the wink");
        assert_eq!(*driven.borrow(), [(ACTOR, p.clip)], "phase {phase:#x}");
        let seg = [0x20, 0x40, 0x90, 0xB0, 0xF0]
            .iter()
            .filter(|&&edge| phase >= edge)
            .count();
        segments[seg] += 1;
    }
    assert!(segments.iter().all(|&n| n > 100), "{segments:?}");
}
