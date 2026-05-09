//! Per-character Seru-magic learning + capture session.
//!
//! In Legaia, magic ("Spirit Magic" / "Seru Magic") is learned by
//! capturing **Seru** — small Ra-Seru attached to monsters. Each
//! captured Seru contributes points toward a per-character spell list.
//! Once a Seru's contribution crosses the per-character per-spell
//! threshold, the spell is added to the character's learned list.
//!
//! ## Components
//!
//! - [`SeruDef`] — one entry in the master Seru registry. Maps a Seru id
//!   to which spell (id) it teaches and how many capture points each
//!   capture grants.
//! - [`SeruRegistry`] — full master table. Engines build at startup.
//! - [`SeruCaptureLog`] — per-character running totals. Engines persist
//!   this in the save file (LGSF v2 carries it).
//! - [`record_capture`] — pure resolver: takes a registry, log, and
//!   capture event; returns the resulting [`CaptureOutcome`] with the
//!   list of new spells learned this capture.
//! - [`SeruCaptureSession`] — UI-facing state machine driving the
//!   "Genocide Crystal succeeded → captured Seru: <name>" → "<X> learned
//!   <spell>!" → close popup flow.

use std::collections::HashMap;

/// One Seru that can be captured. Engines populate a [`SeruRegistry`]
/// with these at startup from the level_up overlay's `seru_table`
/// (still partially overlay-blocked; vanilla data ships approximations).
#[derive(Debug, Clone)]
pub struct SeruDef {
    pub id: u16,
    pub name: String,
    /// Spell id this Seru teaches. Engines look the spell up in
    /// [`crate::spells::SpellCatalog`].
    pub spell_id: u8,
    /// Capture points awarded per successful capture. Once accumulated
    /// per-character, crosses [`learn_threshold`] the spell is learned.
    pub capture_points: u16,
    /// Which characters can learn this spell. Bit 0 = Vahn, 1 = Noa,
    /// 2 = Gala. Bit 3+ are reserved (Songi etc. in retail story).
    pub learnable_mask: u8,
    /// Threshold (capture-points) at which this Seru's spell is
    /// considered learned. Default 100.
    pub learn_threshold: u16,
}

impl SeruDef {
    /// `true` if a character at `char_slot` can learn this Seru's spell.
    pub fn can_be_learned_by(&self, char_slot: u8) -> bool {
        if char_slot >= 8 {
            return false;
        }
        (self.learnable_mask & (1 << char_slot)) != 0
    }
}

/// Master Seru registry.
#[derive(Debug, Default, Clone)]
pub struct SeruRegistry {
    by_id: HashMap<u16, SeruDef>,
}

