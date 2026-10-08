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

    /// Seed a replay's battle frame step: `step` vsyncs a frame from the
    /// first tick the action SM sits in `state`, for the rest of that
    /// battle ([`crate::world::FrameClock::battle_frame_step_seed`]).
    pub fn seed_battle_frame_step(&mut self, step: u8, state: u8) {
        self.clock.battle_frame_step_seed = Some((step.max(1), state));
    }

    /// Install a seeded step once the action SM reaches its state. Called
    /// after every action-SM step.
    pub(in crate::world) fn apply_battle_frame_step_seed(&mut self) {
        if let Some((step, state)) = self.clock.battle_frame_step_seed
            && self.battle_ctx.action_state == state
            && self.battle.frame_clock.step != step
        {
            self.set_battle_frame_step(step);
        }
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
}
