//! `FUN_801EC3E4`, the melee hit resolver, against `World::land_melee_hit`.
//!
//! Both sides run one seated fight, drawn from one `rand()` stream: the block
//! roll, the damage roll pair with its underdog rewrite and chip floor, the
//! equipment fold, the guard ladder, the Spirit accrual, the kill compare and
//! the Seru absorb roll, the status arms of the clip's `+0x7A` class, the
//! reaction entry and the HP write.

use crate::common::{Rng, disc, scus_cpu, split_statuses};
use legaia_actor_formats::monster_archive::{MonsterAnimation, PartPose};
use legaia_engine_battle::accessory_passives::AccessoryPassives;
use legaia_engine_battle::battle_anim::MonsterAnimPlayer;
use legaia_engine_battle::monster_catalog::MonsterDef;
use legaia_engine_core::world::World;
use legaia_game_tables::accessory_passive::AccessoryPassiveTable;
use legaia_game_tables::element_affinity;
use legaia_save::{AutoCommandBand, Party};
use psx_retail_exec::{Cpu, Stop, V0};
use std::cell::Cell;
use std::rc::Rc;

const RESOLVER: u32 = 0x801E_C3E4;
const RAND: u32 = 0x8005_6798;
const CTX_PTR: u32 = 0x8007_BD24;
const ACTORS: u32 = 0x801C_9370;
const MONSTER_RECORDS: u32 = 0x801C_9348;
const SEATS: u32 = 0x8007_BD10;
const CHAR_RECORDS: u32 = 0x8008_4708;
const CHAR_STRIDE: u32 = 0x414;
const OVERLAY_BASE: u32 = 0x801C_E818;

const CTX: u32 = 0x8012_0000;
const ACTOR: u32 = 0x8013_0000;
const ACTOR_STRIDE: u32 = 0x400;
const MONSTER: u32 = 0x8015_0000;
const CLIP: u32 = 0x8016_0000;
const ANIM_NODE: u32 = 0x8016_1000;
const MYSTIC_WORD: u32 = 0x8007_BD84;

/// One generated hit: what both sides are seeded with.
#[derive(Debug, Clone)]
struct Hit {
    attacker: u8,
    target: u8,
    power: u8,
    committed: u8,
    kill_check: bool,
    class: u8,
    atk: [u16; 8],
    udf: [u16; 8],
    ldf: [u16; 8],
    spd: [u16; 8],
    hp: [u16; 8],
    max_hp: [u16; 8],
    status: [u16; 8],
    spirit: [u16; 8],
    accum: u32,
    attack_ramp: i16,
    guard_ramp: i16,
    juggle: u8,
    juggle_window: bool,
    in_block: bool,
    guarding: bool,
    fleeing: bool,
    ability: [[u32; 2]; 3],
    equip: [[u8; 5]; 3],
    element: u8,
    target_element: u8,
    absorb_seru: u8,
    absorb_pct: u8,
    has_block: bool,
    has_getup: bool,
    mystic: bool,
    drain_clip: bool,
    seed: u32,
}

/// A value with its boundaries over-represented.
fn stat(rng: &mut Rng, top: u32) -> u16 {
    match rng.below(8) {
        0 => 0,
        1 => 1,
        2 => top as u16,
        3 => (7 + 8 * rng.below(top / 8 + 1)).min(top) as u16,
        _ => rng.below(top + 1) as u16,
    }
}

