//! The arts banner's flash ramp `FUN_801E2524` with its quad emitter
//! `FUN_801E2650` live beneath it, against `step_flash_ramp` and
//! `flash_quads`: every stage byte (idle, the four draw positions, the
//! cancel band and past it), every level byte and frame step. Each
//! `POLY_FT4` the emitter hands `AddPrim` is compared whole (bar the link
//! word), in order, with the packet the port's quad describes, and so are
//! the two context bytes the step writes back.

use crate::common::{Rng, diff_offsets, disc, scus_cpu};
use legaia_engine_battle_vm::battle_action::{FlashQuad, flash_quads, step_flash_ramp};
use psx_retail_exec::{A0, A1, Stop};
use std::cell::RefCell;
use std::rc::Rc;

const RAMP: u32 = 0x801E_2524;
const EMIT: u32 = 0x801E_2650;
const ADD_PRIM: u32 = 0x8003_D2C4;
const OVERLAY_BASE: u32 = 0x801C_E818;
const CTX_PTR: u32 = 0x8007_BD24;
const CTX: u32 = 0x8012_0000;
const FRAME_STEP: u32 = 0x1F80_0393;
const PRIM_CURSOR: u32 = 0x1F80_0314 + 0x8C;
const OT_PTR: u32 = 0x1F80_0314 + 0xE0;
const PRIMS: u32 = 0x8011_0000;

/// The `POLY_FT4` bytes `+4..+0x28` a port quad stands for.
fn packet(q: &FlashQuad) -> Vec<u8> {
    let mut p = vec![0u8; 0x24];
    let g = u32::from(q.gray);
    let colour = (u32::from(q.code) << 24) | (g << 16) | (g << 8) | g;
    p[0..4].copy_from_slice(&colour.to_le_bytes());
    let corners = [
        (q.x.0, q.y.0, q.u.0, q.v.0),
        (q.x.1, q.y.0, q.u.1, q.v.0),
        (q.x.0, q.y.1, q.u.0, q.v.1),
        (q.x.1, q.y.1, q.u.1, q.v.1),
    ];
    for (i, (x, y, u, v)) in corners.iter().enumerate() {
        let o = 4 + i * 8;
        p[o..o + 2].copy_from_slice(&x.to_le_bytes());
        p[o + 2..o + 4].copy_from_slice(&y.to_le_bytes());
        p[o + 4] = *u;
        p[o + 5] = *v;
    }
    p[0xA..0xC].copy_from_slice(&q.clut.to_le_bytes());
    p[0x12..0x14].copy_from_slice(&q.tpage.to_le_bytes());
    p
}

#[test]
fn the_flash_ramp_draws_what_retail_draws() {
    differential!(prot(898, RAMP));
    let (Some(mut cpu), Some(disc)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem
        .load(OVERLAY_BASE, &disc.read_entry(898).expect("PROT 0898"));
    cpu.mem.wr32(CTX_PTR, CTX);
    cpu.mem.wr32(OT_PTR, 0x8011_F000);
    let added = Rc::new(RefCell::new(Vec::new()));
    {
        let added = added.clone();
        cpu.hook(ADD_PRIM, move |r, _| {
            added.borrow_mut().push((r.r[A0], r.r[A1]))
        });
    }
    let mut rng = Rng(0x464C_5348);
    let (mut drawn, mut retired, mut quads) = (0, 0, 0);
    for n in 0..30_000 {
        let stage = match rng.below(4) {
            0 => rng.next() as u8,
            _ => rng.below(10) as u8,
        };
        let level = match rng.below(3) {
            0 => 0xC0 + rng.below(0x40) as u8,
            _ => rng.next() as u8,
        };
        let step = [1u8, 1, 2, 3, 4][rng.below(5) as usize];
        cpu.mem.wr8(CTX + 0x28B, stage);
        cpu.mem.wr8(CTX + 0x28C, level);
        cpu.mem.wr8(FRAME_STEP, step);
        cpu.mem.wr32(PRIM_CURSOR, PRIMS);
        // Cleared: the packets' two padding halfwords are never written.
        cpu.mem.load(PRIMS, &[0; 0x200]);
        added.borrow_mut().clear();
        assert_eq!(cpu.call(RAMP, &[]), Stop::Returned, "n {n}");

        let frame = step_flash_ramp(stage, level, step);
        let port: Vec<FlashQuad> = frame
            .layers
            .iter()
            .flat_map(|l| flash_quads(l, level).expect("a drawn position"))
            .collect();
        let added = added.borrow();
        assert_eq!(
            added.len(),
            port.len(),
            "n {n} stage {stage} level {level:#x}"
        );
        for (k, ((_, prim), q)) in added.iter().zip(&port).enumerate() {
            let retail = cpu.mem.read(prim + 4, 0x24);
            let want = packet(q);
            assert!(
                retail == want,
                "n {n} stage {stage} level {level:#x} quad {k}: {:?}",
                diff_offsets(&retail, &want)
            );
        }
        quads += port.len();
        assert_eq!(
            (cpu.mem.rd8(CTX + 0x28B), cpu.mem.rd8(CTX + 0x28C)),
            (
                frame.stage_out.unwrap_or(stage),
                frame.level_out.unwrap_or(level)
            ),
            "n {n} stage {stage} level {level:#x} step {step}"
        );
        if !port.is_empty() {
            drawn += 1;
        }
        if frame.stage_out == Some(0) {
            retired += 1;
        }
    }
    assert!(
        drawn > 5000 && retired > 2000 && quads > 30_000,
        "{drawn} {retired} {quads}"
    );
}

#[test]
fn the_flash_emitter_builds_the_quads_retail_builds() {
    differential!(prot(898, EMIT));
    let (Some(mut cpu), Some(disc)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem
        .load(OVERLAY_BASE, &disc.read_entry(898).expect("PROT 0898"));
    cpu.mem.wr32(CTX_PTR, CTX);
    cpu.mem.wr32(OT_PTR, 0x8011_F000);
    let added = Rc::new(RefCell::new(Vec::new()));
    {
        let added = added.clone();
        cpu.hook(ADD_PRIM, move |r, _| added.borrow_mut().push(r.r[A1]));
    }
    let mut rng = Rng(0x454D_4954);
    for n in 0..30_000 {
        let layer = legaia_engine_battle_vm::battle_action::FlashLayer {
            offset: rng.next() as u8,
            percent: match rng.below(3) {
                0 => rng.next() as u8,
                _ => rng.below(60) as u8,
            },
            semi_transparent: rng.below(2) == 0,
            position: rng.below(4) as u8,
        };
        let level = rng.next() as u8;
        cpu.mem.wr8(CTX + 0x28C, level);
        cpu.mem.wr32(PRIM_CURSOR, PRIMS);
        added.borrow_mut().clear();
        assert_eq!(
            cpu.call(
                EMIT,
                &[
                    u32::from(layer.offset),
                    u32::from(layer.percent),
                    u32::from(layer.semi_transparent),
                    u32::from(layer.position),
                ]
            ),
            Stop::Returned
        );
        let port = flash_quads(&layer, level).expect("a drawn position");
        let added = added.borrow();
        assert_eq!(added.len(), 2);
        for (k, (prim, q)) in added.iter().zip(&port).enumerate() {
            let retail = cpu.mem.read(prim + 4, 0x24);
            let want = packet(q);
            assert!(
                retail == want,
                "n {n} {layer:?} level {level:#x} quad {k}: {:?}",
                diff_offsets(&retail, &want)
            );
        }
    }
}
