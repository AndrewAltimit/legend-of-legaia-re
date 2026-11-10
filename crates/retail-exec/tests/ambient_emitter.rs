//! The field's ambient fog emitter `FUN_801D6058` (PROT 0897) against
//! `AmbientEmitter::step`, frame by frame on one scripted `rand()` stream:
//! the master gate, the point arm at the actor's own position, and the
//! scene arm's twenty-four draws over the visible-tile span and their bursts,
//! light and dense. The spawner `FUN_801D629C` it calls is logged in place
//! (its own draws are held by `fog_spawn`).

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_field::ambient_emitter::{
    AmbientEmitter, AmbientParticle, AmbientScene, SceneSpan,
};
use psx_retail_exec::{A0, A1, A2, A3, Stop, V0};
use std::cell::RefCell;
use std::rc::Rc;

const EMIT: u32 = 0x801D_6058;
const SPAWN: u32 = 0x801D_629C;
const RAND: u32 = 0x8005_6798;
const OVERLAY_BASE: u32 = 0x801C_E818;
const GATE: u32 = 0x8007_B854;
const FLAGS: u32 = 0x1F80_0394;
const SPAN: u32 = 0x1F80_03E8;
const CAM_X: u32 = 0x8008_9118;
const CAM_Y: u32 = 0x8008_9120;
const ACTOR: u32 = 0x8013_0000;

#[test]
fn the_ambient_emitter_spawns_what_retail_spawns() {
    differential!(prot(897, EMIT));
    let (Some(mut cpu), Some(d)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem
        .load(OVERLAY_BASE, &d.read_entry(897).expect("PROT 0897"));
    cpu.stub(0x8001_A068, 0);
    let stream: Rc<RefCell<Vec<u32>>> = Rc::default();
    let s = stream.clone();
    cpu.hook(RAND, move |r, _| r.r[V0] = s.borrow_mut().remove(0));
    let log: Rc<RefCell<Vec<AmbientParticle>>> = Rc::default();
    let l = log.clone();
    cpu.hook(SPAWN, move |r, _| {
        l.borrow_mut().push(AmbientParticle {
            x: r.r[A0] as i32,
            y: r.r[A1] as i32,
            vx: r.r[A2] as i32,
            vy: r.r[A3] as i32,
        })
    });
    let mut rng = Rng(0x414D_4245);
    let (mut points, mut bursts) = (0, 0);
    for frame in 0..20_000 {
        let e = AmbientEmitter {
            state: [0i16, 0, 1, 5][rng.below(4) as usize],
            actor_x: rng.next() as u16,
            actor_y: rng.next() as u16,
        };
        let scene = AmbientScene {
            enabled: rng.below(10) != 0,
            dense: rng.below(2) == 0,
            span: SceneSpan {
                x_min: -(rng.below(12) as i8),
                y_min: -(rng.below(12) as i8),
                x_max: rng.below(14) as i8 - 2,
                y_max: rng.below(14) as i8 - 2,
            },
            camera_x: rng.next() as i32 >> rng.below(12),
            camera_y: rng.next() as i32 >> rng.below(12),
        };
        cpu.mem.wr16(ACTOR + 0x1A, e.state as u16);
        cpu.mem.wr16(ACTOR + 0x14, e.actor_x);
        cpu.mem.wr16(ACTOR + 0x18, e.actor_y);
        cpu.mem.wr32(GATE, u32::from(scene.enabled));
        cpu.mem.wr32(FLAGS, u32::from(scene.dense));
        cpu.mem.load(
            SPAN,
            &[
                scene.span.x_min as u8,
                scene.span.y_min as u8,
                scene.span.x_max as u8,
                scene.span.y_max as u8,
            ],
        );
        cpu.mem.wr32(CAM_X, scene.camera_x as u32);
        cpu.mem.wr32(CAM_Y, scene.camera_y as u32);
        // Small draws make the one-in-sixteen gates fire often.
        let draws: Vec<u32> = (0..400)
            .map(|_| {
                if rng.below(3) == 0 {
                    rng.next() & 0x7FF0
                } else {
                    rng.next() & 0x7FFF
                }
            })
            .collect();
        *stream.borrow_mut() = draws.clone();
        log.borrow_mut().clear();
        assert_eq!(cpu.call(EMIT, &[ACTOR]), Stop::Returned, "frame {frame}");
        let left = stream.borrow().len();
        let mut it = draws.into_iter();
        let mut used = 0;
        let port = e.step(&scene, || {
            used += 1;
            it.next().expect("draw")
        });
        assert_eq!(*log.borrow(), port, "frame {frame}: {e:?} {scene:?}");
        assert_eq!(400 - left, used, "frame {frame} draws");
        if e.state == 0 {
            points += port.len();
        } else {
            bursts += port.len();
        }
    }
    assert!(points > 1000 && bursts > 10_000, "{points} {bursts}");
}
