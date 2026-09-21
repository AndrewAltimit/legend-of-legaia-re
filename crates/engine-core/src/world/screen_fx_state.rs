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
    /// [`crate::world::World::tick`]; dropped when the ramp completes. Hosts draw an
    /// overlay from [`crate::fade::FadeState::rgb`] while this is `Some`.
    pub fade: Option<crate::fade::FadeState>,
    /// The live **screen-effect colour tween** the field VM's op `0x34`
    /// sub-0 arm installs - the pool slot retail keeps in `_DAT_8007B62C`.
    ///
    /// The op is a walk-out/walk-in pair, not a value ramp: it retires
    /// whatever slot this names, spawns a tween from the *previous* target
    /// down to black with a one-frame hold, and then spawns the new one from
    /// black up to its operand RGB with a `-1` hold. Both tweens emit
    /// `FUN_80024EE4` pushes, which is the one representation of this
    /// effect - see [`crate::world::World::screen_tint_pushes`].
    pub effect_tween_slot: Option<usize>,
    /// The target RGB the last op `0x34` sub-0 latched - retail's
    /// `_DAT_8007BCCD/CE/CF`, read by the *next* instruction as the walk-out
    /// tween's start colour.
    pub effect_target_rgb: [i16; 3],
    /// Template `[0]`, the tween's **blend** - retail `_DAT_8007BCE0`,
    /// written `(op0 & 1) != 0 ? 2 : 1` at `0x801DFD7C..0x801DFD8C`.
    pub effect_blend: i16,
    /// The spawner's `a1`, the push's screen-effect **kind** - retail
    /// `_DAT_8007BCCC`, written `8` when `op0 & 2`, else `0` when `op0 & 4`,
    /// else `2` (`0x801DFD90..0x801DFDBC`).
    pub effect_kind: i16,
    /// Global multiply screen tint (op `0x4C 0x12` → `DAT_8007BCB8/B9/BA`,
    /// neutral operand `0x80`, stored normalized; ramp via `FUN_8003C5F0`).
    /// The scene-entry fade-in from black - every field scene `P1[0]`'s
    /// `0x52F` arrival arm: `4C 12 00 00 00 00 00` (instant black) then
    /// `4C 12 80 80 80 44 00` (ramp to neutral over 68 frames) - lives here.
    /// Persists across scene changes (retail's cross-scene fade continuity:
    /// a departure fade-to-black carries into the next scene's fade-in).
    /// Stepped once per [`crate::world::World::tick`]; dropped once neutral.
    pub tint: Option<crate::fade::SceneTintRamp>,
    /// Screen-effect widget host (the PROT-0900 mask / sprite / panel /
    /// letterbox family), driven by the field-VM op `0x43` sub-ops
    /// `0x10`/`0x11`/`0x13`/`0x14`/`0x15` - the ending-scene widget
    /// path. See [`crate::screen_fx`].
    pub fx: crate::screen_fx::ScreenFxHost,
    /// The current frame's widget draw list, refreshed by the Field /
    /// Cutscene tick while any widget is live ([`crate::world::World::tick_screen_fx`]).
    /// Renderers composite these 2D overlays above the scene.
    pub fx_frame: crate::screen_fx::ScreenFxFrame,
    /// The live cinematic bar emitter (field-VM op `0x43` sub-`0xC`, retail
    /// template `0x801F2858` / tick `FUN_801DD784`). One at a time, because
    /// its spawner is the one op that allocates it and its envelope retires
    /// itself; [`crate::world::World::tick_field_timer_actors`] steps it and
    /// [`crate::world::ScreenFxState::cinematic_bar`] is what the two hosts draw from.
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
            effect_tween_slot: None,
            effect_target_rgb: [0; 3],
            effect_blend: 0,
            effect_kind: 0,
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
