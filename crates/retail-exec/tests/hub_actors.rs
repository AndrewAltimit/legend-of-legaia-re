//! The field overlay's op-`0x49` submode hub family (PROT 0897) against
//! `legaia_engine_field::submode_hub_actors`: the dispatcher `FUN_801F159C`,
//! the six panel state machines it indexes through `PTR_FUN_801F33B4`, and the
//! seven window painters a panel record names.
//!
//! Hooked and logged: the glyph strings `FUN_80036888` / `FUN_8003CD00` /
//! `FUN_8003CC98`, the sprite and cell emitters `FUN_8002B994` /
//! `FUN_8002C488`, the screen-effect push `FUN_80024EE4`, the panel installer
//! `FUN_801E9B3C`, the two stings, the close-all `FUN_80035A4C`, the draw pump
//! `FUN_80031D00`, the submode re-arm `FUN_801F1278` and the entry list's
//! sub-draw `FUN_801E5B4C`. A string draw records the palette word
//! `0x8007B454` as it stands at the call.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_field::submode_hub_actors::{
    HubAction, HubActor, HubDraw, HubEnv, HubFrame, HubGrid, HubString, close_tick, column_row,
    count_gated_label, deactivate, draw_tick, entry_list, hub_dispatch, hub_prompt, single_label,
    slot, start_menu, submenu, three_line_panel, two_line_panel, two_option_panel,
};
use psx_retail_exec::{A0, A1, A2, A3, Cpu, SP, Stop};
use std::cell::RefCell;
use std::rc::Rc;

const OVERLAY_BASE: u32 = 0x801C_E818;

const DISPATCH: u32 = 0x801F_159C;
const START_MENU: u32 = 0x801F_1138;
const SUBMENU: u32 = 0x801F_1E48;
const PROMPT: u32 = 0x801F_1FDC;
const DRAW_TICK: u32 = 0x801F_20B0;
const CLOSE_TICK: u32 = 0x801F_2134;
const DEACTIVATE: u32 = 0x801F_1D90;
const ENTRY_LIST: u32 = 0x801F_16C0;
const COLUMN_ROW: u32 = 0x801F_17D8;
const THREE_LINE: u32 = 0x801F_1890;
const TWO_OPTION: u32 = 0x801F_1950;
const COUNT_GATED: u32 = 0x801F_1A1C;
const TWO_LINE: u32 = 0x801F_1AB0;
const SINGLE_LABEL: u32 = 0x801F_1B64;

const TEXT: u32 = 0x8003_6888;
const SHORT_TEXT: u32 = 0x8003_CD00;
const HEADER_TEXT: u32 = 0x8003_CC98;
const SPRITE: u32 = 0x8002_B994;
const CELL: u32 = 0x8002_C488;
const EFFECT: u32 = 0x8002_4EE4;
const INSTALL_PANEL: u32 = 0x801E_9B3C;
const PUSH_CUE: u32 = 0x8003_5B50;
const REPLACE_CUE: u32 = 0x8003_5BD0;
const CLOSE_ALL: u32 = 0x8003_5A4C;
const DRAW_PUMP: u32 = 0x8003_1D00;
const REARM: u32 = 0x801F_1278;
const SUB_PANEL: u32 = 0x801E_5B4C;

const SUBMODE: u32 = 0x801F_2734;
const PAD_LATCH: u32 = 0x1F80_0394;
const PAD_EDGE: u32 = 0x8007_B874;
const PAD_HELD: u32 = 0x8007_B850;
const CONFIRM_A: u32 = 0x8008_46D0;
const CONFIRM_B: u32 = 0x8008_46D4;
const CANCEL: u32 = 0x8008_46D8;
const BLOCKED: u32 = 0x8007_BB80;
const ROW: u32 = 0x8007_BB88;
const ROW_ALT: u32 = 0x8007_BB98;
const OPERAND_PTR: u32 = 0x8007_B450;
const PALETTE: u32 = 0x8007_B454;
const ENTRY_CODE: u32 = 0x8007_B469;
const PLAYER_PTR: u32 = 0x8007_C364;
const ROSTER_COUNT: u32 = 0x8008_4594;
const ROSTER: u32 = 0x8008_4598;
const PROGRESS_A: u32 = 0x8007_B868;
const PROGRESS_B: u32 = 0x8007_B98C;
const GRID_PTR: u32 = 0x801C_6EA4;
const TEXT_LAYER: u32 = 0x8007_3F20;
const WINDOW_8: u32 = 0x801F_2B98 + 8 * 0x1C;
const LABELS: u32 = 0x801F_29B0;

