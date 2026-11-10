//! `FUN_801D88CC`, the round boundary's actor sweep, against
//! `BattleRound::boundary`: the gauge restore, the action-stream clear, the
//! stale-target re-pick and the category clear, over every seat; and the
//! re-pick's own kernel `FUN_801DB8B4` against
//! `BattleRound::first_living_monster`.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_core::battle_round::BattleRound;
use legaia_engine_core::world::World;
use legaia_save::Party;
use psx_retail_exec::Stop;

const SWEEP: u32 = 0x801D_88CC;
const FIRST_LIVE: u32 = 0x801D_B8B4;
const OVERLAY_BASE: u32 = 0x801C_E818;
const CTX: u32 = 0x8012_0000;
const CTX_PTR: u32 = 0x8007_BD24;
const ACTORS: u32 = 0x801C_9370;
const ACTOR: u32 = 0x8013_0000;
const STRIDE: u32 = 0x400;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct Seat {
    hp: u16,
    gauge: u16,
    base: u16,
    target: u8,
    category: u8,
    charged: u8,
    stream: [u8; 16],
}

fn actor(slot: u8) -> u32 {
    ACTOR + u32::from(slot) * STRIDE
}

#[test]
fn the_round_sweep_leaves_every_seat_as_retail_leaves_it() {
    differential!(prot(898, SWEEP), prot(898, FIRST_LIVE));
    let (Some(mut cpu), Some(disc)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem
        .load(OVERLAY_BASE, &disc.read_entry(898).expect("PROT 0898"));
    cpu.stub(0x801D_32BC, 0);
    let mut w = World::new();
    let mut rng = Rng(0x524F_554E);
    let mut repicks = 0;
    for round in 0..20000 {
        let party = 1 + rng.below(3) as u8;
        let monsters = 1 + rng.below(5) as u8;
        // Retail's fixed seats: party `0..3`, monster `k` at `3 + k`.
        let retail_of = |k: u8| if k < party { k } else { 3 + k - party };
        let seated = party + monsters;
        let mut seats = [Seat::default(); 8];
        for s in 0..seated {
            let st = &mut seats[usize::from(retail_of(s))];
            st.hp = [0, 1, 50, 9999][rng.below(4) as usize];
            st.base = [0, 1, 2, 4, 5, 100, 0xFF, 0xCE, 0xCF, 0x120, 0x7FFF, 0xFFFF]
                [rng.below(12) as usize];
            st.gauge = rng.below(0x200) as u16;
            st.category = rng.below(6) as u8;
            st.charged = (rng.below(4) == 0) as u8;
            st.target = rng.below(9) as u8;
            for b in &mut st.stream {
                *b = rng.below(256) as u8;
            }
        }
        cpu.mem.wr32(CTX_PTR, CTX);
        for s in 0..8u8 {
            let a = actor(s);
            cpu.mem.wr32(ACTORS + u32::from(s) * 4, a);
            for b in 0..STRIDE {
                cpu.mem.wr8(a + b, 0);
            }
            let st = seats[usize::from(s)];
            cpu.mem.wr16(a + 0x14C, st.hp);
            cpu.mem.wr16(a + 0x154, st.gauge);
            cpu.mem.wr16(a + 0x156, st.base);
            cpu.mem.wr8(a + 0x1DD, st.target);
            cpu.mem.wr8(a + 0x1DE, st.category);
            cpu.mem.wr8(a + 0x1F9, st.charged);
            cpu.mem.load(a + 0x1DF, &st.stream);
        }
        assert_eq!(cpu.call(FIRST_LIVE, &[]), Stop::Returned, "round {round}");
        let first_live = cpu.regs.r[psx_retail_exec::V0] as u8;
        assert_eq!(cpu.call(SWEEP, &[]), Stop::Returned, "round {round}");

        w.load_party(Party::zeroed(party as usize));
        w.enter_battle(party, monsters);
        w.party.party_count = party;
        let port_of = |retail: u8| {
            if retail < 3 {
                if retail < party { retail } else { u8::MAX }
            } else {
                party + (retail - 3)
            }
        };
        for s in 0..seated {
            let st = seats[usize::from(retail_of(s))];
            let a = &mut w.actors[usize::from(s)].battle;
            a.hp = st.hp;
            a.liveness = u16::from(st.hp != 0);
            a.agl = st.gauge;
            a.agl_base = st.base;
            // The port's targets are its own compact seats; an empty retail
            // seat reads as a port seat past the band.
            a.active_target = match port_of(st.target) {
                u8::MAX => 7,
                p => p,
            };
            a.action_category = st.category;
            a.spirit_shield = st.charged;
            a.params[..16].copy_from_slice(&st.stream);
        }
        assert_eq!(
            BattleRound::first_living_monster(&w),
            port_of(first_live),
            "round {round} party {party} monsters {monsters}"
        );
        BattleRound::boundary(&mut w);
        for s in 0..seated {
            let r = retail_of(s);
            let a = actor(r);
            let p = &w.actors[usize::from(s)].battle;
            let before = seats[usize::from(r)];
            let mut stream = [0u8; 16];
            for (i, b) in stream.iter_mut().enumerate() {
                *b = cpu.mem.rd8(a + 0x1DF + i as u32);
            }
            let retail = (cpu.mem.rd16(a + 0x154), cpu.mem.rd8(a + 0x1DE), stream);
            let mut port_stream = [0u8; 16];
            port_stream.copy_from_slice(&p.params[..16]);
            let port = (p.agl, p.action_category, port_stream);
            assert_eq!(
                retail, port,
                "round {round} party {party} monsters {monsters} seat {s} (retail {r}): {before:?}"
            );
            if s < party {
                let rt = cpu.mem.rd8(a + 0x1DD);
                if rt != before.target {
                    repicks += 1;
                }
                assert_eq!(
                    p.active_target,
                    port_of(rt),
                    "round {round} party {party} monsters {monsters} seat {s}: target, {before:?}"
                );
            }
        }
    }
    assert!(repicks > 1000, "{repicks}");
}
