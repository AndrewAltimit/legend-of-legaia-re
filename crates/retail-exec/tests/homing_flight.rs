//! The per-slot half of the move projectile tick `FUN_801E09F8` - the
//! streak's hand-over, the flight, the landing, the landed countdown and the
//! hit - run whole, pass after pass, against `HomingSlots::step_pass` and the
//! streak word's `MoveFxStreak::tick_counter_by`.
//!
//! Every shipped move-power record and mutations of them (speed, raise,
//! landed count, tracking), every frame step `1..=4`, one to four children
//! spread over the seven seats. Per pass the four slots' phase, child,
//! position, heading and counter are compared whole, and so are the list
//! spawns (the prototype scene, the 2D pool and the screen flash, each with
//! its slot, position and heading), the ribbons (`FUN_801E1D98`), the victims
//! the landed arm tints and shivers, the handles a tracking flight drags, the
//! hits and the census's child count `ctx[+0x24D]`. Half the records carry a
//! status selector (`3` Venom, `4` Toxic, `5` Rot) whose roll the landing
//! makes on the way to the hit (`0x801E1584..0x801E1788`): what it lands on
//! each victim's status word, the draws it takes and the Auto strings a new
//! Rot empties are compared with `World::apply_enemy_move_status`, as
//! `enemy_impact` compares the arm entered on its own.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_battle::battle_events::BattleHitFx;
use legaia_engine_battle_vm::battle_cast_census::{
    CensusSlot, EffectChildVictim, cast_census, effect_child_hit,
};
use legaia_engine_effects::action_effect_script::{HomingSlot, HomingSlots, MoveFxStreak};
use psx_retail_exec::{A0, A1, A2, RA, Stop, V0};
use std::cell::RefCell;
use std::rc::Rc;

const TICK: u32 = 0x801E_09F8;
const OVERLAY_BASE: u32 = 0x801C_E818;
const CTX_PTR: u32 = 0x8007_BD24;
const CTX: u32 = 0x8012_0000;
const ACTORS: u32 = 0x801C_9370;
const RECORD: u32 = 0x8014_0000;
const HANDLES: u32 = 0x8015_0000;
const PRIMS: u32 = 0x801A_0000;
const FRAME_STEP: u32 = 0x1F80_0393;
const PRIM_PTR: u32 = 0x1F80_03A0;
const SIN_PTR: u32 = 0x8007_B81C;
const COS_PTR: u32 = 0x8007_B7F8;
const SIN_LUT: u32 = 0x8007_0A2C;
const POOL_2D: u32 = 0x801D_FDF0;
const PROTO_SPAWN: u32 = 0x8005_0ED4;
const FLASH: u32 = 0x8002_4E80;
const SFX: u32 = 0x8005_8490;
const STREAK_QUAD: u32 = 0x801E_1AB0;
const RIBBON: u32 = 0x801E_1D98;
const CUE: u32 = 0x8004_FCC8;
const CAST_HIT: u32 = 0x801D_D0AC;
const RAND: u32 = 0x8005_6798;
const SEATS: u32 = 0x8007_BD10;
const RECORDS: u32 = 0x8008_4708;
const PARTY_STRIDE: u32 = 0x414;
const MOVE_POWER_FILE: usize = 0x26744;
const STRIDE: usize = 26;
const S0: usize = 16;
const S3: usize = 19;

fn actor(seat: u32) -> u32 {
    0x8013_0000 + seat * 0x400
}

/// A list spawn as both sides report it: the effect byte (bit 7 = the 2D
/// pool), the slot, where and the heading.
type Spawn = (u8, u8, [i16; 3], u16, bool);

#[derive(Default)]
struct Seen {
    spawns: Vec<Spawn>,
    ribbons: Vec<(u8, [i16; 3])>,
    hits: Vec<u8>,
    damage: Vec<i32>,
    next_handle: u32,
    roll: u32,
}

fn slot_of(addr: u32) -> u8 {
    ((addr - CTX - 0x1144) / 8) as u8
}

fn pos_at(m: &mut psx_retail_exec::Memory, addr: u32) -> [i16; 3] {
    [
        m.rd16(addr) as i16,
        m.rd16(addr + 2) as i16,
        m.rd16(addr + 4) as i16,
    ]
}

