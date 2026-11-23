//! The victory arm of `FUN_8004E568`, the spoils of a won fight, against
//! `World::apply_battle_loot`: the gold and EXP of the seats that died and were
//! not captured, the Golden Book and doubled-EXP bits, the EXP split over the
//! members standing and not Stoned, the drop roll, the purse and EXP caps, and
//! the level-ups `FUN_801E9504` takes from it on one `rand()` stream.

use crate::common::{Rng, disc, scus, scus_cpu, seat_status};
use legaia_engine_battle::accessory_passives::AccessoryPassives;
use legaia_engine_battle::monster_catalog::{
    FormationDef, FormationSlot, MonsterCatalog, MonsterDef,
};
use legaia_engine_core::world::World;
use legaia_game_tables::accessory_passive::AccessoryPassiveTable;
use legaia_save::{CharacterRecord, Party};
use psx_retail_exec::{Cpu, GP, Stop, V0};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

const SPOILS: u32 = 0x8004_E568;
const RAND: u32 = 0x8005_6798;
const OVERLAY_BASE: u32 = 0x801C_E818;
const CTX: u32 = 0x8012_0000;
const CTX_PTR: u32 = 0x8007_BD24;
const ACTORS: u32 = 0x801C_9370;
const MONSTER_RECORDS: u32 = 0x801C_9348;
const SEATS: u32 = 0x8007_BD10;
const RECORDS: u32 = 0x8008_4708;
const STRIDE: u32 = 0x414;
const PURSE: u32 = 0x8008_459C;
const SPECIAL_WORD: u32 = 0x8007_BAC0;
const ACTOR: u32 = 0x8013_0000;
const ACTOR_STRIDE: u32 = 0x400;
const MONSTER: u32 = 0x8015_0000;

#[derive(Debug, Clone)]
struct Win {
    party: usize,
    monsters: usize,
    member_hp: [u16; 3],
    member_status: [u16; 3],
    word1: [u32; 3],
    word0: [u32; 3],
    xp: [u32; 3],
    level: [u8; 3],
    dead: [bool; 5],
    captured: [bool; 5],
    gold: [u16; 5],
    exp: [u16; 5],
    item: [u8; 5],
    chance: [u8; 5],
    purse: u32,
    special: u32,
    /// The pose seat `ctx[+0x13]`, whose character id seats the results
    /// camera.
    pose_seat: u8,
    seed: u32,
    /// Each member's committed item `+0x1DF` under category Item `+0x1DE`,
    /// and its initiative key `+0x16C` (non-zero: the turn has not run).
    queued: [Option<u8>; 3],
    init_key: [u16; 3],
    /// An item the bag already holds 99 of, which a drop skips
    /// (`0x8004F5A8..0x8004F5BC`).
    full: Option<u8>,
}

fn gen_win(rng: &mut Rng, threshold: impl Fn(usize, u8) -> u32) -> Win {
    let mut w = Win {
        party: 1 + rng.below(3) as usize,
        monsters: 1 + rng.below(5) as usize,
        member_hp: [0; 3],
        member_status: [0; 3],
        word1: [0; 3],
        word0: [0; 3],
        xp: [0; 3],
        level: [1; 3],
        dead: [true; 5],
        captured: [false; 5],
        gold: [0; 5],
        exp: [0; 5],
        item: [0; 5],
        chance: [0; 5],
        purse: match rng.below(4) {
            0 => 99_999_999 - rng.below(40),
            _ => rng.below(100_000),
        },
        special: if rng.below(10) == 0 { 0x200 } else { 0 },
        pose_seat: 0,
        seed: rng.next(),
        queued: [None; 3],
        init_key: [0; 3],
        full: None,
    };
    w.pose_seat = rng.below(w.party as u32) as u8;
    let mut standing = 0;
    for m in 0..3 {
        w.member_hp[m] = if rng.below(4) == 0 {
            0
        } else {
            1 + rng.below(999) as u16
        };
        w.member_status[m] = if rng.below(5) == 0 { 4 } else { 0 };
        if m < w.party && w.member_hp[m] != 0 && w.member_status[m] == 0 {
            standing += 1;
        }
        // `0x1_0000` Gold Boost, `0x2_0000` Items Up, `0x8000` EXP Boost.
        w.word1[m] = [0, 0, 0x1_0000, 0x2_0000, 0x8000, 0x1_8000][rng.below(6) as usize];
        w.word0[m] = if rng.below(4) == 0 { 0x8000 } else { 0 };
        w.level[m] = 1 + rng.below(90) as u8;
        // Close to the next level, or at the EXP cap's edge.
        w.xp[m] = match rng.below(8) {
            0 => {
                w.level[m] = 99;
                9_999_999 - rng.below(40)
            }
            _ => threshold(m, w.level[m]).saturating_sub(1 + rng.below(400)),
        };
    }
    // The routine divides by the members it pays: a win leaves one standing.
    if standing == 0 {
        w.member_hp[0] = 1 + rng.below(999) as u16;
        w.member_status[0] = 0;
    }
    for s in 0..5 {
        w.dead[s] = rng.below(5) != 0;
        w.captured[s] = rng.below(6) == 0;
        w.gold[s] = [0, 1, 2, 3, 0xFFFF, rng.below(2000) as u16][rng.below(6) as usize];
        w.exp[s] = [0, 1, 2, 3, 0xFFFF, rng.below(3000) as u16][rng.below(6) as usize];
        w.item[s] = if rng.below(3) == 0 {
            0
        } else {
            1 + rng.below(100) as u8
        };
        w.chance[s] = [0, 1, 50, 99, 100, rng.below(100) as u8][rng.below(6) as usize];
    }
    for m in 0..3 {
        w.queued[m] = (rng.below(3) == 0).then(|| 1 + rng.below(100) as u8);
        w.init_key[m] = [0, 0, 1, 0x1234][rng.below(4) as usize];
    }
    if rng.below(4) == 0 {
        w.full = Some(w.item[rng.below(5) as usize]).filter(|&i| i != 0);
    }
    w
}

