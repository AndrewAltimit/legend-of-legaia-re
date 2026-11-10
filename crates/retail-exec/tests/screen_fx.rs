//! The PROT 0900 screen-effect widgets off the disc, with the field overlay
//! (PROT 0897) resident under them for the shared interpolator: the
//! interpolator `FUN_801DE4C8` against `screen_fx::interp` over every mode;
//! the screen mask's control API `FUN_801F8D4C` (fresh and live widget, the
//! `-1` defaults) and its per-frame handler `FUN_801F811C` against
//! `MaskWidget`, frame by frame, the four border packets included; and the
//! letterbox's config `FUN_801F8F28` and draw `FUN_801F8A34` against
//! `Letterbox`, its two bands and two subtractive feather strips; and the
//! image panel's spawn `FUN_801F88FC`, move / scale `FUN_801F8E6C` and
//! handler `FUN_801F849C` against `PanelWidget`, one or two textured quads a
//! frame across the 256-pixel page split; and the scripted sprite's spawn
//! `FUN_801F8004` and handler `FUN_801F7A9C` against `SpriteWidget` over
//! generated widget scripts (kill, both flag waits, both tweens, parked and
//! out-of-range records). The actor
//! finder and allocator are answered with one cleared record.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_effects::screen_fx::{
    InterpMode, Letterbox, MaskWidget, PanelQuad, PanelWidget, Screen, SpriteRecord, SpriteWidget,
    interp,
};
use psx_retail_exec::{Cpu, Stop, V0};

const INTERP: u32 = 0x801D_E4C8;
const MASK_SET: u32 = 0x801F_8D4C;
const MASK_TICK: u32 = 0x801F_811C;
const BOX_SET: u32 = 0x801F_8F28;
const PANEL_SPAWN: u32 = 0x801F_88FC;
const PANEL_MOVE: u32 = 0x801F_8E6C;
const PANEL_TICK: u32 = 0x801F_849C;
const SPRITE_SPAWN: u32 = 0x801F_8004;
const SPRITE_TICK: u32 = 0x801F_7A9C;
const FLAG_TEST: u32 = 0x8003_CE64;
const BOX_DRAW: u32 = 0x801F_8A34;
const FIND: u32 = 0x8003_CF04;
const ALLOC: u32 = 0x8002_0DE0;
const FIELD_BASE: u32 = 0x801C_E818;
const SLOT_B: u32 = 0x801F_69D8;
const ACTOR: u32 = 0x8013_0000;
const BLOCK: u32 = 0x8013_1000;
const STACK_TOP: u32 = 0x801F_FF00;
const DT: u32 = 0x1F80_0393;
const PRIM_PTR: u32 = 0x1F80_03A0;
const PRIMS: u32 = 0x8018_0000;
const OT_PTR: u32 = 0x1F80_03F4;
const OT: u32 = 0x8017_0000;
const SCREEN_X0: u32 = 0x1F80_0388;
const SCREEN_Y_OFF: u32 = 0x1F80_038A;
const SCREEN_H: u32 = 0x1F80_038E;

fn retail_cpu() -> Option<Cpu> {
    let mut cpu = scus_cpu()?;
    let d = disc()?;
    cpu.mem
        .load(FIELD_BASE, &d.read_entry(897).expect("PROT 0897"));
    cpu.mem.load(SLOT_B, &d.read_entry(900).expect("PROT 0900"));
    cpu.stub(ALLOC, ACTOR);
    cpu.mem.wr32(OT_PTR, OT);
    Some(cpu)
}

fn seat_screen(cpu: &mut Cpu, s: &Screen) {
    cpu.mem.wr16(SCREEN_X0, s.x0 as u16);
    cpu.mem.wr16(SCREEN_Y_OFF, s.y_off as u16);
    cpu.mem.wr16(SCREEN_H, s.height as u16);
}

/// A flat quad packet's four corners (`+8`, `+0xC`, `+0x10`, `+0x14`).
fn f4(cpu: &mut Cpu, at: u32) -> [(i16, i16); 4] {
    std::array::from_fn(|k| {
        let w = cpu.mem.rd32(at + 8 + 4 * k as u32);
        (w as i16, (w >> 16) as i16)
    })
}

