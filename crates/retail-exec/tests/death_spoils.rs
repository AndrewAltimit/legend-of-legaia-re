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
    differential!(scus(ARM));
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
    not_a_differential!(
        "a census of the steal table against the PROT 0867 records; runs no routine"
    );
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
    differential!(scus(0x8003_5394));
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

const COMMIT: u32 = 0x8004_AD80;
const COMMIT_END: u32 = 0x8004_C140;
/// The first instruction past the death arm's tail (`+0x1DC` raised, the
/// Seru-staged get-up at `0x8004B688..0x8004B6A0`).
const DEATH_TAIL_END: u32 = 0x8004_B6A4;
const MONSTER_TABLES: u32 = 0x801C_9348;
const NODE: u32 = 0x8014_0000;
const TABLE: u32 = 0x8014_1000;
const ENTRY: u32 = 0x8015_0000;
const CUE_RING: u32 = 0x8007_B6D8;

/// The death arm run from the commit's own entry, as a monster's knockdown
/// commits it: the defeat credit (`defeat_credit`), the defeat cue's gate
/// (`defeat_cue_due`) and the spoils (`resolve_death_spoils`) in one pass,
/// with the arm's tail raising `+0x1DC` bit 3 or, with a Seru staged, standing
/// the body up on its get-up entry.
#[test]
fn the_commit_runs_the_death_arm_from_its_entry() {
    differential!(scus(COMMIT));
    let (Some(mut cpu), Some(scus)) = (scus_cpu(), scus()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let table = StealTable::from_scus(&scus).expect("steal table");
    cpu.stub_outside = Some((COMMIT, COMMIT_END));
    let state = Rc::new(Cell::new(0u32));
    let stream = state.clone();
    cpu.hook(0x8005_6798, move |r, _| {
        let next = stream.get().wrapping_mul(0x41C6_4E6D).wrapping_add(0x3039);
        stream.set(next);
        r.r[2] = (next >> 16) & 0x7FFF;
    });
    let granted = Rc::new(RefCell::new(Vec::new()));
    let sink = granted.clone();
    cpu.hook(0x8004_21D4, move |r, _| {
        sink.borrow_mut().push(r.r[4] as u8)
    });
    let held = Rc::new(Cell::new(0u32));
    let count = held.clone();
    cpu.hook(0x8004_2F4C, move |r, _| r.r[2] = count.get());
    cpu.hook(DEATH_TAIL_END, |r, _| r.r[31] = RETURN_SENTINEL);

    let mut rng = Rng(0x4445_4154);
    let (mut credited, mut cued, mut silenced, mut paid, mut stood) = (0, 0, 0, 0, 0);
    for round in 0..40000 {
        for o in (0..0x300).step_by(4) {
            cpu.mem.wr32(CTX + o, 0);
        }
        cpu.mem.wr32(CTX_PTR, CTX);
        let party = 1 + rng.below(3);
        let m = rng.below(4);
        let seat = 3 + m;
        let acting = if rng.below(4) == 0 {
            3 + rng.below(5)
        } else {
            rng.below(party)
        };
        let flags = [0u8, 0x08, 0x04, 0x0C, rng.next() as u8][rng.below(5) as usize];
        let seru = rng.below(6) == 0;
        let scripted = rng.below(3) == 0;
        let second = [0u8, 0, 0x12][rng.below(3) as usize];
        let cursor = rng.below(4) as u8;
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
        // Row `0xBF` is the neighbouring table's (see the arm's own test);
        // `0xAA` would open the Jette fight's monster-count arm.
        let id = loop {
            let id = 1 + rng.below(0xBE) as u8;
            if id != 0xAA {
                break id;
            }
        };
        let seed = rng.next();
        let held_count = [0, 1, 98, 99, 0][rng.below(5) as usize];
        let getup = rng.below(0x10) as u8;
        let defeated: Vec<u32> = (0..3).map(|_| rng.below(1000)).collect();

        cpu.mem.wr8(CTX, party as u8);
        cpu.mem.wr8(CTX + 1, 4);
        cpu.mem.wr8(CTX + 9, cursor);
        cpu.mem.wr8(CTX + 0x13, acting as u8);
        cpu.mem.wr8(CTX + 0x18, if caption_up { 0x5B } else { 1 });
        cpu.mem.wr8(CTX + 0x27, u8::from(attempted));
        cpu.mem.wr8(CTX + 0x269, u8::from(seru));
        cpu.mem.wr8(CTX + 0x287, u8::from(scripted));
        cpu.mem.wr32(SPECIAL_WORD, special);
        for k in 0..4 {
            cpu.mem.wr8(MONSTER_IDS + k, 0);
            cpu.mem.wr16(CUE_RING + 2 * k, 0);
        }
        cpu.mem.wr8(MONSTER_IDS + m, id);
        if m != 1 {
            cpu.mem.wr8(MONSTER_IDS + 1, second);
        }
        let second = cpu.mem.rd8(MONSTER_IDS + 1);
        for s in 0..8 {
            cpu.mem.wr32(BAND + s * 4, 0);
            cpu.mem.wr32(BAND + 0x40 + s * 4, 0);
        }
        cpu.mem.wr32(BAND + m * 4, loot);
        cpu.mem.wr32(BAND + (seat + 5) * 4, took);

        // Every seat's actor, the victim's knockdown entry and its table.
        for s in 0..8u32 {
            let a = ACTOR + s * 0x400;
            for o in (0x14C..0x230).step_by(4) {
                cpu.mem.wr32(a + o, 0);
            }
            cpu.mem.wr32(ACTORS + s * 4, a);
            cpu.mem.wr8(a + 0x1DE, category);
            cpu.mem.wr16(a + 0x14C, 1);
        }
        for o in (0..0x100).step_by(4) {
            cpu.mem.wr32(TABLE + o, 0);
            cpu.mem.wr32(ENTRY + o, 0);
            cpu.mem.wr32(NODE + o, 0);
        }
        for k in 0..0x20 {
            cpu.mem.wr32(TABLE + 0x4C + 4 * k, ENTRY);
        }
        for k in 0..5 {
            cpu.mem.wr32(MONSTER_TABLES + 4 * k, TABLE);
        }
        cpu.mem.wr8(ENTRY, 4);
        cpu.mem.wr32(ENTRY + 0x88, ENTRY + 0x80);
        cpu.mem.wr8(ENTRY + 0x81, 1 + rng.below(0x20) as u8);
        cpu.mem.wr16(NODE + 0x5A, seat as u16);
        cpu.mem.wr32(NODE + 0x4C, ENTRY);
        let victim = ACTOR + seat * 0x400;
        cpu.mem.wr16(victim + 0x14C, 0);
        cpu.mem.wr8(victim + 0x1DC, flags);
        cpu.mem.wr8(victim + 0x1D9, 4);
        cpu.mem.wr8(victim + 0x1DA, 4);
        cpu.mem.wr8(victim + 0x1DB, 4);
        cpu.mem.wr8(victim + 0x1F2, getup);

        let mut words = [[0u32; 2]; 3];
        let mut hp = [0u16; 3];
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
            cpu.mem.wr32(c + 0x9C, defeated[i]);
            cpu.mem.wr32(c + 0xF4, words[i][0]);
            cpu.mem.wr32(c + 0xF8, words[i][1]);
            cpu.mem.wr16(ACTOR + i as u32 * 0x400 + 0x14C, hp[i]);
        }

        state.set(seed);
        held.set(held_count);
        granted.borrow_mut().clear();
        let stop = cpu.call(COMMIT, &[NODE]);
        assert!(matches!(stop, Stop::Returned), "round {round}: {stop:?}");

        let context = format!(
            "round {round}: party {party} seat {seat} acting {acting} flags {flags:#x} \
             seru {seru} scripted {scripted} second {second:#x} id {id:#x}"
        );
        // The credit.
        let mut want = defeated.clone();
        if let Some(s) = bs::defeat_credit(acting as u8, flags)
            && (s as u32) < party
        {
            want[usize::from(s)] += 1;
            credited += 1;
        }
        let got: Vec<u32> = (0..3)
            .map(|i| cpu.mem.rd32(CHARS + i * CHAR_STRIDE + 0x9C))
            .collect();
        for i in 0..party as usize {
            assert_eq!(got[i], want[i], "credit, {context}");
        }
        // The cue.
        let due = bs::defeat_cue_due(flags, seru, scripted, second, seat as u8);
        let next = if cursor + 1 >= 4 { 0 } else { cursor + 1 };
        let retail_cue = (
            cpu.mem.rd8(CTX + 9),
            cpu.mem.rd16(CUE_RING + 2 * u32::from(cursor)),
        );
        let port_cue = if due { (next, 0x0D) } else { (cursor, 0) };
        assert_eq!(retail_cue, port_cue, "cue, {context}");
        if due {
            cued += 1;
        } else if flags & bs::DEATH_ARM_DONE == 0 && !seru {
            silenced += 1;
        }
        // The spoils.
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
        paid += i32::from(spoils.is_some());
        assert_eq!(
            granted.borrow().first().copied(),
            spoils.map(|s| s.item()),
            "spoils, {context}"
        );
        assert_eq!(state.get(), port_state.get(), "rand, {context}");
        assert_eq!(cpu.mem.rd8(CTX + 0x27) != 0, band.attempted, "{context}");
        // The tail: bit 3 raised, or the Seru-staged body stood on its get-up
        // with the idle-at-end flag alone (`World::finish_battle_reaction`).
        let tail = (cpu.mem.rd8(victim + 0x1DC), cpu.mem.rd8(victim + 0x1DA));
        if seru {
            assert_eq!(
                tail,
                (
                    legaia_engine_battle_loop::clip_ladder::ANIM_FLAG_IDLE_AT_END,
                    getup
                ),
                "tail, {context}"
            );
            stood += 1;
        } else {
            assert_ne!(tail.0 & bs::DEATH_ARM_DONE, 0, "tail, {context}");
        }
    }
    assert!(
        credited > 5000 && cued > 5000 && silenced > 1000 && paid > 1000 && stood > 3000,
        "{credited} {cued} {silenced} {paid} {stood}"
    );
}
