//! The three-actor talk spawner `FUN_801D2D38` (PROT 0897), field op `43 02`'s
//! callee, with the system-flag leaves `FUN_8003CE08` / `FUN_8003CE34` /
//! `FUN_8003CE64` and the MAN-header read `FUN_8003D064` run for real and the
//! controller's allocation `FUN_80020DE0` hooked, against the op through
//! `World::step_field`: on the first arm the party collapse to the leader,
//! the talk flags `0x10..0x12` and the lock `0xD`; on a re-arm the saved
//! poses written back onto the instruction's participants, pairing record
//! `i` with participant `i`; and the controller record's script word and
//! record byte.
//!
//! The saved-pose table `0x800845E4` is written by the controller's state 0
//! (`FUN_801D27E0`) on the frame after an arm, where the port captures at
//! arm time; a re-arm round seeds the table with the poses the port's first
//! arm captured.

use crate::common::{Rng, STACK_TOP, disc, scus_cpu};
use legaia_engine_core::world::World;
use legaia_engine_mode::mode::SceneMode;
use psx_retail_exec::{Cpu, Stop, V0};

const SPAWN_TALK: u32 = 0x801D_2D38;
const ALLOC: u32 = 0x8002_0DE0;
const OVERLAY_BASE: u32 = 0x801C_E818;
const FLAGS: u32 = 0x8008_5758;
const COUNT: u32 = 0x8008_4594;
const LEADER: u32 = 0x8008_4597;
const IDS: u32 = 0x8008_4598;
const SAVED: u32 = 0x8008_45E4;
const BUSY: u32 = 0x8007_B648;
const MAN_PTR: u32 = 0x8007_B898;
const MAN: u32 = 0x8015_0000;
const CTRL: u32 = 0x8013_0000;
const ACTORS: u32 = 0x8014_0000;
const TEMPLATE: u32 = 0x801F_22C4;
const LIST_HEAD: u32 = 0x8007_C34C;

fn flag(cpu: &mut Cpu, n: u16) -> bool {
    cpu.mem.rd8(FLAGS + u32::from(n >> 3)) & (0x80 >> (n & 7)) != 0
}

fn set_flag(cpu: &mut Cpu, n: u16, on: bool) {
    let at = FLAGS + u32::from(n >> 3);
    let b = cpu.mem.rd8(at);
    let bit = 0x80u8 >> (n & 7);
    cpu.mem.wr8(at, if on { b | bit } else { b & !bit });
}

