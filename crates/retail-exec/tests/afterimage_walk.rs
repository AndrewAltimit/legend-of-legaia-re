//! The arts after-image walk `FUN_80049348` against `plan_ghosts`: party
//! and monster seats, every anim rate `0..=8` and wider bytes (rate `0`
//! runs the unguarded divide as the chip does), history ids either side of
//! the `0x11` gate, and the per-character base words.
//!
//! The matrix build `FUN_8003D344`, the mesh-variant swap `FUN_8004CCD4` and
//! the body draw `FUN_80048A08` are hooked: each ghost's draw sees the node
//! as the plan says (the ring's position, cursor and clip at the plan's
//! depth, `ghost_draw_word` of the plan's colour, the push bit and full
//! weight), and the node is compared whole after the walk.

use crate::common::{Rng, diff_offsets, scus_cpu};
use legaia_engine_battle::battle_afterimage::{
    GHOST_COLOR_MONSTER, GHOST_COLOR_PARTY, ghost_draw_word, ghost_eligible, plan_ghosts,
};
use psx_retail_exec::{A0, A1, A2, A3, SP, Stop};
use std::cell::RefCell;
use std::rc::Rc;

const WALK: u32 = 0x8004_9348;
const MATRIX: u32 = 0x8003_D344;
const VARIANT: u32 = 0x8004_CCD4;
const DRAW: u32 = 0x8004_8A08;
const ACTORS: u32 = 0x801C_9370;
const ACTOR: u32 = 0x8013_0000;
const ACTOR_LEN: usize = 0x400;
const PARTY_IDS: u32 = 0x8007_BD10;
const NODE: u32 = 0x8012_0000;
const NODE_LEN: usize = 0x80;

#[derive(Debug, Clone, PartialEq, Eq)]
enum Call {
    Matrix([u8; 8]),
    Variant(u32, u32, i32, u32, u32),
    Draw {
        cursor: u16,
        word: u32,
        clip: u32,
        flags: u32,
        weight: u16,
    },
}

