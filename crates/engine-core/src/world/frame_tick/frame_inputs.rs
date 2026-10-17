//! Per-frame inputs to the tick: the clear colour, the frame-step resolve and
//! vsync wait, the RNG draws, the pad latch and the frame-begin skip.

use super::*;
use legaia_engine_mode::mode::SceneMode;

impl World {
    /// The colour the frame is cleared to this tick - the one both hosts hand
    /// their clear (`battle_stage_clear::scene_clear`'s field input).
    ///
    /// The field's draw-environment clear (`presentation.clear_rgb`, op
    /// `4C 13`) outside four minigame modes. Under the slot machine, Baka
    /// Fighter, the Muscle Dome and the dance the frame clears to **black**:
    /// every retail state of those four holds `r0 / g0 / b0 = 0` at
    /// `0x8007BF5D..5F` and a `0x0000` display background around its HUD
    /// (`minigame_slot_machine`, `minigame_baka_fighter`,
    /// `minigame_muscle_dome`, `minigame_dance_noa`), entered from a town
    /// whose own states carry a non-zero clear. The port suspends the field
    /// instead of reloading over it, so without this the town's colour
    /// showed behind the cabinet and the attract card. Fishing keeps the
    /// field's word: its venue (`other1`) writes its own sky colour, which
    /// the retail fishing state carries.
    pub fn frame_clear_rgb(&self) -> [u8; 3] {
        match self.mode {
            SceneMode::SlotMachine
            | SceneMode::BakaFighter
            | SceneMode::MuscleDome
            | SceneMode::Dance => [0; 3],
            _ => self.presentation.clear_rgb,
        }
    }

    /// Consume the frame-begin skip request, returning whether this frame
    /// should be abandoned. Models `FUN_8001698C`'s non-zero return; see
    /// [`legaia_engine_vm::frame_clock::FrameClock::frame_begin_skip`].
    ///
    /// PORT: FUN_8001698c (the frame-skip return; the ring-aging half of the
    /// same function is `legaia_engine_audio::sfx_ring::SfxCueRing::age`)
    pub fn take_frame_begin_skip(&mut self) -> bool {
        std::mem::take(&mut self.clock.frame_begin_skip)
    }

    /// Resolve this frame's cadence the way `FUN_80016B6C` does and install
    /// it into [`legaia_engine_vm::frame_clock::FrameClock::frame_step`].
    ///
    /// PORT: FUN_80016b6c (the `0x80017044 .. 0x800171D8` cadence block; the
    /// telemetry state machine lives in
    /// [`legaia_engine_vm::actor_tick::FrameStepTelemetry`]).
    ///
    /// `elapsed_hblanks` is the frame time retail samples with `VSync(1)`
    /// through `FUN_800173BC`. The floor is [`legaia_engine_vm::frame_clock::FrameClock::frame_step_floor`]
    /// (`DAT_8007B9D8`), installed per scene, and the resolver can only raise
    /// the cadence above it - never below.
    ///
    /// **Hosts that want determinism should not call this.** Retail gates the
    /// whole adaptive path on a boot config word (`gp+0x4CE == 0x10`); with
    /// `frameskip_enabled = false` this returns the floor unchanged, which is
    /// exactly what the replay / trace oracles need. Wall-clock-paced hosts
    /// pass their measured frame time and `true`.
    ///
    /// REPLACED-BY: the fixed-rate simulation plus the scene-installed floor
    /// [`legaia_engine_vm::frame_clock::FrameClock::frame_step_floor`], which is what this
    /// resolves to on every frame that keeps up with vsync.
    ///
    /// The adaptive term is the running maximum of the last sixteen
    /// `VSync(1)` hblank samples against thresholds just under one, two and
    /// three NTSC fields, so it only rises above `1` on a frame the renderer
    /// missed. The engine's simulation never misses one - its tick is
    /// scheduled, not measured - so the resolver's answer is the floor the
    /// scene loader already installed, and nothing samples `FUN_800173BC`'s
    /// hblank count to ask it otherwise. The replay / trace oracles rely on
    /// that: a wall-clock-fed cadence would make them non-deterministic.
    pub fn resolve_frame_step(&mut self, elapsed_hblanks: i32, frameskip_enabled: bool) -> u8 {
        let cadence = self.clock.frame_step_telemetry.resolve(
            elapsed_hblanks,
            frameskip_enabled,
            self.clock.frame_step_floor,
        );
        self.clock.frame_step = cadence.vsyncs_per_tick();
        self.clock.frame_step
    }

