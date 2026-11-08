//! The two full-screen colour ramps an actor pushes to `FUN_80024EE4`: the
//! field overlay's lerp tween `FUN_801DDC20` (PROT 0897), against
//! `step_colour_tween`, and the SCUS fade actor's accumulator, the tick
//! `FUN_80025000` over its step `FUN_80020C14`, against `tick_fade_ramp`.
//! Generated actors at every phase - ahead of the delay, inside the ramp,
//! held, expiring, held for ever - with negative delays, out-of-range
//! channels, every frame step and clocks near the halfword ends. The push is
//! read off a hook on the emitter. The last test loads the block the way the
//! spawn does, `FUN_80020B00` from a template, and plays the fade to its end
//! and, for some, some 40000 vsyncs past it, against `FadeState`.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_field::actor_colour_tween::{ColourTween, step_colour_tween};
use legaia_engine_system::fade::{FadeState, FadeTemplate};
use legaia_engine_system::fade_ramp::{FadeRamp, tick_fade_ramp};
use psx_retail_exec::{A0, A1, A2, Stop};
use std::cell::RefCell;
use std::rc::Rc;

const TWEEN: u32 = 0x801D_DC20;
const FADE_TICK: u32 = 0x8002_5000;
const FADE_LOAD: u32 = 0x8002_0B00;
const TEMPLATE: u32 = 0x8013_1000;
const EMITTER: u32 = 0x8002_4EE4;
const DELTA: u32 = 0x1F80_0393;
const ACTOR: u32 = 0x8013_0000;

fn s16(rng: &mut Rng) -> i16 {
    match rng.below(4) {
        0 => rng.next() as i16,
        1 => rng.below(0x200) as i16 - 0x100,
        _ => rng.below(0x40) as i16,
    }
}

fn emitter(cpu: &mut psx_retail_exec::Cpu) -> Rc<RefCell<Vec<(i16, i32, u32)>>> {
    let pushes = Rc::new(RefCell::new(Vec::new()));
    let p = pushes.clone();
    cpu.hook(EMITTER, move |r, _| {
        p.borrow_mut()
            .push((r.r[A0] as i16, r.r[A1] as i32, r.r[A2]))
    });
    pushes
}

