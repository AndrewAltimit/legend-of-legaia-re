//! Full-screen presentation state: fades, tints, the screen-effect widget host and the cinematic bars.
//!
//! Split out of the composite [`World`] so the state one subsystem owns
//! reads as one unit. Fields keep their retail provenance notes.

use super::*;

/// Full-screen presentation state: fades, tints, the screen-effect widget host and the cinematic bars.
pub struct ScreenFxState {
    /// Last fade colour requested by move-VM ext sub-op 0x3C - engines
    /// drain this each frame to drive the screen fade. `None` when no
    /// fade is pending.
    pub pending_fade: Option<FadeRequest>,
    /// Active full-screen fade, staged by the battle SM's escape teardown
    /// (retail state `0x66` spawns the `DAT_801C9070` black→white ramp via
    /// the fade-primitive spawner `FUN_80024E80`). Stepped once per
    /// [`World::tick`]; dropped when the ramp completes. Hosts draw an
    /// overlay from [`crate::fade::FadeState::rgb`] while this is `Some`.
    pub fade: Option<crate::fade::FadeState>,
    /// Effect-layer global colour (op `0x34` sub-0, `FUN_801E1FB0`; neutral
    /// operand `0xFF`, stored normalized). The opening timeline ramps it in
    /// the crawl gaps (`34 05 00 00 00 D2 00` = to black over 210 frames,
    /// `34 01 FF FF FF 00 00` = instant neutral). Stepped once per
    /// [`World::tick`]; dropped once it lands on the neutral identity.
    /// **Not a screen fade**: the retail cold-boot capture holds the lit
    /// villager tableau across the span where the timeline's black ramp
    /// would blank a full-screen fade, so this value feeds the effect layer
    /// (the creation-glow planes; consumer still an open thread) and stays
    /// out of [`World::scene_screen_tint`]. Scene-local: reset on scene
    /// entry. Distinct from [`Self::screen_fade`] (the battle escape ramp).
    pub effect_tint: Option<crate::fade::SceneTintRamp>,
    /// Global multiply screen tint (op `0x4C 0x12` → `DAT_8007BCB8/B9/BA`,
    /// neutral operand `0x80`, stored normalized; ramp via `FUN_8003C5F0`).
    /// The scene-entry fade-in from black - every field scene `P1[0]`'s
    /// `0x52F` arrival arm: `4C 12 00 00 00 00 00` (instant black) then
    /// `4C 12 80 80 80 44 00` (ramp to neutral over 68 frames) - lives here.
    /// Persists across scene changes (retail's cross-scene fade continuity:
    /// a departure fade-to-black carries into the next scene's fade-in).
    /// Stepped once per [`World::tick`]; dropped once neutral.
    pub tint: Option<crate::fade::SceneTintRamp>,
    /// Screen-effect widget host (the PROT-0900 mask / sprite / panel /
    /// letterbox family), driven by the field-VM op `0x43` sub-ops
    /// `0x10`/`0x11`/`0x13`/`0x14`/`0x15` - the ending-scene widget
    /// path. See [`crate::screen_fx`].
    pub fx: crate::screen_fx::ScreenFxHost,
    /// The current frame's widget draw list, refreshed by the Field /
    /// Cutscene tick while any widget is live ([`Self::tick_screen_fx`]).
    /// Renderers composite these 2D overlays above the scene.
    pub fx_frame: crate::screen_fx::ScreenFxFrame,
    /// The live cinematic bar emitter (field-VM op `0x43` sub-`0xC`, retail
    /// template `0x801F2858` / tick `FUN_801DD784`). One at a time, because
    /// its spawner is the one op that allocates it and its envelope retires
    /// itself; [`World::tick_field_timer_actors`] steps it and
    /// [`Self::cinematic_bar`] is what the two hosts draw from.
    pub cinematic_bars: Option<legaia_engine_vm::field_actor_timers::ShutterBars>,
    /// This frame's bar height in scanlines, republished every tick so a
    /// renderer reads a value rather than re-stepping the envelope.
    pub cinematic_bar: i16,
}

impl ScreenFxState {
    pub fn new() -> Self {
        Self {
            pending_fade: None,
            fade: None,
            effect_tint: None,
            tint: None,
            fx: Default::default(),
            fx_frame: Default::default(),
            cinematic_bars: None,
            cinematic_bar: 0,
        }
    }
}

impl Default for ScreenFxState {
    fn default() -> Self {
        Self::new()
    }
}
