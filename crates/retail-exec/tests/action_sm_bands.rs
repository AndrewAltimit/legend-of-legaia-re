//! The battle action state machine `FUN_801E295C` (PROT 0898), its summon,
//! run and capture bands, the cast begin and the action seed, run whole from
//! its entry one pass at a time against `battle_action::step`, over the
//! harness `action_sm` builds (the context, the eight actors, every call out
//! of the SM stubbed).

use crate::action_sm::{CTX, FRAME_STEP, Host, Retail, S7, actor, host, lcg};
use crate::common::Rng;
use legaia_engine_battle_vm::battle_action::{self as ba, BattleActionCtx};
use std::cell::RefCell;
use std::rc::Rc;

const SM: u32 = 0x801E_295C;
const POSE: u32 = 0x801D_5854;
const DRIVER: u32 = 0x801D_C0A0;
const SFX: u32 = 0x8004_FCC8;
const FADE: u32 = 0x8002_4E80;
const DRIVE: u32 = 0x8003_DE7C;
const STAGER: u32 = 0x801F_1ED4;
const CAPTURE_TICK: u32 = 0x801F_2160;
const CLEARED: u32 = 0x8003_F2B8;
const RAND: u32 = 0x8005_6798;
const ELEMENT: u32 = 0x801D_8DE8;
const AUDIO_LIVE: u32 = 0x8007_B910;
const AUDIO_MASTER: u32 = 0x8008_457C;
const FADE_NODE: u32 = 0x8017_0000;
const CREATURE_NODE: u32 = 0x8017_0100;

/// What one retail pass called out to, in order.
#[derive(Default)]
struct Calls {
    poses: Vec<(u8, u8)>,
    /// `FUN_801DC0A0` and the one-shot cue, as `action_sm`'s `calls` logs them.
    calls: Vec<(u32, u32, u32)>,
    fades: u32,
    ui: Vec<(u8, u8)>,
}

/// Hook the calls the bands make: the framing / pose call, the cast-effect
/// driver (whose case `0x12` raises `ctx[+0x243]`, as the port's close-up
/// models it), the cue, the fade spawner and the element scheduler.
fn hook_calls(r: &mut Retail) -> Rc<RefCell<Calls>> {
    let log = Rc::new(RefCell::new(Calls::default()));
    let l = log.clone();
    r.cpu.hook(POSE, move |regs, _| {
        l.borrow_mut()
            .poses
            .push((regs.r[4] as u8, regs.r[5] as u8))
    });
    let l = log.clone();
    r.cpu.hook(DRIVER, move |regs, m| {
        if regs.r[5] == 0x12 {
            m.wr8(CTX + 0x243, 1);
        }
        l.borrow_mut().calls.push((DRIVER, regs.r[4], regs.r[5]));
    });
    let l = log.clone();
    r.cpu.hook(SFX, move |regs, _| {
        l.borrow_mut().calls.push((SFX, regs.r[4], 0))
    });
    let l = log.clone();
    r.cpu.hook(FADE, move |regs, _| {
        l.borrow_mut().fades += 1;
        regs.r[2] = FADE_NODE;
    });
    let l = log.clone();
    r.cpu.hook(ELEMENT, move |regs, _| {
        l.borrow_mut().ui.push((regs.r[4] as u8, regs.r[5] as u8))
    });
    log
}

/// One seat's state as the summon and capture bands read and write it.
#[derive(Clone, Copy, Debug)]
struct Seat {
    hp: u16,
    clip: u8,
    prim: u32,
    hidden: u8,
}

fn seat(rng: &mut Rng) -> Seat {
    Seat {
        hp: [0u16, 0, 40][rng.below(3) as usize],
        clip: [0u8, 0, 3, 8][rng.below(4) as usize],
        prim: [0u32, 0x2008_0200][rng.below(2) as usize],
        hidden: [0u8, 2, 0xFF][rng.below(3) as usize],
    }
}

