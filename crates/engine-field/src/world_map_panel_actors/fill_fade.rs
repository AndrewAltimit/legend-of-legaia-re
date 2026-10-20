//! The fill-fade actor.

use super::*;

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
/// Wired: `PanelActorKind::FillFade` in
/// `legaia_engine_world_views::world_map_panel_host`.
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