impl SeruRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, seru: SeruDef) {
        self.by_id.insert(seru.id, seru);
    }

    pub fn get(&self, id: u16) -> Option<&SeruDef> {
        self.by_id.get(&id)
    }

    pub fn len(&self) -> usize {
        self.by_id.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_id.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = &SeruDef> {
        self.by_id.values()
    }

    /// Build a vanilla registry approximating the early-game Legaia roster.
    /// Spell ids align with [`crate::spells::SpellCatalog::vanilla`].
    pub fn vanilla() -> Self {
        let mut r = Self::new();
        // Mask = all 3 main characters (0b0000_0111 = 7).
        let all = 0b0000_0111u8;
        r.insert(SeruDef {
            id: 0x0001,
            name: "Spark".into(),
            spell_id: 0x20, // Spark / Flame
            capture_points: 25,
            learnable_mask: all,
            learn_threshold: 100,
        });
        r.insert(SeruDef {
            id: 0x0002,
            name: "Flame".into(),
            spell_id: 0x21, // Burning Heat
            capture_points: 50,
            learnable_mask: all,
            learn_threshold: 200,
        });
        r.insert(SeruDef {
            id: 0x0003,
            name: "Aqua".into(),
            spell_id: 0x22, // Aqua
            capture_points: 25,
            learnable_mask: all,
            learn_threshold: 100,
        });
        r.insert(SeruDef {
            id: 0x0004,
            name: "Storm".into(),
            spell_id: 0x23, // Thunder Bolt
            capture_points: 25,
            learnable_mask: all,
            learn_threshold: 100,
        });
        r.insert(SeruDef {
            id: 0x0005,
            name: "Wind".into(),
            spell_id: 0x24,
            capture_points: 25,
            learnable_mask: all,
            learn_threshold: 100,
        });
        r.insert(SeruDef {
            id: 0x0006,
            name: "Frost".into(),
            spell_id: 0x25,
            capture_points: 25,
            learnable_mask: all,
            learn_threshold: 100,
        });
        r.insert(SeruDef {
            id: 0x0007,
            name: "Crash".into(),
            spell_id: 0x26,
            capture_points: 50,
            learnable_mask: all,
            learn_threshold: 200,
        });
        // Healing
        r.insert(SeruDef {
            id: 0x0010,
            name: "Heal".into(),
            spell_id: 0x10,
            capture_points: 25,
            learnable_mask: all,
            learn_threshold: 100,
        });
        r.insert(SeruDef {
            id: 0x0011,
            name: "Vital".into(),
            spell_id: 0x11,
            capture_points: 50,
            learnable_mask: all,
            learn_threshold: 200,
        });
        // Buffs
        r.insert(SeruDef {
            id: 0x0020,
            name: "Power".into(),
            spell_id: 0x40,
            capture_points: 25,
            learnable_mask: all,
            learn_threshold: 100,
        });
        r.insert(SeruDef {
            id: 0x0021,
            name: "Defense".into(),
            spell_id: 0x41,
            capture_points: 25,
            learnable_mask: all,
            learn_threshold: 100,
        });
        // Utility
        r.insert(SeruDef {
            id: 0x0030,
            name: "Warp".into(),
            spell_id: 0x51,
            capture_points: 100,
            learnable_mask: all,
            learn_threshold: 100,
        });
        r
    }
}

/// One row in the per-character capture log.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct SeruCaptureRow {
    /// Total capture points accumulated so far.
    pub points: u16,
    /// Total times this Seru has been captured.
    pub capture_count: u16,
    /// `true` once this Seru's spell was added to the learned list.
    pub learned: bool,
}

/// Per-character capture log.
#[derive(Debug, Default, Clone)]
pub struct SeruCaptureLog {
    /// Indexed first by `char_slot`, then by `seru_id`.
    rows: HashMap<(u8, u16), SeruCaptureRow>,
    /// Per-character learned-spell list (spell ids the character knows).
    /// Save layer persists this; battle layer reads it for the spell menu.
    learned_spells: HashMap<u8, Vec<u8>>,
}

impl SeruCaptureLog {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn row(&self, char_slot: u8, seru_id: u16) -> SeruCaptureRow {
        self.rows
            .get(&(char_slot, seru_id))
            .copied()
            .unwrap_or_default()
    }

    /// Mark a spell as already learned (e.g. from a loaded save).
    pub fn mark_learned(&mut self, char_slot: u8, seru_id: u16, spell_id: u8) {
        let row = self.rows.entry((char_slot, seru_id)).or_default();
        row.learned = true;
        let list = self.learned_spells.entry(char_slot).or_default();
        if !list.contains(&spell_id) {
            list.push(spell_id);
        }
    }

    /// Returns the per-character learned-spell list. Order matches the
    /// in-game spell-list display order (insertion order).
    pub fn learned_spells(&self, char_slot: u8) -> &[u8] {
        self.learned_spells
            .get(&char_slot)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    /// `true` if `char_slot` already learned the Seru's spell.
    pub fn has_learned(&self, char_slot: u8, seru_id: u16) -> bool {
        self.rows
            .get(&(char_slot, seru_id))
            .is_some_and(|r| r.learned)
    }

    /// Total accumulated capture points for diagnostics.
    pub fn total_points(&self, char_slot: u8) -> u32 {
        self.rows
            .iter()
            .filter(|((c, _), _)| *c == char_slot)
            .map(|(_, r)| r.points as u32)
            .sum()
    }
}

/// Outcome of a single capture event.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CaptureOutcome {
    /// Per-character spell-learn events resulting from this capture.
    pub learns: Vec<LearnEvent>,
    /// Total capture-points awarded (sum across all eligible characters).
    pub awarded_points: u16,
    /// `false` if the capture was rejected (unknown Seru id, etc.).
    pub accepted: bool,
}

