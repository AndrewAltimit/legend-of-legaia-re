use super::*;

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
    let b = w.board.grid.as_ref().unwrap();
    // col advances 0 -> 1 -> 2, then (3,_) is out of bounds -> stops.
    assert_eq!(b.player_col, 2);
    assert_eq!(b.player_row, 0);
    // Actor settled on the (2,0) tile centre and the step is idle.
    let (tx, _tz) = b.tile_world(2, 0);
    assert_eq!(w.actors[0].move_state.world_x as i32, tx);
    assert_eq!(w.board.target, None);
}

#[test]
fn tile_board_takes_multiple_frames_per_tile() {
    let mut w = tile_board_world();
    // One tick: direction committed (col 0 -> 1), target set, but the
    // actor hasn't reached the next tile centre yet.
    w.set_pad(input::PadButton::Right.mask());
    let _ = w.tick();
    assert_eq!(w.board.grid.as_ref().unwrap().player_col, 1);
    assert!(w.board.target.is_some());
    let (tx, _) = w.board.grid.as_ref().unwrap().tile_world(1, 0);
    assert!((w.actors[0].move_state.world_x as i32) < tx);
}

#[test]
fn tile_board_blocked_by_wall() {
    let mut w = tile_board_world();
    // Start the player directly north of the (1,1) wall.
    {
        let b = w.board.grid.as_mut().unwrap();
        b.player_col = 1;
        b.player_row = 0;
    }
    let (x, z) = w.board.grid.as_ref().unwrap().player_world();
    w.actors[0].move_state.world_x = x as i16;
    w.actors[0].move_state.world_z = z as i16;
    let before = w.actors[0].move_state.world_z;
    // Up (`0x1000`, row + 1) would step into the (1,1) wall - rejected,
    // player stays.
    pad_held(&mut w, input::PadButton::Up.mask(), 20);
    let b = w.board.grid.as_ref().unwrap();
    assert_eq!((b.player_col, b.player_row), (1, 0));
    assert_eq!(w.actors[0].move_state.world_z, before);
    assert_eq!(w.board.target, None);
}

#[test]
fn tile_board_gated_by_dialog() {
    let mut w = tile_board_world();
    w.dialog.current = Some(DialogRequest {
        text_id: 0,
        inline: Vec::new(),
        world_x: 0,
        world_z: 0,
        depth_id: 0,
    });
    pad_held(&mut w, input::PadButton::Right.mask(), 20);
    let b = w.board.grid.as_ref().unwrap();
    assert_eq!((b.player_col, b.player_row), (0, 0));
    assert_eq!(w.board.target, None);
}

/// Build the 14-byte op-0x49 sub-5 instruction (`[0x49, 0x05, ...13-byte
/// header]`) from a [`TileBoardHeader`] and install the board on a fresh
/// Field world with a player actor in slot 0.
fn install_board(h: crate::tile_board::TileBoardHeader) -> World {
    let mut w = install_board_raw(h);
    plain_start_cell(&mut w);
    // The walk SM's fade-in (state 1) ignores input until the tiles are at
    // full scale.
    assert_eq!(w.board.sm, crate::tile_board::sm::FADE_IN);
    let mut n = 0;
    while w.board.sm == crate::tile_board::sm::FADE_IN {
        let _ = w.tick();
        n += 1;
        assert!(n < 100, "fade-in finishes");
    }
    finish_walk_in(&mut w);
    w
}

/// Install the board from its op-49 instruction without ticking.
fn install_board_raw(h: crate::tile_board::TileBoardHeader) -> World {
    let mut w = World::new();
    w.mode = SceneMode::Field;
    w.player_actor_slot = Some(0);
    w.actors[0].active = true;
    let instr = [
        0x49,
        0x05,
        h.origin_x,
        h.origin_z,
        h.width,
        h.height,
        h.radius,
        h.mode_flag,
        h.flag_base_set.to_le_bytes()[0],
        h.flag_base_set.to_le_bytes()[1],
        h.flag_base_test.to_le_bytes()[0],
        h.flag_base_test.to_le_bytes()[1],
        h.player_template,
        h.tile_template_base,
    ];
    assert!(w.try_install_tile_board(&instr), "board installs");
    w
}

