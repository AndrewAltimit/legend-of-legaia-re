//! `FUN_801F0450`, the round's Auto assembler, run whole over the real art banks:
//! the pool arm's weights and spend loop, the art insertion tail and its learn
//! gate, and the delegated arm of an Evil Medallion wearer.

use crate::common::{Rng, disc, scus_cpu, seat_status};
use legaia_actor_formats::battle_char_assembly::decode_record0;
use legaia_engine_battle::monster_catalog::MonsterDef;
use legaia_engine_battle_loop::round_flow::PendingPartyAction;
use legaia_engine_core::world::World;
use legaia_save::Party;
use psx_retail_exec::{Cpu, Stop, V0};
use std::cell::Cell;
use std::rc::Rc;

const ASSEMBLER: u32 = 0x801F_0450;
const RAND: u32 = 0x8005_6798;
const OVERLAY_BASE: u32 = 0x801C_E818;
const CTX_PTR: u32 = 0x8007_BD24;
const ACTORS: u32 = 0x801C_9370;
const MONSTER_RECORDS: u32 = 0x801C_9348;
const RECORD0_TABLE: u32 = 0x801C_9360;
const SEATS: u32 = 0x8007_BD10;
const CHAR_RECORDS: u32 = 0x8008_4708;
const CHAR_STRIDE: u32 = 0x414;
const GUARDS: u32 = 0x801F_672C;

const CTX: u32 = 0x8012_0000;
const ACTOR: u32 = 0x8013_0000;
const ACTOR_STRIDE: u32 = 0x400;
const MONSTER: u32 = 0x8015_0000;
const RECORD0: u32 = 0x8016_0000;
const SWINGS: u32 = 0x8011_0000;

#[derive(Debug, Clone)]
struct Round {
    flagged: [bool; 3],
    category: [u8; 3],
    target: [u8; 3],
    gauge: [u16; 3],
    spirit: [u16; 3],
    status: [u16; 3],
    known: [Vec<u8>; 3],
    halved: [bool; 3],
    armed: [bool; 3],
    heads: [[[u8; 4]; 4]; 3],
    costs: [[u8; 4]; 3],
    classes: [u8; 2],
    seed: u32,
}

fn gen_round(rng: &mut Rng) -> Round {
    let mut r = Round {
        flagged: [false; 3],
        category: [3; 3],
        target: [3; 3],
        gauge: [0; 3],
        spirit: [0; 3],
        status: [0; 3],
        known: Default::default(),
        halved: [false; 3],
        armed: [false; 3],
        heads: [[[0; 4]; 4]; 3],
        costs: [[0; 4]; 3],
        classes: [rng.below(4) as u8, rng.below(4) as u8],
        seed: rng.next(),
    };
    for s in 0..3 {
        r.flagged[s] = rng.below(4) != 0;
        r.category[s] = if rng.below(8) == 0 { 1 } else { 3 };
        r.target[s] = 3 + rng.below(2) as u8;
        r.gauge[s] =
            [1, 29, 30, 60, 100, 120, 140, 60 + rng.below(100) as u16][rng.below(8) as usize];
        r.spirit[s] = [0, 18, 20, 32, 50, 100, rng.below(101) as u16][rng.below(7) as usize];
        r.status[s] = [
            0, 0, 0, 0, 0, 0, 0, 8, 0x10, 0x20, 0x38, 0x1000, 4, 0x400, 0x380,
        ][rng.below(15) as usize];
        r.halved[s] = rng.below(4) == 0;
        r.armed[s] = rng.below(2) == 0;
        r.known[s] = if rng.below(2) == 0 {
            (0..15).collect()
        } else {
            (0..15u8).filter(|_| rng.below(3) == 0).collect()
        };
        for i in 0..4 {
            r.costs[s][i] = [0x1E, 0x1E, 0x1E, 0x14, 0x32][rng.below(5) as usize];
            for b in 0..4 {
                r.heads[s][i][b] = match rng.below(5) {
                    0 => 0,
                    1 => 0x0C + rng.below(5) as u8,
                    2 => 0x11 + rng.below(5) as u8,
                    3 => 0x16 + rng.below(5) as u8,
                    _ => 0x1B + rng.below(5) as u8,
                };
            }
        }
    }
    r
}

