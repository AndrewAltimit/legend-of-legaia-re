//! The field vertical settle `FUN_801D1BA0` (PROT 0897) off the disc, its
//! head against `settle_head`: the movement-lock / bind-block / `+0x9E`
//! gates, the glide toward the floor sample (rate, slow-fall halving, the
//! mirror class, 16-bit wrap), the `+0x9E` promotion and the hop gates. The
//! floor sample, the hop probe `FUN_801D1878`, the walk-on dispatcher
//! `FUN_801D1EC4` and the clip selector are hooked, so the run records which
//! of them the head reached; the clip tail itself is `field_player_clip`'s.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_field::field_player_clip::{SettleInputs, settle_head};
use psx_retail_exec::{A0, Stop, V0};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

const SETTLE: u32 = 0x801D_1BA0;
const OVERLAY_BASE: u32 = 0x801C_E818;
const FLOOR: u32 = 0x8001_9278;
const HOP: u32 = 0x801D_1878;
const WALK_ON: u32 = 0x801D_1EC4;
const CLIP_BIND: u32 = 0x8002_04F8;
const SCRATCH_FLAGS: u32 = 0x1F80_0394;
const FRAME_STEP: u32 = 0x1F80_0393;
const WARP_TIMER: u32 = 0x8007_B6B0;
const WARP_HOLD: u32 = 0x8007_B6B4;
const STEP_DELTA: u32 = 0x8007_BDE0;
const ACTOR: u32 = 0x8013_0000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Call {
    Floor,
    Hop,
    WalkOn,
}

#[test]
fn the_settle_head_gates_and_glides_as_retail_does() {
    differential!(prot(897, SETTLE));
    let (Some(mut cpu), Some(d)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem
        .load(OVERLAY_BASE, &d.read_entry(897).expect("PROT 0897"));
    let calls: Rc<RefCell<Vec<Call>>> = Rc::default();
    let floor_v = Rc::new(Cell::new(0i32));
    let hop_v = Rc::new(Cell::new(0u32));
    {
        let (calls, floor_v) = (calls.clone(), floor_v.clone());
        cpu.hook(FLOOR, move |r, _| {
            assert_eq!(r.r[A0], ACTOR);
            calls.borrow_mut().push(Call::Floor);
            r.r[V0] = floor_v.get() as u32;
        });
    }
    {
        let (calls, hop_v) = (calls.clone(), hop_v.clone());
        cpu.hook(HOP, move |r, _| {
            assert_eq!(r.r[A0], ACTOR);
            calls.borrow_mut().push(Call::Hop);
            r.r[V0] = hop_v.get();
        });
    }
    {
        let calls = calls.clone();
        cpu.hook(WALK_ON, move |r, _| {
            assert_eq!(r.r[A0], ACTOR);
            calls.borrow_mut().push(Call::WalkOn);
        });
    }
    cpu.stub(CLIP_BIND, 0);
    let mut rng = Rng(0x5345_544C);
    let mut seen = [0u32; 5];
    for run in 0..40_000 {
        let mut flags = rng.next();
        for bit in [0x8_0000, 0x2000, 0x2000_0000] {
            if rng.below(2) == 0 {
                flags &= !bit;
            }
        }
        let word_9e = [0i16, 0x10, 0x10, 0x11, 0x0F, -1, rng.next() as i16][rng.below(7) as usize];
        let y = rng.next() as i16;
        let mirror = rng.next() as i16;
        let scratch = rng.next() & !(if rng.below(2) == 0 { 0x400 } else { 0 });
        let frame_step = [0u8, 1, 2, 3, 4, rng.next() as u8][rng.below(6) as usize];
        let warp_timer = [0i32, 1, -1, rng.next() as i32][rng.below(4) as usize];
        let warp_hold = [0u32, 0, rng.next()][rng.below(3) as usize];
        let delta = (
            [0i16, 0, rng.next() as i16][rng.below(3) as usize],
            [0i16, 0, rng.next() as i16][rng.below(3) as usize],
        );
        let floor = match rng.below(3) {
            0 => i32::from(y) + (rng.below(80) as i32 - 40),
            1 => i32::from(rng.next() as i16),
            _ => rng.next() as i32,
        };
        floor_v.set(floor);
        hop_v.set(rng.below(2));
        cpu.mem.wr32(ACTOR + 0x10, flags);
        cpu.mem.wr16(ACTOR + 0x9E, word_9e as u16);
        cpu.mem.wr16(ACTOR + 0x16, y as u16);
        cpu.mem.wr16(ACTOR + 0x8E, mirror as u16);
        cpu.mem.wr16(ACTOR + 0x5C, 0);
        cpu.mem.wr32(SCRATCH_FLAGS, scratch);
        cpu.mem.wr8(FRAME_STEP, frame_step);
        cpu.mem.wr32(WARP_TIMER, warp_timer as u32);
        cpu.mem.wr32(WARP_HOLD, warp_hold);
        cpu.mem.wr16(STEP_DELTA, delta.0 as u16);
        cpu.mem.wr16(STEP_DELTA + 4, delta.1 as u16);
        calls.borrow_mut().clear();
        assert_eq!(cpu.call(SETTLE, &[ACTOR]), Stop::Returned, "run {run}");

        let inputs = SettleInputs {
            flags,
            word_9e,
            y,
            mirror,
            bind_block: scratch & 0x400 != 0,
            frame_step,
            warp_timer,
            warp_hold,
            step_delta: delta,
        };
        let mut sampled = false;
        let head = settle_head(&inputs, || {
            sampled = true;
            floor
        });
        let mut want = Vec::new();
        if sampled {
            want.push(Call::Floor);
        }
        let hopped = head.hop_probe && hop_v.get() != 0;
        if head.hop_probe {
            want.push(Call::Hop);
        }
        if head.tail && !hopped {
            want.push(Call::WalkOn);
        }
        let label = format!("run {run} {inputs:?} floor {floor} -> {head:?}");
        assert_eq!(*calls.borrow(), want, "{label}");
        assert_eq!(cpu.mem.rd16(ACTOR + 0x16) as i16, head.y, "{label}: y");
        assert_eq!(
            cpu.mem.rd16(ACTOR + 0x9E) as i16,
            head.word_9e,
            "{label}: 9e"
        );
        seen[0] += u32::from(head.hop_probe);
        seen[1] += u32::from(hopped);
        seen[2] += u32::from(!head.tail);
        seen[3] += u32::from(sampled && head.y != y);
        seen[4] += u32::from(word_9e == 0);
    }
    assert!(seen.iter().all(|&n| n > 500), "{seen:?}");
}