fn corners(l: i16, t: i16, r: i16, b: i16) -> [(i16, i16); 4] {
    [(l, t), (r, t), (l, b), (r, b)]
}

#[test]
fn the_widget_interpolator_matches_retail() {
    differential!(prot(897, INTERP));
    let Some(mut cpu) = retail_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut rng = Rng(0x494E_5450);
    for round in 0..200_000 {
        let wide = round % 4 == 0;
        let v = |rng: &mut Rng| {
            if wide {
                rng.next() as i16 as i32
            } else {
                rng.below(0x200) as i32 - 0x100
            }
        };
        let (target, start) = (v(&mut rng), v(&mut rng));
        let mode = rng.below(6) as u8;
        // Durations the scripts write; the ease-in-out arm halves its
        // duration and divides by the half, so it needs two frames or more.
        let dur = if mode == 4 {
            2 + rng.below(0x200) as i32
        } else {
            1 + rng.below(0x200) as i32
        };
        let t = rng.below(dur as u32 + 4) as i32;
        cpu.mem.wr32(STACK_TOP + 0x10, u32::from(mode));
        assert_eq!(
            cpu.call(INTERP, &[target as u32, start as u32, t as u32, dur as u32]),
            Stop::Returned
        );
        let want = interp(target, start, t, dur, InterpMode::from_byte(mode));
        assert_eq!(
            cpu.regs.r[V0] as i32, want,
            "round {round}: interp({target}, {start}, {t}, {dur}, mode {mode})"
        );
    }
}

#[test]
fn the_screen_mask_matches_retail() {
    differential!(prot(900, MASK_SET), prot(900, MASK_TICK));
    let Some(mut cpu) = retail_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut rng = Rng(0x4D41_534B);
    let mut moved = 0;
    for run in 0..400 {
        let screen = Screen {
            x0: [0i16, 0, 8][rng.below(3) as usize],
            y_off: [4i16, 0][rng.below(2) as usize],
            height: [228i16, 240, 224][rng.below(3) as usize],
        };
        seat_screen(&mut cpu, &screen);
        cpu.mem.load(ACTOR, &[0u8; 0x100]);
        let mut port: Option<MaskWidget> = None;
        for call in 0..6 {
            // The control API: a fresh widget the first time, the live one after.
            let live = port.is_some();
            cpu.stub(FIND, if live { ACTOR } else { 0 });
            let edge = |rng: &mut Rng, open: i16| {
                if rng.below(4) == 0 {
                    -1
                } else {
                    open + rng.below(0x40) as i16 - 0x20
                }
            };
            let (l, t, r, b) = (
                edge(&mut rng, 0x30),
                edge(&mut rng, 0x30),
                edge(&mut rng, 0x110),
                edge(&mut rng, 0xB0),
            );
            let dur = [0i16, 1, 8, 30, 0x40][rng.below(5) as usize];
            cpu.mem.wr32(STACK_TOP + 0x10, dur as i32 as u32);
            let args = [l, t, r, b].map(|v| v as i32 as u32);
            assert_eq!(cpu.call(MASK_SET, &args), Stop::Returned);
            let w = port.get_or_insert_with(|| MaskWidget::spawn(&screen));
            w.set_rect(l, t, r, b, dur, &screen);

            for frame in 0..40 {
                let label = format!("run {run} call {call} frame {frame}");
                let dt = 1 + rng.below(3) as u8;
                cpu.mem.wr8(DT, dt);
                cpu.mem.wr32(PRIM_PTR, PRIMS);
                assert_eq!(cpu.call(MASK_TICK, &[ACTOR]), Stop::Returned, "{label}");
                let quads = w.tick(dt, &screen);
                let rd = |cpu: &mut Cpu, o: u32| cpu.mem.rd16(ACTOR + o) as i16;
                let state = (
                    [0x14, 0x16, 0x18, 0x1A].map(|o| rd(&mut cpu, o)),
                    [0x3C, 0x3E, 0x40, 0x42].map(|o| rd(&mut cpu, o)),
                    rd(&mut cpu, 0x9C),
                    rd(&mut cpu, 0x9E),
                );
                assert_eq!(state, (w.cur, w.target, w.t, w.dur), "{label}: record");
                assert_eq!(cpu.mem.rd32(PRIM_PTR), PRIMS + 4 * 0x18, "{label}");
                for (k, q) in quads.iter().enumerate() {
                    let at = PRIMS + 0x18 * k as u32;
                    assert_eq!(cpu.mem.rd32(at + 4), 0x2800_0000, "{label} quad {k}");
                    assert_eq!(
                        f4(&mut cpu, at),
                        corners(q.left, q.top, q.right, q.bottom),
                        "{label} quad {k}"
                    );
                }
                moved += usize::from(w.dur != 0);
            }
        }
    }
    assert!(moved > 10_000, "{moved}");
}