fn actor(s: u32) -> u32 {
    ACTOR + s * ACTOR_STRIDE
}

fn seed_retail(cpu: &mut Cpu, r: &Round, banks: &[Vec<u8>]) {
    cpu.mem.wr32(CTX_PTR, CTX);
    for b in 0..0x400 {
        cpu.mem.wr8(CTX + b, 0);
    }
    cpu.mem.wr8(CTX, 3);
    cpu.mem.wr8(CTX + 1, 2);
    for s in 0..8u32 {
        cpu.mem.wr32(ACTORS + s * 4, actor(s));
        for b in 0..ACTOR_STRIDE {
            cpu.mem.wr8(actor(s) + b, 0);
        }
        cpu.mem.wr16(actor(s) + 0x14C, 100);
        cpu.mem.wr16(actor(s) + 0x14E, 100);
    }
    for m in 0..2u32 {
        let rec = MONSTER + m * 0x80;
        cpu.mem.wr32(MONSTER_RECORDS + m * 4, rec);
        cpu.mem.wr8(rec + 0x1E, r.classes[m as usize]);
    }
    for s in 0..3u32 {
        let i = s as usize;
        cpu.mem.wr8(SEATS + s, s as u8 + 1);
        cpu.mem.wr8(CTX + 0x266 + s, u8::from(r.flagged[i]));
        cpu.mem.wr8(CTX + 0x25F + s, u8::from(r.armed[i]));
        let a = actor(s);
        cpu.mem.wr8(a + 0x1DE, r.category[i]);
        cpu.mem.wr8(a + 0x1DD, r.target[i]);
        cpu.mem.wr16(a + 0x154, r.gauge[i]);
        cpu.mem.wr16(a + 0x170, r.spirit[i]);
        cpu.mem.wr16(a + 0x16E, r.status[i]);
        let rec = CHAR_RECORDS + s * CHAR_STRIDE;
        for b in 0..CHAR_STRIDE {
            cpu.mem.wr8(rec + b, 0);
        }
        cpu.mem
            .wr32(rec + 0xF8, if r.halved[i] { 0x800 } else { 0 });
        cpu.mem.wr8(rec + 0x185, r.known[i].len() as u8);
        cpu.mem.load(rec + 0x186, &r.known[i]);
        // Record 0, rebased, with its four direction commands' entries.
        let base = RECORD0 + s * 0x2_0000;
        cpu.mem.load(base, &banks[i]);
        let off = u32::from_le_bytes(banks[i][0x58..0x5C].try_into().unwrap());
        cpu.mem.wr32(base + 0x58, base + off);
        cpu.mem.wr32(RECORD0_TABLE + s * 4, base);
        for c in 0..4u32 {
            let block = SWINGS + (s * 4 + c) * 0x100;
            for b in 0..0x100 {
                cpu.mem.wr8(block + b, 0);
            }
            cpu.mem.load(block, &r.heads[i][c as usize]);
            cpu.mem.wr8(block + 0x74, r.costs[i][c as usize]);
            cpu.mem.wr32(base + (0xC + c) * 4, block);
        }
    }
}

