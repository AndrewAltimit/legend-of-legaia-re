//! The Tactical-Arts shout selector `FUN_8004C140` against
//! `arts_voice_cue::select_arts_voice`: every roster id, every byte-sized
//! action constant, both flag polarities, the solo latch, the forced-channel
//! formation and the party-wide last pick, over the real pools in
//! `SCUS_942.54`. The clip player and both sound-cue routes are hooked, so
//! what compares is the calls made, the `rand()` draws taken and the last-pick
//! byte left behind.

use crate::common::{Rng, scus, scus_cpu};
use legaia_art::arts_voice::ArtsVoiceSelectorTables;
use legaia_engine_battle::arts_voice_cue::{ArtsVoiceCall, ArtsVoiceEmit, select_arts_voice};
use psx_retail_exec::{A0, A1, A2, Stop, V0};
use std::cell::RefCell;
use std::rc::Rc;

const SELECTOR: u32 = 0x8004_C140;
const RAND: u32 = 0x8005_6798;
const CLIP: u32 = 0x8003_D53C;
const DISPATCH: u32 = 0x8004_FCC8;
const ROUTE: u32 = 0x8004_FE5C;
const CTX_PTR: u32 = 0x8007_BD24;
const FIRST_ENEMY: u32 = 0x8007_BD0C;
const LAST: u32 = 0x8007_BD62;
const STEP_CELL: u32 = 0x8007_BD08;
const CTX: u32 = 0x8012_0000;
const S3: usize = 19;

#[derive(Default)]
struct Log {
    emits: Vec<ArtsVoiceEmit>,
    draws: Vec<u32>,
    stream: Vec<u32>,
}

#[test]
fn the_shout_selector_picks_what_retail_picks() {
    differential!(scus(SELECTOR));
    let (Some(mut cpu), Some(exe)) = (scus_cpu(), scus()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let tables = ArtsVoiceSelectorTables::parse_from_scus(&exe).expect("selector span");
    let log = Rc::new(RefCell::new(Log::default()));
    {
        let l = log.clone();
        cpu.hook(RAND, move |r, _| {
            let mut l = l.borrow_mut();
            // Dry only in the spin, which the step budget ends.
            let v = l.stream.pop().unwrap_or(0);
            l.draws.push(v);
            r.r[V0] = v;
        });
        let l = log.clone();
        cpu.hook(CLIP, move |r, _| {
            l.borrow_mut().emits.push(ArtsVoiceEmit::Clip {
                slot: r.r[A0] as u8,
                channel: r.r[A1] as u8,
                dur: r.r[A2],
            });
        });
        let l = log.clone();
        cpu.hook(DISPATCH, move |r, _| {
            l.borrow_mut()
                .emits
                .push(ArtsVoiceEmit::Dispatch(r.r[A0] as u16));
        });
        let l = log.clone();
        cpu.hook(ROUTE, move |r, _| {
            l.borrow_mut()
                .emits
                .push(ArtsVoiceEmit::Route(r.r[A0] as u16));
        });
    }
    // A pool whose every member is the last pick spins for ever in retail.
    cpu.step_budget = 200_000;
    let mut rng = Rng(0x5348_4F55);
    let (mut clips, mut cues, mut forced, mut spins) = (0, 0, 0, 0);
    for char_id in 0u8..=4 {
        for action in 0u8..=0xFF {
            for case in 0..6u32 {
                let flag = if case & 1 == 0 {
                    0
                } else {
                    rng.below(0x40) as u8 + 1
                };
                let solo_latch = u8::from(case & 2 != 0);
                let first_enemy = if case >= 4 {
                    0x4F
                } else {
                    rng.below(0x4F) as u8
                };
                let last0 = rng.below(0x10) as u8;
                let caller_seat = rng.below(3) as u8;
                let ctx13 = rng.below(8) as u8;
                let stream: Vec<u32> = (0..64).map(|_| rng.next() & 0x7FFF).collect();

                {
                    let mut l = log.borrow_mut();
                    *l = Log::default();
                    l.stream = stream.iter().rev().copied().collect();
                }
                cpu.mem.wr32(CTX_PTR, CTX);
                cpu.mem.wr8(CTX + 0x243, solo_latch);
                cpu.mem.wr8(CTX + 0x13, ctx13);
                cpu.mem.wr8(FIRST_ENEMY, first_enemy);
                cpu.mem.wr8(LAST, last0);
                cpu.mem.wr8(0x1F80_0393, 2);
                cpu.mem.wr32(STEP_CELL, 0xDEAD);
                cpu.regs.r[S3] = u32::from(caller_seat);
                let stop = cpu.call(
                    SELECTOR,
                    &[u32::from(char_id), u32::from(action), u32::from(flag)],
                );
                let call = ArtsVoiceCall {
                    char_id,
                    action,
                    flag,
                    solo_latch,
                    first_enemy: Some(first_enemy),
                    caller_seat,
                };
                let mut last = last0;
                let mut it = stream.iter().copied();
                let mut port_draws = Vec::new();
                let emits = select_arts_voice(&tables, &call, &mut last, &mut || {
                    let v = it.next().unwrap();
                    port_draws.push(v);
                    v
                });
                if let Stop::StepLimit { .. } = stop {
                    // The spin: the port takes the draw instead (documented).
                    spins += 1;
                    continue;
                }
                assert_eq!(stop, Stop::Returned, "char {char_id} action {action:#x}");
                let l = log.borrow();
                let what = format!(
                    "char {char_id} action {action:#x} flag {flag:#x} solo {solo_latch} \
                     enemy {first_enemy:#x} last {last0:#x}"
                );
                assert_eq!(l.emits, emits, "{what}");
                assert_eq!(l.draws, port_draws, "draws: {what}");
                assert_eq!(cpu.mem.rd8(LAST), last, "last pick: {what}");
                for e in &emits {
                    match e {
                        ArtsVoiceEmit::Clip { channel: 0x0C, .. } if first_enemy == 0x4F => {
                            forced += 1
                        }
                        ArtsVoiceEmit::Clip { .. } => clips += 1,
                        _ => cues += 1,
                    }
                }
            }
        }
    }
    assert!(
        clips > 200 && cues > 50 && forced > 20,
        "{clips} {cues} {forced} {spins}"
    );
}