/// Make the start cell (column 4, row 0) terrain `3` - octant `0`, no exit -
/// so the arrival pass the walk-in ends in cannot leave the board and the
/// pad reaches the board unrotated.
fn plain_start_cell(w: &mut World) {
    let b = w.board.grid.as_mut().unwrap();
    let (c, r) = (
        crate::tile_board::START_COL as usize,
        crate::tile_board::START_ROW as usize,
    );
    if c < b.width as usize && r < b.height as usize {
        let idx = r * b.width as usize + c;
        b.cells[idx] = 3;
    }
}

/// Tick with no input until the walk-in (state 2 after the fade-in) lands.
fn finish_walk_in(w: &mut World) {
    w.set_pad(0);
    let mut n = 0;
    while w.board.target.is_some() {
        let _ = w.tick();
        n += 1;
        assert!(n < 400, "the walk-in reaches the start cell");
    }
}

/// The index of the cell one step "up" (row + 1) from the start cell.
fn above_start(w: &World) -> usize {
    let b = w.board.grid.as_ref().unwrap();
    b.width as usize + crate::tile_board::START_COL as usize
}

/// Tick with no input until the board is torn down, returning the ticks it
/// took (the exit's step, fade-out, park and teardown states).
fn tick_until_torn_down(w: &mut World) -> usize {
    w.set_pad(0);
    let mut n = 0;
    while w.board.grid.is_some() {
        let _ = w.tick();
        n += 1;
        assert!(n < 200, "the exit reaches teardown");
    }
    n
}

/// A [`TileBoardHeader`] for the install tests (only the fields the tests
/// vary; flags default to 0).
fn hdr(
    width: u8,
    height: u8,
    origin_x: u8,
    origin_z: u8,
    radius: u8,
    mode_flag: u8,
    tile_template_base: u8,
) -> crate::tile_board::TileBoardHeader {
    crate::tile_board::TileBoardHeader {
        origin_x,
        origin_z,
        width,
        height,
        radius,
        mode_flag,
        player_template: 0,
        tile_template_base,
        ..Default::default()
    }
}

/// (a) Installing a board spawns exactly one active tile actor per distinct
/// drawable cell value present on the board, in the auto-spawn slot range,
/// with distinct slots; absent values get no slot.
#[test]
fn install_spawns_tile_actor_per_present_cell_value() {
    let w = install_board_raw(hdr(6, 4, 0, 0, 2, 0, 0x30));
    let board = w.board.grid.as_ref().unwrap();
    let mut present = std::collections::BTreeSet::new();
    for &c in &board.cells {
        if crate::tile_board::is_drawable_cell(c) {
            present.insert(c);
        }
    }
    assert!(
        !present.is_empty(),
        "procedural fill produces drawable cells"
    );
    let mut seen_slots = std::collections::BTreeSet::new();
    for value in 2u8..=14 {
        match w.board.actor_slots[value as usize] {
            Some(slot) => {
                assert!(present.contains(&value), "spawned only present values");
                assert!(
                    slot >= FIELD_SPAWN_START_SLOT,
                    "tile actor above the party/scripted range"
                );
                assert!(w.actors[slot as usize].active, "tile actor is active");
                assert!(seen_slots.insert(slot), "distinct slot per value");
            }
            None => assert!(!present.contains(&value), "every present value spawns"),
        }
    }
    // Table slot 0 = the existing player actor.
    assert_eq!(w.board.actor_slots[0], Some(0));
}

