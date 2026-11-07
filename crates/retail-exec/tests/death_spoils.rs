//! The death arm of the battle anim commit `FUN_8004AD80`
//! (`0x8004B29C..0x8004B65C`) against `battle_steal::resolve_death_spoils`: a
//! thief's loot handed back, the once-per-chain steal attack, its doubled odds
//! and the held-count refusal. The arm runs entered at its own head with the
//! registers the commit holds there (`s1` the actor, `s3` the seat).
//!
//! `FUN_80035394`, the MP-cost halving, sits here too as the other leaf of a
//! spell's price: every ability word against `mp_cost_after_ability_bits`.

use crate::common::{Rng, disc, scus, scus_cpu};
use legaia_engine_battle::battle_steal::{
    self as bs, DeathSpoils, ITEMS_UP_BIT, STEAL_ATTACK_BIT, StealAttackInputs, StealBand,
};
use legaia_engine_battle_vm::battle_formulas::{MpCostModifier, mp_cost_after_ability_bits};
use legaia_game_tables::steal_table::StealTable;
use psx_retail_exec::{RETURN_SENTINEL, Stop};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

const ARM: u32 = 0x8004_B29C;
const ARM_END: u32 = 0x8004_B660;
const BAND: u32 = 0x801C_8FE0;
const CTX: u32 = 0x8012_0000;
const CTX_PTR: u32 = 0x8007_BD24;
const ACTORS: u32 = 0x801C_9370;
const ACTOR: u32 = 0x8013_0000;
const SEATS: u32 = 0x8007_BD10;
const MONSTER_IDS: u32 = 0x8007_BD0C;
const SPECIAL_WORD: u32 = 0x8007_BAC0;
const CHARS: u32 = 0x8008_4708;
const CHAR_STRIDE: u32 = 0x414;
const S1: usize = 17;
const S3: usize = 19;

