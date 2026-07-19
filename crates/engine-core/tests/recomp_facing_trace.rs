//! Env-gated frame-level oracle: NPC facing during a story event against a
//! per-frame ground-truth trace captured from the static recomp of the
//! retail game (the town01 Mei dinner walk-on beat, snapshot-ring frame
//! exact on her actor node).
//!
//! Gate: `LEGAIA_RECOMP_TRACE_DIR` must point at a directory containing
//! `run2_dinner_beat.jsonl`. The trace is Sony-derived and never committed -
//! without the env var this test **skips and passes** (the same convention
//! as the `LEGAIA_DISC_BIN` disc gate).
//!
//! What it pins, all data-driven from the trace (no Sony bytes in the repo):
//!
//! - **Scripted facing ramps are linear at per-op rates** (`arc / budget`)
//!   with an exact terminal snap onto an eight-point compass value: every
//!   ramp in the beat is reproduced tick-for-tick by the ported motion-VM
//!   `0x38` RotateToAngle law (`legaia_engine_vm::motion_vm::step`) at the
//!   retail speed scalar, for some 7-bit frame budget - including the
//!   floor-divide increment pattern.
//! - **Mid-ramp headings hold raw values outside `0..0xFFF`** - the trace's
//!   wrap-crossing turns run `0xFFxx` raw headings live, and the port's
//!   unmasked wrapping write-back reproduces them (a per-tick `& 0xFFF`
//!   would diverge on exactly those frames).
//! - **Walk legs hold one compass heading for their whole run** (the
//!   once-per-leg write law), matching the step direction's sign-derived
//!   LUT entry; replayed through both the raw motion VM and the engine's
//!   field-NPC tick (`World::start_field_npc_motion` + `World::tick`).
//!
//! Heading spaces: the trace records retail `+0x26` (`0` = -Z); the engine
//! heading is the same angle rotated a half-turn (`engine = (retail +
//! 0x800) & 0xFFF`). Raw mid-ramp values therefore compare through a
//! constant wrapping offset, and terminal snaps through the masked
//! conversion - both asserted exactly, wraparound-aware.

use legaia_engine_core::world::{SceneMode, World};
use legaia_engine_vm::motion_vm::{MotionState, MotionTarget, StepResult, step, walk_facing_index};
use std::path::PathBuf;

/// Retail speed scalar during the traced beat (`_DAT_1F800393`): the actor
/// tick ran once per two display frames, consuming 2 budget units per tick.
const RETAIL_SPEED: u16 = 2;

/// Mei's actor id in the trace (`node +0x50`).
const MEI_ID: i64 = 0x46;

/// One per-frame Mei sample.
#[derive(Clone, Copy, PartialEq, Eq)]
struct Sample {
    frame: i64,
    x: i64,
    z: i64,
    h: u16,
}

/// Parse the first integer following `"key":` at or after `from`.
fn num_after(line: &str, key: &str, from: usize) -> Option<(i64, usize)> {
    let pat = format!("\"{key}\":");
    let at = from + line[from..].find(&pat)? + pat.len();
    let rest = line[at..].trim_start();
    let skipped = line[at..].len() - rest.len();
    let end = rest
        .find(|c: char| !(c.is_ascii_digit() || c == '-'))
        .unwrap_or(rest.len());
    let v: i64 = rest[..end].parse().ok()?;
    Some((v, at + skipped + end))
}

/// Extract `(frame, x, z, heading)` for the Mei actor from one JSONL line.
fn parse_line(line: &str) -> Option<Sample> {
    let (frame, _) = num_after(line, "frame", 0)?;
    let actors_at = line.find("\"actors\"")?;
    let mut cursor = actors_at;
    loop {
        let (id, after) = num_after(line, "i", cursor)?;
        if id == MEI_ID {
            let (x, after) = num_after(line, "x", after)?;
            let (z, after) = num_after(line, "z", after)?;
            let (h, _) = num_after(line, "heading", after)?;
            return Some(Sample {
                frame,
                x,
                z,
                h: (h as u64 & 0xFFFF) as u16,
            });
        }
        cursor = after;
    }
}

/// A change-point in the per-frame series: the new state plus the state it
/// replaced (one actor sim tick - the retail actor tick ran every 2 display
/// frames during the beat).
#[derive(Clone, Copy)]
struct Ev {
    frame: i64,
    x: i64,
    z: i64,
    h: u16,
    px: i64,
    pz: i64,
    ph: u16,
}

/// A facing ramp: seed heading + the ordered raw per-tick values (terminal
/// snap last).
struct Ramp {
    h0: u16,
    seq: Vec<u16>,
}

/// A walk leg: constant per-tick step vector, held heading.
struct Leg {
    x0: i64,
    z0: i64,
    step: (i64, i64),
    ticks: Vec<(i64, i64)>,
    h: u16,
    ph: u16,
}

