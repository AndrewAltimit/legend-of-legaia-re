//! The per-frame scalars, the frame-step cadence and its telemetry.

/// Global tick scalars.
///
/// The retail dispatcher reads `DAT_1F800393` (per-frame delta multiplier)
/// and `DAT_1F80037D` (game-speed multiplier). Both are unsigned bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TickScalars {
    pub frame_delta: u8,
    pub speed: u8,
}

impl TickScalars {
    /// Idle-frame scalars (`frame_delta = 1`, `speed = 1`). Most of the
    /// dispatcher arithmetic devolves to a straight add at this rate.
    pub const fn idle() -> Self {
        Self {
            frame_delta: 1,
            speed: 1,
        }
    }

    /// `frame_delta * speed` - the multiplier the dispatcher applies in
    /// nearly every arm.
    pub fn product(self) -> u32 {
        u32::from(self.frame_delta) * u32::from(self.speed)
    }

    /// Scalars for a resolved retail cadence: `frame_delta` is the vsyncs
    /// this game tick spans, `speed` the game-speed multiplier.
    pub const fn for_cadence(cadence: FrameCadence, speed: u8) -> Self {
        Self {
            frame_delta: cadence.vsyncs_per_tick(),
            speed,
        }
    }
}

/// The retail logic-tick cadence: how many vsyncs one game tick spans.
///
/// This is `DAT_1F800393`, written once per frame by `FUN_80016B6C` (see
/// `ghidra/scripts/funcs/80016b6c.txt`). It is **not** a fixed constant -
/// retail resolves it every frame as
///
/// ```text
/// adaptive = if frameskip_enabled && worst_frame_hblanks > 0xF0 {
///     if worst > 0x2D0 { 4 } else if worst > 0x1FE { 3 } else { 2 }
/// } else { 1 };
/// DAT_1F800393 = max(adaptive, DAT_8007B9D8);   // per-mode floor
/// ```
///
/// Two independent inputs, and both matter:
///
/// 1. **Adaptive frame-skip.** `FUN_800173BC` returns `VSync(1)` - the
///    elapsed time of the frame just rendered, in hblank (scanline) units.
///    `FUN_80016B6C` keeps a 16-entry ring of those samples at
///    `DAT_80084098` and takes the running **maximum**, so the skip factor
///    is sticky against the worst of the last 16 frames. The thresholds
///    `0xF0 / 0x1FE / 0x2D0` (240 / 510 / 720) sit just under 1 / 2 / 3
///    NTSC fields (263 hblanks each): this is classic "we missed vsync, so
///    advance the sim proportionally and keep wall-clock speed constant".
///    On hardware keeping up it resolves to `1`. It is gated on a boot-time
///    config word (`gp+0x4CE == 0x10`), read at exactly this one site.
/// 2. **Per-mode floor** `DAT_8007B9D8`. This is the part that gives the
///    engine a *deterministic* cadence to match, because it is installed by
///    mode, not by performance. [`FrameCadence::MODE_FLOORS`] tabulates the
///    installers found in the overlay dumps.
///
/// ## Why this does not disturb duration-based parity
///
/// Everything that measures a *duration* in retail accumulates
/// `DAT_1F800393` rather than `1` - the camera mover's
/// `t = min(t + DAT_1F800393, d)` is the canonical example. That makes
/// those systems **cadence-invariant**: a glide with `apply = 600` reaches
/// its target after 600 *vsyncs* whether the sim ticks at cadence 1 (600
/// ticks x 1) or cadence 2 (300 ticks x 2). So retail durations are
/// denominated in vsyncs, and a port running at cadence 1 arrives at the
/// same wall-clock moment.
///
/// What differs is the **sample rate**, not the duration: at cadence 2
/// retail only produces a pose every second vsync, so a port ticking every
/// vsync renders intermediate poses retail never shows. That is the whole
/// of the field-motion divergence - smoother-than-retail motion between
/// identical endpoints.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct FrameCadence(u8);