#[test]
fn the_overlay_tween_lerps_holds_and_retires_as_retail() {
    let (Some(mut cpu), Some(d)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem
        .load(0x801C_E818, &d.read_entry(897).expect("PROT 0897"));
    let pushes = emitter(&mut cpu);
    let mut rng = Rng(0x5457_454E);
    let (mut failures, mut ramps, mut retires) = (Vec::new(), 0, 0);
    for case in 0..200_000 {
        let mut c = || rng.below(0x100) as u16;
        let mut t = ColourTween {
            from: (c(), c(), c()),
            to: (c(), c(), c()),
            delay: s16(&mut rng),
            duration: s16(&mut rng),
            hold: if rng.below(4) == 0 { -1 } else { s16(&mut rng) },
            clock: rng.next() as u16,
            flags: if rng.below(8) == 0 { 8 } else { 0x10 },
            push_kind: s16(&mut rng),
            push_blend: rng.below(4) as i16,
        };
        // Half the clocks land in or just around the ramp.
        if rng.below(2) == 0 {
            let span = u32::from(t.duration.unsigned_abs()) + 4;
            t.clock = (t.delay as u16)
                .wrapping_add(rng.below(span) as u16)
                .wrapping_sub(2);
        }
        let (delay, dur) = (i32::from(t.delay), i32::from(t.duration));
        let clock = i32::from(t.clock as i16);
        // The ramp arm divides by the duration with a `break` guard.
        if delay < clock && clock < delay + dur && dur == 0 {
            continue;
        }
        let delta = 1 + rng.below(4) as u8;
        cpu.mem.wr8(DELTA, delta);
        cpu.mem.load(ACTOR, &[0u8; 0x100]);
        cpu.mem.wr32(ACTOR + 0x10, t.flags);
        for (o, v) in [
            (0xB8, t.from.0),
            (0xBA, t.from.1),
            (0xBC, t.from.2),
            (0xBE, t.to.0),
            (0xC0, t.to.1),
            (0xC2, t.to.2),
            (0xC4, t.delay as u16),
            (0xC6, t.hold as u16),
            (0xC8, t.clock),
            (0xD2, t.push_blend as u16),
            (0xD4, t.duration as u16),
            (0xD6, t.push_kind as u16),
        ] {
            cpu.mem.wr16(ACTOR + o, v);
        }
        pushes.borrow_mut().clear();
        assert_eq!(cpu.call(TWEEN, &[ACTOR]), Stop::Returned, "case {case}");
        let retail = (
            cpu.mem.rd16(ACTOR + 0xC8),
            cpu.mem.rd32(ACTOR + 0x10),
            pushes.borrow().clone(),
        );
        let s = step_colour_tween(t, delta);
        let port = (
            s.clock,
            s.flags,
            s.push
                .map(|p| (p.kind, i32::from(p.blend), p.packed))
                .into_iter()
                .collect::<Vec<_>>(),
        );
        ramps += usize::from(delay < clock && clock < delay + dur);
        retires += usize::from(retail.1 & 8 != 0 && t.flags & 8 == 0);
        if retail != port && failures.len() < 12 {
            failures.push(format!(
                "case {case} {t:?} delta {delta}: retail {retail:x?} port {port:x?}"
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    eprintln!("{ramps} ramp frames, {retires} retires");
    assert!(ramps > 30_000 && retires > 1_000, "{ramps} {retires}");
}

#[test]
fn the_fade_actor_accumulates_clamps_and_finishes_as_retail() {
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let pushes = emitter(&mut cpu);
    let mut rng = Rng(0x4641_4445);
    let (mut failures, mut finished, mut drawn) = (Vec::new(), 0, 0);
    for case in 0..200_000 {
        let q6 = |rng: &mut Rng| match rng.below(4) {
            0 => rng.next() as i16,
            _ => rng.below(0x3FC1) as i16,
        };
        let mut ramp = FadeRamp::new(
            [q6(&mut rng), q6(&mut rng), q6(&mut rng)],
            [q6(&mut rng), q6(&mut rng), q6(&mut rng)],
            std::array::from_fn(|_| rng.below(0x800) as i16 - 0x400),
            rng.below(4) as i32,
            s16(&mut rng),
            if rng.below(4) == 0 { -1 } else { s16(&mut rng) },
            s16(&mut rng),
            s16(&mut rng),
        );
        let dt = 1 + rng.below(4) as u8;
        cpu.mem.wr8(DELTA, dt);
        cpu.mem.load(ACTOR, &[0u8; 0x100]);
        cpu.mem.wr32(ACTOR + 0x10, 0x10);
        let b = ACTOR + 0x7C;
        for c in 0..3u32 {
            cpu.mem.wr16(b + 2 * c, ramp.current_q6[c as usize] as u16);
            cpu.mem
                .wr16(b + 8 + 2 * c, ramp.target_q6[c as usize] as u16);
            cpu.mem
                .wr16(b + 0x10 + 2 * c, ramp.delta_q6[c as usize] as u16);
        }
        cpu.mem.wr32(b + 0x18, ramp.kind as u32);
        cpu.mem.wr16(b + 0x1C, ramp.delay as u16);
        cpu.mem.wr16(b + 0x1E, ramp.hold as u16);
        cpu.mem.wr16(b + 0x20, ramp.duration as u16);
        cpu.mem.wr16(b + 0x22, ramp.id as u16);
        pushes.borrow_mut().clear();
        assert_eq!(cpu.call(FADE_TICK, &[ACTOR]), Stop::Returned, "case {case}");
        let block = |cpu: &mut psx_retail_exec::Cpu, o: u32| cpu.mem.rd16(b + o) as i16;
        let retail = (
            [block(&mut cpu, 0), block(&mut cpu, 2), block(&mut cpu, 4)],
            block(&mut cpu, 0x1C),
            block(&mut cpu, 0x1E),
            block(&mut cpu, 0x20),
            cpu.mem.rd16(ACTOR + 0x62) & 0x100 != 0,
            cpu.mem.rd32(ACTOR + 0x10) & 8 != 0,
            pushes.borrow().clone(),
        );
        let quad = tick_fade_ramp(&mut ramp, dt);
        let port = (
            ramp.current_q6,
            ramp.delay,
            ramp.hold,
            ramp.duration,
            ramp.flags.ramped,
            ramp.flags.finished,
            quad.map(|q| (q.id, q.kind, q.rgb))
                .into_iter()
                .collect::<Vec<_>>(),
        );
        finished += usize::from(retail.5);
        drawn += retail.6.len();
        if retail != port && failures.len() < 12 {
            failures.push(format!(
                "case {case} dt {dt}: retail {retail:x?} port {port:x?}"
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    eprintln!("{drawn} drawn, {finished} finished");
    assert!(drawn > 50_000 && finished > 200, "{drawn} {finished}");
}

/// A fade loaded from its template and played frame by frame: what the quad
/// shows each vsync, against `FadeState::load` and `step_vsyncs`, which keep
/// the ramp as `elapsed / duration` and hand `FUN_80020C14`'s block to the
/// tick each frame. Some fades run about 40000 vsyncs past the ramp, where
/// retail's halfword duration count wraps positive and the port's elapsed
/// count stays at its end. A zero duration divides by zero in the loader
/// (`div` at `0x80020B84`, no `break` guard): the R3000 leaves `-1` in `lo`
/// for a non-negative difference and `1` for a negative one, a delta whose
/// sign always says "overshot", so the first tick lands on the target.
#[test]
fn a_loaded_fade_shows_what_retail_shows_to_its_end_and_past_it() {
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.hardware_zero_div = true;
    let pushes = emitter(&mut cpu);
    let mut rng = Rng(0x4C4F_4144);
    let (mut failures, mut frames, mut long, mut zero) = (Vec::new(), 0u64, 0, 0);
    for case in 0..3_000 {
        let rgb = |rng: &mut Rng| std::array::from_fn(|_| rng.below(0x100) as i16);
        let duration = match rng.below(9) {
            0 => 0,
            1 => 1,
            2 => -(rng.below(0x40) as i16) - 1,
            _ => 1 + rng.below(0x100) as i16,
        };
        let t = FadeTemplate {
            kind: rng.below(4) as i16,
            duration,
            start_rgb: rgb(&mut rng),
            end_rgb: rgb(&mut rng),
            mode: [
                rng.below(0x10) as i16,
                match rng.below(3) {
                    0 => -1,
                    1 => rng.below(4) as i16,
                    _ => rng.below(0x200) as i16,
                },
                rng.below(8) as i16,
            ],
        };
        let words = [
            t.kind,
            t.duration,
            0,
            t.start_rgb[0],
            t.start_rgb[1],
            t.start_rgb[2],
            0,
            t.end_rgb[0],
            t.end_rgb[1],
            t.end_rgb[2],
            t.mode[0],
            t.mode[1],
            t.mode[2],
        ];
        for (i, w) in words.iter().enumerate() {
            cpu.mem.wr16(TEMPLATE + 2 * i as u32, *w as u16);
        }
        cpu.mem.load(ACTOR, &[0u8; 0x100]);
        cpu.mem.wr32(ACTOR + 0x10, 0x10);
        assert_eq!(
            cpu.call(FADE_LOAD, &[ACTOR + 0x7C, TEMPLATE]),
            Stop::Returned
        );
        let mut fade = FadeState::load(&t);
        let run = if case % 50 == 0 { 40_000 } else { 600 };
        long += usize::from(run > 600);
        zero += usize::from(duration == 0);
        for frame in 0..run {
            let dt = 1 + rng.below(3) as u8;
            cpu.mem.wr8(DELTA, dt);
            pushes.borrow_mut().clear();
            assert_eq!(cpu.call(FADE_TICK, &[ACTOR]), Stop::Returned);
            let retail = pushes.borrow().first().map(|p| p.2);
            let gone = cpu.mem.rd32(ACTOR + 0x10) & 8 != 0;
            let alive = fade.step_vsyncs(dt);
            let port = (alive && fade.visible()).then(|| {
                let [r, g, b] = fade.rgb();
                u32::from(r) | u32::from(g) << 8 | u32::from(b) << 16
            });
            frames += 1;
            if (retail, gone) != (port, !alive) {
                if failures.len() < 12 {
                    failures.push(format!(
                        "case {case} {t:?} frame {frame} dt {dt}: retail {retail:x?} gone {gone} \
                         port {port:x?} gone {}",
                        !alive
                    ));
                }
                break;
            }
            if gone {
                break;
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    eprintln!("{frames} frames, {long} long, {zero} zero-duration");
    assert!(long > 30 && zero > 200, "{long} {zero}");
}
