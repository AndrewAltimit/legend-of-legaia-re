//! The soft-reset slide actor.

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
    /// `FUN_80042558(actor)` - the arm call in state 0 (`jal` at
    /// `0x801EDF58`, `a0` = the actor; the `0x20` belongs to the state 1-3
    /// paths).
    ArmReset,
    /// `_DAT_8007B792 += 1` then `FUN_801ED710(0x20, y)` - the records-screen
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
/// Wired: `PanelActorKind::SoftReset` in
/// `legaia_engine_world_views::world_map_panel_host`. The host records the
/// executable reload rather than performing it - there is no boot image to
/// re-load into.
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
