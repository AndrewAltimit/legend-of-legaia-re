//! Battle HUD model - renderer-agnostic UI state for the in-battle screen.
//!
//! Holds per-slot HP / MP / AP / status-icon state plus a queue of damage
//! popups and battle-event log lines ([`BattleHud`]: the slot rows, popup
//! ring, log and combo readout, and the view structs it projects).
//! `legaia_engine_ui::battle_hud_draws_for` turns one of these into a
//! `Vec<TextDraw>` for the GPU pipeline; engines that render via a
//! different path (web / terminal) can read the same struct directly.
//! The `World` projections that feed it live in
//! `legaia_engine_battle_hud`.
//!
//! ## Frame timing
//!
//! Damage popups carry a `frames_remaining` counter; [`BattleHud::tick`]
//! decrements it each frame and drops popups whose counter reaches zero.
//! Default lifetime is 60 frames (~1 s at PSX 60 Hz). Simultaneous popups
//! are bounded by retail's 8-slot ring (`FUN_801F44A0`, delegated to
//! `legaia_engine_battle_vm::battle_gauge_rearm::DamagePopupRing` by
//! [`BattleHud::push_popup`]): a ninth push overwrites the slot the ring
//! cursor names instead of growing the list.

use crate::ap_gauge::ApGauge;
use legaia_engine_battle_vm::battle_gauge_rearm::DamagePopupRing;
use legaia_engine_battle_vm::battle_value_readout::ComboStyle;
use legaia_engine_battle_vm::battle_value_readout::{
    COMBO_SLIDE_FRAMES, COMBO_SLIDE_IN_X, combo_slide,
};
use legaia_engine_battle_vm::status_effects::{StatusEffectTracker, StatusIcon, StatusKind};

mod slot;
mod subdraw;
#[cfg(test)]
mod tests;

pub use slot::*;
pub use subdraw::*;

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
    /// Bounded at [`POPUP_RING_SLOTS`](legaia_engine_battle_vm::battle_gauge_rearm::POPUP_RING_SLOTS): every push routes through
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
    /// `FUN_8004CE2C`. Armed by [`Self::sync_status`] (which every host
    /// already calls once per slot per frame) and
    /// drained by the host's mid-battle VRAM pass through
    /// [`crate::battle_status_clut::StatusClutState::step`].
    pub status_clut: crate::battle_status_clut::StatusClutState,
    /// The right-hand combo counter the current action has put up - `N HIT`
    /// / `TOTAL x` for a physical chain, `DAMAGE x` for a cast - or `None`
    /// while nothing has landed. Accumulated by [`Self::push_popup`] from
    /// the per-hit damage FX, armed and torn down per action by
    /// [`Self::arm_combo`] (which [`sync_battle_hud_rows`] drives from the
    /// live world). Retail closes the cluster in the action SM's `0x51`
    /// fade-down (`FUN_801D8DE8(0x50, 1)`, placement record 80).
    pub combo: Option<ComboReadout>,
    /// The style the action in flight would draw its hits in; `None`
    /// outside an action, which is what keeps a stray popup from raising a
    /// cluster on its own.
    combo_style: Option<ComboStyle>,
    /// The acting actor the cluster belongs to - a new actor is a new
    /// cluster even when the style repeats.
    combo_actor: Option<u8>,
}

/// The combo counter cluster's running values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ComboReadout {
    pub style: ComboStyle,
    /// Landed hits so far (drawn only for [`ComboStyle::HitTotal`]).
    pub hits: u16,
    /// Running damage the value row shows.
    pub total: u32,
    /// Frames since the latest landed hit re-opened the cluster - drives
    /// the slide-in, which every hit starts over ([`BattleHud::push_popup`])
    /// - or, once [`Self::closing`], since the close began.
    pub age: u16,
    /// The `0x51` fade-down closed the cluster: it slides back out to its
    /// off-screen seat over the same sixteen frames
    /// ([`BattleHud::close_combo_on_fade_down`]).
    pub closing: bool,
}

impl ComboReadout {
    /// Horizontal offset from the rest seats this frame
    /// (`battle_value_readout::combo_slide`; the close runs it backwards).
    pub fn slide(&self) -> i32 {
        if self.closing {
            COMBO_SLIDE_IN_X - combo_slide(self.age)
        } else {
            combo_slide(self.age)
        }
    }