fn gen_hit(rng: &mut Rng) -> Hit {
    let party_attacks = rng.below(2) == 0;
    let (attacker, target) = if party_attacks {
        (rng.below(3) as u8, 3 + rng.below(2) as u8)
    } else {
        (3 + rng.below(2) as u8, rng.below(3) as u8)
    };
    let top = [100, 400, 1200, 9999][rng.below(4) as usize];
    let mut h = Hit {
        attacker,
        target,
        power: 0x0C + rng.below(0x14) as u8,
        committed: if party_attacks {
            0x0C + rng.below(6) as u8
        } else {
            // A monster's clip index is a plain entry, any of them.
            1 + rng.below(0x18) as u8
        },
        kill_check: rng.below(2) == 0,
        class: if rng.below(2) == 0 {
            0
        } else {
            rng.below(8) as u8
        },
        atk: [0; 8],
        udf: [0; 8],
        ldf: [0; 8],
        spd: [0; 8],
        hp: [0; 8],
        max_hp: [0; 8],
        status: [0; 8],
        spirit: [0; 8],
        accum: 0,
        attack_ramp: [0, 0, 0x400, 0x800][rng.below(4) as usize],
        guard_ramp: [0, 0, 16, rng.below(0x200) as i16][rng.below(4) as usize],
        juggle: rng.below(6) as u8,
        juggle_window: rng.below(4) == 0,
        in_block: rng.below(8) == 0,
        guarding: target < 3 && rng.below(6) == 0,
        fleeing: target < 3 && rng.below(8) == 0,
        ability: [[0; 2]; 3],
        equip: [[0; 5]; 3],
        element: rng.below(8) as u8,
        target_element: rng.below(8) as u8,
        absorb_seru: if rng.below(2) == 0 {
            0
        } else {
            1 + rng.below(0x20) as u8
        },
        absorb_pct: rng.below(101) as u8,
        has_block: rng.below(2) == 0,
        has_getup: rng.below(2) == 0,
        mystic: rng.below(16) == 0,
        drain_clip: rng.below(6) == 0,
        seed: rng.next(),
    };
    for s in 0..8 {
        h.atk[s] = stat(rng, top);
        h.udf[s] = stat(rng, top);
        h.ldf[s] = stat(rng, top);
        // A zero sum traps retail's `divu`; every record a game seats has SPD.
        h.spd[s] = 1 + stat(rng, top.min(999)).min(998);
        h.max_hp[s] = 1 + rng.below(9999) as u16;
        h.hp[s] = 1 + rng.below(u32::from(h.max_hp[s])) as u16;
        h.spirit[s] = rng.below(101) as u16;
        h.status[s] = match rng.below(6) {
            0 => [1, 2, 3, 4, 0x80, 0x400, 0x1000, 0x1003][rng.below(8) as usize],
            _ => 0,
        };
    }
    for m in 0..3 {
        if rng.below(2) == 0 {
            // The passives the resolver reads; `0x2000` (War God) is the
            // caller's apply mode, so the kill gate is what it passes.
            let w0 = [
                0x4000u32,
                0x8_0000,
                0x10_0000,
                0x20_0000,
                0x2000_0000,
                0x4000_0000,
                0x8000_0000,
            ][rng.below(7) as usize];
            let w1 = [1u32, 2, 4, 8, 0x10, 0x100, 0x200, 0x1000, 0x20_0000][rng.below(9) as usize];
            h.ability[m] = [w0, w1];
        }
        for slot in 0..5 {
            h.equip[m][slot] = rng.below(0x80) as u8;
        }
    }
    h.accum = match rng.below(3) {
        0 => 0,
        _ => rng.below(u32::from(h.hp[target as usize]) + 20),
    };
    h
}

fn actor(slot: u8) -> u32 {
    ACTOR + u32::from(slot) * ACTOR_STRIDE
}

/// `+0x1EF..+0x1F3`, as `FUN_80054CB0` builds it for the clips [`clips`] seats.
fn reaction_map(h: &Hit, slot: u8) -> [u8; 5] {
    if slot < 3 {
        return [2, 3, 4, 5, 0x0B];
    }
    let tags = monster_tags(h);
    legaia_engine_battle_loop::clip_ladder::monster_reaction_map(&tags)
}

