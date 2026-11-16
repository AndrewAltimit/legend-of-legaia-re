//! Frame clock: the adaptive frame-step factor and its telemetry, the vsync accumulators, the sim-tick / display-frame counters and play time.
//!
//! One subsystem's state, held as a unit on the composite `World`.
//! Fields keep their retail provenance notes.

/// Frame clock: the adaptive frame-step factor and its telemetry, the vsync accumulators, the sim-tick / display-frame counters and play time.
#[derive(Debug)]
pub struct FrameClock {
    /// Whole seconds of the retail play counter (`0x80084570`, in the save
    /// block at `+0x430`), which counts **vsyncs the game ran**: the frame
    /// pacer adds the frame step `DAT_1F800393` once per game frame while
    /// bit `0x800` of `_DAT_1F800394` is up (`0x80016ED0..0x80016EFC` in
    /// `FUN_80016B6C`). One `World::tick` is one vsync, so the port adds one
    /// tick per sim tick (`World::count_play_tick`) and carries the
    /// sub-second remainder in [`Self::play_tick_phase`]. Persisted in
    /// [`legaia_save::SaveExtV2::play_time_seconds`].
    ///
    /// REF: FUN_80016B6C
    pub play_time_seconds: u32,
    /// The play time the Records page prints (`_DAT_800845DC`, in whole
    /// seconds here): zero until op `4C EE` copies the running counter into
    /// it, in `edlast`. The page's only reader is `FUN_801ED710`.
    pub records_play_seconds: u32,
    /// Ticks (vsyncs) of the play counter not yet folded into
    /// [`Self::play_time_seconds`], `0..60`. Retail keeps the counter in
    /// vsyncs and divides at display time; the port keeps seconds plus this
    /// remainder, which is the same count.
    pub play_tick_phase: u8,
    /// Adaptive frame-step factor `dt` - the retail scratchpad byte
    /// `DAT_1F800393`, the number of *vsyncs per game tick*. The frame-flip
    /// path (`FUN_80016B6C`, see `ghidra/scripts/funcs/80016b6c.txt`) rewrites
    /// it every frame from the measured frame cost (`1`, `2` past `0xF0`, `3`
    /// past `0x1FE`, `4` past `0x2D0`), clamped up to the per-mode floor
    /// `_DAT_8007B9D8`. Live poll baselines: field/town scenes run at `2`
    /// (30 fps) and the overworld kingdom scenes (`mapNN`) at `3` (20 fps) -
    /// the engine pins those per-scene values on entry
    /// (`legaia_engine_scene_host::scene_host::SceneHost::enter_field_scene`) rather than modelling
    /// the load-adaptive writer. Consumed by everything that advances
    /// per-game-tick in vsync units - the scripted CLUT fades
    /// (`World::step_clut_fx`) and the shell's CLUT-cycle cadence.
    ///
    /// REF: FUN_80016B6C
    pub frame_step: u8,
    /// Retail `DAT_8007B9D8` - the per-mode **floor** under
    /// [`FrameClock::frame_step`], installed by the mode/scene loader and never by
    /// the frame driver. `FUN_80016B6C` applies it as a minimum (`slt` plus a
    /// store taken only when the adaptive value is *below* it), so it raises
    /// the cadence and never caps it. Kept separate from `frame_step` because
    /// folding the two lets a single slow frame ratchet the floor upward
    /// permanently.
    ///
    /// REF: FUN_80016B6C, FUN_801D6704
    pub frame_step_floor: u8,
    /// The frame time, in hblanks, a host hands the adaptive resolver for
    /// every game frame from here on (`World::seed_frame_cost_history`), or
    /// `None` for the deterministic arm, whose step is the one installed.
    ///
    /// Retail measures each frame (`VSync(1)` through `FUN_800173BC`) and
    /// steps by the longest of the last sixteen, so a slow stretch a capture
    /// was saved after - a load, a heavy shot - holds the step up for as many
    /// frames as its samples stay in the ring and then lets it fall. The
    /// engine has no frame time to measure; with the ring a capture held and
    /// the cost its newest frames ran at, [`Self::frame_step_telemetry`]
    /// replays that fall.
    ///
    /// REF: FUN_80016B6C
    pub frame_cost: Option<i32>,
    /// The vsyncs the frame pacer waits before the next game frame while
    /// [`Self::frame_cost`] is set: the step of the frame that just ran.
    /// Retail waits `VSync(0x1F800392)` at the end of a frame and only then
    /// stores the step it resolved there (`0x8001719C`, `0x800171D8`), so the
    /// gap after a frame is that frame's own step and the step resolved at
    /// its end is the next frame's ([`Self::next_frame_step`]).
    ///
    /// REF: FUN_80016B6C
    pub pass_wait: Option<u8>,
    /// The step the last game frame resolved at its end, installed as
    /// [`Self::frame_step`] when the next one opens.
    pub next_frame_step: Option<u8>,
    /// Set to request that the next per-frame mode handler skip its frame.
    ///
    /// Retail's frame-begin pass `FUN_8001698C` returns `1` when `gp+0x3D8`
    /// is set and neither `_DAT_8007B938` nor `gp+0x55C` carries bit `0x800`;
    /// its caller (the per-frame mode handler, `legaia_engine_mode::mode::per_frame_stage`)
    /// then abandons the frame after a pad poll and a `VSync(0)` - no
    /// mid-frame driver, no frame-end pass. Consumed (and cleared) by
    /// `legaia_engine_mode::mode::ModeDriver::tick` via `World::take_frame_begin_skip`.
    ///
    /// Defaults to `false`; a host that never sets it gets the pre-existing
    /// tick-every-frame behaviour - and that is also what **retail** does.
    /// The flag has no retail producer that a shipped disc can reach: a
    /// five-form sweep plus the `gp`-relative sweep over `SCUS_942.54` and
    /// every based overlay image finds exactly three sites touching
    /// `gp+0x3D8`, and two are clears - the mode-change edge's
    /// (`0x800161E8`, which `legaia_engine_mode::mode::ModeSeat` performs) and a reset
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
    /// `World::resolve_frame_step`; a host with no frame-time telemetry
    /// leaves it untouched and keeps the deterministic floor.
    pub frame_step_telemetry: crate::actor_tick::FrameStepTelemetry,
    /// Vsyncs accumulated toward the next **actor** game tick. Same clock as
    /// `AmbientFxState::clut_vsync_accum` and the same law - retail resolves one
    /// `DAT_1F800393` per frame and runs the actor pool once per game tick,
    /// so the per-actor physics / anim / motion passes fire once every
    /// [`FrameClock::frame_step`] vsyncs rather than once per rendered frame. The
    /// tick that fires carries [`FrameClock::frame_step`] into the dispatcher's
    /// scalars ([`crate::actor_tick::TickScalars::for_cadence`]),
    /// which is what keeps wall-clock durations identical while the pose
    /// sample rate drops.
    ///
    /// REF: FUN_80016B6C (cadence resolver), FUN_801D6704 (field floor = 2)
    pub actor_vsync_accum: u8,
    /// Whether the **actor game tick** fired on the current world tick - the
    /// one pass in every [`FrameClock::frame_step`] vsyncs on
    /// which retail runs the actor pool. Consumers that retail reaches from an
    /// actor handler rather than from the vsync-rate frame pump sample on it:
    /// the region encounter reader `FUN_801D9E1C` is called from the entity
    /// handler `FUN_801DA51C`'s state 0 (`jal 0x801D9E1C` at `0x801DA5B0`), so
    /// it sees the player's tile once per game tick, never per vsync.
    ///
    /// REF: FUN_801DA51C
    pub game_tick_fired: bool,
    /// Whether the current world tick ran the game-tick clock at all - a tick
    /// in a mode whose handler runs the master frame driver. While it is up,
    /// [`FrameClock::game_tick_fired`] says which vsyncs are retail game
    /// frames, and a field-VM op that credits `DAT_1F800393` credits
    /// [`FrameClock::frame_step`] on those and nothing between them
    /// ([`FrameClock::script_frame_delta`]). Down before the first tick and
    /// under a mode with no actor pass, where a script step counts one vsync.
    pub game_tick_live: bool,
    /// Monotonic count of sim ticks that ran, advanced once per
    /// `World::tick`. It is the world's cheapest "a frame actually ran"
    /// witness - the mode driver's frame-begin-skip test probes it to tell an
    /// abandoned frame from a live one.
    ///
    /// Historically this was a fixed-point phase accumulator bridging a
    /// claimed 100 Hz sim to retail's 60 Hz display frame. No host ever ticked
    /// at 100 Hz, so the phase only ever *withheld* retail frames; with the
    /// 1:1 denomination (see `World::tick`) there is no phase left to carry.
    pub sim_ticks: u32,
    /// Monotonic count of retail display frames elapsed. Consumers that have
    /// to advance something in retail-frame time (the renderer's cutscene
    /// camera glide, whose `apply_trigger` is a duration in display frames)
    /// diff this rather than counting sim ticks.
    ///
    /// Under the 1:1 denomination this equals `World::frame`; it stays a
    /// separate counter because it names a *unit* (retail display frames) that
    /// the sim-tick counter does not promise.
    pub display_frames: u64,
    /// The display frames the game frames have credited: what a clock
    /// retail runs once a game frame and steps by `DAT_1F800393` reads
    /// ([`Self::count_credited_frame`]). Equal to [`Self::display_frames`]
    /// until a frame outlasts its step ([`frame_wall_vsyncs`]); the vsyncs
    /// past the step credit nothing, so a cutscene camera glide lands that
    /// much later on the wall clock, as retail's does.
    pub credited_frames: u64,
    /// Vsyncs since the current game frame opened (`0` on its first).
    pub frame_vsync: u8,
    /// Whether this world tick's display frame was credited
    /// ([`Self::count_credited_frame`]).
    pub frame_credited: bool,
    /// `1` on every sim tick that maps to a retail display frame - which, under
    /// the 1:1 denomination `World::tick` documents, is every sim tick.
    ///
    /// Consumers gate on it to say "this is retail-frame paced": the narration
    /// roller (whose scroll speed is pinned as 1 px per 6 frames at 60 Hz), the
    /// effect pool, the escape timer, the CLUT / ambient game-tick banks, the
    /// timed sound release, and the field-NPC motion legs. It is a *unit*
    /// marker, not a throttle - a host that re-introduced oversampling would
    /// make it selective again without any of those call sites changing.
    pub display_frame_step: u16,
    /// A replay's retail battle frame step and the action-SM state it takes
    /// effect in (`World::seed_battle_frame_step`): `(step, state)`. Not a
    /// retail field - retail measures its step per frame
    /// (`BattleFrameClock`); the retail-compare
    /// drive seeds the one a capture's frame-time ring reads.
    pub battle_frame_step_seed: Option<(u8, u8)>,
}

