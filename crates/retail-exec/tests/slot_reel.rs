//! The slot machine's reel renderer `FUN_801D0FA8` (PROT 0975) against the
//! reel-cylinder kernels `reel_y` / `reel_z` / `reel_shade`: every reel
//! position and angle the routine is handed, its eight faces' four model-space
//! vertices (caught at the `RotTransPers4` call `FUN_8005BAC8`) and the
//! gouraud shade bytes it writes into each `POLY_GT4`.
//!
//! The routine reads its trig tables through the pointers `_DAT_8007B81C`
//! (sine) and `_DAT_8007B7F8` (cosine), which the test seats on the SCUS
//! tables; the packet cursor `0x1F8003A0` and the ordering table are seated on
//! scratch RAM, and the `addPrim` call `FUN_8003D2C4` is stubbed.

use crate::common::{Rng, disc, scus_cpu};
use legaia_minigame_formats::minigame_slot_scene::{
    COS_TABLE_VA, REEL_ANGLE_BASE, REEL_ANGLE_STEP, REEL_FACES, REEL_WIDTH, SIN_TABLE_VA,
    reel_shade, reel_y, reel_z,
};
use psx_retail_exec::{Cpu, Stop};
use std::cell::RefCell;
use std::rc::Rc;

const OVERLAY_BASE: u32 = 0x801C_E818;
const REEL: u32 = 0x801D_0FA8;
const ROT_TRANS_PERS4: u32 = 0x8005_BAC8;
const ADD_PRIM: u32 = 0x8003_D2C4;
/// The per-reel strip position words (`0x801D3CC0 + reel * 4`).
const POSITIONS: u32 = 0x801D_3CC0;
const SIN_PTR: u32 = 0x8007_B81C;
const COS_PTR: u32 = 0x8007_B7F8;
/// The render block's packet cursor (`0x1F800314 + 0x8C`).
const CURSOR: u32 = 0x1F80_03A0;
const OT_PTR: u32 = 0x1F80_03F4;
const PACKETS: u32 = 0x8010_0000;
const OT: u32 = 0x8018_0000;

#[test]
fn the_reel_faces_are_the_ports_cylinder() {
    differential!(prot(975, REEL));
    let (Some(mut cpu), Some(d)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem
        .load(OVERLAY_BASE, &d.read_entry(975).expect("PROT 0975"));
    cpu.mem.wr32(SIN_PTR, SIN_TABLE_VA);
    cpu.mem.wr32(COS_PTR, COS_TABLE_VA);
    cpu.mem.wr32(OT_PTR, OT);
    cpu.stub(ADD_PRIM, 0);
    let quads: Rc<RefCell<Vec<[[i16; 3]; 4]>>> = Rc::default();
    let sink = Rc::clone(&quads);
    cpu.hook(ROT_TRANS_PERS4, move |r, m| {
        let mut quad = [[0i16; 3]; 4];
        for (k, v) in quad.iter_mut().enumerate() {
            let at = r.r[psx_retail_exec::A0 + k];
            *v = [
                m.rd16(at) as i16,
                m.rd16(at + 2) as i16,
                m.rd16(at + 4) as i16,
            ];
        }
        sink.borrow_mut().push(quad);
        r.r[psx_retail_exec::V0] = 0;
    });
    let mut rng = Rng(0x5245_454C);
    let mut cases = 0u32;
    for case in 0..4000u32 {
        let reel = rng.below(3);
        // A strip position anywhere on the 0x14-symbol strip, and a few
        // negative ones (the `+0xFF` bias of the signed `>> 8`).
        let pos = if case % 16 == 0 {
            -(rng.below(0x100) as i32)
        } else {
            rng.below(0x14 * 0x100) as i32
        };
        let x = rng.below(0x400) as i32 - 0x200;
        let base = rng.below(0x2000) as i32 - 0x1000;
        cpu.mem.wr32(POSITIONS + reel * 4, pos as u32);
        cpu.mem.wr32(CURSOR, PACKETS);
        quads.borrow_mut().clear();
        let stop = cpu.call(REEL, &[x as u32, base as u32, reel]);
        assert_eq!(stop, Stop::Returned, "case {case}");
        let quads = quads.borrow();
        assert_eq!(quads.len(), REEL_FACES, "case {case}");
        // `s1 = pos >> 8` (biased toward zero), `frac = pos - (s1 << 8)`.
        let idx = if pos < 0 { (pos + 0xFF) >> 8 } else { pos >> 8 };
        let frac = pos - (idx << 8);
        for (face, quad) in quads.iter().enumerate() {
            let top = base + REEL_ANGLE_BASE + frac + face as i32 * REEL_ANGLE_STEP;
            let bot = top + REEL_ANGLE_STEP;
            let v = |px: i32, a: i32| [px as i16, reel_y(a) as i16, reel_z(a) as i16];
            let want = [
                v(x, top),
                v(x + REEL_WIDTH, top),
                v(x, bot),
                v(x + REEL_WIDTH, bot),
            ];
            assert_eq!(
                *quad, want,
                "case {case} face {face}: pos {pos:#x} base {base:#x}"
            );
            let packet = PACKETS + face as u32 * 0x34;
            let shade = |cpu: &mut Cpu, o: u32| cpu.mem.rd8(packet + o);
            let (st, sb) = (reel_shade(reel_z(top)) as u8, reel_shade(reel_z(bot)) as u8);
            for o in [4, 5, 6, 0x10, 0x11, 0x12] {
                assert_eq!(
                    shade(&mut cpu, o),
                    st,
                    "case {case} face {face} top +{o:#x}"
                );
            }
            for o in [0x1C, 0x1D, 0x1E, 0x28, 0x29, 0x2A] {
                assert_eq!(
                    shade(&mut cpu, o),
                    sb,
                    "case {case} face {face} bottom +{o:#x}"
                );
            }
        }
        cases += 1;
    }
    assert_eq!(cases, 4000);
}
