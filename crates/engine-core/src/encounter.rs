//! Per-scene encounter table + step-driven random battle trigger.
//!
//! The retail engine tracks a step counter per scene and rolls against the
//! scene's encounter table on every step. When the roll succeeds, the field
//! VM yields control to the battle scene loader. The trigger is gated by
//! several globals (`battle_disabled` flag, current encounter rate, etc.);
//! this module mirrors the gameplay-relevant subset as a clean-room SM.
//!
//! ## Components
//!
//! - [`EncounterEntry`] - one row in a per-scene table: monster-formation
//!   id + relative weight. Heavier rows are more likely; the roll is a
//!   weighted-random pick.
//! - [`EncounterTable`] - the per-scene set of rows plus the base trigger
//!   rate (probability the next step rolls a battle, expressed in 1/256).
//! - [`EncounterTracker`] - running state. Engines feed it a step counter
//!   and an RNG; it returns [`EncounterRoll`] when a battle should fire.
//! - [`EncounterSession`] - higher-level state machine that brackets the
//!   transition: `Idle → Triggered → ConfirmTransition → Loading → Done`.
//!   Engines drive this to handle the camera-shake / fade / battle-load
//!   sequence retail uses.
//!
//! Pure data - no Vfs / disc / world coupling. Engines call
//! [`EncounterTracker::on_step`] from the field-step path and feed the
//! resulting [`EncounterRoll`] into their battle-load routine.

use std::collections::HashMap;

/// One row in an encounter table.
///
/// Each row maps to a `BattleScene` id (the index into the per-scene
/// monster-formation list - the retail engine reads the formation from
/// `battle_data` PROT entries).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EncounterEntry {
    /// Monster-formation id (retail: index into `battle_data` group).
    pub formation_id: u16,
    /// Relative weight for the weighted-random pick. `0` rows are treated
    /// as inactive (skipped).
    pub weight: u16,
    /// Optional minimum-step gate. The roll succeeds only after at least
    /// this many steps have accumulated since the last battle. Used by
    /// the retail "no immediate re-encounter" rule. `0` means no gate.
    pub min_steps_since_last: u16,
}

impl EncounterEntry {
    pub const fn new(formation_id: u16, weight: u16) -> Self {
        Self {
            formation_id,
            weight,
            min_steps_since_last: 0,
        }
    }

    pub const fn with_min_gate(self, min_steps: u16) -> Self {
        Self {
            min_steps_since_last: min_steps,
            ..self
        }
    }
}

/// Per-scene encounter table.
#[derive(Debug, Clone, Default)]
pub struct EncounterTable {
    /// Display name for diagnostics.
    pub scene_label: String,
    /// Trigger rate as a 1/256 probability per step. Default is 8/256 ≈
    /// 3% - matches the retail "moderate" rate. Engines override per
    /// scene from the disc-loaded encounter parameters.
    pub trigger_rate_q8: u8,
    /// Active rows.
    pub entries: Vec<EncounterEntry>,
    /// Per-scene "no-encounter zones" - encoded as inclusive grid-cell
    /// rectangles `(x0, z0, x1, z1)` in scene-local coordinates. Engines
    /// query [`EncounterTable::is_safe_at`] before calling [`on_step`].
    pub safe_zones: Vec<(i16, i16, i16, i16)>,
}

impl EncounterTable {
    pub fn new(scene_label: impl Into<String>) -> Self {
        Self {
            scene_label: scene_label.into(),
            trigger_rate_q8: 8,
            ..Default::default()
        }
    }

    /// Replace the trigger rate (default `8/256`). `0` disables encounters
    /// for the scene (used by towns and cutscenes).
    pub fn set_trigger_rate(&mut self, rate_q8: u8) {
        self.trigger_rate_q8 = rate_q8;
    }

    pub fn push(&mut self, entry: EncounterEntry) {
        self.entries.push(entry);
    }

    pub fn add_safe_zone(&mut self, x0: i16, z0: i16, x1: i16, z1: i16) {
        self.safe_zones
            .push((x0.min(x1), z0.min(z1), x0.max(x1), z0.max(z1)));
    }

    /// Total active weight (rows whose `weight > 0`).
    pub fn total_weight(&self) -> u32 {
        self.entries.iter().map(|e| e.weight as u32).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty() || self.trigger_rate_q8 == 0
    }