/// (b) Each drawn cell's actor lands at the retail world-centre coordinate
/// `(origin + idx) * 0x80 + 0x40`, references the value's spawned actor, and
/// full-board mode draws every cell.
#[test]
fn draw_list_places_each_cell_actor_at_world_centre() {
    // Full-board mode (flag 0), non-zero origin to exercise the origin term.
    let mut w = install_board(hdr(4, 3, 2, 5, 8, 0, 0x30));
    let _ = w.tick();
    let board = w.board.grid.as_ref().unwrap().clone();
    assert!(!w.board.draw_list.is_empty());
    for d in &w.board.draw_list {
        let (ex, ez) = board.tile_world(d.col as i32, d.row as i32);
        assert_eq!(
            (d.world_x, d.world_z),
            (ex, ez),
            "retail (origin+idx)*0x80+0x40"
        );
        assert!(crate::tile_board::is_drawable_cell(d.cell_value));
        assert_eq!(w.board.actor_slots[d.cell_value as usize], Some(d.slot));
        // The reposition pass moved the tile actor to the (last) cell centre.
        let a = &w.actors[d.slot as usize];
        let cells_with_value = w
            .board
            .draw_list
            .iter()
            .filter(|e| e.slot == d.slot)
            .count();
        if cells_with_value == 1 {
            assert_eq!(a.move_state.world_x as i32, d.world_x);
            assert_eq!(a.move_state.world_z as i32, d.world_z);
        }
    }
    // Procedural fill is all-drawable, so full mode draws every cell.
    assert_eq!(w.board.draw_list.len(), board.cells.len());
}

/// (c) Windowed mode restricts the draw set to the radius around the player.
#[test]
fn windowed_mode_restricts_draw_set_to_radius() {
    // 5x5 board, windowed (flag != 0), radius 1, player at the start cell
    // (4,0) -> a 2x2 window.
    let mut w = install_board(hdr(5, 5, 0, 0, 1, 1, 0x30));
    let _ = w.tick();
    assert!(!w.board.draw_list.is_empty());
    for d in &w.board.draw_list {
        assert!(
            d.col >= 3 && d.row <= 1,
            "cell ({},{}) outside the radius-1 window",
            d.col,
            d.row
        );
    }
    // The far corner is drawable on the board but excluded by the window.
    assert!(
        w.board
            .draw_list
            .iter()
            .all(|d| !(d.col == 0 && d.row == 4))
    );
    assert!(w.board.draw_list.len() <= 4);
}

/// (d) Exiting the board (landing on an event cell) despawns the tile actors
/// and clears the table + draw list; the player actor survives.
#[test]
fn board_exit_despawns_tile_actors() {
    let mut w = install_board(hdr(9, 3, 0, 0, 8, 0, 0x30));
    let slots: Vec<u8> = (2u8..=14)
        .filter_map(|v| w.board.actor_slots[v as usize])
        .collect();
    assert!(!slots.is_empty());
    // Put an event cell one row up the board from the player so an Up step
    // exits.
    {
        let idx = above_start(&w);
        w.board.grid.as_mut().unwrap().cells[idx] = crate::tile_board::CELL_EVENT_FIRST;
    }
    pad_held(&mut w, input::PadButton::Up.mask(), 20);
    assert!(
        w.board.grid.is_some(),
        "the exit fades before it tears down"
    );
    tick_until_torn_down(&mut w);
    assert!(w.board.grid.is_none(), "event cell exits the board");
    assert!(w.board.actor_slots.iter().all(|s| s.is_none()));
    assert!(w.board.draw_list.is_empty());
    for slot in slots {
        assert!(
            !w.actors[slot as usize].active,
            "tile actor {slot} despawned"
        );
    }
    assert!(w.actors[0].active, "player actor survives the board exit");
}

#[test]
fn tile_board_is_deterministic() {
    let drive = || {
        let mut w = tile_board_world();
        for &mask in &[
            input::PadButton::Right.mask(),
            input::PadButton::Up.mask(),
            input::PadButton::Right.mask(),
        ] {
            pad_held(&mut w, mask, 12);
        }
        let b = w.board.grid.as_ref().unwrap().clone();
        (
            b.player_col,
            b.player_row,
            w.actors[0].move_state.world_x,
            w.actors[0].move_state.world_z,
        )
    };
    assert_eq!(drive(), drive());
}