#[test]
fn the_afterimage_walk_draws_the_planned_ghosts() {
    differential!(scus(WALK));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.hardware_zero_div = true;
    for s in 0..8 {
        cpu.mem.wr32(ACTORS + s * 4, ACTOR + s * ACTOR_LEN as u32);
    }
    let calls = Rc::new(RefCell::new(Vec::new()));
    {
        let calls = calls.clone();
        cpu.hook(MATRIX, move |r, m| {
            assert_eq!(r.r[A1], r.r[A0] + 0x18);
            let b = m.read(r.r[A0], 8);
            calls.borrow_mut().push(Call::Matrix(b.try_into().unwrap()));
        });
    }
    {
        let calls = calls.clone();
        cpu.hook(VARIANT, move |r, m| {
            let sp10 = m.rd32(r.r[SP] + 0x10);
            calls.borrow_mut().push(Call::Variant(
                r.r[A0],
                r.r[A1],
                r.r[A2] as i32,
                r.r[A3],
                sp10,
            ));
        });
    }
    {
        let calls = calls.clone();
        cpu.hook(DRAW, move |r, m| {
            let n = r.r[A0];
            calls.borrow_mut().push(Call::Draw {
                cursor: m.rd16(n + 0x68),
                word: m.rd32(n + 0x74),
                clip: m.rd32(n + 0x4C),
                flags: m.rd32(n + 0x10),
                weight: m.rd16(n + 0x78),
            });
        });
    }
    let mut rng = Rng(0x4748_4F53);
    let (mut drawn, mut seats) = ([0u32; 3], [0u32; 2]);
    for n in 0..20_000 {
        let slot = rng.below(7) as u8;
        let monster = slot >= 3;
        let char_id = 1 + rng.below(3) as u8;
        cpu.mem.wr8(PARTY_IDS + u32::from(slot.min(2)), char_id);
        let mut actor: Vec<u8> = (0..ACTOR_LEN).map(|_| rng.next() as u8).collect();
        actor[0x21D] = match rng.below(4) {
            0 => rng.next() as u8,
            _ => rng.below(9) as u8,
        };
        for d in 0..33 {
            actor[0x1FB + d] = match rng.below(3) {
                0 => rng.next() as u8,
                1 => 0x10 + rng.below(3) as u8,
                _ => 0x11,
            };
        }
        let a = ACTOR + u32::from(slot) * ACTOR_LEN as u32;
        cpu.mem.load(a, &actor);
        let mut node: Vec<u8> = (0..NODE_LEN).map(|_| rng.next() as u8).collect();
        node[0x5A..0x5C].copy_from_slice(&u16::from(slot).to_le_bytes());
        cpu.mem.load(NODE, &node);
        calls.borrow_mut().clear();
        assert_eq!(cpu.call(WALK, &[NODE]), Stop::Returned, "n {n}");

        let rate = actor[0x21D];
        let base = if monster {
            GHOST_COLOR_MONSTER
        } else {
            GHOST_COLOR_PARTY[usize::from(char_id) - 1]
        };
        let plans = plan_ghosts(rate, monster, base, |d| ghost_eligible(actor[0x1FB + d]));
        let rd16 = |o: usize| u16::from_le_bytes([actor[o], actor[o + 1]]);
        let rd32 = |o: usize| u32::from_le_bytes(actor[o..o + 4].try_into().unwrap());
        let node_flags = u32::from_le_bytes(node[0x10..0x14].try_into().unwrap()) | 0x80_0000;
        let mut want = Vec::new();
        for p in &plans {
            let d = p.depth;
            want.push(Call::Matrix(
                actor[0x4C + 8 * d..0x54 + 8 * d].try_into().unwrap(),
            ));
            let cursor = rd16(0x17A + 2 * d);
            let clip = rd32(0x234 + 4 * d);
            if !monster {
                let sp10 = u32::from_le_bytes(node[0x44..0x48].try_into().unwrap()) + 4;
                want.push(Call::Variant(
                    u32::from(slot),
                    u32::from(char_id - 1),
                    i32::from(cursor as i16) >> 4,
                    clip,
                    sp10,
                ));
            }
            want.push(Call::Draw {
                cursor,
                word: ghost_draw_word(p.color),
                clip,
                flags: node_flags,
                weight: 0x1000,
            });
        }
        want.push(Call::Matrix(actor[0x34..0x3C].try_into().unwrap()));
        let ctx = format!("n {n} slot {slot} rate {rate} plans {plans:?}");
        assert_eq!(*calls.borrow(), want, "{ctx}");

        // The node after the walk: the live pose back, the push bit off,
        // full weight, and the colour word past the last decay.
        let mut want_node = node.clone();
        want_node[0x10..0x14].copy_from_slice(&(node_flags & !0x80_0000).to_le_bytes());
        want_node[0x14..0x1C].copy_from_slice(&actor[0x34..0x3C]);
        want_node[0x78..0x7A].copy_from_slice(&0x1000u16.to_le_bytes());
        let last = match plans.last() {
            None => u32::from(base[0]) | u32::from(base[1]) << 8 | u32::from(base[2]) << 16,
            Some(p) => (ghost_draw_word(p.color) & 0xFF_FFFF).wrapping_add(0xFFEF_EFF0),
        };
        want_node[0x74..0x78].copy_from_slice(&last.to_le_bytes());
        let got = cpu.mem.read(NODE, NODE_LEN);
        assert!(
            got == want_node,
            "{ctx}: node {:?}",
            diff_offsets(&got, &want_node)
        );
        drawn[plans.len()] += 1;
        seats[usize::from(monster)] += 1;
    }
    assert!(drawn.iter().all(|&c| c > 1000), "{drawn:?}");
    assert!(seats.iter().all(|&c| c > 1000), "{seats:?}");
}
