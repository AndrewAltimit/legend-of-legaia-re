//! PROT 0964's Element Change (`FUN_801F88EC`, action `0xAF`) run through its
//! arm 0 off the disc against `element_change_tick`: the re-roll's `rand()`
//! draws, the first monster record's element `+0x1D` and group `+0x1C`, and
//! the last-roll word `0x801C8FE4`.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_cast::cast_module_ticks::{
    CastActorState, CastModuleCtx, ELEMENT_CHANGE_ELEMENTS, element_change_tick,
};
use psx_retail_exec::{Stop, V0};
use std::cell::Cell;
use std::rc::Rc;

const TICK: u32 = 0x801F_88EC;
const MODULE_BASE: u32 = 0x801F_69D8;
const OVERLAY_BASE: u32 = 0x801C_E818;
const RAND: u32 = 0x8005_6798;
const CTX_PTR: u32 = 0x8007_BD24;
const ACTORS: u32 = 0x801C_9370;
const MONSTER_RECORDS: u32 = 0x801C_9348;
const LAST_ROLL: u32 = 0x801C_8FE4;
const CTX: u32 = 0x8012_0000;
const ACTOR: u32 = 0x8013_0000;
const REC: u32 = 0x8015_0000;

fn lcg(s: u32) -> u32 {
    s.wrapping_mul(0x41C6_4E6D).wrapping_add(0x3039)
}

#[test]
fn the_element_reroll_draws_what_retail_draws() {
    let (Some(mut cpu), Some(disc)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem
        .load(OVERLAY_BASE, &disc.read_entry(898).expect("PROT 0898"));
    cpu.mem
        .load(MODULE_BASE, &disc.read_entry(964).expect("PROT 0964"));
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
    for va in [0x8004_FCC8, 0x801D_829C, 0x8005_0ED4] {
        cpu.stub(va, 0);
    }
    cpu.mem.wr32(CTX_PTR, CTX);
    for s in 0..8 {
        cpu.mem.wr32(ACTORS + s * 4, ACTOR + s * 0x400);
    }
    cpu.mem.wr32(MONSTER_RECORDS, REC);
    let mut rng = Rng(0x454C_454D);
    let mut rerolled = 0;
    for round in 0..3000 {
        let last = rng.below(3) as u8;
        cpu.mem.wr32(LAST_ROLL, u32::from(last));
        cpu.mem.wr8(CTX + 0x13, 3 + rng.below(4) as u8);
        cpu.mem.wr8(CTX + 0x279, 0);
        let seed = rng.next();
        stream.set(seed);
        draws.set(0);
        assert_eq!(cpu.call(TICK, &[]), Stop::Returned, "round {round}");

        let mut ctx = CastModuleCtx::default();
        let mut seats = [CastActorState::default(); 7];
        let mut s = seed;
        let mut n = 0u32;
        let (_, out) = element_change_tick(&mut ctx, &mut seats, last, || {
            n += 1;
            s = lcg(s);
            (s >> 16) & 0x7FFF
        });
        let out = out.expect("arm 0 rolls");
        let ctx = format!("round {round} last {last}");
        assert_eq!(draws.get(), n, "draws: {ctx}");
        assert_eq!(cpu.mem.rd32(LAST_ROLL), u32::from(out.roll), "roll: {ctx}");
        assert_eq!(cpu.mem.rd8(REC + 0x1D), out.element, "element: {ctx}");
        assert_eq!(
            out.element,
            ELEMENT_CHANGE_ELEMENTS[usize::from(out.roll)],
            "{ctx}"
        );
        assert_eq!(cpu.mem.rd8(REC + 0x1C), out.group, "group: {ctx}");
        rerolled += u32::from(n > 1);
    }
    assert!(rerolled > 500, "{rerolled}");
}
