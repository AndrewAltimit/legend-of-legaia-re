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
const TRIGGER: u32 = 0x801D_BF9C;
const DRIVER: u32 = 0x801D_C0A0;
const SFX: u32 = 0x8004_FCC8;
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
    busy: bool,
    bits_high: u32,
    calls: Vec<(u32, u32, u32)>,
    bits: u32,
    live: [(i16, i16); 8],
    anchor: [(i16, i16); 8],
    placed: bool,
    counters: Vec<u8>,
    drawn: [bool; 8],
    taunt: u8,
    reordered: u32,
    tag_list: Option<Vec<u8>>,
    ranged: bool,
    /// Retail's spell records (`0x800754C8`, twelve bytes an id) and the
    /// item effect pairs (`0x80074368` -> `0x800752C0`), read off the
    /// executable a test runs.
    spells: Vec<[u8; 12]>,
    item_pairs: Vec<(u8, u8)>,
    applied: Vec<(u8, u8, u8, u8)>,
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
    fn spell_anim_trigger(&mut self, party_slot: u8, spell_id: u8) {
        self.calls
            .push((TRIGGER, u32::from(party_slot), u32::from(spell_id)));
    }
    fn spell_anim_sustain(&mut self, actor_id: u8, anim_id: u8) {
        self.calls
            .push((DRIVER, u32::from(actor_id), u32::from(anim_id)));
    }
    fn one_shot_sfx(&mut self, cue_id: u16) {
        self.calls.push((SFX, u32::from(cue_id), 0));
    }
    fn recompute_battle_order(&mut self) {
        self.reordered += 1;
    }
    fn reaction_hold_view(&self, slot: u8) -> Option<(u8, bool)> {
        let a = self.actors.get(usize::from(slot))?;
        Some((a.current_anim, self.drawn[usize::from(slot)]))
    }
    fn spell_class_byte(&self, spell_id: u8) -> Option<u8> {
        self.spells.get(usize::from(spell_id)).map(|r| r[0])
    }
    fn spell_sub_class_byte(&self, spell_id: u8) -> Option<u8> {
        self.spells.get(usize::from(spell_id)).map(|r| r[1])
    }
    fn spell_mp_cost(&self, spell_id: u8) -> u8 {
        self.spells.get(usize::from(spell_id)).map_or(0, |r| r[3])
    }
    fn item_effect_class_pair(&self, item_id: u8) -> Option<(u8, u8)> {
        self.item_pairs.get(usize::from(item_id)).copied()
    }
    fn apply_damage(&mut self, icon: u8, page: u8, target_slot: u8, party_slot: u8) {
        self.applied.push((icon, page, target_slot, party_slot));
    }
    fn roster_character_id(&self, slot: u8) -> u8 {
        // The bytes past the party's three in `DAT_8007BD10` read `4` for the
        // monster seats, as the engine's host reports them.
        if slot < 3 { slot + 1 } else { 4 }
    }
    fn range_check(&self, actor_slot: u8, target_slot: u8) -> u16 {
        if !self.ranged {
            return 0;
        }
        test_range(actor_slot, target_slot, &self.live, &self.anchor)
    }
    fn set_actor_anchor(&mut self, slot: u8, x: i16, z: i16) {
        self.anchor[usize::from(slot)] = (x, z);
    }
    fn monster_action_tags(&self, _slot: u8) -> Option<Vec<u8>> {
        if let Some(t) = &self.tag_list {
            return Some(t.clone());
        }
        let mut tags = vec![0u8; 8];
        if let Some(t) = tags.get_mut(usize::from(self.taunt)) {
            *t = ba::KO_TAUNT_TAG;
        }
        Some(tags)
    }
    fn begin_counterattack(&mut self, counterer: u8, _attacker: u8) -> bool {
        self.counters.push(counterer);
        true
    }
    fn character_ability_bits(&self, _party_slot: u8) -> u32 {
        self.bits
    }
    fn actor_position(&self, slot: u8) -> Option<(i16, i16)> {
        self.placed.then(|| self.live[usize::from(slot)])
    }
    fn set_actor_position(&mut self, slot: u8, x: i16, z: i16) {
        self.live[usize::from(slot)] = (x, z);
    }
    fn actor_anchor(&self, slot: u8) -> Option<(i16, i16)> {
        self.placed.then(|| self.anchor[usize::from(slot)])
    }
    fn character_ability_bits_high(&self, party_slot: u8) -> u32 {
        // A monster seat has no character record; retail's `DAT_8007BD10`
        // byte there names the empty record `3`.
        if party_slot < 3 { self.bits_high } else { 0 }
    }
    fn previous_action_cleared(&self, _arg: u8) -> bool {
        !self.busy
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

/// States `0x00`, `0x0A` and `0x0B`, the action's opening
/// (`0x801E2AB8..0x801E2B74`, `0x801E2B78..0x801E2B98`, `0x801E3038..0x801E3058`)
/// against `begin`, `pre_action_wait` and `queued_from_menu`: the formation
/// arm's turn cursor and latch, the menu hold and the wait on
/// `FUN_8003F2B8(1)`.
#[test]
fn the_action_opening_states_are_retails() {
    differential!(prot(898, SM));
    let Some(mut r) = Retail::new() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut rng = Rng(0x4F50_454E);
    let mut seen = std::collections::HashSet::new();
    for round in 0..6000 {
        let state = [0x00u8, 0x0A, 0x0B][rng.below(3) as usize];
        let party = 1 + rng.below(3) as u8;
        let monsters = 1 + rng.below(5) as u8;
        let advantage = rng.below(4) as u8;
        let latched = rng.below(3) as u8;
        let cursor = rng.below(8) as u8;
        let menu = rng.below(2) as u8;
        let busy = rng.below(2) != 0;
        r.reset();
        r.cpu.stub(0x8003_F2B8, u32::from(busy));
        let m = &mut r.cpu.mem;
        m.wr8(CTX, party);
        m.wr8(CTX + 1, monsters);
        m.wr8(CTX + 7, state);
        m.wr8(CTX + 0x1A, cursor);
        m.wr8(CTX + 0x276, menu);
        m.wr8(CTX + 0x290, advantage);
        m.wr8(CTX + 0x291, latched);
        m.wr16(S7 + 4, 0x55);
        r.pass();
        let m = &mut r.cpu.mem;
        let retail = (
            m.rd8(CTX + 7),
            m.rd8(CTX + 0x1A),
            m.rd8(CTX + 0x290),
            m.rd8(CTX + 0x291),
        );
        let mut h = host();
        h.busy = busy;
        // The seated counts: the party's slots, then the living monsters.
        for s in 0..usize::from(party + monsters) {
            h.actors[s].liveness = 1;
        }
        let mut ctx = BattleActionCtx {
            action_state: state,
            turn_cursor: cursor,
            menu_open: menu,
            formation_advantage: advantage,
            formation_latched: latched,
            combo_timer: 0x55,
            ..Default::default()
        };
        struct Seated<'a>(&'a mut Host, u8);
        impl BattleActionHost for Seated<'_> {
            fn actor(&self, slot: u8) -> Option<&BattleActor> {
                self.0.actor(slot)
            }
            fn actor_mut(&mut self, slot: u8) -> Option<&mut BattleActor> {
                self.0.actor_mut(slot)
            }
            fn party_count(&self) -> u8 {
                self.1
            }
            fn previous_action_cleared(&self, arg: u8) -> bool {
                self.0.previous_action_cleared(arg)
            }
        }
        ba::step(&mut Seated(&mut h, party), &mut ctx);
        let port = (
            ctx.action_state,
            ctx.turn_cursor,
            ctx.formation_advantage,
            ctx.formation_latched,
        );
        let at = format!(
            "round {round}: state {state:#x} party {party} monsters {monsters} advantage \
             {advantage} cursor {cursor} menu {menu} busy {busy}"
        );
        assert_eq!(retail, port, "{at}");
        if state == 0 {
            assert_eq!(r.cpu.mem.rd16(S7 + 4) as i16, ctx.combo_timer, "{at}");
        }
        seen.insert((state, retail.0));
    }
    assert_eq!(seen.len(), 6, "{seen:?}");
}