#[test]
fn the_letterbox_matches_retail() {
    differential!(prot(900, BOX_SET), prot(900, BOX_DRAW));
    let Some(mut cpu) = retail_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut rng = Rng(0x4C42_4F58);
    for round in 0..20_000 {
        let screen = Screen {
            x0: 0,
            y_off: [4i16, 0][rng.below(2) as usize],
            height: [228i16, 240][rng.below(2) as usize],
        };
        seat_screen(&mut cpu, &screen);
        let block: Vec<u8> = (0..12).map(|_| rng.next() as u8).collect();
        cpu.mem.load(BLOCK, &block);
        cpu.mem.load(ACTOR, &[0u8; 0x100]);
        cpu.stub(FIND, if rng.below(2) == 0 { ACTOR } else { 0 });
        assert_eq!(cpu.call(BOX_SET, &[BLOCK]), Stop::Returned);
        assert_eq!(cpu.regs.r[V0], ACTOR);
        let lb = Letterbox::from_config(&block).unwrap();
        cpu.mem.wr32(PRIM_PTR, PRIMS);
        assert_eq!(cpu.call(BOX_DRAW, &[ACTOR]), Stop::Returned);
        let label = format!("round {round} {lb:?}");
        // Two flat bands, then two Gouraud strips, then the draw mode.
        let bands = lb.solid_bands(&screen);
        for (k, q) in bands.iter().enumerate() {
            let at = PRIMS + 0x18 * k as u32;
            assert_eq!(cpu.mem.rd32(at + 4), 0x2800_0000, "{label} band {k}");
            assert_eq!(
                f4(&mut cpu, at),
                corners(q.left, q.top, q.right, q.bottom),
                "{label} band {k}"
            );
        }
        for (k, (q, top_white)) in lb.gradient_bands().iter().enumerate() {
            let at = PRIMS + 0x30 + 0x24 * k as u32;
            let xy: [(i16, i16); 4] = std::array::from_fn(|v| {
                let w = cpu.mem.rd32(at + 8 + 8 * v as u32);
                (w as i16, (w >> 16) as i16)
            });
            assert_eq!(
                xy,
                corners(q.left, q.top, q.right, q.bottom),
                "{label} strip {k}"
            );
            let rgb: [u32; 4] =
                std::array::from_fn(|v| cpu.mem.rd32(at + 4 + 8 * v as u32) & 0x00FF_FFFF);
            let (top, bottom) = if *top_white {
                (0xFF_FFFF, 0)
            } else {
                (0, 0xFF_FFFF)
            };
            assert_eq!(rgb, [top, top, bottom, bottom], "{label} strip {k}");
            assert_eq!(cpu.mem.rd32(at + 4) >> 24, 0x3B, "{label} strip {k}");
        }
        assert_eq!(cpu.mem.rd32(PRIM_PTR), PRIMS + 0x30 + 0x48 + 0xC, "{label}");
    }
}

