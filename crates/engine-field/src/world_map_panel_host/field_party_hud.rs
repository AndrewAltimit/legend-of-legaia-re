//! The field party HUD strip: member data, its countdown and the phase hold.

use super::*;

/// `_DAT_1F800394` bit `0x0800_0000`, the menu lock: the HUD's second suppress
/// gate (`0x801D0DA4`, `lui v0,0x800`).
pub const HUD_SCRATCH_SUPPRESS_BIT: u32 = 0x0800_0000;

/// `_DAT_1F800394` bit `0x400`, the system lock: a scene-entry rearm term
/// (`andi v0,a0,0x400` at `0x801D0DE0`).
pub const HUD_SCRATCH_REARM_BIT: u32 = 0x400;

/// One member's row of the field party HUD, as the hosts draw it.
///
/// Plain data so it crosses into the renderer-agnostic UI crate (which this
/// crate never links) without carrying a `World` reference: the native window
/// and the browser play page both map it onto
/// `legaia_engine_ui::field_party_hud::FieldHudMember`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FieldHudMemberData {
    pub name: String,
    pub level: u8,
    pub hp: u16,
    pub hp_max: u16,
    pub mp: u16,
    pub mp_max: u16,
    pub alive: bool,
}

/// Host-side owner of the field party HUD's per-frame state.
///
/// The kernel [`field_hud_tick`] is pure - it takes the countdown and the
/// cached player position and hands them back. Retail keeps them in overlay
/// globals (`_DAT_801F348C` and the `_DAT_801F3488/348A` pair); this is that
/// storage, held by whichever host is drawing.
///
/// Separate from [`PanelActorHost`] on purpose: the panel host only exists
/// while the overworld controller does, and the HUD is a **field** surface -
/// it is up in towns and dungeons too, which is most of the game.
#[derive(Debug, Clone, Default)]
pub struct FieldPartyHud {
    pub(super) timer: i16,
    pub(super) cached: Option<(i16, i16)>,
    last: Option<HudDecision>,
    /// `field_hud_rearm_held` (engine-core) as the previous frame left it - what
    /// retail's routine reads ([`Self::rearm_term`]).
    engaged_prev: bool,
    /// `_DAT_8007B5F4`, the short-idle marker: raised by the encounter
    /// hand-off and cleared by this routine ([`Self::tick_marked`]).
    short_idle: bool,
    /// The world's marker-raise count as of the last tick, so a raise the
    /// world published since is seen once. `None` until the first tick, which
    /// adopts the count without raising anything.
    marker_raises_seen: Option<u32>,
}

impl FieldPartyHud {
    pub fn new() -> Self {
        Self::default()
    }

    /// Last frame's decision, for hosts that tick and draw in separate passes.
    pub fn decision(&self) -> Option<HudDecision> {
        self.last
    }

    /// The player-engaged rearm term **as retail's HUD routine sees it**:
    /// the value `held` (`field_hud_rearm_held` (engine-core)) had at the end of the
    /// previous frame. Returns that and stores this frame's.
    ///
    /// `FUN_801D0D38` runs inside the player's tick `FUN_801D1344`
    /// (`jal 0x801D0D38` at `0x801D1660`), and the player node lives on the
    /// first actor list `FUN_80016444` walks (`_DAT_8007C34C`, `lw a0,0x4(s0)`
    /// / `jal 0x8002519C` at `0x800165A4`) - ahead of the field-object list
    /// `_DAT_8007C354` whose per-actor tick `FUN_8003BC08` steps the script
    /// runner that raises the engaged bit (`0x8003BD34`). So the frame a
    /// record first engages the player still draws the readout: the
    /// `keikoku_chest_open` state holds the bit raised and the countdown
    /// unrearmed at `0`. The port ticks the world first and the HUD after
    /// it, so it asks the term one frame late to keep retail's order; the
    /// release lags the same frame.
    pub fn rearm_term(&mut self, held: bool) -> bool {
        std::mem::replace(&mut self.engaged_prev, held)
    }

