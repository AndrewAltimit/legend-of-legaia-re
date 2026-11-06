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
use legaia_retail_exec::{Cpu, GP, Stop, V0};
use legaia_save::{CharacterRecord, Party};
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
    seed: u32,
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
        seed: rng.next(),
    };
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
        w.word1[m] = [0, 0, 0x1_0000, 0x2_0000][rng.below(4) as usize];
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
    world.rng_state = w.seed;
    (FormationDef::new(1000, slots), catalog)
}

#[test]
fn a_win_pays_what_retail_pays() {
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
    let (mut failures, mut drops) = (Vec::new(), 0);
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
        let retail = (
            cpu.mem.rd32(PURSE),
            retail_xp,
            granted.borrow().clone(),
            state.get(),
        );
        let (formation, catalog) = seed_port(&mut world, &w);
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
            r.drops.clone(),
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
    assert!(drops > 100, "{drops} drops");
}
