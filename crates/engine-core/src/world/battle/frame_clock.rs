//! The **battle frame**: how the engine's one-vsync ticks group into the
//! frames retail runs the battle on.
//!
//! Retail runs one battle pass a frame and spans each frame with the frame
//! step `DAT_1F800393` vsyncs. `FUN_80016B6C` measures that step every frame
//! from the frame's own cost: the newest `VSync(1)` hblank sample goes into
//! the sixteen-entry ring `0x80084098` (index `gp+0x440`, a sample of
//! `0x2D0` or more clamped to `0x2BC`), and the step is the ring's maximum
//! against the thresholds `0xF1` / `0x1FF` / `0x2D1` (`1` / `2` / `3`, else
//! `4`; `0x80017098..0x8001715C`), raised to the per-mode floor
//! `0x8007B9D8`. The battle's floor is `1` in every catalogued battle state,
//! so its step is the load alone: `2` on an ordinary frame (one field plus
//! the miss), `3` while a summon's creature and effects fill the frame
//! (`theeder_summon_mid_cast`'s ring peaks at `625`, `gimard_burning_attack`'s
//! sits at the `700` clamp). It is not a property of the game state, so the
//! engine runs the default step ([`DEFAULT_BATTLE_FRAME_STEP`]) and only a
//! replay that knows retail's step installs another.
//!
//! The engine ticks once a vsync and keeps every per-vsync rate; what the
//! step changes is where the frame boundaries fall - the root-motion carry's
//! truncation (`RootMotionCarry`) and the camera walker's steps
//! (`BattleCamera::set_frame_step`).
//!
//! REF: FUN_80016B6C

use super::*;

/// The engine's battle frame step: retail's step on an ordinary battle frame,
/// the one the camera and the root-motion carry assume by default.
pub const DEFAULT_BATTLE_FRAME_STEP: u8 = 2;

/// Groups display ticks into battle frames of [`Self::step`] vsyncs.
///
/// The default clock is `display_frames / 2`; a step change starts a fresh
/// frame on the tick it is made and counts from there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BattleFrameClock {
    /// Vsyncs a battle frame spans.
    pub step: u8,
    /// The display tick the current step took effect on.
    origin_tick: u64,
    /// The battle frame id at [`Self::origin_tick`].
    origin_frame: u64,
}

impl Default for BattleFrameClock {
    fn default() -> Self {
        Self {
            step: DEFAULT_BATTLE_FRAME_STEP,
            origin_tick: 0,
            origin_frame: 0,
        }
    }
}

impl BattleFrameClock {
    /// The battle frame display tick `tick` falls in.
    pub fn frame_of(&self, tick: u64) -> u64 {
        self.origin_frame + tick.saturating_sub(self.origin_tick) / u64::from(self.step.max(1))
    }

    /// Run frames of `step` vsyncs from display tick `tick` on, the first one
    /// opening on `tick`.
    pub fn set_step(&mut self, tick: u64, step: u8) {
        let step = step.max(1);
        if step == self.step {
            return;
        }
        let next = self.frame_of(tick) + 1;
        *self = Self {
            step,
            origin_tick: tick,
            origin_frame: next,
        };
    }
}

impl World {
    /// The battle frame the current tick falls in
    /// ([`BattleFrameClock::frame_of`] at `display_frames`).
    pub fn battle_frame_id(&self) -> u64 {
        self.battle.frame_clock.frame_of(self.clock.display_frames)
    }

    /// Run the battle on frames of `vsyncs` from this tick on: the
    /// root-motion carry's frames and the camera's steps. A replay sets it
    /// from the step retail ran at; play keeps [`DEFAULT_BATTLE_FRAME_STEP`].
    pub fn set_battle_frame_step(&mut self, vsyncs: u8) {
        let tick = self.clock.display_frames;
        self.battle.frame_clock.set_step(tick, vsyncs);
        if let Some(cam) = self.battle.camera.as_mut() {
            cam.set_frame_step(vsyncs);
        }
    }

