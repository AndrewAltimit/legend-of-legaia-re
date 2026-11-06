//! The battle range metric `FUN_8004E2F0` against `motion::range_metric` over
//! the angle `World::battle_range_metric` hands it: every seat pairing, every
//! party reach, the size classes either side of the table's end, and positions
//! from touching to across the stage.

use crate::common::{Rng, scus_cpu};
use legaia_engine_battle_vm::battle_action::bearing_12bit_approx;
use legaia_engine_battle_vm::battle_action::motion::{
    PARTY_REACH_OFFSETS, RangeInputs, range_metric,
};
use legaia_engine_battle_vm::battle_approach::approach_angle;
use legaia_engine_system::trig::trig12;
use legaia_retail_exec::{Stop, V0};

const METRIC: u32 = 0x8004_E2F0;
const ACTORS: u32 = 0x801C_9370;
const MONSTER_RECORDS: u32 = 0x801C_9348;
const SEATS: u32 = 0x8007_BD10;
const BATTLE_END: u32 = 0x8007_BD71;
const SIN_PTR: u32 = 0x8007_B81C;
const COS_PTR: u32 = 0x8007_B7F8;
const SINE_TABLE: u32 = 0x8007_0A2C;
const ACTOR: u32 = 0x8013_0000;
const RECORD: u32 = 0x8014_0000;

#[test]
fn the_range_metric_is_the_retail_metric() {
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem.wr32(SIN_PTR, SINE_TABLE);
    cpu.mem.wr32(COS_PTR, SINE_TABLE + 0x800);
    for s in 0..8 {
        cpu.mem.wr32(ACTORS + s * 4, ACTOR + s * 0x400);
    }
    for m in 0..5 {
        cpu.mem.wr32(MONSTER_RECORDS + m * 4, RECORD + m * 0x100);
    }
    let mut rng = Rng(0x5241_4E47);
    let (mut inside, mut outside) = (0, 0);
    for n in 0..60_000 {
        let attacker = rng.below(8) as u8;
        let target = rng.below(8) as u8;
        cpu.mem.wr8(BATTLE_END, 0xFF);
        let chars: [u8; 3] = std::array::from_fn(|_| 1 + rng.below(4) as u8);
        for (s, c) in chars.iter().enumerate() {
            cpu.mem.wr8(SEATS + s as u32, *c);
        }
        let sizes: [u8; 5] = std::array::from_fn(|_| match rng.below(4) {
            0 => rng.below(3) as u8,
            1 => 3 + rng.below(3) as u8,
            _ => rng.next() as u8,
        });
        for (m, z) in sizes.iter().enumerate() {
            cpu.mem.wr8(RECORD + m as u32 * 0x100 + 0x1F, *z);
        }
        let span = [0x80u32, 0x300, 0x1000, 0x8000][n % 4];
        let mut c = || (rng.below(2 * span) as i32 - span as i32) as i16;
        let live = (c(), c());
        let seat = (c(), c());
        let a = ACTOR + u32::from(attacker) * 0x400;
        let t = ACTOR + u32::from(target) * 0x400;
        cpu.mem.wr16(t + 0x3C, seat.0 as u16);
        cpu.mem.wr16(t + 0x40, seat.1 as u16);
        cpu.mem.wr16(a + 0x34, live.0 as u16);
        cpu.mem.wr16(a + 0x38, live.1 as u16);
        assert_eq!(
            cpu.call(METRIC, &[u32::from(attacker), u32::from(target)]),
            Stop::Returned
        );
        let retail = cpu.regs.r[V0] as u16;
        // The two pairs of one actor alias when it ranges itself.
        let (live, seat) = if attacker == target {
            let x = cpu.mem.rd16(a + 0x34) as i16;
            let z = cpu.mem.rd16(a + 0x38) as i16;
            (
                (x, z),
                (cpu.mem.rd16(a + 0x3C) as i16, cpu.mem.rd16(a + 0x40) as i16),
            )
        } else {
            (live, seat)
        };

        let party = |s: u8| s < 3;
        let size = |s: u8| {
            if party(s) {
                0
            } else {
                sizes[usize::from(s - 3)]
            }
        };
        let inputs = RangeInputs {
            attacker_party: party(attacker),
            target_party: party(target),
            attacker_reach: if party(attacker) {
                PARTY_REACH_OFFSETS[usize::from(chars[usize::from(attacker)] - 1)]
            } else {
                0
            },
            attacker_size: size(attacker),
            target_size: size(target),
            attacker_pos: live,
            target_ref: seat,
        };
        let bearing = bearing_12bit_approx(seat.1, seat.0, live.1, live.0);
        let (sin, cos) = trig12(approach_angle(bearing) as u16);
        let port = range_metric(&inputs, sin, cos);
        assert_eq!(
            retail, port,
            "attacker {attacker} target {target} live {live:?} seat {seat:?} chars {chars:?} sizes {sizes:?}"
        );
        if retail == 0 {
            inside += 1;
        } else {
            outside += 1;
        }
    }
    assert!(inside > 2000 && outside > 2000, "{inside} {outside}");
}
