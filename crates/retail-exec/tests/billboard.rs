//! The SCUS billboard projector `FUN_800195A8` off the disc: the centre's
//! MVMVA under the ambient camera, the 16-bit corner fan-out, the identity
//! reset and optional in-plane `Rz` spin, `RotTransPers4` (`FUN_8005BAC8`:
//! RTPT + RTPS) and the OT shift, against `billboard::project_billboard`,
//! with random cameras, centres (behind the eye included), sizes and spins.

use crate::common::{Rng, scus_cpu};
use legaia_render_kernels::billboard::project_billboard;
use legaia_render_kernels::gte::{GteMat3, GteVec3};
use psx_retail_exec::{Stop, V0};

const PROJECT: u32 = 0x8001_95A8;
const SAVED_CAMERA: u32 = 0x1F80_03C8;
const OT_SHIFT: u32 = 0x1F80_03A4;
const CENTRE: u32 = 0x8010_0000;
const OUT: u32 = 0x8010_0100;
/// `Cpu::call`'s stack top: the caller's stack arguments sit at `+0x10`.
const STACK: u32 = 0x801F_FF00;

#[test]
fn billboard_matches_the_projector() {
    differential!(scus(PROJECT));
    let Some(mut cpu) = scus_cpu() else {
        return;
    };
    let mut rng = Rng(0x4249_4C4C);
    let (mut spun, mut behind) = (0, 0);
    for i in 0..4000 {
        let a = |rng: &mut Rng| rng.below(0x1000) as u16;
        let rot = GteMat3::rot_x_psx(a(&mut rng)).mul(&GteMat3::rot_y_psx(a(&mut rng)));
        let tr = [0, 1, 2].map(|_| rng.below(0x2000) as i32 - 0x1000);
        let tr = [tr[0], tr[1], tr[2] + rng.below(0x3000) as i32];
        let e: Vec<i16> = rot.m.iter().flatten().copied().collect();
        let p = |a: i16, b: i16| (a as u16 as u32) | (b as u16 as u32) << 16;
        let words = [
            p(e[0], e[1]),
            p(e[2], e[3]),
            p(e[4], e[5]),
            p(e[6], e[7]),
            e[8] as u16 as u32,
            tr[0] as u32,
            tr[1] as u32,
            tr[2] as u32,
        ];
        for (k, w) in words.iter().enumerate() {
            cpu.gte.write_ctrl(k, *w);
            cpu.mem.wr32(SAVED_CAMERA + 4 * k as u32, *w);
        }
        let h = 0x100 + rng.below(0x300);
        let (ofx, ofy) = (160, 112 + rng.below(8) as i32);
        cpu.gte.write_ctrl(24, (ofx << 16) as u32);
        cpu.gte.write_ctrl(25, (ofy << 16) as u32);
        cpu.gte.write_ctrl(26, h);
        let shift = rng.below(4);
        cpu.mem.wr8(OT_SHIFT, shift as u8);

        let centre = [0, 1, 2].map(|_| rng.below(0x1000) as i32 - 0x800);
        for (k, v) in centre.iter().enumerate() {
            cpu.mem.wr16(CENTRE + 2 * k as u32, *v as u16);
        }
        let wide = i % 8 == 0;
        let half = |rng: &mut Rng| {
            if wide {
                rng.next() as i16
            } else {
                rng.below(0x100) as i16
            }
        };
        let (hw, hh) = (half(&mut rng), half(&mut rng));
        let angle = if i % 2 == 0 {
            0
        } else {
            rng.below(0x10000) as u16
        };
        for k in 0..4 {
            cpu.mem.wr32(STACK + 0x10 + 4 * k, OUT + 4 * k);
            cpu.mem.wr32(OUT + 4 * k, 0xDEAD_BEEF);
        }
        let args = [CENTRE, hw as u16 as u32, hh as u16 as u32, u32::from(angle)];
        assert_eq!(cpu.call(PROJECT, &args), Stop::Returned);
        let ret = cpu.regs.r[V0];
        let xy: Vec<(i16, i16)> = (0..4)
            .map(|k| {
                let w = cpu.mem.rd32(OUT + 4 * k);
                (w as i16, (w >> 16) as i16)
            })
            .collect();

        let port = project_billboard(
            &rot,
            GteVec3::new(tr[0], tr[1], tr[2]),
            GteVec3::new(centre[0], centre[1], centre[2]),
            hw,
            hh,
            angle,
            h as i32,
            ofx,
            ofy,
            shift,
        );
        let ctx =
            format!("centre {centre:?} tr {tr:?} half ({hw}, {hh}) angle {angle:#x} h {h:#x}");
        assert_eq!(xy, port.xy.to_vec(), "corners, {ctx}");
        assert_eq!(ret as i32, port.depth, "depth, {ctx}");
        spun += usize::from(angle & 0xFFF != 0);
        behind += usize::from(port.behind);
    }
    assert!(spun > 0 && behind > 0, "spun {spun} behind {behind}");
}
