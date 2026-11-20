//! The battle action state machine `FUN_801E295C` (PROT 0898) run whole from
//! its entry, one pass per state, every call out of it stubbed, against
//! `battle_action::step` over a host that logs what the port asks of it.
//!
//! Each test seats one state's inputs on both sides - the context bytes the
//! state reads, the acting actor and its target - runs one pass, and compares
//! the context and actor bytes the state writes with the port's fields, and
//! the element scheduler calls (`FUN_801D8DE8(id, mode)`) with the host's
//! `ui_element` log.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_battle_vm::battle_action::{
    self as ba, BattleActionCtx, BattleActionHost, BattleActor,
};
use psx_retail_exec::{Cpu, Memory, Stop};
use std::cell::RefCell;
use std::rc::Rc;

const SM: u32 = 0x801E_295C;
const SM_END: u32 = 0x801E_6968;
const OVERLAY_BASE: u32 = 0x801C_E818;
const CTX_PTR: u32 = 0x8007_BD24;
const ACTORS: u32 = 0x801C_9370;
const CTX: u32 = 0x8012_0000;
const ACTOR: u32 = 0x8013_0000;
const ACTOR_STRIDE: u32 = 0x400;
const FRAME_STEP: u32 = 0x1F80_0393;
const ELEMENT: u32 = 0x801D_8DE8;
const SERU_GRANT: u32 = 0x801E_92DC;
const HP_SETTLE: u32 = 0x801E_7250;
const RAND: u32 = 0x8005_6798;
const WIN_POSE: u32 = 0x8005_5B4C;
const SEATS: u32 = 0x8007_BD10;
const FORMATION: u32 = 0x8007_BD0C;
const SPECIAL_WORD: u32 = 0x8007_BAC0;
const END_SIGNAL: u32 = 0x8007_BD71;
const END_CAUSE: u32 = 0x8007_BD2C;
/// `ctx + 0x6D6`, the band the SM keeps in `s7`.
const S7: u32 = CTX + 0x6D6;

/// The port's side: a host holding eight actors that logs each element call.
#[derive(Default)]
struct Host {
    actors: Vec<BattleActor>,
    ui: Vec<(u8, u8)>,
    learned: Vec<(u8, u8)>,
    dt: i16,
    pad: u16,
    held: u16,
    special: u32,
    first_monster: u8,
    ended: Vec<ba::BattleEndCause>,
    staged: Vec<u8>,
    rand: u32,
}

impl BattleActionHost for Host {
    fn actor(&self, slot: u8) -> Option<&BattleActor> {
        self.actors.get(usize::from(slot))
    }
    fn actor_mut(&mut self, slot: u8) -> Option<&mut BattleActor> {
        self.actors.get_mut(usize::from(slot))
    }
    fn ui_element(&mut self, effect_id: u8, mode: u8) {
        self.ui.push((effect_id, mode));
    }
    fn learn_absorbed_seru(&mut self, slot: u8, seru: u8) {
        self.learned.push((slot, seru));
    }
    fn frame_dt(&self) -> i16 {
        self.dt
    }
    fn pad_word(&self) -> u16 {
        self.pad
    }
    fn pad_held_32(&self) -> u16 {
        self.held
    }
    fn special_battle_word(&self) -> u32 {
        self.special
    }
    fn first_monster_id(&self) -> u8 {
        self.first_monster
    }
    fn battle_end(&mut self, cause: ba::BattleEndCause) {
        self.ended.push(cause);
    }
    fn victory_stage(&mut self, party_slot: u8) {
        self.staged.push(party_slot);
    }
    fn rng(&mut self) -> u32 {
        self.rand = lcg(self.rand);
        (self.rand >> 16) & 0x7FFF
    }
}

/// The C library's `rand()` step, which both sides draw from.
fn lcg(state: u32) -> u32 {
    state.wrapping_mul(0x41C6_4E6D).wrapping_add(0x3039)
}

/// The retail side: the overlay in, calls stubbed, the element scheduler and
/// the Seru grant logged.
struct Retail {
    cpu: Cpu,
    clean: Memory,
    ui: Rc<RefCell<Vec<(u8, u8)>>>,
    learned: Rc<RefCell<Vec<u8>>>,
}

