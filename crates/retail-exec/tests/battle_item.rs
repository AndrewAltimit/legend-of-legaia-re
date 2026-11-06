//! `FUN_800402F4`, the item applier, on its battle path, against
//! `World::apply_battle_item`: every item the battle menu offers, over the
//! whole actor state the arms write.

use crate::common::{Rng, scus, scus_cpu};
use legaia_engine_battle_vm::status_effects::StatusKind;
use legaia_engine_core::world::World;
use legaia_engine_menus::items::ItemCatalog;
use legaia_game_tables::item_effect::ItemEffectTable;
use legaia_retail_exec::{Cpu, Stop, V0};
use legaia_save::Party;
use std::cell::Cell;
use std::rc::Rc;

const APPLIER: u32 = 0x8004_02F4;
const RAND: u32 = 0x8005_6798;
const CUE_GROUP: u32 = 0x801E_22C8;
const CLUT_RESTORE: u32 = 0x8005_133C;
const MODE: u32 = 0x8007_B83C;
const CTX_PTR: u32 = 0x8007_BD24;
const ACTORS: u32 = 0x801C_9370;
const SEATS: u32 = 0x8007_BD10;
const CTX: u32 = 0x8012_0000;
const ACTOR: u32 = 0x8013_0000;
const STRIDE: u32 = 0x400;

/// One seat's state, on both sides.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct Seat {
    hp: u16,
    max_hp: u16,
    mp: u16,
    max_mp: u16,
    gauge: u16,
    gauge_base: u16,
    atk: (u16, u16),
    udf: (u16, u16),
    ldf: (u16, u16),
    spd: (u16, u16),
    int: (u16, u16),
    status: u16,
    shield: u8,
    render: u8,
    pending: i32,
    mp_show: u16,
}

fn gen_seat(rng: &mut Rng) -> Seat {
    let top = [8, 40, 400, 9999, 8][rng.below(5) as usize];
    let max_hp = 8 + rng.below(top - 7) as u16;
    let hp = match rng.below(4) {
        0 => 0,
        1 => max_hp,
        2 => 1,
        _ => rng.below(u32::from(max_hp) + 1) as u16,
    };
    let max_mp = rng.below(1000) as u16;
    let mp = match rng.below(3) {
        0 => max_mp,
        1 => 0,
        _ => rng.below(u32::from(max_mp) + 1) as u16,
    };
    let stat = |rng: &mut Rng| -> (u16, u16) {
        match rng.below(6) {
            0 => (0xFFFF, 0xFFFF),
            1 => (0xFFFE, 5),
            2 => (4, 0xFFFF),
            _ => (rng.below(2000) as u16, rng.below(2000) as u16),
        }
    };
    Seat {
        hp,
        max_hp,
        mp,
        max_mp,
        gauge: rng.below(0x120) as u16,
        gauge_base: rng.below(0x120) as u16,
        atk: stat(rng),
        udf: stat(rng),
        ldf: stat(rng),
        spd: stat(rng),
        int: stat(rng),
        status: match rng.below(4) {
            0 => [1, 2, 3, 4, 8, 0x10, 0x20, 0x3C, 0x80, 0x400, 0x1000, 0x1403]
                [rng.below(12) as usize],
            _ => 0,
        },
        shield: (rng.below(4) == 0) as u8,
        render: 0,
        pending: 0,
        mp_show: 0,
    }
}

fn actor(slot: u8) -> u32 {
    ACTOR + u32::from(slot) * STRIDE
}

fn seed_retail(cpu: &mut Cpu, seats: &[Seat; 5], caster: u8) {
    cpu.mem.wr32(CTX_PTR, CTX);
    for b in 0..0x900 {
        cpu.mem.wr8(CTX + b, 0);
    }
    cpu.mem.wr8(CTX, 3);
    cpu.mem.wr8(CTX + 0x13, caster);
    cpu.mem.wr16(MODE, 0x15);
    for s in 0..8u8 {
        let a = actor(s);
        cpu.mem.wr32(ACTORS + u32::from(s) * 4, a);
        for b in 0..STRIDE {
            cpu.mem.wr8(a + b, 0);
        }
        cpu.mem
            .wr8(SEATS + u32::from(s), if s < 3 { s + 1 } else { 4 });
        let Some(st) = seats.get(usize::from(s)) else {
            continue;
        };
        cpu.mem.wr32(a + 0x10, st.pending as u32);
        cpu.mem.wr16(a + 0x14C, st.hp);
        cpu.mem.wr16(a + 0x14E, st.max_hp);
        cpu.mem.wr16(a + 0x150, st.mp);
        cpu.mem.wr16(a + 0x152, st.max_mp);
        cpu.mem.wr16(a + 0x154, st.gauge);
        cpu.mem.wr16(a + 0x156, st.gauge_base);
        for (off, p) in [
            (0x158, st.atk),
            (0x15C, st.udf),
            (0x160, st.ldf),
            (0x164, st.spd),
            (0x168, st.int),
        ] {
            cpu.mem.wr16(a + off, p.0);
            cpu.mem.wr16(a + off + 2, p.1);
        }
        cpu.mem.wr16(a + 0x16E, st.status);
        cpu.mem.wr8(a + 0x1F9, st.shield);
    }
}