const ACTOR: u32 = 0x8012_0000;
const GRID: u32 = 0x8012_1000;
const OPERAND: u32 = 0x8012_2000;
const PLAYER: u32 = 0x8012_3000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ev {
    Draw(HubDraw),
    /// A string draw whose pointer the port names by rodata VA.
    Act(HubAction),
    /// `FUN_801E5B4C(actor)`: the entry code and the actor's `+0x0C` it read.
    SubPanel {
        code: u8,
        y: i16,
    },
}

fn hooked(disc_image: &[u8]) -> (Cpu, Rc<RefCell<Vec<Ev>>>) {
    let mut cpu = scus_cpu().expect("checked by the caller");
    cpu.mem.load(OVERLAY_BASE, disc_image);
    let log: Rc<RefCell<Vec<Ev>>> = Rc::new(RefCell::new(Vec::new()));
    let l = log.clone();
    cpu.hook(TEXT, move |r, m| {
        let y = m.rd32(r.r[SP] + 0x10) as i16;
        assert_eq!((r.r[A1], r.r[A2]), (0, 0), "FUN_80036888's two zero args");
        l.borrow_mut().push(Ev::Draw(HubDraw::Text {
            text: HubString::Literal(r.r[A0]),
            x: r.r[A3] as i16,
            y,
            palette: m.rd32(PALETTE) as i32,
        }));
    });
    let l = log.clone();
    cpu.hook(SHORT_TEXT, move |r, m| {
        l.borrow_mut().push(Ev::Draw(HubDraw::ShortText {
            text: HubString::Literal(r.r[A0]),
            x: r.r[A1] as i16,
            y: r.r[A2] as i16,
            palette: m.rd32(PALETTE) as i32,
        }));
    });
    let l = log.clone();
    cpu.hook(HEADER_TEXT, move |r, m| {
        let y = m.rd32(r.r[SP] + 0x10) as i16;
        assert_eq!((r.r[A1], r.r[A2]), (0, 0), "FUN_8003CC98's two zero args");
        l.borrow_mut().push(Ev::Draw(HubDraw::HeaderText {
            text: HubString::Literal(r.r[A0]),
            x: r.r[A3] as i16,
            y,
        }));
    });
    let l = log.clone();
    cpu.hook(SPRITE, move |r, _| {
        l.borrow_mut().push(Ev::Draw(HubDraw::Sprite {
            a: r.r[A0] as i32,
            b: r.r[A1] as i32,
            x: r.r[A2] as i16,
            y: r.r[A3] as i16,
        }));
    });
    let l = log.clone();
    cpu.hook(CELL, move |r, _| {
        l.borrow_mut().push(Ev::Draw(HubDraw::Cell {
            x: r.r[A0] as i16,
            y: r.r[A1] as i16,
            cell: r.r[A2] as i32,
        }));
    });
    let l = log.clone();
    cpu.hook(EFFECT, move |r, _| {
        assert_eq!((r.r[A1], r.r[A2]), (0, 0), "FUN_80024EE4's two zero args");
        l.borrow_mut()
            .push(Ev::Draw(HubDraw::Effect(r.r[A0] as i32)));
    });
    for (at, make) in [
        (
            INSTALL_PANEL,
            (|a| HubAction::InstallPanel(a)) as fn(u32) -> HubAction,
        ),
        (PUSH_CUE, |a| HubAction::EntryCue(a as u8)),
        (REPLACE_CUE, |a| HubAction::ConfirmCue(a as u8)),
        (CLOSE_ALL, |_| HubAction::CloseAllPanels),
        (DRAW_PUMP, |_| HubAction::DrawPump),
        (REARM, |_| HubAction::RearmSubmode),
    ] {
        let l = log.clone();
        cpu.hook(at, move |r, _| {
            l.borrow_mut().push(Ev::Act(make(r.r[A0])));
        });
    }
    let l = log.clone();
    cpu.hook(SUB_PANEL, move |r, m| {
        let actor = r.r[A0];
        l.borrow_mut().push(Ev::SubPanel {
            code: m.rd8(ENTRY_CODE),
            y: m.rd16(actor + 0x0C) as i16,
        });
    });
    cpu.mem.wr32(GRID_PTR, GRID);
    cpu.mem.wr32(OPERAND_PTR, OPERAND);
    cpu.mem.wr32(PLAYER_PTR, PLAYER);
    (cpu, log)
}

