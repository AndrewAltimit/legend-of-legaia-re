//! The battle-texture upload helper `FUN_80053B9C` against
//! `battle_char_assembly::parse_upload_block` over generated blocks (empty
//! and filled CLUT runs, zero and non-zero entries) at every party slot:
//! both `LoadImage` calls (`FUN_800583C8`) - the CLUT run's rect and the
//! entries it hands over after the STP pass, the pixel rect and its
//! payload - and the shadow CLUT copy the helper leaves in the battle
//! context at `+0x894 + slot * 0x1E0`, against the port's STP'd entries.

use crate::common::{Rng, scus_cpu};
use legaia_actor_formats::battle_char_assembly::{TextureRect, parse_upload_block};
use psx_retail_exec::{A0, A1, Stop};
use std::cell::RefCell;
use std::rc::Rc;

const UPLOAD: u32 = 0x8005_3B9C;
const LOAD_IMAGE: u32 = 0x8005_83C8;
const BLOCK: u32 = 0x8014_0000;
const CTX: u32 = 0x8012_0000;

#[derive(Debug, Clone, PartialEq, Eq)]
struct Load {
    rect: [u16; 4],
    data: Vec<u8>,
}

#[test]
fn the_texture_upload_is_retails() {
    differential!(scus(UPLOAD));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem.wr32(0x8007_BD24, CTX);
    let loads: Rc<RefCell<Vec<Load>>> = Rc::default();
    {
        let loads = loads.clone();
        cpu.hook(LOAD_IMAGE, move |r, m| {
            let rect = [0, 2, 4, 6].map(|o| m.rd16(r.r[A0] + o));
            let len = 2 * usize::from(rect[2]) * usize::from(rect[3]);
            let data = m.read(r.r[A1], len);
            loads.borrow_mut().push(Load { rect, data });
        });
    }
    let mut rng = Rng(0x5458_5550);
    let mut stp = 0;
    for round in 0..4_000 {
        let slot = rng.below(3) as u8;
        let rect = TextureRect {
            x0: rng.below(0x80) as u16,
            y0: rng.below(0x100) as u16,
            w: 1 + rng.below(0x20) as u16,
            h: 1 + rng.below(0x20) as u16,
        };
        let clut_x = rng.below(0x100) as u16;
        let clut_n = [0u16, 16, 32, 1 + rng.below(0x40) as u16][rng.below(4) as usize];
        let mut block = Vec::new();
        block.extend_from_slice(&clut_x.to_le_bytes());
        block.extend_from_slice(&clut_n.to_le_bytes());
        for _ in 0..clut_n {
            let e = if rng.below(4) == 0 {
                0
            } else {
                rng.next() as u16
            };
            block.extend_from_slice(&e.to_le_bytes());
        }
        block.extend((0..rect.pixel_bytes()).map(|_| rng.next() as u8));
        cpu.mem.load(BLOCK, &block);
        for b in (0..0x600).step_by(4) {
            cpu.mem.wr32(CTX + 0x894 + b, 0xDEAD_BEEF);
        }
        loads.borrow_mut().clear();
        let xy = u32::from(rect.x0) | u32::from(rect.y0) << 16;
        let wh = u32::from(rect.w) | u32::from(rect.h) << 16;
        assert_eq!(
            cpu.call(UPLOAD, &[BLOCK, xy, wh, u32::from(slot)]),
            Stop::Returned,
            "round {round}"
        );
        let up = parse_upload_block(&block, rect, slot).expect("a block");
        let clut_bytes: Vec<u8> = up.clut.iter().flat_map(|e| e.to_le_bytes()).collect();
        stp += up.clut.iter().filter(|&&e| e & 0x8000 != 0).count();
        let want = vec![
            Load {
                rect: [up.clut_x, up.clut_row(), up.clut.len() as u16, 1],
                data: clut_bytes.clone(),
            },
            Load {
                rect: [up.fb_x(), up.fb_y(), rect.w, rect.h],
                data: up.pixels.clone(),
            },
        ];
        assert_eq!(*loads.borrow(), want, "round {round}");
        let shadow = CTX + 0x894 + u32::from(slot) * 0x1E0 + 2 * u32::from(clut_x);
        assert_eq!(
            cpu.mem.read(shadow, clut_bytes.len()),
            clut_bytes,
            "round {round}: shadow CLUT"
        );
    }
    assert!(stp > 50_000, "{stp}");
}
