//! The bag's accessor family against `RetailInventory`, as a stream of mixed
//! operations over one shared array: `FUN_8004313C` the window, `FUN_800421D4`
//! add, `FUN_80042310` consume by id, `FUN_80043048` consume by slot,
//! `FUN_800423E0` normalize, `FUN_80042EE0` and `FUN_80042F4C` the two finds.
//! The whole 256-slot array has to match after every operation.

use crate::common::{Rng, scus_cpu};
use legaia_save::retail_inventory::{AddOutcome, ItemWindow, RetailInventory};
use psx_retail_exec::{Cpu, GP, Stop, V0};

const BAG: u32 = 0x8008_5958;
const SLOTS: usize = 256;
const MEMBERS: u32 = 0x8008_4594;
const FIRST_MEMBER: u32 = 0x8008_4598;
const FLAG_BANK: u32 = 0x8008_5758;

fn bag(cpu: &mut Cpu) -> Vec<(u8, u8)> {
    (0..SLOTS as u32)
        .map(|i| (cpu.mem.rd8(BAG + i * 2), cpu.mem.rd8(BAG + i * 2 + 1)))
        .collect()
}

fn call(cpu: &mut Cpu, entry: u32, args: &[u32]) -> u32 {
    cpu.regs.r[V0] = 0xDEAD_BEEF;
    assert_eq!(cpu.call(entry, args), Stop::Returned, "{entry:#x} {args:?}");
    cpu.regs.r[V0]
}

#[test]
fn the_bag_behaves_as_retail_does() {
    differential!(
        scus(0x8004_21D4),
        scus(0x8004_2310),
        scus(0x8004_23E0),
        scus(0x8004_2EE0),
        scus(0x8004_2F4C),
        scus(0x8004_3048),
        scus(0x8004_313C)
    );
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let gp = cpu.regs.r[GP];
    let mut rng = Rng(0x4241_4731);
    let (mut holes, mut merges) = (0, 0);
    for session in 0..300 {
        // A party shape, its window, and a bag dense enough to reach the caps.
        let members = rng.below(4) as u8;
        let flag = rng.below(2) == 0;
        let high = rng.below(2) == 0;
        cpu.mem.wr8(MEMBERS, members);
        cpu.mem.wr8(FIRST_MEMBER, u8::from(high));
        cpu.mem.wr8(FLAG_BANK + 2, if flag { 0x08 } else { 0 });
        // A window from before, which a party of none leaves installed.
        cpu.mem.wr16(gp + 0x2D2, 0);
        cpu.mem.wr16(gp + 0x2D4, 0x100);
        cpu.mem.wr16(gp + 0x2D6, 0x100);
        call(&mut cpu, 0x8004_313C, &[]);
        let mut port = RetailInventory::new(BAG, SLOTS);
        if let Some(w) = ItemWindow::select(members, flag, high) {
            port.set_window(w);
        } else {
            port.set_window(ItemWindow::Full);
        }
        let start = cpu.mem.rd16(gp + 0x2D2);
        let end = cpu.mem.rd16(gp + 0x2D4);
        assert_eq!(
            port.window_bounds(),
            (usize::from(start), usize::from(end)),
            "members {members} flag {flag} high {high}"
        );
        let mut raw = vec![(0u8, 0u8); SLOTS];
        let ids = 1 + rng.below(0x60);
        for s in raw.iter_mut() {
            if rng.below(3) == 0 {
                *s = (1 + rng.below(ids) as u8, rng.below(100) as u8);
            }
        }
        for (i, (id, n)) in raw.iter().enumerate() {
            cpu.mem.wr8(BAG + i as u32 * 2, *id);
            cpu.mem.wr8(BAG + i as u32 * 2 + 1, *n);
        }
        port.slots_mut().copy_from_slice(&raw);
        for op in 0..60 {
            let id = 1 + rng.below(ids + 4) as u8;
            let qty = [0u8, 1, 1, 2, 50, 99, 100, 255][rng.below(8) as usize];
            let what = rng.below(7);
            let label = match what {
                0 | 1 => {
                    call(&mut cpu, 0x8004_21D4, &[u32::from(id), u32::from(qty)]);
                    let out = port.add(id, qty);
                    merges += u32::from(matches!(out, AddOutcome::Merged { .. }));
                    if let AddOutcome::OobIdWrite { .. } = out {
                        // The one write the model surfaces as data: the id byte
                        // one slot past a full window.
                        cpu.mem.wr8(BAG + u32::from(end) * 2, raw_at(&port, end));
                    }
                    "add"
                }
                2 => {
                    call(&mut cpu, 0x8004_2310, &[u32::from(id), u32::from(qty)]);
                    port.consume(id, qty);
                    "consume"
                }
                3 => {
                    let slot = rng.below(SLOTS as u32 + 8) as i16;
                    let echo = rng.below(256) as u8;
                    let r = call(
                        &mut cpu,
                        0x8004_3048,
                        &[slot as u32, u32::from(qty), u32::from(echo)],
                    );
                    let p = port.consume_slot(slot, qty, echo);
                    assert_eq!(r as u8, p, "consume_slot {slot} {qty}");
                    holes += u32::from(p == 0);
                    "consume_slot"
                }
                4 => {
                    call(&mut cpu, 0x8004_23E0, &[]);
                    port.normalize();
                    "normalize"
                }
                5 => {
                    let r = call(&mut cpu, 0x8004_2EE0, &[u32::from(id)]);
                    let p = port.find_slot(id).map_or(0x100, |s| s as i32);
                    assert_eq!(r as i16 as i32, p, "find_slot {id}");
                    "find"
                }
                _ => {
                    let r = call(&mut cpu, 0x8004_2F4C, &[u32::from(id)]);
                    assert_eq!(r as u8, port.find_count(id), "find_count {id}");
                    "count"
                }
            };
            let retail = bag(&mut cpu);
            let d: Vec<_> = retail
                .iter()
                .zip(port.slots())
                .enumerate()
                .filter(|(_, (a, b))| a != b)
                .take(4)
                .collect();
            assert!(
                d.is_empty(),
                "session {session} op {op} {label} id {id} qty {qty}: {d:?} window {start}..{end}"
            );
        }
    }
    assert!(
        holes > 100 && merges > 100,
        "{holes} holes, {merges} merges"
    );
}