impl Retail {
    fn new() -> Option<Self> {
        let mut cpu = scus_cpu()?;
        cpu.mem
            .load(OVERLAY_BASE, &disc()?.read_entry(898).expect("PROT 0898"));
        cpu.stub_outside = Some((SM, SM_END));
        let ui = Rc::new(RefCell::new(Vec::new()));
        let sink = ui.clone();
        cpu.hook(ELEMENT, move |r, _| {
            sink.borrow_mut().push((r.r[4] as u8, r.r[5] as u8))
        });
        let learned = Rc::new(RefCell::new(Vec::new()));
        let sink = learned.clone();
        cpu.hook(SERU_GRANT, move |r, _| sink.borrow_mut().push(r.r[4] as u8));
        let clean = cpu.mem.clone();
        Some(Self {
            cpu,
            clean,
            ui,
            learned,
        })
    }

    /// Fresh memory with the context and the eight actors seated.
    fn reset(&mut self) {
        self.cpu.mem = self.clean.clone();
        self.cpu.mem.wr32(CTX_PTR, CTX);
        for k in 0..8u32 {
            self.cpu.mem.wr32(ACTORS + 4 * k, actor(k as u8));
        }
        self.ui.borrow_mut().clear();
        self.learned.borrow_mut().clear();
    }

    fn pass(&mut self) {
        assert_eq!(self.cpu.call(SM, &[]), Stop::Returned);
    }
}

fn actor(slot: u8) -> u32 {
    ACTOR + u32::from(slot) * ACTOR_STRIDE
}

fn host() -> Host {
    Host {
        actors: vec![BattleActor::default(); 8],
        dt: 1,
        ..Default::default()
    }
}

