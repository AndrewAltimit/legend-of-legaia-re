//! `FUN_801DD0AC`, the shared cast kernel with its scale `FUN_801DD864` and
//! finisher `FUN_801DDB30`, on both branches: a monster's special against
//! `World::enemy_move_predamage`, a summon body's hit against
//! `World::summon_module_roll`, each followed by the fold that lands the hit.

use crate::common::{Rng, disc, scus_cpu, split_statuses};
use legaia_engine_battle::monster_catalog::MonsterDef;
use legaia_engine_battle::move_power::MovePowerCatalog;
use legaia_engine_battle::spells::SpellOutcome;
use legaia_engine_core::world::World;
use legaia_game_tables::element_affinity;
use legaia_retail_exec::{Cpu, Stop, V0};
use legaia_save::Party;
use std::cell::Cell;
use std::rc::Rc;

const KERNEL: u32 = 0x801D_D0AC;
const RAND: u32 = 0x8005_6798;
const OVERLAY_BASE: u32 = 0x801C_E818;
const CTX_PTR: u32 = 0x8007_BD24;
const ACTORS: u32 = 0x801C_9370;
const MONSTER_RECORDS: u32 = 0x801C_9348;
const SEATS: u32 = 0x8007_BD10;
const CHAR_RECORDS: u32 = 0x8008_4708;
const CHAR_STRIDE: u32 = 0x414;
const MYSTIC_WORD: u32 = 0x8007_BD84;
const SPECIAL_WORD: u32 = 0x8007_BAC0;
const MOVE_MAP: u32 = 0x801F_4E64;
const POPUP: u32 = 0x8007_BD14;

const CTX: u32 = 0x8012_0000;
const ACTOR: u32 = 0x8013_0000;
const ACTOR_STRIDE: u32 = 0x400;
const MONSTER: u32 = 0x8015_0000;

#[derive(Debug, Clone, Default)]
struct Seat {
    hp: u16,
    max_hp: u16,
    int: u16,
    atk: u16,
    udf: u16,
    ldf: u16,
    status: u16,
    spirit: u16,
    guarding: bool,
}

#[derive(Debug, Clone)]
struct Cast {
    seats: [Seat; 8],
    attacker: u8,
    target: u8,
    ability: [[u32; 2]; 3],
    elements: [u8; 5],
    mystic: bool,
    seed: u32,
    // The summon branch.
    caster: u8,
    spell: u8,
    level: u8,
    group: bool,
    special: bool,
}

fn stat(rng: &mut Rng, top: u32) -> u16 {
    match rng.below(8) {
        0 => 0,
        1 => 1,
        2 => top as u16,
        _ => rng.below(top + 1) as u16,
    }
}

fn gen_cast(rng: &mut Rng) -> Cast {
    let top = [60, 200, 600, 2000, 9999][rng.below(5) as usize];
    let mut seats: [Seat; 8] = Default::default();
    for s in &mut seats {
        s.max_hp = 1 + rng.below(9999) as u16;
        s.hp = 1 + rng.below(u32::from(s.max_hp)) as u16;
        s.int = stat(rng, top);
        s.atk = stat(rng, top);
        s.udf = stat(rng, top);
        s.ldf = stat(rng, top);
        s.spirit = rng.below(101) as u16;
        s.status = match rng.below(6) {
            0 => [1, 2, 3, 4, 0x80, 0x100, 0x400, 0x800, 0x1000, 0x1083][rng.below(10) as usize],
            _ => 0,
        };
    }
    let mut ability = [[0u32; 2]; 3];
    for m in &mut ability {
        if rng.below(2) == 0 {
            m[0] = [0x2000_0000u32, 0x4000_0000, 0x8000_0000, 0][rng.below(4) as usize];
            m[1] = [1u32, 2, 4, 8, 0x10, 0x100, 0x200, 0][rng.below(8) as usize];
        }
    }
    let mut elements = [0; 5];
    for e in &mut elements {
        *e = rng.below(8) as u8;
    }
    let attacker = 3 + rng.below(2) as u8;
    let target = if rng.below(8) == 0 {
        3 + rng.below(2) as u8
    } else {
        rng.below(3) as u8
    };
    let guarding = rng.below(6) == 0;
    seats[usize::from(target)].guarding = guarding && target < 3;
    Cast {
        seats,
        attacker,
        target,
        ability,
        elements,
        mystic: rng.below(16) == 0,
        seed: rng.next(),
        caster: rng.below(3) as u8,
        spell: 0x81 + rng.below(0x0B) as u8,
        level: 1 + rng.below(9) as u8,
        group: rng.below(2) == 0,
        special: rng.below(8) == 0,
    }
}

