//! The SCUS leaves the field VM and the actor pool sit on, each against the
//! port that claims it: the system flag bank's set / clear / test
//! `FUN_8003CE08` / `FUN_8003CE34` / `FUN_8003CE64`, the operand loaders
//! `FUN_8003CE9C` / `FUN_8003CEB8` / `FUN_8003CED8`, the text packet length
//! `FUN_8003CA38`, and the handler finder and retire sweep `FUN_8003CF04` /
//! `FUN_8003CF40` over a linked actor list.

use crate::common::{Rng, scus_cpu};
use legaia_engine_core::world::World;
use legaia_engine_field::actor_handler::ActorHandler;
use legaia_engine_vm::field::dispatch_leaves::{
    load_u16_le, load_u24_le, load_u32_le, party_flag_test,
};
use psx_retail_exec::{Cpu, Stop, V0};

const FLAG_SET: u32 = 0x8003_CE08;
const FLAG_CLEAR: u32 = 0x8003_CE34;
const FLAG_TEST: u32 = 0x8003_CE64;
const LOAD16: u32 = 0x8003_CE9C;
const LOAD24: u32 = 0x8003_CEB8;
const LOAD32: u32 = 0x8003_CED8;
const PACKET_LEN: u32 = 0x8003_CA38;
const FIND: u32 = 0x8003_CF04;
const RETIRE: u32 = 0x8003_CF40;

/// `0x80084140 + 0x1618`, the system flag bank.
const BANK: u32 = 0x8008_5758;
/// The ids the test reaches: `0x1000` flags, 512 bytes of bank.
const BANK_BYTES: usize = 0x200;
const SCRATCH: u32 = 0x8010_0000;

fn call(cpu: &mut Cpu, entry: u32, args: &[u32]) -> u32 {
    cpu.regs.r[V0] = 0xDEAD_BEEF;
    assert_eq!(cpu.call(entry, args), Stop::Returned, "{entry:#x} {args:?}");
    cpu.regs.r[V0]
}

#[test]
fn the_system_flag_bank_matches_retail() {
    differential!(scus(FLAG_SET), scus(FLAG_CLEAR), scus(FLAG_TEST));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut rng = Rng(0x464C_4147);
    let mut w = World::new();
    let start: Vec<u8> = (0..BANK_BYTES).map(|_| rng.next() as u8).collect();
    cpu.mem.load(BANK, &start);
    w.flags.system_flags = start.clone();
    let (mut sets, mut hits) = (0, 0);
    for op in 0..20_000 {
        let idx = rng.below(0x1000) as u16;
        match rng.below(3) {
            0 => {
                call(&mut cpu, FLAG_SET, &[u32::from(idx)]);
                w.system_flag_set(idx);
                sets += 1;
            }
            1 => {
                call(&mut cpu, FLAG_CLEAR, &[u32::from(idx)]);
                w.system_flag_clear(idx);
            }
            _ => {
                let r = call(&mut cpu, FLAG_TEST, &[u32::from(idx)]);
                assert_eq!(
                    r,
                    u32::from(party_flag_test(u32::from(idx), &w.flags.system_flags)),
                    "op {op} test {idx:#x}"
                );
                assert_eq!(r != 0, w.system_flag_test(idx), "op {op} test {idx:#x}");
                hits += u32::from(r != 0);
            }
        }
        if op % 64 == 0 {
            let retail = cpu.mem.read(BANK, BANK_BYTES);
            assert_eq!(retail, w.flags.system_flags, "op {op}");
        }
    }
    assert_eq!(cpu.mem.read(BANK, BANK_BYTES), w.flags.system_flags);
    assert!(sets > 1000 && hits > 1000, "sets {sets} hits {hits}");
}

#[test]
fn the_operand_loaders_match_retail() {
    differential!(scus(LOAD16), scus(LOAD24), scus(LOAD32));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut rng = Rng(0x4C4F_4144);
    for _ in 0..5000 {
        let bytes = rng.next().to_le_bytes();
        // Odd addresses: the loaders read bytewise, so an unaligned operand is
        // the case a bytecode stream reaches.
        let at = SCRATCH + rng.below(16);
        cpu.mem.load(at, &bytes);
        let r16 = call(&mut cpu, LOAD16, &[at]);
        assert_eq!(
            r16,
            load_u16_le(&bytes) as i16 as i32 as u32,
            "{bytes:02x?}"
        );
        let r24 = call(&mut cpu, LOAD24, &[at]);
        assert_eq!(r24, load_u24_le(&bytes), "{bytes:02x?}");
        let r32 = call(&mut cpu, LOAD32, &[at]);
        assert_eq!(r32, load_u32_le(&bytes), "{bytes:02x?}");
    }
}

