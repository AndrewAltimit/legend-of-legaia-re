//! The whole turn picker `FUN_801DABA4` - the dead-seat sweep, the initiative
//! tie pick, the side scans, the picked monster's AI `FUN_801E9FD4`, the
//! dead-target redirect `FUN_801DB124` and the Counterattack roll - against
//! `World::initiative_pick_and_decide`, on one `rand()` stream.

use crate::common::{Rng, disc, scus, scus_cpu};
use legaia_engine_battle::monster_catalog::MonsterDef;
use legaia_engine_battle_loop::round_flow::PendingPartyAction;
use legaia_engine_core::world::{MonsterAction, World};
use legaia_engine_menus::pause_screens::MenuTextTables;
use legaia_game_tables::spell_names::SPELL_COUNT;
use legaia_retail_exec::{Cpu, Stop, V0};
use legaia_save::Party;
use std::cell::Cell;
use std::rc::Rc;

const PICK: u32 = 0x801D_ABA4;
const RAND: u32 = 0x8005_6798;
const OVERLAY_BASE: u32 = 0x801C_E818;
const CTX: u32 = 0x8012_0000;
const CTX_PTR: u32 = 0x8007_BD24;
const ACTORS: u32 = 0x801C_9370;
const MONSTER_RECORDS: u32 = 0x801C_9348;
const SEATS: u32 = 0x8007_BD10;
const MONSTER_IDS: u32 = 0x8007_BD0C;
const SPECIAL_WORD: u32 = 0x8007_BAC0;
const COUNTER: u32 = 0x801F_6970;
const CHAR_RECORDS: u32 = 0x8008_4708;
const CHAR_STRIDE: u32 = 0x414;
const BAG: u32 = 0x8008_5958;
const WINDOW: [u32; 2] = [0x8007_B5EA, 0x8007_B5EC];
const ACTOR: u32 = 0x8013_0000;
const ACTOR_STRIDE: u32 = 0x400;
const RECORD: u32 = 0x8015_0000;
const ENTRY: u32 = 0x8016_0000;

/// The ailment words a seat starts the pick with: nothing, Stone, a
/// counter-blocking Confuse bit, and Numb.
const STATUS: [u16; 6] = [0, 0, 0, 4, 0x80, 0x400];

#[derive(Debug, Clone)]
struct Seat {
    hp: u16,
    key: u16,
    status: u16,
    spirit: u16,
    /// `+0x1DE` / `+0x1DF`: a committed item, refunded if the seat died.
    item: Option<u8>,
    counter: bool,
}

#[derive(Debug, Clone)]
struct Monster {
    id: u8,
    seat: Seat,
    max_hp: u16,
    mp: u16,
    gauge: u16,
    ai: [u8; 8],
    tags: Vec<u8>,
    costs: Vec<u8>,
}

#[derive(Debug, Clone)]
struct Turn {
    party: Vec<Seat>,
    monsters: Vec<Monster>,
    seed: u32,
}

fn key(rng: &mut Rng) -> u16 {
    match rng.below(8) {
        0..=2 => 0,
        3 => 0x8000 + rng.below(0x100) as u16,
        4 => 40,
        _ => 1 + rng.below(300) as u16,
    }
}

fn seat(rng: &mut Rng) -> Seat {
    Seat {
        hp: [0, 1, 50, 999][rng.below(4) as usize],
        key: key(rng),
        status: STATUS[rng.below(STATUS.len() as u32) as usize],
        spirit: rng.below(200) as u16,
        item: (rng.below(3) == 0).then(|| 1 + rng.below(0xFE) as u8),
        counter: rng.below(2) == 0,
    }
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
    let mut s = seat(rng);
    s.item = None;
    s.hp = [0, 1, 300, 999][rng.below(4) as usize];
    Monster {
        // Generic AI ids only; the per-id switch is `monster_pick`'s.
        id: 1 + rng.below(0x40) as u8,
        seat: s,
        max_hp: 999,
        mp: [0, 5, 40, 999][rng.below(4) as usize],
        gauge: [1, 8, 50, 288][rng.below(4) as usize],
        ai,
        tags,
        costs,
    }
}

fn gen_turn(rng: &mut Rng) -> Turn {
    let party = (0..1 + rng.below(3)).map(|_| seat(rng)).collect();
    let monsters = (0..1 + rng.below(4)).map(|_| gen_monster(rng)).collect();
    Turn {
        party,
        monsters,
        seed: rng.next(),
    }
}

