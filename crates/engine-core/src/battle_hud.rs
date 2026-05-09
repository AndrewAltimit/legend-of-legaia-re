//! Battle HUD model — renderer-agnostic UI state for the in-battle screen.
//!
//! Holds per-slot HP / MP / AP / status-icon state plus a queue of damage
//! popups and battle-event log lines. The `engine-render` crate's
//! [`legaia_engine_render::battle_hud_draws_for`] turns one of these into
//! a `Vec<TextDraw>` for the GPU pipeline; engines that render via a
//! different path (web / terminal) can read the same struct directly.
//!
//! The HUD is fed by [`crate::world::World`] events:
//!
//! - `BattleEvent::ApplyArtStrike` → `push_damage_popup` (per-strike
//!   popup with a fade timer).
//! - `StatusEvent::TickDamage` / `Cleared` → `set_status_icons`.
//! - `BattleRound::begin/end` → `sync_from_world` to refresh HP / MP / AP.
//!
//! ## Frame timing
//!
//! Damage popups carry a `frames_remaining` counter; [`BattleHud::tick`]
//! decrements it each frame and drops popups whose counter reaches zero.
//! Default lifetime is 60 frames (~1 s at PSX 60 Hz).

use crate::ap_gauge::ApGauge;
use legaia_engine_vm::status_effects::{StatusEffectTracker, StatusKind};

/// Per-slot row update payload for [`BattleHud::sync_slot`].
///
/// Engines build one of these per actor each frame; the alternative
/// (a 9-arg sync function) trips clippy's argument-count lint and isn't
/// any clearer at call-sites.
#[derive(Debug, Clone, Copy)]
pub struct SlotSyncInfo<'a> {
    pub name: &'a str,
    pub is_party: bool,
    pub alive: bool,
    pub hp: u16,
    pub hp_max: u16,
    pub mp: u16,
    pub mp_max: u16,
    pub ap: Option<&'a ApGauge>,
}

/// Default popup lifetime in frames. PSX retail held damage numbers for
/// roughly 1 s after the strike; the renderer fades them out over the
/// last 16 frames.
pub const DEFAULT_POPUP_FRAMES: u16 = 60;

/// Per-slot HUD snapshot. Engines fold a battle-actor + status state
/// into one of these once per frame; the renderer iterates `slots`.
#[derive(Debug, Clone, Default)]
pub struct BattleSlotHud {
    /// Display name (character name, monster name, …). Empty string
    /// for inactive slots.
    pub name: String,
    /// `true` when this slot is occupied this round (party slot 0..2 or
    /// monster slot 3..7). Engines skip rendering rows where `active`
    /// is `false`.
    pub active: bool,
    /// `true` for party slots (0..2). Drives row colour: party rows are
    /// rendered in white, monster rows in pale red.
    pub is_party: bool,
    /// `true` when `liveness != 0` — actor is up. Dead actors get a
    /// "K.O." overlay and zero-bar HP gauge.
    pub alive: bool,
    pub hp: u16,
    pub hp_max: u16,
    pub mp: u16,
    pub mp_max: u16,
    pub ap_filled: u8,
    pub ap_max: u8,
    /// Per-slot active status effects. Sorted by [`StatusKind`] enum
    /// variant order so the icon strip is stable across frames.
    pub status_icons: Vec<StatusKind>,
}

impl BattleSlotHud {
    pub fn new() -> Self {
        Self::default()
    }

    /// HP fraction in 0..=1. Returns 0.0 when `hp_max == 0` (uninit slot).
    pub fn hp_fraction(&self) -> f32 {
        if self.hp_max == 0 {
            0.0
        } else {
            (self.hp as f32 / self.hp_max as f32).clamp(0.0, 1.0)
        }
    }

    /// MP fraction in 0..=1.
    pub fn mp_fraction(&self) -> f32 {
        if self.mp_max == 0 {
            0.0
        } else {
            (self.mp as f32 / self.mp_max as f32).clamp(0.0, 1.0)
        }
    }

    /// AP fraction in 0..=1. Returns 0.0 when `ap_max == 0`.
    pub fn ap_fraction(&self) -> f32 {
        if self.ap_max == 0 {
            0.0
        } else {
            (self.ap_filled as f32 / self.ap_max as f32).clamp(0.0, 1.0)
        }
    }