    /// Drop the cached position so the next tick takes retail's scene-entry
    /// rearm arm. Hosts call this on a scene change - retail's own rearm
    /// condition includes "the staged scene load is still pending".
    pub fn rearm(&mut self) {
        self.cached = None;
        self.timer = 0;
        self.last = None;
    }

    /// Clamp a running idle countdown to at most `max` frames. A no-op while
    /// the kernel is rearming (no cached position): the capture harness's
    /// phase alignment ([`hud_countdown_cap`]) uses it where the idle is longer
    /// than the run before the capture, so a rearm hold alone cannot bring
    /// the countdown down to the retail value in time.
    pub fn cap_countdown(&mut self, max: i16) {
        if self.cached.is_some() && self.timer > max {
            self.timer = max.max(0);
        }
    }

    /// One frame.
    ///
    /// `hud_disabled` is retail's `_DAT_8007B868` - the "something else owns
    /// the screen" gate; hosts raise it for a menu, a dialog box, a cutscene
    /// or a battle. `view_mode` is `_DAT_800845C4`, the Field HP Display
    /// option (`legaia_engine_world_views::world_map_panel_host::field_hud_view_mode`):
    /// `0` Immediate (0x28-frame idle), `1` Gradual (0xA0), and `2` Display
    /// Off, which suppresses outright. `pad_held` is the **packed** held word, because
    /// the suppress mask is the packed D-pad - a raw word suppresses nothing.
    pub fn tick(
        &mut self,
        hud_disabled: bool,
        view_mode: i32,
        pad_held: u16,
        player_pos: Option<(i16, i16)>,
        timer_delta: i16,
        projected_y: Option<i16>,
    ) -> HudDecision {
        self.tick_inner(
            hud_disabled,
            view_mode,
            pad_held,
            player_pos,
            timer_delta,
            projected_y,
        )
    }

    /// [`Self::tick`] with the short-idle marker `_DAT_8007B5F4` carried.
    ///
    /// `marker_raises` is the world's count of marker raises (the encounter
    /// hand-off, `FUN_801DA51C` at `0x801DA704`, stores `1`); a count that
    /// moved since the last tick raises the marker here. While it is up the
    /// scene-entry arm arms the countdown at `0` under Immediate and `0x50`
    /// under Gradual (`0x801D0E14..0x801D0E70`) instead of `0x28` / `0xA0`.
    /// Retail clears it on every pass that is not the scene-entry arm: the
    /// suppress exit (`sw zero,-0x4a0c(v0)` in the delay slot at
    /// `0x801D0DBC`) and the position compare (`0x801D0EAC`, in the delay
    /// slot of its `bne`, so a moving and a standing player both clear it).
    ///
    /// `resident` is whether the routine runs at all this frame: it lives in
    /// slot A, so a battle, the field-to-battle intro and the overworld map
    /// display (each an image over that slot) neither tick it nor clear the
    /// marker - which is how the marker survives from the encounter to the
    /// first field frame after the fight.
    ///
    /// `scratch` is the scratchpad word `_DAT_1F800394` (the engine's
    /// `StoryFlagState::story_flags`): its bit `0x0800_0000`, the menu lock, is
    /// the routine's second suppress gate (`lw a0,0x80(t1)` off
    /// `t1 = 0x1F800314`, `and` with `lui 0x800` at `0x801D0D98..0x801D0DAC`).
    ///
    /// PORT: FUN_801d0d38 (`_DAT_8007B5F4`: the `0x801D0E30..0x801D0E70`
    /// reads, the `0x801D0DBC` / `0x801D0EAC` clears; the `0x801D0DA0` menu-lock
    /// suppress)
    #[allow(clippy::too_many_arguments)]
    pub fn tick_marked(
        &mut self,
        hud_disabled: bool,
        view_mode: i32,
        pad_held: u16,
        player_pos: Option<(i16, i16)>,
        timer_delta: i16,
        projected_y: Option<i16>,
        marker_raises: u32,
        resident: bool,
        scratch: u32,
    ) -> HudDecision {
        match self.marker_raises_seen {
            Some(seen) if seen != marker_raises => self.short_idle = true,
            _ => {}
        }
        self.marker_raises_seen = Some(marker_raises);
        let rearm_arm = self.cached.is_none() && !hud_disabled;
        let decision = self.tick_inner_with(
            hud_disabled,
            view_mode,
            pad_held,
            player_pos,
            timer_delta,
            projected_y,
            scratch & HUD_SCRATCH_SUPPRESS_BIT != 0,
        );
        let took_entry_arm = rearm_arm && matches!(decision, HudDecision::Rearmed { .. });
        if resident && !took_entry_arm {
            self.short_idle = false;
        }
        decision
    }

