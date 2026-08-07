//! Battle HUD model - renderer-agnostic UI state for the in-battle screen.
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
//! Default lifetime is 60 frames (~1 s at PSX 60 Hz). Simultaneous popups
//! are bounded by retail's 8-slot ring (`FUN_801F44A0`, delegated to
//! `legaia_engine_vm::battle_gauge_rearm::DamagePopupRing` by
//! [`BattleHud::push_popup`]): a ninth push overwrites the slot the ring
//! cursor names instead of growing the list.

use crate::ap_gauge::ApGauge;
use legaia_engine_vm::battle_gauge_rearm::DamagePopupRing;
pub use legaia_engine_vm::battle_gauge_rearm::POPUP_RING_SLOTS;
use legaia_engine_vm::status_effects::{StatusEffectTracker, StatusIcon, StatusKind};

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
    /// `true` when `liveness != 0` - actor is up. Dead actors get a
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
    /// Displayed character level - retail's char record `+0x130`, which the
    /// status element's no-ailment arm draws beside the base marker (see
    /// [`Self::status_element`]). `0` means "unknown", and the hosts draw the
    /// bare marker rather than "LV 0".
    pub level: u8,
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

    /// Gauge fill-colour indices `(hp, mp)` for the drawn bars, in retail's
    /// `FUN_80046A20` code space (`2` dead, `3` status override, `7` high,
    /// `6` mid, `9` low).
    ///
    /// The whole-gauge precedence is retail's: death first (`hp == 0` after
    /// the ramp settles), then the status override, then per-bar fill
    /// ratios. The status flag retail reads is actor `+0x16E`; the engine
    /// approximates it with "any active status icon", the same stand-in
    /// [`legaia_engine_ui::hp_bar_color_index`]'s callers use.
    ///
    /// Since `hp` here is the **displayed** (ramping) value, the bar colour
    /// tracks the drain exactly as retail's gauge does - `FUN_80046A20`
    /// keys its death arm on the same `+0x172` display field, so a slot
    /// whose live HP already hit zero stays in the low band until the bar
    /// finishes draining.
    pub fn gauge_fill_indices(&self) -> (u8, u8) {
        legaia_engine_vm::battle_gauge::gauge_colors(
            self.hp,
            self.hp_max,
            self.mp,
            self.mp_max,
            u16::from(!self.status_icons.is_empty()),
        )
    }

    /// Per-slot status icon strip, encoded as one-byte ASCII letters.
    /// Engines pass this to the renderer's `HudSlotView::status_letters`
    /// without an extra allocation step.
    ///
    /// Letter encoding (first character of the in-game status name):
    ///   `T` Toxic, `N` Numb, `V` Venom, `S` Sleep, `C` Confuse, `F` Faint;
    ///   the two collisions take the rarer status' lowercase form -
    ///   `c` Curse (vs `C` Confuse) and `s` Stone (vs `S` Sleep).
    pub fn status_letters(&self) -> Vec<u8> {
        self.status_icons
            .iter()
            .map(|k| status_kind_letter(*k))
            .collect()
    }

    /// The slot's packed retail status word - battle actor `+0x16E`, the
    /// halfword `FUN_80047430` mirrors to the display record's `+0x6F6`.
    ///
    /// Built from the typed [`Self::status_icons`] set through
    /// [`legaia_engine_vm::status_effects::pack_display_flags`]. Rot packs
    /// its **whole** limb group here rather than a single rolled bit: the HUD
    /// snapshot carries kinds, not instances, and the ladder only tests the
    /// group. Faint contributes no bit - it is the zero-HP arm of the ladder.
    pub fn status_display_flags(&self) -> u16 {
        self.status_icons
            .iter()
            .fold(0u16, |w, k| w | k.display_bit())
    }

    /// The single status element retail draws for this slot, chosen by the
    /// `FUN_8002C2E4` priority ladder over [`Self::status_display_flags`].
    ///
    /// Retail's `present` input is the display record's `+0x6CE`, i.e. the
    /// actor's live HP (`+0x14C`); the engine's `alive` flag is the same
    /// predicate, so a KO'd slot takes the `Sprite(0x20)` arm whatever else
    /// is set. When nothing is set the arm is
    /// [`StatusIcon::BaseWithCount`], whose "count" is [`Self::level`].
    // PORT: FUN_8002C2E4 (the selection; the kernel lives in engine-vm)
    pub fn status_element(&self) -> StatusIcon {
        legaia_engine_vm::status_effects::status_icon(self.status_display_flags(), self.alive)
    }

    /// The retail sprite id the status element resolves to (`0x18..=0x20`),
    /// or `0` for the no-ailment base marker / an unrepresented bit. Hosts
    /// take this as the whole per-slot status readout - one element, not a
    /// strip.
    pub fn status_sprite(&self) -> u8 {
        match self.status_element() {
            StatusIcon::Sprite(id) => id,
            StatusIcon::BaseWithCount | StatusIcon::None => 0,
        }
    }
}

/// Single-letter ASCII abbreviation for a [`StatusKind`]. Engines render
/// these as glyph overlays on the HUD slot row.
pub fn status_kind_letter(kind: StatusKind) -> u8 {
    match kind {
        StatusKind::Toxic => b'T',
        StatusKind::Numb => b'N',
        StatusKind::Venom => b'V',
        StatusKind::Sleep => b'S',
        StatusKind::Confuse => b'C',
        StatusKind::Rot => b'R',
        // The two first-letter collisions (Curse vs Confuse, Stone vs Sleep)
        // take the lowercase form of the rarer status.
        StatusKind::Curse => b'c',
        StatusKind::Stone => b's',
        StatusKind::Faint => b'F',
    }
}

