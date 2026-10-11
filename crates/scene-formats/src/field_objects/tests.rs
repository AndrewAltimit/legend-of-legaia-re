use super::*;

#[test]
fn vahns_house_position_formula() {
    // town01 record 137: x_off=-64, y_off=0, z_off=56, anchor (col 38, row 25).
    assert_eq!(world_x(38, -64), 4864);
    assert_eq!(world_z(25, 56), 3208);
}

#[test]
fn record_parse_reads_offsets_and_flag() {
    let mut table = vec![0u8; OBJECT_RECORD_STRIDE * 2];
    // record 1: x=-64, y=0, z=56, col_delta=-1, row_delta=-1, flags=0x101E.
    let r = &mut table[OBJECT_RECORD_STRIDE..];
    r[0x00..0x02].copy_from_slice(&(-64i16).to_le_bytes());
    r[0x04..0x06].copy_from_slice(&56i16.to_le_bytes());
    r[0x06] = (-1i8) as u8;
    r[0x07] = (-1i8) as u8;
    r[0x12..0x14].copy_from_slice(&0x101Eu16.to_le_bytes());
    let rec = ObjectRecord::parse(&table, 1).unwrap();
    assert_eq!(rec.x_off, -64);
    assert_eq!(rec.z_off, 56);
    assert_eq!(rec.col_delta, -1);
    assert!(rec.is_placed());
}

#[test]
fn parse_placements_emits_placed_tiles_only() {
    // Build a synthetic field map: one placed record (idx 137, the Vahn's-
    // house id: >=120 so its mesh comes from the +0x10 field), grid cell
    // (row 25, col 38) -> idx 137, every other cell -> idx 0 (unplaced,
    // since record 0 has no placed flag).
    let mut map = vec![0u8; 0x12000];
    let r = &mut map[OBJECT_RECORD_STRIDE * 137..OBJECT_RECORD_STRIDE * 138];
    r[0x00..0x02].copy_from_slice(&(-64i16).to_le_bytes());
    r[0x04..0x06].copy_from_slice(&56i16.to_le_bytes());
    r[0x10..0x12].copy_from_slice(&36u16.to_le_bytes()); // pack mesh index
    r[0x12..0x14].copy_from_slice(&0x0004u16.to_le_bytes()); // placed
    let cell = OBJECT_GRID_OFFSET + (25 * GRID_DIM + 38) * 2;
    map[cell..cell + 2].copy_from_slice(&137u16.to_le_bytes());
    let p = parse_placements(&map);
    assert_eq!(p.len(), 1);
    assert_eq!(p[0].obj_idx, 137);
    assert_eq!((p[0].col, p[0].row), (38, 25));
    assert_eq!((p[0].world_x, p[0].world_z), (4864, 3208));
    assert_eq!(p[0].pack_index, Some(36));
}

#[test]
fn parse_terrain_tiles_emits_visible_cells_regardless_of_placed_flag() {
    // Record 5: a terrain tile mesh, NOT placed-flagged (flags = 0).
    let mut map = vec![0u8; 0x12000];
    let r = &mut map[OBJECT_RECORD_STRIDE * 5..OBJECT_RECORD_STRIDE * 6];
    r[0x10..0x12].copy_from_slice(&12u16.to_le_bytes()); // pack mesh index
    // flags stays 0 -> NOT placed (parse_placements would skip it).
    // Cell (row 10, col 20): visible bit set + record index 5.
    let cell = OBJECT_GRID_OFFSET + (10 * GRID_DIM + 20) * 2;
    map[cell..cell + 2].copy_from_slice(&(CELL_VISIBLE | 5).to_le_bytes());
    // A second cell pointing at record 5 but WITHOUT the visible bit: skipped.
    let cell2 = OBJECT_GRID_OFFSET + (11 * GRID_DIM + 20) * 2;
    map[cell2..cell2 + 2].copy_from_slice(&5u16.to_le_bytes());

    // Floor grid: the cell tile at tier 4, its +X / +Z / +XZ corner
    // neighbours at tiers 8 / 4 / 8 - a tier-edge ramp cell.
    map[0x4000 + 10 * GRID_DIM + 20] = 0x14; // wall bits in the high nibble
    map[0x4000 + 10 * GRID_DIM + 21] = 0x08;
    map[0x4000 + 11 * GRID_DIM + 20] = 0x04;
    map[0x4000 + 11 * GRID_DIM + 21] = 0x28;

    // parse_placements drops the unplaced record entirely.
    assert!(parse_placements(&map).is_empty());
    // parse_terrain_tiles emits the visible cell (and only it).
    let t = parse_terrain_tiles(&map);
    assert_eq!(t.len(), 1, "only the CELL_VISIBLE cell is emitted");
    assert_eq!(t[0].obj_idx, 5);
    assert_eq!((t[0].col, t[0].row), (20, 10));
    assert_eq!(t[0].pack_index, Some(12));
    // The terrain sweep carries the 2x2 corner-tile floor-nibble block
    // (flat `+0x4000` offsets +0/+1/+0x80/+0x81, wall nibbles masked off)
    // for the retail per-cell emitters' corner-average Y (`FUN_801F69D8`
    // / `FUN_801F7088`).
    assert_eq!(t[0].floor_nibble, Some(4));
    assert_eq!(t[0].floor_corner_nibbles, Some([4, 8, 4, 8]));
}