/// A textured quad packet (`POLY_FT4`): corners at `+8 + 8k`, texels at
/// `+0xC + 8k`, the page at `+0x16`.
fn ft4(cpu: &mut Cpu, at: u32) -> PanelQuad {
    let xy = |cpu: &mut Cpu, k: u32| {
        let w = cpu.mem.rd32(at + 8 + 8 * k);
        (w as i16, (w >> 16) as i16)
    };
    let (lt, rb) = (xy(cpu, 0), xy(cpu, 3));
    assert_eq!((xy(cpu, 1), xy(cpu, 2)), ((rb.0, lt.1), (lt.0, rb.1)));
    let uv = |cpu: &mut Cpu, k: u32| (cpu.mem.rd8(at + 0xC + 8 * k), cpu.mem.rd8(at + 0xD + 8 * k));
    let (uv0, uv3) = (uv(cpu, 0), uv(cpu, 3));
    assert_eq!((uv(cpu, 1), uv(cpu, 2)), ((uv3.0, uv0.1), (uv0.0, uv3.1)));
    PanelQuad {
        left: lt.0,
        top: lt.1,
        right: rb.0,
        bottom: rb.1,
        u0: uv0.0,
        v0: uv0.1,
        u1: uv3.0,
        v1: uv3.1,
        texpage: cpu.mem.rd16(at + 0x16),
    }
}

#[test]
fn the_image_panel_matches_retail() {
    differential!(
        prot(900, PANEL_SPAWN),
        prot(900, PANEL_MOVE),
        prot(900, PANEL_TICK)
    );
    let Some(mut cpu) = retail_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.stub(FIND, ACTOR);
    let mut rng = Rng(0x5041_4E4C);
    let (mut split, mut still) = (0, 0);
    for run in 0..2000 {
        let screen = Screen {
            x0: 0,
            y_off: [4i16, 0][rng.below(2) as usize],
            height: 228,
        };
        seat_screen(&mut cpu, &screen);
        // `[op][x][y][w][h][tex_x][tex_y]`, halfwords from `+1`.
        let w = [0x40i16, 0x100, 0x101, 0x140, 0x180][rng.below(5) as usize];
        let fields = [
            rng.below(0x100) as i16 - 0x40,
            rng.below(0xC0) as i16,
            w,
            [0x40i16, 0x78, 0xF0][rng.below(3) as usize],
            (rng.below(16) * 64 + rng.below(64)) as i16,
            rng.below(0x200) as i16,
        ];
        let mut rec = vec![0x13u8];
        for f in fields {
            rec.extend_from_slice(&f.to_le_bytes());
        }
        cpu.mem.load(BLOCK, &rec);
        cpu.mem.load(ACTOR, &[0u8; 0x100]);
        assert_eq!(cpu.call(PANEL_SPAWN, &[BLOCK]), Stop::Returned);
        assert_eq!(cpu.regs.r[V0], ACTOR);
        let mut p = PanelWidget::spawn(&rec).unwrap();
        for call in 0..4 {
            if call > 0 {
                let (x, y) = (
                    rng.below(0x140) as i16 - 0x40,
                    rng.below(0xF0) as i16 - 0x20,
                );
                let scale =
                    [0x1000i32, 0x800, 0x1800, rng.below(0x3000) as i32][rng.below(4) as usize];
                let dur = [0i16, 1, 10, 30][rng.below(4) as usize];
                let args = [
                    x as i32 as u32,
                    y as i32 as u32,
                    scale as u32,
                    dur as i32 as u32,
                ];
                assert_eq!(cpu.call(PANEL_MOVE, &args), Stop::Returned);
                p.move_scale(x, y, scale, dur);
            }
            for frame in 0..36 {
                let label = format!("run {run} call {call} frame {frame}");
                let dt = 1 + rng.below(3) as u8;
                cpu.mem.wr8(DT, dt);
                cpu.mem.wr32(PRIM_PTR, PRIMS);
                assert_eq!(cpu.call(PANEL_TICK, &[ACTOR]), Stop::Returned, "{label}");
                let (q0, q1) = p.tick(dt, &screen);
                let rd = |cpu: &mut Cpu, o: u32| cpu.mem.rd16(ACTOR + o) as i16;
                let state = (
                    [0x14, 0x16, 0x18, 0x1A, 0x24].map(|o| rd(&mut cpu, o)),
                    [0x3C, 0x3E, 0x40, 0x42, 0x26].map(|o| rd(&mut cpu, o)),
                    [0xB8, 0xBA, 0xBC].map(|o| rd(&mut cpu, o)),
                    rd(&mut cpu, 0x9C),
                    rd(&mut cpu, 0x9E),
                );
                assert_eq!(
                    state,
                    (p.cur, p.target, p.base, p.t, p.dur),
                    "{label}: record"
                );
                assert_eq!(ft4(&mut cpu, PRIMS), q0, "{label}: page 0");
                assert_eq!(cpu.mem.rd32(PRIMS + 4), 0x2C88_8888, "{label}");
                let drawn = (cpu.mem.rd32(PRIM_PTR) - PRIMS) / 0x28;
                assert_eq!(drawn, 1 + u32::from(q1.is_some()), "{label}: packets");
                if let Some(q1) = q1 {
                    assert_eq!(ft4(&mut cpu, PRIMS + 0x28), q1, "{label}: page 1");
                    split += 1;
                }
                still += usize::from(p.dur == 0 && p.cur != p.target);
            }
        }
    }
    assert!(
        split > 10_000 && still > 1000,
        "split {split} still {still}"
    );
}

