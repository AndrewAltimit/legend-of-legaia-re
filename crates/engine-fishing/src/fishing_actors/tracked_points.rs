//! The tracked-point separation metric.

// --- Tracked-point separation (FUN_801D765C) -------------------------------

/// Runtime VA of the first tracked 2-D point (`+0` = x, `+4` = y).
pub const TRACKED_POINT_A_VA: u32 = 0x801D_9184;

/// Runtime VA of the second (`+0` = x, `+4` = y).
pub const TRACKED_POINT_B_VA: u32 = 0x801D_918C;

/// World units per sub-cell - the shift the separation is reported in.
pub const SUBCELL_SHIFT: u32 = 6;

/// Separation of the overlay's two tracked 2-D points, in sub-cells.
///
/// Retail takes no arguments: it reads `(i16 x, i16 y)` out of
/// [`TRACKED_POINT_A_VA`] / [`TRACKED_POINT_B_VA`] - the same pair
/// `FUN_801D26CC` feeds to the bearing helper `FUN_80019B28` - takes the
/// absolute difference of each component, squares and sums them, and hands
/// the sum to the SCUS normalise helper `FUN_8005AF0C` (`sqrt`).
///
/// The result is arithmetic-shifted right by [`SUBCELL_SHIFT`] and a negative
/// result is clamped to zero. `>> 6` is the **sub-cell** step (64 units), not
/// the 128-unit tile: the same shift `FUN_801D7030` uses to index the grid.
///
/// PORT: FUN_801d765c
// Wired: the play window's fishing developer readout computes the wander
// actor's separation from the venue anchor through this (with an integer
// square root standing in for the SCUS normalise helper `FUN_8005AF0C`,
// which the port takes as a closure rather than owning) - see
// `window/hud.rs`.
pub fn tracked_point_separation(
    a: (i16, i16),
    b: (i16, i16),
    sqrt: impl FnOnce(i32) -> i32,
) -> i32 {
    let dx = (a.0 as i32 - b.0 as i32).abs();
    let dy = (a.1 as i32 - b.1 as i32).abs();
    (sqrt(dx * dx + dy * dy) >> SUBCELL_SHIFT).max(0)
}