/// The placed-object sweep resolves world Y from the single placement-tile
/// nibble (`FUN_8003A55C`), so it must NOT carry the terrain sweeps'
/// corner block - `field_env` keys the two retail Y formulas on it.
#[test]
fn parse_placements_carries_no_corner_block() {
    let mut map = vec![0u8; 0x12000];
    let r = &mut map[OBJECT_RECORD_STRIDE * 7..OBJECT_RECORD_STRIDE * 8];
    r[0x10..0x12].copy_from_slice(&3u16.to_le_bytes());
    r[0x12..0x14].copy_from_slice(&FLAG_PLACED.to_le_bytes());
    let cell = OBJECT_GRID_OFFSET + (10 * GRID_DIM + 20) * 2;
    map[cell..cell + 2].copy_from_slice(&7u16.to_le_bytes());
    map[0x4000 + 10 * GRID_DIM + 20] = 0x06;
    let p = parse_placements(&map);
    assert_eq!(p.len(), 1);
    assert_eq!(p[0].floor_nibble, Some(6));
    assert_eq!(p[0].floor_corner_nibbles, None);
}

#[test]
fn parse_walk_decorations_gates_on_the_draw_bit_and_skips_placed_records() {
    let mut map = vec![0u8; 0x12000];
    // Record 475: a tree mesh (nonzero +0x10, decoration flags 0x0013) -
    // its cells carry both the ground bit and the draw bit, like every
    // forest cell on the retail kingdom maps.
    let r = &mut map[OBJECT_RECORD_STRIDE * 475..OBJECT_RECORD_STRIDE * 476];
    r[0x10..0x12].copy_from_slice(&11u16.to_le_bytes());
    r[0x12..0x14].copy_from_slice(&0x0013u16.to_le_bytes());
    // Record 412: a big enterable mountain (Drake mesh 23, flags 0x0012)
    // whose cell carries the draw bit WITHOUT the ground bit - the mesh
    // is the ground there. The 0x1000-gated sweep lost exactly these.
    let r = &mut map[OBJECT_RECORD_STRIDE * 412..OBJECT_RECORD_STRIDE * 413];
    r[0x00..0x02].copy_from_slice(&(-64i16).to_le_bytes());
    r[0x04..0x06].copy_from_slice(&(-64i16).to_le_bytes());
    r[0x10..0x12].copy_from_slice(&23u16.to_le_bytes());
    r[0x12..0x14].copy_from_slice(&0x0012u16.to_le_bytes());
    // Record 349: a placed landmark (nonzero +0x10 AND the placed flag).
    let r = &mut map[OBJECT_RECORD_STRIDE * 349..OBJECT_RECORD_STRIDE * 350];
    r[0x10..0x12].copy_from_slice(&6u16.to_le_bytes());
    r[0x12..0x14].copy_from_slice(&0x0017u16.to_le_bytes());
    // Record 200: bulk ground (+0x10 == 0, flags 0x0011), ground bit only.
    let r = &mut map[OBJECT_RECORD_STRIDE * 200..OBJECT_RECORD_STRIDE * 201];
    r[0x12..0x14].copy_from_slice(&0x0011u16.to_le_bytes());
    // Record 408: the riverbank/system family - nonzero +0x10, flags
    // 0x0011, and (on every retail kingdom map) cells that carry only the
    // ground bit. Stamping it draws a wall mesh down every river; the
    // draw gate is what keeps it out, not a flag test.
    let r = &mut map[OBJECT_RECORD_STRIDE * 408..OBJECT_RECORD_STRIDE * 409];
    r[0x10..0x12].copy_from_slice(&4u16.to_le_bytes());
    r[0x12..0x14].copy_from_slice(&0x0011u16.to_le_bytes());
    // Record 285: a prop record whose +0x10 is 0 on a draw-gated cell
    // (Drake cell (98, 66), flags 0x0813) - stamps pack slot 0.
    let r = &mut map[OBJECT_RECORD_STRIDE * 285..OBJECT_RECORD_STRIDE * 286];
    r[0x12..0x14].copy_from_slice(&0x0813u16.to_le_bytes());
    for (row, col, idx, bits) in [
        (4, 10, 475u16, CELL_WALK_VISIBLE | CELL_VISIBLE),
        (4, 11, 475, CELL_WALK_VISIBLE | CELL_VISIBLE),
        (80, 39, 412, CELL_VISIBLE),
        (5, 10, 349, CELL_WALK_VISIBLE | CELL_VISIBLE),
        (5, 11, 200, CELL_WALK_VISIBLE),
        (6, 10, 408, CELL_WALK_VISIBLE),
        (66, 98, 285, CELL_WALK_VISIBLE | CELL_VISIBLE),
    ] {
        let cell = OBJECT_GRID_OFFSET + (row * GRID_DIM + col) * 2;
        map[cell..cell + 2].copy_from_slice(&(bits | idx).to_le_bytes());
    }
    let d = parse_walk_decorations(&map);
    // The two tree cells, the mountain, and the slot-0 prop: the placed
    // landmark belongs to parse_placements, and the ground / riverbank
    // cells never carry the draw bit.
    assert_eq!(
        d.iter()
            .map(|p| (p.obj_idx, p.pack_index, p.col, p.row))
            .collect::<Vec<_>>(),
        vec![
            (475, Some(11), 10, 4),
            (475, Some(11), 11, 4),
            (285, Some(0), 98, 66),
            (412, Some(23), 39, 80),
        ]
    );
    assert!(d.iter().all(|p| p.floor_corner_nibbles.is_some()));
    let mountain = d.iter().find(|p| p.obj_idx == 412).unwrap();
    assert_eq!(
        (mountain.world_x, mountain.world_z),
        (world_x(39, -64), world_z(80, -64))
    );
}