impl FrameCadence {
    /// One game tick per vsync (~60 Hz). The adaptive default when the
    /// machine is keeping up and no mode floor is installed.
    pub const FULL: Self = Self(1);

    /// One game tick per 2 vsyncs (~30 Hz) - the floor the field scene
    /// loader installs, and therefore the cadence of ordinary field play.
    pub const FIELD: Self = Self(2);

    /// Per-mode `DAT_8007B9D8` floors recovered from the overlay dumps, as
    /// `(installer, floor)`. The value is a floor, not an assignment: the
    /// adaptive factor still raises it when frames run long.
    ///
    /// The menu family (`FUN_801DC6B4` / `FUN_801DE234` / `FUN_801DD35C`)
    /// uses a save/restore idiom - it drops the floor to `1` on entry and
    /// writes the saved value back (`DAT_801EF19C`) on exit - which is why
    /// the field floor survives a pause-menu round trip. Cutscene dialogue
    /// (`FUN_801D362C`) takes its floor from a script operand at
    /// `param_2 + 4`, so that one is data-driven and has no fixed entry.
    ///
    /// `FUN_801C6C78` is **not** on this list, and should not be re-added: it
    /// installs no floor. Its 441-instruction disassembly writes exactly one
    /// global - `0x8007AA14`, via 17 copies of `sw t0,-0x55ec(at)` - and
    /// contains no store to `DAT_8007B9D8` at all. The `_DAT_8007b9d8 = 2`
    /// that once put it here appears only in the *decompiled* half of
    /// `overlay_0896_801c6c78.txt`, spliced in from the field-overlay bytes
    /// PROT 0896's footprint over-reads into; it is `FUN_801D6704`'s store,
    /// already tabulated above. See `docs/subsystems/actor-vm.md`.
    pub const MODE_FLOORS: &'static [(&'static str, u8)] = &[
        // Field scene loader - ordinary field/town play.
        // Store `sw s0,-0x4628(v0)` at 0x801D6990, with `li s0,0x2` at
        // 0x801D6988 (overlay_0897, base 0x801CE818).
        ("FUN_801D6704", 2),
        // Field->battle intro transition.
        ("FUN_801CFDA0", 3),
        // Menu family: drops to 1 on entry, restores DAT_801EF19C on exit.
        ("FUN_801DC6B4", 1),
        ("FUN_801DE234", 1),
        // Baka Fighter: 1 for the duel proper, 4 for the scripted beat.
        ("FUN_801CF678", 1),
    ];

    /// Build a cadence from a raw `DAT_1F800393` value, clamped to the
    /// `1..=4` range retail can produce.
    pub const fn from_raw(raw: u8) -> Self {
        Self(if raw < 1 {
            1
        } else if raw > 4 {
            4
        } else {
            raw
        })
    }

    /// Vsyncs spanned by one game tick.
    pub const fn vsyncs_per_tick(self) -> u8 {
        self.0
    }

    /// Resolve the cadence the way `FUN_80016B6C` does: the adaptive
    /// frame-skip factor raised to the active per-mode floor.
    ///
    /// `worst_frame_hblanks` is the running max over the last 16 frames of
    /// `VSync(1)`; `frameskip_enabled` mirrors the `gp+0x4CE == 0x10` gate.
    /// A port with no frame-time telemetry passes `frameskip_enabled =
    /// false`, which reduces this to "use the mode floor" - the
    /// deterministic, replay-safe choice.
    pub const fn resolve(
        frameskip_enabled: bool,
        worst_frame_hblanks: u16,
        mode_floor: u8,
    ) -> Self {
        let adaptive = if frameskip_enabled && worst_frame_hblanks > 0xF0 {
            if worst_frame_hblanks > 0x2D0 {
                4
            } else if worst_frame_hblanks > 0x1FE {
                3
            } else {
                2
            }
        } else {
            1
        };
        Self::from_raw(if mode_floor > adaptive {
            mode_floor
        } else {
            adaptive
        })
    }
}