    /// Set the status icon list directly (bulk update from the engine).
    pub fn set_status_icons(&mut self, icons: impl IntoIterator<Item = StatusKind>) {
        self.status_icons.clear();
        self.status_icons.extend(icons);
        // Stable order by variant index so the renderer doesn't blink
        // when the underlying tracker shuffles its Vec.
        self.status_icons.sort_by_key(|k| status_kind_sort_key(*k));
        self.status_icons.dedup();
    }
}

fn status_kind_sort_key(k: StatusKind) -> u8 {
    match k {
        StatusKind::Burned => 0,
        StatusKind::Shocked => 1,
        StatusKind::Poisoned => 2,
        StatusKind::Asleep => 3,
        StatusKind::Confused => 4,
        StatusKind::Silenced => 5,
        StatusKind::Stunned => 6,
        StatusKind::Petrified => 7,
    }
}

/// One pending damage popup. Engines fold these onto the HUD with a
/// floating-text animation; the popup expires automatically after
/// `frames_remaining` reaches zero.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DamagePopup {
    /// Slot the popup is anchored to (0..=7).
    pub slot: u8,
    /// HP delta. Positive = damage dealt; negative (negative-coded as
    /// the high bit) = healed. Engines that want signed math should use
    /// [`Self::is_heal`].
    pub amount: u16,
    /// `true` when the popup represents a heal (rendered in green).
    pub is_heal: bool,
    /// `true` when the strike was a critical / "all-stars" hit (rendered
    /// in yellow with a bigger glyph).
    pub is_crit: bool,
    /// Optional status hint for popups that surface a status application
    /// (`Burned!` / `Asleep`). `None` for plain damage / heal popups.
    pub status: Option<StatusKind>,
    /// Frames left before the popup expires.
    pub frames_remaining: u16,
    /// Total lifetime — used by the renderer to compute the fade alpha.
    pub frames_total: u16,
}

impl DamagePopup {
    pub fn damage(slot: u8, amount: u16) -> Self {
        Self {
            slot,
            amount,
            is_heal: false,
            is_crit: false,
            status: None,
            frames_remaining: DEFAULT_POPUP_FRAMES,
            frames_total: DEFAULT_POPUP_FRAMES,
        }
    }

    pub fn heal(slot: u8, amount: u16) -> Self {
        Self {
            is_heal: true,
            ..Self::damage(slot, amount)
        }
    }

    pub fn crit(mut self) -> Self {
        self.is_crit = true;
        self
    }

    pub fn with_status(mut self, status: StatusKind) -> Self {
        self.status = Some(status);
        self
    }

    pub fn with_lifetime(mut self, frames: u16) -> Self {
        self.frames_remaining = frames;
        self.frames_total = frames;
        self
    }

    /// Fade alpha in 0..=1, computed from frames_remaining / frames_total.
    /// Engines render the popup with this multiplied into the text colour.
    pub fn alpha(&self) -> f32 {
        if self.frames_total == 0 {
            0.0
        } else {
            (self.frames_remaining as f32 / self.frames_total as f32).clamp(0.0, 1.0)
        }
    }
}

/// One battle-event log line, ringed in the HUD's left column. Engines
/// push lines from world-event drains; the buffer is bounded by
/// [`BattleHud::log_capacity`].
#[derive(Debug, Clone)]
pub struct LogLine {
    pub text: String,
    /// Optional accent (party / monster / system colour). The renderer
    /// maps each variant to a colour.
    pub accent: LogAccent,
}

/// Accent colour for [`LogLine`]. Engines pick the variant by event type;
/// the renderer chooses the actual RGBA.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogAccent {
    /// Default white.
    Neutral,
    /// Pale blue — party action.
    Party,
    /// Pale red — monster action.
    Monster,
    /// Yellow — critical hit, level up, status applied.
    Highlight,
    /// Green — heal / cure.
    Heal,
}

/// The HUD model.
#[derive(Debug, Clone)]
pub struct BattleHud {
    /// Per-slot panels (8 = 3 party + 5 monsters, mirrors the actor table).
    pub slots: [BattleSlotHud; 8],
    /// Damage / heal / status popups, drained per frame by [`Self::tick`].
    pub popups: Vec<DamagePopup>,
    /// Battle event log (ring buffer, oldest first).
    pub log: Vec<LogLine>,
    /// Maximum log lines retained. Older lines fall off the front when a
    /// new line is pushed past this cap. Default 6 — matches the retail
    /// 6-line scrolling log column.
    pub log_capacity: usize,
}

