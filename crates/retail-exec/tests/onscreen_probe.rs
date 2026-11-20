//! The render dispatcher's on-screen probe `FUN_8001B73C` off the disc
//! against `field_frame_cost::actor_box_on_screen`, over random field cameras
//! (`FieldGteView::from_globals`, loaded into the GTE as `RT` / `TR` / `H` /
//! `OFX` / `OFY`), actor origins around the focus and in front of and behind
//! the eye, and cull radii `0..=40` with a few negative ones. Compared: the
//! probe's verdict.

use crate::common::{Rng, scus_cpu};
use legaia_engine_core::world::field_frame_cost::actor_box_on_screen;
use legaia_render_kernels::gte::{FIELD_OFX, FieldGteView};
use psx_retail_exec::{Stop, V0};

const PROBE: u32 = 0x8001_B73C;
const ACTOR: u32 = 0x8015_0000;
const OFY: i32 = 114;

#[test]
fn the_onscreen_probe_matches_retail() {
    differential!(scus(PROBE));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut rng = Rng(0x0B0C_5B73);
    let (mut on, mut off) = (0, 0);
    for _ in 0..4000 {
        let angles: [u16; 3] = [
            rng.below(0x400) as u16 + 0xE00,
            rng.next() as u16 & 0xFFF,
            0,
        ];
        let eye = [
            rng.below(0x200) as i32 - 0x100,
            rng.below(0x400) as i32 - 0x200,
            0x4000 + rng.below(0x10000) as i32,
        ];
        let focus = [
            rng.below(0x3000) as i32,
            -(rng.below(0x200) as i32),
            rng.below(0x3000) as i32,
        ];
        let h = 0x100 + rng.below(0x200) as u16;
        let gte = FieldGteView::from_globals(
            angles,
            eye,
            focus.map(|c| -c),
            &FieldGteView::field_base(),
            h,
            FIELD_OFX,
            OFY,
        );
        let m = gte.rot.m;
        let pack = |a: i16, b: i16| u32::from(a as u16) | (u32::from(b as u16) << 16);
        cpu.gte.write_ctrl(0, pack(m[0][0], m[0][1]));
        cpu.gte.write_ctrl(1, pack(m[0][2], m[1][0]));
        cpu.gte.write_ctrl(2, pack(m[1][1], m[1][2]));
        cpu.gte.write_ctrl(3, pack(m[2][0], m[2][1]));
        cpu.gte.write_ctrl(4, m[2][2] as u16 as u32);
        for k in 0..3 {
            cpu.gte.write_ctrl(5 + k, gte.tr[k] as u32);
        }
        cpu.gte.write_ctrl(24, (FIELD_OFX << 16) as u32);
        cpu.gte.write_ctrl(25, (OFY << 16) as u32);
        cpu.gte.write_ctrl(26, u32::from(h));

        let spread = 0x400 << rng.below(4);
        let at = [
            (focus[0] + rng.below(2 * spread) as i32 - spread as i32) as i16,
            (focus[1] - rng.below(0x200) as i32) as i16,
            (focus[2] + rng.below(2 * spread) as i32 - spread as i32) as i16,
        ];
        let r: i16 = if rng.below(16) == 0 {
            -(rng.below(4) as i16)
        } else {
            rng.below(41) as i16
        };
        cpu.mem.wr16(ACTOR + 0x14, at[0] as u16);
        cpu.mem.wr16(ACTOR + 0x16, at[1] as u16);
        cpu.mem.wr16(ACTOR + 0x18, at[2] as u16);
        cpu.mem.wr16(ACTOR + 0x58, r as u16);
        assert_eq!(cpu.call(PROBE, &[ACTOR]), Stop::Returned);
        let retail = cpu.regs.r[V0] != 0;
        let port = actor_box_on_screen(&gte, at, r);
        assert_eq!(
            port, retail,
            "actor {at:?} radius {r} camera {angles:?} {eye:?} {focus:?} h {h}"
        );
        if retail {
            on += 1;
        } else {
            off += 1;
        }
    }
    assert!(on > 400 && off > 400, "on {on} off {off}");
}
