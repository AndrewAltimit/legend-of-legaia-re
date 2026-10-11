//! The pre-hook tick's constants and its debug tile readout.

// --- Pre-hook tick (FUN_801D2050) ------------------------------------------

/// Camera globals the pre-hook tick's one-shot init seeds
/// (`_DAT_80084044`, `_DAT_80084046`).
pub const CAMERA_INIT: (i16, i16) = (-0x7FFF, 100);

/// Species id the fish-sprite spawn special-cases with a larger scale and
/// the extra draw flags.
pub const SPECIAL_SPECIES: u32 = 8;

/// Scale written into both scale fields for [`SPECIAL_SPECIES`].
pub const SPECIAL_SPECIES_SCALE: i16 = 0x88;

/// World-units-per-tile shift the debug readout applies to X and Z.
pub const DEBUG_TILE_SHIFT: u32 = 7;

/// Held-pad bit that, together with the global print flag, enables the
/// overlay's debug readouts (`_DAT_8007B850 & 2`).
pub const PACK_DEBUG_MODIFIER: u16 = 0x0002;

/// Convert a world coordinate to the tile index the debug readout prints.
///
/// Retail biases a negative value by `+0x7F` before the arithmetic shift, so
/// the division truncates toward zero rather than toward negative infinity.
///
/// PORT: FUN_801d2050 (debug readout)
// Wired: the play window's fishing HUD prints the wander actor's tile pair
// through this when the developer readout is up (`window/hud.rs`, gated by
// `debug_readout_visible` off the dev-menu print flag).
#[inline]
pub fn debug_tile(v: i16) -> i32 {
    let v = v as i32;
    let biased = if v < 0 { v + 0x7F } else { v };
    biased >> DEBUG_TILE_SHIFT
}

/// Whether the overlay's debug readouts are showing this frame.
///
/// Both the global print flag `_DAT_8007B9B0` and the held modifier bit have
/// to be set; the same gate switches the bite interval to its debug value
/// (see [`bite_interval`]).
///
/// PORT: FUN_801d2050 (readout gate)
// Wired: the play window's fishing HUD computes this from the dev-menu
// session's presence (the engine's `_DAT_8007B9B0` stand-in) and the held pad
// modifier, and shows the `debug_tile` readout when it holds
// (`window/hud.rs`).
#[inline]
pub fn debug_readout_visible(print_flag: bool, pad_held: u16) -> bool {
    print_flag && pad_held & PACK_DEBUG_MODIFIER != 0
}