/// The retail pool slot of engine seat `e`.
fn pool(t: &Turn, e: u8) -> u8 {
    let p = t.party.len() as u8;
    if e < p { e } else { 3 + e - p }
}

fn seed_retail(cpu: &mut Cpu, t: &Turn) {
    cpu.mem.wr32(CTX_PTR, CTX);
    for b in 0..0x900 {
        cpu.mem.wr8(CTX + b, 0);
    }
    cpu.mem.wr8(CTX, t.party.len() as u8);
    cpu.mem.wr8(CTX + 1, t.monsters.len() as u8);
    // The command-flow byte of the round start's pick: not `0xFF`, so a party
    // pick is not redirected here.
    cpu.mem.wr8(CTX + 6, 0x14);
    cpu.mem.wr32(SPECIAL_WORD, 0);
    cpu.mem.wr32(COUNTER, 0xDEAD);
    for b in 0..0x60 {
        cpu.mem.wr8(0x801C_8FE0 + b, 0);
    }
    for b in 0..4 {
        cpu.mem.wr8(0x8007_BDBC + b, 0xFF);
    }
    cpu.mem.wr32(MONSTER_IDS, 0);
    cpu.mem.wr32(SEATS, 0);
    for b in 0..0x200 {
        cpu.mem.wr8(BAG + b, 0);
    }
    cpu.mem.wr16(WINDOW[0], 0);
    cpu.mem.wr16(WINDOW[1], 0x100);
    for s in 0..8u32 {
        let a = ACTOR + s * ACTOR_STRIDE;
        cpu.mem.wr32(ACTORS + s * 4, a);
        for b in 0..ACTOR_STRIDE {
            cpu.mem.wr8(a + b, 0);
        }
    }
    let write_seat = |cpu: &mut Cpu, a: u32, s: &Seat| {
        cpu.mem.wr16(a + 0x14C, s.hp);
        cpu.mem.wr16(a + 0x16C, s.key);
        cpu.mem.wr16(a + 0x16E, s.status);
        cpu.mem.wr16(a + 0x170, s.spirit);
        if let Some(id) = s.item {
            cpu.mem.wr8(a + 0x1DE, 1);
            cpu.mem.wr8(a + 0x1DF, id);
        }
    };
    for (m, s) in t.party.iter().enumerate() {
        let m = m as u32;
        cpu.mem.wr8(SEATS + m, m as u8 + 1);
        let a = ACTOR + m * ACTOR_STRIDE;
        write_seat(cpu, a, s);
        cpu.mem.wr16(a + 0x14E, 999);
        let r = CHAR_RECORDS + m * CHAR_STRIDE;
        cpu.mem.wr32(r + 0xF4, if s.counter { 0x8000 } else { 0 });
    }
    let mut entry = ENTRY;
    for (i, m) in t.monsters.iter().enumerate() {
        let i = i as u32;
        cpu.mem.wr8(MONSTER_IDS + i, m.id);
        let a = ACTOR + (3 + i) * ACTOR_STRIDE;
        write_seat(cpu, a, &m.seat);
        cpu.mem.wr16(a + 0x14E, m.max_hp);
        cpu.mem.wr16(a + 0x150, m.mp);
        cpu.mem.wr16(a + 0x154, m.gauge);
        cpu.mem.wr16(a + 0x156, m.gauge);
        let r = RECORD + i * 0x400;
        for b in 0..0x400 {
            cpu.mem.wr8(r + b, 0);
        }
        cpu.mem.wr32(MONSTER_RECORDS + i * 4, r);
        cpu.mem.load(r + 0x20, &m.ai);
        cpu.mem.wr8(r + 0x4A, m.tags.len() as u8);
        for (k, tag) in m.tags.iter().enumerate() {
            cpu.mem.wr32(r + 0x4C + k as u32 * 4, entry);
            cpu.mem.wr8(entry, *tag);
            cpu.mem.wr8(entry + 0x74, m.costs[k]);
            entry += 0x80;
        }
    }
}

