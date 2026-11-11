//! The Baka Fighter duel's ground disc `FUN_801D6BB8` (PROT 0976): the floor
//! glow (`kind != 0`) and the fighters' shadows (`kind == 0`), against
//! `legaia_engine_effects::effect_default_arm::{ground_shadow_src,
//! ground_disc_mesh}`.
//!
//! The routine stages a source record and hands it to the mesh builder
//! `FUN_80028158` (differentially covered on its own in `default_arm_mesh`)
//! and the drawer `FUN_80043390`. Both are hooked: the record, the builder's
//! arguments and the drawer's flag word are what this routine decides.

use crate::common::{disc, scus_cpu};
use legaia_engine_effects::effect_default_arm::{
    SHADOW_FLAG_WORD, SHADOW_SCRATCH_OFFSET, SHADOW_SEGMENTS, ground_shadow_src,
};
use psx_retail_exec::{A0, A1, A2, A3, Stop};
use std::cell::RefCell;
use std::rc::Rc;

const OVERLAY_BASE: u32 = 0x801C_E818;
const DISC: u32 = 0x801D_6BB8;
const BUILD: u32 = 0x8002_8158;
const DRAW: u32 = 0x8004_3390;
const SCRATCH_PTR: u32 = 0x8007_B85C;
const SCRATCH: u32 = 0x8010_0000;

#[test]
fn the_duel_disc_matches_retail() {
    differential!(prot(976, DISC));
    let Some(disc) = disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut cpu = scus_cpu().expect("disc present");
    cpu.mem
        .load(OVERLAY_BASE, &disc.read_entry(976).expect("PROT 0976"));
    // The matrix stack calls around the draw stand in as no-ops.
    for va in [
        0x8005_B268,
        0x8003_D20C,
        0x8003_D344,
        0x8003_D1A4,
        0x8005_B308,
    ] {
        cpu.hook(va, |_, _| {});
    }
    let builds = Rc::new(RefCell::new(Vec::<(u32, u32, u32, [u8; 0x24])>::new()));
    let draws = Rc::new(RefCell::new(Vec::<(u32, u32, u32)>::new()));
    let b = builds.clone();
    cpu.hook(BUILD, move |r, m| {
        let src: [u8; 0x24] = std::array::from_fn(|i| m.rd8(r.r[A3] + i as u32));
        b.borrow_mut().push((r.r[A0], r.r[A1], r.r[A2], src));
    });
    let d = draws.clone();
    cpu.hook(DRAW, move |r, _| {
        d.borrow_mut().push((r.r[A0], r.r[A1], r.r[A2]))
    });
    cpu.mem.wr32(SCRATCH_PTR, SCRATCH);

    let mut seen = Vec::new();
    for kind in [0u32, 1, 2, 0xFFFF_FFFF] {
        builds.borrow_mut().clear();
        draws.borrow_mut().clear();
        assert_eq!(
            cpu.call(DISC, &[0x8012_0000, kind]),
            Stop::Returned,
            "kind {kind}"
        );
        let b = builds.borrow();
        assert_eq!(b.len(), 1, "one build: kind {kind}");
        let (out, mode, columns, src) = b[0];
        assert_eq!(out, SCRATCH + SHADOW_SCRATCH_OFFSET, "kind {kind}");
        assert_eq!((mode, columns), (1, SHADOW_SEGMENTS), "kind {kind}");
        // Read the three parameters out of retail's record and rebuild the
        // whole record the port's way: every other byte must agree.
        let inner = u32::from_le_bytes(src[4..8].try_into().unwrap());
        let outer = u32::from_le_bytes(src[8..12].try_into().unwrap());
        let radius = i16::from_le_bytes([src[0x1A], src[0x1B]]);
        let mut want = ground_shadow_src(inner, outer, radius);
        // Retail leaves `+0x00..+0x04` and `+0x0C..+0x18` as stack garbage
        // the builder does not read for shape 0; the port zeroes them.
        for r in [0..4usize, 0xC..0x18] {
            want[r.clone()].copy_from_slice(&src[r]);
        }
        assert_eq!(src, want, "the source record: kind {kind}");
        let d = draws.borrow();
        assert_eq!(d.len(), 1, "one draw: kind {kind}");
        assert_eq!(
            d[0].0,
            out + 0xC,
            "the drawer reads the built object: kind {kind}"
        );
        assert_eq!(d[0].2, 1, "kind {kind}");
        if kind == 0 {
            assert_eq!(d[0].1, SHADOW_FLAG_WORD, "the shadow's flag word");
        }
        seen.push((inner, outer, radius, d[0].1));
    }
    // The two kinds the duel draws: the shadow, and the additive glow.
    assert_eq!(seen[0], (0x0080_8080, 0x0007_0707, 0x100, SHADOW_FLAG_WORD));
    for s in &seen[1..] {
        assert_eq!(*s, (0x0080_A0C0, 0x0007_0707, 0x400, 0x9900_0000));
    }
}