fn monster_tags(h: &Hit) -> Vec<u8> {
    let mut tags = vec![0u8, 2, 4];
    if h.has_getup {
        tags.push(5);
    }
    if h.has_block {
        tags.push(0x0B);
    }
    tags
}

fn clip(action_id: u8, impact_class: u8) -> MonsterAnimation {
    MonsterAnimation {
        variant_track: Default::default(),
        action_id,
        rate: 2,
        attach_key: 0,
        solo_flag: 0,
        impact_class,
        effect_script: Vec::new(),
        part_count: 1,
        frame_count: 2,
        frames: vec![vec![PartPose::default()]; 2],
    }
}

fn seed_retail(cpu: &mut Cpu, h: &Hit) {
    cpu.mem.wr32(CTX_PTR, CTX);
    for b in 0..0x900 {
        cpu.mem.wr8(CTX + b, 0);
    }
    cpu.mem.wr8(CTX, 3);
    cpu.mem.wr8(CTX + 0x0A, h.juggle);
    // The apply gate: `ctx[+0x15]` with no next step in the record.
    cpu.mem.wr8(CTX + 0x15, if h.kill_check { 0xFF } else { 0 });
    cpu.mem.wr16(CTX + 0x6D2, h.attack_ramp as u16);
    cpu.mem.wr16(CTX + 0x6D4, h.guard_ramp as u16);
    for m in 0..3u32 {
        cpu.mem.wr8(SEATS + m, m as u8 + 1);
        let rec = CHAR_RECORDS + m * CHAR_STRIDE;
        cpu.mem.wr32(rec + 0xF4, h.ability[m as usize][0]);
        cpu.mem.wr32(rec + 0xF8, h.ability[m as usize][1]);
        cpu.mem.wr8(rec + 0x1A7, 0x0C);
        cpu.mem.wr8(rec + 0x1B7, 0x0D);
        for s in 0..5 {
            cpu.mem
                .wr8(rec + 0x196 + s, h.equip[m as usize][s as usize]);
        }
    }
    for b in 0..0x100 {
        cpu.mem.wr8(ANIM_NODE + b, 0);
        cpu.mem.wr8(MONSTER + b, 0);
        cpu.mem.wr8(MONSTER + 0x100 + b, 0);
    }
    for s in 0..8u8 {
        let a = actor(s);
        cpu.mem.wr32(ACTORS + u32::from(s) * 4, a);
        for b in 0..ACTOR_STRIDE {
            cpu.mem.wr8(a + b, 0);
        }
        let i = usize::from(s);
        cpu.mem.wr16(a + 0x14C, h.hp[i]);
        cpu.mem.wr16(a + 0x14E, h.max_hp[i]);
        cpu.mem.wr16(a + 0x158, h.atk[i]);
        cpu.mem.wr16(a + 0x15C, h.udf[i]);
        cpu.mem.wr16(a + 0x160, h.ldf[i]);
        cpu.mem.wr16(a + 0x164, h.spd[i]);
        cpu.mem.wr16(a + 0x16E, h.status[i]);
        cpu.mem.wr16(a + 0x170, h.spirit[i]);
        cpu.mem.wr16(a + 0x172, h.hp[i]);
        cpu.mem.wr32(a + 0x22C, ANIM_NODE);
        cpu.mem.wr32(ANIM_NODE + 0x4C, ANIM_NODE);
        for (k, e) in reaction_map(h, s).into_iter().enumerate() {
            cpu.mem.wr8(a + 0x1EF + k as u32, e);
        }
    }
    for m in 0..2u32 {
        let rec = MONSTER + m * 0x80;
        cpu.mem.wr32(MONSTER_RECORDS + m * 4, rec);
    }
    let att = actor(h.attacker);
    let tgt = actor(h.target);
    cpu.mem.wr32(MYSTIC_WORD, u32::from(h.mystic));
    cpu.mem.wr32(MONSTER + 0x4C, ANIM_NODE);
    if h.drain_clip {
        cpu.mem.wr8(ANIM_NODE + 0x77, 0x3B);
    }
    let monster_record = |slot: u8| MONSTER + u32::from(slot - 3) * 0x80;
    if h.attacker >= 3 {
        cpu.mem.wr8(monster_record(h.attacker) + 0x1D, h.element);
    }
    if h.target >= 3 {
        let r = monster_record(h.target);
        cpu.mem.wr8(r + 0x1D, h.target_element);
        cpu.mem.wr8(r + 0x3E, h.absorb_seru);
        cpu.mem.wr8(r + 0x3F, h.absorb_pct);
    }
    cpu.mem.wr8(att + 0x1D9, h.committed);
    cpu.mem.wr8(att + 0x1DD, h.target);
    cpu.mem.wr32(tgt, h.accum);
    if h.juggle_window {
        cpu.mem.wr8(tgt + 0x1F7, 1);
        // The window runs only while a reaction plays.
        cpu.mem.wr8(tgt + 0x1D9, 1);
    }
    if h.in_block && reaction_map(h, h.target)[4] != 0 {
        cpu.mem.wr8(tgt + 0x1D9, reaction_map(h, h.target)[4]);
    }
    if h.guarding {
        cpu.mem.wr8(tgt + 0x1DE, 4);
    } else if h.fleeing {
        cpu.mem.wr8(tgt + 0x1DE, 5);
    }
    // The clip record: one power byte, one step, the impact class.
    for b in 0..0x100 {
        cpu.mem.wr8(CLIP + b, 0);
    }
    cpu.mem.wr8(CLIP, h.power);
    cpu.mem.wr8(CLIP + 0x10, 1);
    cpu.mem.wr8(CLIP + 0x7A, h.class);
}

