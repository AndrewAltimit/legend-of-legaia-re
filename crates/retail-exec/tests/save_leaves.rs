//! Two save-side leaves: the SC block checksum `FUN_801E38D8` (menu overlay,
//! PROT 0899) against `sc_block_checksum` over generated blocks, and the
//! slot-addressed stack add `FUN_80042FE8` (`SCUS_942.54`) against
//! `RetailInventory::add_to_slot` over every slot, window end and count.

use crate::common::{Rng, disc, scus_cpu};
use legaia_save::retail_inventory::{ITEM_WINDOW_BASE, ItemWindow, RetailInventory};
use legaia_save::{BLOCK_SIZE, sc_block_checksum};
use psx_retail_exec::{Stop, V0};

const OVERLAY_BASE: u32 = 0x801C_E818;
const CHECKSUM: u32 = 0x801E_38D8;
const ADD_TO_SLOT: u32 = 0x8004_2FE8;
const BLOCK: u32 = 0x8010_0000;
/// `gp[+0x2D4]`, the active window's end.
const WINDOW_END: u32 = 0x8007_B318 + 0x2D4;

#[test]
fn the_block_checksum_sums_what_retail_sums() {
    differential!(prot(899, CHECKSUM));
    let (Some(mut cpu), Some(d)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem.load(
        OVERLAY_BASE,
        &d.read_entry_footprint(899).expect("PROT 0899"),
    );
    let mut rng = Rng(0x5343_4B53);
    for case in 0..64 {
        let mut block = vec![0u8; BLOCK_SIZE];
        match case % 4 {
            // A zero-filled staging block with a live-state front, as the
            // composer builds it.
            0 => block[..0x1A18]
                .iter_mut()
                .for_each(|b| *b = rng.next() as u8),
            1 => block.iter_mut().for_each(|b| *b = rng.next() as u8),
            // All ones: the sum wraps on every word.
            2 => block.iter_mut().for_each(|b| *b = 0xFF),
            _ => {}
        }
        cpu.mem.load(BLOCK, &block);
        cpu.regs.r[V0] = 0xDEAD_BEEF;
        assert_eq!(cpu.call(CHECKSUM, &[BLOCK]), Stop::Returned);
        assert_eq!(
            Some(cpu.regs.r[V0]),
            sc_block_checksum(&block),
            "case {case}"
        );
    }
}

#[test]
fn the_slot_add_caps_and_refuses_as_retail_does() {
    differential!(scus(ADD_TO_SLOT));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut rng = Rng(0x534C_4F54);
    let windows = [
        (ItemWindow::Full, 256u16),
        (ItemWindow::Low, 128),
        (ItemWindow::High, 256),
    ];
    let (mut stored, mut refused, mut capped) = (0, 0, 0);
    for case in 0..4000 {
        let slots: Vec<(u8, u8)> = (0..256)
            .map(|_| match rng.below(4) {
                0 => (0, rng.next() as u8),
                1 => (rng.next() as u8, 99),
                _ => (rng.next() as u8, rng.below(100) as u8),
            })
            .collect();
        for (i, &(id, n)) in slots.iter().enumerate() {
            cpu.mem.wr8(ITEM_WINDOW_BASE + 2 * i as u32, id);
            cpu.mem.wr8(ITEM_WINDOW_BASE + 2 * i as u32 + 1, n);
        }
        let (window, end) = windows[rng.below(3) as usize];
        cpu.mem.wr16(WINDOW_END, end);
        let slot = rng.below(256) as usize;
        let delta = rng.next() as u8;
        let mut port = RetailInventory::from_slots(ITEM_WINDOW_BASE, slots.clone());
        port.set_window(window);
        // The rejection paths return the caller's `a2` untouched.
        assert_eq!(
            cpu.call(ADD_TO_SLOT, &[slot as u32, u32::from(delta), 0xAB]),
            Stop::Returned
        );
        let r = cpu.regs.r[V0];
        let p = port.add_to_slot(slot, delta);
        assert_eq!(r == 0xAB, p.is_none(), "case {case} slot {slot} end {end}");
        if let Some(n) = p {
            assert_eq!(r, u32::from(n), "case {case}");
            stored += 1;
            capped += u32::from(n == 99);
        } else {
            refused += 1;
        }
        let after: Vec<(u8, u8)> = (0..256u32)
            .map(|i| {
                (
                    cpu.mem.rd8(ITEM_WINDOW_BASE + 2 * i),
                    cpu.mem.rd8(ITEM_WINDOW_BASE + 2 * i + 1),
                )
            })
            .collect();
        assert_eq!(after, port.slots(), "case {case} slot {slot} end {end}");
    }
    assert!(
        stored > 500 && refused > 500 && capped > 100,
        "{stored} {refused} {capped}"
    );
}
