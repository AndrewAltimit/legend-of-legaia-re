//! The world-map / field entity tick `FUN_801DA51C` (PROT 0897) against
//! `world_map_entity::step`, over every state, the door gate, the shared
//! encounter countdown and rate byte, the dialog bit, the player's and the
//! entity's movement-block and cooldown bits and the `-1000` sentinel. The
//! callees are answered by hooks that record what retail asked for - the
//! camera ease `FUN_801DA390`, the BGM poll `FUN_800243F0` (whose return the
//! encounter roll `FUN_801D9E1C` receives), the activation's
//! `FUN_8004313C`, the party-table rebuild, the story-flag calls and the
//! interaction `FUN_80039B7C` - and the port's host records the same events.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_field::world_map_entity::{WorldMapEntityCtx, WorldMapEntityHost, step};
use psx_retail_exec::{A0, A1, Cpu, Stop, V0};
use std::cell::RefCell;
use std::rc::Rc;

const TICK: u32 = 0x801D_A51C;
const OVERLAY_BASE: u32 = 0x801C_E818;
const ENTITY: u32 = 0x8013_0000;
const PLAYER: u32 = 0x8013_1000;
const RECORD: u32 = 0x8013_2000;
const PLAYER_PTR: u32 = 0x8007_C364;
const GATE: u32 = 0x8007_B868;
const COUNTDOWN: u32 = 0x8007_B604;
const RATE: u32 = 0x8007_B5F8;
const SCRATCH_FLAGS: u32 = 0x1F80_0394;
const SENTINEL: u32 = 0x8007_B6B0;

#[derive(Debug, Clone, PartialEq, Eq)]
enum Event {
    Encounter(u32),
    Activating,
    Transition,
    Interact,
}

type Events = Rc<RefCell<Vec<Event>>>;

#[derive(Default)]
struct Host {
    gate_open: bool,
    countdown: u8,
    rate: bool,
    dialog: bool,
    player_flags: u32,
    sentinel: i32,
    poll: u32,
    events: Vec<Event>,
}

impl WorldMapEntityHost for Host {
    fn activation_gate_open(&self) -> bool {
        self.gate_open
    }
    fn bgm_poll(&mut self) -> u32 {
        self.poll
    }
    fn set_player_movement_blocked(&mut self, blocked: bool) {
        if blocked {
            self.player_flags |= 0x80000;
        } else {
            self.player_flags &= !0x80000;
        }
    }
    fn encounter_countdown(&self) -> i8 {
        self.countdown as i8
    }
    fn set_encounter_countdown(&mut self, v: i8) {
        self.countdown = v as u8;
    }
    fn encounter_enabled(&self) -> bool {
        self.rate
    }
    fn on_encounter(&mut self, _idx: usize, resolver: u32) {
        self.events.push(Event::Encounter(resolver));
    }
    fn on_activating(&mut self, _idx: usize) {
        self.events.push(Event::Activating);
    }
    fn on_scene_transition(&mut self, _idx: usize) {
        self.events.push(Event::Transition);
    }
    fn dialog_active(&self) -> bool {
        self.dialog
    }
    fn player_walking(&self) -> bool {
        self.player_flags & 0x80000 != 0
    }
    fn on_interact(&mut self, _idx: usize) {
        self.events.push(Event::Interact);
    }
    fn encounter_counter_is_sentinel(&self) -> bool {
        self.sentinel == -1000
    }
    fn clear_encounter_counter(&mut self) {
        self.sentinel = 0;
    }
}

fn retail_cpu(events: &Events, poll: &Rc<RefCell<u32>>) -> Option<Cpu> {
    let mut cpu = scus_cpu()?;
    cpu.mem
        .load(OVERLAY_BASE, &disc()?.read_entry(897).expect("PROT 0897"));
    cpu.mem.wr32(PLAYER_PTR, PLAYER);
    cpu.stub(0x801D_A390, 0);
    let p = poll.clone();
    cpu.hook(0x8002_43F0, move |r, _| r.r[V0] = *p.borrow());
    let e = events.clone();
    cpu.hook(0x801D_9E1C, move |r, _| {
        assert_eq!(r.r[A0], ENTITY);
        e.borrow_mut().push(Event::Encounter(r.r[A1]));
    });
    let e = events.clone();
    cpu.hook(0x8004_313C, move |_, _| {
        e.borrow_mut().push(Event::Activating)
    });
    cpu.stub(0x801D_E190, 0);
    cpu.stub(0x8003_CE64, 0);
    cpu.stub(0x8003_CE08, 0);
    let e = events.clone();
    cpu.hook(0x8003_CE34, move |r, _| {
        if r.r[A0] == 0x35 {
            e.borrow_mut().push(Event::Transition);
        }
    });
    let e = events.clone();
    cpu.hook(0x8003_9B7C, move |r, _| {
        assert_eq!(r.r[A0], ENTITY);
        e.borrow_mut().push(Event::Interact);
    });
    Some(cpu)
}