impl FrameClock {
    pub fn new() -> Self {
        Self {
            play_time_seconds: 0,
            records_play_seconds: 0,
            play_tick_phase: 0,
            // Field/town baseline; scene entry re-pins (`mapNN` -> 3).
            frame_step: 2,
            frame_step_floor: 2,
            frame_cost: None,
            pass_wait: None,
            next_frame_step: None,
            frame_begin_skip: false,
            frame_step_telemetry: crate::actor_tick::FrameStepTelemetry::new(),
            actor_vsync_accum: 0,
            game_tick_fired: false,
            game_tick_live: false,
            // Every sim tick is a retail display frame under the 1:1
            // denomination, so there is no phase to prime: a world that ticks
            // exactly once advances the roller and the retail-frame-paced
            // record contexts by exactly one frame.
            sim_ticks: 0,
            display_frame_step: 0,
            display_frames: 0,
            credited_frames: 0,
            frame_vsync: 0,
            frame_credited: true,
            battle_frame_step_seed: None,
        }
    }

    /// The vsyncs between two game frames: [`Self::pass_wait`] under a
    /// seeded frame cost, else [`Self::frame_step`]. Never zero.
    pub fn game_frame_wait(&self) -> u8 {
        self.pass_wait.unwrap_or(self.frame_step).max(1)
    }