/// One generated frame's inputs, written to both sides.
struct Seed {
    actor: HubActor,
    grid: HubGrid,
    env: HubEnv,
    /// The operand pointer cell `0x8007B450` as retail holds it.
    operand_ptr: u32,
}

fn seed(rng: &mut Rng, state: u16) -> Seed {
    let small = |rng: &mut Rng| (rng.below(400) as i16) - 60;
    let actor = HubActor {
        x: small(rng),
        y: small(rng),
        width: small(rng),
        flags: rng.next() & !0x8,
        gate: [0i16, 0, 1, -1][rng.below(4) as usize],
        state,
        sub: [0i16, 0, 1, 1, 1, 2, 3, -1][rng.below(8) as usize],
    };
    let grid = HubGrid {
        handback: rng.below(3) as i16,
        done_gate: [1i16, 1, 1, 0][rng.below(4) as usize],
        stashed_state: rng.below(0x34) as u16,
        columns: (0..8).map(|_| rng.below(0x40) as u8).collect(),
    };
    let entry_count = rng.below(5) as u8;
    let operand_ptr = [OPERAND, OPERAND, 0][rng.below(3) as usize];
    let confirm_a = 1u32 << rng.below(8);
    let confirm_b = 1u32 << rng.below(8);
    let pick = |rng: &mut Rng| match rng.below(3) {
        0 => 0,
        1 => 1u32 << rng.below(8),
        _ => rng.next() & 0xFFFF,
    };
    let env = HubEnv {
        submode: [1, 4, 7, 0, 2, 5][rng.below(6) as usize],
        pad_latch: (rng.next() & !0x8000) | (u32::from(rng.below(4) == 0) << 15),
        pad_edge: pick(rng),
        pad_held: pick(rng),
        confirm_mask: confirm_a | confirm_b,
        accept_mask: confirm_a,
        back_mask: confirm_b,
        cancel_mask: 1u32 << rng.below(8),
        input_blocked: i32::from(rng.below(4) == 0),
        cursor_row: rng.below(4) as i32,
        cursor_row_alt: rng.below(4) as i32,
        board_flag: operand_ptr as i32,
        // A null operand pointer reads the three bytes off the zero page.
        board_entries: [
            rng.below(2) as u8,
            rng.below(2) as u8 * 7,
            rng.below(2) as u8,
        ]
        .map(|b| if operand_ptr == 0 { 0 } else { b }),
        busy_word: rng.next(),
        entry_count,
        entry_codes: (0..4).map(|_| rng.below(5) as u8).collect(),
        progress_a: i32::from(rng.below(3) == 0),
        progress_b: i32::from(rng.below(2) == 0),
        ..Default::default()
    };
    Seed {
        actor,
        grid,
        env,
        operand_ptr,
    }
}