fn actor(slot: u8) -> u32 {
    ACTOR + u32::from(slot) * ACTOR_STRIDE
}

fn seed_retail(cpu: &mut Cpu, c: &Cast) {
    cpu.mem.wr32(CTX_PTR, CTX);
    for b in 0..0x900 {
        cpu.mem.wr8(CTX + b, 0);
    }
    cpu.mem.wr8(CTX, 3);
    cpu.mem.wr8(CTX + 1, 2);
    cpu.mem.wr8(CTX + 0x13, c.caster);
    cpu.mem.wr32(MYSTIC_WORD, u32::from(c.mystic));
    cpu.mem.wr32(SPECIAL_WORD, u32::from(c.special));
    cpu.mem.wr32(POPUP, 0);
    for m in 0..3u32 {
        cpu.mem.wr8(SEATS + m, m as u8 + 1);
        let rec = CHAR_RECORDS + m * CHAR_STRIDE;
        for b in 0..CHAR_STRIDE {
            cpu.mem.wr8(rec + b, 0);
        }
        cpu.mem.wr32(rec + 0xF4, c.ability[m as usize][0]);
        cpu.mem.wr32(rec + 0xF8, c.ability[m as usize][1]);
    }
    for s in 0..8u8 {
        let a = actor(s);
        cpu.mem.wr32(ACTORS + u32::from(s) * 4, a);
        for b in 0..ACTOR_STRIDE {
            cpu.mem.wr8(a + b, 0);
        }
        let st = &c.seats[usize::from(s)];
        cpu.mem.wr16(a + 0x14C, st.hp);
        cpu.mem.wr16(a + 0x14E, st.max_hp);
        cpu.mem.wr16(a + 0x168, st.int);
        cpu.mem.wr16(a + 0x158, st.atk);
        cpu.mem.wr16(a + 0x15C, st.udf);
        cpu.mem.wr16(a + 0x160, st.ldf);
        cpu.mem.wr16(a + 0x16E, st.status);
        cpu.mem.wr16(a + 0x170, st.spirit);
        cpu.mem.wr8(a + 0x1DD, c.target);
        if st.guarding {
            cpu.mem.wr8(a + 0x1DE, 4);
        }
    }
    // Seat `3 + m`'s record, and at index 4 the summon body's.
    for m in 0..5u32 {
        let rec = MONSTER + m * 0x80;
        cpu.mem.wr32(MONSTER_RECORDS + m * 4, rec);
        cpu.mem.wr8(rec + 0x1D, c.elements[m as usize]);
    }
    // The cast's own bytes.
    let caster = actor(c.caster);
    cpu.mem.wr8(caster + 0x1DF, c.spell);
    let rec = CHAR_RECORDS + u32::from(c.caster) * CHAR_STRIDE;
    cpu.mem.wr8(rec + 0x13D, c.spell);
    cpu.mem.wr8(rec + 0x161, c.level);
    if c.group {
        cpu.mem.wr8(actor(7) + 0x1DD, 9);
    }
}

