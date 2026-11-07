//! PROT 0907 (Nighto) arm 0's rolls (`0x801F6B50..0x801F6D30`) run off the
//! disc against the draws `World::nighto_verdict` makes and `nighto_outcome`:
//! the kill throw, the resist throw an immune target skips, the extra throw
//! one character takes, and the two words arm 13 forks on.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_cast::seru_magic_ticks::{
    NIGHTO_EXTRA_ROLL_CHARACTER, NightoOutcome, NightoRoll, nighto_outcome,
};
use legaia_retail_exec::{RA, RETURN_SENTINEL, Stop, V0};
use std::cell::Cell;
use std::rc::Rc;

const SLICE: u32 = 0x801F_6B50;
const SLICE_END: u32 = 0x801F_6D30;
const MODULE_BASE: u32 = 0x801F_69D8;
const RAND: u32 = 0x8005_6798;
const KILL_WORD: u32 = 0x801F_8534;
const RESIST_WORD: u32 = 0x801F_853C;
const ACTORS: u32 = 0x801C_9370;
const MONSTER_RECORDS: u32 = 0x801C_9348;
const SEATS: u32 = 0x8007_BD10;
const RECORDS: u32 = 0x8008_4140;
const CTX: u32 = 0x8012_0000;
const ACTOR: u32 = 0x8013_0000;
const MREC: u32 = 0x8015_0000;
const S2: usize = 18;
const S7: usize = 23;
const FP: usize = 30;

fn lcg(s: u32) -> u32 {
    s.wrapping_mul(0x41C6_4E6D).wrapping_add(0x3039)
}

#[test]
fn the_nighto_throws_are_retails() {
    let (Some(mut cpu), Some(disc)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem
        .load(MODULE_BASE, &disc.read_entry(907).expect("PROT 0907"));
    let stream = Rc::new(Cell::new(0u32));
    let draws = Rc::new(Cell::new(0u32));
    {
        let (stream, draws) = (stream.clone(), draws.clone());
        cpu.hook(RAND, move |r, _| {
            let next = lcg(stream.get());
            stream.set(next);
            draws.set(draws.get() + 1);
            r.r[V0] = (next >> 16) & 0x7FFF;
        });
    }
    cpu.hook(SLICE_END, |r, _| r.r[RA] = RETURN_SENTINEL);
    cpu.mem.wr32(0x8007_BD24, CTX);
    for s in 0..8 {
        cpu.mem.wr32(ACTORS + s * 4, ACTOR + s * 0x400);
    }
    for m in 0..4 {
        cpu.mem.wr32(MONSTER_RECORDS + m * 4, MREC + m * 0x100);
    }
    let mut rng = Rng(0x4E49_4748);
    let (mut immune_n, mut extra_n, mut kills, mut resists) = (0, 0, 0, 0);
    for round in 0..20000 {
        let caster = rng.below(3);
        let character = 1 + rng.below(3) as u8;
        let victim = 3 + rng.below(4);
        let spell = 0x80 + rng.below(0x20) as u8;
        let level = rng.below(10) as u8;
        cpu.mem.wr8(CTX + 0x13, caster as u8);
        cpu.mem.wr8(SEATS + caster, character);
        cpu.mem.wr8(ACTOR + caster * 0x400 + 0x1DF, spell);
        let rec = RECORDS + u32::from(character - 1) * 0x414;
        let slot = rng.below(0x20);
        for i in 0..0x20 {
            cpu.mem
                .wr8(rec + 0x705 + i, if i == slot { spell } else { 0 });
            cpu.mem
                .wr8(rec + 0x729 + i, if i == slot { level } else { 0 });
        }
        let scripted = rng.below(3) == 0;
        let byte = rng.below(2) as u8;
        cpu.mem.wr8(CTX + 0x287, u8::from(scripted));
        cpu.mem.wr8(MREC + (victim - 3) * 0x100 + 0x20, byte);
        let immune = scripted && byte != 0;
        cpu.mem.wr32(KILL_WORD, 0xDEAD);
        cpu.mem.wr32(RESIST_WORD, 0xDEAD);
        let seed = rng.next();
        stream.set(seed);
        draws.set(0);
        cpu.regs.r[FP] = 0x8008_0000;
        cpu.regs.r[S7] = ACTORS;
        cpu.regs.r[S2] = victim;
        assert_eq!(cpu.call(SLICE, &[]), Stop::Returned, "round {round}");

        // The port's draws, in `nighto_verdict`'s order.
        let mut s = seed;
        let mut n = 0u32;
        let mut rand = || {
            n += 1;
            s = lcg(s);
            (s >> 16) & 0x7FFF
        };
        let kill_roll = rand();
        let resist_roll = if immune { 0 } else { rand() };
        let extra_roll = (character == NIGHTO_EXTRA_ROLL_CHARACTER).then(&mut rand);
        let out = nighto_outcome(&NightoRoll {
            kill_roll,
            resist_roll,
            magic_level: level,
            target_immune: immune,
            extra_roll,
        });
        let ctx = format!("round {round} immune {immune} char {character} level {level}");
        assert_eq!(draws.get(), n, "draws: {ctx}");
        let deadly = cpu.mem.rd32(KILL_WORD) == 0;
        let resisted = cpu.mem.rd32(RESIST_WORD) != 0;
        let want = match (deadly, resisted) {
            (true, false) => NightoOutcome::Kill,
            (true, true) => NightoOutcome::KillResisted,
            (false, false) => NightoOutcome::Confuse,
            (false, true) => NightoOutcome::ConfuseResisted,
        };
        assert_eq!(out, want, "outcome: {ctx}");
        immune_n += u32::from(immune);
        extra_n += u32::from(extra_roll.is_some());
        kills += u32::from(deadly);
        resists += u32::from(resisted);
    }
    assert!(
        immune_n > 1000 && extra_n > 3000 && kills > 1000 && resists > 1000,
        "{immune_n} {extra_n} {kills} {resists}"
    );
}
