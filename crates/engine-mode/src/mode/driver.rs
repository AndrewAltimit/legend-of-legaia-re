//! `ModeDriver`: the per-mode handler trait and the driver that runs a mode's init / frame handlers.

use super::*;

/// What a mode handler reports back to the driver after a tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HandlerResult {
    /// Stay in the current mode for another frame (the retail "loop"
    /// pattern - most "MODE" handlers do this).
    Continue,
    /// Transition to the table's `next` entry. If `next` is `None` the
    /// driver leaves the current mode unchanged (matching the retail
    /// next == -1 sentinel).
    Done,
    /// Hard transition to a specific mode. Useful for init handlers that
    /// branch (e.g. main menu chooses CARD vs MAPDISP).
    GoTo(GameMode),
}

/// Trait an engine integration implements to provide per-mode behaviour.
///
/// The default impl makes every mode a no-op `Continue`, so an integration
/// can override only the modes it cares about and let the rest stay in a
/// quiescent state. The retail dispatch is much wider (loads scene
/// assets, drives the field VM, etc.); the trait is the seam to plug
/// those in.
///
/// `ModeDriver::tick` calls these in order: it consults the current
/// mode, calls the matching handler, applies the result.
pub trait ModeHandler<W: ?Sized> {
    fn run(&mut self, mode: GameMode, world: &mut W, input: &InputState) -> HandlerResult {
        let _ = (mode, world, input);
        HandlerResult::Continue
    }
}

/// No-op handler. Useful for tests + integrations that just want the
/// driver to track the current mode without driving anything.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoopHandler;
impl<W: ?Sized> ModeHandler<W> for NoopHandler {}

/// The mode driver. Owns the current-mode register (the engine equivalent
/// of `gp[0x524]`) and a frame counter for diagnostics.
#[derive(Debug)]
pub struct ModeDriver {
    current: GameMode,
    /// Total frames the driver has ticked, across all modes.
    pub frames: u64,
    /// Frames spent in the current mode (resets on transition).
    pub frames_in_mode: u64,
    /// The [`PerFrameStage`] resolved on the last [`Self::tick`], or `None`
    /// when the current mode is an INIT mode. Hosts read it to dispatch the
    /// mode's overlay hook (mode 13's `FUN_801CE850`) and to know which
    /// mid-frame driver retail would have run.
    last_stage: Option<PerFrameStage>,
    /// The warp sub-id register [`WARP_SUB_ID_ADDR`], which the driver has to
    /// carry beside the mode word: the mode word alone cannot say which of the
    /// five minigames modes 24 / 25 are running. `None` until a door-warp
    /// stages one, mirroring the fact that retail's register is only meaningful
    /// once the `0x3E` arm has written it.
    warp_sub_id: Option<i16>,
}

impl ModeDriver {
    /// Boot the driver in `MainInit` (mode 2), matching the retail boot
    /// sequence which jumps to MainInit after `gp` setup completes.
    pub fn new_at_boot() -> Self {
        Self::new(GameMode::MainInit)
    }

    pub fn new(start: GameMode) -> Self {
        Self {
            current: start,
            frames: 0,
            frames_in_mode: 0,
            last_stage: None,
            warp_sub_id: None,
        }
    }

    pub fn current(&self) -> GameMode {
        self.current
    }

    /// The staged warp sub-id ([`WARP_SUB_ID_ADDR`]).
    pub fn warp_sub_id(&self) -> Option<i16> {
        self.warp_sub_id
    }

    /// Stage a warp sub-id, the way the field VM's `0x3E` arm writes
    /// `_DAT_8007BA34` before handing the mode word to `0x18`. Retail never
    /// clears the register, so neither does this - a host that wants it gone
    /// passes `None` explicitly.
    pub fn set_warp_sub_id(&mut self, sub_id: Option<i16>) {
        self.warp_sub_id = sub_id;
    }

    /// The [`SceneMode`] the driver's current `(mode, sub-id)` pair resolves
    /// to. This, not [`GameMode::scene_mode`], is what the driver installs into
    /// the World each frame.
    pub fn scene_mode(&self) -> SceneMode {
        self.current.scene_mode_with_warp(self.warp_sub_id)
    }

    /// The per-frame staging plan resolved on the last [`Self::tick`].
    pub fn last_stage(&self) -> Option<PerFrameStage> {
        self.last_stage
    }

    pub fn entry(&self) -> &ModeEntry {
        &TABLE[self.current.as_index()]
    }

    /// Force a transition to `mode`. Resets `frames_in_mode`.
    pub fn jump_to(&mut self, mode: GameMode) {
        if mode != self.current {
            self.current = mode;
            self.frames_in_mode = 0;
        }
    }

    /// Drive one frame: sync the World's [`SceneMode`] to the current
    /// game mode, call the host's [`ModeHandler::run`], apply the result.
    /// Returns the handler's result so engines that want to act on
    /// transitions can observe them.
    pub fn tick<W: ModeWorld + ?Sized, H: ModeHandler<W>>(
        &mut self,
        host: &mut H,
        world: &mut W,
        input: &InputState,
    ) -> HandlerResult {
        // Keep the World's scene-mode in sync each frame. Cheap and
        // idempotent - the World's tick path keys off it. Resolved from the
        // `(mode, warp sub-id)` PAIR: keying on the mode word alone drops a
        // live fishing / dance / casino / duel / dome session into
        // `SceneMode::Title`, because all five share mode `0x19`.
        world.set_scene_mode(self.scene_mode());
        let r = host.run(self.current, world, input);
        // Retail's per-frame handlers ([`per_frame_stage`]) early-out when the
        // frame-begin pass `FUN_8001698C` returns non-zero: that frame gets a
        // pad poll and a `VSync(0)` and nothing else - no mid-frame driver and
        // no frame-end pass. Honour the same skip here. Only the per-frame
        // (odd-indexed) modes have that shape; INIT modes tick unconditionally.
        let skipped = per_frame_stage(self.current).is_some() && world.take_frame_begin_skip();
        self.last_stage = per_frame_stage(self.current);
        // Mode 0 CONFIG INIT runs the sound detach (`FUN_8002689C`) ahead of
        // its staging - the same call the `runs_core_reset` flag records for
        // `FUN_80025CB4`. The latch makes repeat frames in the mode free.
        if matches!(
            mode_init_stage(self.current),
            Some(ModeInitStage {
                runs_core_reset: true,
                ..
            })
        ) {
            world.detach_sound();
        }
        if !skipped {
            // Tick the World after the handler, so a Continue runs the VMs
            // for this mode every frame. Init modes that flip to the run
            // mode via Done get one final World tick before transitioning.
            world.tick_frame();
        }
        self.frames += 1;
        self.frames_in_mode += 1;
        match r {
            HandlerResult::Continue => {}
            HandlerResult::Done => {
                if let Some(next) = self.entry().next {
                    self.jump_to(next);
                }
            }
            HandlerResult::GoTo(mode) => self.jump_to(mode),
        }
        r
    }
}

impl Default for ModeDriver {
    fn default() -> Self {
        Self::new_at_boot()
    }
}
