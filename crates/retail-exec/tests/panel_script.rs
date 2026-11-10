//! The field overlay's panel-script interpreter `FUN_801E9B3C` and the shared
//! list cursor `FUN_801E9DC8` (PROT 0897) against
//! `legaia_engine_field::world_map_panel`.
//!
//! The interpreter is glue over the SCUS window kernel, so the kernel is
//! hooked and logged: the lookup `FUN_80035334`, the spawn `FUN_80032434`, the
//! slide `FUN_800357FC`, the snap `FUN_800358C0`, the close `FUN_80035978`,
//! the close-all `FUN_80035A4C` and the retire `FUN_800319A8`. The lookup
//! answers from a table of open windows the spawn and the retire keep, and the
//! port's effects are unfolded into the calls a host makes for them over the
//! same table.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_field::world_map_panel::{
    CursorPad, PanelCommand, PanelDescriptor, PanelEffect, list_cursor_input, run_panel_script,
};
use psx_retail_exec::{A0, A1, A2, Stop, V0};
use std::cell::RefCell;
use std::rc::Rc;

const OVERLAY_BASE: u32 = 0x801C_E818;
const INSTALL: u32 = 0x801E_9B3C;
const CURSOR: u32 = 0x801E_9DC8;

const FIND: u32 = 0x8003_5334;
const SPAWN: u32 = 0x8003_2434;
const SLIDE: u32 = 0x8003_57FC;
const SNAP: u32 = 0x8003_58C0;
const CLOSE: u32 = 0x8003_5978;
const CLOSE_ALL: u32 = 0x8003_5A4C;
const RETIRE: u32 = 0x8003_19A8;
const CUE: u32 = 0x8003_5B50;

const TABLE: u32 = 0x801F_2B98;
const STRIDE: u32 = 0x1C;
const PARTY_SCRIPT: u32 = 0x801F_3170;
const ROSTER_COUNT: u32 = 0x8008_4594;
const SCRIPT: u32 = 0x8012_0000;
const OBJECTS: u32 = 0x8012_4000;
const PANELS: usize = 17;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Call {
    Find(i16),
    Spawn(i16, u32),
    Slide(i16, i16, i16),
    Snap(i16, i16, i16),
    Close(i16),
    CloseAll,
    Retire(i16),
}

/// The window table both sides share: which panels have an object, and each
/// object's position, style byte and counter.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Windows {
    open: [bool; PANELS],
    pos: [(i16, i16); PANELS],
    style: [u8; PANELS],
    counter: [u16; PANELS],
}

fn object(panel: i16) -> u32 {
    OBJECTS + (panel as u32) * 0x40
}

/// Unfold the port's effects into the kernel calls a host makes for them.
fn unfold(
    effects: &[PanelEffect],
    w: &mut Windows,
    desc: &mut [PanelDescriptor; PANELS],
    nested: &[PanelCommand],
    members: u8,
    out: &mut Vec<Call>,
) {
    let ensure = |panel: i16, w: &mut Windows, out: &mut Vec<Call>| {
        out.push(Call::Find(panel));
        if !w.open[panel as usize] {
            out.push(Call::Spawn(panel, TABLE + panel as u32 * STRIDE));
            w.open[panel as usize] = true;
        }
    };
    for e in effects {
        match *e {
            PanelEffect::OpenAtDescriptor { panel, x, y } | PanelEffect::OpenAt { panel, x, y } => {
                ensure(panel, w, out);
                out.push(Call::Slide(panel, x, y));
            }
            PanelEffect::SnapTo { panel, x, y } => {
                ensure(panel, w, out);
                out.push(Call::Snap(panel, x, y));
            }
            PanelEffect::SetStyleByte { panel, value } => {
                out.push(Call::Find(panel));
                if w.open[panel as usize] {
                    w.style[panel as usize] = value;
                }
            }
            PanelEffect::ClearCounter { panel } => {
                out.push(Call::Find(panel));
                if w.open[panel as usize] {
                    w.counter[panel as usize] = 0;
                }
            }
            PanelEffect::Close { panel } => out.push(Call::Close(panel)),
            PanelEffect::CloseAll => out.push(Call::CloseAll),
            PanelEffect::Retire { panel } => {
                out.push(Call::Retire(panel));
                w.open[panel as usize] = false;
            }
            PanelEffect::Respawn { panel } => {
                out.push(Call::Find(panel));
                if w.open[panel as usize] {
                    let (x, y) = w.pos[panel as usize];
                    out.push(Call::Retire(panel));
                    out.push(Call::Spawn(panel, TABLE + panel as u32 * STRIDE));
                    out.push(Call::Snap(panel, x, y));
                }
            }
            PanelEffect::PartyPanel { panel, geometry } => {
                let d = &mut desc[panel as usize];
                d.y = geometry.y;
                d.height = geometry.height;
                let snapshot = *desc;
                let inner = run_panel_script(nested, members, |p| snapshot[p as usize]);
                unfold(&inner, w, desc, &[], members, out);
            }
            PanelEffect::Nop => {}
        }
    }
}