/// Seat `seats` on both sides: retail's actor bytes and the port's actors.
fn seat_both(r: &mut Retail, h: &mut Host, seats: &[Seat; 8], monsters: u8) {
    for (s, st) in seats.iter().enumerate() {
        // An unseated monster seat holds nothing.
        let seated = s < 3 || s < 3 + usize::from(monsters);
        let st = if seated {
            *st
        } else {
            Seat {
                hp: 0,
                clip: 0,
                prim: 0,
                hidden: st.hidden,
            }
        };
        let a = actor(s as u8);
        let m = &mut r.cpu.mem;
        m.wr16(a + 0x14C, st.hp);
        m.wr8(a + 0x1D9, st.clip);
        m.wr32(a + 4, st.prim);
        m.wr8(a + 0x21C, st.hidden);
        let b = &mut h.actors[s];
        b.hp = st.hp;
        b.liveness = st.hp;
        b.max_hp = if (3..3 + usize::from(monsters)).contains(&s) {
            500
        } else {
            0
        };
        b.current_anim = st.clip;
        b.render_color = st.prim;
        b.render_flag = st.hidden;
    }
    r.cpu.mem.wr8(CTX + 1, monsters);
}

/// The summon band, states `0x32` to `0x38` (`0x801E4980..0x801E4EF8`),
/// against `summon_invoke` .. `summon_done`: the drive hold, the frame index
/// and the staging bytes, the target store, the cue wait and the flash-in,
/// the clip wait, the hide and restore loops over seats `0..7`, the band
/// word's drain and streaming clamp with the audio duck, the stager holds,
/// and `0x37`'s settle wait on its target or on the monster seats.
#[test]
fn the_summon_band_is_retails() {
    differential!(prot(898, SM));
    let Some(mut r) = Retail::new() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let log = hook_calls(&mut r);
    let mut rng = Rng(0x5355_4D4E);
    let mut seen = std::collections::HashSet::new();
    let (mut held37, mut left37) = (0, 0);
    for round in 0..40000 {
        let state = 0x32 + rng.below(7) as u8;
        let slot = [0u8, 1, 2, 2, 4][rng.below(5) as usize];
        let dt = 1 + rng.below(3) as u8;
        let timer = [-3i16, -1, 0, 1, 2, 5, 0x78][rng.below(7) as usize];
        let spell = [0x81u8, 0x8B, 0x96, 0x98, 0x99, 0x9A, 0xA0][rng.below(7) as usize];
        let target = [0u8, 1, 3, 4, 5, 6, 8, 9][rng.below(8) as usize];
        let drive = rng.below(3) == 0;
        let busy = rng.below(3) == 0;
        let cue = rng.below(2) as u8;
        let flags = rng.below(4) as u8;
        let iter = rng.below(3) as u8;
        let menu = rng.below(2) as u8;
        let staging = rng.below(2) as u8;
        let monsters = 1 + rng.below(4) as u8;
        let live = 0x40 + rng.below(0x40);
        let master = 0x40 + rng.below(0x40);
        let mut seats = [Seat {
            hp: 0,
            clip: 0,
            prim: 0,
            hidden: 0,
        }; 8];
        for st in seats.iter_mut() {
            *st = seat(&mut rng);
        }
        let mut h = host();
        r.reset();
        r.cpu.stub(DRIVE, u32::from(drive));
        r.cpu.stub(STAGER, u32::from(busy));
        *log.borrow_mut() = Calls::default();
        seat_both(&mut r, &mut h, &seats, monsters);
        let m = &mut r.cpu.mem;
        let a = actor(slot);
        m.wr8(FRAME_STEP, dt);
        m.wr8(CTX + 7, state);
        m.wr8(CTX + 0x13, slot);
        m.wr8(CTX + 0xD, 2);
        m.wr8(CTX + 0x243, 0);
        m.wr8(CTX + 0x276, menu);
        m.wr8(CTX + 0x278, staging);
        m.wr8(CTX + 0x279, staging);
        m.wr32(CTX + 0x102C, FADE_NODE);
        m.wr16(CTX + 0x6D0, 0x600);
        m.wr16(S7 + 2, timer as u16);
        m.wr16(S7 + 6, 0x33);
        m.wr32(AUDIO_LIVE, live);
        m.wr32(AUDIO_MASTER, master);
        m.wr8(a + 0x1DC, flags);
        m.wr8(a + 0x1DD, target);
        m.wr8(a + 0x1DF, spell);
        m.wr8(a + 0x1F5, cue);
        m.wr8(a + 0x1FA, iter);
        m.wr32(actor(7) + 0x22C, CREATURE_NODE);
        r.pass();
        let m = &mut r.cpu.mem;
        let retail = (
            m.rd8(CTX + 7),
            m.rd16(S7 + 2) as i16,
            m.rd16(S7 + 6) as i16,
            m.rd8(CTX + 0xD),
            m.rd8(CTX + 0x243),
            m.rd8(CTX + 0x276),
            m.rd8(CTX + 0x277),
            m.rd8(CTX + 0x278),
            m.rd8(CTX + 0x279),
            m.rd16(CTX + 0x6D0) as i16,
            m.rd32(CTX + 0x890),
            (m.rd8(a + 0x1DC), m.rd8(a + 0x1FA)),
        );
        let retail_seats: Vec<(u8, u32)> = (0..7u8)
            .map(|s| (m.rd8(actor(s) + 0x21C), m.rd32(actor(s) + 4)))
            .collect();
        let retail_live = m.rd32(AUDIO_LIVE);

        h.dt = i16::from(dt);
        h.drive_busy = drive;
        h.stager_busy = busy;
        let b = &mut h.actors[usize::from(slot)];
        b.flag_bits = ba::ActorFlags(flags);
        b.active_target = target;
        b.params[0] = spell;
        b.anim_cue = cue;
        b.spell_iter = iter;
        let mut ctx = BattleActionCtx {
            action_state: state,
            active_actor: slot,
            frame_timer: timer,
            damage_target: 0x33,
            camera_variant: 2,
            menu_open: menu,
            summon_staging_a: staging,
            summon_staging_b: staging,
            camera_frame_height: 0x600,
            ..Default::default()
        };
        ba::step(&mut h, &mut ctx);
        let b = &h.actors[usize::from(slot)];
        let port = (
            ctx.action_state,
            ctx.frame_timer,
            ctx.damage_target,
            ctx.camera_variant,
            ctx.gauge_rearm_latch,
            ctx.menu_open,
            ctx.summon_frame_idx,
            ctx.summon_staging_a,
            ctx.summon_staging_b,
            ctx.camera_frame_height,
            ctx.ambient_base,
            (b.flag_bits.0, b.spell_iter),
        );
        let port_seats: Vec<(u8, u32)> = h.actors[..7]
            .iter()
            .map(|b| (b.render_flag, b.render_color))
            .collect();
        let at = format!(
            "round {round}: state {state:#x} slot {slot} dt {dt} timer {timer} spell {spell:#x} \
             target {target} drive {drive} busy {busy} cue {cue} menu {menu} monsters \
             {monsters} seats {seats:x?}"
        );
        // Fields the port does not model as retail stores them: the frame
        // index and staging bytes only `0x32` writes, the ambient only the
        // `0x37` exit writes.
        let mut retail = retail;
        let mut port = port;
        if state != 0x32 {
            retail.6 = 0;
            port.6 = 0;
        }
        if retail.0 != 0x38 {
            retail.10 = 0;
            port.10 = 0;
        }
        assert_eq!(retail, port, "{at}");
        // `0x36` restores `+0x21C` but its `+0x08` mode word is not ported
        // (see `magic_capture_finalize`), so the prim words compare only
        // where the band hides.
        assert_eq!(retail_seats, port_seats, "seats, {at}");
        let log = log.borrow();
        assert_eq!(log.poses, h.poses, "poses, {at}");
        assert_eq!(log.calls, h.calls, "calls, {at}");
        assert_eq!(log.fades, h.fades, "fades, {at}");
        if state == 0x35 {
            // The duck floors the live level at the percent of the master
            // the port asks for (`0x801E4BC4..0x801E4C4C`).
            let [pct] = h.ducks[..] else {
                panic!("ducks {:?}, {at}", h.ducks)
            };
            let floor = master * u32::from(pct) / 100;
            assert_eq!(retail_live, (live - u32::from(dt)).max(floor), "duck, {at}");
        }
        if state == 0x37 {
            if retail.0 == 0x37 {
                held37 += 1;
            } else {
                left37 += 1;
            }
        }
        seen.insert((state, retail.0));
    }
    assert!(held37 > 500 && left37 > 500, "{held37} {left37}");
    assert!(seen.len() >= 13, "{seen:x?}");
}