/// One spell-learn event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LearnEvent {
    pub char_slot: u8,
    pub seru_id: u16,
    pub spell_id: u8,
}

/// Pure capture resolver. Accumulates capture points, fires learn events
/// for any character that crosses the threshold this capture.
///
/// The retail engine awards capture points to all eligible characters in
/// the active party — the same Seru capture teaches whichever main
/// characters the player has currently. We follow that convention.
pub fn record_capture(
    registry: &SeruRegistry,
    log: &mut SeruCaptureLog,
    seru_id: u16,
    party_slots: &[u8],
) -> CaptureOutcome {
    let Some(seru) = registry.get(seru_id) else {
        return CaptureOutcome::default();
    };
    let mut out = CaptureOutcome {
        accepted: true,
        awarded_points: seru.capture_points,
        ..Default::default()
    };
    for &char_slot in party_slots {
        if !seru.can_be_learned_by(char_slot) {
            continue;
        }
        let key = (char_slot, seru_id);
        let row = log.rows.entry(key).or_default();
        if row.learned {
            continue;
        }
        row.capture_count = row.capture_count.saturating_add(1);
        row.points = row.points.saturating_add(seru.capture_points);
        if row.points >= seru.learn_threshold {
            row.learned = true;
            let list = log.learned_spells.entry(char_slot).or_default();
            if !list.contains(&seru.spell_id) {
                list.push(seru.spell_id);
            }
            out.learns.push(LearnEvent {
                char_slot,
                seru_id,
                spell_id: seru.spell_id,
            });
        }
    }
    out
}

/// State of the [`SeruCaptureSession`] state machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CaptureState {
    /// Capture roll succeeded; popup shown for `frames_remaining` frames.
    Capturing { frames_remaining: u16, seru_id: u16 },
    /// One or more characters learned a spell — display banner per learn
    /// in turn. `index` is the current banner being shown.
    Announcing { index: usize, frames_remaining: u16 },
    /// Session finished.
    Done,
}

/// UI-facing state machine for the capture flow. Drives the post-capture
/// banner sequence engines render.
#[derive(Debug, Clone)]
pub struct SeruCaptureSession {
    state: CaptureState,
    seru_name: String,
    learns: Vec<(LearnEvent, String, String)>, // (event, char_name, spell_name)
    pub capture_frames: u16,
    pub announce_frames: u16,
}

impl SeruCaptureSession {
    /// Construct a session for a successful capture event.
    ///
    /// `learn_names` resolves `(char_slot, spell_id)` to display names;
    /// engines pass a closure that consults the spell catalog + party.
    pub fn new(
        seru_name: impl Into<String>,
        seru_id: u16,
        outcome: CaptureOutcome,
        mut learn_names: impl FnMut(u8, u8) -> (String, String),
    ) -> Self {
        Self::with_durations(seru_name, seru_id, outcome, 60, 90, &mut learn_names)
    }

    /// Like [`Self::new`] but takes explicit durations for the capture
    /// banner and per-learn announce phases. Used by tests that need
    /// short timings.
    pub fn with_durations(
        seru_name: impl Into<String>,
        seru_id: u16,
        outcome: CaptureOutcome,
        capture_frames: u16,
        announce_frames: u16,
        learn_names: &mut dyn FnMut(u8, u8) -> (String, String),
    ) -> Self {
        let learns: Vec<(LearnEvent, String, String)> = outcome
            .learns
            .into_iter()
            .map(|ev| {
                let (cn, sn) = learn_names(ev.char_slot, ev.spell_id);
                (ev, cn, sn)
            })
            .collect();
        Self {
            state: CaptureState::Capturing {
                frames_remaining: capture_frames,
                seru_id,
            },
            seru_name: seru_name.into(),
            learns,
            capture_frames,
            announce_frames,
        }
    }

