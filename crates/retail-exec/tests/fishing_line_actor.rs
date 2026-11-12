//! The reeling-line actor `FUN_801D4948` (PROT 0972) frame by frame, its
//! sub-state word `DAT_801D91C8` driven from the hook through the catch
//! celebration and back, every call stubbed but the celebration's ring bursts
//! `FUN_801D7A5C` and screen fades `FUN_801D7964`, against `LineActorSim` at
//! generated catch scores and frame steps (`0x1F800393`): the sub-state each
//! frame leaves, the SFX cues it stores in the three request words, the
//! bursts it fires and their eye-space
//! points, the flash pair, and the frame the celebration hands back.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_fishing::fishing_actors::LineActorSim;
use psx_retail_exec::{A0, Stop};
use std::cell::RefCell;
use std::rc::Rc;

const LINE: u32 = 0x801D_4948;
const LINE_END: u32 = 0x801D_5298;
const BURST: u32 = 0x801D_7A5C;
const FADE: u32 = 0x801D_7964;
const OVERLAY_BASE: u32 = 0x801C_E818;
const SUB_STATE: u32 = 0x801D_91C8;
const SCORE: u32 = 0x801D_91B8;
const STAGE: u32 = 0x801D_92A8;
/// The three SFX request words the arm and the celebration store their cues
/// in (`0x8007B6DA` on the hook and the second burst tier, `0x8007B6D8` on
/// the stage and the first tier, `0x8007B6DC` on the third).
const CUE_WORDS: [u32; 3] = [0x8007_B6D8, 0x8007_B6DA, 0x8007_B6DC];
const FRAME_STEP: u32 = 0x1F80_0393;
const ACTOR: u32 = 0x8013_0000;
const FISH: u32 = 0x8013_1000;

#[test]
fn the_line_actor_celebrates_as_retail() {
    differential!(prot(972, LINE));
    let (Some(mut cpu), Some(d)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem
        .load(OVERLAY_BASE, &d.read_entry(972).expect("PROT 0972"));
    cpu.stub_outside = Some((LINE, LINE_END));
    let bursts = Rc::new(RefCell::new(Vec::<(i16, i16, i16)>::new()));
    let b = bursts.clone();
    cpu.hook(BURST, move |r, m| {
        let p = r.r[A0];
        b.borrow_mut()
            .push((m.rd16(p) as i16, m.rd16(p + 2) as i16, m.rd16(p + 4) as i16));
    });
    let fades = Rc::new(RefCell::new(0u32));
    let f = fades.clone();
    cpu.hook(FADE, move |_, _| *f.borrow_mut() += 1);
    let mut rng = Rng(0x4C49_4E45);
    let (mut landed, mut handed_back) = (0, 0);
    for round in 0..400 {
        cpu.mem.load(ACTOR, &[0u8; 0x100]);
        cpu.mem.load(FISH, &[0u8; 0x100]);
        cpu.mem.wr32(ACTOR + 0x48, FISH);
        cpu.mem.wr32(SUB_STATE, 0);
        cpu.mem.wr32(STAGE, 0);
        let score = [
            0,
            150,
            201,
            600,
            601,
            800,
            801,
            0x4B0,
            0x4B1,
            rng.below(0x800) as i32,
        ][rng.below(10) as usize];
        let mut sim = LineActorSim::hooked();
        let land_at = 3 + rng.below(4);
        let varied = rng.below(2) == 0;
        for frame in 0..400u32 {
            if frame == land_at {
                cpu.mem.wr32(SUB_STATE, 4);
                cpu.mem.wr16(ACTOR + 0x22, 0);
                cpu.mem.wr32(SCORE, score as u32);
                sim.land(score);
                landed += 1;
            }
            let step = if varied { 1 + rng.below(3) } else { 2 } as u8;
            cpu.mem.wr8(FRAME_STEP, step);
            for w in CUE_WORDS {
                cpu.mem.wr16(w, 0xFFFF);
            }
            bursts.borrow_mut().clear();
            *fades.borrow_mut() = 0;
            assert_eq!(cpu.call(LINE, &[ACTOR]), Stop::Returned);
            let out = sim.tick(i16::from(step));
            let label = format!("round {round} frame {frame} step {step} score {score}");
            assert_eq!(cpu.mem.rd32(SUB_STATE), sim.raw, "{label}: sub-state");
            let mut retail_cues: Vec<u8> = CUE_WORDS
                .iter()
                .map(|&w| cpu.mem.rd16(w))
                .filter(|&c| c != 0xFFFF)
                .map(|c| c as u8)
                .collect();
            let mut port_cues: Vec<u8> = out
                .cue
                .into_iter()
                .chain(out.bursts.iter().filter_map(|b| b.cue))
                .collect();
            retail_cues.sort_unstable();
            port_cues.sort_unstable();
            assert_eq!(retail_cues, port_cues, "{label}: cues");
            let fired: Vec<(i16, i16, i16)> = bursts.borrow().iter().step_by(2).copied().collect();
            let port_bursts: Vec<(i16, i16, i16)> = out.bursts.iter().map(|b| b.offset).collect();
            assert_eq!(fired, port_bursts, "{label}: bursts");
            assert_eq!(*fades.borrow() == 2, out.flash, "{label}: flash");
            if out.done {
                handed_back += 1;
                break;
            }
            if frame > land_at {
                assert_eq!(
                    cpu.mem.rd16(ACTOR + 0x22) as i16,
                    sim.timer,
                    "{label}: the celebration timer"
                );
            }
        }
    }
    assert!(
        landed == 400 && handed_back == 400,
        "{landed} {handed_back}"
    );
}
