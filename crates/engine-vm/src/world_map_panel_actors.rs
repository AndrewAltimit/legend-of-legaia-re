//! The field-overlay **panel actor state machines** - the `ctx[+0x54]`-keyed
//! phase machines that sit on top of the shared window leaves in
//! [`crate::world_map_panel`].
//!
//! Six of them live in the world-map band of PROT 0897 (base `0x801CE818`):
//! the brightness fade/flash (`FUN_801ED308`), the sub-list open/close
//! (`FUN_801ED590`), the return-to-title soft reset (`FUN_801EDF00`), the
//! screen-fill fade transition (`FUN_801EE5D4`), the text-box dispatcher
//! (`FUN_801EE90C`) and the flag-window picker (`FUN_801EF014`). A seventh,
//! the field party HUD (`FUN_801D0D38`), is not a phase machine at all - it
//! is a per-frame panel builder with an idle timer.
//!
//! Every body here is read out of the statically extracted PROT 0897 image at
//! its own base, not out of a dump: `scripts/ghidra-analysis/locate-entry-image.py`
//! reports a stack-frame prologue for all seven VAs in `897/field` and in no
//! other based image. The `overlay_0897_*`-prefixed dumps at these addresses
//! print bodies that resolve against no image at the queried VA
//! (`docs/tooling/dump-corpus-integrity.md`); the capture-tagged dumps
//! (`801ed308.txt`, `801ed590.txt`, `801edf00.txt`, `801ee5d4.txt`,
//! `801ee90c.txt`, `801ef014.txt`,
//! `overlay_cutscene_dialogue_801d0d38.txt`) agree with the image.
//!
//! ## Shared conventions
//!
//! - `ctx[+0x54]` is the phase halfword; `ctx[+0x50]` is the system-actor
//!   handler id `FUN_801F159C` dispatches on; `ctx[+0x9E]` / `ctx[+0x9C]` are
//!   per-actor frame counters.
//! - Retiring an actor is always the same four stores: `scene[+0x2E] = -1`,
//!   `scene[+0x40] = ctx[+0x50]`, `ctx[+0x50] = <next handler id>`,
//!   `ctx[+0x54] = 0`, through the scene-struct pointer at `0x801C6EA4`. That
//!   is [`ActorExit`].
//! - `_DAT_1F800393` is the scratchpad frame-delta byte every ramp scales by.
//!   It is `frame_delta` throughout.
//! - `_DAT_8007BB80` is the global input lock: while it is non-zero every
//!   picker phase returns without reading the pad.
//!
//! ## NOT WIRED
//!
//! Nothing in the engine hosts a panel actor. `SceneMode` has no dev-menu or
//! panel-window mode, `WorldMapController` owns no window list, no panel
//! descriptor array and no `ctx[+0x54]` phase, and the render halves in
//! `legaia_engine_ui` have no caller either. Both halves are unhosted, so
//! these ports are the simulation side of a screen that does not exist yet;
//! the prerequisite is a panel-window host on `WorldMapController`.

use crate::world_map_panel::{CursorOutcome, CursorPad, list_cursor_input};

/// The four stores every terminal panel arm makes through the scene struct at
/// `0x801C6EA4` before parking the actor.
///
/// PORT: FUN_801ed308 (cases 6/7), FUN_801ed590 (state 2), FUN_801ee5d4
/// (case 4), FUN_801ee90c (the `0x801EEA50` block), FUN_801ef014 (state 3)
///
/// NOT WIRED: no engine host owns the scene struct these stores target - see
/// the module disclosure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActorExit {
    /// `scene[+0x40]` takes the actor's *old* `ctx[+0x50]`.
    pub saved_handler: u16,
    /// The handler id written back into `ctx[+0x50]`.
    pub next_handler: u16,
}

impl ActorExit {
    /// `scene[+0x2E]`, written `-1` on every one of these paths.
    pub const SCENE_SLOT_CLEAR: i16 = -1;
}

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
    /// mirror. Also clears the flash counter and calls `FUN_801D841C`.
    CaptureAndClearTint,
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
/// PORT: FUN_801ed308
///
/// NOT WIRED: no engine host owns the brightness global or the panel actor
/// table - see the module disclosure.
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

// ---------------------------------------------------------------------------
// FUN_801ED590 - sub-list open / close
// ---------------------------------------------------------------------------

/// Panel script the open arm runs (`FUN_801E9B3C(0x801F3274)`).
pub const SUBLIST_OPEN_SCRIPT: u32 = 0x801F_3274;
/// Panel script the close arm runs (`FUN_801E9B3C(0x801F3284)`).
pub const SUBLIST_CLOSE_SCRIPT: u32 = 0x801F_3284;
/// Confirm SFX the sub-list fires (`FUN_80035BD0(0x20)`).
pub const SUBLIST_CONFIRM_SFX: u32 = 0x20;
/// Handler id the close arm hands the actor back to.
pub const SUBLIST_NEXT_HANDLER: u16 = 0x1A;

/// What one tick of the sub-list actor asks the host to do.
///
/// PORT: FUN_801ed590
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubListEffect {
    /// `FUN_801E9B3C(script)`.
    RunPanelScript(u32),
    /// `_DAT_8007B910 >>= 1` (open) or `<<= 1` (close/handoff). Retail uses an
    /// arithmetic shift both ways.
    ScaleBrightness { shift_right: bool },
    /// `FUN_80035BD0(0x20)`.
    PlaySfx(u32),
    /// `_DAT_8007B450 = 0` - drops the op-`0x49` descriptor the picker used.
    ClearWindowDescriptor,
    /// `FUN_800266E0(0x8007052C)` then `FUN_801D84B4()` - the state-3 hand-off.
    HandOff,
    /// The terminal arm.
    Exit(ActorExit),
    /// `FUN_80031D00()` - the text-actor list tick, run on **every** path.
    TickTextActors,
}

/// Per-frame inputs the sub-list actor reads.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SubListInput {
    /// `_DAT_8007BB80`, the global input lock.
    pub input_locked: bool,
    /// `_DAT_8007BB88`, the shared list cursor.
    pub cursor: i32,
    /// The pad banks [`list_cursor_input`] reads.
    pub pad: CursorPad,
    /// `ctx[+0x50]`.
    pub handler_id: u16,
}

/// One tick of `FUN_801ED590`.
///
/// Returns `(phase, cursor, effects)`. The confirm arm's next phase is
/// `cursor + 2`, so cursor `0` closes the window (state 2) and cursor `1`
/// takes the hand-off (state 3); cancel goes straight to state 2.
///
/// PORT: FUN_801ed590
///
/// NOT WIRED: same host gap as the rest of the module.
pub fn sub_list_tick(phase: i16, input: SubListInput) -> (i16, i32, Vec<SubListEffect>) {
    let mut phase = phase;
    let mut cursor = input.cursor;
    let mut out = Vec::new();
    match phase {
        0 => {
            out.push(SubListEffect::ScaleBrightness { shift_right: true });
            cursor = 0;
            out.push(SubListEffect::RunPanelScript(SUBLIST_OPEN_SCRIPT));
            phase += 1;
        }
        1 => {
            if !input.input_locked {
                let (outcome, _) = list_cursor_input(&mut cursor, 2, true, &input.pad);
                match outcome {
                    CursorOutcome::ActionA => {
                        out.push(SubListEffect::PlaySfx(SUBLIST_CONFIRM_SFX));
                        phase = (cursor + 2) as i16;
                    }
                    CursorOutcome::ActionB => phase = 2,
                    _ => {}
                }
            }
        }
        2 => {
            out.push(SubListEffect::RunPanelScript(SUBLIST_CLOSE_SCRIPT));
            out.push(SubListEffect::ClearWindowDescriptor);
            out.push(SubListEffect::ScaleBrightness { shift_right: false });
            out.push(SubListEffect::Exit(ActorExit {
                saved_handler: input.handler_id,
                next_handler: SUBLIST_NEXT_HANDLER,
            }));
            phase = 0;
        }
        3 => {
            out.push(SubListEffect::ScaleBrightness { shift_right: false });
            out.push(SubListEffect::HandOff);
        }
        _ => {}
    }
    out.push(SubListEffect::TickTextActors);
    (phase, cursor, out)
}

