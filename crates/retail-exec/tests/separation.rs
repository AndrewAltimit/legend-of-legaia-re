//! The battle separation pair `FUN_80050BB8` against `push_apart` over the
//! angle `World::tick_battle_separation` hands it: the bearing between the two
//! seats, a half turn on, read off the sine table. Overlapping and clear pairs,
//! coincident seats, both signs, every radius class a formation seats.

use crate::common::{Rng, scus_cpu};
use legaia_engine_battle_vm::battle_action::bearing_12bit_approx;
use legaia_engine_battle_vm::battle_separation::{SepActor, push_apart};
use legaia_engine_system::trig::trig12;
use psx_retail_exec::Stop;

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
    differential!(scus(PAIR));
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

const DRIVER: u32 = 0x8005_1078;
/// Retail's resting colour word, which the port holds as `0` under flag `0`.
const NEUTRAL_WORD: u32 = 0x2008_0200;

/// The driver `FUN_80051078` over the actor table against
/// `World::tick_battle_separation`: every party size and formation, seats
/// crowded so pairs overlap, each body standing, down and drawn, hidden by a
/// cast (`+0x04 = 0`, `+0x21C = 0xFF`) or faded out (`+0x04 = 0`, flag `2`).
/// Retail's gate is the colour word `+0x04` (`0x800510B4..0x800510DC`), not HP.
#[test]
fn the_separation_driver_is_the_retail_driver() {
    differential!(scus(DRIVER));
    use legaia_engine_core::world::World;
    use legaia_save::Party;
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem.wr32(SIN_PTR, SINE_TABLE);
    cpu.mem.wr32(COS_PTR, SINE_TABLE + 0x800);
    let actor = |seat: u32| 0x8013_0000 + seat * 0x400;
    let rec = |seat: u32| 0x8014_0000 + seat * 0x100;
    for seat in 0..7 {
        cpu.mem.wr32(ACTORS + seat * 4, actor(seat));
        cpu.mem.wr32(actor(seat) + 0x22C, rec(seat));
    }
    let mut rng = Rng(0x5345_5044);
    let (mut failures, mut down_drawn, mut moved) = (Vec::new(), 0, 0);
    for round in 0..20_000 {
        let party = 1 + rng.below(3) as u8;
        let monsters = 1 + rng.below(5) as u8;
        let mut w = World::new();
        w.load_party(Party::zeroed(3));
        w.enter_battle(party, monsters);
        w.party.active_party = if rng.below(4) == 0 {
            vec![0, 3, 1][..usize::from(party)].to_vec()
        } else {
            vec![0, 1, 2][..usize::from(party)].to_vec()
        };
        for seat in 0..7 {
            cpu.mem.wr32(actor(seat) + 4, 0);
        }
        let n = usize::from(party + monsters);
        let mut seeds = Vec::new();
        for e in 0..n {
            let retail = if e < usize::from(party) {
                e as u32
            } else {
                3 + (e - usize::from(party)) as u32
            };
            let mut c = |s: u32| (rng.below(2 * s + 1) as i32 - s as i32) as i16;
            let (x, z) = (c(0x100), c(0x100));
            let (lx, lz) = (c(0x400), c(0x400));
            let hp = [0u16, 1, 300][rng.below(3) as usize];
            // (port colour, port flag): resting, tinted, hidden, faded out,
            // fading.
            let (colour, flag) = [
                (0u32, 0u8),
                (0x0040_1004, 0),
                (0, 0xFF),
                (0, 2),
                (0x1008_0100, 2),
            ][rng.below(5) as usize];
            let word = if colour == 0 && flag == 0 {
                NEUTRAL_WORD
            } else {
                colour
            };
            let radius = if e < usize::from(party) {
                if w.party.active_party[e] == 3 {
                    0x400
                } else {
                    0x280
                }
            } else {
                [0x20i16, 0x40, 0x80, 0xC0, 0x100][rng.below(5) as usize]
            };
            let a = actor(retail);
            cpu.mem.wr32(a + 4, word);
            cpu.mem.wr16(rec(retail) + 0x58, radius as u16);
            cpu.mem.wr16(a + 0x3C, x as u16);
            cpu.mem.wr16(a + 0x40, z as u16);
            cpu.mem.wr16(a + 0x34, lx as u16);
            cpu.mem.wr16(a + 0x38, lz as u16);
            cpu.mem.wr16(a + 0x14C, hp);
            let b = &mut w.actors[e];
            b.battle.hp = hp;
            b.battle.liveness = hp;
            b.battle.render_color = colour;
            b.battle.render_flag = flag;
            b.battle.seat = Some((x, z));
            if e >= usize::from(party) {
                b.battle.render_radius = Some(radius);
            }
            b.move_state.world_x = lx;
            b.move_state.world_z = lz;
            if hp == 0 && word != 0 {
                down_drawn += 1;
            }
            seeds.push((retail, x, z, lx, lz, hp, colour, flag, radius));
        }
        assert_eq!(cpu.call(DRIVER, &[]), Stop::Returned);
        w.tick_battle_separation();
        for (e, s) in seeds.iter().enumerate() {
            let a = actor(s.0);
            let retail = (cpu.mem.rd16(a + 0x34) as i16, cpu.mem.rd16(a + 0x38) as i16);
            let port = (
                w.actors[e].move_state.world_x,
                w.actors[e].move_state.world_z,
            );
            moved += i32::from(retail != (s.3, s.4));
            if retail != port && failures.len() < 8 {
                failures.push(format!(
                    "round {round} party {party} seat {e}: retail {retail:?} port {port:?} seeds {seeds:x?}"
                ));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert!(down_drawn > 5_000 && moved > 5_000, "{down_drawn} {moved}");
}