    /// Open a game frame under a seeded frame cost: install the step the
    /// frame before resolved, make the frame's length the wait that follows
    /// it ([`frame_wall_vsyncs`]), and resolve the next frame's step off the
    /// ring with this frame's cost in it. No-op without a seeded cost.
    ///
    /// REF: FUN_80016B6C (`0x80017044..0x800171D8`)
    pub fn open_costed_game_frame(&mut self) {
        let Some(cost) = self.frame_cost else {
            return;
        };
        if let Some(step) = self.next_frame_step.take() {
            self.frame_step = step;
        }
        self.pass_wait = Some(frame_wall_vsyncs(self.frame_step, cost));
        self.next_frame_step = Some(
            self.frame_step_telemetry
                .resolve(cost, true, self.frame_step_floor)
                .vsyncs_per_tick(),
        );
    }

    /// Drop a seeded frame cost: the step stays where it is and the
    /// deterministic arm holds it.
    pub fn clear_frame_cost(&mut self) {
        self.frame_cost = None;
        self.pass_wait = None;
        self.next_frame_step = None;
    }

    /// What a field-VM op that reads `DAT_1F800393` credits on this world
    /// tick.
    ///
    /// Retail runs each script context once per game frame and adds the
    /// frame's length in vsyncs to whatever it is timing: op `0x4A`
    /// `WAIT_FRAMES` does `ctx[+0x54] += DAT_1F800393` on every visit and
    /// passes on the first visit whose sum reaches the operand
    /// (`0x801E09CC..0x801E0A00`). The visit that reaches the op is credited
    /// too, so a wait of `N` at step `s` holds its context for
    /// `s * (ceil(N / s) - 1)` vsyncs - `27` for `4A 1E 00` at step `3`, not
    /// `30` and not `29`.
    ///
    /// The engine steps a context on the vsyncs that are game frames
    /// ([`Self::script_slice_due`]), so it credits the whole step on those
    /// ([`Self::game_tick_fired`]) and nothing on the ones between - the
    /// per-vsync legs and pager that read this clock between two game
    /// frames spend nothing there. One vsync a step while the game-tick
    /// clock is not running ([`Self::game_tick_live`]).
    ///
    /// REF: FUN_801DE840 (op 0x4A, `0x801E09CC`), FUN_80016B6C (the step)
    pub fn script_frame_delta(&self) -> u16 {
        if !self.game_tick_live {
            1
        } else if self.game_tick_fired {
            u16::from(self.frame_step.max(1))
        } else {
            0
        }
    }