fn write(cpu: &mut Cpu, s: &Seed) {
    let (a, g, e) = (&s.actor, &s.grid, &s.env);
    cpu.mem.wr16(ACTOR + 0x0A, a.x as u16);
    cpu.mem.wr16(ACTOR + 0x0C, a.y as u16);
    cpu.mem.wr16(ACTOR + 0x0E, a.width as u16);
    cpu.mem.wr32(ACTOR + 0x10, a.flags);
    cpu.mem.wr16(ACTOR + 0x1A, a.gate as u16);
    cpu.mem.wr16(ACTOR + 0x50, a.state);
    cpu.mem.wr16(ACTOR + 0x54, a.sub as u16);
    cpu.mem.wr16(GRID + 0x2E, g.handback as u16);
    cpu.mem.wr16(GRID + 0x3E, g.done_gate as u16);
    cpu.mem.wr16(GRID + 0x40, g.stashed_state);
    for (i, c) in g.columns.iter().enumerate() {
        cpu.mem.wr8(GRID + 0x54 + i as u32, *c);
    }
    cpu.mem.wr32(SUBMODE, e.submode as u32);
    cpu.mem.wr32(PAD_LATCH, e.pad_latch);
    cpu.mem.wr32(PAD_EDGE, e.pad_edge);
    cpu.mem.wr32(PAD_HELD, e.pad_held);
    cpu.mem.wr32(CONFIRM_A, e.accept_mask);
    cpu.mem.wr32(CONFIRM_B, e.back_mask);
    cpu.mem.wr32(CANCEL, e.cancel_mask);
    cpu.mem.wr32(BLOCKED, e.input_blocked as u32);
    cpu.mem.wr32(ROW, e.cursor_row as u32);
    cpu.mem.wr32(ROW_ALT, e.cursor_row_alt as u32);
    cpu.mem.wr32(OPERAND_PTR, s.operand_ptr);
    for (i, b) in e.board_entries.iter().enumerate() {
        cpu.mem.wr8(OPERAND + 1 + i as u32, *b);
    }
    cpu.mem.wr32(PLAYER + 0x10, e.busy_word);
    cpu.mem.wr8(ROSTER_COUNT, e.entry_count);
    for (i, c) in e.entry_codes.iter().enumerate() {
        cpu.mem.wr8(ROSTER + i as u32, *c);
    }
    cpu.mem.wr32(PROGRESS_A, e.progress_a as u32);
    cpu.mem.wr32(PROGRESS_B, e.progress_b as u32);
    cpu.mem.wr32(PALETTE, 0x55);
    cpu.mem.wr8(ENTRY_CODE, 0x77);
    cpu.mem.wr8(TEXT_LAYER, 0);
    cpu.mem.wr16(WINDOW_8 + 0x0E, 0x1111);
    cpu.mem.wr16(WINDOW_8 + 0x0A, 0x2222);
}

fn read_actor(cpu: &mut Cpu) -> HubActor {
    HubActor {
        x: cpu.mem.rd16(ACTOR + 0x0A) as i16,
        y: cpu.mem.rd16(ACTOR + 0x0C) as i16,
        width: cpu.mem.rd16(ACTOR + 0x0E) as i16,
        flags: cpu.mem.rd32(ACTOR + 0x10),
        gate: cpu.mem.rd16(ACTOR + 0x1A) as i16,
        state: cpu.mem.rd16(ACTOR + 0x50),
        sub: cpu.mem.rd16(ACTOR + 0x54) as i16,
    }
}

fn read_grid(cpu: &mut Cpu, columns: Vec<u8>) -> HubGrid {
    HubGrid {
        handback: cpu.mem.rd16(GRID + 0x2E) as i16,
        done_gate: cpu.mem.rd16(GRID + 0x3E) as i16,
        stashed_state: cpu.mem.rd16(GRID + 0x40),
        columns,
    }
}

/// The cells a frame's memory-side actions land in, as retail leaves them.
#[derive(Debug, PartialEq, Eq)]
struct Cells {
    pad_latch: u32,
    busy_word: u32,
    operand_ptr: u32,
    row: u32,
    window_height: u16,
    window_top: u16,
}

fn read_cells(cpu: &mut Cpu) -> Cells {
    Cells {
        pad_latch: cpu.mem.rd32(PAD_LATCH),
        busy_word: cpu.mem.rd32(PLAYER + 0x10),
        operand_ptr: cpu.mem.rd32(OPERAND_PTR),
        row: cpu.mem.rd32(ROW),
        window_height: cpu.mem.rd16(WINDOW_8 + 0x0E),
        window_top: cpu.mem.rd16(WINDOW_8 + 0x0A),
    }
}