/// The run band and the monster's flight, states `0x64` to `0x6B`
/// (`0x801E57C8..0x801E5C68`), against `run_begin` .. `capture_end`: the
/// outcome line, the party's turn skip and the escape's revive, the failed
/// run's press skip, the end signal, the flight's coin and takedown, the
/// captured marks and the streaming clamp.
#[test]
fn the_run_band_is_retails() {
    differential!(prot(898, SM));
    let Some(mut r) = Retail::new() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let log = hook_calls(&mut r);
    let stream = Rc::new(std::cell::Cell::new(0u32));
    let s2 = stream.clone();
    r.cpu.hook(RAND, move |regs, _| {
        s2.set(lcg(s2.get()));
        regs.r[2] = (s2.get() >> 16) & 0x7FFF;
    });
    // The flight's takedown `FUN_801E7824` has its own differential
    // (`capture_takedown`); here it stays a stub on both sides.
    let takedowns = Rc::new(RefCell::new(Vec::new()));
    let t2 = takedowns.clone();
    r.cpu.hook(0x801E_7824, move |regs, _| {
        t2.borrow_mut().push(regs.r[4] as u8)
    });
    let reorders = Rc::new(std::cell::Cell::new(0u32));
    let o2 = reorders.clone();
    r.cpu.hook(0x801D_ABA4, move |_, _| o2.set(o2.get() + 1));
    const OUTCOME: u32 = 0x8007_726C;
    const PRESSED: u32 = 0x8007_B874;
    const HELD: u32 = 0x8007_B938;
    const ROSTER: u32 = 0x8007_BD10;
    let mut rng = Rng(0x5255_4E21);
    let mut seen = std::collections::HashSet::new();
    for round in 0..30000 {
        let state = [0x64u8, 0x65, 0x66, 0x68, 0x69, 0x6A, 0x6B][rng.below(7) as usize];
        let party = 1 + rng.below(3) as u8;
        let slot = if matches!(state, 0x64..=0x66) {
            rng.below(u32::from(party)) as u8
        } else {
            3 + rng.below(4) as u8
        };
        let dt = 1 + rng.below(3) as u8;
        let timer = [0i16, 1, 2, 5, 0x3C][rng.below(5) as usize];
        let escaped = rng.below(2) == 0;
        let pad = [0u32, 0, 0x40][rng.below(3) as usize];
        let held = [0u32, 0, 0x20][rng.below(3) as usize];
        let menu = rng.below(2) as u8;
        let cursor = rng.below(4) as u8;
        let clock = rng.below(0x1000) as u16;
        let seed = rng.next();
        let mut hp = [0u16; 8];
        let mut keys = [0u16; 8];
        let mut roster = [0u8; 8];
        for s in 0..8 {
            hp[s] = [0u16, 0, 30][rng.below(3) as usize];
            keys[s] = [0u16, 7][rng.below(2) as usize];
            roster[s] = if s < 3 {
                [1u8, 2, 3, 4][rng.below(4) as usize]
            } else {
                4
            };
        }

        let mut h = host();
        r.reset();
        stream.set(seed);
        takedowns.borrow_mut().clear();
        reorders.set(0);
        *log.borrow_mut() = Calls::default();
        let m = &mut r.cpu.mem;
        m.wr8(FRAME_STEP, dt);
        m.wr8(CTX, party);
        m.wr8(CTX + 7, state);
        m.wr8(CTX + 0xD, 3);
        m.wr8(CTX + 0x13, slot);
        m.wr8(CTX + 0x1A, cursor);
        m.wr8(CTX + 0x276, menu);
        m.wr16(S7 + 2, timer as u16);
        m.wr16(S7 + 4, clock);
        m.wr32(OUTCOME, if escaped { CTX + 0x159 } else { CTX + 0x189 });
        m.wr32(PRESSED, pad);
        m.wr32(HELD, held);
        for s in 0..8u8 {
            let a = actor(s);
            m.wr16(a + 0x14C, hp[usize::from(s)]);
            m.wr16(a + 0x16C, keys[usize::from(s)]);
            m.wr8(a + 0x225, 0);
            m.wr8(a + 0x21C, 0);
            m.wr32(a + 4, 0x2008_0200);
            m.wr8(ROSTER + u32::from(s), roster[usize::from(s)]);
        }
        r.pass();
        let m = &mut r.cpu.mem;
        // The SM's prologue steps the `ctx[+0x6DA]` clock by `dt * 4 / 3`
        // (at least one) every pass (`0x801E29E4..0x801E2A24`), which the
        // port does not keep; take it back out.
        let bump = (u16::from(dt) * 4 / 3).max(1);
        let retail = (
            m.rd8(CTX + 7),
            m.rd16(S7 + 2) as i16,
            m.rd16(S7 + 4).wrapping_sub(bump),
            m.rd8(CTX + 0xD),
            m.rd8(CTX + 0x18),
            m.rd8(CTX + 0x1A),
        );
        let retail_actors: Vec<(u16, u16, u8, u8, u32)> = (0..8u8)
            .map(|s| {
                let a = actor(s);
                (
                    m.rd16(a + 0x14C),
                    m.rd16(a + 0x16C),
                    m.rd8(a + 0x225),
                    m.rd8(a + 0x21C),
                    m.rd32(a + 4),
                )
            })
            .collect();
        let ended = m.rd8(0x8007_BD71) == 0xFE;

        struct Run<'a> {
            h: &'a mut Host,
            party: u8,
            roster: [u8; 8],
        }
        impl ba::BattleActionHost for Run<'_> {
            fn actor(&self, slot: u8) -> Option<&ba::BattleActor> {
                self.h.actor(slot)
            }
            fn actor_mut(&mut self, slot: u8) -> Option<&mut ba::BattleActor> {
                self.h.actor_mut(slot)
            }
            fn ui_element(&mut self, id: u8, mode: u8) {
                self.h.ui_element(id, mode)
            }
            fn frame_dt(&self) -> i16 {
                self.h.frame_dt()
            }
            fn pad_word(&self) -> u16 {
                self.h.pad_word()
            }
            fn pad_held_32(&self) -> u16 {
                self.h.pad_held_32()
            }
            fn party_count(&self) -> u8 {
                self.party
            }
            fn roster_character_id(&self, slot: u8) -> u8 {
                self.roster[usize::from(slot)]
            }
            fn rng(&mut self) -> u32 {
                self.h.rng()
            }
            fn pose(&mut self, id: u8, pose: ba::Pose) {
                self.h.pose(id, pose)
            }
            fn battle_end(&mut self, cause: ba::BattleEndCause) {
                self.h.battle_end(cause)
            }
            fn recompute_battle_order(&mut self) {
                self.h.recompute_battle_order()
            }
            fn monster_action_tags(&self, _slot: u8) -> Option<Vec<u8>> {
                None
            }
        }
        h.dt = i16::from(dt);
        h.pad = pad as u16;
        h.held = held as u16;
        h.rand = seed;
        for s in 0..8 {
            let b = &mut h.actors[s];
            b.hp = hp[s];
            b.liveness = hp[s];
            b.init_key = keys[s];
            b.render_color = 0x2008_0200;
        }
        let mut ctx = BattleActionCtx {
            action_state: state,
            active_actor: slot,
            frame_timer: timer,
            combo_timer: clock as i16,
            camera_variant: 3,
            turn_cursor: cursor,
            menu_open: menu,
            // The port carries the run's outcome here (`run.rs`).
            absorbed_seru: u8::from(escaped),
            ..Default::default()
        };
        // The flight's takedown is stubbed on the retail side: hold the
        // port's to the same by running it against a scratch copy.
        let mut run = Run {
            h: &mut h,
            party,
            roster,
        };
        let kept = run.h.actors.clone();
        let out = ba::step(&mut run, &mut ctx);
        if state == 0x68 {
            // Undo `capture_takedown`'s writes to the fleeing seat, which
            // retail's stubbed `FUN_801E7824` does not make here.
            let s = usize::from(slot);
            h.actors[s] = ba::BattleActor {
                capture_state: h.actors[s].capture_state,
                render_flag: h.actors[s].render_flag,
                render_color: h.actors[s].render_color,
                ..kept[s].clone()
            };
            h.ui.retain(|&(id, _)| id != 0x43);
        }
        let port_actors: Vec<(u16, u16, u8, u8, u32)> = h
            .actors
            .iter()
            .map(|b| {
                (
                    b.hp,
                    b.init_key,
                    b.capture_state,
                    b.render_flag,
                    b.render_color,
                )
            })
            .collect();
        // `0x66` leaves for the unmapped `0x67` with the end signal raised;
        // the port ends the battle in the same pass.
        let port_state = if out == ba::StepOutcome::BattleComplete {
            0x67
        } else {
            ctx.action_state
        };
        let port = (
            port_state,
            ctx.frame_timer,
            ctx.combo_timer as u16,
            ctx.camera_variant,
            ctx.action_ui_element,
            ctx.turn_cursor,
        );
        let at = format!(
            "round {round}: state {state:#x} party {party} slot {slot} dt {dt} timer {timer} \
             escaped {escaped} pad {pad:#x}/{held:#x} menu {menu} hp {hp:?} keys {keys:?} \
             roster {roster:?}"
        );
        assert_eq!(retail, port, "{at}");
        assert_eq!(retail_actors, port_actors, "actors, {at}");
        assert_eq!(ended, !h.ended.is_empty(), "end, {at}");
        let log = log.borrow();
        assert_eq!(log.poses, h.poses, "poses, {at}");
        assert_eq!(log.ui, h.ui, "elements, {at}");
        assert_eq!(log.fades, u32::from(state == 0x66), "fades, {at}");
        assert_eq!(
            *takedowns.borrow(),
            if state == 0x68 { vec![slot] } else { vec![] },
            "takedown, {at}"
        );
        assert_eq!(reorders.get(), h.reordered, "reorder, {at}");
        seen.insert((state, retail.0));
    }
    assert!(seen.len() >= 12, "{seen:x?}");
}

