//! The animated-mesh renderer `FUN_8001B964` against the keyframe-pose draw
//! of a sprite-arm node, `effect_sprite_arm::sprite_arm_draws`.
//!
//! A mode-`6` node's `+0x4C` is the one-frame clip the move VM packs from its
//! pose block (`keyframe_pose_entries`), and every slot of its model list is
//! the one built quad. The renderer runs whole off the executable over that
//! clip - the actor matrix `FUN_80026988`, the camera compose, the render
//! scale `+0x72`, the per-part decode `FUN_8001BE80` and the axis rotations -
//! with the camera at identity and the node at the origin, so the GTE
//! rotation and translation it leaves for each part's draw (read at the
//! `jal` to the leaf dispatcher `FUN_80043390`) are the part's pose. Each is
//! applied to the quad's corners and compared with the corners the port
//! poses for that part, within the fixed point's rounding.

use crate::common::{Rng, scus_cpu};
use legaia_engine_effects::effect_sprite_arm::{
    DRAW_KIND_MULTI, SPRITE_ARM_BIT, sprite_arm_draw, sprite_arm_draws,
};
use legaia_engine_vm::move_vm::{ActorState, keyframe_pose_entries};
use psx_retail_exec::Stop;
use std::cell::RefCell;
use std::rc::Rc;

const RENDERER: u32 = 0x8001_B964;
/// The `jal 0x80043390` on the plain-model path.
const LEAF_CALL: u32 = 0x8001_BC84;
const ACTOR: u32 = 0x8010_0000;
const LIST: u32 = 0x8011_0000;
const OBJECT: u32 = 0x8011_2000;
const CLIP: u32 = 0x8011_4000;
const CAMERA: u32 = 0x1F80_03C8;

/// The GTE rotation (row-major) and translation at one part's draw.
type Pose = ([i32; 9], [i32; 3]);

fn node(rng: &mut Rng, parts: usize) -> ActorState {
    let mut s = ActorState {
        move_substate: DRAW_KIND_MULTI as i16,
        move_submode: 6,
        field_9e: SPRITE_ARM_BIT,
        field_72: [0x1000, 0x800, 0x1800, 0x2000][rng.below(4) as usize],
        field_74: 0xC100_0000,
        field_78: 0,
        // The blend cursor.
        y_rot: rng.below(0x1001) as i16,
        ..Default::default()
    };
    s.set_actor_u16(0xB4, 8 + 2 * rng.below(40) as u16);
    s.set_actor_u16(0xB6, 8 + 2 * rng.below(40) as u16);
    s.set_actor_u16(0xAC, 0x2F);
    s.set_actor_u16(0xAE, 0x3F);
    s.set_actor_u32(0xA0, 0x0080_8080);
    // Angles stay in the 12-bit circle the pack shifts to a byte; the
    // translations in the pack's 12 bits.
    let angle = |rng: &mut Rng| (rng.below(0x1000) as i16) & !0xF;
    let coord = |rng: &mut Rng| rng.below(0x800) as i16 - 0x400;
    s.keyframe_pose = (0..parts)
        .map(|_| {
            let mut kf = [0i16; 12];
            for half in [0, 6] {
                kf[half + 1] = angle(rng);
                kf[half + 2] = angle(rng);
                for k in 3..6 {
                    kf[half + k] = coord(rng);
                }
            }
            kf
        })
        .collect();
    s
}

