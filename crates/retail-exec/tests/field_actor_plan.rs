//! `FUN_8003BC08`, the field actor pass's per-actor driver, against
//! `field_actor_plan`: which height law lands in `+0x16` (hold, mirror, snap,
//! glide) and which of the pre-update, spline, pursue, scripted and
//! move-table routines run. Every routine it calls is hooked, so retail's
//! side reports the plan it carried out; the floor sampler returns a
//! generated height, which the port's glide steps toward with
//! `rotate_toward_clamped`.

use crate::common::{Rng, scus_cpu};
use legaia_engine_motion_vm::motion_vm::{
    FieldActorHeight, FieldActorInputs, field_actor_plan, rotate_toward_clamped,
};
use psx_retail_exec::{A0, Stop, V0};
use std::cell::RefCell;
use std::rc::Rc;

const PLAN: u32 = 0x8003_BC08;
const PRE_UPDATE: u32 = 0x801D_79E8;
const FLOOR: u32 = 0x8001_9278;
const SPLINE: u32 = 0x8003_9B7C;
const PURSUE: u32 = 0x8003_774C;
const SCRIPTED: u32 = 0x8003_8158;
const MOVE_TABLE: u32 = 0x8002_04F8;
const COUNTER: u32 = 0x801C_6ED0;
const AMBIENT: u32 = 0x8007_B6A8;
const FRAME_STEP: u32 = 0x1F80_0393;
const SUPPRESS: u32 = 0x1F80_0394;
const SCENE_PTR: u32 = 0x801C_6EA4;
const SCENE: u32 = 0x8014_0000;
const ACTOR: u32 = 0x8013_0000;

#[test]
fn the_field_actor_plan_matches_retail() {
    differential!(scus(PLAN));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let ran: Rc<RefCell<Vec<u32>>> = Rc::default();
    let floor = Rc::new(std::cell::Cell::new(0i16));
    for va in [PRE_UPDATE, SPLINE, PURSUE, SCRIPTED, MOVE_TABLE] {
        let ran = ran.clone();
        cpu.hook(va, move |r, _| {
            assert_eq!(r.r[A0], ACTOR);
            ran.borrow_mut().push(va);
        });
    }
    {
        let ran = ran.clone();
        let floor = floor.clone();
        cpu.hook(FLOOR, move |r, _| {
            assert_eq!(r.r[A0], ACTOR);
            ran.borrow_mut().push(FLOOR);
            r.r[V0] = floor.get() as i32 as u32;
        });
    }
    cpu.mem.wr32(SCENE_PTR, SCENE);
    let mut rng = Rng(0x504C_414E);
    let mut seen = [0u32; 5];
    let mut dispatched = [0u32; 4];
    for run in 0..20_000 {
        let lifetime = [0i16, 1, -1, 5, rng.next() as i16][rng.below(5) as usize];
        // Bias the flag bits the routine reads.
        let mut flags = rng.next();
        for bit in [
            2u32,
            0x2000_0000,
            0x2_0000,
            0x200,
            0x2000,
            0x100,
            0x400,
            8,
            0x1000,
        ] {
            if rng.below(3) == 0 {
                flags &= !bit;
            }
        }
        let inp = FieldActorInputs {
            lifetime,
            flags,
            field_8e: rng.next() as i16,
            path_target_present: rng.below(2) == 0,
            scripted_present: rng.below(2) == 0,
            ambient_gate: rng.below(2) == 0,
            frame_step: [0u8, 1, 2, 3, 0xFF][rng.below(5) as usize],
            scene_guard_clear: rng.below(3) != 0,
            global_suppress: rng.below(6) == 0,
        };
        let y = rng.next() as i16 >> rng.below(10);
        floor.set(if rng.below(4) == 0 {
            y
        } else {
            y.wrapping_add((rng.next() as i16) >> rng.below(12))
        });
        cpu.mem.wr16(ACTOR + 0x5C, lifetime as u16);
        cpu.mem.wr32(ACTOR + 0x10, flags);
        cpu.mem.wr16(ACTOR + 0x8E, inp.field_8e as u16);
        cpu.mem.wr16(ACTOR + 0x16, y as u16);
        let nonzero = |b: bool, rng: &mut Rng| if b { rng.next() | 1 } else { 0 };
        let path = nonzero(inp.path_target_present, &mut rng);
        cpu.mem.wr32(ACTOR + 0x90, path);
        let script = nonzero(inp.scripted_present, &mut rng);
        cpu.mem.wr32(ACTOR + 0x80, script);
        cpu.mem.wr8(
            AMBIENT,
            if inp.ambient_gate {
                1 + rng.below(255) as u8
            } else {
                0
            },
        );
        cpu.mem.wr8(FRAME_STEP, inp.frame_step);
        cpu.mem.wr16(
            SCENE + 8,
            if inp.scene_guard_clear {
                0
            } else {
                1 + rng.below(0xFFFF) as u16
            },
        );
        let suppress = rng.next() & !0x400 | if inp.global_suppress { 0x400 } else { 0 };
        cpu.mem.wr32(SUPPRESS, suppress);
        let counter = rng.next();
        cpu.mem.wr32(COUNTER, counter);
        ran.borrow_mut().clear();
        assert_eq!(cpu.call(PLAN, &[ACTOR]), Stop::Returned, "run {run}");

        let plan = field_actor_plan(inp);
        let mut port_calls = Vec::new();
        if plan.pre_update {
            port_calls.push(PRE_UPDATE);
        }
        let port_y = match plan.height {
            FieldActorHeight::Hold => y,
            FieldActorHeight::Mirror(v) => v,
            FieldActorHeight::SnapToFloor => {
                port_calls.push(FLOOR);
                floor.get()
            }
            FieldActorHeight::GlideToFloor { rate } => {
                port_calls.push(FLOOR);
                rotate_toward_clamped(y, floor.get(), rate)
            }
        };
        seen[match plan.height {
            FieldActorHeight::Hold if plan.pre_update => 0,
            FieldActorHeight::Hold => 1,
            FieldActorHeight::Mirror(_) => 2,
            FieldActorHeight::SnapToFloor => 3,
            FieldActorHeight::GlideToFloor { .. } => 4,
        }] += 1;
        let d = plan.dispatch;
        for (i, (on, va)) in [
            (d.run_spline, SPLINE),
            (d.run_pursue, PURSUE),
            (d.run_scripted, SCRIPTED),
            (d.run_move_table, MOVE_TABLE),
        ]
        .into_iter()
        .enumerate()
        {
            if on {
                port_calls.push(va);
                dispatched[i] += 1;
            }
        }
        let label = format!("run {run}: {inp:?} y {y} floor {}", floor.get());
        assert_eq!(*ran.borrow(), port_calls, "{label}");
        assert_eq!(cpu.mem.rd16(ACTOR + 0x16) as i16, port_y, "{label}");
        assert_eq!(cpu.mem.rd32(COUNTER), counter.wrapping_add(1), "{label}");
        assert_eq!(cpu.mem.rd32(ACTOR + 0x10), flags, "{label}");
    }
    assert!(
        seen.iter().all(|&n| n > 500) && dispatched.iter().all(|&n| n > 2000),
        "{seen:?} {dispatched:?}"
    );
}
