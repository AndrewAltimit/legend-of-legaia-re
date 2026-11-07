//! PROT 0956's Water Hazard sweep (`FUN_801F7298`, action `0x71`, arm 2) off
//! the disc against `water_hazard_tick` on both legs: each hit seat's
//! damage wrapper (`FUN_801DD4B0`, answered alike on both sides with two
//! `rand()` draws) and its 1-in-8 Venom draw, seat by seat in retail's order.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_cast::cast_arm_ticks::water_hazard_tick;
use legaia_engine_cast::cast_module_ticks::{CastActorState, CastModuleCtx};
use psx_retail_exec::{A0, Stop, V0};
use std::cell::Cell;
use std::rc::Rc;

const TICK: u32 = 0x801F_7298;
const MODULE_BASE: u32 = 0x801F_69D8;
const OVERLAY_BASE: u32 = 0x801C_E818;
const RAND: u32 = 0x8005_6798;
const WRAPPER: u32 = 0x801D_D4B0;
const CTX_PTR: u32 = 0x8007_BD24;
const ACTORS: u32 = 0x801C_9370;
const CTX: u32 = 0x8012_0000;
const ACTOR: u32 = 0x8013_0000;
const STRIDE: u32 = 0x400;

fn lcg(s: u32) -> u32 {
    s.wrapping_mul(0x41C6_4E6D).wrapping_add(0x3039)
}

fn damage(r0: u32, r1: u32) -> i32 {
    ((r0 % 300) + (r1 % 300)) as i32
}

#[test]
fn the_sweep_draws_damage_and_venom_seat_by_seat() {
    let (Some(mut cpu), Some(disc)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem
        .load(OVERLAY_BASE, &disc.read_entry(898).expect("PROT 0898"));
    let module = disc.read_entry(956).expect("PROT 0956");
    let stream = Rc::new(Cell::new(0u32));
    let draws = Rc::new(Cell::new(0u32));
    let draw = {
        let (stream, draws) = (stream.clone(), draws.clone());
        move || {
            let next = lcg(stream.get());
            stream.set(next);
            draws.set(draws.get() + 1);
            (next >> 16) & 0x7FFF
        }
    };
    {
        let draw = draw.clone();
        cpu.hook(RAND, move |r, _| r.r[V0] = draw());
    }
    {
        let draw = draw.clone();
        cpu.hook(WRAPPER, move |r, _| {
            assert_eq!(r.r[A0], 0xD0, "the baked power");
            let (r0, r1) = (draw(), draw());
            r.r[V0] = damage(r0, r1) as u32;
        });
    }
    for va in [
        0x8004_FCC8,
        0x801D_829C,
        0x8002_1B04,
        0x801D_CEAC,
        0x801F_44A0,
    ] {
        cpu.stub(va, 0);
    }
    cpu.mem.wr32(0x8007_B7F8, 0x8007_0A2C);
    cpu.mem.wr32(0x8007_B81C, 0x8007_0A2C + 0x800);
    cpu.mem.wr32(CTX_PTR, CTX);
    let mut rng = Rng(0x5741_5452);
    let (mut venom, mut hits, mut skipped, mut monster_leg) = (0, 0, 0, 0);
    for round in 0..1500 {
        cpu.mem.load(MODULE_BASE, &module);
        let party = 1 + rng.below(3) as u8;
        let caster_slot = 3u8;
        let mut seats = vec![CastActorState::default(); 7];
        for s in 0..8u32 {
            let a = ACTOR + s * STRIDE;
            cpu.mem.wr32(ACTORS + s * 4, a);
            if (s as usize) < seats.len() {
                let hp = if rng.below(5) == 0 {
                    0
                } else {
                    1 + rng.below(700) as u16
                };
                let status = if rng.below(6) == 0 { 4u16 } else { 0 };
                cpu.mem.wr16(a + 0x14C, hp);
                cpu.mem.wr16(a + 0x16E, status);
                cpu.mem.wr32(a + 0x10, 0);
                cpu.mem.wr8(a + 0x1F1, 0x0C);
                seats[s as usize] = CastActorState {
                    hp,
                    flags: status,
                    knockdown_anim: 0x0C,
                    ..CastActorState::default()
                };
            }
        }
        let monsters = 1 + rng.below(4) as u8;
        let code = match rng.below(4) {
            0 => 8u8,
            1 => rng.below(u32::from(party)) as u8,
            2 => 3 + rng.below(4) as u8,
            _ => 9,
        };
        cpu.mem
            .wr8(ACTOR + u32::from(caster_slot) * STRIDE + 0x1DD, code);
        cpu.mem.wr8(CTX, party);
        cpu.mem.wr8(CTX + 1, monsters);
        cpu.mem.wr8(CTX + 0x13, caster_slot);
        cpu.mem.wr8(CTX + 0x279, 2);
        cpu.mem.wr32(0x801F_86A0, 0);
        let seed = rng.next();
        stream.set(seed);
        draws.set(0);
        let mut calls = 0;
        while cpu.mem.rd8(CTX + 0x279) == 2 {
            assert_eq!(cpu.call(TICK, &[]), Stop::Returned, "round {round}");
            calls += 1;
            assert!(calls < 400, "round {round}: arm 2 never advanced");
        }
        let retail_draws = draws.get();

        let mut ctx = CastModuleCtx {
            party_count: party,
            monster_count: monsters,
            caster_seat: caster_slot,
            phase: 2,
            ..CastModuleCtx::default()
        };
        let mut caster = seats[usize::from(caster_slot)];
        let mut s = seed;
        let mut n = 0u32;
        let mut port_draw = || {
            n += 1;
            s = lcg(s);
            (s >> 16) & 0x7FFF
        };
        let (_, port_hits) =
            water_hazard_tick(&mut ctx, caster_slot, code, &mut caster, &mut seats, |_| {
                let (r0, r1) = (port_draw(), port_draw());
                (damage(r0, r1), port_draw())
            });
        let ctx = format!("round {round} party {party} code {code}");
        assert_eq!(retail_draws, n, "draws: {ctx}");
        let low = code == 8 || code < 3;
        let row = if low {
            0..usize::from(party)
        } else {
            3..3 + usize::from(monsters)
        };
        for (seat, s) in seats.iter().enumerate().take(row.end).skip(row.start) {
            let a = ACTOR + seat as u32 * STRIDE;
            assert_eq!(cpu.mem.rd16(a + 0x14C), s.hp, "hp {seat}: {ctx}");
            assert_eq!(
                cpu.mem.rd16(a + 0x16E) & 1,
                s.flags & 1,
                "venom {seat}: {ctx}"
            );
            venom += u32::from(s.flags & 1 != 0);
        }
        hits += port_hits.len();
        skipped += row.len() - port_hits.len();
        monster_leg += usize::from(!low);
    }
    assert!(
        venom > 100 && hits > 1000 && skipped > 200 && monster_leg > 500,
        "{venom} {hits} {skipped} {monster_leg}"
    );
}