/// The story-flag probe both sides answer alike: set or clear by flag and
/// frame.
fn flag_set(flag: u16, frame: u32) -> bool {
    (u32::from(flag).wrapping_mul(0x9E37_79B9) ^ frame.wrapping_mul(0x85EB_CA6B)) >> 29 < 3
}

/// A generated widget script: `0x40` records of every sub-op, now and then a
/// record whose first byte is not `0x40` or whose sub-op is out of range.
fn widget_script(rng: &mut Rng) -> Vec<u8> {
    let mut out = Vec::new();
    for _ in 0..1 + rng.below(8) {
        let op = if rng.below(12) == 0 {
            rng.next() as u8
        } else {
            0x40
        };
        let sub = match rng.below(16) {
            0 => 0u8,
            1 => 5 + rng.below(4) as u8,
            2..=4 => 1,
            5..=7 => 2,
            8..=12 => 3,
            _ => 4,
        };
        out.extend_from_slice(&[op, rng.next() as u8, sub]);
        let dur = |rng: &mut Rng| [0i16, 1, 4, 20, 0x30, -3][rng.below(6) as usize].to_le_bytes();
        match sub {
            1 | 2 => out.extend_from_slice(&(rng.below(0x40) as u16).to_le_bytes()),
            3 => {
                out.extend_from_slice(&(rng.below(0x200) as i16 - 0x80).to_le_bytes());
                out.extend_from_slice(&(rng.below(0x100) as i16 - 0x20).to_le_bytes());
                out.extend((0..3).map(|_| rng.next() as u8));
                out.push(rng.below(6) as u8);
                out.extend_from_slice(&dur(rng));
            }
            4 => {
                out.extend((0..3).map(|_| rng.next() as u8));
                out.push(rng.below(6) as u8);
                out.extend_from_slice(&dur(rng));
            }
            _ => {}
        }
    }
    out.extend_from_slice(&[0; 16]);
    out
}