    /// Count this world tick's display frame against the game frame it
    /// falls in: one credited frame while the frame is within its step,
    /// none on the vsyncs a long frame runs past it. Every tick credits one
    /// while the game-tick clock is down.
    ///
    /// REF: FUN_801DC0BC (the camera mover adds `DAT_1F800393` once a
    /// frame), FUN_8005FB84
    pub fn count_credited_frame(&mut self) {
        self.frame_credited = if self.game_tick_live {
            self.frame_vsync = if self.game_tick_fired {
                0
            } else {
                self.frame_vsync.saturating_add(1)
            };
            self.frame_vsync < self.frame_step.max(1)
        } else {
            true
        };
        if self.frame_credited {
            self.credited_frames += 1;
        }
    }

    /// Whether a field script context runs its slice on this world tick.
    ///
    /// Retail runs every script context from the actor-list walk
    /// (`FUN_8002519C` -> `FUN_8003BC08` -> the dispatcher `FUN_801DE840`),
    /// and the walk runs once per **game frame**, not per vsync: a
    /// PCSX-Redux count of dispatcher entries against vsyncs
    /// (`scripts/pcsx-redux/autorun_field_script_cadence.lua`) finds every
    /// entry on a vsync the walk ran on, one walk every `DAT_1F800393`
    /// vsyncs, at frame steps 2, 3 and 4 alike. So a context steps on the
    /// vsyncs that are game frames ([`Self::game_tick_fired`]) and on none
    /// between them; every tick steps while the game-tick clock is down.
    ///
    /// REF: FUN_8002519C, FUN_8003BC08, FUN_801DE840
    pub fn script_slice_due(&self) -> bool {
        !self.game_tick_live || self.game_tick_fired
    }
}