fn seed_port(w: &mut World, c: &Cast) {
    let mut party = Party::zeroed(3);
    for m in 0..3 {
        let mut bytes = [0u8; legaia_save::ABILITY_BITS_LEN];
        bytes[0..4].copy_from_slice(&c.ability[m][0].to_le_bytes());
        bytes[4..8].copy_from_slice(&c.ability[m][1].to_le_bytes());
        party.members[m].set_ability_bits(bytes);
    }
    let mut list = party.members[usize::from(c.caster)].spell_list();
    list.ids[0] = c.spell;
    list.levels[0] = c.level;
    party.members[usize::from(c.caster)].set_spell_list(list);
    w.load_party(party);
    w.enter_battle(3, 2);
    w.party.party_count = 3;
    w.battle.status_effects = Default::default();
    w.battle.monster_ai_state.flag_bd84 = u32::from(c.mystic) as _;
    w.battle.special_word = u32::from(c.special);
    w.battle.guarding = [false; 3];
    w.casting.module_hit_xp = 0;
    for s in 0..5u8 {
        let i = usize::from(s);
        let st = &c.seats[i];
        w.battle.accuracy[i] = st.int;
        w.battle.attack[i] = st.atk;
        w.battle.defense_split[i] = Some((st.udf, st.ldf));
        if let Some(g) = w.battle.guarding.get_mut(i) {
            *g = st.guarding;
        }
        let a = &mut w.actors[i];
        a.battle.liveness = 1;
        a.battle.hp = st.hp;
        a.battle.max_hp = st.max_hp;
        a.battle.field_flags = st.status;
        a.battle.spirit_gauge = st.spirit;
        a.battle.hp_bar_pending = 0;
        a.battle_monster_id = (s >= 3).then_some(0x40 + u16::from(s));
        a.battle_element = (s >= 3).then_some(c.elements[i - 3 * usize::from(s >= 3)]);
    }
    split_statuses(w, c.seed);
    let body = &c.seats[7];
    let mut def = MonsterDef::new(0x100, "body", body.hp, body.atk);
    def.intel = body.int;
    def.element = c.elements[4];
    w.tables.summon_creatures.insert(c.spell, def);
    w.rng_state = c.seed;
}

#[derive(Debug, PartialEq, Eq, Clone)]
struct Outcome {
    net: i32,
    spirit: u16,
    status: u16,
    xp: u32,
    rng: u32,
}

/// The CPU and a World with the same disc tables, sharing nothing else.
fn rig(state: &Rc<Cell<u32>>) -> Option<(Cpu, World)> {
    let (mut cpu, disc) = (scus_cpu()?, disc()?);
    let overlay = disc.read_entry(898).expect("PROT 0898");
    cpu.mem.load(OVERLAY_BASE, &overlay);
    let stream = state.clone();
    cpu.hook(RAND, move |r, _| {
        let next = stream.get().wrapping_mul(0x41C6_4E6D).wrapping_add(0x3039);
        stream.set(next);
        r.r[V0] = (next >> 16) & 0x7FFF;
    });
    let mut w = World::new();
    w.tables.element_affinity = element_affinity::parse(&overlay);
    w.tables.move_power = MovePowerCatalog::from_overlay_0898(&overlay);
    Some((cpu, w))
}

fn land(w: &mut World, target: u8, amount: u16) {
    w.fold_spell_outcome(SpellOutcome::Damage {
        target,
        amount,
        element: legaia_engine_battle::spells::SpellElement::Neutral,
        weakness: false,
    });
}

