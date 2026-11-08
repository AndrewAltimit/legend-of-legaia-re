//! `FUN_801D01B0`, the field pad controller, with its helpers `FUN_800467E8`,
//! `FUN_80046494` and `FUN_801CFE4C`, against `World::step_field_locomotion`:
//! where one tick of a held pad leaves the player, over generated grids.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_core::world::{Actor, World};
use legaia_engine_system::input::PadButton;
use psx_retail_exec::{Cpu, GP, Stop};

const WALK: u32 = 0x801D_01B0;
const OVERLAY_BASE: u32 = 0x801C_E818;
const MAP_PTR: u32 = 0x1F80_03EC;
const FRAME_STEP: u32 = 0x1F80_0393;
const SCRATCH_FLAGS: u32 = 0x1F80_0394;
const MAP: u32 = 0x8010_0000;
const GRID: u32 = MAP + 0x4000;
const GRID_LEN: usize = 0x4000;
const ACTOR: u32 = 0x8013_0000;
const PLAYER_PTR: u32 = 0x8007_C364;
const SCENE_PTR: u32 = 0x801C_6EA4;
const SCENE: u32 = 0x8014_0000;
const PAD_HELD: u32 = 0x8007_B850;
const PAD_EDGE: u32 = 0x8007_B874;
const SLOW_SCENE: u32 = 0x8007_B6A8;
const CAMERA_MODE: u32 = 0x8007_B607;
const STEP_DELTA: u32 = 0x8007_BDE0;
const RUN_BUTTON: u32 = 0x8008_46DC;
const RUN_DEFAULT: u32 = 0x8008_46CC;
const OCTANT: u32 = 0x2D8;

fn retail_cpu() -> Option<Cpu> {
    let mut cpu = scus_cpu()?;
    cpu.mem
        .load(OVERLAY_BASE, &disc()?.read_entry(897).expect("PROT 0897"));
    cpu.stub(0x8003_CE64, 0); // the system-channel gate
    Some(cpu)
}