// ---------------------------------------------------------------------------
// FUN_801EDF00 - return-to-title / soft reset
// ---------------------------------------------------------------------------

/// Start value of the records-screen slide counter `_DAT_801F35B8`.
pub const SOFT_RESET_SLIDE_START: i32 = 0xE6;
/// Value the slide settles at, and the one the pad edge is accepted on.
pub const SOFT_RESET_SLIDE_REST: i32 = 0xE;
/// Pad mask the settle phase accepts (`_DAT_8007B850 & 0x9F0`).
pub const SOFT_RESET_PAD_MASK: u32 = 0x9F0;
/// Frames the white fade the accept path fires runs for.
pub const SOFT_RESET_FADE_FRAMES: i32 = 0x78;
/// Counter value at or above which the executable is reloaded.
pub const SOFT_RESET_RELOAD_AT: i32 = 0x78;

/// What one tick of the soft-reset actor asks the host to do.
///
/// PORT: FUN_801edf00
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SoftResetEffect {
    /// `FUN_80042558(0x20)` - the arm call in state 0.
    ArmReset,
    /// `_DAT_8007B792 += 1` then `FUN_801ED710(actor, y)` - the records-screen
    /// renderer, driven at the slide counter's current value.
    DrawRecords { y: i32 },
    /// `FUN_801D58F0(2, 0, 0xFFFFFF, 0, 0x78, -1)` - the white fade.
    WhiteFade { frames: i32 },
    /// `FUN_80017714(0x801CF5A0)` - reload the boot executable by name.
    ReloadExecutable,
    /// `FUN_80031D00()`, run on every path.
    TickTextActors,
}

/// Per-frame inputs the soft-reset actor reads.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SoftResetInput {
    /// `_DAT_1F800393`.
    pub frame_delta: i32,
    /// `_DAT_801F35B8`.
    pub slide: i32,
    /// `_DAT_8007B850`.
    pub pad: u32,
}

/// One tick of `FUN_801EDF00`.
///
/// Returns `(phase, slide, effects)`.
///
/// State 1 walks the slide counter down to [`SOFT_RESET_SLIDE_REST`] and only
/// then samples the pad; state 2 walks a fresh counter up and reloads the
/// executable at [`SOFT_RESET_RELOAD_AT`] - the comparison is
/// `slti v0,v0,0x78` on the **post-increment** value, so the reload fires on
/// the first frame the counter reaches `0x78`, not at `0x77`.
///
/// PORT: FUN_801edf00
///
/// NOT WIRED: same host gap as the rest of the module.
pub fn soft_reset_tick(phase: i16, input: SoftResetInput) -> (i16, i32, Vec<SoftResetEffect>) {
    let mut phase = phase;
    let mut slide = input.slide;
    let mut out = Vec::new();
    match phase {
        0 => {
            slide = SOFT_RESET_SLIDE_START;
            out.push(SoftResetEffect::ArmReset);
            phase += 1;
        }
        1 => {
            if slide > SOFT_RESET_SLIDE_REST {
                slide -= 1;
            }
            out.push(SoftResetEffect::DrawRecords { y: slide });
            if slide == SOFT_RESET_SLIDE_REST && input.pad & SOFT_RESET_PAD_MASK != 0 {
                out.push(SoftResetEffect::WhiteFade {
                    frames: SOFT_RESET_FADE_FRAMES,
                });
                slide = 0;
                phase += 1;
            }
        }
        2 => {
            out.push(SoftResetEffect::DrawRecords {
                y: SOFT_RESET_SLIDE_REST,
            });
            slide += input.frame_delta;
            if slide >= SOFT_RESET_RELOAD_AT {
                out.push(SoftResetEffect::ReloadExecutable);
            }
        }
        3 => out.push(SoftResetEffect::DrawRecords {
            y: SOFT_RESET_SLIDE_REST,
        }),
        _ => {}
    }
    out.push(SoftResetEffect::TickTextActors);
    (phase, slide, out)
}

// ---------------------------------------------------------------------------
// FUN_801EE5D4 / FUN_801EE90C - the shared screen-fill fade transition
// ---------------------------------------------------------------------------

/// Frames each half of the fill transition holds (`ctx[+0x9E] < 0x10`).
pub const FILL_FADE_HOLD_FRAMES: i16 = 0x10;

/// Panel script the fill-fade actor opens with.
pub const FILL_FADE_SCRIPT: u32 = 0x801F_32B4;

/// The vocabulary the two fill-fade blocks share. `FUN_801EE5D4` runs one as
/// its whole five-case table; `FUN_801EE90C` runs a near-copy at phases
/// 10..13. The two are **not** identical - the dispatcher's copy opens without
/// a panel script, and its first hold arm consults neither the input lock nor
/// the text-actor tick, both of which the five-case actor does. That is why
/// [`text_box_tick`] spells its phases out instead of delegating.
///
/// PORT: FUN_801ee5d4 (cases 0..3), FUN_801ee90c (cases 10..13)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FillFadeEffect {
    /// `FUN_801E9B3C(script)` - only `FUN_801EE5D4` opens with one.
    RunPanelScript(u32),
    /// Post the full-screen fill primitive: tag `2`, extent `0x10`, RGB all
    /// `0xFF`, trailing halfword `-1`, through `FUN_80024E80(&prim, 1)`.
    PostFillPrim,
    /// `FUN_80020DE0(0x800706BC, _DAT_8007C34C)` then capture-and-zero the
    /// live tint triple.
    SpawnSubActorAndCaptureTint,
    /// `FUN_8003CF40` the two DMA lists, repost the fill primitive and restore
    /// the tint. Gated on `FUN_8003CF04` reporting the load complete.
    QueueDmaAndRestoreTint,
    /// `scene_obj[+0x10] |= 0x0008_0000`.
    SetSceneFlagBit,
    /// `FUN_80031D00()`.
    TickTextActors,
    /// The terminal arm.
    Exit(ActorExit),
    /// `scene[+0x3E] = 0` - `FUN_801EE90C`'s case 14 only.
    ClearSceneField3E,
}

