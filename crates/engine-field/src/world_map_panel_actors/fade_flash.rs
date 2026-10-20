//! The brightness fade / flash actor.

use super::*;

// ---------------------------------------------------------------------------
// FUN_801ED308 - screen brightness fade / flash
// ---------------------------------------------------------------------------

/// Brightness units added per frame-delta tick: `delta*4 + delta`, then `<< 1`.
///
/// PORT: FUN_801ed308 (`sll v1,v0,2; addu v1,v1,v0; sll v1,v1,1`)
pub const BRIGHTNESS_STEP: i32 = 10;

/// The value the ramp saturates at (`slti v0,v0,0xf3` then `li v0,0xf2`).
pub const BRIGHTNESS_MAX: i32 = 0xF2;

/// Bias the phase-1 hand-off test adds before comparing against
/// [`BRIGHTNESS_MAX`]`+1`: the tint is captured once `level + 0x70 >= 0xF3`.
pub const BRIGHTNESS_HANDOFF_BIAS: i32 = 0x70;

/// Value of the flash counter `_DAT_8007B43C` at or above which the hold
/// phase restores the tint instead of ramping (`slti v0,v0,6`).
pub const FLASH_COUNTER_RESTORE: i32 = 6;

/// The phase the actor parks on once the ramp has saturated: it re-pins the
/// level at [`BRIGHTNESS_MAX`] every frame and only leaves when the counter
/// reaches [`FLASH_COUNTER_RESTORE`].
pub const FADE_FLASH_HOLD_PHASE: i16 = 3;

/// What the menu overlay's save/load UI adds to the counter on its completion
/// path (`addiu v1,v1,3` at `0x801DC9E0`, inside `FUN_801DC6B4`).
///
/// See [`fade_flash_tick`] for the whole protocol.
pub const FLASH_COUNTER_DONE_STEP: i32 = 3;

/// Counter seed the save-side completion screen leaves before the `+3`
/// (`addiu v1,zero,4` at `0x801D8B64`). `4 + 3 = 7` lands the ramp-down arm's
/// `phase = counter - 1` on phase 6, the exit that picks handler `0x29`.
pub const FLASH_COUNTER_SEED_A: i32 = 4;

/// The sibling seed (`addiu v1,zero,5` at `0x801D8D30`). `5 + 3 = 8` lands on
/// phase 7, the exit that picks handler `0x2B`.
pub const FLASH_COUNTER_SEED_B: i32 = 5;

/// What one tick of the brightness fade/flash actor asks the host to do.
///
/// PORT: FUN_801ed308
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FadeFlashEffect {
    /// `FUN_8003479C(level)` - push the ramped brightness to the display.
    /// Every ramping arm ends on this.
    ApplyBrightness(i32),
    /// Copy the live tint triple `0x8007BF5D..5F` into the save slots
    /// `0x8007B634..636`, then zero both the live triple and its `+0xA1..A3`
    /// mirror. Also clears the flash counter.
    CaptureAndClearTint,
    /// `FUN_801D841C()` - the `jal` at `0x801ED3DC`, in the delay slot of the
    /// last tint store, so it lands immediately after
    /// [`FadeFlashEffect::CaptureAndClearTint`].
    ///
    /// Thirteen instructions: `FUN_80020DE0(0x800706BC, _DAT_8007C34C)` and
    /// `sh 1,0x5c(v0)` on the **returned actor**. Descriptor `0x800706BC`'s
    /// `+0x8` handler word is `0x80024190`, the in-field save/load screen
    /// driver, and that actor's `+0x5C` is its save-vs-load discriminator - so
    /// this is a save-side hand-off, not a screen-flash element. See
    /// [`fade_flash_tick`].
    SpawnSaveScreen,
    /// Copy the saved triple back over both the live triple and the mirror.
    RestoreTint,
    /// `scene[+0x3E] = 0` (case 5).
    ClearSceneField3E,
    /// One of the two terminal arms.
    Exit(ActorExit),
}

/// Per-frame inputs the fade/flash actor reads out of globals.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FadeFlashInput {
    /// `_DAT_1F800393`.
    pub frame_delta: i32,
    /// `_DAT_8007B440` - the brightness accumulator, read/written in place.
    pub level: i32,
    /// `_DAT_8007B43C` - the flash counter. Doubles as the phase the ramp-down
    /// arm returns to (`phase = counter - 1`).
    pub flash_counter: i32,
    /// `ctx[+0x50]`, needed by the terminal arms.
    pub handler_id: u16,
}