/// The retail frame-time telemetry that feeds [`FrameCadence::resolve`].
///
/// PORT: FUN_80016b6c - the cadence half of the frame-end driver
/// (`0x80017044 .. 0x800171D8`: the `gp+0x5D8` forced arm, [`Self::forced`], and the adaptive block), read off the disassembly
/// (`see ghidra/scripts/funcs/80016b6c.txt`) and cross-checked block-for-block
/// against the static-recomp rendering of `func_80016B6C`.
///
/// [`FrameCadence::resolve`] models the *ladder*; this models the **state
/// behind it**, which is what makes the cadence sticky rather than
/// instantaneous. Retail keeps three cells:
///
/// | Retail cell | Here |
/// |---|---|
/// | `DAT_80084098[16]` - 16 `i16` frame-time samples | [`Self::history`] |
/// | `gp+0x440` - the ring write cursor, masked `& 0xF` at the *top* of the next frame | [`Self::cursor`] |
/// | `0x1F800392` - last frame's resolved cadence, the `VSync` argument | [`Self::previous`] |
///
/// Three details in the instruction stream that a "16-sample running max"
/// summary loses, and all three change the resolved cadence:
///
/// 1. **Samples are clamped on the way in.** `slti v0,s0,0x2d0 / li s0,0x2bc`
///    at `0x8001709C` stores `0x2BC` (700) for any sample `>= 0x2D0` (720).
///    So the *history* can never on its own reach the top rung - a stored
///    spike ages out as cadence 3, not 4.
/// 2. **The current sample overrides the max.** At `0x800170F8` the raw
///    (unclamped) sample replaces the ring maximum when it is `>= 0x2D0`.
///    Combined with (1) this is the whole point of the clamp: a `>= 720`
///    frame resolves to cadence 4 **on that frame only**, then decays to 3.
/// 3. **The vsync wait lags one frame.** `0x1F800392` is read for the
///    `VSync(n)` argument *before* the newly resolved cadence is written back
///    to it (`0x800171D8`), and a value `< 2` is passed as `0`. So the frame
///    that detects a slow frame still waits at the old cadence.
///
/// The gate itself is `gp+0x4CE == 0x10` (`0x80017108`) - one comparison, one
/// site - and the floor is applied as a *minimum*: `slt` + a store taken only
/// when the resolved value is **below** `DAT_8007B9D8`. It raises the cadence,
/// it never caps it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameStepTelemetry {
    history: [i16; Self::HISTORY],
    cursor: usize,
    previous: u8,
}

impl FrameStepTelemetry {
    /// Ring length - retail's `sltiu v0,s5,0x10` loop bound.
    pub const HISTORY: usize = 16;

    /// The sample value stored in place of anything `>= [`Self::SPIKE`]`
    /// (retail `li s0,0x2bc`).
    pub const CLAMPED: i16 = 0x2BC;

    /// Samples at or above this are spikes: clamped on store, and used raw
    /// in place of the ring maximum on the frame they occur.
    pub const SPIKE: i32 = 0x2D0;

    /// Fresh telemetry: an all-zero history and a previous cadence of 1
    /// (retail's cold `0x1F800392`).
    pub const fn new() -> Self {
        Self {
            history: [0; Self::HISTORY],
            cursor: 0,
            previous: 1,
        }
    }