/// Per-frame inputs the fill-fade stages read.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FillFadeInput {
    /// `_DAT_1F800393`.
    pub frame_delta: i16,
    /// `ctx[+0x9E]`.
    pub timer: i16,
    /// `_DAT_8007BB80`, the input lock. Only `FUN_801EE5D4`'s hold phase
    /// consults it.
    pub input_locked: bool,
    /// `FUN_8003CF04` result: non-zero means the staged load is still running.
    pub load_pending: bool,
    /// `ctx[+0x50]`.
    pub handler_id: u16,
}

/// One tick of `FUN_801EE5D4`.
///
/// Returns `(phase, timer, effects)`. Case 0 falls through into case 1's body
/// in the same frame - the jump-table arm for phase 0 ends on the phase store
/// and simply continues.
///
/// PORT: FUN_801ee5d4
///
/// NOT WIRED: same host gap as the rest of the module.
pub fn fill_fade_tick(phase: i16, input: FillFadeInput) -> (i16, i16, Vec<FillFadeEffect>) {
    let mut phase = phase;
    let mut timer = input.timer;
    let mut out = Vec::new();
    if !(0..5).contains(&phase) {
        return (phase, timer, out);
    }
    if phase == 0 {
        out.push(FillFadeEffect::RunPanelScript(FILL_FADE_SCRIPT));
        out.push(FillFadeEffect::PostFillPrim);
        timer = 0;
        phase = 1;
    }
    match phase {
        1 => {
            timer += input.frame_delta;
            out.push(FillFadeEffect::TickTextActors);
            if !input.input_locked && timer >= FILL_FADE_HOLD_FRAMES {
                out.push(FillFadeEffect::SpawnSubActorAndCaptureTint);
                phase += 1;
            }
        }
        2 => {
            out.push(FillFadeEffect::TickTextActors);
            if !input.load_pending {
                timer = 0;
                out.push(FillFadeEffect::QueueDmaAndRestoreTint);
                phase += 1;
            }
        }
        3 => {
            out.push(FillFadeEffect::TickTextActors);
            out.push(FillFadeEffect::SetSceneFlagBit);
            timer += input.frame_delta;
            if timer >= FILL_FADE_HOLD_FRAMES {
                phase += 1;
            }
        }
        4 => {
            out.push(FillFadeEffect::TickTextActors);
            out.push(FillFadeEffect::Exit(ActorExit {
                saved_handler: input.handler_id,
                next_handler: 0,
            }));
            phase = 0;
        }
        _ => {}
    }
    (phase, timer, out)
}

// ---------------------------------------------------------------------------
// FUN_801EE90C - world-map text-box dispatcher
// ---------------------------------------------------------------------------

/// Phase the dispatcher's case 0 hands straight to - the fill-fade block.
pub const TEXT_BOX_FADE_PHASE: i16 = 10;

/// Phase below which the epilogue ticks the text-actor list
/// (`slti v0,v0,0xa`).
pub const TEXT_BOX_TICK_BELOW: i16 = 10;

/// Panel script the "yes" arm runs after the party restore.
pub const TEXT_BOX_CONFIRM_SCRIPT: u32 = 0x801F_32DC;
/// Panel script the "no" arm runs.
pub const TEXT_BOX_DECLINE_SCRIPT: u32 = 0x801F_2A88;
/// Cursor-move SFX the confirm arm plays before the restore.
pub const TEXT_BOX_MOVE_SFX: u32 = 0;
/// Restore SFX the confirm arm plays.
pub const TEXT_BOX_RESTORE_SFX: u32 = 0x25;
/// Party slots the restore arm walks (`s1 < 3`, stride `0x414`).
pub const TEXT_BOX_RESTORE_SLOTS: usize = 3;

/// What one tick of the text-box dispatcher asks the host to do, over and
/// above the shared fill-fade stages.
///
/// PORT: FUN_801ee90c
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextBoxEffect {
    /// `FUN_80035BD0(0)` then `FUN_80035B50(0x25)`.
    PlaySfx(u32),
    /// For each of the first [`TEXT_BOX_RESTORE_SLOTS`] party records, copy
    /// `rec[+0x1C4] -> rec[+0x1C6]` and `rec[+0x1C8] -> rec[+0x1CA]` (the
    /// `0x80084140`-relative offsets `0x6CC/0x6CE` and `0x6D0/0x6D2`).
    RestoreParty,
    /// `FUN_801E9B3C(script)`.
    RunPanelScript(u32),
    /// The `0x801EEA50` terminal block.
    Exit(ActorExit),
    /// One of the shared fill-fade stages, replayed at phases 10..13.
    Fade(FillFadeEffect),
    /// Case 14: `scene[+0x3E] = 0`.
    ClearSceneField3E,
    /// `FUN_80031D00()` - the epilogue runs it only while `phase < 10`.
    TickTextActors,
}

/// Per-frame inputs the text-box dispatcher reads.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TextBoxInput {
    /// `_DAT_8007BB80`.
    pub input_locked: bool,
    /// `_DAT_8007BB88`, the shared cursor.
    pub cursor: i32,
    /// The pad banks the shared cursor kernel reads.
    pub pad: CursorPad,
    /// `_DAT_8007BB84 & (rec0[+0x590] | rec0[+0x594])` - phase 2's dismiss
    /// test, pre-reduced to a bool by the caller.
    pub dismiss_pressed: bool,
    /// Inputs the shared fade stages need.
    pub fade: FillFadeInput,
    /// `ctx[+0x50]`.
    pub handler_id: u16,
}

/// One tick of `FUN_801EE90C`.
///
/// Returns `(phase, cursor, timer, effects)`, where `timer` is `ctx[+0x9E]`.
/// The 15-entry jump table is at `0x801CF5FC`; entries 4..9 all point at the
/// epilogue, so those phases do nothing but the `phase < 10` text-actor tick.
///
/// PORT: FUN_801ee90c
///
/// NOT WIRED: same host gap as the rest of the module.
pub fn text_box_tick(phase: i16, input: TextBoxInput) -> (i16, i32, i16, Vec<TextBoxEffect>) {
    let mut phase = phase;
    let mut cursor = input.cursor;
    let mut timer = input.fade.timer;
    let mut out = Vec::new();
    if !(0..15).contains(&phase) {
        return (phase, cursor, timer, out);
    }
    let exit = ActorExit {
        saved_handler: input.handler_id,
        next_handler: 0x1A,
    };
    match phase {
        0 => phase = TEXT_BOX_FADE_PHASE,
        1 => {
            if !input.input_locked {
                let (outcome, _) = list_cursor_input(&mut cursor, 2, true, &input.pad);
                match outcome {
                    CursorOutcome::ActionA => {
                        if cursor == 0 {
                            out.push(TextBoxEffect::PlaySfx(TEXT_BOX_MOVE_SFX));
                            out.push(TextBoxEffect::PlaySfx(TEXT_BOX_RESTORE_SFX));
                            out.push(TextBoxEffect::RestoreParty);
                            out.push(TextBoxEffect::RunPanelScript(TEXT_BOX_CONFIRM_SCRIPT));
                            phase = 2;
                        } else {
                            out.push(TextBoxEffect::RunPanelScript(TEXT_BOX_DECLINE_SCRIPT));
                            phase = 3;
                        }
                    }
                    CursorOutcome::ActionB => {
                        out.push(TextBoxEffect::Exit(exit));
                        phase = 0;
                    }
                    _ => {}
                }
            }
        }
        2 => {
            if !input.input_locked && input.dismiss_pressed {
                out.push(TextBoxEffect::Exit(exit));
                phase = 0;
            }
        }
        3 => {
            if !input.input_locked {
                phase = TEXT_BOX_FADE_PHASE;
            }
        }
        4..=9 => {}
        10 => {
            // Posts the fill primitive, zeroes the timer and falls straight
            // into phase 11's body - no panel script, unlike FUN_801EE5D4.
            out.push(TextBoxEffect::Fade(FillFadeEffect::PostFillPrim));
            timer = 0;
            phase = 11;
            timer += input.fade.frame_delta;
            if timer >= FILL_FADE_HOLD_FRAMES {
                out.push(TextBoxEffect::Fade(
                    FillFadeEffect::SpawnSubActorAndCaptureTint,
                ));
                phase = 12;
            }
        }
        11 => {
            timer += input.fade.frame_delta;
            if timer >= FILL_FADE_HOLD_FRAMES {
                out.push(TextBoxEffect::Fade(
                    FillFadeEffect::SpawnSubActorAndCaptureTint,
                ));
                phase = 12;
            }
        }
        12 => {
            out.push(TextBoxEffect::Fade(FillFadeEffect::TickTextActors));
            if !input.fade.load_pending {
                timer = 0;
                out.push(TextBoxEffect::Fade(FillFadeEffect::QueueDmaAndRestoreTint));
                phase = 13;
            }
        }
        13 => {
            out.push(TextBoxEffect::Fade(FillFadeEffect::SetSceneFlagBit));
            timer += input.fade.frame_delta;
            if timer >= FILL_FADE_HOLD_FRAMES {
                phase = 14;
            }
        }
        14 => out.push(TextBoxEffect::ClearSceneField3E),
        _ => {}
    }
    if phase < TEXT_BOX_TICK_BELOW {
        out.push(TextBoxEffect::TickTextActors);
    }
    (phase, cursor, timer, out)
}