    /// `true` if `(x, z)` falls inside any registered safe zone.
    pub fn is_safe_at(&self, x: i16, z: i16) -> bool {
        self.safe_zones
            .iter()
            .any(|&(x0, z0, x1, z1)| x >= x0 && x <= x1 && z >= z0 && z <= z1)
    }
}

/// Result of a successful encounter roll.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EncounterRoll {
    /// Picked row's `formation_id`.
    pub formation_id: u16,
    /// Index of the row inside the scene's `entries` (for diagnostics).
    pub row_index: usize,
    /// The trigger-roll value for this attempt (0..=255). Engines log this.
    pub roll_q8: u8,
}

/// Per-scene encounter tracker.
#[derive(Debug, Clone, Default)]
pub struct EncounterTracker {
    table: EncounterTable,
    /// Steps accumulated since the last battle. Reset to zero on
    /// successful roll.
    steps_since_last_battle: u32,
    /// Total steps accumulated this scene (diagnostics).
    total_steps: u32,
    /// Bias applied to the trigger roll. Items / accessories that suppress
    /// or boost encounter rate land here. Negative values reduce the
    /// effective rate; positive values boost it. Clamped per-roll.
    rate_bias_q8: i16,
    /// Master "no encounters" override - set during cutscenes, scripted
    /// transitions, and post-battle grace windows.
    suppressed: bool,
    /// Per-formation last-trigger step, used for [`EncounterEntry::min_steps_since_last`].
    last_trigger_step: HashMap<u16, u32>,
}

impl EncounterTracker {
    pub fn new(table: EncounterTable) -> Self {
        Self {
            table,
            ..Default::default()
        }
    }

    pub fn table(&self) -> &EncounterTable {
        &self.table
    }

    pub fn table_mut(&mut self) -> &mut EncounterTable {
        &mut self.table
    }

    pub fn replace_table(&mut self, table: EncounterTable) {
        self.table = table;
        self.steps_since_last_battle = 0;
        self.total_steps = 0;
        self.last_trigger_step.clear();
    }

    pub fn steps_since_last_battle(&self) -> u32 {
        self.steps_since_last_battle
    }

    pub fn total_steps(&self) -> u32 {
        self.total_steps
    }

    pub fn rate_bias(&self) -> i16 {
        self.rate_bias_q8
    }

    /// Add to the per-roll rate bias. Engines apply equipment effects
    /// (Goblin Foot accessory: -32; Encounter Up trinket: +32).
    pub fn add_rate_bias(&mut self, delta: i16) {
        self.rate_bias_q8 = self.rate_bias_q8.saturating_add(delta);
    }

    /// Reset the rate bias.
    pub fn clear_rate_bias(&mut self) {
        self.rate_bias_q8 = 0;
    }

    /// Suppress all rolls until [`Self::clear_suppression`] is called.
    pub fn suppress(&mut self) {
        self.suppressed = true;
    }

    pub fn clear_suppression(&mut self) {
        self.suppressed = false;
    }

    pub fn is_suppressed(&self) -> bool {
        self.suppressed
    }

    /// Resolve the effective trigger rate for the next step.
    ///
    /// Returns `0` when suppressed or when the table is empty. Bias is
    /// added, clamped to `0..=255`.
    pub fn effective_rate_q8(&self) -> u8 {
        if self.suppressed || self.table.is_empty() {
            return 0;
        }
        let base = self.table.trigger_rate_q8 as i32;
        let v = (base + self.rate_bias_q8 as i32).clamp(0, 255);
        v as u8
    }

    /// Mark that a step happened. Returns `Some(EncounterRoll)` when the
    /// roll triggers a battle. Engines must reset their step counter on
    /// success; the tracker resets [`Self::steps_since_last_battle`] to
    /// zero internally.
    ///
    /// `rng_word` is one 32-bit pull from the engine's shared RNG. The
    /// low byte is used for the trigger probability, the upper bytes
    /// drive the weighted-row pick.
    pub fn on_step(&mut self, rng_word: u32) -> Option<EncounterRoll> {
        self.total_steps = self.total_steps.saturating_add(1);
        self.steps_since_last_battle = self.steps_since_last_battle.saturating_add(1);

        let rate = self.effective_rate_q8();
        if rate == 0 {
            return None;
        }
        let trigger_byte = (rng_word & 0xFF) as u8;
        if trigger_byte >= rate {
            return None;
        }

        // Trigger fires. Pick a row by weighted random.
        let total = self.table.total_weight();
        if total == 0 {
            return None;
        }
        // Use the upper 16 bits of rng_word as the weighted-pick driver.
        let pick = (rng_word >> 16) % total.max(1);
        let mut acc: u32 = 0;
        for (idx, entry) in self.table.entries.iter().enumerate() {
            if entry.weight == 0 {
                continue;
            }
            // Check per-row min-step gate. The gate only kicks in
            // after the first trigger; the very first encounter for a
            // formation is always allowed.
            if entry.min_steps_since_last > 0
                && let Some(&last) = self.last_trigger_step.get(&entry.formation_id)
            {
                let elapsed = self.total_steps.saturating_sub(last);
                if elapsed < entry.min_steps_since_last as u32 {
                    continue;
                }
            }
            acc = acc.saturating_add(entry.weight as u32);
            if pick < acc {
                self.steps_since_last_battle = 0;
                self.last_trigger_step
                    .insert(entry.formation_id, self.total_steps);
                return Some(EncounterRoll {
                    formation_id: entry.formation_id,
                    row_index: idx,
                    roll_q8: trigger_byte,
                });
            }
        }
        // No row matched (gates filtered everything out).
        None
    }

