//! The `Rx * Ry * Rz` Euler builder `FUN_80026988` off the disc (the camera
//! matrix and the field light matrix both come from it), over random angle
//! trios, against both ports: `battle_intro::euler_rot_psx` and
//! `field_light::euler_rot`. The sine LUT is read through the pointers the
//! boot installs (`0x8007B81C` sine, `0x8007B7F8` cosine).

use crate::common::{Rng, scus_cpu};
use legaia_render_kernels::battle_intro::euler_rot_psx;
use legaia_render_kernels::field_light::euler_rot;
use psx_retail_exec::Stop;

const EULER: u32 = 0x8002_6988;
const SIN_PTR: u32 = 0x8007_B81C;
const COS_PTR: u32 = 0x8007_B7F8;
const ANGLES: u32 = 0x8010_0000;
const OUT: u32 = 0x8010_0040;

#[test]
fn euler_builder_matches_both_ports() {
    differential!(scus(EULER));
    let Some(mut cpu) = scus_cpu() else {
        return;
    };
    cpu.mem.wr32(SIN_PTR, 0x8007_0A2C);
    cpu.mem.wr32(COS_PTR, 0x8007_122C);
    let mut rng = Rng(0x4555_4C52);
    for i in 0..20_000 {
        let ang: [i16; 3] = if i < 64 {
            // The cardinals, where a sign slip shows.
            std::array::from_fn(|k| ((i >> (2 * k)) & 3) as i16 * 0x400)
        } else {
            std::array::from_fn(|_| rng.next() as i16)
        };
        for (k, a) in ang.iter().enumerate() {
            cpu.mem.wr16(ANGLES + 2 * k as u32, *a as u16);
        }
        assert_eq!(cpu.call(EULER, &[ANGLES, OUT]), Stop::Returned);
        let retail: [[i16; 3]; 3] = std::array::from_fn(|r| {
            std::array::from_fn(|c| cpu.mem.rd16(OUT + 2 * (3 * r + c) as u32) as i16)
        });
        assert_eq!(
            euler_rot_psx((ang[0], ang[1], ang[2])).m,
            retail,
            "euler_rot_psx {ang:?}"
        );
        assert_eq!(
            euler_rot(ang).map(|r| r.map(|v| v as i16)),
            retail,
            "field_light::euler_rot {ang:?}"
        );
    }
}