/// State `0x51`, the Done band's countdown and its UI teardown
/// (`0x801E6044..0x801E6368`) against `done_fade_down`: the HP-bar hold, the
/// level-up banner's pad skip, the menu floor, the exit to `0x5A` or the
/// Seru band `0x52`, the once-per-action teardown latch and its unloads, the
/// Seru grant and the target banner's close.
#[test]
fn the_done_band_counts_down_and_tears_down_as_retail_does() {
    differential!(prot(898, SM));
    let Some(mut r) = Retail::new() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut rng = Rng(0x444F_4E45);
    let (mut left, mut torn, mut skipped, mut granted) = (0, 0, 0, 0);
    for round in 0..20000 {
        let slot = rng.below(8) as u8;
        let target = [0u8, 2, 3, 5, 7, 8, 9][rng.below(7) as usize];
        let category = rng.below(7) as u8;
        let timer =
            [-5i16, -1, 0, 1, 5, 11, 12, 13, 0x5A, 0x5B, 0x5C, 0x96][rng.below(12) as usize];
        let dt = 1 + rng.below(2) as u8;
        let banner = [0u8, 0, 0x2E][rng.below(3) as usize];
        let pad = [0u16, 0, 0x40][rng.below(3) as usize];
        let held = [0u16, 0, 0x20][rng.below(3) as usize];
        let menu = [0u8, 0, 1][rng.below(3) as usize];
        let seru = [0u8, 0, 0, 0x0B][rng.below(4) as usize];
        let latch = [0u8, 0, 1][rng.below(3) as usize];
        let element = [0u8, 6, 0x13, 0x30][rng.below(4) as usize];
        let spirit = [0u8, 0, 1][rng.below(3) as usize];
        let lone = rng.below(2) as u8;
        // A party readout still ramping holds the countdown (`FUN_801E7250`):
        // the port reads it off the target's shown HP.
        let target_hp = [0u16, 30][rng.below(2) as usize];
        let ramping = (target < 3 || target == 8) && rng.below(4) == 0;

        r.reset();
        r.cpu.stub(HP_SETTLE, u32::from(ramping));
        let m = &mut r.cpu.mem;
        m.wr8(FRAME_STEP, dt);
        m.wr8(CTX + 7, 0x51);
        m.wr8(CTX + 0x13, slot);
        m.wr8(CTX + 0x17, latch);
        m.wr8(CTX + 0x18, element);
        m.wr8(CTX + 0x19, spirit);
        m.wr8(CTX + 0x26, banner);
        m.wr8(CTX + 0x269, seru);
        m.wr8(CTX + 0x276, menu);
        m.wr8(CTX + 0x288, lone);
        m.wr16(S7 + 2, timer as u16);
        m.wr32(0x8007_B874, u32::from(pad));
        m.wr32(0x8007_B938, u32::from(held));
        m.wr8(actor(slot) + 0x1DD, target);
        m.wr8(actor(slot) + 0x1DE, category);
        if target < 8 {
            m.wr16(actor(target) + 0x14C, target_hp);
        }
        // The camera word the band steps and clamps (`0x801E5F98..0x801E5FBC`).
        m.wr32(0x8007_B910, [0u32, 0x7FFF_FFFF][rng.below(2) as usize]);
        r.pass();
        let m = &mut r.cpu.mem;
        let retail = (
            m.rd8(CTX + 7),
            m.rd16(S7 + 2) as i16,
            m.rd8(CTX + 0x17),
            m.rd8(CTX + 0x288),
        );

        let mut h = host();
        h.dt = i16::from(dt);
        h.pad = pad;
        h.held = held;
        let a = &mut h.actors[usize::from(slot)];
        a.active_target = target;
        a.action_category = category;
        if target < 8 {
            h.actors[usize::from(target)].hp = target_hp;
        }
        if ramping {
            let shown = if target == 8 { 0 } else { target };
            let a = &mut h.actors[usize::from(shown)];
            a.hp_display = Some(a.hp.wrapping_add(1));
        }
        let mut ctx = BattleActionCtx {
            action_state: 0x51,
            active_actor: slot,
            frame_timer: timer,
            done_ui_torn_down: latch,
            action_ui_element: element,
            spirit_action_count: spirit,
            levelup_banner_element: banner,
            absorbed_seru: seru,
            menu_open: menu,
            lone_defeat_latch: lone,
            ..Default::default()
        };
        ba::step(&mut h, &mut ctx);
        let port = (
            ctx.action_state,
            ctx.frame_timer,
            ctx.done_ui_torn_down,
            ctx.lone_defeat_latch,
        );
        let at = format!(
            "round {round}: slot {slot} target {target} category {category} timer {timer} \
             dt {dt} banner {banner:#x} pad {pad:#x}/{held:#x} menu {menu} seru {seru:#x} \
             latch {latch} element {element:#x} spirit {spirit}"
        );
        assert_eq!(retail, port, "{at}");
        assert_eq!(*r.ui.borrow(), h.ui, "elements, {at}");
        let learned: Vec<u8> = h.learned.iter().map(|&(_, s)| s).collect();
        assert_eq!(*r.learned.borrow(), learned, "grant, {at}");
        left += i32::from(retail.0 != 0x51);
        torn += i32::from(retail.2 != latch);
        skipped += i32::from(retail.1 == -1 && banner != 0);
        granted += i32::from(!learned.is_empty());
    }
    assert!(
        left > 2000 && torn > 2000 && skipped > 100 && granted > 100,
        "{left} {torn} {skipped} {granted}"
    );
}

/// State `0x52`, the Seru band's countdown (`0x801E637C..0x801E6424`) against
/// `done_seru_absorb`: the step, the press skip to `0x13`, the exit to `0x5A`
/// clearing `ctx[+0x269]`, and the capture banner's unload behind the
/// teardown latch.
#[test]
fn the_seru_band_counts_down_as_retail_does() {
    differential!(prot(898, SM));
    let Some(mut r) = Retail::new() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut rng = Rng(0x5345_5255);
    let (mut left, mut cut, mut closed) = (0, 0, 0);
    for round in 0..10000 {
        let slot = rng.below(3) as u8;
        let timer = [0i16, 1, 2, 0x13, 0x14, 0x15, 0x30, 0xB4][rng.below(8) as usize];
        let dt = 1 + rng.below(2) as u8;
        let pad = [0u16, 0x40][rng.below(2) as usize];
        let latch = rng.below(2) as u8;
        let seru = 1 + rng.below(0x20) as u8;
        r.reset();
        let m = &mut r.cpu.mem;
        m.wr8(FRAME_STEP, dt);
        m.wr8(CTX + 7, 0x52);
        m.wr8(CTX + 0x13, slot);
        m.wr8(CTX + 0x17, latch);
        m.wr8(CTX + 0x269, seru);
        m.wr16(S7 + 2, timer as u16);
        m.wr32(0x8007_B874, u32::from(pad));
        r.pass();
        let m = &mut r.cpu.mem;
        let retail = (
            m.rd8(CTX + 7),
            m.rd16(S7 + 2) as i16,
            m.rd8(CTX + 0x17),
            m.rd8(CTX + 0x269),
        );
        let mut h = host();
        h.dt = i16::from(dt);
        h.pad = pad;
        let mut ctx = BattleActionCtx {
            action_state: 0x52,
            active_actor: slot,
            frame_timer: timer,
            done_ui_torn_down: latch,
            absorbed_seru: seru,
            ..Default::default()
        };
        ba::step(&mut h, &mut ctx);
        let port = (
            ctx.action_state,
            ctx.frame_timer,
            ctx.done_ui_torn_down,
            ctx.absorbed_seru,
        );
        let at = format!("round {round}: timer {timer} dt {dt} pad {pad:#x} latch {latch}");
        assert_eq!(retail, port, "{at}");
        assert_eq!(*r.ui.borrow(), h.ui, "elements, {at}");
        left += i32::from(retail.0 == 0x5A);
        cut += i32::from(retail.1 == 0x13);
        closed += i32::from(!h.ui.is_empty());
    }
    assert!(
        left > 500 && cut > 500 && closed > 1000,
        "{left} {cut} {closed}"
    );
}

