//! The two SCUS readers of the per-actor default-move table `0x801C6470`: the
//! motion-pause kick `FUN_8003C9AC` over the scene actor list against
//! `motion_pause_kick`, and the touch post `FUN_8003D038` against
//! `post_touch`. The table carries the `0x8C` "no move" sentinel on a share of
//! its records, and the list carries actors on and off each gate.

use crate::common::{Rng, scus_cpu};
use legaia_engine_field::motion_pause::{PauseKickActor, motion_pause_kick};
use legaia_engine_motion_vm::motion_vm::post_touch;
use psx_retail_exec::Stop;

const KICK: u32 = 0x8003_C9AC;
const POST: u32 = 0x8003_D038;
const TABLE: u32 = 0x801C_6470;
const LIST_HEAD: u32 = 0x8007_C354;
const MAILBOX: u32 = 0x8007_3F1C;
const HEAD: u32 = 0x8011_FF00;
const NODES: u32 = 0x8012_0000;
const STRIDE: u32 = 0x100;

fn seed_table(cpu: &mut psx_retail_exec::Cpu, rng: &mut Rng, ids: usize) -> Vec<u8> {
    let table: Vec<u8> = (0..ids * 4)
        .map(|i| {
            if i % 4 == 0 && rng.below(3) == 0 {
                0x8C
            } else {
                rng.next() as u8
            }
        })
        .collect();
    cpu.mem.load(TABLE, &table);
    table
}

#[test]
fn the_pause_kick_reloads_what_retail_reloads() {
    differential!(scus(KICK));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut rng = Rng(0x5041_5553);
    let (mut kicked, mut sentinel) = (0, 0);
    for round in 0..3000 {
        let ids = 64;
        let table = seed_table(&mut cpu, &mut rng, ids);
        let n = rng.below(24) as usize;
        let mut actors = Vec::new();
        for i in 0..n {
            let node = NODES + i as u32 * STRIDE;
            let a = PauseKickActor {
                flags: rng.next() & !0x2_0000 | if rng.below(4) != 0 { 0x2_0000 } else { 0 },
                has_motion_stream: rng.below(5) != 0,
                actor_id: rng.below(ids as u32) as u16,
                move_id: rng.next() as u16,
                move_id_mirror: rng.next() as u16,
            };
            let next = if i + 1 < n { node + STRIDE } else { 0 };
            cpu.mem.wr32(node, next);
            cpu.mem.wr32(node + 0x10, a.flags);
            cpu.mem.wr32(
                node + 0x80,
                if a.has_motion_stream { 0x8013_0000 } else { 0 },
            );
            cpu.mem.wr16(node + 0x50, a.actor_id);
            cpu.mem.wr16(node + 0x5C, a.move_id);
            cpu.mem.wr16(node + 0x88, a.move_id_mirror);
            actors.push(a);
        }
        // The list global holds the head node, whose `+0` is the first actor.
        cpu.mem.wr32(LIST_HEAD, HEAD);
        cpu.mem.wr32(HEAD, if n == 0 { 0 } else { NODES });
        assert_eq!(cpu.call(KICK, &[]), Stop::Returned);
        kicked += motion_pause_kick(&mut actors, &table);
        sentinel += actors
            .iter()
            .filter(|a| a.flags & 0x2_0000 != 0 && a.has_motion_stream)
            .filter(|a| table[usize::from(a.actor_id) * 4] == 0x8C)
            .count();
        for (i, a) in actors.iter().enumerate() {
            let node = NODES + i as u32 * STRIDE;
            assert_eq!(
                (cpu.mem.rd16(node + 0x5C), cpu.mem.rd16(node + 0x88)),
                (a.move_id, a.move_id_mirror),
                "round {round} actor {i}"
            );
        }
    }
    assert!(kicked > 5000 && sentinel > 2000, "{kicked} {sentinel}");
}

#[test]
fn the_touch_post_posts_what_retail_posts() {
    differential!(scus(POST));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut rng = Rng(0x544F_5543);
    let (mut posted, mut held) = (0, 0);
    for _ in 0..4000 {
        let table = seed_table(&mut cpu, &mut rng, 64);
        let index = rng.below(64) as usize;
        let before = rng.next();
        cpu.mem.wr32(MAILBOX, before);
        assert_eq!(cpu.call(POST, &[index as u32]), Stop::Returned);
        let port = post_touch(&table, index).unwrap_or(before);
        assert_eq!(cpu.mem.rd32(MAILBOX), port, "index {index}");
        if port == before {
            held += 1;
        } else {
            posted += 1;
        }
    }
    assert!(posted > 1000 && held > 500, "{posted} {held}");
}