    /// Reset per-scene state. Engines call this on scene change.
    pub fn reset(&mut self) {
        self.steps_since_last_battle = 0;
        self.total_steps = 0;
        self.last_trigger_step.clear();
    }
}

/// State of the [`EncounterSession`] state machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EncounterPhase {
    /// Steady state - engine ticks the tracker on every step.
    Idle,
    /// A roll succeeded; the engine starts the transition (camera shake,
    /// fade-out). Stays here for [`EncounterSession::transition_frames`]
    /// frames before advancing.
    Transition {
        frames_remaining: u16,
        roll: EncounterRoll,
    },
    /// Transition is done; the battle scene should be loaded now. Engines
    /// drain this once and proceed to load the formation.
    Triggered(EncounterRoll),
    /// Battle is running; tracker is suspended. Engines call
    /// [`EncounterSession::end_battle`] when the battle resolves.
    Battling { roll: EncounterRoll },
    /// Post-battle grace window - encounters suppressed for
    /// [`EncounterSession::grace_frames`]. Decrements per tick.
    Grace { frames_remaining: u16 },
}

/// Higher-level state machine bracketing the encounter transition.
#[derive(Debug, Clone)]
pub struct EncounterSession {
    tracker: EncounterTracker,
    phase: EncounterPhase,
    /// Frames the [`EncounterPhase::Transition`] phase lasts. Default 32
    /// (~0.5s at 60Hz) - matches the retail fade-out duration.
    pub transition_frames: u16,
    /// Frames the [`EncounterPhase::Grace`] phase lasts after a battle.
    /// Default 30 (~0.5s) - the post-battle "no immediate re-encounter"
    /// window the retail engine enforces.
    pub grace_frames: u16,
}

impl EncounterSession {
    pub fn new(tracker: EncounterTracker) -> Self {
        Self {
            tracker,
            phase: EncounterPhase::Idle,
            transition_frames: 32,
            grace_frames: 30,
        }
    }

    pub fn phase(&self) -> EncounterPhase {
        self.phase
    }

    pub fn tracker(&self) -> &EncounterTracker {
        &self.tracker
    }

    pub fn tracker_mut(&mut self) -> &mut EncounterTracker {
        &mut self.tracker
    }

    /// Force-reset to [`EncounterPhase::Idle`] and clear tracker state.
    pub fn reset(&mut self) {
        self.tracker.reset();
        self.phase = EncounterPhase::Idle;
    }

    /// Per-frame tick. Call this every frame from the engine main loop;
    /// the session decides internally whether to advance the transition
    /// or grace timers. Returns `true` when the phase changed this frame.
    pub fn tick_frame(&mut self) -> bool {
        match &mut self.phase {
            EncounterPhase::Idle
            | EncounterPhase::Battling { .. }
            | EncounterPhase::Triggered(_) => false,
            EncounterPhase::Transition {
                frames_remaining,
                roll,
            } => {
                if *frames_remaining > 0 {
                    *frames_remaining -= 1;
                    false
                } else {
                    let roll = *roll;
                    self.phase = EncounterPhase::Triggered(roll);
                    true
                }
            }
            EncounterPhase::Grace { frames_remaining } => {
                if *frames_remaining > 0 {
                    *frames_remaining -= 1;
                    false
                } else {
                    self.tracker.clear_suppression();
                    self.phase = EncounterPhase::Idle;
                    true
                }
            }
        }
    }

