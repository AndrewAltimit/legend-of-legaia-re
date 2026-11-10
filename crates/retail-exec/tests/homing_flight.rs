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
//! hits and the census's child count `ctx[+0x24D]`. The impact selector's
//! status rolls are `enemy_impact`'s; here every record carries a tint-only
//! selector.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_effects::action_effect_script::{HomingSlot, HomingSlots, MoveFxStreak};
use psx_retail_exec::{A0, A1, A2, Stop, V0};
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
    next_handle: u32,
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
            s.borrow_mut().hits.push(r.r[A2] as u8);
            r.r[V0] = 0;
        });
    }
    cpu.stub(SFX, 0);
    cpu.stub(STREAK_QUAD, 0);
    cpu.stub(CUE, 0);
    cpu.stub(RAND, 1);
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
    'scenario: for scenario in 0..3000 {
        let mut rec = shipped[rng.below(shipped.len() as u32) as usize].clone();
        match rng.below(6) {
            0 => rec[0x08] = 1 + rng.below(0x60) as u8,
            1 => rec[0x06..0x08].copy_from_slice(&(rng.below(0x200) as u16).to_le_bytes()),
            2 => rec[0x02..0x04].copy_from_slice(&(rng.below(0x900) as u16).to_le_bytes()),
            3 => rec[0x09] ^= 1,
            4 => {
                let list = [0x12, 0x16][rng.below(2) as usize];
                rec[list + rng.below(4) as usize] = 100 + rng.below(28) as u8;
            }
            _ => {}
        }
        // A tint-only selector: the status rolls are `enemy_impact`'s.
        if rec[0x0A] > 2 {
            rec[0x0A] = 1 + rng.below(2) as u8;
        }
        cpu.mem.load(RECORD, &rec);

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
            cpu.mem.wr16(a + 0x14C, 1 + rng.below(100) as u16);
            cpu.mem.wr16(a + 0x16E, 0);
        }
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
            let next_handle = seen.borrow().next_handle;
            *seen.borrow_mut() = Seen {
                next_handle,
                ..Seen::default()
            };
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

            let seen_now = std::mem::take(&mut *seen.borrow_mut());
            seen.borrow_mut().next_handle = seen_now.next_handle;
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
            if (0..4).all(|i| cpu.mem.rd8(CTX + 0x24E + i) == 0)
                || (0..4).all(|i| cpu.mem.rd8(CTX + 0x252 + i) == 0)
            {
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
}
