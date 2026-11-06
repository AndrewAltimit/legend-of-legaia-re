//! What a round opens with: `FUN_801DA780`, the initiative keys, against
//! `World::reseed_initiative`; `FUN_80051D84`, the formation roll, against
//! `World::roll_battle_formation`; `FUN_801E752C`, the status tick, against
//! `World::tick_status_effects`.

use crate::common::{Rng, disc, scus, scus_cpu};
use legaia_engine_battle::accessory_passives::AccessoryPassives;
use legaia_engine_battle::monster_catalog::{FormationDef, FormationSlot};
use legaia_engine_battle_vm::battle_formulas::FormationAdvantage;
use legaia_engine_core::world::World;
use legaia_game_tables::accessory_passive::AccessoryPassiveTable;
use legaia_retail_exec::{Cpu, GP, Stop, V0};
use legaia_save::Party;
use std::cell::Cell;
use std::rc::Rc;

const RAND: u32 = 0x8005_6798;
const OVERLAY_BASE: u32 = 0x801C_E818;
const CTX: u32 = 0x8012_0000;
const CTX_PTR: u32 = 0x8007_BD24;
const ACTORS: u32 = 0x801C_9370;
const SEATS: u32 = 0x8007_BD10;
const MONSTER_IDS: u32 = 0x8007_BD0C;
const CHAR_RECORDS: u32 = 0x8008_4708;
const CHAR_STRIDE: u32 = 0x414;
const ACTOR: u32 = 0x8013_0000;
const ACTOR_STRIDE: u32 = 0x400;

#[derive(Debug, Clone)]
struct Seats {
    party: u8,
    monsters: u8,
    monster_id: u8,
    round: u8,
    spd: [u16; 7],
    hp: [u16; 7],
    max_hp: [u16; 7],
    status: [u16; 7],
    word0: [u32; 3],
    word1: [u32; 3],
    advantage: u8,
    scripted: bool,
    map: u32,
    seed: u32,
}

fn gen_seats(rng: &mut Rng) -> Seats {
    let top = [8, 60, 300, 999][rng.below(4) as usize];
    let mut s = Seats {
        party: 1 + rng.below(3) as u8,
        monsters: 1 + rng.below(4) as u8,
        monster_id: match rng.below(8) {
            0 => 0xB4,
            1 => 0x4F,
            2 => 0xA7,
            3 => 0x3E,
            _ => 1 + rng.below(0x40) as u8,
        },
        round: rng.below(6) as u8,
        spd: [0; 7],
        hp: [0; 7],
        max_hp: [0; 7],
        status: [0; 7],
        word0: [0; 3],
        word1: [0; 3],
        advantage: if rng.below(4) == 0 {
            1 + rng.below(2) as u8
        } else {
            0
        },
        scripted: rng.below(5) == 0,
        map: [0x0C, 0x15, 1, 7][rng.below(4) as usize],
        seed: rng.next(),
    };
    for i in 0..7 {
        s.spd[i] = 1 + rng.below(top) as u16;
        s.max_hp[i] = 1 + rng.below(9999) as u16;
        s.hp[i] = match rng.below(6) {
            0 => 0,
            1 => s.max_hp[i],
            2 => 1,
            _ => rng.below(u32::from(s.max_hp[i])) as u16,
        };
        s.status[i] = match rng.below(5) {
            0 => [1, 2, 3, 4, 5, 0x1000, 0x1001][rng.below(7) as usize],
            _ => 0,
        };
    }
    // Often level, which is where the formation spreads hit zero.
    if rng.below(3) == 0 {
        let v = s.spd[0];
        s.spd = [v; 7];
    }
    for m in 0..3 {
        s.word0[m] =
            [0, 0, 0, 0x8000, 0x4_0000, 0x2_0000, 0x4_8000, 0x6_0000][rng.below(8) as usize];
        s.word1[m] = [0, 0, 0x4_0000, 0x8_0000, 0xC_0000][rng.below(5) as usize];
    }
    s
}

