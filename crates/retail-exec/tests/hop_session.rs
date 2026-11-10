//! The ledge hop's phase watcher `FUN_801D2298` (PROT 0897, the tick of the
//! `0x801F2294` record `FUN_801D2404` chains) against
//! `advance_hop_session`, frame by frame from take-off to release at every
//! frame step: the player's `+0x62` / `+0x10` bits, the phase word
//! `_DAT_8007BDD8`, the two cues and the record's own tear-down. The follow
//! camera's ease and clamp it runs first are stubbed.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_vm::field_ledge_hop_arc::{HopSession, advance_hop_session};
use psx_retail_exec::{A0, Stop};
use std::cell::Cell;
use std::rc::Rc;

const WATCH: u32 = 0x801D_2298;
const OVERLAY_BASE: u32 = 0x801C_E818;
const PLAYER_PTR: u32 = 0x8007_C364;
const PLAYER: u32 = 0x8013_0000;
const RECORD: u32 = 0x8013_1000;
const PHASE: u32 = 0x8007_BDD8;
const FRAME_STEP: u32 = 0x1F80_0393;
const SFX: u32 = 0x8003_5B50;

#[test]
fn the_hop_phases_run_as_retail_runs_them() {
    differential!(prot(897, WATCH));
    let (Some(mut cpu), Some(d)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem
        .load(OVERLAY_BASE, &d.read_entry(897).expect("PROT 0897"));
    cpu.stub(0x801D_B510, 0);
    cpu.stub(0x801D_AA50, 0);
    let cue = Rc::new(Cell::new(None::<u8>));
    let c = cue.clone();
    cpu.hook(SFX, move |r, _| c.set(Some(r.r[A0] as u8)));
    cpu.mem.wr32(PLAYER_PTR, PLAYER);
    let mut rng = Rng(0x484F_5053);
    let (mut landings, mut releases) = (0, 0);
    for run in 0..3000 {
        let mut s = HopSession {
            cursor: 0,
            extent: [0x10i16, 1, 3, 0x30, 0][rng.below(5) as usize],
        };
        let (mut flags, mut anim) = (rng.next(), rng.next() as u16);
        let mut phase = rng.next();
        cpu.mem.wr32(PLAYER + 0x10, flags);
        cpu.mem.wr16(PLAYER + 0x62, anim);
        cpu.mem.wr32(PHASE, phase);
        cpu.mem.wr32(RECORD + 0x10, 0);
        cpu.mem.wr16(RECORD + 0x9C, 0);
        cpu.mem.wr16(RECORD + 0x9E, s.extent as u16);
        for frame in 0..80 {
            let step = [1u8, 1, 2, 3, 4][rng.below(5) as usize];
            cpu.mem.wr8(FRAME_STEP, step);
            cue.set(None);
            assert_eq!(cpu.call(WATCH, &[RECORD]), Stop::Returned);
            let t = advance_hop_session(&mut s, step);
            // Set before clear, as the World host applies them and as the
            // take-off arm runs ahead of the crossing.
            flags = (flags | t.player_flags_set) & !t.player_flags_clear;
            anim = (anim | t.player_anim_set) & !t.player_anim_clear;
            if let Some(p) = t.phase {
                phase = p;
            }
            let label = format!("run {run} frame {frame} step {step} {s:?}");
            assert_eq!(cpu.mem.rd16(RECORD + 0x9C) as i16, s.cursor, "{label}");
            assert_eq!(cpu.mem.rd32(PLAYER + 0x10), flags, "{label}");
            assert_eq!(cpu.mem.rd16(PLAYER + 0x62), anim, "{label}");
            assert_eq!(cpu.mem.rd32(PHASE), phase, "{label}");
            assert_eq!(cue.get(), t.sfx, "{label}");
            assert_eq!(cpu.mem.rd32(RECORD + 0x10) & 8 != 0, t.finished, "{label}");
            landings += u32::from(t.phase == Some(7));
            if t.finished {
                releases += 1;
                break;
            }
        }
    }
    assert!(landings > 2000 && releases > 2500, "{landings} {releases}");
}