#[test]
fn the_animated_mesh_renderer_poses_each_part_as_the_port_does() {
    differential!(scus(RENDERER));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    // The leaves the plain-model path does not reach, and the three draws.
    for va in [
        0x8004_3390,
        0x8002_735C,
        0x8002_9888,
        0x8001_D184,
        0x8001_C204,
        0x8001_C604,
        0x8001_C394,
        0x8001_CF50,
    ] {
        cpu.stub(va, 0);
    }
    let posed: Rc<RefCell<Vec<Pose>>> = Rc::default();
    {
        let posed = posed.clone();
        cpu.probe(LEAF_CALL, move |_, _, g| {
            let c = |r: usize| g.read_ctrl(r);
            let lo = |w: u32| w as u16 as i16 as i32;
            let hi = |w: u32| (w >> 16) as u16 as i16 as i32;
            let m = [
                lo(c(0)),
                hi(c(0)),
                lo(c(1)),
                hi(c(1)),
                lo(c(2)),
                hi(c(2)),
                lo(c(3)),
                hi(c(3)),
                lo(c(4)),
            ];
            posed
                .borrow_mut()
                .push((m, [c(5) as i32, c(6) as i32, c(7) as i32]));
        });
    }
    // The boot's sine / cosine table pointers `FUN_80026988` reads.
    cpu.mem.wr32(0x8007_B81C, 0x8007_0A2C);
    cpu.mem.wr32(0x8007_B7F8, 0x8007_0A2C + 0x800);
    let mut rng = Rng(0x8001_B964);
    let mut worst = 0.0f64;
    let mut rotated = 0;
    for round in 0..300 {
        let parts = 1 + rng.below(8) as usize;
        let s = node(&mut rng, parts);
        let entries = keyframe_pose_entries(&s);
        assert_eq!(entries.len(), parts);
        // The camera at identity, no translation.
        for i in 0..9u32 {
            let v = if i % 4 == 0 { 0x1000 } else { 0 };
            cpu.mem.wr16(CAMERA + 2 * i, v);
        }
        for o in (0x14..0x20).step_by(4) {
            cpu.mem.wr32(CAMERA + o, 0);
        }
        cpu.mem.wr32(0x1F80_02D0, 0);
        // The node: no rotation, at the origin, the scale, frame 0.
        for o in (0..0x100).step_by(4) {
            cpu.mem.wr32(ACTOR + o, 0);
        }
        cpu.mem.wr16(ACTOR + 0x72, s.field_72);
        cpu.mem.wr32(ACTOR + 0x44, LIST);
        cpu.mem.wr32(ACTOR + 0x4C, CLIP);
        cpu.mem.wr32(ACTOR + 0x74, s.field_74);
        // The model list: every slot the one quad, no base-rotation part.
        cpu.mem.wr32(LIST, parts as u32);
        for i in 0..parts as u32 {
            cpu.mem.wr32(LIST + 4 + 4 * i, OBJECT);
        }
        cpu.mem.wr16(LIST + 0x9A, 0xFFFF);
        cpu.mem.wr32(OBJECT + 0xC, 0);
        // The clip the move VM packs: count, one frame, rate 1, then entries.
        cpu.mem.load(CLIP, &[0; 8]);
        cpu.mem.wr8(CLIP, parts as u8);
        cpu.mem.wr16(CLIP + 2, 1);
        cpu.mem.wr8(CLIP + 6, 1);
        for (i, e) in entries.iter().enumerate() {
            cpu.mem.load(CLIP + 8 + 8 * i as u32, e);
        }
        posed.borrow_mut().clear();
        assert_eq!(cpu.call(RENDERER, &[ACTOR]), Stop::Returned, "{round}");

        let port = sprite_arm_draws(&s);
        let base = sprite_arm_draw(&s).expect("a sprite-arm node");
        let k = f64::from(s.field_72) / 4096.0;
        let posed = posed.borrow();
        assert_eq!(posed.len(), parts, "one draw a part, {round}");
        assert_eq!(port.len(), parts, "{round}");
        for (i, ((m, t), d)) in posed.iter().zip(&port).enumerate() {
            rotated += usize::from(m[1] != 0 || m[2] != 0 || m[5] != 0);
            for (b, p) in base.mesh.positions.iter().zip(&d.mesh.positions) {
                // The quad's own corner, before the scale.
                let v = b.map(|c| (f64::from(c) / k).round());
                for row in 0..3 {
                    let r = (m[3 * row] as f64 * v[0]
                        + m[3 * row + 1] as f64 * v[1]
                        + m[3 * row + 2] as f64 * v[2])
                        / 4096.0
                        + t[row] as f64;
                    let err = (r - f64::from(p[row])).abs();
                    worst = worst.max(err);
                    assert!(
                        err <= 2.0 + 0.004 * r.abs(),
                        "round {round} part {i} row {row}: retail {r} port {} (m {m:?} t {t:?})",
                        p[row]
                    );
                }
            }
        }
    }
    assert!(rotated > 500, "{rotated}");
    eprintln!("worst corner error {worst:.3}");
}