#[test]
fn the_scripted_sprite_matches_retail() {
    differential!(prot(900, SPRITE_SPAWN), prot(900, SPRITE_TICK));
    let Some(mut cpu) = retail_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let frame_no = std::rc::Rc::new(std::cell::Cell::new(0u32));
    let f = frame_no.clone();
    cpu.hook(FLAG_TEST, move |r, _| {
        r.r[V0] = u32::from(flag_set(r.r[psx_retail_exec::A0] as u16, f.get()))
    });
    let mut rng = Rng(0x5350_5254);
    let (mut killed, mut tweens, mut drawn) = (0, 0, 0);
    for run in 0..3000 {
        let mut rec = Vec::new();
        for v in [
            rng.below(0x140) as i16,
            rng.below(0xE0) as i16,
            [16i16, 32, 64][rng.below(3) as usize],
            [16i16, 32, 48][rng.below(3) as usize],
            (rng.below(16) * 64 + rng.below(64)) as i16,
            rng.below(0x200) as i16,
            (rng.below(64) * 16) as i16,
            (0x1E0 + rng.below(0x20)) as i16,
        ] {
            rec.extend_from_slice(&v.to_le_bytes());
        }
        rec.extend((0..3).map(|_| rng.next() as u8));
        rec.extend(widget_script(&mut rng));
        cpu.mem.load(BLOCK, &rec);
        cpu.mem.load(ACTOR, &[0u8; 0x100]);
        assert_eq!(cpu.call(SPRITE_SPAWN, &[BLOCK]), Stop::Returned);
        assert_eq!(cpu.regs.r[V0], ACTOR);
        let mut w = SpriteWidget::spawn(SpriteRecord::parse(&rec).unwrap());
        for frame in 0..60u32 {
            let label = format!("run {run} frame {frame}");
            frame_no.set(frame);
            let dt = 1 + rng.below(3) as u8;
            cpu.mem.wr8(DT, dt);
            cpu.mem.wr32(PRIM_PTR, PRIMS);
            assert_eq!(cpu.call(SPRITE_TICK, &[ACTOR]), Stop::Returned, "{label}");
            let draw = w.tick(&rec, dt, |flag| flag_set(flag, frame));
            let rgb = cpu.mem.rd32(ACTOR + 0x74);
            let state = (
                [0x14, 0x16, 0x3C, 0x3E].map(|o| cpu.mem.rd16(ACTOR + o) as i16),
                [rgb as u8, (rgb >> 8) as u8, (rgb >> 16) as u8],
                cpu.mem.rd16(ACTOR + 0x9C) as i16,
                cpu.mem.rd32(ACTOR + 0x90) - BLOCK,
                cpu.mem.rd32(ACTOR + 0x10) & 8 != 0,
            );
            assert_eq!(
                state,
                (
                    [w.x, w.y, w.start_x, w.start_y],
                    w.rgb,
                    w.t,
                    w.cursor as u32,
                    w.killed
                ),
                "{label}: record"
            );
            let start = cpu.mem.rd32(ACTOR + 0x7C);
            assert_eq!(
                [start as u8, (start >> 8) as u8, (start >> 16) as u8],
                w.start_rgb,
                "{label}: tween start colour"
            );
            let written = cpu.mem.rd32(PRIM_PTR) - PRIMS;
            match draw {
                None => assert_eq!(written, 0, "{label}: killed draws nothing"),
                Some(d) => {
                    assert_eq!(written, 0x14 + 0xC, "{label}: packets");
                    let word = cpu.mem.rd32(PRIMS + 4);
                    assert_eq!(word >> 24, 0x64, "{label}");
                    assert_eq!(
                        [word as u8, (word >> 8) as u8, (word >> 16) as u8],
                        d.rgb,
                        "{label}: colour"
                    );
                    let got = (
                        cpu.mem.rd16(PRIMS + 8) as i16,
                        cpu.mem.rd16(PRIMS + 0xA) as i16,
                        cpu.mem.rd8(PRIMS + 0xC),
                        cpu.mem.rd8(PRIMS + 0xD),
                        cpu.mem.rd16(PRIMS + 0xE) as i16,
                        cpu.mem.rd16(PRIMS + 0x10) as i16,
                        cpu.mem.rd16(PRIMS + 0x12) as i16,
                    );
                    assert_eq!(
                        got,
                        (d.x, d.y, d.u, d.v, d.clut, d.w, d.h),
                        "{label}: sprite"
                    );
                    let mode = cpu.mem.rd32(PRIMS + 0x14 + 4);
                    assert_eq!(mode >> 24, 0xE1, "{label}: draw mode");
                    assert_eq!(mode & 0x1FF, d.texpage as u32 & 0x1FF, "{label}: page");
                    drawn += 1;
                }
            }
            tweens += usize::from(w.t != 0);
        }
        killed += usize::from(w.killed);
    }
    assert!(
        killed > 300 && tweens > 10_000 && drawn > 50_000,
        "killed {killed} tweens {tweens} drawn {drawn}"
    );
}