    /// Mark a step. Only counts when the session is in
    /// [`EncounterPhase::Idle`]; in any other phase the call is a no-op.
    /// Returns `true` if the step triggered a transition.
    pub fn on_step(&mut self, rng_word: u32) -> bool {
        if !matches!(self.phase, EncounterPhase::Idle) {
            return false;
        }
        if let Some(roll) = self.tracker.on_step(rng_word) {
            self.phase = EncounterPhase::Transition {
                frames_remaining: self.transition_frames,
                roll,
            };
            true
        } else {
            false
        }
    }

    /// Drive a roll sourced from *outside* the session's own mean-rate
    /// tracker into the transition SM. Used by the per-region field path
    /// ([`crate::region_encounter::RegionEncounterTracker`]), which owns the
    /// rate counter + formation pick; this session still supplies the
    /// transition / grace bracketing so a region-driven encounter flows
    /// through the same `Transition -> Triggered -> Battling -> Grace`
    /// states as a mean-rate one. No-op (returns `false`) unless the phase
    /// is [`EncounterPhase::Idle`], mirroring [`Self::on_step`]'s own gate.
    pub fn trigger_with(&mut self, roll: EncounterRoll) -> bool {
        if !matches!(self.phase, EncounterPhase::Idle) {
            return false;
        }
        self.phase = EncounterPhase::Transition {
            frames_remaining: self.transition_frames,
            roll,
        };
        true
    }

    /// Drain the [`EncounterPhase::Triggered`] roll. Engines call this
    /// once the transition is done to fetch the formation_id and load
    /// the battle scene. Sets the phase to [`EncounterPhase::Battling`].
    pub fn drain_triggered(&mut self) -> Option<EncounterRoll> {
        match self.phase {
            EncounterPhase::Triggered(roll) => {
                self.phase = EncounterPhase::Battling { roll };
                Some(roll)
            }
            _ => None,
        }
    }