const PLATE: u32 = 0x801F_6968;
const PLATE_RECORD: u32 = 0x8016_0000;
const TABLE: u32 = 0x8016_0200;
const BAR_RECORD: u32 = 0x8016_0100;
const RECORDS: u32 = 0x8008_4140;

/// States `0x46`, `0x47` and `0x48`, the Spirit band
/// (`0x801E52A4..0x801E57C4`) against `spirit_arts_entry`,
/// `spirit_arts_sustain` and `spirit_arts_flush`: the extended gauge and the
/// AP target with the two AP Boost passives and the cap, the clip staging,
/// the level-triggered hold, the AP plate's climb, the bar's growth unless
/// the charge byte sized it, and the hand-off to the Done band.
#[test]
fn the_spirit_band_steps_as_retail_steps_it() {
    differential!(prot(898, SM));
    let Some(mut r) = Retail::new() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut rng = Rng(0x5350_4231);
    let mut seen = std::collections::HashSet::new();
    for round in 0..30000 {
        let state = [0x46u8, 0x47, 0x48][rng.below(3) as usize];
        let slot = rng.below(3) as u8;
        let dt = 1 + rng.below(2) as u8;
        let base = [0u16, 7, 100, 250, 400][rng.below(5) as usize];
        let spirit = [0u16, 30, 60, 75, 100][rng.below(5) as usize];
        let bits = [0u32, 0x100, 0x200, 0x300][rng.below(4) as usize];
        let timer = [-3i16, 0, 1, 2, 8, 15, 16, 17, 0x20, 0x300][rng.below(10) as usize];
        let target = [0i16, 40, 75, 100][rng.below(4) as usize];
        let plate = [0i16, 39, 40, 41, 74, 99, 100][rng.below(7) as usize];
        let gauge_goal = [0i16, 20, 0x60, 0x120][rng.below(4) as usize];
        let width = [0i16, 13, 14, 15, 0x5A, 0x11A][rng.below(6) as usize];
        let current = [0u8, 0, 0x11][rng.below(3) as usize];
        let queued = [0u8, 0, 0x11, 0x12][rng.below(4) as usize];
        let queued_b = rng.below(0x20) as u8;
        let flags = [0u8, 0, 2, 4][rng.below(4) as usize];
        let charged = [0u8, 0, 1][rng.below(3) as usize];

        r.reset();
        let m = &mut r.cpu.mem;
        m.wr8(FRAME_STEP, dt);
        m.wr8(CTX + 7, state);
        m.wr8(CTX + 0x13, slot);
        m.wr16(S7 + 2, timer as u16);
        m.wr16(S7 + 6, gauge_goal as u16);
        m.wr16(S7 + 8, target as u16);
        m.wr32(PLATE, PLATE_RECORD);
        m.wr16(PLATE_RECORD + 0x10, plate as u16);
        m.wr32(CTX + 0x1074, BAR_RECORD);
        m.wr16(BAR_RECORD + 0xE, width as u16);
        for s in 0..3u32 {
            m.wr8(SEATS + s, s as u8 + 1);
            m.wr32(RECORDS + s * 0x414 + 0x6C0, bits);
        }
        let a = actor(slot);
        m.wr16(a + 0x156, base);
        m.wr16(a + 0x170, spirit);
        m.wr8(a + 0x1D9, current);
        m.wr8(a + 0x1DA, queued);
        m.wr8(a + 0x1DC, flags);
        m.wr8(a + 0x1E7, queued_b);
        m.wr8(a + 0x1F9, charged);
        r.pass();
        let m = &mut r.cpu.mem;
        let retail = (
            m.rd8(CTX + 7),
            m.rd16(S7 + 2) as i16,
            m.rd16(S7 + 6) as i16,
            m.rd16(S7 + 8) as i16,
            m.rd16(PLATE_RECORD + 0x10) as i16,
            m.rd16(BAR_RECORD + 0xE) as i16,
            m.rd8(a + 0x1DA),
            m.rd8(a + 0x1DC),
        );
        let retail_depth = m.rd16(CTX + 0x6D0);

        let mut h = host();
        h.dt = i16::from(dt);
        h.bits_high = bits;
        let b = &mut h.actors[usize::from(slot)];
        b.agl_base = base;
        b.spirit_gauge = spirit;
        b.current_anim = current;
        b.queued_anim = queued;
        b.flag_bits = ba::ActorFlags(flags);
        b.queued_anim_b = queued_b;
        b.spirit_shield = charged;
        let mut ctx = BattleActionCtx {
            action_state: state,
            active_actor: slot,
            frame_timer: timer,
            damage_target: gauge_goal,
            hp_bar_target: target,
            spirit_plate_value: plate,
            spirit_bar_width: width,
            ..Default::default()
        };
        ba::step(&mut h, &mut ctx);
        let b = &h.actors[usize::from(slot)];
        let port = (
            ctx.action_state,
            ctx.frame_timer,
            ctx.damage_target,
            ctx.hp_bar_target,
            ctx.spirit_plate_value,
            ctx.spirit_bar_width,
            b.queued_anim,
            b.flag_bits.0,
        );
        let at = format!(
            "round {round}: state {state:#x} dt {dt} base {base} spirit {spirit} bits {bits:#x} \
             timer {timer} target {target} plate {plate} goal {gauge_goal} width {width} \
             anim {current:#x}/{queued:#x} flags {flags} charged {charged}"
        );
        assert_eq!(retail, port, "{at}");
        if state == 0x46 {
            assert_eq!(retail_depth as i16, ctx.camera_frame_height, "{at}");
        }
        seen.insert((state, retail.0));
    }
    assert_eq!(seen.len(), 5, "{seen:?}");
}

/// Hook `entries` to log their first two arguments, in call order.
fn log_calls(cpu: &mut Cpu, entries: &[u32]) -> Rc<RefCell<Vec<(u32, u32, u32)>>> {
    let log = Rc::new(RefCell::new(Vec::new()));
    for &e in entries {
        let sink = log.clone();
        // The one-shot cue takes one argument; `a1` is whatever was left.
        cpu.hook(e, move |r, _| {
            let a1 = if e == SFX { 0 } else { r.r[5] };
            sink.borrow_mut().push((e, r.r[4], a1))
        });
    }
    log
}