    pub fn state(&self) -> &CaptureState {
        &self.state
    }

    pub fn seru_name(&self) -> &str {
        &self.seru_name
    }

    pub fn learns(&self) -> &[(LearnEvent, String, String)] {
        &self.learns
    }

    pub fn is_done(&self) -> bool {
        matches!(self.state, CaptureState::Done)
    }

    /// Banner text engines render. Returns `Some` while a banner is
    /// active.
    pub fn current_banner(&self) -> Option<String> {
        match &self.state {
            CaptureState::Capturing { .. } => Some(format!("Captured: {}!", self.seru_name)),
            CaptureState::Announcing { index, .. } => self
                .learns
                .get(*index)
                .map(|(_, c, s)| format!("{c} learned {s}!")),
            CaptureState::Done => None,
        }
    }

    /// One-frame tick. Advances banner state. Returns `true` when phase
    /// changed.
    pub fn tick_frame(&mut self) -> bool {
        match &mut self.state {
            CaptureState::Capturing {
                frames_remaining, ..
            } => {
                if *frames_remaining > 0 {
                    *frames_remaining -= 1;
                    false
                } else if !self.learns.is_empty() {
                    self.state = CaptureState::Announcing {
                        index: 0,
                        frames_remaining: self.announce_frames,
                    };
                    true
                } else {
                    self.state = CaptureState::Done;
                    true
                }
            }
            CaptureState::Announcing {
                index,
                frames_remaining,
            } => {
                if *frames_remaining > 0 {
                    *frames_remaining -= 1;
                    false
                } else if *index + 1 < self.learns.len() {
                    *index += 1;
                    *frames_remaining = self.announce_frames;
                    true
                } else {
                    self.state = CaptureState::Done;
                    true
                }
            }
            CaptureState::Done => false,
        }
    }

    /// Engine shortcut: tick `frames` frames in one call.
    pub fn tick_frames(&mut self, frames: u32) {
        for _ in 0..frames {
            if self.is_done() {
                break;
            }
            self.tick_frame();
        }
    }

