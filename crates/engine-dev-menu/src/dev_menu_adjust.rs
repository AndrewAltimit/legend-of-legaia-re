//! World-map debug-menu value-adjust kernels, ported from the
//! per-row input state machine `FUN_801E9F64`.
//!
//! The address tag sits on each implementing function below rather than on
//! this module block, because a module-level tag claims every symbol in the
//! file at once.
//!
//! `FUN_801E9F64` (`overlay_world_map_walk_801e9f64.txt`, base `0x801C0000`) is
//! the input half of the world-map developer menu whose list is drawn by
//! `FUN_801EAD98` / `FUN_801E6400`. It is a 20-case dispatcher keyed on
//! `actor[+0x9e] - 3` (jump table `0x801CF294`); each case edits one menu row's
//! backing value from the frame's newly-pressed pad mask
//! (`_DAT_8007BB84`), the **Right** bit `0x2000` stepping the value up and the
//! **Left** bit `0x8000` stepping it down.
//!
//! The rows differ only in how the stepped value is bounded. Two of the arms
//! are pure integer kernels with no VRAM / actor state, ported here:
//!
//! - The camera / encounter counter at `_DAT_8007B6D0` wraps in a **12-bit**
//!   ring (`v = (v +/- 1) & 0xFFF`); disasm at `0x801E9FE4..0x801EA030`.
//! - The rate field at `_DAT_801F2E8C` clamps to the inclusive range
//!   `[1, 255]` after each step (`if v <= 0 { v = 1 }; if v >= 0x100 { v = 255 }`);
//!   disasm at `0x801EA034..0x801EA0AC`.
//!
//! The remaining arms (the `_DAT_801F2E90` BGM-track cycle that scans a
//! sentinel-terminated table at `0x801F2E94`, and the per-character parameter
//! editors that poke the live party records) touch disc data / actor state and
//! stay documented-not-ported; see [`docs/subsystems/world-map.md`].
//!
//! ## Wiring
//!
//! `legaia_engine_menus::dev_menu_host::DevMenuSession` is the host screen:
//! its `MAP CHANGE` row steps [`wrap12_step`] and its `ENCOUNT` row
//! [`clamp1_255_step`], both off the frame's newly-pressed pad word. The
//! sibling [`crate::dev_menu_leaves`] still carries the menu's retail row
//! model, panel sizer and list-picker, which that screen does not use.

/// Newly-pressed pad bit that steps a debug-menu value **up**.
pub const PAD_STEP_UP: u32 = 0x2000;
/// Newly-pressed pad bit that steps a debug-menu value **down**.
pub const PAD_STEP_DOWN: u32 = 0x8000;

/// Resolve a frame's pad mask into a step: `+1`, `-1`, or `0`.
///
/// Mirrors the branch order of `FUN_801E9F64`'s clamp arm
/// (`0x801EA040..0x801EA070`): the up bit is tested first and wins when both
/// bits are set in the same frame. The 12-bit ring arm tests the two bits
/// independently ([`wrap12_step`]).
///
// PORT: FUN_801e9f64 (pad-edge decode shared by every row arm)
#[inline]
pub fn pad_step(pad_pressed: u32) -> i32 {
    if pad_pressed & PAD_STEP_UP != 0 {
        1
    } else if pad_pressed & PAD_STEP_DOWN != 0 {
        -1
    } else {
        0
    }
}

/// Step a value that wraps in a 12-bit ring (`_DAT_8007B6D0`).
///
/// The retail arm is two independent tests, not one decoded step: the up bit
/// adds one and masks to 12 bits, then the down bit takes one off and masks
/// again. Both bits in one frame therefore cancel (unlike the clamp arm,
/// where up wins), and a frame with neither leaves the word as it lies,
/// unmasked.
///
// PORT: FUN_801e9f64 (`0x801E9FE4..0x801EA030`, the 12-bit ring arm)
#[inline]
pub fn wrap12_step(value: u16, pad_pressed: u32) -> u16 {
    let mut v = value;
    if pad_pressed & PAD_STEP_UP != 0 {
        v = v.wrapping_add(1) & 0x0FFF;
    }
    if pad_pressed & PAD_STEP_DOWN != 0 {
        v = v.wrapping_sub(1) & 0x0FFF;
    }
    v
}

/// Step a value clamped to the inclusive range `[1, 255]` (`_DAT_801F2E8C`).
///
/// The retail arm clamps after stepping: any value `<= 0` snaps to `1`, and any
/// value `>= 0x100` snaps to `255`. The clamp runs every tick the row is active,
/// so a value seeded out of range is pulled in on the first frame even without a
/// pad press.
///
// PORT: FUN_801e9f64 (`0x801EA034..0x801EA0AC`, the `[1, 255]` clamp arm)
#[inline]
pub fn clamp1_255_step(value: i32, pad_pressed: u32) -> i32 {
    let mut v = value + pad_step(pad_pressed);
    if v <= 0 {
        v = 1;
    }
    if v >= 0x100 {
        v = 255;
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pad_step_priority_and_neutral() {
        assert_eq!(pad_step(0), 0);
        assert_eq!(pad_step(PAD_STEP_UP), 1);
        assert_eq!(pad_step(PAD_STEP_DOWN), -1);
        // Both bits set in one frame: up wins (branch order in FUN_801E9F64).
        assert_eq!(pad_step(PAD_STEP_UP | PAD_STEP_DOWN), 1);
        // Unrelated bits are ignored.
        assert_eq!(pad_step(0x0040 | 0x0800), 0);
    }

    #[test]
    fn wrap12_stays_in_ring() {
        assert_eq!(wrap12_step(0x100, PAD_STEP_UP), 0x101);
        assert_eq!(wrap12_step(0x100, PAD_STEP_DOWN), 0x0FF);
        // Wrap at the top: 0xFFF + 1 -> 0.
        assert_eq!(wrap12_step(0x0FFF, PAD_STEP_UP), 0x000);
        // Wrap at the bottom: 0 - 1 -> 0xFFF.
        assert_eq!(wrap12_step(0x000, PAD_STEP_DOWN), 0x0FFF);
        // A neutral frame leaves the value as it lies, high bits included:
        // the mask rides the step.
        assert_eq!(wrap12_step(0x123, 0), 0x123);
        assert_eq!(wrap12_step(0x1234, 0), 0x1234);
        assert_eq!(wrap12_step(0x1234, PAD_STEP_UP), 0x235);
        // Both bits in one frame cancel.
        assert_eq!(wrap12_step(0x100, PAD_STEP_UP | PAD_STEP_DOWN), 0x100);
    }

    #[test]
    fn clamp_holds_range() {
        assert_eq!(clamp1_255_step(0x80, PAD_STEP_UP), 0x81);
        assert_eq!(clamp1_255_step(0x80, PAD_STEP_DOWN), 0x7F);
        // Lower edge: 1 stepped down clamps back to 1.
        assert_eq!(clamp1_255_step(1, PAD_STEP_DOWN), 1);
        // Upper edge: 255 stepped up clamps back to 255.
        assert_eq!(clamp1_255_step(255, PAD_STEP_UP), 255);
        // Out-of-range seed is pulled in even with no pad press.
        assert_eq!(clamp1_255_step(0, 0), 1);
        assert_eq!(clamp1_255_step(500, 0), 255);
    }
}
