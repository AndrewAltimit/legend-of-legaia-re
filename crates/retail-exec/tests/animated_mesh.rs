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
/// The `jal 0x8002735C` on the clip arm (a raised `+0x42`).
const CLIP_CALL: u32 = 0x8001_BD88;
/// The `jal 0x80029888` on the shatter path (a non-zero `+0x7A`).
const SHATTER_CALL: u32 = 0x8001_BD70;
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
    // A quarter of the angles are zero: the renderer skips the axis call
    // for a zero keyframe angle (`0x8001BB9C` / `0x8001BBBC` / `0x8001BBDC`).
    let angle = |rng: &mut Rng| {
        if rng.below(4) == 0 {
            0
        } else {
            (rng.below(0x1000) as i16) & !0xF
        }
    };
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

/// The animated renderer's refusals and its ordering-table shift, against
/// the port's.
///
/// * A render scale `+0x72` of `0` draws nothing (`beq v0,zero` at
///   `0x8001B9A0`); the port's node draws no quad at scale `0`.
/// * A clip whose part count is not the model list's draws nothing
///   (`bne v1,a0` at `0x8001BAF0`), nor does an empty list (`0x8001BAF8`):
///   the port's keyframe-mesh node with no seated pose block draws nothing.
/// * A bit of `0x80800000` in the walk's scratch copy of `+0x10`
///   (`0x1F8002D0`) lowers the ordering-table base `0x1F8003F4` by `0x28`,
///   ten buckets, for every part's draw (`0x8001B9B0..0x8001B9CC`), and the
///   draw's end puts it back (`0x8001BDCC..0x8001BDD4`):
///   `legaia_engine_core::world::draws_near_buckets`.
/// * A raised `+0x42` sends every part through `FUN_8001C204` and the TMD
///   renderer `FUN_8002735C` (`0x8001BC34`, `0x8001BC64` -> `0x8001BD80`)
///   in place of the dispatcher: the port's draw carries the kind
///   (`RibbonDraw::object_effect_kind`) its hosts clip it by.
/// * The drop shadow `FUN_8001C394` runs last (`0x8001BE20..0x8001BE48`),
///   past the scale-`0` early-out too, on a class bit of `0x01020000` with
///   `0x200000` clear: `legaia_render_kernels::drop_shadow::casts_shadow`.
/// * A non-zero `+0x7A` on an unraised part sends it through the shatter
///   renderer `FUN_80029888` (`bne v0,zero` at `0x8001BC74`), the raised arm
///   never testing it (`0x8001BC64`): `part_shatter::part_shatter_word`.
/// * A bit of `+0x52 & 0x780` builds the node's matrix camera-relative
///   (`FUN_8001CF50`, `0x8001BA10`), which the hosts read off the draw's
///   `flags_52`.
#[test]
fn the_animated_mesh_renderer_refuses_and_shifts_as_the_port_does() {
    differential!(scus(RENDERER));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
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
    const OT_BASE: u32 = 0x1F80_03F4;
    const WALK_FLAGS: u32 = 0x1F80_02D0;
    // Each part's draw: the table base it links at, and whether it went
    // down the clip arm (`jal 0x8002735C` at `0x8001BD88`).
    // Each part's draw: the table base it links at, and its path - `0` the
    // dispatcher, `1` the clip arm (`jal 0x8002735C` at `0x8001BD88`), `2`
    // the shatter renderer (`jal 0x80029888` at `0x8001BD70`).
    let seen: Rc<RefCell<Vec<(u32, u8)>>> = Rc::default();
    for (va, path) in [(LEAF_CALL, 0), (CLIP_CALL, 1), (SHATTER_CALL, 2)] {
        let seen = seen.clone();
        cpu.probe(va, move |_, m, _| {
            seen.borrow_mut().push((m.rd32(OT_BASE), path));
        });
    }
    // The node matrix's build: the camera-relative variant `FUN_8001CF50`
    // (`0x8001BA24`) or `MulMatrix2` against the camera (`0x8001BA40`).
    let relative: Rc<RefCell<Option<bool>>> = Rc::default();
    for (va, rel) in [(0x8001_BA24, true), (0x8001_BA40, false)] {
        let relative = relative.clone();
        cpu.probe(va, move |_, _, _| *relative.borrow_mut() = Some(rel));
    }
    // The drop shadow's call (`jal 0x8001C394` at `0x8001BE48`).
    let shadows: Rc<RefCell<u32>> = Rc::default();
    {
        let shadows = shadows.clone();
        cpu.probe(0x8001_BE48, move |_, _, _| *shadows.borrow_mut() += 1);
    }
    cpu.mem.wr32(0x8007_B81C, 0x8007_0A2C);
    cpu.mem.wr32(0x8007_B7F8, 0x8007_0A2C + 0x800);
    let mut rng = Rng(0x8001_B9A0);
    let (mut cast_rounds, mut shatter_rounds, mut relative_rounds) = (0, 0, 0);
    let (mut refused, mut shifted, mut plain, mut clip_rounds) = (0, 0, 0, 0);
    for round in 0..800 {
        let parts = 1 + rng.below(8) as usize;
        let mut s = node(&mut rng, parts);
        // Which refusal, if any: 0 none, 1 scale 0, 2 no pose block, 3 an
        // empty list and clip.
        let case = rng.below(4);
        match case {
            1 => s.field_72 = 0,
            2 | 3 => s.keyframe_pose.clear(),
            _ => {}
        }
        let list_parts = if case == 3 { 0 } else { parts };
        // A raised `+0x42` on some rounds sends every part down the clip arm.
        s.field_42 = [0, 0, 1, 2][rng.below(4) as usize];
        // The shatter word and the camera-relative bits on some rounds.
        s.field_7a = [0, 0, 0x10, 0x123][rng.below(4) as usize];
        s.field_52 = [0, 0, 0x80, 0x100, 0x200, 0x400, 0x20][rng.below(7) as usize];
        let entries = keyframe_pose_entries(&s);
        let flags = [
            0,
            0x0080_0000,
            0x8000_0000,
            0x0000_0800,
            0x8080_0000,
            0x0002_0000,
            0x0100_0000,
            0x0122_0000,
            0x0082_0000,
        ][rng.below(9) as usize];
        for i in 0..9u32 {
            cpu.mem
                .wr16(CAMERA + 2 * i, if i % 4 == 0 { 0x1000 } else { 0 });
        }
        for o in (0x14..0x20).step_by(4) {
            cpu.mem.wr32(CAMERA + o, 0);
        }
        cpu.mem.wr32(WALK_FLAGS, flags);
        let base = 0x8012_0000 + 4 * rng.below(0x100);
        cpu.mem.wr32(OT_BASE, base);
        for o in (0..0x100).step_by(4) {
            cpu.mem.wr32(ACTOR + o, 0);
        }
        cpu.mem.wr16(ACTOR + 0x72, s.field_72);
        cpu.mem.wr32(ACTOR + 0x44, LIST);
        cpu.mem.wr32(ACTOR + 0x4C, CLIP);
        cpu.mem.wr32(ACTOR + 0x74, s.field_74);
        cpu.mem.wr16(ACTOR + 0x42, s.field_42);
        cpu.mem.wr16(ACTOR + 0x7A, s.field_7a);
        cpu.mem.wr16(ACTOR + 0x52, s.field_52);
        // The actor's own flag word, which the shadow gate's opt-out reads.
        cpu.mem.wr32(ACTOR + 0x10, flags);
        cpu.mem.wr32(LIST, list_parts as u32);
        for i in 0..parts as u32 {
            cpu.mem.wr32(LIST + 4 + 4 * i, OBJECT);
        }
        cpu.mem.wr16(LIST + 0x9A, 0xFFFF);
        cpu.mem.wr32(OBJECT + 0xC, 0);
        cpu.mem.load(CLIP, &[0; 8]);
        cpu.mem.wr8(CLIP, entries.len() as u8);
        cpu.mem.wr16(CLIP + 2, 1);
        cpu.mem.wr8(CLIP + 6, 1);
        for (i, e) in entries.iter().enumerate() {
            cpu.mem.load(CLIP + 8 + 8 * i as u32, e);
        }
        seen.borrow_mut().clear();
        *shadows.borrow_mut() = 0;
        *relative.borrow_mut() = None;
        assert_eq!(cpu.call(RENDERER, &[ACTOR]), Stop::Returned, "{round}");
        let cast = *shadows.borrow() != 0;
        assert_eq!(
            cast,
            legaia_render_kernels::drop_shadow::casts_shadow(flags),
            "round {round}: flags {flags:#x} cast a blob {cast}"
        );
        cast_rounds += usize::from(cast);

        let port = sprite_arm_draws(&s);
        let seen = seen.borrow();
        assert_eq!(
            seen.len(),
            port.len(),
            "round {round} case {case}: retail drew {} parts, the port {}",
            seen.len(),
            port.len()
        );
        if port.is_empty() {
            refused += 1;
            continue;
        }
        let near = legaia_engine_core::world::draws_near_buckets(flags);
        let want = if near { base - 0x28 } else { base };
        assert!(
            seen.iter().all(|&(b, _)| b == want),
            "round {round}: flags {flags:#x} drew at {seen:x?}, the port's rule {want:#x}"
        );
        assert_eq!(
            cpu.mem.rd32(OT_BASE),
            base,
            "round {round}: the base comes back"
        );
        let shatter = legaia_engine_effects::part_shatter::part_shatter_word(&s) != 0;
        for (&(_, path), d) in seen.iter().zip(&port) {
            let want = if d.object_effect_kind != 0 {
                1
            } else if shatter {
                2
            } else {
                0
            };
            assert_eq!(
                path, want,
                "round {round}: +0x42 {} +0x7A {:#x} took path {path}, the port's {want}",
                s.field_42, s.field_7a
            );
            assert_eq!(d.object_effect_kind, s.field_42, "round {round}");
            // The hosts build a camera-relative node off the draw's `+0x52`.
            assert_eq!(
                *relative.borrow(),
                Some(d.flags_52 & 0x780 != 0),
                "round {round}: +0x52 {:#x}",
                s.field_52
            );
        }
        shatter_rounds += usize::from(shatter);
        relative_rounds += usize::from(s.field_52 & 0x780 != 0);
        clip_rounds += usize::from(s.field_42 != 0);
        if near {
            shifted += 1;
        } else {
            plain += 1;
        }
    }
    assert!(
        refused > 200
            && shifted > 60
            && plain > 40
            && clip_rounds > 60
            && cast_rounds > 60
            && shatter_rounds > 30
            && relative_rounds > 60,
        "{refused} {shifted} {plain} {clip_rounds} {cast_rounds} {shatter_rounds} {relative_rounds}"
    );
}