/// State `0x5A`, the end of an action (`0x801E6474..0x801E67E4`) against
/// `end_of_action`: the pool sweep, the two wipe scans with the special
/// battle's rotted leader, the monster wipe's victory seat (the dead actor's
/// re-pick on `rand()`, the Songi overrides, the win-pose stream) and the
/// turn cursor against the round's bound.
#[test]
fn the_end_of_an_action_ends_what_retail_ends() {
    differential!(prot(898, SM));
    let Some(mut r) = Retail::new() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let stream = Rc::new(std::cell::Cell::new(0u32));
    let s2 = stream.clone();
    r.cpu.hook(RAND, move |regs, _| {
        s2.set(lcg(s2.get()));
        regs.r[2] = (s2.get() >> 16) & 0x7FFF;
    });
    let poses = Rc::new(RefCell::new(Vec::new()));
    let sink = poses.clone();
    r.cpu
        .hook(WIN_POSE, move |regs, _| sink.borrow_mut().push(regs.r[4]));
    let mut rng = Rng(0x454E_4441);
    let (mut party_wipes, mut wins, mut repicks, mut next, mut round_ends) = (0, 0, 0, 0, 0);
    let mut sweeps = 0;
    for round in 0..40000 {
        let monsters = 1 + rng.below(5) as u8;
        let mut hp = [0u16; 8];
        let mut status = [0u16; 8];
        let mut flags = [0u32; 8];
        let mut bytes = [[0u8; 3]; 8];
        for s in 0..8usize {
            let seated = s < 3 || s < 3 + usize::from(monsters);
            let down = if s < 3 { 3 } else { 2 };
            hp[s] = if seated && rng.below(down) != 0 {
                1 + rng.below(500) as u16
            } else {
                0
            };
            status[s] = [0u16, 0, 0, 4, 0x400, 0x38, 0x80][rng.below(7) as usize];
            flags[s] = rng.next();
            bytes[s] = [rng.below(3) as u8, rng.below(3) as u8, rng.below(3) as u8];
        }
        let slot = rng.below(3 + u32::from(monsters)) as u8;
        // A monster acting at a monster wipe is one the wipe counted down:
        // retail keeps a standing one and indexes the roster with it (the
        // charm softlock the port fixes, `victory_pose_fixup`).
        if slot >= 3 && status[usize::from(slot)] & 4 != 0 {
            hp[usize::from(slot)] = 0;
        }
        let special = [0u32, 0, 0, 1][rng.below(4) as usize];
        let first = [0x12u8, 0x12, 0xB3, 0xB4][rng.below(4) as usize];
        let cursor = rng.below(8) as u8;
        let skip = rng.below(3) as u8;
        let seed = rng.next();
        // Retail's re-pick spins without a living, unmasked member.
        if (0..3).all(|s| hp[s] == 0 || status[s] & 0x404 != 0) {
            continue;
        }

        r.reset();
        stream.set(seed);
        poses.borrow_mut().clear();
        let m = &mut r.cpu.mem;
        m.wr8(CTX, 3);
        m.wr8(CTX + 1, monsters);
        m.wr8(CTX + 7, 0x5A);
        m.wr8(CTX + 0x13, slot);
        m.wr8(CTX + 0x1A, cursor);
        m.wr8(CTX + 0x25, skip);
        m.wr32(SPECIAL_WORD, special);
        m.wr8(FORMATION, first);
        m.wr8(END_SIGNAL, 0);
        m.wr32(END_CAUSE, 0x77);
        for s in 0..3u32 {
            m.wr8(SEATS + s, s as u8 + 1);
        }
        for s in 0..8u8 {
            let a = actor(s);
            let i = usize::from(s);
            m.wr16(a + 0x14C, hp[i]);
            m.wr16(a + 0x16E, status[i]);
            m.wr32(a + 8, flags[i]);
            m.wr8(a + 0x21C, bytes[i][0] + 1);
            m.wr8(a + 0x21F, bytes[i][1] + 1);
            m.wr8(a + 0x225, bytes[i][2]);
        }
        r.pass();
        let m = &mut r.cpu.mem;
        let ended = match (m.rd8(END_SIGNAL), m.rd32(END_CAUSE)) {
            (0xFE, 5) => vec![ba::BattleEndCause::PartyWipe],
            (0xFE, 0) => vec![ba::BattleEndCause::MonsterWipe],
            (0, 0x77) => vec![],
            other => panic!("round {round}: end signal {other:?}"),
        };
        let retail_state = (m.rd8(CTX + 7), m.rd8(CTX + 0x1A));
        let retail_seat = m.rd8(CTX + 0x13);
        let retail_actors: Vec<(u32, u8, u8, u8)> = (0..8u8)
            .map(|s| {
                let a = actor(s);
                (
                    m.rd32(a + 8),
                    m.rd8(a + 0x21C),
                    m.rd8(a + 0x21F),
                    m.rd8(a + 0x225),
                )
            })
            .collect();

        let mut h = host();
        h.special = special;
        h.first_monster = first;
        h.rand = seed;
        for s in 0..8usize {
            let a = &mut h.actors[s];
            a.hp = hp[s];
            a.liveness = hp[s];
            a.max_hp = if s < 3 + usize::from(monsters) {
                500
            } else {
                0
            };
            a.field_flags = status[s];
            a.flag_word = flags[s];
            a.render_flag = bytes[s][0] + 1;
            a.impact_state = bytes[s][1] + 1;
            a.capture_state = bytes[s][2];
        }
        let mut ctx = BattleActionCtx {
            action_state: 0x5A,
            active_actor: slot,
            turn_cursor: cursor,
            round_skip: skip,
            ..Default::default()
        };
        ba::step(&mut h, &mut ctx);
        let at = format!(
            "round {round}: monsters {monsters} slot {slot} hp {hp:?} status {status:x?} \
             special {special} first {first:#x} cursor {cursor} skip {skip}"
        );
        assert_eq!(ended, h.ended, "end, {at}");
        let port_actors: Vec<(u32, u8, u8, u8)> = h
            .actors
            .iter()
            .map(|a| (a.flag_word, a.render_flag, a.impact_state, a.capture_state))
            .collect();
        assert_eq!(retail_actors[..7], port_actors[..7], "sweep, {at}");
        sweeps += 1;
        match ended.first() {
            Some(ba::BattleEndCause::MonsterWipe) => {
                assert_eq!(retail_seat, ctx.active_actor, "victory seat, {at}");
                assert_eq!(
                    *poses.borrow(),
                    vec![u32::from(retail_seat) * 3 + 2],
                    "{at}"
                );
                assert_eq!(h.staged, vec![retail_seat], "{at}");
                assert_eq!(stream.get(), h.rand, "rand, {at}");
                wins += 1;
                repicks += i32::from(stream.get() != seed);
            }
            Some(_) => party_wipes += 1,
            None => {
                assert_eq!(retail_state, (ctx.action_state, ctx.turn_cursor), "{at}");
                next += i32::from(retail_state.0 == 0x0A);
                round_ends += i32::from(retail_state.0 == 0xFF);
            }
        }
    }
    assert!(
        party_wipes > 500 && wins > 500 && repicks > 100 && next > 500 && round_ends > 500,
        "{sweeps} {party_wipes} {wins} {repicks} {next} {round_ends}"
    );
}