#[test]
fn the_three_actor_talk_arms_what_retail_arms() {
    differential!(prot(897, SPAWN_TALK));
    let (Some(mut cpu), Some(d)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem
        .load(OVERLAY_BASE, &d.read_entry(897).expect("PROT 0897"));
    cpu.mem.wr32(LIST_HEAD, 0x8016_0000);
    cpu.hook(ALLOC, |r, m| {
        assert_eq!((r.r[4], r.r[5]), (TEMPLATE, 0x8016_0000));
        m.load(CTRL, &[0u8; 0x100]);
        r.r[V0] = CTRL;
    });
    let mut rng = Rng(0x5441_4C4B);
    let (mut first, mut rearm) = (0, 0);
    for round in 0..3000 {
        let ids = [5u8, 6, 7];
        let leader = rng.below(3) as u8;
        let members: Vec<u8> = {
            let mut m = vec![leader];
            for c in 0..3u8 {
                if c != leader && rng.below(2) == 0 {
                    m.push(c);
                }
            }
            m
        };
        let word = rng.next() as u16;
        let byte = rng.next() as u8;
        let header: [i16; 3] = std::array::from_fn(|_| rng.below(0x40) as i16);
        let talk_flags: [bool; 3] = std::array::from_fn(|_| rng.below(2) == 0);
        let poses: Vec<((i16, i16, i16), i16)> = (0..3)
            .map(|_| {
                (
                    (rng.next() as i16, rng.next() as i16, rng.next() as i16),
                    rng.next() as i16 & 0xFFF,
                )
            })
            .collect();
        let rearming = round % 2 == 1;
        let label = format!("round {round} leader {leader} members {members:?}");

        // The port: a field world with the party and the three participants.
        let mut w = World::new();
        w.mode = SceneMode::Field;
        w.party.party_actor_slots = members.iter().map(|&m| Some(m)).collect();
        w.party.party_leader_slot = Some(leader);
        for (k, f) in talk_flags.iter().enumerate() {
            if *f {
                w.system_flag_set(0x10 + k as u16);
            }
        }
        for (k, &(p, h)) in poses.iter().enumerate() {
            w.npcs.positions.insert(ids[k], (p.0, p.2));
            w.npcs.headings.insert(ids[k], h);
        }
        let op = |w: &mut World| {
            let mut b = vec![0x43, 0x02, ids[0], ids[1], ids[2]];
            b.extend_from_slice(&word.to_le_bytes());
            b.push(byte);
            w.field_bytecode = b;
            w.field_pc = 0;
            let _ = w.step_field();
        };
        // A re-arm: the first arm captured `poses`, the talk then moved
        // everyone, and the instruction runs again under the lock.
        let moved: Vec<((i16, i16, i16), i16)> = (0..3)
            .map(|_| {
                (
                    (rng.next() as i16, rng.next() as i16, rng.next() as i16),
                    rng.next() as i16,
                )
            })
            .collect();
        if rearming {
            op(&mut w);
            for (k, &(p, h)) in moved.iter().enumerate() {
                w.npcs.positions.insert(ids[k], (p.0, p.2));
                w.npcs.headings.insert(ids[k], h);
            }
        }
        let party_before = w.party.party_actor_slots.clone();
        let flags_before: [bool; 3] = std::array::from_fn(|k| w.system_flag_test(0x10 + k as u16));
        op(&mut w);
        let talk = w.dialog.three_actor_talk.expect("the talk is staged");

        // Retail, from the matching state.
        let flags_noise: Vec<u8> = (0..0x40).map(|_| rng.next() as u8).collect();
        cpu.mem.load(FLAGS, &flags_noise);
        set_flag(&mut cpu, 0xD, rearming);
        for (k, f) in flags_before.iter().enumerate() {
            set_flag(&mut cpu, 0x10 + k as u16, *f);
        }
        let party_ids: Vec<u8> = if rearming {
            party_before.iter().map(|s| s.unwrap_or(0)).collect()
        } else {
            members.clone()
        };
        cpu.mem.wr8(COUNT, party_ids.len() as u8);
        cpu.mem.wr8(LEADER, leader);
        for k in 0..4 {
            cpu.mem
                .wr8(IDS + k, party_ids.get(k as usize).copied().unwrap_or(0xEE));
        }
        cpu.mem.wr8(BUSY, 0x55);
        let mut man = vec![0u8; 0x40];
        for (k, h) in header.iter().enumerate() {
            man[0x22 + 2 * k..0x24 + 2 * k].copy_from_slice(&h.to_le_bytes());
        }
        cpu.mem.load(MAN, &man);
        cpu.mem.wr32(MAN_PTR, MAN);
        for (k, &(p, h)) in poses.iter().enumerate() {
            let rec = SAVED + 0x10 * k as u32;
            cpu.mem.wr32(rec, p.0 as i32 as u32);
            cpu.mem.wr32(rec + 4, p.1 as i32 as u32);
            cpu.mem.wr32(rec + 8, p.2 as i32 as u32);
            cpu.mem.wr32(rec + 0xC, h as i32 as u32);
        }
        let actor = |k: usize| ACTORS + 0x100 * k as u32;
        for (k, &(p, h)) in moved.iter().enumerate() {
            cpu.mem.load(actor(k), &[0u8; 0x100]);
            cpu.mem.wr16(actor(k) + 0x14, p.0 as u16);
            cpu.mem.wr16(actor(k) + 0x16, p.1 as u16);
            cpu.mem.wr16(actor(k) + 0x18, p.2 as u16);
            cpu.mem.wr16(actor(k) + 0x26, h as u16);
        }
        cpu.mem.wr32(STACK_TOP + 0x10, u32::from(byte));
        assert_eq!(
            cpu.call(
                SPAWN_TALK,
                &[actor(0), actor(1), actor(2), u32::from(word)]
            ),
            Stop::Returned,
            "{label}"
        );
        assert_eq!(cpu.regs.r[V0], CTRL, "{label}");
        for k in 0..3 {
            assert_eq!(cpu.mem.rd32(CTRL + 0x80 + 4 * k), actor(k as usize));
        }
        assert_eq!(cpu.mem.rd16(CTRL + 0x50), word, "{label}");
        assert_eq!(talk.script_id, word, "{label}");
        assert_eq!(cpu.mem.rd16(CTRL + 0x54), 0, "{label}");
        // The record byte less the header's first two partition counts; the
        // spawner adds them back, so the byte as written is the record.
        let ctr = cpu.mem.rd16(CTRL + 0x72);
        assert_eq!(
            ctr.wrapping_add(header[0] as u16).wrapping_add(header[1] as u16),
            u16::from(talk.duration),
            "{label}"
        );
        assert_eq!(talk.actor_ids, ids, "{label}");
        assert!(flag(&mut cpu, 0xD) && w.system_flag_test(0xD), "{label}");
        // Every flag the routine does not name is untouched.
        for n in (0..0x200u16).filter(|n| !matches!(n, 0xD | 0x10..=0x12)) {
            let was = flags_noise[usize::from(n >> 3)] & (0x80 >> (n & 7)) != 0;
            assert_eq!(flag(&mut cpu, n), was, "{label}: flag {n:#x}");
        }
        for k in 0..3u16 {
            assert_eq!(
                flag(&mut cpu, 0x10 + k),
                w.system_flag_test(0x10 + k),
                "{label}: flag {:#x}",
                0x10 + k
            );
        }
        if rearming {
            // The party, the talk flags and the busy byte stand.
            assert_eq!(cpu.mem.rd8(COUNT) as usize, party_ids.len(), "{label}");
            assert_eq!(w.party.party_actor_slots, party_before, "{label}");
            assert_eq!(cpu.mem.rd8(BUSY), 0x55, "{label}");
            for k in 0..3usize {
                let a = actor(k);
                let ((x, y, z), h) = poses[k];
                assert_eq!(
                    (
                        cpu.mem.rd16(a + 0x14) as i16,
                        cpu.mem.rd16(a + 0x16) as i16,
                        cpu.mem.rd16(a + 0x18) as i16,
                        cpu.mem.rd16(a + 0x26) as i16
                    ),
                    (x, y, z, h),
                    "{label}: participant {k}"
                );
                assert_eq!(w.npcs.positions.get(&ids[k]), Some(&(x, z)), "{label}");
                assert_eq!(w.npcs.headings.get(&ids[k]), Some(&h), "{label}");
            }
            rearm += 1;
        } else {
            assert_eq!(cpu.mem.rd8(COUNT), 1, "{label}");
            let list: Vec<u8> = (0..4).map(|k| cpu.mem.rd8(IDS + k)).collect();
            assert_eq!(list, vec![leader, 0, 0, 0], "{label}");
            assert_eq!(w.party.party_actor_slots, vec![Some(leader)], "{label}");
            assert_eq!(cpu.mem.rd8(BUSY), 0, "{label}");
            // The participants are not moved on a first arm.
            for (k, &(p, h)) in moved.iter().enumerate() {
                assert_eq!(cpu.mem.rd16(actor(k) + 0x14) as i16, p.0);
                assert_eq!(cpu.mem.rd16(actor(k) + 0x26) as i16, h);
            }
            first += 1;
        }
    }
    assert!(first > 1000 && rearm > 1000);
}
