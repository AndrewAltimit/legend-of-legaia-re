use super::*;

/// A synthetic scene: every tile walk-visible, wall bytes only at the
/// named **grid** cells (`(col, row)` of the `+0x4000` grid).
fn scene(walls: &[(usize, usize)]) -> World {
    let mut w = World::new();
    let mut grid = vec![0u8; FIELD_GRID_LEN];
    for &(col, row) in walls {
        grid[row * FIELD_GRID_STRIDE + col] = 0xF0;
    }
    w.load_field_collision_grid(&grid);
    // Object grid: `CELL_WALK_VISIBLE` (0x1000) on every tile, LE u16.
    w.load_field_object_cells(&[0x00u8, 0x10].repeat(FIELD_GRID_LEN));
    w.player_actor_slot = Some(0);
    w.actors[0].active = true;
    w
}

/// World centre of collision-grid cell `(col, row)` under the biased
/// derivation the wall probe uses (`x in [128c, 128c+128)`,
/// `z in [128r-128, 128r)`).
fn cell_centre(col: i16, row: i16) -> (i16, i16) {
    (col * 128 + 64, row * 128 - 64)
}

/// Retail's byte index masks **both** cell axes to 7 bits
/// (`0x801D570C` column, `0x801D571C` row), so a Z past the grid's last
/// row wraps onto a real row instead of running off the buffer. The
/// unmasked port read fell out of `Vec::get` and answered "no wall",
/// which handed the player an open corridor along the far edge of every
/// scene.
#[test]
fn wall_probe_wraps_the_row_like_retail() {
    // Grid row 0 is solid, row 1 is open.
    let w = scene(&(0..FIELD_GRID_STRIDE).map(|c| (c, 0)).collect::<Vec<_>>());
    let open = cell_centre(20, 1);
    assert!(!w.field_tile_is_wall(open.0, open.1), "grid row 1 is open");
    // `z = 16300` derives z_cell 256 -> row 128, which retail masks back
    // to row 0 (`andi 0x3f80`) and must read that row's wall bits. The
    // unmasked port index ran off the buffer and answered "open".
    assert!(
        w.field_tile_is_wall(open.0, 16300),
        "a Z past the last grid row wraps onto row 0, not into open floor"
    );
}

/// The seat rescue is a **no-op on a standable tile** - an authored door
/// arrival lands exactly where the op-`0x3F` operand says.
#[test]
fn authored_seat_is_untouched() {
    let mut w = scene(&[]);
    w.seat_player_at_tile(20, 20);
    let ms = &w.actors[0].move_state;
    assert_eq!((ms.world_x, ms.world_z), (20 * 128 + 64, 20 * 128 + 64));
    // ...and the half-tile bit still selects the far half.
    w.seat_player_at_tile(20 | 0x80, 20);
    let ms = &w.actors[0].move_state;
    assert_eq!((ms.world_x, ms.world_z), (20 * 128 + 128, 20 * 128 + 64));
}

/// A seat the walkability grid does not cover is nudged onto the nearest
/// open sub-cell rather than parking the player inside a wall, where
/// every locomotion direction is blocked.
#[test]
fn seat_inside_a_wall_is_rescued_to_open_floor() {
    // A 3x3 grid-cell block of solid wall over the seat tile (20, 20),
    // whose biased cell is (col 20, row 21).
    let walls: Vec<(usize, usize)> = (19..=21)
        .flat_map(|c| (20..=22).map(move |r| (c, r)))
        .collect();
    let mut w = scene(&walls);
    let asked = (20i16 * 128 + 64, 20i16 * 128 + 64);
    assert!(
        w.field_tile_is_wall(asked.0, asked.1),
        "the tile the caller names is a wall"
    );
    w.seat_player_at_tile_rescued(20, 20);
    let seated = {
        let ms = &w.actors[0].move_state;
        (ms.world_x, ms.world_z)
    };
    assert_ne!(seated, asked, "the seat moved off the wall");
    assert!(
        !w.field_tile_is_wall(seated.0, seated.1),
        "the rescued seat is standable"
    );
    // The nudge stays local - within the documented sub-cell radius.
    let d = ((seated.0 - asked.0) as i32)
        .abs()
        .max(((seated.1 - asked.1) as i32).abs());
    assert!(
        d <= SEAT_RESCUE_RADIUS_SUBCELLS * 64 + 64,
        "the nudge is bounded, got {d} units"
    );
}

