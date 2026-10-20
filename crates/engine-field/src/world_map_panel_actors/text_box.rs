//! The text-box (confirm / decline) actor.

use super::*;

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
    /// `rec[+0x104] -> rec[+0x106]` and `rec[+0x108] -> rec[+0x10A]`.
    ///
    /// Retail addresses these off the save block (`0x6CC -> 0x6CE`,
    /// `0x6D0 -> 0x6D2` from `0x80084140`); rebasing by the `0x5C8`
    /// block-to-record distance lands on the record offsets above, which
    /// `legaia_save`'s schema names `hp_max -> hp_cur` and `mp_max -> mp_cur`.
    /// So this arm is a **full HP/MP restore** of the first three characters.
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
/// Wired: `PanelActorKind::TextBox` in
/// `legaia_engine_world_views::world_map_panel_host`; its confirm arm's party
/// restore is applied to the live records by `World::restore_party_hp_mp`.
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
