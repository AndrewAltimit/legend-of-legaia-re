//! `FUN_80021DF4`, the per-frame tick of every move-VM part, against the
//! kernels that port its arms, composed in the order the field's ambient tick
//! (`World::step_ambient_fx`) runs them:
//!
//! - the wait drain and the `+0x22` cursor step
//!   (`move_vm::decrement_wait_timer`, `move_vm::advance_keyframe_cursor`);
//! - the mode-`2` / `6` channel block (`move_vm::integrate_draw_channels`);
//! - the mode-`3` CLUT-cell cycler (`clut_cell_fx::mode3_integrate`);
//! - the mode-`5` positional sound emitter (`sfx_emitter::emitter_tick`);
//! - the motion block every other mode runs and the post-VM clamps
//!   (`part_motion::motion_block`, `part_motion::clamp_levels`);
//! - the move-VM gate (`move_vm::actor_tick`);
//! - the mode-`7` framebuffer grab (`vram_grab::grab_request`);
//! - the mode-`6` pose pack (`move_vm::keyframe_pose_entries`).
//!
//! Each round seats a random part record, runs one retail tick with every call
//! out of the routine hooked or stubbed, and compares the whole record and
//! every call the arms make with the port's.

use crate::common::{Rng, scus_cpu};
use legaia_engine_core::world::runtime_sfx_descriptor_in;
use legaia_engine_effects::part_motion::{clamp_levels, motion_block, runs_motion_block};
use legaia_engine_field::clut_cell_fx::{SPEED_SCALAR, mode3_integrate};
use legaia_engine_system::rotation_lut::{RotationLut, retail_rotation_lut};
use legaia_engine_vm::move_vm::{
    ActorState, ActorTickOutcome, MoveHost, actor_tick, advance_keyframe_cursor,
    decrement_wait_timer, integrate_draw_channels, keyframe_pose_entries,
};
use legaia_engine_vm::sfx_emitter::{EmitterListener, EmitterOp, emitter_tick, emitter_voices};
use legaia_engine_vm::vram_grab::grab_request;
use psx_retail_exec::{A0, A1, A2, A3, Cpu, Memory, SP, Stop, V0};
use std::cell::RefCell;
use std::rc::Rc;

const PART_TICK: u32 = 0x8002_1DF4;
const PART_TICK_END: u32 = 0x8002_3070;
const MOVE_VM: u32 = 0x8002_3070;
const CLUT_CELL: u32 = 0x8001_9D50;
const BASE_MATRIX: u32 = 0x8003_D1A4;
const RTPS: u32 = 0x8003_D368;
const RECT_COPY: u32 = 0x8004_68A4;
const KEY_ON: u32 = 0x8006_5034;
const SET_VOLUME: u32 = 0x8006_57D0;
const RELEASE: u32 = 0x8002_50D4;
/// `DAT_1F800393` (frame step) and `DAT_1F80037D` (speed) off this base.
const SCRATCH: u32 = 0x1F80_0314;
/// The ordering-table shift byte the grab's slot reads.
const OT_SHIFT: u32 = 0x1F80_03A4;
const SIN_PTR: u32 = 0x8007_B81C;
const COS_PTR: u32 = 0x8007_B7F8;
const SIN: u32 = 0x8014_0000;
const COS: u32 = 0x8014_2000;
const ACTOR: u32 = 0x8013_0000;
const RECORD: usize = 0x1AC;
const POSE: u32 = 0x8015_0000;
const POSE_COUNT: u32 = 0x8015_1000;

fn h(r: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([r[o], r[o + 1]])
}
fn w(r: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([r[o], r[o + 1], r[o + 2], r[o + 3]])
}
fn put16(r: &mut [u8], o: usize, v: u16) {
    r[o..o + 2].copy_from_slice(&v.to_le_bytes());
}
fn put32(r: &mut [u8], o: usize, v: u32) {
    r[o..o + 4].copy_from_slice(&v.to_le_bytes());
}

