//! The walk-grid overhead probe.

// --- Walk-grid overhead probe (FUN_801D7030) -------------------------------

/// Bytes per row of the `+0x4000` sub-cell grid (`(x_cell / 2) & 0x7F`
/// column, `(z_cell / 2) & 0x7F` row, `row * 0x80 + column`).
pub const WALK_GRID_PITCH: usize = 0x80;

/// Rows in the same grid.
pub const WALK_GRID_ROWS: usize = 0x80;

/// Probe the per-scene walkability grid's **high** nibble.
///
/// `grid` is the byte block at `*(_DAT_1F8003EC) + 0x4000` - the same block
/// the field overlay's per-axis collision reads (`FUN_801CFE4C`, see
/// `docs/subsystems/field-locomotion.md`), which takes the byte's *low*
/// nibble. This probe takes the high one, so the two read the two 4-bit
/// masks packed into each grid byte independently.
///
/// The two coordinate conversions are **not** the same ladder, which is the
/// thing to keep when re-deriving this:
///
/// - `z` truncates toward zero (`z < 0` is biased `+0x3F` before the
///   arithmetic shift) and is then biased **`+2` sub-cells**.
/// - `x` rounds up (`(x + 0x3F) >> 6`, no sign test - the bias is
///   unconditional) and is then biased **`-1` sub-cell**.
///
/// The byte is `row * 0x80 + column` with `column` from **x** and `row` from
/// **z**; the sub-cell bit is `1 << ((x_cell & 1) + 2 * (z_cell & 1))`.
///
/// PORT: FUN_801d7030
// Wired through [`LureActor::probe`], which both fishing hosts reach: the
// browser minigames page drives it inside the venue-faithful
// [`crate::fishing::PondSession`], and the play window's
// `window/minigames.rs::tick_fishing_actors` drifts its own lure actor with
// it. The probe's consequence is the one retail applies at `0x801D2E18` - a
// `frame_delta << 11` push of the lure's `x` accumulator, signed by the low
// bit of the persistent cast counter `_DAT_80084460` - not a water test.
pub fn walk_grid_overhead(grid: &[u8], x: i32, z: i32) -> bool {
    let zc = (if z < 0 { z + 0x3F } else { z } >> 6) + 2;
    let xc = ((x + 0x3F) >> 6) - 1;
    // `srl 31; addu; sra 1` - divide by two truncating toward zero, which is
    // what Rust's `/` already does on a signed integer.
    let column = (xc / 2) & 0x7F;
    let row = (zc / 2) & 0x7F;
    let idx = row as usize * WALK_GRID_PITCH + column as usize;
    let Some(byte) = grid.get(idx) else {
        return false;
    };
    let bit = 1u8 << ((xc & 1) + 2 * (zc & 1)) as u32;
    (byte >> 4) & bit != 0
}