/// States `0x29` to `0x2E`, the cast band (`0x801E4598..0x801E497C`) against
/// `magic_pre_cast_wait`, `magic_anim_chain`, `magic_sustain`,
/// `magic_hit_loop`, `magic_recovery` and `magic_exit`: the trigger, the
/// summon route, the stream cursor's bumps and stages, the three one-shot
/// cues, the `(clip, shot)` chain, the clip latch, the hit loop's exit and
/// both gates, with every cast-effect driver call and the byte it films.
#[test]
fn the_cast_band_walks_the_stream_as_retail_does() {
    differential!(prot(898, SM));
    let Some(mut r) = Retail::new() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let log = log_calls(&mut r.cpu, &[TRIGGER, DRIVER, SFX]);
    let mut rng = Rng(0x4D41_4749);
    let mut seen = std::collections::HashSet::new();
    for round in 0..40000 {
        let state = 0x29 + rng.below(6) as u8;
        let slot = rng.below(5) as u8;
        let dt = 1 + rng.below(2) as u8;
        let timer = [0i16, 0, 1, 2, 5][rng.below(5) as usize];
        let spell = [0x05u8, 0x2C, 0x3F, 0x6A, 0x80, 0x81, 0x90][rng.below(7) as usize];
        let route = spell >= 0x81 && rng.below(2) == 0;
        let mut params = [0u8; 16];
        params[0] = spell;
        for b in params.iter_mut().skip(1) {
            *b = if rng.below(5) == 0 {
                0xFF
            } else {
                0x10 + rng.below(0x20) as u8
            };
        }
        if route {
            params[1] = 9;
        }
        let cursor = rng.below(7) as u8;
        let latch = [0u8, 0, 1, 2][rng.below(4) as usize];
        let flags = rng.below(8) as u8;
        let current = [0u8, 0x11][rng.below(2) as usize];
        let bound = [0u8, 0, 2, 5][rng.below(4) as usize];
        let hits = [0u8, 1, 2, 5, 6][rng.below(5) as usize];
        let recovery = [0u8, 0, 1][rng.below(3) as usize];
        let exit = [0u8, 0, 1][rng.below(3) as usize];
        let target = [3u8, 4, 8, 9][rng.below(4) as usize];

        r.reset();
        log.borrow_mut().clear();
        let m = &mut r.cpu.mem;
        m.wr8(FRAME_STEP, dt);
        m.wr8(CTX + 7, state);
        m.wr8(CTX + 0x13, slot);
        m.wr8(CTX + 0x15, cursor);
        m.wr8(CTX + 0x249, exit);
        m.wr8(CTX + 0x24C, hits);
        m.wr8(CTX + 0x24D, recovery);
        m.wr16(S7 + 2, timer as u16);
        let a = actor(slot);
        for (k, b) in params.iter().enumerate() {
            m.wr8(a + 0x1DF + k as u32, *b);
        }
        m.wr8(a + 0x1D9, current);
        m.wr8(a + 0x1DC, flags);
        m.wr8(a + 0x1DD, target);
        m.wr8(a + 0x1FA, latch);
        m.wr8(a + 0x21B, bound);
        r.pass();
        let m = &mut r.cpu.mem;
        let retail = (
            m.rd8(CTX + 7),
            m.rd16(S7 + 2) as i16,
            m.rd8(CTX + 0x15),
            m.rd8(a + 0x1DA),
            m.rd8(a + 0x1FA),
            m.rd8(a + 0x1DC),
            m.rd8(a + 0x21B),
        );

        let mut h = host();
        h.dt = i16::from(dt);
        let b = &mut h.actors[usize::from(slot)];
        b.params[..16].copy_from_slice(&params);
        b.sub_route = params[1];
        b.strike_index = cursor;
        b.current_anim = current;
        b.flag_bits = ba::ActorFlags(flags);
        b.active_target = target;
        b.spell_iter = latch;
        b.hit_count_bound = bound;
        let mut ctx = BattleActionCtx {
            action_state: state,
            active_actor: slot,
            frame_timer: timer,
            hit_counter: hits,
            magic_recovery_gate: recovery,
            magic_exit_gate: exit,
            ..Default::default()
        };
        ba::step(&mut h, &mut ctx);
        let b = &h.actors[usize::from(slot)];
        let port = (
            ctx.action_state,
            ctx.frame_timer,
            b.strike_index,
            b.queued_anim,
            b.spell_iter,
            b.flag_bits.0,
            b.hit_count_bound,
        );
        let at = format!(
            "round {round}: state {state:#x} slot {slot} dt {dt} timer {timer} params {params:x?} \
             cursor {cursor} latch {latch} flags {flags} anim {current:#x} bound {bound} hits \
             {hits} gates {recovery}/{exit} target {target}"
        );
        assert_eq!(retail, port, "{at}");
        assert_eq!(*log.borrow(), h.calls, "calls, {at}");
        seen.insert((state, retail.0));
    }
    assert!(seen.len() >= 12, "{seen:x?}");
}

const BEARING: u32 = 0x8001_9B28;
const SIN_PTR: u32 = 0x8007_B81C;
const COS_PTR: u32 = 0x8007_B7F8;
const SINE_TABLE: u32 = 0x8007_0A2C;
const SUPER_TRIGGER: u32 = 0x801F_696C;
const COUNTER: u32 = 0x801F_6970;
const MARKS: u32 = 0x801F_6990;

