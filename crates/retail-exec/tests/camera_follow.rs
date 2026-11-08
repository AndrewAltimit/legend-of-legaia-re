//! The field follow camera's per-frame kernels (PROT 0897): the composer
//! `FUN_801DAB90`, the ease `FUN_801DB510` that calls it and walks the live
//! camera globals toward its staging descriptor, and the focus edge clamp
//! `FUN_801DAA50` every caller runs after it. Held against `compose`,
//! `ease_step` / `ease_step_i16` and `clamp_focus` over generated parameter
//! blocks of every mode nibble and sweep nibble, attribute boxes, players,
//! floors and live poses.
//!
//! The floor sampler under the composer is answered by a hook (its own
//! differential is `floor_height`), and so is the square root `FUN_8005B0B8`,
//! whose leading-zero count is a GTE register this interpreter does not carry
//! (its table is pinned in `parity/tests/field_camera_zone_oracle.rs`).

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_field::camera_zone::{
    CameraZoneConfig, ComposeInputs, EASE_SHIFT_TABLE, clamp_focus, compose, ease_step,
    ease_step_i16, sqrt0,
};
use psx_retail_exec::{A0, Stop, V0};
use std::cell::Cell;
use std::rc::Rc;

const OVERLAY_BASE: u32 = 0x801C_E818;
const EASE: u32 = 0x801D_B510;
const CLAMP: u32 = 0x801D_AA50;
const FLOOR: u32 = 0x8001_9278;
const SQRT: u32 = 0x8005_B0B8;
const BLOCK: u32 = 0x8007_B600;
const PITCH: u32 = 0x8007_B790;
const YAW: u32 = 0x8007_B792;
const H: u32 = 0x8007_B6F4;
const EYE: u32 = 0x8008_40B8;
const FOCUS_X: u32 = 0x8008_9118;
const FOCUS_Z: u32 = 0x8008_9120;
const HALF_EYE: u32 = 0x8007_B6A8;
const PLAYER_PTR: u32 = 0x8007_C364;
const PARTITIONS_PTR: u32 = 0x8007_B898;
const ATTR_BOX: u32 = 0x1F80_0384;
const ATTR_KIND: u32 = 0x1F80_037C;
const WINDOW: u32 = 0x1F80_03E8;
const SCRIPT_FOCUS: u32 = 0x8007_B628;
const FLAGS: u32 = 0x1F80_0394;
const CONTROL_PTR: u32 = 0x801C_6EA4;
const CONTROL: u32 = 0x8014_0000;
const SHAKE_AMP: u32 = 0x8007_B630;
const PLAYER: u32 = 0x8013_0000;
const PARTITIONS: u32 = 0x8013_8000;

fn s16(rng: &mut Rng, span: u32) -> i32 {
    rng.below(2 * span) as i32 - span as i32
}

/// A parameter block of any mode the loader or a ramp can leave.
fn config(rng: &mut Rng) -> CameraZoneConfig {
    let nibble = |rng: &mut Rng, hi: u32| ((rng.below(hi) << 4) | rng.below(16)) as u8;
    let mode_hi = [0, 1, 2, 3, 4, 5, 7][rng.below(7) as usize];
    let word = |rng: &mut Rng| {
        if rng.below(8) == 0 {
            rng.next() as i32 >> rng.below(16)
        } else {
            s16(rng, 0x8000)
        }
    };
    CameraZoneConfig {
        mode: ((mode_hi << 4) | rng.below(16)) as u8,
        b608: nibble(rng, 4),
        b609: nibble(rng, 7),
        b60a: nibble(rng, 7),
        b60b: nibble(rng, 16),
        pitch: word(rng),
        yaw: word(rng),
        depth: word(rng),
        h: word(rng),
        anchor_x: rng.below(0x80) as i32,
        anchor_h: s16(rng, 0x40),
        anchor_z: rng.below(0x80) as i32,
    }
}

struct Pose {
    pitch: i16,
    yaw: i16,
    h: i16,
    eye: [i32; 3],
    focus: [i32; 2],
}

fn read_pose(cpu: &mut psx_retail_exec::Cpu) -> Pose {
    Pose {
        pitch: cpu.mem.rd16(PITCH) as i16,
        yaw: cpu.mem.rd16(YAW) as i16,
        h: cpu.mem.rd16(H) as i16,
        eye: std::array::from_fn(|i| cpu.mem.rd32(EYE + 4 * i as u32) as i32),
        focus: [cpu.mem.rd32(FOCUS_X) as i32, cpu.mem.rd32(FOCUS_Z) as i32],
    }
}