#[test]
fn placement_world_y_averages_corners_or_reads_the_single_nibble() {
    // Runtime LUT convention: every term is `-lut[nibble]`.
    let mut lut = [0i16; 16];
    lut[1] = 48;
    lut[2] = 96;
    lut[4] = 192;
    let mut p = Placement {
        obj_idx: 1,
        col: 0,
        row: 0,
        anchor_col: 0,
        anchor_row: 0,
        anchor_cell: 0,
        world_x: 0,
        world_z: 0,
        y_off: 10,
        floor_nibble: Some(4),
        floor_corner_nibbles: None,
        pack_index: Some(0),
        flags: 0,
        rot_x: 0,
        rot_y: 0,
        rot_z: 0,
        collider_x: 0,
        collider_z: 0,
        cull_radius: 0,
    };
    // Placed object: single nibble.
    assert_eq!(p.world_y(&lut), -192 + 10);
    // Terrain / decoration cell on a tier edge: the corner average, sum
    // divided by 4 rounding toward zero. (-48 - 96 - 48 - 96) / 4 = -72.
    p.floor_corner_nibbles = Some([1, 2, 1, 2]);
    assert_eq!(p.world_y(&lut), -72 + 10);
    // Rounding toward zero on a negative sum: (-48 + 0 + 0 + 0) / 4 = -12,
    // and (-48 - 48 - 48 + 0) = -144 / 4 = -36 exactly; an odd case:
    // (-48 - 96 + 0 + 0) = -144 -> -36; (-96 + 0 + 0 + 0) = -96 -> -24;
    // (-48 - 96 - 96 + 0) = -240 -> -60; and -(48+96+96+96) = -336 -> -84.
    p.floor_corner_nibbles = Some([1, 0, 0, 0]);
    assert_eq!(p.world_y(&lut), -12 + 10);
    // A sum that does not divide evenly: (-48 - 48 - 48 - 96) = -240 / 4
    // = -60; (-48 -48 -96 -96) = -288 / 4 = -72; make one that is odd by
    // using lut entries 48 and 96 with three zeros: -48 -> (-48+3)>>2 =
    // -45>>2 = -12 (toward zero), whereas -48>>2 = -12 as well; use
    // -96 - 48 = -144 -> -36. The toward-zero clause matters for e.g.
    // sum = -50: (-50 + 3) >> 2 = -47 >> 2 = -12, not -13.
    lut[3] = 50;
    p.floor_corner_nibbles = Some([3, 0, 0, 0]);
    assert_eq!(p.world_y(&lut), -12 + 10);
    // No floor grid at all: 0 (the y_off is not applied either, matching
    // the draw resolver's fallback).
    p.floor_corner_nibbles = None;
    p.floor_nibble = None;
    assert_eq!(p.world_y(&lut), 0);
}