// ---------------------------------------------------------------------------
// FUN_801EF014 - flag-window picker
// ---------------------------------------------------------------------------

/// Descriptor the flag-window picker reads through `_DAT_8007B450`, laid out
/// by the MAN's op-`0x49` operands.
///
/// PORT: FUN_801ef014 (`lbu 1/2/3(desc)`, `FUN_8003CE9C(desc + 4)`)
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FlagWindowDescriptor {
    /// `desc[+1]` - how many consecutive story flags the window covers.
    pub count: u8,
    /// `desc[+2]` - the first visible row, and the fallback selection when no
    /// flag in the range is set.
    pub first_visible: u8,
    /// `desc[+3]` - how many rows are visible at once.
    pub rows: u8,
    /// `desc[+4..6]` - the base story-flag id, read as an unaligned `u16`.
    pub base_flag: i32,
}

/// Panel-descriptor index the picker sizes (`0x801F2B98 + 0x192/0x196`, i.e.
/// `14*28 + 10` and `14*28 + 14`).
pub const FLAG_WINDOW_PANEL_INDEX: usize = 14;

/// Rows the panel geometry is measured against (`8 - rows`).
pub const FLAG_WINDOW_PANEL_ROWS: i32 = 8;

/// Bottom anchor the panel's `y` is offset from.
pub const FLAG_WINDOW_PANEL_Y_BASE: i32 = 0x48;

/// Sentinel the cancel and no-op-confirm arms write into `_DAT_8007BB88`.
pub const FLAG_WINDOW_CANCEL_SENTINEL: i32 = 0x64;

/// Panel script the picker opens with.
pub const FLAG_WINDOW_SCRIPT: u32 = 0x801F_3304;

/// The panel `(y, height)` the picker writes into descriptor
/// [`FLAG_WINDOW_PANEL_INDEX`].
///
/// `height = rows * 16` and `y = (8 - rows) * 16 + 0x48`, so the window grows
/// upward from a fixed bottom edge at `0x48 + 8*16 = 0xC8`.
///
/// PORT: FUN_801ef014 (`sll v0,v0,4` / `subu` / `addiu v0,v0,0x48`)
pub fn flag_window_panel_geometry(rows: u8) -> (i32, i32) {
    let rows = i32::from(rows);
    let height = rows << 4;
    let y = ((FLAG_WINDOW_PANEL_ROWS - rows) << 4) + FLAG_WINDOW_PANEL_Y_BASE;
    (y, height)
}

/// Screen row for an absolute selection, and its inverse.
///
/// The list draws bottom-up, so retail converts in both directions with the
/// same expression: `row = rows - (sel - first_visible) - 1`. Applying it
/// twice is the identity, which is exactly how the confirm arm recovers the
/// selection from the cursor the shared kernel moved.
///
/// PORT: FUN_801ef014 (`subu v1,v1,a2; subu v0,v0,v1; addiu v0,v0,-1`)
pub fn flag_window_row_flip(value: i32, desc: FlagWindowDescriptor) -> i32 {
    i32::from(desc.rows) - (value - i32::from(desc.first_visible)) - 1
}

/// What one tick of the flag-window picker asks the host to do.
///
/// PORT: FUN_801ef014
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlagWindowEffect {
    /// Clear every flag in `[base_flag, base_flag + count)` (`FUN_8003CE34`).
    ClearRange { base: i32, count: u8 },
    /// Size panel [`FLAG_WINDOW_PANEL_INDEX`].
    SizePanel { y: i32, height: i32 },
    /// `FUN_801E9B3C(script)`.
    RunPanelScript(u32),
    /// `FUN_8003CE08(base_flag + selection)` - commit the pick.
    SetFlag(i32),
    /// The terminal arm.
    Exit(ActorExit),
    /// `FUN_80031D00()`, run on every path.
    TickTextActors,
}

/// Per-frame inputs the flag-window picker reads.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FlagWindowInput {
    /// The op-`0x49` descriptor at `_DAT_8007B450`.
    pub desc: FlagWindowDescriptor,
    /// `_DAT_8007BB80`.
    pub input_locked: bool,
    /// `_DAT_8007BB88`, the picker's selection.
    pub selection: i32,
    /// `_DAT_8007BB9C`, the row the scan remembered on entry.
    pub remembered: i32,
    /// The pad banks the shared cursor kernel reads.
    pub pad: CursorPad,
    /// `ctx[+0x50]`.
    pub handler_id: u16,
}

/// Scan result for phase 0: the first index in `[0, count)` whose flag is set,
/// or `count` when none is.
///
/// PORT: FUN_801ef014 (the `FUN_8003CE64` scan loop)
pub fn flag_window_initial_row(flag_set: impl Fn(i32) -> bool, desc: FlagWindowDescriptor) -> i32 {
    let count = i32::from(desc.count);
    let mut i = 0;
    while i < count {
        if flag_set(desc.base_flag + i) {
            return i;
        }
        i += 1;
    }
    count
}