    /// Has the slide-in finished?
    pub fn settled(&self) -> bool {
        self.age >= COMBO_SLIDE_FRAMES
    }
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
            combo: None,
            combo_style: None,
            combo_actor: None,
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
    /// [`POPUP_RING_SLOTS`](legaia_engine_battle_vm::battle_gauge_rearm::POPUP_RING_SLOTS) popups exist at once and a ninth simultaneous
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
        // The combo cluster counts every landed damage hit of the action in
        // flight: one more `HIT`, its amount into `TOTAL` / `DAMAGE`. Heals
        // and status tags carry no numeral on the cluster.
        //
        // Every landed hit also re-opens the cluster from its off-screen
        // seat. The melee kernel's HP write raises `DAT_8007B64C = 0x78`
        // beside the hit's damage in `DAT_8007BD14` (`FUN_801EC3E4`,
        // `0x801EEA64..0x801EEA78`), and the readout pass the action SM's
        // prologue calls every pass (`FUN_801E805C`, from `0x801E2A70`)
        // answers a raised flag with `FUN_801D8DE8(0x50, 0)` and clears it
        // (`0x801E808C..0x801E80B0`): mode `0` spawns placement record 80 at
        // seat A and registers a fresh glide to seat B
        // (`0x801D92E8..0x801D93D8`), so the slide starts over.
        // `battle_melee_hit_spark` holds the third hit's numeral `15` with the
        // cluster (`3 HIT`, `TOTAL 29`) twelve vsyncs into its slide.
        if !popup.is_heal
            && popup.status.is_none()
            && popup.amount > 0
            && let Some(style) = self.combo_style
        {
            let c = self.combo.get_or_insert(ComboReadout {
                style,
                hits: 0,
                total: 0,
                age: 0,
                closing: false,
            });
            c.style = style;
            c.hits = c.hits.saturating_add(1);
            c.total = c.total.saturating_add(u32::from(popup.amount));
            c.age = 0;
        }
    }

    /// Arm (or tear down) the combo cluster for the action the world is in.
    ///
    /// `style` is [`battle_combo_style`]'s answer this frame and `actor` the
    /// acting slot: `None` (no action in flight) or a change of actor drops
    /// the cluster, matching retail's per-action open / `0x51` close. Called
    /// once per simulation tick by [`sync_battle_hud_rows`].
    pub fn arm_combo(&mut self, style: Option<ComboStyle>, actor: u8) {
        let owner = style.map(|_| actor);
        if owner != self.combo_actor {
            self.combo = None;
        }
        self.combo_style = style;
        self.combo_actor = owner;
    }

    /// Close the cluster in the action SM's `0x51` fade-down, as retail
    /// does: the band's teardown (countdown under `0xC`, latched once per
    /// action, `0x801E6158..0x801E6214`) hard-resets the element list
    /// (`FUN_801D99BC` at `0x801E6170`) and, when the action landed damage
    /// (`_DAT_8007BD14 != 0`), re-spawns record 80 with mode `1` -
    /// `FUN_801D8DE8(0x50, 1)` at `0x801E6360` - which places it at seat B
    /// and glides it to seat A (`0x801D935C..0x801D93D8`), off the right
    /// edge. `torn_down` is the engine's twin of that latch
    /// (`BattleActionCtx::done_ui_torn_down`); the continuation band `0x52`
    /// lies past it, so the cluster is closed there whatever the latch reads.
    /// `rim_elm_gimard_seru_capture_after` (`0x52`) shows no cluster.
    pub fn close_combo_on_fade_down(&mut self, action_state: u8, torn_down: bool) {
        use legaia_engine_battle_vm::battle_action::ActionState as S;
        let closed = (action_state == S::DoneFadeDown.as_byte() && torn_down)
            || action_state == S::DoneSeruAbsorb.as_byte();
        if closed && let Some(c) = self.combo.as_mut().filter(|c| !c.closing) {
            c.closing = true;
            c.age = 0;
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
        self.combo = None;
        self.combo_actor = None;
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
        if let Some(c) = &mut self.combo {
            c.age = c.age.saturating_add(1);
        }
        // A closed cluster is gone once its slide-out has run.
        if self
            .combo
            .is_some_and(|c| c.closing && c.age >= COMBO_SLIDE_FRAMES)
        {
            self.combo = None;
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
    /// `engine-ui::battle_hud_draws_for`. Owned data - engines that
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
/// Engines convert into `legaia_engine_ui::HudSlotView` trivially:
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
