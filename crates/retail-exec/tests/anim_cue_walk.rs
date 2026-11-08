//! The battle animation cue walker `FUN_800508DC(slot, entry, key)` against
//! `legaia_engine_battle::anim_cue::walk_anim_cues`: the cursor `+0x1F6` it
//! stores back, every cue it routes through `FUN_8004FE5C` or dispatches
//! through `FUN_8004FCC8` (both hooked and logged), the shout tally at record
//! `+0x98` and the `rand()` draws.

use crate::common::{Rng, scus_cpu};
use legaia_engine_battle::anim_cue::{AnimCueActor, AnimCueEmit, AnimCueSlot, walk_anim_cues};
use psx_retail_exec::{A0, A1, Stop, V0};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

const WALK: u32 = 0x8005_08DC;
const ROUTE: u32 = 0x8004_FE5C;
const DISPATCH: u32 = 0x8004_FCC8;
const RAND: u32 = 0x8005_6798;
const ACTORS: u32 = 0x801C_9370;
const SEATS: u32 = 0x8007_BD10;
const RECORDS: u32 = 0x8008_4140;
const CD_BUSY: u32 = 0x8007_BC20;

const ACTOR: u32 = 0x8013_0000;
const ENTRY: u32 = 0x8014_0000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ev {
    Route(u16, u8),
    Dispatch(u16),
}

fn gen_cue(rng: &mut Rng) -> u16 {
    match rng.below(6) {
        0 => [0xD7, 0xE7, 0xF7][rng.below(3) as usize],
        1 => 0xC8 + rng.below(0x38) as u16,
        2 => [0x4C, 0x4D, 0xC7, 0xFA, 0x12, 0x13][rng.below(6) as usize],
        3 => 0,
        _ => 1 + rng.below(0xC7) as u16,
    }
}

#[test]
fn the_walker_matches_retail() {
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let log: Rc<RefCell<Vec<Ev>>> = Rc::new(RefCell::new(Vec::new()));
    let l = log.clone();
    cpu.hook(ROUTE, move |r, _| {
        l.borrow_mut()
            .push(Ev::Route(r.r[A0] as u16, r.r[A1] as u8))
    });
    let l = log.clone();
    cpu.hook(DISPATCH, move |r, _| {
        l.borrow_mut().push(Ev::Dispatch(r.r[A0] as u16))
    });
    let state = Rc::new(Cell::new(0u32));
    let s = state.clone();
    cpu.hook(RAND, move |r, _| {
        let next = s.get().wrapping_mul(0x41C6_4E6D).wrapping_add(0x3039);
        s.set(next);
        r.r[V0] = (next >> 16) & 0x7FFF;
    });
    for slot in 0..8u32 {
        cpu.mem.wr32(ACTORS + slot * 4, ACTOR);
    }
    let mut rng = Rng(0x4355_4553);
    let (mut shouts, mut fallbacks, mut muted, mut failures) = (0, 0, 0, Vec::new());
    for round in 0..40000 {
        let slot = rng.below(7) as u8;
        let char_id = 1 + rng.below(4) as u8;
        let anim_id = if rng.below(3) == 0 {
            0x12
        } else {
            rng.below(0x40) as u8
        };
        let mute = rng.below(3) == 0;
        let cd_busy = rng.below(3) == 0;
        let cursor = rng.below(9) as u8;
        let key = rng.below(0x40) as u8;
        // Ascending trigger frames, so a walk often fires several cues.
        let mut frame = 0u16;
        let track: Vec<AnimCueSlot> = (0..8)
            .map(|_| {
                frame += rng.below(12) as u16;
                AnimCueSlot {
                    frame,
                    cue: gen_cue(&mut rng),
                }
            })
            .collect();
        let tally = rng.next();
        let seed = rng.next();

        // Retail.
        for (i, c) in track.iter().enumerate() {
            cpu.mem.wr16(ENTRY + 0x54 + i as u32 * 4, c.frame);
            cpu.mem.wr16(ENTRY + 0x56 + i as u32 * 4, c.cue);
        }
        // What follows the track in a real entry: the walk reads one slot past it.
        cpu.mem.wr32(ENTRY + 0x74, 0);
        cpu.mem.wr8(ENTRY + 0x77, anim_id);
        cpu.mem.wr8(ACTOR + 0x1F6, cursor);
        cpu.mem.wr8(SEATS + u32::from(slot), char_id);
        let rec = RECORDS + u32::from(char_id - 1) * 0x414;
        cpu.mem.wr32(rec + 0x660, tally);
        cpu.mem.wr32(rec + 0x6C0, if mute { 0x2000 } else { 0 });
        cpu.mem.wr32(CD_BUSY, u32::from(cd_busy));
        log.borrow_mut().clear();
        state.set(seed);
        assert_eq!(
            cpu.call(WALK, &[u32::from(slot), ENTRY, u32::from(key)]),
            Stop::Returned
        );
        let r_cursor = cpu.mem.rd8(ACTOR + 0x1F6);
        let r_tally = cpu.mem.rd32(rec + 0x660).wrapping_sub(tally);
        let retail = (r_cursor, log.borrow().clone(), r_tally, state.get());

        // Port.
        let actor = AnimCueActor {
            slot,
            char_id,
            anim_id,
            voice_muted: mute,
            cd_busy,
        };
        let mut port_state = seed;
        let walk = walk_anim_cues(&actor, &track, cursor, key, &mut || {
            port_state = port_state.wrapping_mul(0x41C6_4E6D).wrapping_add(0x3039);
            ((port_state >> 16) & 0x7FFF) as i32
        });
        let events: Vec<Ev> = walk
            .emits
            .iter()
            .filter_map(|e| match *e {
                AnimCueEmit::Route { id, slot } => Some(Ev::Route(id, slot)),
                AnimCueEmit::Dispatch { id } => Some(Ev::Dispatch(id)),
                AnimCueEmit::Suppressed { .. } => None,
            })
            .collect();
        let p_cursor = if walk.cursor_committed {
            walk.cursor
        } else {
            cursor
        };
        let port = (p_cursor, events, walk.shout_tally, port_state);
        shouts += u32::from(
            retail
                .1
                .iter()
                .any(|e| matches!(e, Ev::Route(id, _) if *id >= 0x100)),
        );
        fallbacks += u32::from(retail.1.iter().any(|e| matches!(e, Ev::Dispatch(_))));
        muted += u32::from(mute && retail.2 > 0);
        if retail != port && failures.len() < 8 {
            failures.push(format!(
                "round {round}: slot {slot} char {char_id} anim {anim_id:#x} mute {mute} \
                 busy {cd_busy} cursor {cursor} key {key}\n {track:?}\n retail {retail:x?}\n \
                 port   {port:x?}"
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert!(
        shouts > 1000 && fallbacks > 300 && muted > 300,
        "{shouts} {fallbacks} {muted}"
    );
}
