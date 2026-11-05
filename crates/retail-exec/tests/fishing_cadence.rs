//! The fishing cadence recogniser `FUN_801D3DB4` against `ReelCadence::feed`,
//! frame by frame over the disc's gesture templates: held-button scripts close
//! to a template, with taps thrown in, and frames of two steps.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_fishing::fishing::ReelCadence;
use legaia_minigame_formats::fishing_species as fs;
use legaia_retail_exec::{Stop, V0};

const RECOGNISE: u32 = 0x801D_3DB4;
const LAST: u32 = 0x801D_9064;
const INDEX: u32 = 0x801D_91DC;
const RING: u32 = 0x801D_91E4;
const HELD: u32 = 0x8007_B850;
const FRAME_STEP: u32 = 0x1F80_0393;

#[test]
fn a_reel_gesture_is_recognised_as_retail_recognises_it() {
    let (Some(mut cpu), Some(d)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let overlay = d
        .read_entry(fs::FISHING_OVERLAY_PROT_INDEX)
        .expect("PROT 0972");
    cpu.mem.load(fs::FISHING_OVERLAY_BASE_VA, &overlay);
    let templates = fs::parse_cadence_templates(&overlay).expect("templates");
    let mut port = ReelCadence::new(templates.clone());
    let mut rng = Rng(0x4341_4445);
    let mut matched = [0u32; 4];
    cpu.mem.wr32(LAST, 0);
    for scenario in 0..4000 {
        // A template played near its timings, sometimes with a stray tap.
        let tpl = &templates[rng.below(templates.len() as u32) as usize];
        let mut script: Vec<(u32, i32)> = Vec::new();
        for step in &tpl.steps {
            let jitter = rng.below(25) as i32 - 12;
            script.push((u32::from(step.button), (step.duration + jitter).max(1)));
            if rng.below(6) == 0 {
                script.push((rng.below(3), 1 + rng.below(6) as i32));
            }
        }
        if rng.below(4) == 0 {
            script.push((rng.below(3), 1 + rng.below(90) as i32));
        }
        let step = if scenario % 2 == 0 { 1 } else { 2 };
        for (button, held) in script {
            let mask = match button {
                1 => 0x40,
                2 => 0x80,
                _ => 0,
            };
            for frame in 0..(held + step - 1) / step {
                cpu.mem.wr32(HELD, mask);
                cpu.mem.wr8(FRAME_STEP, step as u8);
                assert_eq!(cpu.call(RECOGNISE, &[]), Stop::Returned);
                let retail = cpu.regs.r[V0] as i32;
                let ours = port
                    .feed(button as u8, step)
                    .map_or(-1, |t| t as i32);
                assert_eq!(retail, ours, "scenario {scenario} frame {frame}: match");
                if retail >= 0 {
                    matched[retail as usize] += 1;
                }
                let ring: Vec<(u8, i32)> = (0..16u32)
                    .map(|i| {
                        (
                            cpu.mem.rd32(RING + i * 8) as u8,
                            cpu.mem.rd32(RING + i * 8 + 4) as i32,
                        )
                    })
                    .collect();
                assert_eq!(
                    (
                        ring.as_slice(),
                        cpu.mem.rd32(INDEX) as usize,
                        cpu.mem.rd32(LAST) as u8
                    ),
                    port.state(),
                    "scenario {scenario} frame {frame}: ring"
                );
            }
        }
    }
    assert!(matched.iter().all(|&n| n > 10), "{matched:?}");
}
