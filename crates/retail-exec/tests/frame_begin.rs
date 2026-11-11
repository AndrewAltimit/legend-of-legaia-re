//! The frame-begin pass `FUN_8001698C`, run whole with its calls stubbed,
//! against the three pieces of it the port carries.
//!
//! - **The SFX ring's aging** (`0x80016AF4..0x80016B54`) against
//!   `SfxCueRing::age`: the four ids at `0x8007B6D8` and their timers at
//!   `0x8007C338`, over every frame step.
//! - **The audio-level apply** (`0x80016A04..0x80016A38`): the cell
//!   `_DAT_8007B910` against the last applied level `DAT_8007B6EC`, the
//!   store and the halved level handed to `FUN_8002614C`, against what
//!   `duck::duck_apply` hands a host at retail's base volume.
//! - **The frame skip**: the routine's return, which the port carries as
//!   `FrameClock::frame_begin_skip` - up while the debug pause word
//!   `gp+0x3D8` is set and Start is in neither held word, with the pad pump
//!   run in its place and the ring left alone.

use crate::common::{Rng, scus_cpu};
use legaia_engine_audio::duck::{DUCK_LEVEL_REF, duck_apply};
use legaia_engine_audio::sfx_ring::SfxCueRing;
use psx_retail_exec::{A0, Stop, V0};
use std::cell::RefCell;
use std::rc::Rc;

const FRAME_BEGIN: u32 = 0x8001_698C;
const FRAME_BEGIN_END: u32 = 0x8001_6B6C;
const SET_LEVEL: u32 = 0x8002_614C;
const PAD_PUMP: u32 = 0x8001_822C;
const RING_IDS: u32 = 0x8007_B6D8;
const RING_TIMERS: u32 = 0x8007_C338;
const LEVEL_CELL: u32 = 0x8007_B910;
const LEVEL_APPLIED: u32 = 0x8007_B6EC;
const MUTE: u32 = 0x8007_B438;
const PAUSE: u32 = 0x8007_B6F0;
const HELD_32: u32 = 0x8007_B938;
const HELD_ALT: u32 = 0x8007_B874;
const FRAME_STEP: u32 = 0x1F80_0393;
/// The retail master volume `duck_apply` rescales from.
const RETAIL_MASTER: u8 = 107;

#[test]
fn the_frame_begin_pass_is_retails() {
    differential!(scus(FRAME_BEGIN));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.stub_outside = Some((FRAME_BEGIN, FRAME_BEGIN_END));
    let levels = Rc::new(RefCell::new(Vec::new()));
    let l = levels.clone();
    cpu.hook(SET_LEVEL, move |r, _| l.borrow_mut().push(r.r[A0]));
    let pumped = Rc::new(RefCell::new(0u32));
    let p = pumped.clone();
    cpu.hook(PAD_PUMP, move |_, _| *p.borrow_mut() += 1);

    let mut rng = Rng(0x4652_4D42);
    let (mut skipped, mut applied, mut cleared, mut floored) = (0, 0, 0, 0);
    for case in 0..20_000 {
        let step = 1 + rng.below(4) as u8;
        let ids: [i16; 4] = std::array::from_fn(|_| rng.below(0x300) as i16 - 1);
        let timers: [i32; 4] = std::array::from_fn(|_| match rng.below(3) {
            0 => 0,
            1 => 1 + rng.below(4) as i32,
            _ => rng.below(200) as i32,
        });
        let cell = if rng.below(3) == 0 {
            DUCK_LEVEL_REF
        } else {
            rng.below(0x100) as u8
        };
        let last = if rng.below(2) == 0 {
            cell
        } else {
            rng.below(0x100) as u8
        };
        let pause = rng.below(8) == 0;
        let start = rng.below(2) == 0;
        for i in 0..4u32 {
            cpu.mem.wr16(RING_IDS + 2 * i, ids[i as usize] as u16);
            cpu.mem.wr32(RING_TIMERS + 4 * i, timers[i as usize] as u32);
        }
        cpu.mem.wr8(FRAME_STEP, step);
        cpu.mem.wr32(LEVEL_CELL, u32::from(cell));
        cpu.mem.wr32(LEVEL_APPLIED, u32::from(last));
        cpu.mem.wr32(MUTE, 0);
        cpu.mem.wr16(PAUSE, u16::from(pause));
        cpu.mem.wr32(HELD_32, if start { 0x800 } else { 0 });
        cpu.mem.wr32(HELD_ALT, 0);
        levels.borrow_mut().clear();
        *pumped.borrow_mut() = 0;
        assert_eq!(cpu.call(FRAME_BEGIN, &[]), Stop::Returned, "case {case}");
        let skip = cpu.regs.r[V0] != 0;
        assert_eq!(skip, pause && !start, "case {case}: the frame skip");
        assert_eq!(
            *pumped.borrow(),
            u32::from(skip),
            "the skipped frame still polls the pad"
        );

        // The level: applied on a change, halved.
        let changed = cell != last;
        assert_eq!(cpu.mem.rd32(LEVEL_APPLIED), u32::from(cell));
        let want: Vec<u32> = changed
            .then_some(u32::from(cell >> 1))
            .into_iter()
            .collect();
        assert_eq!(
            *levels.borrow(),
            want,
            "case {case}: cell {cell} last {last}"
        );
        if changed {
            assert_eq!(
                duck_apply(true, RETAIL_MASTER, cell),
                Some((cell >> 1).min(127)),
                "case {case}"
            );
            applied += 1;
        }

        let mut ring = SfxCueRing::default();
        for i in 0..4 {
            ring.arm(i, ids[i], timers[i]);
        }
        if !skip {
            ring.age(step);
        } else {
            skipped += 1;
        }
        for (i, s) in ring.slots().iter().enumerate() {
            let at = i as u32;
            assert_eq!(
                (
                    cpu.mem.rd16(RING_IDS + 2 * at) as i16,
                    cpu.mem.rd32(RING_TIMERS + 4 * at) as i32
                ),
                (s.id, s.timer),
                "case {case} slot {i}: id {} timer {} step {step} skip {skip}",
                ids[i],
                timers[i]
            );
            cleared += usize::from(!skip && timers[i] == 0);
            floored += usize::from(!skip && timers[i] != 0 && timers[i] < i32::from(step));
        }
    }
    assert!(
        skipped > 500 && applied > 5000 && cleared > 10_000 && floored > 3000,
        "{skipped} {applied} {cleared} {floored}"
    );
}