#[test]
fn a_held_pad_walks_where_retail_walks() {
    let Some(mut cpu) = retail_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let gp = cpu.regs.r[GP];
    cpu.mem.wr32(MAP_PTR, MAP);
    cpu.mem.wr32(PLAYER_PTR, ACTOR);
    cpu.mem.wr32(SCENE_PTR, SCENE);
    cpu.mem.wr32(RUN_BUTTON, 0x80);
    cpu.mem.wr8(FRAME_STEP, 1);
    let mut w = World::new();
    w.actors = vec![Actor::default(); 2];
    w.actors[0].active = true;
    w.player_actor_slot = Some(0);
    w.locomotion.leading_edge_wall_probes = true;
    cpu.mem.wr32(RUN_BUTTON, 0x80);
    let mut rng = Rng(0x5354_4550);
    let mut moved = 0;
    let mut slid = 0;
    let mut stopped = 0;
    for grid_round in 0..30 {
        let density = [2, 8, 32, 128][rng.below(4) as usize];
        let grid: Vec<u8> = (0..GRID_LEN)
            .map(|_| {
                if rng.below(density) == 0 {
                    (rng.below(16) as u8) << 4
                } else {
                    0
                }
            })
            .collect();
        cpu.mem.load(GRID, &grid);
        w.terrain.collision_grid = grid;
        // The object-grid words, some with the terrain slow bit.
        let cells: Vec<u8> = (0..GRID_LEN)
            .flat_map(|_| {
                let word: u16 = if rng.below(3) == 0 { 0x4000 } else { 0 };
                word.to_le_bytes()
            })
            .collect();
        cpu.mem.load(MAP + 0x8000, &cells);
        w.load_field_object_cells(&cells);
        for round in 0..1500 {
            let (x, z) = (
                (0x400 + rng.below(0x3000)) as i16,
                (0x400 + rng.below(0x3000)) as i16,
            );
            let (x, z) = match rng.below(3) {
                0 => (x & !63, z & !63),
                1 => (x & !1, z & !1),
                _ => (x, z),
            };
            let heading = 0x123u16;
            let held: u32 = [
                0x1000, 0x2000, 0x4000, 0x8000, 0x3000, 0x6000, 0xC000, 0x9000, 0,
            ][rng.below(9) as usize];
            let run = rng.below(3) == 0;
            let slow = rng.below(8) == 0;
            let run_default = rng.below(4) == 0;
            let mode = [0x40u8, 0x40, 0x10, 0x50, 0x00][rng.below(5) as usize];
            let rate: u16 = [0x1000, 0x1000, 0x800, 0x1800][rng.below(4) as usize];
            let frame_step = [1u8, 2, 2, 3][rng.below(4) as usize];
            let octant = if rng.below(2) == 0 { 0 } else { rng.below(8) };
            let scene_61 = [0u8, 1, 1, 2][rng.below(4) as usize];

            cpu.mem.wr16(ACTOR + 0x14, x as u16);
            cpu.mem.wr16(ACTOR + 0x18, z as u16);
            cpu.mem.wr16(ACTOR + 0x26, heading);
            cpu.mem.wr32(ACTOR + 0x10, 0);
            cpu.mem.wr16(ACTOR + 0x72, rate);
            cpu.mem.wr16(STEP_DELTA, 0);
            cpu.mem.wr16(STEP_DELTA + 4, 0);
            cpu.mem.wr32(PAD_HELD, held | if run { 0x80 } else { 0 });
            cpu.mem.wr32(PAD_EDGE, 0);
            cpu.mem.wr32(RUN_DEFAULT, u32::from(run_default));
            cpu.mem.wr8(SLOW_SCENE, u8::from(slow));
            cpu.mem.wr8(CAMERA_MODE, mode);
            cpu.mem.wr8(FRAME_STEP, frame_step);
            cpu.mem.wr8(SCENE + 0x61, scene_61);
            cpu.mem.wr32(SCRATCH_FLAGS, 0);
            cpu.mem.wr32(gp + OCTANT, octant);
            let stop = cpu.call(WALK, &[ACTOR]);
            assert_eq!(stop, Stop::Returned, "grid {grid_round} round {round}");
            let retail = (
                cpu.mem.rd16(ACTOR + 0x14) as i16,
                cpu.mem.rd16(ACTOR + 0x18) as i16,
                cpu.mem.rd16(ACTOR + 0x26) as i16 & 0xFFF,
            );

            {
                let ms = &mut w.actors[0].move_state;
                ms.world_x = x;
                ms.world_z = z;
                // The engine's heading is retail's plus a half turn.
                ms.render_26 = (heading as i16).wrapping_add(0x800);
                ms.flags = 0;
                ms.field_72 = rate;
            }
            w.move_vm.ramp_ratio = frame_step;
            w.clock.frame_step = frame_step;
            w.locomotion.camera_azimuth = (octant * 0x200) as i32 as _;
            w.locomotion.run_default = run_default;
            w.party.scene_save_allowed = slow;
            w.locomotion.camera_mode_byte = Some(mode);
            w.locomotion.scene_ctrl_61 = scene_61;
            let mut mask = 0u16;
            for (bit, button) in [
                (0x1000u32, PadButton::Up),
                (0x2000, PadButton::Right),
                (0x4000, PadButton::Down),
                (0x8000, PadButton::Left),
            ] {
                if held & bit != 0 {
                    mask |= button.mask();
                }
            }
            if run {
                mask |= PadButton::Square.mask();
            }
            w.set_pad(mask);
            w.step_field_locomotion();
            let ms = &w.actors[0].move_state;
            let port = (
                ms.world_x,
                ms.world_z,
                ms.render_26.wrapping_sub(0x800) & 0xFFF,
            );
            assert_eq!(
                retail, port,
                "grid {grid_round} round {round}: at ({x}, {z}) held {held:#x} octant {octant} \
                 run {run} default {run_default} slow {slow} mode {mode:#x} rate {rate:#x} step {frame_step}"
            );
            if (retail.0, retail.1) != (x, z) {
                moved += 1;
            }
            if held != 0 && (retail.0, retail.1) == (x, z) {
                stopped += 1;
            }
            if held.count_ones() == 1 && retail.0 != x && retail.1 != z {
                slid += 1;
            }
        }
    }
    assert!(
        moved > 5000 && slid > 50 && stopped > 200,
        "{moved} {slid} {stopped}"
    );
}

