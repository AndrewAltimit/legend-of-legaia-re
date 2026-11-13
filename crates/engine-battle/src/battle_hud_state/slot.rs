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
    /// [`Self::status_element`]). `0` means "unknown", and the hosts draw
    /// the bare marker rather than "LV 0".
    pub level: u8,
    /// The slot's whole retail `+0x16E` status word as the world holds it
    /// (`World::raw_status_word`: the raw half a cast or a hit's status arm
    /// writes, OR the tracker's packed bits), seated by
    /// [`BattleHud::sync_status_word`]. `None` for a host that syncs only the
    /// tracker, which then reads [`Self::status_icons`] alone.
    pub status_word: Option<u16>,
    /// The readout draws `?` cells in place of the HP / MP figures: retail's
    /// readout spawner `FUN_801D8DE8` registers the four number records with
    /// type `5` instead of `2` for present-party character id `4` (Terra),
    /// and the per-frame draw `FUN_80031D00` blits separator glyph `0xC`
    /// (`?`) once per cell for a type-`5` record, never reading the value
    /// ([`BattleHud::sync_hidden_readout`]). Her roster panel draws no
    /// level / status element either: its frame style is her id plus `0x32`,
    /// `0x36`, and `FUN_8002C69C` runs the element `FUN_8002C2E4` for styles
    /// `0x33..=0x35` only.
    pub hidden_readout: bool,
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
    /// ratios. The status flag retail reads is the whole actor `+0x16E` word
    /// (`lhu v0,0x16e(a1)` / `beq v0,zero` at `0x80046AE0` and `0x80046C24`),
    /// so any bit greys the gauge to `3`: a Curse a cast wrote raw, a rotted
    /// limb, a Venom from an enemy special. [`Self::status_display_flags`]
    /// is that word.
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
            self.status_display_flags(),
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
    /// The world's word when a host seated one ([`Self::status_word`]): the
    /// raw half matters, because retail's appliers write bits the tracker
    /// never holds (a cast's Curse, a hit's Venom), and the status element
    /// and the gauge read the word.
    ///
    /// Without it, built from the typed [`Self::status_icons`] set through
    /// [`legaia_engine_battle_vm::status_effects::pack_display_flags`]. Rot packs
    /// its **whole** limb group there rather than a single rolled bit: the HUD
    /// snapshot carries kinds, not instances, and the ladder only tests the
    /// group. Faint contributes no bit - it is the zero-HP arm of the ladder.
    pub fn status_display_flags(&self) -> u16 {
        self.status_word.unwrap_or_else(|| {
            self.status_icons
                .iter()
                .fold(0u16, |w, k| w | k.display_bit())
        })
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
    /// The ring timer `ctx[+0x85C]` as the renderer `FUN_801DF6B8` left it
    /// on its last draw: zeroed on push, stepped `0x10 * DAT_1F800393` by
    /// each draw (`0x801DF7B8..0x801DF7C8`), once a battle frame.
    pub timer: i32,
    /// The frame step the last draw stepped [`Self::timer`] by; `0` until
    /// the first draw.
    pub drawn_step: u8,
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
            timer: 0,
            drawn_step: 0,
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

    /// Step the ring timer as one draw of `FUN_801DF6B8` does, on a battle
    /// frame of `step` vsyncs.
    ///
    /// REF: FUN_801DF6B8 (the timer step, `0x801DF7B8..0x801DF7C8`)
    pub fn step_timer(&mut self, step: u8) {
        let step = step.max(1);
        self.timer += i32::from(step) * 0x10;
        self.drawn_step = step;
    }

    /// The timer the last draw read before its step - what the anchor's
    /// rise reads - in vsyncs of [`POPUP_TIMER_STEP`](legaia_engine_battle_vm::battle_value_readout::POPUP_TIMER_STEP),
    /// with that draw's step: `None` before the first draw.
    pub fn drawn_age(&self) -> Option<(u16, u8)> {
        (self.drawn_step != 0).then(|| {
            let before = self.timer / 0x10 - i32::from(self.drawn_step);
            (before.max(0) as u16, self.drawn_step)
        })
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