/// States `0x1E` and `0x1F`, the strike loop and its recovery wait
/// (`0x801E36E0..0x801E3B1C`) against `attack_chain` and `attack_recovery`:
/// the facing store onto the target's body pair, the in-flight hold, the
/// stage (cursor, `+0x1DC` bit 1, `+0x1DA`), the War God Icon's pass bump
/// and end-of-stream refill with its starter demotion, the swing drift on
/// both live pairs, the terminator, and `0x1F`'s wait and exit to `0x20`.
/// The bearing `FUN_80019B28` answers through `bearing_12bit_approx` (its
/// own differential is `bearing`), the trig tables are the executable's.
#[test]
fn the_strike_loop_stages_and_drifts_as_retail_does() {
    use legaia_engine_battle_vm::battle_action::bearing_12bit_approx;
    differential!(prot(898, SM));
    let Some(mut r) = Retail::new() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    r.cpu.hook(BEARING, |regs, _| {
        let [a0, a1, a2, a3] = [4, 5, 6, 7].map(|i| regs.r[i] as i16);
        regs.r[2] = u32::from(bearing_12bit_approx(a0, a1, a2, a3));
    });
    let built = Rc::new(RefCell::new(Vec::new()));
    let sink = built.clone();
    r.cpu.hook(0x801E_ED1C, move |regs, _| {
        sink.borrow_mut().push(regs.r[4] as u8)
    });
    let mut rng = Rng(0x5354_524B);
    let mut swapped = 0;
    let mut seen = std::collections::HashSet::new();
    let (mut drifted, mut refilled, mut bumped) = (0, 0, 0);
    for round in 0..40000 {
        let state = if rng.below(4) == 0 { 0x1F } else { 0x1E };
        let slot = rng.below(5) as u8;
        let target = loop {
            let t = rng.below(8) as u8;
            if t != slot {
                break t;
            }
        };
        let dt = 1 + rng.below(2) as u8;
        let mut params = [0u8; ba::ACTION_QUEUE_CAP];
        let len = if rng.below(8) == 0 {
            18
        } else {
            rng.below(8) as usize
        };
        for b in params.iter_mut().take(len) {
            *b = [0x0C, 0x0D, 0x19, 0x1A, 0x1B][rng.below(5) as usize];
        }
        let cursor = rng.below(len as u32 + 1) as u8;
        let flags = [0u8, 0, 2, 3][rng.below(4) as usize];
        let x2 = [0u8, 0, 1, 2][rng.below(4) as usize];
        let bits = [0u32, 0x2000][rng.below(2) as usize];
        let header = [0u8, 0, 1][rng.below(3) as usize];
        let trigger = rng.below(2);
        let latched = [0x0Cu8, 0x10, 0x1A, 0x1B, 0x20][rng.below(5) as usize];
        let rate = [8u8, 4, 2, 0x10][rng.below(4) as usize];
        let marks: [u32; ba::ACTION_QUEUE_CAP] =
            std::array::from_fn(|_| [0u32, 0, 1, 4][rng.below(4) as usize]);
        // An exhausted stream staged with the latch down: retail stages the
        // `0`, raises `+0x1DC` bit 1 and leaves for `0x1F`, where it waits on
        // a commit of entry `0`; the port releases the latch instead
        // (`attack_chain`'s empty-stream arm), which the engine's anim
        // channel needs, having nothing to commit for a `0`.
        if state == 0x1E && flags & 2 == 0 && params[usize::from(cursor)] == 0 {
            continue;
        }
        // The counterattack latch: the counterer's seat plus one.
        let counter = [0u8, 0, 0, 1, 2, 3][rng.below(6) as usize];
        let target_category = [3u8, 3, 1][rng.below(3) as usize];
        let turn = rng.below(6) as u8;
        // The counterer is the party member the monster struck, never the
        // striker itself.
        if counter != 0 && (counter - 1 == slot || counter - 1 == target) {
            continue;
        }
        let mut live = [(0i16, 0i16); 8];
        let mut anchor = [(0i16, 0i16); 8];
        let mut facing = [0u16; 8];
        for s in 0..8 {
            live[s] = (
                rng.below(0x800) as i16 - 0x400,
                rng.below(0x800) as i16 - 0x400,
            );
            anchor[s] = (
                rng.below(0x800) as i16 - 0x400,
                rng.below(0x800) as i16 - 0x400,
            );
            facing[s] = rng.below(0x1000) as u16;
        }

        r.reset();
        let m = &mut r.cpu.mem;
        m.wr32(SIN_PTR, SINE_TABLE);
        m.wr32(COS_PTR, SINE_TABLE + 0x800);
        m.wr8(FRAME_STEP, dt);
        m.wr8(CTX + 7, state);
        m.wr8(CTX + 0x13, slot);
        m.wr8(CTX + 0x15, cursor);
        m.wr8(CTX + 0x16, x2);
        m.wr8(CTX + 0x243, header);
        m.wr32(SUPER_TRIGGER, trigger);
        m.wr32(COUNTER, u32::from(counter));
        m.wr8(CTX + 0x1A, turn);
        built.borrow_mut().clear();
        for (k, mk) in marks.iter().enumerate() {
            m.wr32(MARKS + 4 * k as u32, *mk);
        }
        for s in 0..3u32 {
            m.wr8(SEATS + s, s as u8 + 1);
            m.wr32(RECORDS + s * 0x414 + 0x6BC, bits);
        }
        for s in 0..8u8 {
            let a = actor(s);
            let i = usize::from(s);
            m.wr16(a + 0x34, live[i].0 as u16);
            m.wr16(a + 0x38, live[i].1 as u16);
            m.wr16(a + 0x3C, anchor[i].0 as u16);
            m.wr16(a + 0x40, anchor[i].1 as u16);
            m.wr16(a + 0x46, facing[i]);
            m.wr8(a + 0x21D, rate);
            m.wr16(a + 0x16C, 0x40);
            for (k, b) in params.iter().enumerate() {
                m.wr8(a + 0x1DF + k as u32, *b);
            }
        }
        m.wr8(actor(target) + 0x1DE, target_category);
        let a = actor(slot);
        m.wr8(a + 0x1DB, latched);
        m.wr8(a + 0x1DC, flags);
        m.wr8(a + 0x1DD, target);
        r.pass();
        let m = &mut r.cpu.mem;
        let acting = m.rd8(CTX + 0x13);
        let swap = (
            acting,
            m.rd8(CTX + 0x1A),
            m.rd32(COUNTER) as u8,
            m.rd8(actor(acting) + 0x1DD),
            m.rd16(actor(acting) + 0x16C),
            m.rd8(actor(slot) + 0x1DC),
        );
        let a = actor(acting);
        let retail = (
            m.rd8(CTX + 7),
            m.rd8(CTX + 0x15),
            m.rd8(CTX + 0x16),
            m.rd8(a + 0x1DA),
            m.rd8(a + 0x1DC),
            m.rd16(a + 0x46),
        );
        let retail_queue: Vec<u8> = (0..ba::ACTION_QUEUE_CAP as u32)
            .map(|k| m.rd8(a + 0x1DF + k))
            .collect();
        let retail_live: Vec<(i16, i16)> = (0..8u8)
            .map(|s| {
                (
                    m.rd16(actor(s) + 0x34) as i16,
                    m.rd16(actor(s) + 0x38) as i16,
                )
            })
            .collect();

        let mut h = host();
        h.dt = i16::from(dt);
        h.bits = bits;
        h.placed = true;
        h.live = live;
        h.anchor = anchor;
        for (b, f) in h.actors.iter_mut().zip(facing) {
            b.facing_angle = f;
            b.anim_rate = ba::AnimRate(rate);
            b.init_key = 0x40;
            b.params[..ba::ACTION_QUEUE_CAP].copy_from_slice(&params);
            b.strike_index = cursor;
            // Retail's marks are one global array the builder rewrites.
            b.starter_marks = Some(marks);
        }
        h.actors[usize::from(target)].action_category = target_category;
        let b = &mut h.actors[usize::from(slot)];
        b.latched_anim = latched;
        b.flag_bits = ba::ActorFlags(flags);
        b.active_target = target;
        b.starter_marks = Some(marks);
        let mut ctx = BattleActionCtx {
            action_state: state,
            active_actor: slot,
            attack_x2_pass: x2,
            gauge_rearm_latch: header,
            super_trigger: trigger != 0,
            counter_pending: counter,
            turn_cursor: turn,
            ..Default::default()
        };
        ba::step(&mut h, &mut ctx);
        let b = &h.actors[usize::from(ctx.active_actor)];
        let port_swap = (
            ctx.active_actor,
            ctx.turn_cursor,
            ctx.counter_pending,
            b.active_target,
            b.init_key,
            h.actors[usize::from(slot)].flag_bits.0,
        );
        let port = (
            ctx.action_state,
            b.strike_index,
            ctx.attack_x2_pass,
            b.queued_anim,
            b.flag_bits.0,
            b.facing_angle,
        );
        let at = format!(
            "round {round}: state {state:#x} slot {slot} target {target} params {params:x?} \
             cursor {cursor} flags {flags} x2 {x2} bits {bits:#x} header {header} trigger \
             {trigger} latched {latched:#x} rate {rate} dt {dt} counter {counter} \
             target category {target_category}"
        );
        if state == 0x1E {
            assert_eq!(swap, port_swap, "swap, {at}");
            assert_eq!(*built.borrow(), h.counters, "builder, {at}");
            swapped += i32::from(acting != slot);
        }
        assert_eq!(retail, port, "{at}");
        assert_eq!(
            retail_queue,
            b.params[..ba::ACTION_QUEUE_CAP].to_vec(),
            "queue, {at}"
        );
        assert_eq!(retail_live, h.live.to_vec(), "live pairs, {at}");
        drifted += i32::from(retail_live != live.to_vec());
        refilled += i32::from(retail.2 > x2 && retail.1 == 0);
        bumped += i32::from(retail.2 > x2 && retail.1 != 0);
        seen.insert((state, retail.0));
    }
    assert!(
        seen.len() == 4 && drifted > 2000 && refilled > 100 && bumped > 100 && swapped > 500,
        "{seen:x?} {drifted} {refilled} {bumped} {swapped}"
    );
}

const TAG_LOOKUP: u32 = 0x8005_0E2C;
const MONSTER_TABLES: u32 = 0x801C_9348;
const NODE: u32 = 0x8015_0000;