#[test]
fn the_packet_length_matches_retail() {
    differential!(scus(PACKET_LEN));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut rng = Rng(0x5041_434B);
    let mut escapes = 0;
    for _ in 0..5000 {
        let n = rng.below(40) as usize;
        let mut buf: Vec<u8> = (0..n)
            .map(|_| match rng.below(6) {
                // An escape lead, whose partner is skipped whatever it is.
                0 => 0xC0 | rng.below(16) as u8,
                1 => rng.below(0x20) as u8,
                _ => rng.next() as u8,
            })
            .collect();
        escapes += buf.iter().filter(|b| *b & 0xF0 == 0xC0).count();
        // Retail reads on until a terminator; the port stops at the slice.
        // A tail of terminators gives both the same end.
        buf.extend_from_slice(&[0; 4]);
        cpu.mem.load(SCRATCH, &buf);
        let r = call(&mut cpu, PACKET_LEN, &[SCRATCH]);
        let p = legaia_scene_formats::field_disasm::packet_length(&buf);
        assert_eq!(r as usize, p, "{buf:02x?}");
    }
    assert!(escapes > 1000);
}

#[test]
fn the_handler_finder_and_retire_sweep_match_retail() {
    differential!(scus(FIND), scus(RETIRE));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    // A real handler set: the ported ones and one the port keeps raw.
    let handlers = [
        0x801D_A930u32,
        0x801E_5154,
        0x801D_DC20,
        0x8003_7174,
        0x801F_0000,
    ];
    let head = SCRATCH;
    let node = |slot: usize| SCRATCH + 0x100 + slot as u32 * 0x40;
    let mut rng = Rng(0x4841_4E44);
    let (mut found, mut skipped_dead, mut retired) = (0, 0, 0);
    for round in 0..2000 {
        let mut w = World::new();
        let slots = w.actors.len().min(1 + rng.below(40) as usize);
        let mut live = Vec::new();
        for slot in 0..slots {
            if rng.below(4) == 0 {
                continue;
            }
            let h = handlers[rng.below(handlers.len() as u32) as usize];
            let flags = rng.next() & !0x8 | if rng.below(3) == 0 { 0x8 } else { 0 };
            let a = &mut w.actors[slot];
            a.active = true;
            a.handler = ActorHandler::from_va(h);
            a.physics.status_flags = flags;
            cpu.mem.wr32(node(slot) + 0xC, h);
            cpu.mem.wr32(node(slot) + 0x10, flags);
            live.push(slot);
        }
        // Retail's list holds the live actors; the port walks its pool in slot
        // order, so the list runs in slot order too.
        let mut next = 0u32;
        for &slot in live.iter().rev() {
            cpu.mem.wr32(node(slot), next);
            next = node(slot);
        }
        cpu.mem.wr32(head, next);
        let target = handlers[rng.below(handlers.len() as u32) as usize];
        let handler = ActorHandler::from_va(target);
        let r = call(&mut cpu, FIND, &[head, target]);
        let p = w.find_actor_by_handler(handler).map_or(0, node);
        assert_eq!(r, p, "round {round} find {target:#x}");
        found += u32::from(r != 0);
        skipped_dead +=
            u32::from(live.iter().any(|&s| {
                w.actors[s].handler == handler && w.actors[s].physics.status_flags & 8 != 0
            }));
        call(&mut cpu, RETIRE, &[head, target]);
        retired += w.retire_actors_by_handler(handler);
        for &slot in &live {
            assert_eq!(
                cpu.mem.rd32(node(slot) + 0x10),
                w.actors[slot].physics.status_flags,
                "round {round} slot {slot}"
            );
        }
    }
    assert!(
        found > 500 && skipped_dead > 300 && retired > 1000,
        "{found} {skipped_dead} {retired}"
    );
}