    /// The `VSync(n)` argument retail would pass this frame - **last** frame's
    /// cadence, with `< 2` passed as `0` (`0x8001719C`, read before the new
    /// value is written back at `0x800171D8`).
    ///
    /// REF: FUN_80016B6C
    pub fn frame_step_vsync_wait(&self) -> u8 {
        self.clock.frame_step_telemetry.vsync_wait()
    }

    /// Increment the deterministic LCG and return the new **raw** state.
    ///
    /// Not a retail draw: a port of a `jal 0x80056798` site goes through
    /// [`Self::next_rand`]. The raw word's low bits have short periods.
    pub fn next_rng(&mut self) -> u32 {
        // The BIOS `rand()` step (`world_lcg_step`).
        self.rng_state = legaia_engine_system::rand::world_lcg_step(self.rng_state);
        self.rng_state
    }

    /// One retail **`rand()`** draw: the next [`Self::next_rng`] state shaped
    /// the way BIOS `A(2Fh)` (`FUN_80056798`) shapes its own, `0..=0x7FFF`
    /// ([`legaia_engine_system::rand::bios_rand_shape`]).
    ///
    /// A port that stands in for a `jal 0x80056798` should draw through this
    /// rather than the raw state: retail callers test the result's low bits
    /// and divide it as a 15-bit value. Every battle-side consumer draws
    /// here (see `docs/subsystems/battle-formulas.md`, RNG primitive). Retail's `rand()` has one
    /// kernel seed shared by every caller in SCUS and every overlay (the
    /// executable carries no `srand` thunk), which is why this is one world
    /// stream rather than a generator per subsystem.
    pub fn next_rand(&mut self) -> u32 {
        legaia_engine_system::rand::bios_rand_shape(self.next_rng())
    }

    /// Replace the per-frame pad bitmask snapshot. Equivalent to
    /// `self.input.set_pad(mask)` but available without importing
    /// [`input::InputState`] at the call site. Hosts that drive the
    /// world from a scripted timeline (`legaia-engine replay`, the
    /// v0.1 playthrough oracle) call this before each [`Self::tick`].
    /// Also latches [`crate::world::FieldLocomotion::run_button_held`] off the same word, so the
    /// run modifier reaches every host that feeds a pad - native window,
    /// browser play page, replay driver - without any of them wiring it
    /// separately. Deriving it here rather than per-host is deliberate: a
    /// per-host derivation is exactly the shape the UI-drift gate exists to
    /// catch, and this way there is nothing to keep in sync.
    ///
    /// The buttons are [`crate::world::FieldLocomotion::run_button_mask`], which **defaults to
    /// retail's** `Cross | R1` (the config word `0x800846DC` = `0x48`, seeded
    /// by `FUN_80034A6C` and read at `0x801D0364`), plus Square as an
    /// alternate. Square alone was the port's binding for a while and is not
    /// retail's - it is the debug-turbo bit `0x80` on the same selector - so
    /// the default now leads with the retail pair. Rebinding is a *key*
    /// question, not a button one: `legaia-engine config set --binding
    /// W=R1` moves which key produces R1.
    ///
    /// REF: FUN_80034A6C
    pub fn set_pad(&mut self, mask: u16) {
        self.input.set_pad(mask);
        self.locomotion.run_button_held = mask & self.locomotion.run_button_mask != 0;
    }
}