/// The halfword fields of `ActorState` by record offset.
macro_rules! halves {
    ($m:ident) => {
        $m!(0x14, world_x, i16);
        $m!(0x16, world_y, i16);
        $m!(0x18, world_z, i16);
        $m!(0x22, y_rot, i16);
        $m!(0x24, render_24, i16);
        $m!(0x26, render_26, i16);
        $m!(0x28, render_28, i16);
        $m!(0x2A, world_y_mirror, i16);
        $m!(0x3C, anim_3c, i16);
        $m!(0x3E, anim_3e, i16);
        $m!(0x40, anim_40, i16);
        $m!(0x52, field_52, u16);
        $m!(0x54, wait_timer, i16);
        $m!(0x56, move_substate, i16);
        $m!(0x5A, move_submode, i16);
        $m!(0x68, field_68, i16);
        $m!(0x6A, field_6a, i16);
        $m!(0x72, field_72, u16);
        $m!(0x78, field_78, u16);
        $m!(0x7A, field_7a, u16);
        $m!(0x80, anim_80, i16);
        $m!(0x82, anim_82, i16);
        $m!(0x84, anim_84, i16);
        $m!(0x86, field_86, u16);
        $m!(0x90, tween_src_x, i16);
        $m!(0x92, tween_src_y, i16);
        $m!(0x94, tween_src_z, i16);
        $m!(0x96, tween_scale_x, i16);
        $m!(0x98, tween_scale_y, i16);
        $m!(0x9A, tween_scale_z, i16);
    };
}

fn from_record(r: &[u8]) -> ActorState {
    let mut s = ActorState::new();
    macro_rules! rd {
        ($o:expr, $f:ident, $t:ty) => {
            s.$f = h(r, $o) as $t;
        };
    }
    halves!(rd);
    s.flags = w(r, 0x10);
    for o in (0x9C..RECORD).step_by(2) {
        s.set_actor_u16(o, h(r, o));
    }
    s
}

fn to_record(s: &ActorState, r: &mut [u8]) {
    macro_rules! wr {
        ($o:expr, $f:ident, $t:ty) => {
            put16(r, $o, s.$f as u16);
        };
    }
    halves!(wr);
    put32(r, 0x10, s.flags);
    for o in (0x9C..RECORD).step_by(2) {
        put16(r, o, s.actor_u16(o));
    }
}

/// A random record: every halfword random, then the fields the arms under
/// test do not own pinned (no field cull, no clip advancer, no pose block).
fn random_record(rng: &mut Rng, mode: i16) -> Vec<u8> {
    let mut r: Vec<u8> = (0..RECORD).map(|_| rng.next() as u8).collect();
    put16(&mut r, 0x5A, mode as u16);
    put16(&mut r, 0x56, 0);
    let f86 = h(&r, 0x86) & !0x2000;
    put16(&mut r, 0x86, f86);
    put32(&mut r, 0x44, POSE_COUNT);
    put32(&mut r, 0x4C, 0);
    // A wait timer either side of the gate.
    put16(&mut r, 0x54, (rng.below(160) as i16 - 60) as u16);
    // The depth-cue level and scale either side of their clamps.
    for o in [0x72, 0x78] {
        put16(&mut r, o, rng.below(0x4400) as u16);
    }
    r
}

/// The two speed bytes and the rotation tables every tick reads.
fn seat_tick(cpu: &mut Cpu, frame_step: u8, speed: u8) {
    cpu.mem.wr8(SCRATCH + 0x7F, frame_step);
    cpu.mem.wr8(SCRATCH + 0x69, speed);
}

fn seat_tables(cpu: &mut Cpu) {
    let lut = retail_rotation_lut();
    for i in 0..0x1000u32 {
        cpu.mem.wr16(SIN + i * 2, lut.b(i as i32) as u16);
        cpu.mem.wr16(COS + i * 2, lut.a(i as i32) as u16);
    }
    cpu.mem.wr32(SIN_PTR, SIN);
    cpu.mem.wr32(COS_PTR, COS);
}

struct Null;
impl MoveHost for Null {}

/// Whether the move-VM gate runs the VM this tick, and whether its run
/// leaves the part halted - the port's `actor_tick` on a scratch copy, with
/// the VM itself stubbed on both sides.
fn gate(st: &ActorState) -> (bool, bool) {
    let mut probe = st.clone();
    match actor_tick(&mut Null, &mut probe, &[], 1) {
        ActorTickOutcome::Waiting => (false, false),
        ActorTickOutcome::Halted => (true, true),
        _ => (true, false),
    }
}

