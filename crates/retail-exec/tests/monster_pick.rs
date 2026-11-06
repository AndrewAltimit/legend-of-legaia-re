//! `FUN_801E9FD4`, the monster action picker, against
//! `World::pick_monster_action`: the generic core's magic and physical arms, the
//! swing budget, the flee checkpoint, the per-monster-id switch and the
//! recent-target ring, all on one `rand()` stream.

use crate::common::{Rng, disc, scus, scus_cpu, seat_status};
use legaia_engine_battle::monster_catalog::MonsterDef;
use legaia_engine_core::world::{MonsterAction, World};
use legaia_engine_menus::pause_screens::MenuTextTables;
use legaia_game_tables::spell_names::SPELL_COUNT;
use legaia_retail_exec::{Cpu, Stop, V0};
use legaia_save::Party;
use std::cell::Cell;
use std::rc::Rc;

const PICKER: u32 = 0x801E_9FD4;
const RAND: u32 = 0x8005_6798;
const OVERLAY_BASE: u32 = 0x801C_E818;
const CTX: u32 = 0x8012_0000;
const CTX_PTR: u32 = 0x8007_BD24;
const ACTORS: u32 = 0x801C_9370;
const MONSTER_RECORDS: u32 = 0x801C_9348;
const SEATS: u32 = 0x8007_BD10;
const MONSTER_IDS: u32 = 0x8007_BD0C;
const SPECIAL_WORD: u32 = 0x8007_BAC0;
const ACTOR: u32 = 0x8013_0000;
const ACTOR_STRIDE: u32 = 0x400;
const RECORD: u32 = 0x8015_0000;
const ENTRY: u32 = 0x8016_0000;

#[derive(Debug, Clone)]
struct Monster {
    id: u8,
    hp: u16,
    max_hp: u16,
    mp: u16,
    atk: u16,
    int: u16,
    gauge: u16,
    ai: [u8; 8],
    tags: Vec<u8>,
    costs: Vec<u8>,
    spirit: u16,
    /// The monster's own `+0x16E`: Numb (`0x400`) empties its pick at the tail.
    status: u16,
}

#[derive(Debug, Clone)]
struct Pick {
    party: u8,
    party_hp: [u16; 3],
    party_mp: [u16; 3],
    party_keys: [u16; 3],
    party_status: [u16; 3],
    monsters: Vec<Monster>,
    caster: u8,
    round: u8,
    no_flee: bool,
    seed: u32,
}

fn gen_monster(rng: &mut Rng) -> Monster {
    let mut ai = [0u8; 8];
    ai[0] = rng.below(5) as u8;
    for b in ai[1..4].iter_mut() {
        if rng.below(3) != 0 {
            *b = 1 + rng.below(SPELL_COUNT as u32 - 1) as u8;
        }
    }
    let n = 2 + rng.below(6) as usize;
    let mut tags = vec![0u8; n];
    let mut costs = vec![0xFFu8; n];
    for i in 1..n {
        tags[i] = match rng.below(4) {
            0 => 0x23,
            1 => [2, 3, 4, 5, 0x0B][rng.below(5) as usize],
            _ => 0x0C + rng.below(0x14) as u8,
        };
        if (0x0C..0x20).contains(&tags[i]) {
            costs[i] = [1, 4, 8, 16, 40, 0xFF][rng.below(6) as usize];
        }
    }
    let max_hp = 1 + rng.below(9999) as u16;
    Monster {
        id: match rng.below(3) {
            0 => [
                0x4C, 0x4F, 0x53, 0x54, 0x55, 0x88, 0x89, 0x8A, 0xA2, 0xA3, 0xA4, 0xA6, 0xA8, 0xA9,
                0xB3, 0xB4, 0xB5, 0xB6,
            ][rng.below(18) as usize],
            _ => 1 + rng.below(0xC0) as u8,
        },
        hp: match rng.below(3) {
            0 => max_hp,
            1 => 1 + (u32::from(max_hp) / 3) as u16,
            _ => 1 + rng.below(u32::from(max_hp)) as u16,
        },
        max_hp,
        mp: [0, 5, 40, 999][rng.below(4) as usize],
        atk: rng.below(500) as u16,
        int: rng.below(500) as u16,
        gauge: [0, 1, 8, 50, 288][rng.below(5) as usize],
        ai,
        tags,
        costs,
        spirit: rng.below(101) as u16,
        status: [0, 0, 0, 0x400, 0x80, 0x380, 0x1381][rng.below(7) as usize],
    }
}