fn segment(events: &[Ev]) -> (Vec<Ramp>, Vec<Leg>) {
    let mut ramps = Vec::new();
    let mut legs = Vec::new();
    let mut i = 0usize;
    while i < events.len() {
        let e = events[i];
        let moved = (e.x, e.z) != (e.px, e.pz);
        let jump = (e.x - e.px).abs().max((e.z - e.pz).abs()) > 64;
        if !moved && e.h != e.ph {
            // Facing ramp: contiguous 2-frame-cadence heading changes with
            // the position parked.
            let mut seq = vec![e.h];
            let mut j = i + 1;
            while j < events.len() {
                let n = events[j];
                if (n.x, n.z) != (n.px, n.pz) || n.h == n.ph || n.frame - events[j - 1].frame != 2 {
                    break;
                }
                seq.push(n.h);
                j += 1;
            }
            ramps.push(Ramp { h0: e.ph, seq });
            i = j;
        } else if moved && !jump {
            // Walk leg: contiguous 2-frame-cadence steps with a constant
            // step vector (a speed change is a new leg - a new walk op).
            let step_v = (e.x - e.px, e.z - e.pz);
            let mut ticks = vec![(e.x, e.z)];
            let mut j = i + 1;
            while j < events.len() {
                let n = events[j];
                if (n.x - n.px, n.z - n.pz) != step_v
                    || n.h != e.h
                    || n.frame - events[j - 1].frame != 2
                {
                    break;
                }
                ticks.push((n.x, n.z));
                j += 1;
            }
            legs.push(Leg {
                x0: e.px,
                z0: e.pz,
                step: step_v,
                ticks,
                h: e.h,
                ph: e.ph,
            });
            i = j;
        } else {
            i += 1; // teleport (seat-poke / despawn) or a lone event
        }
    }
    (ramps, legs)
}

/// Drive the ported `0x38` RotateToAngle over every 7-bit budget and return
/// the budget that reproduces the traced raw per-tick sequence exactly, or
/// `None`. `h0`/`seq` are retail-space; the VM runs in engine space (retail
/// rotated a half-turn), so mid-ramp raws compare through the constant
/// wrapping offset `seed - h0`, terminal snaps through the masked conversion.
fn ramp_budget(h0: u16, seq: &[u16]) -> Option<u8> {
    let hn = *seq.last()?;
    let idx = ((hn & 0xFFF) / 0x200) as u8;
    let decreasing = seq[0].wrapping_sub(h0) >= 0x8000;
    let seed = h0.wrapping_add(0x800) & 0xFFF;
    let delta = seed.wrapping_sub(h0);
    'budget: for budget in 1..=0x7Fu8 {
        let bc = [0x38, idx, u8::from(decreasing) << 7 | budget];
        let mut st = MotionState {
            yaw: seed,
            speed: RETAIL_SPEED,
            ..Default::default()
        };
        let mut got = Vec::new();
        for _ in 0..0x100 {
            let r = step(&mut st, MotionTarget::default(), &bc);
            got.push(st.yaw);
            if r == StepResult::Done {
                break;
            }
        }
        if got.len() != seq.len() {
            continue;
        }
        let (last, mid) = got.split_last().unwrap();
        for (g, s) in mid.iter().zip(seq) {
            if *g != s.wrapping_add(delta) {
                continue 'budget;
            }
        }
        if *last != hn.wrapping_add(0x800) & 0xFFF {
            continue;
        }
        return Some(budget);
    }
    None
}