#[allow(clippy::needless_range_loop)]
fn seed_port(w: &mut World, r: &Round, banks: &[Vec<u8>], guards: [u16; 4]) {
    let mut party = Party::zeroed(3);
    for m in 0..3 {
        let mut list = party.members[m].displayed_skills();
        list.count = r.known[m].len() as u8;
        list.ids = [0; 16];
        list.ids[..r.known[m].len()].copy_from_slice(&r.known[m]);
        party.members[m].set_displayed_skills(list);
        let mut bits = [0u8; legaia_save::ABILITY_BITS_LEN];
        if r.halved[m] {
            bits[5] = 0x08;
        }
        party.members[m].set_ability_bits(bits);
        let mut eq = party.members[m].equipment();
        eq.slots[if m == 1 { 2 } else { 3 }] = u8::from(r.armed[m]);
        party.members[m].set_equipment(eq);
    }
    w.load_party(party);
    w.enter_battle(3, 2);
    w.party.party_count = 3;
    w.battle.status_effects = Default::default();
    w.battle.auto_combo.guards = Some(guards);
    for m in 0..2u8 {
        let id = 0x40 + u16::from(m);
        let mut def = MonsterDef::new(id, "m", 100, 10);
        def.swing_class = r.classes[usize::from(m)];
        w.tables.monster_catalog.insert(def);
        let a = &mut w.actors[usize::from(3 + m)];
        a.battle_monster_id = Some(id);
        a.battle.hp = 100;
        a.battle.max_hp = 100;
        a.battle.liveness = 1;
    }
    for s in 0..3usize {
        let a = &mut w.actors[s];
        a.battle.hp = 100;
        a.battle.max_hp = 100;
        a.battle.liveness = 1;
        a.battle.agl = r.gauge[s];
        a.battle.spirit_gauge = r.spirit[s];
        seat_status(w, s as u8, r.status[s], r.seed >> s & 1 != 0);
        let a = &mut w.actors[s];
        a.battle.params = Default::default();
        w.battle.auto_combo.flags[s] = r.flagged[s];
        w.battle.round_flow.pending[s] =
            (r.category[s] == 3).then_some(PendingPartyAction::Attack {
                target: r.target[s],
            });
        w.battle.swing_costs[s] = r.costs[s].map(u16::from);
        let inputs = &mut w.battle.auto_combo.inputs[s];
        inputs.command_heads = Some(r.heads[s]);
        let bank = &banks[s];
        let off = u32::from_le_bytes(bank[0x58..0x5C].try_into().unwrap()) as usize;
        inputs.art_count = bank[off];
        inputs.art_records = (0..usize::from(bank[off]))
            .map(|k| {
                bank[off + 4 + k * 0xD0..off + 4 + (k + 1) * 0xD0]
                    .try_into()
                    .unwrap()
            })
            .collect();
    }
    w.rng_state = r.seed;
}

fn rig(state: &Rc<Cell<u32>>) -> Option<(Cpu, Vec<Vec<u8>>, [u16; 4])> {
    let (mut cpu, disc) = (scus_cpu()?, disc()?);
    cpu.mem
        .load(OVERLAY_BASE, &disc.read_entry(898).expect("PROT 0898"));
    let stream = state.clone();
    cpu.hook(RAND, move |r, _| {
        let next = stream.get().wrapping_mul(0x41C6_4E6D).wrapping_add(0x3039);
        stream.set(next);
        r.r[V0] = (next >> 16) & 0x7FFF;
    });
    let banks = (0..3)
        .map(|c| decode_record0(&disc.read_entry(863 + c).expect("player file")).expect("record0"))
        .collect();
    let mut guards = [0; 4];
    for (i, g) in guards.iter_mut().enumerate() {
        *g = cpu.mem.rd16(GUARDS + i as u32 * 2);
    }
    Some((cpu, banks, guards))
}