    /// `_DAT_8007B5F4` as the last tick left it.
    pub fn short_idle(&self) -> bool {
        self.short_idle
    }

    fn tick_inner(
        &mut self,
        hud_disabled: bool,
        view_mode: i32,
        pad_held: u16,
        player_pos: Option<(i16, i16)>,
        timer_delta: i16,
        projected_y: Option<i16>,
    ) -> HudDecision {
        self.tick_inner_with(
            hud_disabled,
            view_mode,
            pad_held,
            player_pos,
            timer_delta,
            projected_y,
            false,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn tick_inner_with(
        &mut self,
        hud_disabled: bool,
        view_mode: i32,
        pad_held: u16,
        player_pos: Option<(i16, i16)>,
        timer_delta: i16,
        projected_y: Option<i16>,
        scratch_suppress: bool,
    ) -> HudDecision {
        let rearm = self.cached.is_none();
        let stationary = match (self.cached, player_pos) {
            (Some(a), Some(b)) => a == b,
            _ => false,
        };
        let (timer, decision) = field_hud_tick(HudInput {
            hud_disabled,
            view_mode,
            pad: u32::from(pad_held),
            scratch_suppress,
            rearm,
            short_idle: self.short_idle,
            timer: self.timer,
            timer_delta,
            player_stationary: stationary,
            projected_y,
        });
        self.timer = timer;
        // A suppressed frame must NOT refresh the cache: retail's suppress
        // arms return before the position store, so releasing the D-pad after
        // a walk re-enters through the position compare (which rearms the
        // countdown) rather than through the stationary arm (which would let
        // the HUD pop back the instant the stick centred).
        if !matches!(decision, HudDecision::Suppressed) {
            self.cached = player_pos;
        }
        self.last = Some(decision);
        decision
    }
}

/// The other half of the phase alignment, for an idle longer than the run
/// before the capture (the overworld's `0xA0` against a capture at tick
/// `120`): the countdown the kernel may hold **after** host tick `tick` so
/// that it still reads `retail_countdown` at `capture_tick`. `None` past the
/// capture.
pub fn hud_countdown_cap(tick: u64, capture_tick: u64, retail_countdown: i16) -> Option<i16> {
    let left = capture_tick.checked_sub(tick)?;
    let cap = i64::from(retail_countdown.max(0)) + i64::try_from(left).ok()?;
    Some(i16::try_from(cap).unwrap_or(i16::MAX))
}

/// Phase-align the HUD's idle countdown to a retail frame: `true` on every
/// host tick at which the countdown must be rearmed so that, at
/// `capture_tick`, it reads `retail_countdown` - the value of retail's
/// `_DAT_801F348C` in the state being compared.
///
/// A capture harness ticks a fixed settle window, which outlasts the near
/// idle (`0x28` frames), while a retail state is one instant at an arbitrary
/// point of the countdown - a card-load state is typically two or three
/// frames into it. Without the hold, a stationary seat would score the
/// readout the retail frame is still `retail_countdown` frames short of.
/// The hold rearms through tick `capture_tick - (idle - retail_countdown)`;
/// the countdown then runs down one per frame and reads exactly
/// `retail_countdown` at the capture (a `0` countdown is a drawn readout).
pub fn hud_phase_hold(tick: u64, capture_tick: u64, retail_countdown: i16, idle: i16) -> bool {
    let lead =
        u64::try_from(i32::from(idle) - i32::from(retail_countdown.clamp(0, idle))).unwrap_or(0);
    tick + lead <= capture_tick
}
