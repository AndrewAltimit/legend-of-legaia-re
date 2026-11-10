//! The scene-entry top-up of the encounter step counter, the slice
//! `0x8003AC78..0x8003ACB4` of the system-script installer `FUN_8003AB2C`,
//! with the field overlay's reroll `FUN_801DDF48` live beneath it, against
//! `encounter_counter_scene_entry_top_up`: the signed `< 0x1E7` gate and the
//! half reroll rounded toward zero.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_battle::region_encounter::encounter_counter_scene_entry_top_up;
use psx_retail_exec::{RA, RETURN_SENTINEL, Stop, V0};
use std::cell::Cell;
use std::rc::Rc;

const SLICE: u32 = 0x8003_AC78;
const SLICE_END: u32 = 0x8003_ACB4;
const OVERLAY_BASE: u32 = 0x801C_E818;
const RAND: u32 = 0x8005_6798;
const COUNTER: u32 = 0x8007_B5FC;

fn lcg(s: u32) -> u32 {
    s.wrapping_mul(0x41C6_4E6D).wrapping_add(0x3039)
}

#[test]
fn the_scene_entry_top_up_is_retails() {
    differential!(scus(SLICE));
    let (Some(mut cpu), Some(disc)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem
        .load(OVERLAY_BASE, &disc.read_entry(897).expect("PROT 0897"));
    let stream = Rc::new(Cell::new(0u32));
    {
        let stream = stream.clone();
        cpu.hook(RAND, move |r, _| {
            let next = lcg(stream.get());
            stream.set(next);
            r.r[V0] = (next >> 16) & 0x7FFF;
        });
    }
    cpu.hook(SLICE_END, |r, _| r.r[RA] = RETURN_SENTINEL);
    let mut rng = Rng(0x544F_5055);
    let (mut topped, mut carried) = (0, 0);
    for round in 0..20000 {
        let counter = match rng.below(4) {
            0 => 0x1E7 + rng.below(2000) as i32,
            1 => -(rng.below(2000) as i32),
            _ => rng.below(0x1E8) as i32,
        };
        cpu.mem.wr32(COUNTER, counter as u32);
        let seed = rng.next();
        stream.set(seed);
        assert_eq!(cpu.call(SLICE, &[]), Stop::Returned, "round {round}");
        let mut s = seed;
        let want = encounter_counter_scene_entry_top_up(counter, || {
            s = lcg(s);
            (s >> 16) & 0x7FFF
        });
        let c = format!("round {round} counter {counter}");
        assert_eq!(cpu.mem.rd32(COUNTER) as i32, want, "counter: {c}");
        assert_eq!(stream.get(), s, "draws: {c}");
        topped += u32::from(want != counter);
        carried += u32::from(counter >= 0x1E7);
    }
    assert!(topped > 5000 && carried > 2000, "{topped} {carried}");
}
