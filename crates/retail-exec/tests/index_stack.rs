//! Two small SCUS kernels the field's fog pool and VRAM-copy effects sit on:
//! the halfword index stack - the identity seed `FUN_8001FA00`, the pop
//! `FUN_8001FA34` and the push `FUN_8001FA68` - as a stream of operations
//! against `init_identity_index_list` / `sprite_stack_pop` /
//! `sprite_stack_push`, and the `MoveImage` packet builder `FUN_80057914`
//! against `build_packet`.

use crate::common::{Rng, scus_cpu};
use legaia_engine_system::movie::cutscene::{sprite_stack_pop, sprite_stack_push};
use legaia_engine_system::scus_leaf_kernels::init_identity_index_list;
use legaia_engine_vm::vram_rect_copy::{SrcRect, build_packet};
use psx_retail_exec::{Cpu, Stop, V0};

const SEED: u32 = 0x8001_FA00;
const POP: u32 = 0x8001_FA34;
const PUSH: u32 = 0x8001_FA68;
const PACKET: u32 = 0x8005_7914;
const COUNT: u32 = 0x8014_0000;
const TABLE: u32 = 0x8014_0004;
const SLOTS: usize = 0x50;
const PKT: u32 = 0x8014_1000;
const RECT: u32 = 0x8014_2000;

fn call(cpu: &mut Cpu, entry: u32, args: &[u32]) -> u32 {
    cpu.regs.r[V0] = 0xDEAD_BEEF;
    assert_eq!(
        cpu.call(entry, args),
        Stop::Returned,
        "{entry:#x} {args:x?}"
    );
    cpu.regs.r[V0]
}

#[test]
fn the_index_stack_behaves_as_retail_does() {
    differential!(scus(SEED), scus(POP), scus(PUSH));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut rng = Rng(0x5354_4B31);
    let (mut pops, mut empties, mut pushes) = (0, 0, 0);
    for session in 0..200 {
        // A count below `-1` sends retail's push below the table, over the
        // count word itself; the one seed on the disc is `0x50`.
        let n = [SLOTS as i16, 1, 0, 7][rng.below(4) as usize];
        let junk: Vec<u8> = (0..SLOTS * 2 + 4).map(|_| rng.next() as u8).collect();
        cpu.mem.load(COUNT, &junk);
        let mut list: Vec<u16> = (0..SLOTS)
            .map(|i| u16::from_le_bytes([junk[4 + i * 2], junk[5 + i * 2]]))
            .collect();
        call(&mut cpu, SEED, &[COUNT, TABLE, n as i32 as u32]);
        let mut count = init_identity_index_list(&mut list, n);
        let mut table: Vec<i16> = list.iter().map(|v| *v as i16).collect();
        for op in 0..300 {
            if rng.below(2) == 0 {
                let r = call(&mut cpu, POP, &[COUNT, TABLE]);
                let p = sprite_stack_pop(&mut count, &table);
                assert_eq!(
                    r as i32,
                    p.map_or(-1, i32::from),
                    "session {session} op {op} pop"
                );
                if p.is_some() {
                    pops += 1;
                } else {
                    empties += 1;
                }
            } else if count < SLOTS as i16 - 1 {
                let v = rng.below(SLOTS as u32) as i16;
                call(&mut cpu, PUSH, &[COUNT, TABLE, 0, v as u32]);
                sprite_stack_push(&mut count, &mut table, v);
                pushes += 1;
            }
            assert_eq!(
                cpu.mem.rd16(COUNT) as i16,
                count,
                "session {session} op {op}"
            );
            for (i, t) in table.iter().enumerate() {
                assert_eq!(
                    cpu.mem.rd16(TABLE + i as u32 * 2) as i16,
                    *t,
                    "session {session} op {op} slot {i}"
                );
            }
        }
    }
    assert!(
        pops > 10_000 && empties > 1000 && pushes > 10_000,
        "{pops} {empties} {pushes}"
    );
}

#[test]
fn the_move_image_packet_matches_retail() {
    differential!(scus(PACKET));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut rng = Rng(0x504B_5431);
    let mut degenerate = 0;
    for _ in 0..20_000 {
        let pick = |rng: &mut Rng| match rng.below(4) {
            0 => 0,
            1 => -(rng.below(0x40) as i16),
            _ => rng.below(0x400) as i16,
        };
        let src = SrcRect {
            x: pick(&mut rng),
            y: pick(&mut rng),
            w: pick(&mut rng),
            h: pick(&mut rng),
        };
        let (dx, dy) = (rng.next() as i16, rng.next() as i16);
        cpu.mem.wr16(RECT, src.x as u16);
        cpu.mem.wr16(RECT + 2, src.y as u16);
        cpu.mem.wr16(RECT + 4, src.w as u16);
        cpu.mem.wr16(RECT + 6, src.h as u16);
        let head = rng.next() & 0x00FF_FFFF;
        cpu.mem.wr32(PKT, head);
        call(
            &mut cpu,
            PACKET,
            &[PKT, RECT, dx as i32 as u32, dy as i32 as u32],
        );
        let p = build_packet(src, dx, dy);
        let label = format!("{src:?} -> ({dx}, {dy})");
        assert_eq!(cpu.mem.rd8(PKT + 3), p.tag_len, "{label}");
        assert_eq!(cpu.mem.rd32(PKT) & 0x00FF_FFFF, head, "{label}");
        assert_eq!(
            [
                cpu.mem.rd32(PKT + 4),
                cpu.mem.rd32(PKT + 8),
                cpu.mem.rd32(PKT + 0xC),
                cpu.mem.rd32(PKT + 0x10),
                cpu.mem.rd32(PKT + 0x14),
            ],
            [p.word1, p.command, p.src, p.dst, p.extent],
            "{label}"
        );
        degenerate += u32::from(p.tag_len == 0);
    }
    assert!(degenerate > 5000, "{degenerate}");
}