    /// One frame of `FUN_80016B6C`'s cadence block.
    ///
    /// `elapsed_hblanks` is the frame time `FUN_800173BC` returns (retail's
    /// `VSync(1)`); `frameskip_enabled` is the `gp+0x4CE == 0x10` gate;
    /// `mode_floor` is `DAT_8007B9D8`. Returns the newly resolved cadence -
    /// the value retail writes to `DAT_1F800393`.
    pub fn resolve(
        &mut self,
        elapsed_hblanks: i32,
        frameskip_enabled: bool,
        mode_floor: u8,
    ) -> FrameCadence {
        // 0x8001709C: clamp on the way into the ring.
        let stored = if elapsed_hblanks < Self::SPIKE {
            elapsed_hblanks as i16
        } else {
            Self::CLAMPED
        };
        self.history[self.cursor] = stored;
        // 0x800170C8: the cursor advances unmasked and is masked `& 0xF` at
        // the top of the next frame (0x8001704C) - equivalent to wrapping here.
        self.cursor = (self.cursor + 1) % Self::HISTORY;

        // 0x800170D0: running maximum over all 16 slots, including the one
        // just written.
        let mut worst = 0i32;
        for &s in &self.history {
            if i32::from(s) > worst {
                worst = i32::from(s);
            }
        }
        // 0x800170F8: a spiking *current* sample replaces the max outright.
        if elapsed_hblanks >= Self::SPIKE {
            worst = elapsed_hblanks;
        }

        let cadence = FrameCadence::resolve(
            frameskip_enabled,
            worst.clamp(0, i32::from(u16::MAX)) as u16,
            mode_floor,
        );
        // 0x800171D8: the resolved cadence becomes next frame's VSync arg.
        self.previous = cadence.vsyncs_per_tick();
        cadence
    }

    /// The forced-cadence arm that precedes the adaptive block
    /// (`0x80017050..0x80017090`). When the word `gp+0x5D8` is non-zero,
    /// retail stores its **low byte** straight into `DAT_1F800393`, calls
    /// `DrawSync(0)` (`FUN_80058104`), waits `VSync(n)` with `n = forced`
    /// except that `1` is passed as `0`, and jumps to `0x800171DC` - past
    /// the ring write, the maximum, the ladder and the `0x1F800392` store.
    /// So a forced frame leaves [`Self::history`] and [`Self::previous`]
    /// exactly as they were.
    ///
    /// Returns `None` when `forced == 0` (the adaptive block runs), else
    /// `(frame step, VSync argument)`.
    pub const fn forced(forced: u32) -> Option<(u8, u32)> {
        if forced == 0 {
            return None;
        }
        let vsync = if forced == 1 { 0 } else { forced };
        Some(((forced & 0xFF) as u8, vsync))
    }

    /// One frame of the whole cadence block: the forced arm when
    /// `forced != 0` ([`Self::forced`]), else [`Self::resolve`]. Returns the
    /// value retail leaves in `DAT_1F800393`.
    pub fn step(
        &mut self,
        forced: u32,
        elapsed_hblanks: i32,
        frameskip_enabled: bool,
        mode_floor: u8,
    ) -> u8 {
        match Self::forced(forced) {
            Some((step, _vsync)) => step,
            None => self
                .resolve(elapsed_hblanks, frameskip_enabled, mode_floor)
                .vsyncs_per_tick(),
        }
    }

    /// The `VSync(n)` argument for the frame just resolved: last frame's
    /// cadence, with `< 2` passed as `0` (`0x8001719C`).
    ///
    /// Call this **before** [`Self::resolve`] to get retail's ordering - the
    /// wait happens on the previous frame's cadence.
    pub const fn vsync_wait(&self) -> u8 {
        if self.previous < 2 { 0 } else { self.previous }
    }

    /// Last frame's resolved cadence (retail `0x1F800392`).
    pub const fn previous(&self) -> u8 {
        self.previous
    }
}

impl Default for FrameStepTelemetry {
    fn default() -> Self {
        Self::new()
    }
}

impl Default for FrameCadence {
    fn default() -> Self {
        Self::FIELD
    }
}

#[cfg(test)]
mod cadence_tests {
    use super::*;

