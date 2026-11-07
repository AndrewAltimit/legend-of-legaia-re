//! `FUN_801DD0AC`, the move and summon damage kernel, with its scale stage
//! `FUN_801DD864` and finisher `FUN_801DDB30`, against the two paths the engine
//! rolls it on: `World::enemy_move_predamage` (the arts / physical branch, a
//! monster special) and `World::player_summon_predamage` (the summon branch,
//! attacker slot 7).

use crate::common::{Rng, disc, scus, split_statuses};
use legaia_engine_battle::accessory_passives::AccessoryPassives;
use legaia_engine_battle::monster_catalog::MonsterDef;
use legaia_engine_battle_vm::battle_formulas::ROLL_ALIAS_KILLS;
use legaia_engine_core::world::World;
use legaia_game_tables::accessory_passive::AccessoryPassiveTable;
use legaia_game_tables::element_affinity;
use legaia_save::Party;
use psx_retail_exec::{Cpu, Stop, V0};
use std::cell::Cell;
use std::rc::Rc;

const KERNEL: u32 = 0x801D_D0AC;
const RAND: u32 = 0x8005_6798;
const MOVE_POWER: u32 = 0x801F_4F5C;
const CTX_PTR: u32 = 0x8007_BD24;
const ACTORS: u32 = 0x801C_9370;
const MONSTER_RECORDS: u32 = 0x801C_9348;
const SEATS: u32 = 0x8007_BD10;
const CHAR_RECORDS: u32 = 0x8008_4708;
const CHAR_STRIDE: u32 = 0x414;
const OVERLAY_BASE: u32 = 0x801C_E818;
const MYSTIC_WORD: u32 = 0x8007_BD84;
const CTX: u32 = 0x8012_0000;
const ACTOR: u32 = 0x8013_0000;
const ACTOR_STRIDE: u32 = 0x400;
const MONSTER: u32 = 0x8015_0000;

fn stat(rng: &mut Rng, top: u32) -> u16 {
    match rng.below(8) {
        0 => 0,
        1 => 1,
        2 => top as u16,
        _ => rng.below(top + 1) as u16,
    }
}

#[derive(Debug, Clone)]
struct Roll {
    attacker: u8,
    target: u8,
    power: i16,
    int: [u16; 8],
    atk: [u16; 8],
    hp: [u16; 8],
    max_hp: [u16; 8],
    udf: [u16; 8],
    ldf: [u16; 8],
    status: [u16; 8],
    spirit: [u16; 8],
    guarding: bool,
    mystic: bool,
    ability: [[u32; 2]; 3],
    element: u8,
    target_element: u8,
    level: u8,
    seed: u32,
}

fn gen_roll(rng: &mut Rng, attacker: u8, target: u8) -> Roll {
    let top = [60, 300, 999, 9999][rng.below(4) as usize];
    let mut r = Roll {
        attacker,
        target,
        power: match rng.below(6) {
            0 => 0,
            1 => 1,
            2 => 0x7FFF,
            _ => rng.below(1200) as i16,
        },
        int: [0; 8],
        atk: [0; 8],
        hp: [0; 8],
        max_hp: [0; 8],
        udf: [0; 8],
        ldf: [0; 8],
        status: [0; 8],
        spirit: [0; 8],
        guarding: target < 3 && rng.below(4) == 0,
        mystic: rng.below(8) == 0,
        ability: [[0; 2]; 3],
        element: rng.below(8) as u8,
        target_element: rng.below(8) as u8,
        level: 1 + rng.below(9) as u8,
        seed: rng.next(),
    };
    for s in 0..8 {
        r.int[s] = stat(rng, top);
        r.atk[s] = stat(rng, top);
        r.udf[s] = stat(rng, top);
        r.ldf[s] = stat(rng, top);
        r.max_hp[s] = 1 + rng.below(9999) as u16;
        r.hp[s] = 1 + rng.below(u32::from(r.max_hp[s])) as u16;
        r.spirit[s] = rng.below(101) as u16;
        r.status[s] = if rng.below(4) == 0 {
            [1, 2, 3, 0x80, 0x400, 0x1000][rng.below(6) as usize]
        } else {
            0
        };
    }
    for m in 0..3 {
        if rng.below(2) == 0 {
            let w0 = [0x2000_0000u32, 0x4000_0000, 0x8000_0000][rng.below(3) as usize];
            let w1 = [1u32, 2, 4, 8, 0x10, 0x100, 0x200][rng.below(7) as usize];
            r.ability[m] = [w0, w1];
        }
    }
    r
}

