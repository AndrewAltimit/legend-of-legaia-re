//! The two flee rolls: `FUN_801E791C`, the party's Run, against
//! `World::roll_battle_escape`, and `FUN_801EC0DC`, a monster's, against
//! `World::monster_flee_roll`.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_core::world::World;
use legaia_save::Party;
use psx_retail_exec::{Cpu, Stop, V0};
use std::cell::Cell;
use std::rc::Rc;

const RAND: u32 = 0x8005_6798;
const OVERLAY_BASE: u32 = 0x801C_E818;
const CTX_PTR: u32 = 0x8007_BD24;
const ACTORS: u32 = 0x801C_9370;
const MODELS: u32 = 0x801C_9360;
const SEATS: u32 = 0x8007_BD10;
const CHAR_RECORDS: u32 = 0x8008_4708;
const CHAR_STRIDE: u32 = 0x414;
const SPECIAL_WORD: u32 = 0x8007_BAC0;
const CTX: u32 = 0x8012_0000;
const ACTOR: u32 = 0x8013_0000;
const ACTOR_STRIDE: u32 = 0x400;
const MODEL: u32 = 0x8015_0000;

#[derive(Debug, Clone)]
struct Fight {
    party: u8,
    monsters: u8,
    spd: [u16; 8],
    hp: [u16; 8],
    max_hp: [u16; 8],
    atk: [u16; 8],
    int: [u16; 8],
    word1: [u32; 3],
    no_escape: bool,
    forced: bool,
    preemptive: bool,
    seed: u32,
}

fn gen_fight(rng: &mut Rng) -> Fight {
    let top = [40, 300, 999, 9999][rng.below(4) as usize];
    let mut f = Fight {
        party: 1 + rng.below(3) as u8,
        monsters: 1 + rng.below(5) as u8,
        spd: [0; 8],
        hp: [0; 8],
        max_hp: [0; 8],
        atk: [0; 8],
        int: [0; 8],
        word1: [0; 3],
        no_escape: rng.below(8) == 0,
        forced: rng.below(16) == 0,
        preemptive: rng.below(8) == 0,
        seed: rng.next(),
    };
    for s in 0..8 {
        f.spd[s] = 1 + rng.below(top) as u16;
        f.max_hp[s] = 1 + rng.below(9999) as u16;
        f.hp[s] = match rng.below(5) {
            0 => 0,
            1 => f.max_hp[s],
            _ => rng.below(u32::from(f.max_hp[s]) + 1) as u16,
        };
        f.atk[s] = rng.below(top + 1) as u16;
        f.int[s] = rng.below(top + 1) as u16;
    }
    for w in &mut f.word1 {
        *w = [0, 0, 0x10_0000, 0x40_0000, 0x80_0000][rng.below(5) as usize];
    }
    f
}

fn seed_retail(cpu: &mut Cpu, f: &Fight) {
    cpu.mem.wr32(CTX_PTR, CTX);
    for b in 0..0x400 {
        cpu.mem.wr8(CTX + b, 0);
    }
    cpu.mem.wr8(CTX, f.party);
    cpu.mem.wr8(CTX + 1, f.monsters);
    cpu.mem.wr8(CTX + 0x287, u8::from(f.no_escape));
    cpu.mem.wr8(CTX + 0x291, if f.preemptive { 2 } else { 0 });
    cpu.mem.wr32(SPECIAL_WORD, if f.forced { 0x100 } else { 0 });
    for m in 0..3u32 {
        cpu.mem.wr8(SEATS + m, m as u8 + 1);
        cpu.mem
            .wr32(CHAR_RECORDS + m * CHAR_STRIDE + 0xF8, f.word1[m as usize]);
    }
    for s in 0..8u32 {
        let a = ACTOR + s * ACTOR_STRIDE;
        cpu.mem.wr32(ACTORS + s * 4, a);
        for b in 0..ACTOR_STRIDE {
            cpu.mem.wr8(a + b, 0);
        }
        let i = s as usize;
        cpu.mem.wr16(a + 0x164, f.spd[i]);
        cpu.mem.wr16(a + 0x14C, f.hp[i]);
        cpu.mem.wr16(a + 0x14E, f.max_hp[i]);
        cpu.mem.wr16(a + 0x158, f.atk[i]);
        cpu.mem.wr16(a + 0x168, f.int[i]);
    }
    for m in 0..4u32 {
        cpu.mem.wr32(MODELS + m * 4, MODEL);
    }
    cpu.mem.wr32(MODEL + 4, MODEL + 0x100);
}