/// State `0x20`, the strike band's return (`0x801E54EC..0x801E5658`) against
/// `attack_return`: the wait on the attacker's committed clip, the target
/// reaction hold (a party target on its downed loop, a body still drawn, the
/// lone scripted monster's bypass) and a monster's KO taunt, staged while a
/// party member still stands. The tag lookup `FUN_80050E2C` answers through
/// the host's tag list; the node's colour word stands for the drawn test.
#[test]
fn the_strike_return_waits_and_taunts_as_retail_does() {
    differential!(prot(898, SM));
    let Some(mut r) = Retail::new() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut rng = Rng(0x5245_5455);
    let (mut held, mut left, mut taunted) = (0, 0, 0);
    for round in 0..30000 {
        let slot = rng.below(6) as u8;
        let target = loop {
            let t = rng.below(8) as u8;
            if t != slot {
                break t;
            }
        };
        let committed = [0u8, 0, 0, 0x0C][rng.below(4) as usize];
        let target_anim = [0u8, 0, 4, 8, 0x11][rng.below(5) as usize];
        let drawn = rng.below(3) != 0;
        let scripted = rng.below(3) == 0;
        let lone = rng.below(2) as u8;
        // The latch's one writer raises it for a lone-monster formation
        // only (`BattleActionCtx::lone_defeat_latch`).
        let second = if scripted && lone != 0 {
            0
        } else {
            [0u8, 0x12][rng.below(2) as usize]
        };
        let target_hp = [0u16, 0, 20][rng.below(3) as usize];
        let taunt = [0xFFu8, 2, 5][rng.below(3) as usize];
        let party_hp: [u16; 3] = std::array::from_fn(|_| [0u16, 30][rng.below(2) as usize]);
        let flags = rng.below(4) as u8;

        r.reset();
        r.cpu.stub(TAG_LOOKUP, u32::from(taunt));
        let m = &mut r.cpu.mem;
        m.wr8(CTX, 3);
        m.wr8(CTX + 7, 0x20);
        m.wr8(CTX + 0x13, slot);
        m.wr8(CTX + 0x287, u8::from(scripted));
        m.wr8(CTX + 0x288, lone);
        m.wr8(FORMATION + 1, second);
        for k in 0..5u32 {
            m.wr32(MONSTER_TABLES + 4 * k, TABLE);
        }
        for s in 0..3u8 {
            m.wr16(actor(s) + 0x14C, party_hp[usize::from(s)]);
        }
        let a = actor(slot);
        m.wr8(a + 0x1D9, committed);
        m.wr8(a + 0x1DC, flags);
        m.wr8(a + 0x1DD, target);
        let t = actor(target);
        m.wr16(t + 0x14C, target_hp);
        m.wr8(t + 0x1D9, target_anim);
        m.wr32(t + 0x22C, NODE);
        m.wr32(NODE + 0x74, if drawn { 0x0080_8080 } else { 0x0100_0000 });
        r.pass();
        let m = &mut r.cpu.mem;
        let retail = (m.rd8(CTX + 7), m.rd8(a + 0x1DA), m.rd8(a + 0x1DC));

        let mut h = host();
        h.taunt = taunt;
        h.drawn[usize::from(target)] = drawn;
        for (a, hp) in h.actors.iter_mut().zip(party_hp) {
            a.liveness = hp;
            a.hp = hp;
        }
        let b = &mut h.actors[usize::from(slot)];
        b.current_anim = committed;
        b.flag_bits = ba::ActorFlags(flags);
        b.active_target = target;
        let tb = &mut h.actors[usize::from(target)];
        tb.liveness = target_hp;
        tb.hp = target_hp;
        tb.current_anim = target_anim;
        let mut ctx = BattleActionCtx {
            action_state: 0x20,
            active_actor: slot,
            scripted_fight: u8::from(scripted),
            lone_defeat_latch: lone,
            ..Default::default()
        };
        ba::step(&mut h, &mut ctx);
        let b = &h.actors[usize::from(slot)];
        let port = (ctx.action_state, b.queued_anim, b.flag_bits.0);
        let at = format!(
            "round {round}: slot {slot} target {target} committed {committed:#x} target anim \
             {target_anim} drawn {drawn} scripted {scripted} lone {lone} second {second:#x} \
             target hp {target_hp} taunt {taunt:#x} party {party_hp:?}"
        );
        assert_eq!(retail, port, "{at}");
        held += i32::from(retail.0 == 0x20 && committed == 0);
        left += i32::from(retail.0 == 0x50);
        taunted += i32::from(retail.2 & 2 != 0 && flags & 2 == 0);
    }
    assert!(
        held > 1000 && left > 5000 && taunted > 300,
        "{held} {left} {taunted}"
    );
}

const REORDER: u32 = 0x801D_ABA4;

/// State `0x50`'s frame around the Spirit block (`0x801E5C6C..0x801E5F60`)
/// against `done_cleanup`: the standing census that gates the order
/// recompute `FUN_801DABA4`, the clip-exit bit, the Done band's countdown
/// seed and its level-up banner stretch, the state store and the teardown
/// latch.
#[test]
fn the_done_cleanup_frame_is_retails() {
    differential!(prot(898, SM));
    let Some(mut r) = Retail::new() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let reorders = Rc::new(std::cell::Cell::new(0u32));
    let count = reorders.clone();
    r.cpu.hook(REORDER, move |_, _| count.set(count.get() + 1));
    let mut rng = Rng(0x434C_4E55);
    let (mut reordered, mut skipped) = (0, 0);
    for round in 0..20000 {
        let slot = rng.below(8) as u8;
        let monsters = rng.below(6) as u8;
        let category = rng.below(7) as u8;
        let banner = [0u8, 0, 0x2E][rng.below(3) as usize];
        let latch = rng.below(3) as u8;
        let flags = rng.below(0x10) as u8;
        let mut hp = [0u16; 8];
        let mut status = [0u16; 8];
        let all_down = rng.below(3) == 0;
        for s in 0..8 {
            let seated = s < 3 + usize::from(monsters);
            hp[s] = if all_down || !seated || rng.below(2) == 0 {
                0
            } else {
                40
            };
            status[s] = [0u16, 0, 4, 0x400][rng.below(4) as usize];
        }
        r.reset();
        reorders.set(0);
        let m = &mut r.cpu.mem;
        m.wr8(CTX, 3);
        m.wr8(CTX + 1, monsters);
        m.wr8(CTX + 7, 0x50);
        m.wr8(CTX + 0x13, slot);
        m.wr8(CTX + 0x17, latch);
        m.wr8(CTX + 0x26, banner);
        for s in 0..8u8 {
            m.wr16(actor(s) + 0x14C, hp[usize::from(s)]);
            m.wr16(actor(s) + 0x16E, status[usize::from(s)]);
        }
        m.wr8(actor(slot) + 0x1DC, flags);
        m.wr8(actor(slot) + 0x1DE, category);
        // The rate restore's gate stays shut: a party seat's staged id `>= 0x10`.
        m.wr8(actor(slot) + 0x1D9, 0x11);
        r.pass();
        let m = &mut r.cpu.mem;
        let retail = (
            m.rd8(CTX + 7),
            m.rd16(S7 + 2) as i16,
            m.rd8(CTX + 0x17),
            m.rd8(actor(slot) + 0x1DC),
            reorders.get(),
        );

        let mut h = host();
        for s in 0..8 {
            let a = &mut h.actors[s];
            a.liveness = hp[s];
            a.hp = hp[s];
            a.field_flags = status[s];
            // A monster seat the formation leaves empty is no combatant.
            a.max_hp = if s < 3 || s < 3 + usize::from(monsters) {
                40
            } else {
                0
            };
        }
        let b = &mut h.actors[usize::from(slot)];
        b.flag_bits = ba::ActorFlags(flags);
        b.action_category = category;
        b.current_anim = 0x11;
        let mut ctx = BattleActionCtx {
            action_state: 0x50,
            active_actor: slot,
            done_ui_torn_down: latch,
            levelup_banner_element: banner,
            ..Default::default()
        };
        ba::step(&mut h, &mut ctx);
        let port = (
            ctx.action_state,
            ctx.frame_timer,
            ctx.done_ui_torn_down,
            h.actors[usize::from(slot)].flag_bits.0,
            h.reordered,
        );
        let at = format!(
            "round {round}: slot {slot} monsters {monsters} category {category} banner \
             {banner:#x} hp {hp:?} status {status:x?}"
        );
        assert_eq!(retail, port, "{at}");
        reordered += i32::from(retail.4 != 0);
        skipped += i32::from(retail.4 == 0);
    }
    assert!(reordered > 3000 && skipped > 3000, "{reordered} {skipped}");
}