fn seed_port(w: &mut World, h: &Hit) {
    let mut party = Party::zeroed(3);
    for m in 0..3 {
        let mut bytes = [0u8; legaia_save::ABILITY_BITS_LEN];
        bytes[0..4].copy_from_slice(&h.ability[m][0].to_le_bytes());
        bytes[4..8].copy_from_slice(&h.ability[m][1].to_le_bytes());
        party.members[m].set_ability_bits(bytes);
        party.members[m].set_auto_command_string(AutoCommandBand::Primary, [0x0C; 16]);
        party.members[m].set_auto_command_string(AutoCommandBand::Secondary, [0x0D; 16]);
    }
    w.load_party(party);
    w.enter_battle(3, 2);
    w.party.party_count = 3;
    w.battle.status_effects = Default::default();
    w.battle.juggle = h.juggle;
    w.battle.monster_ai_state.flag_bd84 = u32::from(h.mystic) as _;
    w.battle.attack_ramp = h.attack_ramp;
    w.battle.guard_ramp = h.guard_ramp;
    w.battle.guarding = [false; 3];
    for s in 0..5u8 {
        let i = usize::from(s);
        let is_party = s < 3;
        w.battle.set_battle_attack(s, h.atk[i]);
        w.battle.attack[i] = h.atk[i];
        w.battle.defense_split[i] = Some((h.udf[i], h.ldf[i]));
        w.battle.speed[i] = h.spd[i];
        let a = &mut w.actors[i];
        a.battle.liveness = 1;
        a.battle.hp = h.hp[i];
        a.battle.max_hp = h.max_hp[i];
        a.battle.spd = h.spd[i];
        a.battle.field_flags = h.status[i];
        a.battle.spirit_gauge = h.spirit[i];
        a.battle.damage_accum = 0;
        a.battle.action_category = 0;
        a.battle.current_anim = 0;
        a.battle.hp_display = None;
        a.battle.hp_bar_pending = 0;
        a.battle_juggle_window = false;
        a.battle_reaction_entry = None;
        a.battle_animation = None;
        a.battle_monster_id = if is_party {
            None
        } else {
            Some(0x40 + u16::from(s))
        };
        a.battle_element = (!is_party).then_some(if s == h.attacker {
            h.element
        } else {
            h.target_element
        });
        let tags = if is_party {
            (0..0x12u8).collect::<Vec<_>>()
        } else {
            monster_tags(h)
        };
        let clips: Vec<_> = tags.iter().map(|&t| Some(clip(t, 0))).collect();
        w.set_actor_battle_action_clips(i, std::sync::Arc::new(clips));
        if !is_party {
            let mut def = MonsterDef::new(0x40 + u16::from(s), "m", 1, 10);
            def.absorb_seru = h.absorb_seru;
            def.absorb_chance_pct = h.absorb_pct;
            w.tables.monster_catalog.insert(def);
        }
    }
    split_statuses(w, h.seed);
    let ti = usize::from(h.target);
    let ai = usize::from(h.attacker);
    w.actors[ti].battle.damage_accum = h.accum;
    w.actors[ti].battle_juggle_window = h.juggle_window;
    if h.juggle_window {
        w.actors[ti].battle_reaction_entry = Some(1);
    }
    if h.in_block && reaction_map(h, h.target)[4] != 0 {
        w.actors[ti].battle_reaction_entry = Some(reaction_map(h, h.target)[4]);
    }
    if h.guarding && h.target < 3 {
        w.battle.guarding[ti] = true;
    }
    if h.fleeing && !h.guarding {
        w.actors[ti].battle.action_category = 5;
    }
    w.actors[ai].battle.current_anim = h.committed;
    w.actors[ai].battle.active_target = h.target;
    let mut playing = clip(0x0C, h.class);
    playing.attach_key = if h.drain_clip { 0x3B } else { 0 };
    w.actors[ai].battle_animation = MonsterAnimPlayer::new_one_shot(&playing);
    w.rng_state = h.seed;
}