#[test]
fn pack_mesh_index_rule() {
    let mut rec = ObjectRecord::parse(&[0u8; OBJECT_RECORD_STRIDE], 0).unwrap();
    rec.pack_index_field = 15;
    // >= 120 and == 83 use the +0x10 field.
    assert_eq!(pack_mesh_index(230, &rec), Some(15));
    assert_eq!(pack_mesh_index(83, &rec), Some(15));
    // Ids 93..=118 are NOT positional: they take `+0x10` like every other
    // id (the old `obj_idx - 5` "field-actor band" rule is falsified - see
    // the town0c cell (30,17) / env-mesh-2 prim match in the fn docs).
    assert_eq!(pack_mesh_index(96, &rec), Some(15));
    assert_eq!(pack_mesh_index(118, &rec), Some(15));
    // Protagonist / NPC ids draw from a different pool.
    assert_eq!(pack_mesh_index(1, &rec), None);
    assert_eq!(pack_mesh_index(3, &rec), None);
}

#[test]
fn heightfield_emits_quad_per_visible_cell_with_floor_heights() {
    let mut map = vec![0u8; 0x12000];
    // Floor LUT: nibble 2 -> height 80, nibble 5 -> 200 (negated in mesh).
    let lut = {
        let mut l = [0i16; 16];
        l[2] = 80;
        l[5] = 200;
        l
    };
    // Tile (col 10, row 4): floor nibble 2; mark its object cell walk-visible
    // with obj_idx 7, and give record 7 a terrain run (+0x14..+0x18): tile
    // 0x2A on the mountain page 0x0C with CLUT 0x7EC0.
    map[COLLISION_GRID_OFFSET + 4 * GRID_DIM + 10] = 0x02;
    let cell = OBJECT_GRID_OFFSET + (4 * GRID_DIM + 10) * 2;
    map[cell..cell + 2].copy_from_slice(&(CELL_WALK_VISIBLE | 7).to_le_bytes());
    map[7 * OBJECT_RECORD_STRIDE + 0x14] = 0x2A; // tile index
    map[7 * OBJECT_RECORD_STRIDE + 0x15] = 0x0C; // tpage (mountain page)
    map[7 * OBJECT_RECORD_STRIDE + 0x16] = 0xC0; // clut low
    map[7 * OBJECT_RECORD_STRIDE + 0x17] = 0x7E; // clut high

    let hf = build_walk_heightfield(&map, &lut);
    assert_eq!(hf.quad_count(), 1, "one visible cell -> one quad");
    assert_eq!(hf.positions.len(), 4);
    assert_eq!(hf.indices.len(), 6);
    // Corner (10,4) sits at world (10*128, -lut[2], 4*128).
    assert_eq!(hf.positions[0], [1280.0, -80.0, 512.0]);
    // The far corner (11,5) reads nibble 0 (height 0) since those tiles
    // weren't painted.
    assert_eq!(hf.positions[3][1], 0.0);
    // Every vertex carries the cell's +0x14 tile id.
    assert!(hf.tile_ids.iter().all(|&t| t == 0x2A));
    // Atlas tile 0x2A = 42 -> (42 % 8, 42 / 8) = (col 2, row 5) -> UV origin
    // (64, 160), spanning a 32x32 rect. V is flipped vs the row axis (retail
    // emitter): the low-Z (row) corner takes the tile's bottom texel row.
    assert_eq!(hf.uvs.len(), 4);
    assert_eq!(hf.uvs[0], [64, 191]); // (col,   row)   low-Z -> bottom row
    assert_eq!(hf.uvs[1], [95, 191]); // (col+1, row)
    assert_eq!(hf.uvs[2], [64, 160]); // (col,   row+1) high-Z -> top row
    assert_eq!(hf.uvs[3], [95, 160]); // (col+1, row+1)
    // Every vertex carries the cell's [clut, tpage] from +0x15 / +0x16..+0x18.
    assert!(hf.cba_tsb.iter().all(|&ct| ct == [0x7EC0, 0x000C]));
}