    /// Seed a replay's battle frame step: `step` vsyncs a frame on every tick
    /// the action SM sits in `state`, the default everywhere else
    /// ([`crate::world::FrameClock::battle_frame_step_seed`]).
    pub fn seed_battle_frame_step(&mut self, step: u8, state: u8) {
        self.clock.battle_frame_step_seed = Some((step.max(1), state));
    }

    /// Install a seeded step while the action SM sits in its state, and the
    /// default outside it. Called after every action-SM step.
    pub(in crate::world) fn apply_battle_frame_step_seed(&mut self) {
        let Some((step, state)) = self.clock.battle_frame_step_seed else {
            return;
        };
        // Scoped to the state: the ring a capture holds speaks for its own
        // state's frames, not for the states around it.
        let want = if self.battle_ctx.action_state == state {
            step
        } else {
            DEFAULT_BATTLE_FRAME_STEP
        };
        if self.battle.frame_clock.step != want {
            self.set_battle_frame_step(want);
        }
    }

    /// The battle frame driver's discarded `rand()` draw: `FUN_80046A20`
    /// calls the generator on every pass (`jal 0x80056798` at `0x80046D2C`,
    /// after the four-slot loop that every path of falls through to
    /// `0x80046D10`) and overwrites `v0` at `0x80046D34` without reading it.
    /// A pass is one battle frame, so the engine, which ticks once a vsync,
    /// draws on the first tick of each [`BattleFrameClock`] frame. The
    /// call sits ahead of the side-band pass (`jal 0x80056208` at
    /// `0x80046D60`) and both state machines, and nothing on the pass gates
    /// it: the Begin prompt, a dialogue box and the results sequencer all
    /// draw. Gated by [`crate::world::WorldToggles::battle_pass_rand_draw`].
    ///
    // PORT: FUN_80046A20 (the discarded rand() draw, 0x80046D2C)
    pub(in crate::world) fn tick_battle_pass_draw(&mut self) {
        if !self.toggles.battle_pass_rand_draw || self.game_over_hold {
            return;
        }
        let frame = self.battle_frame_id();
        if self.battle.pass_draw_frame == Some(frame) {
            return;
        }
        self.battle.pass_draw_frame = Some(frame);
        let _discarded = self.next_rand();
    }

    /// The kept object 1's spin: on every pass `FUN_80046A20` adds half the
    /// frame step to the backdrop draw's slot-1 Y angle while battle init's
    /// keep-object-1 byte is up (`lbu v0,0x333(gp)` = `0x8007B64B`, then
    /// `0x800891D2 += *0x1F800393 >> 1`, `0x80046D34..0x80046D5C`). The byte
    /// is the one that keeps the object in the draw list, so the object the
    /// region kept is the one that turns - nilboa's horizon mist ribbon
    /// drifts round the arena at one unit a frame. A pass is one battle
    /// frame, so the engine winds on the first tick of each
    /// [`BattleFrameClock`] frame. The store sits right after the pass's
    /// discarded `rand()` draw and nothing gates it but the byte.
    ///
    /// The angle has two references on the whole disc, this writer and the
    /// backdrop draw's read, and no reset: it accumulates from boot, and a
    /// stage that drops object 1 draws whatever sits in slot 1 at the angle
    /// the last keeping fight left.
    ///
    // PORT: FUN_80046A20 (the slot-1 backdrop spin, 0x80046D34..0x80046D5C)
    pub(in crate::world) fn tick_battle_backdrop_spin(&mut self) {
        let frame = self.battle_frame_id();
        if self.battle.backdrop_spin_frame == Some(frame) {
            return;
        }
        self.battle.backdrop_spin_frame = Some(frame);
        let keep = self
            .encounters
            .region_setup
            .and_then(|s| s.keep_backdrop_object_1)
            .unwrap_or(false);
        if keep {
            let step = u16::from(self.battle.frame_clock.step >> 1);
            self.battle.backdrop_slot_1_yaw = self.battle.backdrop_slot_1_yaw.wrapping_add(step);
        }
    }