fn actor(slot: u8) -> u32 {
    ACTOR + u32::from(slot) * ACTOR_STRIDE
}

fn seed_retail(cpu: &mut Cpu, r: &Roll) {
    cpu.mem.wr32(CTX_PTR, CTX);
    for b in 0..0x900 {
        cpu.mem.wr8(CTX + b, 0);
    }
    cpu.mem.wr32(MYSTIC_WORD, u32::from(r.mystic));
    for m in 0..3u32 {
        cpu.mem.wr8(SEATS + m, m as u8 + 1);
        let rec = CHAR_RECORDS + m * CHAR_STRIDE;
        cpu.mem.wr32(rec + 0xF4, r.ability[m as usize][0]);
        cpu.mem.wr32(rec + 0xF8, r.ability[m as usize][1]);
    }
    for s in 0..8u8 {
        let a = actor(s);
        cpu.mem.wr32(ACTORS + u32::from(s) * 4, a);
        for b in 0..ACTOR_STRIDE {
            cpu.mem.wr8(a + b, 0);
        }
        let i = usize::from(s);
        cpu.mem.wr16(a + 0x14C, r.hp[i]);
        cpu.mem.wr16(a + 0x14E, r.max_hp[i]);
        cpu.mem.wr16(a + 0x15C, r.udf[i]);
        cpu.mem.wr16(a + 0x160, r.ldf[i]);
        cpu.mem.wr16(a + 0x158, r.atk[i]);
        cpu.mem.wr16(a + 0x168, r.int[i]);
        cpu.mem.wr16(a + 0x16E, r.status[i]);
        cpu.mem.wr16(a + 0x170, r.spirit[i]);
    }
    for m in 0..5u32 {
        let rec = MONSTER + m * 0x80;
        cpu.mem.wr32(MONSTER_RECORDS + m * 4, rec);
        cpu.mem.wr8(rec + 0x1D, r.target_element);
    }
    if r.attacker >= 3 {
        cpu.mem
            .wr8(MONSTER + u32::from(r.attacker - 3) * 0x80 + 0x1D, r.element);
    }
    if r.guarding {
        cpu.mem.wr8(actor(r.target) + 0x1DE, 4);
    }
    cpu.mem.wr16(MOVE_POWER, r.power as u16);
}

fn seed_port(w: &mut World, r: &Roll) {
    let mut party = Party::zeroed(3);
    for m in 0..3 {
        let mut bytes = [0u8; legaia_save::ABILITY_BITS_LEN];
        bytes[0..4].copy_from_slice(&r.ability[m][0].to_le_bytes());
        bytes[4..8].copy_from_slice(&r.ability[m][1].to_le_bytes());
        party.members[m].set_ability_bits(bytes);
    }
    w.load_party(party);
    w.enter_battle(3, 2);
    w.party.party_count = 3;
    w.battle.status_effects = Default::default();
    w.battle.guarding = [false; 3];
    w.battle.monster_ai_state.flag_bd84 = u8::from(r.mystic);
    for s in 0..5u8 {
        let i = usize::from(s);
        w.battle.accuracy[i] = r.int[i];
        w.battle.attack[i] = r.atk[i];
        w.battle.defense_split[i] = Some((r.udf[i], r.ldf[i]));
        let a = &mut w.actors[i];
        a.battle.hp = r.hp[i];
        a.battle.max_hp = r.max_hp[i];
        a.battle.field_flags = r.status[i];
        a.battle.spirit_gauge = r.spirit[i];
        a.battle_monster_id = (s >= 3).then_some(0x40 + u16::from(s));
        a.battle_element = (s >= 3).then_some(if s == r.attacker {
            r.element
        } else {
            r.target_element
        });
    }
    split_statuses(w, r.seed);
    if r.guarding {
        w.battle.guarding[usize::from(r.target)] = true;
    }
    w.rng_state = r.seed;
}