    /// Notify the session that the active battle finished. Drops into
    /// the grace phase with encounters suppressed.
    pub fn end_battle(&mut self) {
        self.tracker.suppress();
        self.phase = EncounterPhase::Grace {
            frames_remaining: self.grace_frames,
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn small_table() -> EncounterTable {
        let mut t = EncounterTable::new("test_scene");
        t.set_trigger_rate(64); // ~25%
        t.push(EncounterEntry::new(1, 50));
        t.push(EncounterEntry::new(2, 30));
        t.push(EncounterEntry::new(3, 20));
        t
    }

    #[test]
    fn empty_table_never_triggers() {
        let t = EncounterTable::new("empty");
        assert!(t.is_empty());
        let mut tracker = EncounterTracker::new(t);
        for i in 0..1000 {
            assert!(tracker.on_step(i as u32).is_none());
        }
    }

    #[test]
    fn rate_zero_never_triggers() {
        let mut t = small_table();
        t.set_trigger_rate(0);
        let mut tracker = EncounterTracker::new(t);
        for i in 0..1000 {
            assert!(tracker.on_step(i as u32).is_none());
        }
    }

    #[test]
    fn rate_full_always_triggers() {
        let mut t = small_table();
        t.set_trigger_rate(255);
        let mut tracker = EncounterTracker::new(t);
        for i in 0..100 {
            // Ensure trigger byte (low 8 bits) < 255.
            let rng = (0x12345600u32) | (i as u32 & 0xFE);
            assert!(tracker.on_step(rng).is_some());
        }
    }

    #[test]
    fn suppression_blocks_triggers() {
        let t = small_table();
        let mut tracker = EncounterTracker::new(t);
        tracker.suppress();
        for i in 0..1000 {
            assert!(tracker.on_step(i as u32).is_none());
        }
        tracker.clear_suppression();
        // Now triggers can fire (probabilistically).
        let mut hit = false;
        for i in 0..1000 {
            if tracker.on_step((i as u32) << 1).is_some() {
                hit = true;
                break;
            }
        }
        assert!(hit, "should trigger at least once with normal rate");
    }

    #[test]
    fn rate_bias_clamps() {
        let mut t = small_table();
        t.set_trigger_rate(8);
        let mut tracker = EncounterTracker::new(t);
        tracker.add_rate_bias(-100);
        assert_eq!(tracker.effective_rate_q8(), 0);
        tracker.clear_rate_bias();
        tracker.add_rate_bias(300);
        assert_eq!(tracker.effective_rate_q8(), 255);
    }

    #[test]
    fn weighted_pick_distribution() {
        let t = small_table();
        let mut tracker = EncounterTracker::new(t);
        // Force every step to trigger: trigger byte 0 always < 64.
        let mut counts = [0u32; 4];
        for i in 0..10_000u32 {
            let rng = (i << 16) & 0xFFFF_FF00;
            if let Some(roll) = tracker.on_step(rng) {
                counts[roll.formation_id as usize] += 1;
            }
        }
        // Row 1 weight 50, row 2 weight 30, row 3 weight 20 → 50/30/20.
        assert!(counts[1] > counts[2], "1>2: {counts:?}");
        assert!(counts[2] > counts[3], "2>3: {counts:?}");
        // No formation 0 should appear.
        assert_eq!(counts[0], 0);
    }

    #[test]
    fn min_gate_filters_immediate_re_trigger() {
        let mut t = EncounterTable::new("gated");
        t.set_trigger_rate(255);
        t.push(EncounterEntry::new(7, 100).with_min_gate(50));
        let mut tracker = EncounterTracker::new(t);
        // First trigger (step 1).
        let r0 = tracker.on_step(0).unwrap();
        assert_eq!(r0.formation_id, 7);
        // Next 49 steps: gated.
        for _ in 0..49 {
            assert!(tracker.on_step(0).is_none());
        }
        // 50th step: gate clears.
        let r1 = tracker.on_step(0).unwrap();
        assert_eq!(r1.formation_id, 7);
    }

    #[test]
    fn safe_zone_predicate() {
        let mut t = EncounterTable::new("zoned");
        t.add_safe_zone(0, 0, 10, 10);
        assert!(t.is_safe_at(5, 5));
        assert!(t.is_safe_at(0, 0));
        assert!(t.is_safe_at(10, 10));
        assert!(!t.is_safe_at(11, 5));
        assert!(!t.is_safe_at(-1, 5));
    }

    #[test]
    fn session_transition_then_triggered() {
        let t = small_table();
        let mut s = EncounterSession::new(EncounterTracker::new(t));
        s.transition_frames = 3;
        // Force a roll that hits.
        assert!(s.on_step(0));
        assert!(matches!(
            s.phase(),
            EncounterPhase::Transition {
                frames_remaining: 3,
                ..
            }
        ));
        // Steps in transition are no-ops.
        assert!(!s.on_step(0));
        // Tick the transition timer.
        s.tick_frame();
        s.tick_frame();
        s.tick_frame(); // hits zero this frame, but holds
        assert!(matches!(s.phase(), EncounterPhase::Transition { .. }));
        s.tick_frame(); // 0 → Triggered
        assert!(matches!(s.phase(), EncounterPhase::Triggered(_)));
        let r = s.drain_triggered().unwrap();
        assert!(matches!(s.phase(), EncounterPhase::Battling { .. }));
        assert_eq!(r.formation_id, 1);
    }

    #[test]
    fn session_grace_after_battle() {
        let t = small_table();
        let mut s = EncounterSession::new(EncounterTracker::new(t));
        s.transition_frames = 0;
        s.grace_frames = 2;
        s.on_step(0);
        s.tick_frame();
        s.drain_triggered();
        s.end_battle();
        assert!(matches!(s.phase(), EncounterPhase::Grace { .. }));
        // Steps during grace are no-ops.
        assert!(!s.on_step(0));
        s.tick_frame();
        s.tick_frame(); // ends grace
        s.tick_frame();
        assert!(matches!(s.phase(), EncounterPhase::Idle));
    }

    #[test]
    fn replace_table_resets_steps() {
        let t = small_table();
        let mut tracker = EncounterTracker::new(t);
        for i in 0..50 {
            tracker.on_step(i as u32);
        }
        assert!(tracker.total_steps() > 0);
        tracker.replace_table(EncounterTable::new("new"));
        assert_eq!(tracker.total_steps(), 0);
        assert_eq!(tracker.steps_since_last_battle(), 0);
    }

    #[test]
    fn total_weight_excludes_inactive_rows() {
        let mut t = EncounterTable::new("mixed");
        t.push(EncounterEntry::new(1, 50));
        t.push(EncounterEntry::new(2, 0));
        t.push(EncounterEntry::new(3, 100));
        assert_eq!(t.total_weight(), 150);
    }

    #[test]
    fn effective_rate_zero_when_table_empty() {
        let tracker = EncounterTracker::new(EncounterTable::new("empty"));
        assert_eq!(tracker.effective_rate_q8(), 0);
    }
}
