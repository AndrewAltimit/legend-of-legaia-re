//! The anim node tick's commit sites, `FUN_80047430`
//! `0x800478EC..0x80047B58`, against
//! `engine_battle_vm::battle_action::tick_commits`: when a clip staged at
//! `+0x1DA` is handed to the commit `FUN_8004AD80` - at once under `+0x1DC`
//! bit 0, past the gate frame under bit 1, never mid-clip over an entry
//! whose `+0x76` lock is set, and at the natural end whatever is staged.
//!
//! The commit is hooked: it logs the `+0x1DC` / `+0x1DA` pair it is entered
//! on and does the two things the tick reads back from it, the node cursor's
//! zero (`sh zero,0x68` at `0x8004B068`) and the entry it installs at node
//! `+0x4C`. Every other call out of the routine is stubbed, the gate-index
//! reader `FUN_80050E00` among them, so each event list here carries a zero
//! behind its first frame - the shape that reader answers `0` on.

use crate::common::{Rng, scus_cpu};
use legaia_engine_battle_vm::battle_action::{TickCommits, tick_commits};
use psx_retail_exec::{A0, Stop};
use std::cell::RefCell;
use std::rc::Rc;

const TICK: u32 = 0x8004_7430;
const COMMIT: u32 = 0x8004_AD80;
const ACTORS: u32 = 0x801C_9370;
const SEATS: u32 = 0x8007_BD10;
const CTX_PTR: u32 = 0x8007_BD24;
const CTX: u32 = 0x8012_0000;
const ACTOR: u32 = 0x8013_0000;
const NODE: u32 = 0x8014_0000;
const ENTRY: u32 = 0x8014_1000;
const NEXT_ENTRY: u32 = 0x8014_2000;
const STREAM: u32 = 0x8015_0000;
const NEXT_STREAM: u32 = 0x8015_1000;
const FRAME_STEP: u32 = 0x1F80_0393;

#[test]
fn the_tick_commits_a_staged_clip_where_retail_does() {
    differential!(scus(TICK));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.stub_outside = Some((TICK, 0x8004_8108));
    cpu.step_budget = 200_000;
    cpu.mem.wr32(CTX_PTR, CTX);
    cpu.mem.wr32(ACTORS, ACTOR);
    cpu.mem.wr8(SEATS, 1);
    let log: Rc<RefCell<Vec<(u8, u8)>>> = Rc::new(RefCell::new(Vec::new()));
    let l = log.clone();
    cpu.hook(COMMIT, move |r, m| {
        let node = r.r[A0];
        l.borrow_mut()
            .push((m.rd8(ACTOR + 0x1DC), m.rd8(ACTOR + 0x1DA)));
        m.wr16(node + 0x68, 0);
        m.wr32(node + 0x4C, NEXT_ENTRY);
    });

    let mut rng = Rng(0x434F_4D54);
    // cut on bit 0, cut on bit 1, a locked bit-0 stage held, a bit-1 stage
    // short of its gate, a natural end, idle at the end, both in one tick
    let mut reached = [0u32; 7];
    for round in 0..40_000 {
        let frames = 2 + rng.below(20) as u8;
        let next_frames = [0u8, 1, 4, 9][rng.below(4) as usize];
        let cursor_frame = rng.below(u32::from(frames) + 1) as u16;
        let flags = rng.below(16) as u8;
        let staged = 1 + rng.below(9) as u8;
        let lock = [0u8, 0, 0, 1, 0x80][rng.below(5) as usize];
        let gate = rng.below(u32::from(frames)) as u8;
        let events = [gate, 0, rng.below(4) as u8, rng.below(4) as u8];
        let dt = 1 + rng.below(3) as u8;

        cpu.mem.wr16(NODE + 0x5A, 0);
        cpu.mem.wr32(NODE + 0x74, 0x10);
        cpu.mem.wr32(NODE + 0x4C, ENTRY);
        cpu.mem.wr16(NODE + 0x68, cursor_frame << 4);
        for (entry, stream, n) in [
            (ENTRY, STREAM, frames),
            (NEXT_ENTRY, NEXT_STREAM, next_frames),
        ] {
            cpu.mem.wr8(entry + 0x78, 1);
            cpu.mem.wr8(entry + 0x85, 0);
            cpu.mem.wr8(entry + 0x86, 0);
            cpu.mem.wr16(entry + 0xE, 0);
            cpu.mem.wr8(entry + 0x76, 0);
            cpu.mem.wr32(entry + 0x88, stream);
            cpu.mem.wr8(stream, 1);
            cpu.mem.wr8(stream + 1, n);
        }
        cpu.mem.wr8(ENTRY + 0x76, lock);
        for (i, e) in events.iter().enumerate() {
            cpu.mem.wr8(ENTRY + 0x10 + i as u32, *e);
        }
        // Rate 0: the advance adds nothing, so the cursor under test is the
        // one seeded.
        cpu.mem.wr8(ACTOR + 0x21D, 0);
        cpu.mem.wr16(ACTOR + 0x176, 0);
        cpu.mem.wr8(ACTOR + 0x21B, 0);
        cpu.mem.wr16(ACTOR + 0x16E, 0);
        cpu.mem.wr16(ACTOR + 0x14C, 100);
        cpu.mem.wr8(ACTOR + 0x1D9, 1);
        cpu.mem.wr8(ACTOR + 0x1DA, staged);
        cpu.mem.wr8(ACTOR + 0x1DC, flags);
        cpu.mem.wr8(ACTOR + 0x228, 0);
        cpu.mem.wr32(ACTOR + 0x10, 0);
        cpu.mem.wr16(ACTOR + 0x178, 0);
        cpu.mem.wr8(FRAME_STEP, dt);

        log.borrow_mut().clear();
        assert_eq!(cpu.call(TICK, &[NODE]), Stop::Returned, "{round}");
        let calls = log.borrow().clone();

        let frame = cursor_frame as i16;
        let port = tick_commits(
            flags,
            staged,
            &events,
            None,
            lock,
            frame,
            frames,
            (0, next_frames),
        );
        // What the hook saw: the cut enters with `+0x1DA` untouched.
        let mut want = Vec::new();
        if let Some(f) = port.cut {
            want.push((f, staged));
        }
        if let Some(pair) = port.natural_end {
            want.push(pair);
        }
        assert_eq!(
            calls, want,
            "round {round}: flags {flags:#x} staged {staged} lock {lock:#x} frame {frame}/{frames} \
             gate {gate} next {next_frames}"
        );
        let TickCommits { cut, natural_end } = port;
        reached[0] += u32::from(cut.is_some() && flags & 1 != 0);
        reached[1] += u32::from(cut.is_some() && flags & 1 == 0);
        reached[2] += u32::from(cut.is_none() && flags & 1 != 0 && lock != 0);
        reached[3] += u32::from(cut.is_none() && flags & 3 == 2 && lock == 0);
        reached[4] += u32::from(natural_end.is_some() && cut.is_none());
        reached[5] += u32::from(natural_end.is_some_and(|(_, s)| s == 0));
        reached[6] += u32::from(natural_end.is_some() && cut.is_some());
    }
    assert!(reached.iter().all(|&n| n > 200), "{reached:?}");
}