fn seed_port(w: &mut World, f: &Fight) {
    let mut party = Party::zeroed(usize::from(f.party));
    for m in 0..usize::from(f.party) {
        let mut bytes = [0u8; legaia_save::ABILITY_BITS_LEN];
        bytes[4..8].copy_from_slice(&f.word1[m].to_le_bytes());
        party.members[m].set_ability_bits(bytes);
    }
    w.load_party(party);
    w.enter_battle(f.party, f.monsters);
    w.party.party_count = f.party;
    w.battle.no_escape = f.no_escape;
    w.battle.special_word = if f.forced { 0x100 } else { 0 };
    w.battle_ctx.formation_latched = if f.preemptive { 2 } else { 0 };
    let seat = |s: u8| {
        if s < f.party {
            usize::from(s)
        } else {
            usize::from(s - f.party) + 3
        }
    };
    for s in 0..f.party + f.monsters {
        let i = usize::from(s);
        let r = seat(s);
        w.battle.speed[i] = f.spd[r];
        w.battle.attack[i] = f.atk[r];
        w.battle.accuracy[i] = f.int[r];
        let a = &mut w.actors[i];
        a.battle.spd = f.spd[r];
        a.battle.hp = f.hp[r];
        a.battle.max_hp = f.max_hp[r];
        a.battle.liveness = u16::from(f.hp[r] != 0);
        a.battle_monster_id = (s >= f.party).then_some(0x40);
    }
    w.rng_state = f.seed;
}

fn cpu(state: &Rc<Cell<u32>>) -> Option<Cpu> {
    let mut cpu = scus_cpu()?;
    cpu.mem.load(OVERLAY_BASE, &disc()?.read_entry(898).ok()?);
    let stream = state.clone();
    cpu.hook(RAND, move |r, _| {
        let next = stream.get().wrapping_mul(0x41C6_4E6D).wrapping_add(0x3039);
        stream.set(next);
        r.r[V0] = (next >> 16) & 0x7FFF;
    });
    cpu.stub(0x801D_829C, 0); // The escape camera.
    Some(cpu)
}

#[test]
fn the_party_gets_away_when_retail_does() {
    differential!(prot(898, 0x801E_791C));
    let state = Rc::new(Cell::new(0));
    let Some(mut cpu) = cpu(&state) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut w = World::new();
    let mut rng = Rng(0x4553_4341);
    let (mut away, mut failures) = (0, Vec::new());
    for round in 0..20000 {
        let f = gen_fight(&mut rng);
        seed_retail(&mut cpu, &f);
        state.set(f.seed);
        assert_eq!(cpu.call(0x801E_791C, &[]), Stop::Returned, "{f:?}");
        let retail = (cpu.regs.r[V0] != 0, state.get());
        seed_port(&mut w, &f);
        let port = (w.roll_battle_escape(), w.rng_state);
        away += u32::from(retail.0);
        if retail != port && failures.len() < 6 {
            failures.push(format!(
                "round {round}: retail {retail:?} port {port:?}\n {f:?}"
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert!(away > 1000 && away < 19000, "{away} got away");
}

#[test]
fn a_monster_flees_when_retail_does() {
    differential!(prot(898, 0x801E_C0DC));
    let state = Rc::new(Cell::new(0));
    let Some(mut cpu) = cpu(&state) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut w = World::new();
    let mut rng = Rng(0x464C_4545);
    let (mut fled, mut failures) = (0, Vec::new());
    for round in 0..40000 {
        let f = gen_fight(&mut rng);
        seed_retail(&mut cpu, &f);
        let m = rng.below(u32::from(f.monsters)) as u8;
        state.set(f.seed);
        match cpu.call(0x801E_C0DC, &[3 + u32::from(m)]) {
            Stop::Returned => {}
            // A party with no living member and an INT of nothing: `break`,
            // and the picker never asks with the party gone.
            Stop::DivideByZero { .. } => continue,
            other => panic!("{other:?}: {f:?}"),
        }
        let retail = (cpu.regs.r[V0] != 0, state.get());
        seed_port(&mut w, &f);
        let port = (w.monster_flee_roll(f.party + m), w.rng_state);
        fled += u32::from(retail.0);
        if retail != port && failures.len() < 6 {
            failures.push(format!(
                "round {round}: m{m} retail {retail:?} port {port:?}\n {f:?}"
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert!(fled > 0, "no monster ever fled");
}