fn seed_retail(cpu: &mut Cpu, w: &Win) {
    let gp = cpu.regs.r[GP];
    cpu.mem.wr32(CTX_PTR, CTX);
    cpu.mem.wr32(gp + 0xA0C, CTX);
    for b in 0..0x900 {
        cpu.mem.wr8(CTX + b, 0);
    }
    cpu.mem.wr8(CTX, w.party as u8);
    cpu.mem.wr8(CTX + 1, w.monsters as u8);
    cpu.mem.wr8(CTX + 0x13, w.pose_seat);
    cpu.mem.wr16(CTX + 0x6D0, 0x0C00);
    cpu.mem.wr8(gp + 0x332, 0);
    // The arm that pays: state 5, the seat's line finished, first pass.
    cpu.mem.wr32(gp + 0xA14, 5);
    cpu.mem.wr8(gp + 0xA48, 0x80);
    cpu.mem.wr32(gp + 0x384, 0x8016_0000);
    cpu.mem.wr32(PURSE, w.purse);
    cpu.mem.wr32(SPECIAL_WORD, w.special);
    for b in 0..256u32 {
        cpu.mem.wr8(0x8008_5958 + b, 0);
    }
    // The bag's live slot range `gp+0x2D2 .. gp+0x2D4` over `(id, count)`
    // pairs at `0x80085958`.
    cpu.mem.wr16(gp + 0x2D2, 0);
    cpu.mem.wr16(gp + 0x2D4, u16::from(w.full.is_some()));
    if let Some(id) = w.full {
        cpu.mem.wr8(0x8008_5958, id);
        cpu.mem.wr8(0x8008_5959, 99);
    }
    for s in 0..8u32 {
        let a = ACTOR + s * ACTOR_STRIDE;
        cpu.mem.wr32(ACTORS + s * 4, a);
        for b in 0..ACTOR_STRIDE {
            cpu.mem.wr8(a + b, 0);
        }
    }
    for m in 0..w.party {
        let a = ACTOR + m as u32 * ACTOR_STRIDE;
        cpu.mem.wr8(SEATS + m as u32, m as u8 + 1);
        cpu.mem.wr16(a + 0x14C, w.member_hp[m]);
        cpu.mem.wr16(a + 0x16E, w.member_status[m]);
        cpu.mem.wr16(a + 0x16C, w.init_key[m]);
        match w.queued[m] {
            Some(id) => {
                cpu.mem.wr8(a + 0x1DE, 1);
                cpu.mem.wr8(a + 0x1DF, id);
            }
            None => cpu.mem.wr8(a + 0x1DE, 3),
        }
        let rec = RECORDS + m as u32 * STRIDE;
        for b in 0..STRIDE {
            cpu.mem.wr8(rec + b, 0);
        }
        cpu.mem.wr32(rec, w.xp[m]);
        cpu.mem.wr16(rec + 0x106, w.member_hp[m]);
        cpu.mem.wr16(rec + 0x11C, 1000);
        cpu.mem.wr16(rec + 0x11E, 100);
        cpu.mem.wr16(rec + 0x120, 100);
        cpu.mem.wr16(rec + 0x122, 100);
        for off in [0x124, 0x126, 0x128, 0x12A, 0x12C] {
            cpu.mem.wr16(rec + off, 100);
        }
        cpu.mem.wr8(rec + 0x130, w.level[m]);
        cpu.mem.wr32(rec + 0xF4, w.word0[m]);
        cpu.mem.wr32(rec + 0xF8, w.word1[m]);
    }
    for s in 0..w.monsters {
        let a = ACTOR + (3 + s as u32) * ACTOR_STRIDE;
        cpu.mem.wr16(a + 0x14C, u16::from(!w.dead[s]));
        cpu.mem.wr8(a + 0x227, u8::from(w.captured[s]));
        let r = MONSTER + s as u32 * 0x80;
        cpu.mem.wr32(MONSTER_RECORDS + s as u32 * 4, r);
        cpu.mem.wr16(r + 0x44, w.gold[s]);
        cpu.mem.wr16(r + 0x46, w.exp[s]);
        cpu.mem.wr8(r + 0x48, w.item[s]);
        cpu.mem.wr8(r + 0x49, w.chance[s]);
    }
}