fn seed_port(w: &mut World, t: &Turn) {
    let pc = t.party.len() as u8;
    let mut party = Party::zeroed(t.party.len());
    for (m, s) in t.party.iter().enumerate() {
        let mut bits = [0u8; 16];
        if s.counter {
            bits[1] = 0x80;
        }
        party.members[m].set_ability_bits(bits);
    }
    w.load_party(party);
    w.enter_battle(pc, t.monsters.len() as u8);
    w.party.party_count = pc;
    w.party.inventory = Default::default();
    w.battle.status_effects = Default::default();
    w.battle.special_word = 0;
    w.battle.no_escape = false;
    w.battle.monster_ai_state = Default::default();
    w.battle_ctx.round_skip = 0;
    w.battle.round_flow = Default::default();
    for a in w.actors.iter_mut() {
        a.battle.hp = 0;
        a.battle.max_hp = 0;
        a.battle.mp = 0;
        a.battle.liveness = 0;
        a.battle.init_key = 0;
        a.battle.agl = 0;
        a.battle.agl_base = 0;
        a.battle.field_flags = 0;
        a.battle.action_category = 0;
        a.battle.active_target = 0;
        a.battle.spirit_gauge = 0;
        a.battle.params = Default::default();
        a.battle_monster_id = None;
    }
    // Every seat has SPD, as every shipped combatant does; the port's flat
    // walk for a battle with none is not retail's.
    for v in w.battle.speed.iter_mut() {
        *v = 1;
    }
    let put = |w: &mut World, e: usize, s: &Seat| {
        let a = &mut w.actors[e].battle;
        a.hp = s.hp;
        a.liveness = u16::from(s.hp != 0);
        a.init_key = s.key;
        a.field_flags = s.status;
        a.spirit_gauge = s.spirit;
        if let Some(id) = s.item {
            a.action_category = 1;
            a.params[0] = id;
        }
    };
    for (m, s) in t.party.iter().enumerate() {
        put(w, m, s);
        w.actors[m].battle.max_hp = 999;
        w.battle.attack[m] = 0;
        w.battle.accuracy[m] = 0;
        if let Some(id) = s.item {
            w.battle.round_flow.pending[m] = Some(PendingPartyAction::Item {
                item_id: id,
                used_slots: vec![m as u8],
            });
        }
    }
    for (i, m) in t.monsters.iter().enumerate() {
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
        let e = usize::from(pc) + i;
        let key = def.id;
        w.tables.monster_catalog.insert(def);
        w.battle.attack[e] = 0;
        w.battle.accuracy[e] = 0;
        put(w, e, &m.seat);
        let a = &mut w.actors[e];
        a.battle.max_hp = m.max_hp;
        a.battle.mp = m.mp;
        a.battle.agl = m.gauge;
        a.battle.agl_base = m.gauge;
        a.battle_monster_id = Some(key);
    }
    w.rng_state = t.seed;
}

fn steps(mut s: u32, end: u32) -> i32 {
    for n in 0..64 {
        if s == end {
            return n;
        }
        s = s.wrapping_mul(0x41C6_4E6D).wrapping_add(0x3039);
    }
    -1
}

#[derive(Debug, PartialEq, Eq)]
struct After {
    pick: Option<u8>,
    keys: Vec<u16>,
    spirit: Vec<u16>,
    skipped: u8,
    refunded: Vec<(u8, u8)>,
    /// The picked monster's category and its target pool slot.
    monster: Option<(u8, u8)>,
    counter: u8,
    rand: u32,
}

