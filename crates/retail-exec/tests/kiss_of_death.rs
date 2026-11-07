//! PROT 0955's Kiss of Death (`FUN_801F86A4`, action `0x6E`) arm 4 off the
//! disc against `kiss_of_death_tick`: the one `rand()` past the gate, the odd
//! leg's Numb mark and turn steal (with its item refund), the even leg's
//! status strip, one-point hit and reaction stage, and a Stoned victim's skip.

use crate::common::{Rng, scus_cpu};
use legaia_engine_cast::cast_module_ticks::{CastActorState, CastModuleCtx, kiss_of_death_tick};
use legaia_retail_exec::{A0, Stop, V0};
use std::cell::Cell;
use std::rc::Rc;

const BODY: u32 = 0x801F_86A4;
const MODULE_BASE: u32 = 0x801F_69D8;
const RAND: u32 = 0x8005_6798;
const REFUND: u32 = 0x8004_21D4;
const CTX_PTR: u32 = 0x8007_BD24;
const ACTORS: u32 = 0x801C_9370;
const COUNTDOWN: u32 = 0x801F_9D28;
const CTX: u32 = 0x8012_0000;
const ACTOR: u32 = 0x8013_0000;
const STRIDE: u32 = 0x400;

#[test]
fn kiss_of_death_lands_what_retail_lands() {
    let (Some(mut cpu), Some(disc)) = (scus_cpu(), crate::common::disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let module = disc.read_entry(955).expect("PROT 0955");
    let roll = Rc::new(Cell::new(0u32));
    let draws = Rc::new(Cell::new(0u32));
    {
        let (roll, draws) = (roll.clone(), draws.clone());
        cpu.hook(RAND, move |r, _| {
            draws.set(draws.get() + 1);
            r.r[V0] = roll.get();
        });
    }
    let refunded = Rc::new(Cell::new(None::<u8>));
    {
        let refunded = refunded.clone();
        cpu.hook(REFUND, move |r, _| refunded.set(Some(r.r[A0] as u8)));
    }
    // The message bar and the popup push.
    for va in [0x801D_8DE8, 0x801F_44A0] {
        cpu.stub(va, 0);
    }
    cpu.mem.wr32(CTX_PTR, CTX);
    let mut rng = Rng(0x4B49_5353);
    let (mut hits, mut misses, mut stoned, mut refunds) = (0, 0, 0, 0);
    for round in 0..4000 {
        cpu.mem.load(MODULE_BASE, &module);
        for b in 0..0x300 {
            cpu.mem.wr8(CTX + b, 0);
        }
        let caster = 3 + rng.below(4) as u8;
        let victim = rng.below(3) as u8;
        let mut v = CastActorState {
            hp: 1 + rng.below(999) as u16,
            flags: [0, 4, 0x400, 0x0F80, 0x1083, 0x0884][rng.below(6) as usize],
            hp_bar_delta: rng.below(50) as i32,
            init_key: [0, 1, 0x40][rng.below(3) as usize],
            action_category: [0, 1, 2][rng.below(3) as usize],
            queued_action: rng.below(0x80) as u8,
            reaction_gate: rng.below(2) as u8,
            knockdown_anim: 0x0C,
            reaction_alt: 0x0D,
            reaction_alt2: 0x0E,
            ..CastActorState::default()
        };
        for s in 0..8u32 {
            let a = ACTOR + s * STRIDE;
            cpu.mem.wr32(ACTORS + s * 4, a);
            for b in 0..STRIDE {
                cpu.mem.wr8(a + b, 0);
            }
        }
        let va = ACTOR + u32::from(victim) * STRIDE;
        cpu.mem.wr8(ACTOR + u32::from(caster) * STRIDE + 0x1DD, victim);
        cpu.mem.wr16(va + 0x14C, v.hp);
        cpu.mem.wr16(va + 0x16E, v.flags);
        cpu.mem.wr32(va + 0x10, v.hp_bar_delta as u32);
        cpu.mem.wr16(va + 0x16C, v.init_key);
        cpu.mem.wr8(va + 0x1DE, v.action_category);
        cpu.mem.wr8(va + 0x1DF, v.queued_action);
        cpu.mem.wr8(va + 0x1F2, v.reaction_gate);
        cpu.mem.wr8(va + 0x1F1, v.knockdown_anim);
        cpu.mem.wr8(va + 0x1EF, v.reaction_alt);
        cpu.mem.wr8(va + 0x1F0, v.reaction_alt2);
        let turn = rng.below(4) as u8;
        cpu.mem.wr8(CTX + 0x13, caster);
        cpu.mem.wr8(CTX + 0x1A, turn);
        cpu.mem.wr8(CTX + 0x279, 4);
        // Past the gate on this pass: the word reaches zero.
        cpu.mem.wr8(0x1F80_037D, 1);
        cpu.mem.wr8(0x1F80_0393, 1);
        cpu.mem.wr32(COUNTDOWN, 1);
        let r = rng.next() & 0x7FFF;
        roll.set(r);
        draws.set(0);
        refunded.set(None);
        assert_eq!(cpu.call(BODY, &[]), Stop::Returned, "round {round}");

        let mut ctx = CastModuleCtx {
            caster_seat: caster,
            phase: 4,
            turn_cursor: turn,
            ..CastModuleCtx::default()
        };
        let refund = kiss_of_death_tick(&mut ctx, &mut v, Some(r)).1;
        let c = format!("round {round} roll {r} victim {v:?}");
        assert_eq!(draws.get(), 1, "draws: {c}");
        assert_eq!(cpu.mem.rd8(CTX + 0x279), ctx.phase, "phase: {c}");
        assert_eq!(cpu.mem.rd8(CTX + 0x1A), ctx.turn_cursor, "turn: {c}");
        assert_eq!(refunded.get(), refund, "refund: {c}");
        assert_eq!(cpu.mem.rd16(va + 0x14C), v.hp, "hp: {c}");
        assert_eq!(cpu.mem.rd16(va + 0x16E), v.flags, "status: {c}");
        assert_eq!(cpu.mem.rd32(va + 0x10) as i32, v.hp_bar_delta, "bar: {c}");
        assert_eq!(cpu.mem.rd16(va + 0x16C), v.init_key, "init: {c}");
        assert_eq!(cpu.mem.rd8(va + 0x1DE), v.action_category, "category: {c}");
        assert_eq!(cpu.mem.rd8(va + 0x1DA), v.staged_anim, "clip: {c}");
        assert_eq!(cpu.mem.rd8(va + 0x1DC), v.restage, "restage: {c}");
        let odd = r & 1 != 0;
        misses += u32::from(odd);
        stoned += u32::from(!odd && cpu.mem.rd16(va + 0x16E) & 4 != 0);
        hits += u32::from(!odd && cpu.mem.rd16(va + 0x16E) & 4 == 0);
        refunds += u32::from(refund.is_some());
    }
    assert!(
        hits > 1000 && misses > 1000 && stoned > 200 && refunds > 100,
        "{hits} {misses} {stoned} {refunds}"
    );
}