fn read_retail(cpu: &mut Cpu, slot: u8) -> Seat {
    let a = actor(slot);
    let pair = |cpu: &mut Cpu, off: u32| (cpu.mem.rd16(a + off), cpu.mem.rd16(a + off + 2));
    Seat {
        hp: cpu.mem.rd16(a + 0x14C),
        max_hp: cpu.mem.rd16(a + 0x14E),
        mp: cpu.mem.rd16(a + 0x150),
        max_mp: cpu.mem.rd16(a + 0x152),
        gauge: cpu.mem.rd16(a + 0x154),
        gauge_base: cpu.mem.rd16(a + 0x156),
        atk: pair(cpu, 0x158),
        udf: pair(cpu, 0x15C),
        ldf: pair(cpu, 0x160),
        spd: pair(cpu, 0x164),
        int: pair(cpu, 0x168),
        status: cpu.mem.rd16(a + 0x16E),
        shield: cpu.mem.rd8(a + 0x1F9),
        render: cpu.mem.rd8(a + 0x21C),
        pending: cpu.mem.rd32(a + 0x10) as i32,
        mp_show: cpu.mem.rd16(a + 0x178),
    }
}

fn seed_port(w: &mut World, seats: &[Seat; 5], caster: u8, typed: bool, rng_seed: u32) {
    w.load_party(Party::zeroed(3));
    w.enter_battle(3, 2);
    w.party.party_count = 3;
    w.battle.status_effects = Default::default();
    w.battle_ctx.active_actor = caster;
    w.tables.character_max_mp = seats[..3].iter().map(|s| s.max_mp).collect();
    for (i, st) in seats.iter().enumerate() {
        let slot = i as u8;
        w.battle.attack[i] = st.atk.0;
        w.battle.attack_base[i] = st.atk.1;
        w.battle.defense_split[i] = Some((st.udf.0, st.ldf.0));
        w.battle.defense_base[i] = Some((st.udf.1, st.ldf.1));
        w.battle.speed[i] = st.spd.0;
        w.battle.speed_base[i] = st.spd.1;
        w.battle.accuracy[i] = st.int.0;
        w.battle.accuracy_base[i] = st.int.1;
        w.battle.evasion[i] = st.int.0;
        let a = &mut w.actors[i].battle;
        a.hp = st.hp;
        a.max_hp = st.max_hp;
        a.liveness = u16::from(st.hp != 0);
        a.mp = st.mp;
        a.agl = st.gauge;
        a.agl_base = st.gauge_base;
        a.spirit_shield = st.shield;
        a.render_flag = 0;
        a.last_mp_cost = 0;
        a.hp_display = Some(st.hp);
        a.hp_bar_pending = st.pending;
        a.field_flags = 0;
        if typed {
            // A status a hit landed lives in the tracker, one a cast wrote in
            // the raw word; both are one word in retail.
            for kind in [StatusKind::Venom, StatusKind::Toxic] {
                if st.status & kind.display_bit() != 0 {
                    w.battle.status_effects.apply(slot, kind);
                }
            }
            let tracked = w.battle.status_effects.display_flags(slot);
            w.actors[i].battle.field_flags = st.status & !tracked;
        } else {
            a.field_flags = st.status;
        }
    }
    w.rng_state = rng_seed;
}

fn read_port(w: &World, slot: u8, max_mp: u16) -> Seat {
    let i = usize::from(slot);
    let a = &w.actors[i].battle;
    let split = w.battle.defense_split[i].unwrap_or_default();
    let base = w.battle.defense_base[i].unwrap_or_default();
    Seat {
        hp: a.hp,
        max_hp: a.max_hp,
        mp: a.mp,
        max_mp: w.tables.character_max_mp.get(i).copied().unwrap_or(max_mp),
        gauge: a.agl,
        gauge_base: a.agl_base,
        atk: (w.battle.attack[i], w.battle.attack_base[i]),
        udf: (split.0, base.0),
        ldf: (split.1, base.1),
        spd: (w.battle.speed[i], w.battle.speed_base[i]),
        int: (w.battle.accuracy[i], w.battle.accuracy_base[i]),
        status: w.raw_status_word(slot),
        shield: a.spirit_shield,
        render: a.render_flag,
        pending: a.hp_bar_pending,
        mp_show: a.last_mp_cost,
    }
}

