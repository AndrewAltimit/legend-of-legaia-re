//! `FUN_801E7320`, the seed's retarget for a monster under the `0x380` group,
//! against `World::resolve_monster_target`, in a party of three and of two.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_core::world::World;
use legaia_save::Party;
use psx_retail_exec::{Stop, V0};
use std::cell::Cell;
use std::rc::Rc;

const ACTOR: u32 = 0x8013_0000;
const CTX: u32 = 0x8012_0000;

#[test]
fn a_confused_monster_turns_where_retail_turns() {
    let (Some(mut cpu), Some(d)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem
        .load(0x801C_E818, &d.read_entry(898).expect("PROT 0898"));
    let state = Rc::new(Cell::new(0u32));
    let stream = state.clone();
    cpu.hook(0x8005_6798, move |r, _| {
        let next = stream.get().wrapping_mul(0x41C6_4E6D).wrapping_add(0x3039);
        stream.set(next);
        r.r[V0] = (next >> 16) & 0x7FFF;
    });
    cpu.mem.wr32(0x8007_BD24, CTX);
    let mut w = World::new();
    let mut rng = Rng(0x434F_4E46);
    let (mut fizzled, mut failures) = (0, Vec::new());
    for round in 0..40000 {
        let party = 2 + rng.below(2) as u8;
        let monsters = 1 + rng.below(3) as u8;
        let caster = rng.below(u32::from(monsters)) as u8;
        // Retail seats: party, then monsters from 3.
        let class_kind = rng.below(4);
        let (retail_class, port_class) = match class_kind {
            0 => {
                let t = rng.below(u32::from(party)) as u8;
                (t, t)
            }
            1 => {
                let m = rng.below(u32::from(monsters)) as u8;
                (3 + m, party + m)
            }
            2 => (8, 8),
            _ => (9, 9),
        };
        let party_up: [bool; 3] = std::array::from_fn(|_| rng.below(3) != 0);
        let monster_up: [bool; 3] = std::array::from_fn(|_| rng.below(3) != 0);
        let mut party_up = party_up;
        let mut monster_up = monster_up;
        party_up[rng.below(u32::from(party)) as usize] = true;
        monster_up[usize::from(caster)] = true;
        let seed = rng.next();

        cpu.mem.wr8(CTX, party);
        cpu.mem.wr8(CTX + 1, monsters);
        cpu.mem.wr8(CTX + 0x13, 3 + caster);
        for s in 0..8u32 {
            let a = ACTOR + s * 0x400;
            cpu.mem.wr32(0x801C_9370 + s * 4, a);
            cpu.mem.wr16(a + 0x14C, 0);
            cpu.mem.wr8(a + 0x1DE, 3);
        }
        for s in 0..party {
            cpu.mem.wr16(
                ACTOR + u32::from(s) * 0x400 + 0x14C,
                u16::from(party_up[usize::from(s)]),
            );
        }
        for m in 0..monsters {
            cpu.mem.wr16(
                ACTOR + u32::from(3 + m) * 0x400 + 0x14C,
                u16::from(monster_up[usize::from(m)]),
            );
        }
        let me = ACTOR + u32::from(3 + caster) * 0x400;
        cpu.mem.wr8(me + 0x1DD, retail_class);
        state.set(seed);
        assert_eq!(cpu.call(0x801E_7320, &[]), Stop::Returned, "round {round}");
        let cat = cpu.mem.rd8(me + 0x1DE);
        let t = cpu.mem.rd8(me + 0x1DD);
        // The byte, re-seated in the port's frame.
        let t_port = match t {
            8 | 9 => t,
            t if t >= 3 => t - 3 + party,
            t => t,
        };
        let retail = (cat, t_port, state.get());

        w.load_party(Party::zeroed(3));
        w.enter_battle(party, monsters);
        w.party.party_count = party;
        for s in 0..usize::from(party) + usize::from(monsters) {
            let up = if s < usize::from(party) {
                party_up[s]
            } else {
                monster_up[s - usize::from(party)]
            };
            let a = &mut w.actors[s];
            a.active = true;
            a.battle.max_hp = 1;
            a.battle.hp = u16::from(up);
            a.battle.liveness = u16::from(up);
            a.battle_monster_id = (s >= usize::from(party)).then_some(0x40);
        }
        let me = usize::from(party + caster);
        w.actors[me].battle.action_category = 3;
        w.actors[me].battle.active_target = port_class;
        w.rng_state = seed;
        w.resolve_monster_target(party + caster);
        let b = &w.actors[me].battle;
        let port = (b.action_category, b.active_target, w.rng_state);
        fizzled += u32::from(cat == 0);
        if retail != port && failures.len() < 6 {
            failures.push(format!(
                "round {round} party {party} monsters {monsters} caster {caster} class {retail_class}:\n retail {retail:?}\n port   {port:?}"
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert!(fizzled > 1000, "{fizzled}");
}