/// Walk-SM state 8: landing on event cell `8 + v` sets system flag
/// `A + v + 1`, and also `A` when flag `B + v` is clear (`0x801EFC38..`).
#[test]
fn event_cell_arrival_writes_the_state_8_flags() {
    let mut h = hdr(9, 3, 0, 0, 8, 0, 0x30);
    h.flag_base_set = 0x200;
    h.flag_base_test = 0x300;
    let run = |pre_set_test: bool| {
        let mut w = install_board(h);
        {
            let idx = above_start(&w);
            w.board.grid.as_mut().unwrap().cells[idx] = crate::tile_board::CELL_EVENT_FIRST + 2;
        }
        if pre_set_test {
            w.system_flag_set(0x300 + 2);
        }
        pad_held(&mut w, input::PadButton::Up.mask(), 20);
        tick_until_torn_down(&mut w);
        assert!(w.board.grid.is_none(), "event cell exits the board");
        (w.system_flag_test(0x200 + 2 + 1), w.system_flag_test(0x200))
    };
    // TEST base + v clear -> both flags set.
    assert_eq!(run(false), (true, true));
    // TEST base + v already set -> only the per-cell flag.
    assert_eq!(run(true), (true, false));
}

/// Walk-SM state 3: a trigger cell (`7`) leaves the board with no flag write.
#[test]
fn trigger_cell_arrival_exits_without_flags() {
    let mut h = hdr(9, 3, 0, 0, 8, 0, 0x30);
    h.flag_base_set = 0x200;
    let mut w = install_board(h);
    {
        let idx = above_start(&w);
        w.board.grid.as_mut().unwrap().cells[idx] = crate::tile_board::CELL_TRIGGER;
    }
    pad_held(&mut w, input::PadButton::Up.mask(), 20);
    tick_until_torn_down(&mut w);
    assert!(w.board.grid.is_none(), "trigger cell exits the board");
    assert!(!w.system_flag_test(0x200));
}

/// Walk-SM state 3's animated pass advances every animated cell on the
/// board, not only the arrived one.
#[test]
fn plain_arrival_advances_every_animated_cell() {
    let mut w = tile_board_world();
    {
        let b = w.board.grid.as_mut().unwrap();
        b.cells[8] = crate::tile_board::CELL_ANIM_LAST; // (2,2), not visited
        b.cells[6] = crate::tile_board::CELL_ANIM_FIRST; // (0,2), not visited
    }
    // One Right step to (1,0): an ordinary cell.
    w.set_pad(input::PadButton::Right.mask());
    for _ in 0..20 {
        let _ = w.tick();
        w.set_pad(0);
    }
    let b = w.board.grid.as_ref().unwrap();
    assert_eq!(b.player_col, 1);
    assert_eq!(
        b.cells[8],
        crate::tile_board::CELL_ANIM_FIRST,
        "0xE wraps to 0xB"
    );
    assert_eq!(b.cells[6], crate::tile_board::CELL_ANIM_FIRST + 1);
}