fn seed_retail(cpu: &mut Cpu, s: &Seats) {
    cpu.mem.wr32(CTX_PTR, CTX);
    for b in 0..0x400 {
        cpu.mem.wr8(CTX + b, 0);
    }
    cpu.mem.wr8(CTX, s.party);
    cpu.mem.wr8(CTX + 1, s.monsters);
    cpu.mem.wr8(CTX + 0x28A, s.round);
    cpu.mem.wr8(CTX + 0x290, s.advantage);
    cpu.mem.wr8(CTX + 0x287, u8::from(s.scripted));
    cpu.mem.wr32(0x8007_BD0C, 0);
    cpu.mem.wr32(0x8007_BD10, 0);
    cpu.mem.wr8(0x8007_B64A, 0);
    cpu.mem.wr32(0x8008_4540, s.map);
    for i in 0..u32::from(s.monsters) {
        cpu.mem.wr8(MONSTER_IDS + i, s.monster_id);
    }
    for m in 0..3u32 {
        let rec = CHAR_RECORDS + m * CHAR_STRIDE;
        for b in 0..CHAR_STRIDE {
            cpu.mem.wr8(rec + b, 0);
        }
        cpu.mem.wr32(rec + 0xF4, s.word0[m as usize]);
        cpu.mem.wr32(rec + 0xF8, s.word1[m as usize]);
        if m < u32::from(s.party) {
            cpu.mem.wr8(SEATS + m, m as u8 + 1);
        }
    }
    for i in 0..7u32 {
        let a = ACTOR + i * ACTOR_STRIDE;
        cpu.mem.wr32(ACTORS + i * 4, a);
        for b in 0..ACTOR_STRIDE {
            cpu.mem.wr8(a + b, 0);
        }
        let seated = if i < 3 {
            i < u32::from(s.party)
        } else {
            i - 3 < u32::from(s.monsters)
        };
        if seated {
            let k = i as usize;
            cpu.mem.wr16(a + 0x164, s.spd[k]);
            cpu.mem.wr16(a + 0x14C, s.hp[k]);
            cpu.mem.wr16(a + 0x14E, s.max_hp[k]);
            cpu.mem.wr16(a + 0x172, s.hp[k]);
            cpu.mem.wr16(a + 0x16E, s.status[k]);
            if i >= 3 {
                cpu.mem.wr16(a + 0x16C, 0);
            }
        }
    }
}

fn engine_seat(s: &Seats, retail: u8) -> Option<u8> {
    if retail < 3 {
        (retail < s.party).then_some(retail)
    } else {
        (retail - 3 < s.monsters).then_some(s.party + retail - 3)
    }
}

fn seed_port(w: &mut World, s: &Seats) {
    let mut party = Party::zeroed(usize::from(s.party));
    for m in 0..usize::from(s.party) {
        let mut bytes = [0u8; legaia_save::ABILITY_BITS_LEN];
        bytes[0..4].copy_from_slice(&s.word0[m].to_le_bytes());
        bytes[4..8].copy_from_slice(&s.word1[m].to_le_bytes());
        party.members[m].set_ability_bits(bytes);
    }
    w.load_party(party);
    w.enter_battle(s.party, s.monsters);
    w.party.party_count = s.party;
    w.battle.status_effects = Default::default();
    w.battle.scripted_fight = s.scripted;
    w.battle.no_escape = s.scripted;
    w.battle.map_id = s.map as _;
    w.battle.monster_ai_state.mode_flags = s.round;
    w.set_battle_formation(FormationAdvantage::from_byte(s.advantage));
    for a in w.actors.iter_mut() {
        a.battle.hp = 0;
        a.battle.max_hp = 0;
        a.battle.liveness = 0;
        a.battle.init_key = 0;
        a.battle.field_flags = 0;
        a.battle_monster_id = None;
    }
    w.battle.speed = [0; 8];
    for r in 0..7u8 {
        let Some(e) = engine_seat(s, r) else { continue };
        let (i, k) = (usize::from(e), usize::from(r));
        w.battle.speed[i] = s.spd[k];
        let a = &mut w.actors[i];
        a.battle.hp = s.hp[k];
        a.battle.max_hp = s.max_hp[k];
        a.battle.liveness = u16::from(s.hp[k] != 0);
        a.battle.field_flags = s.status[k];
        if r >= 3 {
            a.battle_monster_id = Some(u16::from(s.monster_id));
        }
    }
    w.rng_state = s.seed;
}

fn battle_cpu(state: &Rc<Cell<u32>>) -> Option<Cpu> {
    let mut cpu = scus_cpu()?;
    cpu.mem.load(OVERLAY_BASE, &disc()?.read_entry(898).ok()?);
    let stream = state.clone();
    cpu.hook(RAND, move |r, _| {
        let next = stream.get().wrapping_mul(0x41C6_4E6D).wrapping_add(0x3039);
        stream.set(next);
        r.r[V0] = (next >> 16) & 0x7FFF;
    });
    let _ = GP;
    Some(cpu)
}

