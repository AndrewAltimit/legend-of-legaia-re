//! The Muscle Dome's between-leg tally in PROT 0977: the per-frame roll-up
//! `FUN_801CF074` with its drain-rate leaf `FUN_801D14B0` and its voice cue
//! `FUN_801D1288`, against `ScoreTallyRamp` and the two leaves it calls.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_minigames::other_game_overlay::{
    ScoreTallyRamp, VoiceAttrCue, arena_voice_cue, cue_volume, step_scale,
};
use psx_retail_exec::{A0, A1, A2, A3, Cpu, SP, Stop, V0};
use std::cell::RefCell;
use std::rc::Rc;

const OVERLAY_BASE: u32 = 0x801C_E818;
const TICK: u32 = 0x801C_F074;
const CUE: u32 = 0x801D_1288;
const STEP: u32 = 0x801D_14B0;
const VOICE_ATTR: u32 = 0x8006_5034;
const BOOST: u32 = 0x801D_1AB4;
const COUNTER: u32 = 0x801D_1AE4;
const VOLUME: u32 = 0x8008_4580;
const FADE: [u32; 4] = [0x801D_1ABC, 0x801D_1AC0, 0x801D_1AC4, 0x801D_1AB8];
const PENDING: [u32; 4] = [0x801D_1ACC, 0x801D_1AD0, 0x801D_1AD4, 0x801D_1AAC];
const HP_ACCUM: u32 = 0x801D_1AC8;
const TALLY: u32 = 0x8008_4440;
const FRAME_STEP: u32 = 0x1F80_0393;
/// The row value draw and the label draw; their arguments are the screen's.
const VALUE_DRAW: u32 = 0x801D_1308;
const LABEL_DRAW: u32 = 0x801D_08EC;

fn rig() -> Option<(Cpu, Rc<RefCell<Vec<VoiceAttrCue>>>)> {
    let (mut cpu, d) = (scus_cpu()?, disc()?);
    cpu.mem
        .load(OVERLAY_BASE, &d.read_entry(977).expect("PROT 0977"));
    let cues = Rc::new(RefCell::new(Vec::new()));
    let sink = cues.clone();
    cpu.hook(VOICE_ATTR, move |r, m| {
        let sp = r.r[SP];
        let arg = |m: &mut psx_retail_exec::Memory, i: u32| m.rd32(sp + 0x10 + i * 4) as i32;
        sink.borrow_mut().push(VoiceAttrCue {
            voice: r.r[A0],
            vab_program_tone: (r.r[A1] as i32, r.r[A2] as i32, r.r[A3] as i32),
            note_and_fine: (arg(m, 0), arg(m, 1)),
            volume: (arg(m, 2), arg(m, 3)),
        });
    });
    Some((cpu, cues))
}

#[test]
fn the_drain_rate_is_retails() {
    differential!(prot(977, STEP));
    let Some((mut cpu, _)) = rig() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut rng = Rng(0x5354_4550);
    let edges = [
        i32::MIN,
        i32::MIN + 1,
        -6,
        -5,
        -1,
        0,
        1,
        2,
        3,
        5,
        6,
        7,
        i32::MAX,
    ];
    for round in 0..20_000 {
        let step = if round < edges.len() * 2 {
            edges[round / 2]
        } else {
            match rng.below(3) {
                0 => rng.below(40) as i32 - 20,
                1 => rng.below(100_000) as i32,
                _ => rng.next() as i32,
            }
        };
        let boost = round % 2 == 1 || rng.below(5) == 0;
        cpu.mem.wr32(BOOST, u32::from(boost));
        assert_eq!(cpu.call(STEP, &[step as u32]), Stop::Returned);
        assert_eq!(
            cpu.regs.r[V0] as i32,
            step_scale(step, boost),
            "step {step} boost {boost}"
        );
    }
}