#[test]
fn an_auto_round_builds_the_queue_retail_builds() {
    let state = Rc::new(Cell::new(0));
    let Some((mut cpu, banks, guards)) = rig(&state) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut w = World::new();
    let mut rng = Rng(0x4155_544F);
    let (mut spent, mut hangs) = (0, 0);
    cpu.step_budget = 20_000;
    let mut failures = Vec::new();
    for round in 0..20000 {
        let r = gen_round(&mut rng);
        seed_retail(&mut cpu, &r, &banks);
        state.set(r.seed);
        match cpu.call(ASSEMBLER, &[]) {
            Stop::Returned => {}
            // The spend loop's own hang: nothing left the gauge can buy, with
            // the gauge still at the floor. The port ends the spend.
            Stop::StepLimit { pc } if (0x801F_0A64..0x801F_0B4C).contains(&pc) => {
                hangs += 1;
                continue;
            }
            other => panic!("round {round}: {other:?} {r:?}"),
        }
        let retail_rng = state.get();
        seed_port(&mut w, &r, &banks, guards);
        w.run_auto_attack_pool_arms();
        let mut bad = retail_rng != w.rng_state;
        let mut lines = Vec::new();
        for s in 0..3u32 {
            let i = s as usize;
            if !r.flagged[i] || r.category[i] != 3 {
                continue;
            }
            let retail: Vec<u8> = cpu
                .mem
                .read(actor(s) + 0x1DF, 48)
                .into_iter()
                .take_while(|&b| b != 0)
                .collect();
            let port: Vec<u8> = w.battle.auto_combo.queues[i]
                .clone()
                .unwrap_or_default()
                .into_iter()
                .take_while(|&b| b != 0)
                .collect();
            spent += u32::from(!retail.is_empty());
            if retail != port {
                bad = true;
            }
            lines.push(format!(
                " seat {s}: retail {retail:02x?}\n         port   {port:02x?}"
            ));
        }
        if bad && failures.len() < 5 {
            failures.push(format!(
                "round {round}: rng retail {retail_rng:#x} port {:#x}\n{}\n {r:?}",
                w.rng_state,
                lines.join("\n")
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert!(spent > 1000 && hangs > 0, "{spent} {hangs}");
}

/// The delegated arm: a member the game drives (record `+0xF8 & 0x2000`) takes a
/// target roll through the dead-target redirect and a blind draw off the learned
/// list, in seat order, ahead of everything else in the round.
#[test]
#[allow(clippy::needless_range_loop)]
fn a_delegated_member_draws_what_retail_draws() {
    let state = Rc::new(Cell::new(0));
    let Some((mut cpu, banks, guards)) = rig(&state) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut w = World::new();
    let mut rng = Rng(0x4445_4C45);
    let (mut filled, mut failures) = (0, Vec::new());
    for round in 0..20000 {
        let mut r = gen_round(&mut rng);
        r.flagged = [false; 3];
        let delegated: [bool; 3] = std::array::from_fn(|_| rng.below(2) == 0);
        let dead: [bool; 2] = [rng.below(3) == 0, rng.below(3) == 0];
        if dead[0] && dead[1] {
            continue;
        }
        seed_retail(&mut cpu, &r, &banks);
        for s in 0..3u32 {
            let rec = CHAR_RECORDS + s * CHAR_STRIDE;
            let word = if r.halved[s as usize] { 0x800 } else { 0 };
            let word = word | if delegated[s as usize] { 0x2000 } else { 0 };
            cpu.mem.wr32(rec + 0xF8, word);
            cpu.mem.wr8(actor(s) + 0x1DE, 0);
        }
        for m in 0..2u32 {
            if dead[m as usize] {
                cpu.mem.wr16(actor(3 + m) + 0x14C, 0);
            }
        }
        state.set(r.seed);
        assert_eq!(
            cpu.call(ASSEMBLER, &[]),
            Stop::Returned,
            "round {round}: {r:?}"
        );
        let retail_rng = state.get();
        seed_port(&mut w, &r, &banks, guards);
        for s in 0..3usize {
            let mut bits = w.party.roster.members[s].ability_bits();
            if delegated[s] {
                bits[5] |= 0x20;
            }
            w.party.roster.members[s].set_ability_bits(bits);
            w.actors[s].battle.action_category = 0;
            w.battle.round_flow.pending[s] = None;
        }
        for m in 0..2usize {
            if dead[m] {
                w.actors[3 + m].battle.hp = 0;
                w.actors[3 + m].battle.liveness = 0;
            }
        }
        w.battle_ctx = Default::default();
        w.run_round_state_zero();
        let mut bad = retail_rng != w.rng_state;
        let mut lines = Vec::new();
        for s in 0..3u32 {
            let i = s as usize;
            let vetoed = r.status[i] & 0x404 != 0;
            if !delegated[i] || vetoed {
                continue;
            }
            let retail_queue: Vec<u8> = cpu
                .mem
                .read(actor(s) + 0x1DF, 48)
                .into_iter()
                .take_while(|&b| b != 0)
                .collect();
            let a = &w.actors[i].battle;
            let port_queue: Vec<u8> = a.params.iter().copied().take_while(|&b| b != 0).collect();
            let retail = (
                cpu.mem.rd8(actor(s) + 0x1DE),
                cpu.mem.rd8(actor(s) + 0x1DD),
                &retail_queue,
            );
            let port = (a.action_category, a.active_target, &port_queue);
            filled += u32::from(!retail_queue.is_empty());
            if retail != port {
                bad = true;
            }
            lines.push(format!(
                " seat {s}: retail {retail:02x?}\n         port   {port:02x?}"
            ));
        }
        if bad && failures.len() < 5 {
            failures.push(format!(
                "round {round}: rng retail {retail_rng:#x} port {:#x} dead {dead:?}\n{}\n known {:?}",
                w.rng_state,
                lines.join("\n"),
                r.known
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert!(filled > 1000, "{filled}");
}

/// The round's seed runs the builder over what the assembler left, so the
/// arrows the tail spliced in come out as arts.
#[test]
fn the_seed_tokenizes_what_the_assembler_built() {
    use legaia_actor_formats::battle_char_assembly::art_animation_bank;
    use legaia_art::queue::{ActionConstant, Command};
    let state = Rc::new(Cell::new(0));
    let Some((mut cpu, banks, guards)) = rig(&state) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut w = World::new();
    for (ch, rec0) in banks.iter().enumerate() {
        let character = legaia_art::Character::all()[ch];
        for r in &art_animation_bank(rec0).expect("art bank") {
            let Some(action) = ActionConstant::from_byte(0x10 + r.index as u8) else {
                continue;
            };
            let commands: Vec<Command> = r
                .combo
                .iter()
                .filter_map(|&b| Command::from_byte(b))
                .collect();
            let mut extra = vec![r.stream_source];
            extra.extend_from_slice(&r.chain_sources);
            w.set_art_record(
                character,
                action,
                legaia_art::ArtRecord {
                    action,
                    commands,
                    anim_index: 0,
                    anim_extra: extra,
                    name: None,
                    power: vec![],
                    dmg_timing: vec![],
                    effect_cues: Default::default(),
                    hit_cues: vec![],
                    identifier: 0,
                    anim_speed: 0,
                    enemy_effect: legaia_art::record::EnemyEffect::Toxic,
                    repeat_frames: Default::default(),
                    background: 0,
                    runtime_address: None,
                },
            );
        }
    }
    let mut rng = Rng(0x544F_4B4E);
    let (mut arts, mut failures) = (0, Vec::new());
    for round in 0..8000 {
        let mut r = gen_round(&mut rng);
        r.flagged = [true; 3];
        r.category = [3; 3];
        r.status = [0; 3];
        r.armed = [true; 3];
        r.spirit = [100; 3];
        r.costs = [[0x1E; 4]; 3];
        r.known = std::array::from_fn(|_| (0..15).collect());
        seed_retail(&mut cpu, &r, &banks);
        for c in 0..4 {
            cpu.mem.wr8(0x801F_686C + c, 0);
        }
        state.set(r.seed);
        cpu.step_budget = 20_000;
        let stop = cpu.call(ASSEMBLER, &[]);
        cpu.step_budget = 0;
        match stop {
            Stop::Returned => {}
            Stop::StepLimit { .. } => continue,
            other => panic!("{other:?}"),
        }
        seed_port(&mut w, &r, &banks, guards);
        w.run_auto_attack_pool_arms();
        for s in 0..3u32 {
            assert_eq!(cpu.call(0x801E_ED1C, &[s]), Stop::Returned, "round {round}");
            let i = s as usize;
            w.dispatch_pending_party_action(
                s as u8,
                PendingPartyAction::Attack {
                    target: r.target[i],
                },
            );
            let retail: Vec<u8> = cpu
                .mem
                .read(actor(s) + 0x1DF, 19)
                .into_iter()
                .take_while(|&b| b != 0)
                .collect();
            let port: Vec<u8> = w.actors[i]
                .battle
                .params
                .iter()
                .copied()
                .take_while(|&b| b != 0)
                .collect();
            arts += u32::from(retail.iter().any(|&b| b == 0x19 || b == 0x1A));
            if (retail != port || state.get() != w.rng_state) && failures.len() < 5 {
                failures.push(format!(
                    "round {round} seat {s}:\n retail {retail:02x?} {:#x}\n port   {port:02x?} {:#x}",
                    state.get(),
                    w.rng_state
                ));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert!(arts > 200, "{arts}");
}