fn raw_at(port: &RetailInventory, end: u16) -> u8 {
    port.slots().get(usize::from(end)).map_or(0, |s| s.0)
}

/// `FUN_80035C24`, the window kernel's cursor auto-repeat.
#[test]
fn a_held_direction_repeats_when_retail_repeats() {
    differential!(scus(0x8003_5C24));
    use legaia_engine_system::retail_pad::{MENU_REPEAT_MASK, MENU_REPEAT_MASK_BATTLE, MenuRepeat};
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    const HELD: u32 = 0x8007_B850;
    const EDGE: u32 = 0x8007_B874;
    const MODE: u32 = 0x8007_B83C;
    let gp = cpu.regs.r[psx_retail_exec::GP];
    let mut rng = Rng(0x5245_5054);
    for battle in [false, true] {
        let mut port = MenuRepeat::default();
        let mask = if battle {
            MENU_REPEAT_MASK_BATTLE
        } else {
            MENU_REPEAT_MASK
        };
        cpu.mem.wr16(MODE, if battle { 0x15 } else { 3 });
        cpu.mem.wr32(gp + 0x854, 0);
        cpu.mem.wr32(gp + 0x864, 0);
        let mut held = 0u16;
        let mut fired = 0;
        for frame in 0..40000 {
            if rng.below(30) == 0 {
                held = [
                    0x1000, 0x4000, 0x2000, 0x8000, 0x5000, 0x0004, 0x0040, 0x1040, 0,
                ][rng.below(9) as usize];
            }
            let dt = [1u32, 1, 2, 3][rng.below(4) as usize];
            let edge = rng.next() & 0xF0;
            cpu.mem.wr32(HELD, u32::from(held));
            cpu.mem.wr32(EDGE, edge);
            cpu.mem.wr8(0x1F80_0393, dt as u8);
            cpu.regs.r[psx_retail_exec::GP] = gp;
            assert_eq!(cpu.call(0x8003_5C24, &[]), psx_retail_exec::Stop::Returned);
            let retail = cpu.mem.rd32(gp + 0x86C);
            let bits = port.step(held, mask, dt);
            assert_eq!(
                retail,
                edge | u32::from(bits),
                "frame {frame} held {held:#x} dt {dt}"
            );
            fired += u32::from(bits != 0);
        }
        assert!(fired > 500, "{fired}");
    }
}