fn seed_port(world: &mut World, w: &Win) -> (FormationDef, MonsterCatalog) {
    let mut party = Party::zeroed(w.party);
    for m in 0..w.party {
        let mut raw = vec![0u8; STRIDE as usize];
        raw[0..4].copy_from_slice(&w.xp[m].to_le_bytes());
        let mut put = |off: usize, v: u16| raw[off..off + 2].copy_from_slice(&v.to_le_bytes());
        put(0x106, w.member_hp[m]);
        put(0x11C, 1000);
        put(0x11E, 100);
        put(0x120, 100);
        put(0x122, 100);
        for off in [0x124, 0x126, 0x128, 0x12A, 0x12C] {
            put(off, 100);
        }
        raw[0x130] = w.level[m];
        raw[0xF4..0xF8].copy_from_slice(&w.word0[m].to_le_bytes());
        raw[0xF8..0xFC].copy_from_slice(&w.word1[m].to_le_bytes());
        party.members[m] = CharacterRecord::parse(&raw).unwrap();
    }
    world.load_party(party);
    world.party.party_count = w.party as u8;
    world.party.money = w.purse as i32;
    world.party.inventory.clear();
    world.seru.battle_captures.clear();
    world.battle.special_word = w.special;
    world.battle.status_effects = Default::default();
    let mut catalog = MonsterCatalog::new();
    let mut slots = Vec::new();
    for m in 0..w.party {
        world.party.level_up_tracker.level[m] = w.level[m];
        world.party.level_up_tracker.xp[m] = w.xp[m];
        let a = &mut world.actors[m];
        a.battle.hp = w.member_hp[m];
        a.battle_monster_id = None;
        a.battle.init_key = w.init_key[m];
        a.battle.action_category = if w.queued[m].is_some() { 1 } else { 3 };
        world.battle.round_flow.pending[m] =
            w.queued[m].map(
                |id| legaia_engine_battle_loop::round_flow::PendingPartyAction::Item {
                    item_id: id,
                    used_slots: vec![],
                },
            );
        seat_status(world, m as u8, w.member_status[m], w.seed >> m & 1 != 0);
    }
    for s in 0..w.monsters {
        let id = 0x40 + s as u16;
        let mut def = MonsterDef::new(id, "m", 10, 10);
        def.gold = w.gold[s];
        def.exp = w.exp[s];
        def.drop_item = Some(w.item[s]);
        def.drop_chance_pct = w.chance[s];
        catalog.insert(def);
        slots.push(FormationSlot::new(id));
        let a = &mut world.actors[w.party + s];
        a.battle.hp = u16::from(!w.dead[s]);
        a.battle_monster_id = Some(id);
        if w.captured[s] {
            world.seru.battle_captures.push(id);
        }
    }
    if let Some(id) = w.full {
        world.party.inventory.insert(id, 99);
    }
    world.rng_state = w.seed;
    (FormationDef::new(1000, slots), catalog)
}