/// A decoration cell draws its record's `+0x10` mesh whatever the
/// record's index - the per-cell pass has no id test - while a placed
/// record on ids `1..=3` keeps the spawner's carve-out.
#[test]
fn a_decoration_cell_on_a_low_record_id_keeps_its_mesh() {
    let mut map = vec![0u8; 0x12000];
    // Record 3: unplaced scenery, pack mesh 10. Record 2: placed.
    let r3 = 3 * OBJECT_RECORD_STRIDE;
    map[r3 + 0x10] = 10;
    map[r3 + 0x12] = 0x13;
    let r2 = 2 * OBJECT_RECORD_STRIDE;
    map[r2 + 0x10] = 31;
    map[r2 + 0x12] = 0x12 | FLAG_PLACED as u8;
    let set = |map: &mut Vec<u8>, col: usize, row: usize, word: u16| {
        let c = OBJECT_GRID_OFFSET + (row * GRID_DIM + col) * 2;
        map[c..c + 2].copy_from_slice(&word.to_le_bytes());
    };
    set(&mut map, 105, 64, CELL_VISIBLE | CELL_WALK_VISIBLE | 3);
    set(&mut map, 20, 9, CELL_VISIBLE | 2);
    let tiles = parse_terrain_tiles(&map);
    let at = |col: u8, row: u8| tiles.iter().find(|p| (p.col, p.row) == (col, row)).unwrap();
    assert_eq!(at(105, 64).pack_index, Some(10));
    assert_eq!(at(20, 9).pack_index, None);
}

#[test]
fn overworld_gate_draws_every_textured_cell_and_skips_untextured_ones() {
    let lut = [0i16; 16];
    let mut map = vec![0u8; 0x12000];
    // Record 1 carries a terrain page, record 2 does not.
    map[OBJECT_RECORD_STRIDE + 0x15] = 0x1A;
    let set = |map: &mut Vec<u8>, col: usize, row: usize, word: u16| {
        let c = OBJECT_GRID_OFFSET + (row * GRID_DIM + col) * 2;
        map[c..c + 2].copy_from_slice(&word.to_le_bytes());
    };
    set(&mut map, 3, 3, CELL_WALK_VISIBLE | 1); // visible, textured
    set(&mut map, 4, 3, 0x2000 | 1); // no 0x1000, textured
    set(&mut map, 5, 3, 0x2000 | 2); // no 0x1000, no terrain page
    let field = build_walk_heightfield(&map, &lut);
    let overworld = build_ground_heightfield(&map, &lut, GroundCellGate::TerrainRecord);
    let xs = |hf: &WalkHeightfield| {
        hf.positions
            .chunks(4)
            .map(|c| c[0][0] as i32 / TILE)
            .collect::<Vec<_>>()
    };
    // The field gate keeps the one visible cell (record 2's cell 5 is
    // absent too, and every zero word falls to record 0, untextured).
    assert_eq!(xs(&field), vec![3]);
    // The overworld draws cell 4 as well, and still not the untextured 5.
    assert_eq!(xs(&overworld), vec![3, 4]);
}

#[test]
fn ground_page_and_clut_are_per_cell_from_record() {
    // Two adjacent walk-visible cells with different terrain records: a
    // grass cell (page 0x1A) and a water cell (page 0x1B) coexist in one
    // heightfield, each sampling its own page - the multi-page terrain atlas.
    let lut = [0i16; 16];
    let mut map = vec![0u8; 0x12000];
    // record 1: grass tile 9 on page 0x1A / CLUT 0x7C40.
    let g = OBJECT_RECORD_STRIDE;
    map[g + 0x14] = 9;
    map[g + 0x15] = 0x1A;
    map[g + 0x16] = 0x40;
    map[g + 0x17] = 0x7C;
    // record 2: water tile 17 on page 0x1B / CLUT 0x7F41.
    let w = 2 * OBJECT_RECORD_STRIDE;
    map[w + 0x14] = 17;
    map[w + 0x15] = 0x1B;
    map[w + 0x16] = 0x41;
    map[w + 0x17] = 0x7F;
    let cg = OBJECT_GRID_OFFSET; // (col 0, row 0)
    map[cg..cg + 2].copy_from_slice(&(CELL_WALK_VISIBLE | 1).to_le_bytes());
    let cw = OBJECT_GRID_OFFSET + 2; // (col 1, row 0)
    map[cw..cw + 2].copy_from_slice(&(CELL_WALK_VISIBLE | 2).to_le_bytes());

    let hf = build_walk_heightfield(&map, &lut);
    assert_eq!(hf.quad_count(), 2);
    // First cell (grass): tile 9 -> origin (32, 32); first (low-Z) corner takes
    // the bottom texel row (V flipped vs row), page 0x1A / CLUT 0x7C40.
    assert_eq!(hf.uvs[0], [32, 63]);
    assert_eq!(hf.cba_tsb[0], [0x7C40, 0x001A]);
    // Second cell (water): tile 17 -> (17 % 8, 17 / 8) = (1, 2) -> origin
    // (32, 64); first corner -> bottom row (32, 95), page 0x1B / CLUT 0x7F41.
    assert_eq!(hf.uvs[4], [32, 95]);
    assert_eq!(hf.cba_tsb[4], [0x7F41, 0x001B]);
}