fn gen_pick(rng: &mut Rng) -> Pick {
    let count = 1 + rng.below(4) as usize;
    let monsters: Vec<Monster> = (0..count).map(|_| gen_monster(rng)).collect();
    Pick {
        party: 1 + rng.below(3) as u8,
        party_hp: [
            1 + rng.below(999) as u16,
            rng.below(999) as u16,
            rng.below(999) as u16,
        ],
        party_mp: [
            rng.below(200) as u16,
            rng.below(200) as u16,
            rng.below(200) as u16,
        ],
        party_keys: [
            rng.below(2) as u16 * 40,
            rng.below(2) as u16 * 40,
            rng.below(2) as u16 * 40,
        ],
        party_status: [
            [0, 0, 4, 0x400][rng.below(4) as usize],
            [0, 0, 4, 0x400][rng.below(4) as usize],
            [0, 0, 4, 0x400][rng.below(4) as usize],
        ],
        caster: rng.below(count as u32) as u8,
        monsters,
        round: rng.below(6) as u8,
        no_flee: rng.below(2) == 0,
        seed: rng.next(),
    }
}

fn seed_retail(cpu: &mut Cpu, p: &Pick) {
    cpu.mem.wr32(CTX_PTR, CTX);
    for b in 0..0x900 {
        cpu.mem.wr8(CTX + b, 0);
    }
    cpu.mem.wr8(CTX, p.party);
    cpu.mem.wr8(CTX + 1, p.monsters.len() as u8);
    cpu.mem.wr8(CTX + 0x28A, p.round);
    cpu.mem.wr8(CTX + 0x287, u8::from(p.no_flee));
    cpu.mem.wr32(SPECIAL_WORD, 0);
    // The AI's memory across turns: per-seat latches and the recent-target ring.
    for b in 0..0x60 {
        cpu.mem.wr8(0x801C_8FE0 + b, 0);
    }
    for b in 0..4 {
        cpu.mem.wr8(0x8007_BDBC + b, 0xFF);
    }
    cpu.mem.wr32(MONSTER_IDS, 0);
    cpu.mem.wr32(SEATS, 0);
    for s in 0..8u32 {
        let a = ACTOR + s * ACTOR_STRIDE;
        cpu.mem.wr32(ACTORS + s * 4, a);
        for b in 0..ACTOR_STRIDE {
            cpu.mem.wr8(a + b, 0);
        }
    }
    for m in 0..u32::from(p.party) {
        cpu.mem.wr8(SEATS + m, m as u8 + 1);
        let a = ACTOR + m * ACTOR_STRIDE;
        cpu.mem.wr16(a + 0x14C, p.party_hp[m as usize]);
        cpu.mem.wr16(a + 0x14E, 999);
        cpu.mem.wr16(a + 0x150, p.party_mp[m as usize]);
        cpu.mem.wr16(a + 0x16C, p.party_keys[m as usize]);
        cpu.mem.wr16(a + 0x16E, p.party_status[m as usize]);
    }
    let mut entry = ENTRY;
    for (i, m) in p.monsters.iter().enumerate() {
        let i = i as u32;
        cpu.mem.wr8(MONSTER_IDS + i, m.id);
        let a = ACTOR + (3 + i) * ACTOR_STRIDE;
        cpu.mem.wr16(a + 0x14C, m.hp);
        cpu.mem.wr16(a + 0x14E, m.max_hp);
        cpu.mem.wr16(a + 0x150, m.mp);
        cpu.mem.wr16(a + 0x154, m.gauge);
        cpu.mem.wr16(a + 0x156, m.gauge);
        cpu.mem.wr16(a + 0x158, m.atk);
        cpu.mem.wr16(a + 0x168, m.int);
        cpu.mem.wr16(a + 0x170, m.spirit);
        cpu.mem.wr16(a + 0x16E, m.status);
        let r = RECORD + i * 0x400;
        for b in 0..0x400 {
            cpu.mem.wr8(r + b, 0);
        }
        cpu.mem.wr32(MONSTER_RECORDS + i * 4, r);
        cpu.mem.load(r + 0x20, &m.ai);
        cpu.mem.wr8(r + 0x4A, m.tags.len() as u8);
        for (k, t) in m.tags.iter().enumerate() {
            cpu.mem.wr32(r + 0x4C + k as u32 * 4, entry);
            cpu.mem.wr8(entry, *t);
            cpu.mem.wr8(entry + 0x74, m.costs[k]);
            entry += 0x80;
        }
    }
}

