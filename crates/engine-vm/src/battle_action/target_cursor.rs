//! Target-select cursor highlight over the monster actor slots.
//!
//! PORT: FUN_801da6b4
//!
//! While the player is choosing which monster an Attack / Art / spell should
//! hit, the battle overlay tints the pointed-at monster bright and dims the
//! rest. `FUN_801da6b4` is the one-shot that stamps that render state across
//! the four monster slots the cursor can land on; it is called with the
//! highlight either **on** (retail `param_1 == 0`) or **off** (any non-zero
//! `param_1`, which clears the tint back to neutral).
//!
//! Retail (`ghidra/scripts/funcs/overlay_battle_action_801da6b4.txt`) walks a
//! **fixed** slot window - actor-pointer-table entries `3..=6` (`&DAT_801C937C`
//! = `&DAT_801C9370 + 3` up to index `6`) - regardless of how many monsters
//! are actually seated, and touches only the *alive* ones (`+0x14C != 0`). The
//! "which slot is pointed at" test compares the loop's slot index against the
//! **acting actor's** current target byte (`+0x1DD`), resolved through
//! `(&DAT_801C9370)[ctx[+0x13]]`. Three render words move per slot:
//!
//! | field | pointed-at (on) | other (on) | off |
//! |---|---|---|---|
//! | `+0x21C` render flag | `5` | `200` | `0` |
//! | `+0x4` colour word | `0x20080200` | `0x00401004` | `0x20080200` |
//! | `+0xC` scale word | `0x1000` | `0x1000` | `0` |
//!
//! Note the off path and the pointed-at path share the same `+0x4` write
//! (`0x20080200`) - retail falls through to it - while the dimmed path takes a
//! different colour and skips that shared store. This module reproduces that
//! exactly.

use super::*;

/// First monster slot the target cursor scans (retail `&DAT_801C937C`).
pub const TARGET_CURSOR_FIRST_SLOT: u8 = 3;
/// Last monster slot the target cursor scans (inclusive; retail loop bound
/// `uVar4 <= 6`). The window is fixed at four slots independent of party size.
pub const TARGET_CURSOR_LAST_SLOT: u8 = 6;

/// Render-flag brightness stamped on the pointed-at monster.
pub const CURSOR_FLAG_SELECTED: u8 = 5;
/// Render-flag brightness stamped on the non-pointed-at monsters.
pub const CURSOR_FLAG_DIMMED: u8 = 200;
/// Colour word for a bright (pointed-at or cleared) actor.
pub const CURSOR_COLOR_BRIGHT: u32 = 0x2008_0200;
/// Colour word for a dimmed actor.
pub const CURSOR_COLOR_DIM: u32 = 0x0040_1004;
/// Neutral q12 scale word applied while the cursor is up.
pub const CURSOR_SCALE_ON: u32 = 0x1000;

/// Stamp (or clear) the target-select cursor tint across the monster slots.
///
/// `enable` is retail's highlight-on case (`param_1 == 0`); `false` clears the
/// tint on every live monster slot. Dead slots (`liveness == 0`) are skipped,
/// matching retail's `+0x14C != 0` gate.
pub fn target_cursor_highlight<H: BattleActionHost + ?Sized>(
    host: &mut H,
    ctx: &BattleActionCtx,
    enable: bool,
) {
    // The pointed-at slot is the acting actor's current target (`+0x1DD`).
    let active_target = host
        .actor(ctx.active_actor)
        .map(|a| a.active_target)
        .unwrap_or(0);

    for slot in TARGET_CURSOR_FIRST_SLOT..=TARGET_CURSOR_LAST_SLOT {
        // Skip missing / dead slots.
        if host.actor(slot).is_none_or(|a| a.liveness == 0) {
            continue;
        }
        let selected = slot == active_target;
        let Some(actor) = host.actor_mut(slot) else {
            continue;
        };
        if !enable {
            actor.render_flag = 0;
            actor.render_scale = 0;
            actor.render_color = CURSOR_COLOR_BRIGHT;
        } else if selected {
            actor.render_scale = CURSOR_SCALE_ON;
            actor.render_flag = CURSOR_FLAG_SELECTED;
            actor.render_color = CURSOR_COLOR_BRIGHT;
        } else {
            actor.render_scale = CURSOR_SCALE_ON;
            actor.render_flag = CURSOR_FLAG_DIMMED;
            actor.render_color = CURSOR_COLOR_DIM;
        }
    }
}