#[test]
fn a_panel_script_makes_the_calls_retail_makes() {
    differential!(prot(897, INSTALL));
    let (Some(mut cpu), Some(disc)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem
        .load(OVERLAY_BASE, &disc.read_entry(897).expect("PROT 0897"));
    let log: Rc<RefCell<Vec<Call>>> = Rc::new(RefCell::new(Vec::new()));
    let open: Rc<RefCell<[bool; PANELS]>> = Rc::new(RefCell::new([false; PANELS]));
    let (l, o) = (log.clone(), open.clone());
    cpu.hook(FIND, move |r, _| {
        let p = r.r[A0] as i16;
        l.borrow_mut().push(Call::Find(p));
        r.r[V0] = if o.borrow()[p as usize] { object(p) } else { 0 };
    });
    let (l, o) = (log.clone(), open.clone());
    cpu.hook(SPAWN, move |r, _| {
        let p = r.r[A0] as i16;
        l.borrow_mut().push(Call::Spawn(p, r.r[A1]));
        o.borrow_mut()[p as usize] = true;
    });
    let (l, o) = (log.clone(), open.clone());
    cpu.hook(RETIRE, move |r, _| {
        let p = r.r[A0] as i16;
        l.borrow_mut().push(Call::Retire(p));
        o.borrow_mut()[p as usize] = false;
    });
    for (at, make) in [
        (
            SLIDE,
            (|p, x, y| Call::Slide(p, x, y)) as fn(i16, i16, i16) -> Call,
        ),
        (SNAP, |p, x, y| Call::Snap(p, x, y)),
        (CLOSE, |p, _, _| Call::Close(p)),
        (CLOSE_ALL, |_, _, _| Call::CloseAll),
    ] {
        let l = log.clone();
        cpu.hook(at, move |r, _| {
            l.borrow_mut()
                .push(make(r.r[A0] as i16, r.r[A1] as i16, r.r[A2] as i16));
        });
    }
    // The party arm's nested script, as the image holds it.
    let mut nested = Vec::new();
    for i in 0.. {
        let at = PARTY_SCRIPT + i * 8;
        let op = cpu.mem.rd16(at);
        if op == 0 {
            break;
        }
        nested.push(PanelCommand {
            op,
            panel: cpu.mem.rd16(at + 2) as i16,
            operand: cpu.mem.rd32(at + 4),
        });
    }
    assert!(!nested.is_empty(), "the party script is empty");

    let mut rng = Rng(0x5041_4E4C);
    let (mut spawned, mut respawned, mut party, mut nops) = (0, 0, 0, 0);
    for round in 0..3000 {
        let mut w = Windows::default();
        let mut desc = [PanelDescriptor::default(); PANELS];
        for p in 0..PANELS {
            w.open[p] = rng.below(2) == 0;
            w.pos[p] = (rng.below(320) as i16 - 20, rng.below(240) as i16 - 20);
            w.style[p] = rng.below(256) as u8;
            w.counter[p] = 1 + rng.below(100) as u16;
            desc[p] = PanelDescriptor {
                x: rng.below(320) as i16 - 20,
                y: rng.below(240) as i16 - 20,
                height: rng.below(200) as i16,
            };
            let rec = TABLE + p as u32 * STRIDE;
            cpu.mem.wr16(rec + 0x08, desc[p].x as u16);
            cpu.mem.wr16(rec + 0x0A, desc[p].y as u16);
            cpu.mem.wr16(rec + 0x0E, desc[p].height as u16);
            cpu.mem.wr16(rec + 0x12, 0x7777);
            let obj = object(p as i16);
            cpu.mem.wr16(obj + 0x0A, w.pos[p].0 as u16);
            cpu.mem.wr16(obj + 0x0C, w.pos[p].1 as u16);
            cpu.mem.wr8(obj + 0x1D, w.style[p]);
            cpu.mem.wr16(obj + 0x20, w.counter[p]);
        }
        *open.borrow_mut() = w.open;
        let members = rng.below(5) as u8;
        cpu.mem.wr8(ROSTER_COUNT, members);
        let len = rng.below(7) as usize;
        let mut script = Vec::new();
        for _ in 0..len {
            let op = match rng.below(12) {
                0 => 14 + rng.below(40) as u16,
                1 => 0xFFFF - rng.below(3) as u16,
                _ => 1 + rng.below(13) as u16,
            };
            script.push(PanelCommand {
                op,
                panel: rng.below(PANELS as u32) as i16,
                operand: match rng.below(4) {
                    0 => 0,
                    1 => rng.next(),
                    _ => (rng.below(240) << 16) | rng.below(320),
                },
            });
        }
        for (i, c) in script.iter().enumerate() {
            let at = SCRIPT + i as u32 * 8;
            cpu.mem.wr16(at, c.op);
            cpu.mem.wr16(at + 2, c.panel as u16);
            cpu.mem.wr32(at + 4, c.operand);
        }
        cpu.mem.wr32(SCRIPT + len as u32 * 8, 0);
        cpu.mem.wr32(SCRIPT + len as u32 * 8 + 4, 0x1234_5678);
        log.borrow_mut().clear();
        assert_eq!(cpu.call(INSTALL, &[SCRIPT]), Stop::Returned, "round {round}");
        // The interpreter returns the terminator's address.
        assert_eq!(cpu.regs.r[V0], SCRIPT + len as u32 * 8, "round {round}");

        // The port decodes record by record against the descriptors as they
        // stand, the way its host does; the party arm rewrites descriptor 7
        // for every record after it.
        let mut want = Vec::new();
        for c in &script {
            let snapshot = desc;
            let effects = run_panel_script(&[*c], members, |p| snapshot[p as usize]);
            party += u32::from(matches!(
                effects.first(),
                Some(PanelEffect::PartyPanel { .. })
            ));
            nops += u32::from(effects.first() == Some(&PanelEffect::Nop));
            unfold(&effects, &mut w, &mut desc, &nested, members, &mut want);
        }
        let ctx = format!("round {round} script {script:?}");
        let got = log.borrow().clone();
        assert_eq!(got, want, "calls: {ctx}");
        spawned += got.iter().filter(|c| matches!(c, Call::Spawn(..))).count();
        respawned += got
            .windows(2)
            .filter(|p| matches!(p, [Call::Retire(_), Call::Spawn(..)]))
            .count();
        for p in 0..PANELS {
            let (rec, obj) = (TABLE + p as u32 * STRIDE, object(p as i16));
            assert_eq!(cpu.mem.rd16(rec + 0x0A) as i16, desc[p].y, "y {p}: {ctx}");
            assert_eq!(
                cpu.mem.rd16(rec + 0x0E) as i16,
                desc[p].height,
                "height {p}: {ctx}"
            );
            assert_eq!(cpu.mem.rd8(obj + 0x1D), w.style[p], "style {p}: {ctx}");
            assert_eq!(cpu.mem.rd16(obj + 0x20), w.counter[p], "counter {p}: {ctx}");
        }
    }
    assert!(
        spawned > 500 && respawned > 100 && party > 100 && nops > 300,
        "{spawned} {respawned} {party} {nops}"
    );
}

#[test]
fn the_list_cursor_steps_as_retail_does() {
    differential!(prot(897, CURSOR));
    let (Some(mut cpu), Some(disc)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem
        .load(OVERLAY_BASE, &disc.read_entry(897).expect("PROT 0897"));
    let cues: Rc<RefCell<Vec<u32>>> = Rc::new(RefCell::new(Vec::new()));
    let c = cues.clone();
    cpu.hook(CUE, move |r, _| c.borrow_mut().push(r.r[A0]));
    const CELL: u32 = 0x8012_0000;
    let mut rng = Rng(0x4355_5253);
    let (mut moved, mut wrapped, mut acted, mut held) = (0, 0, 0, 0);
    for round in 0..20000 {
        let count = 1 + rng.below(5) as i32;
        let start = rng.below(count as u32 + 1) as i32 - i32::from(rng.below(16) == 0);
        let wrap = rng.below(2) == 0;
        let pad = CursorPad {
            held: match rng.below(4) {
                0 => rng.next() & 0xFFFF,
                _ => 0,
            },
            pressed: match rng.below(8) {
                0 => 0,
                1 | 2 | 3 => 0x1000,
                4 | 5 | 6 => 0x4000,
                _ => 0x5000,
            } | (rng.next() & 0xAFFF & if rng.below(2) == 0 { 0 } else { !0 }),
            action_a_mask: 1 << rng.below(16),
            action_b_mask: 1 << rng.below(16),
        };
        cpu.mem.wr32(CELL, start as u32);
        cpu.mem.wr32(0x8007_B874, pad.held);
        cpu.mem.wr32(0x8007_BB84, pad.pressed);
        cpu.mem.wr32(0x8008_46D0, pad.action_a_mask);
        cpu.mem.wr32(0x8008_46D4, pad.action_b_mask);
        cues.borrow_mut().clear();
        // The wrap flag is read as a byte.
        let flag = u32::from(wrap) | (rng.below(2) << 8);
        let wrap = flag & 0xFF != 0;
        assert_eq!(
            cpu.call(CURSOR, &[CELL, count as u32, flag]),
            Stop::Returned,
            "round {round}"
        );
        let mut cursor = start;
        let (outcome, sfx) = list_cursor_input(&mut cursor, count, wrap, &pad);
        let ctx = format!("round {round} start {start} count {count} wrap {wrap} {pad:?}");
        assert_eq!(cpu.regs.r[V0] as i32, outcome.code(), "outcome: {ctx}");
        assert_eq!(cpu.mem.rd32(CELL) as i32, cursor, "cursor: {ctx}");
        // Up and Down on one frame cue twice in wrapping mode; the port
        // reports the cue once.
        assert_eq!(cues.borrow().last().copied(), sfx, "cue: {ctx}");
        moved += u32::from(outcome.code() == 3);
        wrapped += u32::from(wrap && (cursor - start).abs() > 1);
        acted += u32::from(matches!(outcome.code(), 1 | 2));
        held += u32::from(outcome.code() == 0 && pad.pressed & 0x5000 != 0);
    }
    assert!(
        moved > 3000 && wrapped > 300 && acted > 500 && held > 500,
        "{moved} {wrapped} {acted} {held}"
    );
}