/// One tick of `FUN_801EF014`.
///
/// Returns `(phase, selection, remembered, effects)`.
///
/// `flag_set` answers `FUN_8003CE64` for phase 0's scan; it is never consulted
/// in any other phase.
///
/// PORT: FUN_801ef014
///
/// NOT WIRED: same host gap as the rest of the module.
pub fn flag_window_tick(
    phase: i16,
    input: FlagWindowInput,
    flag_set: impl Fn(i32) -> bool,
) -> (i16, i32, i32, Vec<FlagWindowEffect>) {
    let desc = input.desc;
    let mut phase = phase;
    let mut selection = input.selection;
    let mut remembered = input.remembered;
    let mut out = Vec::new();
    match phase {
        0 => {
            let found = flag_window_initial_row(flag_set, desc);
            remembered = found;
            selection = if found == i32::from(desc.count) {
                i32::from(desc.first_visible)
            } else {
                found
            };
            out.push(FlagWindowEffect::ClearRange {
                base: desc.base_flag,
                count: desc.count,
            });
            let (y, height) = flag_window_panel_geometry(desc.rows);
            out.push(FlagWindowEffect::SizePanel { y, height });
            out.push(FlagWindowEffect::RunPanelScript(FLAG_WINDOW_SCRIPT));
            phase += 1;
        }
        1 => {
            if !input.input_locked {
                let mut row = flag_window_row_flip(selection, desc);
                let (outcome, _) =
                    list_cursor_input(&mut row, i32::from(desc.rows), false, &input.pad);
                let picked = flag_window_row_flip(row, desc);
                selection = picked;
                match outcome {
                    CursorOutcome::ActionA => {
                        if picked == remembered {
                            selection = FLAG_WINDOW_CANCEL_SENTINEL;
                            phase += 2;
                        } else {
                            out.push(FlagWindowEffect::SetFlag(desc.base_flag + picked));
                            phase += 1;
                        }
                    }
                    CursorOutcome::ActionB => {
                        selection = FLAG_WINDOW_CANCEL_SENTINEL;
                        phase += 2;
                    }
                    _ => {}
                }
            }
        }
        2 => phase += 1,
        3 => {
            out.push(FlagWindowEffect::Exit(ActorExit {
                saved_handler: input.handler_id,
                next_handler: 0x1A,
            }));
            phase = 0;
        }
        _ => {}
    }
    out.push(FlagWindowEffect::TickTextActors);
    (phase, selection, remembered, out)
}

// ---------------------------------------------------------------------------
// FUN_801D0D38 - field party HUD
// ---------------------------------------------------------------------------

/// Idle frames before the HUD appears in the low-camera mode
/// (`_DAT_800845C4 == 0`).
pub const HUD_IDLE_FRAMES_NEAR: i16 = 0x28;
/// Idle frames before it appears in the far mode (`_DAT_800845C4 == 1`).
pub const HUD_IDLE_FRAMES_FAR: i16 = 0xA0;
/// Shortened idle when `_DAT_8007B5F4 == 1` in the far mode.
pub const HUD_IDLE_FRAMES_FAR_SHORT: i16 = 0x50;
/// Pad mask that suppresses the HUD outright (the four D-pad bits).
pub const HUD_SUPPRESS_PAD_MASK: u32 = 0xF000;
/// Default panel top edge.
pub const HUD_Y_TOP: i16 = 12;
/// Panel top edge used when the projected player is near the screen top.
pub const HUD_Y_BOTTOM: i16 = 0xAA;
/// Projected screen `y` below which the panel moves out of the way.
pub const HUD_DODGE_THRESHOLD: i16 = 0x30;
/// Horizontal stride between party members' panels.
pub const HUD_MEMBER_PITCH: i16 = 0x64;

/// What the field party-HUD builder decides for one frame.
///
/// PORT: FUN_801d0d38
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HudDecision {
    /// The HUD is suppressed and the countdown reset marker
    /// `_DAT_8007B5F4` is cleared.
    Suppressed,
    /// The player moved, or the scene was just entered: the countdown is
    /// rearmed at `timer` and the cached position is refreshed.
    Rearmed { timer: i16 },
    /// The player is stationary and the countdown is still running.
    CountingDown { timer: i16 },
    /// The countdown expired: build the panel at `y`, one column per party
    /// member at [`HUD_MEMBER_PITCH`] apart.
    Draw { y: i16 },
}

/// Per-frame inputs the HUD builder reads.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct HudInput {
    /// `_DAT_8007B868` - a non-zero value suppresses the HUD.
    pub hud_disabled: bool,
    /// `_DAT_800845C4` - the camera/view mode. `2` also suppresses.
    pub view_mode: i32,
    /// `_DAT_8007B850` - the pad bank the D-pad mask is taken from.
    pub pad: u32,
    /// `_DAT_1F800394 & 0x0800_0000` - the second suppress gate.
    pub scratch_suppress: bool,
    /// True on the frame the scene is (re-)entered: any of the object flag
    /// `0x0008_0000`, `_DAT_1F800394 & 0x400`, `_DAT_8007B6B4 != 0`, or
    /// `_DAT_8007B6B0 == 0`.
    pub rearm: bool,
    /// `_DAT_8007B5F4` - the short-idle marker.
    pub short_idle: bool,
    /// `_DAT_801F348C` - the idle countdown.
    pub timer: i16,
    /// `_DAT_1F80038F` - the countdown's per-frame decrement.
    pub timer_delta: i16,
    /// True when the cached `(x, z)` at `_DAT_801F3488/348A` still match the
    /// player object's `+0x14` / `+0x18`.
    pub player_stationary: bool,
    /// Projected screen `y` of the player (`FUN_800195A8` output `+0x2A`).
    /// `None` when the projection was not reached because the staged load
    /// `FUN_8003CF04` was still pending, which forces the low panel.
    pub projected_y: Option<i16>,
}

/// The idle countdown this view mode arms.
///
/// PORT: FUN_801d0d38 (`0x28` / `0xA0` / `0x50` immediates)
pub fn hud_idle_frames(view_mode: i32, short_idle: bool) -> i16 {
    match (view_mode, short_idle) {
        (0, true) => 0,
        (0, _) => HUD_IDLE_FRAMES_NEAR,
        (1, true) => HUD_IDLE_FRAMES_FAR_SHORT,
        (1, _) => HUD_IDLE_FRAMES_FAR,
        // Any other mode leaves the previous value in place; the rearm path
        // reached from the position compare uses the two-way form below.
        _ => HUD_IDLE_FRAMES_NEAR,
    }
}

