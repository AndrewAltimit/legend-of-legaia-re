//! The dead-target redirect `FUN_801DB124`, run on its own, against
//! `redirect_dead_target` with the class byte `World` hands it
//! (`SpellNameTable`'s `+0` class off `SCUS_942.54`): every category, cast
//! ids across the low tiers and the real classes, both item ids that
//! re-roll and the ones that do not, targets on both sides and past the
//! seat table, and formations from one seat to a full side with fallen seats
//! among them. The final `+0x1DD` and the number of `rand()`s drawn are
//! compared.

use crate::common::{Rng, disc, scus, scus_cpu};
use legaia_engine_battle_vm::battle_action::{RedirectQuery, redirect_dead_target};
use legaia_game_tables::spell_names::SpellNameTable;
use psx_retail_exec::{Stop, V0};
use std::cell::Cell;
use std::rc::Rc;

const REDIRECT: u32 = 0x801D_B124;
const RAND: u32 = 0x8005_6798;
const OVERLAY_BASE: u32 = 0x801C_E818;
const CTX_PTR: u32 = 0x8007_BD24;
const CTX: u32 = 0x8012_0000;
const ACTORS: u32 = 0x801C_9370;
const ACTOR: u32 = 0x8013_0000;

#[test]
fn a_dead_target_redirects_where_retail_redirects() {
    differential!(prot(898, REDIRECT));
    let (Some(mut cpu), Some(scus), Some(disc)) = (scus_cpu(), scus(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem
        .load(OVERLAY_BASE, &disc.read_entry(898).expect("PROT 0898"));
    cpu.mem.wr32(CTX_PTR, CTX);
    for s in 0..8 {
        cpu.mem.wr32(ACTORS + s * 4, ACTOR + s * 0x400);
    }
    let spells = SpellNameTable::from_scus(&scus).expect("spell table");
    let state = Rc::new(Cell::new(0u32));
    let draws = Rc::new(Cell::new(0u32));
    {
        let (state, draws) = (state.clone(), draws.clone());
        cpu.hook(RAND, move |r, _| {
            let next = state.get().wrapping_mul(0x41C6_4E6D).wrapping_add(0x3039);
            state.set(next);
            draws.set(draws.get() + 1);
            r.r[V0] = (next >> 16) & 0x7FFF;
        });
    }
    let mut rng = Rng(0x5245_4449);
    let (mut party_rolls, mut monster_rolls, mut kept) = (0, 0, 0);
    for n in 0..40_000 {
        let party = 1 + rng.below(3) as u8;
        let monsters = 1 + rng.below(5) as u8;
        cpu.mem.wr8(CTX, party);
        cpu.mem.wr8(CTX + 1, monsters);
        let mut alive = [false; 8];
        for (s, a) in alive.iter_mut().enumerate() {
            let seated = if s < 3 {
                (s as u8) < party
            } else {
                ((s - 3) as u8) < monsters
            };
            *a = seated && rng.below(3) != 0;
            cpu.mem.wr16(
                ACTOR + s as u32 * 0x400 + 0x14C,
                if *a { 1 + rng.below(999) as u16 } else { 0 },
            );
        }
        // Every side keeps a standing seat, or both sides loop for ever.
        let p = rng.below(u32::from(party)) as usize;
        let m = 3 + rng.below(u32::from(monsters)) as usize;
        for s in [p, m] {
            alive[s] = true;
            cpu.mem.wr16(ACTOR + s as u32 * 0x400 + 0x14C, 1);
        }
        let actor_seat = rng.below(8);
        cpu.mem.wr8(CTX + 0x274, actor_seat as u8);
        let a = ACTOR + actor_seat * 0x400;
        let target = match rng.below(8) {
            0 => 8 + rng.below(4) as u8,
            _ => rng.below(8) as u8,
        };
        let category = rng.below(5) as u8;
        let param0 = match (category, rng.below(3)) {
            (1, 0) => 0xFE,
            (1, 1) => 0x98,
            _ => rng.below(0xA4) as u8,
        };
        // The actor may be its own target: its `+0x14C` is then the one read.
        cpu.mem.wr8(a + 0x1DD, target);
        cpu.mem.wr8(a + 0x1DE, category);
        cpu.mem.wr8(a + 0x1DF, param0);
        let seed = rng.next();
        state.set(seed);
        draws.set(0);
        assert_eq!(cpu.call(REDIRECT, &[]), Stop::Returned, "n {n}");
        let retail = cpu.mem.rd8(a + 0x1DD);
        let retail_draws = draws.get();

        let class = spells.entry(param0).map_or(0, |e| e.class);
        let port_state = Rc::new(Cell::new(seed));
        let port_draws = Rc::new(Cell::new(0u32));
        let roll = || {
            let next = port_state
                .get()
                .wrapping_mul(0x41C6_4E6D)
                .wrapping_add(0x3039);
            port_state.set(next);
            port_draws.set(port_draws.get() + 1);
            ((next >> 16) & 0x7FFF) as i32
        };
        let port = redirect_dead_target(
            RedirectQuery {
                target_slot: target,
                category,
                param0,
            },
            party,
            monsters,
            roll,
            |s| alive.get(usize::from(s)).copied().unwrap_or(false),
            |_| class,
        )
        .unwrap_or(target);
        assert_eq!(
            (retail, retail_draws),
            (port, port_draws.get()),
            "n {n} target {target} category {category} param {param0:#x} class {class:#x} \
             party {party} monsters {monsters} alive {alive:?}"
        );
        match (retail_draws, target < 3) {
            (0, _) => kept += 1,
            (_, true) => party_rolls += 1,
            (_, false) => monster_rolls += 1,
        }
    }
    assert!(
        party_rolls > 2000 && monster_rolls > 2000 && kept > 10_000,
        "{party_rolls} {monster_rolls} {kept}"
    );
}