#[test]
fn a_monster_special_rolls_what_retail_rolls() {
    let state = Rc::new(Cell::new(0));
    let Some((mut cpu, mut w)) = rig(&state) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let cat = w.tables.move_power.clone().expect("move power");
    const BYPASS: [u8; 7] = [0x37, 0x5C, 0x5D, 0x5E, 0x79, 0x7A, 0x7B];
    let moves: Vec<u8> = (1..0x80u8)
        .filter(|m| !BYPASS.contains(m))
        .filter(|&m| cat.power_for_move_id(m).is_some())
        .collect();
    assert!(moves.len() > 20, "{}", moves.len());
    let mut rng = Rng(0x4341_5354);
    let mut failures = Vec::new();
    let (mut bonus, mut floor, mut aliased) = (0, 0, 0);
    for round in 0..40000 {
        let c = gen_cast(&mut rng);
        let id = moves[rng.below(moves.len() as u32) as usize];
        seed_retail(&mut cpu, &c);
        let record = cpu.mem.rd8(MOVE_MAP + u32::from(id) - 1);
        state.set(c.seed);
        let stop = cpu.call(
            KERNEL,
            &[
                u32::from(record),
                u32::from(c.attacker),
                u32::from(c.target),
            ],
        );
        assert_eq!(stop, Stop::Returned, "round {round}: {c:?}");
        let t = actor(c.target);
        let retail = Outcome {
            net: cpu.regs.r[V0] as i32,
            spirit: cpu.mem.rd16(t + 0x170),
            status: cpu.mem.rd16(t + 0x16E),
            xp: 0,
            rng: state.get(),
        };
        seed_port(&mut w, &c);
        let power = cat.power_for_move_id(id).unwrap();
        let amount = w
            .enemy_move_predamage(c.attacker, c.target, power, id)
            .expect("rolled");
        let net = match amount {
            u16::MAX => -1,
            // The fold's own absorb: the roll stands, the hit lands as nothing.
            _ if c.seats[usize::from(c.target)].status & 4 != 0 => 0,
            _ => i32::from(amount),
        };
        land(&mut w, c.target, amount);
        let ta = &w.actors[usize::from(c.target)];
        let port = Outcome {
            net,
            spirit: ta.battle.spirit_gauge,
            status: w.raw_status_word(c.target),
            xp: 0,
            rng: w.rng_state,
        };
        // Any negative net is the same landed hit: the caller's clamp takes all.
        let same = Outcome {
            net: if retail.net < 0 { -1 } else { retail.net },
            ..retail.clone()
        } == port;
        floor += u32::from(retail.net >= 8 && retail.net <= 16);
        aliased += u32::from(retail.net < 0);
        bonus += u32::from(retail.net > 16);
        if !same && failures.len() < 8 {
            failures.push(format!(
                "round {round} move {id:#x} power {power}:\n retail {retail:?}\n port   {port:?}\n a {} {:?} t {} {:?}\n {:x?} el {:?}",
                c.attacker, c.seats[usize::from(c.attacker)], c.target, c.seats[usize::from(c.target)], c.ability, c.elements
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert!(
        bonus > 1000 && floor > 10 && aliased > 0,
        "{bonus} {floor} {aliased}"
    );
}

#[test]
fn a_summon_body_rolls_what_retail_rolls() {
    let state = Rc::new(Cell::new(0));
    let Some((mut cpu, mut w)) = rig(&state) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut rng = Rng(0x5355_4D4E);
    let mut failures = Vec::new();
    for round in 0..40000 {
        let mut c = gen_cast(&mut rng);
        // A player cast lands on the monster row.
        c.target = 3 + rng.below(2) as u8;
        for s in &mut c.seats {
            s.guarding = false;
        }
        // Summon bodies never carry a status at the roll.
        c.seats[7].status = 0;
        c.seats[usize::from(c.caster)].status = 0;
        seed_retail(&mut cpu, &c);
        state.set(c.seed);
        let stop = cpu.call(KERNEL, &[0x12, 7, u32::from(c.target)]);
        assert_eq!(stop, Stop::Returned, "round {round}: {c:?}");
        let t = actor(c.target);
        let rec = CHAR_RECORDS + u32::from(c.caster) * CHAR_STRIDE;
        let retail = Outcome {
            net: cpu.regs.r[V0] as i32,
            spirit: cpu.mem.rd16(t + 0x170),
            status: cpu.mem.rd16(t + 0x16E),
            xp: cpu.mem.rd32(rec + 8),
            rng: state.get(),
        };
        seed_port(&mut w, &c);
        let roll = w
            .player_summon_predamage(c.caster, c.target, c.spell)
            .expect("rolled");
        let xp = w.summon_hit_xp(c.target, u32::from(roll), c.group);
        land(&mut w, c.target, roll);
        let ta = &w.actors[usize::from(c.target)];
        let port = Outcome {
            net: if c.seats[usize::from(c.target)].status & 4 != 0 {
                0
            } else {
                i32::from(roll)
            },
            spirit: ta.battle.spirit_gauge,
            status: w.raw_status_word(c.target),
            xp,
            rng: w.rng_state,
        };
        if retail != port && failures.len() < 8 {
            failures.push(format!(
                "round {round}:\n retail {retail:?}\n port   {port:?}\n t {} {:?}\n body {:?} caster {} {:?} spell {:x} lv {} group {} special {} {:x?} el {:?} my {}",
                c.target, c.seats[usize::from(c.target)], c.seats[7], c.caster, c.seats[usize::from(c.caster)], c.spell, c.level, c.group, c.special, c.ability, c.elements, c.mystic
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The two per-move wrappers a capture-class module calls: `FUN_801DD4B0`
/// respects the party's Jewels, `FUN_801DD6B4` does not. Each against the
/// World's wrapper roll and against the module path's own.
#[test]
fn the_two_wrappers_roll_what_retail_rolls() {
    use legaia_engine_cast::cast_module_ticks::{CastDamageShape, CastWrapper};
    static POWERS: [u16; 5] = [0x30, 0x40, 0x80, 0x100, 0x400];
    let state = Rc::new(Cell::new(0));
    let Some((mut cpu, mut w)) = rig(&state) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut rng = Rng(0x5752_4150);
    let mut failures = Vec::new();
    for round in 0..40000 {
        let mut c = gen_cast(&mut rng);
        c.target = rng.below(3) as u8;
        let site = rng.below(5) as usize;
        let power = POWERS[site];
        let bypass = rng.below(2) == 0;
        let entry = if bypass { 0x801D_D6B4 } else { 0x801D_D4B0 };
        seed_retail(&mut cpu, &c);
        state.set(c.seed);
        let stop = cpu.call(
            entry,
            &[u32::from(power), u32::from(c.attacker), u32::from(c.target)],
        );
        assert_eq!(stop, Stop::Returned, "round {round}: {c:?}");
        let t = actor(c.target);
        let retail = Outcome {
            net: cpu.regs.r[V0] as i32,
            spirit: cpu.mem.rd16(t + 0x170),
            status: cpu.mem.rd16(t + 0x16E),
            xp: 0,
            rng: state.get(),
        };
        let stoned = c.seats[usize::from(c.target)].status & 4 != 0;
        // The World's roll, then the fold that lands it.
        seed_port(&mut w, &c);
        let i32_power = i32::from(power);
        let amount = if bypass {
            w.capture_bypass_predamage(c.attacker, c.target, i32_power)
        } else {
            w.capture_respect_predamage(c.attacker, c.target, i32_power)
        }
        .expect("rolled");
        land(&mut w, c.target, amount);
        let port = Outcome {
            net: match amount {
                u16::MAX => retail.net.min(-1),
                _ if stoned => 0,
                _ => i32::from(amount),
            },
            spirit: w.actors[usize::from(c.target)].battle.spirit_gauge,
            status: w.raw_status_word(c.target),
            xp: 0,
            rng: w.rng_state,
        };
        // The module path: its finisher has spent the Spirit and the strip.
        seed_port(&mut w, &c);
        let shape = CastDamageShape {
            prot_entry: 0,
            routine: 0,
            wrapper: if bypass {
                CastWrapper::Bypass
            } else {
                CastWrapper::Respect
            },
            never_kills: false,
            powers: &POWERS,
        };
        let net = w
            .capture_module_roll_site(&shape, site, c.attacker, c.target)
            .expect("rolled");
        let module = Outcome {
            net,
            spirit: w.actors[usize::from(c.target)].battle.spirit_gauge,
            status: w.raw_status_word(c.target),
            xp: 0,
            rng: w.rng_state,
        };
        if (retail != port || retail != module) && failures.len() < 8 {
            failures.push(format!(
                "round {round} bypass {bypass} power {power}:\n retail {retail:?}\n port   {port:?}\n module {module:?}\n a {} {:?} t {} {:?}\n {:x?} el {:?}",
                c.attacker, c.seats[usize::from(c.attacker)], c.target, c.seats[usize::from(c.target)], c.ability, c.elements
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Slot-B cast / summon modules' link base: every module image loads here.
const SLOT_B_BASE: u32 = 0x801F_69D8;
const RESPECT_WRAPPER: u32 = 0x801D_D4B0;
const BYPASS_WRAPPER: u32 = 0x801D_D6B4;

/// What a capture-class module's damage site hands its wrapper, read off the
/// module image itself: the `jal` at `site` (whose target names the wrapper)
/// and the `addiu a0, zero, imm` that loads its power - the delay slot's when
/// it writes `a0`, else the nearest one before the call.
fn module_site(image: &[u8], site: u32) -> Option<(u32, u16)> {
    let word = |va: u32| -> Option<u32> {
        let o = va.checked_sub(SLOT_B_BASE)? as usize;
        Some(u32::from_le_bytes(image.get(o..o + 4)?.try_into().ok()?))
    };
    let jal = word(site)?;
    if jal >> 26 != 3 {
        return None;
    }
    let target = 0x8000_0000 | ((jal & 0x03FF_FFFF) << 2);
    // `addiu a0, zero, imm`.
    let loads_a0 = |w: u32| w >> 16 == 0x2404;
    let power = std::iter::once(site + 4)
        .chain((1..=32).map(|k| site - 4 * k))
        .filter_map(word)
        .find(|&w| loads_a0(w))?;
    Some((target, power as u16))
}

/// A monster's capture-class special end to end: the World's roll for the
/// move id ([`World::enemy_move_predamage`], which routes it to a module's
/// baked power and wrapper) against retail's wrapper called the way the
/// module's own damage site calls it - the wrapper its `jal` names, the power
/// its `a0` immediate holds. Party victims stand in the Spirit stance half the
/// time, the case the per-move wrappers double the defender roll on and the
/// finisher halves.
#[test]
fn a_capture_special_rolls_what_its_module_site_rolls() {
    use legaia_engine_cast::cast_module_ticks::CAPTURE_SITE_POWERS;
    let state = Rc::new(Cell::new(0));
    let Some((mut cpu, mut w)) = rig(&state) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let disc = disc().expect("disc");
    w.install_menu_text(&crate::common::scus().expect("SCUS"));
    // Every move id the World pages one of the site table's modules for.
    let mut cases = Vec::new();
    for &(entry, ids, power, site) in CAPTURE_SITE_POWERS {
        let image = disc.read_entry(entry as usize).expect("module image");
        let (wrapper, retail_power) = module_site(&image, site)
            .unwrap_or_else(|| panic!("PROT {entry} {site:#x}: no wrapper call"));
        assert!(
            wrapper == RESPECT_WRAPPER || wrapper == BYPASS_WRAPPER,
            "PROT {entry} {site:#x}: jal {wrapper:#x}"
        );
        assert_eq!(retail_power, power, "PROT {entry} {site:#x}: a0");
        for id in 1..=0xFFu8 {
            if w.cast_module_for(id) == Some(entry) && (ids.is_empty() || ids.contains(&id)) {
                cases.push((id, entry, wrapper, retail_power));
            }
        }
    }
    let mut seen: Vec<u32> = cases.iter().map(|c| c.1).collect();
    seen.dedup();
    // PROT 0961's evolved-Cort roll is the one site the World reaches only in
    // that formation (`World::dead_end_crisis_wipes`): elsewhere a flat wipe.
    cases.retain(|c| c.1 != 961);
    assert!(cases.len() >= 12, "{} cases over {seen:?}", cases.len());
    assert!(cases.iter().any(|c| c.0 == 0xB1), "Rogue Thunder");

    let mut rng = Rng(0x4341_5054);
    let mut failures = Vec::new();
    let mut guarded = 0;
    for round in 0..20000 {
        let mut c = gen_cast(&mut rng);
        c.target = rng.below(3) as u8;
        c.seats[usize::from(c.target)].guarding = rng.below(2) == 0;
        guarded += u32::from(c.seats[usize::from(c.target)].guarding);
        let (id, entry, wrapper, power) = cases[rng.below(cases.len() as u32) as usize];
        seed_retail(&mut cpu, &c);
        state.set(c.seed);
        let stop = cpu.call(
            wrapper,
            &[u32::from(power), u32::from(c.attacker), u32::from(c.target)],
        );
        assert_eq!(stop, Stop::Returned, "round {round}: {c:?}");
        let t = actor(c.target);
        let retail = Outcome {
            net: cpu.regs.r[V0] as i32,
            spirit: cpu.mem.rd16(t + 0x170),
            status: cpu.mem.rd16(t + 0x16E),
            xp: 0,
            rng: state.get(),
        };
        seed_port(&mut w, &c);
        // The table's scalar is not what the module rolls with; a seed the
        // routing failed to replace shows as a wrong figure.
        let amount = w
            .enemy_move_predamage(c.attacker, c.target, 1, id)
            .expect("rolled");
        land(&mut w, c.target, amount);
        let port = Outcome {
            net: match amount {
                u16::MAX => retail.net.min(-1),
                _ if c.seats[usize::from(c.target)].status & 4 != 0 => 0,
                _ => i32::from(amount),
            },
            spirit: w.actors[usize::from(c.target)].battle.spirit_gauge,
            status: w.raw_status_word(c.target),
            xp: 0,
            rng: w.rng_state,
        };
        if retail != port && failures.len() < 8 {
            failures.push(format!(
                "round {round} move {id:#x} PROT {entry} jal {wrapper:#x} a0 {power:#x}:\n retail {retail:?}\n port   {port:?}\n a {} {:?} t {} {:?}\n {:x?} el {:?}",
                c.attacker, c.seats[usize::from(c.attacker)], c.target, c.seats[usize::from(c.target)], c.ability, c.elements
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert!(guarded > 5000, "{guarded}");
}