/// The fields the hit arm reads and writes on a victim.
fn victim_at(m: &mut psx_retail_exec::Memory, a: u32) -> EffectChildVictim {
    EffectChildVictim {
        hp: m.rd16(a + 0x14C),
        hp_bar_delta: m.rd32(a + 0x10) as i32,
        flags: m.rd16(a + 0x16E),
        staged_anim: m.rd8(a + 0x1DA),
        restage: m.rd8(a + 0x1DC),
        reaction_alt: m.rd8(a + 0x1EF),
        reaction_alt2: m.rd8(a + 0x1F0),
        knockdown_anim: m.rd8(a + 0x1F1),
        reaction_gate: m.rd8(a + 0x1F2),
    }
}

fn i16s(p: [i32; 3]) -> [i16; 3] {
    p.map(|v| v as i16)
}

#[test]
fn the_projectile_flies_lands_and_hits_as_retails_does() {
    differential!(prot(898, TICK));
    let (Some(mut cpu), Some(disc)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let overlay = disc.read_entry(898).expect("PROT 0898");
    cpu.mem.load(OVERLAY_BASE, &overlay);
    let shipped: Vec<Vec<u8>> = (0..44)
        .map(|i| overlay[MOVE_POWER_FILE + i * STRIDE..][..STRIDE].to_vec())
        .filter(|r| r[0x08] != 0 || r[0x12] != 0 || r[0x16] != 0)
        .collect();
    assert!(shipped.len() > 20);

    let seen: Rc<RefCell<Seen>> = Rc::default();
    {
        let s = seen.clone();
        cpu.hook(POOL_2D, move |r, m| {
            let at = pos_at(m, r.r[A1]);
            s.borrow_mut().spawns.push((
                r.r[A0] as u8 | 0x80,
                slot_of(r.r[A1]),
                at,
                r.r[A2] as u16,
                false,
            ));
        });
        let s = seen.clone();
        cpu.hook(PROTO_SPAWN, move |r, m| {
            let slot = slot_of(r.r[A0]);
            let at = pos_at(m, r.r[A0]);
            let heading = m.rd16(r.r[A1] + 2);
            let mut s = s.borrow_mut();
            s.spawns.push((r.r[S0] as u8, slot, at, heading, false));
            s.next_handle = (s.next_handle + 1) % 0x1000;
            r.r[V0] = HANDLES + 0x200 + s.next_handle * 0x40;
        });
        let s = seen.clone();
        cpu.hook(FLASH, move |r, m| {
            let slot = r.r[S3] as u8;
            let at = pos_at(m, CTX + 0x1144 + 8 * u32::from(slot));
            let heading = m.rd16(CTX + 0x1166 + 8 * u32::from(slot));
            s.borrow_mut().spawns.push((100, slot, at, heading, true));
        });
        let s = seen.clone();
        cpu.hook(RIBBON, move |r, m| {
            let at = pos_at(m, r.r[A0]);
            s.borrow_mut().ribbons.push((slot_of(r.r[A0]), at));
        });
        let s = seen.clone();
        cpu.hook(CAST_HIT, move |r, _| {
            // The damage wrapper's result: nothing, a scratch, or more than
            // a seat holds.
            let mut s = s.borrow_mut();
            s.roll = s.roll.wrapping_mul(1_103_515_245).wrapping_add(12345);
            let dmg = [0, 1, 7, 40, 120, 999][(s.roll >> 16) as usize % 6];
            s.hits.push(r.r[A2] as u8);
            s.damage.push(dmg);
            r.r[V0] = dmg as u32;
        });
    }
    cpu.stub(SFX, 0);
    cpu.stub(STREAK_QUAD, 0);
    cpu.stub(CUE, 0);
    // The status arm's draws (`0x801E1630`, `0x801E1660`, `0x801E16FC`) come
    // off the LCG `World::next_rand` steps; every other caller reads `1`.
    let lcg = Rc::new(std::cell::Cell::new((0u32, 0u32)));
    {
        let l = lcg.clone();
        cpu.hook(RAND, move |r, _| {
            if [0x801E_1638, 0x801E_1668, 0x801E_1704].contains(&r.r[RA]) {
                let (state, draws) = l.get();
                let next = state.wrapping_mul(0x41C6_4E6D).wrapping_add(0x3039);
                l.set((next, draws + 1));
                r.r[V0] = (next >> 16) & 0x7FFF;
            } else {
                r.r[V0] = 1;
            }
        });
    }
    // The status arm's port, and the party records its Rot reads.
    let mut w = legaia_engine_core::world::World::new();
    w.load_party(legaia_save::Party::zeroed(3));
    w.enter_battle(3, 4);
    w.party.party_count = 3;
    w.tables.move_power =
        legaia_engine_battle::move_power::MovePowerCatalog::from_overlay_0898(&overlay);
    assert!(w.actors.len() >= 7);
    let move_of: Vec<(u8, u8)> = (3..=5u8)
        .map(|sel| {
            let id = (0..=255u8)
                .find(|&id| {
                    w.tables
                        .move_power
                        .as_ref()
                        .and_then(|c| c.record_for_move_id(id))
                        .is_some_and(|r| r.impact_effect() == sel)
                })
                .expect("a move with the selector");
            (sel, id)
        })
        .collect();
    for s in 0..3u32 {
        cpu.mem.wr8(SEATS + s, s as u8 + 1);
    }
    let (mut venom, mut rotted) = (0u32, 0u32);
    cpu.mem.wr32(CTX_PTR, CTX);
    cpu.mem.wr32(CTX + 0x1014, RECORD);
    cpu.mem.wr32(SIN_PTR, SIN_LUT);
    cpu.mem.wr32(COS_PTR, SIN_LUT + 0x800);
    for s in 0..8 {
        cpu.mem.wr32(ACTORS + 4 * s, actor(s));
    }

    let mut rng = Rng(0x484F_4D45);
    let mut failures: Vec<String> = Vec::new();
    let (mut flown, mut landed_counting, mut hit, mut trailed, mut tracked, mut flashed) =
        (0u32, 0u32, 0u32, 0u32, 0u32, 0u32);
    let (mut sole, mut idle, mut staged) = (0u32, 0u32, 0u32);
    'scenario: for scenario in 0..3000 {
        let pick = rng.below(shipped.len() as u32) as usize;
        let mut rec = shipped[pick].clone();
        match rng.below(7) {
            0 => rec[0x08] = 1 + rng.below(0x60) as u8,
            1 => rec[0x06..0x08].copy_from_slice(&(rng.below(0x200) as u16).to_le_bytes()),
            2 => rec[0x02..0x04].copy_from_slice(&(rng.below(0x900) as u16).to_le_bytes()),
            3 => rec[0x09] ^= 1,
            // No streak list: the landing list `+0x16` runs instead
            // (`s5 = 3`, `0x801E0D00..0x801E0D10`), its bytes the shipped
            // lists' kinds - a prototype, a 2D-pool byte, a skip, the flash.
            6 => {
                rec[0x12..0x16].fill(0);
                for b in &mut rec[0x16..0x1A] {
                    *b = [0x15, 0x1E, 0x28, 0x8D, 0x9D, 0xFF, 0x64, 0][rng.below(8) as usize];
                }
            }
            4 => {
                let list = [0x12, 0x16][rng.below(2) as usize];
                rec[list + rng.below(4) as usize] = 100 + rng.below(28) as u8;
            }
            _ => {}
        }
        // No selector, a tint-only one, or a status one; `6` is past the
        // ladder's last arm (`0x801E1620` tests `5` alone) and rolls nothing.
        rec[0x0A] = match rng.below(8) {
            0 => 0,
            1..=3 => 1 + rng.below(2) as u8,
            7 => 6,
            _ => 3 + rng.below(3) as u8,
        };
        let ability = [0u32, 1 << 0x18, 1 << 0x1C, 1 << 0x10][rng.below(4) as usize];
        for s in 0..3u32 {
            let r = RECORDS + s * PARTY_STRIDE;
            cpu.mem.wr32(r + 0xF4, ability);
            cpu.mem.wr8(r + 0x1A7, 0x41);
            cpu.mem.wr8(r + 0x1B7, 0x42);
        }
        cpu.mem.load(RECORD, &rec);
        // The table runs on: a list with no zero byte reads the next
        // record's first byte as its fifth (`0x801E0F48..0x801E0F5C`) and
        // stops on the four-byte bound (`0x801E0E04`).
        cpu.mem
            .load(RECORD + STRIDE as u32, &shipped[(pick + 1) % shipped.len()]);

        // The actors: seven seats spread over the floor.
        let mut seats = [[0i16; 3]; 8];
        for s in 0..7u32 {
            let a = actor(s);
            let p = [
                rng.below(0x2400) as i16 - 0x1200,
                -(rng.below(0x200) as i16),
                rng.below(0x2400) as i16 - 0x1200,
            ];
            seats[s as usize] = p;
            cpu.mem.wr16(a + 0x34, p[0] as u16);
            cpu.mem.wr16(a + 0x36, p[1] as u16);
            cpu.mem.wr16(a + 0x38, p[2] as u16);
            cpu.mem.wr16(a + 0x3A, 0);
            cpu.mem.wr32(a + 0x04, u32::from(rng.below(2) == 0));
            // The census head's inputs: an animation id (`8`, a downed
            // member's, is taken back out on a party seat) and the HP that
            // doubles as liveness, a few seats down.
            cpu.mem
                .wr8(a + 0x1D9, [0u8, 0, 0, 8, 3][rng.below(5) as usize]);
            let hp = if rng.below(4) == 0 {
                0
            } else {
                1 + rng.below(100) as u16
            };
            cpu.mem.wr16(a + 0x14C, hp);
            // The hit arm's reaction bytes and the Stone bit that skips its
            // face-toward store.
            cpu.mem
                .wr16(a + 0x16E, [0u16, 0, 0, 4][rng.below(4) as usize]);
            cpu.mem.wr32(a + 0x10, 0);
            for (o, v) in [
                (0x1DA, rng.below(0x20)),
                (0x1DC, rng.below(4) * 2),
                (0x1EF, [0, 0, 0x11][rng.below(3) as usize]),
                (0x1F0, 0x12 + rng.below(4)),
                (0x1F1, 0x18 + rng.below(4)),
                (0x1F2, u32::from(rng.below(4) == 0)),
            ] {
                cpu.mem.wr8(a + o, v as u8);
            }
        }
        // The census reads the seats as the pass finds them: the landing's
        // shiver and the hit arm write the victims between passes.
        let census_slots = |cpu: &mut psx_retail_exec::Cpu| -> Vec<CensusSlot> {
            (0..7u32)
                .map(|s| CensusSlot {
                    render_word: cpu.mem.rd32(actor(s) + 0x04),
                    current_anim: cpu.mem.rd8(actor(s) + 0x1D9),
                    liveness: cpu.mem.rd16(actor(s) + 0x14C),
                })
                .collect()
        };
        let census_of = |cpu: &mut psx_retail_exec::Cpu| {
            [0x249u32, 0x24A, 0x24B, 0x24D].map(|o| cpu.mem.rd8(CTX + o))
        };
        cpu.mem.wr8(CTX + 0x13, rng.below(7) as u8);
        cpu.mem.wr8(CTX + 0x262, 0);

        // The terminator's seed: every slot at the launch in phase 1, the
        // children on distinct seats.
        let launch = [
            rng.below(0x2400) as i16 - 0x1200,
            -(rng.below(0x300) as i16),
            rng.below(0x2400) as i16 - 0x1200,
        ];
        let mut port = HomingSlots {
            record: Some(0),
            emits: true,
            ..HomingSlots::default()
        };
        let mut free: Vec<u8> = (1..=7).collect();
        for i in 0..4u32 {
            let child = if i == 0 || rng.below(3) != 0 {
                let k = rng.below(free.len() as u32) as usize;
                free.swap_remove(k)
            } else {
                0
            };
            let heading = rng.below(0x1000) as u16;
            let base = CTX + 0x1144 + 8 * i;
            cpu.mem.wr8(CTX + 0x24E + i, 1);
            cpu.mem.wr8(CTX + 0x252 + i, child);
            cpu.mem.wr16(base, launch[0] as u16);
            cpu.mem.wr16(base + 2, launch[1] as u16);
            cpu.mem.wr16(base + 4, launch[2] as u16);
            cpu.mem.wr16(base + 6, 0);
            cpu.mem.wr16(CTX + 0x1164 + 8 * i, 0);
            cpu.mem.wr16(CTX + 0x1166 + 8 * i, heading);
            cpu.mem.wr16(CTX + 0x1168 + 8 * i, 0);
            cpu.mem.wr16(CTX + 0x6C6 + 2 * i, 0);
            cpu.mem.wr32(CTX + 0x1018 + 4 * i, HANDLES + 0x40 * i);
            port.slots[i as usize] = HomingSlot {
                phase: 1,
                child,
                pos: launch.map(i32::from),
                heading,
                counter: 0,
            };
        }
        let streak_word = [0u16, 0, 8, 0x40, 0x100, 0x3C0][rng.below(6) as usize];
        cpu.mem.wr16(CTX + 0x6C6, streak_word);
        let mut streak = MoveFxStreak {
            counter_word: streak_word,
            phase: 1,
            ..MoveFxStreak::default()
        };

        for pass in 0..400 {
            let step = 1 + rng.below(4) as u8;
            cpu.mem.wr8(FRAME_STEP, step);
            cpu.mem.wr32(PRIM_PTR, PRIMS);
            for s in 0..7 {
                cpu.mem.wr8(actor(s) + 0x21F, 0);
                cpu.mem.wr8(actor(s) + 0x21D, 0);
            }
            let handles: Vec<u32> = (0..4).map(|i| cpu.mem.rd32(CTX + 0x1018 + 4 * i)).collect();
            for &h in &handles {
                cpu.mem.load(h + 0x14, &[0xA5; 8]);
            }
            let streaking = cpu.mem.rd8(CTX + 0x24E) == 1 && cpu.mem.rd16(CTX + 0x6C6) != 0;
            let (next_handle, roll) = (seen.borrow().next_handle, seen.borrow().roll);
            *seen.borrow_mut() = Seen {
                next_handle,
                roll,
                ..Seen::default()
            };
            let slots_now = census_slots(&mut cpu);
            let victims_before: Vec<EffectChildVictim> =
                (0..8).map(|s| victim_at(&mut cpu.mem, actor(s))).collect();
            let cursor_before = cpu.mem.rd8(CTX + 0x262);
            let heads_before: Vec<(u8, u8)> = (0..3)
                .map(|s| {
                    let r = RECORDS + s * PARTY_STRIDE;
                    (cpu.mem.rd8(r + 0x1A7), cpu.mem.rd8(r + 0x1B7))
                })
                .collect();
            let lcg_seed = rng.next();
            lcg.set((lcg_seed, 0));
            assert_eq!(cpu.call(TICK, &[]), Stop::Returned, "{scenario}/{pass}");

            // The port, as `World::tick_homing_slots_pass` drives it.
            // The census at the head of the pass counts the slots as the
            // pass found them (`0x801E0B64..0x801E0BF0`).
            let census = port.children();
            let before = streak.counter_word;
            if port.slots[0].phase == 1 {
                streak.tick_counter_by(step);
            }
            let spawns = port.step_pass(&rec, before, step, |c| {
                let p = seats[usize::from(c) - 1];
                Some(p.map(i32::from))
            });

            let mut why: Vec<String> = Vec::new();
            for i in 0..4u32 {
                let s = port.slots[i as usize];
                let base = CTX + 0x1144 + 8 * i;
                let retail = (
                    cpu.mem.rd8(CTX + 0x24E + i),
                    cpu.mem.rd8(CTX + 0x252 + i),
                    pos_at(&mut cpu.mem, base),
                    cpu.mem.rd16(CTX + 0x1166 + 8 * i),
                    cpu.mem.rd16(CTX + 0x6C6 + 2 * i) as i16,
                );
                let counter = if i == 0 && s.phase == 1 {
                    streak.counter_word as i16
                } else {
                    s.counter
                };
                let ours = (s.phase, s.child, i16s(s.pos), s.heading, counter);
                if retail != ours {
                    why.push(format!("slot {i}: retail {retail:x?} port {ours:x?}"));
                }
            }
            let children = cpu.mem.rd8(CTX + 0x24D);
            if children != census {
                why.push(format!("children retail {children} port {census}"));
            }
            // The rest of the census head, against `cast_census`.
            let c = cast_census(&slots_now, [1; 4], [0; 4]);
            let head = census_of(&mut cpu);
            let want = [
                c.anim_outstanding,
                c.sole_party_target,
                c.sole_monster_target,
                children,
            ];
            if head != want {
                why.push(format!("census retail {head:x?} port {want:x?}"));
            }
            sole += u32::from(c.sole_party_target != 0 && c.sole_monster_target != 0);

            let seen_now = std::mem::take(&mut *seen.borrow_mut());
            seen.borrow_mut().next_handle = seen_now.next_handle;
            seen.borrow_mut().roll = seen_now.roll;
            let want: Vec<Spawn> = spawns
                .iter()
                .map(|s| (s.effect, s.slot, i16s(s.at), s.heading, s.flash))
                .collect();
            if seen_now.spawns != want {
                why.push(format!(
                    "spawns retail {:x?} port {want:x?}",
                    seen_now.spawns
                ));
            }
            flashed += u32::from(seen_now.spawns.iter().any(|s| s.4));

            if !streaking {
                let mut want = Vec::new();
                let trail_bytes = rec[0x12..0x16].iter().filter(|&&b| b == 0xFF).count();
                let shock_bytes = rec[0x16..0x1A].iter().filter(|&&b| b == 0xFF).count();
                for i in 0..4 {
                    if let Some(p) = port.trails[i] {
                        want.extend((0..trail_bytes).map(|_| (i as u8, i16s(p))));
                    }
                    if port.shocks[i] != 0 {
                        let p = i16s(port.slots[i].pos);
                        want.extend((0..shock_bytes).map(|_| (i as u8, p)));
                    }
                }
                trailed += u32::from(port.trails.iter().any(Option::is_some));
                if seen_now.ribbons != want {
                    why.push(format!(
                        "ribbons retail {:x?} port {want:x?}",
                        seen_now.ribbons
                    ));
                }
            }

            let mut shiver = Vec::new();
            let mut tinted = Vec::new();
            for s in 0..7u8 {
                if cpu.mem.rd8(actor(u32::from(s)) + 0x21D) == 0x20 {
                    shiver.push(s + 1);
                }
                if rec[0x0A] != 0 && cpu.mem.rd8(actor(u32::from(s)) + 0x21F) == rec[0x0A] {
                    tinted.push(s + 1);
                }
            }
            let sorted = |v: [u8; 4]| {
                let mut v: Vec<u8> = v.into_iter().filter(|&c| c != 0).collect();
                v.sort_unstable();
                v
            };
            if shiver != sorted(port.shocks) {
                why.push(format!("shiver retail {shiver:?} port {:?}", port.shocks));
            }
            if rec[0x0A] != 0 && tinted != sorted(port.landed) {
                why.push(format!("tinted retail {tinted:?} port {:?}", port.landed));
            }
            landed_counting += u32::from(port.shocks.iter().any(|&c| c != 0));
            let hits: Vec<u8> = port
                .hits
                .iter()
                .filter(|&&c| c != 0)
                .map(|c| c - 1)
                .collect();
            if seen_now.hits != hits {
                why.push(format!("hits retail {:?} port {hits:?}", seen_now.hits));
            }
            // Each hit's victim as `effect_child_hit` leaves it, from the
            // seat as the pass found it and the damage the wrapper gave.
            let mut want = victims_before.clone();
            // The status arm, slot by slot in the order the pass lands them,
            // on one rand stream: each landed victim's word gains what the
            // port's roll lands (the arm only ORs bits in).
            let mut heads = heads_before.clone();
            w.rng_state = lcg_seed;
            let mut port_draws = 0u32;
            if (3..=5).contains(&rec[0x0A]) {
                let id = move_of[usize::from(rec[0x0A] - 3)].1;
                for &c in port.landed.iter().filter(|&&c| c != 0) {
                    let t = c - 1;
                    let before = want[usize::from(t)].flags;
                    for s in 0..7u8 {
                        w.battle.status_effects.drop_slot(s);
                        w.actors[usize::from(s)].battle.field_flags = 0;
                        w.actors[usize::from(s)].pending_status = None;
                    }
                    w.party.character_ability_bits[..3].fill(ability);
                    for r in w.party.roster.members.iter_mut() {
                        r.raw[0xF4..0xF8].copy_from_slice(&ability.to_le_bytes());
                        r.raw[0x1A7] = 0x41;
                        r.raw[0x1B7] = 0x42;
                    }
                    w.battle.hit_fx.clear();
                    w.battle.hit_fx.push(BattleHitFx {
                        target_slot: t,
                        amount: 1,
                        is_heal: false,
                        is_crit: false,
                    });
                    let state = w.rng_state;
                    w.apply_enemy_move_status(4, id, 0);
                    port_draws += u32::from(w.rng_state != state);
                    let landed = w.raw_status_word(t);
                    // A Rot limb already rotted lands nothing new, and
                    // empties no strings.
                    if landed & !before != 0 {
                        want[usize::from(t)].flags = before | landed;
                        venom += u32::from(landed & 3 != 0);
                        if t < 3 && landed & 0x38 != 0 {
                            let r = &w.party.roster.members[usize::from(t)];
                            heads[usize::from(t)] = (r.raw[0x1A7], r.raw[0x1B7]);
                            rotted += 1;
                        }
                    }
                }
            }
            let (retail_state, retail_draws) = lcg.get();
            if (retail_state, retail_draws) != (w.rng_state, port_draws) {
                why.push(format!(
                    "status draws retail {retail_draws} ({retail_state:#x}) port {port_draws} ({:#x})",
                    w.rng_state
                ));
            }
            for s in 0..3u32 {
                let r = RECORDS + s * PARTY_STRIDE;
                let got = (cpu.mem.rd8(r + 0x1A7), cpu.mem.rd8(r + 0x1B7));
                if got != heads[s as usize] {
                    why.push(format!(
                        "seat {s} Auto heads retail {got:x?} port {:x?}",
                        heads[s as usize]
                    ));
                }
            }
            for &c in port.landed.iter().filter(|&&c| c != 0) {
                let t = usize::from(c - 1);
                let got = cpu.mem.rd16(actor(t as u32) + 0x16E);
                if !seen_now.hits.contains(&(c - 1)) && got != want[t].flags {
                    why.push(format!(
                        "status {t}: retail {got:#x} port {:#x}",
                        want[t].flags
                    ));
                }
            }
            let mut cursor = cursor_before;
            for (&v, &dmg) in seen_now.hits.iter().zip(&seen_now.damage) {
                effect_child_hit(&mut want[usize::from(v)], dmg, &mut cursor);
            }
            for &v in &seen_now.hits {
                let got = victim_at(&mut cpu.mem, actor(u32::from(v)));
                if got != want[usize::from(v)] {
                    why.push(format!(
                        "victim {v}: retail {got:x?} port {:x?}",
                        want[usize::from(v)]
                    ));
                }
                staged += 1;
            }
            if !seen_now.hits.is_empty() && cpu.mem.rd8(CTX + 0x262) != cursor {
                why.push(format!(
                    "readout cursor retail {} port {cursor}",
                    cpu.mem.rd8(CTX + 0x262)
                ));
            }
            hit += hits.len() as u32;

            for (i, &h) in handles.iter().enumerate() {
                let dragged = cpu.mem.read(h + 0x14, 8) != [0xA5; 8];
                let retail = dragged.then(|| pos_at(&mut cpu.mem, h + 0x14));
                let ours = port.follows[i].map(i16s);
                if retail != ours {
                    why.push(format!("follow {i}: retail {retail:x?} port {ours:x?}"));
                }
                tracked += u32::from(dragged);
            }
            flown += u32::from(port.slots.iter().any(|s| s.phase == 2));

            if !why.is_empty() {
                failures.push(format!(
                    "scenario {scenario} pass {pass} step {step} record {:02x?}:\n  {}",
                    rec,
                    why.join("\n  ")
                ));
                continue 'scenario;
            }
            if (0..4).all(|i| cpu.mem.rd8(CTX + 0x24E + i) == 0) {
                // The loop gate: with every slot's phase clear the tick
                // leaves after the census (`0x801E0BA8`), its child count
                // left at zero, whatever the child bytes still hold.
                cpu.mem.wr8(FRAME_STEP, step);
                let slots_now = census_slots(&mut cpu);
                assert_eq!(cpu.call(TICK, &[]), Stop::Returned, "{scenario}: idle");
                let c = cast_census(&slots_now, [0; 4], [1; 4]);
                let head = census_of(&mut cpu);
                let want = [
                    c.anim_outstanding,
                    c.sole_party_target,
                    c.sole_monster_target,
                    c.effect_children,
                ];
                assert_eq!(head, want, "{scenario}: the idle pass's census");
                assert!(seen.borrow().spawns.is_empty() && seen.borrow().hits.is_empty());
                idle += 1;
                break;
            }
            if (0..4).all(|i| cpu.mem.rd8(CTX + 0x252 + i) == 0) {
                break;
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{} scenarios differ:\n{}",
        failures.len(),
        failures[..failures.len().min(8)].join("\n")
    );
    eprintln!(
        "flown {flown} counting {landed_counting} hits {hit} trails {trailed} tracked {tracked} flashes {flashed}"
    );
    assert!(flown > 1000 && landed_counting > 100 && hit > 1000);
    assert!(trailed > 100 && tracked > 100 && flashed > 10);
    assert!(
        sole > 100 && idle > 100 && staged > 1000,
        "sole {sole} idle {idle} staged {staged}"
    );
    assert!(venom > 20 && rotted > 20, "venom {venom} rotted {rotted}");
}