type Calls = Rc<RefCell<Vec<(u32, [u32; 8])>>>;

/// Log every call to `va`: its four register arguments and four stack ones.
fn log_calls(
    cpu: &mut Cpu,
    va: u32,
    calls: &Calls,
    ret: impl Fn(&mut Memory, &[u32; 8]) -> u32 + 'static,
) {
    let calls = calls.clone();
    cpu.hook(va, move |r, m| {
        let sp = r.r[SP];
        let args = [
            r.r[A0],
            r.r[A1],
            r.r[A2],
            r.r[A3],
            m.rd32(sp + 0x10),
            m.rd32(sp + 0x14),
            m.rd32(sp + 0x18),
            m.rd32(sp + 0x1C),
        ];
        r.r[V0] = ret(m, &args);
        calls.borrow_mut().push((va, args));
    });
}

/// Every mode but the scroller (`4`, `scene_small_leaves`) and the emitter
/// (`5`, below): the drain, the cursor, the channel block, the CLUT-cell
/// cycler, the motion block, the gate, the clamps, the grab and the pose pack
/// each against its port.
#[test]
fn the_part_tick_steps_the_part_the_port_steps() {
    differential!(scus(PART_TICK));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.stub_outside = Some((PART_TICK, PART_TICK_END));
    seat_tables(&mut cpu);
    let calls: Calls = Rc::default();
    // The projection the grab makes: `SXY2` into the second argument, `IR3`
    // back.
    const SX: i16 = 0x55;
    const SY: i16 = -0x21;
    const IR3: u32 = 0x1234;
    log_calls(&mut cpu, RTPS, &calls, |m, a| {
        m.wr16(a[1], SX as u16);
        m.wr16(a[1] + 2, SY as u16);
        IR3
    });
    for va in [MOVE_VM, CLUT_CELL, BASE_MATRIX, RECT_COPY] {
        log_calls(&mut cpu, va, &calls, |_, _| 0);
    }
    let mut rng = Rng(0x5041_5254);
    let mut seen = std::collections::BTreeMap::<&str, u32>::new();
    for round in 0..6_000 {
        let mode = [0i16, 1, 2, 3, 6, 7, 8, -1, 0x10][rng.below(9) as usize];
        let mut rec = random_record(&mut rng, mode);
        let frame_step = 1 + rng.below(3) as u8;
        let speed = if mode == 3 {
            SPEED_SCALAR as u8
        } else {
            1 + rng.below(16) as u8
        };
        if mode == 3 {
            // The cycler's gate, white level and its rate as op `0x2C` and
            // the HSV ops leave them.
            let gate = match rng.below(4) {
                0 => 0,
                _ => rng.below(1_110) as i16 - 4,
            };
            put16(&mut rec, 0x9C, gate as u16);
            put16(&mut rec, 0x68, (rng.below(0x400) as i16 - 0x200) as u16);
            put16(&mut rec, 0x6A, (rng.below(0x800) as i16 - 0x400) as u16);
            // The tick zeroes a negative `+0x7A` in every mode; the port's
            // motion block only where it runs.
            let f7a = h(&rec, 0x7A) & 0x7FFF;
            put16(&mut rec, 0x7A, f7a);
            // The capture buffer: the op-`0x2C` pointer at `+0xA8`, or
            // none and the record's own `+0xAC` block.
            if rng.below(2) == 0 {
                put32(&mut rec, 0xA8, 0);
            }
        }
        let shift = rng.below(4) as u8;
        cpu.mem.wr8(OT_SHIFT, shift);
        // A seated pose block on half the mode-6 parts.
        let parts = rng.below(6) as i32 - 1;
        let mut pose = vec![0i16; 12 * parts.max(0) as usize];
        if mode == 6 && rng.below(2) == 0 {
            put32(&mut rec, 0x4C, POSE);
            cpu.mem.wr32(POSE_COUNT, parts as u32);
            for (i, v) in pose.iter_mut().enumerate() {
                *v = rng.next() as i16;
                let at = POSE + 8 + 8 * parts as u32 + 2 * i as u32;
                cpu.mem.wr16(at, *v as u16);
            }
        }
        cpu.mem.load(ACTOR, &rec);
        seat_tick(&mut cpu, frame_step, speed);
        calls.borrow_mut().clear();
        assert_eq!(
            cpu.call(PART_TICK, &[ACTOR]),
            Stop::Returned,
            "round {round}"
        );

        // The port, in the ambient tick's order.
        let mut st = from_record(&rec);
        let delta = u16::from(frame_step) * u16::from(speed);
        decrement_wait_timer(&mut st, delta);
        advance_keyframe_cursor(&mut st, frame_step);
        integrate_draw_channels(&mut st, delta);
        let cell = if st.move_submode == 3 {
            mode3_integrate(&mut st, frame_step)
        } else {
            None
        };
        if runs_motion_block(&st) {
            motion_block(&mut st, delta);
        }
        let (ran, halted) = gate(&st);
        let mut want: Vec<(u32, Vec<u32>)> = Vec::new();
        if let Some(fx) = cell {
            want.push((
                CLUT_CELL,
                vec![
                    fx.mode as u32,
                    fx.white as u32,
                    fx.h_add as u32,
                    fx.s_add as u32,
                    fx.v_add as u32,
                ],
            ));
        }
        if ran {
            want.push((MOVE_VM, vec![ACTOR]));
        }
        let mut grab = None;
        if !halted {
            clamp_levels(&mut st);
            grab = grab_request(&st);
            if let Some(g) = grab {
                if g.camera_locked {
                    want.push((BASE_MATRIX, vec![0x8007_BF10]));
                }
                want.push((RTPS, vec![ACTOR + 0x14]));
                want.push((
                    RECT_COPY,
                    vec![
                        ((IR3 as i32 + i32::from(g.depth_bias)) >> (shift + 2)) as u32,
                        SX.wrapping_add(g.offset[0]) as u32,
                        SY.wrapping_add(g.offset[1]) as u32,
                        g.size[0] as u32,
                        g.size[1] as u32,
                        g.dest[0] as u32,
                        g.dest[1] as u32,
                    ],
                ));
            }
        }
        // What retail called, cut to the arguments the port names.
        let got: Vec<(u32, Vec<u32>)> = calls
            .borrow()
            .iter()
            .map(|&(va, a)| {
                let args = match va {
                    CLUT_CELL => vec![
                        a[0] as i16 as u32,
                        a[1] as i16 as u32,
                        a[2] as i16 as u32,
                        a[3] as i16 as u32,
                        a[4] as i16 as u32,
                    ],
                    MOVE_VM | BASE_MATRIX | RTPS => vec![a[0]],
                    RECT_COPY => vec![
                        a[0],
                        a[1] as i16 as u32,
                        a[2] as i16 as u32,
                        a[3] as i16 as u32,
                        a[4] as i16 as u32,
                        a[5] as i16 as u32,
                        a[6] as i16 as u32,
                    ],
                    _ => unreachable!(),
                };
                (va, args)
            })
            .collect();
        assert_eq!(got, want, "round {round}: mode {mode} calls");
        // The cycler's rect is the one op `0x2C` captured, read through the
        // pointer retail hands the kernel.
        if let Some(fx) = cell {
            let &(_, a) = calls.borrow().iter().find(|c| c.0 == CLUT_CELL).unwrap();
            let rect: Vec<u16> = (0..4).map(|k| cpu.mem.rd16(a[6] + 2 * k)).collect();
            assert_eq!(
                rect,
                [fx.rect.0, fx.rect.1, fx.rect.2, fx.rect.3],
                "round {round}: cell rect"
            );
            *seen.entry("cell").or_default() += 1;
        }
        // The pose pack.
        if !halted && mode == 6 && w(&rec, 0x4C) != 0 {
            st.keyframe_pose = pose
                .chunks(12)
                .map(|c| std::array::from_fn(|k| c[k]))
                .collect();
            for (k, entry) in keyframe_pose_entries(&st).iter().enumerate() {
                let got: Vec<u8> = (0..8)
                    .map(|b| cpu.mem.rd8(POSE + 8 + 8 * k as u32 + b))
                    .collect();
                assert_eq!(got, entry, "round {round}: pose part {k}");
            }
            *seen
                .entry(if parts > 0 { "pose" } else { "no parts" })
                .or_default() += 1;
        }
        let mut port = rec.clone();
        to_record(&st, &mut port);
        let retail: Vec<u8> = (0..RECORD as u32).map(|i| cpu.mem.rd8(ACTOR + i)).collect();
        let diff: Vec<String> = retail
            .iter()
            .zip(&port)
            .enumerate()
            .filter(|(_, (a, b))| a != b)
            .take(8)
            .map(|(i, (a, b))| format!("+{i:#x}: retail {a:#04x} port {b:#04x}"))
            .collect();
        assert!(diff.is_empty(), "round {round}: mode {mode}: {diff:?}");
        *seen.entry(if ran { "vm" } else { "waiting" }).or_default() += 1;
        if halted {
            *seen.entry("halted").or_default() += 1;
        }
        if grab.is_some() {
            *seen.entry("grab").or_default() += 1;
        }
    }
    for (what, at_least) in [
        ("cell", 200),
        ("pose", 100),
        ("no parts", 20),
        ("vm", 1_000),
        ("waiting", 1_000),
        ("halted", 300),
        ("grab", 300),
    ] {
        assert!(seen.get(what).copied().unwrap_or(0) >= at_least, "{seen:?}");
    }
}