    /// The backdrop draw's slot-1 Y angle this frame, in 4096ths of a turn
    /// ([`Self::tick_battle_backdrop_spin`]). Both battle hosts turn their
    /// slot-1 backdrop draws by it through
    /// [`crate::scene::backdrop_slot_1_basis`].
    pub fn battle_backdrop_slot_1_yaw(&self) -> u16 {
        self.battle.backdrop_slot_1_yaw
    }

    /// Stamp the slot-1 angle directly, for a replay of a retail capture
    /// whose RAM holds it (`0x800891D2`): the angle is time since boot spent
    /// in keeping fights, which no seed replays.
    pub fn seed_battle_backdrop_slot_1_yaw(&mut self, yaw: u16) {
        self.battle.backdrop_slot_1_yaw = yaw;
    }

    /// The battle frame step in vsyncs ([`Self::set_battle_frame_step`]).
    pub fn battle_frame_step(&self) -> u8 {
        self.battle.frame_clock.step
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_clock_is_two_vsync_frames() {
        let c = BattleFrameClock::default();
        for t in 0..20u64 {
            assert_eq!(c.frame_of(t), t / 2);
        }
    }

    #[test]
    fn a_seeded_step_holds_only_inside_its_state() {
        let mut w = World::new();
        w.seed_battle_frame_step(3, 0x33);
        w.battle_ctx.action_state = 0x32;
        w.apply_battle_frame_step_seed();
        assert_eq!(w.battle_frame_step(), DEFAULT_BATTLE_FRAME_STEP);
        w.battle_ctx.action_state = 0x33;
        w.apply_battle_frame_step_seed();
        assert_eq!(w.battle_frame_step(), 3);
        w.battle_ctx.action_state = 0x34;
        w.apply_battle_frame_step_seed();
        assert_eq!(w.battle_frame_step(), DEFAULT_BATTLE_FRAME_STEP);
    }

    #[test]
    fn a_step_change_opens_a_fresh_frame_and_counts_on() {
        let mut c = BattleFrameClock::default();
        // Tick 7 is in frame 3; frame 4 opens on tick 7 at step 3.
        c.set_step(7, 3);
        assert_eq!(c.frame_of(7), 4);
        assert_eq!(c.frame_of(9), 4);
        assert_eq!(c.frame_of(10), 5);
        // Ids never run backwards across a change.
        c.set_step(11, 2);
        assert_eq!(c.frame_of(11), 6);
        assert_eq!(c.frame_of(13), 7);
    }

    #[test]
    fn the_kept_object_spins_half_the_frame_step_a_battle_frame() {
        let mut w = World::default();
        w.seed_battle_backdrop_keep_object_1(true);
        // Default step 2: one unit a two-vsync frame.
        for _ in 0..20 {
            w.tick_battle_backdrop_spin();
            w.clock.display_frames += 1;
        }
        assert_eq!(w.battle_backdrop_slot_1_yaw(), 10);
        // Step 3 still adds `3 >> 1 = 1`, once every three vsyncs.
        w.set_battle_frame_step(3);
        for _ in 0..30 {
            w.tick_battle_backdrop_spin();
            w.clock.display_frames += 1;
        }
        assert_eq!(w.battle_backdrop_slot_1_yaw(), 20);
        // Step 1 adds nothing: `1 >> 1 = 0`.
        w.set_battle_frame_step(1);
        for _ in 0..8 {
            w.tick_battle_backdrop_spin();
            w.clock.display_frames += 1;
        }
        assert_eq!(w.battle_backdrop_slot_1_yaw(), 20);
    }

    #[test]
    fn a_stage_that_drops_object_1_keeps_the_angle_it_was_left() {
        let mut w = World::default();
        w.seed_battle_backdrop_slot_1_yaw(0x1AD3);
        w.seed_battle_backdrop_keep_object_1(false);
        for _ in 0..40 {
            w.tick_battle_backdrop_spin();
            w.clock.display_frames += 1;
        }
        // No keep byte, no winding - and no reset either.
        assert_eq!(w.battle_backdrop_slot_1_yaw(), 0x1AD3);
    }
}
