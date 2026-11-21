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

/// The tick's moves and walks against the World seams that port them, one
/// arm a round: the event cut's pro-rated `+0x0E` step (`0x80047950..
/// 0x80047A28`, `motion::event_cut_root_step`), the natural end's full step
/// and its gates (`0x80047A68..0x80047B2C`, `motion::natural_end_steps`), the
/// Seru-absorb puff behind it (`0x80047B5C..0x80047BC0`,
/// `World::absorb_puff_plan`), the in-range back-off (`0x80047C3C..
/// 0x80047D1C`, `World::drive_in_range_backoff_with`), the signed root-motion
/// term (`0x80047D20..0x80047E18`, `World::drive_playing_root_motion_with`)
/// and the walk gates on `DAT_8007BD71` (`World::battle_node_walk_gates`).
/// The range poll `FUN_8004E2F0` is hooked on both sides with one answer.
#[test]
fn the_tick_moves_and_walks_where_retail_does() {
    differential!(scus(TICK));
    use legaia_actor_formats::monster_archive::parse_animation_stream;
    use legaia_engine_battle::battle_anim::MonsterAnimPlayer;
    use legaia_engine_battle_loop::round_flow::RoundPhase;
    use legaia_engine_battle_vm::battle_action::motion::{
        end_root_step, event_cut_root_step, natural_end_steps,
    };
    use legaia_engine_battle_vm::battle_action::{ActorFlags, BattleEndCause};
    use legaia_engine_core::world::World;
    use legaia_engine_core::world::arm_drivers::BattleArmDrivers;
    use legaia_engine_system::trig::{sin12, trig12};
    use psx_retail_exec::{A0, A1, A2};
    use std::cell::Cell;
    const RANGE: u32 = 0x8004_E2F0;
    const SPAWN: u32 = 0x801D_FDF0;
    const CUE: u32 = 0x8004_FCC8;
    const SIGNAL: u32 = 0x8007_BD71;
    const LATCH: u32 = 0x801F_698C;
    const SIN_PTR: u32 = 0x8007_B81C;
    const COS_PTR: u32 = 0x8007_B7F8;
    const SIN: u32 = 0x8016_0000;
    const COS: u32 = 0x8016_4000;
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.stub_outside = Some((TICK, 0x8004_8108));
    cpu.step_budget = 200_000;
    cpu.mem.wr32(CTX_PTR, CTX);
    cpu.mem.wr32(SIN_PTR, SIN);
    cpu.mem.wr32(COS_PTR, COS);
    for a in 0..0x1400u32 {
        cpu.mem.wr16(SIN + a * 2, sin12(a as u16) as u16);
        cpu.mem
            .wr16(COS + a * 2, sin12((a as u16).wrapping_add(0x400)) as u16);
    }
    let committed = Rc::new(Cell::new(0u8));
    let c = committed.clone();
    let actor_at = Rc::new(Cell::new(ACTOR));
    let at = actor_at.clone();
    cpu.hook(COMMIT, move |r, m| {
        m.wr16(r.r[A0] + 0x68, 0);
        m.wr32(r.r[A0] + 0x4C, NEXT_ENTRY);
        m.wr8(at.get() + 0x1D9, c.get());
    });
    let range = Rc::new(Cell::new(0u32));
    let rg = range.clone();
    cpu.hook(RANGE, move |r, _| r.r[psx_retail_exec::V0] = rg.get());
    type Spawns = Vec<(u32, [i16; 3], u32)>;
    let spawns: Rc<RefCell<Spawns>> = Rc::new(RefCell::new(Vec::new()));
    let sp = spawns.clone();
    cpu.hook(SPAWN, move |r, m| {
        let p = r.r[A1];
        sp.borrow_mut().push((
            r.r[A0],
            [m.rd16(p) as i16, m.rd16(p + 2) as i16, m.rd16(p + 4) as i16],
            r.r[A2],
        ));
    });
    let cues: Rc<RefCell<Vec<u32>>> = Rc::new(RefCell::new(Vec::new()));
    let cu = cues.clone();
    cpu.hook(CUE, move |r, _| cu.borrow_mut().push(r.r[A0]));

    let mut w = World::new();
    w.load_party(legaia_save::Party::zeroed(3));
    w.enter_battle(3, 2);
    w.party.party_count = 3;
    let mut rng = Rng(0x4D4F_5645);
    let mut seen = [0u32; 6];
    for round in 0..30_000 {
        let arm = rng.below(6);
        let seat = if rng.below(3) == 0 { 3 } else { 0 };
        let actor = ACTOR + seat * 0x400;
        actor_at.set(actor);
        cpu.mem.wr32(ACTORS + seat * 4, actor);
        cpu.mem.wr8(SEATS + seat, if seat < 3 { 1 } else { 4 });
        let frames = 4 + rng.below(12) as u8;
        let step = rng.below(0x300) as i16 - 0x180;
        let speed = rng.below(0x20) as i16 - 0x10;
        let facing = rng.below(0x1000) as u16;
        let dt = 1 + rng.below(3) as u8;
        let rate = [0u8, 0, 4, 8][rng.below(4) as usize];
        let hp = if rng.below(3) == 0 {
            0
        } else {
            1 + rng.below(500) as u16
        };
        let seru = if rng.below(2) == 0 {
            0
        } else {
            1 + rng.below(0x20) as u8
        };
        let latch_up = rng.below(2) == 0;
        let r = [0u32, 0, 1, 2][rng.below(4) as usize];
        let signal = [0xFFu8, 0xFF, 0xFE][rng.below(3) as usize];
        let flow = if rng.below(2) == 0 { 0xFF } else { 0 };
        let category = [3u8, 3, 1, 2][rng.below(4) as usize];
        let acting = if rng.below(4) == 0 { 1 } else { seat as u8 };
        let mut flags = rng.below(16) as u8;
        // Each arm's own inputs; the others stand neutral.
        let (cursor, entry_step, entry_speed, live_rate) = match arm {
            0 => {
                flags = (flags & !3) | 1;
                (
                    u16::from(rng.below(u32::from(frames)) as u8) << 4,
                    step,
                    0,
                    0,
                )
            }
            1 | 2 => {
                flags &= !3;
                (u16::from(frames) << 4, step, 0, 0)
            }
            _ => {
                flags &= !3;
                (0, 0, speed, rate.max(1))
            }
        };
        let back_off = arm == 3;
        cpu.mem.wr16(NODE + 0x5A, seat as u16);
        cpu.mem.wr32(NODE + 0x74, 0x10);
        cpu.mem.wr32(NODE + 0x4C, ENTRY);
        cpu.mem.wr16(NODE + 0x68, cursor);
        for (entry, stream, n) in [(ENTRY, STREAM, frames), (NEXT_ENTRY, NEXT_STREAM, 0x40)] {
            for k in 0..0x8C {
                cpu.mem.wr8(entry + k, 0);
            }
            cpu.mem.wr8(entry + 0x78, 1);
            cpu.mem.wr32(entry + 0x88, stream);
            cpu.mem.wr8(stream, 1);
            cpu.mem.wr8(stream + 1, n);
        }
        cpu.mem.wr16(ENTRY + 0xE, entry_step as u16);
        cpu.mem.wr16(ENTRY + 0xC, entry_speed as u16);
        // Every event frame far out, so no gate cut on bit 1.
        cpu.mem.wr8(ENTRY + 0x10, 0x7F);
        cpu.mem.wr8(ACTOR + 0x21D, 0);
        cpu.mem.wr8(actor + 0x21D, live_rate);
        cpu.mem.wr16(actor + 0x176, 0);
        cpu.mem.wr8(actor + 0x21B, 0);
        cpu.mem.wr16(actor + 0x16E, 0);
        cpu.mem.wr16(actor + 0x14C, hp);
        cpu.mem.wr8(actor + 0x1D9, 1);
        cpu.mem.wr8(actor + 0x1DA, 2);
        cpu.mem.wr8(actor + 0x1DC, flags);
        cpu.mem.wr8(actor + 0x1DD, 4);
        cpu.mem
            .wr8(actor + 0x1DE, if back_off { category } else { 0 });
        cpu.mem.wr8(actor + 0x228, 0);
        cpu.mem.wr16(actor + 0x46, facing);
        cpu.mem.wr16(actor + 0x34, 0);
        cpu.mem.wr16(actor + 0x36, 0);
        cpu.mem.wr16(actor + 0x38, 0);
        cpu.mem.wr32(actor + 0x10, 0);
        cpu.mem.wr16(actor + 0x178, 0);
        cpu.mem.wr8(CTX + 0x06, if back_off { flow } else { 0 });
        cpu.mem.wr8(CTX + 0x13, acting);
        cpu.mem.wr8(CTX + 0x269, seru);
        cpu.mem.wr32(LATCH, u32::from(latch_up));
        cpu.mem.wr8(SIGNAL, if arm == 5 { signal } else { 0xFF });
        cpu.mem.wr8(FRAME_STEP, dt);
        committed.set(if rng.below(2) == 0 { 0 } else { 1 });
        range.set(r);
        spawns.borrow_mut().clear();
        cues.borrow_mut().clear();
        cpu.calls.clear();
        assert_eq!(cpu.call(TICK, &[NODE]), Stop::Returned, "{round}");
        let moved = (
            cpu.mem.rd16(actor + 0x34) as i16,
            cpu.mem.rd16(actor + 0x38) as i16,
        );

        let (sin, cos) = trig12(facing);
        let i = seat as usize;
        let port_moved: (i16, i16) = match arm {
            0 => {
                let (dx, dz) =
                    event_cut_root_step(sin, cos, entry_step, (cursor >> 4) as i16, frames);
                if entry_step != 0 {
                    (dx as i16, dz as i16)
                } else {
                    (0, 0)
                }
            }
            1 | 2 => {
                let steps = natural_end_steps(entry_step, flags, hp, seru != 0);
                let (dx, dz) = end_root_step(sin, cos, entry_step);
                if steps {
                    (dx as i16, dz as i16)
                } else {
                    (0, 0)
                }
            }
            3 | 4 => {
                let mut head = vec![0u8; 0x88];
                head[0x0C..0x0E].copy_from_slice(&entry_speed.to_le_bytes());
                head[0x78] = 1;
                let block = {
                    let mut b = vec![1u8, 0x40];
                    b.resize(2 + 0x40 * 9, 0);
                    b
                };
                let anim = parse_animation_stream(&block, 0, 1, 0, 0, head).expect("stream");
                let a = &mut w.actors[i];
                a.battle_animation = MonsterAnimPlayer::new_one_shot(&anim);
                a.battle.facing_angle = facing;
                a.battle.anim_rate = legaia_engine_battle_vm::battle_anim_rate::AnimRate(live_rate);
                a.battle.flag_bits = ActorFlags(flags);
                a.battle.action_category = if back_off { category } else { 0 };
                a.battle.active_target = 4;
                a.move_state.world_x = 0;
                a.move_state.world_z = 0;
                w.battle_ctx.active_actor = acting;
                w.battle.round_flow.phase = if back_off && flow == 0xFF {
                    RoundPhase::Execute
                } else {
                    RoundPhase::Command
                };
                // The pass backs off the acting seat alone
                // (`World::tick_battle_locomotion_dt`), then steps every
                // seat's root motion.
                if usize::from(acting) == i {
                    w.in_range_backoff(i, dt, r as u16);
                }
                w.playing_root_motion(i, dt, r as u16);
                let a = &w.actors[i];
                (a.move_state.world_x, a.move_state.world_z)
            }
            _ => (moved.0, moved.1),
        };
        assert_eq!(
            moved, port_moved,
            "round {round} arm {arm} seat {seat} step {entry_step} speed {entry_speed} facing \
             {facing:#x} dt {dt} rate {live_rate} flags {flags:#x} hp {hp} seru {seru} range {r} \
             flow {flow:#x} category {category} acting {acting}"
        );
        if arm == 2 {
            // The absorb puff, on the pair the natural end left.
            let a = &mut w.actors[i];
            a.battle.current_anim = committed.get();
            a.battle.hp = hp;
            a.move_state.world_x = moved.0;
            a.move_state.world_y = 0;
            a.move_state.world_z = moved.1;
            w.battle_ctx.absorbed_seru = seru;
            w.battle.absorb_cue_latch = latch_up;
            let plan = w.absorb_puff(i);
            let want_spawns: Vec<(u32, [i16; 3], u32)> =
                plan.iter().map(|&(p, _)| (0x14, p, 0)).collect();
            let want_cues: Vec<u32> = plan.iter().filter(|p| p.1).map(|_| 0x1B).collect();
            assert_eq!(
                (spawns.borrow().clone(), cues.borrow().clone()),
                (want_spawns, want_cues),
                "round {round} committed {} hp {hp} seru {seru} latch {latch_up}",
                committed.get()
            );
            seen[2] += u32::from(plan.is_some());
        }
        if arm == 5 {
            w.battle.end = (signal == 0xFE).then_some(BattleEndCause::MonsterWipe);
            let (hits_fx, cue_walk) = w.node_walk_gates();
            let retail = (
                cpu.calls.contains(&0x801E_C3E4) && cpu.calls.contains(&0x801D_EA50),
                cpu.calls.contains(&0x8005_08DC),
            );
            assert_eq!(
                retail,
                (hits_fx, cue_walk),
                "round {round} signal {signal:#x}"
            );
            w.battle.end = None;
        }
        seen[arm as usize] += u32::from(moved != (0, 0) || arm == 5);
    }
    assert!(seen.iter().all(|&n| n > 300), "{seen:?}");
}
