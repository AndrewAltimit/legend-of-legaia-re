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
//! so its step is the load alone: `2` on a lone member's frame (one field plus
//! the miss), `3` on a full party's or while a summon's creature and effects fill the frame
//! (`theeder_summon_mid_cast`'s ring peaks at `625`, `gimard_burning_attack`'s
//! sits at the `700` clamp). It is not a property of the game state, so the
//! engine runs the same law over a measured cost ([`FrameCostRing`],
//! [`measured_frame_cost`]), the cast arm tables inside a module band, and only
//! a replay that knows retail's step installs another.
//!
//! The engine ticks once a vsync and keeps every per-vsync rate; what the
//! step changes is where the frame boundaries fall - the root-motion carry's
//! truncation (`RootMotionCarry`) and the camera walker's steps
//! (`BattleCamera::set_frame_step`).
//!
//! REF: FUN_80016B6C

/// The engine's battle frame step: retail's step on an ordinary battle frame,
/// the one the camera and the root-motion carry assume by default.
pub const DEFAULT_BATTLE_FRAME_STEP: u8 = 2;

/// Port of the frame driver's step law (`FUN_80016B6C`): the sixteen-entry
/// history `0x80084098` and its maximum against `0xF1` / `0x1FF` / `0x2D1`
/// (`0x80017098..0x8001715C`). What feeds it differs. Retail stores the hblanks
/// since the last vsync once the CPU has finished the frame and `DrawSync(0)`
/// has returned (`FUN_800173BC`), so the entry is `max(cpu, gpu of the last
/// table)`; the engine has no such clock and pushes [`measured_frame_cost`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FrameCostRing {
    entries: [u16; 16],
    cursor: u8,
}

impl FrameCostRing {
    /// Store one frame's cost (`li s0,0x2bc` clamps a frame of `0x2D0` or
    /// more) and read the step the next frame runs on.
    pub fn push(&mut self, cost: u16) -> u8 {
        let raw = cost;
        self.entries[usize::from(self.cursor & 0xF)] = if raw >= 0x2D0 { 0x2BC } else { raw };
        self.cursor = self.cursor.wrapping_add(1) & 0xF;
        let longest = self.entries.iter().copied().max().unwrap_or(0);
        // The newest raw duration stands in for the clamped entry
        // (`0x800170F8..0x80017104`), which is how a clamped frame reads step 4.
        let longest = if raw >= 0x2D0 { raw } else { longest };
        step_of_cost(longest)
    }
}

/// The step a frame cost reads (`0x80017124..0x8001715C`).
pub const fn step_of_cost(cost: u16) -> u8 {
    if cost < 0xF1 {
        1
    } else if cost < 0x1FF {
        2
    } else if cost < 0x2D1 {
        3
    } else {
        4
    }
}

/// A frame's cost as the ring would have stored it, for a party of this size in
/// this action state: the median entry of the library's battle states and of
/// three whole fights recorded on PCSX-Redux (lone member, two, full party).
/// A member adds about `0x7C` hblanks, and on this machine the entry is the CPU's
/// alone: `gp+0x3A8` (CPU done) and `gp+0x3AC` (after `DrawSync`) read the same.
/// It is a measurement of one emulated machine, like the cast arm tables, and no
/// model of a frame's work: Mednafen reads a full party's action about `100` lower,
/// with the GPU wait in it, and no real hardware has been measured.
///
/// The summon close-up `0x33` and the flash `0x34` are read off the step
/// rather than a ring entry: on every PCSX-Redux summon film of a lone caster
/// the step drops from `2` to `1` sixteen frames after the close-up's first few
/// frames (so those frames cost under `0xF1`), and on a full party's from `3`
/// to `2` at `0x34`'s entry (under `0x1FF`). The two-member figure is the
/// member step between them; no film records it.
pub const fn measured_frame_cost(party_count: u8, action_state: u8) -> u16 {
    let row: [u16; 3] = match action_state {
        0x33 | 0x34 => [200, 330, 460],
        0x00 => [282, 411, 545],
        0x0A => [340, 457, 527],
        0x0C => [354, 463, 608],
        0x14 => [329, 453, 593],
        0x19 => [379, 483, 594],
        0x1E => [373, 479, 612],
        0x1F => [382, 453, 700],
        0x20 => [383, 540, 667],
        0x29 => [284, 411, 500],
        0x2B => [354, 387, 534],
        0x2E => [354, 494, 560],
        0x50 => [349, 468, 599],
        0x51 => [341, 457, 595],
        0x5A => [310, 452, 428],
        _ => [345, 460, 595],
    };
    row[match party_count {
        0 | 1 => 0,
        2 => 1,
        _ => 2,
    }]
}

/// The cost a frame an arm table names its step for leaves in the ring, so the
/// frames after a whole-screen arm read what its cost left there: the mid-rung
/// of that step.
pub const fn cost_of_step(step: u8) -> u16 {
    match step {
        0 | 1 => 200,
        2 => 400,
        3 => 600,
        _ => 0x2D1,
    }
}

