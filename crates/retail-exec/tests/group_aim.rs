//! Two battle-overlay kernels the cast camera and the HUD lean on:
//!
//! - `FUN_801DCEAC`, the group aim, against `target_group_aim` over every
//!   group code (party `8`, monsters `9`, everyone `0xA`, a single seat), with
//!   the gate retail applies per seat (a party seat's roster byte, a monster's
//!   colour word `+0x04`) and positions from a huddle to the whole `i16`
//!   plane. A group with nobody in it stops on the routine's `break`, where
//!   the port answers `None`.
//! - `FUN_801F44A0`, the popup ring push, against `DamagePopupRing::push`:
//!   the three arrays, the cursor and the push count over long runs of
//!   pushes with every value and parameter.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_battle_vm::battle_gauge_rearm::{DamagePopupRing, POPUP_RING_SLOTS};
use legaia_engine_battle_vm::battle_target_group::{GroupAim, GroupSlot, target_group_aim};
use psx_retail_exec::Stop;

const AIM: u32 = 0x801D_CEAC;
const PUSH: u32 = 0x801F_44A0;
const OVERLAY_BASE: u32 = 0x801C_E818;
const CTX_PTR: u32 = 0x8007_BD24;
const CTX: u32 = 0x8012_0000;
const ACTORS: u32 = 0x801C_9370;
const SEATS: u32 = 0x8007_BD10;
const ACTOR: u32 = 0x8013_0000;
const OUT_CENTRE: u32 = 0x8011_0000;
const OUT_EXTENT: u32 = 0x8011_0100;

#[test]
fn the_group_aim_is_retails() {
    differential!(prot(898, AIM));
    let (Some(mut cpu), Some(disc)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem
        .load(OVERLAY_BASE, &disc.read_entry(898).expect("PROT 0898"));
    for s in 0..8 {
        cpu.mem.wr32(ACTORS + s * 4, ACTOR + s * 0x400);
    }
    let mut rng = Rng(0x4752_5550);
    let (mut aimed, mut empty, mut floored, mut wide) = (0, 0, 0, 0);
    for n in 0..60_000 {
        let code = match rng.below(5) {
            0 => 8,
            1 => 9,
            2 => 0xA,
            _ => rng.below(7) as u8,
        };
        let span: i32 = [0x100, 0x800, 0x3000, 0x8000][n % 4];
        let mut slots = Vec::with_capacity(7);
        for s in 0..7u32 {
            let x = (rng.below(2 * span as u32) as i32 - span) as i16;
            let z = (rng.below(2 * span as u32) as i32 - span) as i16;
            let a = ACTOR + s * 0x400;
            cpu.mem.wr16(a + 0x34, x as u16);
            cpu.mem.wr16(a + 0x38, z as u16);
            let up = rng.below(5) != 0;
            let live = if s < 3 {
                cpu.mem
                    .wr8(SEATS + s, if up { 1 + rng.below(4) as u8 } else { 0 });
                // A party seat's colour word is not read.
                cpu.mem.wr32(a + 0x04, rng.below(2));
                up
            } else {
                cpu.mem.wr32(a + 0x04, if up { 0x8010_0000 + s } else { 0 });
                up
            };
            slots.push(GroupSlot { live, x, z });
        }
        cpu.mem.wr16(OUT_CENTRE, 0x5A5A);
        cpu.mem.wr16(OUT_CENTRE + 4, 0x5A5A);
        cpu.mem.wr16(OUT_EXTENT + 4, 0x5A5A);
        let stop = cpu.call(AIM, &[u32::from(code), OUT_CENTRE, OUT_EXTENT]);
        let port = target_group_aim(code, &slots);
        match stop {
            Stop::Returned => {
                let retail = GroupAim {
                    centroid_x: cpu.mem.rd16(OUT_CENTRE) as i16,
                    centroid_z: cpu.mem.rd16(OUT_CENTRE + 4) as i16,
                    extent: cpu.mem.rd16(OUT_EXTENT + 4) as i16,
                };
                assert_eq!(Some(retail), port, "code {code:#x} {slots:?}");
                aimed += 1;
                if retail.extent == 0x400 {
                    floored += 1;
                }
                if span == 0x8000 {
                    wide += 1;
                }
            }
            Stop::DivideByZero { .. } => {
                assert_eq!(port, None, "code {code:#x} {slots:?}");
                empty += 1;
            }
            other => panic!("code {code:#x}: {other:?}"),
        }
    }
    assert!(
        aimed > 20_000 && empty > 2000 && floored > 2000 && wide > 2000,
        "{aimed} {empty} {floored} {wide}"
    );
}

#[test]
fn the_popup_ring_pushes_what_retail_pushes() {
    differential!(prot(898, PUSH));
    let (Some(mut cpu), Some(disc)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem
        .load(OVERLAY_BASE, &disc.read_entry(898).expect("PROT 0898"));
    cpu.mem.wr32(CTX_PTR, CTX);
    let mut rng = Rng(0x504F_5055);
    for run in 0..200 {
        let mut ring = DamagePopupRing::default();
        for i in 0..POPUP_RING_SLOTS {
            ring.value[i] = rng.next() as i32;
            ring.param[i] = rng.next() as u16;
            ring.timer[i] = rng.next() as i32;
            let i = i as u32;
            cpu.mem
                .wr32(CTX + 0x83C + i * 4, ring.value[i as usize] as u32);
            cpu.mem.wr16(CTX + 0x318 + i * 2, ring.param[i as usize]);
            cpu.mem
                .wr32(CTX + 0x85C + i * 4, ring.timer[i as usize] as u32);
        }
        ring.cursor = rng.below(8) as u8;
        ring.pushed = rng.next() as u8;
        cpu.mem.wr8(CTX + 0x262, ring.cursor);
        cpu.mem.wr8(CTX + 0x273, ring.pushed);
        for push in 0..40 {
            // The caller's arguments arrive as whole registers: the routine
            // takes the low halfword and the low byte.
            let value = rng.next();
            let param = rng.next();
            assert_eq!(cpu.call(PUSH, &[value, param]), Stop::Returned);
            ring.push(value as u16 as i16, param as u8);
            for i in 0..POPUP_RING_SLOTS as u32 {
                assert_eq!(
                    (
                        cpu.mem.rd32(CTX + 0x83C + i * 4) as i32,
                        cpu.mem.rd16(CTX + 0x318 + i * 2),
                        cpu.mem.rd32(CTX + 0x85C + i * 4) as i32,
                    ),
                    (
                        ring.value[i as usize],
                        ring.param[i as usize],
                        ring.timer[i as usize]
                    ),
                    "run {run} push {push} slot {i}"
                );
            }
            assert_eq!(
                (cpu.mem.rd8(CTX + 0x262), cpu.mem.rd8(CTX + 0x273)),
                (ring.cursor, ring.pushed),
                "run {run} push {push}"
            );
        }
    }
}