/// Fold the port's frame into the same shape: the hooked calls in order, and
/// the cells its memory-side actions write.
fn port_side(s: &Seed, frame: &HubFrame, labels: &[u32; 5]) -> (Vec<Ev>, Vec<Ev>, Cells) {
    let mut cells = Cells {
        pad_latch: s.env.pad_latch,
        busy_word: s.env.busy_word,
        operand_ptr: s.operand_ptr,
        row: s.env.cursor_row as u32,
        window_height: 0x1111,
        window_top: 0x2222,
    };
    let mut draws = Vec::new();
    for d in &frame.draws {
        let d = match *d {
            HubDraw::Text {
                text: HubString::EntryLabel(code),
                x,
                y,
                palette,
            } => HubDraw::Text {
                text: HubString::Literal(labels[usize::from(code) + 2]),
                x,
                y,
                palette,
            },
            HubDraw::EntrySubPanel(_) => continue,
            other => other,
        };
        draws.push(Ev::Draw(d));
    }
    let mut acts = Vec::new();
    for a in &frame.actions {
        match *a {
            HubAction::ReleasePadLatch => cells.pad_latch &= !0x8000,
            HubAction::ClearBusyBit => cells.busy_word &= !0x8_0000,
            HubAction::SetBoardFlag => cells.operand_ptr = 1,
            HubAction::ClearBoardFlag => cells.operand_ptr = 0,
            HubAction::ClearCursorRow => cells.row = 0,
            HubAction::SizePanel { height, top } => {
                cells.window_height = height as u16;
                cells.window_top = top as u16;
            }
            HubAction::SetEntryCode(_) => {}
            other => acts.push(Ev::Act(other)),
        }
    }
    (draws, acts, cells)
}

fn split(log: &[Ev]) -> (Vec<Ev>, Vec<Ev>, Vec<Ev>) {
    let pick = |f: fn(&Ev) -> bool| log.iter().copied().filter(f).collect::<Vec<_>>();
    (
        pick(|e| matches!(e, Ev::Draw(_))),
        pick(|e| matches!(e, Ev::Act(_))),
        pick(|e| matches!(e, Ev::SubPanel { .. })),
    )
}

fn labels(cpu: &mut Cpu) -> [u32; 5] {
    std::array::from_fn(|i| cpu.mem.rd32(LABELS + i as u32 * 4))
}

type Handler = fn(&mut HubActor, &HubEnv, &mut HubGrid) -> HubFrame;

const MACHINES: [(u16, u32, Handler); 6] = [
    (slot::START_MENU, START_MENU, start_menu),
    (slot::SUBMENU, SUBMENU, submenu),
    (slot::PROMPT, PROMPT, hub_prompt),
    (slot::DRAW_TICK, DRAW_TICK, draw_tick),
    (slot::CLOSE_TICK, CLOSE_TICK, close_tick),
    (slot::DEACTIVATE, DEACTIVATE, deactivate),
];