impl Default for BattleHud {
    fn default() -> Self {
        Self::new()
    }
}

impl BattleHud {
    pub fn new() -> Self {
        Self {
            slots: Default::default(),
            popups: Vec::new(),
            log: Vec::new(),
            log_capacity: 6,
        }
    }

    /// Replace the per-slot HP / MP / status row from a slice of party
    /// names + a battle-actor table view. Engines pre-resolve names from
    /// the save record; this function does not touch popups / log.
    pub fn sync_slot(&mut self, slot: u8, info: SlotSyncInfo<'_>) {
        if (slot as usize) >= self.slots.len() {
            return;
        }
        let s = &mut self.slots[slot as usize];
        s.name = info.name.to_string();
        s.active = true;
        s.is_party = info.is_party;
        s.alive = info.alive;
        s.hp = info.hp;
        s.hp_max = info.hp_max;
        s.mp = info.mp;
        s.mp_max = info.mp_max;
        if let Some(ap) = info.ap {
            // "Filled" in HUD terms is the amount of AP committed to the
            // queue this turn — `ceiling - current` (spent so far).
            let ceiling = ap.ceiling();
            s.ap_filled = ceiling.saturating_sub(ap.current_ap);
            s.ap_max = ceiling;
        } else {
            s.ap_filled = 0;
            s.ap_max = 0;
        }
    }

    /// Pull the active status icons for `slot` from a tracker. Replaces
    /// any previously stored icons.
    pub fn sync_status(&mut self, slot: u8, tracker: &StatusEffectTracker) {
        if (slot as usize) >= self.slots.len() {
            return;
        }
        let icons: Vec<StatusKind> = tracker.statuses(slot).iter().map(|s| s.kind).collect();
        self.slots[slot as usize].set_status_icons(icons);
    }

    /// Mark a slot as inactive (empty actor pool entry). Clears name and
    /// gauges so the renderer skips the row.
    pub fn clear_slot(&mut self, slot: u8) {
        if (slot as usize) < self.slots.len() {
            self.slots[slot as usize] = BattleSlotHud::default();
        }
    }

    /// Push a fresh damage popup with the default lifetime.
    pub fn push_damage(&mut self, slot: u8, amount: u16) {
        self.popups.push(DamagePopup::damage(slot, amount));
    }

    /// Push a fresh heal popup.
    pub fn push_heal(&mut self, slot: u8, amount: u16) {
        self.popups.push(DamagePopup::heal(slot, amount));
    }

    /// Push a status-applied popup (no HP delta).
    pub fn push_status(&mut self, slot: u8, status: StatusKind) {
        self.popups
            .push(DamagePopup::damage(slot, 0).with_status(status));
    }

    /// Push a pre-built popup. Useful for engines that customise the
    /// crit / fade fields per source event.
    pub fn push_popup(&mut self, popup: DamagePopup) {
        self.popups.push(popup);
    }

    /// Append a battle log line. When the log exceeds [`Self::log_capacity`],
    /// the oldest entry is dropped.
    pub fn push_log(&mut self, text: impl Into<String>, accent: LogAccent) {
        self.log.push(LogLine {
            text: text.into(),
            accent,
        });
        let cap = self.log_capacity;
        if self.log.len() > cap {
            let drop = self.log.len() - cap;
            self.log.drain(0..drop);
        }
    }

    /// Drop every queued popup. Engines call this on battle abort / scene
    /// transition so stale popups don't bleed into the next encounter.
    pub fn clear_popups(&mut self) {
        self.popups.clear();
    }

    /// Drop every log line.
    pub fn clear_log(&mut self) {
        self.log.clear();
    }

    /// One-frame advance. Decrements every popup's `frames_remaining`
    /// and drops popups that have expired. Returns the number of popups
    /// remaining after the tick.
    pub fn tick(&mut self) -> usize {
        self.popups.retain(|p| p.frames_remaining > 0);
        for p in self.popups.iter_mut() {
            p.frames_remaining = p.frames_remaining.saturating_sub(1);
        }
        // Re-prune in case the saturating_sub above dropped any to zero
        // (kept above zero before, zero now — render once more then drop
        // on the next tick).
        self.popups.len()
    }

    /// Number of slots currently active.
    pub fn active_slots(&self) -> usize {
        self.slots.iter().filter(|s| s.active).count()
    }