fn seed_port(w: &mut World, p: &Pick) {
    w.load_party(Party::zeroed(usize::from(p.party)));
    w.enter_battle(p.party, p.monsters.len() as u8);
    w.party.party_count = p.party;
    w.battle.status_effects = Default::default();
    w.battle.no_escape = p.no_flee;
    w.battle.special_word = 0;
    w.battle.monster_ai_state = Default::default();
    w.battle.monster_ai_state.mode_flags = p.round;
    for a in w.actors.iter_mut() {
        a.battle.hp = 0;
        a.battle.max_hp = 0;
        a.battle.mp = 0;
        a.battle.liveness = 0;
        a.battle.agl = 0;
        a.battle.agl_base = 0;
        a.battle.field_flags = 0;
        a.battle.action_category = 0;
        a.battle.active_target = 0;
        a.battle.params = Default::default();
        a.battle_monster_id = None;
    }
    for m in 0..usize::from(p.party) {
        let a = &mut w.actors[m];
        a.battle.hp = p.party_hp[m];
        a.battle.max_hp = 999;
        a.battle.mp = p.party_mp[m];
        a.battle.liveness = u16::from(p.party_hp[m] != 0);
        a.battle.init_key = p.party_keys[m];
        seat_status(w, m as u8, p.party_status[m], p.seed >> m & 1 != 0);
        w.battle.attack[m] = 0;
        w.battle.accuracy[m] = 0;
    }
    for (i, m) in p.monsters.iter().enumerate() {
        let id = 0x100 + u16::from(m.id) + 0x100 * i as u16;
        let mut def = MonsterDef::new(u16::from(m.id) | (id & 0xFF00), "m", m.max_hp, m.mp);
        def.ai_bytes = m.ai;
        def.entry_tags = m.tags.clone();
        def.agl = m.gauge;
        for (k, c) in m.costs.iter().enumerate() {
            if (0x0C..0x20).contains(&m.tags[k]) && *c != 0xFF {
                def.action_costs.push(*c);
                def.action_entries.push(k as u8);
            }
        }
        let seat = usize::from(p.party) + i;
        let key = def.id;
        w.tables.monster_catalog.insert(def);
        w.battle.attack[seat] = m.atk;
        w.battle.accuracy[seat] = m.int;
        let a = &mut w.actors[seat];
        a.battle.hp = m.hp;
        a.battle.max_hp = m.max_hp;
        a.battle.mp = m.mp;
        a.battle.agl = m.gauge;
        a.battle.agl_base = m.gauge.max(1);
        a.battle.spirit_gauge = m.spirit;
        a.battle.liveness = 1;
        a.battle_monster_id = Some(key);
        seat_status(w, seat as u8, m.status, p.seed >> (8 + i) & 1 != 0);
    }
    w.rng_state = p.seed;
}

#[test]
fn a_monster_picks_what_retail_picks() {
    let (Some(mut cpu), Some(scus), Some(disc)) = (scus_cpu(), scus(), disc()) else {
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
    cpu.stub(0x8003_EAE4, 0);
    let mut w = World::new();
    w.menu.text = Some(MenuTextTables::from_scus(&scus));
    let mut rng = Rng(0x5049_434B);
    let (mut failures, mut casts, mut swings) = (Vec::new(), 0, 0);
    for round in 0..8000 {
        let p = gen_pick(&mut rng);
        seed_retail(&mut cpu, &p);
        state.set(p.seed);
        let a = ACTOR + (3 + u32::from(p.caster)) * ACTOR_STRIDE;
        let stop = cpu.call(PICKER, &[u32::from(p.caster)]);
        match stop {
            Stop::Returned => {}
            // A record with no swing entry traps at `0x801EA2F8`; no shipped
            // monster is one.
            Stop::DivideByZero { .. } => continue,
            // The forced magic arm of `0xB4` retries for ever on a record whose
            // every match fails; its real record has a match.
            Stop::StepLimit { .. } if p.monsters[usize::from(p.caster)].id == 0xB4 => continue,
            other => panic!("round {round}: {other:?}: {p:?}"),
        }
        let retail_cat = cpu.mem.rd8(a + 0x1DE);
        let retail_spell = if retail_cat == 2 {
            cpu.mem.rd8(a + 0x1DF)
        } else {
            0
        };
        // The stored byte itself: a walk that matched nothing stores no
        // category, and the seat's stale `0` is what the turn picker's
        // redirect and Counterattack roll read next.
        let retail = (retail_cat, retail_spell, state.get());
        // `0xB4`'s arms store a spell and leave the category to the forced magic
        // arm, which a generated record with no cast entry never sets.
        if p.monsters[usize::from(p.caster)].id == 0xB4 && retail_cat != 2 {
            continue;
        }
        seed_port(&mut w, &p);
        let seat = p.party + p.caster;
        let action = w.pick_monster_action(seat);

        let cat = w.actors[usize::from(seat)].battle.action_category;
        let spell = match action {
            MonsterAction::Cast { spell_id, .. } if cat == 2 => spell_id,
            _ => 0,
        };
        casts += u32::from(cat == 2);
        swings += u32::from(cat == 3);
        let port = (cat, spell, w.rng_state);
        if retail != port {
            failures.push(format!(
                "round {round}:\n retail {retail:?}\n port   {port:?}\n {p:?}"
            ));
            if failures.len() >= 5 {
                break;
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert!(casts > 50 && swings > 50, "{casts} casts, {swings} swings");
}

/// The generator's spell ids stop at the table's end because no shipped record
/// names one past it: what retail reads there is whatever follows the table.
#[test]
fn no_shipped_monster_names_a_spell_past_the_table() {
    let Some(disc) = disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let entry = disc.read_entry(867).expect("PROT 0867");
    let records = legaia_actor_formats::monster_archive::records(&entry).expect("archive");
    assert!(!records.is_empty());
    for r in &records {
        for id in &r.ai_bytes[1..4] {
            assert!(usize::from(*id) < SPELL_COUNT, "{:#x}: {id:#x}", r.id);
        }
    }
}