#[test]
fn a_round_opens_on_the_keys_retail_rolls() {
    let state = Rc::new(Cell::new(0));
    let Some(mut cpu) = battle_cpu(&state) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.stub(0x801D_ABA4, 0); // The first pick; `+0x16C` is what is compared.
    let mut w = World::new();
    let mut rng = Rng(0x494E_4954);
    let mut failures = Vec::new();
    for round in 0..20000 {
        let mut s = gen_seats(&mut rng);
        // A round only opens with someone standing.
        s.hp[3] = s.max_hp[3];
        seed_retail(&mut cpu, &s);
        state.set(s.seed);
        assert_eq!(cpu.call(0x801D_A780, &[]), Stop::Returned, "{s:?}");
        seed_port(&mut w, &s);
        w.reseed_initiative();
        let mut r_keys = Vec::new();
        let mut p_keys = Vec::new();
        for r in 0..7u8 {
            let Some(e) = engine_seat(&s, r) else {
                continue;
            };
            r_keys.push(cpu.mem.rd16(ACTOR + u32::from(r) * ACTOR_STRIDE + 0x16C));
            p_keys.push(w.actors[usize::from(e)].battle.init_key);
        }
        let retail = (r_keys, state.get());
        let port = (p_keys, w.rng_state);
        if retail != port && failures.len() < 6 {
            failures.push(format!(
                "round {round}:\n retail {retail:?}\n port   {port:?}\n {s:?}"
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn a_fight_opens_on_the_formation_retail_rolls() {
    let state = Rc::new(Cell::new(0));
    let Some(mut cpu) = battle_cpu(&state) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    // The two spread divides carry no `break`: level sides divide by zero on
    // the real chip, and what the chip leaves is what ships.
    cpu.hardware_zero_div = true;
    let mut w = World::new();
    let mut rng = Rng(0x464F_524D);
    let (mut failures, mut seen) = (Vec::new(), [0u32; 3]);
    for round in 0..40000 {
        let mut s = gen_seats(&mut rng);
        s.advantage = 0;
        // Everyone seated stands at the fight's open.
        s.hp = s.max_hp;
        s.status = [0; 7];
        seed_retail(&mut cpu, &s);
        state.set(s.seed);
        assert_eq!(cpu.call(0x8005_1D84, &[]), Stop::Returned, "{s:?}");
        let retail = (cpu.mem.rd8(CTX + 0x290), state.get());
        seen[usize::from(retail.0)] += 1;
        if s.scripted {
            continue;
        }
        seed_port(&mut w, &s);
        let formation = FormationDef::new(
            1000,
            (0..s.monsters)
                .map(|_| FormationSlot::new(u16::from(s.monster_id)))
                .collect(),
        );
        w.roll_battle_formation(&formation);
        let port = (w.battle_formation().to_byte(), w.rng_state);
        if retail != port && failures.len() < 6 {
            failures.push(format!(
                "round {round}:\n retail {retail:?}\n port   {port:?}\n {s:?}"
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert!(seen[1] > 50 && seen[2] > 50, "{seen:?}");
}

/// The tick's two regeneration passives: a party member with `+0xF8 & 0x20` or
/// `& 0x40` runs the applier on its own seat each round (`0x801E7730..0x801E7768`).
#[test]
fn the_regeneration_passives_restore_what_retail_restores() {
    let state = Rc::new(Cell::new(0));
    let (Some(mut cpu), Some(scus)) = (battle_cpu(&state), scus()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem.wr32(0x8007_B83C, 0x15);
    for stub in [0x801E_22C8u32, 0x8005_133C, 0x801F_44A0, 0x8003_5C00] {
        cpu.stub(stub, 0); // The cue and the popup.
    }
    let mut w = World::new();
    w.set_accessory_passives(AccessoryPassives::from_disc(
        &AccessoryPassiveTable::from_scus(&scus).expect("passives"),
    ));
    let mut rng = Rng(0x5245_4745);
    let mut failures = Vec::new();
    let mut healed = 0;
    for round in 0..4000 {
        let mut s = gen_seats(&mut rng);
        s.status = [0; 7];
        for m in 0..3 {
            s.word1[m] = [0x20, 0x40, 0x60, 0][rng.below(4) as usize];
        }
        seed_retail(&mut cpu, &s);
        for m in 0..3u32 {
            let a = ACTOR + m * ACTOR_STRIDE;
            cpu.mem.wr16(a + 0x150, 40);
            cpu.mem.wr16(a + 0x152, 400);
        }
        state.set(s.seed);
        assert_eq!(cpu.call(0x801E_752C, &[]), Stop::Returned, "{s:?}");
        seed_port(&mut w, &s);
        w.tables.character_max_mp = vec![400; 3];
        for m in 0..3 {
            w.actors[m].battle.mp = 40;
        }
        w.tick_status_effects();
        let mut r_out = Vec::new();
        let mut p_out = Vec::new();
        for r in 0..s.party {
            let a = ACTOR + u32::from(r) * ACTOR_STRIDE;
            r_out.push((cpu.mem.rd16(a + 0x14C), cpu.mem.rd16(a + 0x150)));
            let b = &w.actors[usize::from(r)].battle;
            p_out.push((b.hp, b.mp));
            healed += u32::from(cpu.mem.rd16(a + 0x14C) > s.hp[usize::from(r)]);
        }
        if r_out != p_out && failures.len() < 6 {
            failures.push(format!(
                "round {round}:\n retail {r_out:?}\n port   {p_out:?}\n {s:?}"
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert!(healed > 100, "{healed}");
}

#[test]
fn the_status_tick_drains_what_retail_drains() {
    let state = Rc::new(Cell::new(0));
    let (Some(mut cpu), Some(scus)) = (battle_cpu(&state), scus()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut w = World::new();
    w.set_accessory_passives(AccessoryPassives::from_disc(
        &AccessoryPassiveTable::from_scus(&scus).expect("passives"),
    ));
    let mut rng = Rng(0x444F_5420);
    let mut failures = Vec::new();
    for round in 0..20000 {
        let s = gen_seats(&mut rng);
        seed_retail(&mut cpu, &s);
        state.set(s.seed);
        assert_eq!(cpu.call(0x801E_752C, &[]), Stop::Returned, "{s:?}");
        seed_port(&mut w, &s);
        w.tick_status_effects();
        let mut r_out = Vec::new();
        let mut p_out = Vec::new();
        for r in 0..7u8 {
            let Some(e) = engine_seat(&s, r) else {
                continue;
            };
            let a = ACTOR + u32::from(r) * ACTOR_STRIDE;
            r_out.push((cpu.mem.rd16(a + 0x14C), cpu.mem.rd16(a + 0x16E)));
            p_out.push((w.actors[usize::from(e)].battle.hp, w.raw_status_word(e)));
        }
        let retail = (r_out, state.get());
        let port = (p_out, w.rng_state);
        if retail != port && failures.len() < 6 {
            failures.push(format!(
                "round {round}:\n retail {retail:?}\n port   {port:?}\n {s:?}"
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The tick over consecutive rounds with an ailment the port's status
/// tracker holds - Sleep (`0x800`), the `0x380` group, Numb (`0x400`) - beside
/// the same bits in retail's word: retail's tick never clears them, so
/// neither may a count of the tracker's.
#[test]
fn a_tracked_ailment_lasts_as_long_as_the_word() {
    use legaia_engine_battle_vm::status_effects::StatusKind;
    let state = Rc::new(Cell::new(0));
    let (Some(mut cpu), Some(scus)) = (battle_cpu(&state), scus()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut w = World::new();
    w.set_accessory_passives(AccessoryPassives::from_disc(
        &AccessoryPassiveTable::from_scus(&scus).expect("passives"),
    ));
    let kinds = [
        (StatusKind::Sleep, 0x0800u16),
        (StatusKind::Confuse, 0x0380),
        (StatusKind::Numb, 0x0400),
    ];
    let mut rng = Rng(0x534C_5045);
    let mut failures = Vec::new();
    let mut held = 0;
    for round in 0..2000 {
        let mut s = gen_seats(&mut rng);
        s.status = [0; 7];
        let mut ailed: Vec<(u8, usize)> = Vec::new();
        for r in 0..7u8 {
            if rng.below(2) == 0 {
                ailed.push((r, rng.below(3) as usize));
            }
        }
        for &(r, k) in &ailed {
            s.status[usize::from(r)] = kinds[k].1;
        }
        seed_retail(&mut cpu, &s);
        seed_port(&mut w, &s);
        for &(r, k) in &ailed {
            if let Some(e) = engine_seat(&s, r) {
                w.actors[usize::from(e)].battle.field_flags = 0;
                w.battle.status_effects.apply(e, kinds[k].0);
            }
        }
        state.set(s.seed);
        for tick in 0..6 {
            assert_eq!(cpu.call(0x801E_752C, &[]), Stop::Returned, "{s:?}");
            w.tick_status_effects();
            for r in 0..7u8 {
                let Some(e) = engine_seat(&s, r) else {
                    continue;
                };
                let a = ACTOR + u32::from(r) * ACTOR_STRIDE;
                let retail = cpu.mem.rd16(a + 0x16E) & 0x0F80;
                let port = w.raw_status_word(e) & 0x0F80;
                held += u32::from(retail != 0);
                if retail != port && failures.len() < 6 {
                    failures.push(format!(
                        "round {round} tick {tick} seat {r}: retail {retail:#x} port {port:#x}"
                    ));
                }
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert!(held > 5000, "{held}");
}