    /// With frame-skip disabled the cadence is exactly the mode floor -
    /// the deterministic path a replay-safe port takes.
    #[test]
    fn floor_alone_drives_cadence_when_frameskip_off() {
        assert_eq!(FrameCadence::resolve(false, 0, 1), FrameCadence::FULL);
        assert_eq!(FrameCadence::resolve(false, 0, 2), FrameCadence::FIELD);
        // A long frame cannot raise the cadence while the gate is closed.
        assert_eq!(FrameCadence::resolve(false, 0x400, 1), FrameCadence::FULL);
        assert_eq!(FrameCadence::resolve(false, 0x400, 2), FrameCadence::FIELD);
    }

    /// The adaptive ladder reproduces `FUN_80016B6C`'s thresholds exactly:
    /// `<=0xF0 -> 1`, `>0xF0 -> 2`, `>0x1FE -> 3`, `>0x2D0 -> 4`.
    #[test]
    fn adaptive_ladder_matches_retail_thresholds() {
        let c = |w| FrameCadence::resolve(true, w, 1).vsyncs_per_tick();
        assert_eq!(c(0x00), 1);
        assert_eq!(c(0xF0), 1, "threshold is strict >");
        assert_eq!(c(0xF1), 2);
        assert_eq!(c(0x1FE), 2, "threshold is strict >");
        assert_eq!(c(0x1FF), 3);
        assert_eq!(c(0x2D0), 3, "threshold is strict >");
        assert_eq!(c(0x2D1), 4);
        assert_eq!(c(0xFFFF), 4, "saturates at 4");
    }

    /// `DAT_1F800393 = max(adaptive, floor)` - the floor raises a fast
    /// frame, and a slow frame raises past the floor.
    #[test]
    fn cadence_is_max_of_adaptive_and_floor() {
        // Fast frames, field floor -> floor wins.
        assert_eq!(FrameCadence::resolve(true, 0x10, 2), FrameCadence::FIELD);
        // Slow frames, field floor -> adaptive wins.
        assert_eq!(FrameCadence::resolve(true, 0x300, 2).vsyncs_per_tick(), 4);
        // Battle-intro floor of 3 beats a mid adaptive of 2.
        assert_eq!(FrameCadence::resolve(true, 0x150, 3).vsyncs_per_tick(), 3);
    }

    /// `gp+0x5D8 != 0` forces the step from its low byte and skips the ring
    /// entirely - a forced frame neither records a sample nor moves the
    /// previous cadence.
    #[test]
    fn a_forced_cadence_bypasses_the_telemetry() {
        assert_eq!(FrameStepTelemetry::forced(0), None);
        assert_eq!(
            FrameStepTelemetry::forced(1),
            Some((1, 0)),
            "VSync(1) is passed as 0"
        );
        assert_eq!(FrameStepTelemetry::forced(3), Some((3, 3)));
        assert_eq!(FrameStepTelemetry::forced(0x102), Some((2, 0x102)));
        let mut t = FrameStepTelemetry::new();
        let before = t;
        assert_eq!(t.step(4, 0x400, true, 1), 4);
        assert_eq!(t, before, "no ring write, no previous-cadence store");
        assert_eq!(
            t.step(0, 0x400, true, 1),
            4,
            "the adaptive block when not forced"
        );
        assert_ne!(t, before);
    }

    /// A spike `>= 0x2D0` reaches the top rung on the frame it happens and
    /// then decays to 3, because the history stores the clamped `0x2BC`
    /// (`0x8001709C`) while the ladder sees the raw sample (`0x800170F8`).
    #[test]
    fn a_spike_hits_cadence_four_only_on_its_own_frame() {
        let mut t = FrameStepTelemetry::new();
        assert_eq!(t.resolve(0x400, true, 1).vsyncs_per_tick(), 4);
        // The spike is in the history now, but clamped to 700 - which is
        // below the `> 0x2D0` rung.
        assert_eq!(t.resolve(0x10, true, 1).vsyncs_per_tick(), 3);
    }