/// The *authored* seat keeps the wall tile. A door is a gap in a wall, so
/// a real op-`0x3F` arrival tile routinely reads as closed - `jou`'s
/// castle door `(94, 97)` and two of `map01`'s four `suimon` portals do.
/// Rescuing those lands the player off the destination's own walk-on band
/// and the door goes dead, which is why the nudge is opt-in.
#[test]
fn an_authored_door_seat_keeps_its_wall_tile() {
    let walls: Vec<(usize, usize)> = (19..=21)
        .flat_map(|c| (20..=22).map(move |r| (c, r)))
        .collect();
    let mut w = scene(&walls);
    let asked = (20i16 * 128 + 64, 20i16 * 128 + 64);
    assert!(
        w.field_tile_is_wall(asked.0, asked.1),
        "the door tile is a wall"
    );
    w.seat_player_at_tile(20, 20);
    let ms = &w.actors[0].move_state;
    assert_eq!(
        (ms.world_x, ms.world_z),
        asked,
        "an op-0x3F arrival lands byte-exactly on its operand even when \
         the walkability grid marks that tile closed"
    );
}

/// Nothing standable within the radius: the caller's coordinate is
/// returned unchanged rather than the player being hauled across the map.
#[test]
fn a_seat_with_no_open_floor_nearby_is_left_alone() {
    let walls: Vec<(usize, usize)> = (0..FIELD_GRID_STRIDE)
        .flat_map(|c| (0..FIELD_GRID_STRIDE).map(move |r| (c, r)))
        .collect();
    let w = scene(&walls);
    assert_eq!(w.nearest_standable_seat(2624, 2624), (2624, 2624));
}

/// The cold-spawn resolver's main region skips an open area that reaches
/// three or more map edges - the shape a kingdom overworld's sea has, and
/// the reason a size-only pick seated the player offshore. Here the
/// border ring is the bigger region and the enclosed pocket the smaller
/// one; the pocket must win.
#[test]
fn cold_spawn_prefers_an_enclosed_region_over_the_map_border() {
    // Wall ring around grid cells 8..=40 seals an interior pocket;
    // everything outside it is one big open region touching all four map
    // edges - the shape of a kingdom overworld's sea.
    let mut walls: Vec<(usize, usize)> = Vec::new();
    for i in 8..=40 {
        walls.push((i, 8));
        walls.push((i, 40));
        walls.push((8, i));
        walls.push((40, i));
    }
    let w = scene(&walls);
    let inside = (FIELD_COLD_SPAWN_XZ, FIELD_COLD_SPAWN_XZ);
    let pocket = w.field_walk_component_size(inside.0, inside.1);
    assert!(pocket > 0, "the pocket is open floor");
    assert!(
        w.field_largest_walk_component_size() > pocket,
        "the border region is the larger of the two - a size-only pick \
         would take it"
    );
    // The retail cold seat (0xA40) is inside the pocket, so rule 1 keeps
    // it: that is the assertion that the pocket, not the border region,
    // is the main region.
    assert_eq!(
        w.resolve_cold_field_spawn(&[], &[]),
        (FIELD_COLD_SPAWN_XZ, FIELD_COLD_SPAWN_XZ)
    );
}

/// The picker's door-arrival seat takes the first arrival standing in an
/// enclosed room: one in the open space around the map is skipped, and one
/// on an avoided tile (a teleport pad, a walk-on trigger) is stepped off it.
#[test]
fn picker_arrival_seat_takes_the_first_enclosed_standable_arrival() {
    // Walls everywhere except an open room over grid cells 10..30 (both
    // axes) and the outer band (cols/rows >= 100), which reaches the map
    // edges and so reads as the open space around the scene.
    let walls: Vec<(usize, usize)> = (0..FIELD_GRID_STRIDE)
        .flat_map(|c| (0..FIELD_GRID_STRIDE).map(move |r| (c, r)))
        .filter(|&(c, r)| !((10..30).contains(&c) && (10..30).contains(&r)))
        .filter(|&(c, r)| c < 100 && r < 100)
        .collect();
    let w = scene(&walls);
    let room = cell_centre(20, 20);
    let pad = cell_centre(15, 15);
    let outside = cell_centre(110, 110);
    let pad_tile = ((pad.0 >> 7) as u8, (pad.1 >> 7) as u8);
    assert_eq!(
        w.picker_arrival_seat(&[outside, room], &[pad_tile]),
        Some((1, room)),
        "the open-space arrival is skipped; a room arrival is taken as is"
    );
    let (i, seat) = w
        .picker_arrival_seat(&[outside, pad, room], &[pad_tile])
        .expect("the pad arrival steps off its pad");
    assert_eq!(i, 1);
    assert_ne!(((seat.0 >> 7) as u8, (seat.1 >> 7) as u8), pad_tile);
    assert!((seat.0 - pad.0).abs() <= 128 && (seat.1 - pad.1).abs() <= 128);
    assert_eq!(w.picker_arrival_seat(&[outside], &[]), None);
}