/// The range law both sides answer through: `motion::range_metric` (held to
/// `FUN_8004E2F0` by `range_metric`) over live and anchor pairs, every seat
/// below 3 a party seat with roster id `seat + 1`, monster size classes `0`.
fn test_range(attacker: u8, target: u8, live: &Pairs, anchor: &Pairs) -> u16 {
    use legaia_engine_battle_vm::battle_action::bearing_12bit_approx;
    use legaia_engine_battle_vm::battle_action::motion::{
        PARTY_REACH_OFFSETS, RangeInputs, range_metric,
    };
    if attacker >= 8 || target >= 8 {
        return 1;
    }
    let inputs = RangeInputs {
        attacker_party: attacker < 3,
        target_party: target < 3,
        attacker_reach: if attacker < 3 {
            PARTY_REACH_OFFSETS[usize::from(attacker)]
        } else {
            0
        },
        attacker_size: 0,
        target_size: 0,
        attacker_pos: live[usize::from(attacker)],
        target_ref: anchor[usize::from(target)],
    };
    let bearing = bearing_12bit_approx(
        inputs.target_ref.1,
        inputs.target_ref.0,
        inputs.attacker_pos.1,
        inputs.attacker_pos.0,
    );
    let angle = legaia_engine_battle_vm::battle_approach::approach_angle(bearing) as u16;
    let (sin, cos) = legaia_engine_system::trig::trig12(angle);
    range_metric(&inputs, sin, cos)
}

/// The eight seats' `(x, z)` pairs.
type Pairs = [(i16, i16); 8];

/// The live and anchor pairs of the eight seats as retail RAM holds them.
fn pairs(m: &mut Memory) -> (Pairs, Pairs) {
    let mut live = [(0, 0); 8];
    let mut anchor = [(0, 0); 8];
    for s in 0..8u8 {
        let a = actor(s);
        live[usize::from(s)] = (m.rd16(a + 0x34) as i16, m.rd16(a + 0x38) as i16);
        anchor[usize::from(s)] = (m.rd16(a + 0x3C) as i16, m.rd16(a + 0x40) as i16);
    }
    (live, anchor)
}

/// States `0x15` to `0x19`, the approach (`0x801E32E0..0x801E35EC`) against
/// `attack_windup`, `attack_advance`, `attack_close_range`, `attack_strike`
/// and `attack_short_step`: the facing store, each state's wait on its staged
/// clip, the walk and close-in staged off the monster's tag list, the range
/// polls, and the arrival shove that pushes the target's live and body pairs
/// back out to the reach's edge. The range law, the bearing and the tag
/// lookup `FUN_80050E2C` answer through the port's kernels.
#[test]
fn the_approach_walks_and_shoves_as_retail_does() {
    use legaia_engine_battle_vm::battle_action::bearing_12bit_approx;
    differential!(prot(898, SM));
    let Some(mut r) = Retail::new() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    r.cpu.hook(BEARING, |regs, _| {
        let [a0, a1, a2, a3] = [4, 5, 6, 7].map(|i| regs.r[i] as i16);
        regs.r[2] = u32::from(bearing_12bit_approx(a0, a1, a2, a3));
    });
    r.cpu.hook(0x8004_E2F0, |regs, m| {
        let (live, anchor) = pairs(m);
        regs.r[2] = u32::from(test_range(regs.r[4] as u8, regs.r[5] as u8, &live, &anchor));
    });
    let tag_map = Rc::new(RefCell::new(Vec::<u8>::new()));
    let tags = tag_map.clone();
    r.cpu.hook(TAG_LOOKUP, move |regs, _| {
        let want = regs.r[5] as u8;
        regs.r[2] = tags
            .borrow()
            .iter()
            .position(|&t| t == want)
            .map_or(0xFF, |i| i as u32);
    });
    let mut rng = Rng(0x4150_5052);
    let mut seen = std::collections::HashSet::new();
    let mut shoved = 0;
    for round in 0..30000 {
        let state = 0x15 + rng.below(5) as u8;
        let slot = rng.below(5) as u8;
        let target = loop {
            let t = rng.below(5) as u8;
            if t != slot {
                break t;
            }
        };
        let current = rng.below(3) as u8;
        let queued = if rng.below(2) == 0 {
            current
        } else {
            rng.below(3) as u8
        };
        let list: Vec<u8> = (0..6)
            .map(|_| [0u8, 1, 0x20, 0x21, 5][rng.below(5) as usize])
            .collect();
        let reach = [0x60i16, 0x200][rng.below(2) as usize];
        let mut c = || (rng.below(2 * reach as u32) as i32 - i32::from(reach)) as i16;
        let mut live = [(0i16, 0i16); 8];
        let mut anchor = [(0i16, 0i16); 8];
        for s in 0..5 {
            live[s] = (c(), c());
            anchor[s] = (c(), c());
        }
        let flags = rng.below(4) as u8;
        let x2 = rng.below(3) as u8;

        r.reset();
        *tag_map.borrow_mut() = list.clone();
        let m = &mut r.cpu.mem;
        m.wr32(SIN_PTR, SINE_TABLE);
        m.wr32(COS_PTR, SINE_TABLE + 0x800);
        m.wr8(CTX + 7, state);
        m.wr8(CTX + 0x13, slot);
        m.wr8(CTX + 0x16, x2);
        for k in 0..5 {
            m.wr32(MONSTER_TABLES + 4 * k, TABLE);
        }
        for s in 0..8u8 {
            let a = actor(s);
            let (l, b) = (live[usize::from(s)], anchor[usize::from(s)]);
            m.wr16(a + 0x34, l.0 as u16);
            m.wr16(a + 0x38, l.1 as u16);
            m.wr16(a + 0x3C, b.0 as u16);
            m.wr16(a + 0x40, b.1 as u16);
        }
        let a = actor(slot);
        m.wr8(a + 0x1D9, current);
        m.wr8(a + 0x1DA, queued);
        m.wr8(a + 0x1DC, flags);
        m.wr8(a + 0x1DD, target);
        r.pass();
        let m = &mut r.cpu.mem;
        let retail = (
            m.rd8(CTX + 7),
            m.rd8(CTX + 0x16),
            m.rd8(a + 0x1DA),
            m.rd8(a + 0x1DC),
            m.rd16(a + 0x46),
        );
        let retail_pairs = pairs(m);

        let mut h = host();
        h.placed = true;
        h.ranged = true;
        h.live = live;
        h.anchor = anchor;
        h.tag_list = Some(list.clone());
        let b = &mut h.actors[usize::from(slot)];
        b.current_anim = current;
        b.queued_anim = queued;
        b.flag_bits = ba::ActorFlags(flags);
        b.active_target = target;
        let mut ctx = BattleActionCtx {
            action_state: state,
            active_actor: slot,
            attack_x2_pass: x2,
            ..Default::default()
        };
        ba::step(&mut h, &mut ctx);
        let b = &h.actors[usize::from(slot)];
        let port = (
            ctx.action_state,
            ctx.attack_x2_pass,
            b.queued_anim,
            b.flag_bits.0,
            b.facing_angle,
        );
        let at = format!(
            "round {round}: state {state:#x} slot {slot} target {target} anim \
             {current}/{queued} tags {list:x?} live {live:?} anchor {anchor:?}"
        );
        assert_eq!(retail, port, "{at}");
        assert_eq!(retail_pairs, (h.live, h.anchor), "pairs, {at}");
        shoved += i32::from(retail_pairs.0 != live);
        seen.insert((state, retail.0));
    }
    assert!(seen.len() >= 9 && shoved > 300, "{seen:x?} {shoved}");
}