/// The walk SM's fade (states 1 and `0xA`/`0xC`): the tiles grow in at
/// install and shrink away at the exit, the event tile under the player
/// excepted; teardown follows the park.
#[test]
fn the_board_fades_in_and_out_around_its_exit() {
    use crate::tile_board::sm;
    let mut w = World::new();
    w.mode = SceneMode::Field;
    w.player_actor_slot = Some(0);
    w.actors[0].active = true;
    let instr = [0x49, 0x05, 0, 0, 9, 3, 8, 0, 0, 0, 0, 0, 0, 0x30];
    assert!(w.try_install_tile_board(&instr));
    plain_start_cell(&mut w);
    assert_eq!((w.board.sm, w.board.fade), (sm::FADE_IN, 0));
    // Input is ignored while fading in.
    pad_held(&mut w, input::PadButton::Right.mask(), 1);
    assert_eq!(
        w.board.grid.as_ref().unwrap().player_col,
        crate::tile_board::START_COL
    );
    assert!(w.board.fade > 0 && w.board.fade < crate::tile_board::FADE_FULL);
    let v = w.board.draw_list.first().map(|d| d.cell_value).unwrap();
    let s = w.tile_board_cell_scale(v);
    assert!(s > 0.0 && s < 1.0, "a tile mid-fade draws small: {s}");
    let mut n = 1;
    while w.board.sm == sm::FADE_IN {
        pad_held(&mut w, 0, 1);
        n += 1;
    }
    // `+0x9C += 96` a vsync to `0x1000`.
    assert_eq!(n, (0x1000 + 95) / 96);
    assert_eq!(w.tile_board_cell_scale(v), 1.0);
    finish_walk_in(&mut w);
    // Event cell one row up the board: step onto it.
    {
        let idx = above_start(&w);
        w.board.grid.as_mut().unwrap().cells[idx] = crate::tile_board::CELL_EVENT_FIRST;
    }
    let mut seen = Vec::new();
    w.set_pad(input::PadButton::Up.mask());
    for _ in 0..200 {
        let _ = w.tick();
        w.set_pad(0);
        if w.board.grid.is_none() {
            break;
        }
        seen.push(w.board.sm);
        if w.board.sm == sm::EVENT_FADE_OUT {
            // The event tile the player stands on keeps its size.
            assert_eq!(
                w.tile_board_cell_scale(crate::tile_board::CELL_EVENT_FIRST),
                1.0
            );
        }
    }
    assert!(w.board.grid.is_none(), "torn down");
    for st in [sm::EVENT_STEP, sm::EVENT_FADE_OUT, sm::PARK, sm::TEARDOWN] {
        assert!(seen.contains(&st), "passed state {st:#x}: {seen:?}");
    }
    // `+0x9C -= 256` a vsync from `0x1000`: sixteen fade frames and one more
    // to go below zero.
    let fade_frames = seen.iter().filter(|&&s| s == sm::EVENT_FADE_OUT).count();
    assert_eq!(fade_frames, 17);
}

/// Triangle opens the quit prompt on its second row; Up + confirm quits
/// through the exit fade; cancel returns to walking.
#[test]
fn the_quit_prompt_opens_on_triangle_and_quits_on_row_0() {
    use crate::tile_board::sm;
    let mut w = install_board(hdr(9, 3, 0, 0, 8, 0, 0x30));
    let press = |w: &mut World, b: input::PadButton| {
        w.set_pad(b.mask());
        let _ = w.tick();
        w.set_pad(0);
        let _ = w.tick();
    };
    press(&mut w, input::PadButton::Triangle);
    assert_eq!(w.board.sm, sm::PROMPT);
    assert_eq!(w.tile_board_prompt_cursor(), Some(1));
    // Cancel: back to walking.
    press(&mut w, input::PadButton::Circle);
    assert_eq!(w.board.sm, sm::WALK);
    assert_eq!(w.tile_board_prompt_cursor(), None);
    // Confirm on row 1 goes back too.
    press(&mut w, input::PadButton::Triangle);
    press(&mut w, input::PadButton::Cross);
    assert_eq!(w.board.sm, sm::WALK);
    // Up to row 0 and confirm: the board leaves.
    press(&mut w, input::PadButton::Triangle);
    press(&mut w, input::PadButton::Up);
    assert_eq!(w.tile_board_prompt_cursor(), Some(0));
    press(&mut w, input::PadButton::Cross);
    assert_ne!(w.board.sm, sm::PROMPT);
    tick_until_torn_down(&mut w);
    assert!(w.board.grid.is_none());
    assert!(w.board.armed, "the op-49 script reads Done next");
}

