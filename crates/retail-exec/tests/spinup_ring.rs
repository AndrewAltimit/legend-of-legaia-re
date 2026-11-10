//! The spin-up style's ring `FUN_801D1CFC` (PROT 0979) against
//! `legaia_render_kernels::battle_intro`'s spin-up ring constants and
//! `emit_spinup_ring`'s phase gate. The routine decides two calls: the
//! mesh builder `FUN_80028158` (differentially covered on its own in
//! `default_arm_mesh`) with its shape, column count and parameter record, and
//! the drawer `FUN_80043390` with its flag word and fade. Both are hooked
//! and their arguments held to what the port's ring is built from, over
//! every phase around the gate. The ring's vertices themselves are the
//! port's model of the builder's case-0 band and are not compared here.

use crate::common::{disc, scus_cpu};
use legaia_render_kernels::battle_intro::{
    SPINUP_RING_DEPTH, SPINUP_RING_PHASE_MAX, SPINUP_RING_RGB, SPINUP_RING_SCALE,
    SPINUP_RING_SEGMENTS, emit_spinup_ring,
};
use psx_retail_exec::{A0, A1, A2, A3, Stop};
use std::cell::RefCell;
use std::rc::Rc;

const OVERLAY_BASE: u32 = 0x801C_E818;
const RING: u32 = 0x801D_1CFC;
const BUILD: u32 = 0x8002_8158;
const DRAW: u32 = 0x8004_3390;
const SCRATCH_PTR: u32 = 0x8007_B85C;
const SCRATCH: u32 = 0x8010_0000;
/// The ring's object sits `0x5DC00` into the scratch buffer.
const RING_OBJECT: u32 = SCRATCH + 0x5_DC00;

#[test]
fn the_spinup_ring_builds_and_draws_what_the_port_models() {
    differential!(prot(979, RING));
    let (Some(mut cpu), Some(d)) = (scus_cpu(), disc()) else {
        return;
    };
    cpu.mem
        .load(OVERLAY_BASE, &d.read_entry(979).expect("PROT 0979"));
    cpu.mem.wr32(SCRATCH_PTR, SCRATCH);
    let builds = Rc::new(RefCell::new(Vec::<(u32, u32, u32, [u8; 0x24])>::new()));
    let draws = Rc::new(RefCell::new(Vec::<(u32, u32, u32)>::new()));
    let b = builds.clone();
    cpu.hook(BUILD, move |r, m| {
        let src: [u8; 0x24] = std::array::from_fn(|i| m.rd8(r.r[A3] + i as u32));
        b.borrow_mut().push((r.r[A0], r.r[A1], r.r[A2], src));
    });
    let dr = draws.clone();
    cpu.hook(DRAW, move |r, _| {
        dr.borrow_mut().push((r.r[A0], r.r[A1], r.r[A2]))
    });
    let h = |src: &[u8; 0x24], o: usize| i16::from_le_bytes([src[o], src[o + 1]]);
    let mut drawn = 0;
    for phase in (-4..=0x1004).chain([0x7FFF_FFFF, -0x8000_0000]) {
        builds.borrow_mut().clear();
        draws.borrow_mut().clear();
        assert_eq!(
            cpu.call(RING, &[phase as u32]),
            Stop::Returned,
            "phase {phase:#x}"
        );
        let mut prims = Vec::new();
        let gated = (1..=SPINUP_RING_PHASE_MAX).contains(&phase);
        if !gated {
            assert!(!emit_spinup_ring(phase, &mut prims), "phase {phase:#x}");
            assert!(
                builds.borrow().is_empty() && draws.borrow().is_empty(),
                "phase {phase:#x}"
            );
            continue;
        }
        drawn += 1;
        let b = builds.borrow();
        assert_eq!(b.len(), 1, "phase {phase:#x}: one build");
        let (dest, shape, segments, src) = b[0];
        assert_eq!(dest, RING_OBJECT);
        assert_eq!(shape, 0, "the builder's case-0 band");
        assert_eq!(segments as usize, SPINUP_RING_SEGMENTS);
        assert_eq!(
            u32::from_le_bytes([src[8], src[9], src[10], src[11]]),
            SPINUP_RING_RGB
        );
        assert_eq!(i32::from(h(&src, 0x18)), phase, "the inner radius");
        assert_eq!(i32::from(h(&src, 0x1A)), phase + 2, "the outer radius");
        assert_eq!(
            (h(&src, 0x1C), h(&src, 0x1E)),
            (0, SPINUP_RING_DEPTH),
            "the rims' z"
        );
        assert_eq!(
            (i32::from(h(&src, 0x20)), i32::from(h(&src, 0x22))),
            (SPINUP_RING_SCALE, SPINUP_RING_SCALE),
            "the scale pair"
        );
        assert_eq!(
            *draws.borrow(),
            vec![(RING_OBJECT + 0xC, 0x8900_0000, phase as u32)],
            "phase {phase:#x}: double-sided, faded by the phase"
        );
    }
    assert_eq!(drawn, SPINUP_RING_PHASE_MAX as usize);
}
