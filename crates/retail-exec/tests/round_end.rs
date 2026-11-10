//! The round end's Numb waker `FUN_801F45A4` against
//! `World::tick_status_0x400_wakes`: seven seats, live or down, with Numb
//! (`0x400`) in the raw half of the port's word, in its status tracker, or in
//! neither, on one `rand()` stream.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_battle_vm::status_effects::StatusKind;
use legaia_engine_core::world::World;
use legaia_save::Party;
use psx_retail_exec::{Stop, V0};
use std::cell::Cell;
use std::rc::Rc;

const WAKER: u32 = 0x801F_45A4;
const RAND: u32 = 0x8005_6798;
const OVERLAY_BASE: u32 = 0x801C_E818;
const ACTORS: u32 = 0x801C_9370;
const ACTOR: u32 = 0x8013_0000;
const ACTOR_STRIDE: u32 = 0x400;

#[test]
fn numb_wears_off_where_retail_wears_it_off() {
    differential!(prot(898, WAKER));
    let (Some(mut cpu), Some(disc)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem
        .load(OVERLAY_BASE, &disc.read_entry(898).expect("PROT 0898"));
    let state = Rc::new(Cell::new(0u32));
    let stream = state.clone();
    cpu.hook(RAND, move |r, _| {
        let next = stream.get().wrapping_mul(0x41C6_4E6D).wrapping_add(0x3039);
        stream.set(next);
        r.r[V0] = (next >> 16) & 0x7FFF;
    });
    let mut rng = Rng(0x4E55_4D42);
    let (mut failures, mut woke) = (Vec::new(), 0);
    let mut w = World::new();
    for round in 0..20000 {
        w.load_party(Party::zeroed(3));
        w.enter_battle(3, 4);
        w.battle.status_effects = Default::default();
        let seed = rng.next();
        let mut seats = Vec::new();
        for s in 0..7u32 {
            let hp = [0u16, 1, 500][rng.below(3) as usize];
            let other = [0u16, 1, 4, 0x80, 0x800, 0x1000][rng.below(6) as usize];
            // 0: no Numb, 1: the raw half, 2: the tracker.
            let numb = rng.below(3);
            let a = ACTOR + s * ACTOR_STRIDE;
            cpu.mem.wr32(ACTORS + s * 4, a);
            cpu.mem.wr16(a + 0x14C, hp);
            cpu.mem
                .wr16(a + 0x16E, other | if numb != 0 { 0x400 } else { 0 });
            let b = &mut w.actors[s as usize].battle;
            b.hp = hp;
            b.liveness = u16::from(hp != 0);
            b.field_flags = other | if numb == 1 { 0x400 } else { 0 };
            if numb == 2 {
                w.battle.status_effects.apply(s as u8, StatusKind::Numb);
            }
            seats.push((hp, other, numb));
        }
        state.set(seed);
        w.rng_state = seed;
        assert_eq!(cpu.call(WAKER, &[]), Stop::Returned);
        w.tick_status_0x400_wakes();
        let retail: Vec<u16> = (0..7u32)
            .map(|s| cpu.mem.rd16(ACTOR + s * ACTOR_STRIDE + 0x16E))
            .collect();
        let port: Vec<u16> = (0..7u8).map(|s| w.raw_status_word(s)).collect();
        woke += retail
            .iter()
            .zip(&seats)
            .filter(|(r, (_, _, n))| *n != 0 && **r & 0x400 == 0)
            .count();
        if (retail.clone(), state.get()) != (port.clone(), w.rng_state) && failures.len() < 6 {
            failures.push(format!(
                "round {round}: retail {retail:x?} port {port:x?} seats {seats:?}"
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert!(woke > 1000, "{woke} wakes");
}