/// One tick of `FUN_801ED308`.
///
/// Returns the new `(phase, level, flash_counter)` plus the effects, in the
/// order retail performs them.
///
/// The eight-entry jump table is at `0x801CF4FC`. Cases 0 and 2 fall *into*
/// the next arm rather than branching to the epilogue, which is why phase 0
/// both arms and ramps in the same frame and why phase 2's saturating path
/// runs phase 3's tint restore.
///
/// ## What this machine is half of
///
/// The white-out is not decoration: phase 1's hand-off spawns the **in-field
/// save/load screen driver** ([`FadeFlashEffect::SpawnSaveScreen`]), which
/// takes the game mode over, pages the menu overlay (PROT 899) in over the
/// field overlay, runs the memory-card UI and pages the field overlay back.
/// This actor is frozen for the whole of that - mode 23's frame body ticks
/// only the save actor - so the ramp it left at [`BRIGHTNESS_MAX`] is what
/// covers the swap.
///
/// The two halves talk through exactly two globals, and both are written from
/// the **menu overlay's** save/load UI while the field overlay is not resident:
///
/// | Global | Field side (here) | Menu side |
/// |---|---|---|
/// | `_DAT_8007B440` brightness | ramped up, then down | pinned to `0xF2` at `0x801DC9DC` / `0x801D8B60` / `0x801DD190` |
/// | `_DAT_8007B43C` counter | parks on it, then decodes it | seeded 1..5, then `+= 3` at `0x801DC9E0` |
///
/// `FUN_801DC6B4` returns `counter >= 6` - the same
/// [`FLASH_COUNTER_RESTORE`] threshold phase 2/3 waits on - and the ramp-down
/// arm's `phase = counter - 1` turns the seed the UI left into *which* exit
/// this actor takes: seed [`FLASH_COUNTER_SEED_A`] (`+3 = 7`) selects phase 6
/// and handler `0x29`, seed [`FLASH_COUNTER_SEED_B`] (`+3 = 8`) selects phase
/// 7 and handler `0x2B`. So the counter is a return channel, not a timer.
///
/// PORT: FUN_801ed308
///
/// Wired: `PanelActorKind::FadeFlash` in
/// `legaia_engine_world_views::world_map_panel_host`. The host owns the brightness
/// accumulator and the flash counter, performs the spawn through
/// `crate::save_screen_spawn::save_screen_spawn`, and
/// releases the park through `PanelActorHost::release_flash`, which writes the
/// counter the way the menu side does.
pub fn fade_flash_tick(phase: i16, input: FadeFlashInput) -> (i16, i32, i32, Vec<FadeFlashEffect>) {
    let mut level = input.level;
    let mut counter = input.flash_counter;
    let mut phase = phase;
    let mut out = Vec::new();
    if !(0..8).contains(&phase) {
        return (phase, level, counter, out);
    }
    // Case 0 arms the ramp and drops straight into case 1's body.
    if phase == 0 {
        level = 0;
        phase = 1;
    }
    match phase {
        1 => {
            level += input.frame_delta * BRIGHTNESS_STEP;
            if level + BRIGHTNESS_HANDOFF_BIAS > BRIGHTNESS_MAX {
                out.push(FadeFlashEffect::CaptureAndClearTint);
                counter = 0;
                out.push(FadeFlashEffect::SpawnSaveScreen);
                phase = 2;
            }
            out.push(FadeFlashEffect::ApplyBrightness(level));
        }
        2 => {
            if counter < FLASH_COUNTER_RESTORE {
                level += input.frame_delta * BRIGHTNESS_STEP;
                if level > BRIGHTNESS_MAX {
                    level = BRIGHTNESS_MAX;
                    phase = 3;
                }
                out.push(FadeFlashEffect::ApplyBrightness(level));
            } else {
                // The saturating path jumps into case 3's body. Retail writes
                // phase 3 first and case 3 immediately overwrites it with 4,
                // so only the 4 is observable.
                level = BRIGHTNESS_MAX;
                out.push(FadeFlashEffect::RestoreTint);
                phase = 4;
                out.push(FadeFlashEffect::ApplyBrightness(level));
            }
        }
        3 => {
            level = BRIGHTNESS_MAX;
            if counter >= FLASH_COUNTER_RESTORE {
                out.push(FadeFlashEffect::RestoreTint);
                phase = 4;
            }
            out.push(FadeFlashEffect::ApplyBrightness(level));
        }
        4 => {
            level -= input.frame_delta * BRIGHTNESS_STEP;
            if level <= 0 {
                // `phase = (u16)counter - 1`, then the counter is cleared.
                phase = (counter as u16).wrapping_sub(1) as i16;
                level = 0;
                counter = 0;
            }
            out.push(FadeFlashEffect::ApplyBrightness(level));
        }
        5 => out.push(FadeFlashEffect::ClearSceneField3E),
        6 | 7 => {
            let next_handler = if phase == 6 { 0x29 } else { 0x2B };
            out.push(FadeFlashEffect::Exit(ActorExit {
                saved_handler: input.handler_id,
                next_handler,
            }));
            phase = 0;
        }
        _ => {}
    }
    (phase, level, counter, out)
}