#[test]
fn a_death_pays_what_retail_pays() {
    let (Some(mut cpu), Some(scus)) = (scus_cpu(), scus()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let table = StealTable::from_scus(&scus).expect("steal table");
    let state = Rc::new(Cell::new(0u32));
    let stream = state.clone();
    cpu.hook(0x8005_6798, move |r, _| {
        let next = stream.get().wrapping_mul(0x41C6_4E6D).wrapping_add(0x3039);
        stream.set(next);
        r.r[2] = (next >> 16) & 0x7FFF;
    });
    for caption in [0x8003_CA78, 0x8003_CB54, 0x8003_CAC4, 0x801D_8DE8] {
        cpu.stub(caption, 0);
    }
    let granted = Rc::new(RefCell::new(Vec::new()));
    let sink = granted.clone();
    cpu.hook(0x8004_21D4, move |r, _| {
        sink.borrow_mut().push(r.r[4] as u8)
    });
    let held = Rc::new(Cell::new(0u32));
    let count = held.clone();
    cpu.hook(0x8004_2F4C, move |r, _| r.r[2] = count.get());
    // The commit's epilogue is out of the arm's scope: the arm's two exits end here.
    cpu.hook(ARM_END, |r, _| r.r[31] = RETURN_SENTINEL);
    cpu.mem.wr32(CTX_PTR, CTX);

    let mut rng = Rng(0x5354_4541);
    let (mut stole, mut handed, mut refused) = (0, 0, 0);
    for round in 0..60000 {
        let party = 1 + rng.below(3);
        let m = rng.below(4);
        let seat = 3 + m;
        let acting = if rng.below(4) == 0 {
            3 + rng.below(5)
        } else {
            rng.below(party)
        };
        let loot = if rng.below(4) == 0 {
            1 + rng.below(255)
        } else {
            0
        };
        let took = rng.below(2);
        let caption_up = rng.below(4) == 0;
        let attempted = rng.below(4) == 0;
        let special = if rng.below(8) == 0 { 2 } else { 0 };
        let category = [3, 3, 3, 2, 0][rng.below(5) as usize] as u8;
        // Row `0xBF` holds the neighbouring table's `[9, 0]`; no shipped monster
        // reaches it (see the test below), and the port's "no item, no steal" guard
        // answers it where retail would add item `0`.
        let id = 1 + rng.below(0xBE) as u8;
        let seed = rng.next();
        let held_count = [0, 1, 98, 99, 0][rng.below(5) as usize];

        cpu.mem.wr8(CTX, party as u8);
        cpu.mem.wr8(CTX + 0x13, acting as u8);
        cpu.mem.wr8(CTX + 0x18, if caption_up { 0x5B } else { 1 });
        cpu.mem.wr8(CTX + 0x27, u8::from(attempted));
        cpu.mem.wr32(SPECIAL_WORD, special);
        cpu.mem.wr8(MONSTER_IDS + m, id);
        for s in 0..8 {
            cpu.mem.wr32(BAND + s * 4, 0);
            cpu.mem.wr32(BAND + 0x40 + s * 4, 0);
        }
        cpu.mem.wr32(BAND + m * 4, loot);
        cpu.mem.wr32(BAND + (seat + 5) * 4, took);
        let mut words = [[0u32; 2]; 3];
        let mut hp = [0u16; 3];
        for s in 0..8u32 {
            cpu.mem.wr32(ACTORS + s * 4, ACTOR + s * 0x400);
            cpu.mem.wr8(ACTOR + s * 0x400 + 0x1DE, category);
        }
        for i in 0..party as usize {
            let c = CHARS + i as u32 * CHAR_STRIDE;
            words[i] = [
                if rng.below(2) == 0 {
                    STEAL_ATTACK_BIT
                } else {
                    0
                },
                if rng.below(3) == 0 { ITEMS_UP_BIT } else { 0 },
            ];
            hp[i] = rng.below(2) as u16 * 40;
            cpu.mem.wr8(SEATS + i as u32, i as u8 + 1);
            cpu.mem.wr32(c + 0xF4, words[i][0]);
            cpu.mem.wr32(c + 0xF8, words[i][1]);
            cpu.mem.wr16(ACTOR + i as u32 * 0x400 + 0x14C, hp[i]);
        }

        state.set(seed);
        held.set(held_count);
        granted.borrow_mut().clear();
        cpu.regs.r[S1] = ACTOR + seat * 0x400;
        cpu.regs.r[S3] = seat;
        cpu.regs.r[2] = 0x801D_0000;
        let stop = cpu.call(ARM, &[]);
        assert!(matches!(stop, Stop::Returned), "round {round}: {stop:?}");

        let mut band = StealBand {
            attempted,
            ..Default::default()
        };
        band.stolen[m as usize] = loot as u8;
        band.took[m as usize] = took;
        let inputs = StealAttackInputs {
            acting_party: acting < 3,
            special_battle: special != 0,
            killer_attack: category == bs::ATTACK_CATEGORY,
            killer_steals: acting < party && words[acting as usize][0] & STEAL_ATTACK_BIT != 0,
            items_up: (0..party as usize).any(|i| hp[i] != 0 && words[i][1] & ITEMS_UP_BIT != 0),
            entry: table.entry(u16::from(id)),
            caption_up,
        };
        let port_state = Cell::new(seed);
        let spoils = bs::resolve_death_spoils(
            &mut band,
            m as usize,
            &inputs,
            || {
                let n = port_state
                    .get()
                    .wrapping_mul(0x41C6_4E6D)
                    .wrapping_add(0x3039);
                port_state.set(n);
                (n >> 16) & 0x7FFF
            },
            |_| held_count as u8,
        );
        let retail = granted.borrow().first().copied();
        match spoils {
            Some(DeathSpoils::Stole(_)) => stole += 1,
            Some(_) => handed += 1,
            None if held_count == 99 => refused += 1,
            None => {}
        }
        let context = format!(
            "round {round}: party {party} seat {seat} acting {acting} loot {loot} id {id:#x} \
             {inputs:?} held {held_count}"
        );
        assert_eq!(retail, spoils.map(|s| s.item()), "{context}");
        assert_eq!(state.get(), port_state.get(), "{context}");
        assert_eq!(cpu.mem.rd8(CTX + 0x27) != 0, band.attempted, "{context}");
        assert_eq!(
            cpu.mem.rd32(BAND + m * 4),
            u32::from(band.stolen[m as usize]),
            "{context}"
        );
    }
    assert!(
        stole > 50 && handed > 500 && refused > 50,
        "{stole} {handed} {refused}"
    );
}

#[test]
fn no_shipped_monster_steals_item_zero() {
    let (Some(scus), Some(disc)) = (scus(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let table = StealTable::from_scus(&scus).expect("steal table");
    let entry = disc.read_entry(867).expect("PROT 0867");
    let records = legaia_actor_formats::monster_archive::records(&entry).expect("archive");
    assert!(!records.is_empty());
    for r in &records {
        let e = table.entry(r.id).expect("row");
        assert!(e.chance_pct == 0 || e.item_id != 0, "{:#x}: {e:?}", r.id);
    }
}

#[test]
fn a_spell_costs_what_retail_charges() {
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let word = 0x8008_47FC;
    for bits in [0u32, 0x10, 0x20, 0x30] {
        cpu.mem.wr32(word, bits);
        let modifier = match bits {
            0 => MpCostModifier::Full,
            0x10 => MpCostModifier::Quarter,
            _ => MpCostModifier::Half,
        };
        for cost in (0..=u32::from(u16::MAX))
            .step_by(7)
            .chain([0, 1, 2, 3, 0xFFFF])
        {
            let stop = cpu.call(0x8003_5394, &[0, cost]);
            assert!(matches!(stop, Stop::Returned));
            let port = mp_cost_after_ability_bits(cost as u16, modifier);
            assert_eq!(cpu.regs.r[2], u32::from(port), "bits {bits:#x} cost {cost}");
        }
    }
}
