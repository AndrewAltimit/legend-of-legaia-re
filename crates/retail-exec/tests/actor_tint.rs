//! The battle body's colour pass `FUN_8004A908` against `battle_actor_tint`:
//! every render flag the pass tests (the target cursor's dim `0xC8` with
//! the neutral formation id `0xA8`), lane words from black to the neutral
//! `0x20080200` and random, a tint blend or none, the outdoor recolour,
//! every status arm, fades, radii and view depths from behind the eye to far
//! off, and both seat-7 and ordinary seats.
//!
//! The node's view transform `FUN_8003D344` is GTE work; it is hooked to
//! write the view-space vector the test draws, which is the one input the
//! port takes from it. Compared: the node's `+0x74` colour and `+0x78`
//! weight, and the blend word the dim arm writes back to the actor.

use crate::common::{Rng, scus_cpu};
use legaia_engine_battle_vm::battle_actor_tint::{BattleTintInputs, battle_actor_tint};
use psx_retail_exec::{A1, Stop};
use std::cell::Cell;
use std::rc::Rc;

const TINT: u32 = 0x8004_A908;
const VIEW: u32 = 0x8003_D344;
const CTX_PTR: u32 = 0x8007_BD24;
const CTX: u32 = 0x8012_0000;
const ACTORS: u32 = 0x801C_9370;
const FORMATION: u32 = 0x8007_BD09;
const OUTDOOR: u32 = 0x8007_BDA8;
const ACTOR: u32 = 0x8013_0000;
const NODE: u32 = 0x8015_0000;
const MESH: u32 = 0x8016_0000;

#[test]
fn the_tint_pass_colours_as_retail_colours() {
    differential!(scus(TINT));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem.wr32(CTX_PTR, CTX);
    for s in 0..8 {
        cpu.mem.wr32(ACTORS + s * 4, ACTOR + s * 0x400);
    }
    let view_z = Rc::new(Cell::new(0i32));
    {
        let view_z = view_z.clone();
        cpu.hook(VIEW, move |r, m| {
            m.wr32(r.r[A1], 0);
            m.wr32(r.r[A1] + 4, 0);
            m.wr32(r.r[A1] + 8, view_z.get() as u32);
        });
    }
    let mut rng = Rng(0x5449_4E54);
    let mut arms = std::collections::HashMap::new();
    let (mut faded, mut inverted, mut statused) = (0, 0, 0);
    for n in 0..60_000 {
        let seat = rng.below(8) as i16;
        let s1 = ACTOR + seat as u32 * 0x400;
        let render_flag = match rng.below(6) {
            0 => 0xC8,
            1 => rng.next() as u8,
            _ => 0,
        };
        let lanes = match rng.below(6) {
            0 => 0,
            1 => 0x2008_0200,
            2 => {
                let g = rng.below(0x400);
                g | (g << 10) | (g << 20)
            }
            _ => rng.next() & 0x3FFF_FFFF,
        };
        let top = match rng.below(4) {
            0 => 0,
            _ => rng.next() & 0xFF00_0000,
        };
        let blend = if rng.below(3) == 0 {
            rng.below(0x2000)
        } else {
            0
        };
        let status = match rng.below(5) {
            0 => 1 << rng.below(12),
            1 => rng.next() as u16,
            _ => 0,
        };
        let fade = if rng.below(4) == 0 {
            rng.below(0x80) as u8
        } else {
            0
        };
        let radius = match rng.below(3) {
            0 => rng.next() as i16,
            _ => rng.below(0x800) as i16,
        };
        let z = match rng.below(4) {
            0 => rng.next() as i32,
            1 => -(rng.below(0x1000) as i32),
            _ => rng.below(0x8000) as i32,
        };
        let cell = if rng.below(4) == 0 {
            0xA8
        } else {
            rng.next() as u8
        };
        let outdoor = rng.below(2) == 0;
        let ctx_243 = rng.below(4) == 0;
        let prev_colour = rng.next();
        let prev_weight = rng.next() as u16;

        cpu.mem.wr16(NODE + 0x5A, seat as u16);
        cpu.mem.wr32(NODE + 0x74, prev_colour);
        cpu.mem.wr16(NODE + 0x78, prev_weight);
        cpu.mem.wr8(s1 + 0x21C, render_flag);
        cpu.mem.wr32(s1 + 0x04, lanes);
        cpu.mem.wr32(s1 + 0x08, top | (rng.next() & 0x00FF_FFFF));
        cpu.mem.wr32(s1 + 0x0C, blend);
        cpu.mem.wr16(s1 + 0x16E, status);
        cpu.mem.wr8(s1 + 0x226, fade);
        cpu.mem.wr32(s1 + 0x22C, MESH);
        cpu.mem.wr16(MESH + 0x58, radius as u16);
        cpu.mem.wr8(FORMATION + seat as u32, cell);
        cpu.mem.wr8(OUTDOOR, u8::from(outdoor));
        cpu.mem.wr8(CTX + 0x243, u8::from(ctx_243));
        view_z.set(z);
        assert_eq!(cpu.call(TINT, &[NODE]), Stop::Returned, "n {n}");

        let port = battle_actor_tint(&BattleTintInputs {
            lanes,
            top,
            blend,
            render_flag,
            status,
            fade,
            radius,
            view_z: z,
            seat,
            formation_cell: cell,
            outdoor,
            ctx_243,
            prev_colour,
            prev_weight,
        });
        let retail = (cpu.mem.rd32(NODE + 0x74), cpu.mem.rd16(NODE + 0x78));
        assert_eq!(
            retail,
            (port.colour, port.weight),
            "n {n} {:?} arm {:?}",
            (
                seat,
                render_flag,
                lanes,
                top,
                blend,
                status,
                fade,
                radius,
                z,
                cell,
                outdoor
            ),
            port.arm
        );
        assert_eq!(
            cpu.mem.rd32(s1 + 0x0C),
            port.blend_writeback.unwrap_or(blend),
            "n {n}"
        );
        *arms.entry(format!("{:?}", port.arm)).or_insert(0u32) += 1;
        if fade != 0 {
            faded += 1;
        }
        if status & 0x383 != 0 {
            statused += 1;
        }
        if outdoor && render_flag == 0 && port.weight < 0x200 {
            inverted += 1;
        }
    }
    assert!(
        arms.len() == 4 && arms.values().all(|&c| c > 2000),
        "{arms:?}"
    );
    assert!(
        faded > 5000 && statused > 5000 && inverted > 500,
        "{faded} {statused} {inverted}"
    );
}
