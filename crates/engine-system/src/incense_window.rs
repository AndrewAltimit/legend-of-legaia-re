//! The Incense window top-up: the item applier's selector-`0x82` arm.
//!
//! Port provenance (disassembly, not the decompiled C):
//! `see ghidra/scripts/funcs/80046870.txt`.
//!
//! `FUN_80046870` ([`top_up_cooldown`]) is the whole of the item applier's
//! selector-`0x82` arm (`0x800421A0`, the jump table's slot `0x82`), and
//! `0x82` is the effect class of **Incense** (item `0x8A`, "Decrease
//! encounter rate for a period of time"). The word it ramps, `gp + 0x2E8` =
//! `_DAT_8007B600` (`gp` is `0x8007B318`: `80026ca8` `lui gp,0x8008` +
//! `80026cac` `addiu gp,gp,-0x4ce8`), is the **Incense window**, counted in
//! walk-regen ticks rather than frames:
//!
//! - the field walk tick `FUN_801D0B90` (PROT 0897) decrements it once per
//!   running tick (`0x801D0CD4..0x801D0CE8`) and, on the transition to zero,
//!   installs `0x801F2278` as the field event pointer `_DAT_8007B450`, sets
//!   `0x80000` in the player actor's flag word and registers it with
//!   `FUN_80020DE0` - the "the Incense wore off" hand-off;
//! - the region encounter roll `FUN_801D9E1C` skips the whole roll while it
//!   is non-zero (`lw v0,-0x4a00(v0)` / `bne v0,zero` at `0x801DA174`), so
//!   an Incense suppresses encounters outright for its window rather than
//!   scaling the rate;
//! - the validator's arm-`0x82` gate `FUN_80046898` admits another use only
//!   while fewer than `0xE0` ticks remain.
//!
//! Every host of that is field or pause-menu code, and the engine runs all
//! three: the counter is `engine-core`'s `FieldLocomotion::walk_regen_window`
//! (decremented by `walk_regen::tick_walk_regen`), the pause Items screen
//! counts each Incense confirm and `field_menu_dispatch::apply_pause_items_outcome`
//! tops the window up through this kernel once per confirm, the same
//! screen greys the Incense row through `item_count_gate` at build, and
//! `World::on_field_step` skips the region roll while the window is
//! non-zero.

/// Advance a per-frame charge gauge by one step and clamp at the ceiling.
///
// PORT: FUN_80046870
///
/// The retail word `gp+0x2e8` (`_DAT_8007B600`) is the **Incense window**,
/// not a charge gauge: each call tops it up by `+0x40` walk-regen ticks and
/// saturates it at `0x100`, the field walk tick counts it down by one per
/// running tick and the region encounter roll gates on zero (see the module
/// notes). Faithful to the original `slti v0,v0,0x100` clamp: the sum is
/// clamped only when it reaches or exceeds `0x100`.
pub const COOLDOWN_STEP: i32 = 0x40;
/// Ceiling the cooldown saturates at.
pub const COOLDOWN_MAX: i32 = 0x100;

/// See [`COOLDOWN_STEP`] / [`COOLDOWN_MAX`].
///
// PORT: FUN_80046870
//
// WIRED: `legaia_engine_menu_runtime::field_menu_dispatch::apply_pause_items_outcome`
// tops up `FieldLocomotion::walk_regen_window` once per Incense confirm the
// pause Items screen committed; both play hosts apply the finished screen
// through it.
pub fn top_up_cooldown(value: i32) -> i32 {
    let next = value + COOLDOWN_STEP;
    if next < COOLDOWN_MAX {
        next
    } else {
        COOLDOWN_MAX
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gauge_accumulates_by_step() {
        assert_eq!(top_up_cooldown(0), 0x40);
        assert_eq!(top_up_cooldown(0x40), 0x80);
        assert_eq!(top_up_cooldown(0xC0), COOLDOWN_MAX);
    }

    #[test]
    fn gauge_saturates_and_never_exceeds_max() {
        // 0xC0 + 0x40 = 0x100 -> clamp (slti is strict <).
        assert_eq!(top_up_cooldown(0xC0), 0x100);
        assert_eq!(top_up_cooldown(0x100), 0x100);
        assert_eq!(top_up_cooldown(0x1000), 0x100);
    }
}
