//! The battle separation pair `FUN_80050BB8` against `push_apart` over the
//! angle `World::tick_battle_separation` hands it: the bearing between the two
//! seats, a half turn on, read off the sine table. Overlapping and clear pairs,
//! coincident seats, both signs, every radius class a formation seats.

use crate::common::{Rng, scus_cpu};
use legaia_engine_battle_vm::battle_action::bearing_12bit_approx;
use legaia_engine_battle_vm::battle_separation::{SepActor, push_apart};
use legaia_engine_system::trig::trig12;
use legaia_retail_exec::Stop;

const PAIR: u32 = 0x8005_0BB8;
const ACTORS: u32 = 0x801C_9370;
const SIN_PTR: u32 = 0x8007_B81C;
const COS_PTR: u32 = 0x8007_B7F8;
const SINE_TABLE: u32 = 0x8007_0A2C;
const A: u32 = 0x8013_0000;
const B: u32 = 0x8013_1000;
const A_REC: u32 = 0x8013_0800;
const B_REC: u32 = 0x8013_1800;

#[test]
fn the_separation_nudge_is_the_retail_nudge() {
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    // The boot code points both reads at the one table, cosine a quarter on.
    cpu.mem.wr32(SIN_PTR, SINE_TABLE);
    cpu.mem.wr32(COS_PTR, SINE_TABLE + 0x800);
    cpu.mem.wr32(ACTORS, A);
    cpu.mem.wr32(ACTORS + 4, B);
    cpu.mem.wr32(A + 0x22C, A_REC);
    cpu.mem.wr32(B + 0x22C, B_REC);
    let mut rng = Rng(0x5345_5041);
    let (mut pushed, mut clear) = (0, 0);
    for n in 0..40_000 {
        let span = [0x40i32, 0x100, 0x400, 0x2000][n % 4];
        let mut c = |s: i32| (rng.below(2 * s as u32 + 1) as i32 - s) as i16;
        let ax = c(span);
        let az = c(span);
        let (bx, bz) = if n % 7 == 0 {
            (ax, az)
        } else {
            (c(span), c(span))
        };
        let ra = [0x20i16, 0x40, 0x60, 0x80, 0xC0, 0x100][rng.below(6) as usize];
        let rb = [0x20i16, 0x40, 0x60, 0x80, 0xC0, 0x100][rng.below(6) as usize];
        let live = [
            rng.next() as u16,
            rng.next() as u16,
            rng.next() as u16,
            rng.next() as u16,
        ];
        cpu.mem.wr16(A_REC + 0x58, ra as u16);
        cpu.mem.wr16(B_REC + 0x58, rb as u16);
        cpu.mem.wr16(A + 0x3C, ax as u16);
        cpu.mem.wr16(A + 0x40, az as u16);
        cpu.mem.wr16(B + 0x3C, bx as u16);
        cpu.mem.wr16(B + 0x40, bz as u16);
        cpu.mem.wr16(A + 0x34, live[0]);
        cpu.mem.wr16(A + 0x38, live[1]);
        cpu.mem.wr16(B + 0x34, live[2]);
        cpu.mem.wr16(B + 0x38, live[3]);
        assert_eq!(cpu.call(PAIR, &[0, 1]), Stop::Returned);
        let retail = [
            cpu.mem.rd16(A + 0x34),
            cpu.mem.rd16(A + 0x38),
            cpu.mem.rd16(B + 0x34),
            cpu.mem.rd16(B + 0x38),
        ];

        let mut a = SepActor {
            radius: ra,
            x: ax,
            z: az,
            acc_x: live[0],
            acc_z: live[1],
        };
        let mut b = SepActor {
            radius: rb,
            x: bx,
            z: bz,
            acc_x: live[2],
            acc_z: live[3],
        };
        let bearing = bearing_12bit_approx(b.z, b.x, a.z, a.x);
        let (sin, cos) = trig12(bearing.wrapping_add(0x800) & 0xFFF);
        let hit = push_apart(&mut a, &mut b, sin, cos);
        let port = [a.acc_x, a.acc_z, b.acc_x, b.acc_z];
        assert_eq!(
            retail, port,
            "a ({ax}, {az}) r {ra:#x} b ({bx}, {bz}) r {rb:#x} live {live:?}"
        );
        if hit {
            pushed += 1;
        } else {
            clear += 1;
        }
    }
    assert!(pushed > 2_000 && clear > 2_000, "{pushed} {clear}");
}
