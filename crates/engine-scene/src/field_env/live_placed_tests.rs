use super::*;

/// Turn about the draw's own origin, then move: the translation picks up
/// the displacement, the linear part the turn, and an unbound draw keeps
/// its model.
#[test]
fn turn_then_move_about_the_origin() {
    let mut model = [0.0f32; 16];
    model[0] = 1.0;
    model[5] = 1.0;
    model[10] = 1.0;
    model[15] = 1.0;
    model[12] = 100.0;
    model[14] = 200.0;
    // A quarter turn about Y.
    let mut turn = [0.0f32; 16];
    turn[2] = -1.0;
    turn[5] = 1.0;
    turn[8] = 1.0;
    turn[15] = 1.0;
    let turns = HashMap::from([(7usize, turn)]);
    let moves = HashMap::from([(7usize, [5, -6, 7])]);
    let out = live_placed_model(&model, Some(7), &turns, &moves);
    assert_eq!(&out[12..15], &[105.0, -6.0, 207.0]);
    assert_eq!((out[0], out[2], out[8]), (0.0, -1.0, 1.0));
    assert_eq!(live_placed_model(&model, None, &turns, &moves), model);
    assert_eq!(live_placed_model(&model, Some(8), &turns, &moves), model);
}

/// Only a record's first placement swaps, and only to a slot below the
/// stream's `0xF0` sentinel range.
#[test]
fn model_swaps_take_the_first_placement_below_the_sentinel() {
    let records = [Some(3), None, Some(3), Some(4), Some(5)];
    let models = std::collections::BTreeMap::from([(3usize, 9i16), (4, 0xF0), (5, -1)]);
    assert_eq!(
        placed_model_swaps(&records, &models),
        vec![Some(9), None, None, None, None]
    );
}