const APPLIER: u32 = 0x8004_02F4;
const CAST_CUE: u32 = 0x801F_3990;

/// States `0x3C`, `0x3D`, `0x3F` and `0x40`, the cast and item band around the
/// applier (`0x801E3B20..0x801E3E10`, `0x801E40D8..0x801E4294`) against
/// `spirit_pre_arm`, `spirit_wait`, `spirit_fire_damage` and
/// `spirit_post_damage`: the clip staged, the class pair off the spell record
/// or the item table, the MP price with the two cost passives, the readout
/// bar's member and the element calls; the wait; the countdown into the
/// applier `FUN_800402F4(class, tier, target, character)`; and the afterglow's
/// Fury Boost bar and plate, its countdown, its hold on a reacting target and
/// its exit.
#[test]
fn the_cast_and_item_band_is_retails() {
    differential!(prot(898, SM));
    let Some(mut r) = Retail::new() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let applied = Rc::new(RefCell::new(Vec::new()));
    let sink = applied.clone();
    r.cpu.hook(APPLIER, move |regs, _| {
        sink.borrow_mut().push((
            regs.r[4] as u8,
            regs.r[5] as u8,
            regs.r[6] as u8,
            regs.r[7] as u8,
        ))
    });
    r.cpu.stub(CAST_CUE, 0);
    let m = &mut r.cpu.mem;
    let spells: Vec<[u8; 12]> = (0..256u32)
        .map(|id| std::array::from_fn(|k| m.rd8(0x8007_54C8 + id * 12 + k as u32)))
        .collect();
    let item_pairs: Vec<(u8, u8)> = (0..256u32)
        .map(|id| {
            let e = u32::from(m.rd8(0x8007_4368 + id * 12 + 1));
            (m.rd8(0x8007_52C0 + e * 4), m.rd8(0x8007_52C0 + e * 4 + 1))
        })
        .collect();
    let mut rng = Rng(0x4341_5354);
    let mut seen = std::collections::HashSet::new();
    for round in 0..30000 {
        let state = [0x3Cu8, 0x3D, 0x3F, 0x40][rng.below(4) as usize];
        let slot = rng.below(5) as u8;
        let target = [0u8, 1, 3, 4, 7, 8, 9][rng.below(7) as usize];
        let category = [1u8, 2, 2, 0][rng.below(4) as usize];
        let id = if rng.below(8) == 0 {
            0xFE
        } else {
            rng.below(0x90) as u8
        };
        // Retail's spend is a bare `subu` and wraps below the price; the
        // port floors it at zero. The menus and the picker only cast what the
        // pool can pay, so the pool here always covers the price.
        let mp = [0x100u16, 999][rng.below(2) as usize];
        let bits = [0u32, 0x10, 0x20, 0x30][rng.below(4) as usize];
        let current = [0u8, 0, 3][rng.below(3) as usize];
        let queued = [0u8, 3][rng.below(2) as usize];
        let staged = rng.below(0x20) as u8;
        // `0x3F` is entered on `0x3E`'s `0x20`, so its countdown starts at or
        // above zero and leaves on the pass it goes negative.
        let timer = if state == 0x3F {
            [0i16, 1, 2, 0x20][rng.below(4) as usize]
        } else {
            [-2i16, -1, 0, 1, 2, 0x80][rng.below(6) as usize]
        };
        let dt = 1 + rng.below(2) as u8;
        let class = [5u8, 5, 1, 0x14, 0x0E][rng.below(5) as usize];
        let tier = rng.below(4) as u8;
        let goal = [0i16, 0x20, 0x120][rng.below(3) as usize];
        let bar = [0i16, 0x19, 0x11A][rng.below(3) as usize];
        let plate = [0i16, 40, 100][rng.below(3) as usize];
        let plate_target = [40i16, 100][rng.below(2) as usize];
        let t_hp = [0u16, 30][rng.below(2) as usize];
        let t_anim = [0u8, 4][rng.below(2) as usize];
        let t_colour = [0u32, 0x2008_0200][rng.below(2) as usize];
        if target < 8 && target == slot {
            continue;
        }

        r.reset();
        applied.borrow_mut().clear();
        let m = &mut r.cpu.mem;
        m.wr8(FRAME_STEP, dt);
        m.wr8(CTX + 7, state);
        m.wr8(CTX + 0x13, slot);
        m.wr16(S7 + 2, timer as u16);
        m.wr16(S7 + 6, goal as u16);
        m.wr16(S7 + 8, plate_target as u16);
        m.wr32(PLATE, PLATE_RECORD);
        m.wr16(PLATE_RECORD + 0x10, plate as u16);
        m.wr32(CTX + 0x1080, BAR_RECORD);
        m.wr16(BAR_RECORD + 0xE, bar as u16);
        for s in 0..8u32 {
            m.wr8(SEATS + s, if s < 3 { s as u8 + 1 } else { 4 });
            if s < 3 {
                m.wr32(RECORDS + s * 0x414 + 0x6BC, bits);
            }
        }
        let a = actor(slot);
        m.wr8(a + 0x1D9, current);
        m.wr8(a + 0x1DA, queued);
        m.wr8(a + 0x1DD, target);
        m.wr8(a + 0x1DE, category);
        m.wr8(a + 0x1DF, id);
        m.wr8(a + 0x1E7, staged);
        m.wr8(a + 0x1E8, class);
        m.wr8(a + 0x1E9, tier);
        m.wr16(a + 0x150, mp);
        if target < 8 {
            let t = actor(target);
            m.wr16(t + 0x14C, t_hp);
            m.wr8(t + 0x1D9, t_anim);
            m.wr32(t + 4, t_colour);
        }
        r.pass();
        let m = &mut r.cpu.mem;
        let retail = (
            m.rd8(CTX + 7),
            m.rd16(S7 + 2) as i16,
            m.rd8(a + 0x1DA),
            m.rd8(a + 0x1DE),
            m.rd8(a + 0x1E8),
            m.rd8(a + 0x1E9),
            m.rd16(a + 0x150),
            m.rd16(a + 0x178),
            m.rd8(CTX + 0x243),
        );
        let retail_hud = (
            m.rd16(BAR_RECORD + 0xE) as i16,
            m.rd16(PLATE_RECORD + 0x10) as i16,
            m.rd16(CTX + 0x6D0) as i16,
        );

        let mut h = host();
        h.dt = i16::from(dt);
        h.bits = bits;
        h.spells = spells.clone();
        h.item_pairs = item_pairs.clone();
        let b = &mut h.actors[usize::from(slot)];
        b.current_anim = current;
        b.queued_anim = queued;
        b.active_target = target;
        b.action_category = category;
        b.params[0] = id;
        b.queued_anim_b = staged;
        b.cast_class = class;
        b.cast_sub_class = tier;
        b.mp = mp;
        if target < 8 {
            let t = &mut h.actors[usize::from(target)];
            t.liveness = t_hp;
            t.hp = t_hp;
            t.current_anim = t_anim;
            h.drawn[usize::from(target)] = t_colour != 0;
        }
        let mut ctx = BattleActionCtx {
            action_state: state,
            active_actor: slot,
            frame_timer: timer,
            damage_target: goal,
            hp_bar_target: plate_target,
            spirit_plate_value: plate,
            gauge_extend_bar: bar,
            ..Default::default()
        };
        ba::step(&mut h, &mut ctx);
        let b = &h.actors[usize::from(slot)];
        let port = (
            ctx.action_state,
            ctx.frame_timer,
            b.queued_anim,
            b.action_category,
            b.cast_class,
            b.cast_sub_class,
            b.mp,
            b.last_mp_cost,
            ctx.gauge_rearm_latch,
        );
        let at = format!(
            "round {round}: state {state:#x} slot {slot} target {target} category {category} \
             id {id:#x} mp {mp} bits {bits:#x} anim {current}/{queued} timer {timer} dt {dt} \
             class {class}/{tier} goal {goal} bar {bar} plate {plate}->{plate_target} target \
             {t_hp}/{t_anim}/{t_colour:#x}"
        );
        assert_eq!(retail, port, "{at}");
        if state == 0x40 {
            let depth = if ctx.camera_frame_height != 0 {
                ctx.camera_frame_height
            } else {
                0
            };
            assert_eq!(
                retail_hud,
                (ctx.gauge_extend_bar, ctx.spirit_plate_value, depth),
                "afterglow, {at}"
            );
        }
        if state == 0x3C {
            assert_eq!(*r.ui.borrow(), h.ui, "elements, {at}");
            assert_eq!(retail_hud.2, ctx.camera_frame_height, "depth, {at}");
        }
        let retail_applied: Vec<_> = applied
            .borrow()
            .iter()
            .copied()
            .filter(|c| ba::effect_selector_dispatches(c.0))
            .collect();
        assert_eq!(retail_applied, h.applied, "applier, {at}");
        seen.insert((state, retail.0));
    }
    assert!(seen.len() >= 7, "{seen:x?}");
}