#[test]
fn the_hub_state_machines_match_retail() {
    differential!(
        prot(897, START_MENU),
        prot(897, SUBMENU),
        prot(897, PROMPT),
        prot(897, DRAW_TICK),
        prot(897, CLOSE_TICK),
        prot(897, DEACTIVATE)
    );
    let Some(disc) = disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let (mut cpu, log) = hooked(&disc.read_entry(897).expect("PROT 0897"));
    let labels = labels(&mut cpu);
    let mut rng = Rng(0x4855_4253);
    let (mut handbacks, mut installs, mut gates) = (0, 0, 0);
    for round in 0..6000 {
        let (state, va, handler) = MACHINES[round % MACHINES.len()];
        let s = seed(&mut rng, state);
        write(&mut cpu, &s);
        log.borrow_mut().clear();
        assert_eq!(cpu.call(va, &[ACTOR]), Stop::Returned, "round {round}");
        let (r_draws, r_acts, _) = split(&log.borrow());

        let (mut actor, mut grid) = (s.actor, s.grid.clone());
        let frame = handler(&mut actor, &s.env, &mut grid);
        let (p_draws, p_acts, p_cells) = port_side(&s, &frame, &labels);
        let ctx = format!("round {round} slot {state:#x} sub {}", s.actor.sub);
        assert_eq!(r_acts, p_acts, "calls: {ctx}");
        assert_eq!(r_draws, p_draws, "draws: {ctx}");
        assert_eq!(read_actor(&mut cpu), actor, "actor: {ctx}");
        assert_eq!(
            read_grid(&mut cpu, s.grid.columns.clone()),
            grid,
            "grid: {ctx}"
        );
        assert_eq!(read_cells(&mut cpu), p_cells, "cells: {ctx}");
        handbacks += u32::from(grid.handback == -1 && s.grid.handback != -1);
        installs += u32::from(
            p_acts
                .iter()
                .any(|e| matches!(e, Ev::Act(HubAction::InstallPanel(_)))),
        );
        gates += u32::from(grid.done_gate == 0 && s.grid.done_gate != 0);
    }
    assert!(
        handbacks > 300 && installs > 500 && gates > 100,
        "{handbacks} {installs} {gates}"
    );
}

#[test]
fn the_hub_dispatcher_matches_retail() {
    differential!(prot(897, DISPATCH));
    let Some(disc) = disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let (mut cpu, log) = hooked(&disc.read_entry(897).expect("PROT 0897"));
    let labels = labels(&mut cpu);
    let mut rng = Rng(0x4449_5350);
    let (mut retired, mut rearmed, mut suspended, mut idle) = (0, 0, 0, 0);
    for round in 0..8000 {
        // The six ported machines, and the close tick's other five slots.
        let (state, handler) = match rng.below(8) {
            7 => (0x14 + rng.below(5) as u16, close_tick as Handler),
            i => {
                let (state, _, handler) = MACHINES[i as usize % MACHINES.len()];
                (state, handler)
            }
        };
        let s = seed(&mut rng, state);
        write(&mut cpu, &s);
        log.borrow_mut().clear();
        assert_eq!(
            cpu.call(DISPATCH, &[ACTOR]),
            Stop::Returned,
            "round {round}"
        );
        let (r_draws, r_acts, _) = split(&log.borrow());

        let (mut actor, mut grid) = (s.actor, s.grid.clone());
        let frame = hub_dispatch(&mut actor, &s.env, &mut grid, |a, g| handler(a, &s.env, g));
        let (p_draws, p_acts, p_cells) = port_side(&s, &frame, &labels);
        let ctx = format!(
            "round {round} slot {state:#x} sub {} submode {}",
            s.actor.sub, s.env.submode
        );
        assert_eq!(r_acts, p_acts, "calls: {ctx}");
        assert_eq!(r_draws, p_draws, "draws: {ctx}");
        assert_eq!(read_actor(&mut cpu), actor, "actor: {ctx}");
        assert_eq!(
            read_grid(&mut cpu, s.grid.columns.clone()),
            grid,
            "grid: {ctx}"
        );
        assert_eq!(read_cells(&mut cpu), p_cells, "cells: {ctx}");
        let ran = [1, 4, 7].contains(&s.env.submode);
        assert_eq!(cpu.mem.rd8(TEXT_LAYER), if ran { 0x0C } else { 0 }, "{ctx}");
        idle += u32::from(!ran);
        retired += u32::from(ran && actor.flags & 8 != 0 && grid.done_gate == 0);
        suspended += u32::from(ran && actor.flags & 8 != 0 && frame.actions.is_empty());
        rearmed += u32::from(frame.actions.contains(&HubAction::RearmSubmode));
    }
    assert!(
        retired > 500 && rearmed > 500 && suspended > 100 && idle > 1000,
        "{retired} {rearmed} {suspended} {idle}"
    );
}

