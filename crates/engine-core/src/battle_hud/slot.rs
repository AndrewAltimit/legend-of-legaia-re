//! One battle slot's HUD row (`BattleSlotHud`), its status-icon letters, and the damage popup.

use super::*;

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
        legaia_engine_battle_vm::battle_gauge::gauge_colors(
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
    /// [`legaia_engine_battle_vm::status_effects::pack_display_flags`]. Rot packs
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
        legaia_engine_battle_vm::status_effects::status_icon(
            self.status_display_flags(),
            self.alive,
        )
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