#[test]
fn the_entity_tick_matches_retail() {
    differential!(prot(897, TICK));
    let events: Events = Rc::default();
    let poll = Rc::new(RefCell::new(0u32));
    let Some(mut cpu) = retail_cpu(&events, &poll) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut rng = Rng(0x4441_3531);
    let mut seen = [0usize; 4];
    for round in 0..200_000 {
        let state = [0u16, 0, 0, 1, 1, 2, 3, 4, 5, 0xFFFF][rng.below(10) as usize];
        let entity_flags = rng.next() & 0x0018_0FFF;
        let mut host = Host {
            gate_open: rng.below(4) != 0,
            countdown: [0u8, 0, 1, 2, 0x80, 0xFF][rng.below(6) as usize],
            rate: rng.below(2) == 0,
            dialog: rng.below(4) == 0,
            player_flags: rng.next() & 0x0018_0000,
            sentinel: [0i32, -1000, 5][rng.below(3) as usize],
            poll: [0u32, u32::MAX, 3, 6][rng.below(4) as usize],
            events: Vec::new(),
        };
        let field_88 = rng.next() as u16;
        let mut ctx = WorldMapEntityCtx {
            state,
            pad_flags: entity_flags,
            field_88,
        };

        cpu.mem.load(ENTITY, &[0u8; 0x100]);
        cpu.mem.wr32(ENTITY + 0x10, entity_flags);
        cpu.mem.wr16(ENTITY + 0x88, field_88);
        cpu.mem.wr16(ENTITY + 0x8A, state);
        cpu.mem.load(RECORD, &[0, 0, 0, 2, 7, 9, 0, 0]);
        cpu.mem.wr32(ENTITY + 0x94, RECORD);
        cpu.mem.wr32(PLAYER + 0x10, host.player_flags);
        cpu.mem.wr32(GATE, u32::from(!host.gate_open));
        cpu.mem.wr8(COUNTDOWN, host.countdown);
        cpu.mem.wr8(RATE, u8::from(host.rate));
        cpu.mem
            .wr32(SCRATCH_FLAGS, if host.dialog { 0x8000 } else { 0 });
        cpu.mem.wr32(SENTINEL, host.sentinel as u32);
        *poll.borrow_mut() = host.poll;
        events.borrow_mut().clear();

        assert_eq!(cpu.call(TICK, &[ENTITY]), Stop::Returned, "round {round}");
        step(0, &mut ctx, &mut host);

        let label = format!(
            "round {round}: state {state} gate {} cd {}",
            host.gate_open, host.countdown
        );
        let retail = (
            cpu.mem.rd16(ENTITY + 0x8A),
            cpu.mem.rd16(ENTITY + 0x88),
            cpu.mem.rd32(ENTITY + 0x10),
            cpu.mem.rd32(PLAYER + 0x10),
            cpu.mem.rd8(COUNTDOWN),
            cpu.mem.rd32(SENTINEL) as i32,
        );
        let port = (
            ctx.state,
            ctx.field_88,
            ctx.pad_flags,
            host.player_flags,
            host.countdown,
            host.sentinel,
        );
        assert_eq!(retail, port, "{label}");
        assert_eq!(*events.borrow(), host.events, "{label}: calls");
        for e in &host.events {
            seen[match e {
                Event::Encounter(_) => 0,
                Event::Activating => 1,
                Event::Transition => 2,
                Event::Interact => 3,
            }] += 1;
        }
    }
    assert!(seen.iter().all(|&n| n > 1000), "{seen:?}");
}
