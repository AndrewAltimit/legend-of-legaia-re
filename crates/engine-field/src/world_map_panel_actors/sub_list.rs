//! The sub-list actor.

use super::*;

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
    /// arithmetic shift both ways. The cell is the **live audio level** (every
    /// traced reader is a volume setter), so this is the sub-window ducking
    /// the mix by half and restoring it - not a screen dim, which rides the
    /// separate accumulator `_DAT_8007B440`.
    ScaleAudioLevel { shift_right: bool },
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
/// Wired: `PanelActorKind::SubList` in
/// `legaia_engine_world_views::world_map_panel_host`; it is the entry point the
/// world-map screen's chord installs, and the one whose state-3 hand-off
/// reaches the travel art.
pub fn sub_list_tick(phase: i16, input: SubListInput) -> (i16, i32, Vec<SubListEffect>) {
    let mut phase = phase;
    let mut cursor = input.cursor;
    let mut out = Vec::new();
    match phase {
        0 => {
            out.push(SubListEffect::ScaleAudioLevel { shift_right: true });
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
            out.push(SubListEffect::ScaleAudioLevel { shift_right: false });
            out.push(SubListEffect::Exit(ActorExit {
                saved_handler: input.handler_id,
                next_handler: SUBLIST_NEXT_HANDLER,
            }));
            phase = 0;
        }
        3 => {
            out.push(SubListEffect::ScaleAudioLevel { shift_right: false });
            out.push(SubListEffect::HandOff);
        }
        _ => {}
    }
    out.push(SubListEffect::TickTextActors);
    (phase, cursor, out)
}