/// Hblanks a vsync holds: the NTSC field's `263` lines, which is what libetc
/// `VSync(1)` counts the frame time in (root counter 1 on hblank).
pub const HBLANKS_PER_VSYNC: i32 = 263;

/// The vsyncs a game frame lasts at step `step` when its work costs
/// `cost_hblanks`.
///
/// The pacer ends each frame with libetc `VSync(step)` (`jal 0x8005FB84` at
/// `0x800171C0`). `VSync(n)` waits for the vblank count to reach the count
/// its previous call returned on plus `n - 1` (`FUN_8005FCCC` at
/// `0x8005FC14`) and then for one more vblank past wherever the count
/// stands (`0x8005FC38`), so a frame whose work outlasts its step ends on
/// the first vblank after the work: `max(step, cost / 263 + 1)`. A frame
/// past four vsyncs therefore lasts longer than the step it credits.
///
/// REF: FUN_8005FB84, FUN_80016B6C
pub fn frame_wall_vsyncs(step: u8, cost_hblanks: i32) -> u8 {
    let step = step.max(1);
    if cost_hblanks < i32::from(step) * HBLANKS_PER_VSYNC {
        return step;
    }
    let ran = cost_hblanks / HBLANKS_PER_VSYNC + 1;
    step.max(ran.clamp(1, i32::from(u8::MAX)) as u8)
}

impl Default for FrameClock {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A script step credits the whole game frame on the vsync that is one,
    /// nothing between two, and one vsync while the game-tick clock is down.
    #[test]
    fn script_frame_delta_follows_the_game_tick() {
        let mut c = FrameClock::new();
        c.frame_step = 3;
        assert_eq!(c.script_frame_delta(), 1, "clock not running");
        c.game_tick_live = true;
        assert_eq!(c.script_frame_delta(), 0, "between game frames");
        c.game_tick_fired = true;
        assert_eq!(c.script_frame_delta(), 3, "on the game frame");
        // Over two frames of `frame_step` vsyncs the credit is that many.
        let mut sum = 0;
        for vsync in 0..6 {
            c.game_tick_fired = vsync % 3 == 2;
            sum += c.script_frame_delta();
        }
        assert_eq!(sum, 6);
    }

    /// A frame whose work fits its step lasts the step; one that runs past
    /// it ends on the vblank after the work.
    #[test]
    fn a_frame_lasts_its_step_or_its_work() {
        assert_eq!(frame_wall_vsyncs(3, 616), 3);
        assert_eq!(frame_wall_vsyncs(4, 1051), 4);
        assert_eq!(frame_wall_vsyncs(3, 1940), 8);
        assert_eq!(frame_wall_vsyncs(4, 1796), 7);
        assert_eq!(frame_wall_vsyncs(2, 789), 4);
    }

    /// A frame priced past its step lasts its work, and the spike steps the
    /// frame after it at `4` and leaves the clamp in the ring.
    #[test]
    fn a_long_frame_waits_its_work_and_steps_the_next_at_four() {
        let mut c = FrameClock::new();
        c.frame_cost = Some(400);
        c.open_costed_game_frame();
        assert_eq!(c.game_frame_wait(), 2);
        assert_eq!(c.next_frame_step, Some(2));
        c.frame_cost = Some(1940);
        c.open_costed_game_frame();
        assert_eq!(c.game_frame_wait(), 8);
        assert_eq!(c.next_frame_step, Some(4));
        c.frame_cost = Some(600);
        c.open_costed_game_frame();
        assert_eq!(c.frame_step, 4);
        assert_eq!(c.game_frame_wait(), 4);
        assert_eq!(c.next_frame_step, Some(3));
    }

    /// A frame that lasts eight vsyncs at step three credits three of them.
    #[test]
    fn a_long_frame_credits_its_step() {
        let mut c = FrameClock::new();
        c.frame_step = 3;
        c.game_tick_live = true;
        for v in 0..8 {
            c.game_tick_fired = v == 0;
            c.count_credited_frame();
        }
        assert_eq!(c.credited_frames, 3);
        c.game_tick_fired = true;
        c.count_credited_frame();
        assert_eq!(c.credited_frames, 4);
    }
}
