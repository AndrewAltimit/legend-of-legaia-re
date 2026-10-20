//! The field HUD actor: idle timing and the dodge decision.

// ---------------------------------------------------------------------------
// FUN_801D0D38 - field party HUD
// ---------------------------------------------------------------------------

/// Idle frames before the HUD appears under Field HP Display **Immediate**
/// (`_DAT_800845C4 == 0`).
pub const HUD_IDLE_FRAMES_NEAR: i16 = 0x28;

/// Idle frames before it appears under **Gradual** (`_DAT_800845C4 == 1`).
pub const HUD_IDLE_FRAMES_FAR: i16 = 0xA0;

/// Shortened idle when `_DAT_8007B5F4 == 1` under Gradual.
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
    /// `_DAT_800845C4` - the options screen's Field HP Display row
    /// (Immediate / Gradual / Display Off). `2` suppresses.
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
    /// `_DAT_1F800393` - the countdown's per-frame decrement. Retail reads it
    /// as `lbu v1,0x7f(t1)` off the scratchpad base `t1 = 0x1F800314`
    /// (`0x801D0EF8`); the same cell drives the soft-reset actor's counter at
    /// `0x801EE02C`. An earlier note said `0x1F80038F`, which is four bytes
    /// short of the address the load forms.
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
/// Wired: `PanelActorHost::tick_party_hud` in
/// `legaia_engine_world_views::world_map_panel_host` runs it every overworld frame,
/// caching the player position itself. Retail installs the handler in the
/// field band; the engine drives it from the overworld tick, where the party
/// panel and the player marker both live. `projected_y` stays `None` there -
/// the host has no screen projection for the player - which is retail's own
/// staged-load path and forces the low panel.
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