#[test]
fn the_hub_painters_match_retail() {
    differential!(
        prot(897, ENTRY_LIST),
        prot(897, COLUMN_ROW),
        prot(897, THREE_LINE),
        prot(897, TWO_OPTION),
        prot(897, COUNT_GATED),
        prot(897, TWO_LINE),
        prot(897, SINGLE_LABEL)
    );
    let Some(disc) = disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let (mut cpu, log) = hooked(&disc.read_entry(897).expect("PROT 0897"));
    let labels = labels(&mut cpu);
    let mut rng = Rng(0x5041_494E);
    let painters = [
        ENTRY_LIST,
        COLUMN_ROW,
        THREE_LINE,
        TWO_OPTION,
        COUNT_GATED,
        TWO_LINE,
        SINGLE_LABEL,
    ];
    let (mut listed, mut skipped, mut cells_drawn, mut cursors) = (0, 0, 0, 0);
    for round in 0..7000 {
        let va = painters[round % painters.len()];
        let s = seed(&mut rng, 0);
        write(&mut cpu, &s);
        log.borrow_mut().clear();
        assert_eq!(cpu.call(va, &[ACTOR]), Stop::Returned, "round {round}");
        let (r_draws, r_acts, r_subs) = split(&log.borrow());

        let mut actor = s.actor;
        let frame = match va {
            ENTRY_LIST => entry_list(&mut actor, &s.env),
            COLUMN_ROW => column_row(&actor, &s.env, &s.grid),
            THREE_LINE => three_line_panel(&actor, &s.env),
            TWO_OPTION => two_option_panel(&actor, &s.env),
            COUNT_GATED => count_gated_label(&actor, &s.env),
            TWO_LINE => two_line_panel(&actor, &s.env),
            _ => single_label(&actor),
        };
        let (p_draws, p_acts, p_cells) = port_side(&s, &frame, &labels);
        let ctx = format!("round {round} painter {va:#x}");
        assert_eq!(r_draws, p_draws, "draws: {ctx}");
        assert_eq!(r_acts, p_acts, "calls: {ctx}");
        assert_eq!(read_actor(&mut cpu), actor, "actor: {ctx}");
        assert_eq!(read_cells(&mut cpu), p_cells, "cells: {ctx}");
        if va == ENTRY_LIST {
            // The sub-draw is the port's `equip_stat_panel`, hooked here: one
            // call per drawn entry, on the code the port publishes and `0x0D`
            // below that entry's label.
            let codes: Vec<u8> = frame
                .actions
                .iter()
                .filter_map(|a| match a {
                    HubAction::SetEntryCode(c) => Some(*c),
                    _ => None,
                })
                .collect();
            let want: Vec<u8> = s.env.entry_codes[..usize::from(s.env.entry_count)].to_vec();
            assert_eq!(codes, want, "entry codes: {ctx}");
            let p_subs: Vec<Ev> = p_draws
                .iter()
                .zip(codes.iter().filter(|c| **c < 3))
                .map(|(d, code)| match d {
                    Ev::Draw(HubDraw::Text { y, .. }) => Ev::SubPanel {
                        code: *code,
                        y: y.wrapping_add(0x0D),
                    },
                    other => panic!("an entry list draws labels only: {other:?}"),
                })
                .collect();
            assert_eq!(r_subs, p_subs, "sub-draws: {ctx}");
            // The entry code the list found is back when it returns.
            assert_eq!(cpu.mem.rd8(ENTRY_CODE), 0x77, "{ctx}");
            listed += r_subs.len();
            skipped += codes.len() - r_subs.len();
        }
        cells_drawn += p_draws
            .iter()
            .filter(|e| matches!(e, Ev::Draw(HubDraw::Cell { .. })))
            .count();
        cursors += p_draws
            .iter()
            .filter(|e| matches!(e, Ev::Draw(HubDraw::Sprite { .. })))
            .count();
    }
    assert!(
        listed > 500 && skipped > 300 && cells_drawn > 500 && cursors > 2000,
        "{listed} {skipped} {cells_drawn} {cursors}"
    );
}