/// The emitter's world inputs, retail's globals.
mod emitter_globals {
    /// The camera focus, stored negated (X, then Z eight bytes on).
    pub const FOCUS_X: u32 = 0x8008_9118;
    pub const FOCUS_Z: u32 = 0x8008_9120;
    pub const VOICE_VOLUME: u32 = 0x8008_4580;
    pub const MONO: u32 = 0x8008_46BC;
    pub const SCALE_ROWS: u32 = 0x8009_1658;
    pub const GAME_MODE: u32 = 0x8007_B83C;
    pub const SIDE_BAND_ACK: u32 = 0x8007_BABC;
    pub const SIDE_BAND_REQ: u32 = 0x8007_BAA0;
    pub const RELEASE_ALL: u32 = 0x8007_B9EC;
    pub const DEBUG_WORD: u32 = 0x8007_B6D0;
    pub const FORCED_BANK: u32 = 0x8007_BA88;
    pub const RUNTIME_BUNDLE: u32 = 0x8007_B8D0;
    /// The static cue descriptors, eight bytes a cue.
    pub const STATIC_CUES: u32 = 0x8006_F198;
}

/// The render-mode-5 arm, a positional sound emitter: the level ramp, the
/// stop request, the box and its falloff, the side-band and field gates, the
/// release-all, and the key / volume / release calls it makes, against
/// `sfx_emitter::emitter_tick` and `emitter_voices`, with the rest of the
/// tick (drain, gate, clamps) as above.
#[test]
fn the_part_tick_sounds_the_emitter_the_port_sounds() {
    differential!(scus(PART_TICK));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    use emitter_globals::*;
    const BUNDLE: u32 = 0x8016_0000;
    cpu.stub_outside = Some((PART_TICK, PART_TICK_END));
    seat_tables(&mut cpu);
    let calls: Calls = Rc::default();
    for va in [MOVE_VM, KEY_ON, SET_VOLUME, RELEASE] {
        log_calls(&mut cpu, va, &calls, |_, _| 0);
    }
    let mut rng = Rng(0x534F_554E);
    // A runtime bundle: the header halfword at `+2` names the first row.
    let bundle: Vec<u8> = {
        let mut b: Vec<u8> = (0..0x2000).map(|_| rng.next() as u8).collect();
        b[2..4].copy_from_slice(&0x40u16.to_le_bytes());
        b
    };
    cpu.mem.load(BUNDLE, &bundle);
    cpu.mem.wr32(RUNTIME_BUNDLE, BUNDLE);
    cpu.mem.wr32(FORCED_BANK, 0);
    let mut seen = std::collections::BTreeMap::<&str, u32>::new();
    for round in 0..8_000 {
        let mut rec = random_record(&mut rng, 5);
        let frame_step = 1 + rng.below(3) as u8;
        let speed = 1 + rng.below(16) as u8;
        // Levels and rates as op `0x34` and the ramp ops leave them.
        for o in [0x90, 0x92, 0x94, 0x96] {
            put16(&mut rec, o, rng.below(0x80) as u16);
        }
        for o in [0x98, 0x9A] {
            put16(&mut rec, o, (rng.below(0x40) as i16 - 0x20) as u16);
        }
        // Part, focus and box: inside, outside, or a zero box.
        let (x, z) = (
            rng.below(8_000) as i32 - 4_000,
            rng.below(8_000) as i32 - 4_000,
        );
        put16(&mut rec, 0x14, x as u16);
        put16(&mut rec, 0x18, z as u16);
        let (fx, fz) = (
            -(x + rng.below(3_000) as i32 - 1_500),
            -(z + rng.below(3_000) as i32 - 1_500),
        );
        // Mostly a box that straddles the part; now and then one whose edges
        // share a sign, which the falloff measures from the far side.
        let edge = |rng: &mut Rng, sign: i32| {
            let sign = if rng.below(8) == 0 { -sign } else { sign };
            sign * (1 + rng.below(2_000) as i32)
        };
        let mut range = [
            edge(&mut rng, -1),
            edge(&mut rng, 1),
            edge(&mut rng, 1),
            edge(&mut rng, -1),
        ];
        if rng.below(6) == 0 {
            range[0] = 0;
        }
        for (k, v) in range.iter().enumerate() {
            put32(&mut rec, 0x9C + 4 * k, *v as u32);
        }
        // The cue: the static table or the runtime bundle.
        let cue = if rng.below(2) == 0 {
            rng.below(0x100)
        } else {
            0x200 + rng.below(0x100)
        };
        put32(&mut rec, 0xAC, cue);
        let voice = rng.below(26) as i16 - 2;
        put16(&mut rec, 0xB0, voice as u16);
        let row = rng.below(4);
        put16(&mut rec, 0xB2, row as u16);
        put32(&mut rec, 0xB4, rng.below(2));
        // The timed glide on a third of the parts.
        let glide = rng.below(3) == 0;
        put32(&mut rec, 0xB8, u32::from(glide));
        put32(&mut rec, 0xBC, rng.below(12));
        put32(&mut rec, 0xC0, 1 + rng.below(12));
        let mut flags = w(&rec, 0x10) & !0x1_0008;
        if rng.below(10) == 0 {
            flags |= 0x1_0000;
        }
        put32(&mut rec, 0x10, flags);
        let stop = flags & 0x1_0000 != 0;
        let f7a = h(&rec, 0x7A) & 0x7FFF;
        put16(&mut rec, 0x7A, f7a);
        cpu.mem.load(ACTOR, &rec);

        let listener = EmitterListener {
            focus_stored: (fx, fz),
            voice_volume: rng.below(0x400) as i32,
            mono: rng.below(4) == 0,
            scale: (
                rng.below(0x180) as i32 - 0x40,
                rng.below(0x180) as i32 - 0x40,
            ),
            field_mode: rng.below(5) != 0,
            stream_settled: rng.below(6) != 0,
            release_all: rng.below(12) == 0,
            frame_step: i32::from(frame_step),
            speed: i32::from(speed),
        };
        cpu.mem.wr32(FOCUS_X, fx as u32);
        cpu.mem.wr32(FOCUS_Z, fz as u32);
        cpu.mem.wr32(VOICE_VOLUME, listener.voice_volume as u32);
        cpu.mem.wr32(MONO, u32::from(listener.mono));
        cpu.mem.wr32(SCALE_ROWS + 16 * row, listener.scale.0 as u32);
        cpu.mem
            .wr32(SCALE_ROWS + 16 * row + 4, listener.scale.1 as u32);
        cpu.mem
            .wr16(GAME_MODE, if listener.field_mode { 3 } else { 2 });
        cpu.mem.wr32(SIDE_BAND_REQ, 7);
        cpu.mem
            .wr32(SIDE_BAND_ACK, if listener.stream_settled { 7 } else { 6 });
        cpu.mem.wr32(RELEASE_ALL, u32::from(listener.release_all));
        // The debug print's two gates: it writes nothing the port models.
        cpu.mem
            .wr32(DEBUG_WORD, [0u32, 4, 5, 1][rng.below(4) as usize]);
        seat_tick(&mut cpu, frame_step, speed);
        calls.borrow_mut().clear();
        assert_eq!(
            cpu.call(PART_TICK, &[ACTOR]),
            Stop::Returned,
            "round {round}"
        );

        let mut st = from_record(&rec);
        let delta = u16::from(frame_step) * u16::from(speed);
        decrement_wait_timer(&mut st, delta);
        advance_keyframe_cursor(&mut st, frame_step);
        integrate_draw_channels(&mut st, delta);
        let op = emitter_tick(&mut st, &listener);
        assert!(!runs_motion_block(&st));
        let (ran, halted) = gate(&st);
        let mut want: Vec<(u32, Vec<u32>)> = Vec::new();
        let count = if cue < 0x200 {
            cpu.mem.rd8(STATIC_CUES + 8 * cue + 3)
        } else {
            runtime_sfx_descriptor_in(&bundle, cue as i16).unwrap()[3]
        };
        match op {
            Some(EmitterOp::Key {
                cue: c,
                voice: v,
                vol_l,
                vol_r,
            }) => {
                assert_eq!((c as u32, v), (cue, voice));
                for vc in emitter_voices(v, count) {
                    want.push((KEY_ON, vec![vc as u32, vol_l as u32, vol_r as u32]));
                }
                *seen.entry("key").or_default() += 1;
            }
            Some(EmitterOp::Volume {
                cue: c,
                voice: v,
                left,
                right,
            }) => {
                assert_eq!((c as u32, v), (cue, voice));
                for i in 0..i32::from(count & 0x1F) {
                    want.push((
                        SET_VOLUME,
                        vec![(i32::from(v) + i) as u32, u32::from(left), u32::from(right)],
                    ));
                }
                *seen.entry("volume").or_default() += 1;
            }
            Some(EmitterOp::Release { cue: c, voice: v }) => {
                want.push((RELEASE, vec![c as u32, v as u32]));
                *seen.entry("release").or_default() += 1;
            }
            None => *seen.entry("quiet").or_default() += 1,
        }
        // A stop request leaves the routine from the arm (`0x800221FC` to the
        // epilogue): no VM run and no clamps that tick, as the ambient tick
        // skips the rest of the part.
        if !stop {
            if ran {
                want.push((MOVE_VM, vec![ACTOR]));
            }
            if !halted {
                clamp_levels(&mut st);
            }
        }
        let got: Vec<(u32, Vec<u32>)> = calls
            .borrow()
            .iter()
            .map(|&(va, a)| {
                let args = match va {
                    KEY_ON => vec![a[0] as i16 as u32, a[6] as i16 as u32, a[7] as i16 as u32],
                    SET_VOLUME => vec![a[0] as i16 as u32, a[1] as i16 as u32, a[2] as i16 as u32],
                    RELEASE => vec![a[0], a[1] as i16 as u32],
                    MOVE_VM => vec![a[0]],
                    _ => unreachable!(),
                };
                (va, args)
            })
            .collect();
        assert_eq!(got, want, "round {round}: {op:?}");
        let mut port = rec.clone();
        to_record(&st, &mut port);
        let retail: Vec<u8> = (0..RECORD as u32).map(|i| cpu.mem.rd8(ACTOR + i)).collect();
        let diff: Vec<String> = retail
            .iter()
            .zip(&port)
            .enumerate()
            .filter(|(_, (a, b))| a != b)
            .take(8)
            .map(|(i, (a, b))| format!("+{i:#x}: retail {a:#04x} port {b:#04x}"))
            .collect();
        assert!(diff.is_empty(), "round {round}: {op:?}: {diff:?}");
        if stop {
            *seen.entry("stop").or_default() += 1;
        }
        if glide {
            *seen.entry("glide").or_default() += 1;
        }
    }
    for (what, at_least) in [
        ("key", 300),
        ("volume", 300),
        ("release", 300),
        ("quiet", 300),
        ("stop", 300),
        ("glide", 1_000),
    ] {
        assert!(seen.get(what).copied().unwrap_or(0) >= at_least, "{seen:?}");
    }
}
