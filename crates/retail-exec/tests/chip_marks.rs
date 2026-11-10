//! The command ring's four chip marks against their quads: the cross-out
//! `FUN_801DBC30` (`cross_out_mark`), the Rot stamp on the Attack chip
//! `FUN_801DBD04` (`rot_stamp_on_chip`), the Rot stamp on an arts chip
//! `FUN_801DBDDC` (`rot_stamp_on_arts_chip`, every cost byte) and the Curse
//! plate `FUN_801DBEC4` (`curse_plate_on_chip`), at anchors across the
//! `i16` plane, with the `ctx[+0x6CE]` gate (`strip_draws`) up and down.
//! The `POLY_FT4` handed to `AddPrim` is compared whole, and its absence
//! when the gate is up.

use crate::common::{Rng, diff_offsets, disc, scus_cpu};
use legaia_engine_battle_vm::battle_party_panel::{
    STRIP_PRIM_BYTES, StripQuad, cross_out_mark, curse_plate_on_chip, rot_stamp_on_arts_chip,
    rot_stamp_on_chip, strip_draws,
};
use psx_retail_exec::{A1, Stop};
use std::cell::RefCell;
use std::rc::Rc;

const CROSS: u32 = 0x801D_BC30;
const ROT: u32 = 0x801D_BD04;
const ROT_ARTS: u32 = 0x801D_BDDC;
const CURSE: u32 = 0x801D_BEC4;
const ADD_PRIM: u32 = 0x8003_D2C4;
const OVERLAY_BASE: u32 = 0x801C_E818;
const CTX_PTR: u32 = 0x8007_BD24;
const CTX: u32 = 0x8012_0000;
const PRIM_CURSOR: u32 = 0x1F80_0314 + 0x8C;
const OT_PTR: u32 = 0x1F80_0314 + 0xE0;
const PRIMS: u32 = 0x8011_0000;

fn bytes(q: &StripQuad) -> Vec<u8> {
    let mut p = vec![0u8; STRIP_PRIM_BYTES];
    p[0..4].copy_from_slice(&q.tag.to_le_bytes());
    p[4..8].copy_from_slice(&q.code_colour.to_le_bytes());
    for i in 0..4 {
        let o = 8 + i * 8;
        p[o..o + 2].copy_from_slice(&q.xy[i].0.to_le_bytes());
        p[o + 2..o + 4].copy_from_slice(&q.xy[i].1.to_le_bytes());
        p[o + 4] = q.uv[i].0;
        p[o + 5] = q.uv[i].1;
    }
    p[0xE..0x10].copy_from_slice(&q.clut.to_le_bytes());
    p[0x16..0x18].copy_from_slice(&q.tpage.to_le_bytes());
    p
}

#[test]
fn the_chip_marks_are_the_quads_retail_draws() {
    differential!(
        prot(898, CROSS),
        prot(898, ROT),
        prot(898, ROT_ARTS),
        prot(898, CURSE)
    );
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
    let mut rng = Rng(0x4D41_524B);
    let (mut drawn, mut gated) = (0, 0);
    for n in 0..40_000 {
        let gate = if rng.below(5) == 0 {
            rng.next() as i16
        } else {
            0
        };
        cpu.mem.wr16(CTX + 0x6CE, gate as u16);
        cpu.mem.wr32(PRIM_CURSOR, PRIMS);
        cpu.mem.load(PRIMS, &[0; 0x40]);
        added.borrow_mut().clear();
        let (x, y) = match rng.below(3) {
            0 => (rng.next() as i16, rng.next() as i16),
            _ => (rng.below(0x140) as i16, rng.below(0xF0) as i16),
        };
        let w = rng.next() as u8;
        let which = n % 4;
        let (entry, port) = match which {
            0 => (CROSS, cross_out_mark(x, y)),
            1 => (ROT, rot_stamp_on_chip(x, y)),
            2 => (ROT_ARTS, rot_stamp_on_arts_chip(x, y, w)),
            _ => (CURSE, curse_plate_on_chip(x, y)),
        };
        // The anchors arrive as whole registers; the routines keep the low
        // halfword, and the arts stamp's cost its low byte.
        let reg = |v: i16, rng: &mut Rng| (v as u16 as u32) | (rng.next() & 0xFFFF_0000);
        let args = [reg(x, &mut rng), reg(y, &mut rng), u32::from(w)];
        assert_eq!(cpu.call(entry, &args), Stop::Returned, "n {n}");
        let added = added.borrow();
        if !strip_draws(gate) {
            assert!(added.is_empty(), "n {n} {entry:#x} gate {gate}");
            assert_eq!(cpu.mem.rd32(PRIM_CURSOR), PRIMS);
            gated += 1;
            continue;
        }
        assert_eq!(added.as_slice(), &[PRIMS], "n {n} {entry:#x}");
        let retail = cpu.mem.read(PRIMS, STRIP_PRIM_BYTES);
        let want = bytes(&port);
        assert!(
            retail == want,
            "n {n} {entry:#x} ({x}, {y}) w {w}: {:?}",
            diff_offsets(&retail, &want)
        );
        drawn += 1;
    }
    assert!(drawn > 20_000 && gated > 5000, "{drawn} {gated}");
}
