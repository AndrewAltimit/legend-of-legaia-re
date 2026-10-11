//! The menu cursor steppers.

/// PORT: FUN_801cf388 (`0x801CFA64`..`0x801CFAF8`) - the **player-select**
/// cursor.
///
/// Left steps down, right steps up, and the fold is **two** independent clamps
/// in retail's order: `>= span` goes to `0` first, then `< 0` goes to
/// `span - 1`. So this cursor wraps both ways. Returns the new cursor plus
/// whether a cursor blip fires.
///
/// The two two-option menus (the tally choice `0x68` and the pause menu `0xBE`)
/// fold with `& 1` instead, which is the same thing for a span of `2` -
/// `-1 & 1 == 1` and `2 & 1 == 0` - so they use this too.
pub fn cursor_step(cursor: i32, span: i32, pad_edge: u16, left: u16, right: u16) -> (i32, bool) {
    let (mut c, moved) = cursor_raw(cursor, pad_edge, left, right);
    if c >= span {
        c = 0;
    }
    if c < 0 {
        c = span - 1;
    }
    (c, moved)
}

/// PORT: FUN_801cf388 (`0x801D14B4`..`0x801D1508`, `0x801D1748`..`0x801D17B0`) -
/// the **modulo** cursor fold, used by the 3-option pause menu (`0xBF`) and the
/// 5-row developer menu (`0xC8`).
///
/// These two do not wrap the way the player-select cursor does. Retail folds
/// them with an **unsigned** `% span` (the `0xAAAAAAAB` / `0xCCCCCCCD`
/// reciprocal multiplies), and `-1` as a `u32` is `0xFFFFFFFF`, which both `3`
/// and `5` divide exactly - so stepping *up* off the top row lands on row `0`
/// again rather than on the bottom row. The cursor is sticky at the top and
/// wraps only at the bottom.
pub fn cursor_step_mod(
    cursor: i32,
    span: i32,
    pad_edge: u16,
    left: u16,
    right: u16,
) -> (i32, bool) {
    let (c, moved) = cursor_raw(cursor, pad_edge, left, right);
    if span <= 0 {
        return (c, moved);
    }
    ((c as u32 % span as u32) as i32, moved)
}

/// The step both folds share: left decrements, right increments, and either
/// edge counts as a move (both in one frame cancel out but still blip).
fn cursor_raw(cursor: i32, pad_edge: u16, left: u16, right: u16) -> (i32, bool) {
    let mut c = cursor;
    let mut moved = false;
    if pad_edge & left != 0 {
        c -= 1;
        moved = true;
    }
    if pad_edge & right != 0 {
        c += 1;
        moved = true;
    }
    (c, moved)
}
