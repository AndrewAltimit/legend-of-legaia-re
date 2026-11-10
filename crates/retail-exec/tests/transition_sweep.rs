//! The field overlay's scene-transition teardown sweep `FUN_801D7518`
//! (PROT 0897) against
//! `legaia_engine_field::scene_transition_sweep::sweep_actor`.
//!
//! The routine walks a linked actor list; the port decides one actor. The
//! test links a generated list, runs the walk once, and holds every actor to
//! the port's decision: the flag word, the side-buffer reallocation with its
//! four seeds, the CLUT-walk accumulator, and the move-VM arm, which shows as
//! the `*(+0x44) = 1` store a render-mode-4 actor takes there. The move-VM
//! arm's two buffer rebuilds (`+0x5A == 3` / `== 6`) are kept off: the port
//! reports that arm and does not model them.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_field::scene_transition_sweep::{
    ACTOR_SIDE_BUFFER_BYTES, MOVE_VM_HANDLER, RETIRED_HANDLERS, SweepActor, sweep_actor,
};
use psx_retail_exec::{A0, A1, Stop, V0};
use std::cell::RefCell;
use std::rc::Rc;

const OVERLAY_BASE: u32 = 0x801C_E818;
const SWEEP: u32 = 0x801D_7518;
const ALLOC: u32 = 0x8001_7888;
const SIDE_INIT: u32 = 0x8002_4D78;

const HEAD: u32 = 0x8012_0000;
const ACTORS: u32 = 0x8012_1000;
const STRIDE: u32 = 0x100;
const OLD_SIDE: u32 = 0x8013_0000;
const NEW_SIDE: u32 = 0x8014_0000;
const SIDE_STRIDE: u32 = 0x100;

#[test]
fn the_transition_sweep_matches_retail() {
    differential!(prot(897, SWEEP));
    let Some(disc) = disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut cpu = scus_cpu().expect("disc present");
    cpu.mem
        .load(OVERLAY_BASE, &disc.read_entry(897).expect("PROT 0897"));
    let allocs = Rc::new(RefCell::new(Vec::<(u32, u32)>::new()));
    let inits = Rc::new(RefCell::new(Vec::<u32>::new()));
    let a = allocs.clone();
    cpu.hook(ALLOC, move |r, _| {
        let n = a.borrow().len() as u32;
        a.borrow_mut().push((r.r[A0], r.r[A1]));
        r.r[V0] = NEW_SIDE + SIDE_STRIDE * n;
    });
    let i = inits.clone();
    cpu.hook(SIDE_INIT, move |r, _| i.borrow_mut().push(r.r[A0]));

    let mut rng = Rng(0x5357_4550);
    let (mut retired, mut realloced, mut seeded, mut move_vm, mut empty) = (0, 0, 0, 0, 0);
    for round in 0..3000 {
        let n = rng.below(9);
        let actors: Vec<SweepActor> = (0..n)
            .map(|_| SweepActor {
                handler: match rng.below(6) {
                    0 => RETIRED_HANDLERS[rng.below(3) as usize],
                    1 | 2 => MOVE_VM_HANDLER,
                    3 => RETIRED_HANDLERS[rng.below(3) as usize] + 4,
                    _ => 0x8002_0000 + 4 * rng.below(0x4000),
                },
                flags: (rng.next() & !0x1_0808)
                    | [0, 0x800][rng.below(2) as usize]
                    | (rng.below(2) * 8),
                render_mode: match rng.below(4) {
                    0 => 4,
                    1 => 0xB | (rng.below(0x10) as u16) << 4,
                    _ => rng.next() as u16,
                },
            })
            .collect();
        cpu.mem.wr32(HEAD, if n == 0 { 0 } else { ACTORS });
        for (k, a) in actors.iter().enumerate() {
            let at = ACTORS + STRIDE * k as u32;
            let next = if k + 1 < actors.len() { at + STRIDE } else { 0 };
            cpu.mem.wr32(at, next);
            cpu.mem.wr32(at + 0xC, a.handler);
            cpu.mem.wr32(at + 0x10, a.flags);
            cpu.mem.wr32(at + 0x44, OLD_SIDE + SIDE_STRIDE * k as u32);
            cpu.mem.wr16(at + 0x56, a.render_mode);
            // Neither rebuild of the move-VM arm.
            cpu.mem
                .wr16(at + 0x5A, [0u16, 1, 2, 4, 5, 7][rng.below(6) as usize]);
            cpu.mem.wr16(at + 0x68, 0xA5A5);
            for base in [OLD_SIDE, NEW_SIDE] {
                let side = base + SIDE_STRIDE * k as u32;
                cpu.mem.wr32(side, 0xDEAD_BEEF);
                for o in (0x90..0xA0).step_by(2) {
                    cpu.mem.wr16(side + o, 0x5A5A);
                }
            }
        }
        allocs.borrow_mut().clear();
        inits.borrow_mut().clear();
        assert_eq!(cpu.call(SWEEP, &[HEAD]), Stop::Returned, "round {round}");

        let (mut want_allocs, mut want_inits) = (Vec::new(), Vec::new());
        for (k, a) in actors.iter().enumerate() {
            let at = ACTORS + STRIDE * k as u32;
            let ctx = format!("round {round} actor {k} {a:x?}");
            let d = sweep_actor(*a);
            assert_eq!(cpu.mem.rd32(at + 0x10), d.flags, "flags: {ctx}");
            let mut side = OLD_SIDE + SIDE_STRIDE * k as u32;
            if d.realloc_side_buffer {
                side = NEW_SIDE + SIDE_STRIDE * want_allocs.len() as u32;
                want_allocs.push((0, ACTOR_SIDE_BUFFER_BYTES as u32));
                want_inits.push(at);
                let tail: Vec<u16> = (0x90..0xA0)
                    .step_by(2)
                    .map(|o| cpu.mem.rd16(side + o))
                    .collect();
                assert_eq!(
                    tail,
                    [0x5A5A, 0x5A5A, 0, 0, 0, 0xFFFF, 0x5A5A, 0x5A5A],
                    "side seeds: {ctx}"
                );
                realloced += 1;
            }
            assert_eq!(cpu.mem.rd32(at + 0x44), side, "side buffer: {ctx}");
            let first = if d.move_vm_arm && a.render_mode == 4 {
                1
            } else {
                0xDEAD_BEEF
            };
            assert_eq!(cpu.mem.rd32(side), first, "move-VM arm store: {ctx}");
            assert_eq!(
                cpu.mem.rd16(at + 0x68),
                d.clut_walk_seed.unwrap_or(0xA5A5),
                "accumulator: {ctx}"
            );
            retired += u32::from(d.retired);
            seeded += u32::from(d.clut_walk_seed.is_some());
            move_vm += u32::from(first == 1);
        }
        assert_eq!(*allocs.borrow(), want_allocs, "round {round}");
        assert_eq!(*inits.borrow(), want_inits, "round {round}");
        empty += u32::from(n == 0);
    }
    assert!(
        retired > 1000 && realloced > 3000 && seeded > 2000 && move_vm > 500 && empty > 100,
        "{retired} {realloced} {seeded} {move_vm} {empty}"
    );
}
