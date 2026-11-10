//! The party attach swap `FUN_8004CCD4` against `resolve_attach_swap` and
//! `AttachSwap::apply`: every party seat, both pairs pinned or not, windows
//! that open, close, sit at the clip cursor's edges or are disabled by a
//! zero end, cursors from negative to past every window, and the
//! extra-channel escape when the playing stream's part count is not the
//! idle stream's. The whole per-channel model table is compared.
//!
//! The escape is seeded with a pinned pair only: with none pinned, retail
//! indexes the pair `-1` (the neighbouring seat's snapshot), where the port
//! declines to write, as its docs say.

use crate::common::{Rng, scus_cpu};
use legaia_engine_battle::battle_attach_swap::{
    AttachPair, AttachPairs, resolve_attach_swap, windows_from_entry,
};
use psx_retail_exec::Stop;

const SWAP: u32 = 0x8004_CCD4;
const CTX_PTR: u32 = 0x8007_BD24;
const CTX: u32 = 0x8012_0000;
const IDLE_PTRS: u32 = 0x801C_9360;
const IDLE: u32 = 0x8013_0000;
const IDLE_STREAM: u32 = 0x8013_1000;
const ENTRY: u32 = 0x8014_0000;
const STREAM: u32 = 0x8014_1000;
const TABLE: u32 = 0x8015_0000;
const STACK_ARG: u32 = 0x801F_FF00 + 0x10;

#[test]
fn the_attach_swap_installs_what_retail_installs() {
    differential!(scus(SWAP));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem.wr32(CTX_PTR, CTX);
    let mut rng = Rng(0x4154_5443);
    let (mut escapes, mut variants, mut pinned) = (0, 0, 0);
    for n in 0..40_000 {
        let slot = rng.below(3);
        let pairs: [AttachPair; 2] = std::array::from_fn(|_| AttachPair {
            channel: rng.below(0x20) as u8,
            default_object: rng.next(),
            variant_object: rng.next(),
        });
        let escape = rng.below(5) == 0;
        let pin = if escape {
            1 + rng.below(2) as u8
        } else {
            [0u8, 0, 1, 2, 3][rng.below(5) as usize]
        };
        for (k, p) in pairs.iter().enumerate() {
            let k = k as u32;
            cpu.mem.wr8(CTX + 0x23A + slot * 2 + k, p.channel);
            cpu.mem
                .wr32(CTX + 0x1030 + slot * 0x10 + k * 8, p.default_object);
            cpu.mem
                .wr32(CTX + 0x1034 + slot * 0x10 + k * 8, p.variant_object);
        }
        cpu.mem.wr8(CTX + 0x240 + slot, pin);
        let idle_parts = 1 + rng.below(0x14) as u8;
        let playing_parts = if escape {
            idle_parts.wrapping_add(1 + rng.below(3) as u8)
        } else {
            idle_parts
        };
        cpu.mem.wr32(IDLE_PTRS + slot * 4, IDLE);
        cpu.mem.wr32(IDLE, IDLE_STREAM);
        cpu.mem.wr8(IDLE_STREAM + 0xAC, idle_parts);
        cpu.mem.wr32(ENTRY + 0x88, STREAM);
        cpu.mem.wr8(STREAM, playing_parts);
        let windows: [u8; 8] = std::array::from_fn(|_| match rng.below(4) {
            0 => 0,
            _ => rng.below(0x30) as u8,
        });
        cpu.mem.load(ENTRY + 0xA4, &windows);
        let frame = match rng.below(4) {
            0 => -(rng.below(0x10) as i16),
            _ => rng.below(0x34) as i16,
        };
        let table: Vec<u32> = (0..0x20).map(|_| rng.next()).collect();
        for (i, w) in table.iter().enumerate() {
            cpu.mem.wr32(TABLE + i as u32 * 4, *w);
        }
        cpu.mem.wr32(STACK_ARG, TABLE);
        // The cursor arrives as the caller's word; the routine takes its low
        // halfword.
        let frame_reg = (frame as u32 & 0xFFFF) | (rng.next() & 0xFFFF_0000);
        assert_eq!(
            cpu.call(SWAP, &[slot, 0, frame_reg, ENTRY]),
            Stop::Returned,
            "n {n}"
        );
        let retail: Vec<u32> = (0..0x20).map(|i| cpu.mem.rd32(TABLE + i * 4)).collect();

        let snapshot = AttachPairs {
            pairs,
            pinned_ordinal: pin,
        };
        let swap = resolve_attach_swap(
            &snapshot,
            &windows_from_entry(&windows),
            frame,
            playing_parts,
            idle_parts,
        );
        let mut port = table.clone();
        swap.apply(&mut port);
        assert_eq!(
            retail, port,
            "n {n} slot {slot} pin {pin} frame {frame} windows {windows:?} parts {playing_parts}/{idle_parts}"
        );
        if swap.escaped {
            escapes += 1;
        }
        if swap.writes.iter().any(|w| {
            matches!(
                w.choice,
                legaia_engine_battle::battle_attach_swap::AttachChoice::Variant { .. }
            )
        }) {
            variants += 1;
        }
        if pin == 1 || pin == 2 {
            pinned += 1;
        }
    }
    assert!(
        escapes > 5000 && variants > 5000 && pinned > 10_000,
        "{escapes} {variants} {pinned}"
    );
}