/// The look turn: the object the model list's `+0x9A` names is turned by
/// `RotZ(+0x98) RotY(+0x96) RotX(+0x94)` ahead of its keyframe rotation
/// (`0x8001BB40..0x8001BB88`), every other object keeps its own, against
/// `legaia_engine_field::actor_look::apply_look` over the decoded keyframes.
#[test]
fn the_animated_mesh_renderer_turns_the_looking_object_as_the_port_does() {
    differential!(scus(RENDERER));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
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
    let posed: Rc<RefCell<Vec<[i32; 9]>>> = Rc::default();
    {
        let posed = posed.clone();
        cpu.probe(LEAF_CALL, move |_, _, g| {
            let c = |r: usize| g.read_ctrl(r);
            let lo = |w: u32| w as u16 as i16 as i32;
            let hi = |w: u32| (w >> 16) as u16 as i16 as i32;
            posed.borrow_mut().push([
                lo(c(0)),
                hi(c(0)),
                lo(c(1)),
                hi(c(1)),
                lo(c(2)),
                hi(c(2)),
                lo(c(3)),
                hi(c(3)),
                lo(c(4)),
            ]);
        });
    }
    cpu.mem.wr32(0x8007_B81C, 0x8007_0A2C);
    cpu.mem.wr32(0x8007_B7F8, 0x8007_0A2C + 0x800);
    let euler = |a: [i16; 3]| -> [f64; 9] {
        let r = |v: i16| f64::from(v) * std::f64::consts::TAU / 4096.0;
        let (sx, cx) = r(a[0]).sin_cos();
        let (sy, cy) = r(a[1]).sin_cos();
        let (sz, cz) = r(a[2]).sin_cos();
        // Rz * Ry * Rx.
        [
            cz * cy,
            cz * sy * sx - sz * cx,
            cz * sy * cx + sz * sx,
            sz * cy,
            sz * sy * sx + cz * cx,
            sz * sy * cx - cz * sx,
            -sy,
            cy * sx,
            cy * cx,
        ]
    };
    let mut rng = Rng(0x8001_BB50);
    let mut turned = 0;
    for round in 0..300 {
        let parts = 1 + rng.below(6) as usize;
        let mut s = node(&mut rng, parts);
        s.field_72 = 0x1000;
        let entries = keyframe_pose_entries(&s);
        let object = if rng.below(4) == 0 {
            -1
        } else {
            rng.below(parts as u32) as i16
        };
        let look_angles = [0, 1, 2].map(|_| rng.below(0x1000) as i16 - 0x800);
        for i in 0..9u32 {
            cpu.mem
                .wr16(CAMERA + 2 * i, if i % 4 == 0 { 0x1000 } else { 0 });
        }
        for o in (0x14..0x20).step_by(4) {
            cpu.mem.wr32(CAMERA + o, 0);
        }
        cpu.mem.wr32(0x1F80_02D0, 0);
        for o in (0..0x100).step_by(4) {
            cpu.mem.wr32(ACTOR + o, 0);
        }
        cpu.mem.wr16(ACTOR + 0x72, 0x1000);
        cpu.mem.wr32(ACTOR + 0x44, LIST);
        cpu.mem.wr32(ACTOR + 0x4C, CLIP);
        cpu.mem.wr32(LIST, parts as u32);
        for i in 0..parts as u32 {
            cpu.mem.wr32(LIST + 4 + 4 * i, OBJECT);
        }
        cpu.mem.wr16(LIST + 0x9A, object as u16);
        for (k, a) in look_angles.iter().enumerate() {
            cpu.mem.wr16(LIST + 0x94 + 2 * k as u32, *a as u16);
        }
        cpu.mem.wr32(OBJECT + 0xC, 0);
        cpu.mem.load(CLIP, &[0; 8]);
        cpu.mem.wr8(CLIP, parts as u8);
        cpu.mem.wr16(CLIP + 2, 1);
        cpu.mem.wr8(CLIP + 6, 1);
        for (i, e) in entries.iter().enumerate() {
            cpu.mem.load(CLIP + 8 + 8 * i as u32, e);
        }
        posed.borrow_mut().clear();
        assert_eq!(cpu.call(RENDERER, &[ACTOR]), Stop::Returned, "{round}");

        let mut bones: Vec<([i16; 3], [i16; 3])> = entries
            .iter()
            .map(|e| {
                let t = legaia_actor_formats::player_anm::BoneTransform::decode(e);
                (
                    [t.t_x as i16, t.t_y as i16, t.t_z as i16],
                    [t.r_x as i16, t.r_y as i16, t.r_z as i16],
                )
            })
            .collect();
        legaia_engine_field::actor_look::apply_look(
            &mut bones,
            legaia_engine_field::actor_look::ActorLook {
                object,
                angles: look_angles,
            },
        );
        let posed = posed.borrow();
        assert_eq!(posed.len(), parts, "{round}");
        for (i, (m, (_, rot))) in posed.iter().zip(&bones).enumerate() {
            let want = euler(*rot);
            for (k, (&r, w)) in m.iter().zip(want).enumerate() {
                let err = (f64::from(r) / 4096.0 - w).abs();
                assert!(
                    err < 0.01,
                    "round {round} part {i} (look on {object}) [{k}]: retail {} port {w:.4}",
                    f64::from(r) / 4096.0
                );
            }
        }
        turned += usize::from(object >= 0);
    }
    assert!(turned > 150, "{turned}");
}
