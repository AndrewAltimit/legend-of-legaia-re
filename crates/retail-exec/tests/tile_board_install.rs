//! The tile board's walk state machine `FUN_801EF2B0` (PROT 0897) in its
//! state `0`, the board install: the cell array's procedural fill off BIOS
//! `rand`, the player's start cell and walk-in point, the four set-base flags
//! it clears, the saved pad octant and the step into the fade-in. Run from the
//! routine's entry over random headers against `World::try_install_tile_board`,
//! the two drawing one `rand()` stream.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_core::world::World;
use legaia_engine_minigames::tile_board::{
    ArrivalAction, FADE_FULL, TileBoardHeader, advance_animated_cells, arrival_action,
    event_cell_flag_writes, fade_exempt_value, fade_in_step, fade_out_step, prompt_next, sm,
    step_for_mask, walker_octant,
};
use legaia_engine_system::menu_input::{NavButtons, menu_cursor_nav};
use legaia_engine_system::rand::{bios_rand_shape, world_lcg_step};
use psx_retail_exec::{A0, A1, Stop, V0};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

const OVERLAY_BASE: u32 = 0x801C_E818;
const WALK_SM: u32 = 0x801E_F2B0;
const ALLOC: u32 = 0x8001_7888;
const RESOLVE: u32 = 0x8003_C83C;
const CLIP: u32 = 0x8002_04F8;
const RAND: u32 = 0x8005_6798;
const HEADER_PTR: u32 = 0x8007_B450;
const OCTANT: u32 = 0x8007_B5F0;
const TABLE_PTR: u32 = 0x801F_35BC;
const CELLS_PTR: u32 = 0x801F_35C0;
const SAVED_OCTANT: u32 = 0x801F_35C4;
const FRAME_STEP: u32 = 0x1F80_0393;
const HELD: u32 = 0x8007_B850;
const PRESSED: u32 = 0x8007_B874;
/// The quit prompt's cursor word (`-0x4478` off `0x80080000`).
const PROMPT_CURSOR: u32 = 0x8007_BB88;
/// The pad's auto-repeat word the picker reads its rows on, and the confirm
/// / cancel masks (`0x80084140 + 0x590` / `+ 0x594`).
const REPEAT: u32 = 0x8007_BB84;
const CONFIRM_MASK: u32 = 0x8008_46D0;
const CANCEL_MASK: u32 = 0x8008_46D4;
const PLAYER_CELL: u32 = 0x801F_35C8;
const WALK_IN: u32 = 0x801F_35D0;
/// The system flag bank (`0x80084140 + 0x1618`).
const FLAGS: u32 = 0x8008_5758;
const ACTOR: u32 = 0x8012_0000;
const HEADER: u32 = 0x8012_1000;
const HEAP: u32 = 0x8013_0000;
const SPAWNED: u32 = 0x8015_0000;