#[test]
fn a_win_pays_what_retail_pays() {
    differential!(scus(SPOILS));
    let (Some(mut cpu), Some(scus), Some(disc)) = (scus_cpu(), scus(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem
        .load(OVERLAY_BASE, &disc.read_entry(898).expect("PROT 0898"));
    cpu.mem.wr32(0x8007_B81C, 0x8007_0A2C);
    let state = Rc::new(Cell::new(0u32));
    let stream = state.clone();
    cpu.hook(RAND, move |r, _| {
        let next = stream.get().wrapping_mul(0x41C6_4E6D).wrapping_add(0x3039);
        stream.set(next);
        r.r[V0] = (next >> 16) & 0x7FFF;
    });
    // The grant call, which is how the drop leaves the routine.
    let granted = Rc::new(RefCell::new(Vec::<u8>::new()));
    let log = granted.clone();
    cpu.hook(0x8004_21D4, move |r, _| log.borrow_mut().push(r.r[4] as u8));
    for stub in [
        0x8003_CE08u32, // The fanfare.
        0x8003_CBF8,    // The text pool.
        0x8003_CA38,
        0x8005_6758,
        0x8003_563C, // The results windows.
        0x8004_FCC8, // The voice.
        0x801D_5854, // The camera.
        0x801D_8DE8, // The banner.
    ] {
        cpu.stub(stub, 0);
    }
    let mut world = World::default();
    world.install_retail_progression_tables(&scus);
    world.set_accessory_passives(AccessoryPassives::from_disc(
        &AccessoryPassiveTable::from_scus(&scus).expect("passives"),
    ));
    let mut rng = Rng(0x534F_494C);
    let (mut failures, mut drops, mut refunds) = (Vec::new(), 0, 0);
    for round in 0..4000 {
        let tracker = &world.party.level_up_tracker;
        let w = gen_win(&mut rng, |m, level| {
            tracker.threshold_for(m, level).unwrap_or(0)
        });
        seed_retail(&mut cpu, &w);
        granted.borrow_mut().clear();
        state.set(w.seed);
        match cpu.call(SPOILS, &[]) {
            Stop::Returned => {}
            other => panic!("round {round}: {other:?}: {w:?}"),
        }
        let retail_xp: Vec<(u32, u8)> = (0..w.party as u32)
            .map(|m| {
                let rec = RECORDS + m * STRIDE;
                (cpu.mem.rd32(rec), cpu.mem.rd8(rec + 0x130))
            })
            .collect();
        // The results camera (`0x8004EF1C..0x8004EF4C`): the pose seat's
        // character id is its seat byte, `seat + 1` here.
        let (yaw, depth) = legaia_engine_battle::victory::victory_results_camera(w.pose_seat + 1);
        assert_eq!(
            (
                cpu.mem.rd16(CTX + 0x6DA) as i32,
                cpu.mem.rd16(CTX + 0x6D0) as i16
            ),
            (yaw, depth),
            "round {round}: results camera, pose seat {}",
            w.pose_seat
        );
        let retail = (
            cpu.mem.rd32(PURSE),
            retail_xp,
            granted.borrow().clone(),
            state.get(),
        );
        let (formation, catalog) = seed_port(&mut world, &w);
        // The refund runs in the per-member loop, ahead of the drops.
        world.drive_refund_unacted_items();
        let refunded: Vec<u8> = (0..w.party)
            .filter(|&m| world.battle.round_flow.pending[m].is_none())
            .filter_map(|m| w.queued[m])
            .collect();
        refunds += refunded.len();
        let r = world.apply_battle_loot(&formation, &catalog);
        let port_xp: Vec<(u32, u8)> = (0..w.party)
            .map(|m| {
                let raw = world.party.roster.members[m].write();
                (
                    u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]),
                    raw[0x130],
                )
            })
            .collect();
        let port = (
            world.party.money as u32,
            port_xp,
            [refunded, r.drops.clone()].concat(),
            world.rng_state,
        );
        drops += r.drops.len();
        if retail != port {
            failures.push(format!(
                "round {round}:\n retail {retail:?}\n port   {port:?}\n {w:?}"
            ));
            if failures.len() >= 6 {
                break;
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert!(
        drops > 100 && refunds > 100,
        "{drops} drops {refunds} refunds"
    );
}

/// The results hold (`gp+0xA54`, `0x8004F740..0x8004F778`): the results frame
/// zeroes it and adds the frame step on that same pass, and the exit fade
/// (`FUN_80024E80`) is spawned on the pass the count reaches `0x100`. Run
/// pass by pass from the results frame at a frame step of one vsync, against
/// `World::tick_battle_end_sequence` counted from the tick that opens the
/// results frame.
#[test]
fn the_results_hold_spawns_the_fade_when_retail_does() {
    differential!(scus(SPOILS));
    use legaia_engine_battle_vm::battle_action::BattleEndCause;
    use legaia_engine_core::world::test_drivers::BattleTestDrivers;
    let (Some(mut cpu), Some(scus), Some(disc)) = (scus_cpu(), scus(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem
        .load(OVERLAY_BASE, &disc.read_entry(898).expect("PROT 0898"));
    cpu.mem.wr32(0x8007_B81C, 0x8007_0A2C);
    let state = Rc::new(Cell::new(0u32));
    let stream = state.clone();
    cpu.hook(RAND, move |r, _| {
        let next = stream.get().wrapping_mul(0x41C6_4E6D).wrapping_add(0x3039);
        stream.set(next);
        r.r[V0] = (next >> 16) & 0x7FFF;
    });
    let faded = Rc::new(Cell::new(false));
    let f2 = faded.clone();
    cpu.hook(0x8002_4E80, move |_, _| f2.set(true));
    for stub in [
        0x8003_CE08u32,
        0x8003_CBF8,
        0x8003_CA38,
        0x8005_6758,
        0x8003_563C,
        0x8004_FCC8,
        0x801D_5854,
        0x801D_8DE8,
        0x8004_21D4,
        0x8002_5358,
        0x8001_7B94,
    ] {
        cpu.stub(stub, 0);
    }
    let mut world = World::default();
    world.install_retail_progression_tables(&scus);
    world.set_accessory_passives(AccessoryPassives::from_disc(
        &AccessoryPassiveTable::from_scus(&scus).expect("passives"),
    ));
    let mut rng = Rng(0x484F_4C44);
    for round in 0..20 {
        let tracker = &world.party.level_up_tracker;
        let w = gen_win(&mut rng, |m, level| {
            tracker.threshold_for(m, level).unwrap_or(0)
        });
        seed_retail(&mut cpu, &w);
        // The frame step, and the load state the win's fade waits on.
        cpu.mem.wr8(0x1F80_0393, 1);
        cpu.mem.wr8(CTX + 0xB, 0);
        cpu.mem.wr8(CTX + 0xC, 2);
        state.set(w.seed);
        faded.set(false);
        let mut retail = None;
        for pass in 0..0x200u32 {
            assert_eq!(
                cpu.call(SPOILS, &[]),
                Stop::Returned,
                "round {round} pass {pass}"
            );
            if faded.get() {
                retail = Some(pass);
                break;
            }
        }

        let (formation, catalog) = seed_port(&mut world, &w);
        world.battle.active_formation = Some(formation);
        world.tables.monster_catalog = catalog;
        world.battle.victory = None;
        world.presentation.fade = None;
        world.game_over_hold = false;
        world.battle.end = Some(BattleEndCause::MonsterWipe);
        world.begin_battle_end_sequence();
        let mut results_at = None;
        let mut port = None;
        for tick in 0..0x400u32 {
            world.tick_battle_end_sequence();
            let in_results = matches!(
                world.battle.victory.map(|v| v.phase),
                Some(legaia_engine_battle::victory::VictoryPhase::Results { .. })
            );
            if results_at.is_none() && in_results {
                results_at = Some(tick);
            }
            if world.presentation.fade.is_some() {
                port = results_at.map(|r| tick - r);
                break;
            }
        }
        assert_eq!(
            retail, port,
            "round {round}: passes from the results frame to the fade"
        );
    }
}

/// A win with only Terra standing (`docs/subsystems/level-up.md`, XP
/// distribution): seat byte `4` leaves the divisor loop
/// (`0x8004F130..0x8004F178`), so with Noa down the `divu` at `0x8004F198`
/// divides by zero. Run with the R3000's answer (`lo` all ones), the pass pays
/// Terra the all-ones share, which wraps her EXP down by one or caps an EXP
/// of `0` at 9,999,999, and parks the same word in `gp+0xA04` for the window.
/// Retail only; the port floors the divisor at one by the owner's choice.
#[test]
fn a_terra_only_win_pays_her_all_ones() {
    not_a_differential!("retail only: the port floors the Terra-only divisor at one by choice");
    let (Some(mut cpu), Some(disc)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem
        .load(OVERLAY_BASE, &disc.read_entry(898).expect("PROT 0898"));
    cpu.mem.wr32(0x8007_B81C, 0x8007_0A2C);
    cpu.hardware_zero_div = true;
    cpu.hook(RAND, |r, _| r.r[V0] = 0x1234);
    for stub in [
        0x8003_CE08u32,
        0x8003_CBF8,
        0x8003_CA38,
        0x8005_6758,
        0x8003_563C,
        0x8004_FCC8,
        0x801D_5854,
        0x801D_8DE8,
    ] {
        cpu.stub(stub, 0);
    }
    let terra_exp = RECORDS + 3 * STRIDE;
    for (before, after) in [(0u32, 9_999_999u32), (1234, 1233), (9_999_999, 9_999_998)] {
        let mut w = gen_win(&mut Rng(1), |_, _| 0);
        w.party = 2;
        w.monsters = 2;
        w.member_hp = [0, 200, 0];
        w.member_status = [0; 3];
        w.word0 = [0; 3];
        w.word1 = [0; 3];
        w.xp = [156, before, 0];
        w.level = [3, 1, 1];
        w.dead = [true; 5];
        w.captured = [false; 5];
        w.exp = [40, 0, 0, 0, 0];
        w.special = 0;
        seed_retail(&mut cpu, &w);
        // Seat Noa (2) and Terra (4) on their own record indices.
        let recs: Vec<Vec<u8>> = (0..2u32)
            .map(|seat| {
                (0..STRIDE)
                    .map(|b| cpu.mem.rd8(RECORDS + seat * STRIDE + b))
                    .collect()
            })
            .collect();
        for (seat, id) in [(0u32, 2u32), (1, 4)] {
            cpu.mem.wr8(SEATS + seat, id as u8);
            for (b, v) in recs[seat as usize].iter().enumerate() {
                cpu.mem.wr8(RECORDS + (id - 1) * STRIDE + b as u32, *v);
            }
        }
        let gp = cpu.regs.r[GP];
        assert_eq!(cpu.call(SPOILS, &[]), Stop::Returned);
        assert_eq!(cpu.mem.rd32(terra_exp), after, "Terra's EXP from {before}");
        assert_eq!(cpu.mem.rd32(gp + 0xA04), u32::MAX, "the share");
    }
}

/// The spoils window's EXP figure is a `u32` number record that
/// `FUN_80031D00` reads live and compares signed against its power table
/// (`0x80032068..0x800320A0`), so the all-ones share of a Terra-only win draws
/// a single `0` in the field's last cell.
#[test]
fn the_window_draws_all_ones_as_zero() {
    not_a_differential!("retail only: what the spoils readout draws for an all-ones share");
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let gp = cpu.regs.r[GP];
    let glyphs = Rc::new(RefCell::new(Vec::<(i16, u16)>::new()));
    let log = glyphs.clone();
    // The numeral blit: rect `(x, y)` in `a0`, texel `(u, v)` in `a1`.
    cpu.hook(0x8003_C11C, move |r, m| {
        let x = m.rd16(r.r[4]) as i16;
        let u = m.rd16(r.r[5]);
        log.borrow_mut().push((x, u / 8));
    });
    cpu.stub(0x8003_1AE4, 0);
    cpu.stub(0x8003_6888, 0);
    const LIST: u32 = 0x8017_0000;
    const ACTOR_A: u32 = 0x8017_0100;
    const RECORD: u32 = 0x8017_0400;
    const VALUE: u32 = 0x8017_0600;
    for (value, digits) in [
        (u32::MAX, vec![(78, 0)]),
        (1234, vec![(54, 1), (62, 2), (70, 3), (78, 4)]),
    ] {
        for b in 0..0x800 {
            cpu.mem.wr8(LIST + b, 0);
        }
        // A one-actor ring holding one five-cell `u32` record at x `0x2E`.
        cpu.mem.wr32(gp + 0x148, LIST);
        cpu.mem.wr32(LIST, ACTOR_A);
        cpu.mem.wr32(ACTOR_A, LIST);
        cpu.mem.wr32(ACTOR_A + 0x2C, RECORD);
        cpu.mem.wr16(ACTOR_A + 0x30, 1);
        cpu.mem.wr32(VALUE, value);
        cpu.mem.wr32(RECORD, VALUE);
        cpu.mem.wr32(RECORD + 4, 4);
        cpu.mem.wr16(RECORD + 8, 0x2E);
        cpu.mem.wr16(RECORD + 0xA, 0x0E);
        cpu.mem.wr16(RECORD + 0xC, 5);
        cpu.mem.wr16(RECORD + 0xE, 7);
        cpu.mem.wr32(gp + 0x15C, 0);
        glyphs.borrow_mut().clear();
        assert_eq!(cpu.call(0x8003_1D00, &[]), Stop::Returned);
        assert_eq!(*glyphs.borrow(), digits, "value {value:#x}");
    }
}

/// The annihilated arm (`0x8004F8C0..0x8004FC7C`), pass by pass from its
/// results frame to its fade, against `World::begin_battle_end_sequence` /
/// `World::tick_battle_end_sequence` on a party wipe: the loss window
/// `0x42` behind the special-battle word, the knockout taunt staged on a
/// monster pose actor (the first `0x22` entry of its action list, unless it
/// is the committed clip), the boss bark keyed on the first monster id and
/// gated on the drive word `0x8007BC20 < 2` (retried every pass until a
/// clear one; `0xA9` draws `rand()` then), the `0x100` hold and the fade
/// frame's HP floor on every member.
#[test]
fn a_party_wipe_barks_taunts_and_fades_as_retail() {
    differential!(scus(SPOILS));
    use legaia_engine_battle_vm::battle_action::{ActorFlags, BattleEndCause};
    use legaia_engine_battle_vm::battle_cast_cue::{
        VoiceCueGates, VoiceCueVerdict, admit_voice_cue,
    };
    use legaia_engine_core::world::test_drivers::BattleTestDrivers;
    let (Some(mut cpu), Some(scus), Some(disc)) = (scus_cpu(), scus(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let _ = scus;
    cpu.mem
        .load(OVERLAY_BASE, &disc.read_entry(898).expect("PROT 0898"));
    cpu.mem.wr32(0x8007_B81C, 0x8007_0A2C);
    const XA_WORD: u32 = 0x8007_BC20;
    const FIRST_MONSTER: u32 = 0x8007_BD0C;
    const ACTIONS: u32 = 0x8016_8000;
    let state = Rc::new(Cell::new(0u32));
    let stream = state.clone();
    let draws = Rc::new(Cell::new(0u32));
    let d2 = draws.clone();
    cpu.hook(RAND, move |r, _| {
        let next = stream.get().wrapping_mul(0x41C6_4E6D).wrapping_add(0x3039);
        stream.set(next);
        d2.set(d2.get() + 1);
        r.r[V0] = (next >> 16) & 0x7FFF;
    });
    let faded = Rc::new(Cell::new(false));
    let f2 = faded.clone();
    cpu.hook(0x8002_4E80, move |_, _| f2.set(true));
    let cues = Rc::new(RefCell::new(Vec::<u32>::new()));
    let c2 = cues.clone();
    cpu.hook(0x8004_FCC8, move |r, _| c2.borrow_mut().push(r.r[4]));
    let windows = Rc::new(RefCell::new(Vec::<u32>::new()));
    let w2 = windows.clone();
    cpu.hook(0x801D_8DE8, move |r, _| w2.borrow_mut().push(r.r[4]));
    for stub in [
        0x8003_CE08u32,
        0x8003_CBF8,
        0x8003_CA38,
        0x8005_6758,
        0x8003_563C,
        0x801D_5854,
        0x8004_21D4,
        0x8002_5358,
        0x8001_7B94,
    ] {
        cpu.stub(stub, 0);
    }
    let barkers = [
        0x4Cu8, 0x88, 0xA2, 0xA3, 0xA4, 0xA5, 0xA6, 0xA9, 0xB4, 0x40, 0x10,
        // The switch tree's gaps: ids that fall out between its compares.
        0x90, 0xA1, 0xA7, 0xB0, 0xB5,
    ];
    let durations = vec![300u16; 0x200];
    let mut rng = Rng(0x5749_5045);
    let mut seen = std::collections::HashMap::<&str, u32>::new();
    for round in 0..300 {
        let mut w = gen_win(&mut rng, |_, _| 0);
        w.member_hp = [0; 3];
        w.dead = [false; 5];
        let first = barkers[rng.below(barkers.len() as u32) as usize];
        // The pose actor: the monster that landed the blow, or a member.
        let monster_pose = rng.below(4) != 0;
        let pose_monster = rng.below(w.monsters as u32) as usize;
        w.pose_seat = if monster_pose {
            3 + pose_monster as u8
        } else {
            rng.below(w.party as u32) as u8
        };
        let busy = [0u16, 0, 1, 3, 20][rng.below(5) as usize];
        // The pose monster's action list: a `0x22` entry or none.
        let tags: Vec<u8> = (0..1 + rng.below(8))
            .map(|_| [0u8, 2, 4, 5, 0x0B, 0x22][rng.below(6) as usize])
            .collect();
        let taunt = tags.iter().position(|&t| t == 0x22);
        let current = match (taunt, rng.below(3)) {
            (Some(t), 0) => t as u8,
            _ => rng.below(8) as u8,
        };
        seed_retail(&mut cpu, &w);
        let gp = cpu.regs.r[GP];
        cpu.mem.wr8(gp + 0xA48, 0);
        cpu.mem.wr8(FIRST_MONSTER, first);
        let pose_actor = ACTOR + u32::from(w.pose_seat) * ACTOR_STRIDE;
        cpu.mem.wr8(pose_actor + 0x1D9, current);
        if monster_pose {
            let r = MONSTER + pose_monster as u32 * 0x80;
            cpu.mem.wr8(r + 0x4A, tags.len() as u8);
            for (i, t) in tags.iter().enumerate() {
                let e = ACTIONS + i as u32 * 0x10;
                cpu.mem.wr8(e, *t);
                cpu.mem.wr32(r + 0x4C + 4 * i as u32, e);
            }
        }
        cpu.mem.wr8(0x1F80_0393, 1);
        state.set(w.seed);
        faded.set(false);

        // The port, armed on the signal tick.
        let mut world = World::default();
        let (mut formation, catalog) = seed_port(&mut world, &w);
        formation.slots[0].monster_id = u16::from(first);
        world.battle.active_formation = Some(formation);
        world.tables.monster_catalog = catalog;
        world.audio.xa_cue_durations = Some(durations.clone());
        world.battle.victory = None;
        world.presentation.fade = None;
        world.game_over_hold = false;
        world.battle_ctx.active_actor =
            w.pose_seat - if monster_pose { 3 - w.party as u8 } else { 0 };
        if monster_pose {
            let seat = w.party + pose_monster;
            let clips: Vec<_> = tags.iter().map(|&t| Some(clip(t))).collect();
            world.set_actor_battle_action_clips(seat, std::sync::Arc::new(clips));
            world.actors[seat].battle.current_anim = current;
            world.actors[seat].battle.queued_anim = 0xEE;
        }
        world.audio.battle_xa_busy_frames = busy;
        world.battle.end = Some(BattleEndCause::PartyWipe);
        world.begin_battle_end_sequence();
        let opened = world.battle.victory.is_some_and(|v| v.window_opened);

        let label = format!(
            "round {round}: first {first:#x} pose {} busy {busy} tags {tags:?} current {current} special {:#x}",
            w.pose_seat, w.special
        );
        let mut retail_bark = None;
        let mut port_bark = None;
        let mut retail_fade = None;
        for pass in 0..0x200u32 {
            // The port's drive word, as its tick counts it down.
            world.audio.battle_xa_busy_frames = world.audio.battle_xa_busy_frames.saturating_sub(1);
            let xa_up = world.audio.battle_xa_busy_frames != 0 && port_bark.is_none();
            cpu.mem.wr32(XA_WORD, if xa_up { 2 } else { 0 });
            cues.borrow_mut().clear();
            windows.borrow_mut().clear();
            draws.set(0);
            assert_eq!(cpu.call(SPOILS, &[]), Stop::Returned, "{label} pass {pass}");
            if pass == 0 {
                assert_eq!(
                    windows.borrow().contains(&0x42),
                    opened,
                    "{label}: the loss window"
                );
                // The taunt, on the results frame.
                let staged = cpu.mem.rd8(pose_actor + 0x1DC) & 2 != 0;
                if monster_pose {
                    let seat = w.party + pose_monster;
                    let a = &world.actors[seat].battle;
                    let port_staged = a.queued_anim != 0xEE;
                    assert_eq!(staged, port_staged, "{label}: taunt staged");
                    if staged {
                        assert_eq!(cpu.mem.rd8(pose_actor + 0x1DA), a.queued_anim, "{label}");
                        assert_ne!(a.flag_bits.0 & ActorFlags::ADVANCE_DONE, 0, "{label}");
                        *seen.entry("taunt").or_default() += 1;
                    }
                } else {
                    assert!(!staged, "{label}: a member pose stages nothing");
                }
            }
            let rand_before = world.rng_state;
            let xa_before = world.audio.battle_xa_cues.len();
            world.tick_battle_end_sequence();
            if let Some(&id) = cues.borrow().iter().find(|&&c| c >= 0x100) {
                assert!(retail_bark.is_none(), "{label}: one bark");
                retail_bark = Some((pass, id, draws.get()));
            }
            if world.audio.battle_xa_cues.len() > xa_before {
                let c = world.audio.battle_xa_cues[xa_before];
                port_bark = Some((pass, c, u32::from(world.rng_state != rand_before)));
            }
            if faded.get() {
                retail_fade = Some(pass);
                for m in 0..w.party as u32 {
                    assert_eq!(cpu.mem.rd16(ACTOR + m * ACTOR_STRIDE + 0x14C), 1, "{label}");
                    assert_eq!(world.actors[m as usize].battle.hp, 1, "{label}: HP floor");
                }
                assert!(
                    world.presentation.fade.is_some(),
                    "{label}: the fade, pass {pass}"
                );
                break;
            }
            assert!(
                world.presentation.fade.is_none(),
                "{label}: early fade, pass {pass}"
            );
        }
        assert!(retail_fade.is_some(), "{label}");
        match (retail_bark, port_bark) {
            (None, None) => *seen.entry("no bark").or_default() += 1,
            (Some((rp, id, rd)), Some((pp, clip, pd))) => {
                assert_eq!(rp, pp, "{label}: the bark's pass");
                let VoiceCueVerdict::Play(req) = admit_voice_cue(
                    id as u16,
                    VoiceCueGates {
                        side_band_stage: 0,
                        stream_read: false,
                    },
                    Some(300),
                ) else {
                    panic!("{label}");
                };
                assert_eq!(
                    (req.clip_slot, req.channel),
                    (clip.clip, clip.channel),
                    "{label}: bark {id:#x}"
                );
                assert_eq!(rd.min(1), pd, "{label}: the bark's rand draw");
                *seen
                    .entry(if rp > 0 { "bark held" } else { "bark" })
                    .or_default() += 1;
            }
            (r, p) => panic!("{label}: retail bark {r:?} port {p:?}"),
        }
    }
    for key in ["taunt", "no bark", "bark", "bark held"] {
        assert!(seen.get(key).copied().unwrap_or(0) > 5, "{key}: {seen:?}");
    }
}

fn clip(action_id: u8) -> legaia_actor_formats::monster_archive::MonsterAnimation {
    legaia_actor_formats::monster_archive::MonsterAnimation {
        variant_track: Default::default(),
        action_id,
        rate: 2,
        attach_key: 0,
        solo_flag: 0,
        impact_class: 0,
        effect_script: Vec::new(),
        part_count: 1,
        frame_count: 2,
        frames: vec![vec![Default::default()]; 2],
    }
}