#[test]
fn the_tally_cue_keys_what_retail_keys() {
    differential!(prot(977, CUE));
    let Some((mut cpu, cues)) = rig() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut rng = Rng(0x4355_4521);
    for round in 0..20_000 {
        let counter = [0, 3, u32::MAX, rng.next()][rng.below(4) as usize];
        let volume = [0, 200, 0x1_FFFF, 0xFFFF_FFFF, rng.next()][rng.below(5) as usize];
        cpu.mem.wr32(COUNTER, counter);
        cpu.mem.wr32(VOLUME, volume);
        cues.borrow_mut().clear();
        assert_eq!(cpu.call(CUE, &[]), Stop::Returned);
        let mut port_counter = counter;
        let port = arena_voice_cue(&mut port_counter, volume);
        assert_eq!(cue_volume(volume), port.volume.0);
        assert_eq!(
            (cues.borrow().clone(), cpu.mem.rd32(COUNTER)),
            (vec![port], port_counter),
            "round {round} counter {counter:#x} volume {volume:#x}"
        );
    }
}

/// Whole rolls, frame by frame, at frame steps of one to three, with the
/// bypass flag flipped mid-roll, against `ScoreTallyRamp::tick`. The screen's
/// six value draws are compared too: `FUN_801D1308(x, y, value, brightness)`,
/// which the routine makes in two passes.
#[test]
fn a_tally_rolls_as_retail_rolls_it() {
    differential!(prot(977, TICK));
    let Some((mut cpu, cues)) = rig() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let draws = Rc::new(RefCell::new(Vec::<(i32, i32)>::new()));
    let sink = draws.clone();
    cpu.hook(VALUE_DRAW, move |r, _| {
        sink.borrow_mut().push((r.r[A2] as i32, r.r[A3] as i32));
    });
    cpu.stub(LABEL_DRAW, 0);
    let mut rng = Rng(0x5441_4C59);
    let (mut finished, mut cued) = (0, 0);
    for roll in 0..3000 {
        let pending: [i32; 4] = std::array::from_fn(|_| match rng.below(4) {
            0 => 0,
            1 => rng.below(8) as i32,
            2 => rng.below(500) as i32,
            _ => rng.below(100_000) as i32,
        });
        let volume = rng.next();
        let fade = [0, 0x40, 0x80, rng.below(0x81) as i32][rng.below(4) as usize];
        let tally0 = rng.below(10_000) as i32;
        let mut port = ScoreTallyRamp {
            pending,
            cue_counter: rng.next(),
            ..ScoreTallyRamp::default()
        };
        for lane in 0..4 {
            cpu.mem.wr32(FADE[lane], 0);
            cpu.mem.wr32(PENDING[lane], pending[lane] as u32);
        }
        cpu.mem.wr32(HP_ACCUM, 0);
        cpu.mem.wr32(TALLY, tally0 as u32);
        cpu.mem.wr32(COUNTER, port.cue_counter);
        cpu.mem.wr32(VOLUME, volume);
        let mut tally = tally0;
        let mut boost = false;
        for frame in 0..4000 {
            let dt = 1 + rng.below(3) as u8;
            if rng.below(200) == 0 {
                boost = !boost;
            }
            cpu.mem.wr8(FRAME_STEP, dt);
            cpu.mem.wr32(BOOST, u32::from(boost));
            cues.borrow_mut().clear();
            draws.borrow_mut().clear();
            assert_eq!(cpu.call(TICK, &[fade as u32]), Stop::Returned);
            let step = port.tick(dt, boost, volume);
            tally += step.tally_gain;
            let retail = (
                cpu.regs.r[V0] != 0,
                FADE.map(|a| cpu.mem.rd32(a) as i32),
                PENDING.map(|a| cpu.mem.rd32(a) as i32),
                cpu.mem.rd32(HP_ACCUM) as i32,
                cpu.mem.rd32(TALLY) as i32,
                cpu.mem.rd32(COUNTER),
                cues.borrow().clone(),
            );
            let ported = (
                step.rolling,
                port.fade,
                port.pending,
                port.hp_accum,
                tally,
                port.cue_counter,
                step.cues.clone(),
            );
            assert_eq!(retail, ported, "roll {roll} frame {frame} dt {dt}");
            let shown: Vec<(i32, i32)> = port
                .row_values(tally)
                .into_iter()
                .zip(port.row_brightness(fade))
                .collect();
            // Each row is drawn twice, the second pass over the first.
            let shown = [shown.clone(), shown].concat();
            assert_eq!(
                draws.borrow().as_slice(),
                shown.as_slice(),
                "roll {roll} frame {frame}: the six value draws"
            );
            cued += u32::from(!step.cues.is_empty());
            if !step.rolling {
                finished += 1;
                break;
            }
        }
    }
    assert!(finished > 2900 && cued > 10_000, "{finished} {cued}");
}