#[test]
fn mei_dinner_beat_facing_matches_the_recomp_trace_frame_exact() {
    let Some(dir) = std::env::var_os("LEGAIA_RECOMP_TRACE_DIR") else {
        eprintln!("[skip] LEGAIA_RECOMP_TRACE_DIR unset (recomp-trace-gated)");
        return;
    };
    let path = PathBuf::from(dir).join("run2_dinner_beat.jsonl");
    let Ok(text) = std::fs::read_to_string(&path) else {
        eprintln!("[skip] {} not readable", path.display());
        return;
    };

    // Per-frame series -> actor sim-tick change points.
    let samples: Vec<Sample> = text.lines().filter_map(parse_line).collect();
    assert!(
        samples.len() > 1000,
        "trace unexpectedly short ({} Mei samples)",
        samples.len()
    );
    let mut events = Vec::new();
    for w in samples.windows(2) {
        let (p, c) = (w[0], w[1]);
        if (c.x, c.z, c.h) != (p.x, p.z, p.h) {
            events.push(Ev {
                frame: c.frame,
                x: c.x,
                z: c.z,
                h: c.h,
                px: p.x,
                pz: p.z,
                ph: p.h,
            });
        }
    }
    let (ramps, legs) = segment(&events);

    // --- Scripted facing ramps: linear per-op rates, terminal compass snap,
    // raw pre-unwrap mid-ramp holds - all reproduced by the ported law.
    assert!(
        ramps.len() >= 6,
        "the beat authors at least six facing ramps (got {})",
        ramps.len()
    );
    let mut wrap_crossing = 0usize;
    for (n, r) in ramps.iter().enumerate() {
        let hn = *r.seq.last().unwrap();
        assert_eq!(
            hn & 0x1FF,
            0,
            "ramp {n}: terminal heading {hn:#06X} must snap onto the compass"
        );
        assert!(
            (0..=0xFFF).contains(&i32::from(r.h0)),
            "ramp {n}: seed heading {:#06X} should be a settled in-range value",
            r.h0
        );
        if r.seq.iter().any(|&h| h > 0xFFF) {
            wrap_crossing += 1;
        }
        assert!(
            ramp_budget(r.h0, &r.seq).is_some(),
            "ramp {n} ({:#06X} -> {hn:#06X}, {} ticks, raws {:X?}): no 7-bit \
             budget reproduces it through the ported rotate law",
            r.h0,
            r.seq.len(),
            r.seq,
        );
    }
    assert!(
        wrap_crossing >= 1,
        "the beat's turns cross the 0x1000 wrap at least once (raw 0xFFxx \
         mid-ramp headings) - segmentation lost them"
    );

    // --- Walk legs: one heading per leg, the step direction's compass entry.
    assert!(
        legs.len() >= 3,
        "the beat walks Mei over several legs (got {})",
        legs.len()
    );
    assert!(
        legs.iter()
            .any(|l| l.step.0.abs() > 0 && l.step.1.abs() > 0),
        "the walk-off diagonal leg is present"
    );
    for (n, l) in legs.iter().enumerate() {
        if l.ticks.len() < 3 {
            continue; // too short to pin anything
        }
        let idx = walk_facing_index(l.step.0 as i32, l.step.1 as i32).expect("moving leg");
        assert_eq!(
            l.h,
            u16::from(idx) * 0x200,
            "leg {n}: held heading is the step direction's compass entry"
        );

        // Raw motion VM replay: per-tick positions match, and the heading is
        // written once at the leg start, then held.
        let speed = l.step.0.abs().max(l.step.1.abs()) as u16;
        let mut st = MotionState {
            world_x: l.x0 as i16,
            world_z: l.z0 as i16,
            speed,
            yaw: l.ph.wrapping_add(0x800) & 0xFFF,
            ..Default::default()
        };
        let target = MotionTarget {
            x: (l.x0 + l.step.0 * l.ticks.len() as i64) as i16,
            z: (l.z0 + l.step.1 * l.ticks.len() as i64) as i16,
            ..Default::default()
        };
        for (t, &(ex, ez)) in l.ticks.iter().enumerate() {
            let _ = step(&mut st, target, &[0x47]);
            assert_eq!(
                (i64::from(st.world_x), i64::from(st.world_z)),
                (ex, ez),
                "leg {n} tick {t}: position"
            );
            assert_eq!(
                st.yaw_written,
                t == 0,
                "leg {n} tick {t}: heading writes once per leg, at leg start"
            );
            assert_eq!(
                st.yaw,
                l.h.wrapping_add(0x800) & 0xFFF,
                "leg {n} tick {t}: held engine heading"
            );
        }

        // Engine NPC-tick replay: the same leg through the World field tick
        // (start kernel + per-frame motion step + render-heading store).
        let mut world = World::new();
        world.mode = SceneMode::Field;
        let slot = 1u8;
        world
            .field_npc_positions
            .insert(slot, (l.x0 as i16, l.z0 as i16));
        world
            .field_npc_headings
            .insert(slot, (l.ph.wrapping_add(0x800) & 0xFFF) as i16);
        world.field_npc_glide_speeds.insert(slot, speed);
        assert!(world.start_field_npc_motion(slot, target.x, target.z));
        for (t, &(ex, ez)) in l.ticks.iter().enumerate() {
            let _ = world.tick();
            assert_eq!(
                world.field_npc_positions.get(&slot),
                Some(&(ex as i16, ez as i16)),
                "leg {n} tick {t}: NPC-tick position"
            );
            assert_eq!(
                world.field_npc_headings.get(&slot),
                Some(&((l.h.wrapping_add(0x800) & 0xFFF) as i16)),
                "leg {n} tick {t}: NPC-tick render heading"
            );
        }
    }

    eprintln!(
        "[oracle] {} ramps ({} wrap-crossing) + {} walk legs replayed \
         frame-exact against {}",
        ramps.len(),
        wrap_crossing,
        legs.len(),
        path.display()
    );
}