#[test]
fn the_turn_picker_picks_what_retail_picks() {
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
    // The voice cue the picker can raise, and the side-band slot pick a
    // wiped monster side reaches.
    cpu.stub(0x8003_EAE4, 0);
    cpu.stub(0x8005_5B4C, 0);
    let mut w = World::new();
    w.menu.text = Some(MenuTextTables::from_scus(&scus));
    let mut rng = Rng(0x5455_524E);
    let mut failures = Vec::new();
    let (mut monster_turns, mut counters, mut refunds, mut ends) = (0, 0, 0, 0);
    for round in 0..20000 {
        let t = gen_turn(&mut rng);
        seed_retail(&mut cpu, &t);
        state.set(t.seed);
        match cpu.call(PICK, &[]) {
            Stop::Returned => {}
            // A record with no swing entry traps in the picker; no shipped
            // monster is one.
            Stop::DivideByZero { .. } => continue,
            other => panic!("round {round}: {other:?}: {t:?}"),
        }
        let p = t.party.len() as u8;
        let seats: Vec<u8> = (0..p).chain(3..3 + t.monsters.len() as u8).collect();
        let actor = |s: u8| ACTOR + u32::from(s) * ACTOR_STRIDE;
        let rpick = cpu.mem.rd8(CTX + 0x274);
        let live = cpu.mem.rd16(actor(rpick) + 0x16C) != 0;
        let retail = {
            let mut keys: Vec<u16> = seats
                .iter()
                .map(|&s| cpu.mem.rd16(actor(s) + 0x16C))
                .collect();
            // The action SM spends the pick's key at its seed (`0x801E2CDC`);
            // the port spends it with the pick.
            if live && let Some(i) = seats.iter().position(|&s| s == rpick) {
                keys[i] = 0;
            }
            let monster = cpu.calls.contains(&0x801E_9FD4).then(|| {
                (
                    cpu.mem.rd8(actor(rpick) + 0x1DE),
                    cpu.mem.rd8(actor(rpick) + 0x1DD),
                )
            });
            After {
                pick: live.then_some(rpick),
                keys,
                spirit: seats
                    .iter()
                    .map(|&s| cpu.mem.rd16(actor(s) + 0x170))
                    .collect(),
                skipped: cpu.mem.rd8(CTX + 0x25),
                refunded: (0..0x100)
                    .map(|i| (cpu.mem.rd8(BAG + i * 2), cpu.mem.rd8(BAG + i * 2 + 1)))
                    .filter(|&(id, _)| id != 0)
                    .collect(),
                monster,
                counter: cpu.mem.rd32(COUNTER) as u8,
                rand: state.get(),
            }
        };
        seed_port(&mut w, &t);
        let picked = w.initiative_pick_and_decide();
        let port = {
            let monster = w.parked_monster_action().map(|(s, a)| {
                let target = |e: u8| pool(&t, e);
                let rt = retail.monster.map_or(0, |(_, rt)| rt);
                match a {
                    MonsterAction::Physical { target: e } => (3, target(*e)),
                    // A group byte (`7` / `8` / `9`) resolves to a list, a
                    // single member in a party of one; only a single target
                    // is a seat to compare.
                    MonsterAction::Cast { targets, .. } => match targets.as_slice() {
                        [one] if *one != *s && rt < 7 => (2, target(*one)),
                        _ => (2, rt),
                    },
                    MonsterAction::Flee => (5, rt),
                    // Nothing armed: the pick left the category it found.
                    MonsterAction::Wait => (w.actors[usize::from(*s)].battle.action_category, rt),
                }
            });
            After {
                pick: picked.map(|(e, _)| pool(&t, e)),
                keys: (0..seats.len())
                    .map(|e| w.actors[e].battle.init_key)
                    .collect(),
                spirit: (0..seats.len())
                    .map(|e| w.actors[e].battle.spirit_gauge)
                    .collect(),
                skipped: w.battle_ctx.round_skip,
                refunded: w
                    .party
                    .inventory
                    .slots()
                    .iter()
                    .copied()
                    .filter(|&(id, _)| id != 0)
                    .collect(),
                monster: monster.map(|(c, tgt)| {
                    // A walk that matched nothing stores no category; the
                    // engine plays the seat's stale `0` as a strike.
                    let rc = retail.monster.map_or(0, |(c, _)| c);
                    (if rc == 0 && c == 3 { 0 } else { c }, tgt)
                }),
                counter: match w.battle_ctx.counter_pending {
                    0 => 0,
                    c => pool(&t, c - 1) + 1,
                },
                rand: w.rng_state,
            }
        };
        monster_turns += u32::from(retail.monster.is_some());
        counters += u32::from(retail.counter != 0);
        refunds += u32::from(!retail.refunded.is_empty());
        ends += u32::from(retail.pick.is_none());
        if retail != port && failures.len() < 6 {
            failures.push(format!(
                "round {round}: calls {:x?} draws retail {} port {}\n retail {retail:?}\n port   {port:?}\n {t:?}",
                cpu.calls, steps(t.seed, retail.rand), steps(t.seed, port.rand)
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert!(
        monster_turns > 2000 && counters > 100 && refunds > 500 && ends > 500,
        "monster turns {monster_turns} counters {counters} refunds {refunds} ends {ends}"
    );
}