#[test]
fn the_board_install_matches_retail() {
    differential!(prot(897, WALK_SM));
    let (Some(mut cpu), Some(d)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem
        .load(OVERLAY_BASE, &d.read_entry(897).expect("PROT 0897"));
    let heap = Rc::new(Cell::new(HEAP));
    {
        let heap = heap.clone();
        cpu.hook(ALLOC, move |r, _| {
            r.r[V0] = heap.get();
            heap.set(heap.get() + ((r.r[A1] + 0xFF) & !0xFF).max(0x100));
        });
    }
    let spawned = Rc::new(RefCell::new(Vec::<u32>::new()));
    {
        let spawned = spawned.clone();
        cpu.hook(RESOLVE, move |r, _| {
            let mut s = spawned.borrow_mut();
            r.r[V0] = SPAWNED + 0x100 * s.len() as u32;
            s.push(r.r[A0]);
        });
    }
    cpu.stub(CLIP, 0);
    // The quit prompt's panel the render tail draws in state 5 (text, the
    // glyph and frame draws).
    for draw in [
        0x8003_6888,
        0x8002_B994,
        0x8003_4B6C,
        0x8002_C69C,
        0x8003_1D00,
        // The heap free the teardown calls.
        0x8001_7B94,
    ] {
        cpu.stub(draw, 0);
    }
    let rng = Rc::new(Cell::new(0u32));
    {
        let rng = rng.clone();
        cpu.hook(RAND, move |r, _| {
            let s = world_lcg_step(rng.get());
            rng.set(s);
            r.r[V0] = bios_rand_shape(s);
        });
    }
    let mut g = Rng(0x5449_4C45);
    let mut events = 0usize;
    let (mut fades, mut exempted) = (0usize, 0usize);
    let mut arrivals = [0usize; 3];
    let mut walks = [0usize; 4];
    let mut prompts = [0usize; 3];
    let mut walked = 0usize;
    // The two cue producers state 4 calls: the push (`FUN_80035B50`, the
    // step) and the overwrite (`FUN_80035BD0`, the bonk).
    let cues = Rc::new(RefCell::new(Vec::<u32>::new()));
    for producer in [0x8003_5B50, 0x8003_5BD0] {
        let cues = cues.clone();
        cpu.hook(producer, move |r, _| cues.borrow_mut().push(r.r[A0]));
    }
    for round in 0..600 {
        let width = 1 + g.below(14) as u8;
        let height = 1 + g.below(14) as u8;
        let flag_base = g.below(0x7F0) as u16;
        let window: [u8; 13] = [
            5,
            g.below(0x70) as u8,
            g.below(0x70) as u8,
            width,
            height,
            g.below(8) as u8,
            g.below(2) as u8,
            flag_base as u8,
            (flag_base >> 8) as u8,
            g.next() as u8,
            g.next() as u8 & 0x7,
            g.below(0x40) as u8,
            g.below(0x40) as u8,
        ];
        let header = TileBoardHeader::parse(&window).expect("header");
        let flags: Vec<u8> = (0..0x100).map(|_| g.next() as u8).collect();
        let seed = g.next();

        // The port.
        let mut w = World::new();
        w.rng_state = seed;
        w.flags.system_flags = flags.clone();
        let mut instr = vec![0x49];
        instr.extend_from_slice(&window);
        assert!(w.try_install_tile_board(&instr), "round {round}");

        // Retail, from the routine's entry in state 0.
        for o in (0..0x100).step_by(4) {
            cpu.mem.wr32(ACTOR + o, 0);
        }
        cpu.mem.load(HEADER, &window);
        cpu.mem.wr32(HEADER_PTR, HEADER);
        cpu.mem.wr32(OCTANT, 0);
        cpu.mem.load(FLAGS, &flags);
        heap.set(HEAP);
        spawned.borrow_mut().clear();
        rng.set(seed);
        assert_eq!(cpu.call(WALK_SM, &[ACTOR]), Stop::Returned, "round {round}");

        let ctx = format!("round {round} header {header:?}");
        let n = usize::from(width) * usize::from(height);
        let at = cpu.mem.rd32(CELLS_PTR);
        let cells = cpu.mem.read(at, n);
        let board = w.board.grid.as_ref().expect("installed");
        assert_eq!(cells, board.cells, "cells, {ctx}");
        assert_eq!(rng.get(), w.rng_state, "rand draws, {ctx}");
        assert_eq!(
            (cpu.mem.rd32(PLAYER_CELL), cpu.mem.rd32(PLAYER_CELL + 4)),
            (u32::from(board.player_col), u32::from(board.player_row)),
            "start cell, {ctx}"
        );
        let walk_in = (
            cpu.mem.rd32(WALK_IN) as i32,
            cpu.mem.rd32(WALK_IN + 4) as i32,
        );
        assert_eq!(Some(walk_in), w.board.target, "walk-in point, {ctx}");
        assert_eq!(
            cpu.mem.read(FLAGS, flags.len()),
            w.flags.system_flags[..flags.len()],
            "flags, {ctx}"
        );
        assert_eq!(cpu.mem.rd32(SAVED_OCTANT), w.board.saved_octant, "{ctx}");
        assert_eq!(cpu.mem.rd16(ACTOR + 0x54), u16::from(sm::FADE_IN), "{ctx}");
        assert_eq!(cpu.mem.rd16(ACTOR + 0x9C) as i16, w.board.fade, "{ctx}");
        // The table: the player from `+0xB`, then one actor per cell value
        // `2..=14` from `+0xC` on, every value whether the board holds it
        // or not.
        let want: Vec<u32> = std::iter::once(u32::from(header.player_template))
            .chain((0..13).map(|k| u32::from(header.tile_template_base) + k))
            .collect();
        assert_eq!(*spawned.borrow(), want, "tile-actor templates, {ctx}");
        assert_ne!(cpu.mem.rd32(TABLE_PTR), 0);
        events += cells.iter().filter(|&&c| (8..=0xA).contains(&c)).count();

        // Then the board's fades on every third round: state 1's fade-in
        // against `fade_in_step` until the walk-in (`2`), and the exit -
        // states 9, `0xA`'s fade-out against `fade_out_step` with the event
        // tile under the player left alone (`fade_exempt_value`) - to the
        // park `0xD`. Every tile actor `2..=14` takes the scale `+0x72`.
        if round % 3 != 0 {
            continue;
        }
        let table = cpu.mem.rd32(TABLE_PTR);
        let tiles: Vec<u32> = (2..15u32).map(|v| cpu.mem.rd32(table + 4 * v)).collect();
        let exempt = fade_exempt_value(board.cell_under_player());
        // A start cell past the buffer's end (a board under five cells
        // wide and one row high) reads the heap after it in retail, which
        // the port does not hold: those tiles' scales are not compared.
        let past =
            usize::from(board.player_row) * usize::from(width) + usize::from(board.player_col) >= n;
        let mut fade = w.board.fade;
        let mut frame = 0;
        loop {
            let step = 1 + g.below(3) as u8;
            cpu.mem.wr8(FRAME_STEP, step);
            assert_eq!(cpu.call(WALK_SM, &[ACTOR]), Stop::Returned, "{ctx}");
            let (next, done) = fade_in_step(fade, step);
            fade = next;
            let c = format!("{ctx} fade-in frame {frame} step {step}");
            assert_eq!(cpu.mem.rd16(ACTOR + 0x9C) as i16, fade, "{c}");
            for (k, &t) in tiles.iter().enumerate() {
                assert_eq!(cpu.mem.rd16(t + 0x72) as i16, fade, "{c} tile {}", k + 2);
            }
            assert_eq!(cpu.mem.rd16(ACTOR + 0x54), if done { 2 } else { 1 }, "{c}");
            frame += 1;
            if done {
                break;
            }
        }
        fades += 1;
        // The quit's exit (`9 -> 0xA`) on even rounds, the event cell's
        // (`0xB -> 0xC`) on odd ones; both fade the same way.
        let (exit_step, fade_state) = if round % 2 == 0 {
            (sm::EXIT_STEP, sm::FADE_OUT)
        } else {
            (sm::EVENT_STEP, sm::EVENT_FADE_OUT)
        };
        cpu.mem.wr16(ACTOR + 0x54, u16::from(exit_step));
        assert_eq!(cpu.call(WALK_SM, &[ACTOR]), Stop::Returned, "{ctx}");
        assert_eq!(cpu.mem.rd16(ACTOR + 0x54), u16::from(fade_state), "{ctx}");
        loop {
            let step = 1 + g.below(3) as u8;
            cpu.mem.wr8(FRAME_STEP, step);
            assert_eq!(cpu.call(WALK_SM, &[ACTOR]), Stop::Returned, "{ctx}");
            let (next, done) = fade_out_step(fade, step);
            fade = next;
            let c = format!("{ctx} fade-out step {step}");
            assert_eq!(cpu.mem.rd16(ACTOR + 0x9C) as i16, fade, "{c}");
            for (k, &t) in tiles.iter().enumerate().filter(|_| !past) {
                let v = k as u8 + 2;
                let want = if exempt == Some(v) { FADE_FULL } else { fade };
                assert_eq!(cpu.mem.rd16(t + 0x72) as i16, want, "{c} tile {v}");
            }
            let state = if done { sm::PARK } else { fade_state };
            assert_eq!(cpu.mem.rd16(ACTOR + 0x54), u16::from(state), "{c}");
            if done {
                break;
            }
        }
        exempted += usize::from(exempt.is_some());
        // The park (`0xD`): every tile actor but the exempt one goes to
        // `(0x3FC0, 0x3FC0)`; then the teardown (`0xE`) puts the saved pad
        // octant back, as `World::tile_board_teardown` does.
        let before: Vec<(u16, u16)> = tiles
            .iter()
            .map(|&t| (cpu.mem.rd16(t + 0x14), cpu.mem.rd16(t + 0x18)))
            .collect();
        assert_eq!(cpu.call(WALK_SM, &[ACTOR]), Stop::Returned, "{ctx}");
        assert_eq!(cpu.mem.rd16(ACTOR + 0x54), u16::from(sm::TEARDOWN), "{ctx}");
        for (k, &t) in tiles.iter().enumerate().filter(|_| !past) {
            let v = k as u8 + 2;
            let want = if exempt == Some(v) {
                before[k]
            } else {
                (0x3FC0, 0x3FC0)
            };
            assert_eq!(
                (cpu.mem.rd16(t + 0x14), cpu.mem.rd16(t + 0x18)),
                want,
                "{ctx} park tile {v}"
            );
        }
        cpu.mem.wr32(OCTANT, 0xFFFF);
        assert_eq!(cpu.call(WALK_SM, &[ACTOR]), Stop::Returned, "{ctx}");
        assert_eq!(
            cpu.mem.rd32(OCTANT),
            w.board.saved_octant,
            "{ctx}: the octant back"
        );

        // The arrival (state 3) on cells across the board, against
        // `arrival_action` and `advance_animated_cells`, and the event arm
        // (state 8) after it against `event_cell_flag_writes` over the
        // flag bank, with the port's own flag test and set.
        for _ in 0..6 {
            let board = w.board.grid.as_mut().expect("installed");
            board.player_col = g.below(u32::from(width)) as u8;
            board.player_row = g.below(u32::from(height)) as u8;
            let cell = board.cell_under_player().expect("on the board");
            cpu.mem.wr32(PLAYER_CELL, u32::from(board.player_col));
            cpu.mem.wr32(PLAYER_CELL + 4, u32::from(board.player_row));
            cpu.mem.load(at, &board.cells);
            cpu.mem.load(FLAGS, &w.flags.system_flags[..flags.len()]);
            // State 3 falls into state 4's body: no pad, so it reads none.
            cpu.mem.wr32(HELD, 0);
            cpu.mem.wr32(PRESSED, 0);
            cpu.mem.wr16(ACTOR + 0x54, 3);
            assert_eq!(cpu.call(WALK_SM, &[ACTOR]), Stop::Returned, "{ctx}");
            let c = format!("{ctx} arrival on {cell:#x}");
            let action = arrival_action(cell);
            let state = match action {
                ArrivalAction::Continue => {
                    advance_animated_cells(&mut board.cells);
                    sm::WALK
                }
                ArrivalAction::ExitTrigger => sm::TRIGGER_EXIT,
                ArrivalAction::ExitEvent => 8,
            };
            assert_eq!(cpu.mem.rd16(ACTOR + 0x54), u16::from(state), "{c}");
            assert_eq!(cpu.mem.read(at, n), board.cells, "{c}: the cells");
            arrivals[match action {
                ArrivalAction::Continue => 0,
                ArrivalAction::ExitTrigger => 1,
                ArrivalAction::ExitEvent => 2,
            }] += 1;
            if action != ArrivalAction::ExitEvent {
                continue;
            }
            assert_eq!(cpu.call(WALK_SM, &[ACTOR]), Stop::Returned, "{c}");
            let writes = event_cell_flag_writes(&header, cell, |i| w.system_flag_test(i));
            for i in writes {
                w.system_flag_set(i);
            }
            assert_eq!(
                cpu.mem.read(FLAGS, flags.len()),
                w.flags.system_flags[..flags.len()],
                "{c}: the event flags"
            );
            assert_eq!(cpu.mem.rd16(ACTOR + 0x54), u16::from(sm::EVENT_STEP), "{c}");
        }

        // The walker's input state 4 from cells across the board: the menu
        // edge into the quit prompt, the octant store off the cell under
        // the player (`walker_octant`), the held pad remapped through it
        // (`FUN_800467E8`, live; `World::remap_pad_direction`), decoded
        // (`step_for_mask`) and gated against the board (`TileBoard::try_step`):
        // the committed cell and walk target, the step and bonk cues.
        for _ in 0..12 {
            let board = w.board.grid.as_mut().expect("installed");
            board.player_col = g.below(u32::from(width)) as u8;
            board.player_row = g.below(u32::from(height)) as u8;
            cpu.mem.wr32(PLAYER_CELL, u32::from(board.player_col));
            cpu.mem.wr32(PLAYER_CELL + 4, u32::from(board.player_row));
            cpu.mem.load(at, &board.cells);
            let held = [0u32, 0x1000, 0x2000, 0x4000, 0x8000, 0x3000, 0x9000][g.below(7) as usize];
            let pressed = if g.below(8) == 0 { 0x10 } else { 0 };
            cpu.mem.wr32(HELD, held);
            cpu.mem.wr32(PRESSED, pressed);
            cpu.mem.wr32(OCTANT, 0xFFFF);
            cpu.mem.wr16(ACTOR + 0x54, u16::from(sm::WALK));
            cues.borrow_mut().clear();
            let tp = cpu.mem.rd32(TABLE_PTR);
            let lead = cpu.mem.rd32(tp);
            assert_eq!(
                cpu.call(WALK_SM, &[ACTOR]),
                Stop::Returned,
                "{ctx} walk from ({}, {}) held {held:#x} pressed {pressed:#x} cells {:?} actor {lead:x}",
                board.player_col,
                board.player_row,
                board.cells,
            );
            let c = format!(
                "{ctx} walk from ({}, {}) held {held:#x} pressed {pressed:#x}",
                board.player_col, board.player_row
            );
            if pressed != 0 {
                assert_eq!(cpu.mem.rd16(ACTOR + 0x54), u16::from(sm::PROMPT), "{c}");
                assert_eq!(cpu.mem.rd16(ACTOR + 0x9C) as i16, FADE_FULL, "{c}");
                assert_eq!(cpu.mem.rd32(PROMPT_CURSOR), 1, "{c}: the prompt row");
                walks[3] += 1;
                continue;
            }
            let octant = walker_octant(board.cell_under_player().unwrap_or(0));
            assert_eq!(cpu.mem.rd32(OCTANT), octant, "{c}: the octant");
            let remapped = World::remap_pad_direction(held as u16, octant);
            let (state, cue) = match step_for_mask(remapped) {
                None => (sm::WALK, None),
                Some(dir) => match board.try_step(dir) {
                    Some(target) => {
                        let walk_in = (
                            cpu.mem.rd32(WALK_IN) as i32,
                            cpu.mem.rd32(WALK_IN + 4) as i32,
                        );
                        assert_eq!(walk_in, target, "{c}: the target");
                        (2, Some(0x21))
                    }
                    None => (sm::WALK, Some(0x23)),
                },
            };
            walks[match cue {
                None => 0,
                Some(0x21) => 1,
                _ => 2,
            }] += 1;
            assert_eq!(
                cpu.mem.rd16(ACTOR + 0x54),
                u16::from(state),
                "{c}: the state"
            );
            assert_eq!(cues.borrow().last().copied(), cue, "{c}: the cue");
            assert_eq!(
                (cpu.mem.rd32(PLAYER_CELL), cpu.mem.rd32(PLAYER_CELL + 4)),
                (u32::from(board.player_col), u32::from(board.player_row)),
                "{c}: the cell"
            );
        }

        // The quit prompt, state 5: the two-row picker `FUN_801E9DC8`
        // (live) against `menu_cursor_nav` and `prompt_next` - confirm and
        // cancel on the pad's configured masks, the rows on the repeat
        // word, wrapping.
        cpu.mem.wr32(CONFIRM_MASK, 0x40);
        cpu.mem.wr32(CANCEL_MASK, 0x20);
        for _ in 0..12 {
            let mut cursor = g.below(2);
            let pressed = [0u32, 0, 0x40, 0x20, 0x60][g.below(5) as usize];
            let repeat = [0u32, 0x1000, 0x4000, 0x5000][g.below(4) as usize];
            cpu.mem.wr32(PROMPT_CURSOR, cursor);
            cpu.mem.wr32(PRESSED, pressed);
            cpu.mem.wr32(REPEAT, repeat);
            cpu.mem.wr16(ACTOR + 0x54, u16::from(sm::PROMPT));
            cues.borrow_mut().clear();
            assert_eq!(cpu.call(WALK_SM, &[ACTOR]), Stop::Returned, "{ctx}");
            let nav = menu_cursor_nav(
                &mut cursor,
                2,
                true,
                NavButtons::new(
                    pressed & 0x40 != 0,
                    pressed & 0x20 != 0,
                    repeat & 0x1000 != 0,
                    repeat & 0x4000 != 0,
                ),
            );
            let c = format!("{ctx} prompt pressed {pressed:#x} repeat {repeat:#x}");
            assert_eq!(cpu.mem.rd32(PROMPT_CURSOR), cursor, "{c}: the row");
            assert_eq!(
                cpu.mem.rd16(ACTOR + 0x54),
                u16::from(prompt_next(nav, cursor)),
                "{c}: the state"
            );
            assert_eq!(
                cues.borrow().last().copied(),
                nav.sfx_cue().map(u32::from),
                "{c}: the cue"
            );
            prompts[match prompt_next(nav, cursor) {
                sm::PROMPT => 0,
                sm::WALK => 1,
                _ => 2,
            }] += 1;
        }

        // The walker's state 2 (`0x801EFA88`) toward a committed target from
        // a seat around it, one game frame at step 2 against two vsync ticks
        // of `World::tile_board_walk_step` (the port walks the board on the
        // vsync, `TILE / 8` a tick): the position each frame, and the facing
        // the first tick writes off the frame's own delta (each axis clamped
        // to `+-0x20`, the octant of the signs, `+0x26 = octant << 9`). On
        // the frame retail finds no delta left it binds the idle clip and
        // steps to the arrival `3`; the port has run its arrival on the tick
        // that reached the centre, a game frame earlier.
        {
            use legaia_engine_core::world::field_test_drivers::FieldTestDrivers;
            let target = (g.below(0x800) as i32, g.below(0x800) as i32);
            let start = (
                target.0 + g.below(0x140) as i32 - 0xA0,
                target.1 + g.below(0x140) as i32 - 0xA0,
            );
            let table = cpu.mem.rd32(TABLE_PTR);
            let lead = cpu.mem.rd32(table);
            cpu.mem.wr16(lead + 0x14, start.0 as u16);
            cpu.mem.wr16(lead + 0x18, start.1 as u16);
            cpu.mem.wr32(WALK_IN, target.0 as u32);
            cpu.mem.wr32(WALK_IN + 4, target.1 as u32);
            cpu.mem.wr16(ACTOR + 0x54, 2);
            cpu.mem.wr8(FRAME_STEP, 2);
            w.board.target = Some(target);
            w.board.sm = sm::WALK;
            let slot = 0usize;
            w.actors[slot].move_state.world_x = start.0 as i16;
            w.actors[slot].move_state.world_z = start.1 as i16;
            let mut frame = 0;
            loop {
                let before = (
                    cpu.mem.rd16(lead + 0x14) as i16,
                    cpu.mem.rd16(lead + 0x18) as i16,
                );
                assert_eq!(cpu.call(WALK_SM, &[ACTOR]), Stop::Returned, "{ctx}");
                let c = format!("{ctx} walk-in from {start:?} to {target:?} frame {frame}");
                let state = cpu.mem.rd16(ACTOR + 0x54);
                let pos = (
                    cpu.mem.rd16(lead + 0x14) as i16,
                    cpu.mem.rd16(lead + 0x18) as i16,
                );
                if state == 3 {
                    assert_eq!(pos, before, "{c}: the arrival frame holds");
                    assert_eq!((i32::from(pos.0), i32::from(pos.1)), target, "{c}: arrived");
                    assert_eq!(w.board.target, None, "{c}: the port arrived");
                    walked += 1;
                    break;
                }
                assert_eq!(state, 2, "{c}");
                w.tile_board_walk(slot);
                let facing = w.actors[slot].move_state.render_26;
                w.tile_board_walk(slot);
                let ms = &w.actors[slot].move_state;
                assert_eq!((ms.world_x, ms.world_z), pos, "{c}: the position");
                let retail_facing = ((cpu.mem.rd16(lead + 0x26) + 0x800) & 0xFFF) as i16;
                assert_eq!(facing, retail_facing, "{c}: the facing");
                frame += 1;
                assert!(frame < 64, "{c}: no arrival");
            }
        }
    }
    assert!(walked > 150, "walks in {walked}");
    assert!(arrivals.iter().all(|&n| n > 20), "arrivals {arrivals:?}");
    assert!(walks.iter().all(|&n| n > 20), "walks {walks:?}");
    assert!(prompts.iter().all(|&n| n > 20), "prompts {prompts:?}");
    assert!(events > 1000, "event cells {events}");
    assert!(
        fades > 100 && exempted > 1,
        "{fades} fades, {exempted} on an event tile"
    );
}
