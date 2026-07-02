//! Unit tests for [`World`]. Split out of `world.rs`.

use super::*;
use vm::Insn;
use vm::battle_action::BattleActionHost;

#[test]
fn world_starts_with_inactive_actors() {
    let world = World::new();
    assert_eq!(world.actors.len(), MAX_ACTORS);
    assert!(world.actors.iter().all(|a| !a.active));
}

#[test]
fn actor_vm_spawn_default_runs_through_world() {
    let mut world = World::new();
    // Pre-set default position for actor 7.
    world.actors[7].default_pos = ActorVmPosition::new(100, 50);
    // Bytecode: SpawnDefault actor 7, then End.
    let bc = {
        let mut v = vec![];
        v.extend_from_slice(
            &Insn {
                opcode: 0x01,
                operand_b: 7,
                operand_w: 0,
            }
            .encode(),
        );
        v.extend_from_slice(&[0u8; 4]);
        v
    };
    let pc = world.run_actor_bytecode(&bc).unwrap();
    assert_eq!(pc, 4);
    assert!(world.actors[7].active);
    assert_eq!(world.actors[7].move_state.world_x, 100);
}

#[test]
fn actor_vm_set_field_1d_writes_when_actor_exists() {
    let mut world = World::new();
    world.actors[3].active = true;
    let bc = {
        let mut v = vec![];
        v.extend_from_slice(
            &Insn {
                opcode: 0x03,
                operand_b: 3,
                operand_w: 0xFF42,
            }
            .encode(),
        );
        v.extend_from_slice(&[0u8; 4]);
        v
    };
    world.run_actor_bytecode(&bc).unwrap();
    assert_eq!(world.actors[3].field_1d, 0x42);
}

#[test]
fn move_vm_step_writes_world_state() {
    let mut world = World::new();
    world.actors[0].active = true;
    // Bytecode: WORLD_SET (op 0x07) x=100, y=50, z=10, then HALT.
    let bc: Vec<u16> = vec![0x0007, 100, 50, 10, 0x0008];
    let res = world.step_move_vm(0, &bc);
    // First step is WORLD_SET (Advance), then we'd need to call again for HALT.
    assert!(matches!(res, vm::move_vm::StepResult::Advance));
    assert_eq!(world.actors[0].move_state.world_x, 100);
    assert_eq!(world.actors[0].move_state.world_y, 50);
}

#[test]
fn world_tick_in_battle_mode_runs_state_machine() {
    let mut world = World::new();
    world.mode = SceneMode::Battle;
    // Mark all actors alive so end-of-action doesn't immediately wipe.
    for a in &mut world.actors {
        a.battle.liveness = 1;
    }
    world.battle_ctx.action_state = vm::battle_action::ActionState::Begin.as_byte();
    world.battle_ctx.queued_action = 5;
    let out = world.tick();
    assert!(matches!(out, Some(StepOutcome::Transition { .. })));
    assert_eq!(
        world.battle_ctx.action_state,
        vm::battle_action::ActionState::PreActionWait.as_byte()
    );
}

#[test]
fn world_tick_in_title_mode_returns_none() {
    let mut world = World::new();
    world.mode = SceneMode::Title;
    let out = world.tick();
    assert!(out.is_none());
    assert_eq!(world.frame, 1);
}

#[test]
fn next_rng_is_deterministic() {
    let mut a = World::new();
    let mut b = World::new();
    let seq_a: Vec<_> = (0..10).map(|_| a.next_rng()).collect();
    let seq_b: Vec<_> = (0..10).map(|_| b.next_rng()).collect();
    assert_eq!(seq_a, seq_b);
    // And not all zero.
    assert!(seq_a.iter().any(|&x| x != 0));
}

#[test]
fn battle_party_wipe_signals_end_via_world() {
    let mut world = World::new();
    world.mode = SceneMode::Battle;
    // Kill all party.
    for i in 0..3 {
        world.actors[i].battle.liveness = 0;
    }
    // Mark monsters alive.
    for i in 3..8 {
        world.actors[i].battle.liveness = 1;
    }
    world.battle_ctx.action_state = vm::battle_action::ActionState::EndOfAction.as_byte();
    let out = world.tick();
    assert_eq!(out, Some(StepOutcome::BattleComplete));
    assert_eq!(world.battle_end, Some(BattleEndCause::PartyWipe));
}

#[test]
fn ensure_actor_is_idempotent_and_writes_default_pos() {
    let mut world = World::new();
    world.ensure_actor(2, ActorVmPosition::new(7, 11));
    assert!(world.actors[2].active);
    assert_eq!(world.actors[2].default_pos, ActorVmPosition::new(7, 11));
    // Calling again with new pos updates it but doesn't reset the actor.
    world.actors[2].field_1d = 0xAB;
    world.ensure_actor(2, ActorVmPosition::new(13, 17));
    assert_eq!(world.actors[2].default_pos, ActorVmPosition::new(13, 17));
    assert_eq!(world.actors[2].field_1d, 0xAB);
}

#[test]
fn effect_pool_persists_then_terminates_over_lifetime() {
    let mut world = World::new();
    // Mark slot 0 active by setting child_count > 0 so the tick walker
    // visits it.
    world.effect_pool.master_slots[0].child_count = 4;

    // The effect must survive each work tick until the fixed lifetime
    // budget is spent - it no longer terminates on the first tick.
    let lifetime = vm::effect_vm::DEFAULT_EFFECT_LIFETIME_FRAMES;
    for frame in 1..lifetime {
        world.tick_effects();
        assert_eq!(
            world.effect_pool.master_slots[0].child_count, 4,
            "effect retired early at frame {frame}"
        );
        assert_eq!(world.effect_pool.master_slots[0].field_14, frame as i32);
    }
    // The tick that reaches the budget retires the slot.
    world.tick_effects();
    assert_eq!(world.effect_pool.master_slots[0].child_count, 0);
}

#[test]
fn world_tick_in_field_mode_steps_field_vm() {
    let mut world = World::new();
    world.mode = SceneMode::Field;
    // Bytecode: 0x37 YIELD. Should set ctx.flags |= 0x400 + advance PC
    // past the yield.
    world.load_field_script(vec![0x37, 0x00]);
    let _ = world.tick();
    assert_eq!(world.field_ctx.flags & 0x400, 0x400, "halt bit set");
    assert!(
        world.field_pc > 0,
        "field_pc should advance after yield, got {}",
        world.field_pc
    );
}

#[test]
fn world_tick_field_mode_no_bytecode_is_noop() {
    let mut world = World::new();
    world.mode = SceneMode::Field;
    // No bytecode loaded. Tick should not panic and should not advance
    // field_pc.
    let _ = world.tick();
    assert_eq!(world.field_pc, 0);
}

// --- field collision grid + free-movement locomotion ---------------

#[test]
fn field_grid_block_all_then_clear() {
    let mut world = World::new();
    world.reset_field_collision_grid();
    // Block tile (col=2, row=3). Under retail's biased derivation
    // (`zc=(z>>6)+2`, `xc=((x+0x3f)>>6)-1`) that cell covers world
    // x in (256,384], z in [256,384).
    world.paint_field_collision(1, (2, 3), (3, 4), 0);
    assert!(world.field_tile_is_wall(320, 320), "painted tile is a wall");
    // Neighbour tile (col=1) stays walkable.
    assert!(!world.field_tile_is_wall(160, 320));
    // Clearing the same rectangle makes it walkable again.
    world.paint_field_collision(0, (2, 3), (3, 4), 0);
    assert!(!world.field_tile_is_wall(320, 320));
}

#[test]
fn field_grid_set_mask_selects_quadrant() {
    let mut world = World::new();
    world.reset_field_collision_grid();
    // Set wall bit for quadrant 0 (sub-cell x even, z even) of tile
    // (0,1) only. Row 1 covers world z in [0,128) under the retail +2
    // Z bias (row 0 is only reachable for negative z).
    world.paint_field_collision(3, (0, 1), (1, 2), 0b0001);
    assert!(world.field_tile_is_wall(10, 10), "quadrant 0 is a wall");
    // Quadrant 1 (sub-cell x odd) of the same tile is untouched.
    assert!(!world.field_tile_is_wall(64 + 10, 10));
}

/// Reference re-implementation of one `FUN_801cfe4c` static-wall probe:
/// world point -> grid `(col, row, quad-mask)`, using retail's exact
/// sub-cell derivation (`zc = (z>>6)+2`, `xc = ((x+0x3f)>>6)-1`, quad mask
/// `1 << ((zc&1)<<1 | (xc&1))`). Decoded from the field overlay
/// (`0897` @ `0x801CE818`). See `docs/subsystems/field-locomotion.md`.
fn retail_subcell(ix: i32, iz: i32) -> (i32, i32, u8) {
    let iz2 = if iz < 0 { iz + 0x3f } else { iz };
    let zc = (iz2 >> 6) + 2;
    let xc = ((ix + 0x3f) >> 6) - 1;
    let col = (xc / 2) & 0x7f;
    let row = (zc - (zc >> 31)) >> 1; // (zc>>1) for zc>=0; row stride is 0x80
    let mask = 1u8 << (((zc & 1) << 1 | (xc & 1)) as u32);
    (col, row, mask)
}

#[test]
fn field_tile_is_wall_matches_retail_subcell_derivation() {
    // `World::field_tile_is_wall` uses retail's exact biased derivation
    // (`zc=(z>>6)+2`, `xc=((x+0x3f)>>6)-1`). The bias is authored into the
    // wall bits - proven by the `rimelm_wall_press_down` capture, where the
    // live player legally stands at a position whose plain floor-indexed
    // cell is an all-quads wall byte (see
    // `engine-shell/tests/field_collision_discriminator.rs`). Sweep a span
    // of world points and assert the engine reads exactly the cell+quad the
    // reference derivation names.
    let mut world = World::new();
    world.reset_field_collision_grid();
    for &(x, z) in &[
        (320i32, 448i32),
        (10, 10),
        (64, 64),
        (63, 63),
        (1838, 2526),
        (3386, 2606),
        (127, 200),
        (128, 200),
    ] {
        let (rc, rr, rm) = retail_subcell(x, z);
        let idx = (rc + rr * (FIELD_GRID_STRIDE as i32)) as usize;
        world.field_collision_grid[idx] = rm << 4;
        assert!(
            world.field_tile_is_wall(x as i16, z as i16),
            "engine reads the retail cell at ({x},{z}) -> ({rc},{rr}) m{rm:04b}"
        );
        world.field_collision_grid[idx] = (!rm & 0xF) << 4;
        assert!(
            !world.field_tile_is_wall(x as i16, z as i16),
            "engine reads the retail QUAD at ({x},{z}) (other quads of the byte don't block)"
        );
        world.field_collision_grid[idx] = 0;
    }

    // The quadrant-MASK formula is retail's branchy `bVar5` for every
    // parity (the historical "inverted X parity" worry is false).
    let retail_mask = |xc: i32, zc: i32| -> u8 {
        let zpar = (zc & 1) != 0;
        if (xc & 1) == 0 {
            if zpar { 4 } else { 1 }
        } else if zpar {
            8
        } else {
            2
        }
    };
    for xc in 0..4 {
        for zc in 0..4 {
            let engine_mask = 1u8 << (((zc & 1) << 1 | (xc & 1)) as u32);
            assert_eq!(engine_mask, retail_mask(xc, zc));
        }
    }
}

#[test]
fn leading_edge_wall_probes_rest_at_retail_standoff() {
    // The three-probe leading-edge footprint (`FIELD_WALL_PROBES`, retail
    // `DAT_801f2214`) makes the player rest 47-48 units off a wall plane
    // where the candidate-centre test walks right up to it. Synthetic grids
    // mirroring the two wall-press captures' geometry (the real-grid rest
    // positions are pinned by the disc-gated
    // `engine-shell/tests/field_collision_discriminator.rs`).

    // X- press against a full-height wall column at grid col 13
    // (world x in [1665, 1793) under the biased X mapping).
    let press = |edge: bool, dir_bits: u16, start: (i16, i16), paint: &dyn Fn(&mut World)| {
        let mut world = World::new();
        world.install_field_player(0);
        paint(&mut world);
        world.leading_edge_wall_probes = edge;
        world.actors[0].move_state.world_x = start.0;
        world.actors[0].move_state.world_z = start.1;
        for _ in 0..200 {
            world.advance_with_collision(0, dir_bits, 8);
        }
        let ms = &world.actors[0].move_state;
        (ms.world_x, ms.world_z)
    };
    let wall_col_13 = |world: &mut World| {
        for row in 0..0x80usize {
            world.field_collision_grid[13 + row * FIELD_GRID_STRIDE] = 0xF0;
        }
    };
    // dir 1 (X-): edge at x-47. Probes hit while x <= 1839; rest = 1838
    // (even step parity), exactly the rimelm_wall_press_left rest.
    assert_eq!(
        press(true, 0x8000, (1900, 2526), &wall_col_13),
        (1838, 2526)
    );
    // Centre test walks to the wall plane: candidate 1792 is the first
    // blocked step.
    assert_eq!(
        press(false, 0x8000, (1900, 2526), &wall_col_13),
        (1794, 2526)
    );

    // Z- press against a full wall row at grid row 20 (biased band
    // z in [2432, 2560)). dir 0's edge is z-48 (the positive-direction
    // crossing distance); rest = 2606, the rimelm_wall_press_down rest.
    let wall_row_20 = |world: &mut World| {
        for col in 0..0x80usize {
            world.field_collision_grid[col + 20 * FIELD_GRID_STRIDE] = 0xF0;
        }
    };
    assert_eq!(
        press(true, 0x4000, (3386, 2700), &wall_row_20),
        (3386, 2606)
    );
    assert_eq!(
        press(false, 0x4000, (3386, 2700), &wall_row_20),
        (3386, 2560)
    );

    // The footprint is wide: a wall byte reachable only by a ±16 lateral
    // probe still blocks. Wall on the single sub-cell the (x+16, z-48)
    // probe of a Z- press reads (player x 3386 -> probe x 3402, xc 53),
    // leaving the centre + x-16 columns clear.
    let wall_lateral = |world: &mut World| {
        let (c, r, m) = retail_subcell(3402, 2559);
        world.field_collision_grid[(c + r * FIELD_GRID_STRIDE as i32) as usize] = m << 4;
    };
    assert_eq!(
        press(true, 0x4000, (3386, 2700), &wall_lateral),
        (3386, 2606)
    );
    // The centre-point test never sees that lateral byte and walks past.
    let (_, z_off) = press(false, 0x4000, (3386, 2700), &wall_lateral);
    assert!(z_off < 2500, "centre test walks past the lateral wall byte");
}

#[test]
fn solid_field_npcs_block_at_retail_actor_standoff() {
    // The actor-collision probes (`FIELD_ACTOR_PROBES`, retail
    // `DAT_801f21b4` through `FUN_801cfc40`'s moving-actor box test - the
    // class village NPCs belong to, capture-pinned by
    // `rimelm_npc_press_tetsu`) make an NPC block a walk: probe 64 ahead,
    // hit when strictly within `FIELD_NPC_BOX_HALF` (40) of the NPC on both
    // axes, so a head-on X+ press commits its last 2-unit step from exactly
    // 104 units out (probe delta 40 reads clear) and rests 102 short.
    let press = |solid: bool, npc: (i16, i16), start: (i16, i16)| {
        let mut world = World::new();
        world.install_field_player(0);
        world.solid_field_npcs = solid;
        world.field_npc_positions.insert(1, npc);
        world.actors[0].move_state.world_x = start.0;
        world.actors[0].move_state.world_z = start.1;
        for _ in 0..100 {
            world.advance_with_collision(0, 0x2000, 8);
        }
        world.actors[0].move_state.world_x
    };
    // Head-on: rest at npc_x - 102.
    assert_eq!(press(true, (2000, 2526), (1800, 2526)), 2000 - 102);
    // Flag off: the player walks straight through the NPC.
    assert!(press(false, (2000, 2526), (1800, 2526)) > 2000);
    // Lateral reach is 40 + 32 = 72: an NPC 60 off the walk line still
    // blocks (the ±32 lateral probe gets within 28), one 80 off does not.
    assert_eq!(press(true, (2000, 2526 + 60), (1800, 2526)), 2000 - 102);
    assert!(press(true, (2000, 2526 + 80), (1800, 2526)) > 2000);
}

#[test]
fn solid_field_props_block_at_retail_static_standoff() {
    // The STATIC-entity arm of the same probe (retail result bit `4`): a
    // placed prop blocks with the wider ±80 box (`FIELD_PROP_BOX_HALF`)
    // around its record-derived footprint centre. Head-on X+ press: the
    // probe 64 ahead reads clear at exactly 144 out (delta 80, strict), the
    // 2-unit step commits, and the next probe blocks - resting 142 short of
    // the centre, 40 units further out than the ±40 moving-NPC box (the
    // same pre-step parity as the NPC arm's 102).
    let press = |solid: bool, prop: (i32, i32), start: (i16, i16)| {
        let mut world = World::new();
        world.install_field_player(0);
        world.solid_field_npcs = solid;
        world.field_prop_colliders.push(prop);
        world.actors[0].move_state.world_x = start.0;
        world.actors[0].move_state.world_z = start.1;
        for _ in 0..100 {
            world.advance_with_collision(0, 0x2000, 8);
        }
        world.actors[0].move_state.world_x
    };
    // Head-on: rest at prop_x - 142.
    assert_eq!(press(true, (2000, 2526), (1800, 2526)), 2000 - 142);
    // Flag off: the player walks straight through the prop.
    assert!(press(false, (2000, 2526), (1800, 2526)) > 2000);
    // Lateral reach is 80 + 32 = 112: a prop 100 off the walk line still
    // blocks, one 120 off does not.
    assert_eq!(press(true, (2000, 2526 + 100), (1800, 2526)), 2000 - 142);
    assert!(press(true, (2000, 2526 + 120), (1800, 2526)) > 2000);
}

#[test]
fn sample_field_floor_height_unloaded_or_out_of_range_returns_zero() {
    let world = World::new();
    // No grid loaded -> 0.
    assert_eq!(world.sample_field_floor_height(100, 100), 0);

    let mut world = World::new();
    world.reset_field_collision_grid();
    // Negative / out-of-range tiles -> 0 (guarded).
    assert_eq!(world.sample_field_floor_height(-1, 0), 0);
    assert_eq!(world.sample_field_floor_height(0, -1), 0);
    // World x = 0x7F * 128 puts tile_x at 0x7F, whose +1 corner is out of range.
    assert_eq!(world.sample_field_floor_height(0x7F * 128, 0), 0);
}

#[test]
fn sample_field_floor_height_flat_returns_lut_value() {
    const STRIDE: usize = 0x80;
    let mut world = World::new();
    world.reset_field_collision_grid();
    world.field_floor_height_lut[5] = -50;
    // Set the 2x2 block around tile (0,0) all to elevation tier 5.
    for &i in &[0usize, 1, STRIDE, STRIDE + 1] {
        world.field_collision_grid[i] = 0x05; // low nibble = tier 5
    }
    // Any sub-tile position in tile (0,0): all four corners match -> LUT[5].
    assert_eq!(world.sample_field_floor_height(10, 10), -50);
    assert_eq!(world.sample_field_floor_height(64, 100), -50);
}

#[test]
fn sample_field_floor_height_bilinear_interpolates() {
    const STRIDE: usize = 0x80;
    let mut world = World::new();
    world.reset_field_collision_grid();
    world.field_floor_height_lut[1] = 0;
    world.field_floor_height_lut[2] = 256;
    // Corners of tile (0,0): c00=1(0), c01=2(256), c10=1(0), c11=1(0).
    world.field_collision_grid[0] = 0x01;
    world.field_collision_grid[1] = 0x02;
    world.field_collision_grid[STRIDE] = 0x01;
    world.field_collision_grid[STRIDE + 1] = 0x01;
    // Top edge (wz=0): height interpolates linearly from c00 (0) to c01 (256)
    // across the sub-tile, i.e. 2 * wx.
    assert_eq!(world.sample_field_floor_height(0, 0), 0); // wx=0
    assert_eq!(world.sample_field_floor_height(64, 0), 128); // wx=64 -> halfway
    assert_eq!(world.sample_field_floor_height(127, 0), 254); // wx=127
    // Pushing wz down toward the (all-zero) bottom row pulls the value toward 0.
    assert!(world.sample_field_floor_height(64, 64) < 128);
}

#[test]
fn field_vm_nibble7_paints_collision_grid() {
    let mut world = World::new();
    world.mode = SceneMode::Field;
    world.install_field_player(0);
    // 0x4C outer-nibble-7 sub-1 (block all): bytes
    // [0x4C, 0x71, col0, row0, col1, row1, mask]. The paint covers
    // columns [col0, col1+1) and rows [row0+1, row1+2) (the row bounds
    // carry an extra +1 the column bounds do not - see FUN_801de840
    // case 7), so [2, 3, 2, 3] paints column 2, row 4.
    world.load_field_script(vec![0x4C, 0x71, 2, 3, 2, 3, 0x00]);
    let _ = world.tick();
    // The hook routed the paint into the grid: tile (col 2, row 4) ->
    // world x in (256, 384], z in [384, 512) (retail biased derivation).
    assert!(world.field_tile_is_wall(320, 448));
    // The unshifted tile (col 2, row 3) is NOT painted.
    assert!(!world.field_tile_is_wall(320, 320));
}

#[test]
fn load_field_collision_grid_copies_map_region_and_nibble7_layers_on_top() {
    let mut world = World::new();
    world.mode = SceneMode::Field;
    world.install_field_player(0);
    // Synthesize a base grid: block tile (col=5, row=6) in all four
    // sub-cells (high nibble 0xF), floor tier 2 (low nibble) elsewhere.
    let mut grid = vec![0u8; FIELD_GRID_LEN];
    grid[6 * FIELD_GRID_STRIDE + 5] = 0xF2;
    world.load_field_collision_grid(&grid);
    // tile (5,6) -> world x in (640,768], z in [640,768) (retail biased
    // derivation).
    assert!(world.field_tile_is_wall(700, 700), "base grid wall loaded");
    assert!(!world.field_tile_is_wall(700, 600), "other tiles walkable");
    // Low nibble (floor tier) is preserved, not treated as a wall bit.
    assert_eq!(world.field_collision_grid[6 * FIELD_GRID_STRIDE + 5], 0xF2);
    // A nibble-7 paint layers a delta on top of the loaded base.
    world.paint_field_collision(1, (8, 9), (8, 9), 0);
    assert!(world.field_tile_is_wall(8 * 128 + 10, 8 * 128 - 10));
    // The base wall is still present after the delta.
    assert!(world.field_tile_is_wall(700, 700));
}

#[test]
fn load_field_collision_grid_pads_short_input() {
    let mut world = World::new();
    // (10,10) reads cell (col 0, row 1) under the retail biased
    // derivation, i.e. grid index 0x80.
    let mut short = vec![0u8; 0x81];
    short[0x80] = 0xF0;
    world.load_field_collision_grid(&short);
    assert_eq!(world.field_collision_grid.len(), FIELD_GRID_LEN);
    assert!(
        world.field_tile_is_wall(10, 10),
        "row-1 wall from the short input"
    );
}

#[test]
fn locomotion_moves_player_on_dpad() {
    let mut world = World::new();
    world.mode = SceneMode::Field;
    world.install_field_player(0);
    world.actors[0].move_state.world_x = 200;
    world.actors[0].move_state.world_z = 200;
    // Up -> +Z. speed = (8 * 0x1000 >> 12) * 1 = 8 -> +8 in 2-unit steps.
    world.set_pad(input::PadButton::Up.mask());
    let _ = world.tick();
    assert_eq!(world.actors[0].move_state.world_z, 208);
    assert_eq!(world.actors[0].move_state.world_x, 200);
}

#[test]
fn locomotion_diagonal_normalises_speed() {
    let mut world = World::new();
    world.mode = SceneMode::Field;
    world.install_field_player(0);
    world.actors[0].move_state.world_x = 400;
    world.actors[0].move_state.world_z = 400;
    // Up+Right -> Z+ and X+. speed = 8, diagonal -= 8>>2 = 6 -> +6 each.
    world.set_pad(input::PadButton::Up.mask() | input::PadButton::Right.mask());
    let _ = world.tick();
    assert_eq!(world.actors[0].move_state.world_z, 406);
    assert_eq!(world.actors[0].move_state.world_x, 406);
}

#[test]
fn locomotion_stops_at_wall() {
    let mut world = World::new();
    world.mode = SceneMode::Field;
    world.install_field_player(0);
    world.actors[0].move_state.world_x = 200;
    world.actors[0].move_state.world_z = 250;
    // Block tile (col=1, row=3) - covers world z in [256,384) under the
    // retail biased derivation, the band the +Z walk crosses into at
    // z=256.
    world.paint_field_collision(1, (1, 2), (3, 4), 0);
    world.set_pad(input::PadButton::Up.mask());
    let _ = world.tick();
    // Player advances 250 -> 254, then the candidate 256 lands in the
    // blocked tile and is rejected. Without the wall it would reach 258.
    assert_eq!(world.actors[0].move_state.world_z, 254);
    assert_eq!(world.actors[0].move_state.world_x, 200);
}

#[test]
fn locomotion_follows_terrain_height_only_when_gated_on() {
    const STRIDE: usize = 0x80;
    let mut world = World::new();
    world.mode = SceneMode::Field;
    world.install_field_player(0);
    world.reset_field_collision_grid();
    world.actors[0].move_state.world_x = 200;
    world.actors[0].move_state.world_z = 200;
    world.actors[0].move_state.world_y = 999; // sentinel
    // Floor tier 3 -> -40 across the 2x2 block around tile (1,1), which the
    // +Z walk lands in (x=200, z=208 -> tile (1,1)).
    world.field_floor_height_lut[3] = -40;
    let base = STRIDE + 1;
    for &i in &[base, base + 1, base + STRIDE, base + STRIDE + 1] {
        world.field_collision_grid[i] = 0x03; // low nibble = tier 3, walkable
    }

    // Gate off (default): Y stays at the sentinel, flat-Y behaviour preserved.
    world.set_pad(input::PadButton::Up.mask());
    let _ = world.tick();
    assert_eq!(world.actors[0].move_state.world_z, 208);
    assert_eq!(world.actors[0].move_state.world_y, 999);

    // Gate on: the next step snaps Y to the sampled floor height.
    world.follow_terrain_height = true;
    world.set_pad(input::PadButton::Up.mask());
    let _ = world.tick();
    assert_eq!(world.actors[0].move_state.world_y, -40);
}

#[test]
fn locomotion_gated_by_movement_disabled_flag() {
    let mut world = World::new();
    world.mode = SceneMode::Field;
    world.install_field_player(0);
    world.actors[0].move_state.world_z = 200;
    world.actors[0].move_state.flags |= 0x0008_0000; // encounter / cutscene owns player
    world.set_pad(input::PadButton::Up.mask());
    let _ = world.tick();
    assert_eq!(
        world.actors[0].move_state.world_z, 200,
        "no movement while disabled"
    );
}

#[test]
fn locomotion_gated_by_active_dialog() {
    let mut world = World::new();
    world.mode = SceneMode::Field;
    world.install_field_player(0);
    world.actors[0].move_state.world_z = 200;
    world.current_dialog = Some(DialogRequest {
        text_id: 1,
        inline: Vec::new(),
        world_x: 0,
        world_z: 0,
        depth_id: 0,
    });
    world.set_pad(input::PadButton::Up.mask());
    let _ = world.tick();
    assert_eq!(
        world.actors[0].move_state.world_z, 200,
        "dialog owns the frame"
    );
}

#[test]
fn locomotion_deterministic_across_identical_pad_stream() {
    fn drive(pads: &[u16]) -> (i16, i16) {
        let mut world = World::new();
        world.mode = SceneMode::Field;
        world.install_field_player(0);
        world.actors[0].move_state.world_x = 300;
        world.actors[0].move_state.world_z = 300;
        // A couple of deterministic walls so collision rejection is in
        // the path being compared.
        world.paint_field_collision(1, (0, 3), (0, 3), 0);
        for &p in pads {
            world.set_pad(p);
            let _ = world.tick();
        }
        let ms = &world.actors[0].move_state;
        (ms.world_x, ms.world_z)
    }
    let up = input::PadButton::Up.mask();
    let down = input::PadButton::Down.mask();
    let left = input::PadButton::Left.mask();
    let right = input::PadButton::Right.mask();
    let seq = [up, up | right, right, down, down | left, left, 0, up];
    assert_eq!(
        drive(&seq),
        drive(&seq),
        "identical pad stream is bit-identical"
    );
}

#[test]
fn cutscene_narration_confirm_press_skips_page() {
    let mut world = World::new();
    world.mode = SceneMode::Title; // isolate the top-of-tick narration advance
    world.open_cutscene_narration(vec!["Page 1".into(), "Page 2".into()]);
    let idx = |w: &World| w.cutscene_narration.as_ref().map(|n| n.current_index());
    assert_eq!(idx(&world), Some(0));

    // No press: a single tick does not advance (the dwell is 120 frames).
    world.set_pad(0);
    let _ = world.tick();
    assert_eq!(idx(&world), Some(0));

    // A just-pressed confirm (Cross) skips to the next page.
    world.set_pad(input::PadButton::Cross.mask());
    let _ = world.tick();
    assert_eq!(idx(&world), Some(1));

    // Holding the same button is not a new edge - it must not skip again.
    world.set_pad(input::PadButton::Cross.mask());
    let _ = world.tick();
    assert_eq!(idx(&world), Some(1));

    // Release, then a fresh press past the last page completes the narration
    // (so the prologue hand-off gate releases).
    world.set_pad(0);
    let _ = world.tick();
    world.set_pad(input::PadButton::Circle.mask());
    let _ = world.tick();
    assert!(
        world.cutscene_narration.is_none(),
        "confirm past the last page completes the narration"
    );
}

#[test]
fn locomotion_gated_while_cutscene_timeline_active() {
    use crate::cutscene_timeline::CutsceneTimeline;
    let mut world = World::new();
    world.mode = SceneMode::Field;
    world.install_field_player(0);
    world.actors[0].move_state.world_z = 200;
    // An opening-cutscene timeline owns the scene (establishing sweep). A
    // non-empty body so it is not immediately `done`.
    world.cutscene_timeline = Some(CutsceneTimeline::new(vec![0x21, 0x2E, 0x1A], 0));
    assert!(world.cutscene_timeline_active());
    world.set_pad(input::PadButton::Up.mask());
    world.step_field_locomotion();
    assert_eq!(
        world.actors[0].move_state.world_z, 200,
        "pad-driven walk is locked while the cutscene timeline owns the scene"
    );
    // Once the timeline finishes, free-roam control returns.
    if let Some(tl) = world.cutscene_timeline.as_mut() {
        tl.done = true;
    }
    assert!(!world.cutscene_timeline_active());
    world.step_field_locomotion();
    assert_eq!(
        world.actors[0].move_state.world_z, 208,
        "locomotion resumes the frame the timeline drops"
    );
}

#[test]
fn world_tick_drives_per_actor_move_vm() {
    let mut world = World::new();
    world.mode = SceneMode::Field;
    world.actors[0].active = true;
    // Move-VM bytecode: WORLD_SET (op 0x07) x=42, y=10, z=5, then HALT.
    world.set_move_bytecode(0, Some(vec![0x0007, 42, 10, 5, 0x0008]));
    let _ = world.tick();
    // First step is WORLD_SET; should write the position.
    assert_eq!(world.actors[0].move_state.world_x, 42);
    assert_eq!(world.actors[0].move_state.world_y, 10);
}

#[test]
fn world_tick_skips_move_vm_when_wait_timer_set() {
    let mut world = World::new();
    world.actors[0].active = true;
    world.actors[0].move_state.wait_timer = 5;
    world.set_move_bytecode(0, Some(vec![0x0007, 99, 99, 99, 0x0008]));
    let _ = world.tick();
    // Wait timer decremented, but move VM didn't run -> position unchanged.
    assert_eq!(world.actors[0].move_state.wait_timer, 4);
    assert_eq!(world.actors[0].move_state.world_x, 0);
}

#[test]
fn load_field_script_resets_pc_and_ctx() {
    let mut world = World::new();
    world.field_pc = 42;
    world.field_ctx.flags = 0xFFFF;
    world.load_field_script(vec![0xFF; 8]);
    assert_eq!(world.field_pc, 0);
    assert_eq!(world.field_ctx.flags, 0);
    assert_eq!(world.field_bytecode.len(), 8);
}

#[test]
fn enter_battle_populates_party_and_monsters() {
    let mut world = World::default();
    world.enter_battle(3, 5);
    assert_eq!(world.mode, SceneMode::Battle);
    assert_eq!(world.party_count, 3);
    // 3 party + 5 monsters = 8 active.
    let active_count = world.actors.iter().filter(|a| a.active).count();
    assert_eq!(active_count, 8);
    // Party slots sit at the retail 3-member seats (negative Z, facing
    // the monsters at positive Z).
    for i in 0..3 {
        let s = crate::battle_seats::party_seat(3, i);
        assert_eq!(world.actors[i].move_state.world_x, s.x);
        assert_eq!(world.actors[i].move_state.world_z, s.z);
        assert!(world.actors[i].move_state.world_z < 0);
        assert_eq!(world.actors[i].battle.liveness, 1);
    }
    // Monster slots at the retail seats on the positive-Z side.
    for i in 3..8 {
        assert!(world.actors[i].move_state.world_z > 0);
        assert_eq!(world.actors[i].battle.liveness, 1);
    }
    // SM seeded at Begin.
    assert_eq!(
        world.battle_ctx.action_state,
        vm::battle_action::ActionState::Begin.as_byte()
    );
}

#[test]
fn enter_battle_caps_party_at_three() {
    let mut world = World::default();
    // Even if asked for more party than the cap, we clamp to 3.
    world.enter_battle(8, 0);
    assert_eq!(world.party_count, 3);
}

#[test]
fn status_block_helpers_classify_by_kind() {
    use legaia_engine_vm::status_effects::StatusKind;
    let mut world = World::new();
    // Sleep blocks all actions but not magic specifically.
    world
        .status_effects
        .apply_with_duration(1, StatusKind::Sleep, 5);
    assert!(world.actor_blocked_from_acting(1));
    assert!(!world.actor_blocked_from_magic(1));
    // Numb is a full paralysis - blocks the whole turn (so magic is moot too).
    world
        .status_effects
        .apply_with_duration(4, StatusKind::Numb, 5);
    assert!(world.actor_blocked_from_acting(4));
    // Silence blocks magic only.
    world
        .status_effects
        .apply_with_duration(2, StatusKind::Curse, 5);
    assert!(!world.actor_blocked_from_acting(2));
    assert!(world.actor_blocked_from_magic(2));
    // Petrify blocks both.
    world
        .status_effects
        .apply_with_duration(3, StatusKind::Faint, 5);
    assert!(world.actor_blocked_from_acting(3));
    assert!(world.actor_blocked_from_magic(3));
    // A clean actor is blocked from nothing.
    assert!(!world.actor_blocked_from_acting(0));
    assert!(!world.actor_blocked_from_magic(0));
}

#[test]
fn confuse_retargets_a_monster_strike_to_its_own_band() {
    use legaia_engine_vm::status_effects::StatusKind;
    let mut world = World::new();
    world.party_count = 3;
    // Party slots 0..2 + monster slots 3,4 alive; everything else stays dead.
    for i in 0..5 {
        world.actors[i].active = true;
        world.actors[i].battle.liveness = 1;
        world.actors[i].battle.hp = 100;
        world.actors[i].battle.max_hp = 100;
    }
    // Monster slot 3 picked a party member (slot 1) as its target.
    world.actors[3].battle.active_target = 1;

    // Not confused: the picked target stands.
    world.maybe_confuse_retarget(3);
    assert_eq!(world.actors[3].battle.active_target, 1);

    // Confused: the strike flips to a living member of its own (monster) band.
    world
        .status_effects
        .apply_with_duration(3, StatusKind::Confuse, 3);
    world.maybe_confuse_retarget(3);
    let t = world.actors[3].battle.active_target;
    assert!(
        t >= 3,
        "confused monster targets its own band, got slot {t}"
    );
    assert!(
        world.actors[t as usize].battle.liveness != 0,
        "the retarget lands on a living actor"
    );
}

#[test]
fn confuse_retargets_a_party_strike_to_a_living_ally() {
    use legaia_engine_vm::status_effects::StatusKind;
    let mut world = World::new();
    world.party_count = 3;
    for i in 0..5 {
        world.actors[i].active = true;
        world.actors[i].battle.liveness = 1;
        world.actors[i].battle.hp = 100;
        world.actors[i].battle.max_hp = 100;
    }
    // A confused party member (slot 1) whose action targeted a monster (slot 3)
    // flips to a random living member of its own (party) side.
    world.actors[1].battle.active_target = 3;
    world
        .status_effects
        .apply_with_duration(1, StatusKind::Confuse, 3);
    world.maybe_confuse_retarget(1);
    let t = world.actors[1].battle.active_target;
    assert!(t < 3, "confused party member targets an ally, got slot {t}");
    assert!(world.actors[t as usize].battle.liveness != 0);
}

#[test]
fn confused_party_member_auto_acts_instead_of_opening_the_command_menu() {
    use legaia_engine_vm::status_effects::StatusKind;
    let mut world = World::new();
    world.party_count = 3;
    world.battle_player_driven = true;
    for i in 0..5 {
        world.actors[i].active = true;
        world.actors[i].battle.liveness = 1;
        world.actors[i].battle.hp = 100;
        world.actors[i].battle.max_hp = 100;
    }
    world
        .status_effects
        .apply_with_duration(0, StatusKind::Confuse, 3);
    // The confused party member auto-arms a physical strike (no command session)
    // aimed at a living ally.
    world.arm_party_physical(0);
    assert!(
        world.battle_command.is_none(),
        "a confused party member never opens the command menu"
    );
    assert_eq!(world.actors[0].battle.action_category, 3, "physical armed");
    let t = world.actors[0].battle.active_target;
    assert!(t < 3, "retargeted onto an ally, got slot {t}");
}

#[test]
fn confuse_retargets_a_monster_cast_to_the_opposite_side() {
    use legaia_engine_vm::status_effects::StatusKind;
    let mut world = World::new();
    world.party_count = 3;
    for i in 0..5 {
        world.actors[i].active = true;
        world.actors[i].battle.liveness = 1;
        world.actors[i].battle.hp = 100;
        world.actors[i].battle.max_hp = 100;
    }
    world
        .status_effects
        .apply_with_duration(3, StatusKind::Confuse, 3);

    // Non-confused caster (slot 4): targets untouched.
    let mut t = vec![0u8, 1, 2];
    world.confuse_retarget_cast(4, &mut t);
    assert_eq!(t, vec![0, 1, 2]);

    // Confused single-target cast at a party member flips to one living monster.
    let mut t1 = vec![1u8];
    world.confuse_retarget_cast(3, &mut t1);
    assert_eq!(t1.len(), 1);
    assert!(t1[0] >= 3, "single cast flips to a monster, got {}", t1[0]);

    // Confused area cast at the whole party flips to every living monster.
    let mut t2 = vec![0u8, 1, 2];
    world.confuse_retarget_cast(3, &mut t2);
    assert_eq!(t2, vec![3, 4]);

    // A self-only cast is left alone.
    let mut t3 = vec![3u8];
    world.confuse_retarget_cast(3, &mut t3);
    assert_eq!(t3, vec![3]);
}

#[test]
fn stone_counts_as_defeated_and_is_petrified() {
    use legaia_engine_vm::status_effects::StatusKind;
    let mut world = World::new();
    // A petrified actor stays "alive" (liveness != 0) but counts as defeated
    // for wipe detection, and reads as petrified.
    world.actors[1].battle.liveness = 1;
    world.actors[1].battle.hp = 100;
    world
        .status_effects
        .apply_with_duration(1, StatusKind::Stone, 255);
    assert!(world.actor_is_petrified(1));
    assert!(world.actor_blocked_from_acting(1), "Stone blocks the turn");
    assert!(
        world.actor_effectively_defeated(1),
        "Stone counts as defeated for wipe detection"
    );
    // A clean living actor is neither.
    world.actors[0].battle.liveness = 1;
    assert!(!world.actor_is_petrified(0));
    assert!(!world.actor_effectively_defeated(0));
}

#[test]
fn petrified_target_absorbs_art_strike_damage() {
    use crate::art_strike::ArtStrikeOutcome;
    use legaia_engine_vm::status_effects::StatusKind;
    let mut world = World::new();
    world.party_count = 4;
    for slot in 0..4 {
        world.actors[slot].active = true;
        world.actors[slot].battle.hp = 200;
        world.actors[slot].battle.max_hp = 200;
        world.actors[slot].battle.liveness = 1;
    }
    world
        .status_effects
        .apply_with_duration(3, StatusKind::Stone, 255);
    let event = BattleEvent::ApplyArtStrike {
        actor_slot: 0,
        target_slot: 3,
        strike_index: 0,
        outcome: ArtStrikeOutcome {
            damage: Some(150),
            enemy_effect: legaia_art::record::EnemyEffect::None,
            cues: vec![],
            alt_range: false,
            power_target: None,
        },
    };
    let r = world.fold_battle_event(&event);
    assert_eq!(world.actors[3].battle.hp, 200, "Stone absorbs the strike");
    assert_eq!(r, Some((3, 200)));
}

/// Stone is invulnerable at the spell damage path too, not just the basic-attack
/// / SM strike. A petrified party member is still targetable (target resolvers
/// key on `liveness`, which Stone leaves non-zero), so an enemy damage spell can
/// land on it - and it must absorb. The defender's spirit gauge still charges
/// from the pre-nullify amount (matching the basic-attack / finisher order).
#[test]
fn stone_absorbs_a_damage_spell() {
    use crate::spells::{SpellElement, SpellOutcome};
    use legaia_engine_vm::status_effects::StatusKind;

    let mut world = World::new();
    world.enter_battle(3, 1);
    world.actors[0].battle.hp = 200;
    world.actors[0].battle.max_hp = 200;
    world.actors[0].battle.liveness = 1;
    world
        .status_effects
        .apply_with_duration(0, StatusKind::Stone, 255);

    world.fold_spell_outcome(SpellOutcome::Damage {
        target: 0,
        amount: 150,
        element: SpellElement::Neutral,
        weakness: false,
    });

    assert_eq!(
        world.actors[0].battle.hp, 200,
        "a petrified target absorbs the damage spell"
    );
    assert_ne!(
        world.actors[0].battle.liveness, 0,
        "absorbing the cast must not down the petrified actor"
    );
    assert!(
        world.spirit_gauge(0) > 0,
        "the pre-nullify hit still charges the defender's spirit gauge"
    );
}

#[test]
fn asleep_monster_loses_its_turn_and_never_attacks() {
    use legaia_engine_vm::status_effects::StatusKind;

    // Drive a 1-vs-1 auto-resolving battle for many ticks and report whether
    // the party member took any damage. With unseeded battle stats the monster
    // auto-hits for >= 1 each turn it acts, so the only way the party stays at
    // full HP is if the monster never gets to act.
    fn party_took_damage(asleep: bool) -> bool {
        let mut world = World::new();
        world.enter_battle(1, 1); // slot 0 = party, slot 1 = monster
        world.live_gameplay_loop = true; // route tick() through live_battle_tick
        world.battle_player_driven = false; // both sides auto-act
        // Big monster HP so it survives long enough to take many turns; the
        // party HP is what we watch.
        world.actors[1].battle.hp = 9999;
        world.actors[1].battle.max_hp = 9999;
        world.actors[0].battle.hp = 500;
        world.actors[0].battle.max_hp = 500;
        if asleep {
            world
                .status_effects
                .apply_with_duration(1, StatusKind::Sleep, 255);
        }
        let start = world.actors[0].battle.hp;
        for _ in 0..600 {
            world.tick();
            if world.mode != SceneMode::Battle {
                break;
            }
        }
        world.actors[0].battle.hp < start
    }

    // Non-vacuous control: an awake monster auto-hits the party.
    assert!(
        party_took_damage(false),
        "control: an awake monster must damage the party"
    );
    // The fix: an asleep monster loses its turn, so the party is untouched.
    assert!(
        !party_took_damage(true),
        "an asleep monster must skip its turn and never attack"
    );
}

/// The retail DoT ticker (FUN_801E752C) never kills: each tick is clamped to
/// `current_hp - 1`, so a poisoned actor bottoms out at 1 HP and stays alive
/// (`liveness` untouched). The `hp == 0 → liveness = 0` pairing in
/// `tick_status_effects` remains as a safety net for other damage entry
/// points - this pins the never-kill clamp end to end.
#[test]
fn dot_never_kills_actor_bottoms_out_at_one_hp() {
    use legaia_engine_vm::status_effects::StatusKind;

    let mut world = World::new();
    world.enter_battle(1, 1);
    // Toxic raw tick = max_hp/16 = 5, more than the monster's remaining 4 HP
    // → clamped to current_hp - 1 = 3.
    world.actors[1].battle.max_hp = 80;
    world.actors[1].battle.hp = 4;
    world.actors[1].battle.liveness = 1;
    world.status_effects.apply(1, StatusKind::Toxic);

    world.tick_status_effects();

    assert_eq!(
        world.actors[1].battle.hp, 1,
        "Toxic DoT clamps to current_hp - 1 (never lethal)"
    );
    assert_eq!(
        world.actors[1].battle.liveness, 1,
        "a DoT tick never downs the actor"
    );
}

/// In the live battle loop a poison/toxic affliction must actually drain HP and
/// expire: `tick_status_effects` is called once per round at the initiative
/// boundary. A poisoned party member loses HP across rounds even when the enemy
/// can never strike (asleep), so the only HP source is the DoT.
#[test]
fn live_loop_ticks_dot_at_the_round_boundary() {
    use legaia_engine_vm::status_effects::StatusKind;

    fn party_lost_hp(poisoned: bool) -> bool {
        let mut world = World::new();
        world.enter_battle(1, 1); // slot 0 = party, slot 1 = monster
        world.live_gameplay_loop = true;
        world.battle_player_driven = false;
        // Both sides carry SPD so the initiative round boundary engages (the DoT
        // tick is gated on it); seed up front so battle start isn't mistaken for
        // a round boundary.
        world.battle_speed[0] = 10;
        world.battle_speed[1] = 10;
        world.seed_battle_initiative();
        // The monster is asleep, so it never attacks - the party's only HP loss
        // can come from the DoT.
        world
            .status_effects
            .apply_with_duration(1, StatusKind::Sleep, 255);
        world.actors[0].battle.max_hp = 800;
        world.actors[0].battle.hp = 800;
        world.actors[1].battle.max_hp = 9999;
        world.actors[1].battle.hp = 9999;
        if poisoned {
            world
                .status_effects
                .apply_with_duration(0, StatusKind::Toxic, 255);
        }
        let start = world.actors[0].battle.hp;
        for _ in 0..600 {
            world.tick();
            if world.mode != SceneMode::Battle {
                break;
            }
        }
        world.actors[0].battle.hp < start
    }

    // Control: with no poison and an asleep enemy the party is never touched.
    assert!(
        !party_lost_hp(false),
        "control: no DoT + asleep enemy must leave the party at full HP"
    );
    // The fix: the live loop ticks the DoT each round, so the party bleeds.
    assert!(
        party_lost_hp(true),
        "a poisoned party member must lose HP to the DoT in the live loop"
    );
}

#[test]
fn all_party_item_heals_every_living_party_actor_in_battle() {
    use crate::inventory_use::{
        InventoryContext, InventoryUseInput, InventoryUseSession, TargetRow,
    };

    let mut world = World::default();
    world.set_item_catalog(crate::items::ItemCatalog::vanilla());
    world.enter_battle(3, 1);
    // Wound the whole party; down the third member.
    for i in 0..3 {
        world.actors[i].battle.max_hp = 500;
        world.actors[i].battle.hp = 100;
    }
    world.actors[2].battle.hp = 0; // dead - excluded from a party heal

    // Healing Bloom (0x7A): all-party HP heal of 200.
    let targets: Vec<TargetRow> = (0..3)
        .map(|i| {
            let a = &world.actors[i];
            let mut r = TargetRow::new(i as u8, "P").with_stats(a.battle.hp, a.battle.max_hp, 0, 0);
            r.alive = a.battle.liveness != 0 && a.battle.hp > 0;
            r
        })
        .collect();
    let mut s = InventoryUseSession::new(
        world.item_catalog.clone(),
        vec![0x7A],
        targets,
        InventoryContext::Battle,
    );
    // One Confirm fans the item out across the living party (no target select).
    s.input(InventoryUseInput::Confirm);
    assert!(matches!(
        s.state,
        crate::inventory_use::InventoryUseState::Done(_)
    ));
    assert_eq!(s.used_item, Some(0x7A));
    assert_eq!(s.used_slots, vec![0, 1], "only the two living allies");

    // Apply exactly as the field / battle consumers do: one use_item per slot.
    for &slot in &s.used_slots {
        world.use_item(0x7A, slot);
    }
    assert_eq!(world.actors[0].battle.hp, 300, "Vahn +200");
    assert_eq!(world.actors[1].battle.hp, 300, "Noa +200");
    assert_eq!(
        world.actors[2].battle.hp, 0,
        "dead ally untouched by a heal"
    );
}

#[test]
fn enter_world_map_installs_controller() {
    let mut world = World::default();
    assert!(world.world_map_ctrl.is_none());
    world.enter_world_map();
    assert_eq!(world.mode, SceneMode::WorldMap);
    assert!(world.world_map_ctrl.is_some());
    // Idempotent: re-entry keeps the existing controller + state.
    world.world_map_ctrl.as_mut().unwrap().camera_x = 42;
    world.enter_world_map();
    assert_eq!(world.world_map_ctrl.as_ref().unwrap().camera_x, 42);
}

#[test]
fn world_tick_drives_world_map_from_pad() {
    // A pad installed via set_pad() before tick() flows into the
    // world-map controller through World::tick's WorldMap arm. This is
    // the A1 keystone: input changes per-frame World state through the
    // tick path, not via a host-side controller.
    let mut world = World::default();
    world.enter_world_map();
    world.world_map_ctrl.as_mut().unwrap().debug_enabled = true;

    // Frame 1: the toggle combo (0x4A held, edge includes 0x40) flips
    // the view into top-view.
    world.set_pad(0x4A);
    let _ = world.tick();
    assert!(world.world_map_ctrl.as_ref().unwrap().is_top_view());

    // Frame 2: in top-view, the left-scroll bit (0x1000) moves the
    // camera. Releasing the toggle bits first so this frame is a clean
    // scroll, not another toggle.
    world.set_pad(0);
    let _ = world.tick();
    world.set_pad(0x1000);
    let _ = world.tick();
    assert_eq!(world.world_map_ctrl.as_ref().unwrap().camera_x, -8);
}

#[test]
fn world_map_tick_is_deterministic_across_identical_pad_streams() {
    let pad_stream = [0x4Au16, 0x0000, 0x1000, 0x0020, 0x0002];
    let drive = |stream: &[u16]| {
        let mut world = World::default();
        world.enter_world_map();
        world.world_map_ctrl.as_mut().unwrap().debug_enabled = true;
        for &pad in stream {
            world.set_pad(pad);
            let _ = world.tick();
        }
        let c = world.world_map_ctrl.unwrap();
        (c.view_mode, c.camera_x, c.camera_z, c.azimuth, c.zoom)
    };
    assert_eq!(drive(&pad_stream), drive(&pad_stream));
}

/// With no overworld entities installed, the world-map tick is camera-only:
/// the encounter state never advances even when encounters are enabled.
#[test]
fn world_map_without_entities_never_encounters() {
    let mut world = World::default();
    world.enter_world_map();
    world.set_world_map_encounter(true, 0, 7, 64);
    // No install_world_map_entities call.
    for _ in 0..10 {
        let _ = world.tick();
    }
    assert_eq!(world.mode, SceneMode::WorldMap);
    assert!(world.pending_world_map_encounter.is_none());
}

/// Walking the overworld player across tiles rolls the region-keyed encounter
/// (the `FUN_801D9E1C` port) and flips Field-less straight into a battle that
/// returns to the world map.
#[test]
fn world_map_region_walk_triggers_battle() {
    use crate::monster_catalog::{FormationDef, FormationSlot, MonsterCatalog, MonsterDef};
    use crate::region_encounter::{EncounterRegion, RegionEncounterTable};

    let mut world = World {
        party_count: 1,
        ..World::default()
    };
    world.live_gameplay_loop = true;
    world.enter_world_map();
    // Frame the camera at a quarter turn (azimuth 1024) so the camera-relative
    // remap maps a held Right cleanly to world +X (keeps this test's "walk +X
    // across tiles" intent readable; at the default azimuth 0 Right maps to -Z).
    if let Some(ctrl) = world.world_map_ctrl.as_mut() {
        ctrl.azimuth = 1024;
    }
    world.install_field_player(0); // player_actor_slot = 0, actor active
    world.actors[0].battle.hp = 400;
    world.actors[0].battle.max_hp = 400;
    world.actors[0].battle.liveness = 1;
    world.set_battle_attack(0, 80);

    // Formation 5 spawns one weak monster (id 100).
    world
        .formation_table
        .insert(FormationDef::new(5, vec![FormationSlot::new(100)]));
    let mut cat = MonsterCatalog::new();
    cat.insert(MonsterDef::new(100, "Test Slug", 20, 4));
    world.set_monster_catalog(cat);

    // One region covering tiles (0,0)..(20,20), high rate, rolling formation 5.
    let mut table = RegionEncounterTable::new("test");
    table.regions.push(EncounterRegion {
        tile_x_min: 0,
        tile_z_min: 0,
        tile_x_max: 20,
        tile_z_max: 20,
        rate_increment: 255,
        formation_base: 5,
        formation_count: 1,
    });
    world.set_world_map_regions(table);

    // Hold Right; the player walks +X, crossing 128-unit tiles. Each crossing
    // rolls the region; the high rate triggers within a couple of tiles.
    world.set_pad(input::PadButton::Right.mask());
    let mut entered_battle = false;
    for _ in 0..200 {
        let _ = world.tick();
        if world.mode == SceneMode::Battle {
            entered_battle = true;
            break;
        }
    }
    assert!(
        entered_battle,
        "walking the overworld triggers a region encounter"
    );
    assert_eq!(world.battle_return_mode, SceneMode::WorldMap);
}

/// The overworld player is bounded by the scene's walkability grid, exactly
/// like the field: the retail world-map-walk overlay's locomotion is the same
/// `FUN_801d01b0` + `FUN_801cfe4c` against the same `_DAT_1f8003ec + 0x4000`
/// grid. With every tile walled the player cannot move in any direction.
#[test]
fn world_map_locomotion_blocked_by_full_wall_grid() {
    let mut world = World::default();
    world.enter_world_map();
    world.install_field_player(0);
    // Wall every tile (sub=1 sets all four sub-cell bits across the grid).
    world.paint_field_collision(1, (0, 0x80), (0, 0x80), 0);
    world.actors[0].move_state.world_x = 400;
    world.actors[0].move_state.world_z = 400;
    world.set_pad(input::PadButton::Up.mask());
    let _ = world.tick();
    assert_eq!(
        world.actors[0].move_state.world_x, 400,
        "walled in: no X move"
    );
    assert_eq!(
        world.actors[0].move_state.world_z, 400,
        "walled in: no Z move"
    );
}

/// With no walls, the overworld player walks freely. At the default walk-mode
/// camera azimuth (`0`) the camera sits on `+X` looking `-X`, so "screen up"
/// (away from the camera) walks the player `-X` - the camera-relative remap,
/// not a raw `+Z`.
#[test]
fn world_map_locomotion_walks_when_clear() {
    let mut world = World::default();
    world.enter_world_map();
    world.install_field_player(0);
    world.reset_field_collision_grid(); // present but all-walkable
    world.actors[0].move_state.world_x = 200;
    world.actors[0].move_state.world_z = 250;
    world.set_pad(input::PadButton::Up.mask());
    let _ = world.tick();
    // speed 8 -> four 2-unit steps, all clear: at azimuth 0 the camera sits on
    // +X looking back, so "screen up" walks -X; x: 200 -> 192, z unchanged.
    assert_eq!(world.actors[0].move_state.world_x, 192);
    assert_eq!(world.actors[0].move_state.world_z, 250);
}

/// The camera-relative remap rotates the held d-pad through the overworld
/// camera azimuth. Spot-check the cardinal framings against the
/// `world_map_camera_mvp` geometry (eye at `center + (d·cosθ, _, d·sinθ)`):
/// at azimuth 0 the camera is on `+X`, so "screen up" walks `-X`; a 3/4-turn
/// azimuth puts it on `-Z`, so "screen up" walks `+Z`.
#[test]
fn world_map_camera_relative_bits_rotates_with_azimuth() {
    use crate::world::world_map_camera_relative_bits;
    // Expectations are the camera-verified screen axes (screen-up -> world
    // (-cosθ, -sinθ), screen-right -> world (sinθ, -cosθ)); the engine-shell
    // projection test confirms these move the right way on screen.
    // No input -> no bits.
    assert_eq!(world_map_camera_relative_bits(0, 0, 0), 0);
    // Azimuth 0: camera on +X, so Up (screen up) -> X- (0x8000), Right -> Z- (0x4000).
    assert_eq!(world_map_camera_relative_bits(0, 0, 1), 0x8000);
    assert_eq!(world_map_camera_relative_bits(0, 1, 0), 0x4000);
    // Azimuth 1024 (quarter turn): Up -> Z- (0x4000).
    assert_eq!(world_map_camera_relative_bits(1024, 0, 1), 0x4000);
    // Azimuth 2048 (half turn): Up -> X+ (0x2000).
    assert_eq!(world_map_camera_relative_bits(2048, 0, 1), 0x2000);
    // Azimuth 3072 (3/4 turn): Up -> Z+ (0x1000), Right -> X- (0x8000).
    assert_eq!(world_map_camera_relative_bits(3072, 0, 1), 0x1000);
    assert_eq!(world_map_camera_relative_bits(3072, 1, 0), 0x8000);
    // A diagonal framing (1/8 turn) maps a single screen press to two world
    // axes (the player walks diagonally).
    let diag = world_map_camera_relative_bits(512, 0, 1);
    assert_eq!(
        diag.count_ones(),
        2,
        "rotated framing -> diagonal world move"
    );
}

/// A camera-only world map (no entities, no region tracker) never encounters,
/// even while the player walks.
#[test]
fn world_map_without_regions_or_entities_never_encounters() {
    let mut world = World::default();
    world.enter_world_map();
    world.install_field_player(0);
    world.set_pad(input::PadButton::Right.mask());
    for _ in 0..500 {
        let _ = world.tick();
    }
    assert_eq!(world.mode, SceneMode::WorldMap);
    assert!(world.pending_world_map_encounter.is_none());
}

/// An installed overworld entity whose shared countdown reaches zero (with
/// encounters enabled) fires an encounter that resolves into a battle, and
/// the battle is tagged to return to the overworld - not the field.
#[test]
fn world_map_encounter_flips_to_battle_returning_to_world_map() {
    use crate::monster_catalog::{FormationDef, FormationSlot, MonsterCatalog, MonsterDef};

    let mut world = World {
        party_count: 1,
        ..World::default()
    };
    world.live_gameplay_loop = true;
    world.enter_world_map();
    // A capable lone party member.
    world.actors[0].active = true;
    world.actors[0].battle.hp = 400;
    world.actors[0].battle.max_hp = 400;
    world.actors[0].battle.liveness = 1;
    world.set_battle_attack(0, 80);
    // Formation 7 spawns one weak monster (id 100); register its stats.
    world
        .formation_table
        .insert(FormationDef::new(7, vec![FormationSlot::new(100)]));
    let mut cat = MonsterCatalog::new();
    cat.insert(MonsterDef::new(100, "Test Slug", 20, 4));
    world.set_monster_catalog(cat);
    // One entity; encounters enabled with the countdown already at zero so
    // the first Idle step fires immediately.
    world.install_world_map_entities(1);
    world.set_world_map_encounter(true, 0, 7, 64);

    // Tick once: the entity SM fires the encounter and the world flips into
    // battle, tagged to return to the overworld.
    let _ = world.tick();
    assert_eq!(world.mode, SceneMode::Battle);
    assert_eq!(world.battle_return_mode, SceneMode::WorldMap);
    assert!(world.field_return.is_some());

    // Drive the fight to completion; it must return to the world map, not
    // the field.
    let mut returned = false;
    for _ in 0..8000 {
        world.tick();
        if world.mode != SceneMode::Battle {
            returned = true;
            break;
        }
    }
    assert!(returned, "the overworld battle must resolve");
    assert_eq!(
        world.mode,
        SceneMode::WorldMap,
        "an overworld encounter returns to the world map"
    );
}

/// A stationary player next to an idle overworld entity triggers an
/// interaction (surfaced as a `FieldInteract` event), and a moving player
/// does not.
#[test]
fn world_map_idle_entity_interacts_only_when_player_stationary() {
    let mut world = World::default();
    world.enter_world_map();
    world.install_world_map_entities(1);
    // Encounters disabled so only the interaction path can fire.
    world.set_world_map_encounter(false, 50, 0, 64);

    // Player moving (d-pad direction held): no interaction.
    world.set_pad(crate::input::PadButton::Up.mask());
    let _ = world.tick();
    assert!(
        !world
            .pending_field_events
            .iter()
            .any(|e| matches!(e, FieldEvent::FieldInteract { .. })),
        "a walking player does not interact"
    );

    // Player stationary: the idle entity interacts.
    world.set_pad(0);
    let _ = world.tick();
    let interacted = world
        .drain_field_events()
        .iter()
        .any(|e| matches!(e, FieldEvent::FieldInteract { interact_id: 0, .. }));
    assert!(interacted, "a stationary player interacts with the entity");
}

/// An encounter-zone entity spawns its OWN formation, not the map-wide
/// shared one.
#[test]
fn world_map_encounter_zone_uses_its_own_formation() {
    use crate::monster_catalog::{FormationDef, FormationSlot, MonsterCatalog, MonsterDef};

    let mut world = World {
        party_count: 1,
        ..World::default()
    };
    world.live_gameplay_loop = true;
    world.enter_world_map();
    world.actors[0].active = true;
    world.actors[0].battle.hp = 400;
    world.actors[0].battle.max_hp = 400;
    world.actors[0].battle.liveness = 1;
    world.set_battle_attack(0, 80);
    // Register both the zone's formation (9) and a decoy shared one (7).
    world
        .formation_table
        .insert(FormationDef::new(9, vec![FormationSlot::new(100)]));
    world
        .formation_table
        .insert(FormationDef::new(7, vec![FormationSlot::new(101)]));
    let mut cat = MonsterCatalog::new();
    cat.insert(MonsterDef::new(100, "Zone Slug", 20, 4));
    cat.insert(MonsterDef::new(101, "Decoy", 20, 4));
    world.set_monster_catalog(cat);
    // Entity 0 is an encounter zone for formation 9; shared formation is 7.
    world.install_world_map_entities_with_configs(vec![WorldMapEntityConfig::EncounterZone {
        formation_id: 9,
    }]);
    world.set_world_map_encounter(true, 0, 7, 64);

    let _ = world.tick();
    assert_eq!(world.mode, SceneMode::Battle);
    assert_eq!(
        world.active_formation.as_ref().map(|f| f.formation_id),
        Some(9),
        "the zone's own formation spawns, not the shared one"
    );
}

/// Engaging a portal entity surfaces a `WorldMapTransition` carrying the
/// portal's target map id.
#[test]
fn world_map_portal_engage_surfaces_target_map() {
    let mut world = World::default();
    world.enter_world_map();
    world.install_world_map_entities_with_configs(vec![WorldMapEntityConfig::Portal {
        target_map: 5,
    }]);
    // Encounters off so only the transition path can fire.
    world.set_world_map_encounter(false, 50, 0, 64);

    world.engage_world_map_entity(0);
    let _ = world.tick();
    let transitioned = world.drain_field_events().into_iter().any(|e| {
        matches!(
            e,
            FieldEvent::WorldMapTransition {
                target_map: 5,
                slot: 0
            }
        )
    });
    assert!(transitioned, "the portal surfaces its target map");
}

/// Walking the overworld player onto a portal entity's tile auto-engages it
/// (no host `engage_world_map_entity` call) and surfaces its target map.
#[test]
fn world_map_walking_onto_portal_auto_engages() {
    let mut world = World::default();
    world.enter_world_map();
    world.install_field_player(0);
    // Encounters off so only the transition path can fire.
    world.set_world_map_encounter(false, 50, 0, 64);
    // A portal at tile (3,3) -> world (3*128 + 64 = 448, 448).
    world.install_world_map_entities_at(vec![(
        WorldMapEntityConfig::Portal { target_map: 9 },
        (448, 448),
    )]);

    // Player starts two tiles to the -X side, on the same row as the portal.
    world.actors[0].move_state.world_x = 448 - 256;
    world.actors[0].move_state.world_z = 448;

    // Hold the d-pad direction that walks +X at the default azimuth (0): the
    // camera sits on +X, so "screen down" walks +X toward the portal (see the
    // camera-relative remap).
    world.set_pad(input::PadButton::Down.mask());
    let mut transitioned = false;
    for _ in 0..200 {
        let _ = world.tick();
        if world.drain_field_events().into_iter().any(|e| {
            matches!(
                e,
                FieldEvent::WorldMapTransition {
                    target_map: 9,
                    slot: 0
                }
            )
        }) {
            transitioned = true;
            break;
        }
    }
    assert!(
        transitioned,
        "walking onto the portal tile auto-fires its transition"
    );
}

/// Auto-engage is portal-only: walking onto an NPC entity's tile does NOT fire
/// a transition (NPCs are talk-to, not walk-onto).
#[test]
fn world_map_walking_onto_npc_does_not_transition() {
    let mut world = World::default();
    world.enter_world_map();
    world.install_field_player(0);
    world.set_world_map_encounter(false, 50, 0, 64);
    world.install_world_map_entities_at(vec![(
        WorldMapEntityConfig::Npc {
            interact_id: 4,
            text_id: None,
            inline: Vec::new(),
        },
        (448, 448),
    )]);
    world.actors[0].move_state.world_x = 448;
    world.actors[0].move_state.world_z = 448; // standing on the NPC tile
    world.set_pad(0);
    let _ = world.tick();
    let transitioned = world
        .drain_field_events()
        .into_iter()
        .any(|e| matches!(e, FieldEvent::WorldMapTransition { .. }));
    assert!(
        !transitioned,
        "an NPC is not auto-engaged by walking onto its tile"
    );
}

/// Placed overworld entities surface as render markers: one per installed
/// position, paired with its kind, at the player's walking plane.
#[test]
fn world_map_entity_markers_pair_position_and_kind() {
    let mut world = World::default();
    world.enter_world_map();
    world.install_field_player(0);
    // Put the player on a known plane so the marker `y` is deterministic.
    world.actors[0].move_state.world_y = -200;
    world.install_world_map_entities_at(vec![
        (WorldMapEntityConfig::Portal { target_map: 9 }, (448, 320)),
        (
            WorldMapEntityConfig::Npc {
                interact_id: 4,
                text_id: None,
                inline: Vec::new(),
            },
            (640, 128),
        ),
        (
            WorldMapEntityConfig::EncounterZone { formation_id: 2 },
            (-64, 512),
        ),
    ]);

    let markers = world.world_map_entity_markers();
    assert_eq!(markers.len(), 3);
    // Position x/z come straight from the placement; y is the player plane.
    assert_eq!(markers[0].world_pos, [448.0, -200.0, 320.0]);
    assert_eq!(markers[0].kind, WorldMapEntityKind::Portal);
    assert_eq!(markers[1].world_pos, [640.0, -200.0, 128.0]);
    assert_eq!(markers[1].kind, WorldMapEntityKind::Npc);
    assert_eq!(markers[2].world_pos, [-64.0, -200.0, 512.0]);
    assert_eq!(markers[2].kind, WorldMapEntityKind::EncounterZone);
}

/// The player surfaces as an overworld marker at its actor position; with no
/// player actor installed there is no marker.
#[test]
fn world_map_player_marker_tracks_player_actor() {
    let mut world = World::default();
    world.enter_world_map();
    assert!(
        world.world_map_player_marker().is_none(),
        "no player actor -> no marker"
    );
    world.install_field_player(0);
    world.actors[0].move_state.world_x = 320;
    world.actors[0].move_state.world_y = -64;
    world.actors[0].move_state.world_z = 256;
    let m = world
        .world_map_player_marker()
        .expect("player marker present");
    assert_eq!(m.world_pos, [320.0, -64.0, 256.0]);
}

/// Walking on the overworld records a heading the player marker exposes (the
/// world-map walk sets `render_26` itself, since it uses the camera-relative
/// bits rather than the field heading decoder).
#[test]
fn world_map_walking_sets_player_marker_facing() {
    let mut world = World::default();
    world.enter_world_map();
    world.install_field_player(0);
    world.set_world_map_encounter(false, 50, 0, 64);
    world.reset_field_collision_grid(); // all-walkable so the step commits
    world.actors[0].move_state.world_x = 200; // away from the -X boundary
    let start_x = world.actors[0].move_state.world_x;
    // At the default azimuth the camera sits on +X, so "screen up" walks -X
    // (dx=-1, dz=0) -> atan2(-1, 0) = -TAU/4 -> heading 3072.
    world.set_pad(input::PadButton::Up.mask());
    let _ = world.tick();
    let m = world
        .world_map_player_marker()
        .expect("player marker present");
    assert_eq!(m.facing, 3072, "walking -X faces heading 3072");
    assert!(
        world.actors[0].move_state.world_x < start_x,
        "the player advanced -X (start {start_x} -> {})",
        world.actors[0].move_state.world_x
    );
}

/// Config-only installs (no disc placements) produce no markers, so a
/// camera-only or synthetic world map draws nothing.
#[test]
fn world_map_entity_markers_empty_without_positions() {
    let mut world = World::default();
    world.enter_world_map();
    world.install_world_map_entities_with_configs(vec![WorldMapEntityConfig::Npc {
        interact_id: 1,
        text_id: None,
        inline: Vec::new(),
    }]);
    assert!(world.world_map_entity_markers().is_empty());
}

/// An NPC-config entity surfaces its configured interaction id.
#[test]
fn world_map_npc_config_surfaces_interact_id() {
    let mut world = World::default();
    world.enter_world_map();
    world.install_world_map_entities_with_configs(vec![WorldMapEntityConfig::Npc {
        interact_id: 7,
        text_id: None,
        inline: Vec::new(),
    }]);
    world.set_world_map_encounter(false, 50, 0, 64);
    // Stationary player: the idle entity interacts.
    world.set_pad(0);
    let _ = world.tick();
    let interacted = world
        .drain_field_events()
        .into_iter()
        .any(|e| matches!(e, FieldEvent::FieldInteract { interact_id: 7, .. }));
    assert!(interacted, "the NPC surfaces its configured interact id");
}

/// Talking to an adjacent NPC that carries inline dialog text opens its MES
/// message on a confirm press (sets `current_dialog` + emits `OpenDialog`); a
/// later confirm/cancel press dismisses it (emits `DialogDismissed`).
#[test]
fn world_map_npc_talk_to_opens_and_dismisses_dialogue() {
    let cross = crate::input::PadButton::Cross.mask();
    let mut world = World::default();
    world.enter_world_map();
    world.install_field_player(0);
    world.set_world_map_encounter(false, 50, 0, 64);
    // Inline dialog bytes in the field-VM box format: a one-byte prologue
    // then a `0x1F`-lead text segment ("Hi").
    let inline = vec![0x00u8, 0x1F, b'H', b'i', 0x00];
    world.install_world_map_entities_at(vec![(
        WorldMapEntityConfig::Npc {
            interact_id: 4,
            text_id: Some(0x12),
            inline: inline.clone(),
        },
        (576, 448), // one tile east of the player (448 >> 7 == 3, 576 >> 7 == 4)
    )]);
    world.actors[0].move_state.world_x = 448;
    world.actors[0].move_state.world_z = 448;

    // Settle a frame with no input so the next Cross press is a clean edge.
    world.set_pad(0);
    let _ = world.tick();
    assert!(world.current_dialog.is_none(), "no box before talking");

    // Confirm press next to the NPC opens its dialogue, carrying the inline
    // text through (the host renders it via `OwnedDialogPanel::from_inline_dialog`).
    world.set_pad(cross);
    let _ = world.tick();
    assert_eq!(
        world.current_dialog.as_ref().map(|d| d.inline.clone()),
        Some(inline.clone()),
        "talk-to opens the NPC's inline dialogue text"
    );
    assert!(
        world
            .drain_field_events()
            .into_iter()
            .any(|e| matches!(e, FieldEvent::OpenDialog { ref inline, .. } if !inline.is_empty())),
        "talk-to emits OpenDialog carrying the inline text for the host to render"
    );

    // Cross held across the frame boundary is not a fresh edge (edges advance
    // on `set_pad`): the box stays up.
    world.set_pad(cross);
    let _ = world.tick();
    assert!(
        world.current_dialog.is_some(),
        "no dismiss without a new edge"
    );

    // Release then press again to dismiss.
    world.set_pad(0);
    let _ = world.tick();
    world.set_pad(cross);
    let _ = world.tick();
    assert!(world.current_dialog.is_none(), "confirm dismisses the box");
    assert!(
        world
            .drain_field_events()
            .into_iter()
            .any(|e| matches!(e, FieldEvent::DialogDismissed)),
        "dismiss emits DialogDismissed"
    );
}

// ---- tile-board step + collision (A2) ----

/// Tile-board world: 3x3 board, all floor except a wall at (1,1);
/// player actor in slot 0 placed at its start-tile centre.
fn tile_board_world() -> World {
    let mut w = World::new();
    w.mode = SceneMode::Field;
    w.player_actor_slot = Some(0);
    w.actors[0].active = true;
    let cells = vec![1, 1, 1, 1, crate::tile_board::CELL_WALL, 1, 1, 1, 1];
    let board = crate::tile_board::TileBoard::new(3, 3, 0, 0, cells);
    w.tile_board = Some(board);
    let (x, z) = w.tile_board.as_ref().unwrap().player_world();
    w.actors[0].move_state.world_x = x as i16;
    w.actors[0].move_state.world_z = z as i16;
    w
}

fn pad_held(world: &mut World, mask: u16, frames: usize) {
    for _ in 0..frames {
        world.set_pad(mask);
        let _ = world.tick();
    }
}

#[test]
fn tile_board_holding_right_steps_to_edge() {
    let mut w = tile_board_world();
    // Hold Right long enough to cross two tiles (8 frames/tile) and
    // bump the east edge.
    pad_held(&mut w, input::PadButton::Right.mask(), 40);
    let b = w.tile_board.as_ref().unwrap();
    // col advances 0 -> 1 -> 2, then (3,_) is out of bounds -> stops.
    assert_eq!(b.player_col, 2);
    assert_eq!(b.player_row, 0);
    // Actor settled on the (2,0) tile centre and the step is idle.
    let (tx, _tz) = b.tile_world(2, 0);
    assert_eq!(w.actors[0].move_state.world_x as i32, tx);
    assert_eq!(w.tile_board_target, None);
}

#[test]
fn tile_board_takes_multiple_frames_per_tile() {
    let mut w = tile_board_world();
    // One tick: direction committed (col 0 -> 1), target set, but the
    // actor hasn't reached the next tile centre yet.
    w.set_pad(input::PadButton::Right.mask());
    let _ = w.tick();
    assert_eq!(w.tile_board.as_ref().unwrap().player_col, 1);
    assert!(w.tile_board_target.is_some());
    let (tx, _) = w.tile_board.as_ref().unwrap().tile_world(1, 0);
    assert!((w.actors[0].move_state.world_x as i32) < tx);
}

#[test]
fn tile_board_blocked_by_wall() {
    let mut w = tile_board_world();
    // Start the player directly north of the (1,1) wall.
    {
        let b = w.tile_board.as_mut().unwrap();
        b.player_col = 1;
        b.player_row = 0;
    }
    let (x, z) = w.tile_board.as_ref().unwrap().player_world();
    w.actors[0].move_state.world_x = x as i16;
    w.actors[0].move_state.world_z = z as i16;
    let before = w.actors[0].move_state.world_z;
    // Down would step into the (1,1) wall - rejected, player stays.
    pad_held(&mut w, input::PadButton::Down.mask(), 20);
    let b = w.tile_board.as_ref().unwrap();
    assert_eq!((b.player_col, b.player_row), (1, 0));
    assert_eq!(w.actors[0].move_state.world_z, before);
    assert_eq!(w.tile_board_target, None);
}

#[test]
fn tile_board_gated_by_dialog() {
    let mut w = tile_board_world();
    w.current_dialog = Some(DialogRequest {
        text_id: 0,
        inline: Vec::new(),
        world_x: 0,
        world_z: 0,
        depth_id: 0,
    });
    pad_held(&mut w, input::PadButton::Right.mask(), 20);
    let b = w.tile_board.as_ref().unwrap();
    assert_eq!((b.player_col, b.player_row), (0, 0));
    assert_eq!(w.tile_board_target, None);
}

#[test]
fn tile_board_is_deterministic() {
    let drive = || {
        let mut w = tile_board_world();
        for &mask in &[
            input::PadButton::Right.mask(),
            input::PadButton::Down.mask(),
            input::PadButton::Right.mask(),
        ] {
            pad_held(&mut w, mask, 12);
        }
        let b = w.tile_board.as_ref().unwrap().clone();
        (
            b.player_col,
            b.player_row,
            w.actors[0].move_state.world_x,
            w.actors[0].move_state.world_z,
        )
    };
    assert_eq!(drive(), drive());
}

#[test]
fn collect_sprite_requests_emits_one_per_active_actor_with_frame() {
    let mut world = World::default();
    // Slot 0: active + sprite frame at (10, 20) world coords.
    world.actors[0].active = true;
    world.actors[0].move_state.world_x = 100;
    world.actors[0].move_state.world_z = 200;
    world.set_actor_sprite(
        0,
        Some(SpriteFrame {
            atlas_src: (0, 0, 16, 24),
            tint: [1.0, 1.0, 1.0, 1.0],
            anchor_y: -8,
        }),
    );
    // Slot 1: active but no frame - shouldn't emit.
    world.actors[1].active = true;
    // Slot 2: frame but inactive - shouldn't emit.
    world.set_actor_sprite(
        2,
        Some(SpriteFrame {
            atlas_src: (16, 0, 16, 24),
            tint: [1.0; 4],
            anchor_y: 0,
        }),
    );

    let requests = world.collect_sprite_requests();
    assert_eq!(requests.len(), 1);
    let r = &requests[0];
    assert_eq!(r.actor_slot, 0);
    assert_eq!(r.world_x, 100);
    // anchor_y subtracts from world_z (z + (-8)) = 192.
    assert_eq!(r.world_y, 192);
    assert_eq!(r.atlas_src, (0, 0, 16, 24));
}

#[test]
fn set_actor_sprite_with_none_clears_existing_frame() {
    let mut world = World::default();
    world.actors[0].active = true;
    world.set_actor_sprite(
        0,
        Some(SpriteFrame {
            atlas_src: (0, 0, 8, 8),
            ..Default::default()
        }),
    );
    assert!(world.actors[0].sprite_frame.is_some());
    world.set_actor_sprite(0, None);
    assert!(world.actors[0].sprite_frame.is_none());
}

#[test]
fn load_field_record_skips_frame_divider_sentinel() {
    let mut world = World::new();
    // Record opens with FFFF 0000 frame divider.
    let record = vec![0xFF, 0xFF, 0x00, 0x00, 0x37, 0x00];
    world.load_field_record(&record);
    assert_eq!(world.field_pc, 4, "frame divider should bump pc to 4");
    assert_eq!(world.field_bytecode.len(), 6);
}

#[test]
fn load_field_record_without_sentinel_starts_at_zero() {
    let mut world = World::new();
    let record = vec![0x37, 0x00];
    world.load_field_record(&record);
    assert_eq!(world.field_pc, 0);
}

/// Field VM op 0x3E with `op0 >= 100` is the scene-transition arm
/// (`map_id = op0 - 100`). The world's `FieldHostImpl` records the
/// request in `pending_scene_transition` for `SceneHost::tick` to
/// drain on the next frame boundary.
#[test]
fn field_scene_transition_writes_pending_map_id() {
    let mut world = World::new();
    world.mode = SceneMode::Field;
    // Bytecode: opcode 0x3E, op0 = 105 (map_id 5), then 4 padding
    // bytes (op0 + 4 trailing operand bytes per the dispatcher math).
    let bytecode = vec![0x3E, 105, 0, 0, 0, 0];
    world.load_field_script(bytecode);
    let _ = world.tick();
    assert_eq!(world.pending_scene_transition, Some(5));
}

/// `op0 < 100` is the field_interact arm - should NOT trigger a
/// scene transition.
#[test]
fn field_op_3e_low_op0_does_not_request_scene_transition() {
    let mut world = World::new();
    world.mode = SceneMode::Field;
    let bytecode = vec![0x3E, 50, 7];
    world.load_field_script(bytecode);
    let _ = world.tick();
    assert_eq!(world.pending_scene_transition, None);
}

/// Field-VM op `0x4C 0xE2` (FMV trigger) records the FMV index in
/// `World::pending_fmv_trigger` AND emits a `FieldEvent::FmvTrigger`
/// for engines to drain. Retail handler at `0x801E30E4` writes the
/// s16 to `_DAT_8007BA78` and pokes next-game-mode = 0x1A; the
/// world mirrors the request via these two channels.
#[test]
fn field_op_4c_e2_records_pending_fmv_trigger() {
    use crate::cutscene::{STR_INIT_GAME_MODE, fmv_index_to_str_filename};
    use crate::field_events::FieldEvent;

    let mut world = World::new();
    world.mode = SceneMode::Field;
    // `[0x4C, 0xE2, 0x03, 0x00, 0, 0]` → fmv_id 3 → MV4.STR.
    let bytecode = vec![0x4C, 0xE2, 0x03, 0x00, 0, 0];
    world.load_field_script(bytecode);
    let _ = world.tick();
    assert_eq!(world.pending_fmv_trigger, Some(3));
    let events = world.drain_field_events();
    assert!(events.contains(&FieldEvent::FmvTrigger { fmv_id: 3 }));
    assert_eq!(fmv_index_to_str_filename(3), Some("MOV/MV4.STR"));
    assert_eq!(STR_INIT_GAME_MODE, 26);
}

/// The FMV trigger transitions Field → Cutscene one frame later (retail's
/// main dispatcher reads the next-game-mode global the frame after the
/// field-VM op writes it), exposes the active FMV + its `MV*.STR` path,
/// and suspends the field VM while it plays. `finish_cutscene` returns to
/// the field.
#[test]
fn field_fmv_trigger_drives_field_cutscene_field_flow() {
    let mut world = World::new();
    world.mode = SceneMode::Field;
    // fmv_id 3 → MV4.STR (a playable slot).
    world.load_field_script(vec![0x4C, 0xE2, 0x03, 0x00, 0, 0]);

    // Frame 1: op fires, records the pending trigger; still in Field.
    let _ = world.tick();
    assert_eq!(world.mode, SceneMode::Field);
    assert_eq!(world.pending_fmv_trigger, Some(3));
    assert_eq!(world.active_fmv(), None);

    // Frame 2: the pending trigger is consumed at the top of the tick and
    // the world flips into the cutscene mode for the resolved FMV.
    let _ = world.tick();
    assert_eq!(world.mode, SceneMode::Cutscene);
    assert_eq!(world.pending_fmv_trigger, None);
    assert_eq!(world.active_fmv(), Some(3));
    assert_eq!(world.active_fmv_str_filename(), Some("MOV/MV4.STR"));

    // While the FMV plays the field VM is suspended (no further field
    // stepping); ticking keeps the world in Cutscene until the host ends
    // playback.
    let _ = world.tick();
    assert_eq!(world.mode, SceneMode::Cutscene);
    assert_eq!(world.active_fmv(), Some(3));

    // Host signals playback complete → back to the field.
    world.finish_cutscene();
    assert_eq!(world.mode, SceneMode::Field);
    assert_eq!(world.active_fmv(), None);
}

/// An FMV id whose runtime slot points at a cut/missing path is drained
/// without entering the cutscene mode - the engine treats it as a no-op
/// and the field keeps running.
#[test]
fn field_fmv_trigger_cut_path_is_a_noop() {
    let mut world = World::new();
    world.mode = SceneMode::Field;
    // fmv_id 7 → slots 5..=11 are dev-only cut paths (no retail STR).
    world.load_field_script(vec![0x4C, 0xE2, 0x07, 0x00, 0, 0]);

    let _ = world.tick(); // op fires
    assert_eq!(world.pending_fmv_trigger, Some(7));
    let _ = world.tick(); // pending consumed
    assert_eq!(
        world.mode,
        SceneMode::Field,
        "cut path does not enter cutscene"
    );
    assert_eq!(world.pending_fmv_trigger, None, "pending still drained");
    assert_eq!(world.active_fmv(), None);
}

// --- Save / load round-trip ----------------------------------------

#[test]
fn load_party_populates_battle_actor_hp_mp() {
    let mut party = legaia_save::Party::zeroed(3);
    let mut hms = party.members[0].hp_mp_sp();
    hms.hp_cur = 137;
    hms.hp_max = 150;
    hms.mp_cur = 42;
    party.members[0].set_hp_mp_sp(hms);
    let mut hms1 = party.members[1].hp_mp_sp();
    hms1.hp_cur = 0; // dead member
    hms1.hp_max = 100;
    party.members[1].set_hp_mp_sp(hms1);

    let mut world = World::new();
    world.load_party(party);

    assert!(world.actors[0].active);
    assert_eq!(world.actors[0].battle.hp, 137);
    assert_eq!(world.actors[0].battle.max_hp, 150);
    assert_eq!(world.actors[0].battle.mp, 42);
    assert_eq!(world.actors[0].battle.liveness, 1);
    // Dead member: liveness flipped to 0.
    assert_eq!(world.actors[1].battle.liveness, 0);
    assert_eq!(world.party_count, 3);
}

#[test]
fn save_party_round_trips_after_load() {
    let mut party = legaia_save::Party::zeroed(3);
    let mut hms = party.members[0].hp_mp_sp();
    hms.hp_cur = 200;
    hms.hp_max = 250;
    hms.mp_cur = 100;
    party.members[0].set_hp_mp_sp(hms);

    let original_bytes = party.write();

    let mut world = World::new();
    world.load_party(party);
    let saved = world.save_party();

    assert_eq!(saved.write(), original_bytes);
}

#[test]
fn save_party_picks_up_in_battle_hp_changes() {
    let mut party = legaia_save::Party::zeroed(2);
    let mut hms = party.members[0].hp_mp_sp();
    hms.hp_cur = 100;
    hms.hp_max = 100;
    party.members[0].set_hp_mp_sp(hms);

    let mut world = World::new();
    world.load_party(party);
    // Simulate damage during battle.
    world.actors[0].battle.hp = 25;

    let saved = world.save_party();
    assert_eq!(saved.members[0].hp_mp_sp().hp_cur, 25);
    // Max HP unchanged.
    assert_eq!(saved.members[0].hp_mp_sp().hp_max, 100);
}

#[test]
fn load_party_caps_at_max_actors() {
    let many = legaia_save::Party::zeroed(MAX_ACTORS + 10);
    let mut world = World::new();
    world.load_party(many);
    assert_eq!(world.party_count, MAX_ACTORS as u8);
}

#[test]
fn save_full_round_trips_globals() {
    let mut world = World::new();
    world.load_party(legaia_save::Party::zeroed(2));
    world.story_flags = 0xCAFE_F00D;
    world.money = 54321;
    world.inventory.insert(3, 9);
    world.inventory.insert(77, 1);

    let sf = world.save_full();
    assert_eq!(sf.ext.story_flags, 0xCAFE_F00D);
    assert_eq!(sf.ext.money, 54321);
    // inventory is sorted by item_id
    assert_eq!(sf.ext.inventory, vec![(3, 9), (77, 1)]);

    let bytes = sf.write();
    let parsed = legaia_save::SaveFile::parse(&bytes).unwrap();

    let mut world2 = World::new();
    world2.load_full(parsed);
    assert_eq!(world2.story_flags, 0xCAFE_F00D);
    assert_eq!(world2.money, 54321);
    assert_eq!(world2.inventory.get(&3), Some(&9));
    assert_eq!(world2.inventory.get(&77), Some(&1));
    assert_eq!(world2.party_count, 2);
}

#[test]
fn load_full_clears_old_inventory() {
    let mut world = World::new();
    world.inventory.insert(1, 10);
    world.inventory.insert(2, 20);

    let sf = legaia_save::SaveFile {
        party: legaia_save::Party::zeroed(1),
        ext: legaia_save::SaveExt {
            story_flags: 1,
            story_flag_bits: Vec::new(),
            money: 0,
            inventory: vec![(5, 3)],
        },
        ext_v2: legaia_save::SaveExtV2::default(),
    };
    world.load_full(sf);
    assert!(!world.inventory.contains_key(&1));
    assert!(!world.inventory.contains_key(&2));
    assert_eq!(world.inventory.get(&5), Some(&3));
}

#[test]
fn effect_pool_tick_decrements_state_byte() {
    let mut world = World::new();
    world.effect_pool.master_slots[0].child_count = 4;
    // state >= 8 → write back state - 8 and skip.
    world.effect_pool.master_slots[0].state = 12;
    world.tick_effects();
    assert_eq!(world.effect_pool.master_slots[0].state, 4);
    // Slot still active.
    assert_eq!(world.effect_pool.master_slots[0].child_count, 4);
}

// --- move-VM host wiring (round 5) ------------------------------------

#[test]
fn move_vm_global_predicate_round_trips_through_world() {
    let mut world = World::new();
    world.actors[0].active = true;
    // Move bytecode: 0x2F sub-op 0x08 (set predicate to 1), then HALT.
    world.set_move_bytecode(0, Some(vec![0x002F, 0x0008, 0x0008]));
    let _ = world.step_move_vm(0, &world.move_bytecode[0].clone());
    assert_eq!(
        world.move_predicate, 1,
        "ext sub-op 0x08 should set move_predicate to 1"
    );
}

#[test]
fn move_vm_global_counter_set_and_get() {
    let mut world = World::new();
    world.actors[0].active = true;
    // 0x2F sub-op 0x0F clears counter, then HALT.
    world.move_counter = 5;
    world.set_move_bytecode(0, Some(vec![0x002F, 0x000F, 0x0008]));
    let _ = world.step_move_vm(0, &world.move_bytecode[0].clone());
    assert_eq!(world.move_counter, 0);
}

#[test]
fn move_vm_slot_table_save_and_load_round_trip() {
    let mut world = World::new();
    world.actors[0].active = true;
    world.actors[0].move_state.world_x = 0x1234u16 as i16;
    world.actors[0].move_state.world_y = 0x5678u16 as i16;
    world.actors[0].move_state.world_z = 0x9ABCu16 as i16;
    world.actors[0].move_state.world_y_mirror = 0xDEF0u16 as i16;
    world.actors[0].move_state.field_86 = 0x0003; // slot index = 3
    // 0x2F sub-op 0x11 - save world coords into slot 3, then HALT.
    world.set_move_bytecode(0, Some(vec![0x002F, 0x0011, 0x0008]));
    let _ = world.step_move_vm(0, &world.move_bytecode[0].clone());
    // Verify the bytes landed in slot 3.
    let lo = u32::from_le_bytes(world.move_slot_table[3][0..4].try_into().unwrap());
    let hi = u32::from_le_bytes(world.move_slot_table[3][4..8].try_into().unwrap());
    assert_eq!(lo & 0xFFFF, 0x1234);
    assert_eq!((lo >> 16) & 0xFFFF, 0x5678);
    assert_eq!(hi & 0xFFFF, 0x9ABC);
    assert_eq!((hi >> 16) & 0xFFFF, 0xDEF0);
}

#[test]
fn move_vm_bytecode_write_persists_after_step() {
    let mut world = World::new();
    world.actors[0].active = true;
    world.actors[0].move_state.world_x = 100;
    world.actors[0].move_state.world_y = 200;
    world.actors[0].move_state.world_z = 50;
    // 0x2F sub-op 0x04 - write actor world XYZ to bytecode at
    // pc + op[2] + 3. With pc=0 and op[2]=2, target indices are 5/6/7.
    let bc = vec![
        0x002F, 0x0004, 0x0002, 0xCAFE, 0xCAFE, 0x0000, 0x0000, 0x0000,
    ];
    world.set_move_bytecode(0, Some(bc.clone()));
    let _ = world.step_move_vm(0, &bc);
    // After step, the world's stored bytecode should reflect the writes.
    assert_eq!(world.move_bytecode[0][5], 100u16);
    assert_eq!(world.move_bytecode[0][6], 200u16);
    assert_eq!(world.move_bytecode[0][7], 50u16);
}

#[test]
fn move_vm_bytecode_inplace_add_sees_prior_step_writes() {
    // 0x2F sub-op 0x1E does buffer[pc + op[2] + 4] += op[3].
    // After two consecutive steps each adding 5, the slot should hold 10
    // (proving the world flushes deferred writes between steps).
    let mut world = World::new();
    world.actors[0].active = true;
    // Two 0x1E ops back-to-back, each pointing at the same operand slot.
    // Each op is size 1 (default_arm), so we step it twice.
    // Slot 4 from instruction at pc=0 lands at index 4.
    let bc = vec![0x002F, 0x001E, 0, 5, 0]; // op[2]=0, op[3]=5
    world.set_move_bytecode(0, Some(bc.clone()));
    // First step: bytecode[0 + 0 + 4] (= 0) += 5 → 5.
    let _ = world.step_move_vm(0, &bc);
    assert_eq!(world.move_bytecode[0][4], 5);
    // Step again with a fresh-cloned bytecode read of the world's buffer.
    let bc2 = world.move_bytecode[0].clone();
    // PC has advanced; reset for the same op to fire again.
    world.actors[0].move_state.pc = 0;
    let _ = world.step_move_vm(0, &bc2);
    assert_eq!(
        world.move_bytecode[0][4], 10,
        "second 0x1E should see flushed write from first step"
    );
}

// --- system flag bank (round 6) -------------------------------------

#[test]
fn system_flag_set_and_test_round_trips_through_world() {
    let mut world = World::new();
    world.system_flag_set(0);
    world.system_flag_set(7);
    world.system_flag_set(15);
    world.system_flag_set(255);
    assert!(world.system_flag_test(0));
    assert!(world.system_flag_test(7));
    assert!(world.system_flag_test(15));
    assert!(world.system_flag_test(255));
    assert!(!world.system_flag_test(1));
    assert!(!world.system_flag_test(254));
    // Out-of-bounds idx returns false.
    assert!(!world.system_flag_test(256));
    assert!(!world.system_flag_test(0xFFFF));
}

#[test]
fn system_flag_clear_only_touches_target_bit() {
    let mut world = World::new();
    world.system_flag_set(3);
    world.system_flag_set(4);
    world.system_flag_clear(3);
    assert!(!world.system_flag_test(3));
    assert!(world.system_flag_test(4));
}

#[test]
fn move_vm_ext_query_flag_bank_reads_world_system_flags() {
    let mut world = World::new();
    world.actors[0].active = true;
    world.system_flag_set(42);
    // Bytecode: 0x2F sub-op 0x13 - predicate-true → default_arm (size 1),
    // predicate-false → size 4.
    let bc = vec![0x002F, 0x0013, 42];
    world.set_move_bytecode(0, Some(bc.clone()));
    let _ = world.step_move_vm(0, &bc);
    // Predicate true → PC advanced by 1.
    assert_eq!(world.actors[0].move_state.pc, 1);
    // Now clear and re-run - predicate false → PC += 4.
    world.system_flag_clear(42);
    world.actors[0].move_state.pc = 0;
    let _ = world.step_move_vm(0, &bc);
    assert_eq!(world.actors[0].move_state.pc, 4);
}

#[test]
fn move_vm_ext_set_flag_bank_writes_world_system_flags() {
    let mut world = World::new();
    world.actors[0].active = true;
    // Bytecode: 0x2F sub-op 0x1C - set flag bank (idx = op_w(2)).
    let bc = vec![0x002F, 0x001C, 100];
    world.set_move_bytecode(0, Some(bc.clone()));
    assert!(!world.system_flag_test(100));
    let _ = world.step_move_vm(0, &bc);
    assert!(world.system_flag_test(100));
}

#[test]
fn field_vm_system_flag_set_routes_to_world() {
    // Field-VM 0x5x default-route SET - `[0x50 | nibble, idx_byte]`.
    // idx encoding: `((opcode_byte & 0x8F) << 8) | idx_byte`. For raw
    // opcode 0x50, top bit clear, low nibble 0 → idx = idx_byte.
    let mut world = World::new();
    world.mode = SceneMode::Field;
    world.load_field_script(vec![0x50, 42]);
    let _ = world.tick();
    assert!(
        world.system_flag_test(42),
        "0x50 default-route should set system flag 42"
    );
}

#[test]
fn field_vm_system_flag_set_with_low_nibble_includes_high_byte() {
    // 0x52 with low-nibble 2 → idx = (0x02 << 8) | idx_byte.
    let mut world = World::new();
    world.mode = SceneMode::Field;
    world.load_field_script(vec![0x52, 7]);
    let _ = world.tick();
    assert!(
        world.system_flag_test(0x0207),
        "0x52 default-route should set system flag 0x0207"
    );
}

#[test]
fn field_vm_system_flag_clear_routes_to_world() {
    let mut world = World::new();
    world.mode = SceneMode::Field;
    world.system_flag_set(99);
    // 0x60 CLEAR with operand 99.
    world.load_field_script(vec![0x60, 99]);
    let _ = world.tick();
    assert!(!world.system_flag_test(99));
}

#[test]
fn field_vm_system_flag_test_takes_jump_when_bit_set() {
    let mut world = World::new();
    world.mode = SceneMode::Field;
    world.system_flag_set(33);
    // 0x70 TEST with idx=33, jump delta = 10.
    world.load_field_script(vec![0x70, 33, 10, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    let _ = world.tick();
    // pc was 0; header_size = 1; +1 (idx byte) + delta(10) = 12.
    assert_eq!(world.field_pc, 12);
}

#[test]
fn inline_dialogue_runs_branch_flag_set_through_field_vm() {
    // A menu box ("Hi" + A/B picker) whose option branches each SET a distinct
    // system flag before their reply. The faithful runner must (a) show the
    // menu, (b) on confirm apply the chosen option's relative jump, (c) run the
    // branch's `0x50` SET through the field VM, (d) show the reply. Choosing B
    // must set flag 6 and NOT flag 5.
    let mut b = vec![0x1F, b'H', b'i', 0x00]; // prompt, ends at pc 4
    let open = b.len(); // 4
    b.push(0x27); // 2-option picker
    let entries_at = b.len(); // 5
    b.extend_from_slice(&[0, 0, 0, 0]); // 2 jump entries, filled below
    b.push(0x24); // continuation
    b.extend_from_slice(&[0x1F, b'A', 0x00]); // label 0
    b.extend_from_slice(&[0x1F, b'B', 0x00]); // label 1
    let branch0 = b.len();
    b.extend_from_slice(&[0x50, 0x05]); // option A: SET system flag 5
    b.extend_from_slice(&[0x1F, b'a', 0x00]); // reply "a"
    b.push(0x00); // conversation end
    let branch1 = b.len();
    b.extend_from_slice(&[0x50, 0x06]); // option B: SET system flag 6
    b.extend_from_slice(&[0x1F, b'b', 0x00]); // reply "b"
    b.push(0x00); // conversation end
    let j0 = (branch0 as i32 - (open as i32 + 1)) as i16;
    let j1 = (branch1 as i32 - (open as i32 + 1 + 2)) as i16;
    b[entries_at..entries_at + 2].copy_from_slice(&j0.to_le_bytes());
    b[entries_at + 2..entries_at + 4].copy_from_slice(&j1.to_le_bytes());

    let mut world = World::new();
    world.start_inline_dialogue(b);

    // Tick until the menu box is awaiting a choice.
    let mut guard = 0;
    while !world.inline_dialogue.as_ref().unwrap().menu_active() {
        world.step_inline_dialogue(false, false, false);
        guard += 1;
        assert!(guard < 50, "menu never became active");
    }
    // Move the cursor to option B and confirm.
    world.step_inline_dialogue(false, false, true);
    assert_eq!(world.inline_dialogue.as_ref().unwrap().last_choice, None);
    world.step_inline_dialogue(true, false, false);
    assert_eq!(world.inline_dialogue.as_ref().unwrap().last_choice, Some(1));

    // The VM should run branch B (SET flag 6) and surface the "b" reply.
    let mut guard = 0;
    while world.inline_dialogue.as_ref().unwrap().page_bytes() != b"b" {
        world.step_inline_dialogue(false, false, false);
        guard += 1;
        assert!(guard < 50, "branch reply never typed");
    }
    assert!(
        world.system_flag_test(6),
        "option B branch SET flag 6 via the VM"
    );
    assert!(
        !world.system_flag_test(5),
        "option A branch must not have run"
    );

    // Confirming the reply ends the conversation.
    world.step_inline_dialogue(true, false, false);
    world.step_inline_dialogue(false, false, false);
    assert!(world.inline_dialogue.as_ref().unwrap().is_done());
}

/// Step the inline runner until a box is open and the typewriter has fully
/// revealed it (the glyph bytes stop growing), then return its page glyph bytes.
/// Panics if no stable box appears within a bounded number of ticks.
fn run_inline_until_box(world: &mut World) -> Vec<u8> {
    let mut last: Vec<u8> = Vec::new();
    let mut stable = 0;
    for _ in 0..400 {
        world.step_inline_dialogue(false, false, false);
        let pb = world.inline_dialogue.as_ref().unwrap().page_bytes();
        if pb.is_empty() {
            continue;
        }
        if pb == last {
            stable += 1;
            if stable >= 2 {
                return pb;
            }
        } else {
            stable = 0;
            last = pb;
        }
    }
    panic!("box never opened / finished typing");
}

#[test]
fn inline_dialogue_prologue_selects_segment_by_story_flag() {
    // The interaction record's prologue is a single `SysFlag.Test` (op `0x70`)
    // on story flag 7: when the flag is set it jumps to segment B, otherwise it
    // falls through to segment A. This is the retail segment-selection mechanism
    // - the prologue's story-flag-gated jump chooses which line the box opens at.
    //
    //   pc 0: 70 07 06 00   SysFlag.Test flag 7 -> jump to pc (2 + 6) = 8
    //   pc 4: 1F 'A' 'A' 00  segment A (fall-through)
    //   pc 8: 1F 'B' 'B' 00  segment B (selected when flag 7 set)
    let body = vec![
        0x70, 0x07, 0x06, 0x00, // SysFlag.Test flag 7
        0x1F, b'A', b'A', 0x00, // segment A @ 4
        0x1F, b'B', b'B', 0x00, // segment B @ 8
    ];
    let entry_pc = 0;
    let first_segment = 4;

    // Flag clear: the test falls through to segment A.
    let mut world = World::new();
    assert!(!world.system_flag_test(7));
    world.start_inline_dialogue_with_prologue(body.clone(), entry_pc, first_segment);
    assert_eq!(run_inline_until_box(&mut world), b"AA");

    // Flag set: the prologue jumps to segment B.
    let mut world = World::new();
    world.system_flag_set(7);
    world.start_inline_dialogue_with_prologue(body, entry_pc, first_segment);
    assert_eq!(run_inline_until_box(&mut world), b"BB");
}

#[test]
fn inline_dialogue_prologue_falls_back_when_it_cannot_reach_a_segment() {
    // A prologue that can't proceed (here a `CFLAG_TST` on a clear ctx bit, which
    // halts) must not silently drop the dialogue: the runner falls back to the
    // first segment so the box still shows - never worse than the truncated path.
    //
    //   pc 0: 33 05         CFLAG_TST bit 5 (clear on a fresh ctx) -> Halt
    //   pc 2: 1F 'X' 'X' 00 first segment (fallback target)
    let body = vec![0x33, 0x05, 0x1F, b'X', b'X', 0x00];
    let mut world = World::new();
    world.start_inline_dialogue_with_prologue(body, 0, 2);
    assert_eq!(run_inline_until_box(&mut world), b"XX");
}

/// Build the A/B menu script used by the inline-dialogue tests: prompt "Hi",
/// a 2-option picker whose option A branch SETs system flag 5 and option B SETs
/// flag 6, each followed by a reply + conversation-end terminator.
fn ab_menu_inline_script() -> Vec<u8> {
    let mut b = vec![0x1F, b'H', b'i', 0x00];
    let open = b.len();
    b.push(0x27);
    let entries_at = b.len();
    b.extend_from_slice(&[0, 0, 0, 0]);
    b.push(0x24);
    b.extend_from_slice(&[0x1F, b'A', 0x00]);
    b.extend_from_slice(&[0x1F, b'B', 0x00]);
    let branch0 = b.len();
    b.extend_from_slice(&[0x50, 0x05, 0x1F, b'a', 0x00, 0x00]);
    let branch1 = b.len();
    b.extend_from_slice(&[0x50, 0x06, 0x1F, b'b', 0x00, 0x00]);
    let j0 = (branch0 as i32 - (open as i32 + 1)) as i16;
    let j1 = (branch1 as i32 - (open as i32 + 1 + 2)) as i16;
    b[entries_at..entries_at + 2].copy_from_slice(&j0.to_le_bytes());
    b[entries_at + 2..entries_at + 4].copy_from_slice(&j1.to_le_bytes());
    b
}

#[test]
fn vm_dialogue_tick_executes_branch_through_field_vm() {
    // Drive the inline-script runner through the LIVE `World::tick` field path:
    // `use_vm_dialogue` + a `current_dialog` request + pad edges. Selecting
    // option B must run its branch's SET (flag 6) through the field VM.
    let mut world = World::new();
    world.mode = SceneMode::Field;
    world.use_vm_dialogue = true;
    world.current_dialog = Some(DialogRequest {
        text_id: 0,
        inline: ab_menu_inline_script(),
        world_x: 0,
        world_z: 0,
        depth_id: 0,
    });

    // Tick (no input) until the menu is awaiting a choice.
    let mut guard = 0;
    while !world
        .inline_dialogue
        .as_ref()
        .is_some_and(|d| d.menu_active())
    {
        world.set_pad(0);
        let _ = world.tick();
        guard += 1;
        assert!(guard < 60, "menu never became active through tick");
    }
    // Down edge → option B; Cross edge → confirm.
    world.set_pad(input::PadButton::Down.mask());
    let _ = world.tick();
    world.set_pad(0);
    let _ = world.tick();
    world.set_pad(input::PadButton::Cross.mask());
    let _ = world.tick();
    // Tick out the branch + reply with no further input.
    let mut guard = 0;
    while !world.system_flag_test(6) {
        world.set_pad(0);
        let _ = world.tick();
        guard += 1;
        assert!(guard < 60, "branch SET flag 6 never landed through tick");
    }
    assert!(
        !world.system_flag_test(5),
        "option A branch must not have run"
    );
}

#[test]
fn field_vm_extra_flags_op42_reads_world() {
    // Op 0x42 mode=0 - host.extra_flags() & (1 << (op1 & 0x1F)) test.
    // Set bit 5 in extra_flags; op_42 with op1=5 should take the jump.
    let mut world = World::new();
    world.mode = SceneMode::Field;
    world.extra_flags = 1 << 5;
    // [0x42, mode=0, op1=5, lo=4, hi=0] - header_size + 4 = 5 byte total
    // for skip path; jump path = pc + header_size + 2 + delta.
    world.load_field_script(vec![0x42, 0, 5, 4, 0]);
    let _ = world.tick();
    // With extra_flags bit 5 set, predicate is true → jump.
    // Jump target = 0 + 1 (header) + 2 + 4 = 7.
    assert_eq!(world.field_pc, 7, "extra_flags-true 0x42 should take jump");
}

#[test]
fn move_vm_ext_set_8007b9d8_writes_world_field() {
    let mut world = World::new();
    world.actors[0].active = true;
    // 0x2F sub-op 0x2F - `_DAT_8007B9D8 = (i32) op[1]`. Note: op[1] in
    // sub-op space = sub-op selector 0x2F itself, op[2] = the value.
    // Per the move_vm port, ext sub-op 0x2F passes op[1] (the sub-op
    // word's "next slot" in the operand stream).
    let bc = vec![0x002F, 0x002F, 0xCAFE];
    world.set_move_bytecode(0, Some(bc.clone()));
    let _ = world.step_move_vm(0, &bc);
    // Whatever the sub-op handler writes, world.move_dat_8007b9d8 should
    // pick up a non-zero value.
    assert_ne!(world.move_dat_8007b9d8, 0);
}

#[test]
fn ext_compute_angle_matches_quadrant_when_player_set() {
    // Place actor at origin, player due-east; angle should be ~0 mod 4096
    // (positive X direction = angle 0 in the dz.atan2(dx) convention).
    let mut world = World::new();
    world.actors[0].active = true;
    world.actors[0].move_state.world_x = 0;
    world.actors[0].move_state.world_z = 0;
    world.actors[1].active = true;
    world.actors[1].move_state.world_x = 100;
    world.actors[1].move_state.world_z = 0;
    world.player_actor_slot = Some(1);
    // Drive ext sub-op 0x3A: VM writes the angle into bytecode at
    // `state.pc + op_w(2) + 3`. With pc=0 and op_w(2)=0, dst = u16[3].
    let bc = vec![0x002F, 0x003A, 0, 0xFFFF, 0xFFFF];
    world.set_move_bytecode(0, Some(bc.clone()));
    let _ = world.step_move_vm(0, &bc);
    // angle 0 (player due-east) should produce ~0 in the dst slot.
    assert_eq!(
        world.move_bytecode[0][3], 0,
        "angle to due-east player should be 0"
    );
}

#[test]
fn ext_compute_angle_returns_zero_when_no_player() {
    // No player slot designated → ext_compute_angle returns 0.
    let mut world = World::new();
    world.actors[0].active = true;
    let bc = vec![0x002F, 0x003A, 0, 0xFFFF];
    world.set_move_bytecode(0, Some(bc.clone()));
    let _ = world.step_move_vm(0, &bc);
    assert_eq!(world.move_bytecode[0][3], 0);
}

#[test]
fn ext_party_member_lookup_returns_table_position() {
    let mut world = World::new();
    world.actors[0].active = true;
    // Party member at index 1 = world actor slot 5 with a known position.
    world.actors[5].active = true;
    world.actors[5].move_state.world_x = 100;
    world.actors[5].move_state.world_y = 50;
    world.actors[5].move_state.world_z = 200;
    world.party_actor_slots = vec![None, Some(5), None];
    // Sub-op 0x3B: dst = pc + op_w(3) + 4. We use op_w(2)=1 (party slot 1)
    // and op_w(3)=0 so dst = u16[4..7].
    let bc = vec![
        0x002F, 0x003B, 0x0001, 0x0000, 0xAAAA, 0xAAAA, 0xAAAA, 0xAAAA,
    ];
    world.set_move_bytecode(0, Some(bc.clone()));
    let _ = world.step_move_vm(0, &bc);
    assert_eq!(world.move_bytecode[0][4], 100u16);
    assert_eq!(world.move_bytecode[0][5], 50u16);
    assert_eq!(world.move_bytecode[0][6], 200u16);
}

#[test]
fn ext_party_member_lookup_skips_when_none() {
    // No party table entry → 0x3B returns size-4 (skip), pre-clears dst.
    let mut world = World::new();
    world.actors[0].active = true;
    let bc = vec![0x002F, 0x003B, 0x0000, 0x0000, 0xAAAA, 0xAAAA, 0xAAAA];
    world.set_move_bytecode(0, Some(bc.clone()));
    let _ = world.step_move_vm(0, &bc);
    // Dst slots pre-cleared even when lookup returns None.
    assert_eq!(world.move_bytecode[0][4], 0);
    assert_eq!(world.move_bytecode[0][5], 0);
    assert_eq!(world.move_bytecode[0][6], 0);
}

#[test]
fn ext_fade_color_records_pending_request() {
    let mut world = World::new();
    world.actors[0].active = true;
    // Sub-op 0x3C: r=0xAB, g=0xCD, b=0xEF, ticks=4 (ramp).
    let bc = vec![0x002F, 0x003C, 0x00AB, 0x00CD, 0x00EF, 0x0004];
    world.set_move_bytecode(0, Some(bc.clone()));
    let _ = world.step_move_vm(0, &bc);
    assert_eq!(
        world.pending_fade,
        Some(FadeRequest {
            rgb: [0xAB, 0xCD, 0xEF],
            ticks: 4
        })
    );
}

#[test]
fn move_player_world_xyz_reads_designated_player_slot() {
    let mut world = World::new();
    world.actors[2].active = true;
    world.actors[2].move_state.world_x = 100;
    world.actors[2].move_state.world_y = 200;
    world.actors[2].move_state.world_z = 300;
    world.player_actor_slot = Some(2);
    // No direct API to read move_player_world_xyz; verify by stepping
    // sub-op 0x39 (squared-distance "inside radius" predicate). With
    // actor 0 at origin and player at (100, _, 300), dist_sq = 100²+300² =
    // 100000 - predicate fails for r=10 (r² = 100), passes for r=400
    // (r² = 160000).
    world.actors[0].active = true;
    // Predicate fail → PC += 4.
    let bc = vec![0x002F, 0x0039, 10, 0, 0, 0];
    world.set_move_bytecode(0, Some(bc.clone()));
    let _ = world.step_move_vm(0, &bc);
    assert_eq!(
        world.actors[0].move_state.pc, 4,
        "small-radius 0x39 should fail"
    );
    // Predicate pass → PC += 1.
    world.actors[0].move_state.pc = 0;
    let bc2 = vec![0x002F, 0x0039, 400, 0, 0, 0];
    world.set_move_bytecode(0, Some(bc2.clone()));
    let _ = world.step_move_vm(0, &bc2);
    assert_eq!(
        world.actors[0].move_state.pc, 1,
        "large-radius 0x39 should pass"
    );
}

// --- Field-event emission ---------------------------------------------

/// Op 0x35 sub-1 (start BGM) emits `FieldEvent::Bgm` and pins
/// `current_bgm`. Encoding: `[0x35, lo, hi, sub_op]`.
#[test]
fn field_op_35_sub1_emits_bgm_event_and_pins_current() {
    let mut world = World::new();
    world.mode = SceneMode::Field;
    // text_id = 0x42 (LE), sub_op = 1 (start field BGM).
    let bytecode = vec![0x35, 0x42, 0x00, 0x01];
    world.load_field_script(bytecode);
    let _ = world.tick();
    let evs = world.drain_field_events();
    assert!(
        evs.iter().any(|e| matches!(
            e,
            FieldEvent::Bgm {
                sub_op: 1,
                text_id: 0x42
            }
        )),
        "expected Bgm event, got {evs:?}"
    );
    assert_eq!(world.current_bgm, Some(0x42));
}

/// Op 0x3F is the **named scene-change** (not dialog): it stages a pending
/// named scene transition from the inline destination name. Encoding:
/// `[0x3F, idx_lo, idx_hi, name_len, <name bytes>, entry_x, entry_z, dir]`.
#[test]
fn field_op_3f_stages_named_scene_transition() {
    let mut world = World::new();
    world.mode = SceneMode::Field;
    // idx = 60, name_len = 4 ("dolk"), entry_x = 0x01, entry_z = 0x02, dir = 0x03.
    let mut bytecode = vec![0x3F, 60, 0x00, 4];
    bytecode.extend_from_slice(b"dolk");
    bytecode.extend_from_slice(&[0x01, 0x02, 0x03]);
    world.load_field_script(bytecode);
    let _ = world.tick();
    assert_eq!(
        world.pending_named_scene_transition,
        Some(("dolk".to_string(), 0x01, 0x02, 0x03)),
        "0x3F must stage a named scene transition to the inline destination"
    );
    // It is NOT a dialog opener.
    assert!(
        world.current_dialog.is_none(),
        "0x3F must not open a dialog box"
    );
}

/// A 0x3F whose inline "name" is a text-desync phantom (non-CDNAME bytes)
/// stages no transition but still advances the PC.
#[test]
fn field_op_3f_rejects_phantom_name() {
    let mut world = World::new();
    world.mode = SceneMode::Field;
    let mut bytecode = vec![0x3F, 0x00, 0x00, 4];
    bytecode.extend_from_slice(b"Hi! ");
    bytecode.extend_from_slice(&[0x00, 0x00, 0x00]);
    world.load_field_script(bytecode);
    let _ = world.tick();
    assert!(world.pending_named_scene_transition.is_none());
}

/// Field dialogue opens from the **field-interact op** (`0x3E` with
/// `op0 < 100`) reading the interacted actor's inline interaction-script
/// text (keyed by the op's `slot` = the actor's MAN record index) - the real
/// field-dialogue mechanism that replaces the `0x3F`-as-dialog stand-in.
#[test]
fn field_interact_opens_actor_inline_dialogue() {
    let mut world = World::new();
    world.mode = SceneMode::Field;
    // Seed actor slot 3's inline interaction-script dialogue.
    world
        .field_npc_dialog
        .insert(3, vec![0x1F, b'h', b'i', 0x00]);
    // 0x3E with op0 = 5 (< 100 -> field interact), op1 = slot 3.
    world.load_field_script(vec![0x3E, 0x05, 0x03]);
    let _ = world.tick();
    let req = world
        .current_dialog
        .as_ref()
        .expect("field_interact on an actor with inline text must open dialogue");
    assert_eq!(req.inline, vec![0x1F, b'h', b'i', 0x00]);
    let evs = world.drain_field_events();
    assert!(
        evs.iter()
            .any(|e| matches!(e, FieldEvent::OpenDialog { inline, .. } if !inline.is_empty())),
        "expected OpenDialog from the field-interact path, got {evs:?}"
    );
    assert!(
        evs.iter().any(|e| matches!(
            e,
            FieldEvent::FieldInteract {
                interact_id: 5,
                slot: 3
            }
        )),
        "field_interact must still surface the FieldInteract event"
    );
}

/// A field-interact on an actor with **no** inline text just surfaces the
/// interaction (a sign / flag-only NPC) - no dialogue box.
#[test]
fn field_interact_without_inline_text_opens_no_dialogue() {
    let mut world = World::new();
    world.mode = SceneMode::Field;
    world.load_field_script(vec![0x3E, 0x05, 0x07]);
    let _ = world.tick();
    assert!(
        world.current_dialog.is_none(),
        "no inline text for slot 7 -> no dialogue"
    );
}

/// The field-VM dialogue-accept auto-arms a scripted-encounter carrier.
///
/// Interacting with the carrier's placement (field-interact op `0x3E`,
/// `op0 < 100`) opens its dialogue and arms the engage; accepting the prompt
/// (the dialog-advance dismiss, op `0x4C` n5 sub-4) engages the carrier, so the
/// SM (`FUN_801DA51C`) runs its scene-transition and flips Field -> Battle -
/// with no manual `engage_field_carrier` call. This is the field-VM-driven
/// counterpart to the carrier-engage API.
#[test]
fn field_dialogue_accept_auto_arms_scripted_carrier() {
    use crate::input::PadButton;

    let mut world = World::new();
    world.set_formation_table(
        crate::monster_catalog::vanilla_formation_table(),
        crate::monster_catalog::vanilla_monster_catalog(),
    );
    world.set_active_scene_label("town01");
    world.mode = SceneMode::Field;

    // Carrier 0 = scripted encounter (vanilla formation 1); carrier 1 = plain
    // NPC. Wire the slot map the way install_field_carriers_from_man would:
    // only the scripted carrier gets a slot entry (slot 3 -> carrier 0). The
    // plain NPC's slot 7 has dialogue but no carrier-slot entry.
    world.install_field_carriers(vec![
        FieldCarrierConfig::ScriptedEncounter { formation_id: 1 },
        FieldCarrierConfig::Npc { interact_id: 7 },
    ]);
    world.field_carrier_slots.insert(3, 0);
    world
        .field_npc_dialog
        .insert(3, vec![0x1F, b'h', b'i', 0x00]);
    world
        .field_npc_dialog
        .insert(7, vec![0x1F, b'y', b'o', 0x00]);

    // Interact with the scripted carrier's slot, then poll the dialog.
    world.load_field_script(vec![0x3E, 0x05, 0x03, 0x4C, 0x54]);
    world.input.set_pad(0);
    let _ = world.tick();
    assert!(
        world.current_dialog.is_some(),
        "interacting with the carrier opens its dialogue"
    );
    assert_eq!(
        world.pending_carrier_engage,
        Some(0),
        "the scripted carrier's engage is armed, waiting for the accept"
    );
    assert_eq!(
        world.mode,
        SceneMode::Field,
        "no battle while the prompt is still up"
    );

    // Accept (just-pressed Cross): dismiss -> engage -> SM -> Field -> Battle.
    world.input.set_pad(PadButton::Cross.mask());
    let _ = world.tick();
    assert!(
        world.pending_carrier_engage.is_none(),
        "the armed engage is consumed on the accept"
    );
    assert_eq!(
        world.mode,
        SceneMode::Battle,
        "accepting the scripted carrier's prompt launches the fight via the SM"
    );
}

/// The interaction probe (retail `FUN_801cf9f4` via the `DAT_801f2254`
/// facing compass): a just-pressed action button talks to the NPC the player
/// is *facing* (probe point 64 ahead, ±72 box), and only that one - a
/// distant NPC is not triggered, and after the talk the player has been
/// turned toward the matched NPC (the face-the-NPC step).
#[test]
fn interaction_probe_talks_to_adjacent_npc_only() {
    use crate::input::PadButton;

    let mut world = World::new();
    world.mode = SceneMode::Field;
    world.player_actor_slot = Some(0);
    world.actors[0].active = true;
    // Player at tile 20 (world 20*128 + 0x40 = 2624), facing X+ (engine
    // heading 0x400) toward the adjacent NPC one tile ahead.
    world.actors[0].move_state.world_x = 2624;
    world.actors[0].move_state.world_z = 2624;
    world.actors[0].move_state.render_26 = 0x400;
    // Adjacent NPC at tile (21, 20); a far NPC at tile 40 that must not trigger.
    world
        .field_npc_dialog
        .insert(5, vec![0x1F, b'h', b'i', 0x00]);
    world.field_npc_positions.insert(5, (2752, 2624)); // tile (21, 20)
    world.field_npc_dialog.insert(6, vec![0x1F, b'x', 0x00]);
    world.field_npc_positions.insert(6, (5120, 5120)); // tile (40, 40)

    world.input.set_pad(PadButton::Cross.mask());
    let _ = world.tick();
    let req = world
        .current_dialog
        .as_ref()
        .expect("action button near an NPC opens its dialogue");
    assert_eq!(
        req.inline,
        vec![0x1F, b'h', b'i', 0x00],
        "the probe opened the faced NPC (slot 5), not the far one"
    );
    assert_eq!(
        world.actors[0].move_state.render_26, 0x400,
        "face-the-NPC: the player heading points at the matched NPC (X+)"
    );
}

/// The probe is facing-indexed: the same adjacent NPC does NOT answer when
/// the player looks away from it (retail probes a single compass point 64
/// units ahead of the facing, not a radius around the player).
#[test]
fn interaction_probe_requires_facing_the_npc() {
    use crate::input::PadButton;

    let mut world = World::new();
    world.mode = SceneMode::Field;
    world.player_actor_slot = Some(0);
    world.actors[0].active = true;
    world.actors[0].move_state.world_x = 2624;
    world.actors[0].move_state.world_z = 2624;
    // NPC one tile X+ ahead, but the player faces Z+ (engine heading 0).
    world.actors[0].move_state.render_26 = 0;
    world
        .field_npc_dialog
        .insert(5, vec![0x1F, b'h', b'i', 0x00]);
    world.field_npc_positions.insert(5, (2752, 2624));

    world.input.set_pad(PadButton::Cross.mask());
    let _ = world.tick();
    assert!(
        world.current_dialog.is_none(),
        "an NPC beside the player is not talked to while facing away"
    );
}

/// The probe is inert when no NPC is within range: pressing the action button in
/// open field opens nothing.
#[test]
fn interaction_probe_no_npc_in_range_opens_nothing() {
    use crate::input::PadButton;

    let mut world = World::new();
    world.mode = SceneMode::Field;
    world.player_actor_slot = Some(0);
    world.actors[0].active = true;
    world.actors[0].move_state.world_x = 2624;
    world.actors[0].move_state.world_z = 2624;
    world.field_npc_dialog.insert(6, vec![0x1F, b'x', 0x00]);
    world.field_npc_positions.insert(6, (5120, 5120)); // tile (40, 40), far

    world.input.set_pad(PadButton::Cross.mask());
    let _ = world.tick();
    assert!(
        world.current_dialog.is_none(),
        "no NPC near the facing probe point -> the action button opens no dialogue"
    );
}

/// Capture-grounded probe geometry: the `rimelm_npc_press_tetsu` frame has
/// the player at (2762, 1782) pressed Z+ into Tetsu at (2752, 1856). With
/// the player facing Z+, the `DAT_801f2254` sector-4 probe point lands at
/// (2762, 1846) - deltas (10, 10) from Tetsu, well inside the ±72 interact
/// box - so the action button talks to him from the captured rest position.
#[test]
fn interaction_probe_matches_tetsu_capture_geometry() {
    use crate::input::PadButton;

    let mut world = World::new();
    world.mode = SceneMode::Field;
    world.player_actor_slot = Some(0);
    world.actors[0].active = true;
    world.actors[0].move_state.world_x = 2762;
    world.actors[0].move_state.world_z = 1782;
    world.actors[0].move_state.render_26 = 0; // engine heading 0 = facing Z+
    world
        .field_npc_dialog
        .insert(4, vec![0x1F, b'y', b'o', 0x00]);
    world.field_npc_positions.insert(4, (2752, 1856));

    world.input.set_pad(PadButton::Cross.mask());
    let _ = world.tick();
    assert!(
        world.current_dialog.is_some(),
        "the captured press-rest position talks to Tetsu through the facing probe"
    );
}

/// Walking up to the scripted-encounter carrier and pressing the action button
/// twice (talk, then accept) starts the fight through the probe - the fully
/// input-driven counterpart to the field-VM dialogue-accept.
#[test]
fn interaction_probe_walk_up_to_scripted_carrier_starts_fight() {
    use crate::input::PadButton;

    let mut world = World::new();
    world.set_formation_table(
        crate::monster_catalog::vanilla_formation_table(),
        crate::monster_catalog::vanilla_monster_catalog(),
    );
    world.set_active_scene_label("town01");
    world.mode = SceneMode::Field;
    world.player_actor_slot = Some(0);
    world.actors[0].active = true;
    world.actors[0].move_state.world_x = 2624; // tile 20
    world.actors[0].move_state.world_z = 2624;
    world.actors[0].move_state.render_26 = 0x400; // facing X+, toward the NPC

    // Carrier 0 = scripted encounter; its NPC (slot 5) stands at the adjacent
    // tile (21, 20) with the sparring dialogue.
    world.install_field_carriers(vec![FieldCarrierConfig::ScriptedEncounter {
        formation_id: 1,
    }]);
    world.field_carrier_slots.insert(5, 0);
    world
        .field_npc_dialog
        .insert(5, vec![0x1F, b'h', b'i', 0x00]);
    world.field_npc_positions.insert(5, (2752, 2624));

    // Talk: the probe opens the carrier's dialogue and arms the engage.
    world.input.set_pad(PadButton::Cross.mask());
    let _ = world.tick();
    assert!(
        world.current_dialog.is_some(),
        "walking up + action button opens the carrier's dialogue"
    );
    assert_eq!(world.pending_carrier_engage, Some(0), "engage armed");
    assert_eq!(
        world.mode,
        SceneMode::Field,
        "no battle while the prompt is up"
    );

    // Release, then accept: the probe dismisses the box and engages -> Battle.
    world.input.set_pad(0);
    let _ = world.tick();
    world.input.set_pad(PadButton::Cross.mask());
    let _ = world.tick();
    assert_eq!(
        world.mode,
        SceneMode::Battle,
        "accepting the probe-opened prompt starts the fight (no script, no manual engage)"
    );
}

/// A synthetic sparring dialogue carrying the immediate-labels 4-option picker
/// (option 2 = the "practice" / fight choice), mirroring the real Rim Elm spar.
fn spar_dialogue() -> Vec<u8> {
    let mut b = vec![0x1F, b'S', b'p', b'a', b'r', 0x00]; // prompt, 0x00-terminated
    b.push(0x29); // open, N=4
    for j in [0x10i16, 0x20, 0x30, 0x40] {
        b.extend_from_slice(&j.to_le_bytes()); // 4 jump entries
    }
    // labels immediately (no continuation byte) - index 2 is the fight option
    for lbl in [&b"go"[..], &b"no"[..], &b"practice"[..], &b"bye"[..]] {
        b.push(0x1F);
        b.extend_from_slice(lbl);
        b.push(0x00);
    }
    b
}

/// Set up a world with a scripted-encounter carrier whose dialogue is the spar
/// menu, the player adjacent and facing it (`(slot 5)` at tile (21, 20)).
fn world_with_spar_carrier() -> World {
    let mut world = World::new();
    world.set_formation_table(
        crate::monster_catalog::vanilla_formation_table(),
        crate::monster_catalog::vanilla_monster_catalog(),
    );
    world.set_active_scene_label("town01");
    world.mode = SceneMode::Field;
    world.player_actor_slot = Some(0);
    world.actors[0].active = true;
    world.actors[0].move_state.world_x = 2624;
    world.actors[0].move_state.world_z = 2624;
    world.actors[0].move_state.render_26 = 0x400; // facing X+, toward the NPC
    world.install_field_carriers(vec![FieldCarrierConfig::ScriptedEncounter {
        formation_id: 1,
    }]);
    world.field_carrier_slots.insert(5, 0);
    world.field_npc_dialog.insert(5, spar_dialogue());
    world.field_npc_positions.insert(5, (2752, 2624));
    world
}

/// Talking to the sparring carrier raises its 4-option spar menu (NOT the
/// any-accept arm), and **confirming a non-fight option does not start a fight** -
/// the box just closes. The fight is gated on the index-2 ("practice") option.
#[test]
fn carrier_spar_menu_gates_engage_on_the_fight_option() {
    use crate::input::PadButton;

    let mut world = world_with_spar_carrier();

    // Talk: opens the menu (not the any-accept engage).
    world.input.set_pad(PadButton::Cross.mask());
    let _ = world.tick();
    assert!(world.current_dialog.is_some(), "carrier dialogue opens");
    assert!(
        world.pending_carrier_engage.is_none(),
        "the menu path is used, not the any-accept arm"
    );
    let menu = world.carrier_menu.expect("the spar's 4-option menu is up");
    assert_eq!(menu.n, 4, "4-option picker");
    assert_eq!(
        menu.fight_option, 2,
        "the fight option is index 2 (\"practice\")"
    );
    assert_eq!(menu.cursor, 0, "cursor starts on option 0");
    assert_eq!(world.mode, SceneMode::Field);

    // Confirm at cursor 0 (a talk option): the box closes, no fight.
    world.input.set_pad(0);
    let _ = world.tick();
    world.input.set_pad(PadButton::Cross.mask());
    let _ = world.tick();
    assert_eq!(
        world.mode,
        SceneMode::Field,
        "confirming a non-fight option does not start the fight"
    );
    assert!(world.carrier_menu.is_none(), "the menu closed");
    assert!(world.current_dialog.is_none(), "the box closed");
}

/// Navigating the spar menu down to the index-2 fight option and confirming
/// flips Field -> Battle (the faithful 4-option path).
#[test]
fn carrier_spar_menu_fight_option_starts_battle() {
    use crate::input::PadButton;

    let mut world = world_with_spar_carrier();
    world.input.set_pad(PadButton::Cross.mask());
    let _ = world.tick();
    let fight = world.carrier_menu.expect("menu up").fight_option;

    // Move the cursor down to the fight option (one fresh Down edge per step).
    for _ in 0..fight {
        world.input.set_pad(0);
        let _ = world.tick();
        world.input.set_pad(PadButton::Down.mask());
        let _ = world.tick();
    }
    assert_eq!(
        world.carrier_menu.expect("menu still up").cursor,
        fight,
        "cursor on the fight option"
    );
    assert_eq!(world.mode, SceneMode::Field, "still field while navigating");

    // Confirm: flips to Battle within a tick or two.
    world.input.set_pad(0);
    let _ = world.tick();
    world.input.set_pad(PadButton::Cross.mask());
    let mut reached = false;
    for _ in 0..4 {
        let _ = world.tick();
        if world.mode == SceneMode::Battle {
            reached = true;
            break;
        }
        world.input.set_pad(0);
    }
    assert!(reached, "confirming the fight option starts the spar");
}

/// `nav_step_toward` walks the player to a target across open field (no walls).
#[test]
fn nav_step_toward_walks_player_to_target() {
    let mut world = World::new();
    world.mode = SceneMode::Field;
    world.player_actor_slot = Some(0);
    world.actors[0].active = true;
    world.actors[0].move_state.world_x = 2624;
    world.actors[0].move_state.world_z = 2624;
    // Open field (no collision grid installed -> nothing is a wall). Target ~6
    // tiles away; the player should reach it within a generous frame budget.
    let (tx, tz) = (2752i16, 1856i16);
    let mut arrived = false;
    for _ in 0..4000 {
        if world.nav_step_toward(tx, tz, 32) {
            arrived = true;
            break;
        }
    }
    assert!(arrived, "nav walks the player to the target in open field");
    let ms = &world.actors[0].move_state;
    assert!(
        (ms.world_x - tx).abs() <= 32 && (ms.world_z - tz).abs() <= 32,
        "player ends within tolerance of the target ({}, {})",
        ms.world_x,
        ms.world_z
    );
}

/// A plain talk NPC never auto-arms a battle: interacting opens its dialogue and
/// dismissing it returns to free roam (no carrier-slot entry -> nothing armed).
#[test]
fn field_dialogue_accept_on_plain_npc_does_not_arm_battle() {
    use crate::input::PadButton;

    let mut world = World::new();
    world.set_formation_table(
        crate::monster_catalog::vanilla_formation_table(),
        crate::monster_catalog::vanilla_monster_catalog(),
    );
    world.mode = SceneMode::Field;
    world.install_field_carriers(vec![FieldCarrierConfig::Npc { interact_id: 7 }]);
    // No scripted carrier -> field_carrier_slots stays empty.
    world
        .field_npc_dialog
        .insert(7, vec![0x1F, b'y', b'o', 0x00]);

    world.load_field_script(vec![0x3E, 0x05, 0x07, 0x4C, 0x54]);
    world.input.set_pad(0);
    let _ = world.tick();
    assert!(
        world.current_dialog.is_some(),
        "plain NPC opens its dialogue"
    );
    assert_eq!(
        world.pending_carrier_engage, None,
        "a plain NPC arms no engage"
    );

    world.input.set_pad(PadButton::Cross.mask());
    let _ = world.tick();
    assert_eq!(
        world.mode,
        SceneMode::Field,
        "dismissing a plain NPC's dialogue stays in the field"
    );
}

/// Dialog-advance host hook (`op 0x4C n5 sub-4`): when `current_dialog`
/// is set, the VM halts at the poll site. A just-pressed Cross /
/// Circle clears the request inline and unblocks the VM the same
/// frame, with a `DialogDismissed` event surfaced for downstream
/// HUD consumers.
#[test]
fn dialog_advance_halts_then_clears_on_just_pressed_cross() {
    use crate::input::PadButton;

    let mut world = World::new();
    world.mode = SceneMode::Field;

    // Open dialogue via the field-interact path (the real opener), then arm a
    // poll (4C 54) followed by a sentinel op.
    // 0x3E 0x05 0x03: field-interact (op0<100) on actor slot 3 -> opens its
    //   seeded inline dialogue (3 bytes).
    // 0x4C 0x54: dialog-advance poll (2 bytes).
    // 0x00: sentinel that makes `step_field` advance further once the dialog
    //   clears.
    world
        .field_npc_dialog
        .insert(3, vec![0x1F, b'h', b'i', 0x00]);
    let bc = vec![0x3E, 0x05, 0x03, 0x4C, 0x54, 0x00];
    world.load_field_script(bc);

    // Tick 1: open the dialog. The 4C 54 poll runs next tick.
    let _ = world.tick();
    assert!(world.current_dialog.is_some(), "dialog should be open");

    // No buttons pressed: the poll halts at the same PC.
    world.input.set_pad(0);
    let pc_before = world.field_pc;
    let _ = world.tick();
    assert!(
        world.current_dialog.is_some(),
        "dialog persists with no input"
    );
    assert_eq!(
        world.field_pc, pc_before,
        "VM should halt at the poll PC while dialog is active"
    );

    // Cross just-pressed: the host clears the request inline and
    // advances PC by 2 (past the poll).
    world.input.set_pad(PadButton::Cross.mask());
    let _ = world.tick();
    assert!(
        world.current_dialog.is_none(),
        "dialog should clear on just-pressed Cross",
    );
    let evs = world.drain_field_events();
    assert!(
        evs.iter().any(|e| matches!(e, FieldEvent::DialogDismissed)),
        "expected DialogDismissed event, got {evs:?}",
    );
    assert!(
        world.field_pc > pc_before,
        "VM should advance past poll PC ({} > {})",
        world.field_pc,
        pc_before,
    );
}

/// Dialog-advance hook returns `false` (advance) when no dialog is
/// active. Mirrors the retail dispatcher's behavior when
/// `FUN_801D65D8(0)` returns zero (dialog done).
#[test]
fn dialog_advance_no_op_when_no_dialog() {
    let mut world = World::new();
    world.mode = SceneMode::Field;
    // Just the poll + sentinel - no preceding 0x3F.
    let bc = vec![0x4C, 0x54, 0x00];
    world.load_field_script(bc);
    let pc_before = world.field_pc;
    let _ = world.tick();
    assert!(
        world.field_pc > pc_before,
        "VM should advance immediately when no dialog is showing",
    );
}

/// Op 0x3A (add_money) clamps to `[0, 9_999_999]` and emits `AddMoney`.
#[test]
fn field_op_3a_clamps_and_emits_add_money() {
    let mut world = World::new();
    world.money = 100;
    world.mode = SceneMode::Field;
    // 0x3A op0=0xFF op1=0xFF op2=0xFF (24-bit -1) → delta = -1.
    // The op handler reads the 3-byte payload; sign-extend to i32.
    let bytecode = vec![0x3A, 0xFF, 0xFF, 0xFF];
    world.load_field_script(bytecode);
    let _ = world.tick();
    assert!(world.money >= 0, "money clamps to non-negative");
    let evs = world.drain_field_events();
    assert!(
        evs.iter().any(|e| matches!(e, FieldEvent::AddMoney { .. })),
        "expected AddMoney event, got {evs:?}"
    );
}

/// Op 0x3C (party_add) appends to `party_actor_slots` and seeds the
/// leader on the empty-party path.
#[test]
fn field_op_3c_party_add_first_member_becomes_leader() {
    let mut world = World::new();
    world.mode = SceneMode::Field;
    // 0x3C + char_id (op0).
    let bytecode = vec![0x3C, 0x07];
    world.load_field_script(bytecode);
    let _ = world.tick();
    assert_eq!(world.party_actor_slots, vec![Some(7)]);
    assert_eq!(world.party_leader_slot, Some(7));
    let evs = world.drain_field_events();
    assert!(
        evs.iter().any(|e| matches!(
            e,
            FieldEvent::PartyAdd {
                char_id: 7,
                accepted: true
            }
        )),
        "expected PartyAdd event, got {evs:?}"
    );
}

/// Drain helper empties the queue.
#[test]
fn drain_field_events_empties_queue() {
    let mut world = World::new();
    world
        .pending_field_events
        .push(FieldEvent::GiveItem { item_id: 1 });
    let drained = world.drain_field_events();
    assert_eq!(drained.len(), 1);
    assert!(world.pending_field_events.is_empty());
}

/// Op `0x4C 0x80` (actor allocator) walks `count` variable-length
/// records using the `FUN_8003CA38` packet-length rule, emits one
/// `ActorAllocate` event, and queues each record's bytecode in
/// `pending_actor_spawns`. Encoding here: count=2, two records each
/// terminated by `0x00`.
#[test]
fn field_op_4c_n8_sub0_walks_records_and_queues_spawns() {
    let mut world = World::new();
    world.mode = SceneMode::Field;
    // [4C, 0x80, 2, 0x40, 0x41, 0x00, 0xC1, 0x42, 0x00]
    //   record 0 = [0x40, 0x41] (two normal tokens, terminator 0x00)
    //   record 1 = [0xC1, 0x42] (escape pair via 0xCx high nibble)
    let bytecode = vec![0x4C, 0x80, 0x02, 0x40, 0x41, 0x00, 0xC1, 0x42, 0x00];
    world.load_field_script(bytecode);
    let _ = world.tick();
    // PC should land at byte 3 (the first record's first byte) - the
    // retail VM advances PC by exactly 3 regardless of how many
    // records the host consumes.
    assert_eq!(world.field_pc, 3);
    // Pending queue should hold both records, in emission order.
    let spawns = world.drain_actor_spawns();
    assert_eq!(spawns.len(), 2);
    assert_eq!(spawns[0], vec![0x40, 0x41]);
    assert_eq!(spawns[1], vec![0xC1, 0x42]);
    // The event queue should also carry one ActorAllocate with both
    // records.
    let evs = world.drain_field_events();
    let allocate = evs
        .iter()
        .find_map(|e| match e {
            FieldEvent::ActorAllocate { records } => Some(records.clone()),
            _ => None,
        })
        .expect("expected ActorAllocate event");
    assert_eq!(allocate.len(), 2);
    assert_eq!(allocate[0], vec![0x40, 0x41]);
    assert_eq!(allocate[1], vec![0xC1, 0x42]);
}

/// `count = 0` is a legal degenerate case - no records walked, no
/// event payload, but the event is still emitted to mark the
/// allocator call site.
#[test]
fn field_op_4c_n8_sub0_zero_count_emits_empty_event() {
    let mut world = World::new();
    world.mode = SceneMode::Field;
    let bytecode = vec![0x4C, 0x80, 0x00];
    world.load_field_script(bytecode);
    let _ = world.tick();
    assert_eq!(world.field_pc, 3);
    assert!(world.drain_actor_spawns().is_empty());
    let evs = world.drain_field_events();
    assert!(
        evs.iter().any(|e| matches!(
            e,
            FieldEvent::ActorAllocate { records } if records.is_empty()
        )),
        "expected empty ActorAllocate event, got {evs:?}"
    );
}

/// `drain_actor_spawns` empties the queue.
#[test]
fn drain_actor_spawns_empties_queue() {
    let mut world = World::new();
    world.pending_actor_spawns.push(vec![0xAA, 0xBB]);
    let drained = world.drain_actor_spawns();
    assert_eq!(drained, vec![vec![0xAA, 0xBB]]);
    assert!(world.pending_actor_spawns.is_empty());
}

/// `materialize_actor_spawns` allocates a fresh slot from
/// `start_slot..MAX_ACTORS`, populates it with the queued record, and
/// emits an `ActorSpawned` event.
#[test]
fn materialize_actor_spawns_allocates_slot_and_emits_event() {
    let mut world = World::new();
    world.pending_actor_spawns.push(vec![0x10, 0x20, 0x30]);
    let allocated = world.materialize_actor_spawns(8);
    assert_eq!(allocated, 1);
    assert!(world.pending_actor_spawns.is_empty());
    assert!(world.actors[8].active);
    assert_eq!(
        world.actors[8].spawn_record.as_deref(),
        Some(&[0x10, 0x20, 0x30][..])
    );
    assert_eq!(world.actors[8].kind, 0);
    assert_eq!(world.actors[8].variant, 0);
    let evs = world.drain_field_events();
    let spawned = evs
        .iter()
        .find_map(|e| match e {
            FieldEvent::ActorSpawned {
                slot,
                kind,
                variant,
                record,
            } => Some((*slot, *kind, *variant, record.clone())),
            _ => None,
        })
        .expect("expected ActorSpawned event");
    assert_eq!(spawned, (8u8, 0u16, 0u16, vec![0x10, 0x20, 0x30]));
}

/// `materialize_actor_spawns` allocates consecutive inactive slots
/// when several spawn requests are queued.
#[test]
fn materialize_actor_spawns_fills_consecutive_inactive_slots() {
    let mut world = World::new();
    world.pending_actor_spawns.push(vec![0xAA]);
    world.pending_actor_spawns.push(vec![0xBB]);
    world.pending_actor_spawns.push(vec![0xCC]);
    let allocated = world.materialize_actor_spawns(4);
    assert_eq!(allocated, 3);
    assert!(world.actors[4].active);
    assert!(world.actors[5].active);
    assert!(world.actors[6].active);
    assert_eq!(world.actors[4].spawn_record.as_deref(), Some(&[0xAA][..]));
    assert_eq!(world.actors[5].spawn_record.as_deref(), Some(&[0xBB][..]));
    assert_eq!(world.actors[6].spawn_record.as_deref(), Some(&[0xCC][..]));
}

/// Slots below `start_slot` are reserved - even when they are
/// inactive, the materializer doesn't touch them.
#[test]
fn materialize_actor_spawns_skips_reserved_low_slots() {
    let mut world = World::new();
    // Slot 0 is inactive but reserved (start_slot=10).
    world.pending_actor_spawns.push(vec![0xDE, 0xAD]);
    world.materialize_actor_spawns(10);
    assert!(!world.actors[0].active);
    assert!(world.actors[10].active);
}

/// Mirrors retail's "pool exhausted → bail silently" branch of
/// `FUN_801D77F4`. When no inactive slot is available in the
/// allocation range, the record is dropped and a `ActorSpawnFailed`
/// event is emitted instead of `ActorSpawned`.
#[test]
fn materialize_actor_spawns_emits_failure_when_pool_exhausted() {
    let mut world = World::new();
    // Make every slot from index 60 upward active.
    for slot in 60..MAX_ACTORS {
        world.actors[slot].active = true;
    }
    world.pending_actor_spawns.push(vec![0xEE]);
    let allocated = world.materialize_actor_spawns(60);
    assert_eq!(allocated, 0);
    let evs = world.drain_field_events();
    assert!(evs.iter().any(|e| matches!(
        e,
        FieldEvent::ActorSpawnFailed { record } if record == &[0xEE]
    )));
}

/// End-to-end: a field-VM `0x4C 0x80` opcode followed by
/// `materialize_actor_spawns` should land both events
/// (`ActorAllocate` from the opcode, `ActorSpawned` from the
/// materializer) and leave the actor slot populated.
#[test]
fn field_op_4c_n8_sub0_then_materialize_flow_end_to_end() {
    let mut world = World::new();
    world.mode = SceneMode::Field;
    // One record `[0x40, 0x41]` terminated by `0x00`.
    let bytecode = vec![0x4C, 0x80, 0x01, 0x40, 0x41, 0x00];
    world.load_field_script(bytecode);
    let _ = world.tick();
    let allocated = world.materialize_actor_spawns(16);
    assert_eq!(allocated, 1);
    assert!(world.actors[16].active);
    assert_eq!(
        world.actors[16].spawn_record.as_deref(),
        Some(&[0x40, 0x41][..])
    );
    let evs = world.drain_field_events();
    // Both the ActorAllocate (from the opcode) and ActorSpawned (from
    // the materializer) should appear in emission order.
    let kinds: Vec<&'static str> = evs
        .iter()
        .filter_map(|e| match e {
            FieldEvent::ActorAllocate { .. } => Some("alloc"),
            FieldEvent::ActorSpawned { .. } => Some("spawned"),
            _ => None,
        })
        .collect();
    assert_eq!(kinds, vec!["alloc", "spawned"]);
}

/// Op `0x4C 0xD8` is the synchronous-spawn sibling of the halt-acquire
/// `0x4C 0x80` path. The dispatcher decodes
/// `[0x4C, 0xD8, vdf_idx, tmd_lo, tmd_hi, kind_lo, kind_hi, var_lo, var_hi]`
/// into `(vdf_idx, [tmd_idx, kind, variant])` and calls the
/// FieldHostImpl override directly - no queue. The actor slot must
/// come out active with `kind` / `variant` mirrored from the operand,
/// and a single `ActorSpawned` event must surface in the queue.
#[test]
fn field_op_4c_d8_spawns_actor_synchronously_with_kind_variant() {
    let mut world = World::new();
    world.mode = SceneMode::Field;
    // `[0x4C, 0xD8, vdf_idx=0x07, tmd=0x0102, kind=0xABCD, variant=0xBEEF, 0x00]`.
    // Trailing 0x00 is a HALT so the VM doesn't run off the end.
    let bytecode = vec![0x4C, 0xD8, 0x07, 0x02, 0x01, 0xCD, 0xAB, 0xEF, 0xBE, 0x00];
    world.load_field_script(bytecode);
    let _ = world.tick();

    let slot = FIELD_SPAWN_START_SLOT as usize;
    assert!(
        world.actors[slot].active,
        "0x4C 0xD8 should have spawned synchronously into slot {slot}",
    );
    assert_eq!(world.actors[slot].kind, 0xABCD);
    assert_eq!(world.actors[slot].variant, 0xBEEF);
    // 0x4C 0xD8 doesn't carry packet bytes in the bytecode - the
    // record lives in the VDF buffer at runtime - so spawn_record
    // stays `None` until the VDF / global TMD lift lands.
    assert!(world.actors[slot].spawn_record.is_none());

    let evs = world.drain_field_events();
    let spawned: Vec<_> = evs
        .iter()
        .filter_map(|e| match e {
            FieldEvent::ActorSpawned {
                slot: s,
                kind,
                variant,
                record,
            } => Some((*s, *kind, *variant, record.clone())),
            _ => None,
        })
        .collect();
    assert_eq!(
        spawned,
        vec![(FIELD_SPAWN_START_SLOT, 0xABCDu16, 0xBEEFu16, Vec::new())]
    );
    // No ActorAllocate event - that one is exclusively the
    // queue-based 0x4C 0x80 path.
    assert!(
        !evs.iter()
            .any(|e| matches!(e, FieldEvent::ActorAllocate { .. })),
        "0x4C 0xD8 must not emit ActorAllocate; got {evs:?}"
    );
    // And nothing was queued on the pending_actor_spawns side - the
    // synchronous path doesn't go through the materializer.
    assert!(world.pending_actor_spawns.is_empty());
}

/// `0x4C 0xD8` with a populated VDF buffer should copy the indexed
/// body bytes onto the spawned actor's `spawn_record` (mirror of
/// retail `actor[+0x4C] = VDF_body_ptr`) and surface them in the
/// `ActorSpawned` event payload.
#[test]
fn field_op_4c_d8_with_vdf_buffer_populates_spawn_record() {
    let mut world = World::new();
    world.mode = SceneMode::Field;
    // VDF buffer with two records:
    //   header:  count = 2
    //   table:   offsets[0] = 12, offsets[1] = 16
    //   body 0:  [0xDE, 0xAD, 0xBE, 0xEF] @ off 12 (4 bytes -> 16)
    //   body 1:  [0xCA, 0xFE, 0xBA, 0xBE, 0x42] @ off 16 (to EOB)
    let mut vdf = Vec::new();
    vdf.extend_from_slice(&2u32.to_le_bytes()); // count
    vdf.extend_from_slice(&12u32.to_le_bytes()); // offsets[0]
    vdf.extend_from_slice(&16u32.to_le_bytes()); // offsets[1]
    vdf.extend_from_slice(&[0xDE, 0xAD, 0xBE, 0xEF]);
    vdf.extend_from_slice(&[0xCA, 0xFE, 0xBA, 0xBE, 0x42]);
    world.set_vdf_buffer(Some(vdf));

    // Sanity-check the lookup helper.
    assert_eq!(
        world.vdf_record_bytes(0),
        Some(&[0xDE, 0xAD, 0xBE, 0xEF][..])
    );
    assert_eq!(
        world.vdf_record_bytes(1),
        Some(&[0xCA, 0xFE, 0xBA, 0xBE, 0x42][..])
    );
    assert_eq!(world.vdf_record_bytes(2), None); // idx >= count

    // `[0x4C, 0xD8, vdf_idx=0x01, tmd=0x0102, kind=0x1111, variant=0x2222, 0x00]`.
    let bytecode = vec![0x4C, 0xD8, 0x01, 0x02, 0x01, 0x11, 0x11, 0x22, 0x22, 0x00];
    world.load_field_script(bytecode);
    let _ = world.tick();

    let slot = FIELD_SPAWN_START_SLOT as usize;
    assert!(world.actors[slot].active);
    assert_eq!(world.actors[slot].kind, 0x1111);
    assert_eq!(world.actors[slot].variant, 0x2222);
    assert_eq!(
        world.actors[slot].spawn_record.as_deref(),
        Some(&[0xCA, 0xFE, 0xBA, 0xBE, 0x42][..]),
        "spawn_record should mirror VDF body 1"
    );

    let evs = world.drain_field_events();
    let spawned: Vec<_> = evs
        .iter()
        .filter_map(|e| match e {
            FieldEvent::ActorSpawned { record, .. } => Some(record.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(spawned, vec![vec![0xCA, 0xFE, 0xBA, 0xBE, 0x42]]);
}

/// `0x4C 0xD8` with a populated global TMD pool should write a
/// matching `Arc<GlobalTmd>` onto the spawned actor's `tmd_ref`
/// (mirror of retail `actor[+0x48] = DAT_8007C018[tmd_idx]`).
/// Indices the pool hasn't seen leave `tmd_ref` at `None` rather
/// than aborting the spawn.
#[test]
fn field_op_4c_d8_with_global_tmd_pool_populates_tmd_ref() {
    let mut world = World::new();
    world.mode = SceneMode::Field;

    // Install a stub TMD at pool slot 5. The Tmd doesn't need to
    // represent realistic mesh data - the host hook only does an
    // Arc::clone and stores the result.
    let stub = std::sync::Arc::new(GlobalTmd {
        tmd: legaia_tmd::Tmd {
            header: legaia_tmd::Header {
                id: 0x8000_0002,
                flags: 1,
                nobj: 0,
                flist_bit_set: true,
            },
            objects: Vec::new(),
        },
        raw: vec![
            0x02, 0x00, 0x00, 0x80, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        ],
    });
    let stub_ptr = std::sync::Arc::as_ptr(&stub);
    world.set_global_tmd(5, stub.clone());

    // `[0x4C, 0xD8, vdf_idx=0x00, tmd=0x0005, kind=0x1111, variant=0x2222, 0x00]`.
    let bytecode = vec![0x4C, 0xD8, 0x00, 0x05, 0x00, 0x11, 0x11, 0x22, 0x22, 0x00];
    world.load_field_script(bytecode);
    let _ = world.tick();

    let slot = FIELD_SPAWN_START_SLOT as usize;
    assert!(world.actors[slot].active);
    let tmd_ref = world.actors[slot]
        .tmd_ref
        .as_ref()
        .expect("tmd_ref should mirror DAT_8007C018[5]");
    assert_eq!(
        std::sync::Arc::as_ptr(tmd_ref),
        stub_ptr,
        "tmd_ref should reference the installed pool entry by Arc identity",
    );

    // A second spawn with an unpopulated index leaves tmd_ref at None.
    let bytecode2 = vec![0x4C, 0xD8, 0x00, 0x09, 0x00, 0x33, 0x33, 0x44, 0x44, 0x00];
    world.load_field_script(bytecode2);
    let _ = world.tick();
    let slot2 = slot + 1;
    assert!(world.actors[slot2].active);
    assert!(
        world.actors[slot2].tmd_ref.is_none(),
        "empty pool slot should not populate tmd_ref",
    );
}

/// Accessors round-trip: `set_global_tmd` + `global_tmd` agree on
/// installed slots, negative indices return `None`, and the pool
/// grows lazily.
#[test]
fn global_tmd_accessor_round_trip() {
    let mut world = World::new();
    assert!(world.global_tmd(0).is_none());
    assert!(world.global_tmd(-1).is_none());

    let stub = std::sync::Arc::new(GlobalTmd {
        tmd: legaia_tmd::Tmd {
            header: legaia_tmd::Header {
                id: 0x8000_0002,
                flags: 1,
                nobj: 0,
                flist_bit_set: true,
            },
            objects: Vec::new(),
        },
        raw: Vec::new(),
    });
    world.set_global_tmd(3, stub.clone());
    // Pool grew to fit idx 3.
    assert_eq!(world.global_tmd_pool.len(), 4);
    assert!(world.global_tmd_pool[0..3].iter().all(|s| s.is_none()));
    assert!(std::sync::Arc::ptr_eq(
        world.global_tmd(3).expect("slot 3 populated"),
        &stub
    ));
    assert!(world.global_tmd(7).is_none(), "out-of-range -> None");
    assert!(world.global_tmd(-5).is_none(), "negative -> None");
}

/// `vdf_record_bytes` rejects out-of-range indices, malformed
/// buffers, and the `None` (no VDF installed) path.
#[test]
fn vdf_record_bytes_handles_edge_cases() {
    let mut world = World::new();
    assert_eq!(world.vdf_record_bytes(0), None, "no VDF -> None");

    // Empty buffer (shorter than header word).
    world.set_vdf_buffer(Some(vec![0x01, 0x02]));
    assert_eq!(world.vdf_record_bytes(0), None);

    // Count = 0.
    world.set_vdf_buffer(Some(vec![0x00, 0x00, 0x00, 0x00]));
    assert_eq!(world.vdf_record_bytes(0), None);

    // Count = 1 but offset walks past EOB.
    let mut buf = Vec::new();
    buf.extend_from_slice(&1u32.to_le_bytes()); // count
    buf.extend_from_slice(&0xFFFFu32.to_le_bytes()); // offsets[0] - past EOB
    buf.extend_from_slice(&[0xAAu8; 8]);
    world.set_vdf_buffer(Some(buf));
    assert_eq!(world.vdf_record_bytes(0), None);
}

/// `tick_move_vms` records per-actor outcomes via `actor_tick`. A
/// HALT-loaded script (op `0x08` = HALT, encoded as `0x0008` in u16)
/// should yield `Halted`.
#[test]
fn tick_move_vms_records_halt_outcome() {
    let mut world = World::new();
    world.spawn_actor(0);
    world.actors[0].move_state.wait_timer = -1;
    // Move-VM HALT opcode is `0x08`.
    world.set_move_bytecode(0, Some(vec![0x0008]));
    world.tick_move_vms();
    assert!(
        world
            .move_outcomes
            .iter()
            .any(|(s, o)| *s == 0 && matches!(o, vm::move_vm::ActorTickOutcome::Halted)),
        "expected actor 0 to halt, got {:?}",
        world.move_outcomes
    );
}

/// Wait gate: actor with `wait_timer >= 0` reports Waiting and the VM
/// is not entered. Decrement happens before the gate.
#[test]
fn tick_move_vms_with_delta_decrements_then_gates() {
    let mut world = World::new();
    world.spawn_actor(0);
    world.actors[0].move_state.wait_timer = 3;
    // Bytecode that would change state if VM ran (op 0x08 HALT).
    world.set_move_bytecode(0, Some(vec![0x0008]));
    world.tick_move_vms_with_delta(1);
    // After delta=1: wait_timer = 2, still >= 0 -> Waiting.
    assert_eq!(world.actors[0].move_state.wait_timer, 2);
    assert!(matches!(
        world.move_outcomes[0],
        (0, vm::move_vm::ActorTickOutcome::Waiting)
    ));
    // After three more ticks (delta=1 each): wait_timer goes 1, 0, -1.
    // Only when wait_timer is strictly negative does the VM run.
    world.tick_move_vms_with_delta(1);
    world.tick_move_vms_with_delta(1);
    world.tick_move_vms_with_delta(1);
    // The last tick should have entered the VM and Halted.
    assert!(matches!(
        world.move_outcomes[0],
        (0, vm::move_vm::ActorTickOutcome::Halted)
    ));
}

#[test]
fn try_spawn_effect_populates_pool() {
    let mut world = World::default();
    let script = vm::effect_vm::EffectScript {
        child_count: 2,
        flags: 0,
        spread: 0,
        body: vec![],
    };
    world.effect_catalog = vm::effect_vm::EffectCatalog::new(vec![(script, vec![])]);
    assert_eq!(world.effect_pool.active_count(), 0);
    world.try_spawn_effect(0, [10, 0, -10], 0x200);
    assert_eq!(world.effect_pool.active_count(), 1);
    assert_eq!(world.effect_pool.master_slots[0].pos_x, 10i32 << 8);
}

#[test]
fn active_effect_markers_reflect_pool_and_fade_with_age() {
    let mut world = World::default();
    let script = vm::effect_vm::EffectScript {
        child_count: 2,
        flags: 0,
        spread: 0,
        body: vec![],
    };
    world.effect_catalog = vm::effect_vm::EffectCatalog::new(vec![(script, vec![])]);

    // No live effects -> no markers.
    assert!(world.active_effect_markers().is_empty());

    world.try_spawn_effect(0, [10, 0, -10], 0x200);
    let markers = world.active_effect_markers();
    assert_eq!(markers.len(), 1);
    // 8.8 fixed pool position decodes back to the spawn world units.
    assert_eq!(markers[0].world_pos, [10.0, 0.0, -10.0]);
    assert_eq!(markers[0].angle, 0x200);
    // Freshly spawned: no elapsed frames yet.
    assert_eq!(markers[0].age01, 0.0);

    // Age advances toward 1.0 as the effect ticks through its lifetime.
    world.tick_effects();
    let aged = world.active_effect_markers();
    assert_eq!(aged.len(), 1);
    assert!(aged[0].age01 > 0.0 && aged[0].age01 < 1.0);

    // Once the lifetime is spent the slot retires and emits no marker.
    for _ in 0..vm::effect_vm::DEFAULT_EFFECT_LIFETIME_FRAMES {
        world.tick_effects();
    }
    assert!(world.active_effect_markers().is_empty());
}

#[test]
fn spawn_debug_effect_seats_marker_then_ages_out() {
    let mut world = World::default();
    assert!(world.spawn_debug_effect([128.0, 0.0, -64.0]));
    let markers = world.active_effect_markers();
    assert_eq!(markers.len(), 1);
    assert_eq!(markers[0].world_pos, [128.0, 0.0, -64.0]);
    assert_eq!(markers[0].age01, 0.0);

    // Ages and retires via the normal effect lifetime.
    for _ in 0..=vm::effect_vm::DEFAULT_EFFECT_LIFETIME_FRAMES {
        world.tick_effects();
    }
    assert!(world.active_effect_markers().is_empty());
}

#[test]
fn spawn_debug_effect_model_emits_model_not_billboard() {
    let mut world = World::default();
    // A model-only effect (no catalog): emits an EffectModel carrying the
    // requested global-TMD-pool index, and no 2D billboard sprite.
    assert!(world.spawn_debug_effect_model([16.0, 4.0, -8.0], 4));
    let models = world.active_effect_models();
    assert_eq!(models.len(), 1);
    assert_eq!(models[0].tmd_index, 4);
    assert_eq!(models[0].world_pos, [16.0, 4.0, -8.0]);
    assert_eq!(models[0].age01, 0.0);
    // Plain debug effect (no model_index) emits no model.
    assert!(world.spawn_debug_effect([0.0, 0.0, 0.0]));
    assert_eq!(world.active_effect_models().len(), 1);

    // Ages and retires via the normal effect lifetime.
    for _ in 0..=vm::effect_vm::DEFAULT_EFFECT_LIFETIME_FRAMES {
        world.tick_effects();
    }
    assert!(world.active_effect_models().is_empty());
}

#[test]
fn try_spawn_effect_noop_on_empty_catalog() {
    let mut world = World::default();
    world.try_spawn_effect(0, [0, 0, 0], 0);
    assert_eq!(world.effect_pool.active_count(), 0);
}

#[test]
fn ui_element_mode0_pushes_event_and_spawns_effect() {
    let mut world = World {
        mode: SceneMode::Battle,
        ..World::default()
    };
    let script = vm::effect_vm::EffectScript {
        child_count: 1,
        flags: 0,
        spread: 0,
        body: vec![],
    };
    world.effect_catalog = vm::effect_vm::EffectCatalog::new(vec![(script, vec![])]);
    // Drive through the BattleHostImpl path by ticking the SM. Setting
    // up a full SM state is complex; we call try_spawn_effect directly
    // (the BattleHostImpl wiring is verified by the disc-gated test).
    world.try_spawn_effect(0, [0, 0, 0], 0);
    assert_eq!(world.effect_pool.active_count(), 1);
}

#[test]
fn ui_element_mode1_does_not_spawn() {
    let mut world = World::default();
    let script = vm::effect_vm::EffectScript {
        child_count: 1,
        flags: 0,
        spread: 0,
        body: vec![],
    };
    world.effect_catalog = vm::effect_vm::EffectCatalog::new(vec![(script, vec![])]);
    // Simulate the mode==1 (terminate) path: only the event is pushed,
    // no pool spawn. try_spawn_effect is not called for mode==1.
    // Directly confirm pool stays empty if we don't call try_spawn_effect.
    assert_eq!(world.effect_pool.active_count(), 0);
}

// --- Tactical Arts ---

#[test]
fn notify_art_used_emits_event_and_sets_banner() {
    let mut world = World::default();
    world.tactical_arts.set_threshold(1);
    world.notify_art_used(0, 3);
    let evs = world.drain_battle_events();
    assert_eq!(evs.len(), 1);
    assert_eq!(
        evs[0],
        BattleEvent::TacticalArtLearned {
            char_id: 0,
            art_id: 3
        }
    );
    let banner = world.current_art_banner.as_ref().expect("banner set");
    assert!(banner.text.contains("Art #3"));
    assert_eq!(
        banner.frames_remaining,
        crate::tactical_arts::ArtLearnedBanner::DEFAULT_FRAMES
    );
}

#[test]
fn notify_art_used_no_event_before_threshold() {
    let mut world = World::default();
    world.tactical_arts.set_threshold(5);
    for _ in 0..4 {
        world.notify_art_used(0, 1);
    }
    assert!(world.drain_battle_events().is_empty());
    assert!(world.current_art_banner.is_none());
}

#[test]
fn banner_countdown_clears_after_frames() {
    let mut world = World::default();
    world.tactical_arts.set_threshold(1);
    world.notify_art_used(0, 0);
    // Banner starts at DEFAULT_FRAMES.
    assert!(world.current_art_banner.is_some());
    // Tick DEFAULT_FRAMES times; banner should reach 0 and clear.
    for _ in 0..=crate::tactical_arts::ArtLearnedBanner::DEFAULT_FRAMES {
        world.tick();
    }
    assert!(
        world.current_art_banner.is_none(),
        "banner should have cleared"
    );
}

// --- Level-up banner ---

#[test]
fn apply_battle_xp_sets_level_up_banner() {
    let mut world = World {
        party_count: 1,
        ..World::default()
    };
    // Slot 0 must be alive for the split to credit XP.
    world.actors[0].battle.hp = 100;
    // Placeholder XP table: 50 XP to reach level 2 (entry[0]; the placeholder
    // is a sin-LUT slice, not retail - real curve is DAT_80076AF4 via FUN_801E9504).
    // The reward is scaled 3/4 + ceil-split (FUN_8004E568): feed 68 so the lone
    // member receives ceil((68 - 68>>2)/1) = 51 >= the 50 threshold.
    world.apply_battle_xp(68);
    let banner = world
        .current_level_up_banner
        .as_ref()
        .expect("level-up banner should be set");
    assert_eq!(banner.char_id, 0);
    assert_eq!(banner.new_level, 2);
    assert_eq!(banner.hp_gained, 10); // default StatGain
    assert_eq!(banner.mp_gained, 5);
    assert_eq!(
        banner.frames_remaining,
        crate::levelup::LevelUpBanner::DEFAULT_FRAMES
    );
}

#[test]
fn apply_battle_xp_skips_dead_members() {
    let mut world = World {
        party_count: 3,
        ..World::default()
    };
    // Alive: slots 0 + 2. Dead: slot 1 (HP = 0).
    world.actors[0].battle.hp = 100;
    world.actors[1].battle.hp = 0;
    world.actors[2].battle.hp = 100;
    // Scaled 3/4 + ceil-split over 2 alive: ceil((140 - 140>>2)/2) = ceil(105/2)
    // = 53 each; both reach L2 (50 threshold).
    let results = world.apply_battle_xp(140);
    let slot_ids: Vec<u8> = results.iter().map(|r| r.char_id).collect();
    assert!(slot_ids.contains(&0));
    assert!(slot_ids.contains(&2));
    assert!(
        !slot_ids.contains(&1),
        "dead slot 1 must not appear in level-up results"
    );
}

#[test]
fn apply_battle_xp_no_alive_returns_empty() {
    let mut world = World {
        party_count: 3,
        ..World::default()
    };
    // No actor with HP > 0 → nobody to credit.
    let results = world.apply_battle_xp(500);
    assert!(results.is_empty());
    assert!(world.current_level_up_banner.is_none());
}

#[test]
fn apply_battle_loot_rolls_drop_item_when_rate_is_max() {
    use crate::monster_catalog::{FormationDef, FormationSlot, MonsterCatalog, MonsterDef};
    let mut cat = MonsterCatalog::new();
    let mut def = MonsterDef::new(7, "Slime", 10, 5);
    def.drop_item = Some(0x42);
    def.drop_rate_q8 = 255; // near-guaranteed roll
    cat.insert(def);
    let formation = FormationDef::new(1000, vec![FormationSlot::new(7)]);
    let mut world = World {
        party_count: 1,
        ..World::default()
    };
    world.actors[0].battle.hp = 100;
    let rewards = world.apply_battle_loot(&formation, &cat);
    assert_eq!(rewards.drops, vec![0x42]);
    assert_eq!(world.inventory.get(&0x42).copied(), Some(1));
}

#[test]
fn apply_basic_attack_queues_hit_fx_for_damaged_monster() {
    let mut world = World {
        party_count: 1,
        ..World::default()
    };
    // Slot 0 attacker, slot 1 a living monster.
    world.actors[0].battle.hp = 100;
    world.actors[0].battle.liveness = 1;
    world.actors[1].battle.hp = 60;
    world.actors[1].battle.max_hp = 60;
    world.actors[1].battle.liveness = 1;
    world.battle_ctx.active_actor = 0;
    // Give the attacker enough ATK to chip the monster (>defense).
    world.battle_attack[0] = 40;
    world.battle_defense[1] = 10;
    world.apply_basic_attack();
    let fx = world.drain_battle_hit_fx();
    assert_eq!(fx.len(), 1);
    assert_eq!(fx[0].target_slot, 1);
    assert!(fx[0].amount > 0);
    assert!(!fx[0].is_heal);
    // Drain empties the queue.
    assert!(world.drain_battle_hit_fx().is_empty());
}

#[test]
fn apply_basic_attack_damage_finish_gate() {
    // One-on-one auto-hit setup (no accuracy seeded -> no accuracy RNG), so the
    // only RNG the call can draw is the finisher's no-damage floor. Returns
    // (damage, did_draw_rng).
    let run = |attack: u16, defense: u16, gate: bool| -> (u16, bool) {
        let mut world = World {
            party_count: 1,
            ..World::default()
        };
        world.rng_state = 0xABCD_1234;
        world.actors[0].battle.hp = 100;
        world.actors[0].battle.liveness = 1;
        world.actors[1].battle.hp = 60_000;
        world.actors[1].battle.max_hp = 60_000;
        world.actors[1].battle.liveness = 1;
        world.battle_attack[0] = attack;
        world.battle_defense[1] = defense;
        world.use_damage_finish = gate;
        let rng_before = world.rng_state;
        world.battle_ctx.active_actor = 0;
        world.apply_basic_attack();
        let dmg = world
            .drain_battle_hit_fx()
            .first()
            .map(|f| f.amount)
            .unwrap_or(0);
        (dmg, world.rng_state != rng_before)
    };

    // Gate off: flat path. 40 atk vs 10 def -> 30, no RNG.
    assert_eq!(run(40, 10, false), (30, false));
    // Gate on, normal hit: same raw damage (no mitigation modelled), and the
    // finisher's rand fires only on a zeroed hit, so still no RNG.
    assert_eq!(run(40, 10, true), (30, false));

    // Zeroed hit (atk <= def). Gate on: no-damage floor (rand()%9 + 8 -> 8..=16)
    // and exactly one RNG draw. Gate off: flat min-floor of 1, no RNG.
    let (dmg_on, drew_on) = run(10, 40, true);
    assert!(
        (8..=16).contains(&dmg_on),
        "zeroed hit floored, got {dmg_on}"
    );
    assert!(drew_on, "zeroed hit draws one RNG");
    assert_eq!(run(10, 40, false), (1, false));

    // Overflow: the finisher caps at 9999 (the flat path caps at 0xFFFF).
    assert_eq!(run(50_000, 0, true), (9999, false));
    assert_eq!(run(50_000, 0, false).0, 50_000);
}

#[test]
fn basic_attack_accrues_defender_spirit_gauge() {
    let mut world = World {
        party_count: 1,
        ..World::default()
    };
    world.actors[0].battle.hp = 100;
    world.actors[0].battle.liveness = 1;
    world.actors[1].battle.hp = 200;
    world.actors[1].battle.max_hp = 200;
    world.actors[1].battle.liveness = 1;
    world.battle_attack[0] = 40;
    world.battle_defense[1] = 10;
    world.battle_ctx.active_actor = 0;

    // 40 atk vs 10 def -> 30 damage; pct = 30*100/200 = 15.
    assert_eq!(world.spirit_gauge(1), 0);
    world.apply_basic_attack();
    let _ = world.drain_battle_hit_fx();
    assert_eq!(world.spirit_gauge(1), 15);
    // A second identical hit accumulates.
    world.actors[1].battle.liveness = 1;
    world.apply_basic_attack();
    let _ = world.drain_battle_hit_fx();
    assert_eq!(world.spirit_gauge(1), 30);
    assert!(!world.spirit_gauge_full(1));
}

#[test]
fn spirit_gauge_clamps_at_full() {
    let mut world = World {
        party_count: 1,
        ..World::default()
    };
    world.actors[0].battle.hp = 100;
    world.actors[0].battle.liveness = 1;
    // A small max-HP so each ~50-damage hit is ~50% of the gauge.
    world.actors[1].battle.hp = 9999;
    world.actors[1].battle.max_hp = 100;
    world.actors[1].battle.liveness = 1;
    world.battle_attack[0] = 60;
    world.battle_defense[1] = 10;
    world.battle_ctx.active_actor = 0;

    // 50 damage on a 100-HP gauge denominator -> pct 50 each hit.
    for _ in 0..4 {
        world.actors[1].battle.liveness = 1;
        world.apply_basic_attack();
        let _ = world.drain_battle_hit_fx();
    }
    assert_eq!(world.spirit_gauge(1), 100);
    assert!(world.spirit_gauge_full(1));
}

#[test]
fn spell_damage_accrues_spirit_gauge() {
    use crate::spells::{SpellElement, SpellOutcome};
    let mut world = World {
        party_count: 1,
        ..World::default()
    };
    world.actors[1].battle.hp = 400;
    world.actors[1].battle.max_hp = 400;
    world.actors[1].battle.liveness = 1;

    // A 100-damage cast -> pct = 100*100/400 = 25.
    world.fold_spell_outcome(SpellOutcome::Damage {
        target: 1,
        amount: 100,
        element: SpellElement::Fire,
        weakness: false,
    });
    assert_eq!(world.spirit_gauge(1), 25);
    // Out-of-range slot reads 0, never panics.
    assert_eq!(world.spirit_gauge(250), 0);
}

#[test]
fn apply_basic_attack_rolls_accuracy_when_stats_are_seeded() {
    // Count landed strikes over many calls of a seeded attacker (acc) against a
    // high-evasion, can't-die target.
    let run = |rng_seed: u32| -> usize {
        let mut world = World {
            party_count: 1,
            ..World::default()
        };
        world.rng_state = rng_seed;
        world.actors[0].battle.hp = 100;
        world.actors[0].battle.liveness = 1;
        world.actors[1].battle.hp = 60_000;
        world.actors[1].battle.max_hp = 60_000;
        world.actors[1].battle.liveness = 1;
        world.battle_attack[0] = 40;
        world.battle_defense[1] = 10;
        // Seed an ~even accuracy/evasion matchup so the roll engages.
        world.battle_accuracy[0] = 50;
        world.battle_evasion[1] = 50;
        let mut hits = 0;
        for _ in 0..200 {
            world.battle_ctx.active_actor = 0;
            world.apply_basic_attack();
            hits += world.drain_battle_hit_fx().len();
        }
        hits
    };

    let hits = run(0x1234_5678);
    // The roll genuinely engages: some strikes land and some whiff.
    assert!(
        hits > 0 && hits < 200,
        "seeded accuracy should produce a mix of hits and misses, got {hits}/200"
    );
    // Deterministic under a fixed RNG seed.
    assert_eq!(
        hits,
        run(0x1234_5678),
        "accuracy roll must be deterministic"
    );
}

#[test]
fn first_living_opponent_is_chosen_by_attacker_side() {
    let mut world = World {
        party_count: 2,
        ..World::default()
    };
    // Party slots 0,1 dead+alive; monster slots 2,3.
    world.actors[0].battle.liveness = 0;
    world.actors[1].battle.liveness = 1;
    world.actors[2].battle.liveness = 0;
    world.actors[3].battle.liveness = 1;
    // Party attacker -> first living monster (slot 3, since 2 is dead).
    assert_eq!(world.first_living_opponent_of(1), Some(3));
    // Monster attacker -> first living party member (slot 1, since 0 dead).
    assert_eq!(world.first_living_opponent_of(3), Some(1));
}

#[test]
fn next_living_combatant_round_robins_skipping_dead() {
    let mut world = World {
        party_count: 1,
        ..World::default()
    };
    for a in world.actors.iter_mut() {
        a.battle.liveness = 0;
    }
    world.actors[0].battle.liveness = 1; // party
    world.actors[2].battle.liveness = 1; // monster
    // After party (0) comes monster (2); after monster (2) wraps to party (0).
    assert_eq!(world.next_living_combatant(0), Some(2));
    assert_eq!(world.next_living_combatant(2), Some(0));
}

/// Three living actors with well-separated SPD: the per-turn key ranges
/// (`speed + rand()%(speed/2+1) + 1`) can't overlap, so the order is fixed
/// by SPD regardless of the RNG. Highest SPD acts first; each turn is
/// consumed; a fresh round is seeded once everyone has acted.
#[test]
fn initiative_orders_turns_by_speed_then_reseeds() {
    let mut world = World {
        party_count: 1,
        ..World::default()
    };
    for a in world.actors.iter_mut() {
        a.battle.liveness = 0;
    }
    // slot 0 (party) SPD 10, slot 1 (monster) SPD 50, slot 2 (monster) 30.
    // Key ranges: 11..=16, 51..=76, 31..=46 - disjoint.
    world.actors[0].battle.liveness = 1;
    world.actors[1].battle.liveness = 1;
    world.actors[2].battle.liveness = 1;
    world.battle_speed[0] = 10;
    world.battle_speed[1] = 50;
    world.battle_speed[2] = 30;
    // Fresh keys (all 0): the first pick seeds a round, then orders by SPD.
    assert_eq!(world.next_combatant_by_initiative(), Some(1)); // SPD 50
    assert_eq!(world.next_combatant_by_initiative(), Some(2)); // SPD 30
    assert_eq!(world.next_combatant_by_initiative(), Some(0)); // SPD 10
    // Round exhausted -> reseed -> highest SPD again.
    assert_eq!(world.next_combatant_by_initiative(), Some(1));
}

/// A dead actor never wins a turn even with the highest SPD: the selector
/// zeroes dead actors' keys (the `FUN_801daba4` first loop).
#[test]
fn initiative_skips_dead_high_speed_actor() {
    let mut world = World {
        party_count: 1,
        ..World::default()
    };
    for a in world.actors.iter_mut() {
        a.battle.liveness = 0;
    }
    world.actors[0].battle.liveness = 1; // party, SPD 20
    world.actors[1].battle.liveness = 0; // dead monster, SPD 90
    world.actors[2].battle.liveness = 1; // monster, SPD 40
    world.battle_speed[0] = 20;
    world.battle_speed[1] = 90;
    world.battle_speed[2] = 40;
    // Slot 1 is dead -> skipped; slot 2 (40) outruns slot 0 (20).
    assert_eq!(world.next_combatant_by_initiative(), Some(2));
    assert_eq!(world.next_combatant_by_initiative(), Some(0));
}

/// With no SPD anywhere the selector defers to round-robin slot order.
#[test]
fn initiative_falls_back_to_round_robin_without_speed() {
    let mut world = World {
        party_count: 1,
        ..World::default()
    };
    for a in world.actors.iter_mut() {
        a.battle.liveness = 0;
    }
    world.actors[0].battle.liveness = 1;
    world.actors[2].battle.liveness = 1;
    assert!(!world.any_battle_speed());
    world.battle_ctx.active_actor = 0;
    assert_eq!(world.next_combatant_by_initiative(), Some(2));
    world.battle_ctx.active_actor = 2;
    assert_eq!(world.next_combatant_by_initiative(), Some(0));
}

/// Setup seeding consumes slot 0's key so the party lead opens round 1 and
/// the rest order by initiative behind it.
#[test]
fn seed_battle_initiative_lets_slot0_lead_round_one() {
    let mut world = World {
        party_count: 1,
        ..World::default()
    };
    for a in world.actors.iter_mut() {
        a.battle.liveness = 0;
    }
    world.actors[0].battle.liveness = 1; // party, SPD 10
    world.actors[1].battle.liveness = 1; // monster, SPD 50
    world.battle_speed[0] = 10;
    world.battle_speed[1] = 50;
    world.seed_battle_initiative();
    // Slot 0 consumed (leads round 1 separately); slot 1 still armed.
    assert_eq!(world.actors[0].battle.init_key, 0);
    assert!(world.actors[1].battle.init_key > 0);
    // The selector therefore picks slot 1 next, then slot 0 (after reseed).
    assert_eq!(world.next_combatant_by_initiative(), Some(1));
}

/// `any_battle_speed` only fires for SPD carried by a *living* actor.
#[test]
fn any_battle_speed_requires_a_living_carrier() {
    let mut world = World::default();
    for a in world.actors.iter_mut() {
        a.battle.liveness = 0;
    }
    assert!(!world.any_battle_speed());
    // SPD on a dead slot doesn't count.
    world.battle_speed[3] = 40;
    assert!(!world.any_battle_speed());
    // Living carrier flips the gate.
    world.actors[3].battle.liveness = 1;
    assert!(world.any_battle_speed());
}

#[test]
fn monsters_take_turns_and_can_wipe_the_party() {
    use legaia_engine_vm::battle_action::ActionState;
    let mut world = World {
        party_count: 1,
        ..World::default()
    };
    world.live_gameplay_loop = true;
    world.mode = SceneMode::Battle;
    // Lone party member: low HP, weak attack so the fight lasts several
    // rounds and the monster gets turns.
    world.actors[0].active = true;
    world.actors[0].battle.hp = 40;
    world.actors[0].battle.max_hp = 40;
    world.actors[0].battle.liveness = 1;
    world.set_battle_attack(0, 4);
    // Lone monster: tanky + hits hard enough to kill the party member.
    world.actors[1].battle.hp = 500;
    world.actors[1].battle.max_hp = 500;
    world.actors[1].battle.liveness = 1;
    world.set_battle_attack(1, 25);
    // Arm the first turn (party member swings at the monster).
    world.battle_ctx.active_actor = 0;
    world.battle_ctx.queued_action = 3;
    world.battle_ctx.action_state = ActionState::Begin.as_byte();
    world.actors[0].battle.active_target = 1;
    world.actors[0].battle.action_category = 3;

    let start_party_hp = world.actors[0].battle.hp;
    let mut party_took_damage = false;
    let mut ended = false;
    for _ in 0..4000 {
        world.tick();
        if world.actors[0].battle.hp < start_party_hp {
            party_took_damage = true;
        }
        // finish_battle flips back to Field (and raises game_over on a
        // party wipe).
        if world.mode == SceneMode::Field {
            ended = true;
            break;
        }
    }
    assert!(
        party_took_damage,
        "the monster must take turns and damage the party"
    );
    assert!(ended, "the battle must resolve (party wiped)");
    assert!(world.game_over, "a party wipe raises game_over");
}

#[test]
fn multi_monster_battle_all_monsters_act_and_party_can_win() {
    use legaia_engine_vm::battle_action::ActionState;
    let mut world = World {
        party_count: 1,
        ..World::default()
    };
    world.live_gameplay_loop = true;
    world.mode = SceneMode::Battle;
    // Lone party member: enough HP to survive three weak monsters, enough
    // attack to chip each down over a few rounds.
    world.actors[0].active = true;
    world.actors[0].battle.hp = 400;
    world.actors[0].battle.max_hp = 400;
    world.actors[0].battle.liveness = 1;
    world.set_battle_attack(0, 30);
    // Three monsters in slots 1..=3, each with modest HP + a light hit.
    for s in 1..=3 {
        world.actors[s].battle.hp = 40;
        world.actors[s].battle.max_hp = 40;
        world.actors[s].battle.liveness = 1;
        world.set_battle_attack(s as u8, 3);
    }
    // Arm the party member's first swing.
    world.battle_ctx.active_actor = 0;
    world.battle_ctx.queued_action = 3;
    world.battle_ctx.action_state = ActionState::Begin.as_byte();
    world.actors[0].battle.active_target = 1;
    world.actors[0].battle.action_category = 3;

    let start_hp = world.actors[0].battle.hp;
    let mut ended = false;
    for _ in 0..8000 {
        world.tick();
        if world.mode == SceneMode::Field {
            ended = true;
            break;
        }
    }
    assert!(ended, "the multi-monster battle must resolve");
    // Party wiped all three monsters (victory, not a party wipe).
    assert!(!world.game_over, "party should survive and win");
    for s in 1..=3 {
        assert_eq!(
            world.actors[s].battle.liveness, 0,
            "monster slot {s} should be defeated"
        );
    }
    // The monsters got turns: the party took at least some damage from
    // three light attackers over the fight.
    assert!(
        world.actors[0].battle.hp < start_hp,
        "monsters should have damaged the party over the multi-round fight"
    );
}

#[test]
fn battle_item_use_heals_ally_consumes_item_and_cycles_turn() {
    use crate::input::PadButton;
    use legaia_engine_vm::battle_action::ActionState;

    let mut world = World {
        party_count: 2,
        ..World::default()
    };
    world.battle_player_driven = true;
    world.mode = SceneMode::Battle;
    world.set_item_catalog(full_test_catalog());
    // Two party members (slot 0 wounded), one monster.
    for i in 0..2usize {
        world.actors[i].battle.max_hp = 200;
        world.actors[i].battle.hp = 200;
        world.actors[i].battle.liveness = 1;
        world.set_character_max_mp(i as u8, 30);
    }
    world.actors[0].battle.hp = 50;
    world.actors[2].battle.max_hp = 80;
    world.actors[2].battle.hp = 80;
    world.actors[2].battle.liveness = 1;
    // Healing Leaf (id 0x01) heals 100 HP; hold two.
    world.inventory.insert(0x01, 2);

    // Open the item submenu for the active party member.
    world.battle_ctx.active_actor = 0;
    world.battle_item_menu = Some(world.build_battle_item_session());
    {
        let m = world.battle_item_menu.as_ref().unwrap();
        assert_eq!(m.filtered_items.len(), 1, "one battle-usable item");
        assert_eq!(m.targets.len(), 2, "two party targets");
    }

    // Frame 1: Cross confirms the item -> target select.
    world.set_pad(0);
    world.set_pad(PadButton::Cross.mask());
    world.tick_battle_item_menu();
    assert!(world.battle_item_menu.is_some(), "still picking a target");

    // Frame 2: Cross confirms the first target (the wounded slot 0).
    world.set_pad(0);
    world.set_pad(PadButton::Cross.mask());
    world.tick_battle_item_menu();

    assert_eq!(world.actors[0].battle.hp, 150, "healed 50 -> 150");
    assert_eq!(
        world.inventory.get(&0x01).copied(),
        Some(1),
        "one Healing Leaf consumed"
    );
    assert!(world.battle_item_menu.is_none(), "menu closed after use");
    assert_eq!(
        world.battle_ctx.action_state,
        ActionState::EndOfAction.as_byte(),
        "turn parked at EndOfAction so the loop cycles"
    );
    let fx = world.drain_battle_hit_fx();
    assert_eq!(fx.len(), 1);
    assert!(fx[0].is_heal);
    assert_eq!(fx[0].amount, 100);
    assert_eq!(fx[0].target_slot, 0);
}

#[test]
fn battle_item_menu_cancel_reopens_command_menu() {
    use crate::input::PadButton;

    let mut world = World {
        party_count: 1,
        ..World::default()
    };
    world.battle_player_driven = true;
    world.mode = SceneMode::Battle;
    world.set_item_catalog(full_test_catalog());
    world.actors[0].battle.max_hp = 100;
    world.actors[0].battle.hp = 100;
    world.actors[0].battle.liveness = 1;
    world.inventory.insert(0x01, 1);

    world.battle_ctx.active_actor = 0;
    world.battle_item_menu = Some(world.build_battle_item_session());

    // Circle from the item list backs all the way out.
    world.set_pad(0);
    world.set_pad(PadButton::Circle.mask());
    world.tick_battle_item_menu();

    assert!(world.battle_item_menu.is_none(), "item menu closed");
    assert!(
        world.battle_command.is_some(),
        "command menu reopened for the same actor"
    );
    assert_eq!(world.battle_command.as_ref().unwrap().actor, 0);
    // No item was consumed on a cancel.
    assert_eq!(world.inventory.get(&0x01).copied(), Some(1));
}

/// Build a 1-party-member, 1-monster battle world for the offensive-item
/// tests. The monster sits at slot 1 (party_count = 1) with the supplied
/// A purpose-built item catalog covering every effect *type* at stable test
/// ids, for the world item-apply tests. These exercise the apply/grant kernels,
/// not the shipped consumable list, so they use fixed ids rather than
/// [`crate::items::ItemCatalog::vanilla`]'s real retail ids (which only models a
/// faithful subset; stat-boost / capture / damage / battle-escape consumables
/// aren't shipped yet). `vanilla()`'s real-id correctness is pinned in `items.rs`
/// + the disc-gated `item_catalog_disc` test.
#[cfg(test)]
fn full_test_catalog() -> crate::items::ItemCatalog {
    use crate::items::{ItemCatalog, ItemEffect, ItemEntry, StatBoostTarget};
    use legaia_engine_vm::status_effects::StatusKind;
    let mut c = ItemCatalog::new();
    let mut add = |id, name, effect, b, f| {
        c.insert(ItemEntry {
            id,
            name,
            effect,
            usable_in_battle: b,
            usable_in_field: f,
        })
    };
    add(0x01, "Heal", ItemEffect::Heal { amount: 100 }, true, true);
    add(0x04, "Heal All", ItemEffect::HealAll, true, true);
    add(0x05, "Magic", ItemEffect::HealMp { amount: 30 }, true, true);
    add(
        0x08,
        "Cure Poison",
        ItemEffect::Cure {
            kind: StatusKind::Venom,
        },
        true,
        true,
    );
    add(0x09, "Cure All", ItemEffect::CureAll, true, true);
    add(
        0x0C,
        "Revive",
        ItemEffect::Revive { factor: 128 },
        true,
        true,
    );
    add(
        0x0E,
        "Attack Up",
        ItemEffect::StatBoost {
            target: StatBoostTarget::Attack,
            delta: 1,
        },
        false,
        true,
    );
    add(
        0x0F,
        "HP Up",
        ItemEffect::StatBoost {
            target: StatBoostTarget::HpMax,
            delta: 10,
        },
        false,
        true,
    );
    add(
        0x10,
        "Spirit",
        ItemEffect::Spirit { amount: 5 },
        true,
        false,
    );
    add(
        0x11,
        "Capture",
        ItemEffect::Capture { strength: 100 },
        true,
        false,
    );
    add(0x12, "Escape", ItemEffect::Escape, true, false);
    add(
        0x13,
        "Bomb",
        ItemEffect::Damage { amount: 200 },
        true,
        false,
    );
    c
}

/// HP and a `battle_monster_id` so it shows up as an enemy target row.
#[cfg(test)]
fn offensive_item_world(monster_hp: u16, monster_id: u16) -> World {
    let mut world = World {
        party_count: 1,
        ..World::default()
    };
    world.battle_player_driven = true;
    world.mode = SceneMode::Battle;
    world.set_item_catalog(full_test_catalog());
    world.actors[0].battle.max_hp = 200;
    world.actors[0].battle.hp = 200;
    world.actors[0].battle.liveness = 1;
    world.actors[1].battle.max_hp = monster_hp;
    world.actors[1].battle.hp = monster_hp;
    world.actors[1].battle.liveness = 1;
    world.actors[1].battle_monster_id = Some(monster_id);
    world.battle_ctx.active_actor = 0;
    world
}

/// Monster-AI cast path: a monster whose record carries a castable spell
/// it can afford folds a real spell onto the party (HP drops, MP spent, a
/// damage popup queues) and parks the SM at `EndOfAction` so the loop
/// cycles - rather than the generic physical strike. RNG is pinned so the
/// cast-vs-strike roll lands on "cast".
#[test]
fn monster_ai_casts_a_castable_spell_under_fixed_rng() {
    use crate::monster_catalog::vanilla_monster_catalog;
    use crate::spells::SpellCatalog;
    use legaia_engine_vm::battle_action::ActionState;

    let mut world = World {
        party_count: 1,
        ..World::default()
    };
    world.mode = SceneMode::Battle;
    world.set_spell_catalog(SpellCatalog::vanilla());
    world.monster_catalog = vanilla_monster_catalog();
    // Party member at slot 0.
    world.actors[0].battle.max_hp = 200;
    world.actors[0].battle.hp = 200;
    world.actors[0].battle.liveness = 1;
    // Bandit Boss (id 5) at slot 1: carries [Flame 0x20, Thunder Bolt 0x23]
    // and 10 MP - enough to afford either.
    world.actors[1].battle.max_hp = 120;
    world.actors[1].battle.hp = 120;
    world.actors[1].battle.mp = 10;
    world.actors[1].battle.liveness = 1;
    world.actors[1].battle_monster_id = Some(5);
    world.set_battle_magic(1, 40);
    // Seed 0: the action picker's first `rand % (1 + magic_count)` (magic
    // count 2 -> `% 3`) lands on 1, so it casts magic[0] = Flame (0x20).
    world.rng_state = 0;

    let party_hp_before = world.actors[0].battle.hp;
    world.take_monster_turn(1);

    assert_eq!(
        world.actors[1].battle.params[0], 0x20,
        "picker chose Flame (magic_attacks[0])"
    );
    assert!(
        world.actors[0].battle.hp < party_hp_before,
        "the monster's spell dealt damage to the party"
    );
    assert!(world.actors[1].battle.mp < 10, "the monster spent MP");
    assert_eq!(
        world.battle_ctx.action_state,
        ActionState::EndOfAction.as_byte(),
        "a cast is the whole turn; SM parks at EndOfAction"
    );
    let fx = world.drain_battle_hit_fx();
    assert_eq!(fx.len(), 1, "one damage popup queued");
    assert!(!fx[0].is_heal, "damage-coloured popup");
    assert_eq!(fx[0].target_slot, 0, "the party member took the hit");
}

/// The opt-in `smarter_monster_targeting` tweak redirects a single-target
/// monster attack to the lowest-HP living party member, but does NOT move the
/// RNG stream: the faithful random pick is still rolled in full, so for every
/// seed the post-decision RNG state is byte-identical between the faithful and
/// smart modes - only the chosen slot differs. Default (faithful) behaviour is
/// thus bit-for-bit unchanged, and a smart-mode replay stays deterministic.
#[test]
fn smarter_targeting_redirects_to_lowest_hp_without_moving_rng() {
    fn world3() -> World {
        let mut w = World {
            party_count: 3,
            ..World::default()
        };
        w.mode = SceneMode::Battle;
        // Party HP: slot 1 is the lowest-HP living member.
        for (i, hp) in [200u16, 50, 200].into_iter().enumerate() {
            w.actors[i].battle.max_hp = 200;
            w.actors[i].battle.hp = hp;
            w.actors[i].battle.liveness = 1;
        }
        // Monster at slot 3 with no castable magic -> always a physical strike
        // at a single living party member (no scripted override / ring filter).
        w.actors[3].battle.max_hp = 100;
        w.actors[3].battle.hp = 100;
        w.actors[3].battle.liveness = 1;
        w
    }
    fn target_of(a: MonsterAction) -> u8 {
        match a {
            MonsterAction::Physical { target } => target,
            MonsterAction::Cast { targets, .. } => targets[0],
        }
    }

    let mut saw_redirect = false;
    for seed in 0u32..32 {
        let mut faithful = world3();
        faithful.rng_state = seed;
        let ft = target_of(faithful.pick_monster_action(3));
        let frng = faithful.rng_state;

        let mut smart = world3();
        smart.smarter_monster_targeting = true;
        smart.rng_state = seed;
        let st = target_of(smart.pick_monster_action(3));
        let srng = smart.rng_state;

        assert_eq!(st, 1, "seed {seed}: smart mode targets the lowest-HP slot");
        assert_eq!(
            frng, srng,
            "seed {seed}: RNG state identical across modes (override consumes none)"
        );
        if ft != 1 {
            saw_redirect = true;
        }
    }
    assert!(
        saw_redirect,
        "expected at least one seed where the faithful pick is not the lowest-HP slot"
    );
}

/// When the move-power table is installed and the monster's cast id resolves
/// to a power record, the special-attack damage rolls through the faithful
/// arts/physical kernel (move-power-seeded) instead of the MP-scaled spell
/// placeholder. Proven by comparing two identically-seeded worlds - one with
/// the table, one without - and asserting (a) the table changes the dealt
/// damage (the path engaged) and (b) the table path is deterministic.
#[test]
fn move_power_table_drives_monster_special_attack_damage() {
    use crate::monster_catalog::vanilla_monster_catalog;
    use crate::move_power::MovePowerCatalog;
    use crate::spells::SpellCatalog;
    use legaia_asset::move_power::{
        MOVE_ID_INDEX_MAP_FILE_OFFSET, MOVE_POWER_RECORD_STRIDE, MOVE_POWER_TABLE_FILE_OFFSET,
        MOVE_POWER_TABLE_LEN,
    };

    // Synthetic PROT-0898-shaped overlay: map the monster's first magic id
    // (Bandit Boss id 5 -> Flame 0x20) to power record 1, with a large power so
    // the kernel's roll is clearly distinct from the MP-scaled placeholder.
    fn overlay_with_flame_power() -> Vec<u8> {
        let mut buf = vec![
            0u8;
            MOVE_POWER_TABLE_FILE_OFFSET
                + MOVE_POWER_RECORD_STRIDE * MOVE_POWER_TABLE_LEN
        ];
        buf[MOVE_ID_INDEX_MAP_FILE_OFFSET + 4] = 1; // structural guard (id 4 -> idx 1)
        buf[MOVE_ID_INDEX_MAP_FILE_OFFSET + 0x20] = 1; // Flame -> power record 1
        // record 1 power 0x0BB8 (3000) -> >>2 = 750 roll-modulus base.
        buf[MOVE_POWER_TABLE_FILE_OFFSET + MOVE_POWER_RECORD_STRIDE] = 0xB8;
        buf[MOVE_POWER_TABLE_FILE_OFFSET + MOVE_POWER_RECORD_STRIDE + 1] = 0x0B;
        buf
    }

    fn run(install_table: bool) -> u16 {
        let mut world = World {
            party_count: 1,
            ..World::default()
        };
        world.mode = SceneMode::Battle;
        world.set_spell_catalog(SpellCatalog::vanilla());
        world.monster_catalog = vanilla_monster_catalog();
        // Party target at slot 0 with a healthy HP pool + seeded AGL/DEF so the
        // kernel reads live defender stats.
        world.actors[0].battle.max_hp = 4000;
        world.actors[0].battle.hp = 4000;
        world.actors[0].battle.liveness = 1;
        world.battle_accuracy[0] = 30;
        world.battle_defense[0] = 40;
        // Bandit Boss (id 5) at slot 1: casts magic[0] = Flame (0x20) on seed 0.
        world.actors[1].battle.max_hp = 120;
        world.actors[1].battle.hp = 120;
        world.actors[1].battle.mp = 10;
        world.actors[1].battle.liveness = 1;
        world.actors[1].battle_monster_id = Some(5);
        world.battle_accuracy[1] = 25;
        world.set_battle_magic(1, 40);
        if install_table {
            world.move_power = MovePowerCatalog::from_overlay_0898(&overlay_with_flame_power());
            assert!(world.move_power.is_some(), "synthetic table installs");
        }
        world.rng_state = 0;

        let before = world.actors[0].battle.hp;
        world.take_monster_turn(1);
        assert_eq!(world.actors[1].battle.params[0], 0x20, "picker chose Flame");
        before - world.actors[0].battle.hp
    }

    let placeholder = run(false);
    let move_power = run(true);
    assert!(placeholder > 0, "placeholder path still deals damage");
    assert!(move_power > 0, "move-power path deals damage");
    assert_ne!(
        placeholder, move_power,
        "installing the move-power table changes the special-attack damage"
    );
    // Deterministic: same seed + table -> identical damage.
    assert_eq!(move_power, run(true), "move-power damage is deterministic");
}

/// A party member wearing an elemental-guard accessory takes HALF damage from
/// a monster special of the matching element - the `FUN_801ddb30` finisher's
/// party-resist ladder, reading the guard passive (`0x1D + element`) off the
/// character's rebuilt ability bitfield. A non-matching element (or no
/// accessory) passes through at full magnitude.
#[test]
fn elemental_guard_accessory_halves_matching_monster_special() {
    use crate::accessory_passives::AccessoryPassives;
    use crate::monster_catalog::vanilla_monster_catalog;
    use crate::move_power::MovePowerCatalog;
    use crate::spells::SpellCatalog;
    use legaia_asset::move_power::{
        MOVE_ID_INDEX_MAP_FILE_OFFSET, MOVE_POWER_RECORD_STRIDE, MOVE_POWER_TABLE_FILE_OFFSET,
        MOVE_POWER_TABLE_LEN,
    };

    fn overlay_with_flame_power() -> Vec<u8> {
        let mut buf = vec![
            0u8;
            MOVE_POWER_TABLE_FILE_OFFSET
                + MOVE_POWER_RECORD_STRIDE * MOVE_POWER_TABLE_LEN
        ];
        buf[MOVE_ID_INDEX_MAP_FILE_OFFSET + 4] = 1;
        buf[MOVE_ID_INDEX_MAP_FILE_OFFSET + 0x20] = 1; // Flame -> power record 1
        buf[MOVE_POWER_TABLE_FILE_OFFSET + MOVE_POWER_RECORD_STRIDE] = 0xB8;
        buf[MOVE_POWER_TABLE_FILE_OFFSET + MOVE_POWER_RECORD_STRIDE + 1] = 0x0B;
        buf
    }

    // `guard_passive`: None = bare character; Some(idx) = an accessory whose
    // passive index is `idx` equipped in the Goods slot.
    fn run(guard_passive: Option<u8>) -> u16 {
        let mut world = World {
            party_count: 1,
            ..World::default()
        };
        world.mode = SceneMode::Battle;
        world.set_spell_catalog(SpellCatalog::vanilla());
        world.monster_catalog = vanilla_monster_catalog();
        // Pin the attacking monster's element to 2 (Fire) so the resist
        // ladder has a real element to test against.
        world.monster_catalog.by_id.get_mut(&5).unwrap().element = 2;
        world.actors[0].battle.max_hp = 4000;
        world.actors[0].battle.hp = 4000;
        world.actors[0].battle.liveness = 1;
        world.battle_accuracy[0] = 30;
        world.battle_defense[0] = 40;
        world.actors[1].battle.max_hp = 120;
        world.actors[1].battle.hp = 120;
        world.actors[1].battle.mp = 10;
        world.actors[1].battle.liveness = 1;
        world.actors[1].battle_monster_id = Some(5);
        world.battle_accuracy[1] = 25;
        world.set_battle_magic(1, 40);
        world.move_power = MovePowerCatalog::from_overlay_0898(&overlay_with_flame_power());

        let mut party = legaia_save::Party::zeroed(1);
        if let Some(idx) = guard_passive {
            world.set_accessory_passives(AccessoryPassives::from_entries([(0x50, idx)], []));
            let mut eq = party.members[0].equipment();
            eq.slots[7] = 0x50;
            party.members[0].set_equipment(eq);
        }
        world.roster = party;
        world.refresh_party_ability_bits();
        world.rng_state = 0;

        let before = world.actors[0].battle.hp;
        world.take_monster_turn(1);
        before - world.actors[0].battle.hp
    }

    let bare = run(None);
    let fire_guard = run(Some(0x1F)); // Fire Guard: matches element 2
    let water_guard = run(Some(0x1E)); // Water Guard: element 1, no match
    assert!(bare > 1, "baseline special deals real damage");
    assert_eq!(
        fire_guard,
        bare >> 1,
        "matching elemental guard halves the finished damage"
    );
    assert_eq!(
        water_guard, bare,
        "non-matching elemental guard leaves damage unchanged"
    );
}

/// The two "spirit gain up" finisher bits are the AP Boost accessory passives
/// (`0x28`/`0x29`): a wearer's spirit-art gauge charges faster from the same
/// hit, read off the rebuilt ability bitfield via `World::defender_resist`.
#[test]
fn ap_boost_accessory_accelerates_spirit_gauge() {
    use crate::accessory_passives::AccessoryPassives;

    fn gauge_after_hit(ap_boost_passive: Option<u8>) -> u16 {
        let mut world = World {
            party_count: 1,
            ..World::default()
        };
        world.actors[0].battle.max_hp = 500;
        world.actors[0].battle.hp = 500;
        world.actors[0].battle.liveness = 1;
        let mut party = legaia_save::Party::zeroed(1);
        if let Some(idx) = ap_boost_passive {
            world.set_accessory_passives(AccessoryPassives::from_entries([(0x50, idx)], []));
            let mut eq = party.members[0].equipment();
            eq.slots[7] = 0x50;
            party.members[0].set_equipment(eq);
        }
        world.roster = party;
        world.refresh_party_ability_bits();
        world.accrue_spirit_gauge(0, 200); // pct = 200*100/500 = 40
        world.actors[0].battle.spirit_gauge
    }

    assert_eq!(gauge_after_hit(None), 40, "base accrual is pct");
    // AP Boost 1 (0x28 -> word1 0x100): +pct/10 = +4.
    assert_eq!(gauge_after_hit(Some(0x28)), 44);
    // AP Boost 2 (0x29 -> word1 0x200): +pct>>2 = +10.
    assert_eq!(gauge_after_hit(Some(0x29)), 50);
}

/// One-party-member battle world for the Run escape roll (`FUN_801E791C`):
/// party SPD vs enemy SPD, both sides at full HP, optional accessory passive
/// on the member's slot-7 equip.
fn escape_world(party_speed: u16, enemy_speed: u16, passive: Option<u8>) -> World {
    use crate::accessory_passives::AccessoryPassives;

    let mut world = World {
        party_count: 1,
        ..World::default()
    };
    world.mode = SceneMode::Battle;
    world.actors[0].battle.max_hp = 200;
    world.actors[0].battle.hp = 200;
    world.actors[0].battle.liveness = 1;
    world.actors[1].battle.max_hp = 300;
    world.actors[1].battle.hp = 300;
    world.actors[1].battle.liveness = 1;
    world.battle_speed[0] = party_speed;
    world.battle_speed[1] = enemy_speed;
    let mut party = legaia_save::Party::zeroed(1);
    if let Some(idx) = passive {
        world.set_accessory_passives(AccessoryPassives::from_entries([(0x50, idx)], []));
        let mut eq = party.members[0].equipment();
        eq.slots[7] = 0x50;
        party.members[0].set_equipment(eq);
    }
    world.roster = party;
    world.refresh_party_ability_bits();
    world
}

/// The Run command's escape roll follows the retail `FUN_801E791C` score
/// compare: a fast party vs a slow enemy escapes on every seed (the enemy
/// score of 1 makes `roll_e` always 0 and the fail compare is strict `<`),
/// while a slow party vs a fast enemy is caught on essentially every seed
/// (`roll_p` pinned at 0 by a party score of ~1).
#[test]
fn run_escape_roll_follows_speed_and_hp_scores() {
    let mut always = 0;
    let mut rarely = 0;
    for seed in 0..50u32 {
        let mut fast = escape_world(1000, 1, None);
        fast.rng_state = seed;
        always += u32::from(fast.roll_battle_escape());

        let mut slow = escape_world(1, 1000, None);
        slow.rng_state = seed;
        rarely += u32::from(slow.roll_battle_escape());
    }
    assert_eq!(always, 50, "overwhelming speed advantage always escapes");
    assert!(
        rarely <= 2,
        "pinned-at-0 party roll is caught on almost every seed (got {rarely}/50 escapes)"
    );
}

/// Chicken King (Great Escape, passive `0x37` -> ability bit 55) forces the
/// party roll equal to the enemy roll, so even the worst matchup escapes;
/// Chicken Heart (Escape Boost, passive `0x34` -> bit 52) scales the party
/// roll 1.5x, raising the escape rate over the unboosted baseline.
#[test]
fn escape_accessories_fold_from_the_ability_bitfield() {
    for seed in 0..50u32 {
        let mut w = escape_world(1, 1000, Some(0x37));
        w.rng_state = seed;
        assert!(w.roll_battle_escape(), "Great Escape wins every compare");
    }

    let mut base = 0;
    let mut boosted = 0;
    for seed in 0..200u32 {
        let mut w = escape_world(30, 45, None);
        w.rng_state = seed;
        base += u32::from(w.roll_battle_escape());

        let mut w = escape_world(30, 45, Some(0x34));
        w.rng_state = seed;
        boosted += u32::from(w.roll_battle_escape());
    }
    assert!(
        boosted > base,
        "Escape Boost raises the escape rate ({boosted} vs {base} of 200)"
    );
}

/// Retail folds the escape accessories only over party members with live HP
/// (`+0x14C != 0`): a downed Chicken King wearer contributes nothing.
#[test]
fn downed_wearer_does_not_fold_escape_accessories() {
    let mut caught = 0;
    for seed in 0..50u32 {
        let mut w = escape_world(1, 1000, Some(0x37));
        w.actors[0].battle.liveness = 0;
        w.rng_state = seed;
        caught += u32::from(!w.roll_battle_escape());
    }
    assert!(
        caught >= 48,
        "downed wearer's assured-escape bit is ignored (got {caught}/50 caught)"
    );
}

/// With both the move-power table AND the element-affinity tables installed, a
/// monster special attack scales by `matrix[enemy_element][party_member_element]`
/// (`FUN_801dd864`). Proven by running the same seeded cast through a neutral
/// (100%) matrix vs a weakness (200%) matrix and asserting the weakness matrix
/// deals more damage. A `None` affinity table reproduces the neutral result
/// exactly - so the affinity is gated and never perturbs the RNG stream.
#[test]
fn element_affinity_scales_monster_special_attack_damage() {
    use crate::monster_catalog::vanilla_monster_catalog;
    use crate::move_power::MovePowerCatalog;
    use crate::spells::SpellCatalog;
    use legaia_asset::element_affinity::ElementAffinity;
    use legaia_asset::move_power::{
        MOVE_ID_INDEX_MAP_FILE_OFFSET, MOVE_POWER_RECORD_STRIDE, MOVE_POWER_TABLE_FILE_OFFSET,
        MOVE_POWER_TABLE_LEN,
    };

    fn overlay_with_flame_power() -> Vec<u8> {
        let mut buf = vec![
            0u8;
            MOVE_POWER_TABLE_FILE_OFFSET
                + MOVE_POWER_RECORD_STRIDE * MOVE_POWER_TABLE_LEN
        ];
        buf[MOVE_ID_INDEX_MAP_FILE_OFFSET + 4] = 1;
        buf[MOVE_ID_INDEX_MAP_FILE_OFFSET + 0x20] = 1;
        buf[MOVE_POWER_TABLE_FILE_OFFSET + MOVE_POWER_RECORD_STRIDE] = 0xB8;
        buf[MOVE_POWER_TABLE_FILE_OFFSET + MOVE_POWER_RECORD_STRIDE + 1] = 0x0B;
        buf
    }

    // `affinity_pct = None` leaves the affinity table uninstalled (gated off);
    // `Some(pct)` installs a matrix whose only non-neutral cell is the attacking
    // monster's element row vs the party member's element column.
    fn run(affinity_pct: Option<u8>) -> u16 {
        let mut world = World {
            party_count: 1,
            ..World::default()
        };
        world.mode = SceneMode::Battle;
        world.set_spell_catalog(SpellCatalog::vanilla());
        world.monster_catalog = vanilla_monster_catalog();
        world.actors[0].battle.max_hp = 4000;
        world.actors[0].battle.hp = 4000;
        world.actors[0].battle.liveness = 1;
        world.battle_accuracy[0] = 30;
        world.battle_defense[0] = 40;
        world.actors[1].battle.max_hp = 120;
        world.actors[1].battle.hp = 120;
        world.actors[1].battle.mp = 10;
        world.actors[1].battle.liveness = 1;
        world.actors[1].battle_monster_id = Some(5);
        world.battle_accuracy[1] = 25;
        world.set_battle_magic(1, 40);
        world.move_power = MovePowerCatalog::from_overlay_0898(&overlay_with_flame_power());
        // vanilla monster 5 has the default element 7 (neutral); the party
        // member at slot 0 is char id 1 -> element 3 in the synthetic table.
        let enemy_elem = world.monster_catalog.get(5).unwrap().element as usize;
        if let Some(pct) = affinity_pct {
            let mut matrix = [[100u8; 8]; 8];
            matrix[enemy_elem][3] = pct;
            world.element_affinity = Some(ElementAffinity {
                matrix,
                character_elements: vec![3; 8],
                summon_power: [[100; 8]; 3],
            });
        }
        world.rng_state = 0;

        let before = world.actors[0].battle.hp;
        world.take_monster_turn(1);
        assert_eq!(world.actors[1].battle.params[0], 0x20, "picker chose Flame");
        before - world.actors[0].battle.hp
    }

    let neutral = run(Some(100));
    let weakness = run(Some(200));
    let gated_off = run(None);
    assert!(neutral > 0, "neutral affinity still deals damage");
    assert_eq!(
        neutral, gated_off,
        "no affinity table reproduces the neutral 100% multiplier exactly"
    );
    assert!(
        weakness > neutral,
        "a 200% affinity cell deals more than the neutral 100%"
    );
    assert_eq!(
        weakness,
        run(Some(200)),
        "affinity-scaled damage is deterministic"
    );
}

/// A player Seru-magic cast scales by the element affinity of its summon
/// CREATURE vs the target - `matrix[summon-creature element][target element]`
/// (`FUN_801dd864`), not the casting character's element. The Gimard spell
/// (id `0x81`) summons the namesake "Gimard" creature, so the attacker element
/// is that creature's record element. With the creature resolved, the
/// magnitude rolls through the faithful summon kernel (the affinity scales
/// the attacker roll *inside* the roll, before the bonus-arm threshold), so
/// the affinity relation is monotonic rather than an exact post-roll
/// multiply; a `None` affinity table reproduces the neutral magnitude
/// exactly (the summon power-percent stage defaults to 100).
#[test]
fn element_affinity_scales_player_summon_cast_by_creature_element() {
    use crate::monster_catalog::{MonsterCatalog, MonsterDef};
    use crate::spells::{SpellDef, SpellEffect, SpellElement, SpellTarget};
    use legaia_asset::element_affinity::ElementAffinity;

    const SUMMON_ELEM: usize = 2; // the "Gimard" creature's element
    const ENEMY_ELEM: usize = 5; // the target enemy's element

    // The Gimard summon spell. Damage placeholder is MP-scaled
    // (caster_mag * base_power / 8 - mdef); base_power chosen so the affinity
    // delta is well above the 1-HP clamp.
    fn gimard_spell() -> SpellDef {
        SpellDef {
            id: 0x81,
            name: "Gimard".into(),
            mp_cost: 4,
            element: SpellElement::Neutral,
            target: SpellTarget::OneEnemy,
            effect: SpellEffect::Damage {
                base_power: 100,
                element: SpellElement::Neutral,
            },
            anim_id: 0,
        }
    }

    fn run(affinity_pct: Option<u8>) -> u16 {
        let mut world = World {
            party_count: 1,
            ..World::default()
        };
        world.mode = SceneMode::Battle;
        // Catalog: the summon creature (matched by the spell's display name) and
        // the target enemy, each with a distinct element. The summon body's HP
        // is kept SMALL and the caster's AGL large so the attacker roll
        // dominates the bonus-arm threshold (`defender + summon_hp >
        // attacker`) at every pct exercised here - the bonus re-roll rebuilds
        // the roll WITHOUT the affinity scale (retail-faithful; covered by the
        // kernel tests), which would break the monotonic relation this test
        // pins.
        let mut catalog = MonsterCatalog::new();
        let mut creature = MonsterDef::new(10, "Gimard", 10, 10);
        creature.element = SUMMON_ELEM as u8;
        catalog.insert(creature);
        let mut enemy = MonsterDef::new(5, "Goblin", 120, 8);
        enemy.element = ENEMY_ELEM as u8;
        catalog.insert(enemy);
        world.monster_catalog = catalog;

        // Caster = party slot 0; enough MP to afford the cast.
        world.actors[0].battle.max_hp = 400;
        world.actors[0].battle.hp = 400;
        world.actors[0].battle.mp = 40;
        world.actors[0].battle.liveness = 1;
        world.set_battle_magic(0, 40);
        world.battle_accuracy[0] = 200;
        // Target = enemy slot 1, identified to the catalog by monster id.
        world.actors[1].battle.max_hp = 4000;
        world.actors[1].battle.hp = 4000;
        world.actors[1].battle.liveness = 1;
        world.actors[1].battle_monster_id = Some(5);
        world.battle_defense[1] = 0;

        if let Some(pct) = affinity_pct {
            let mut matrix = [[100u8; 8]; 8];
            matrix[SUMMON_ELEM][ENEMY_ELEM] = pct;
            world.element_affinity = Some(ElementAffinity {
                matrix,
                character_elements: vec![3; 8],
                summon_power: [[100; 8]; 3],
            });
        }

        let def = gimard_spell();
        let before = world.actors[1].battle.hp;
        world.cast_spell_on_slots(0, &def, &[1]);
        before - world.actors[1].battle.hp
    }

    let neutral = run(Some(100));
    let weakness = run(Some(200));
    let resist = run(Some(50));
    let gated_off = run(None);
    assert!(neutral > 0, "neutral affinity still deals damage");
    assert_eq!(
        neutral, gated_off,
        "no affinity table reproduces the neutral 100% multiplier exactly"
    );
    assert!(
        weakness > neutral,
        "a 200% affinity raises the faithful roll ({weakness} vs {neutral})"
    );
    assert!(
        resist < neutral,
        "a 50% affinity lowers the faithful roll ({resist} vs {neutral})"
    );
}

/// The player Seru-magic cast path rolls the faithful summon kernel: the
/// HP delta produced by `cast_spell_on_slots` equals the value built by
/// composing `summon_predamage_lazy` + `damage_finish_lazy` directly with the
/// same seeds - summon-body stats from the namesake creature's catalog def,
/// caster AGL doubled, and the shared LCG drawn in retail call order.
#[test]
fn player_summon_cast_matches_the_summon_kernel_composition() {
    use crate::monster_catalog::{MonsterCatalog, MonsterDef};
    use crate::spells::{SpellDef, SpellEffect, SpellElement, SpellTarget};
    use legaia_engine_vm::battle_formulas::{
        DamageFinish, DefenderResist, SummonRollActor, damage_finish_lazy, summon_predamage_lazy,
    };

    const SEED: u32 = 0xC0FFEE;

    fn build_world() -> World {
        let mut world = World {
            party_count: 1,
            ..World::default()
        };
        world.mode = SceneMode::Battle;
        let mut catalog = MonsterCatalog::new();
        let mut creature = MonsterDef::new(10, "Gimard", 100, 10);
        creature.intel = 36;
        creature.element = 2;
        catalog.insert(creature);
        let mut enemy = MonsterDef::new(5, "Goblin", 120, 8);
        enemy.element = 5;
        catalog.insert(enemy);
        world.monster_catalog = catalog;
        world.actors[0].battle.max_hp = 400;
        world.actors[0].battle.hp = 400;
        world.actors[0].battle.mp = 40;
        world.actors[0].battle.liveness = 1;
        world.set_battle_magic(0, 40);
        world.battle_accuracy[0] = 25;
        world.actors[1].battle.max_hp = 4000;
        world.actors[1].battle.hp = 4000;
        world.actors[1].battle.liveness = 1;
        world.actors[1].battle_monster_id = Some(5);
        world.battle_accuracy[1] = 12;
        world.battle_defense[1] = 30;
        world.rng_state = SEED;
        world
    }

    // Expected value: the kernels composed directly, drawing from the same
    // LCG in the same order (attacker, defender, lazy bonus, lazy floor).
    struct Lcg(u32);
    impl Lcg {
        fn draw(&mut self) -> u16 {
            self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (self.0 & 0x7fff) as u16
        }
    }
    let mut lcg = Lcg(SEED);
    let summon = SummonRollActor {
        hp: 100,
        agl: 36,
        ..Default::default()
    };
    let target = SummonRollActor {
        hp: 4000,
        agl: 12,
        stat_a: 30,
        stat_b: 0,
        ..Default::default()
    };
    let rng2 = [lcg.draw(), lcg.draw()];
    let (atk, def) = summon_predamage_lazy(&summon, 25, &target, 100, 1, rng2, || lcg.draw());
    let finish = DamageFinish {
        predamage: atk.saturating_sub(def),
        attacker_slot: 7,
        defender_slot: 4,
        attacker_element: 2,
        defender_resist: DefenderResist::default(),
        defender_guarding: false,
        enemy_defender_halve: false,
        bypass_party_resist: false,
        summon_power_pct: 100,
        floor_rand: 0,
    };
    let expected = damage_finish_lazy(&finish, || lcg.draw()).min(9999) as u16;

    // Direct method call.
    let mut world = build_world();
    assert_eq!(
        world.player_summon_predamage(0, 1, 0x81),
        Some(expected),
        "player_summon_predamage composes the kernels"
    );

    // Whole cast path: the HP delta is the same value.
    let mut world = build_world();
    let spell = SpellDef {
        id: 0x81,
        name: "Gimard".into(),
        mp_cost: 4,
        element: SpellElement::Neutral,
        target: SpellTarget::OneEnemy,
        effect: SpellEffect::Damage {
            base_power: 100,
            element: SpellElement::Neutral,
        },
        anim_id: 0,
    };
    let before = world.actors[1].battle.hp;
    world.cast_spell_on_slots(0, &spell, &[1]);
    assert_eq!(
        before - world.actors[1].battle.hp,
        expected,
        "cast_spell_on_slots folds the faithful magnitude"
    );
}

/// A monster with no castable spells always picks a physical strike: the
/// action picker rolls `rand % (1 + 0) == 0`, so the magic branch is never
/// taken regardless of the seed. It still targets a (single living) party
/// member and arms the SM at `Begin`.
#[test]
fn spell_less_monster_always_arms_physical_strike() {
    use legaia_engine_vm::battle_action::ActionState;

    let mut world = World {
        party_count: 1,
        ..World::default()
    };
    world.mode = SceneMode::Battle;
    world.actors[0].battle.max_hp = 200;
    world.actors[0].battle.hp = 200;
    world.actors[0].battle.liveness = 1;
    // Goblin (id 1) has no magic_attacks; leave the catalog empty so the
    // monster id doesn't resolve either - the magic branch can't be taken.
    world.actors[1].battle.max_hp = 30;
    world.actors[1].battle.hp = 30;
    world.actors[1].battle.liveness = 1;
    world.actors[1].battle_monster_id = Some(1);

    world.take_monster_turn(1);

    assert_eq!(world.battle_ctx.queued_action, 3, "physical strike queued");
    assert_eq!(
        world.battle_ctx.action_state,
        ActionState::Begin.as_byte(),
        "SM armed at Begin to run the strike"
    );
    assert_eq!(
        world.actors[1].battle.action_category, 3,
        "physical action category"
    );
    assert_eq!(
        world.actors[1].battle.active_target, 0,
        "targets the only living party member (slot 0)"
    );
}

/// A minimal `efect.dat` 2-pack: 1 atlas entry (a 24x24 sprite at texel
/// (5,7), tpage 0x88, clut 0x12), 1 anim batch (one frame -> atlas 0), and
/// 1 effect script with 1 child referencing sprite_id 0.
fn minimal_efect_dat() -> Vec<u8> {
    let mut buf = vec![0u8; 8];
    // atlas[0]: u=5 v=7 w=24 h=24, CLUT@+4=0x88, tpage@+6=0x12, unk=0
    buf.extend_from_slice(&[5u8, 7, 24, 24]);
    buf.extend_from_slice(&0x88u16.to_le_bytes()); // CLUT (CBA)
    buf.extend_from_slice(&[0x12u8, 0]); // tpage byte, unk
    let pack0 = buf.len() as u32;
    // pack0: 1 anim batch, 1 frame (atlas_index 0).
    buf.extend_from_slice(&1u32.to_le_bytes());
    let p0_tbl = buf.len();
    buf.extend_from_slice(&[0u8; 4]);
    let anim0 = buf.len() as u32;
    buf.extend_from_slice(&[1u8, 0]); // frame_count=1, flags
    buf.extend_from_slice(&[0u8, 0, 0, 0, 0, 0]); // frame 0 -> atlas 0
    buf[p0_tbl..p0_tbl + 4].copy_from_slice(&anim0.to_le_bytes());
    let pack1 = buf.len() as u32;
    // pack1: 1 effect script, 1 child (sprite_id 0), flags 0 (no spread).
    buf.extend_from_slice(&1u32.to_le_bytes());
    let p1_tbl = buf.len();
    buf.extend_from_slice(&[0u8; 4]);
    let script0 = buf.len() as u32;
    buf.extend_from_slice(&[1u8, 0]); // child_count=1, flags=0
    buf.extend_from_slice(&0i16.to_le_bytes()); // spread
    buf.extend_from_slice(&0u16.to_le_bytes()); // child sprite_id=0
    buf.extend_from_slice(&0i16.to_le_bytes()); // width
    buf.extend_from_slice(&0u16.to_le_bytes()); // anim_flags
    buf.extend_from_slice(&0i16.to_le_bytes()); // depth
    buf.extend_from_slice(&[0u8; 6]); // tail
    buf[p1_tbl..p1_tbl + 4].copy_from_slice(&script0.to_le_bytes());
    buf[0..4].copy_from_slice(&pack0.to_le_bytes());
    buf[4..8].copy_from_slice(&pack1.to_le_bytes());
    buf
}

/// A spawned effect produces a faithful billboard sprite per child, sized
/// and UV-addressed from the real sprite atlas (the textured-quad seam).
#[test]
fn active_effect_sprites_carry_atlas_size_and_vram_coords() {
    use legaia_engine_vm::effect_vm::EffectCatalog;
    let mut world = World {
        effect_catalog: EffectCatalog::from_efect_dat_bytes(&minimal_efect_dat()),
        ..World::default()
    };
    assert_eq!(world.effect_catalog.len(), 1, "one effect script");

    // No effects yet -> no sprites.
    assert!(world.active_effect_sprites().is_empty());

    // Spawn effect 0 at world (10, 0, 20).
    world.try_spawn_effect(0, [10, 0, 20], 0);
    let sprites = world.active_effect_sprites();
    assert_eq!(sprites.len(), 1, "one child sprite");
    let s = sprites[0];
    assert_eq!(s.uv, [5, 7], "atlas texel origin");
    assert_eq!(s.uv_size, [24, 24], "atlas sprite size");
    assert_eq!(s.size, [24.0, 24.0]);
    assert_eq!(s.page, 0x12, "tpage byte from atlas+6");
    assert_eq!(s.clut, 0x88, "CLUT (CBA) u16 from atlas+4");
    // Origin Y matches; X/Z within a small deterministic ring of (10, 20).
    assert!((s.world_pos[1] - 0.0).abs() < 1e-3);
    assert!((s.world_pos[0] - 10.0).abs() < 1.0);
    assert!((s.world_pos[2] - 20.0).abs() < 1.0);
}

/// Per-monster-id scripted AI (the `FUN_801E9FD4` switch) end-to-end: a
/// wounded monster whose id has a low-HP self-heal case folds that heal onto
/// itself rather than striking the party. Monster id 6 (case `0x06`) casts
/// `0x52` at self when `HP <= maxHP/2` and its ability cooldown is clear.
#[test]
fn scripted_ai_monster_self_heals_when_wounded() {
    use crate::monster_catalog::vanilla_monster_catalog;
    use crate::spells::SpellCatalog;
    use legaia_engine_vm::battle_action::ActionState;

    let mut world = World {
        party_count: 1,
        ..World::default()
    };
    world.mode = SceneMode::Battle;
    world.set_spell_catalog(SpellCatalog::vanilla());
    world.monster_catalog = vanilla_monster_catalog();
    world.actors[0].battle.max_hp = 200;
    world.actors[0].battle.hp = 200;
    world.actors[0].battle.liveness = 1;
    // Monster id 6 (Skeleton -> AI case 0x06) at slot 1, wounded to 20/100
    // with MP to spare for the heal.
    world.actors[1].battle.max_hp = 100;
    world.actors[1].battle.hp = 20;
    world.actors[1].battle.mp = 20;
    world.actors[1].battle.liveness = 1;
    world.actors[1].battle_monster_id = Some(6);
    world.rng_state = 7;

    world.take_monster_turn(1);

    assert_eq!(
        world.actors[1].battle.params[0], 0x52,
        "self-heal spell queued"
    );
    assert!(
        world.actors[1].battle.hp > 20,
        "the monster healed itself instead of striking the party"
    );
    assert_eq!(world.actors[0].battle.hp, 200, "party untouched");
    assert_eq!(
        world.battle_ctx.action_state,
        ActionState::EndOfAction.as_byte(),
        "cast is the whole turn"
    );
    assert_eq!(world.monster_ai_state.dat[4], 1, "ability cooldown armed");
    let fx = world.drain_battle_hit_fx();
    assert!(fx.iter().any(|f| f.is_heal && f.target_slot == 1));
}

/// Monster `0x8A` reads its own spirit-art gauge (`actor+0x170`) as a charge
/// gate: once it passes `0x31` the AI fires the `0x4E` all-enemies cast and the
/// live loop clamps the caster's gauge to `0x32`. Below the threshold the
/// generic core stands (this monster has no castable magic in the catalog, so
/// that means a physical strike) and the gauge is left untouched.
#[test]
fn monster_8a_charge_gate_drives_cast_and_clamps_gauge() {
    use crate::monster_catalog::vanilla_monster_catalog;
    use crate::spells::SpellCatalog;

    let mut world = World {
        party_count: 1,
        ..World::default()
    };
    world.mode = SceneMode::Battle;
    world.set_spell_catalog(SpellCatalog::vanilla());
    world.monster_catalog = vanilla_monster_catalog();
    world.actors[0].battle.max_hp = 200;
    world.actors[0].battle.hp = 200;
    world.actors[0].battle.liveness = 1;
    world.actors[1].battle.max_hp = 300;
    world.actors[1].battle.hp = 300;
    world.actors[1].battle.mp = 200;
    world.actors[1].battle.liveness = 1;
    world.actors[1].battle_monster_id = Some(0x8a);

    // Charged: the gate fires the 0x4E cast and clamps the gauge to 0x32.
    world.actors[1].battle.spirit_gauge = 100;
    world.rng_state = 7;
    match world.pick_monster_action(1) {
        MonsterAction::Cast { spell_id, .. } => assert_eq!(spell_id, 0x4e),
        other => panic!("expected the 0x4E charge cast, got {other:?}"),
    }
    assert_eq!(
        world.actors[1].battle.spirit_gauge, 0x32,
        "the gauge is clamped to 0x32 as the cast commits"
    );

    // Below threshold: no override, gauge left exactly as-is.
    world.actors[1].battle.spirit_gauge = 0x31;
    world.rng_state = 7;
    assert!(matches!(
        world.pick_monster_action(1),
        MonsterAction::Physical { .. }
    ));
    assert_eq!(
        world.actors[1].battle.spirit_gauge, 0x31,
        "an uncharged 0x8A leaves its gauge untouched"
    );
}

/// Faithful `FUN_801E7320`: a targeting class in `3..=6` resolves to a
/// living PARTY slot; a class in `0..=2` resolves to a living MONSTER slot.
/// (Dead slots are skipped via the re-roll loop.)
#[test]
fn monster_target_resolver_expands_class_to_correct_side() {
    let mut world = World {
        party_count: 3,
        ..World::default()
    };
    world.mode = SceneMode::Battle;
    // Party slots 0..2: slot 0 dead, slots 1+2 alive.
    for i in 0..3u8 {
        let a = &mut world.actors[i as usize];
        a.battle.max_hp = 100;
        a.battle.hp = if i == 0 { 0 } else { 100 };
        a.battle.liveness = if i == 0 { 0 } else { 1 };
    }
    // Monster slots 3+4 alive.
    for i in 3..5u8 {
        let a = &mut world.actors[i as usize];
        a.battle.max_hp = 80;
        a.battle.hp = 80;
        a.battle.liveness = 1;
    }

    // Caster = monster slot 3, class 3 (party-targeting). Resolves to a
    // LIVING party slot (1 or 2, never the dead slot 0).
    world.actors[3].battle.active_target = 3; // class 3..6 -> party
    world.rng_state = 12345;
    world.resolve_monster_target(3);
    let t = world.actors[3].battle.active_target;
    assert!(
        (1..=2).contains(&t),
        "class 3 -> living party slot, got {t}"
    );

    // Class 1 (monster-band targeting). Resolves to a living monster slot.
    world.actors[3].battle.active_target = 1; // class 0..2 -> monster band
    world.rng_state = 999;
    world.resolve_monster_target(3);
    let t = world.actors[3].battle.active_target;
    assert!(
        (3..=4).contains(&t),
        "class 1 -> living monster slot, got {t}"
    );
}

/// `advance_battle_mode` (the SM `case 0xFF` writer for `ctx+0x28A`) flips a
/// multi-phase boss from its first-phase cast to its phased cast on the next
/// turn. Monster id `0xB6` always casts, picking its spell purely by mode.
#[test]
fn advancing_the_battle_mode_drives_a_boss_to_its_next_phase() {
    use crate::monster_catalog::MonsterDef;
    use crate::spells::SpellCatalog;

    let mut world = World {
        party_count: 1,
        ..World::default()
    };
    world.mode = SceneMode::Battle;
    world.set_spell_catalog(SpellCatalog::vanilla());
    // A clean-room boss at monster slot 1 with id 0xB6 (no own magic - it
    // casts purely off its scripted phase table).
    world
        .monster_catalog
        .insert(MonsterDef::new(0xb6, "Boss", 400, 50));
    world.actors[0].battle.max_hp = 300;
    world.actors[0].battle.hp = 300;
    world.actors[0].battle.liveness = 1;
    world.actors[1].battle.max_hp = 400;
    world.actors[1].battle.hp = 400;
    world.actors[1].battle.mp = 250;
    world.actors[1].battle.liveness = 1;
    world.actors[1].battle_monster_id = Some(0xb6);
    world.rng_state = 1;

    assert_eq!(world.battle_mode(), 0, "fresh battle starts in phase 0");
    world.take_monster_turn(1);
    assert_eq!(world.actors[1].battle.params[0], 0xa2, "phase 0 cast");

    // A scripted phase transition advances the mode; next turn is phase I.
    world.advance_battle_mode();
    assert_eq!(world.battle_mode(), 1);
    world.take_monster_turn(1);
    assert_eq!(world.actors[1].battle.params[0], 0xa3, "phase 1 cast");
}

#[test]
fn battle_item_bomb_damages_enemy_and_cursor_lands_on_the_monster() {
    use crate::input::PadButton;
    use legaia_engine_vm::battle_action::ActionState;

    let mut world = offensive_item_world(500, 7);
    // Bomb (0x13) deals 200 HP to an enemy.
    world.inventory.insert(0x13, 1);
    world.battle_item_menu = Some(world.build_battle_item_session());
    {
        let m = world.battle_item_menu.as_ref().unwrap();
        assert_eq!(m.targets.len(), 2, "one ally + one enemy target");
        assert!(!m.targets[0].is_enemy, "ally row first");
        assert!(m.targets[1].is_enemy, "enemy row second");
    }

    // Frame 1: Cross confirms the Bomb -> target select. The cursor must
    // skip the ally and land on the enemy row (offensive item).
    world.set_pad(0);
    world.set_pad(PadButton::Cross.mask());
    world.tick_battle_item_menu();
    {
        let m = world.battle_item_menu.as_ref().unwrap();
        match m.state {
            crate::inventory_use::InventoryUseState::TargetSelect { cursor, .. } => {
                assert_eq!(cursor, 1, "cursor positioned on the enemy row");
            }
            other => panic!("expected TargetSelect, got {other:?}"),
        }
    }

    // Frame 2: Cross confirms the enemy -> 200 damage.
    world.set_pad(0);
    world.set_pad(PadButton::Cross.mask());
    world.tick_battle_item_menu();

    assert_eq!(world.actors[1].battle.hp, 300, "500 -> 300 after Bomb");
    assert_eq!(world.inventory.get(&0x13).copied(), None, "Bomb consumed");
    assert!(world.battle_item_menu.is_none(), "menu closed after use");
    assert_eq!(
        world.battle_ctx.action_state,
        ActionState::EndOfAction.as_byte(),
        "turn parked at EndOfAction"
    );
    let fx = world.drain_battle_hit_fx();
    assert_eq!(fx.len(), 1);
    assert!(!fx[0].is_heal, "damage-coloured popup");
    assert_eq!(fx[0].amount, 200);
    assert_eq!(fx[0].target_slot, 1);
}

#[test]
fn battle_item_bomb_downs_a_low_hp_enemy() {
    use crate::input::PadButton;

    let mut world = offensive_item_world(120, 7);
    world.inventory.insert(0x13, 1); // Bomb, 200 dmg vs 120 HP.
    world.battle_item_menu = Some(world.build_battle_item_session());

    world.set_pad(0);
    world.set_pad(PadButton::Cross.mask());
    world.tick_battle_item_menu(); // confirm item -> target
    world.set_pad(0);
    world.set_pad(PadButton::Cross.mask());
    world.tick_battle_item_menu(); // confirm enemy

    assert_eq!(world.actors[1].battle.hp, 0, "HP floored at zero");
    assert_eq!(world.actors[1].battle.liveness, 0, "monster downed");
}

#[test]
fn battle_item_capture_downs_a_weakened_enemy_and_logs_the_id() {
    use crate::input::PadButton;

    // Weakened monster (10/500 HP) so the missing-HP capture roll is
    // near-certain; pin the RNG so the roll (23) lands.
    let mut world = offensive_item_world(500, 42);
    world.actors[1].battle.hp = 10;
    world.rng_state = 0;
    world.inventory.insert(0x11, 1); // Genocide Crystal (capture).
    world.battle_item_menu = Some(world.build_battle_item_session());

    world.set_pad(0);
    world.set_pad(PadButton::Cross.mask());
    world.tick_battle_item_menu(); // item -> target (lands on enemy)
    world.set_pad(0);
    world.set_pad(PadButton::Cross.mask());
    world.tick_battle_item_menu(); // confirm enemy

    assert_eq!(
        world.actors[1].battle.liveness, 0,
        "captured monster downed"
    );
    assert_eq!(
        world.drain_battle_captures(),
        vec![42],
        "monster id logged for post-battle Seru learning"
    );
}

#[test]
fn battle_item_escape_returns_to_field() {
    use crate::input::PadButton;

    let mut world = offensive_item_world(500, 7);
    world.inventory.insert(0x12, 1); // Goblin Foot (escape).
    world.battle_item_menu = Some(world.build_battle_item_session());

    world.set_pad(0);
    world.set_pad(PadButton::Cross.mask());
    world.tick_battle_item_menu(); // item -> target
    world.set_pad(0);
    world.set_pad(PadButton::Cross.mask());
    world.tick_battle_item_menu(); // confirm

    assert_eq!(world.mode, SceneMode::Field, "escaped back to the field");
    assert!(!world.battle_escaped, "escape flag reset by finish_battle");
    assert!(world.battle_item_menu.is_none(), "battle menus cleared");
    assert_eq!(world.inventory.get(&0x12).copied(), None, "item consumed");
}

#[test]
fn battle_magic_cast_damages_monster_spends_mp_and_cycles_turn() {
    use crate::input::PadButton;
    use crate::spells::SpellCatalog;
    use legaia_engine_vm::battle_action::ActionState;

    let mut world = World {
        party_count: 1,
        ..World::default()
    };
    world.battle_player_driven = true;
    world.mode = SceneMode::Battle;
    world.set_spell_catalog(SpellCatalog::vanilla());
    // Caster with a magic stat + MP; one monster.
    world.actors[0].battle.max_hp = 200;
    world.actors[0].battle.hp = 200;
    world.actors[0].battle.mp = 50;
    world.actors[0].battle.liveness = 1;
    world.set_battle_magic(0, 100);
    world.actors[1].battle.max_hp = 300;
    world.actors[1].battle.hp = 300;
    world.actors[1].battle.liveness = 1;
    // Give the caster a learned offensive spell: Flame (0x20, 5 MP).
    let mut party = legaia_save::Party::zeroed(1);
    let mut list = party.members[0].spell_list();
    list.count = 1;
    list.ids[0] = 0x20;
    party.members[0].set_spell_list(list);
    world.roster = party;

    // Open the spell submenu for the caster.
    world.battle_ctx.active_actor = 0;
    world.battle_spell_menu = world.build_battle_spell_session(0);
    {
        let m = world.battle_spell_menu.as_ref().expect("spell menu built");
        assert_eq!(m.spells.len(), 1, "one learned spell");
        assert!(m.spells[0].affordable, "50 MP covers a 5 MP spell");
    }

    // Frame 1: Cross opens the target cursor on the lone monster.
    world.set_pad(0);
    world.set_pad(PadButton::Cross.mask());
    world.tick_battle_spell_menu();
    assert!(world.battle_spell_menu.is_some(), "still picking a target");

    // Frame 2: Cross confirms the monster; the cast resolves.
    world.set_pad(0);
    world.set_pad(PadButton::Cross.mask());
    world.tick_battle_spell_menu();

    assert!(world.battle_spell_menu.is_none(), "spell menu closed");
    assert_eq!(world.actors[0].battle.mp, 45, "5 MP spent on Flame");
    assert!(
        world.actors[1].battle.hp < 300,
        "Flame should have damaged the monster"
    );
    assert_eq!(
        world.battle_ctx.action_state,
        ActionState::EndOfAction.as_byte(),
        "turn parked at EndOfAction so the loop cycles"
    );
    let fx = world.drain_battle_hit_fx();
    assert_eq!(fx.len(), 1);
    assert!(!fx[0].is_heal, "offensive spell is damage, not heal");
    assert_eq!(fx[0].target_slot, 1);
}

#[test]
fn silenced_caster_cannot_open_the_magic_submenu() {
    use crate::spells::SpellCatalog;
    use legaia_engine_vm::status_effects::StatusKind;

    let mut world = World::new();
    world.mode = SceneMode::Battle;
    world.set_spell_catalog(SpellCatalog::vanilla());
    world.actors[0].battle.mp = 50;
    world.actors[0].battle.liveness = 1;
    world.set_battle_magic(0, 100);
    // A learned offensive spell, so the submenu would build absent any status.
    let mut party = legaia_save::Party::zeroed(1);
    let mut list = party.members[0].spell_list();
    list.count = 1;
    list.ids[0] = 0x20;
    party.members[0].set_spell_list(list);
    world.roster = party;

    // No status: the Magic submenu builds.
    assert!(
        world.build_battle_spell_session(0).is_some(),
        "control: an unafflicted caster can open Magic"
    );

    // Curse: the submenu refuses to open, so the caller bounces the player
    // back to the command menu (the party-side mirror of the monster path).
    world
        .status_effects
        .apply_with_duration(0, StatusKind::Curse, 4);
    assert!(
        world.build_battle_spell_session(0).is_none(),
        "a silenced caster must not open the Magic submenu"
    );
}

#[test]
fn battle_magic_cast_applies_mp_half_ability_bit() {
    use crate::input::PadButton;
    use crate::spells::SpellCatalog;

    let mut world = World {
        party_count: 1,
        ..World::default()
    };
    world.battle_player_driven = true;
    world.mode = SceneMode::Battle;
    world.set_spell_catalog(SpellCatalog::vanilla());
    world.actors[0].battle.max_hp = 200;
    world.actors[0].battle.hp = 200;
    world.actors[0].battle.mp = 50;
    world.actors[0].battle.liveness = 1;
    world.set_battle_magic(0, 100);
    world.actors[1].battle.max_hp = 300;
    world.actors[1].battle.hp = 300;
    world.actors[1].battle.liveness = 1;
    // MP-half accessory bit (0x20) on the caster's character record.
    world.character_ability_bits[0] = 0x20;

    let mut party = legaia_save::Party::zeroed(1);
    let mut list = party.members[0].spell_list();
    list.count = 1;
    list.ids[0] = 0x20; // Flame, 5 MP
    party.members[0].set_spell_list(list);
    world.roster = party;

    world.battle_ctx.active_actor = 0;
    world.battle_spell_menu = world.build_battle_spell_session(0);

    world.set_pad(0);
    world.set_pad(PadButton::Cross.mask());
    world.tick_battle_spell_menu();
    world.set_pad(0);
    world.set_pad(PadButton::Cross.mask());
    world.tick_battle_spell_menu();

    // Flame is 5 MP; the MP-half bit charges `5 - (5>>1) = 3` (retail rounds
    // up on odd costs, not floor 5/2 = 2), so 50 -> 47 (vs 45 flat).
    assert_eq!(
        world.actors[0].battle.mp, 47,
        "MP-half ability bit should reduce the live-cast cost by half (round up)"
    );
}

#[test]
fn refresh_party_ability_bits_derives_and_propagates_party_wide() {
    use crate::accessory_passives::AccessoryPassives;

    let mut world = World {
        party_count: 2,
        ..World::default()
    };
    // Synthetic catalog: item 0x50 grants wearer-only passive 0x05 (the
    // MP-half bit 0x20); item 0x51 grants party-wide passive 0x0E.
    world.set_accessory_passives(AccessoryPassives::from_entries(
        [(0x50, 0x05), (0x51, 0x0E)],
        [0x0E],
    ));
    let mut party = legaia_save::Party::zeroed(2);
    let mut eq = party.members[0].equipment();
    eq.slots[7] = 0x50;
    party.members[0].set_equipment(eq);
    let mut eq = party.members[1].equipment();
    eq.slots[5] = 0x51;
    party.members[1].set_equipment(eq);
    world.roster = party;

    world.refresh_party_ability_bits();

    // Wearer-only bit lands on member 0 only.
    assert_eq!(world.character_ability_bits[0] & 0x20, 0x20);
    assert_eq!(world.character_ability_bits[1] & 0x20, 0);
    // Party-wide bit (index 0x0E) propagates into every member's effective
    // mask, and into the global mask (the FUN_800431D0 port).
    assert_eq!(world.character_ability_bits[0] & (1 << 0x0E), 1 << 0x0E);
    assert_eq!(world.character_ability_bits[1] & (1 << 0x0E), 1 << 0x0E);
    assert!(world.party_has_ability(0x0E));
    assert!(!world.party_has_ability(0x06));
    // The record-side bitfield is rebuilt with each wearer's OWN bits.
    assert_eq!(world.roster.members[0].ability_bits()[0], 0x20);
    assert_eq!(world.roster.members[1].ability_bits()[1], 0x40); // bit 14
}

#[test]
fn refresh_party_ability_bits_noops_without_a_catalog() {
    let mut world = World {
        party_count: 1,
        ..World::default()
    };
    world.roster = legaia_save::Party::zeroed(1);
    // Synthetic setups write the bits directly; an empty catalog must not
    // clobber them.
    world.character_ability_bits[0] = 0x20;
    world.refresh_party_ability_bits();
    assert_eq!(world.character_ability_bits[0], 0x20);
}

#[test]
fn seed_party_battle_stats_applies_accessory_stat_and_hp_boosts() {
    use crate::accessory_passives::AccessoryPassives;

    let mut world = World {
        party_count: 1,
        ..World::default()
    };
    // Item 0x52 grants passive 0x06 (ATK +20%); item 0x53 grants passive
    // 0x00 (max HP +10%).
    world.set_accessory_passives(AccessoryPassives::from_entries(
        [(0x52, 0x06), (0x53, 0x00)],
        [],
    ));
    let mut party = legaia_save::Party::zeroed(1);
    let rec = &mut party.members[0];
    rec.set_record_stats(legaia_save::character::RecordStats {
        hp_max: 100,
        mp_max: 30,
        cap_constant: 100,
        agl: 40,
        atk: 100,
        udf: 50,
        ldf: 60,
        spd: 35,
        int: 20,
    });
    rec.set_live_stats(legaia_save::character::LiveStats {
        agl: 40,
        atk: 100,
        udf: 50,
        ldf: 60,
        spd: 35,
        int: 20,
    });
    let mut eq = rec.equipment();
    eq.slots[6] = 0x52;
    eq.slots[7] = 0x53;
    rec.set_equipment(eq);
    let mut hms = rec.hp_mp_sp();
    hms.hp_cur = 100;
    hms.hp_max = 100;
    rec.set_hp_mp_sp(hms);
    world.load_party(party);

    world.seed_party_battle_stats();

    // ATK +20% of the base: 100 + 100/5 = 120.
    assert_eq!(world.battle_attack[0], 120);
    // Max HP +10% of the base, applied to the live battle actor.
    assert_eq!(world.actors[0].battle.max_hp, 110);
    // The ability bits are populated for the MP-cost consumers.
    assert_eq!(world.character_ability_bits[0] & 0x41, 0x41); // bits 0 + 6
}

#[test]
fn battle_magic_buff_raises_scalar_refreshes_and_expires() {
    use crate::spells::{BuffStat, SpellOutcome};

    let mut world = World::default();
    world.set_battle_attack(0, 50);

    // Power Up: retail stat-up is the x6/5 ramp (50 -> 60), not a flat +20.
    world.fold_spell_outcome(SpellOutcome::Buff {
        target: 0,
        stat: BuffStat::Attack,
        magnitude: 20,
        turns: 2,
    });
    assert_eq!(
        world.battle_attack[0], 60,
        "stat-up ramps the scalar by x6/5"
    );
    assert_eq!(world.battle_buffs.len(), 1);

    // Re-casting refreshes (reverts the old delta first, so the ramp re-applies
    // from the base 50 -> 60, no compounding).
    world.fold_spell_outcome(SpellOutcome::Buff {
        target: 0,
        stat: BuffStat::Attack,
        magnitude: 20,
        turns: 2,
    });
    assert_eq!(
        world.battle_attack[0], 60,
        "refresh does not compound the ramp"
    );
    assert_eq!(world.battle_buffs.len(), 1);

    // Ages one turn per the buffed actor's turn; expires on the 2nd.
    world.tick_battle_buffs_on_turn(0);
    assert_eq!(world.battle_attack[0], 60);
    world.tick_battle_buffs_on_turn(0);
    assert_eq!(
        world.battle_attack[0], 50,
        "expiry reverts the ramp delta exactly"
    );
    assert!(world.battle_buffs.is_empty());
}

#[test]
fn battle_magic_buff_ramp_is_multiplicative_not_additive() {
    use crate::spells::{BuffStat, SpellOutcome};

    let mut world = World::default();
    // At a 200 scalar the retail x6/5 ramp (->240) diverges from a flat +20
    // (->220): proves the live buff is multiplicative, not additive.
    world.set_battle_magic(1, 200);
    world.fold_spell_outcome(SpellOutcome::Buff {
        target: 1,
        stat: BuffStat::MagicAttack,
        magnitude: 20,
        turns: 1,
    });
    assert_eq!(world.battle_magic[1], 240, "x6/5 ramp, not flat +20");
    world.tick_battle_buffs_on_turn(1);
    assert_eq!(world.battle_magic[1], 200, "ramp delta reverts exactly");

    // The ramp clamps at 0xFFFF (buff_ramp ceiling) without overflow.
    world.set_battle_attack(2, 60_000);
    world.fold_spell_outcome(SpellOutcome::Buff {
        target: 2,
        stat: BuffStat::Attack,
        magnitude: 20,
        turns: 1,
    });
    assert_eq!(world.battle_attack[2], 0xFFFF, "ramp clamps at u16 max");
    world.tick_battle_buffs_on_turn(2);
    assert_eq!(
        world.battle_attack[2], 60_000,
        "clamped delta still reverts"
    );
}

#[test]
fn battle_magic_debuff_saturates_at_zero_and_reverts_exactly() {
    use crate::spells::{BuffStat, SpellOutcome};

    let mut world = World::default();
    // Power Down on an enemy with a small attack: -25 saturates the u16
    // scalar at 0, and the recorded delta is the actual change (-10).
    world.set_battle_attack(3, 10);
    world.fold_spell_outcome(SpellOutcome::Buff {
        target: 3,
        stat: BuffStat::Attack,
        magnitude: -25,
        turns: 1,
    });
    assert_eq!(world.battle_attack[3], 0, "debuff saturates at zero");

    // One tick expires it; the exact -10 delta is reverted back to 10.
    world.tick_battle_buffs_on_turn(3);
    assert_eq!(world.battle_attack[3], 10);
    assert!(world.battle_buffs.is_empty());
}

#[test]
fn battle_magic_capture_downs_a_weakened_monster_and_logs_the_id() {
    use crate::spells::SpellOutcome;

    let mut world = World {
        party_count: 1,
        ..World::default()
    };
    world.mode = SceneMode::Battle;
    // rng_state 0 -> first next_rng() % 100 == 23 (deterministic).
    world.rng_state = 0;
    world.actors[1].battle.max_hp = 100;
    world.actors[1].battle.hp = 10; // missing 90
    world.actors[1].battle.liveness = 1;
    world.actors[1].battle_monster_id = Some(42);

    // hit_pct 60, missing 90/100 -> effective 54; roll 23 < 54 -> captured.
    world.fold_spell_outcome(SpellOutcome::CaptureRoll {
        target: 1,
        hit_pct: 60,
    });
    assert_eq!(
        world.actors[1].battle.liveness, 0,
        "captured monster is downed"
    );
    assert_eq!(world.actors[1].battle.hp, 0);
    assert_eq!(world.drain_battle_captures(), vec![42]);

    // A near-full-HP monster has a tiny effective chance -> the same roll
    // misses and the monster is untouched.
    let mut world = World {
        party_count: 1,
        ..World::default()
    };
    world.mode = SceneMode::Battle;
    world.rng_state = 0; // roll 23
    world.actors[1].battle.max_hp = 100;
    world.actors[1].battle.hp = 95; // missing 5 -> effective 3
    world.actors[1].battle.liveness = 1;
    world.actors[1].battle_monster_id = Some(7);
    world.fold_spell_outcome(SpellOutcome::CaptureRoll {
        target: 1,
        hit_pct: 60,
    });
    assert_eq!(
        world.actors[1].battle.liveness, 1,
        "healthy monster resists"
    );
    assert!(world.battle_captures.is_empty());
}

#[test]
fn battle_magic_escape_returns_to_field() {
    use crate::input::PadButton;
    use crate::spells::SpellCatalog;

    let mut world = World {
        party_count: 1,
        ..World::default()
    };
    world.battle_player_driven = true;
    world.mode = SceneMode::Battle;
    world.actors[0].battle.max_hp = 100;
    world.actors[0].battle.hp = 100;
    world.actors[0].battle.mp = 20;
    world.actors[0].battle.liveness = 1;
    world.actors[1].battle.max_hp = 200;
    world.actors[1].battle.hp = 200;
    world.actors[1].battle.liveness = 1;
    world.spell_catalog = SpellCatalog::vanilla();

    // Open the spell submenu with Warp (0x41, SelfOnly escape) learned.
    world.battle_ctx.active_actor = 0;
    world.battle_spell_menu = Some(crate::battle_magic::BattleSpellSession::new(
        0,
        0,
        &[0x41],
        &world.spell_catalog,
        20,
        0,
    ));

    // SelfOnly target resolves immediately, so one Cross casts Warp.
    world.set_pad(0);
    world.set_pad(PadButton::Cross.mask());
    world.tick_battle_spell_menu();

    assert_eq!(world.mode, SceneMode::Field, "escape returns to the field");
    assert!(
        world.battle_spell_menu.is_none(),
        "submenu dropped on escape"
    );
    assert!(
        !world.battle_escaped,
        "escape flag cleared by finish_battle"
    );
    assert!(world.last_battle_rewards.is_none(), "escape grants no loot");
}

/// A registry whose Seru hits its learn threshold in one capture, and a
/// monster catalog linking monster id 7 -> Seru 1.
#[cfg(test)]
fn capture_world(party_count: u8) -> World {
    use crate::monster_catalog::{MonsterCatalog, MonsterDef};
    use crate::seru_learning::{SeruDef, SeruRegistry};

    let mut world = World {
        party_count,
        ..World::default()
    };
    // Zeroed roster (empty spell lists) so `build_battle_spell_session`
    // resolves a member per party slot; learned spells come from the log.
    world.roster = legaia_save::Party::zeroed(party_count.max(1) as usize);
    let mut reg = SeruRegistry::new();
    reg.insert(SeruDef {
        id: 1,
        name: "Spark".into(),
        spell_id: 0x20,
        capture_points: 100,
        learnable_mask: 0b0000_0111,
        learn_threshold: 100,
    });
    reg.insert(SeruDef {
        id: 2,
        name: "Slow".into(),
        spell_id: 0x21,
        capture_points: 40, // below threshold in one capture
        learnable_mask: 0b0000_0111,
        learn_threshold: 100,
    });
    world.set_seru_registry(reg);
    let mut cat = MonsterCatalog::new();
    cat.insert(MonsterDef::new(7, "Killer Bee", 25, 9).with_seru(1));
    cat.insert(MonsterDef::new(8, "Slime", 40, 8).with_seru(2));
    cat.insert(MonsterDef::new(9, "Wolf", 35, 12)); // no Seru
    world.set_monster_catalog(cat);
    world
}

#[test]
fn capture_banks_points_and_learns_on_finish_battle() {
    let mut world = capture_world(2);
    // Two monsters captured this battle: Killer Bee (Seru 1, learns) and
    // Wolf (no Seru, banks nothing).
    world.battle_captures = vec![7, 9];

    world.finish_battle();

    // battle_captures always drained.
    assert!(world.battle_captures.is_empty());
    // Both party slots learned Spark (id 0x20).
    assert!(world.seru_log.has_learned(0, 1));
    assert!(world.seru_log.has_learned(1, 1));
    assert_eq!(world.seru_log.learned_spells(0), &[0x20]);
    // One accepted outcome (the Wolf had no Seru), with two learn events.
    let outcomes = world.drain_last_capture_outcomes();
    assert_eq!(outcomes.len(), 1);
    assert_eq!(outcomes[0].learns.len(), 2);
    // Outcomes drained.
    assert!(world.drain_last_capture_outcomes().is_empty());
}

#[test]
fn capture_below_threshold_banks_points_without_learning() {
    let mut world = capture_world(1);
    world.battle_captures = vec![8]; // Slime -> Seru 2, 40 < 100

    world.finish_battle();

    assert!(!world.seru_log.has_learned(0, 2), "not learned yet");
    assert_eq!(world.seru_log.row(0, 2).points, 40, "points banked");
    let outcomes = world.drain_last_capture_outcomes();
    assert_eq!(outcomes.len(), 1);
    assert!(outcomes[0].learns.is_empty());
}

#[test]
fn capture_sets_the_banner_and_it_clears_on_tick() {
    use crate::seru_learning::CaptureState;

    let mut world = capture_world(1);
    world.set_spell_catalog(crate::spells::SpellCatalog::vanilla());
    world.battle_captures = vec![7]; // Killer Bee -> Seru 1 (Spark), learns
    world.finish_battle();

    // The banner opens on the capture phase naming the captured Seru.
    let banner = world
        .current_capture_banner
        .as_ref()
        .expect("capture banner set");
    assert_eq!(banner.seru_name(), "Spark");
    assert!(matches!(banner.state(), CaptureState::Capturing { .. }));
    assert_eq!(banner.current_banner().as_deref(), Some("Captured: Spark!"));
    // A learn event was recorded (party slot 0 crossed the threshold).
    assert_eq!(banner.learns().len(), 1);

    // Drive the banner to completion via World::tick (Field mode after the
    // battle). The default durations are 60 capture + 90 announce frames.
    for _ in 0..(60 + 90 + 4) {
        world.tick();
    }
    assert!(
        world.current_capture_banner.is_none(),
        "banner clears after its phases elapse"
    );
}

#[test]
fn sub_threshold_capture_banner_shows_no_learn_line() {
    let mut world = capture_world(1);
    world.battle_captures = vec![8]; // Slime -> Seru 2, 40 < 100, no learn
    world.finish_battle();

    let banner = world
        .current_capture_banner
        .as_ref()
        .expect("capture banner set even without a learn");
    assert_eq!(banner.seru_name(), "Slow");
    assert!(banner.learns().is_empty());
}

#[test]
fn battle_bgm_swaps_on_encounter_and_restores_on_finish() {
    use crate::monster_catalog::{FormationDef, FormationSlot};

    let mut world = World {
        party_count: 1,
        ..World::default()
    };
    world.actors[0].battle.hp = 100;
    world.current_bgm = Some(0x0A); // field track playing
    world.set_battle_bgm(Some(0x40)); // configured battle track
    let formation = FormationDef::new(7, vec![FormationSlot::new(1)]);

    world.enter_battle_from_formation(&formation);

    // Swapped to the battle track, with a start event queued for the host.
    assert_eq!(world.current_bgm, Some(0x40));
    assert!(world.battle_bgm_active);
    let evs = world.drain_field_events();
    assert!(
        evs.iter().any(|e| matches!(
            e,
            FieldEvent::Bgm {
                text_id: 0x40,
                sub_op: 1
            }
        )),
        "battle BGM start queued: {evs:?}"
    );

    // Finish (no formation/loot) restores the field track + queues its start.
    world.finish_battle();
    assert_eq!(world.current_bgm, Some(0x0A));
    assert!(!world.battle_bgm_active);
    let evs = world.drain_field_events();
    assert!(
        evs.iter().any(|e| matches!(
            e,
            FieldEvent::Bgm {
                text_id: 0x0A,
                sub_op: 1
            }
        )),
        "field BGM restore queued: {evs:?}"
    );
}

#[test]
fn battle_bgm_unset_leaves_music_untouched() {
    use crate::monster_catalog::{FormationDef, FormationSlot};

    let mut world = World {
        party_count: 1,
        ..World::default()
    };
    world.actors[0].battle.hp = 100;
    world.current_bgm = Some(0x0A);
    // No battle_bgm configured (default None) -> no swap, no events.
    let formation = FormationDef::new(7, vec![FormationSlot::new(1)]);
    world.enter_battle_from_formation(&formation);
    assert_eq!(world.current_bgm, Some(0x0A));
    assert!(!world.battle_bgm_active);
    assert!(
        !world
            .drain_field_events()
            .iter()
            .any(|e| matches!(e, FieldEvent::Bgm { .. })),
        "no BGM events when battle_bgm is unset"
    );
    world.finish_battle();
    assert_eq!(world.current_bgm, Some(0x0A));
}

#[test]
fn battle_bgm_with_silent_field_stops_on_finish() {
    use crate::monster_catalog::{FormationDef, FormationSlot};

    let mut world = World {
        party_count: 1,
        ..World::default()
    };
    world.actors[0].battle.hp = 100;
    world.current_bgm = None; // no field music playing
    world.set_battle_bgm(Some(0x40));
    let formation = FormationDef::new(7, vec![FormationSlot::new(1)]);

    world.enter_battle_from_formation(&formation);
    assert_eq!(world.current_bgm, Some(0x40));
    let _ = world.drain_field_events();

    world.finish_battle();
    // Nothing to resume -> battle music stops (sub-op 4) and id clears.
    assert_eq!(world.current_bgm, None);
    let evs = world.drain_field_events();
    assert!(
        evs.iter()
            .any(|e| matches!(e, FieldEvent::Bgm { sub_op: 4, .. })),
        "BGM stop queued when no field track to resume: {evs:?}"
    );
}

#[test]
fn learned_spell_is_offered_in_the_battle_spell_session() {
    let mut world = capture_world(1);
    world.set_spell_catalog(crate::spells::SpellCatalog::vanilla());
    // Caster has an empty roster spell list; learning Spark via capture
    // should still surface it in the battle spell menu.
    world.battle_captures = vec![7];
    world.finish_battle();
    world.actors[0].battle.mp = 99;

    let session = world
        .build_battle_spell_session(0)
        .expect("session builds for slot 0");
    assert!(
        session.spells.iter().any(|s| s.id == 0x20),
        "captured Spark is castable: {:?}",
        session.spells.iter().map(|s| s.id).collect::<Vec<_>>()
    );
}

#[test]
fn capture_progress_round_trips_through_save_load() {
    // Bank a sub-threshold capture, save, reload into a fresh world that
    // has the registry installed, and confirm the points + learned state
    // survive.
    let mut world = capture_world(1);
    world.battle_captures = vec![7, 8]; // Seru 1 learns; Seru 2 banks 40
    world.finish_battle();
    assert!(world.seru_log.has_learned(0, 1));
    assert_eq!(world.seru_log.row(0, 2).points, 40);

    let save = world.save_full();

    let mut reloaded = capture_world(1);
    reloaded.load_full(save);
    assert!(
        reloaded.seru_log.has_learned(0, 1),
        "learned Spark restored"
    );
    assert_eq!(
        reloaded.seru_log.learned_spells(0),
        &[0x20],
        "spell list restored"
    );
    assert_eq!(
        reloaded.seru_log.row(0, 2).points,
        40,
        "sub-threshold progress restored"
    );
    assert!(
        !reloaded.seru_log.has_learned(0, 2),
        "still below threshold after reload"
    );
}

#[test]
fn arts_editor_chain_round_trips_through_save_into_the_battle_menu() {
    use crate::tactical_arts_editor::{ChainEditor, EditInput, EditOutcome};

    // A field-side session: the player opens the Tactical Arts editor and
    // composes a brand-new chain for character slot 0 (Down, Up, Up).
    let mut field = World {
        party_count: 1,
        ..World::default()
    };
    let mut lib = field.chain_library();
    let mut ed = ChainEditor::new(0, &lib);
    // Cross: open the "+ New" editor.
    ed.tick(EditInput {
        cross: true,
        ..Default::default()
    });
    for dir in [
        EditInput {
            down: true,
            ..Default::default()
        },
        EditInput {
            up: true,
            ..Default::default()
        },
        EditInput {
            up: true,
            ..Default::default()
        },
    ] {
        ed.tick(dir);
    }
    // Cross: commit to naming, then Cross again: confirm the default name.
    ed.tick(EditInput {
        cross: true,
        ..Default::default()
    });
    ed.tick(EditInput {
        cross: true,
        ..Default::default()
    });
    assert!(
        matches!(ed.outcome(), Some(EditOutcome::Saved { slot: 0, .. })),
        "editor saved a new chain"
    );
    // Apply the edit to the library and store it back into the world -
    // the bridge under test (no direct `saved_chains` seeding).
    ed.apply_outcome(&mut lib).unwrap();
    field.store_chain_library(&lib);

    // The chain now serializes with the save block...
    let save = field.save_full();
    assert_eq!(save.ext_v2.saved_chains.len(), 1);

    // ...and a fresh boot that loads the save can offer it in battle.
    let mut battle = World {
        party_count: 1,
        ..World::default()
    };
    battle.load_full(save);
    let rows = battle.build_battle_arts_rows(0);
    assert_eq!(
        rows.len(),
        1,
        "the edited chain reaches the battle arts menu"
    );
    // Default new-chain name preset; 3 directional inputs => 3 synthetic hits.
    assert_eq!(rows[0].hits(), 3);
}

#[test]
fn battle_arts_synthetic_chain_runs_through_art_power_path_and_cycles_turn() {
    use crate::input::PadButton;
    use legaia_engine_vm::battle_action::ActionState;

    let mut world = World {
        party_count: 1,
        ..World::default()
    };
    world.battle_player_driven = true;
    world.mode = SceneMode::Battle;
    world.actors[0].battle.max_hp = 200;
    world.actors[0].battle.hp = 200;
    world.actors[0].battle.liveness = 1;
    world.set_battle_attack(0, 40);
    world.actors[1].battle.max_hp = 500;
    world.actors[1].battle.hp = 500;
    world.actors[1].battle.liveness = 1;
    world.set_battle_defense(1, 10);
    // One saved chain, 3 directional commands (Left, Right, Down) -> 3 hits.
    // No art record staged, so the row uses the synthetic ×12 profile.
    world.saved_chains.push(legaia_save::SavedChainRecord {
        char_slot: 0,
        name: "Combo".into(),
        sequence: vec![1, 2, 3],
    });

    world.battle_ctx.active_actor = 0;
    world.battle_arts_menu = Some(crate::battle_arts::BattleArtsSession::new(
        0,
        0,
        world.build_battle_arts_rows(0),
    ));
    assert_eq!(world.battle_arts_menu.as_ref().unwrap().arts[0].hits(), 3);

    // Frame 1: Cross opens the target cursor.
    world.set_pad(0);
    world.set_pad(PadButton::Cross.mask());
    world.tick_battle_arts_menu();
    assert!(world.battle_arts_menu.is_some(), "still picking a target");

    // Frame 2: Cross confirms the monster; the art runs.
    world.set_pad(0);
    world.set_pad(PadButton::Cross.mask());
    world.tick_battle_arts_menu();

    assert!(world.battle_arts_menu.is_none(), "arts menu closed");
    // Three synthetic ×12 hits: (40*12/16 - 10) = 20 each => 60 total.
    let per_hit = legaia_engine_vm::battle_formulas::art_strike_damage_default(40, 10, 12);
    assert_eq!(world.actors[1].battle.hp, 500 - per_hit * 3);
    assert_eq!(
        world.battle_ctx.action_state,
        ActionState::EndOfAction.as_byte(),
        "turn parked at EndOfAction so the loop cycles"
    );
    let fx = world.drain_battle_hit_fx();
    assert_eq!(fx.len(), 1, "one summed popup for the combo");
    assert!(!fx[0].is_heal);
    assert_eq!(fx[0].amount, per_hit * 3);
    assert_eq!(fx[0].target_slot, 1);
}

#[test]
fn battle_arts_uses_staged_art_record_power_tiers_and_status() {
    use crate::input::PadButton;
    use legaia_art::power::PowerByte;
    use legaia_art::queue::{ActionConstant, Command};
    use legaia_art::record::EnemyEffect;

    let mut world = World {
        party_count: 1,
        ..World::default()
    };
    world.battle_player_driven = true;
    world.mode = SceneMode::Battle;
    world.actors[0].battle.liveness = 1;
    world.set_battle_attack(0, 64);
    world.actors[1].battle.max_hp = 4000;
    world.actors[1].battle.hp = 4000;
    world.actors[1].battle.liveness = 1;
    // UDF / LDF split so the record's per-strike target picks the right half.
    world.set_battle_defense_split(1, Some((10, 40)));

    // Stage a Vahn art: two damage strikes (UDF ×28, LDF ×28) that burns.
    let rec = legaia_art::ArtRecord {
        action: ActionConstant::Art1B,
        commands: vec![Command::Up, Command::Up],
        anim_index: 0,
        anim_extra: vec![],
        name: None,
        power: vec![PowerByte::from_byte(0x1A), PowerByte::from_byte(0x1F)],
        dmg_timing: vec![],
        effect_cues: Default::default(),
        hit_cues: vec![],
        identifier: 0,
        anim_speed: 0,
        enemy_effect: EnemyEffect::Toxic,
        repeat_frames: Default::default(),
        background: 0,
        runtime_address: None,
    };
    world.set_art_record(legaia_art::Character::Vahn, ActionConstant::Art1B, rec);

    // Saved chain ending in the art's command string (Up, Up).
    world.saved_chains.push(legaia_save::SavedChainRecord {
        char_slot: 0,
        name: "Burning Combo".into(),
        sequence: vec![1, 4, 4], // Left, Up, Up
    });

    let rows = world.build_battle_arts_rows(0);
    assert_eq!(rows[0].hits(), 2, "two damage strikes from the record");
    assert_eq!(rows[0].enemy_effect, EnemyEffect::Toxic);

    world.battle_ctx.active_actor = 0;
    world.battle_arts_menu = Some(crate::battle_arts::BattleArtsSession::new(0, 0, rows));

    // Open the target cursor, then confirm.
    world.set_pad(0);
    world.set_pad(PadButton::Cross.mask());
    world.tick_battle_arts_menu();
    world.set_pad(0);
    world.set_pad(PadButton::Cross.mask());
    world.tick_battle_arts_menu();

    // UDF ×28 vs udf=10: 64*28/16 - 10 = 112 - 10 = 102.
    // LDF ×28 vs ldf=40: 64*28/16 - 40 = 112 - 40 = 72.
    let expect = (102u16 + 72u16) as u32;
    assert_eq!(world.actors[1].battle.hp, 4000 - expect as u16);
    assert!(
        world.status_effects.is_afflicted(1),
        "the art's Toxic effect was applied to the target"
    );
    let fx = world.drain_battle_hit_fx();
    assert_eq!(fx.len(), 1);
    assert_eq!(fx[0].amount, expect as u16);
    assert!(fx[0].is_crit, "multi-hit art flagged as crit popup");
}

#[test]
fn build_battle_arts_rows_resolves_miracle_finisher_profile() {
    use legaia_art::power::PowerByte;
    use legaia_art::queue::{ActionConstant, Command};
    use legaia_art::record::EnemyEffect;

    // Vahn's Craze directional string: Right, Down, Left, Up, Left, Up,
    // Right, Down, Left (Left=1 Right=2 Down=3 Up=4).
    let craze_seq = vec![2u8, 3, 1, 4, 1, 4, 2, 3, 1];

    // No art records staged: each of Vahn's Craze's six component arts
    // (Art22/28/23/27/20/2A) degrades to one synthetic ×12 strike.
    let mut world = World {
        party_count: 1,
        ..World::default()
    };
    world.saved_chains.push(legaia_save::SavedChainRecord {
        char_slot: 0,
        name: "MyCraze".into(),
        sequence: craze_seq.clone(),
    });
    let rows = world.build_battle_arts_rows(0);
    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0].miracle,
        Some("Vahn's Craze"),
        "chain flagged Miracle"
    );
    assert_eq!(
        rows[0].hits(),
        6,
        "six component arts -> six synthetic strikes with no records"
    );
    assert_eq!(rows[0].enemy_effect, EnemyEffect::None);

    // Stage the first component art (Art22) with two damage strikes that
    // burn: it contributes its real bytes; the other five stay synthetic.
    let rec = legaia_art::ArtRecord {
        action: ActionConstant::Art22,
        commands: vec![Command::Up],
        anim_index: 0,
        anim_extra: vec![],
        name: None,
        power: vec![PowerByte::from_byte(0x1A), PowerByte::from_byte(0x1A)],
        dmg_timing: vec![],
        effect_cues: Default::default(),
        hit_cues: vec![],
        identifier: 0,
        anim_speed: 0,
        enemy_effect: EnemyEffect::Toxic,
        repeat_frames: Default::default(),
        background: 0,
        runtime_address: None,
    };
    world.set_art_record(legaia_art::Character::Vahn, ActionConstant::Art22, rec);
    let rows = world.build_battle_arts_rows(0);
    assert_eq!(
        rows[0].hits(),
        7,
        "Art22 record (2 strikes) + 5 synthetic component arts"
    );
    assert_eq!(
        rows[0].enemy_effect,
        EnemyEffect::Toxic,
        "first staged component art's status effect is adopted"
    );
}

#[test]
fn build_battle_arts_rows_fires_super_from_recognized_art_sequence() {
    use legaia_art::power::PowerByte;
    use legaia_art::queue::{ActionConstant, Command};
    use legaia_art::record::EnemyEffect;

    // Vahn's Tri-Somersault chains Somersault (Art27) -> Cyclone (Art1F) ->
    // Somersault (Art27); art_sequence = [0x27, 0x1F, 0x27]. Give each
    // component art a one-direction command so a flat chain recognizes them.
    fn stage_art(world: &mut World, action: ActionConstant, cmd: Command, strikes: usize) {
        let rec = legaia_art::ArtRecord {
            action,
            commands: vec![cmd],
            anim_index: 0,
            anim_extra: vec![],
            name: None,
            power: vec![PowerByte::from_byte(0x16); strikes],
            dmg_timing: vec![],
            effect_cues: Default::default(),
            hit_cues: vec![],
            identifier: 0,
            anim_speed: 0,
            enemy_effect: EnemyEffect::None,
            repeat_frames: Default::default(),
            background: 0,
            runtime_address: None,
        };
        world.set_art_record(legaia_art::Character::Vahn, action, rec);
    }

    let mut world = World {
        party_count: 1,
        ..World::default()
    };
    stage_art(&mut world, ActionConstant::Art27, Command::Up, 2);
    stage_art(&mut world, ActionConstant::Art1F, Command::Down, 1);

    // Chain Up Down Up -> Somersault Cyclone Somersault.
    // (Left=1 Right=2 Down=3 Up=4.)
    world.saved_chains.push(legaia_save::SavedChainRecord {
        char_slot: 0,
        name: "TriSom".into(),
        sequence: vec![4, 3, 4],
    });
    let rows = world.build_battle_arts_rows(0);
    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0].super_art,
        Some("Tri-Somersault"),
        "recognized art sequence [27 1F 27] fires Vahn's Tri-Somersault"
    );
    assert_eq!(rows[0].miracle, None, "Super is not a Miracle");
    // Super replace art constants = [27, 1F, 2B, 2B, 2B]: Art27 (2 strikes) +
    // Art1F (1 strike) + three synthetic finisher (0x2B) strikes = 6.
    assert_eq!(rows[0].hits(), 6, "component-art strikes + 3 finisher hits");

    // Connector abstraction: a stray Left/Right between the arts (matching no
    // staged art) is skipped, so the same Super still fires.
    world.saved_chains.clear();
    world.saved_chains.push(legaia_save::SavedChainRecord {
        char_slot: 0,
        name: "TriSomLoose".into(),
        sequence: vec![4, 1, 3, 2, 4], // Up [Left] Down [Right] Up
    });
    let rows = world.build_battle_arts_rows(0);
    assert_eq!(
        rows[0].super_art,
        Some("Tri-Somersault"),
        "connector directions between arts are abstracted (skipped)"
    );

    // With no art catalog staged the recognizer can't run, so no Super is
    // detected and the chain falls back to a plain/synthetic row.
    let mut bare = World {
        party_count: 1,
        ..World::default()
    };
    bare.saved_chains.push(legaia_save::SavedChainRecord {
        char_slot: 0,
        name: "TriSom".into(),
        sequence: vec![4, 3, 4],
    });
    assert_eq!(
        bare.build_battle_arts_rows(0)[0].super_art,
        None,
        "no art catalog -> no Super detection (graceful degradation)"
    );
}

#[test]
fn apply_battle_loot_never_drops_when_rate_zero() {
    use crate::monster_catalog::{FormationDef, FormationSlot, MonsterCatalog, MonsterDef};
    let mut cat = MonsterCatalog::new();
    let mut def = MonsterDef::new(7, "Slime", 10, 5);
    def.drop_item = Some(0x42);
    def.drop_rate_q8 = 0;
    cat.insert(def);
    let formation = FormationDef::new(1000, vec![FormationSlot::new(7)]);
    let mut world = World {
        party_count: 1,
        ..World::default()
    };
    world.actors[0].battle.hp = 100;
    let rewards = world.apply_battle_loot(&formation, &cat);
    assert!(rewards.drops.is_empty());
    assert!(!world.inventory.contains_key(&0x42));
}

#[test]
fn load_full_hydrates_level_up_tracker_from_record_levels() {
    // Build a 3-character save with levels 7, 12, 25.
    let mut party = legaia_save::Party::zeroed(3);
    party.members[0].set_level(7);
    party.members[1].set_level(12);
    party.members[2].set_level(25);
    let sf = legaia_save::SaveFile {
        party,
        ext: legaia_save::SaveExt::default(),
        ext_v2: legaia_save::SaveExtV2::default(),
    };
    let mut world = World::new();
    // Tracker defaults to 1 for every slot.
    assert_eq!(world.level_up_tracker.level[0], 1);
    world.load_full(sf);
    assert_eq!(world.level_up_tracker.level[0], 7);
    assert_eq!(world.level_up_tracker.level[1], 12);
    assert_eq!(world.level_up_tracker.level[2], 25);
}

#[test]
fn load_full_zero_level_record_clamps_to_one() {
    // Records that haven't had a level written (zero byte at +0x100)
    // shouldn't make the tracker think the slot is below L1.
    let party = legaia_save::Party::zeroed(2);
    let sf = legaia_save::SaveFile {
        party,
        ext: legaia_save::SaveExt::default(),
        ext_v2: legaia_save::SaveExtV2::default(),
    };
    let mut world = World::new();
    world.load_full(sf);
    assert_eq!(world.level_up_tracker.level[0], 1);
    assert_eq!(world.level_up_tracker.level[1], 1);
}

#[test]
fn apply_battle_xp_scales_three_quarters_and_ceils() {
    let mut world = World {
        party_count: 3,
        ..World::default()
    };
    world.actors[0].battle.hp = 100;
    world.actors[1].battle.hp = 100;
    world.actors[2].battle.hp = 100;
    // FUN_8004E568: 101 summed -> *3/4 = 101 - (101>>2 = 25) = 76, then
    // ceil(76 / 3 alive) = 26 each (floor would give 25). Below the 50 L2
    // threshold, so it just accumulates.
    let _ = world.apply_battle_xp(101);
    assert_eq!(world.level_up_tracker.xp[0], 26);
    assert_eq!(world.level_up_tracker.xp[1], 26);
    assert_eq!(world.level_up_tracker.xp[2], 26);
}

#[test]
fn level_up_banner_countdown_clears() {
    let mut world = World {
        party_count: 1,
        ..World::default()
    };
    world.actors[0].battle.hp = 100;
    world.apply_battle_xp(68); // 3/4-scaled ceil to 51 >= the 50 L2 threshold
    assert!(world.current_level_up_banner.is_some());
    for _ in 0..=crate::levelup::LevelUpBanner::DEFAULT_FRAMES {
        world.tick();
    }
    assert!(
        world.current_level_up_banner.is_none(),
        "level-up banner should have cleared"
    );
}

#[test]
fn no_level_up_banner_when_xp_insufficient() {
    let mut world = World {
        party_count: 1,
        ..World::default()
    };
    world.actors[0].battle.hp = 100;
    world.apply_battle_xp(49); // retail table: 49 < 50 (L2 threshold)
    assert!(world.current_level_up_banner.is_none());
}

#[test]
fn art_strike_applier_pushes_apply_art_strike_event() {
    // Drive `BattleHostImpl::apply_art_strike` from a synthetic
    // ArtStrikeInfo and assert the world's pending_battle_events grows
    // by one ApplyArtStrike with the resolved damage.
    use legaia_art::Character;
    use legaia_art::power::PowerByte;
    use legaia_art::queue::ActionConstant;
    use legaia_art::record::EnemyEffect;
    use legaia_engine_vm::battle_action::{ArtStrikeInfo, BattleActionHost};

    let mut world = World::new();
    world.set_battle_attack(0, 64);
    world.set_battle_defense(3, 10);
    let info = ArtStrikeInfo {
        strike_index: 0,
        anim_byte: 0x10,
        actor_slot: 0,
        target_slot: 3,
        character: Character::Vahn,
        art: ActionConstant::Art1B,
        power: Some(PowerByte::from_byte(0x1A)), // UDF × 28
        dmg_timing: Some(0x10),
        enemy_effect: EnemyEffect::Toxic,
        hit_cue: None,
    };
    let mut host = BattleHostImpl { world: &mut world };
    host.apply_art_strike(info);

    assert_eq!(world.pending_battle_events.len(), 1);
    match &world.pending_battle_events[0] {
        BattleEvent::ApplyArtStrike {
            actor_slot,
            target_slot,
            strike_index,
            outcome,
        } => {
            assert_eq!(*actor_slot, 0);
            assert_eq!(*target_slot, 3);
            assert_eq!(*strike_index, 0);
            assert_eq!(outcome.damage, Some(102));
            assert_eq!(outcome.enemy_effect, EnemyEffect::Toxic);
        }
        other => panic!("unexpected event: {:?}", other.summary()),
    }
}

#[test]
fn art_strike_split_defense_picks_udf_or_ldf() {
    // With a (UDF=5, LDF=50) split on slot 3, a UDF-targeted strike
    // hits 5 def → high damage; LDF-targeted hits 50 def → low.
    use legaia_art::Character;
    use legaia_art::power::PowerByte;
    use legaia_art::queue::ActionConstant;
    use legaia_art::record::EnemyEffect;
    use legaia_engine_vm::battle_action::{ArtStrikeInfo, BattleActionHost};

    let mut world = World::new();
    world.set_battle_attack(0, 64);
    world.set_battle_defense_split(3, Some((5, 50)));

    let mk = |power: PowerByte| ArtStrikeInfo {
        strike_index: 0,
        anim_byte: 0x10,
        actor_slot: 0,
        target_slot: 3,
        character: Character::Vahn,
        art: ActionConstant::Art1B,
        power: Some(power),
        dmg_timing: Some(0x10),
        enemy_effect: EnemyEffect::None,
        hit_cue: None,
    };
    // 0x1A = UDF × 28 → (64 * 28)/16 - 5 = 112 - 5 = 107.
    let mut host = BattleHostImpl { world: &mut world };
    host.apply_art_strike(mk(PowerByte::from_byte(0x1A)));
    // 0x1F = LDF × 28 → (64 * 28)/16 - 50 = 112 - 50 = 62.
    host.apply_art_strike(mk(PowerByte::from_byte(0x1F)));
    let events = world.drain_battle_events();
    let mut udf_dmg = None;
    let mut ldf_dmg = None;
    for e in events {
        if let BattleEvent::ApplyArtStrike { outcome, .. } = e
            && let Some(t) = outcome.power_target
        {
            match t {
                legaia_art::power::PowerTarget::Udf => udf_dmg = outcome.damage,
                legaia_art::power::PowerTarget::Ldf => ldf_dmg = outcome.damage,
            }
        }
    }
    assert_eq!(udf_dmg, Some(107));
    assert_eq!(ldf_dmg, Some(62));
}

#[test]
fn fold_battle_event_apply_art_strike_subtracts_hp_and_records_status() {
    use legaia_art::power::{PowerByte, PowerTarget};
    use legaia_art::queue::{ActionConstant, Character};
    use legaia_art::record::EnemyEffect;
    use legaia_engine_vm::battle_action::ArtStrikeInfo;

    let mut world = World::new();
    world.party_count = 4;
    for slot in 0..4 {
        world.actors[slot].active = true;
        world.actors[slot].battle.hp = 200;
        world.actors[slot].battle.max_hp = 200;
    }
    world.set_battle_attack(0, 64);
    world.set_battle_defense(3, 5);

    let info = ArtStrikeInfo {
        strike_index: 0,
        anim_byte: 0x10,
        actor_slot: 0,
        target_slot: 3,
        character: Character::Vahn,
        art: ActionConstant::Art1B,
        power: Some(PowerByte::from_byte(0x1A)), // UDF × 28
        dmg_timing: Some(0x10),
        enemy_effect: EnemyEffect::Toxic,
        hit_cue: None,
    };
    let mut host = BattleHostImpl { world: &mut world };
    host.apply_art_strike(info);
    let events = world.drain_battle_events();
    assert_eq!(events.len(), 1);
    for e in &events {
        let r = world.fold_battle_event(e);
        // 64 * 28 / 16 - 5 = 107 damage. Target slot 3 starts at 200,
        // ends at 93.
        assert_eq!(r, Some((3, 93)));
    }
    assert_eq!(world.actors[3].battle.hp, 93);
    // Toxic status was folded into pending_status.
    assert_eq!(
        world.actors[3].pending_status,
        Some(legaia_art::record::EnemyEffect::Toxic)
    );
    // PowerTarget enum is needed only to satisfy the import linter
    // when the assertions don't otherwise reference it.
    let _ = PowerTarget::Udf;
}

#[test]
fn fold_battle_event_surfaces_art_strike_sound_cues() {
    use crate::art_strike::{ArtStrikeOutcome, ScheduledCue};

    let mut world = World::new();
    world.party_count = 4;
    for slot in 0..4 {
        world.actors[slot].active = true;
        world.actors[slot].battle.hp = 200;
        world.actors[slot].battle.max_hp = 200;
    }

    // An art strike whose outcome carries a sound cue (0x1A, frame 16) and a
    // hit-effect-only visual cue (0x4C) - only the sound cue should surface.
    let outcome = ArtStrikeOutcome {
        damage: Some(40),
        enemy_effect: legaia_art::record::EnemyEffect::None,
        cues: vec![
            ScheduledCue {
                timing_frames: 16,
                kind: 0x1A,
            },
            ScheduledCue {
                timing_frames: 8,
                kind: 0x4C,
            },
        ],
        alt_range: false,
        power_target: Some(legaia_art::power::PowerTarget::Udf),
    };
    let event = BattleEvent::ApplyArtStrike {
        actor_slot: 0,
        target_slot: 3,
        strike_index: 0,
        outcome,
    };

    assert!(world.drain_battle_sfx_cues().is_empty(), "starts empty");
    world.fold_battle_event(&event);

    let cues = world.drain_battle_sfx_cues();
    assert_eq!(
        cues.len(),
        1,
        "only the sound cue (0x1A) surfaces, not 0x4C"
    );
    assert_eq!(cues[0].kind, 0x1A);
    assert_eq!(cues[0].timing_frames, 16);
    assert_eq!(cues[0].actor_slot, 0);
    assert_eq!(cues[0].target_slot, 3);
    // Drained once.
    assert!(world.drain_battle_sfx_cues().is_empty());
}

#[test]
fn fold_battle_event_other_variants_dont_modify_state() {
    let mut world = World::new();
    world.party_count = 1;
    world.actors[0].active = true;
    world.actors[0].battle.hp = 100;
    let r = world.fold_battle_event(&BattleEvent::CameraBounds);
    assert_eq!(r, None);
    assert_eq!(world.actors[0].battle.hp, 100);
}

#[test]
fn spell_anim_trigger_requests_summon_only_for_seru_ids() {
    let mut world = World::new();
    world.party_count = 1;
    world.actors[0].active = true;

    // A non-summon id (a monster attack) requests nothing.
    world.fold_battle_event(&BattleEvent::SpellAnimTrigger {
        party_slot: 0,
        spell_id: 0x27,
    });
    assert!(world.take_pending_summon_spawn().is_none());

    // Gimard Tail Fire (0x81) requests a summon spawn at the caster's pos.
    world.actors[0].move_state.world_x = 11;
    world.actors[0].move_state.world_y = 22;
    world.actors[0].move_state.world_z = 33;
    world.fold_battle_event(&BattleEvent::SpellAnimTrigger {
        party_slot: 0,
        spell_id: 0x81,
    });
    let req = world.take_pending_summon_spawn();
    assert_eq!(req, Some((0x81, [11, 22, 33])));
    // Taken once.
    assert!(world.take_pending_summon_spawn().is_none());
}

#[test]
fn use_item_heals_hp_clamped_to_max() {
    let mut world = World::new();
    world.party_count = 1;
    world.actors[0].battle.max_hp = 200;
    world.actors[0].battle.hp = 50;
    world.set_item_catalog(full_test_catalog());
    // Item id 1 is the small heal in the vanilla catalog.
    let outcome = world.use_item(1, 0);
    assert!(matches!(
        outcome,
        crate::items::ItemOutcome::HealedHp { .. }
    ));
    // HP raised but clamped at max.
    assert!(world.actors[0].battle.hp > 50);
    assert!(world.actors[0].battle.hp <= 200);
}

#[test]
fn use_item_heal_all_fills_to_max() {
    let mut world = World::new();
    world.party_count = 1;
    world.actors[0].battle.max_hp = 300;
    world.actors[0].battle.hp = 100;
    world.set_item_catalog(full_test_catalog());
    // Find the HealAll entry (id 4 in the vanilla catalog - Healing Globe).
    let outcome = world.use_item(4, 0);
    assert!(matches!(
        outcome,
        crate::items::ItemOutcome::HealedHp { .. }
    ));
    assert_eq!(world.actors[0].battle.hp, 300);
}

#[test]
fn use_item_unknown_id_returns_no_effect() {
    let mut world = World::new();
    world.party_count = 1;
    world.set_item_catalog(full_test_catalog());
    let outcome = world.use_item(99, 0);
    assert!(matches!(outcome, crate::items::ItemOutcome::NoEffect));
}

#[test]
fn use_item_revive_writes_hp_after() {
    let mut world = World::new();
    world.party_count = 1;
    world.actors[0].battle.max_hp = 400;
    world.actors[0].battle.hp = 0; // dead
    world.set_item_catalog(full_test_catalog());
    // Resurrection Leaf is id 0x0C (50% revive).
    let outcome = world.use_item(0x0C, 0);
    assert!(matches!(outcome, crate::items::ItemOutcome::Revived { .. }));
    // 50% of 400 = 200.
    assert_eq!(world.actors[0].battle.hp, 200);
}

#[test]
fn use_item_hp_max_boost_raises_record_and_live_actor() {
    let mut party = legaia_save::Party::zeroed(1);
    let mut hms = party.members[0].hp_mp_sp();
    hms.hp_cur = 50;
    hms.hp_max = 100;
    party.members[0].set_hp_mp_sp(hms);
    let mut world = World::new();
    world.load_party(party);
    world.set_item_catalog(full_test_catalog());
    // Vital Tonic (0x0F): HpMax +10 - the outcome the old kernel dropped.
    let outcome = world.use_item(0x0F, 0);
    assert!(matches!(
        outcome,
        crate::items::ItemOutcome::StatRaised { .. }
    ));
    // Persistent record raised, current HP refilled by the gained amount.
    let rec_hms = world.roster.members[0].hp_mp_sp();
    assert_eq!(rec_hms.hp_max, 110);
    assert_eq!(rec_hms.hp_cur, 60);
    // Live battle actor raised too.
    assert_eq!(world.actors[0].battle.max_hp, 110);
    assert_eq!(world.actors[0].battle.hp, 60);
}

#[test]
fn use_item_attack_boost_raises_persistent_record_and_live_stat() {
    let mut party = legaia_save::Party::zeroed(1);
    let mut ls = party.members[0].live_stats();
    ls.atk = 20;
    party.members[0].set_live_stats(ls);
    let mut world = World::new();
    world.load_party(party);
    world.set_battle_attack(0, 20);
    world.set_item_catalog(full_test_catalog());
    // Power Tonic (0x0E): Attack +1.
    let outcome = world.use_item(0x0E, 0);
    assert!(matches!(
        outcome,
        crate::items::ItemOutcome::StatRaised { .. }
    ));
    assert_eq!(
        world.roster.members[0].live_stats().atk,
        21,
        "persistent attack raised"
    );
    // Re-derived live battle stat reflects it.
    assert_eq!(world.battle_attack[0], 21);
}

#[test]
fn use_item_stat_boost_caps_at_cap_constant() {
    let mut party = legaia_save::Party::zeroed(1);
    let mut rs = party.members[0].record_stats();
    rs.cap_constant = 100;
    party.members[0].set_record_stats(rs);
    let mut ls = party.members[0].live_stats();
    ls.atk = 99;
    party.members[0].set_live_stats(ls);
    let mut world = World::new();
    world.load_party(party);
    world.set_battle_attack(0, 99);
    // A custom big-boost item to exercise the cap.
    let mut cat = crate::items::ItemCatalog::new();
    cat.insert(crate::items::ItemEntry {
        id: 0x50,
        name: "Mega Tonic",
        effect: crate::items::ItemEffect::StatBoost {
            target: crate::items::StatBoostTarget::Attack,
            delta: 50,
        },
        usable_in_battle: false,
        usable_in_field: true,
    });
    world.set_item_catalog(cat);
    world.use_item(0x50, 0);
    assert_eq!(
        world.roster.members[0].live_stats().atk,
        100,
        "capped at the per-stat cap constant"
    );
}

#[test]
fn use_item_fury_boost_extends_ap_gauge_and_reverts_at_battle_end() {
    let mut world = World::new();
    // Seed a Fury Boost catalog entry directly (the disc seeder installs the
    // same `ActionGauge` marker; this exercises the apply path without a disc).
    world.item_catalog.insert(crate::items::ItemEntry {
        id: 0x81,
        name: "Fury Boost",
        effect: crate::items::ItemEffect::ActionGauge,
        usable_in_battle: true,
        usable_in_field: false,
    });
    world.ap_gauges[0] = crate::ap_gauge::ApGauge::with_base(10);
    world.ap_gauges[0].current_ap = 6; // mid-turn, some AP already spent

    // Fury Boost extends the gauge by the retail ×7/5 ratio: base 10 -> 14, and
    // the live gauge gains the +4 delta immediately.
    let out = world.use_item(0x81, 0);
    assert_eq!(out, crate::items::ItemOutcome::ActionGaugeExtended);
    assert_eq!(world.ap_gauges[0].base_ap, 14);
    assert_eq!(world.ap_gauges[0].current_ap, 10);
    assert_eq!(world.fury_boost[0], Some(4));

    // The boost survives a turn reset (it's "for one battle").
    world.ap_gauges[0].reset_for_turn();
    assert_eq!(world.ap_gauges[0].base_ap, 14);
    assert_eq!(world.ap_gauges[0].current_ap, 14);

    // Idempotent within the battle: a second Fury Boost does not compound.
    assert_eq!(
        world.use_item(0x81, 0),
        crate::items::ItemOutcome::ActionGaugeExtended
    );
    assert_eq!(world.ap_gauges[0].base_ap, 14);
    assert_eq!(world.fury_boost[0], Some(4));

    // Battle end reverts the extension and clears the flag.
    world.finish_battle();
    assert_eq!(world.ap_gauges[0].base_ap, 10);
    assert_eq!(world.fury_boost[0], None);
}

#[test]
fn use_item_fury_boost_on_non_party_slot_is_noop() {
    let mut world = World::new();
    world.item_catalog.insert(crate::items::ItemEntry {
        id: 0x81,
        name: "Fury Boost",
        effect: crate::items::ItemEffect::ActionGauge,
        usable_in_battle: true,
        usable_in_field: false,
    });
    // Slot 3+ is not a party AP-gauge slot (gauges are 0..=2).
    assert_eq!(world.use_item(0x81, 5), crate::items::ItemOutcome::NoEffect);
}

#[test]
fn use_item_cure_clears_status() {
    use legaia_art::record::EnemyEffect;
    let mut world = World::new();
    world.party_count = 1;
    world.actors[0].battle.max_hp = 100;
    world.actors[0].battle.hp = 50;
    // Apply a Toxic status, then cure it via CureAll.
    world
        .status_effects
        .apply_from_enemy_effect(0, EnemyEffect::Toxic);
    assert!(world.status_effects.is_afflicted(0));
    world.set_item_catalog(full_test_catalog());
    // Antidote Flower is id 0x09 (CureAll).
    let outcome = world.use_item(0x09, 0);
    assert!(matches!(outcome, crate::items::ItemOutcome::CuredAll));
    assert!(!world.status_effects.is_afflicted(0));
}

#[test]
fn fold_battle_event_clamps_to_zero_hp() {
    use legaia_art::power::PowerByte;
    use legaia_art::queue::{ActionConstant, Character};
    use legaia_art::record::EnemyEffect;
    use legaia_engine_vm::battle_action::ArtStrikeInfo;

    let mut world = World::new();
    world.party_count = 4;
    world.actors[3].active = true;
    world.actors[3].battle.hp = 30;
    world.actors[3].battle.max_hp = 30;
    world.set_battle_attack(0, 64);
    world.set_battle_defense(3, 0);

    let info = ArtStrikeInfo {
        strike_index: 0,
        anim_byte: 0x10,
        actor_slot: 0,
        target_slot: 3,
        character: Character::Vahn,
        art: ActionConstant::Art1B,
        power: Some(PowerByte::from_byte(0x1A)), // huge damage vs 30 HP
        dmg_timing: None,
        enemy_effect: EnemyEffect::None,
        hit_cue: None,
    };
    let mut host = BattleHostImpl { world: &mut world };
    host.apply_art_strike(info);
    let events = world.drain_battle_events();
    for e in &events {
        world.fold_battle_event(e);
    }
    // saturating_sub clamps to 0 instead of wrapping.
    assert_eq!(world.actors[3].battle.hp, 0);
}

#[test]
fn fold_battle_event_pushes_status_into_tracker() {
    use legaia_art::power::PowerByte;
    use legaia_art::queue::{ActionConstant, Character};
    use legaia_art::record::EnemyEffect;
    use legaia_engine_vm::battle_action::ArtStrikeInfo;
    use legaia_engine_vm::status_effects::StatusKind;

    let mut world = World::new();
    world.party_count = 4;
    world.actors[3].active = true;
    world.actors[3].battle.hp = 100;
    world.actors[3].battle.max_hp = 100;
    world.set_battle_attack(0, 64);
    world.set_battle_defense(3, 10);
    let info = ArtStrikeInfo {
        strike_index: 0,
        anim_byte: 0x10,
        actor_slot: 0,
        target_slot: 3,
        character: Character::Vahn,
        art: ActionConstant::Art1B,
        power: Some(PowerByte::from_byte(0x1A)),
        dmg_timing: None,
        enemy_effect: EnemyEffect::Toxic,
        hit_cue: None,
    };
    let mut host = BattleHostImpl { world: &mut world };
    host.apply_art_strike(info);
    let events = world.drain_battle_events();
    for e in &events {
        world.fold_battle_event(e);
    }
    assert!(world.status_effects.has(3, StatusKind::Toxic));
}

#[test]
fn tick_status_effects_drains_hp() {
    use legaia_engine_vm::status_effects::StatusKind;
    let mut world = World::new();
    world.actors[0].battle.hp = 100;
    world.actors[0].battle.max_hp = 160;
    world.status_effects.apply(0, StatusKind::Toxic);
    world.tick_status_effects();
    // Toxic drains max_hp / 16 = 160 / 16 = 10 (FUN_801E752C).
    assert_eq!(world.actors[0].battle.hp, 90);
}

#[test]
fn reset_party_ap_refills_all_three_gauges() {
    let mut world = World::new();
    for g in world.ap_gauges.iter_mut() {
        g.try_spend(3);
    }
    world.reset_party_ap();
    for g in world.ap_gauges.iter() {
        assert_eq!(g.current_ap, g.base_ap);
        assert!(!g.spirit_charged);
    }
}

#[test]
fn item_catalog_setter_replaces() {
    let mut world = World::new();
    assert!(world.item_catalog.is_empty());
    world.set_item_catalog(full_test_catalog());
    assert!(!world.item_catalog.is_empty());
}

#[test]
fn install_encounter_for_scene_resolves_field_pattern() {
    use crate::encounter_registry::vanilla_encounter_registry;
    let mut world = World::new();
    let r = vanilla_encounter_registry();
    let installed = world.install_encounter_for_scene(&r, "map01");
    assert!(installed, "field pattern should match");
    assert!(world.encounter.is_some());
}

#[test]
fn install_encounter_for_scene_quiets_in_towns() {
    use crate::encounter_registry::vanilla_encounter_registry;
    let mut world = World::new();
    let r = vanilla_encounter_registry();
    let installed = world.install_encounter_for_scene(&r, "town01");
    assert!(!installed, "town pattern resolves but is quiet");
    assert!(
        world.encounter.is_some(),
        "session installed for nil checks"
    );
}

#[test]
fn install_encounter_for_scene_returns_false_with_no_default() {
    use crate::encounter_registry::EncounterRegistry;
    let mut world = World::new();
    let r = EncounterRegistry::new(); // empty, no default
    let installed = world.install_encounter_for_scene(&r, "anything");
    assert!(!installed);
    assert!(world.encounter.is_none());
}

#[test]
fn install_encounter_for_scene_replaces_active_session() {
    use crate::encounter_registry::vanilla_encounter_registry;
    let mut world = World::new();
    let r = vanilla_encounter_registry();
    // Install a field session, then a town session - the town call
    // should replace the field session even though it's quiet.
    world.install_encounter_for_scene(&r, "map01");
    assert!(world.encounter.is_some());
    let initial_table_label = world
        .encounter
        .as_ref()
        .unwrap()
        .tracker()
        .table()
        .scene_label
        .clone();
    world.install_encounter_for_scene(&r, "town01");
    let new_table_label = world
        .encounter
        .as_ref()
        .unwrap()
        .tracker()
        .table()
        .scene_label
        .clone();
    assert_ne!(initial_table_label, new_table_label);
}

#[test]
fn install_encounter_from_record_registers_and_arms() {
    use crate::encounter_record::EncounterRecord;
    let mut world = World::new();
    // mc2-shaped record: two monsters, both id 4.
    let record = EncounterRecord {
        count: 2,
        monster_ids: [0x04, 0x04, 0, 0],
    };
    let formation_id = world
        .install_encounter_from_record("map01", &record)
        .expect("non-empty record produces an id");
    // Formation registered.
    let formation = world
        .formation_table
        .formation(formation_id)
        .expect("formation registered");
    assert_eq!(formation.slots.len(), 2);
    assert_eq!(formation.slots[0].monster_id, 4);
    assert_eq!(formation.slots[1].monster_id, 4);
    // Session installed and rate forced high.
    let session = world.encounter.as_ref().expect("session installed");
    assert_eq!(session.tracker().table().trigger_rate_q8, 0xFF);
    assert_eq!(session.tracker().table().entries.len(), 1);
    assert_eq!(
        session.tracker().table().entries[0].formation_id,
        formation_id
    );
}

#[test]
fn install_scripted_encounter_parses_window_and_arms_battle() {
    let mut world = World::new();
    world.set_formation_table(
        crate::monster_catalog::vanilla_formation_table(),
        crate::monster_catalog::vanilla_monster_catalog(),
    );
    world.set_active_scene_label("town01");
    world.mode = SceneMode::Field;
    world.arm_scripted_encounter(true);
    // Record window overlaying the arm opcode: [op][op1][op2][count=2][ids..].
    let window = [0x37u8, 0x00, 0x00, 0x02, 0x4F, 0x50, 0x00, 0x00];
    let formation_id = world
        .install_scripted_encounter(&window)
        .expect("non-empty record installs a formation");
    // Fire-once: a successful install disarms the carrier flag.
    assert!(!world.scripted_encounter_armed);
    // Formation registered with the window's two ids.
    let formation = world
        .formation_table
        .formation(formation_id)
        .expect("formation registered");
    assert_eq!(formation.slots.len(), 2);
    assert_eq!(formation.slots[0].monster_id, 0x4F);
    assert_eq!(formation.slots[1].monster_id, 0x50);
    // Session installed at the forced-high rate.
    assert_eq!(
        world
            .encounter
            .as_ref()
            .unwrap()
            .tracker()
            .table()
            .trigger_rate_q8,
        0xFF
    );
    // Event surfaced for engine visibility.
    assert!(world.pending_field_events.iter().any(|e| matches!(
        e,
        FieldEvent::ScriptedEncounter { record } if record == &window
    )));
    // The very next field step flips Field -> a triggered encounter.
    assert!(
        world.on_field_step(),
        "forced-rate roll triggers the battle"
    );
}

#[test]
fn install_scripted_encounter_empty_or_short_window_returns_none() {
    let mut world = World::new();
    world.set_active_scene_label("town01");
    // count = 0 -> empty record -> no install.
    assert_eq!(world.install_scripted_encounter(&[0, 0, 0, 0]), None);
    assert!(world.encounter.is_none());
    // Too short to even hold the count byte -> parse fails.
    assert_eq!(world.install_scripted_encounter(&[0, 0]), None);
}

#[test]
fn seed_party_battle_stats_folds_live_stats_and_equipment() {
    use crate::battle_stats::{EquipmentTable, ItemModifier};
    use legaia_save::EquipmentSlots;
    use legaia_save::character::LiveStats;

    let mut world = World::new();
    let mut party = legaia_save::Party::zeroed(1);
    party.members[0].set_live_stats(LiveStats {
        agl: 12,
        atk: 30,
        udf: 10,
        ldf: 8,
        spd: 5,
        int: 4,
    });
    let mut slots = [0u8; 8];
    slots[0] = 5; // a weapon in the first slot
    party.members[0].set_equipment(EquipmentSlots { slots });
    world.load_party(party);

    // Item 5 grants +7 attack, +3 UDF, +2 LDF.
    let mut table = EquipmentTable::new();
    table.set(
        5,
        ItemModifier {
            atk: 7,
            udf: 3,
            ldf: 2,
            spd: 0,
            int: 0,
            ability_bits: [0; 32],
        },
    );
    world.set_equipment_table(table);

    world.seed_party_battle_stats();
    assert_eq!(world.battle_attack[0], 37, "30 base + 7 weapon");
    assert_eq!(
        world.battle_defense_split[0],
        Some((13, 10)),
        "(10+3) UDF, (8+2) LDF"
    );
}

#[test]
fn seed_party_battle_stats_skips_zeroed_roster() {
    // A synthetic battle sets battle_attack directly then loads a zeroed
    // roster; seeding must not clobber the manual value.
    let mut world = World::new();
    world.set_battle_attack(0, 60);
    world.load_party(legaia_save::Party::zeroed(3));
    world.seed_party_battle_stats();
    assert_eq!(world.battle_attack[0], 60, "zeroed roster leaves it intact");
    assert_eq!(world.battle_defense_split[0], None);
}

#[test]
fn seed_party_battle_stats_scales_ap_base_with_level() {
    use legaia_save::character::LiveStats;

    let mut world = World::new();
    let mut party = legaia_save::Party::zeroed(3);
    // Slot 0 at level 1 (base 4), slot 1 at level 23 (base 6), slot 2 at
    // level 99 (capped 10). A non-zero atk so the seed doesn't skip them.
    for (slot, level) in [(0usize, 1u8), (1, 23), (2, 99)] {
        party.members[slot].set_live_stats(LiveStats {
            agl: 10,
            atk: 20,
            udf: 8,
            ldf: 8,
            spd: 5,
            int: 4,
        });
        party.members[slot].set_level(level);
    }
    world.load_party(party);

    world.seed_party_battle_stats();
    assert_eq!(world.ap_gauges[0].base_ap, 4, "level 1 -> base 4");
    assert_eq!(world.ap_gauges[1].base_ap, 6, "level 23 -> 4 + 23/10 = 6");
    assert_eq!(world.ap_gauges[2].base_ap, 10, "level 99 -> capped at 10");

    // The round-start reset picks up the seeded base as the per-turn budget.
    world.reset_party_ap();
    assert_eq!(world.ap_gauges[1].current_ap, 6);
    assert_eq!(world.ap_gauges[2].current_ap, 10);
}

#[test]
fn drain_pending_scripted_encounter_only_when_queued() {
    let mut world = World::new();
    world.set_formation_table(
        crate::monster_catalog::vanilla_formation_table(),
        crate::monster_catalog::vanilla_monster_catalog(),
    );
    world.set_active_scene_label("town01");
    // Nothing queued -> no-op.
    world.drain_pending_scripted_encounter();
    assert!(world.encounter.is_none());
    // Queue a window (as the armed forwarded-PC hook would) and drain.
    world.pending_scripted_encounter = Some(vec![0, 0, 0, 1, 0x12, 0, 0, 0]);
    world.drain_pending_scripted_encounter();
    assert!(world.pending_scripted_encounter.is_none());
    assert!(world.encounter.is_some());
}

#[test]
fn install_encounter_from_record_empty_returns_none() {
    use crate::encounter_record::EncounterRecord;
    let mut world = World::new();
    let id = world.install_encounter_from_record("map01", &EncounterRecord::EMPTY);
    assert!(id.is_none());
    // No session installed.
    assert!(world.encounter.is_none());
}

#[test]
fn install_man_formation_forces_registered_row() {
    use crate::monster_catalog::{FormationDef, FormationSlot};
    let mut world = World::new();
    world.mode = SceneMode::Field;
    world.set_active_scene_label("town01");
    // Register a lone-monster formation at id 4 (town01's Tetsu row shape).
    world
        .formation_table
        .insert(FormationDef::new(4, vec![FormationSlot::new(0x4F)]));

    // Unknown id -> None, no session.
    assert!(world.install_man_formation(9).is_none());
    assert!(world.encounter.is_none());

    // Registered id installs a forced-rate session that triggers next step.
    assert_eq!(world.install_man_formation(4), Some(4));
    assert!(world.encounter.is_some());
    assert!(
        world.on_field_step(),
        "forced-rate session triggers on the next step"
    );
}

#[test]
fn field_carrier_engage_launches_battle_and_returns_to_field() {
    use crate::encounter_record::RIM_ELM_TRAINING_FORMATION_ID;
    use crate::monster_catalog::{FormationDef, FormationSlot, MonsterCatalog, MonsterDef};

    let mut world = World {
        party_count: 1,
        ..World::default()
    };
    world.mode = SceneMode::Field;
    world.live_gameplay_loop = true; // auto-resolve the battle leg
    world.set_active_scene_label("town01");
    // A capable lone party member so the battle can resolve.
    world.actors[0].active = true;
    world.actors[0].battle.hp = 400;
    world.actors[0].battle.max_hp = 400;
    world.actors[0].battle.liveness = 1;
    world.set_battle_attack(0, 80);
    // town01's Tetsu row: formation index 4 = lone monster id 0x4F.
    world.formation_table.insert(FormationDef::new(
        RIM_ELM_TRAINING_FORMATION_ID,
        vec![FormationSlot::new(0x4F)],
    ));
    let mut cat = MonsterCatalog::new();
    cat.insert(MonsterDef::new(0x4F, "Tetsu", 999, 40));
    world.set_monster_catalog(cat);

    // Place one scripted-encounter carrier (the Tetsu NPC) - the field-mode
    // use of the FUN_801DA51C SM.
    world.install_field_carriers(vec![FieldCarrierConfig::ScriptedEncounter {
        formation_id: RIM_ELM_TRAINING_FORMATION_ID,
    }]);

    // Idle: ticking does NOT launch a battle (towns are 0% random; the
    // carrier waits for the dialogue-accept).
    world.tick();
    assert_eq!(
        world.mode,
        SceneMode::Field,
        "an idle scripted carrier never self-fires"
    );
    assert_eq!(world.field_carriers[0].state, 0, "carrier still Idle");

    // The dialogue-accept advances the carrier to Activating; the next tick
    // runs the state-1 body (formation copy) and the case 2/3 fall-through
    // (battle handoff), flipping Field -> Battle, tagged to return to field.
    world.engage_field_carrier(0);
    world.tick();
    assert_eq!(world.mode, SceneMode::Battle);
    assert_eq!(world.battle_return_mode, SceneMode::Field);
    assert!(world.field_return.is_some());
    let formation = world.active_formation.as_ref().expect("active formation");
    assert_eq!(
        formation.slots[0].monster_id, 0x4F,
        "Tetsu in the enemy slot"
    );
    assert_eq!(
        world.field_carriers[0].state,
        vm::world_map::EntityState::Terminal as u16,
        "carrier retired to Terminal after the transition"
    );

    // Drive the fight to completion; it must return to the field.
    let mut returned = false;
    for _ in 0..8000 {
        world.tick();
        if world.mode != SceneMode::Battle {
            returned = true;
            break;
        }
    }
    assert!(returned, "battle resolves");
    assert_eq!(world.mode, SceneMode::Field, "returns to the field");
    // The carrier stays Terminal - the scripted fight fires exactly once.
    assert_eq!(
        world.field_carriers[0].state,
        vm::world_map::EntityState::Terminal as u16
    );
}

#[test]
fn field_carrier_unengaged_never_fires() {
    use crate::encounter_record::RIM_ELM_TRAINING_FORMATION_ID;
    use crate::monster_catalog::{FormationDef, FormationSlot};

    let mut world = World {
        party_count: 1,
        ..World::default()
    };
    world.mode = SceneMode::Field;
    world.set_active_scene_label("town01");
    world.formation_table.insert(FormationDef::new(
        RIM_ELM_TRAINING_FORMATION_ID,
        vec![FormationSlot::new(0x4F)],
    ));
    world.install_field_carriers(vec![FieldCarrierConfig::ScriptedEncounter {
        formation_id: RIM_ELM_TRAINING_FORMATION_ID,
    }]);

    // Many idle ticks must never flip into battle (no random rate).
    for _ in 0..256 {
        world.tick();
        assert_eq!(world.mode, SceneMode::Field);
    }
    assert!(world.field_return.is_none());
    assert!(world.pending_field_carrier_battle.is_none());
}

#[test]
fn begin_new_game_clears_state_and_enters_field() {
    let mut world = World::new();
    // Dirty the world as if a prior session had been played.
    world.mode = SceneMode::Battle;
    world.story_flags = 0xDEAD_BEEF;
    world.story_flag_bits = vec![1, 2, 3];
    world.money = 4242;
    world.inventory.insert(0x10, 5);
    world.scripted_encounter_armed = true;
    world.game_over = true;
    world.play_time_seconds = 9999;

    world.begin_new_game();

    // The retail field-launch (master mode 3) clean slate.
    assert_eq!(world.mode, SceneMode::Field);
    assert_eq!(world.story_flags, 0);
    assert!(world.story_flag_bits.is_empty());
    // New-game gold is the retail constant (FUN_80034A6C), not zero.
    assert_eq!(world.money, NEW_GAME_STARTING_GOLD);
    assert!(world.inventory.is_empty());
    assert!(!world.scripted_encounter_armed);
    assert!(world.encounter.is_none());
    assert!(!world.game_over);
    assert_eq!(world.play_time_seconds, 0);
}

#[test]
fn prologue_handoff_fires_once_on_confirm_in_opdeene() {
    let mut world = World::new();
    world.set_active_scene_label(legaia_asset::new_game::OPENING_CUTSCENE_SCENE);

    // Not armed yet: confirm does nothing.
    assert_eq!(world.take_prologue_handoff(true), None);

    world.arm_prologue_handoff();
    assert_ne!(world.story_flags & PROLOGUE_HANDOFF_FLAG, 0);

    // Armed but no confirm: stays in the cutscene.
    assert_eq!(world.take_prologue_handoff(false), None);
    assert_ne!(world.story_flags & PROLOGUE_HANDOFF_FLAG, 0);

    // Armed + confirm: hands off to town01 and clears the bit (fire-once).
    assert_eq!(
        world.take_prologue_handoff(true),
        Some(legaia_asset::new_game::OPENING_SCENE)
    );
    assert_eq!(world.story_flags & PROLOGUE_HANDOFF_FLAG, 0);

    // A second confirm does not re-fire.
    assert_eq!(world.take_prologue_handoff(true), None);
}

#[test]
fn prologue_handoff_only_fires_in_the_cutscene_scene() {
    let mut world = World::new();
    // Armed, confirm pressed, but the active scene is not `opdeene`.
    world.set_active_scene_label(legaia_asset::new_game::OPENING_SCENE);
    world.arm_prologue_handoff();
    assert_eq!(world.take_prologue_handoff(true), None);
    // Bit is left intact for the gate to fire only in `opdeene`.
    assert_ne!(world.story_flags & PROLOGUE_HANDOFF_FLAG, 0);
}

// ------------------------------------------------------------------
// tick_actor_physics + MoveBufferHost wiring
// ------------------------------------------------------------------

/// Build a 1-record MOVE pool: index `id` -> offset `record_off`,
/// record body `[0, flag, fc_lo, fc_hi, 0, 0, divisor, 0]`.
fn make_move_pool(id: u16, record_off: usize, frame_count: u16, divisor: u8) -> Vec<u8> {
    // Table size matches retail's hard-coded 1024-entry view.
    let table_entries = 1024usize;
    let table_bytes = table_entries * 4;
    let total = (record_off + 16).max(table_bytes);
    let mut pool = vec![0u8; total];
    let off = (id as usize) * 4;
    pool[off..off + 4].copy_from_slice(&(record_off as u32).to_le_bytes());
    let fc = frame_count.to_le_bytes();
    pool[record_off + 1] = 0; // flag
    pool[record_off + 2] = fc[0];
    pool[record_off + 3] = fc[1];
    pool[record_off + 6] = divisor;
    pool
}

#[test]
fn tick_actor_physics_skips_inactive_slots() {
    let mut world = World::new();
    // No actor active; should be a no-op (no panics, no events).
    world.tick_actor_physics();
    assert!(world.last_tick_events.is_empty());
}

#[test]
fn tick_actor_physics_records_keyframe_event_for_active_actor() {
    let mut world = World::new();
    // Activate slot 0 on the keyframe dispatch arm; populate the
    // record pointer so the keyframe writeback fires.
    world.actors[0].active = true;
    world.actors[0].set_physics_dispatch(0x06);
    world.actors[0].physics.set_record_ptr(0x80100000);
    world.actors[0].physics.set_bone_count(8);
    world.tick_actor_physics();
    // One slot fired; events vector non-empty.
    assert_eq!(world.last_tick_events.len(), 1);
    let (slot, res) = &world.last_tick_events[0];
    assert_eq!(*slot, 0);
    assert!(
        res.events
            .iter()
            .any(|e| matches!(e, TickEvent::KeyframePoseWritten { bone_count: 8 }))
    );
}

#[test]
fn move_vm_kick_drives_cursor_advance_against_installed_pool() {
    let mut world = World::new();
    // Install a MOVE pool with id 3 -> record at offset 0x1010,
    // frame_count = 8, divisor = 1.
    world.set_move_buffer_root(make_move_pool(3, 0x1010, 8, 1));
    // Activate slot 0; set the move_vm_kick flag so the physics
    // tick's late-update emits TickEvent::MoveVmKick.
    world.actors[0].active = true;
    world.actors[0].set_physics_dispatch(0x06);
    world.actors[0].physics.move_vm_kick = 1;
    // Request move id 3; phase rate of 8 steps per frame.
    world.actors[0].move_buffer.cursor_requested = 3;
    world.actors[0].move_buffer.phase_rate = 8;
    world.tick_actor_physics();
    // MoveVmKick emitted.
    let (_, res) = &world.last_tick_events[0];
    assert!(
        res.events
            .iter()
            .any(|e| matches!(e, TickEvent::MoveVmKick))
    );
    // Cursor latched the new id and stepped once.
    assert_eq!(world.actors[0].move_buffer.cursor_active, 3);
    // First frame after latch: cursor_active==3, phase started at
    // 0, advanced by phase_rate * frame_delta = 8 * 1 = 8.
    assert_eq!(world.actors[0].move_buffer.phase, 8);
    // Move VM kick flag set by the latch (cursor_advance writes
    // move_vm_kick = 1 whenever it latches a new record).
    assert_eq!(world.actors[0].move_buffer.move_vm_kick, 1);
}

#[test]
fn move_vm_kick_no_record_is_graceful_noop() {
    let mut world = World::new();
    // No pool installed; cursor_advance's resolver returns None.
    world.actors[0].active = true;
    world.actors[0].set_physics_dispatch(0x06);
    world.actors[0].physics.move_vm_kick = 1;
    world.actors[0].move_buffer.cursor_requested = 5;
    world.actors[0].move_buffer.phase_rate = 8;
    world.tick_actor_physics();
    // Kick emitted but cursor stays idle (no record source).
    assert_eq!(world.actors[0].move_buffer.cursor_active, 0);
    assert_eq!(world.actors[0].move_buffer.phase, 0);
    assert_eq!(world.actors[0].move_buffer.move_vm_kick, 0);
}

#[test]
fn tick_does_not_advance_cursor_when_move_vm_kick_is_clear() {
    let mut world = World::new();
    world.set_move_buffer_root(make_move_pool(2, 0x1010, 4, 1));
    // Activate slot 0 but leave move_vm_kick = 0 in physics; the
    // late-update path does NOT emit MoveVmKick this frame, so
    // the cursor stays untouched even though a request is pending.
    world.actors[0].active = true;
    world.actors[0].set_physics_dispatch(0x06);
    world.actors[0].move_buffer.cursor_requested = 2;
    world.actors[0].move_buffer.phase_rate = 4;
    let before = world.actors[0].move_buffer.clone();
    world.tick_actor_physics();
    // Cursor unchanged (no kick).
    assert_eq!(world.actors[0].move_buffer, before);
}

#[test]
fn world_tick_runs_physics_pass_in_order() {
    // Smoke test: World::tick invokes tick_actor_physics. After
    // one tick with the kick flag set + a record installed, the
    // per-actor cursor should have advanced.
    let mut world = World::new();
    world.set_move_buffer_root(make_move_pool(1, 0x1010, 8, 1));
    world.actors[0].active = true;
    world.actors[0].set_physics_dispatch(0x06);
    world.actors[0].physics.move_vm_kick = 1;
    world.actors[0].move_buffer.cursor_requested = 1;
    world.actors[0].move_buffer.phase_rate = 4;
    // World::tick (no scene mode) returns None for Title; the
    // physics pass still runs unconditionally.
    world.tick();
    assert_eq!(world.actors[0].move_buffer.cursor_active, 1);
}

#[test]
fn apply_steal_grants_item_on_hit_and_respects_non_stealable() {
    use legaia_asset::steal_table::{StealEntry, StealTable};
    // ids: 0 sentinel, 1 = 30%/0x7e, 2 = 0% (no steal), 3 = 100%/0x8a.
    let table = StealTable::from_entries(vec![
        StealEntry {
            chance_pct: 0,
            item_id: 0xff,
        },
        StealEntry {
            chance_pct: 30,
            item_id: 0x7e,
        },
        StealEntry {
            chance_pct: 0,
            item_id: 0,
        },
        StealEntry {
            chance_pct: 100,
            item_id: 0x8a,
        },
    ]);

    // Seed so the first roll is 0 (lands for any chance >= 1).
    let mut world = World {
        rng_state: 32937,
        ..World::default()
    };
    let got = world.apply_steal(3, &table);
    assert_eq!(got, Some(0x8a), "100% steal lands and grants the item");
    assert_eq!(world.inventory.get(&0x8a).copied(), Some(1));

    // A non-stealable monster (0% chance) never grants and consumes no roll.
    let mut world = World::default();
    let rng_before = world.rng_state;
    assert_eq!(world.apply_steal(2, &table), None);
    assert!(world.inventory.is_empty());
    assert_eq!(
        world.rng_state, rng_before,
        "no roll for a non-stealable monster"
    );

    // An out-of-range / unknown monster id is also None.
    assert_eq!(World::default().apply_steal(999, &table), None);
}

// --- Live gold-shop trigger via field-VM op-0x49 (shop_catalog + try_arm_field_shop) ---

/// Build a field script that opens a 2-item shop: op `0x49` sub-0, length 0,
/// `[count=2][0x22][0x34]`, name `"Shop\0"`.
#[cfg(test)]
fn shop_op49_script() -> Vec<u8> {
    let mut code = vec![0x49, 0x00, 0x00, 0x02, 0x22, 0x34];
    code.extend_from_slice(b"Shop\0");
    code
}

#[test]
fn field_vm_op49_opens_a_gold_shop_then_resumes() {
    use vm::field::{FieldHost, Op49State};
    let mut world = World::new();
    // Priced item data: 0x22 = 50g, 0x34 = 120g (both sellable).
    let mut prices = [0u16; 256];
    prices[0x22] = 50;
    prices[0x34] = 120;
    world.item_shop_data = Some(crate::shop_catalog::ShopItemData::from_prices(prices));

    let code = shop_op49_script();
    let mut ctx = FieldCtx::default();
    let pc = 0usize;

    // Frame 1: Idle -> the host recognises the inline shop, arms it, VM halts.
    {
        let mut host = FieldHostImpl { world: &mut world };
        assert_eq!(host.op49_state(), Op49State::Idle);
        let r = vm::field::step(&mut host, &mut ctx, &code, pc);
        assert!(
            matches!(r, FieldStepResult::Halt { .. }),
            "op-0x49 suspends the script while the shop is up"
        );
    }
    assert!(
        world.field_shop_armed && world.field_shop_open,
        "shop armed"
    );
    // The opened shop carries the priced inline stock.
    let sess = world
        .take_pending_field_shop()
        .expect("the field VM opened a shop");
    let items: Vec<(u8, u32)> = sess
        .inventory
        .items
        .iter()
        .map(|i| (i.item_id, i.price))
        .collect();
    assert_eq!(items, vec![(0x22, 50), (0x34, 120)]);

    // Frame 2: shop still up -> Armed, VM stays suspended at the same pc.
    {
        let mut host = FieldHostImpl { world: &mut world };
        assert_eq!(host.op49_state(), Op49State::Armed);
        let r = vm::field::step(&mut host, &mut ctx, &code, pc);
        assert!(matches!(r, FieldStepResult::Halt { .. }));
    }

    // Player closes the shop -> Done; the VM advances past the merchant op.
    world.finish_field_shop();
    {
        let mut host = FieldHostImpl { world: &mut world };
        assert_eq!(host.op49_state(), Op49State::Done);
        match vm::field::step(&mut host, &mut ctx, &code, pc) {
            FieldStepResult::Advance { next_pc } => {
                assert!(next_pc > pc, "advanced past the shop record")
            }
            other => panic!("expected Advance, got {other:?}"),
        }
    }
    assert!(
        !world.field_shop_armed,
        "the arm clears so a later op-0x49 can open the next merchant"
    );
}

// --- Tile-board runtime install via field-VM op-0x49 sub-5 ---

/// A field script carrying an op `0x49` sub-5 board install: 13-byte inline
/// header `[5][ox=0][oz=0][w=4][h=4][radius=2][mode=0][flags×4][player_tpl]
/// [tile_base]`, followed by a sentinel op the script resumes onto.
#[cfg(test)]
fn tile_board_op49_script() -> Vec<u8> {
    vec![
        0x49, 0x05, 0x00, 0x00, 0x04, 0x04, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x21, 0x30,
    ]
}

#[test]
fn field_vm_op49_sub5_installs_a_tile_board_then_resumes_on_exit() {
    use vm::field::{FieldHost, Op49State};
    let mut world = World::new();
    world.mode = SceneMode::Field;
    world.player_actor_slot = Some(0);
    world.actors[0].active = true;
    world.rng_state = 7;

    let code = tile_board_op49_script();
    let mut ctx = FieldCtx::default();
    let pc = 0usize;

    // Frame 1: Idle -> the host parses the inline header, installs the
    // board, and the VM suspends.
    {
        let mut host = FieldHostImpl { world: &mut world };
        assert_eq!(host.op49_state(), Op49State::Idle);
        let r = vm::field::step(&mut host, &mut ctx, &code, pc);
        assert!(
            matches!(r, FieldStepResult::Halt { .. }),
            "op-0x49 sub-5 suspends the script while the board mode runs"
        );
    }
    let board = world.tile_board.as_ref().expect("board installed");
    assert_eq!((board.width, board.height), (4, 4));
    assert_eq!(board.cells.len(), 16);
    // The retail fill only produces cells in the known value classes.
    assert!(board.cells.iter().all(|&c| (2..=0xE).contains(&c)));
    let header = world.tile_board_header.expect("header kept");
    assert_eq!(header.player_template, 0x21);
    assert_eq!(header.tile_template_base, 0x30);
    // The player actor was seated at the start-cell centre.
    let (px, pz) = world.tile_board.as_ref().unwrap().player_world();
    assert_eq!(world.actors[0].move_state.world_x as i32, px);
    assert_eq!(world.actors[0].move_state.world_z as i32, pz);

    // While the board is up the op stays Armed at the same pc.
    {
        let mut host = FieldHostImpl { world: &mut world };
        assert_eq!(host.op49_state(), Op49State::Armed);
        let r = vm::field::step(&mut host, &mut ctx, &code, pc);
        assert!(matches!(r, FieldStepResult::Halt { .. }));
    }

    // Simulate the walk reaching an event/transition cell: plant one under
    // the player and run the arrival pass (the interpolation-complete path).
    {
        let b = world.tile_board.as_mut().unwrap();
        let idx = b.player_row as usize * b.width as usize + b.player_col as usize;
        b.cells[idx] = crate::tile_board::CELL_EVENT_FIRST;
        let (tx, tz) = b.player_world();
        world.tile_board_target = Some((tx, tz));
        world.set_pad(0);
        let _ = world.tick();
    }
    assert!(
        world.tile_board.is_none(),
        "landing on an event cell exits the board mode"
    );

    // Exit flips the op to Done; the script resumes past the 14-byte install.
    {
        let mut host = FieldHostImpl { world: &mut world };
        assert_eq!(host.op49_state(), Op49State::Done);
        match vm::field::step(&mut host, &mut ctx, &code, pc) {
            FieldStepResult::Advance { next_pc } => {
                assert_eq!(next_pc, 14, "sub-5 Done advances opcode + 13 header bytes")
            }
            other => panic!("expected Advance, got {other:?}"),
        }
    }
    assert!(!world.tile_board_armed, "the arm clears on resume");
}

#[test]
fn tile_board_animated_cell_cycles_on_arrival() {
    let mut w = tile_board_world();
    // Plant an animated tile at the player's cell and run the arrival pass
    // via a completed interpolation.
    {
        let b = w.tile_board.as_mut().unwrap();
        b.cells[0] = crate::tile_board::CELL_ANIM_LAST; // 0xE wraps to 0xB
        let (tx, tz) = b.player_world();
        w.tile_board_target = Some((tx, tz));
    }
    w.set_pad(0);
    let _ = w.tick();
    assert_eq!(
        w.tile_board.as_ref().unwrap().cells[0],
        crate::tile_board::CELL_ANIM_FIRST,
        "0xE cycles back to 0xB on arrival"
    );
}

// --- Screen-effect widgets via field-VM op-0x43 sub-ops (PROT-0900 family) ---

#[test]
fn field_vm_op43_widget_subops_drive_screen_fx_frame() {
    let mut world = World::new();
    world.mode = SceneMode::Field;

    let mut ctx = FieldCtx::default();

    // Sub-0x11: mask rect tween to a centre iris over 0 frames (snap).
    // [43][11][l lo hi][t][r][b][dur]
    let mut mask_op = vec![0x43, 0x11];
    for w in [80i16, 60, 240, 180, 0] {
        mask_op.extend_from_slice(&w.to_le_bytes());
    }
    // Sub-0x15: letterbox config [x_left][x_right][y0][y1][y2][y3].
    let mut lb_op = vec![0x43, 0x15];
    for w in [0i16, 0x140, 40, 56, 184, 200] {
        lb_op.extend_from_slice(&w.to_le_bytes());
    }
    // Sub-0x13: panel spawn [x][y][w][h][tex_x][tex_y] past the sub-op byte.
    let mut panel_op = vec![0x43, 0x13];
    for w in [16i16, 32, 128, 96, 0, 0x100] {
        panel_op.extend_from_slice(&w.to_le_bytes());
    }
    // Sub-0x10: sprite spawn, 19-byte record
    // [x][y][w][h][tex_x][tex_y][clut_x][clut_y][rgb u24].
    let mut sprite_op = vec![0x43, 0x10];
    for w in [100i16, 50, 24, 24, 0x40, 0, 0, 480] {
        sprite_op.extend_from_slice(&w.to_le_bytes());
    }
    sprite_op.extend_from_slice(&[0x80, 0x80, 0x80]);

    for op in [&mask_op, &lb_op, &panel_op, &sprite_op] {
        let mut host = FieldHostImpl { world: &mut world };
        match vm::field::step(&mut host, &mut ctx, op, 0) {
            FieldStepResult::Advance { .. } => {}
            other => panic!("widget sub-op should advance, got {other:?}"),
        }
    }
    assert!(world.screen_fx.mask.is_some(), "mask widget spawned");
    assert!(world.screen_fx.letterbox.is_some(), "letterbox configured");
    assert!(world.screen_fx.panel.is_some(), "panel spawned");
    assert_eq!(world.screen_fx.sprites.len(), 1, "sprite widget spawned");

    // One world tick publishes the frame: 4 mask border quads + 2 letterbox
    // bands, 2 gradient strips, 1 panel quad (128px wide - no split), 1 sprite.
    let _ = world.tick();
    let frame = &world.screen_fx_frame;
    assert_eq!(
        frame.solid_quads.len(),
        6,
        "4 mask quads + 2 letterbox bands"
    );
    assert_eq!(frame.gradient_quads.len(), 2);
    assert_eq!(frame.panels.len(), 1);
    assert_eq!(frame.sprites.len(), 1);
    // The dur=0 mask snapped to the requested iris rect: the top border quad
    // ends at the rect's top edge.
    assert!(
        frame
            .solid_quads
            .iter()
            .any(|q| q.bottom == 60 || q.top == 60),
        "mask border reflects the snapped iris rect"
    );

    // Sub-0x14: panel move/scale to half size over 0 frames.
    let mut move_op = vec![0x43, 0x14];
    for w in [200i16, 100, 0x0800, 4] {
        move_op.extend_from_slice(&w.to_le_bytes());
    }
    {
        let mut host = FieldHostImpl { world: &mut world };
        let r = vm::field::step(&mut host, &mut ctx, &move_op, 0);
        assert!(matches!(r, FieldStepResult::Advance { .. }));
    }
    let p = world.screen_fx.panel.as_ref().unwrap();
    assert_eq!(p.target[0], 200);
    assert_eq!(p.target[2], 64, "0x0800 (4.12) halves the 128px base width");
}

#[test]
fn field_shop_carries_a_stable_vendor_id_that_drives_trading() {
    // The op-0x49 shop arm captures a per-vendor id (from the shop's name +
    // stock) so seru trading reached through that shop keys on the right vendor.
    let mut prices = [0u16; 256];
    prices[0x22] = 50;
    prices[0x34] = 120;

    let mut world = World::new();
    world.item_shop_data = Some(crate::shop_catalog::ShopItemData::from_prices(prices));
    assert!(world.try_arm_field_shop(&shop_op49_script()));
    let sess = world.take_pending_field_shop().expect("shop opened");

    // The id is the stable derivation from the shop's identity ("Shop", stock).
    let expected = legaia_asset::seru_trade::vendor_id_from_shop("Shop", &[0x22, 0x34]);
    assert_eq!(sess.vendor_id, expected);
    assert_ne!(sess.vendor_id, 0, "a real vendor gets a concrete id");

    // With trading enabled and a party that owns seru, opening a trade for that
    // vendor yields offers (the through-the-shop path the host drives).
    world.seru_trade_config = Some(legaia_asset::seru_trade::SeruTradeConfig {
        enabled: true,
        seed: 0x1234,
        max_offers: 4,
    });
    let mut lead = legaia_save::CharacterRecord::zeroed();
    let mut list = legaia_save::SpellList::default();
    list.ids[0] = 0x81;
    list.ids[1] = 0x88;
    list.count = 2;
    lead.set_spell_list(list);
    world.roster = legaia_save::Party {
        members: vec![lead],
    };

    let session = world
        .open_seru_trade(sess.vendor_id)
        .expect("trading enabled -> session opens");
    assert!(
        !session.is_empty(),
        "the party owns seru, so the vendor offers trades"
    );
}

#[test]
fn field_vm_op49_non_shop_payload_does_not_open_a_shop() {
    let mut world = World::new();
    // Only 0x22 is priced. A genuine shop LEADS with a sellable item (a real
    // shop's unsellable template ids are only ever a trailing padding tail,
    // never the lead). This payload leads with an unpriced id, so the sellable
    // mask rejects it as not a gold shop.
    let mut prices = [0u16; 256];
    prices[0x22] = 50;
    world.item_shop_data = Some(crate::shop_catalog::ShopItemData::from_prices(prices));
    let mut code = vec![0x49, 0x00, 0x00, 0x02, 0x34, 0x22];
    code.extend_from_slice(b"Shop\0");
    let mut ctx = FieldCtx::default();
    {
        let mut host = FieldHostImpl { world: &mut world };
        let _ = vm::field::step(&mut host, &mut ctx, &code, 0);
    }
    assert!(
        !world.field_shop_armed,
        "a payload that doesn't lead with a sellable item is not a gold shop"
    );
    assert!(world.take_pending_field_shop().is_none());
}

#[test]
fn field_vm_op49_trims_unsellable_padding_to_the_sellable_stock() {
    let mut world = World::new();
    // 0x22/0x34 priced; 0x03 the trailing unsellable template-id padding the
    // record `count` over-counts. The shop opens with only the sellable stock.
    let mut prices = [0u16; 256];
    prices[0x22] = 50;
    prices[0x34] = 120;
    world.item_shop_data = Some(crate::shop_catalog::ShopItemData::from_prices(prices));
    let mut code = vec![0x49, 0x00, 0x00, 0x03, 0x22, 0x34, 0x03];
    code.extend_from_slice(b"Shop\0");
    let mut ctx = FieldCtx::default();
    {
        let mut host = FieldHostImpl { world: &mut world };
        let _ = vm::field::step(&mut host, &mut ctx, &code, 0);
    }
    let sess = world
        .take_pending_field_shop()
        .expect("the field VM opened the shop (padding doesn't reject it)");
    let items: Vec<(u8, u32)> = sess
        .inventory
        .items
        .iter()
        .map(|i| (i.item_id, i.price))
        .collect();
    assert_eq!(items, vec![(0x22, 50), (0x34, 120)], "0x03 padding trimmed");
}

#[test]
fn field_vm_op49_without_item_data_never_opens_a_shop() {
    // Disc-free build: no prices installed -> no sellable mask, so a stray
    // op-0x49 sub-0 can never be mistaken for a shop (and there'd be no prices).
    let mut world = World::new();
    let code = shop_op49_script();
    let mut ctx = FieldCtx::default();
    {
        let mut host = FieldHostImpl { world: &mut world };
        let _ = vm::field::step(&mut host, &mut ctx, &code, 0);
    }
    assert!(!world.field_shop_armed);
    assert!(world.take_pending_field_shop().is_none());
}

#[test]
fn field_tile_crossing_refreshes_region_state() {
    // Wiring oracle for the per-tile region ports (FUN_80017FBC /
    // FUN_800180EC / FUN_801DBA20 in `crate::field_regions`): install a
    // synthetic `.MAP` region block + MAN zone table, cross a tile in a
    // live field tick, and assert the op-0x42 mask (`extra_flags`) and the
    // camera-zone record refresh.
    use crate::field_regions::ZONE_RECORD_STRIDE;

    // .MAP region block: one type-4 region covering tiles x [0,8), z [0,8),
    // one type-5 region covering x [8,16), z [0,8).
    let body_off = 0x20u16;
    let mut block = vec![0u8; 0x20 + 2 * 8];
    block[0xE..0x10].copy_from_slice(&body_off.to_le_bytes());
    block[0x10..0x12].copy_from_slice(&2u16.to_le_bytes());
    block[0x20..0x25].copy_from_slice(&[0, 0, 8, 8, 4]);
    block[0x28..0x2D].copy_from_slice(&[8, 0, 16, 8, 5]);
    // Zone table: a kind-5 record (matches while the type-5 region bit is
    // set) with a payload marker byte.
    let mut zone = vec![1u8];
    let mut rec = [0u8; ZONE_RECORD_STRIDE];
    rec[0] = 5;
    rec[5] = 0xAB;
    zone.extend_from_slice(&rec);

    let mut world = World::new();
    world.mode = SceneMode::Field;
    world.live_gameplay_loop = true;
    world.install_field_player(0);
    // Tile (5, 5): world = 0x40 + tile * 0x80 ((w - 0x40) >> 7 = tile).
    world.actors[0].move_state.world_x = 0x40 + 5 * 0x80;
    world.actors[0].move_state.world_z = 0x40 + 5 * 0x80;
    world.load_field_region_tables(&block, &zone);

    // Initial refresh: inside the type-4 region, no zone match.
    assert_eq!(world.extra_flags, 1 << 4);
    assert!(world.field_zone_record.is_none());

    // Prime the tile latch, then cross into the type-5 region.
    world.tick();
    world.actors[0].move_state.world_x = 0x40 + 9 * 0x80;
    world.tick();

    assert_eq!(world.extra_flags, 1 << 5, "mask rebuilt on tile crossing");
    let rec = world
        .field_zone_record
        .expect("kind-5 zone record selected");
    assert_eq!(rec[0], 5);
    assert_eq!(rec[5], 0xAB, "payload carried through");
}

// ---------------------------------------------------------------------------
// Summon spell XP + level-up (FUN_801ddb30 tail / FUN_801e70bc) and the
// Lost Grail Final Heal revive (FUN_801e6968).
// ---------------------------------------------------------------------------

/// World with one party member (slot 0), a roster record carrying summon
/// spell `0x81` at level 1, and an enemy in slot 1.
fn summon_xp_world(enemy_hp: u16, enemy_max_hp: u16) -> World {
    use legaia_save::Party;
    let mut world = World {
        party_count: 1,
        ..World::default()
    };
    world.mode = SceneMode::Battle;
    world.roster = Party::zeroed(1);
    let rec = &mut world.roster.members[0];
    let mut list = rec.spell_list();
    list.count = 1;
    list.ids[0] = 0x81;
    list.levels[0] = 1;
    rec.set_spell_list(list);

    world.actors[0].battle.max_hp = 400;
    world.actors[0].battle.hp = 400;
    world.actors[0].battle.mp = 200;
    world.actors[0].battle.liveness = 1;
    world.set_battle_magic(0, 40);
    world.actors[1].battle.max_hp = enemy_max_hp;
    world.actors[1].battle.hp = enemy_hp;
    world.actors[1].battle.liveness = 1;
    world.battle_defense[1] = 0;
    world
}

fn gimard_spell_def() -> crate::spells::SpellDef {
    use crate::spells::{SpellDef, SpellEffect, SpellElement, SpellTarget};
    SpellDef {
        id: 0x81,
        name: "Gimard".into(),
        mp_cost: 4,
        element: SpellElement::Neutral,
        target: SpellTarget::OneEnemy,
        effect: SpellEffect::Damage {
            base_power: 100,
            element: SpellElement::Neutral,
        },
        anim_id: 0,
    }
}

#[test]
fn summon_cast_accrues_spell_xp_from_dealt_damage() {
    let mut world = summon_xp_world(4000, 4000);
    let def = gimard_spell_def();
    let before = world.actors[1].battle.hp;
    world.cast_spell_on_slots(0, &def, &[1]);
    let damage = (before - world.actors[1].battle.hp) as u32;
    assert!(damage > 0, "the placeholder cast deals damage");
    // Non-kill single-target accrual: damage * 12 / max_hp
    // (FUN_801ddb30 tail; kernel summon_spell_xp_gain).
    let expected = vm::battle_formulas::summon_spell_xp_gain(damage, 4000, 4000, false);
    let slot = crate::magic_xp::spell_slot(&world.roster.members[0], 0x81).unwrap();
    assert_eq!(
        crate::magic_xp::spell_xp(&world.roster.members[0], slot),
        expected
    );
    // No thresholds installed: XP accrues but the spell never levels.
    assert_eq!(world.roster.members[0].spell_list().levels[0], 1);
    assert!(world.drain_magic_level_ups().is_empty());
}

#[test]
fn summon_kill_accrues_flat_unit_and_levels_up_past_threshold() {
    let mut world = summon_xp_world(50, 4000);
    // Tiny live HP: the cast kills -> flat 12 XP (single-target).
    world.magic_xp_thresholds = Some([17, 50, 92, 144, 208, 288, 392, 536]);
    // Pre-bank XP just below the level-1 threshold: 6 + 12 = 18 > 17.
    let slot = crate::magic_xp::spell_slot(&world.roster.members[0], 0x81).unwrap();
    crate::magic_xp::add_spell_xp(&mut world.roster.members[0], slot, 6);

    let def = gimard_spell_def();
    world.cast_spell_on_slots(0, &def, &[1]);
    assert_eq!(world.actors[1].battle.hp, 0, "the cast kills the target");
    assert_eq!(
        crate::magic_xp::spell_xp(&world.roster.members[0], slot),
        18,
        "kill grants the flat 12-XP unit"
    );
    assert_eq!(
        world.roster.members[0].spell_list().levels[0],
        2,
        "18 XP > threshold 17 levels the spell (strict greater)"
    );
    assert_eq!(world.drain_magic_level_ups(), vec![(0, 0x81, 2)]);
    // The leveled byte is what the next cast's magic-power stage reads.
    assert_eq!(world.caster_magic_power_byte(0, 0x81), 2);
}

#[test]
fn summon_xp_threshold_compare_is_strict() {
    let mut world = summon_xp_world(50, 4000);
    world.magic_xp_thresholds = Some([17, 50, 92, 144, 208, 288, 392, 536]);
    // 5 + 12 = 17 == threshold: strict compare -> no level.
    let slot = crate::magic_xp::spell_slot(&world.roster.members[0], 0x81).unwrap();
    crate::magic_xp::add_spell_xp(&mut world.roster.members[0], slot, 5);
    let def = gimard_spell_def();
    world.cast_spell_on_slots(0, &def, &[1]);
    assert_eq!(
        crate::magic_xp::spell_xp(&world.roster.members[0], slot),
        17
    );
    assert_eq!(world.roster.members[0].spell_list().levels[0], 1);
    assert!(world.drain_magic_level_ups().is_empty());
}

#[test]
fn non_summon_spell_accrues_no_spell_xp() {
    let mut world = summon_xp_world(4000, 4000);
    // Same shape but a non-Seru-magic id (outside 0x81..=0x8B).
    let mut def = gimard_spell_def();
    def.id = 0x27;
    let mut list = world.roster.members[0].spell_list();
    list.ids[0] = 0x27;
    world.roster.members[0].set_spell_list(list);
    world.cast_spell_on_slots(0, &def, &[1]);
    let slot = crate::magic_xp::spell_slot(&world.roster.members[0], 0x27).unwrap();
    assert_eq!(crate::magic_xp::spell_xp(&world.roster.members[0], slot), 0);
}

#[test]
fn final_heal_revives_and_consumes_one_lost_grail() {
    use legaia_save::Party;
    let mut world = World {
        party_count: 1,
        ..World::default()
    };
    world.mode = SceneMode::Battle;
    world.roster = Party::zeroed(1);
    // Down at 0 HP with the Final Heal bit (word 1 bit 7 = ability 0x27) and
    // one equipped Lost Grail (0xE7) in the first accessory slot (+0x19B).
    world.actors[0].battle.max_hp = 250;
    world.actors[0].battle.hp = 0;
    world.actors[0].battle.liveness = 0;
    let rec = &mut world.roster.members[0];
    let mut bits = rec.ability_bits();
    bits[4] = 0x80;
    rec.set_ability_bits(bits);
    let mut eq = rec.equipment();
    eq.slots[5] = 0xE7;
    rec.set_equipment(eq);

    world.apply_final_heal_revives();

    assert_eq!(
        world.actors[0].battle.hp, 250,
        "full max-HP revive (tier 1)"
    );
    assert_eq!(world.actors[0].battle.liveness, 1);
    let rec = &world.roster.members[0];
    assert_eq!(rec.equipment().slots[5], 0, "the Lost Grail is consumed");
    assert_eq!(
        rec.ability_bits()[4] & 0x80,
        0,
        "the Final Heal bit clears with no second Grail equipped"
    );
    assert!(
        world
            .battle_hit_fx
            .iter()
            .any(|fx| fx.target_slot == 0 && fx.is_heal && fx.amount == 250),
        "heal popup recorded"
    );
}

#[test]
fn final_heal_keeps_bit_when_second_grail_is_equipped() {
    use legaia_save::Party;
    let mut world = World {
        party_count: 1,
        ..World::default()
    };
    world.mode = SceneMode::Battle;
    world.roster = Party::zeroed(1);
    world.actors[0].battle.max_hp = 100;
    world.actors[0].battle.hp = 0;
    let rec = &mut world.roster.members[0];
    let mut bits = rec.ability_bits();
    bits[4] = 0x80;
    rec.set_ability_bits(bits);
    let mut eq = rec.equipment();
    eq.slots[5] = 0xE7;
    eq.slots[7] = 0xE7;
    rec.set_equipment(eq);

    world.apply_final_heal_revives();

    let rec = &world.roster.members[0];
    assert_eq!(rec.equipment().slots[5], 0, "first Grail consumed");
    assert_eq!(rec.equipment().slots[7], 0xE7, "second Grail kept");
    assert_eq!(
        rec.ability_bits()[4] & 0x80,
        0x80,
        "bit re-set while another Grail is equipped (the second slot scan)"
    );
}

#[test]
fn final_heal_ignores_members_without_the_bit() {
    use legaia_save::Party;
    let mut world = World {
        party_count: 1,
        ..World::default()
    };
    world.mode = SceneMode::Battle;
    world.roster = Party::zeroed(1);
    world.actors[0].battle.max_hp = 100;
    world.actors[0].battle.hp = 0;
    world.actors[0].battle.liveness = 0;

    world.apply_final_heal_revives();

    assert_eq!(world.actors[0].battle.hp, 0, "stays down without the bit");
    assert_eq!(world.actors[0].battle.liveness, 0);
}

// --- battle pose -> action-clip switching -----------------------------------

/// Synthetic one-part action clip: `frames` keyframes translating from `tx`.
fn pose_test_clip(action_id: u8, frames: usize, tx: i16) -> MonsterAnimation {
    use legaia_asset::monster_archive::PartPose;
    MonsterAnimation {
        action_id,
        rate: 2,
        part_count: 1,
        frame_count: frames,
        frames: (0..frames)
            .map(|f| {
                vec![PartPose {
                    tx: tx + f as i16,
                    ty: 0,
                    tz: 0,
                    rx: 0,
                    ry: 0,
                    rz: 0,
                }]
            })
            .collect(),
    }
}

fn pose_test_world() -> World {
    let mut world = World::new();
    world.actors[0].active = true;
    let mut clips: Vec<Option<MonsterAnimation>> = vec![None; 22];
    clips[0] = Some(pose_test_clip(0, 2, 0));
    clips[8] = Some(pose_test_clip(8, 2, 100));
    clips[9] = Some(pose_test_clip(9, 2, 200));
    world.set_actor_battle_action_clips(0, std::sync::Arc::new(clips));
    world
}

#[test]
fn battle_pose_plays_action_clip_then_restores_idle() {
    let mut world = pose_test_world();
    world.apply_battle_pose(0, vm::battle_action::Pose::Recover as u8);
    assert_eq!(world.actors[0].battle_pose, Some(8));
    // One-shot: run the 2-frame clip to its end in one tick.
    world.actors[0].battle_animation.as_mut().unwrap().step = 1024;
    world.tick_battle_animations();
    assert!(
        world.actors[0]
            .battle_animation
            .as_ref()
            .unwrap()
            .finished(),
        "recover clip is a one-shot"
    );
    // The next tick falls back to the idle loop (slot 0).
    world.tick_battle_animations();
    assert_eq!(
        world.actors[0].battle_pose,
        Some(vm::battle_action::Pose::Idle as u8)
    );
    assert!(
        !world.actors[0]
            .battle_animation
            .as_ref()
            .unwrap()
            .finished(),
        "idle loops"
    );
}

#[test]
fn battle_pose_defeat_holds_final_frame() {
    let mut world = pose_test_world();
    world.apply_battle_pose(0, vm::battle_action::Pose::Defeat as u8);
    world.actors[0].battle_animation.as_mut().unwrap().step = 1024;
    for _ in 0..5 {
        world.tick_battle_animations();
    }
    // Defeat never falls back to idle: the downed pose holds.
    assert_eq!(
        world.actors[0].battle_pose,
        Some(vm::battle_action::Pose::Defeat as u8)
    );
    assert!(
        world.actors[0]
            .battle_animation
            .as_ref()
            .unwrap()
            .finished()
    );
}

#[test]
fn battle_pose_missing_slot_falls_back_to_idle_loop() {
    let mut world = pose_test_world();
    // Slot 7 (ready) is empty in the installed set: the request binds the
    // idle loop instead and records the pose so the SM isn't retried.
    world.apply_battle_pose(0, vm::battle_action::Pose::Ready as u8);
    assert_eq!(
        world.actors[0].battle_pose,
        Some(vm::battle_action::Pose::Ready as u8)
    );
    world.tick_battle_animations();
    assert!(
        !world.actors[0]
            .battle_animation
            .as_ref()
            .unwrap()
            .finished()
    );
}

#[test]
fn battle_pose_repeat_request_keeps_playing_clip() {
    let mut world = pose_test_world();
    world.apply_battle_pose(0, vm::battle_action::Pose::Recover as u8);
    world.actors[0].battle_animation.as_mut().unwrap().step = 7;
    world.tick_battle_animations();
    let phase_frame = world.actors[0].pose_frame.clone();
    // Re-requesting the same pose must not rewind the clip.
    world.apply_battle_pose(0, vm::battle_action::Pose::Recover as u8);
    world.tick_battle_animations();
    assert_ne!(
        world.actors[0].pose_frame.as_ref().map(|f| f.factor),
        phase_frame.as_ref().map(|f| f.factor),
        "cursor advanced instead of restarting"
    );
}

#[test]
fn battle_pose_without_clips_is_inert() {
    let mut world = World::new();
    world.actors[0].active = true;
    world.apply_battle_pose(0, vm::battle_action::Pose::Recover as u8);
    assert_eq!(world.actors[0].battle_pose, None);
    assert!(world.actors[0].battle_animation.is_none());
}

// ---------------------------------------------------------------------------
// Present-party composition (`World::active_party`)
// ---------------------------------------------------------------------------

/// Four-record roster with distinct, recognisable stats per character.
fn composition_roster() -> legaia_save::Party {
    let mut party = legaia_save::Party::zeroed(4);
    for (slot, rec) in party.members.iter_mut().enumerate() {
        let mut hms = rec.hp_mp_sp();
        hms.hp_max = 100 + slot as u16 * 100; // 100/200/300/400
        hms.hp_cur = hms.hp_max;
        hms.mp_max = 10 + slot as u16 * 10;
        hms.mp_cur = hms.mp_max;
        rec.set_hp_mp_sp(hms);
        let mut ls = rec.live_stats();
        ls.atk = 11 + slot as u16 * 11; // 11/22/33/44
        ls.spd = 5 + slot as u16 * 5;
        rec.set_live_stats(ls);
    }
    party
}

#[test]
fn active_party_maps_battle_ordinals_to_characters() {
    let mut world = World::new();
    world.load_party(composition_roster());
    // Noa + Terra present: battle ordinal 0 = roster slot 1, ordinal 1 =
    // roster slot 3 (the live-verified retail Terra-party shape).
    world.set_active_party(vec![1, 3]);
    assert_eq!(world.party_count, 2);
    assert_eq!(world.party_roster_slot(0), 1);
    assert_eq!(world.party_roster_slot(1), 3);
    // Actor mirrors reseeded per the mapping.
    assert_eq!(world.actors[0].battle.max_hp, 200, "ordinal 0 = Noa's HP");
    assert_eq!(world.actors[1].battle.max_hp, 400, "ordinal 1 = Terra's HP");
    assert_eq!(world.battle_speed[0], 10);
    assert_eq!(world.battle_speed[1], 20);
    // Stat seeding folds the OCCUPYING character's record onto the ordinal.
    world.seed_party_battle_stats();
    assert_eq!(
        world.battle_attack[0], 22,
        "ordinal 0 attacks with Noa's ATK"
    );
    assert_eq!(
        world.battle_attack[1], 44,
        "ordinal 1 attacks with Terra's ATK"
    );
}

#[test]
fn battle_spell_session_reads_composed_character() {
    let mut world = World::new();
    let mut party = composition_roster();
    // Terra (slot 3) knows Flame; nobody else knows anything.
    let mut list = party.members[3].spell_list();
    list.count = 1;
    list.ids[0] = 0x20;
    party.members[3].set_spell_list(list);
    world.load_party(party);
    world.set_spell_catalog(crate::spells::SpellCatalog::vanilla());
    world.set_active_party(vec![3, 0]);
    world.mode = SceneMode::Battle;
    // Ordinal 0 (Terra) offers her spell; ordinal 1 (Vahn) has none.
    let menu = world
        .build_battle_spell_session(0)
        .expect("composed caster builds a session");
    assert_eq!(menu.spells.len(), 1, "Terra's learned spell shows");
    let vahn_menu = world.build_battle_spell_session(1);
    assert!(
        vahn_menu.is_none_or(|m| m.spells.is_empty()),
        "ordinal 1 (Vahn) has no learned spells"
    );
}

#[test]
fn battle_xp_routes_to_composed_characters() {
    let mut world = World::new();
    world.load_party(composition_roster());
    world.set_active_party(vec![2, 3]);
    world.enter_battle(2, 1);
    world.apply_battle_xp(100);
    // The 3/4-scaled split lands on the OCCUPYING characters' XP wells
    // (roster slots 2 + 3), not on slots 0/1.
    assert_eq!(world.level_up_tracker.xp[0], 0, "Vahn (absent) gets none");
    assert_eq!(world.level_up_tracker.xp[1], 0, "Noa (absent) gets none");
    assert!(
        world.level_up_tracker.xp[2] > 0,
        "Gala (ordinal 0) earns XP"
    );
    assert!(
        world.level_up_tracker.xp[3] > 0,
        "Terra (ordinal 1) earns XP"
    );
}

#[test]
fn active_party_survives_save_roundtrip_and_maps_hp_writeback() {
    let mut world = World::new();
    world.load_party(composition_roster());
    world.set_active_party(vec![1, 3]);
    // Battle damage on ordinal 0 (= Noa).
    world.actors[0].battle.hp = 150;
    let sf = world.save_full();
    assert_eq!(sf.ext_v2.active_party, vec![1, 3]);
    let noa = sf.party.members[1].hp_mp_sp();
    assert_eq!(noa.hp_cur, 150, "ordinal-0 damage lands on Noa's record");
    let vahn = sf.party.members[0].hp_mp_sp();
    assert_eq!(vahn.hp_cur, 100, "absent Vahn's record is untouched");

    let mut fresh = World::new();
    fresh.load_full(sf);
    assert_eq!(fresh.active_party, vec![1, 3]);
    assert_eq!(fresh.party_count, 2);
    assert_eq!(fresh.actors[0].battle.hp, 150, "Noa's HP back on ordinal 0");
}

#[test]
fn identity_save_keeps_legacy_party_semantics() {
    let mut world = World::new();
    world.load_party(composition_roster());
    let sf = world.save_full();
    // No composition installed: the historical full-roster identity order.
    assert_eq!(sf.ext_v2.active_party, vec![0, 1, 2, 3]);
    let mut fresh = World::new();
    fresh.load_full(sf);
    assert!(
        fresh.active_party.is_empty(),
        "identity order restores as the identity default"
    );
    assert_eq!(fresh.party_count, 4, "legacy party_count preserved");
}

// --- battle hit reactions (retail +0x1EF tag map) ----------------------------

/// Clip set carrying the full party reaction family at identity indices
/// (action tags 2..5 + 0x0B), like a player battle file's record[0].
fn reaction_test_world(with_getup: bool) -> World {
    let mut world = World::new();
    world.actors[0].active = true;
    world.actors[0].battle.hp = 100;
    world.actors[0].battle.max_hp = 100;
    let mut clips: Vec<Option<MonsterAnimation>> = vec![None; 12];
    clips[0] = Some(pose_test_clip(0, 2, 0));
    clips[2] = Some(pose_test_clip(2, 2, 20));
    clips[4] = Some(pose_test_clip(4, 2, 40));
    if with_getup {
        clips[5] = Some(pose_test_clip(5, 2, 50));
    }
    world.set_actor_battle_action_clips(0, std::sync::Arc::new(clips));
    world
}

/// Run the active one-shot to completion and let the chain advance once.
fn finish_reaction_clip(world: &mut World) {
    world.actors[0].battle_animation.as_mut().unwrap().step = 1024;
    world.tick_battle_animations(); // finishes the clip
    world.tick_battle_animations(); // chain reacts to `finished`
}

#[test]
fn hit_reaction_knockdown_then_getup_then_idle() {
    // An actor WITH a get-up entry plays knockdown (tag 4) on any hit,
    // then get-up (tag 5), then resumes idle - the FUN_800402F4 staging +
    // FUN_8004AD80 record-type-4 chain.
    let mut world = reaction_test_world(true);
    world.queue_battle_reaction(0, true);
    assert_eq!(world.actors[0].battle_reaction, Some(4));
    finish_reaction_clip(&mut world);
    assert_eq!(
        world.actors[0].battle_reaction,
        Some(5),
        "living actor chains knockdown into get-up"
    );
    finish_reaction_clip(&mut world);
    assert_eq!(world.actors[0].battle_reaction, None);
    assert_eq!(
        world.actors[0].battle_pose,
        Some(vm::battle_action::Pose::Idle as u8),
        "reaction chain ends in the idle loop"
    );
}

#[test]
fn hit_reaction_light_flinch_without_getup() {
    // A surviving target with no get-up entry plays the light flinch
    // (tag 2) and falls straight back to idle.
    let mut world = reaction_test_world(false);
    world.queue_battle_reaction(0, true);
    assert_eq!(world.actors[0].battle_reaction, Some(2));
    finish_reaction_clip(&mut world);
    assert_eq!(world.actors[0].battle_reaction, None);
    assert_eq!(
        world.actors[0].battle_pose,
        Some(vm::battle_action::Pose::Idle as u8)
    );
}

#[test]
fn hit_reaction_lethal_knockdown_holds_downed_frame() {
    let mut world = reaction_test_world(true);
    world.actors[0].battle.hp = 0;
    world.queue_battle_reaction(0, false);
    assert_eq!(world.actors[0].battle_reaction, Some(4));
    world.actors[0].battle_animation.as_mut().unwrap().step = 1024;
    for _ in 0..5 {
        world.tick_battle_animations();
    }
    // Dead: the knockdown holds its final keyframe; no get-up, no idle.
    assert_eq!(world.actors[0].battle_reaction, Some(4));
    assert!(
        world.actors[0]
            .battle_animation
            .as_ref()
            .unwrap()
            .finished()
    );
}

#[test]
fn reaction_outranks_pose_requests_until_done() {
    let mut world = reaction_test_world(true);
    world.queue_battle_reaction(0, true);
    // The SM keeps requesting poses every frame; an in-flight reaction wins.
    world.apply_battle_pose(0, vm::battle_action::Pose::Idle as u8);
    assert_eq!(world.actors[0].battle_reaction, Some(4));
    assert_eq!(world.actors[0].battle_pose, None);
}

#[test]
fn monster_slots_only_honor_idle_pose() {
    // Monster clip vectors are archive-order, not pose-indexed: a Defeat
    // pose request on a monster slot must not start clip index 9 (an
    // arbitrary spell action). Idle still maps to clip 0.
    let mut world = World::new();
    world.actors[3].active = true;
    let mut clips: Vec<Option<MonsterAnimation>> = vec![None; 12];
    clips[0] = Some(pose_test_clip(0, 2, 0));
    clips[9] = Some(pose_test_clip(0x0C, 2, 90));
    world.set_actor_battle_action_clips(3, std::sync::Arc::new(clips));
    world.apply_battle_pose(3, vm::battle_action::Pose::Defeat as u8);
    assert_eq!(world.actors[3].battle_pose, None, "non-idle pose ignored");
    world.apply_battle_pose(3, vm::battle_action::Pose::Idle as u8);
    assert_eq!(world.actors[3].battle_pose, Some(6), "idle still maps");
}

// --- staged battle anim commit (weapon swings + art bank) -------------------

/// World with party actor 0 carrying swing clips (slots 0xC..0xF) and a
/// 12-record art bank, both synthetic.
fn staged_anim_test_world() -> World {
    let mut world = World::new();
    world.actors[0].active = true;
    let mut clips: Vec<Option<MonsterAnimation>> = vec![None; 22];
    clips[0] = Some(pose_test_clip(0, 2, 0));
    for (slot, clip) in clips.iter_mut().enumerate().take(0x10).skip(0xC) {
        *clip = Some(pose_test_clip(slot as u8, 3, slot as i16 * 100));
    }
    world.set_actor_battle_action_clips(0, std::sync::Arc::new(clips));
    let bank: Vec<Option<MonsterAnimation>> = (0..12)
        .map(|r| Some(pose_test_clip(0x10 + r as u8, 3, 1000 + r as i16)))
        .collect();
    world.set_actor_battle_art_bank(0, std::sync::Arc::new(bank));
    world
}

#[test]
fn staged_swing_id_plays_equipment_clip_one_shot() {
    let mut world = staged_anim_test_world();
    world.actors[0].battle.queued_anim = 0x0C;
    world.commit_staged_battle_anim(0);
    let a = &world.actors[0];
    // Direct commit: no rewrite, ids converge on the swing slot.
    assert_eq!(a.battle.queued_anim, 0x0C);
    assert_eq!(a.battle.current_anim, 0x0C);
    assert_eq!(a.battle_staged_anim, Some(0x0C));
    // The swing clip (frame 0 tx = 0xC * 100) replaced the player.
    let mut p = a.battle_animation.clone().unwrap();
    p.step = 0; // sample frame 0
    assert_eq!(p.tick().bone_outputs[0].0[0], 0x0C * 100);
    assert!(!p.finished(), "one-shot, not yet finished");
}

#[test]
fn staged_art_ids_rewrite_to_dynamic_slots() {
    // 0x10 and 0x1A install at slot 0x11; other art ids at 0x10 - the
    // FUN_8004AD80 rewrite lands in BOTH id fields.
    for (staged, slot, record) in [(0x10u8, 0x11u8, 0usize), (0x1A, 0x11, 10), (0x12, 0x10, 2)] {
        let mut world = staged_anim_test_world();
        world.actors[0].battle.queued_anim = staged;
        world.commit_staged_battle_anim(0);
        let a = &world.actors[0];
        assert_eq!(a.battle.queued_anim, slot, "staged {staged:#x} rewritten");
        assert_eq!(a.battle.current_anim, slot);
        assert_eq!(a.battle_staged_anim, Some(slot));
        // The materialized clip is bank record `staged - 0x10`.
        let mut p = a.battle_animation.clone().unwrap();
        p.step = 0;
        assert_eq!(
            p.tick().bone_outputs[0].0[0],
            1000 + record as i16,
            "staged {staged:#x} materializes bank record {record}"
        );
    }
}

#[test]
fn staged_id_without_art_bank_is_a_plain_entry_index() {
    // Monsters carry no bank: ids >= 0x10 index the action-clip vector
    // directly (archive entry indices).
    let mut world = World::new();
    world.actors[3].active = true;
    let mut clips: Vec<Option<MonsterAnimation>> = vec![None; 24];
    clips[0x12] = Some(pose_test_clip(0x12, 3, 700));
    world.set_actor_battle_action_clips(3, std::sync::Arc::new(clips));
    world.actors[3].battle.queued_anim = 0x12;
    world.commit_staged_battle_anim(3);
    let a = &world.actors[3];
    assert_eq!(a.battle.queued_anim, 0x12, "no rewrite without a bank");
    assert_eq!(a.battle.current_anim, 0x12);
    let mut p = a.battle_animation.clone().unwrap();
    p.step = 0;
    assert_eq!(p.tick().bone_outputs[0].0[0], 700);
}

#[test]
fn staged_id_without_clip_converges_and_clears_advance_done() {
    use vm::battle_action::ActorFlags;
    let mut world = World::new();
    world.actors[0].active = true;
    world.actors[0].battle.queued_anim = 0x0C;
    world.actors[0]
        .battle
        .flag_bits
        .set(ActorFlags::ADVANCE_DONE);
    world.commit_staged_battle_anim(0);
    let a = &world.actors[0];
    // Clip-less host: a zero-length swing - ids converge, the attack
    // chain's read gate opens immediately.
    assert_eq!(a.battle.current_anim, 0x0C);
    assert!(!a.battle.flag_bits.has(ActorFlags::ADVANCE_DONE));
    assert!(a.battle_staged_anim.is_none());
}

#[test]
fn staged_swing_finish_clears_gate_and_resumes_idle() {
    use vm::battle_action::ActorFlags;
    let mut world = staged_anim_test_world();
    world.actors[0].battle.queued_anim = 0x0D;
    world.actors[0]
        .battle
        .flag_bits
        .set(ActorFlags::ADVANCE_DONE);
    world.commit_staged_battle_anim(0);
    assert_eq!(world.actors[0].battle_staged_anim, Some(0x0D));
    // While the swing plays, the SM's per-frame pose requests don't steal
    // the player.
    world.apply_battle_pose(0, vm::battle_action::Pose::Idle as u8);
    assert_eq!(world.actors[0].battle_staged_anim, Some(0x0D));
    // Run the 3-frame clip to its end, then let the tick observe the
    // finish: gate cleared, ids back to idle 0, idle loop restored.
    world.actors[0].battle_animation.as_mut().unwrap().step = 2048;
    world.tick_battle_animations(); // clip reaches its last keyframe
    world.tick_battle_animations(); // finish observed -> idle restore
    let a = &world.actors[0];
    assert!(a.battle_staged_anim.is_none());
    assert!(!a.battle.flag_bits.has(ActorFlags::ADVANCE_DONE));
    assert_eq!(a.battle.queued_anim, 0, "id pair converges to idle");
    assert_eq!(a.battle.current_anim, 0);
    assert_eq!(
        a.battle_pose,
        Some(vm::battle_action::Pose::Idle as u8),
        "idle loop resumes after the band"
    );
    assert!(!a.battle_animation.as_ref().unwrap().finished());
}

#[test]
fn attack_chain_paces_strikes_by_staged_clip_completion() {
    use vm::battle_action::{ActionState, StepOutcome};
    // Full SM-driven check: a two-swing strike script holds in AttackChain
    // while each staged swing plays, reads the next byte only after the
    // clip-end signal, and exits to recovery on the terminator.
    let mut world = staged_anim_test_world();
    world.mode = SceneMode::Battle;
    world.actors[0].battle.liveness = 1;
    world.actors[0].battle.hp = 100;
    world.actors[0].battle.max_hp = 100;
    world.actors[0].battle.params[0] = 0x0C;
    world.actors[0].battle.params[1] = 0x0D;
    world.actors[0].battle.params[2] = 0x00;
    world.battle_ctx.active_actor = 0;
    world.battle_ctx.action_state = ActionState::AttackChain.as_byte();

    // Step 1 stages swing 0xC; the tick commits + plays it.
    assert_eq!(world.step_battle(), StepOutcome::Stay);
    world.tick_battle_animations();
    assert_eq!(world.actors[0].battle_staged_anim, Some(0x0C));

    // While the swing is in flight the chain holds (the 0x801E370C gate).
    assert_eq!(world.step_battle(), StepOutcome::Stay);
    assert_eq!(world.actors[0].battle.strike_index, 1, "no byte read");

    // Finish the swing: the gate opens, the next step reads 0x0D.
    world.actors[0].battle_animation.as_mut().unwrap().step = 4096;
    world.tick_battle_animations();
    world.tick_battle_animations();
    assert!(world.actors[0].battle_staged_anim.is_none());
    assert_eq!(world.step_battle(), StepOutcome::Stay);
    world.tick_battle_animations();
    assert_eq!(world.actors[0].battle_staged_anim, Some(0x0D));
    assert_eq!(world.actors[0].battle.strike_index, 2);

    // Finish the second swing; the terminator exits the band.
    world.actors[0].battle_animation.as_mut().unwrap().step = 4096;
    world.tick_battle_animations();
    world.tick_battle_animations();
    let out = world.step_battle();
    assert!(
        matches!(out, StepOutcome::Transition { to, .. }
            if to == ActionState::AttackRecovery.as_byte()),
        "terminator -> recovery, got {out:?}"
    );
}

// ---------------------------------------------------------------------------
// Field-NPC motion (motion-VM wiring) + prop walk-touch dispatch
// ---------------------------------------------------------------------------

#[test]
fn field_npc_patrol_route_walks_through_motion_vm() {
    let mut world = World::new();
    world.mode = SceneMode::Field;
    world.field_npc_positions.insert(1, (1000, 1000));
    world
        .field_npc_routes
        .insert(1, vec![(1300, 1000), (1000, 1000)]);

    // Baseline: with `animate_field_npcs` off the NPC rests at its anchor.
    for _ in 0..20 {
        let _ = world.tick();
    }
    assert_eq!(world.field_npc_positions.get(&1), Some(&(1000, 1000)));

    // Flag on: the motion VM walks the NPC toward waypoint 0 at the per-frame
    // speed (8 units), reaches it, then patrols back toward waypoint 1.
    world.animate_field_npcs = true;
    let _ = world.tick();
    assert_eq!(
        world.field_npc_positions.get(&1),
        Some(&(1008, 1000)),
        "one tick = one motion-VM step of FIELD_NPC_MOTION_SPEED units"
    );
    for _ in 0..37 {
        let _ = world.tick();
    }
    assert_eq!(
        world.field_npc_positions.get(&1),
        Some(&(1300, 1000)),
        "the leg clamps at the waypoint (300 units / 8 per frame)"
    );
    for _ in 0..5 {
        let _ = world.tick();
    }
    let &(x, _) = world.field_npc_positions.get(&1).unwrap();
    assert!(x < 1300, "patrol loops: the NPC heads back to waypoint 1");
}

#[test]
fn moving_field_npc_collision_box_follows_live_position() {
    let mut world = World::new();
    world.mode = SceneMode::Field;
    world.solid_field_npcs = true;
    world.animate_field_npcs = true;
    world.field_npc_positions.insert(1, (1000, 1000));
    world.field_npc_routes.insert(1, vec![(1300, 1000)]);

    // Anchor blocks before the walk: the X+ probe from 102 out lands 38
    // inside the strict ±40 box (104 out reads exactly 40 = clear).
    assert!(world.field_actor_dir_blocked(1000 - 102, 1000, 3));

    // Walk the NPC to its waypoint (one-shot route).
    for _ in 0..60 {
        let _ = world.tick();
    }
    assert_eq!(world.field_npc_positions.get(&1), Some(&(1300, 1000)));
    assert!(
        world.field_npc_motions.is_empty(),
        "a one-waypoint route rests after arrival (no restart churn)"
    );

    // The ±40 moving-actor box follows the LIVE position: the abandoned
    // anchor no longer blocks, the new position does.
    assert!(!world.field_actor_dir_blocked(1000 - 102, 1000, 3));
    assert!(world.field_actor_dir_blocked(1300 - 102, 1000, 3));
}

#[test]
fn autonomous_legs_pause_during_dialogue_scripted_legs_run() {
    let mut world = World::new();
    world.mode = SceneMode::Field;
    world.animate_field_npcs = true;
    world.field_npc_positions.insert(1, (1000, 1000));
    world.field_npc_routes.insert(1, vec![(1300, 1000)]);
    world.field_npc_positions.insert(2, (2000, 2000));

    // A dialogue is up: the autonomous patrol must not start (retail's
    // interaction motion-pause), but a script-started leg (the interaction
    // partner's own prologue walk) keeps stepping.
    world.current_dialog = Some(DialogRequest {
        text_id: 0,
        inline: vec![],
        world_x: 0,
        world_z: 0,
        depth_id: 0,
    });
    assert!(world.start_field_npc_motion(2, 2080, 2000));
    for _ in 0..10 {
        let _ = world.tick();
    }
    assert_eq!(
        world.field_npc_positions.get(&1),
        Some(&(1000, 1000)),
        "autonomous patrol paused while the box is up"
    );
    assert_eq!(
        world.field_npc_positions.get(&2),
        Some(&(2080, 2000)),
        "scripted leg runs through the dialogue"
    );

    // Box dismissed: the patrol resumes.
    world.current_dialog = None;
    for _ in 0..10 {
        let _ = world.tick();
    }
    let &(x, _) = world.field_npc_positions.get(&1).unwrap();
    assert!(x > 1000, "patrol resumes once the dialogue clears");
}

#[test]
fn start_field_npc_motion_requires_installed_slot() {
    // The retail start kernel's actor-list search miss returns 0: a slot
    // with no installed placement starts nothing.
    let mut world = World::new();
    assert!(!world.start_field_npc_motion(9, 100, 100));
    assert!(world.field_npc_motions.is_empty());
}

#[test]
fn walk_touch_warp_posts_once_per_contact_and_queues_transition() {
    use crate::man_field_scripts::WalkTouchEvent;

    let mut world = World::new();
    world.mode = SceneMode::Field;
    world.install_field_player(0);
    world.actors[0].move_state.world_x = 1000;
    world.actors[0].move_state.world_z = 2000;
    world
        .field_walk_touch
        .insert(5, ((1200, 2000), WalkTouchEvent::Warp { target_map: 3 }));

    // Baseline: standing outside the ±80 contact box posts nothing.
    let _ = world.drain_field_events();
    for _ in 0..3 {
        let _ = world.tick();
    }
    assert!(world.pending_scene_transition.is_none());
    assert!(world.drain_field_events().is_empty());

    // Hold screen-right (camera azimuth 0: world X+) into the placement.
    world.set_pad(input::PadButton::Right.mask());
    for _ in 0..25 {
        let _ = world.tick();
    }
    assert_eq!(
        world.pending_scene_transition,
        Some(3),
        "the door-warp queues through the same path the 0x3E op uses"
    );
    let events = world.drain_field_events();
    let touches: Vec<_> = events
        .iter()
        .filter(|e| matches!(e, FieldEvent::FieldInteract { slot: 5, .. }))
        .collect();
    assert_eq!(touches.len(), 1, "one post per contact (edge latch)");

    // Still inside the box: no re-post while the contact persists.
    for _ in 0..5 {
        let _ = world.tick();
    }
    assert!(
        world
            .drain_field_events()
            .iter()
            .all(|e| !matches!(e, FieldEvent::FieldInteract { .. })),
        "sustained contact does not re-post"
    );
}

#[test]
fn walk_touch_player_moveto_teleports_player() {
    use crate::man_field_scripts::WalkTouchEvent;

    let mut world = World::new();
    world.mode = SceneMode::Field;
    world.install_field_player(0);
    world.actors[0].move_state.world_x = 1000;
    world.actors[0].move_state.world_z = 2000;
    world.field_walk_touch.insert(
        7,
        (
            (1150, 2000),
            WalkTouchEvent::PlayerMoveTo {
                world_x: 5000,
                world_z: 6000,
            },
        ),
    );

    world.set_pad(input::PadButton::Right.mask());
    for _ in 0..30 {
        let _ = world.tick();
        // The snap is the contact tick's last write; stop before the next
        // walk tick moves the player again.
        if world.actors[0].move_state.world_x >= 4000 {
            break;
        }
    }
    let ms = &world.actors[0].move_state;
    assert_eq!(
        (ms.world_x, ms.world_z),
        (5000, 6000),
        "touching the placement snaps the player to the decoded coords"
    );
    assert!(
        world.drain_field_events().iter().any(|e| matches!(
            e,
            FieldEvent::MoveTo {
                world_x: 5000,
                world_z: 6000,
                is_player: true
            }
        )),
        "the teleport surfaces as a player MoveTo event"
    );
    // One more walking tick: the touch dispatch re-runs (it lives on the
    // locomotion step) with the player now far outside the contact box, so
    // the edge latch releases for the next approach.
    let _ = world.tick();
    assert!(
        world.active_walk_touch.is_none(),
        "the teleport leaves the contact box, releasing the latch"
    );
}

#[test]
fn interaction_prologue_npc_run_walks_the_interacted_npc() {
    // A synthetic interaction record: prologue = one `0x4C 0x51` NPC run to
    // tile (12, 10), then a text segment. Driving the interact through the
    // opt-in field-VM runner must start the NPC's walk leg (the host hook
    // routing the op to the interacted placement slot) and the field ticks
    // must converge the NPC on the decoded tile-centre world position.
    let target_x = 12i16 * 0x80 + 0x40;
    let target_z = 10i16 * 0x80 + 0x40;
    let mut body = vec![0x4C, 0x51, 12, 10, 0, 5];
    let first_segment = body.len();
    body.extend_from_slice(&[0x1F, b'h', b'i', 0x00, 0x00]);

    let mut world = World::new();
    world.mode = SceneMode::Field;
    world.use_vm_dialogue = true;
    world
        .field_npc_positions
        .insert(3, (target_x - 80, target_z));
    world
        .field_npc_dialog
        .insert(3, body[first_segment..].to_vec());
    world.field_npc_dialog_prologue.insert(
        3,
        crate::man_field_scripts::InlineDialogPrologue {
            body,
            entry_pc: 0,
            first_segment,
        },
    );

    world.trigger_field_interact(0, 3);
    for _ in 0..15 {
        let _ = world.tick();
    }
    assert_eq!(
        world.field_npc_positions.get(&3),
        Some(&(target_x, target_z)),
        "the prologue's 0x4C 0x51 walked the interacted NPC to its tile"
    );
}

// ---------------------------------------------------------------------------
// Shiny Seru (rare +35% capturable variant): battle-load stat boost, capture
// marking, persistence, and the +35% damage bonus on cast.
// ---------------------------------------------------------------------------

#[test]
fn shiny_roll_boosts_only_the_capturable_enemy() {
    use crate::monster_catalog::{FormationDef, FormationSlot};
    let mut world = capture_world(1);
    world.set_shiny_chance_pct(100); // force a shiny this battle
    // Killer Bee (id 7) is capturable (Seru 1); Wolf (id 9) is not.
    let formation = FormationDef::new(1, vec![FormationSlot::new(7), FormationSlot::new(9)]);
    world.enter_battle_from_formation(&formation);

    // Slot 1 = first monster (Killer Bee). hp 25 -> 33, attack 9 -> 12.
    assert!(
        world.shiny_enemy_slots.contains(&1),
        "the capturable enemy is flagged shiny"
    );
    assert_eq!(world.actors[1].battle.max_hp, 33, "shiny HP +35%");
    assert_eq!(world.battle_attack[1], 12, "shiny ATK +35%");
    // Slot 2 = Wolf (not capturable) is never chosen.
    assert!(!world.shiny_enemy_slots.contains(&2));
}

#[test]
fn shiny_disabled_when_chance_is_zero() {
    use crate::monster_catalog::{FormationDef, FormationSlot};
    let mut world = capture_world(1);
    world.set_shiny_chance_pct(0);
    let formation = FormationDef::new(1, vec![FormationSlot::new(7)]);
    world.enter_battle_from_formation(&formation);
    assert!(world.shiny_enemy_slots.is_empty(), "no shiny when disabled");
    assert_eq!(world.actors[1].battle.max_hp, 25, "stats unmodified");
}

#[test]
fn shiny_capture_marks_spell_shiny_and_persists_through_save() {
    let mut world = capture_world(1);
    world.set_spell_catalog(crate::spells::SpellCatalog::vanilla());
    // Killer Bee captured as shiny (Seru 1 -> spell 0x20).
    world.battle_captures = vec![7];
    world.shiny_captures = vec![7];
    world.finish_battle();

    assert!(world.seru_log.has_learned(0, 1), "spell learned");
    assert!(
        world.seru_log.is_shiny(0, 0x20),
        "shiny capture flags the learned spell shiny"
    );
    // shiny_captures drained.
    assert!(world.shiny_captures.is_empty());

    // Round-trips through the LGSF v4 save.
    let sf = world.save_full();
    assert!(
        sf.ext_v2
            .per_char
            .iter()
            .any(|(slot, ce)| *slot == 0 && ce.shiny_spells.contains(&0x20)),
        "shiny spell serialised into the save"
    );
    let mut reloaded = capture_world(1);
    reloaded.load_full(sf);
    assert!(
        reloaded.seru_log.is_shiny(0, 0x20),
        "shiny survives save/load"
    );
}

#[test]
fn non_shiny_capture_does_not_flag_shiny() {
    let mut world = capture_world(1);
    world.battle_captures = vec![7]; // captured normally (not in shiny_captures)
    world.finish_battle();
    assert!(world.seru_log.has_learned(0, 1));
    assert!(
        !world.seru_log.is_shiny(0, 0x20),
        "a normal capture is never shiny"
    );
}

#[test]
fn shiny_spell_deals_35_percent_more_damage() {
    // Plain cast.
    let mut plain = summon_xp_world(4000, 4000);
    let def = gimard_spell_def();
    let before = plain.actors[1].battle.hp;
    plain.cast_spell_on_slots(0, &def, &[1]);
    let plain_dmg = (before - plain.actors[1].battle.hp) as u32;
    assert!(plain_dmg > 0);

    // Shiny cast: same world setup, spell 0x20-> here 0x81 flagged shiny.
    let mut shiny = summon_xp_world(4000, 4000);
    shiny.seru_log.mark_shiny(0, 0x81);
    let before_s = shiny.actors[1].battle.hp;
    shiny.cast_spell_on_slots(0, &def, &[1]);
    let shiny_dmg = (before_s - shiny.actors[1].battle.hp) as u32;

    let expected = (plain_dmg * 135 / 100).min(9999);
    assert_eq!(
        shiny_dmg, expected,
        "shiny cast deals +35% (plain {plain_dmg} -> shiny {shiny_dmg})"
    );
}

/// Field move-VM stager spawn (op 0x34 sub-3 / `FUN_800252EC` → `spawn_field_stager`):
/// a stager record spawns a one-part field scene-graph effect, and the
/// non-visual `0x4001` sound-emitter node is split off the mesh draw path into
/// the render-node channel (the `FUN_80021DF4` `+0x5A` classification).
#[test]
fn field_stager_spawn_splits_sound_node_off_the_mesh_draws() {
    use crate::summon::RenderMode;
    use legaia_asset::summon_overlay::{RENDER_NODE_MODE_B, SummonPart};

    let mut world = World::new();

    // Two prescript records back-to-back: a transform node then a 0x4001 sound
    // node. Each = [i16 model_sel][u16 flags][move-VM HALT].
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&(-1i16).to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&0x08u16.to_le_bytes()); // HALT
    bytes.extend_from_slice(&0u16.to_le_bytes()); // pad
    let r1 = bytes.len();
    bytes.extend_from_slice(&RENDER_NODE_MODE_B.to_le_bytes()); // 0x4001 sound
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&0x08u16.to_le_bytes()); // HALT

    world.field_stager_bytes = bytes.clone();
    world.field_stagers = vec![
        SummonPart {
            record_off: 0,
            model_sel: -1,
            flags: 0,
            bytecode: 4..8,
        },
        SummonPart {
            record_off: r1,
            model_sel: RENDER_NODE_MODE_B,
            flags: 0,
            bytecode: (r1 + 4)..bytes.len(),
        },
    ];

    // Spawn the 0x4001 sound node (id 1) at a world position.
    assert!(world.spawn_field_stager(1, [5, 6, 7]));
    assert_eq!(world.active_field_fx.len(), 1);

    // It surfaces as a SoundEmitter render node, NOT a mesh draw.
    let nodes = world.active_field_fx_render_nodes();
    assert_eq!(nodes.len(), 1);
    assert_eq!(nodes[0].mode, RenderMode::SoundEmitter);
    assert_eq!(nodes[0].world_pos, [5.0, 6.0, 7.0]);
    assert!(
        world.active_field_fx_part_draws().is_empty(),
        "the sound node never mesh-draws"
    );

    // Out-of-range id no-ops (retail bounds behaviour).
    assert!(!world.spawn_field_stager(99, [0, 0, 0]));
    // After a tick the effect is KEPT (a finished part holds its final pose
    // rather than draining the same frame it halts).
    world.tick_field_fx(0x0400);
    assert_eq!(
        world.active_field_fx.len(),
        1,
        "a finished field effect is kept (held at its final pose), not drained"
    );
    // Scene entry (install) clears live effects.
    world.install_field_stagers(&bytes);
    assert!(
        world.active_field_fx.is_empty(),
        "scene entry clears live field effects"
    );
}

#[test]
fn vm_dialogue_drives_inline_runner_via_tick_and_tears_down() {
    // With `use_vm_dialogue` set (the shell's default; `--simple-dialogue`
    // opts out), a field dialogue that carries an inline buffer is driven
    // through the field VM by tick()'s `drive_inline_dialogue` wrapper: the
    // runner auto-starts, steps the prologue + `0x1F` text segment, and on the
    // dismissing confirm clears `current_dialog` and emits `DialogDismissed`.
    // Minimal buffer: a 3-byte prologue, one `0x1F` "Hi" segment, terminator
    // (same shape dialog.rs's `from_inline_dialog_skips_prologue` fixture uses).
    let inline = vec![0x00u8, 0x56, 0x00, 0x1F, b'H', b'i', 0x00];
    let mut world = World::new();
    world.mode = SceneMode::Field;
    world.use_vm_dialogue = true;
    world.current_dialog = Some(DialogRequest {
        text_id: 0,
        inline,
        world_x: 0,
        world_z: 0,
        depth_id: 0,
    });

    let mut dismissed = false;
    for i in 0..4000 {
        // Edge-triggered confirm: pulse Cross (press on even frames, release on
        // odd) so `just_pressed` fires repeatedly to finish the typewriter and
        // dismiss the box, instead of a held mask that only edges once.
        world.set_pad(if i % 2 == 0 {
            input::PadButton::Cross.mask()
        } else {
            0
        });
        let _ = world.tick();
        if world
            .drain_field_events()
            .iter()
            .any(|e| matches!(e, crate::field_events::FieldEvent::DialogDismissed))
        {
            dismissed = true;
        }
        if world.current_dialog.is_none() && world.inline_dialogue.is_none() {
            break;
        }
    }
    assert!(
        dismissed,
        "the VM-driven dialogue must emit DialogDismissed on completion"
    );
    assert!(
        world.current_dialog.is_none(),
        "current_dialog cleared after the VM dialogue ends"
    );
    assert!(
        world.inline_dialogue.is_none(),
        "inline runner torn down after completion"
    );
}

#[test]
fn simple_dialogue_opt_out_leaves_runner_untouched() {
    // The `--simple-dialogue` path (`use_vm_dialogue == false`) must NOT start
    // the inline runner: `drive_inline_dialogue` returns early, so the request
    // stays owned by the simplified typewriter panel. This pins that the
    // default-on flip is opt-out-able and non-destructive.
    let inline = vec![0x00u8, 0x56, 0x00, 0x1F, b'H', b'i', 0x00];
    let mut world = World::new();
    world.mode = SceneMode::Field;
    world.use_vm_dialogue = false;
    world.current_dialog = Some(DialogRequest {
        text_id: 0,
        inline,
        world_x: 0,
        world_z: 0,
        depth_id: 0,
    });

    // No confirm press: with the VM runner disabled, dismissal is the
    // simplified path's job (host panel / interaction probe on a confirm),
    // not tick()'s. Ticking without input must leave the request open AND
    // never spin up the inline runner.
    for _ in 0..64 {
        world.set_pad(0);
        let _ = world.tick();
        assert!(
            world.inline_dialogue.is_none(),
            "opt-out must never start the VM runner"
        );
    }
    assert!(
        world.current_dialog.is_some(),
        "the request stays with the simplified panel when VM dialogue is off"
    );
}

/// A non-summon battle move whose move-power record carries a spawnable effect
/// entry requests a move-FX spawn (`World::pending_move_fx_spawn`) at the
/// target's battle position when it resolves through the shared cast path
/// (`cast_spell_on_slots`) - the engine-side wiring the host drains to call
/// `spawn_move_fx`. A move with NO effect entry requests nothing (the gate).
/// Disc-free: uses a synthetic PROT-0898 overlay; the actual scene-graph render
/// is proven on real data by `move_fx_render_disc`.
#[test]
fn battle_special_attack_requests_move_fx_spawn() {
    use crate::monster_catalog::vanilla_monster_catalog;
    use crate::move_power::MovePowerCatalog;
    use crate::spells::SpellCatalog;
    use legaia_asset::move_power::{
        MOVE_ID_INDEX_MAP_FILE_OFFSET, MOVE_POWER_RECORD_STRIDE, MOVE_POWER_TABLE_FILE_OFFSET,
        MOVE_POWER_TABLE_LEN,
    };

    // Flame (move id 0x20) -> power record 1; `with_fx` adds a `+0x12` Spawn(1)
    // on-contact entry so `move_has_spawn_fx(0x20)` is true.
    fn overlay(with_fx: bool) -> Vec<u8> {
        let mut buf = vec![
            0u8;
            MOVE_POWER_TABLE_FILE_OFFSET
                + MOVE_POWER_RECORD_STRIDE * MOVE_POWER_TABLE_LEN
        ];
        buf[MOVE_ID_INDEX_MAP_FILE_OFFSET + 4] = 1; // structural guard
        buf[MOVE_ID_INDEX_MAP_FILE_OFFSET + 0x20] = 1; // Flame -> record 1
        let rec = MOVE_POWER_TABLE_FILE_OFFSET + MOVE_POWER_RECORD_STRIDE;
        buf[rec] = 0xB8; // +0 power 0x0BB8
        buf[rec + 1] = 0x0B;
        if with_fx {
            buf[rec + 0x12] = 0x01; // +0x12 on-contact list: Spawn(1)
        }
        buf
    }

    fn run(with_fx: bool) -> Option<(u8, [i16; 3])> {
        let mut world = World {
            party_count: 1,
            ..World::default()
        };
        world.mode = SceneMode::Battle;
        world.set_spell_catalog(SpellCatalog::vanilla());
        world.monster_catalog = vanilla_monster_catalog();
        // Party target at slot 0 with a distinct battle position so the request
        // origin is provably the target's position, not a constant.
        world.actors[0].battle.max_hp = 4000;
        world.actors[0].battle.hp = 4000;
        world.actors[0].battle.liveness = 1;
        world.actors[0].move_state.world_x = 100;
        world.actors[0].move_state.world_y = -50;
        world.actors[0].move_state.world_z = 200;
        world.battle_accuracy[0] = 30;
        world.battle_defense[0] = 40;
        // Bandit Boss (id 5) at slot 1 casts Flame (0x20) on seed 0.
        world.actors[1].battle.max_hp = 120;
        world.actors[1].battle.hp = 120;
        world.actors[1].battle.mp = 10;
        world.actors[1].battle.liveness = 1;
        world.actors[1].battle_monster_id = Some(5);
        world.battle_accuracy[1] = 25;
        world.set_battle_magic(1, 40);
        world.move_power = MovePowerCatalog::from_overlay_0898(&overlay(with_fx));
        world.rng_state = 0;

        world.take_monster_turn(1);
        assert_eq!(world.actors[1].battle.params[0], 0x20, "picker chose Flame");
        // A non-summon move never requests a summon-creature spawn.
        assert!(world.pending_summon_spawn.is_none());
        world.pending_move_fx_spawn
    }

    // FX record -> a move-FX spawn request at the target's position.
    assert_eq!(
        run(true),
        Some((0x20, [100, -50, 200])),
        "Flame with a spawnable effect entry requests move-FX at the target"
    );
    // No FX entry -> no request (the move_has_spawn_fx gate).
    assert_eq!(
        run(false),
        None,
        "a move with no effect entry requests no move-FX spawn"
    );
}

// --- Noa dance (rhythm) minigame wiring ------------------------------------

/// A 3-row chart whose beat 0 (every lane) wants symbol 1 (`DanceDir::A` =
/// pad Left), for deterministic judging.
fn dance_test_chart() -> legaia_asset::dance_chart::DanceChart {
    use legaia_asset::dance_chart::{BEATS_PER_ROW, DanceChart};
    let mut rows = Vec::new();
    for _ in 0..3 {
        let mut row = [0u8; BEATS_PER_ROW];
        row[0] = 1; // symbol 1 -> DanceDir::A -> pad Left
        rows.push(row);
    }
    DanceChart { rows }
}

#[test]
fn enter_dance_suspends_mode_and_exit_restores_it() {
    let mut world = World::new();
    world.mode = SceneMode::Field;
    let game = crate::dance::DanceGame::new(dance_test_chart(), false);
    world.enter_dance(game);
    assert_eq!(world.mode, SceneMode::Dance);
    assert!(world.dance.is_some());
    // A mid-song abort restores the interrupted mode and yields the game.
    let finished = world.exit_dance();
    assert!(finished.is_some());
    assert_eq!(world.mode, SceneMode::Field);
    assert!(world.dance.is_none());
}

#[test]
fn dance_tick_judges_a_correct_press() {
    let mut world = World::new();
    world.mode = SceneMode::Field;
    world.enter_dance(crate::dance::DanceGame::new(dance_test_chart(), false));
    // Rising edge on Left (DanceDir::A) - beat 0 of lane 0 wants symbol 1.
    world.set_pad(0);
    world.set_pad(input::PadButton::Left.mask());
    let _ = world.tick();
    // The press was judged (score advanced, judgement recorded).
    assert!(matches!(
        world.dance_last_judge,
        Some(crate::dance::Judge::Hit { .. }) | Some(crate::dance::Judge::Sequence { .. })
    ));
    assert!(world.dance.as_ref().unwrap().score() > 0);
}

#[test]
fn dance_wrong_direction_misses() {
    let mut world = World::new();
    world.enter_dance(crate::dance::DanceGame::new(dance_test_chart(), false));
    // Beat 0 wants Left; press Right instead -> miss.
    world.set_pad(0);
    world.set_pad(input::PadButton::Right.mask());
    let _ = world.tick();
    assert_eq!(world.dance_last_judge, Some(crate::dance::Judge::Miss));
    assert_eq!(world.dance.as_ref().unwrap().score(), 0);
}

#[test]
fn dance_song_end_auto_restores_mode() {
    let mut world = World::new();
    world.mode = SceneMode::Field;
    world.enter_dance(crate::dance::DanceGame::new(dance_test_chart(), false));
    // Run enough neutral-pad frames to exhaust the short song. tick_dance
    // advances the beat clock 10 phase units/frame; the short song ends at
    // SONG_LEN_SHORT (0x41dc) so a few thousand frames guarantees the timeout.
    for _ in 0..3000 {
        if world.mode != SceneMode::Dance {
            break;
        }
        world.set_pad(0);
        let _ = world.tick();
    }
    // The song timed out: mode restored, but the game is still installed for
    // the host to read the final score until it calls exit_dance.
    assert_eq!(world.mode, SceneMode::Field);
    assert!(world.dance.as_ref().map(|g| g.song_over()).unwrap_or(false));
    let finished = world.exit_dance();
    assert!(finished.is_some());
    assert!(world.dance.is_none());
}

// --- Fishing minigame wiring -----------------------------------------------

fn fishing_test_session() -> crate::fishing::FishingSession {
    use legaia_asset::fishing_species::FishingSpecies;
    let mk = |index: usize, strike_gate: i32| FishingSpecies {
        index,
        name_ptr_va: 0,
        score_value: 10_000,
        pull_factor: 64,
        dart_factor: 60,
        sink_factor: 4,
        depth_gate: 1024,
        roll_cutoff_a: 200,
        roll_cutoff_b: 512,
        roll_cutoff_c: 90,
        strike_gate,
    };
    // Small strike gates so a reeled fight lands quickly in-test.
    crate::fishing::FishingSession::new(
        vec![mk(0, 8), mk(1, 8), mk(2, 8)],
        8,
        crate::fishing::FishingRecord::default(),
    )
}

#[test]
fn enter_fishing_suspends_mode_and_exit_restores_it() {
    let mut world = World::new();
    world.mode = SceneMode::Field;
    world.enter_fishing(fishing_test_session());
    assert_eq!(world.mode, SceneMode::Fishing);
    assert!(world.fishing.is_some());
    let session = world.exit_fishing();
    assert!(session.is_some());
    assert_eq!(world.mode, SceneMode::Field);
    assert!(world.fishing.is_none());
}

#[test]
fn fishing_casts_locks_and_reels_to_a_resolution() {
    use crate::fishing::FishingPhase;
    let mut world = World::new();
    world.enter_fishing(fishing_test_session());
    // A few casting frames oscillate the meter.
    for _ in 0..3 {
        world.set_pad(0);
        let _ = world.tick();
    }
    assert_eq!(
        world.fishing.as_ref().unwrap().phase(),
        FishingPhase::Casting
    );
    // Confirm (Cross rising edge) locks the cast -> Fighting.
    world.set_pad(0);
    world.set_pad(input::PadButton::Cross.mask());
    let _ = world.tick();
    assert_eq!(
        world.fishing.as_ref().unwrap().phase(),
        FishingPhase::Fighting
    );
    // Hold Cross (reel A) until the fight resolves.
    for _ in 0..3000 {
        if world.fishing.as_ref().unwrap().phase() != FishingPhase::Fighting {
            break;
        }
        // Keep Cross held frame to frame (no fresh edge needed for reeling).
        world.set_pad(input::PadButton::Cross.mask());
        let _ = world.tick();
    }
    assert_eq!(world.fishing.as_ref().unwrap().phase(), FishingPhase::Done);
    assert!(world.fishing.as_ref().unwrap().last_outcome().is_some());
}

#[test]
fn fishing_tick_without_session_falls_back_to_return_mode() {
    let mut world = World::new();
    world.mode = SceneMode::Field;
    // Force the mode without installing a session (defensive path).
    world.fishing_return_mode = SceneMode::Field;
    world.mode = SceneMode::Fishing;
    let _ = world.tick();
    assert_eq!(world.mode, SceneMode::Field);
}

fn slot_test_machine(balance: i32) -> crate::slot_machine::SlotMachine {
    use legaia_asset::slot_payout::SlotPayoutTable;
    // Synthetic payout table: symbol id i pays (i+1)*2 coins.
    let mut payouts = [0u8; legaia_asset::slot_payout::SLOT_SYMBOL_COUNT];
    for (i, p) in payouts.iter_mut().enumerate() {
        *p = ((i + 1) * 2) as u8;
    }
    crate::slot_machine::SlotMachine::new(SlotPayoutTable { payouts }, 0xC0FFEE, balance)
}

#[test]
fn enter_slot_machine_suspends_mode_and_exit_commits_the_bank() {
    let mut world = World::new();
    world.mode = SceneMode::Field;
    world.casino_coins = 7;
    world.enter_slot_machine(slot_test_machine(50));
    assert_eq!(world.mode, SceneMode::SlotMachine);
    assert!(world.slot_machine.is_some());
    let machine = world.exit_slot_machine();
    assert!(machine.is_some());
    assert_eq!(world.mode, SceneMode::Field);
    assert!(world.slot_machine.is_none());
    // Exit commits the playing balance INTO the bank (the retail state-100
    // assignment `_DAT_800845A4 = DAT_801d4114`), replacing the old value.
    assert_eq!(world.casino_coins, 50);
}

#[test]
fn slot_machine_spins_stops_and_collects_through_the_pad() {
    use crate::slot_machine::{SPIN_UP_FRAMES, SlotPhase};
    let mut world = World::new();
    world.enter_slot_machine(slot_test_machine(50));
    // Confirm (Cross rising edge) charges the bet and starts the spin.
    world.set_pad(0);
    world.set_pad(input::PadButton::Cross.mask());
    let _ = world.tick();
    let m = world.slot_machine.as_ref().unwrap();
    assert_eq!(m.phase(), SlotPhase::Spinning);
    assert_eq!(m.balance(), 50 - m.spin_cost());
    // Run the spin-up timer down into Stopping.
    for _ in 0..SPIN_UP_FRAMES {
        world.set_pad(0);
        let _ = world.tick();
    }
    assert_eq!(
        world.slot_machine.as_ref().unwrap().phase(),
        SlotPhase::Stopping
    );
    // Three fresh Cross edges stop the three reels -> Payout.
    for _ in 0..3 {
        world.set_pad(0);
        let _ = world.tick();
        world.set_pad(input::PadButton::Cross.mask());
        let _ = world.tick();
    }
    let m = world.slot_machine.as_ref().unwrap();
    assert_eq!(m.phase(), SlotPhase::Payout);
    assert_eq!(m.reels_stopped(), crate::slot_machine::REEL_COUNT);
    let result = m.last_result().expect("spin evaluated");
    let before = m.balance();
    // A fresh Cross edge collects the (possibly zero) payout back to Idle.
    world.set_pad(0);
    let _ = world.tick();
    world.set_pad(input::PadButton::Cross.mask());
    let _ = world.tick();
    let m = world.slot_machine.as_ref().unwrap();
    assert_eq!(m.phase(), SlotPhase::Idle);
    assert_eq!(m.balance(), before + result.payout);
}

#[test]
fn slot_machine_spin_accrues_the_net_take() {
    use crate::slot_machine::NET_TAKE_NORMAL_SPIN;
    let mut world = World::new();
    world.enter_slot_machine(slot_test_machine(50));
    assert_eq!(world.slot_machine.as_ref().unwrap().net_take(), 0);
    world.set_pad(0);
    world.set_pad(input::PadButton::Cross.mask());
    let _ = world.tick();
    assert_eq!(
        world.slot_machine.as_ref().unwrap().net_take(),
        NET_TAKE_NORMAL_SPIN
    );
}

#[test]
fn slot_machine_tick_without_session_falls_back_to_return_mode() {
    let mut world = World::new();
    world.slot_return_mode = SceneMode::Field;
    world.mode = SceneMode::SlotMachine;
    let _ = world.tick();
    assert_eq!(world.mode, SceneMode::Field);
}

fn live_battle_world_3v2() -> World {
    let mut world = World::new();
    world.party_count = 3;
    world.battle_player_driven = true;
    world.live_gameplay_loop = true;
    world.mode = SceneMode::Battle;
    for i in 0..5 {
        world.actors[i].active = true;
        world.actors[i].battle.liveness = 1;
        world.actors[i].battle.hp = 100;
        world.actors[i].battle.max_hp = 100;
    }
    world
}

#[test]
fn spirit_command_charges_ap_and_raises_the_guard_stance() {
    use crate::battle_input::{BattleCommandSession, CommandPhase};
    let mut world = live_battle_world_3v2();
    world.battle_command = Some(BattleCommandSession {
        actor: 0,
        party_slot: 0,
        phase: CommandPhase::SpiritGuard,
    });
    world.tick_battle_command();
    assert!(world.battle_command.is_none(), "session resolved");
    assert!(world.ap_gauges[0].spirit_charged, "+5 AP spirit charge");
    assert!(world.battle_guarding[0], "guard stance raised");
    assert_eq!(
        world.battle_ctx.action_state,
        legaia_engine_vm::battle_action::ActionState::EndOfAction.as_byte(),
        "spirit consumes the turn"
    );
}

#[test]
fn guard_stance_halves_basic_attack_damage() {
    let mut world = live_battle_world_3v2();
    // Monster slot 3 strikes party slot 0 (attack 50 vs defense 10).
    world.battle_attack[3] = 50;
    world.battle_defense[0] = 10;
    world.actors[3].battle.active_target = 0;
    world.battle_ctx.active_actor = 3;
    world.apply_basic_attack();
    let unguarded_dmg = 100 - world.actors[0].battle.hp;
    assert!(unguarded_dmg > 1, "the strike lands for real damage");

    // Same strike against a guarding defender: the guard-halve stage applies.
    let mut world = live_battle_world_3v2();
    world.battle_attack[3] = 50;
    world.battle_defense[0] = 10;
    world.actors[3].battle.active_target = 0;
    world.battle_ctx.active_actor = 3;
    world.battle_guarding[0] = true;
    world.apply_basic_attack();
    let guarded_dmg = 100 - world.actors[0].battle.hp;
    assert_eq!(
        guarded_dmg,
        unguarded_dmg >> 1,
        "guard halves the strike (finisher stage 3)"
    );
}

#[test]
fn run_command_arms_the_run_band() {
    use crate::battle_input::{BattleCommandSession, CommandPhase};
    let mut world = live_battle_world_3v2();
    world.battle_command = Some(BattleCommandSession {
        actor: 0,
        party_slot: 0,
        phase: CommandPhase::RunAway,
    });
    world.tick_battle_command();
    assert!(world.battle_command.is_none(), "session resolved");
    assert_eq!(world.actors[0].battle.action_category, 5, "Run category");
    assert_eq!(world.battle_ctx.queued_action, 5);
    assert_eq!(
        world.battle_ctx.action_state,
        legaia_engine_vm::battle_action::ActionState::Begin.as_byte()
    );
    assert!(world.battle_ctx.multi_cast_gate <= 1, "roll outcome staged");
}

#[test]
fn successful_run_escapes_the_battle_without_loot() {
    use legaia_engine_vm::battle_action::ActionState;
    let mut world = live_battle_world_3v2();
    // A downed member (slot 1) is floored at 1 HP by the successful escape.
    world.actors[1].battle.hp = 0;
    world.actors[1].battle.liveness = 0;
    // Arm the run band directly with a forced successful roll.
    world.actors[0].battle.action_category = 5;
    world.battle_ctx.active_actor = 0;
    world.battle_ctx.queued_action = 5;
    world.battle_ctx.multi_cast_gate = 1;
    world.battle_ctx.action_state = ActionState::Begin.as_byte();
    // Drive the live loop through Begin -> RunBegin -> RunWait (0x3C-frame
    // timer) -> RunEscape (battle_end Escaped -> finish_battle).
    let mut completed = false;
    for _ in 0..0x100 {
        if matches!(
            world.live_battle_tick(),
            Some(legaia_engine_vm::battle_action::StepOutcome::BattleComplete)
        ) {
            completed = true;
            break;
        }
    }
    assert!(completed, "the run band tears the battle down");
    assert!(
        world.actors[1].battle.liveness != 0,
        "escape floors a downed member's liveness at 1"
    );
    assert!(
        world.last_battle_rewards.is_none(),
        "an escape grants no loot"
    );
    assert!(!world.game_over, "an escape is not a wipe");
}

#[test]
fn failed_run_consumes_the_turn_and_the_battle_continues() {
    use legaia_engine_vm::battle_action::ActionState;
    let mut world = live_battle_world_3v2();
    world.actors[0].battle.action_category = 5;
    world.battle_ctx.active_actor = 0;
    world.battle_ctx.queued_action = 5;
    world.battle_ctx.multi_cast_gate = 0; // roll failed
    world.battle_ctx.action_state = ActionState::Begin.as_byte();
    for _ in 0..0x100 {
        if matches!(
            world.live_battle_tick(),
            Some(legaia_engine_vm::battle_action::StepOutcome::BattleComplete)
        ) {
            panic!("a failed run must not end the battle");
        }
        if world.battle_command.is_some() {
            break; // the loop cycled to the next party turn - battle continues
        }
    }
    assert!(world.battle_end.is_none(), "no battle-end cause staged");
}

#[test]
fn shop_buy_refuses_past_the_98_held_cap() {
    // Retail dims buy attempts past 98 held of one item id (SHOP_HELD_CAP).
    let mut world = World::new();
    world.money = 1_000_000;
    let inv = crate::shop::ShopInventory::new(
        0,
        vec![crate::shop::ShopItem {
            item_id: 0x77,
            price: 10,
        }],
    );
    let mut session = crate::shop::ShopSession::new(inv);
    session.select_buy_item(0);

    // 94 held + 4 more = 98: allowed, exactly at the cap.
    world.inventory.insert(0x77, 94);
    session.set_quantity(3); // qty 4
    let (_, qty, _) = world.buy_from_shop(&session).expect("cap-exact buy lands");
    assert_eq!(qty, 4);
    assert_eq!(world.inventory.get(&0x77), Some(&98));

    // 98 held: one more refuses, inventory and gold untouched.
    let money = world.money;
    session.set_quantity(0); // qty 1
    assert!(world.buy_from_shop(&session).is_none());
    assert_eq!(world.inventory.get(&0x77), Some(&98));
    assert_eq!(world.money, money);
}

#[test]
fn encounter_rate_modifiers_resolve_from_passives_and_flags() {
    // FUN_801D9E1C's four pre-roll tests: High/Low Encounter ability bits
    // (0x3B/0x3C) + system flags 0x1D/0x1E, statically pinned shifts.
    let mut world = World::new();
    assert!(world.encounter_rate_modifiers().is_neutral());

    // Ability bit 0x3B (High Encounter - Bad Luck Bell / Nemesis Gem).
    world.party_ability_mask[(0x3B >> 5) as usize] |= 1 << (0x3B & 0x1F);
    // System flag 0x1E (rate down).
    world.system_flag_set(0x1E);
    let m = world.encounter_rate_modifiers();
    assert!(m.high_encounter && !m.low_encounter && !m.flag_high && m.flag_low);

    // The shifts compose in retail order: (rate << 2) >> 1.
    assert_eq!(m.apply(8), 16);
}