const CAMERA_SIZE: u32 = 0x801F_0348;

/// State `0x3E`, the cast's release (`0x801E3E14..0x801E40D4`) against
/// `spirit_fire`: the wait on the clip, the element calls (the `0x4C` close,
/// a party spell's name plate, the target plates or the gauge extension's
/// two bars), the gauge extension's `rand()` camera variant, extended gauge
/// and AP target, and the Point Card's selector and three-quarter frame. The
/// frame sizing `FUN_801F0348` answers through `camera_height_for_frame`.
#[test]
fn the_cast_release_is_retails() {
    use legaia_engine_battle_vm::battle_formulas::camera_height_for_frame;
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
    r.cpu.hook(CAMERA_SIZE, |regs, m| {
        let slot = m.rd8(CTX + 0x13);
        let target = m.rd8(actor(slot) + 0x1DD);
        let h = camera_height_for_frame(slot, target, 3, |_| 0);
        m.wr16(CTX + 0x6D0, h as u16);
        regs.r[2] = 0;
    });
    r.cpu.stub(0x8003_19A8, 0);
    // The plate record the `0x52` raise answers with.
    r.cpu.unhook(ELEMENT);
    let ui = r.ui.clone();
    r.cpu.hook(ELEMENT, move |regs, _| {
        ui.borrow_mut().push((regs.r[4] as u8, regs.r[5] as u8));
        regs.r[2] = PLATE_RECORD;
    });
    let mut rng = Rng(0x5245_4C45);
    let (mut extended, mut carded) = (0, 0);
    for round in 0..20000 {
        let slot = rng.below(5) as u8;
        let target = [0u8, 1, 2, 3, 4, 8][rng.below(6) as usize];
        let category = [1u8, 2, 3][rng.below(3) as usize];
        let class = [5u8, 1, 0x14][rng.below(3) as usize];
        let head = if rng.below(4) == 0 {
            0xFE
        } else {
            rng.below(0x80) as u8
        };
        let current = [0u8, 0, 4][rng.below(3) as usize];
        let base = [0u16, 7, 100, 250][rng.below(4) as usize];
        let spirit = [0u16, 50, 95][rng.below(3) as usize];
        let bits = [0u32, 0x200][rng.below(2) as usize];
        let depth = [0x800i16, 0x1400][rng.below(2) as usize];
        let seed = rng.next();
        if target == slot {
            continue;
        }
        r.reset();
        stream.set(seed);
        let m = &mut r.cpu.mem;
        m.wr8(CTX + 7, 0x3E);
        m.wr8(CTX + 0x13, slot);
        m.wr16(CTX + 0x6D0, depth as u16);
        for s in 0..8u32 {
            m.wr8(SEATS + s, if s < 3 { s as u8 + 1 } else { 4 });
            if s < 3 {
                m.wr32(RECORDS + s * 0x414 + 0x6C0, bits);
            }
        }
        let a = actor(slot);
        m.wr8(a + 0x1D9, current);
        m.wr8(a + 0x1DD, target);
        m.wr8(a + 0x1DE, category);
        m.wr8(a + 0x1DF, head);
        m.wr8(a + 0x1E8, class);
        m.wr16(a + 0x170, spirit);
        if target < 8 {
            m.wr16(actor(target) + 0x156, base);
        }
        r.pass();
        let m = &mut r.cpu.mem;
        let retail = (
            m.rd8(CTX + 7),
            m.rd16(S7 + 2) as i16,
            m.rd16(S7 + 6) as i16,
            m.rd16(S7 + 8) as i16,
            m.rd8(CTX + 0x19),
            m.rd8(CTX + 0x0D),
            m.rd8(a + 0x1E8),
            m.rd8(CTX + 0x18),
            m.rd16(CTX + 0x6D0) as i16,
        );
        let retail_member = m.rd8(CTX + 0x20);

        let mut h = host();
        h.bits_high = bits;
        h.rand = seed;
        let b = &mut h.actors[usize::from(slot)];
        b.current_anim = current;
        b.active_target = target;
        b.action_category = category;
        b.params[0] = head;
        b.cast_class = class;
        b.spirit_gauge = spirit;
        if target < 8 {
            h.actors[usize::from(target)].agl_base = base;
        }
        let mut ctx = BattleActionCtx {
            action_state: 0x3E,
            active_actor: slot,
            camera_frame_height: depth,
            ..Default::default()
        };
        ba::step(&mut h, &mut ctx);
        let b = &h.actors[usize::from(slot)];
        let fired = current == 0;
        let port = (
            ctx.action_state,
            ctx.frame_timer,
            ctx.damage_target,
            ctx.hp_bar_target,
            ctx.spirit_action_count,
            ctx.camera_variant,
            b.cast_class,
            ctx.action_ui_element,
            ctx.camera_frame_height,
        );
        let at = format!(
            "round {round}: slot {slot} target {target} category {category} class {class} head \\
             {head:#x} clip {current} base {base} spirit {spirit} bits {bits:#x}"
        );
        assert_eq!(retail, port, "{at}");
        assert_eq!(*r.ui.borrow(), h.ui, "elements, {at}");
        assert_eq!(stream.get(), h.rand, "rand, {at}");
        if fired && target < 3 && class != 5 {
            assert_eq!(retail_member, ctx.readout_bar_member, "{at}");
        }
        extended += i32::from(fired && class == 5);
        carded += i32::from(fired && class != 5 && head == 0xFE);
    }
    assert!(extended > 1000 && carded > 500, "{extended} {carded}");
}
