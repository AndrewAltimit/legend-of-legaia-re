//! The field camera off the disc: the per-frame matrix build `FUN_800172C0`
//! (the Euler rotation, the base matrix product, the translation through the
//! focus) against `FieldGteView::from_globals`, then the single-point `RTPS`
//! `FUN_8003D368` through the matrix it left loaded against
//! `FieldGteView::rtps` and the framebuffer grab's `vram_grab::rtps_point`
//! (fed the `f32` view a host resolves from the same globals),
//! over random angles, eye and focus words (the focus beyond `i16` included,
//! where the build's `lhu` wraps it), base scales, `H` and points.

use crate::common::{Rng, scus_cpu};
use legaia_engine_camera::psx_camera::FieldCameraView;
use legaia_render_kernels::gte::{FIELD_OFX, FieldGteView, GteMat3};
use legaia_render_kernels::vram_grab::rtps_point;
use psx_retail_exec::{Stop, V0};

const BUILD: u32 = 0x8001_72C0;
const RTPS: u32 = 0x8003_D368;
const ANGLES: u32 = 0x8007_B790;
const EYE: u32 = 0x8008_40B8;
const FOCUS: u32 = 0x8008_9118;
const BASE: u32 = 0x8007_BF10;
const MATRIX: u32 = 0x1F80_03C8;
const POINT: u32 = 0x8010_0000;
const OUT: u32 = 0x8010_0010;
const OFY: i32 = 114;

#[test]
fn the_field_camera_build_and_rtps_match() {
    differential!(scus(BUILD), scus(RTPS));
    let Some(mut cpu) = scus_cpu() else {
        return;
    };
    cpu.mem.wr32(0x8007_B81C, 0x8007_0A2C);
    cpu.mem.wr32(0x8007_B7F8, 0x8007_122C);
    let mut rng = Rng(0x4641_4D45);
    let (mut behind, mut grabbed) = (0, 0);
    for round in 0..3000 {
        let angles: [u16; 3] = std::array::from_fn(|_| rng.next() as u16);
        let eye: [i32; 3] = [
            rng.below(0x800) as i32 - 0x400,
            rng.below(0x1000) as i32 - 0x800,
            rng.below(0x30000) as i32,
        ];
        let wide = round % 8 == 0;
        let neg_focus: [i32; 3] = std::array::from_fn(|_| {
            if wide {
                rng.next() as i32
            } else {
                rng.below(0x8000) as i32 - 0x4000
            }
        });
        let s = if round % 4 == 0 {
            0x1000 + rng.below(0x6000) as i16
        } else {
            0x6000
        };
        let base = GteMat3 {
            m: [[s, 0, 0], [0, s, 0], [0, 0, s]],
        };
        for k in 0..3 {
            cpu.mem.wr16(ANGLES + 2 * k as u32, angles[k]);
            cpu.mem.wr32(EYE + 4 * k as u32, eye[k] as u32);
            cpu.mem.wr32(FOCUS + 4 * k as u32, neg_focus[k] as u32);
        }
        for r in 0..3 {
            for c in 0..3 {
                cpu.mem
                    .wr16(BASE + 2 * (3 * r + c) as u32, base.m[r][c] as u16);
            }
        }
        assert_eq!(cpu.call(BUILD, &[]), Stop::Returned);
        let h = 0x100 + rng.below(0x200) as u16;
        let port = FieldGteView::from_globals(angles, eye, neg_focus, &base, h, FIELD_OFX, OFY);
        let ctx =
            format!("round {round} angles {angles:x?} eye {eye:?} focus {neg_focus:?} s {s:#x}");
        let rot: [[i16; 3]; 3] = std::array::from_fn(|r| {
            std::array::from_fn(|c| cpu.mem.rd16(MATRIX + 2 * (3 * r + c) as u32) as i16)
        });
        assert_eq!(rot, port.rot.m, "rotation, {ctx}");
        let tr: [i32; 3] =
            std::array::from_fn(|k| cpu.mem.rd32(MATRIX + 0x14 + 4 * k as u32) as i32);
        assert_eq!(tr, port.tr, "translation, {ctx}");

        cpu.gte.write_ctrl(24, (FIELD_OFX << 16) as u32);
        cpu.gte.write_ctrl(25, (OFY << 16) as u32);
        cpu.gte.write_ctrl(26, u32::from(h));
        for _ in 0..4 {
            let p: [i16; 3] = std::array::from_fn(|_| rng.next() as i16);
            for (k, v) in p.iter().enumerate() {
                cpu.mem.wr16(POINT + 2 * k as u32, *v as u16);
            }
            assert_eq!(cpu.call(RTPS, &[POINT, OUT]), Stop::Returned);
            let got = port.rtps(p);
            let w = cpu.mem.rd32(OUT);
            assert_eq!((w as i16, (w >> 16) as i16), got.sxy, "SXY of {p:?}, {ctx}");
            assert_eq!(cpu.regs.r[V0] as i32, got.ir[2], "IR3 of {p:?}, {ctx}");
            // The grab's projector, fed the view a host resolves from the
            // same globals (`engine-field`'s `scripted_view`).
            if s == 0x6000 && got.mac[2] > 0 {
                let rad = |a: u16| f32::from(a & 0xFFF) / 4096.0 * std::f32::consts::TAU;
                let view = FieldCameraView {
                    focus: neg_focus.map(|v| -(v as f32)),
                    pitch: rad(angles[0]),
                    yaw: rad(angles[1]),
                    roll: rad(angles[2]),
                    h: f32::from(h),
                    tr_eye: eye.map(|v| v as f32 / 6.0),
                };
                let grab = rtps_point(p, false, &view).expect("in front");
                assert_eq!(grab, (got.sxy, got.ir[2]), "rtps_point of {p:?}, {ctx}");
                grabbed += 1;
            }
            behind += usize::from(got.sz == 0);
        }
    }
    assert!(
        behind > 0 && grabbed > 1000,
        "behind {behind} grabbed {grabbed}"
    );
}
