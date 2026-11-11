//! The tile board's walk state machine `FUN_801EF2B0` (PROT 0897) in its
//! state `0`, the board install: the cell array's procedural fill off BIOS
//! `rand`, the player's start cell and walk-in point, the four set-base flags
//! it clears, the saved pad octant and the step into the fade-in. Run from the
//! routine's entry over random headers against `World::try_install_tile_board`,
//! the two drawing one `rand()` stream.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_core::world::World;
use legaia_engine_minigames::tile_board::{TileBoardHeader, sm};
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
    }
    assert!(events > 1000, "event cells {events}");
}