fn status_kind_sort_key(k: StatusKind) -> u8 {
    match k {
        StatusKind::Toxic => 0,
        StatusKind::Numb => 1,
        StatusKind::Venom => 2,
        StatusKind::Sleep => 3,
        StatusKind::Confuse => 4,
        StatusKind::Rot => 5,
        StatusKind::Curse => 6,
        StatusKind::Stone => 7,
        StatusKind::Faint => 8,
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
    /// (`Toxic!` / `Sleep`). `None` for plain damage / heal popups.
    pub status: Option<StatusKind>,
    /// Frames left before the popup expires.
    pub frames_remaining: u16,
    /// Total lifetime - used by the renderer to compute the fade alpha.
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
    /// Pale blue - party action.
    Party,
    /// Pale red - monster action.
    Monster,
    /// Yellow - critical hit, level up, status applied.
    Highlight,
    /// Green - heal / cure.
    Heal,
}

/// The HUD model.
#[derive(Debug, Clone)]
pub struct BattleHud {
    /// Per-slot panels (8 = 3 party + 5 monsters, mirrors the actor table).
    pub slots: [BattleSlotHud; 8],
    /// Damage / heal / status popups, drained per frame by [`Self::tick`].
    ///
    /// Bounded at [`POPUP_RING_SLOTS`]: every push routes through
    /// [`Self::push_popup`], which delegates the cursor bookkeeping to
    /// retail's 8-slot ring ([`Self::popup_ring`]) - a ninth simultaneous
    /// popup overwrites the slot the cursor names rather than growing the
    /// list. Read freely; push through the `push_*` methods.
    pub popups: Vec<DamagePopup>,
    /// Retail's damage-popup ring state (`ctx+0x262` write cursor,
    /// `ctx+0x273` push counter) - the bookkeeping authority for
    /// [`Self::popups`], advanced only by [`Self::push_popup`].
    pub popup_ring: DamagePopupRing,
    /// Ring slot each `popups` entry occupies (parallel to `popups`).
    /// Private: the pairing is what makes the overwrite land on the same
    /// *slot* retail's cursor names.
    popup_ring_slots: Vec<u8>,
    /// Battle event log (ring buffer, oldest first).
    pub log: Vec<LogLine>,
    /// Maximum log lines retained. Older lines fall off the front when a
    /// new line is pushed past this cap. Default 6 - matches the retail
    /// 6-line scrolling log column.
    pub log_capacity: usize,
    /// The status CLUT recolour latch + party palette copies - pass 4 of
    /// `FUN_8004CE2C`. Armed by [`Self::sync_status`] (which every host and
    /// the `battle_session` driver already call once per slot per frame) and
    /// drained by the host's mid-battle VRAM pass through
    /// [`crate::battle_status_clut::StatusClutState::step`].
    pub status_clut: crate::battle_status_clut::StatusClutState,
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
            popup_ring: DamagePopupRing::default(),
            popup_ring_slots: Vec::new(),
            log: Vec::new(),
            log_capacity: 6,
            status_clut: Default::default(),
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
            // queue this turn - `ceiling - current` (spent so far).
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
    ///
    /// Also folds the slot's Stone bit into [`Self::status_clut`] - retail's
    /// applier writes the `actor[+0x220]` latch that `FUN_8004CE2C`'s fourth
    /// pass consumes, and this is the one per-slot-per-frame call every host
    /// already makes with the tracker in hand.
    pub fn sync_status(&mut self, slot: u8, tracker: &StatusEffectTracker) {
        if (slot as usize) >= self.slots.len() {
            return;
        }
        let icons: Vec<StatusKind> = tracker.statuses(slot).iter().map(|s| s.kind).collect();
        self.status_clut
            .arm(slot, icons.contains(&StatusKind::Stone));
        self.slots[slot as usize].set_status_icons(icons);
    }

    /// Set the slot's displayed level - the count retail draws beside the
    /// no-ailment base marker (char record `+0x130`,
    /// [`legaia_save::CharacterRecord::magic_rank`]). Separate from
    /// [`Self::sync_slot`] because the level lives on the save record rather
    /// than on the battle actor the row is otherwise built from.
    pub fn sync_level(&mut self, slot: u8, level: u8) {
        if let Some(s) = self.slots.get_mut(slot as usize) {
            s.level = level;
        }
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
        self.push_popup(DamagePopup::damage(slot, amount));
    }

    /// Push a fresh heal popup.
    pub fn push_heal(&mut self, slot: u8, amount: u16) {
        self.push_popup(DamagePopup::heal(slot, amount));
    }

    /// Push a status-applied popup (no HP delta).
    pub fn push_status(&mut self, slot: u8, status: StatusKind) {
        self.push_popup(DamagePopup::damage(slot, 0).with_status(status));
    }

    /// Push a pre-built popup through retail's popup ring.
    ///
    /// Retail has no growable popup list: `FUN_801F44A0` writes the pushed
    /// value / parameter / timer at the battle context's write cursor
    /// (`ctx+0x262`) and advances it `(cursor + 1) & 7`, so at most
    /// [`POPUP_RING_SLOTS`] popups exist at once and a ninth simultaneous
    /// push **overwrites** the slot the cursor names - the first popup,
    /// when none has expired. The cursor + push-counter bookkeeping is
    /// delegated to the ported kernel ([`DamagePopupRing::push`], the
    /// `PORT: FUN_801F44A0` site), and each display entry is keyed to the
    /// ring slot its push landed in, so the overwrite target is the *slot*
    /// the cursor names - retail's cursor is independent of expiry, not a
    /// front-of-queue rule.
    ///
    /// The ring's `value` is the signed HP delta retail pushes (a heal
    /// stores negative); its `param` byte carries the target slot.
    ///
    /// REF: FUN_801F44A0
    pub fn push_popup(&mut self, popup: DamagePopup) {
        // `popups` is a public field; if something mutated it out-of-band,
        // re-pair the slot record defensively. `0xFF` never matches a
        // cursor (`& 7`), so unpaired entries are simply never overwritten.
        self.popup_ring_slots.resize(self.popups.len(), 0xFF);
        let slot = self.popup_ring.cursor & 0x7;
        let magnitude = popup.amount.min(i16::MAX as u16) as i16;
        let value = if popup.is_heal { -magnitude } else { magnitude };
        self.popup_ring.push(value, popup.slot);
        match self.popup_ring_slots.iter().position(|&s| s == slot) {
            Some(i) => self.popups[i] = popup,
            None => {
                self.popups.push(popup);
                self.popup_ring_slots.push(slot);
            }
        }
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
        self.popup_ring_slots.clear();
        // The ring lives in the battle context, which retail rebuilds per
        // encounter - reset the cursor + counter with the display list.
        self.popup_ring = DamagePopupRing::default();
    }

    /// Drop every log line.
    pub fn clear_log(&mut self) {
        self.log.clear();
    }

    /// One-frame advance. Decrements every popup's `frames_remaining`
    /// and drops popups that have expired. Returns the number of popups
    /// remaining after the tick.
    pub fn tick(&mut self) -> usize {
        // Keep the ring-slot record paired with the entries that survive.
        self.popup_ring_slots.resize(self.popups.len(), 0xFF);
        let slots = &mut self.popup_ring_slots;
        let mut i = 0;
        self.popups.retain(|p| {
            let keep = p.frames_remaining > 0;
            if keep {
                i += 1;
            } else {
                slots.remove(i);
            }
            keep
        });
        for p in self.popups.iter_mut() {
            p.frames_remaining = p.frames_remaining.saturating_sub(1);
        }
        // Re-prune in case the saturating_sub above dropped any to zero
        // (kept above zero before, zero now - render once more then drop
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

    /// Build a sequence of plain [`SlotView`]s suitable for handing to
    /// `engine-render::battle_hud_draws_for`. Owned data - engines that
    /// want zero-copy can iterate `iter_active()` and build their own
    /// view structs.
    pub fn slot_views(&self) -> Vec<SlotView> {
        self.iter_active()
            .map(|(slot_idx, s)| {
                let (hp_fill, mp_fill) = s.gauge_fill_indices();
                SlotView {
                    slot: slot_idx,
                    name: s.name.clone(),
                    is_party: s.is_party,
                    alive: s.alive,
                    hp: s.hp,
                    hp_max: s.hp_max,
                    mp: s.mp,
                    mp_max: s.mp_max,
                    ap_filled: s.ap_filled,
                    ap_max: s.ap_max,
                    hp_fill,
                    mp_fill,
                    status_sprite: s.status_sprite(),
                    level: s.level,
                }
            })
            .collect()
    }

    /// Plain view for popups, without renderer types.
    pub fn popup_views(&self) -> Vec<PopupView> {
        self.popups
            .iter()
            .map(|p| PopupView {
                slot: p.slot,
                amount: p.amount,
                is_heal: p.is_heal,
                is_crit: p.is_crit,
                status_letter: p.status.map(status_kind_letter),
                alpha: p.alpha(),
            })
            .collect()
    }

    /// Plain view for log lines, without renderer types. Each entry's
    /// `color_rgba` is filled from a shared palette so engines don't have
    /// to re-derive it.
    pub fn log_views(&self) -> Vec<LogView> {
        self.log
            .iter()
            .map(|l| LogView {
                text: l.text.clone(),
                color_rgba: log_accent_color(l.accent),
            })
            .collect()
    }
}

/// Plain HUD slot view - owned strings + bytes, no renderer types.
/// Engines convert into `legaia_engine_render::HudSlotView` trivially:
/// the field shapes match by name.
#[derive(Debug, Clone)]
pub struct SlotView {
    pub slot: u8,
    pub name: String,
    pub is_party: bool,
    pub alive: bool,
    pub hp: u16,
    pub hp_max: u16,
    pub mp: u16,
    pub mp_max: u16,
    pub ap_filled: u8,
    pub ap_max: u8,
    /// Gauge fill-colour indices (retail `FUN_80046A20` code space) - see
    /// [`BattleSlotHud::gauge_fill_indices`].
    pub hp_fill: u8,
    pub mp_fill: u8,
    /// Retail status-element sprite id (`0x18..=0x20`), or `0` for the
    /// no-ailment base marker. See [`BattleSlotHud::status_sprite`] - retail
    /// draws exactly one, never a strip.
    pub status_sprite: u8,
    /// Displayed character level, the base-marker arm's count
    /// ([`BattleSlotHud::level`]).
    pub level: u8,
}

/// Plain popup view.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PopupView {
    pub slot: u8,
    pub amount: u16,
    pub is_heal: bool,
    pub is_crit: bool,
    pub status_letter: Option<u8>,
    pub alpha: f32,
}

/// Plain log view with the resolved colour pre-baked.
#[derive(Debug, Clone)]
pub struct LogView {
    pub text: String,
    pub color_rgba: [f32; 4],
}

/// Standard colour for each [`LogAccent`]. Engines that want a custom
/// palette can override per-line.
pub fn log_accent_color(accent: LogAccent) -> [f32; 4] {
    match accent {
        LogAccent::Neutral => [1.0, 1.0, 1.0, 1.0],
        LogAccent::Party => [0.7, 0.85, 1.0, 1.0],
        LogAccent::Monster => [1.0, 0.7, 0.7, 1.0],
        LogAccent::Highlight => [1.0, 0.95, 0.4, 1.0],
        LogAccent::Heal => [0.5, 1.0, 0.5, 1.0],
    }
}

/// Numeric half of one battle-HUD row, staged while the world is still
/// borrowed and applied to the HUD model afterwards.
struct SlotRow {
    is_party: bool,
    alive: bool,
    hp: u16,
    hp_max: u16,
    mp: u16,
    mp_max: u16,
    /// Index into `World::ap_gauges` for party rows; `None` for monsters.
    ap_slot: Option<usize>,
    /// Displayed level (char record `+0x130`) for party rows; `0` for
    /// monsters, which retail's status element never draws a count for.
    level: u8,
}

/// Fold the live battle-actor table into `hud`'s per-slot rows, so a host's
/// shared `battle_hud_draws_for` builder has something to draw.
///
/// Without this the HUD model carries only popups and status icons and every
/// slot reads `active == false`, so the builder emits an empty draw list.
/// Shared by the native window and the browser play page - each host calls it
/// once per battle frame, then projects `BattleHud` into its renderer's view
/// types.
///
/// Slot indices are **absolute actor-table indices**, not a compacted list:
/// party ordinals `0..party_count`, monsters above that. Renderers key popup
/// anchoring off the same index space, so compacting here would mis-anchor
/// every damage number.
pub fn sync_battle_hud_rows(hud: &mut BattleHud, world: &crate::world::World) {
    let pc = (world.party_count.clamp(1, 3) as usize).min(world.actors.len());
    let party_names = crate::field_menu_dispatch::roster_names(world);

    // Party rows. `character_max_mp` is the only MP ceiling the world carries
    // (`BattleActor` has live `mp` but no max), and it is keyed by battle
    // ordinal - the same index `build_battle_item_session` uses.
    //
    // HP is the **displayed** value, not the live one: retail's readout draws
    // actor `+0x172`, the bar the per-frame ramp `FUN_80047430` walks toward
    // live HP a quarter of the debt at a time. The sim maintains that ramp in
    // `BattleActor::hp_display` (`World::apply_battle_hp_delta` seeds it,
    // `tick_battle_hp_bars` drains it); reading live `hp` here showed the
    // ramp's end state instantly and left the animation computed but unseen.
    // `None` means "never armed / settled at live HP".
    let mut rows: Vec<(u8, String, SlotRow)> = Vec::new();
    for (i, a) in world.actors.iter().take(pc).enumerate() {
        let name = party_names
            .get(world.party_roster_slot(i))
            .filter(|n| !n.is_empty())
            .cloned()
            .unwrap_or_else(|| format!("P{}", i + 1));
        rows.push((
            i as u8,
            name,
            SlotRow {
                is_party: true,
                alive: a.battle.liveness != 0,
                hp: a.battle.hp_display.unwrap_or(a.battle.hp),
                hp_max: a.battle.max_hp,
                mp: a.battle.mp,
                mp_max: world.character_max_mp.get(i).copied().unwrap_or(0),
                ap_slot: (i < world.ap_gauges.len()).then_some(i),
                // The status element's no-ailment arm draws the character
                // record's `+0x130` beside the base marker (`FUN_8002C2E4`
                // reads it as the display record's `+0x6F8`).
                level: world
                    .roster
                    .members
                    .get(world.party_roster_slot(i))
                    .map(|m| m.magic_rank())
                    .unwrap_or(0),
            },
        ));
    }

    // Monster rows. Named from the live catalog when the formation resolved
    // one; `M<n>` otherwise. Monsters have no MP ceiling in the model, so the
    // builder draws no MP field for them.
    let mut cleared: Vec<u8> = Vec::new();
    for slot in pc..world.actors.len().min(hud.slots.len()) {
        let a = &world.actors[slot];
        if a.battle.max_hp == 0 {
            cleared.push(slot as u8);
            continue;
        }
        let name = a
            .battle_monster_id
            .and_then(|id| world.monster_catalog.get(id))
            .map(|d| d.name.clone())
            .unwrap_or_else(|| format!("M{}", slot - pc + 1));
        rows.push((
            slot as u8,
            name,
            SlotRow {
                is_party: false,
                alive: a.battle.liveness != 0,
                // Same displayed-HP read as the party rows; monster slots
                // (>= 3) settle in one frame per FUN_80047430's slot split,
                // so their display only ever lags by a single tick.
                hp: a.battle.hp_display.unwrap_or(a.battle.hp),
                hp_max: a.battle.max_hp,
                mp: 0,
                mp_max: 0,
                ap_slot: None,
                level: 0,
            },
        ));
    }
    // Slots past the actor table are stale from a previous formation.
    for slot in world.actors.len()..hud.slots.len() {
        cleared.push(slot as u8);
    }

    for (slot, name, row) in &rows {
        let ap = row.ap_slot.map(|i| &world.ap_gauges[i]);
        hud.sync_slot(
            *slot,
            SlotSyncInfo {
                name,
                is_party: row.is_party,
                alive: row.alive,
                hp: row.hp,
                hp_max: row.hp_max,
                mp: row.mp,
                mp_max: row.mp_max,
                ap,
            },
        );
        hud.sync_level(*slot, row.level);
    }
    for slot in cleared {
        hud.clear_slot(slot);
    }
}

/// Build the deduplicated enemy target-menu rows straight off the live
/// world's monster slots - the host-facing entry to
/// [`crate::target_picker::enemy_menu_rows`] (retail `FUN_801D9D3C`).
///
/// The engine seats a formation's monsters directly after the party
/// (`World::enter_battle`), and the pickers the hosts drive index enemies
/// the same way (`CursorRow::Enemy` slot `i` = actor `party_count + i`), so
/// a row's `first_slot` compares directly against the picker's cursor slot.
///
/// Occupancy stands in for retail's `_DAT_8007BD0C` monster-id table: a
/// dead or unseeded slot contributes `0` (retail's "no monster here"), and
/// distinct catalog names get distinct synthetic ids so identical adjacent
/// monsters collapse into one labelled run exactly as retail's identical-id
/// runs do. Names come from the same live catalog the HUD rows use.
///
/// The projected screen X each row averages is the battle actor's `+0x34` -
/// a GTE projection result the renderer owns - so the accumulator is left
/// at `0` here: every row then centres at `MENU_CENTRE_X` and the retail
/// overlap-relaxation pass in
/// [`crate::target_picker::layout_enemy_menu_rows`] spreads them. Callers
/// run that layout with their own text measurer before drawing.
pub fn battle_enemy_target_rows(
    world: &crate::world::World,
) -> Vec<crate::target_picker::EnemyMenuRow> {
    use crate::target_picker::{DEDUP_GLYPH_FALLBACK, FORMATION_SLOTS, enemy_menu_rows};
    let pc = (world.party_count.clamp(1, 3) as usize).min(world.actors.len());
    let mut ids = [0u8; FORMATION_SLOTS];
    let mut names: Vec<String> = vec![String::new(); FORMATION_SLOTS];
    for i in 0..FORMATION_SLOTS {
        let Some(a) = world.actors.get(pc + i) else {
            continue;
        };
        if a.battle.max_hp == 0 || a.battle.hp == 0 {
            continue;
        }
        let name = a
            .battle_monster_id
            .and_then(|id| world.monster_catalog.get(id))
            .map(|d| d.name.clone())
            .unwrap_or_else(|| format!("M{}", i + 1));
        let pos = names[..i].iter().position(|n| !n.is_empty() && n == &name);
        ids[i] = (pos.unwrap_or(i) + 1) as u8;
        names[i] = name;
    }
    enemy_menu_rows(
        ids,
        DEDUP_GLYPH_FALLBACK,
        |slot| names[slot as usize].clone(),
        |_| 0,
    )
}

/// The actor the current battle frame belongs to: `(actor-table slot, name)`.
///
/// Retail's battle screen keys two surfaces off this actor - the top-left
/// name plaque (`battle_chrome::name_plaque`, which reads "Vahn" on his turn
/// and the monster's name through its attack) and the full-width active-actor
/// bar, which replaces the resting per-member panels for exactly this actor.
///
/// The engine has no single "whose turn is it" cursor, so this reads the two
/// states it does have, in retail's own precedence: an open command session
/// names its acting party member; otherwise the first live monster stands in
/// for the enemy turn. That fallback is also the port's whole **monster**
/// readout - retail's HUD draws no monster gauge at all
/// (`docs/subsystems/battle-action.md`), so a monster's name is all it
/// contributes to the drawn surface.
///
/// `None` with no command session and every formation slot cleared, which is
/// what stops the plaque drawing over the victory frames.
pub fn battle_active_actor(world: &crate::world::World) -> Option<(u8, String)> {
    let pc = (world.party_count.clamp(1, 3) as usize).min(world.actors.len());
    if let Some(cmd) = world.battle_command.as_ref() {
        let names = crate::field_menu_dispatch::roster_names(world);
        let ordinal = (cmd.party_slot as usize).min(pc.saturating_sub(1));
        let name = names
            .get(world.party_roster_slot(ordinal))
            .filter(|n| !n.is_empty())
            .cloned()
            .unwrap_or_else(|| format!("P{}", ordinal + 1));
        return Some((cmd.actor, name));
    }
    const MAX_FORMATION_MONSTERS: usize = 5;
    for (i, a) in world
        .actors
        .iter()
        .enumerate()
        .skip(pc)
        .take(MAX_FORMATION_MONSTERS)
    {
        if a.battle.max_hp == 0 || a.battle.hp == 0 {
            continue;
        }
        return Some((
            i as u8,
            a.battle_monster_id
                .and_then(|id| world.monster_catalog.get(id))
                .map(|d| d.name.clone())
                .unwrap_or_else(|| format!("M{}", i - pc + 1)),
        ));
    }
    None
}

/// Element-badge index the actor-name plaque wears in front of the name, or
/// `None` for an actor that carries no badge.
///
/// Retail's plaque grows its interior by `20 + 5` when the actor has one
/// (`battle_chrome::name_plaque`), and the eight badge records `0x8B..=0x92`
/// are the only strip that fits that 20x12 slot. The **selector** is what is
/// inferred rather than pinned: no dumped caller computes the badge id, so
/// this reads the monster record's own element (`+0x1D`, the id space
/// `element_affinity` decodes) and takes badge `element` out of the strip.
/// A party member gets no badge - the captured plaques that carry one are
/// the monster frames (`Gimard`), and the party ones (`Vahn`, `Noa`) do not.
pub fn battle_plaque_element_badge(world: &crate::world::World) -> Option<u8> {
    let (slot, _) = battle_active_actor(world)?;
    let pc = (world.party_count.clamp(1, 3) as usize).min(world.actors.len());
    if (slot as usize) < pc {
        return None;
    }
    let actor = world.actors.get(slot as usize)?;
    let def = world
        .monster_catalog
        .get(actor.battle_monster_id?)
        .filter(|d| (d.element as usize) < legaia_asset::element_affinity::ELEMENT_COUNT)?;
    Some(def.element)
}

/// Is the port's encounter-transition banner enabled?
///
/// The "ENCOUNTER!" head line has no retail counterpart - retail's
/// `Field -> Battle` edge draws no banner at all - so it is off by default
/// and rides the same shared toggle as the diagnostic HUD rows:
/// `LEGAIA_DIAG_HUD` set to anything but `0` / empty. Reading the
/// environment keeps both hosts on one answer; on wasm the variable never
/// exists and the banner stays off.
pub fn encounter_banner_enabled() -> bool {
    std::env::var("LEGAIA_DIAG_HUD")
        .map(|v| !v.is_empty() && v != "0")
        .unwrap_or(false)
}

/// Formation label for the encounter-transition banner, reusing the battle
/// HUD's monster naming: the live catalog name when the formation resolved
/// one, `M<n>` otherwise. Slots with `max_hp == 0` (unseeded / cleared) are
/// skipped, so a formation that has not resolved HP yet yields an empty label
/// and the banner shows only its "ENCOUNTER!" head line. Shared by both hosts
/// (armed on each `Field -> Battle` mode edge).
///
/// The banner itself is a port invention - retail shows nothing on the
/// `Field -> Battle` edge - so hosts arm it only when
/// [`encounter_banner_enabled`] says so.
pub fn encounter_banner_label(world: &crate::world::World) -> String {
    let pc = (world.party_count.clamp(1, 3) as usize).min(world.actors.len());
    // `World::actors` is the fixed 64-slot table, not a battle-sized list;
    // a formation seats at most 5 monsters directly after the party
    // (`World::enter_battle`), so only those slots can be formation members.
    const MAX_FORMATION_MONSTERS: usize = 5;
    let mut names: Vec<String> = Vec::new();
    for (i, a) in world
        .actors
        .iter()
        .enumerate()
        .skip(pc)
        .take(MAX_FORMATION_MONSTERS)
    {
        if a.battle.max_hp == 0 {
            continue;
        }
        let name = a
            .battle_monster_id
            .and_then(|id| world.monster_catalog.get(id))
            .map(|d| d.name.clone())
            .unwrap_or_else(|| format!("M{}", i - pc + 1));
        names.push(name);
    }
    names.join("  ")
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
        s.set_status_icons([StatusKind::Faint, StatusKind::Toxic, StatusKind::Confuse]);
        assert_eq!(
            s.status_icons,
            vec![StatusKind::Toxic, StatusKind::Confuse, StatusKind::Faint]
        );
    }

    #[test]
    fn slot_hud_status_icons_dedup_repeated_kinds() {
        let mut s = BattleSlotHud::new();
        s.set_status_icons([StatusKind::Toxic, StatusKind::Toxic, StatusKind::Sleep]);
        assert_eq!(s.status_icons, vec![StatusKind::Toxic, StatusKind::Sleep]);
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
        let p = DamagePopup::damage(0, 0).with_status(StatusKind::Sleep);
        assert_eq!(p.status, Some(StatusKind::Sleep));
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
    fn a_ninth_simultaneous_popup_overwrites_the_first_and_len_is_ring_bounded() {
        let mut h = BattleHud::new();
        for i in 0..9u16 {
            h.push_popup(DamagePopup::damage(0, 100 + i));
        }
        assert_eq!(
            h.popups.len(),
            POPUP_RING_SLOTS,
            "len never exceeds the 8-slot ring"
        );
        assert!(
            !h.popups.iter().any(|p| p.amount == 100),
            "the ninth push overwrites the first popup"
        );
        assert!(
            h.popups.iter().any(|p| p.amount == 108),
            "the ninth popup is present"
        );
        assert_eq!(h.popup_ring.pushed, 9, "ctx+0x273 counts every push");
        assert_eq!(h.popup_ring.cursor, 1, "ctx+0x262 wrapped past slot 0");
        for i in 0..20u16 {
            h.push_popup(DamagePopup::damage(1, 500 + i));
        }
        assert_eq!(h.popups.len(), POPUP_RING_SLOTS, "still bounded after 29");
    }

    #[test]
    fn the_overwrite_target_is_the_cursor_slot_not_the_oldest_live_popup() {
        // Retail's cursor is independent of expiry: with eight pushed and a
        // mid-ring slot expired, the ninth push still lands on slot 0 - the
        // FIRST popup - even though a dead slot exists elsewhere.
        let mut h = BattleHud::new();
        for i in 0..8u16 {
            let life = if i == 4 { 1 } else { 60 };
            h.push_popup(DamagePopup::damage(0, 100 + i).with_lifetime(life));
        }
        h.tick(); // decrements the short-lived fifth popup to zero
        h.tick(); // drops it - slot 4 is now dead
        assert_eq!(h.popups.len(), 7);
        h.push_popup(DamagePopup::damage(0, 999));
        assert_eq!(h.popups.len(), 7, "slot 0 is replaced, not appended");
        assert!(
            !h.popups.iter().any(|p| p.amount == 100),
            "the first popup (ring slot 0) is the one overwritten"
        );
        assert!(h.popups.iter().any(|p| p.amount == 999));
    }

    #[test]
    fn clear_popups_resets_the_ring_cursor_with_the_display_list() {
        let mut h = BattleHud::new();
        for _ in 0..5 {
            h.push_damage(0, 10);
        }
        h.clear_popups();
        assert!(h.popups.is_empty());
        assert_eq!(h.popup_ring, DamagePopupRing::default());
        h.push_damage(1, 20);
        assert_eq!(h.popup_ring.cursor, 1, "a fresh encounter starts at slot 0");
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
        tracker.apply(2, StatusKind::Toxic);
        tracker.apply(2, StatusKind::Venom);
        h.sync_status(2, &tracker);
        // Sorted order: Toxic (0) before Venom (2).
        assert_eq!(
            h.slots[2].status_icons,
            vec![StatusKind::Toxic, StatusKind::Venom]
        );
    }

    /// The status CLUT recolour reaches VRAM through the one per-slot call
    /// every host already makes. Drop the `status_clut.arm(..)` line from
    /// [`BattleHud::sync_status`] and this fails at the `armed()` assert -
    /// the kernel is intact but nothing ever asks it to run.
    #[test]
    fn stone_reaches_the_party_clut_row_through_sync_status() {
        use crate::battle_status_clut::{PARTY_CLUT_ENTRIES, PARTY_CLUT_ROW_BASE};

        let mut vram = legaia_tim::Vram::new();
        let row = PARTY_CLUT_ROW_BASE + 1;
        // A resident party palette: STP-set, deliberately not grey.
        let base: Vec<u16> = (0..PARTY_CLUT_ENTRIES)
            .map(|i| 0x8000 | ((i as u16 % 31) + 1) | (0x0A << 5) | (0x1F << 10))
            .collect();
        let bytes: Vec<u8> = base.iter().flat_map(|w| w.to_le_bytes()).collect();
        vram.write_clut_row(0, row, &bytes);

        let mut h = BattleHud::new();
        let mut tracker = StatusEffectTracker::new();

        // Baseline: an ordinary ailment must not touch the palette.
        tracker.apply(1, StatusKind::Venom);
        h.sync_status(1, &tracker);
        assert!(!h.status_clut.armed());
        assert!(!h.status_clut.step(&mut vram));

        tracker.apply(1, StatusKind::Stone);
        h.sync_status(1, &tracker);
        assert!(h.status_clut.armed(), "the Stone edge arms actor +0x220");
        assert!(h.status_clut.step(&mut vram), "the pass writes VRAM");

        for x in 0..PARTY_CLUT_ENTRIES {
            let w = vram.pixel(x, row as usize);
            let (r, g, b) = (w & 0x1F, (w >> 5) & 0x1F, (w >> 10) & 0x1F);
            assert_eq!((r, g, b), (r, r, r), "entry {x} is not grey");
        }
        assert_ne!(
            (0..PARTY_CLUT_ENTRIES)
                .map(|x| vram.pixel(x, row as usize))
                .collect::<Vec<_>>(),
            base,
            "the row actually changed"
        );

        // Held affliction: no re-run, so the row is stable frame to frame.
        h.sync_status(1, &tracker);
        assert!(!h.status_clut.armed());
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
        h.push_status(0, StatusKind::Sleep);
        assert_eq!(h.popups[0].amount, 0);
        assert_eq!(h.popups[0].status, Some(StatusKind::Sleep));
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

    #[test]
    fn status_kind_letter_uses_first_char_with_collisions_lowercased() {
        assert_eq!(status_kind_letter(StatusKind::Toxic), b'T');
        assert_eq!(status_kind_letter(StatusKind::Numb), b'N');
        assert_eq!(status_kind_letter(StatusKind::Sleep), b'S');
        assert_eq!(status_kind_letter(StatusKind::Confuse), b'C');
        // Collisions take the lowercase form.
        assert_eq!(status_kind_letter(StatusKind::Curse), b'c');
        assert_eq!(status_kind_letter(StatusKind::Stone), b's');
        assert_eq!(status_kind_letter(StatusKind::Faint), b'F');
    }

    #[test]
    fn slot_hud_status_letters_returns_one_byte_per_icon() {
        let mut s = BattleSlotHud::new();
        s.set_status_icons([StatusKind::Toxic, StatusKind::Sleep]);
        let letters = s.status_letters();
        assert_eq!(letters, vec![b'T', b'S']);
    }

    #[test]
    fn slot_views_filters_inactive_slots() {
        let mut hud = BattleHud::new();
        hud.sync_slot(
            0,
            SlotSyncInfo {
                name: "Vahn",
                is_party: true,
                alive: true,
                hp: 100,
                hp_max: 100,
                mp: 30,
                mp_max: 30,
                ap: None,
            },
        );
        // Slot 1 untouched - should not appear.
        let views = hud.slot_views();
        assert_eq!(views.len(), 1);
        assert_eq!(views[0].slot, 0);
        assert_eq!(views[0].name, "Vahn");
    }

    #[test]
    fn slot_views_carries_the_single_retail_status_element() {
        let mut hud = BattleHud::new();
        hud.sync_slot(
            0,
            SlotSyncInfo {
                name: "Vahn",
                is_party: true,
                alive: true,
                hp: 100,
                hp_max: 100,
                mp: 30,
                mp_max: 30,
                ap: None,
            },
        );
        hud.sync_level(0, 12);
        hud.slots[0].set_status_icons([StatusKind::Toxic, StatusKind::Confuse]);
        let views = hud.slot_views();
        // Two kinds, ONE element: retail's ladder puts the delegation group
        // (`0x0380` -> sprite `0x1C`) above Toxic (`0x0002` -> `0x19`).
        assert_eq!(views[0].status_sprite, 0x1C);
        assert_eq!(views[0].level, 12);
    }

    #[test]
    fn slot_status_element_is_the_packed_word_through_the_retail_ladder() {
        let mut s = BattleSlotHud {
            alive: true,
            level: 7,
            ..Default::default()
        };
        // No ailment: the base marker + the level count, not a sprite.
        assert_eq!(s.status_display_flags(), 0);
        assert_eq!(s.status_element(), StatusIcon::BaseWithCount);
        assert_eq!(s.status_sprite(), 0);

        // Venom alone packs bit 0 and selects sprite 0x18.
        s.set_status_icons([StatusKind::Venom]);
        assert_eq!(s.status_display_flags(), 0x0001);
        assert_eq!(s.status_sprite(), 0x18);

        // Adding Stone changes the *element* without changing the set order:
        // the ladder tests 0x0004 first.
        s.set_status_icons([StatusKind::Venom, StatusKind::Stone]);
        assert_eq!(s.status_display_flags(), 0x0005);
        assert_eq!(s.status_sprite(), 0x1A);

        // A KO'd slot takes the zero-HP arm whatever else is set - retail
        // tests `+0x6CE` before it inspects a bit.
        s.alive = false;
        assert_eq!(s.status_sprite(), 0x20);
    }

    #[test]
    fn popup_views_emits_one_per_popup() {
        let mut hud = BattleHud::new();
        hud.push_damage(0, 50);
        hud.push_heal(1, 25);
        let views = hud.popup_views();
        assert_eq!(views.len(), 2);
        assert_eq!(views[0].slot, 0);
        assert_eq!(views[0].amount, 50);
        assert!(!views[0].is_heal);
        assert_eq!(views[1].slot, 1);
        assert!(views[1].is_heal);
    }

    #[test]
    fn popup_views_carries_status_letter_when_set() {
        let mut hud = BattleHud::new();
        hud.push_status(2, StatusKind::Faint);
        let views = hud.popup_views();
        assert_eq!(views[0].status_letter, Some(b'F'));
    }

    #[test]
    fn log_accent_color_distinct_per_variant() {
        assert_ne!(
            log_accent_color(LogAccent::Neutral),
            log_accent_color(LogAccent::Party)
        );
        assert_ne!(
            log_accent_color(LogAccent::Highlight),
            log_accent_color(LogAccent::Heal)
        );
    }

    /// The drawn-bar fill index follows retail's whole-gauge precedence
    /// (FUN_80046A20 via `engine-vm::battle_gauge`): death first, then the
    /// status override, then per-bar fill bands.
    #[test]
    fn gauge_fill_indices_follow_the_retail_precedence() {
        let mut s = BattleSlotHud::new();
        s.alive = true;
        s.hp = 80;
        s.hp_max = 100;
        s.mp = 5;
        s.mp_max = 40;
        // HP high band (7), MP low band (9), coloured independently.
        assert_eq!(s.gauge_fill_indices(), (7, 9));
        // Any active status forces the whole gauge to the override colour.
        s.set_status_icons([StatusKind::Toxic]);
        assert_eq!(s.gauge_fill_indices(), (3, 3));
        // Death (displayed HP zero) wins over everything.
        s.hp = 0;
        assert_eq!(s.gauge_fill_indices(), (2, 2));
        s.status_icons.clear();
        assert_eq!(s.gauge_fill_indices(), (2, 2));
    }

    /// Identical adjacent monsters must collapse into one dedup-labelled
    /// retail row (FUN_801D9D3C via `target_picker::enemy_menu_rows`), and a
    /// dead slot must contribute nothing (retail's zero id).
    #[test]
    fn enemy_target_rows_collapse_runs_and_skip_dead_slots() {
        use crate::monster_catalog::MonsterDef;
        use crate::world::{Actor, World};
        let mut w = World::new();
        while w.actors.len() < 8 {
            w.actors.push(Actor::default());
        }
        w.party_count = 1;
        w.monster_catalog
            .insert(MonsterDef::new(7, "Gimard", 40, 5));
        w.monster_catalog
            .insert(MonsterDef::new(9, "Zenoir", 40, 5));
        // Slots 1..=3: Gimard, Gimard, Zenoir. Slot 2's twin is dead.
        for (i, (id, hp)) in [(7u16, 40u16), (7, 40), (9, 40)].iter().enumerate() {
            let a = &mut w.actors[1 + i];
            a.battle.hp = *hp;
            a.battle.max_hp = 40;
            a.battle.liveness = 1;
            a.battle_monster_id = Some(*id);
        }
        let rows = battle_enemy_target_rows(&w);
        assert_eq!(rows.len(), 2, "the Gimard pair collapses into one row");
        assert_eq!(rows[0].first_slot, 0);
        assert_eq!(rows[0].members, 2);
        // The second member overwrites the label's final character with the
        // dedup glyph (fallback 'A'), keeping the byte length.
        assert_eq!(rows[0].label, "GimarA");
        assert_eq!(rows[1].label, "Zenoir");
        assert_eq!(rows[1].first_slot, 2);

        // Kill the second Gimard: the run breaks and no dedup glyph remains.
        w.actors[2].battle.hp = 0;
        let rows = battle_enemy_target_rows(&w);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].label, "Gimard");
        assert_eq!(rows[0].members, 1);
    }

    /// The HUD row must carry the **ramping** HP (`BattleActor::hp_display`,
    /// retail actor `+0x172` / FUN_80047430), not the live value the sim
    /// already settled - otherwise the drain animation is computed every
    /// frame and never shown.
    #[test]
    fn sync_reads_ramped_display_hp_not_live_hp() {
        use crate::world::{Actor, World};
        let mut w = World::new();
        while w.actors.len() < 4 {
            w.actors.push(Actor::default());
        }
        w.party_count = 1;
        w.actors[0].battle.hp = 100;
        w.actors[0].battle.max_hp = 200;
        w.actors[0].battle.liveness = 1;
        // Mid-ramp: live HP already at 100, bar still showing 160.
        w.actors[0].battle.hp_display = Some(160);
        // Monster slot mid-ramp too.
        w.actors[1].battle.hp = 10;
        w.actors[1].battle.max_hp = 50;
        w.actors[1].battle.liveness = 1;
        w.actors[1].battle.hp_display = Some(30);

        let mut hud = BattleHud::new();
        sync_battle_hud_rows(&mut hud, &w);
        assert_eq!(hud.slots[0].hp, 160, "party row shows the ramping bar");
        assert_eq!(hud.slots[1].hp, 30, "monster row shows the ramping bar");

        // Settled (`None`) falls back to live HP.
        w.actors[0].battle.hp_display = None;
        sync_battle_hud_rows(&mut hud, &w);
        assert_eq!(hud.slots[0].hp, 100, "settled bar reads live HP");
    }

    #[test]
    fn log_views_resolves_color_from_accent() {
        let mut hud = BattleHud::new();
        hud.push_log("hi", LogAccent::Heal);
        let views = hud.log_views();
        assert_eq!(views[0].text, "hi");
        assert_eq!(views[0].color_rgba, log_accent_color(LogAccent::Heal));
    }
}