#[derive(Debug, PartialEq, Eq, Clone)]
struct Outcome {
    accum: u32,
    hp: u16,
    pending: i32,
    spirit: u16,
    status: u16,
    juggle: u8,
    ramps: (i16, i16),
    reaction: u8,
    attacker_hp: u16,
    attacker_pending: i32,
    auto_heads: Option<(u8, u8)>,
    absorbed: u8,
    rng: u32,
}

#[test]
fn a_melee_hit_lands_what_retail_lands() {
    differential!(prot(898, RESOLVER));
    let (Some(mut cpu), Some(disc)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let overlay = disc.read_entry(898).expect("PROT 0898");
    cpu.mem.load(OVERLAY_BASE, &overlay);
    let state = Rc::new(Cell::new(0u32));
    let stream = state.clone();
    cpu.hook(RAND, move |r, _| {
        let next = stream.get().wrapping_mul(0x41C6_4E6D).wrapping_add(0x3039);
        stream.set(next);
        r.r[V0] = (next >> 16) & 0x7FFF;
    });
    cpu.stub(0x8003_D53C, 0); // The voice cue.
    cpu.stub(0x8004_FE5C, 0); // The Mystic Shield's sound.
    cpu.stub(0x8001_9B28, 0); // The bearing; turning is compared elsewhere.

    let scus = crate::common::scus().unwrap();
    let mut w = World::new();
    w.tables.element_affinity = element_affinity::parse(&overlay);
    w.set_accessory_passives(AccessoryPassives::from_disc(
        &AccessoryPassiveTable::from_scus(&scus).expect("passives"),
    ));

    // The attack byte of the equipment two-hop, read off the executable.
    let equip_byte = |cpu: &mut Cpu, id: u8| -> u8 {
        let row = cpu.mem.rd8(0x8007_4368 + u32::from(id) * 0xC + 1);
        cpu.mem.rd8(0x8007_4F68 + u32::from(row) * 8 + 1)
    };

    let mut rng = Rng(0x4D45_4C45);
    let mut blocked = 0;
    let mut hits = 0;
    let mut rewrites = 0;
    let mut failures: Vec<String> = Vec::new();
    let rounds: u32 = std::env::var("MELEE_ROUNDS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(20000);
    for round in 0..rounds {
        let h = gen_hit(&mut rng);
        seed_retail(&mut cpu, &h);
        state.set(h.seed);
        let stop = cpu.call(RESOLVER, &[u32::from(h.attacker), CLIP, 0, 0]);
        assert_eq!(stop, Stop::Returned, "round {round}: {h:?}");
        let t = actor(h.target);
        let a = actor(h.attacker);
        let retail = Outcome {
            accum: cpu.mem.rd32(t),
            hp: cpu.mem.rd16(t + 0x14C),
            pending: cpu.mem.rd32(t + 0x10) as i32,
            spirit: cpu.mem.rd16(t + 0x170),
            status: cpu.mem.rd16(t + 0x16E),
            juggle: cpu.mem.rd8(CTX + 0x0A),
            ramps: (
                cpu.mem.rd16(CTX + 0x6D2) as i16,
                cpu.mem.rd16(CTX + 0x6D4) as i16,
            ),
            reaction: cpu.mem.rd8(t + 0x1DA),
            attacker_hp: cpu.mem.rd16(a + 0x14C),
            attacker_pending: cpu.mem.rd32(a + 0x10) as i32,
            auto_heads: (h.target < 3).then(|| {
                let rec = CHAR_RECORDS + u32::from(h.target) * CHAR_STRIDE;
                (cpu.mem.rd8(rec + 0x1A7), cpu.mem.rd8(rec + 0x1B7))
            }),
            absorbed: cpu.mem.rd8(CTX + 0x269),
            rng: state.get(),
        };

        seed_port(&mut w, &h);
        let commits = w.actors[usize::from(h.target)].battle_clip_commits;
        for m in 0..3 {
            w.battle.equip_atk[m] = h.equip[m].map(|id| equip_byte(&mut cpu, id));
        }
        let dmg = w.land_melee_hit(
            h.attacker,
            h.target,
            h.power,
            h.committed,
            false,
            h.kill_check,
        );
        if h.kill_check {
            w.apply_combo_total(h.target);
        }
        let ta = &w.actors[usize::from(h.target)];
        let port = Outcome {
            accum: ta.battle.damage_accum,
            hp: ta.battle.hp,
            pending: ta.battle.hp_bar_pending,
            spirit: ta.battle.spirit_gauge,
            status: w.raw_status_word(h.target),
            juggle: w.battle.juggle,
            ramps: (w.battle.attack_ramp, w.battle.guard_ramp),
            reaction: if ta.battle_clip_commits != commits {
                ta.battle_reaction_entry.unwrap_or(0)
            } else {
                0
            },
            attacker_hp: w.actors[usize::from(h.attacker)].battle.hp,
            attacker_pending: w.actors[usize::from(h.attacker)].battle.hp_bar_pending,
            auto_heads: (h.target < 3).then(|| {
                let rec = &w.party.roster.members[usize::from(h.target)];
                (
                    rec.auto_command_string(AutoCommandBand::Primary)[0],
                    rec.auto_command_string(AutoCommandBand::Secondary)[0],
                )
            }),
            absorbed: w.battle_ctx.absorbed_seru,
            rng: w.rng_state,
        };
        if dmg == 0 {
            blocked += 1;
        } else {
            hits += 1;
        }
        if cpu.steps > 0 && retail.rng != state.get() {
            rewrites += 1;
        }
        if retail != port {
            failures.push(format!(
                "round {round}:\n retail {retail:?}\n port   {port:?}\n {h:?}"
            ));
            if failures.len() >= 6 {
                break;
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert!(
        blocked > 100 && hits > 1000,
        "{blocked} blocked, {hits} hits, {rewrites}"
    );
}
