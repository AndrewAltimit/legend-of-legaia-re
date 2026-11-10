//! Two short routines of the field overlay (PROT 0897): the save-screen
//! hand-off `FUN_801D841C` against
//! `legaia_engine_field::save_screen_spawn::save_screen_spawn`, and the slot-6
//! audio release `FUN_801D8450` against
//! `legaia_engine_field::field_audio_release::field_audio_release_steps`.
//!
//! Each is a call list and a store or two, so the calls are hooked and logged
//! and the stores read back.

use crate::common::{disc, scus_cpu};
use legaia_engine_field::field_audio_release::{ReleaseStep, field_audio_release_steps};
use legaia_engine_field::save_screen_spawn::save_screen_spawn;
use psx_retail_exec::{A0, A1, Stop, V0};
use std::cell::RefCell;
use std::rc::Rc;

const OVERLAY_BASE: u32 = 0x801C_E818;
const SAVE_SPAWN: u32 = 0x801D_841C;
const AUDIO_RELEASE: u32 = 0x801D_8450;

const SPAWN: u32 = 0x8002_0DE0;
const STOP_VOICE: u32 = 0x8006_53C8;
const CLOSE_VAB: u32 = 0x8001_FF58;

const ACTOR: u32 = 0x8012_0000;

#[test]
fn the_save_screen_spawn_matches_retail() {
    differential!(prot(897, SAVE_SPAWN));
    let Some(disc) = disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut cpu = scus_cpu().expect("disc present");
    cpu.mem
        .load(OVERLAY_BASE, &disc.read_entry(897).expect("PROT 0897"));
    let log = Rc::new(RefCell::new(Vec::<(u32, u32)>::new()));
    let l = log.clone();
    cpu.hook(SPAWN, move |r, _| {
        l.borrow_mut().push((r.r[A0], r.r[A1]));
        r.r[V0] = ACTOR;
    });
    let want = save_screen_spawn();
    cpu.mem.wr32(want.pool, 0x8012_7000);
    // The spawned actor's record, so the one store shows against its
    // neighbours.
    for o in (0..0x100).step_by(2) {
        cpu.mem.wr16(ACTOR + o, 0xA5A5);
    }
    assert_eq!(cpu.call(SAVE_SPAWN, &[]), Stop::Returned);
    assert_eq!(*log.borrow(), [(want.descriptor, 0x8012_7000)]);
    for o in (0..0x100).step_by(2) {
        let expect = if o == 0x5C {
            want.field_5c as u16
        } else {
            0xA5A5
        };
        assert_eq!(cpu.mem.rd16(ACTOR + o), expect, "actor +{o:#x}");
    }
}

#[test]
fn the_field_audio_release_matches_retail() {
    differential!(prot(897, AUDIO_RELEASE));
    let Some(disc) = disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut cpu = scus_cpu().expect("disc present");
    cpu.mem
        .load(OVERLAY_BASE, &disc.read_entry(897).expect("PROT 0897"));
    let steps = field_audio_release_steps();
    let globals: Vec<u32> = steps
        .iter()
        .filter_map(|s| match *s {
            ReleaseStep::ClearGlobal(va) => Some(va),
            _ => None,
        })
        .collect();
    // The log carries each call with the two latches as they stand at it:
    // both clears come after the last call.
    let log = Rc::new(RefCell::new(Vec::<(u32, u32, Vec<u32>)>::new()));
    for va in [STOP_VOICE, CLOSE_VAB] {
        let (l, g) = (log.clone(), globals.clone());
        cpu.hook(va, move |r, m| {
            let seen = g.iter().map(|&a| m.rd32(a)).collect();
            l.borrow_mut().push((va, r.r[A0], seen));
        });
    }
    for &g in &globals {
        cpu.mem.wr32(g, 0x1234_5678);
        cpu.mem.wr32(g - 4, 0x0BAD_F00D);
        cpu.mem.wr32(g + 4, 0x0BAD_F00D);
    }
    assert_eq!(cpu.call(AUDIO_RELEASE, &[]), Stop::Returned);
    let live = vec![0x1234_5678; globals.len()];
    let want: Vec<(u32, u32, Vec<u32>)> = steps
        .iter()
        .filter_map(|s| match *s {
            ReleaseStep::StopVoice(v) => Some((STOP_VOICE, u32::from(v), live.clone())),
            ReleaseStep::ReleaseVabSlot(n) => Some((CLOSE_VAB, u32::from(n), live.clone())),
            ReleaseStep::ClearGlobal(_) => None,
        })
        .collect();
    assert_eq!(*log.borrow(), want);
    assert_eq!(globals.len(), 2);
    assert!(
        matches!(
            steps[steps.len() - 2..],
            [ReleaseStep::ClearGlobal(_), ReleaseStep::ClearGlobal(_)]
        ),
        "the clears close the list"
    );
    for &g in &globals {
        assert_eq!(cpu.mem.rd32(g), 0, "{g:#x}");
        assert_eq!(cpu.mem.rd32(g - 4), 0x0BAD_F00D, "below {g:#x}");
        assert_eq!(cpu.mem.rd32(g + 4), 0x0BAD_F00D, "above {g:#x}");
    }
}