#[test]
fn collision_footprint_offset_matches_live_verified_records() {
    // town01 record 315 (live static actor, `v0_1_pre_battle_tetsu`):
    // col_delta 1, sub (-3, -3), no correction flag -> (80, -48).
    let mut rec = [0u8; OBJECT_RECORD_STRIDE];
    rec[0x06] = 1;
    rec[0x0E] = 0xFD;
    rec[0x0F] = 0xFD;
    rec[0x12] = 0x16;
    let r = ObjectRecord::parse(&rec, 0).unwrap();
    assert_eq!(collision_footprint_offset(&r), (80, -48));

    // town01 record 137 (the correction arm: flag bit 0x8 -> the spawn
    // mirrors it into actor +0x52, and the offset subtracts x_off /
    // adds z_off): deltas (-1, -1), sub (2, 2), x_off -64, z_off 56
    // -> (-96 + 64, -96 + 56) = (-32, -40).
    let mut rec = [0u8; OBJECT_RECORD_STRIDE];
    rec[0x00..0x02].copy_from_slice(&(-64i16).to_le_bytes());
    rec[0x04..0x06].copy_from_slice(&56i16.to_le_bytes());
    rec[0x06] = 0xFF;
    rec[0x07] = 0xFF;
    rec[0x0E] = 2;
    rec[0x0F] = 2;
    rec[0x12..0x14].copy_from_slice(&0x101Eu16.to_le_bytes());
    let r = ObjectRecord::parse(&rec, 0).unwrap();
    assert_eq!(collision_footprint_offset(&r), (-32, -40));
}

#[test]
fn placements_carry_collider_centres() {
    // A placed record with a footprint offset: the placement's collider
    // centre = its world position + the offset.
    let mut map = vec![0u8; 0x12000];
    let rec = OBJECT_RECORD_STRIDE; // record 1
    map[rec + 0x06] = 1; // col_delta 1 -> +128
    map[rec + 0x0E] = 0xFD; // sub X -3 -> -48
    map[rec + 0x0F] = 0xFD; // sub Z -3 -> -48
    map[rec + 0x12] = FLAG_PLACED as u8;
    let cell = OBJECT_GRID_OFFSET + (3 * GRID_DIM + 5) * 2; // (col 5, row 3)
    map[cell..cell + 2].copy_from_slice(&1u16.to_le_bytes());
    let ps = parse_placements(&map);
    assert_eq!(ps.len(), 1);
    let p = &ps[0];
    assert_eq!((p.world_x, p.world_z), (5 * 128 + 64, 3 * 128 + 64));
    assert_eq!(p.collider_x, p.world_x + 128 - 48);
    assert_eq!(p.collider_z, p.world_z - 48);
}

#[test]
fn ground_cell_without_terrain_bytes_falls_back_to_grass_page() {
    // A visible cell whose record carries no terrain run (tpage 0) uses the
    // grass fallback page, never tpage 0 (= fb (0,0), the framebuffer).
    let lut = [0i16; 16];
    let mut map = vec![0u8; 0x12000];
    let cell = OBJECT_GRID_OFFSET;
    map[cell..cell + 2].copy_from_slice(&(CELL_WALK_VISIBLE | 1).to_le_bytes());
    let hf = build_walk_heightfield(&map, &lut);
    assert_eq!(hf.quad_count(), 1);
    assert_eq!(hf.cba_tsb[0], [GROUND_ATLAS_CLUT, GROUND_ATLAS_TPAGE]);
}
