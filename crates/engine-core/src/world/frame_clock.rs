//! Frame clock: the adaptive frame-step factor and its telemetry, the vsync accumulators, the sim-tick / display-frame counters and play time.
//!
//! Split out of the composite [`World`] so the state one subsystem owns
//! reads as one unit. Fields keep their retail provenance notes.

use super::*;

/// Frame clock: the adaptive frame-step factor and its telemetry, the vsync accumulators, the sim-tick / display-frame counters and play time.
pub struct FrameClock {
    /// Total game time in wall-clock seconds since the world was
    /// instantiated or loaded. Engines tick this independently of
    /// `frame` (which can pause-skip during dialogs / cutscenes).
    /// Persisted in [`legaia_save::SaveExtV2::play_time_seconds`].
    pub play_time_seconds: u32,
    /// Adaptive frame-step factor `dt` - the retail scratchpad byte
    /// `DAT_1F800393`, the number of *vsyncs per game tick*. The frame-flip
    /// path (`FUN_80016B6C`, see `ghidra/scripts/funcs/80016b6c.txt`) rewrites
    /// it every frame from the measured frame cost (`1`, `2` past `0xF0`, `3`
    /// past `0x1FE`, `4` past `0x2D0`), clamped up to the per-mode floor
    /// `_DAT_8007B9D8`. Live poll baselines: field/town scenes run at `2`
    /// (30 fps) and the overworld kingdom scenes (`mapNN`) at `3` (20 fps) -
    /// the engine pins those per-scene values on entry
    /// ([`crate::scene::SceneHost::enter_field_scene`]) rather than modelling
    /// the load-adaptive writer. Consumed by everything that advances
    /// per-game-tick in vsync units - the scripted CLUT fades
    /// ([`Self::step_clut_fx`]) and the shell's CLUT-cycle cadence.
    ///
    /// REF: FUN_80016B6C
    pub frame_step: u8,
    /// Retail `DAT_8007B9D8` - the per-mode **floor** under
    /// [`crate::world::FrameClock::frame_step`], installed by the mode/scene loader and never by
    /// the frame driver. `FUN_80016B6C` applies it as a minimum (`slt` plus a
    /// store taken only when the adaptive value is *below* it), so it raises
    /// the cadence and never caps it. Kept separate from `frame_step` because
    /// folding the two lets a single slow frame ratchet the floor upward
    /// permanently.
    ///
    /// REF: FUN_80016B6C, FUN_801D6704
    pub frame_step_floor: u8,
    /// Set to request that the next per-frame mode handler skip its frame.
    ///
    /// Retail's frame-begin pass `FUN_8001698C` returns `1` when `gp+0x3D8`
    /// is set and neither `_DAT_8007B938` nor `gp+0x55C` carries bit `0x800`;
    /// its caller (the per-frame mode handler, [`crate::mode::per_frame_stage`])
    /// then abandons the frame after a pad poll and a `VSync(0)` - no
    /// mid-frame driver, no frame-end pass. Consumed (and cleared) by
    /// [`crate::mode::ModeDriver::tick`] via [`World::take_frame_begin_skip`].
    ///
    /// Defaults to `false`; a host that never sets it gets the pre-existing
    /// tick-every-frame behaviour - and that is also what **retail** does.
    /// The flag has no retail producer that a shipped disc can reach: a
    /// five-form sweep plus the `gp`-relative sweep over `SCUS_942.54` and
    /// every based overlay image finds exactly three sites touching
    /// `gp+0x3D8`, and two are clears - the mode-change edge's
    /// (`0x800161E8`, which [`crate::mode::ModeSeat`] performs) and a reset
    /// path's (`0x8001E100`). The one **setter** is
    /// `_DAT_8007B6F0 = ~_DAT_8007B6F0` at `0x80018850`, the R1+Start pause
    /// toggle in `FUN_8001822C`'s dev-hotkey tail, and that whole tail sits
    /// behind `_DAT_8007B98C != 0` (`beq` at `0x800185FC`), which is zero on
    /// retail. So this is a *debug pause* channel, and the port's own debug
    /// surface - not a missing engine wire - is what would set it.
    ///
    /// REF: FUN_8001698C
    /// REF: FUN_8001822C - the dev-hotkey tail that owns the only setter.
    pub frame_begin_skip: bool,
    /// Retail's frame-time history behind the adaptive cadence
    /// (`DAT_80084098[16]` + `0x1F800392`). Only advanced when a host calls
    /// [`World::resolve_frame_step`]; a host with no frame-time telemetry
    /// leaves it untouched and keeps the deterministic floor.
    pub frame_step_telemetry: vm::actor_tick::FrameStepTelemetry,
    /// Vsyncs accumulated toward the next **actor** game tick. Same clock as
    /// [`crate::world::AmbientFxState::clut_vsync_accum`] and the same law - retail resolves one
    /// `DAT_1F800393` per frame and runs the actor pool once per game tick,
    /// so the per-actor physics / anim / motion passes fire once every
    /// [`crate::world::FrameClock::frame_step`] vsyncs rather than once per rendered frame. The
    /// tick that fires carries [`crate::world::FrameClock::frame_step`] into the dispatcher's
    /// scalars ([`legaia_engine_vm::actor_tick::TickScalars::for_cadence`]),
    /// which is what keeps wall-clock durations identical while the pose
    /// sample rate drops.
    ///
    /// REF: FUN_80016B6C (cadence resolver), FUN_801D6704 (field floor = 2)
    pub actor_vsync_accum: u8,
    /// Monotonic count of sim ticks that ran, advanced once per
    /// [`Self::tick`]. It is the world's cheapest "a frame actually ran"
    /// witness - the mode driver's frame-begin-skip test probes it to tell an
    /// abandoned frame from a live one.
    ///
    /// Historically this was a fixed-point phase accumulator bridging a
    /// claimed 100 Hz sim to retail's 60 Hz display frame. No host ever ticked
    /// at 100 Hz, so the phase only ever *withheld* retail frames; with the
    /// 1:1 denomination (see [`Self::tick`]) there is no phase left to carry.
    pub sim_ticks: u32,
    /// Monotonic count of retail display frames elapsed. Consumers that have
    /// to advance something in retail-frame time (the renderer's cutscene
    /// camera glide, whose `apply_trigger` is a duration in display frames)
    /// diff this rather than counting sim ticks.
    ///
    /// Under the 1:1 denomination this equals [`Self::frame`]; it stays a
    /// separate counter because it names a *unit* (retail display frames) that
    /// the sim-tick counter does not promise.
    pub display_frames: u64,
    /// `1` on every sim tick that maps to a retail display frame - which, under
    /// the 1:1 denomination [`Self::tick`] documents, is every sim tick.
    ///
    /// Consumers gate on it to say "this is retail-frame paced": the narration
    /// roller (whose scroll speed is pinned as 1 px per 6 frames at 60 Hz), the
    /// effect pool, the escape timer, the CLUT / ambient game-tick banks, the
    /// timed sound release, and the field-NPC motion legs. It is a *unit*
    /// marker, not a throttle - a host that re-introduced oversampling would
    /// make it selective again without any of those call sites changing.
    pub display_frame_step: u16,
}

impl FrameClock {
    pub fn new() -> Self {
        Self {
            play_time_seconds: 0,
            // Field/town baseline; scene entry re-pins (`mapNN` -> 3).
            frame_step: 2,
            frame_step_floor: 2,
            frame_begin_skip: false,
            frame_step_telemetry: vm::actor_tick::FrameStepTelemetry::new(),
            actor_vsync_accum: 0,
            // Every sim tick is a retail display frame under the 1:1
            // denomination, so there is no phase to prime: a world that ticks
            // exactly once advances the roller and the retail-frame-paced
            // record contexts by exactly one frame.
            sim_ticks: 0,
            display_frame_step: 0,
            display_frames: 0,
        }
    }
}

impl Default for FrameClock {
    fn default() -> Self {
        Self::new()
    }
}