#[test]
fn every_battle_item_writes_what_retail_writes() {
    let (Some(mut cpu), Some(scus)) = (scus_cpu(), scus()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let state = Rc::new(Cell::new(0u32));
    let stream = state.clone();
    cpu.hook(RAND, move |r, _| {
        let next = stream.get().wrapping_mul(0x41C6_4E6D).wrapping_add(0x3039);
        stream.set(next);
        r.r[V0] = (next >> 16) & 0x7FFF;
    });
    cpu.stub(CUE_GROUP, 0);
    cpu.stub(CLUT_RESTORE, 0);
    let table = ItemEffectTable::from_scus(&scus).expect("item effects");
    let mut w = World::new();
    w.set_item_effects(table.clone());
    w.set_item_catalog(ItemCatalog::vanilla());
    let mut rng = Rng(0x4249_5445);
    let mut failures: Vec<String> = Vec::new();
    let mut per_class = [0u32; 256];
    for id in 0..=255u8 {
        let Some(e) = table.effect(id) else { continue };
        // The Point Card has its own test: state `0x3E` rewrites its class.
        if table.kind(id) != 2 || e.flags & 0x04 == 0 || e.class > 8 || id == 0xFE {
            continue;
        }
        let mut item_failed = false;
        for round in 0..400 {
            let mut seats: [Seat; 5] = std::array::from_fn(|_| gen_seat(&mut rng));
            // A readout still running: the arms assign over it, not add.
            if rng.below(4) == 0 {
                seats[0].pending = -(rng.below(500) as i32);
                seats[1].pending = rng.below(500) as i32;
            }
            // A monster acts through this routine too; the revive arm reads it.
            let caster = rng.below(5) as u8;
            let target = rng.below(3) as u8;
            let seed = rng.next();
            seed_retail(&mut cpu, &seats, caster);
            state.set(seed);
            let stop = cpu.call(
                APPLIER,
                &[
                    u32::from(e.class),
                    u32::from(e.tier),
                    u32::from(target),
                    u32::from(caster),
                ],
            );
            assert_eq!(stop, Stop::Returned, "item {id:#04x} {seats:?}");
            let typed = rng.below(2) == 0;
            seed_port(&mut w, &seats, caster, typed, seed);
            if e.flags & 0x20 != 0 {
                for m in 0..3u8 {
                    w.apply_battle_item(id, m);
                }
            } else {
                w.apply_battle_item(id, target);
            }
            per_class[usize::from(e.class)] += 1;
            for s in 0..5u8 {
                let mut r = read_retail(&mut cpu, s);
                // The fade `+0x21C = 2` is `stand_revived_party_member` here.
                r.render = 0;
                let p = read_port(&w, s, seats[usize::from(s)].max_mp);
                if r != p {
                    failures.push(format!(
                        "item {id:#04x} class {} tier {} round {round} target {target} seat {s}:\n before {:?}\n retail {r:?}\n port   {p:?}",
                        e.class,
                        e.tier,
                        seats[usize::from(s)]
                    ));
                    item_failed = true;
                    break;
                }
            }
            if !item_failed && state.get() != w.rng_state {
                failures.push(format!("item {id:#04x} class {}: rand stream", e.class));
                item_failed = true;
            }
            if item_failed {
                break;
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    for class in [0, 1, 2, 3, 4, 5, 7, 8] {
        assert!(
            per_class[class] > 100,
            "class {class}: {}",
            per_class[class]
        );
    }
}

const BANK: u32 = 0x8008_45B4;
const POP: u32 = 0x801F_44A0;

/// Selector `0x0E`: the Point Card, which the action SM reaches by item id.
/// The bank's boundaries are the arm's: empty, one spend, the `9999` clamp.
#[test]
fn the_point_card_discharges_what_retail_discharges() {
    use legaia_actor_formats::monster_archive::{MonsterAnimation, PartPose};
    let (Some(mut cpu), Some(scus)) = (scus_cpu(), scus()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let popped = Rc::new(Cell::new(u32::MAX));
    let pop = popped.clone();
    cpu.hook(POP, move |r, _| pop.set(r.r[legaia_retail_exec::A0]));
    let table = ItemEffectTable::from_scus(&scus).expect("item effects");
    let mut w = World::new();
    w.set_item_effects(table);
    w.set_item_catalog(ItemCatalog::vanilla());
    assert!(
        w.tables
            .item_catalog
            .get(0xFE)
            .is_some_and(|e| e.usable_in_battle),
        "the Point Card is offered in battle"
    );
    let clip = |action_id: u8| MonsterAnimation {
        variant_track: Default::default(),
        action_id,
        rate: 2,
        attach_key: 0,
        solo_flag: 0,
        impact_class: 0,
        effect_script: Vec::new(),
        part_count: 1,
        frame_count: 2,
        frames: vec![vec![PartPose::default()]; 2],
    };
    let mut rng = Rng(0x5043_4152);
    let mut seen_clamp = 0;
    for round in 0..4000 {
        let seats: [Seat; 5] = std::array::from_fn(|_| gen_seat(&mut rng));
        let target = 3 + rng.below(2) as u8;
        let bank = [0, 1, 9998, 9999, 10000, 123_456, rng.below(20000)][rng.below(7) as usize];
        let has_getup = rng.below(2) == 0;
        let tags: Vec<u8> = if has_getup {
            vec![0, 2, 4, 5, 0x0B]
        } else {
            vec![0, 2, 4, 0x0B]
        };
        let map = legaia_engine_battle_loop::clip_ladder::monster_reaction_map(&tags);
        seed_retail(&mut cpu, &seats, 0);
        let t = actor(target);
        cpu.mem.wr32(BANK, bank);
        for (k, e) in map.into_iter().enumerate() {
            cpu.mem.wr8(t + 0x1EF + k as u32, e);
        }
        popped.set(u32::MAX);
        let stop = cpu.call(APPLIER, &[0x0E, 0, u32::from(target), 0]);
        assert_eq!(stop, Stop::Returned, "round {round}");
        seed_port(&mut w, &seats, 0, false, 0);
        w.minigames.point_card = bank as i32;
        w.actors[usize::from(target)].battle_monster_id = Some(0x40);
        let clips: Vec<_> = tags.iter().map(|&x| Some(clip(x))).collect();
        w.set_actor_battle_action_clips(usize::from(target), std::sync::Arc::new(clips));
        let commits = w.actors[usize::from(target)].battle_clip_commits;
        w.apply_battle_item(0xFE, target);
        let a = &w.actors[usize::from(target)];
        let retail = (
            cpu.mem.rd32(BANK),
            cpu.mem.rd16(t + 0x14C),
            cpu.mem.rd8(t + 0x1DA),
        );
        let staged = if a.battle_clip_commits != commits {
            a.battle_reaction_entry.unwrap_or(0)
        } else {
            0
        };
        let port = (w.minigames.point_card as u32, a.battle.hp, staged);
        assert_eq!(
            retail,
            port,
            "round {round}: bank {bank} getup {has_getup} {:?}",
            seats[usize::from(target)]
        );
        if bank > 9999 {
            seen_clamp += 1;
            assert_eq!(popped.get(), 9999);
        }
    }
    assert!(seen_clamp > 100);
}

const BAG_ADD: u32 = 0x8004_21D4;

/// Selectors 9 and 10, the opposed INT rolls, single target and the all-party
/// loop (`target == 8`), with the Stone arm's refund and cancel.
#[test]
fn the_stone_and_curse_arms_roll_what_retail_rolls() {
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let state = Rc::new(Cell::new(0u32));
    let stream = state.clone();
    cpu.hook(RAND, move |r, _| {
        let next = stream.get().wrapping_mul(0x41C6_4E6D).wrapping_add(0x3039);
        stream.set(next);
        r.r[V0] = (next >> 16) & 0x7FFF;
    });
    let refunds = Rc::new(Cell::new(0u32));
    let refunded = refunds.clone();
    cpu.hook(BAG_ADD, move |r, _| {
        assert_eq!(r.r[legaia_retail_exec::A1], 1);
        refunded.set(refunded.get() + 1);
    });
    let mut w = World::new();
    let mut rng = Rng(0x5354_4F4E);
    let mut landed = 0;
    let mut missed = 0;
    let mut gave_back = 0;
    for round in 0..8000 {
        let mut seats: [Seat; 5] = std::array::from_fn(|_| gen_seat(&mut rng));
        // Every seat a game fields has INT; a zero sum traps on both arms' callers.
        for st in &mut seats {
            st.int.0 = [1, 2, 7, 500, 999, 1 + rng.below(2000) as u16][rng.below(6) as usize];
            st.status = [0, 0, 1, 4, 0x1000][rng.below(5) as usize];
        }
        let caster = 3 + rng.below(2) as u8;
        let selector = 9 + rng.below(2);
        let all = rng.below(3) == 0;
        let target = rng.below(3) as u8;
        let category: [u8; 3] = std::array::from_fn(|_| [0, 1, 1, 2, 4][rng.below(5) as usize]);
        let key: [u16; 3] = std::array::from_fn(|_| [0, 40][rng.below(2) as usize]);
        seed_retail(&mut cpu, &seats, caster);
        for m in 0..3u8 {
            let a = actor(m);
            cpu.mem.wr8(a + 0x1DE, category[usize::from(m)]);
            cpu.mem.wr16(a + 0x16C, key[usize::from(m)]);
            cpu.mem.wr8(a + 0x1DF, 0x77);
        }
        let seed = rng.next();
        state.set(seed);
        refunds.set(0);
        let arg = if all { 8 } else { u32::from(target) };
        let stop = cpu.call(APPLIER, &[selector, 0, arg, 0]);
        assert_eq!(stop, Stop::Returned, "round {round}");

        seed_port(&mut w, &seats, caster, false, seed);
        w.party.character_ability_bits = Default::default();
        w.party.inventory.clear();
        for m in 0..3 {
            let a = &mut w.actors[m].battle;
            a.action_category = category[m];
            a.init_key = key[m];
            a.params[0] = 0x77;
        }
        let list: Vec<u8> = if all { vec![0, 1, 2] } else { vec![target] };
        let kind = if selector == 9 {
            legaia_engine_battle_vm::status_effects::StatusKind::Stone
        } else {
            legaia_engine_battle_vm::status_effects::StatusKind::Curse
        };
        w.roll_status_class(caster, 0, kind, &list, false);
        for m in 0..3u8 {
            let i = usize::from(m);
            let a = actor(m);
            let retail = (cpu.mem.rd16(a + 0x16E), cpu.mem.rd8(a + 0x1DE));
            let port = (w.raw_status_word(m), w.actors[i].battle.action_category);
            assert_eq!(
                retail, port,
                "round {round} selector {selector} all {all} target {target} seat {m}: {:?}",
                seats[i]
            );
            if retail.0 != seats[i].status {
                landed += 1;
            } else {
                missed += 1;
            }
        }
        assert_eq!(state.get(), w.rng_state, "round {round} rand");
        let bag = w.party.inventory.get(&0x77).copied().unwrap_or(0);
        assert_eq!(refunds.get(), u32::from(bag), "round {round} refund");
        gave_back += refunds.get();
    }
    assert!(
        landed > 500 && missed > 500 && gave_back > 100,
        "{landed} {missed} {gave_back}"
    );
}

/// State `0x3F`'s applier call, driven through the band: the card's descriptor
/// reads class 1, whose arm serves the whole party, but the band runs selector
/// `0x0E` on the one target byte (`0x801E40C0`, `0x801E4134`).
#[test]
fn the_point_card_strikes_its_target_and_not_the_party() {
    use legaia_engine_battle_vm::battle_action::{ActionCategory, ActionState};
    let Some(scus) = scus() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut w = World::new();
    w.set_item_effects(ItemEffectTable::from_scus(&scus).expect("item effects"));
    w.set_item_catalog(ItemCatalog::vanilla());
    w.load_party(Party::zeroed(3));
    w.enter_battle(3, 1);
    w.party.party_count = 3;
    for a in w.actors.iter_mut().take(3) {
        a.battle.hp = 900;
        a.battle.max_hp = 900;
    }
    w.actors[3].battle.hp = 5000;
    w.actors[3].battle.max_hp = 5000;
    w.minigames.point_card = 1200;
    let a = &mut w.actors[0].battle;
    a.action_category = ActionCategory::Item.as_byte();
    a.params[0] = 0xFE;
    a.active_target = 3;
    a.cast_class = 0x0E;
    w.battle_ctx.active_actor = 0;
    w.battle_ctx.action_state = ActionState::SpiritFireDamage.as_byte();
    w.battle_ctx.frame_timer = 1;
    w.step_battle();
    assert_eq!(w.actors[3].battle.hp, 3800);
    for m in 0..3 {
        assert_eq!(w.actors[m].battle.hp, 900, "member {m}");
    }
    assert_eq!(w.minigames.point_card, 0);
}