/// The step a full fight of this party settles on, for a caller with no action
/// state to read.
pub const fn steady_battle_frame_step(party_count: u8) -> u8 {
    step_of_cost(measured_frame_cost(party_count, 0xFF))
}

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

    /// Re-time the frame `tick` opens to `step` vsyncs, keeping its id: the
    /// pass that ran on `tick` belongs to a frame whose length retail
    /// measures only at its end (`FUN_80016B6C` samples the frame's cost at
    /// the frame-end pass and waits `VSync(step)` before the next pass), so
    /// a step taken on the pass's own tick spans that pass's frame. A new
    /// id here would open a second frame on the same tick, and the next
    /// tick would claim it for a second pass.
    pub fn retime_frame(&mut self, tick: u64, step: u8) {
        let step = step.max(1);
        if step == self.step {
            return;
        }
        let id = self.frame_of(tick);
        *self = Self {
            step,
            origin_tick: tick,
            origin_frame: id,
        };
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

impl BattleFrameClock {
    /// How many display ticks into its battle frame `tick` falls.
    pub fn ticks_into_frame(&self, tick: u64) -> u64 {
        tick.saturating_sub(self.origin_tick) % u64::from(self.step.max(1))
    }
}

/// A retail recording's frame steps, per action-SM key, for a film that wants
/// the frame's cost taken out of what it scores (`--retail-steps` in
/// `scripts/ci/cast-sequence.py`). Retail's step is the emulated machine's
/// measure of each frame ([`crate::frame_clock`]), which the engine does not
/// model, so a recording's camera and timers walk on steps the engine's table
/// only samples. A harness seat: no play path installs one.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RetailStepTrack {
    /// `(action state, module phase)` to the step of each retail frame under it.
    frames: std::collections::BTreeMap<(u8, u8), Vec<u8>>,
    /// Passes the engine has run under each key.
    passes: std::collections::BTreeMap<(u8, u8), usize>,
}

impl RetailStepTrack {
    /// Parse `state phase step` lines, one retail frame each, in film order.
    pub fn parse(text: &str) -> Self {
        let mut track = Self::default();
        for line in text.lines() {
            let f: Vec<u8> = line
                .split_whitespace()
                .filter_map(|w| w.parse().ok())
                .collect();
            if let [state, phase, step] = f[..] {
                track
                    .frames
                    .entry((state, phase))
                    .or_default()
                    .push(step.max(1));
            }
        }
        track
    }

    /// The step of the pass just run under this key: retail's frame of the
    /// same ordinal under the key, its last one past that, `None` for a key
    /// the film never saw.
    pub fn next_step(&mut self, state: u8, phase: u8) -> Option<u8> {
        let frames = self.frames.get(&(state, phase))?;
        let n = self.passes.entry((state, phase)).or_default();
        let step = frames.get(*n).or(frames.last()).copied();
        *n += 1;
        step
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ticks_into_frame_counts_from_the_step_origin() {
        let mut c = BattleFrameClock::default();
        assert_eq!(
            (10..14).map(|t| c.ticks_into_frame(t)).collect::<Vec<_>>(),
            [0, 1, 0, 1]
        );
        c.set_step(7, 3);
        assert_eq!(
            (7..11).map(|t| c.ticks_into_frame(t)).collect::<Vec<_>>(),
            [0, 1, 2, 0]
        );
    }

    /// A step retimed on the tick a pass ran keeps that pass's frame: the
    /// next tick is still inside it, so no second pass opens there.
    #[test]
    fn a_retimed_frame_keeps_its_id() {
        let mut c = BattleFrameClock::default();
        let id = c.frame_of(10);
        c.retime_frame(10, 3);
        assert_eq!(c.frame_of(10), id);
        assert_eq!(c.frame_of(11), id);
        assert_eq!(c.frame_of(12), id);
        assert_eq!(c.frame_of(13), id + 1);
        assert_eq!(c.ticks_into_frame(13), 0);
    }

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

#[cfg(test)]
mod frame_cost_tests {
    use super::*;

    #[test]
    fn the_ring_holds_a_costly_frame_for_sixteen_passes() {
        let mut ring = FrameCostRing::default();
        assert_eq!(ring.push(400), 2);
        assert_eq!(ring.push(600), 3);
        for _ in 0..14 {
            assert_eq!(ring.push(400), 3);
        }
        assert_eq!(
            ring.push(400),
            3,
            "the costly entry is one write from its slot"
        );
        assert_eq!(ring.push(400), 2);
    }

    #[test]
    fn a_frame_past_the_clamp_reads_step_four_and_stores_the_clamp() {
        let mut ring = FrameCostRing::default();
        assert_eq!(ring.push(0x2D1), 4);
        assert_eq!(ring.push(400), 3, "the clamp `0x2BC` reads step 3 after it");
    }

    #[test]
    fn a_full_partys_fight_settles_on_three_and_a_lone_members_on_two() {
        assert_eq!(steady_battle_frame_step(3), 3);
        assert_eq!(steady_battle_frame_step(2), 2);
        assert_eq!(steady_battle_frame_step(1), 2);
    }
}