const ACTOR_COUNT: u32 = 0x8007_B6B8;
const ACTOR_TABLE: u32 = 0x801C_93C8;
const TOUCH: u32 = 0x801D_5B5C;
const FLAG_TOUCH: u32 = 0x8003_D038;

/// One tick of a held pad among seated actors: `FUN_801CFC40`'s moving-class
/// and static-class boxes under the walk's three probes, with the broad-phase
/// list already built, against `World::step_field_locomotion` on the port's
/// NPC and prop channels. The touch and flag posts are stubbed; what is
/// compared is where the player ends up.
#[test]
fn a_held_pad_stops_on_actors_where_retail_stops() {
    use legaia_engine_core::world::FieldPropCollider;
    let Some(mut cpu) = retail_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let gp = cpu.regs.r[GP];
    cpu.mem.wr32(MAP_PTR, MAP);
    cpu.mem.wr32(PLAYER_PTR, ACTOR);
    cpu.mem.wr32(SCENE_PTR, SCENE);
    cpu.mem.wr32(RUN_BUTTON, 0x80);
    cpu.stub(TOUCH, 0);
    cpu.stub(FLAG_TOUCH, 0);
    cpu.mem.load(GRID, &vec![0u8; GRID_LEN]);
    cpu.mem.load(MAP + 0x8000, &vec![0u8; GRID_LEN * 2]);
    let mut w = World::new();
    w.actors = vec![Actor::default(); 2];
    w.actors[0].active = true;
    w.player_actor_slot = Some(0);
    w.locomotion.leading_edge_wall_probes = true;
    w.terrain.collision_grid = vec![0; GRID_LEN];
    w.load_field_object_cells(&vec![0u8; GRID_LEN * 2]);
    w.npcs.solid = true;
    let mut rng = Rng(0x4143_544F);
    let (mut moved, mut stopped) = (0u32, 0u32);
    let mut failures = Vec::new();
    for round in 0..80_000 {
        let (x, z) = (
            (0x800 + rng.below(0x1000) as i16) & !1,
            (0x800 + rng.below(0x1000) as i16) & !1,
        );
        let held: u32 = [0x1000, 0x2000, 0x4000, 0x8000][rng.below(4) as usize];
        cpu.mem.wr16(ACTOR + 0x14, x as u16);
        cpu.mem.wr16(ACTOR + 0x18, z as u16);
        cpu.mem.wr16(ACTOR + 0x26, 0x123);
        cpu.mem.wr32(ACTOR + 0x10, 0);
        cpu.mem.wr16(ACTOR + 0x72, 0x1000);
        cpu.mem.wr16(STEP_DELTA, 0);
        cpu.mem.wr16(STEP_DELTA + 4, 0);
        cpu.mem.wr32(PAD_HELD, held);
        cpu.mem.wr32(PAD_EDGE, 0);
        cpu.mem.wr32(RUN_DEFAULT, 0);
        cpu.mem.wr8(SLOW_SCENE, 0);
        cpu.mem.wr8(CAMERA_MODE, 0x40);
        cpu.mem.wr8(FRAME_STEP, 1);
        cpu.mem.wr8(SCENE + 0x61, 0);
        cpu.mem.wr32(SCRATCH_FLAGS, 0);
        cpu.mem.wr32(gp + OCTANT, 0);

        w.npcs.positions.clear();
        w.props.colliders.clear();
        let n = rng.below(5);
        cpu.mem.wr32(ACTOR_COUNT, n);
        for i in 0..n {
            let a = 0x8014_0000 + i * 0x100;
            cpu.mem.wr32(ACTOR_TABLE + i * 4, a);
            let (ax, az) = (
                x + rng.below(0x100) as i16 - 0x80,
                z + rng.below(0x100) as i16 - 0x80,
            );
            cpu.mem.wr16(a + 0x14, ax as u16);
            cpu.mem.wr16(a + 0x18, az as u16);
            // The record of a static-class object lies at `id * 0x20`.
            let id = 0x40 + i;
            cpu.mem.wr16(a + 0x60, id as u16);
            let rec = MAP + id * 0x20;
            let moving = rng.below(2) == 0;
            let interact = rng.below(2) == 0;
            let flags = if moving { 0x0002_0000 } else { 0 } | if interact { 0x4000_0000 } else { 0 };
            cpu.mem.wr32(a + 0x10, flags);
            if moving {
                w.npcs.positions.insert(i as u8, (ax, az));
            } else {
                let (b6, be, b7, bf) = (
                    rng.below(4) as i8 - 2,
                    rng.below(8) as i8 - 4,
                    rng.below(4) as i8 - 2,
                    rng.below(8) as i8 - 4,
                );
                let corner = rng.below(2) == 0;
                let (h0, h4) = (rng.below(0x40) as i16, rng.below(0x40) as i16);
                cpu.mem.wr8(rec + 6, b6 as u8);
                cpu.mem.wr8(rec + 0xE, be as u8);
                cpu.mem.wr8(rec + 7, b7 as u8);
                cpu.mem.wr8(rec + 0xF, bf as u8);
                cpu.mem.wr16(rec, h0 as u16);
                cpu.mem.wr16(rec + 4, h4 as u16);
                cpu.mem.wr16(a + 0x52, if corner { 8 } else { 0 });
                let mut cx = i32::from(b6) * 0x80 + i32::from(be) * 0x10;
                let mut cz = i32::from(b7) * 0x80 + i32::from(bf) * 0x10;
                if corner {
                    cx -= i32::from(h0);
                    cz += i32::from(h4);
                }
                w.props.colliders.push(FieldPropCollider {
                    anchor: None,
                    center: (cx + i32::from(ax), cz + i32::from(az)),
                    live: (i32::from(ax), i32::from(az)),
                    moving_box: false,
                    interact,
                    solid: true,
                });
            }
        }
        assert_eq!(cpu.call(WALK, &[ACTOR]), Stop::Returned, "round {round}");
        let retail = (cpu.mem.rd16(ACTOR + 0x14) as i16, cpu.mem.rd16(ACTOR + 0x18) as i16);
        {
            let ms = &mut w.actors[0].move_state;
            ms.world_x = x;
            ms.world_z = z;
            ms.render_26 = 0x123i16.wrapping_add(0x800);
            ms.flags = 0;
            ms.field_72 = 0x1000;
        }
        w.locomotion.camera_mode_byte = Some(0x40);
        w.locomotion.camera_azimuth = 0;
        w.move_vm.ramp_ratio = 1;
        w.clock.frame_step = 1;
        let mut mask = 0u16;
        for (bit, button) in [
            (0x1000u32, PadButton::Up),
            (0x2000, PadButton::Right),
            (0x4000, PadButton::Down),
            (0x8000, PadButton::Left),
        ] {
            if held & bit != 0 {
                mask |= button.mask();
            }
        }
        w.set_pad(mask);
        w.step_field_locomotion();
        let ms = &w.actors[0].move_state;
        let port = (ms.world_x, ms.world_z);
        if retail != port && failures.len() < 20 {
            failures.push(format!(
                "round {round} at ({x}, {z}) held {held:#x} n {n}: retail {retail:?} port {port:?}"
            ));
        }
        if retail != (x, z) {
            moved += 1;
        } else {
            stopped += 1;
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert!(moved > 20_000 && stopped > 5_000, "{moved} {stopped}");
}