fn write_pose(cpu: &mut psx_retail_exec::Cpu, p: &Pose) {
    cpu.mem.wr16(PITCH, p.pitch as u16);
    cpu.mem.wr16(YAW, p.yaw as u16);
    cpu.mem.wr16(H, p.h as u16);
    for (i, e) in p.eye.iter().enumerate() {
        cpu.mem.wr32(EYE + 4 * i as u32, *e as u32);
    }
    cpu.mem.wr32(FOCUS_X, p.focus[0] as u32);
    cpu.mem.wr32(FOCUS_Z, p.focus[1] as u32);
}

fn fmt_pose(p: &Pose) -> String {
    format!(
        "pitch {:#x} yaw {:#x} h {:#x} eye {:?} focus {:?}",
        p.pitch, p.yaw, p.h, p.eye, p.focus
    )
}

#[test]
fn the_follow_camera_composes_eases_and_clamps_as_retail() {
    let (Some(mut cpu), Some(d)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem
        .load(OVERLAY_BASE, &d.read_entry(897).expect("PROT 0897"));
    // The sine / cosine views the boot code installs.
    cpu.mem.wr32(0x8007_B81C, 0x8007_0A2C);
    cpu.mem.wr32(0x8007_B7F8, 0x8007_0A2C + 0x800);
    cpu.mem.wr32(PLAYER_PTR, PLAYER);
    cpu.mem.wr32(PARTITIONS_PTR, PARTITIONS);
    cpu.mem.wr32(CONTROL_PTR, CONTROL);
    cpu.mem.wr32(SHAKE_AMP, 0);
    let floor = Rc::new(Cell::new(0i32));
    {
        let floor = floor.clone();
        cpu.hook(FLOOR, move |r, _| r.r[V0] = floor.get() as u32);
    }
    cpu.hook(SQRT, |r, _| r.r[V0] = sqrt0(r.r[A0] as i32) as u32);

    let mut rng = Rng(0x4341_4D46);
    let (mut failures, mut cases, mut traps, mut eased, mut mode_hits) =
        (Vec::new(), 0, 0, 0, [0u32; 8]);
    for case in 0..40_000 {
        let cfg = config(&mut rng);
        cpu.mem.load(BLOCK, &cfg.to_retail_block());
        let enable = rng.below(8) != 0;
        cpu.mem.wr8(BLOCK + 6, u8::from(enable));
        let half_eye = rng.below(3) == 0;
        cpu.mem.wr8(HALF_EYE, u8::from(half_eye));
        let bx0 = rng.below(0x70);
        let bz0 = rng.below(0x70);
        let attr_box = [
            bx0 as u8,
            bz0 as u8,
            (bx0 + rng.below(0x10)) as u8,
            (bz0 + rng.below(0x10)) as u8,
        ];
        for (k, b) in attr_box.iter().enumerate() {
            cpu.mem.wr8(ATTR_BOX + k as u32, *b);
        }
        let px = (u32::from(attr_box[0]) * 0x80
            + rng.below(u32::from(attr_box[2] - attr_box[0] + 1) * 0x80 + 0x100))
            as i32
            - 0x80;
        let pz = (u32::from(attr_box[1]) * 0x80
            + rng.below(u32::from(attr_box[3] - attr_box[1] + 1) * 0x80 + 0x100))
            as i32
            - 0x80;
        let footing = s16(&mut rng, 0x400);
        floor.set(s16(&mut rng, 0x400));
        let moved = rng.below(3) != 0;
        let (ox, oy, oz) = if moved {
            match rng.below(3) {
                0 => (px - 2, footing, pz),
                1 => (px, footing - 4, pz),
                _ => (px, footing, pz + 2),
            }
        } else {
            (px, footing, pz)
        };
        for (o, v) in [
            (0x14, px),
            (0x16, footing),
            (0x18, pz),
            (0x1C, ox),
            (0x1E, oy),
            (0x20, oz),
        ] {
            cpu.mem.wr16(PLAYER + o, v as u16);
        }
        let flags = [0, 0, 0x400, 0x4_0000][rng.below(4) as usize];
        cpu.mem.wr32(FLAGS, flags);
        cpu.mem.wr32(CONTROL + 0x18, 0);
        cpu.mem.wr32(CONTROL + 0x1C, 0);
        let start = Pose {
            pitch: s16(&mut rng, 0x2000) as i16,
            yaw: s16(&mut rng, 0x2000) as i16,
            h: rng.below(0x400) as i16,
            eye: [
                s16(&mut rng, 0x400),
                s16(&mut rng, 0x800),
                s16(&mut rng, 0x8000),
            ],
            focus: [-px + s16(&mut rng, 0x200), -pz + s16(&mut rng, 0x200)],
        };
        write_pose(&mut cpu, &start);
        let kind = rng.below(4) as u8;
        cpu.mem.wr8(ATTR_KIND, kind);
        let window: [i8; 4] = std::array::from_fn(|_| s16(&mut rng, 8) as i8);
        for (k, w) in window.iter().enumerate() {
            cpu.mem.wr8(WINDOW + k as u32, *w as u8);
        }
        let script_focus: [i16; 2] = std::array::from_fn(|_| {
            if rng.below(6) == 0 {
                s16(&mut rng, 0x2000) as i16
            } else {
                0
            }
        });
        cpu.mem.wr16(SCRIPT_FOCUS, script_focus[0] as u16);
        cpu.mem.wr16(SCRIPT_FOCUS + 2, script_focus[1] as u16);

        // Retail: the ease (which composes), then the clamp.
        let stop = cpu.call(EASE, &[PLAYER]);
        if let Stop::DivideByZero { .. } = stop {
            traps += 1;
            continue;
        }
        assert_eq!(stop, Stop::Returned, "case {case}");
        assert_eq!(cpu.call(CLAMP, &[]), Stop::Returned, "case {case}");
        let retail = read_pose(&mut cpu);
        cases += 1;

        // Port: the same control flow over the ported kernels.
        let mut port = Pose { ..start };
        let hold = !enable || flags & 0x400 != 0;
        let mode = cfg.mode >> 4;
        if hold {
            port.focus = [-px, -pz];
        } else if moved || flags & 0x4_0000 != 0 {
            eased += 1;
            mode_hits[usize::from(mode)] += 1;
            let composed = compose(
                &cfg,
                &ComposeInputs {
                    player: [px, footing, pz],
                    floor_y: floor.get(),
                    attr_box,
                    live_pitch: i32::from(start.pitch),
                    live_yaw: i32::from(start.yaw),
                    half_eye_y: half_eye,
                },
            );
            if let Some(ly) = composed.live_yaw {
                port.yaw = ly;
            }
            let t = composed.target;
            let code = EASE_SHIFT_TABLE[usize::from(cfg.b60b >> 4)];
            port.pitch = ease_step_i16(port.pitch, t.pitch, code);
            port.yaw = ease_step_i16(port.yaw, t.yaw, code);
            for (axis, e) in t.eye.iter().enumerate() {
                port.eye[axis] = ease_step(port.eye[axis], i32::from(*e), code);
            }
            port.h = ease_step_i16(port.h, t.h, code);
            if mode == 5 {
                port.focus[0] = ease_step(port.focus[0], -(cfg.anchor_x << 7) - 0x40, code);
                port.focus[1] = ease_step(port.focus[1], -(cfg.anchor_z << 7) - 0x40, code);
            } else {
                port.focus = [-px, -pz];
            }
        }
        port.focus = clamp_focus(
            port.focus,
            mode,
            kind != 0,
            attr_box,
            window,
            half_eye,
            script_focus,
        );
        let same = (retail.pitch, retail.yaw, retail.h, retail.eye, retail.focus)
            == (port.pitch, port.yaw, port.h, port.eye, port.focus);
        if !same && failures.len() < 12 {
            failures.push(format!(
                "case {case} mode {:#04x} b608 {:#04x} b609 {:#04x} b60a {:#04x} b60b {:#04x} \
                 moved {moved} flags {flags:#x} enable {enable}\n  retail {}\n  port   {}",
                cfg.mode,
                cfg.b608,
                cfg.b609,
                cfg.b60a,
                cfg.b60b,
                fmt_pose(&retail),
                fmt_pose(&port)
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    eprintln!("{cases} cases, {eased} eased, {traps} divide traps, per mode {mode_hits:?}");
    assert!(cases > 30_000 && eased > 15_000, "{cases} {eased}");
    assert!(
        mode_hits
            .iter()
            .enumerate()
            .all(|(m, &n)| m == 6 || n > 500),
        "{mode_hits:?}"
    );
}