    /// Skip current banner. Engines call this from the player's "press
    /// confirm to advance" handler.
    pub fn advance(&mut self) {
        match &mut self.state {
            CaptureState::Capturing {
                frames_remaining, ..
            } => {
                *frames_remaining = 0;
                self.tick_frame();
            }
            CaptureState::Announcing {
                frames_remaining, ..
            } => {
                *frames_remaining = 0;
                self.tick_frame();
            }
            CaptureState::Done => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn registry() -> SeruRegistry {
        let mut r = SeruRegistry::new();
        r.insert(SeruDef {
            id: 1,
            name: "Spark".into(),
            spell_id: 0x20,
            capture_points: 50,
            learnable_mask: 0b0000_0111,
            learn_threshold: 100,
        });
        r.insert(SeruDef {
            id: 2,
            name: "VahnOnly".into(),
            spell_id: 0x40,
            capture_points: 100,
            learnable_mask: 0b0000_0001,
            learn_threshold: 100,
        });
        r
    }

    #[test]
    fn unknown_seru_not_accepted() {
        let r = registry();
        let mut log = SeruCaptureLog::new();
        let out = record_capture(&r, &mut log, 99, &[0, 1, 2]);
        assert!(!out.accepted);
        assert!(out.learns.is_empty());
    }

    #[test]
    fn capture_below_threshold_no_learn() {
        let r = registry();
        let mut log = SeruCaptureLog::new();
        let out = record_capture(&r, &mut log, 1, &[0, 1, 2]);
        assert!(out.accepted);
        assert!(out.learns.is_empty());
        assert_eq!(log.row(0, 1).points, 50);
    }

    #[test]
    fn capture_at_threshold_learns_for_all_eligible() {
        let r = registry();
        let mut log = SeruCaptureLog::new();
        record_capture(&r, &mut log, 1, &[0, 1, 2]);
        let out = record_capture(&r, &mut log, 1, &[0, 1, 2]);
        assert!(out.accepted);
        assert_eq!(out.learns.len(), 3);
        assert!(log.has_learned(0, 1));
        assert!(log.has_learned(1, 1));
        assert!(log.has_learned(2, 1));
    }

    #[test]
    fn restricted_seru_only_teaches_eligible_chars() {
        let r = registry();
        let mut log = SeruCaptureLog::new();
        let out = record_capture(&r, &mut log, 2, &[0, 1, 2]);
        assert_eq!(out.learns.len(), 1);
        assert_eq!(out.learns[0].char_slot, 0);
        assert!(!log.has_learned(1, 2));
        assert!(!log.has_learned(2, 2));
    }

    #[test]
    fn re_capture_after_learn_no_duplicate() {
        let r = registry();
        let mut log = SeruCaptureLog::new();
        record_capture(&r, &mut log, 1, &[0]);
        record_capture(&r, &mut log, 1, &[0]);
        assert!(log.has_learned(0, 1));
        let out = record_capture(&r, &mut log, 1, &[0]);
        assert!(out.learns.is_empty());
        // Spell list still has one entry.
        assert_eq!(log.learned_spells(0).len(), 1);
    }

    #[test]
    fn mark_learned_seeds_log() {
        let mut log = SeruCaptureLog::new();
        log.mark_learned(0, 1, 0x20);
        assert!(log.has_learned(0, 1));
        assert_eq!(log.learned_spells(0), &[0x20]);
    }

    #[test]
    fn vanilla_registry_non_empty() {
        let r = SeruRegistry::vanilla();
        assert!(r.len() >= 10);
    }

    #[test]
    fn vanilla_registry_warp_high_threshold() {
        let r = SeruRegistry::vanilla();
        let warp = r.iter().find(|s| s.name == "Warp").unwrap();
        assert!(warp.capture_points >= 100);
    }

    #[test]
    fn capture_session_capturing_then_done_when_no_learns() {
        let outcome = CaptureOutcome {
            accepted: true,
            awarded_points: 25,
            learns: Vec::new(),
        };
        let mut s = SeruCaptureSession::with_durations("Spark", 1, outcome, 2, 1, &mut |_, _| {
            ("Vahn".into(), "Heal".into())
        });
        // Tick through the capture phase.
        s.tick_frame();
        s.tick_frame();
        s.tick_frame(); // transitions to Done since no learns
        assert!(s.is_done());
    }

    #[test]
    fn capture_session_announces_each_learn() {
        let outcome = CaptureOutcome {
            accepted: true,
            awarded_points: 50,
            learns: vec![
                LearnEvent {
                    char_slot: 0,
                    seru_id: 1,
                    spell_id: 0x20,
                },
                LearnEvent {
                    char_slot: 1,
                    seru_id: 1,
                    spell_id: 0x20,
                },
            ],
        };
        let mut s = SeruCaptureSession::with_durations("Spark", 1, outcome, 1, 1, &mut |c, _| {
            (
                if c == 0 { "Vahn".into() } else { "Noa".into() },
                "Spark".into(),
            )
        });
        s.advance();
        // Now in Announcing[0].
        let banner_0 = s.current_banner().unwrap();
        assert!(banner_0.contains("Vahn"));
        s.advance();
        let banner_1 = s.current_banner().unwrap();
        assert!(banner_1.contains("Noa"));
        s.advance();
        assert!(s.is_done());
    }

    #[test]
    fn outcome_default_is_rejected() {
        let o = CaptureOutcome::default();
        assert!(!o.accepted);
        assert!(o.learns.is_empty());
    }

    #[test]
    fn total_points_aggregates() {
        let r = registry();
        let mut log = SeruCaptureLog::new();
        record_capture(&r, &mut log, 1, &[0]);
        record_capture(&r, &mut log, 2, &[0]);
        // 50 from Spark + 100 from VahnOnly = 150.
        assert_eq!(log.total_points(0), 150);
    }
}
