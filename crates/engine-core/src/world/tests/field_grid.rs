use super::*;

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