    /// Iterate active slots in (slot_index, slot_hud) order.
    pub fn iter_active(&self) -> impl Iterator<Item = (u8, &BattleSlotHud)> {
        self.slots
            .iter()
            .enumerate()
            .filter_map(|(i, s)| if s.active { Some((i as u8, s)) } else { None })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use legaia_engine_vm::status_effects::StatusEffectTracker;

    #[test]
    fn slot_hud_default_has_no_active_state() {
        let s = BattleSlotHud::default();
        assert!(!s.active);
        assert!(!s.is_party);
        assert!(!s.alive);
        assert_eq!(s.hp, 0);
        assert_eq!(s.hp_max, 0);
        assert_eq!(s.hp_fraction(), 0.0);
    }

    #[test]
    fn slot_hud_fractions_clamp_to_unit_interval() {
        let mut s = BattleSlotHud::new();
        s.hp = 200;
        s.hp_max = 100; // overflow case
        assert_eq!(s.hp_fraction(), 1.0);

        s.mp = 0;
        s.mp_max = 50;
        assert_eq!(s.mp_fraction(), 0.0);
    }

    #[test]
    fn slot_hud_status_icons_sort_by_kind_order() {
        let mut s = BattleSlotHud::new();
        s.set_status_icons([
            StatusKind::Petrified,
            StatusKind::Burned,
            StatusKind::Confused,
        ]);
        assert_eq!(
            s.status_icons,
            vec![
                StatusKind::Burned,
                StatusKind::Confused,
                StatusKind::Petrified
            ]
        );
    }

    #[test]
    fn slot_hud_status_icons_dedup_repeated_kinds() {
        let mut s = BattleSlotHud::new();
        s.set_status_icons([StatusKind::Burned, StatusKind::Burned, StatusKind::Asleep]);
        assert_eq!(s.status_icons, vec![StatusKind::Burned, StatusKind::Asleep]);
    }

    #[test]
    fn damage_popup_default_is_60_frames_no_crit() {
        let p = DamagePopup::damage(2, 100);
        assert_eq!(p.slot, 2);
        assert_eq!(p.amount, 100);
        assert_eq!(p.frames_remaining, DEFAULT_POPUP_FRAMES);
        assert_eq!(p.frames_total, DEFAULT_POPUP_FRAMES);
        assert!(!p.is_heal);
        assert!(!p.is_crit);
        assert_eq!(p.alpha(), 1.0);
    }

    #[test]
    fn damage_popup_alpha_scales_with_remaining_frames() {
        let mut p = DamagePopup::damage(0, 50).with_lifetime(20);
        p.frames_remaining = 10;
        assert!((p.alpha() - 0.5).abs() < 1e-5);
        p.frames_remaining = 0;
        assert_eq!(p.alpha(), 0.0);
    }

    #[test]
    fn damage_popup_with_status_carries_kind() {
        let p = DamagePopup::damage(0, 0).with_status(StatusKind::Asleep);
        assert_eq!(p.status, Some(StatusKind::Asleep));
    }

    #[test]
    fn hud_push_damage_appends_popup_with_default_lifetime() {
        let mut h = BattleHud::new();
        h.push_damage(3, 250);
        assert_eq!(h.popups.len(), 1);
        assert_eq!(h.popups[0].slot, 3);
        assert_eq!(h.popups[0].amount, 250);
        assert_eq!(h.popups[0].frames_remaining, DEFAULT_POPUP_FRAMES);
    }

    #[test]
    fn hud_tick_decrements_and_expires_popups() {
        let mut h = BattleHud::new();
        h.push_popup(DamagePopup::damage(0, 50).with_lifetime(3));
        // Tick 1: 3 -> 2.
        h.tick();
        assert_eq!(h.popups[0].frames_remaining, 2);
        // Tick 2: 2 -> 1.
        h.tick();
        assert_eq!(h.popups[0].frames_remaining, 1);
        // Tick 3: 1 -> 0; still kept (the retain pass on this tick
        // keeps non-zero, then decrements).
        h.tick();
        // Tick 4: filter at 0 drops it.
        h.tick();
        assert!(h.popups.is_empty());
    }

    #[test]
    fn hud_tick_keeps_popup_with_remaining_frames() {
        let mut h = BattleHud::new();
        h.push_popup(DamagePopup::damage(0, 50).with_lifetime(60));
        for _ in 0..30 {
            h.tick();
        }
        assert_eq!(h.popups.len(), 1);
        assert_eq!(h.popups[0].frames_remaining, 30);
    }

    #[test]
    fn hud_log_drops_oldest_at_capacity() {
        let mut h = BattleHud::new();
        h.log_capacity = 3;
        h.push_log("a", LogAccent::Neutral);
        h.push_log("b", LogAccent::Neutral);
        h.push_log("c", LogAccent::Neutral);
        h.push_log("d", LogAccent::Neutral);
        assert_eq!(h.log.len(), 3);
        // Oldest "a" was dropped.
        let texts: Vec<&str> = h.log.iter().map(|l| l.text.as_str()).collect();
        assert_eq!(texts, vec!["b", "c", "d"]);
    }

    #[test]
    fn hud_sync_slot_populates_panel() {
        let mut h = BattleHud::new();
        let mut ap = ApGauge::with_base(8);
        ap.try_spend(3);
        h.sync_slot(
            0,
            SlotSyncInfo {
                name: "Vahn",
                is_party: true,
                alive: true,
                hp: 250,
                hp_max: 300,
                mp: 12,
                mp_max: 30,
                ap: Some(&ap),
            },
        );
        let s = &h.slots[0];
        assert!(s.active);
        assert!(s.is_party);
        assert!(s.alive);
        assert_eq!(s.name, "Vahn");
        assert_eq!(s.hp, 250);
        assert_eq!(s.hp_max, 300);
        assert_eq!(s.ap_filled, 3);
        assert_eq!(s.ap_max, 8);
    }

    #[test]
    fn hud_sync_status_pulls_from_tracker() {
        let mut h = BattleHud::new();
        let mut tracker = StatusEffectTracker::new();
        tracker.apply(2, StatusKind::Burned);
        tracker.apply(2, StatusKind::Poisoned);
        h.sync_status(2, &tracker);
        // Sorted order: Burned (0) before Poisoned (2).
        assert_eq!(
            h.slots[2].status_icons,
            vec![StatusKind::Burned, StatusKind::Poisoned]
        );
    }

    #[test]
    fn hud_clear_slot_returns_panel_to_default() {
        let mut h = BattleHud::new();
        h.sync_slot(
            0,
            SlotSyncInfo {
                name: "Vahn",
                is_party: true,
                alive: true,
                hp: 100,
                hp_max: 100,
                mp: 0,
                mp_max: 0,
                ap: None,
            },
        );
        h.clear_slot(0);
        assert!(!h.slots[0].active);
        assert_eq!(h.slots[0].name, "");
    }

    #[test]
    fn hud_iter_active_skips_inactive_slots() {
        let mut h = BattleHud::new();
        h.sync_slot(
            0,
            SlotSyncInfo {
                name: "A",
                is_party: true,
                alive: true,
                hp: 10,
                hp_max: 10,
                mp: 0,
                mp_max: 0,
                ap: None,
            },
        );
        h.sync_slot(
            2,
            SlotSyncInfo {
                name: "C",
                is_party: false,
                alive: true,
                hp: 5,
                hp_max: 5,
                mp: 0,
                mp_max: 0,
                ap: None,
            },
        );
        let visible: Vec<u8> = h.iter_active().map(|(i, _)| i).collect();
        assert_eq!(visible, vec![0, 2]);
        assert_eq!(h.active_slots(), 2);
    }

    #[test]
    fn hud_clear_popups_drains_queue() {
        let mut h = BattleHud::new();
        h.push_damage(0, 10);
        h.push_damage(1, 20);
        h.clear_popups();
        assert!(h.popups.is_empty());
    }

    #[test]
    fn hud_push_status_emits_zero_amount_with_status_set() {
        let mut h = BattleHud::new();
        h.push_status(0, StatusKind::Asleep);
        assert_eq!(h.popups[0].amount, 0);
        assert_eq!(h.popups[0].status, Some(StatusKind::Asleep));
    }

    #[test]
    fn log_accent_variants_distinct() {
        // Sanity: Eq lets us use accent in renderer comparisons.
        assert_eq!(LogAccent::Neutral, LogAccent::Neutral);
        assert_ne!(LogAccent::Party, LogAccent::Monster);
    }

    #[test]
    fn slot_hud_ap_fraction_zero_when_max_zero() {
        let s = BattleSlotHud::new();
        assert_eq!(s.ap_fraction(), 0.0);
    }
}
