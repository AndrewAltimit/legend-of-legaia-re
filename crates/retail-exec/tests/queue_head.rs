//! The head of the queue builder `FUN_801EED1C`: the AI companion's own arm
//! against `World::arm_ai_companion`, the HP it pins on her seat, and the string
//! the spar's third lesson forces on the lead.

use crate::common::{Rng, disc, scus_cpu, split_statuses};
use legaia_engine_battle::monster_catalog::MonsterDef;
use legaia_engine_core::world::World;
use legaia_save::Party;
use psx_retail_exec::{Cpu, Stop, V0};
use std::cell::Cell;
use std::rc::Rc;

const BUILDER: u32 = 0x801E_ED1C;
const RAND: u32 = 0x8005_6798;
const OVERLAY_BASE: u32 = 0x801C_E818;
const CTX_PTR: u32 = 0x8007_BD24;
const ACTORS: u32 = 0x801C_9370;
const MONSTER_RECORDS: u32 = 0x801C_9348;
const SEATS: u32 = 0x8007_BD10;

const CTX: u32 = 0x8012_0000;
const ACTOR: u32 = 0x8013_0000;
const ACTOR_STRIDE: u32 = 0x400;
const MONSTER: u32 = 0x8015_0000;

fn rig(state: &Rc<Cell<u32>>) -> Option<Cpu> {
    let (mut cpu, disc) = (scus_cpu()?, disc()?);
    cpu.mem
        .load(OVERLAY_BASE, &disc.read_entry(898).expect("PROT 0898"));
    let stream = state.clone();
    cpu.hook(RAND, move |r, _| {
        let next = stream.get().wrapping_mul(0x41C6_4E6D).wrapping_add(0x3039);
        stream.set(next);
        r.r[V0] = (next >> 16) & 0x7FFF;
    });
    // The Super applier the tail reaches, compared in `arts_queue`.
    cpu.stub(0x801E_F9E4, 0);
    Some(cpu)
}

fn actor(s: u32) -> u32 {
    ACTOR + s * ACTOR_STRIDE
}

#[derive(Debug, Clone)]
struct Turn {
    lead_hp: u16,
    lead_max: u16,
    lead_status: u16,
    terra_hp: u16,
    terra_mp: u16,
    monsters: u8,
    alive: [bool; 3],
    classes: [u8; 3],
    seed: u32,
}

fn gen_turn(rng: &mut Rng) -> Turn {
    let lead_max = 1 + rng.below(999) as u16;
    let mut alive = [rng.below(2) == 0, rng.below(2) == 0, rng.below(2) == 0];
    let monsters = 1 + rng.below(3) as u8;
    alive[rng.below(u32::from(monsters)) as usize] = true;
    Turn {
        lead_hp: match rng.below(6) {
            0 => 0,
            1 => lead_max / 2,
            2 => lead_max / 2 + 1,
            3 => 1 + rng.below(u32::from(lead_max)) as u16,
            _ => lead_max,
        },
        lead_max,
        lead_status: if rng.below(4) == 0 {
            [1, 2, 8, 0x80, 0x400, 0x1000][rng.below(6) as usize]
        } else {
            0
        },
        terra_hp: 1 + rng.below(200) as u16,
        terra_mp: rng.below(200) as u16,
        monsters,
        alive,
        classes: [rng.below(4) as u8, rng.below(4) as u8, rng.below(4) as u8],
        seed: rng.next(),
    }
}