/// The capture band, states `0x6E` to `0x71` (`0x801E4EFC..0x801E52A0`),
/// against `magic_capture_branch` .. `magic_capture_finalize`: the drive
/// hold, the fade-in's duck and camera pull-in, the clear wait, the module
/// hold and its style pin, and `0x71`'s settle test with its restore walk.
#[test]
fn the_capture_band_is_retails() {
    differential!(prot(898, SM));
    let Some(mut r) = Retail::new() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let log = hook_calls(&mut r);
    let mut rng = Rng(0x4341_5054);
    let mut seen = std::collections::HashSet::new();
    let (mut held71, mut left71) = (0, 0);
    for round in 0..30000 {
        let state = 0x6E + rng.below(4) as u8;
        let slot = rng.below(7) as u8;
        let dt = 1 + rng.below(3) as u8;
        let target = [0u8, 1, 2, 3, 4, 6, 8, 9][rng.below(8) as usize];
        // A full party, where retail's pool numbering and the engine's
        // compact seating agree (`battle_slot`).
        let party = 3;
        let scripted = rng.below(2) as u8;
        let drive = rng.below(3) == 0;
        let cleared = rng.below(2) == 0;
        let busy = rng.below(3) == 0;
        let height = [0x200u16, 0x600, 0x800][rng.below(3) as usize];
        let live = 0x40 + rng.below(0x40);
        let master = 0x40 + rng.below(0x40);
        let mut seats = [Seat {
            hp: 0,
            clip: 0,
            prim: 0,
            hidden: 0,
        }; 8];
        for st in seats.iter_mut() {
            *st = seat(&mut rng);
        }

        let mut h = host();
        r.reset();
        r.cpu.stub(DRIVE, u32::from(drive));
        r.cpu.stub(CLEARED, u32::from(!cleared));
        r.cpu.stub(CAPTURE_TICK, u32::from(busy));
        *log.borrow_mut() = Calls::default();
        seat_both(&mut r, &mut h, &seats, 2);
        let m = &mut r.cpu.mem;
        let a = actor(slot);
        m.wr8(FRAME_STEP, dt);
        m.wr8(CTX, party);
        m.wr8(CTX + 7, state);
        m.wr8(CTX + 0xD, 2);
        m.wr8(CTX + 0x13, slot);
        m.wr8(CTX + 0x243, 1);
        m.wr8(CTX + 0x278, 1);
        m.wr8(CTX + 0x279, 1);
        m.wr8(CTX + 0x287, scripted);
        m.wr16(CTX + 0x6D0, height);
        m.wr32(AUDIO_LIVE, live);
        m.wr32(AUDIO_MASTER, master);
        m.wr8(a + 0x1DD, target);
        r.pass();
        let m = &mut r.cpu.mem;
        let retail = (
            m.rd8(CTX + 7),
            m.rd8(CTX + 0xD),
            m.rd8(CTX + 0x243),
            m.rd8(CTX + 0x278),
            m.rd8(CTX + 0x279),
        );
        let retail_height = m.rd16(CTX + 0x6D0);
        let retail_flags: Vec<u8> = (0..8u8).map(|s| m.rd8(actor(s) + 0x21C)).collect();
        let retail_live = m.rd32(AUDIO_LIVE);

        struct Capture<'a> {
            h: &'a mut Host,
            party: u8,
            cleared: bool,
        }
        impl ba::BattleActionHost for Capture<'_> {
            fn actor(&self, slot: u8) -> Option<&ba::BattleActor> {
                self.h.actor(slot)
            }
            fn actor_mut(&mut self, slot: u8) -> Option<&mut ba::BattleActor> {
                self.h.actor_mut(slot)
            }
            fn frame_dt(&self) -> i16 {
                self.h.frame_dt()
            }
            fn party_count(&self) -> u8 {
                self.party
            }
            fn pose(&mut self, id: u8, pose: ba::Pose) {
                self.h.pose(id, pose)
            }
            fn sound_bank_ready(&self, arg: u8) -> bool {
                self.h.sound_bank_ready(arg)
            }
            fn previous_action_cleared(&self, _arg: u8) -> bool {
                self.cleared
            }
            fn capture_stager_tick(&mut self) -> bool {
                self.h.capture_stager_tick()
            }
            fn capture_exit_clears_style(&self) -> bool {
                // The stubbed module leaves the style as `0x70` pinned it.
                false
            }
            fn duck_audio_level(&mut self, pct: u8) {
                self.h.duck_audio_level(pct)
            }
            fn is_capture_spell(&self, _id: u8) -> bool {
                true
            }
        }
        h.dt = i16::from(dt);
        h.drive_busy = drive;
        h.capture_busy = busy;
        h.actors[usize::from(slot)].active_target = target;
        let mut ctx = BattleActionCtx {
            action_state: state,
            active_actor: slot,
            camera_variant: 2,
            gauge_rearm_latch: 1,
            summon_staging_a: 1,
            summon_staging_b: 1,
            scripted_fight: scripted,
            camera_frame_height: height as i16,
            ..Default::default()
        };
        let mut cap = Capture {
            h: &mut h,
            party,
            cleared,
        };
        ba::step(&mut cap, &mut ctx);
        let port = (
            ctx.action_state,
            ctx.camera_variant,
            ctx.gauge_rearm_latch,
            ctx.summon_staging_a,
            ctx.summon_staging_b,
        );
        let port_flags: Vec<u8> = h.actors.iter().map(|b| b.render_flag).collect();
        let at = format!(
            "round {round}: state {state:#x} slot {slot} dt {dt} target {target} party \
             {party} scripted {scripted} drive {drive} cleared {cleared} busy {busy} seats \
             {seats:x?}"
        );
        assert_eq!(retail, port, "{at}");
        assert_eq!(retail_flags, port_flags, "seats, {at}");
        // `0x6F` pulls the depth in by the frame step times sixteen; the
        // other exits re-seed it through `FUN_801F0348`, stubbed here.
        if state == 0x6F {
            assert_eq!(retail_height as i16, ctx.camera_frame_height, "depth, {at}");
        }
        if matches!(state, 0x6F | 0x70) {
            let floor = master * 75 / 100;
            if scripted != 0 {
                assert_eq!(h.ducks, vec![75], "{at}");
                assert_eq!(retail_live, (live - u32::from(dt)).max(floor), "duck, {at}");
            } else {
                assert!(h.ducks.is_empty(), "{at}");
                assert_eq!(retail_live, live, "duck, {at}");
            }
        }
        let log = log.borrow();
        assert_eq!(log.poses, h.poses, "poses, {at}");
        if state == 0x71 {
            if retail.0 == 0x71 {
                held71 += 1;
            } else {
                left71 += 1;
            }
        }
        seen.insert((state, retail.0));
    }
    assert!(held71 > 300 && left71 > 300, "{held71} {left71}");
    assert!(seen.len() >= 8, "{seen:x?}");
}