    /// The history is a running maximum, so one slow frame keeps the cadence
    /// raised for the next 16 frames and then releases it.
    #[test]
    fn the_history_max_is_sticky_for_exactly_sixteen_frames() {
        let mut t = FrameStepTelemetry::new();
        assert_eq!(t.resolve(0x200, true, 1).vsyncs_per_tick(), 3);
        for i in 0..(FrameStepTelemetry::HISTORY - 1) {
            assert_eq!(
                t.resolve(0, true, 1).vsyncs_per_tick(),
                3,
                "frame {i} should still see the sample in the ring"
            );
        }
        assert_eq!(
            t.resolve(0, true, 1).vsyncs_per_tick(),
            1,
            "the sample has now been overwritten"
        );
    }

    /// The floor raises, never caps - and with the adaptive gate off the
    /// resolver is the floor, which is what makes replay deterministic.
    #[test]
    fn telemetry_respects_the_floor_and_the_gate() {
        let mut t = FrameStepTelemetry::new();
        assert_eq!(t.resolve(0x400, false, 2).vsyncs_per_tick(), 2);
        assert_eq!(t.resolve(0x10, true, 3).vsyncs_per_tick(), 3);
        assert_eq!(
            t.resolve(0x400, true, 2).vsyncs_per_tick(),
            4,
            "the floor is a minimum, not a maximum"
        );
    }

    /// `VSync(n)` is called with the *previous* frame's cadence, and `< 2`
    /// goes in as `0`.
    #[test]
    fn the_vsync_wait_lags_one_frame() {
        let mut t = FrameStepTelemetry::new();
        assert_eq!(t.vsync_wait(), 0, "cold previous cadence of 1 -> 0");
        t.resolve(0x400, true, 1);
        assert_eq!(t.previous(), 4);
        assert_eq!(t.vsync_wait(), 4, "the wait now reflects the slow frame");
        t.resolve(0, true, 1);
        assert_eq!(t.vsync_wait(), 3, "one frame behind the decayed cadence");
    }

    /// Durations are denominated in vsyncs, so a `t += cadence` accumulator
    /// arrives after the same number of vsyncs at every cadence. This is
    /// why the camera-mover oracle stays frame-exact across a cadence
    /// change - it is the invariant the retail code buys by adding
    /// `DAT_1F800393` instead of `1`.
    #[test]
    fn duration_accumulators_are_cadence_invariant() {
        const APPLY: u32 = 600;
        for cadence in [1u32, 2, 3, 4] {
            let (mut t, mut vsyncs) = (0u32, 0u32);
            while t < APPLY {
                t = (t + cadence).min(APPLY);
                vsyncs += cadence;
            }
            assert_eq!(
                vsyncs, APPLY,
                "cadence {cadence} must arrive after exactly {APPLY} vsyncs"
            );
        }
    }

    /// The field floor really is 2 - the claim under test. Sourced from
    /// `FUN_801D6704`, the field scene loader.
    #[test]
    fn field_scene_loader_installs_a_two_vsync_floor() {
        let field = FrameCadence::MODE_FLOORS
            .iter()
            .find(|(f, _)| *f == "FUN_801D6704")
            .expect("field scene loader floor is tabulated");
        assert_eq!(field.1, 2);
        assert_eq!(
            FrameCadence::resolve(false, 0, field.1),
            FrameCadence::FIELD
        );
    }

    #[test]
    fn scalars_carry_the_cadence_into_the_dispatcher_multiplier() {
        let s = TickScalars::for_cadence(FrameCadence::FIELD, 1);
        assert_eq!(s.frame_delta, 2);
        assert_eq!(s.product(), 2);
        assert_eq!(
            TickScalars::for_cadence(FrameCadence::FULL, 1),
            TickScalars::idle()
        );
    }

    #[test]
    fn raw_values_clamp_into_the_retail_range() {
        assert_eq!(FrameCadence::from_raw(0).vsyncs_per_tick(), 1);
        assert_eq!(FrameCadence::from_raw(9).vsyncs_per_tick(), 4);
        assert_eq!(FrameCadence::from_raw(3).vsyncs_per_tick(), 3);
    }
}