/// One frame of `FUN_801D0D38`.
///
/// The gates run in retail's order: the two suppress globals, then the D-pad
/// mask, then the scratchpad bit, then the rearm condition, then the cached
/// position compare, then the countdown.
///
/// PORT: FUN_801d0d38
///
/// NOT WIRED: the engine's field HUD is drawn by `legaia_engine_ui`'s own
/// overlay path and has no idle-timer host; nothing produces the cached
/// player position or `_DAT_800845C4` this consumes.
pub fn field_hud_tick(input: HudInput) -> (i16, HudDecision) {
    if input.hud_disabled || input.view_mode == 2 {
        return (input.timer, HudDecision::Suppressed);
    }
    if input.pad & HUD_SUPPRESS_PAD_MASK != 0 || input.scratch_suppress {
        return (input.timer, HudDecision::Suppressed);
    }
    if input.rearm {
        let timer = hud_idle_frames(input.view_mode, input.short_idle);
        return (timer, HudDecision::Rearmed { timer });
    }
    if !input.player_stationary {
        // The position-compare rearm ignores `short_idle` and picks purely on
        // `view_mode != 0`.
        let timer = if input.view_mode != 0 {
            HUD_IDLE_FRAMES_FAR
        } else {
            HUD_IDLE_FRAMES_NEAR
        };
        return (timer, HudDecision::Rearmed { timer });
    }
    let timer = input.timer - input.timer_delta;
    if timer > 0 {
        return (timer, HudDecision::CountingDown { timer });
    }
    let y = match input.projected_y {
        Some(py) if py >= HUD_DODGE_THRESHOLD => HUD_Y_TOP,
        Some(_) => HUD_Y_BOTTOM,
        None => HUD_Y_BOTTOM,
    };
    (0, HudDecision::Draw { y })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pad(held: u32, pressed: u32) -> CursorPad {
        CursorPad {
            held,
            pressed,
            action_a_mask: 0x20,
            action_b_mask: 0x40,
        }
    }

    // --- FUN_801ED308 -----------------------------------------------------

    #[test]
    fn fade_flash_case0_arms_and_ramps_in_one_frame() {
        let (phase, level, _, out) = fade_flash_tick(
            0,
            FadeFlashInput {
                frame_delta: 1,
                level: 999,
                ..Default::default()
            },
        );
        assert_eq!(phase, 1);
        assert_eq!(level, BRIGHTNESS_STEP, "case 0 zeroes then case 1 ramps");
        assert_eq!(out, vec![FadeFlashEffect::ApplyBrightness(BRIGHTNESS_STEP)]);
    }

    #[test]
    fn fade_flash_captures_the_tint_at_the_biased_threshold() {
        // 0x82 + 0x70 == 0xF2, still below the bound.
        let (phase, _, _, out) = fade_flash_tick(
            1,
            FadeFlashInput {
                frame_delta: 0,
                level: 0x82,
                ..Default::default()
            },
        );
        assert_eq!(phase, 1);
        assert_eq!(out, vec![FadeFlashEffect::ApplyBrightness(0x82)]);

        let (phase, _, _, out) = fade_flash_tick(
            1,
            FadeFlashInput {
                frame_delta: 0,
                level: 0x83,
                ..Default::default()
            },
        );
        assert_eq!(phase, 2);
        assert_eq!(out[0], FadeFlashEffect::CaptureAndClearTint);
    }

    #[test]
    fn fade_flash_saturating_hold_runs_the_restore_arm() {
        let (phase, level, _, out) = fade_flash_tick(
            2,
            FadeFlashInput {
                frame_delta: 1,
                level: 0,
                flash_counter: FLASH_COUNTER_RESTORE,
                ..Default::default()
            },
        );
        assert_eq!(phase, 4, "case 2 falls into case 3's body");
        assert_eq!(level, BRIGHTNESS_MAX);
        assert_eq!(out[0], FadeFlashEffect::RestoreTint);
    }

    #[test]
    fn fade_flash_rampdown_returns_to_counter_minus_one() {
        let (phase, level, counter, _) = fade_flash_tick(
            4,
            FadeFlashInput {
                frame_delta: 100,
                level: 1,
                flash_counter: 7,
                ..Default::default()
            },
        );
        assert_eq!(phase, 6);
        assert_eq!(level, 0);
        assert_eq!(counter, 0);
    }

    #[test]
    fn fade_flash_terminal_arms_pick_different_handlers() {
        let inp = FadeFlashInput {
            handler_id: 0x11,
            ..Default::default()
        };
        let (_, _, _, six) = fade_flash_tick(6, inp);
        let (_, _, _, seven) = fade_flash_tick(7, inp);
        assert_eq!(
            six,
            vec![FadeFlashEffect::Exit(ActorExit {
                saved_handler: 0x11,
                next_handler: 0x29
            })]
        );
        assert_eq!(
            seven,
            vec![FadeFlashEffect::Exit(ActorExit {
                saved_handler: 0x11,
                next_handler: 0x2B
            })]
        );
    }

    #[test]
    fn fade_flash_out_of_range_phase_does_nothing() {
        let (phase, _, _, out) = fade_flash_tick(8, FadeFlashInput::default());
        assert_eq!(phase, 8);
        assert!(out.is_empty());
    }

    // --- FUN_801ED590 -----------------------------------------------------

    #[test]
    fn sub_list_open_halves_the_scale_and_zeroes_the_cursor() {
        let (phase, cursor, out) = sub_list_tick(
            0,
            SubListInput {
                cursor: 5,
                ..Default::default()
            },
        );
        assert_eq!((phase, cursor), (1, 0));
        assert_eq!(out[0], SubListEffect::ScaleBrightness { shift_right: true });
        assert_eq!(out[1], SubListEffect::RunPanelScript(SUBLIST_OPEN_SCRIPT));
        assert_eq!(*out.last().unwrap(), SubListEffect::TickTextActors);
    }

    #[test]
    fn sub_list_confirm_phase_is_cursor_plus_two() {
        for cursor in 0..2 {
            let (phase, _, out) = sub_list_tick(
                1,
                SubListInput {
                    cursor,
                    pad: pad(0x20, 0),
                    ..Default::default()
                },
            );
            assert_eq!(phase, (cursor + 2) as i16);
            assert_eq!(out[0], SubListEffect::PlaySfx(SUBLIST_CONFIRM_SFX));
        }
    }

    #[test]
    fn sub_list_cancel_always_closes() {
        let (phase, _, _) = sub_list_tick(
            1,
            SubListInput {
                cursor: 1,
                pad: pad(0x40, 0),
                ..Default::default()
            },
        );
        assert_eq!(phase, 2);
    }

    #[test]
    fn sub_list_ignores_the_pad_while_locked() {
        let (phase, cursor, out) = sub_list_tick(
            1,
            SubListInput {
                input_locked: true,
                cursor: 1,
                pad: pad(0x20, 0),
                ..Default::default()
            },
        );
        assert_eq!((phase, cursor), (1, 1));
        assert_eq!(out, vec![SubListEffect::TickTextActors]);
    }

    #[test]
    fn sub_list_close_arm_doubles_back_and_exits() {
        let (phase, _, out) = sub_list_tick(
            2,
            SubListInput {
                handler_id: 7,
                ..Default::default()
            },
        );
        assert_eq!(phase, 0);
        assert!(out.contains(&SubListEffect::ClearWindowDescriptor));
        assert!(out.contains(&SubListEffect::Exit(ActorExit {
            saved_handler: 7,
            next_handler: SUBLIST_NEXT_HANDLER
        })));
    }

    // --- FUN_801EDF00 -----------------------------------------------------

    #[test]
    fn soft_reset_slide_walks_down_to_the_rest_value() {
        let (mut phase, mut slide, _) = soft_reset_tick(0, SoftResetInput::default());
        assert_eq!((phase, slide), (1, SOFT_RESET_SLIDE_START));
        for _ in 0..(SOFT_RESET_SLIDE_START - SOFT_RESET_SLIDE_REST) {
            let (p, s, _) = soft_reset_tick(
                phase,
                SoftResetInput {
                    slide,
                    ..Default::default()
                },
            );
            phase = p;
            slide = s;
        }
        assert_eq!((phase, slide), (1, SOFT_RESET_SLIDE_REST));
    }

    #[test]
    fn soft_reset_pad_is_only_sampled_at_rest() {
        let (phase, _, out) = soft_reset_tick(
            1,
            SoftResetInput {
                slide: 0x40,
                pad: SOFT_RESET_PAD_MASK,
                ..Default::default()
            },
        );
        assert_eq!(phase, 1);
        assert!(
            !out.iter()
                .any(|e| matches!(e, SoftResetEffect::WhiteFade { .. }))
        );

        let (phase, slide, out) = soft_reset_tick(
            1,
            SoftResetInput {
                slide: SOFT_RESET_SLIDE_REST,
                pad: SOFT_RESET_PAD_MASK,
                ..Default::default()
            },
        );
        assert_eq!((phase, slide), (2, 0));
        assert!(out.contains(&SoftResetEffect::WhiteFade {
            frames: SOFT_RESET_FADE_FRAMES
        }));
    }

    #[test]
    fn soft_reset_reloads_on_reaching_the_bound_not_one_before() {
        let (_, _, out) = soft_reset_tick(
            2,
            SoftResetInput {
                frame_delta: 1,
                slide: SOFT_RESET_RELOAD_AT - 2,
                ..Default::default()
            },
        );
        assert!(!out.contains(&SoftResetEffect::ReloadExecutable));
        let (_, slide, out) = soft_reset_tick(
            2,
            SoftResetInput {
                frame_delta: 1,
                slide: SOFT_RESET_RELOAD_AT - 1,
                ..Default::default()
            },
        );
        assert_eq!(slide, SOFT_RESET_RELOAD_AT);
        assert!(out.contains(&SoftResetEffect::ReloadExecutable));
    }

    // --- FUN_801EE5D4 -----------------------------------------------------

    #[test]
    fn fill_fade_case0_posts_and_falls_into_the_hold() {
        let (phase, timer, out) = fill_fade_tick(
            0,
            FillFadeInput {
                frame_delta: 3,
                timer: 999,
                ..Default::default()
            },
        );
        assert_eq!(phase, 1);
        assert_eq!(timer, 3, "the hold arm ticks in the same frame");
        assert_eq!(out[0], FillFadeEffect::RunPanelScript(FILL_FADE_SCRIPT));
        assert_eq!(out[1], FillFadeEffect::PostFillPrim);
    }

    #[test]
    fn fill_fade_hold_respects_the_input_lock() {
        let (phase, _, _) = fill_fade_tick(
            1,
            FillFadeInput {
                timer: FILL_FADE_HOLD_FRAMES,
                input_locked: true,
                ..Default::default()
            },
        );
        assert_eq!(phase, 1);
    }

    #[test]
    fn fill_fade_load_gate_holds_phase_two() {
        let (phase, _, _) = fill_fade_tick(
            2,
            FillFadeInput {
                load_pending: true,
                ..Default::default()
            },
        );
        assert_eq!(phase, 2);
        let (phase, timer, out) = fill_fade_tick(
            2,
            FillFadeInput {
                timer: 9,
                ..Default::default()
            },
        );
        assert_eq!((phase, timer), (3, 0));
        assert!(out.contains(&FillFadeEffect::QueueDmaAndRestoreTint));
    }

    #[test]
    fn fill_fade_every_phase_ticks_the_text_actors() {
        for phase in 1..5 {
            let (_, _, out) = fill_fade_tick(phase, FillFadeInput::default());
            assert!(
                out.contains(&FillFadeEffect::TickTextActors),
                "phase {phase}"
            );
        }
    }

    // --- FUN_801EE90C -----------------------------------------------------

    #[test]
    fn text_box_case0_jumps_to_the_fade_block() {
        let (phase, _, _, out) = text_box_tick(0, TextBoxInput::default());
        assert_eq!(phase, TEXT_BOX_FADE_PHASE);
        assert!(
            !out.contains(&TextBoxEffect::TickTextActors),
            "phase 10 is not below the tick bound"
        );
    }

    #[test]
    fn text_box_confirm_on_row_zero_restores_the_party() {
        let (phase, _, _, out) = text_box_tick(
            1,
            TextBoxInput {
                cursor: 0,
                pad: pad(0x20, 0),
                ..Default::default()
            },
        );
        assert_eq!(phase, 2);
        assert!(out.contains(&TextBoxEffect::RestoreParty));
        assert!(out.contains(&TextBoxEffect::RunPanelScript(TEXT_BOX_CONFIRM_SCRIPT)));
    }

    #[test]
    fn text_box_confirm_on_row_one_declines() {
        let (phase, _, _, out) = text_box_tick(
            1,
            TextBoxInput {
                cursor: 1,
                pad: pad(0x20, 0),
                ..Default::default()
            },
        );
        assert_eq!(phase, 3);
        assert!(out.contains(&TextBoxEffect::RunPanelScript(TEXT_BOX_DECLINE_SCRIPT)));
        assert!(!out.contains(&TextBoxEffect::RestoreParty));
    }

    #[test]
    fn text_box_dismiss_needs_both_the_unlock_and_the_press() {
        let base = TextBoxInput {
            handler_id: 3,
            ..Default::default()
        };
        assert_eq!(text_box_tick(2, base).0, 2);
        let (phase, _, _, out) = text_box_tick(
            2,
            TextBoxInput {
                dismiss_pressed: true,
                ..base
            },
        );
        assert_eq!(phase, 0);
        assert!(out.iter().any(|e| matches!(e, TextBoxEffect::Exit(_))));
        assert_eq!(
            text_box_tick(
                2,
                TextBoxInput {
                    dismiss_pressed: true,
                    input_locked: true,
                    ..base
                }
            )
            .0,
            2
        );
    }

    #[test]
    fn text_box_idle_phases_only_tick_text_actors() {
        for phase in 4..10 {
            let (p, _, _, out) = text_box_tick(phase, TextBoxInput::default());
            assert_eq!(p, phase);
            assert_eq!(out, vec![TextBoxEffect::TickTextActors], "phase {phase}");
        }
    }

    #[test]
    fn text_box_fade_block_posts_the_prim_and_resets_the_timer() {
        // Phase 10 zeroes the timer that phase 11's body then ticks, so a
        // large incoming timer must NOT let it skip a frame.
        let (phase, _, timer, out) = text_box_tick(
            10,
            TextBoxInput {
                fade: FillFadeInput {
                    timer: 999,
                    frame_delta: 1,
                    ..Default::default()
                },
                ..Default::default()
            },
        );
        assert_eq!((phase, timer), (11, 1));
        assert_eq!(out, vec![TextBoxEffect::Fade(FillFadeEffect::PostFillPrim)]);
    }

    #[test]
    fn text_box_fade_hold_ignores_the_input_lock() {
        // FUN_801EE5D4's hold arm gates on `_DAT_8007BB80`; the dispatcher's
        // copy does not, and that difference is the reason the two blocks are
        // written out separately.
        let (phase, _, _, _) = text_box_tick(
            11,
            TextBoxInput {
                input_locked: true,
                fade: FillFadeInput {
                    timer: FILL_FADE_HOLD_FRAMES,
                    input_locked: true,
                    ..Default::default()
                },
                ..Default::default()
            },
        );
        assert_eq!(phase, 12);
    }

    #[test]
    fn text_box_fade_block_walks_to_the_terminal_case() {
        let held = FillFadeInput {
            timer: FILL_FADE_HOLD_FRAMES,
            ..Default::default()
        };
        for (from, to) in [(11i16, 12i16), (12, 13), (13, 14)] {
            let (phase, _, _, _) = text_box_tick(
                from,
                TextBoxInput {
                    fade: held,
                    ..Default::default()
                },
            );
            assert_eq!(phase, to, "phase {from}");
        }
        let (_, _, _, out) = text_box_tick(14, TextBoxInput::default());
        assert_eq!(out, vec![TextBoxEffect::ClearSceneField3E]);
    }

    // --- FUN_801EF014 -----------------------------------------------------

    fn fw_desc() -> FlagWindowDescriptor {
        FlagWindowDescriptor {
            count: 8,
            first_visible: 0,
            rows: 4,
            base_flag: 0x138,
        }
    }

    #[test]
    fn flag_window_panel_grows_from_a_fixed_bottom_edge() {
        for rows in 1u8..=8 {
            let (y, h) = flag_window_panel_geometry(rows);
            assert_eq!(
                y + h,
                FLAG_WINDOW_PANEL_ROWS * 16 + FLAG_WINDOW_PANEL_Y_BASE
            );
        }
    }

    #[test]
    fn flag_window_row_flip_is_an_involution() {
        let d = fw_desc();
        for v in -4..12 {
            assert_eq!(flag_window_row_flip(flag_window_row_flip(v, d), d), v);
        }
    }

    #[test]
    fn flag_window_scan_falls_back_to_first_visible() {
        let d = FlagWindowDescriptor {
            first_visible: 3,
            ..fw_desc()
        };
        let (phase, sel, remembered, out) = flag_window_tick(
            0,
            FlagWindowInput {
                desc: d,
                ..Default::default()
            },
            |_| false,
        );
        assert_eq!(phase, 1);
        assert_eq!(remembered, i32::from(d.count));
        assert_eq!(sel, i32::from(d.first_visible));
        assert!(out.contains(&FlagWindowEffect::ClearRange {
            base: d.base_flag,
            count: d.count
        }));
    }

    #[test]
    fn flag_window_scan_finds_the_first_set_flag() {
        let d = fw_desc();
        let (_, sel, remembered, _) = flag_window_tick(
            0,
            FlagWindowInput {
                desc: d,
                ..Default::default()
            },
            |f| f == d.base_flag + 5,
        );
        assert_eq!((sel, remembered), (5, 5));
    }

    #[test]
    fn flag_window_confirm_on_the_remembered_row_sets_nothing() {
        let d = fw_desc();
        let (phase, sel, _, out) = flag_window_tick(
            1,
            FlagWindowInput {
                desc: d,
                selection: 2,
                remembered: 2,
                pad: pad(0x20, 0),
                ..Default::default()
            },
            |_| false,
        );
        assert_eq!(phase, 3);
        assert_eq!(sel, FLAG_WINDOW_CANCEL_SENTINEL);
        assert!(
            !out.iter()
                .any(|e| matches!(e, FlagWindowEffect::SetFlag(_)))
        );
    }

    #[test]
    fn flag_window_confirm_on_a_new_row_commits_the_flag() {
        let d = fw_desc();
        let (phase, _, _, out) = flag_window_tick(
            1,
            FlagWindowInput {
                desc: d,
                selection: 2,
                remembered: 0,
                pad: pad(0x20, 0),
                ..Default::default()
            },
            |_| false,
        );
        assert_eq!(phase, 2);
        assert!(out.contains(&FlagWindowEffect::SetFlag(d.base_flag + 2)));
    }

    #[test]
    fn flag_window_cursor_moves_against_the_inverted_row_space() {
        let d = fw_desc();
        // Screen row for selection 2 is 4 - (2 - 0) - 1 = 1; pressing Down
        // moves the screen row to 2, which is selection 1.
        let (phase, sel, _, _) = flag_window_tick(
            1,
            FlagWindowInput {
                desc: d,
                selection: 2,
                remembered: 9,
                pad: pad(0, crate::world_map_panel::PAD_DOWN),
                ..Default::default()
            },
            |_| false,
        );
        assert_eq!(phase, 1);
        assert_eq!(sel, 1);
    }

    #[test]
    fn flag_window_cancel_writes_the_sentinel() {
        let d = fw_desc();
        let (phase, sel, _, _) = flag_window_tick(
            1,
            FlagWindowInput {
                desc: d,
                selection: 2,
                pad: pad(0x40, 0),
                ..Default::default()
            },
            |_| false,
        );
        assert_eq!((phase, sel), (3, FLAG_WINDOW_CANCEL_SENTINEL));
    }

    // --- FUN_801D0D38 -----------------------------------------------------

    #[test]
    fn hud_is_suppressed_by_each_gate_in_turn() {
        let base = HudInput {
            player_stationary: true,
            timer: 1,
            ..Default::default()
        };
        for gated in [
            HudInput {
                hud_disabled: true,
                ..base
            },
            HudInput {
                view_mode: 2,
                ..base
            },
            HudInput {
                pad: HUD_SUPPRESS_PAD_MASK,
                ..base
            },
            HudInput {
                scratch_suppress: true,
                ..base
            },
        ] {
            assert_eq!(field_hud_tick(gated).1, HudDecision::Suppressed);
        }
    }

    #[test]
    fn hud_rearm_picks_the_view_mode_idle() {
        assert_eq!(
            field_hud_tick(HudInput {
                rearm: true,
                view_mode: 0,
                ..Default::default()
            })
            .1,
            HudDecision::Rearmed {
                timer: HUD_IDLE_FRAMES_NEAR
            }
        );
        assert_eq!(
            field_hud_tick(HudInput {
                rearm: true,
                view_mode: 1,
                short_idle: true,
                ..Default::default()
            })
            .1,
            HudDecision::Rearmed {
                timer: HUD_IDLE_FRAMES_FAR_SHORT
            }
        );
    }

    #[test]
    fn hud_movement_rearms_ignoring_the_short_idle_marker() {
        let d = field_hud_tick(HudInput {
            view_mode: 1,
            short_idle: true,
            player_stationary: false,
            ..Default::default()
        })
        .1;
        assert_eq!(
            d,
            HudDecision::Rearmed {
                timer: HUD_IDLE_FRAMES_FAR
            }
        );
    }

    #[test]
    fn hud_draws_once_the_countdown_expires() {
        let (timer, d) = field_hud_tick(HudInput {
            player_stationary: true,
            timer: 1,
            timer_delta: 1,
            projected_y: Some(0x40),
            ..Default::default()
        });
        assert_eq!(timer, 0);
        assert_eq!(d, HudDecision::Draw { y: HUD_Y_TOP });
    }

    #[test]
    fn hud_dodges_a_player_near_the_screen_top() {
        let (_, d) = field_hud_tick(HudInput {
            player_stationary: true,
            timer: 0,
            timer_delta: 0,
            projected_y: Some(HUD_DODGE_THRESHOLD - 1),
            ..Default::default()
        });
        assert_eq!(d, HudDecision::Draw { y: HUD_Y_BOTTOM });
        let (_, d) = field_hud_tick(HudInput {
            player_stationary: true,
            timer: 0,
            timer_delta: 0,
            projected_y: None,
            ..Default::default()
        });
        assert_eq!(d, HudDecision::Draw { y: HUD_Y_BOTTOM });
    }
}