fn retail_cpu(state: &Rc<Cell<u32>>) -> Option<(Cpu, Vec<u8>)> {
    let mut cpu = crate::common::scus_cpu()?;
    let overlay = disc()?.read_entry(898).ok()?;
    cpu.mem.load(OVERLAY_BASE, &overlay);
    let stream = state.clone();
    cpu.hook(RAND, move |r, _| {
        let next = stream.get().wrapping_mul(0x41C6_4E6D).wrapping_add(0x3039);
        stream.set(next);
        r.r[V0] = (next >> 16) & 0x7FFF;
    });
    Some((cpu, overlay))
}

fn port_world(overlay: &[u8]) -> World {
    let scus = scus().unwrap();
    let mut w = World::new();
    w.tables.element_affinity = element_affinity::parse(overlay);
    w.set_accessory_passives(AccessoryPassives::from_disc(
        &AccessoryPassiveTable::from_scus(&scus).expect("passives"),
    ));
    w
}

#[test]
fn a_monster_special_rolls_what_retail_rolls() {
    let state = Rc::new(Cell::new(0u32));
    let Some((mut cpu, overlay)) = retail_cpu(&state) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut w = port_world(&overlay);
    let mut rng = Rng(0x4152_5453);
    let mut failures = Vec::new();
    let mut negative = 0;
    for round in 0..20000 {
        let r = gen_roll(&mut rng, 3 + rng_pick(round), (round % 3) as u8);
        seed_retail(&mut cpu, &r);
        state.set(r.seed);
        cpu.regs.r[V0] = 0;
        let stop = cpu.call(KERNEL, &[0, u32::from(r.attacker), u32::from(r.target)]);
        assert_eq!(stop, Stop::Returned, "round {round}: {r:?}");
        // The caller's unsigned clamp: a negative net takes the victim's HP.
        let net = match cpu.regs.r[V0] as i32 {
            n if n < 0 => i32::from(ROLL_ALIAS_KILLS),
            n => n,
        };
        negative += i32::from(net == i32::from(ROLL_ALIAS_KILLS));
        let t = actor(r.target);
        let retail = (
            net,
            cpu.mem.rd16(t + 0x170),
            cpu.mem.rd16(t + 0x16E) & 0xF000,
            state.get(),
        );

        seed_port(&mut w, &r);
        let dmg = w
            .enemy_move_predamage(r.attacker, r.target, i32::from(r.power), 0)
            .unwrap();
        w.accrue_spirit_gauge(r.target, dmg);
        let port = (
            i32::from(dmg),
            w.actors[usize::from(r.target)].battle.spirit_gauge,
            w.raw_status_word(r.target) & 0xF000,
            w.rng_state,
        );
        if retail != port {
            failures.push(format!(
                "round {round}:\n retail {retail:?}\n port   {port:?}\n {r:?}"
            ));
            if failures.len() >= 6 {
                break;
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert!(negative > 0, "the alias arm never ran");
}

/// The two wrappers the capture modules call instead: `FUN_801DD4B0` respects
/// the party's resist ladder, `FUN_801DD6B4` skips it.
#[test]
fn the_capture_wrappers_roll_what_retail_rolls() {
    const WRAPPERS: [u32; 2] = [0x801D_D4B0, 0x801D_D6B4];
    let state = Rc::new(Cell::new(0u32));
    let Some((mut cpu, overlay)) = retail_cpu(&state) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut w = port_world(&overlay);
    let mut rng = Rng(0x5752_4150);
    let mut failures = Vec::new();
    for round in 0..40000u32 {
        let which = (round & 1) as usize;
        let r = gen_roll(&mut rng, 3 + rng_pick(round), ((round >> 1) % 3) as u8);
        seed_retail(&mut cpu, &r);
        let power = r.power as u16 as u32;
        state.set(r.seed);
        cpu.regs.r[V0] = 0;
        let stop = cpu.call(
            WRAPPERS[which],
            &[power, u32::from(r.attacker), u32::from(r.target)],
        );
        assert_eq!(stop, Stop::Returned, "round {round}: {r:?}");
        let net = match cpu.regs.r[V0] as i32 {
            n if n < 0 => i32::from(ROLL_ALIAS_KILLS),
            n => n,
        };
        let retail = (net, state.get());
        seed_port(&mut w, &r);
        let dmg = if which == 0 {
            w.capture_respect_predamage(r.attacker, r.target, power as i32)
        } else {
            w.capture_bypass_predamage(r.attacker, r.target, power as i32)
        }
        .unwrap();
        let port = (i32::from(dmg), w.rng_state);
        if retail != port {
            failures.push(format!(
                "round {round} wrapper {which}:\n retail {retail:?}\n port   {port:?}\n {r:?}"
            ));
            if failures.len() >= 6 {
                break;
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

fn rng_pick(round: u32) -> u8 {
    ((round >> 2) & 1) as u8
}

#[test]
fn a_summon_rolls_what_retail_rolls() {
    const SPELL: u8 = 0x81;
    let state = Rc::new(Cell::new(0u32));
    let Some((mut cpu, overlay)) = retail_cpu(&state) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut w = port_world(&overlay);
    let mut rng = Rng(0x5355_4D4E);
    let mut failures = Vec::new();
    for round in 0..20000u32 {
        let caster = (round % 3) as u8;
        let target = 3 + rng_pick(round);
        let mut r = gen_roll(&mut rng, 7, target);
        r.status[7] = 0;
        seed_retail(&mut cpu, &r);
        // The body in slot 7 and its streamed record.
        let body_hp = r.hp[7];
        let body_int = r.int[7];
        cpu.mem.wr8(MONSTER + 4 * 0x80 + 0x1D, r.element);
        cpu.mem.wr8(CTX + 0x13, caster);
        cpu.mem.wr8(actor(caster) + 0x1DF, SPELL);
        // The caster's list: the spell in slot 0 at its level.
        let rec = CHAR_RECORDS + u32::from(caster) * CHAR_STRIDE;
        cpu.mem.wr8(rec + 0x13D, SPELL);
        cpu.mem.wr8(rec + 0x161, r.level);
        cpu.mem.wr32(rec + 8, 0);
        cpu.mem
            .wr8(actor(7) + 0x1DD, if round % 2 == 0 { 3 } else { 9 });
        state.set(r.seed);
        cpu.regs.r[V0] = 0;
        let stop = cpu.call(KERNEL, &[0x12, 7, u32::from(target)]);
        assert_eq!(stop, Stop::Returned, "round {round}: {r:?}");
        let net = cpu.regs.r[V0] as i32;
        let t = actor(target);
        let retail = (net, cpu.mem.rd16(t + 0x170), state.get());

        seed_port(&mut w, &r);
        let mut list = w.party.roster.members[usize::from(caster)].spell_list();
        list.ids[0] = SPELL;
        list.levels[0] = r.level;
        w.party.roster.members[usize::from(caster)].set_spell_list(list);
        let mut def = MonsterDef::new(0x90, "body", body_hp, 10);
        def.hp = body_hp;
        def.intel = body_int;
        def.element = r.element;
        w.tables.summon_creatures.insert(SPELL, def);
        let dmg = w.player_summon_predamage(caster, target, SPELL).unwrap();
        w.accrue_spirit_gauge(target, dmg);
        let port = (
            i32::from(dmg),
            w.actors[usize::from(target)].battle.spirit_gauge,
            w.rng_state,
        );
        if retail != port {
            failures.push(format!(
                "round {round}:\n retail {retail:?}\n port   {port:?}\n {r:?}"
            ));
            if failures.len() >= 6 {
                break;
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