#[test]
fn the_companion_picks_what_retail_picks() {
    let state = Rc::new(Cell::new(0));
    let Some(mut cpu) = rig(&state) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut w = World::new();
    let mut rng = Rng(0x5445_5252);
    let (mut magic, mut strikes, mut idle) = (0, 0, 0);
    let mut failures = Vec::new();
    for round in 0..20000 {
        let t = gen_turn(&mut rng);
        // Retail.
        cpu.mem.wr32(CTX_PTR, CTX);
        for b in 0..0x400 {
            cpu.mem.wr8(CTX + b, 0);
        }
        cpu.mem.wr8(CTX, 2);
        cpu.mem.wr8(CTX + 1, t.monsters);
        cpu.mem.wr8(SEATS, 2);
        cpu.mem.wr8(SEATS + 1, 4);
        for s in 0..8u32 {
            cpu.mem.wr32(ACTORS + s * 4, actor(s));
            for b in 0..ACTOR_STRIDE {
                cpu.mem.wr8(actor(s) + b, 0);
            }
        }
        cpu.mem.wr16(actor(0) + 0x14C, t.lead_hp);
        cpu.mem.wr16(actor(0) + 0x14E, t.lead_max);
        cpu.mem.wr16(actor(0) + 0x16E, t.lead_status);
        cpu.mem.wr16(actor(1) + 0x14C, t.terra_hp);
        cpu.mem.wr16(actor(1) + 0x150, t.terra_mp);
        for m in 0..3u32 {
            let rec = MONSTER + m * 0x80;
            cpu.mem.wr32(MONSTER_RECORDS + m * 4, rec);
            cpu.mem.wr8(rec + 0x1E, t.classes[m as usize]);
            cpu.mem
                .wr16(actor(3 + m) + 0x14C, u16::from(t.alive[m as usize]));
        }
        state.set(t.seed);
        assert_eq!(
            cpu.call(BUILDER, &[1]),
            Stop::Returned,
            "round {round}: {t:?}"
        );
        let a = actor(1);
        let retail_cat = cpu.mem.rd8(a + 0x1DE);
        let retail_target = cpu.mem.rd8(a + 0x1DD);
        let retail_stream = cpu.mem.read(a + 0x1DF, 9);
        let retail_rng = state.get();
        let retail_pin = (cpu.mem.rd16(a + 0x14C), cpu.mem.rd16(a + 0x150));

        // Port: the party of two, her record on seat 1, the monsters after.
        w.load_party(Party::zeroed(4));
        w.set_active_party(vec![1, 3]);
        w.enter_battle(2, t.monsters);
        w.party.party_count = 2;
        w.battle.status_effects = Default::default();
        for m in 0..3u8 {
            let id = 0x40 + u16::from(m);
            let mut def = MonsterDef::new(id, "m", 100, 10);
            def.swing_class = t.classes[usize::from(m)];
            w.tables.monster_catalog.insert(def);
        }
        for s in 0..2 + usize::from(t.monsters) {
            let a = &mut w.actors[s];
            a.active = true;
            a.battle.params = Default::default();
            a.battle.action_category = 0;
            a.battle.hp = 1;
            a.battle.max_hp = 1;
        }
        for m in 0..usize::from(t.monsters) {
            let a = &mut w.actors[2 + m];
            a.battle_monster_id = Some(0x40 + m as u16);
            a.battle.hp = u16::from(t.alive[m]);
        }
        w.actors[0].battle.hp = t.lead_hp;
        w.actors[0].battle.max_hp = t.lead_max;
        w.actors[0].battle.field_flags = t.lead_status;
        split_statuses(&mut w, t.seed);
        w.actors[1].battle.hp = t.terra_hp;
        w.actors[1].battle.mp = t.terra_mp;
        w.rng_state = t.seed;
        w.queue_builder_head();
        assert!(w.arm_ai_companion(1), "seat 1 is the companion");
        let b = &w.actors[1].battle;
        let port_stream: Vec<u8> = {
            let mut v = b.params[..9].to_vec();
            if b.action_category == 2 {
                v[8] = b.sub_route;
            }
            v
        };
        // A pick the port maps back: a monster on engine seat `2 + m`.
        let port_target = match b.action_category {
            0 => 0,
            3 => b.active_target + 1,
            _ => b.active_target,
        };
        let retail = (
            retail_cat,
            retail_target,
            retail_stream.clone(),
            retail_rng,
            retail_pin,
        );
        let port = (
            b.action_category,
            port_target,
            port_stream,
            w.rng_state,
            (b.hp, b.mp),
        );
        match retail_cat {
            2 => magic += 1,
            3 => strikes += 1,
            _ => idle += 1,
        }
        if retail != port && failures.len() < 6 {
            failures.push(format!(
                "round {round}:\n retail {retail:02x?}\n port   {port:02x?}\n {t:?}"
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert!(
        magic > 1000 && strikes > 1000 && idle > 1000,
        "{magic} {strikes} {idle}"
    );
}

/// The third round of Tetsu's spar, under the Automatic option: the builder
/// overwrites the lead's stream, whichever seat it was called for.
#[test]
fn the_spar_forces_the_lesson_string() {
    use legaia_engine_system::option_values::SelectAttackOpt;
    let state = Rc::new(Cell::new(0));
    let Some(mut cpu) = rig(&state) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut w = World::new();
    let mut forced = 0;
    for monster in [0x4Fu8, 0x40] {
        for round in 0..6u8 {
            for (word, option) in [
                (0u32, SelectAttackOpt::Select),
                (1, SelectAttackOpt::Automatic),
                (2, SelectAttackOpt::Command),
            ] {
                cpu.mem.wr32(CTX_PTR, CTX);
                for b in 0..0x400 {
                    cpu.mem.wr8(CTX + b, 0);
                }
                cpu.mem.wr8(CTX, 1);
                cpu.mem.wr8(CTX + 1, 1);
                cpu.mem.wr8(CTX + 0x28A, round);
                cpu.mem.wr8(0x8007_BD0C, monster);
                cpu.mem.wr32(0x8008_46C4, word);
                cpu.mem.wr8(SEATS, 1);
                cpu.mem.wr8(SEATS + 1, 0);
                for s in 0..8u32 {
                    cpu.mem.wr32(ACTORS + s * 4, actor(s));
                    for b in 0..ACTOR_STRIDE {
                        cpu.mem.wr8(actor(s) + b, 0);
                    }
                }
                assert_eq!(cpu.call(BUILDER, &[0]), Stop::Returned);
                let retail = cpu.mem.read(actor(0) + 0x1DF, 4) == [0x0E, 0x0F, 0x0E, 0x0F];

                w.load_party(Party::zeroed(1));
                w.enter_battle(1, 1);
                w.party.party_count = 1;
                w.actors[1].active = true;
                w.actors[1].battle_monster_id = Some(u16::from(monster));
                w.battle.monster_ai_state.mode_flags = round;
                w.toggles.select_attack = option;
                let port = w.spar_lesson_arrows(0).is_some();
                assert_eq!(retail, port, "{monster:#x} round {round} word {word}");
                forced += u32::from(retail);
            }
        }
    }
    assert_eq!(forced, 1);
}