/// State 0 seats the player's cell at column 4, row 0 and state 2 walks the
/// actor there from wherever it stood; the arrival pass then runs on the
/// start cell (`0x801EF630..0x801EF67C`, `0x801EFA88..0x801EFAF8`).
#[test]
fn the_player_walks_in_to_column_4_row_0_after_the_fade_in() {
    use crate::tile_board::sm;
    let mut w = World::new();
    w.mode = SceneMode::Field;
    w.player_actor_slot = Some(0);
    w.actors[0].active = true;
    w.actors[0].move_state.world_x = 0;
    w.actors[0].move_state.world_z = 0;
    // origin (2, 1): the walk-in target is (hdr[1]*128 + 0x240, hdr[2]*128 + 0x40).
    let instr = [0x49, 0x05, 2, 1, 9, 5, 8, 0, 0, 0, 0, 0, 0, 0x30];
    assert!(w.try_install_tile_board(&instr));
    plain_start_cell(&mut w);
    let b = w.board.grid.as_ref().unwrap();
    assert_eq!((b.player_col, b.player_row), (4, 0));
    let target = (2 * 128 + 0x240, 128 + 0x40);
    assert_eq!(w.board.target, Some(target));
    // Not seated: the actor has not moved during the fade-in.
    while w.board.sm == sm::FADE_IN {
        let _ = w.tick();
        assert_eq!(
            (
                w.actors[0].move_state.world_x,
                w.actors[0].move_state.world_z
            ),
            (0, 0)
        );
    }
    finish_walk_in(&mut w);
    assert_eq!(
        (
            w.actors[0].move_state.world_x as i32,
            w.actors[0].move_state.world_z as i32
        ),
        target
    );
    assert_eq!(w.board.sm, sm::WALK);
}

/// A trigger cell under the start cell exits through the arrival pass the
/// walk-in ends in - retail's fill does not protect the start cell.
#[test]
fn the_walk_in_arrival_runs_on_the_start_cell() {
    let mut w = World::new();
    w.mode = SceneMode::Field;
    w.player_actor_slot = Some(0);
    w.actors[0].active = true;
    let instr = [0x49, 0x05, 0, 0, 9, 3, 8, 0, 0, 0, 0, 0, 0, 0x30];
    assert!(w.try_install_tile_board(&instr));
    {
        let b = w.board.grid.as_mut().unwrap();
        b.cells[crate::tile_board::START_COL as usize] = crate::tile_board::CELL_TRIGGER;
    }
    tick_until_torn_down(&mut w);
    assert!(w.board.grid.is_none());
}

/// The walker stores an octant off the cell under the player and remaps the
/// held pad through it; the board saves the incoming octant on install and
/// restores it at teardown (`0x801EF320` / `0x801EFE7C`).
#[test]
fn the_board_rotates_the_pad_by_the_cell_octant_and_restores_it() {
    let mut w = install_board(hdr(9, 5, 0, 0, 8, 0, 0x30));
    // (install_board ran on a world whose octant was 0; re-run the save with
    // a non-zero incoming value to see it come back.)
    w.board.saved_octant = 3;
    // Terrain 5 under the player: octant (5 - 3) * 2 = 4, a half turn, so
    // holding Up (`0x1000`, row + 1) walks row - 1 instead - off the board
    // from row 0 - and Down walks row + 1.
    let idx = above_start(&w);
    {
        let b = w.board.grid.as_mut().unwrap();
        b.cells[crate::tile_board::START_COL as usize] = 5;
        b.cells[idx] = 3;
    }
    pad_held(&mut w, input::PadButton::Up.mask(), 1);
    assert_eq!(w.locomotion.pad_octant, 4);
    assert_eq!(
        w.board.grid.as_ref().unwrap().player_row,
        0,
        "Up is rotated off the board"
    );
    pad_held(&mut w, input::PadButton::Down.mask(), 1);
    assert_eq!(
        w.board.grid.as_ref().unwrap().player_row,
        1,
        "Down is rotated onto row 1"
    );
    // Quit through the prompt and check the octant comes back.
    let press = |w: &mut World, b: input::PadButton| {
        w.set_pad(b.mask());
        let _ = w.tick();
        w.set_pad(0);
        let _ = w.tick();
    };
    finish_walk_in(&mut w);
    press(&mut w, input::PadButton::Triangle);
    press(&mut w, input::PadButton::Up);
    press(&mut w, input::PadButton::Cross);
    tick_until_torn_down(&mut w);
    assert_eq!(
        w.locomotion.pad_octant, 3,
        "the incoming octant is restored"
    );
}
