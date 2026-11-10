//! The CLUT-cell HSV writer `FUN_80019D50` against `apply_hsv_cell`: every
//! captured texel through the RGB / HSV round trip with the adds applied, the
//! mode-1 white blend at every amount the integrator can leave (it caps above
//! at `0x100` and has no floor), zero texels and the STP bit, written to the
//! `LoadImage` buffer it carves out of `*(0x1F8003A0)`.

use crate::common::{Rng, scus_cpu};
use legaia_engine_field::clut_cell_fx::{ClutCellFx, apply_hsv_cell};
use psx_retail_exec::{A1, Stop};
use std::cell::Cell;
use std::rc::Rc;

const WRITER: u32 = 0x8001_9D50;
const LOAD_IMAGE: u32 = 0x8005_83C8;
const PACKET_PTR: u32 = 0x1F80_03A0;
const PACKETS: u32 = 0x8014_0000;
const TEXELS: u32 = 0x8015_0000;
const RECT: u32 = 0x8015_8000;
const STACK_TOP: u32 = 0x801F_FF00;

#[test]
fn the_clut_cell_writer_writes_what_retail_writes() {
    differential!(scus(WRITER));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let uploaded = Rc::new(Cell::new(0u32));
    let up = uploaded.clone();
    cpu.hook(LOAD_IMAGE, move |r, _| up.set(r.r[A1]));
    let mut rng = Rng(0x434C_5554);
    let (mut blended, mut negative, mut cells) = (0, 0, 0);
    for round in 0..3000 {
        let (w, h) = (
            [16u16, 16, 32, 7, 1][rng.below(5) as usize],
            1 + rng.below(3) as u16,
        );
        let n = usize::from(w) * usize::from(h);
        let src: Vec<u16> = (0..n)
            .map(|_| match rng.below(6) {
                0 => 0,
                1 => 0x8000,
                _ => rng.next() as u16,
            })
            .collect();
        let fx = ClutCellFx {
            rect: (rng.below(1024) as u16, rng.below(512) as u16, w, h),
            h_add: rng.below(0x400) as i16 - 0x200,
            s_add: rng.below(0x300) as i16 - 0x180,
            v_add: rng.below(0x300) as i16 - 0x180,
            mode: [0i16, 1, 1, 2][rng.below(4) as usize],
            white: match rng.below(4) {
                0 => 0x100,
                1 => -(rng.below(0x200) as i16),
                _ => rng.below(0x101) as i16,
            },
        };
        for (i, t) in src.iter().enumerate() {
            cpu.mem.wr16(TEXELS + i as u32 * 2, *t);
        }
        cpu.mem.wr16(RECT, fx.rect.0);
        cpu.mem.wr16(RECT + 2, fx.rect.1);
        cpu.mem.wr16(RECT + 4, fx.rect.2);
        cpu.mem.wr16(RECT + 6, fx.rect.3);
        cpu.mem.wr32(PACKET_PTR, PACKETS);
        cpu.mem.wr32(STACK_TOP + 0x10, fx.v_add as i32 as u32);
        cpu.mem.wr32(STACK_TOP + 0x14, TEXELS);
        cpu.mem.wr32(STACK_TOP + 0x18, RECT);
        uploaded.set(0);
        let stop = cpu.call(
            WRITER,
            &[
                fx.mode as i32 as u32,
                fx.white as i32 as u32,
                fx.h_add as i32 as u32,
                fx.s_add as i32 as u32,
            ],
        );
        assert_eq!(stop, Stop::Returned);
        assert_eq!(uploaded.get(), PACKETS + 8, "round {round}");
        let port = apply_hsv_cell(&src, &fx);
        for (i, p) in port.iter().enumerate() {
            let r = cpu.mem.rd16(PACKETS + 8 + i as u32 * 2);
            assert_eq!(
                r, *p,
                "round {round} texel {i} {:#06x} under {fx:?}",
                src[i]
            );
        }
        cells += 1;
        blended += u32::from(fx.mode == 1 && fx.white != 0);
        negative += u32::from(fx.mode == 1 && fx.white < 0);
    }
    assert!(
        cells == 3000 && blended > 1000 && negative > 300,
        "{blended} {negative}"
    );
}
