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
