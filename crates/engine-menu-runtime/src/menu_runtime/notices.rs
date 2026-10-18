//! Timed notices the menu runtime owns: the point-card toast, the spell-level notice
//! and the art-learned notice.

use super::*;

impl MenuRuntime {
    /// The live Point Card toast's credit, or `None` when window 31 is not
    /// up. A host paints the window while this is `Some`; the number it
    /// prints is the **bank** (`World::minigames.point_card`), not this delta - retail
    /// hands `_DAT_800845B4` to the renderer.
    pub fn point_card_toast(&self) -> Option<i32> {
        self.point_card_toast
    }

    /// The live spell level-up notice (retail window 7), or `None` when no
    /// menu cast leveled a spell. A host paints the window while this is
    /// `Some` and routes the pad through
    /// [`Self::dismiss_spell_level_notice`] instead of the screen below.
    pub fn spell_level_notice(&self) -> Option<&legaia_engine_battle::magic_xp::SpellLevelNotice> {
        self.spell_level_notice.as_ref()
    }

    /// The live art-learned notice (retail window 8), or `None`.
    pub fn art_learned_notice(
        &self,
    ) -> Option<&legaia_engine_menus::pause_screens::ArtLearnedNotice> {
        self.art_learned_notice.as_ref()
    }

    /// Park a pause-menu art-learned notice on the window-8 beat - hosts
    /// take it off [`legaia_engine_core::world::MenuState::pending_art_notice`] after
    /// applying a finished Items screen.
    pub fn arm_art_learned_notice(
        &mut self,
        notice: legaia_engine_menus::pause_screens::ArtLearnedNotice,
    ) {
        self.art_learned_notice = Some(notice);
    }

    /// Arm whichever of the window-7 / window-8 notices a finished pause-menu
    /// sub-session surfaced ([`crate::field_menu_dispatch::finish_subsession`]),
    /// taking them out of `finished`.
    pub fn arm_finished_notices(
        &mut self,
        finished: &mut crate::field_menu_dispatch::FinishedSubsession,
    ) {
        if let Some(n) = finished.spell_level_notice.take() {
            self.arm_spell_level_notice(n);
        }
        if let Some(n) = finished.art_learned_notice.take() {
            self.arm_art_learned_notice(n);
        }
    }

    /// One frame of the window-8 hold: the confirm or cancel mask closes it.
    /// Returns `true` while the notice owns the pad.
    pub fn dismiss_art_learned_notice(
        &mut self,
        cross: bool,
        circle: bool,
        triangle: bool,
    ) -> bool {
        if self.art_learned_notice.is_none() {
            return false;
        }
        if cross || circle || triangle {
            self.art_learned_notice = None;
        }
        true
    }

    /// Park a menu-cast level-up on the window-7 beat - the engine's
    /// `FUN_80035C00` (hosts arm it with
    /// [`crate::field_menu_dispatch::apply_spell_outcome`]'s return).
    pub fn arm_spell_level_notice(
        &mut self,
        notice: legaia_engine_battle::magic_xp::SpellLevelNotice,
    ) {
        self.spell_level_notice = Some(notice);
    }

    /// One frame of the window-7 hold: retail's cast sub-screens stall
    /// until the confirm or cancel mask fires, then close the window and
    /// return to the list. Returns `true` while the notice owns the pad
    /// (whether or not this press dismissed it) so the caller skips the
    /// screen below.
    pub fn dismiss_spell_level_notice(
        &mut self,
        cross: bool,
        circle: bool,
        triangle: bool,
    ) -> bool {
        if self.spell_level_notice.is_none() {
            return false;
        }
        if cross || circle || triangle {
            self.spell_level_notice = None;
        }
        true
    }

    /// Credit the Point Card for a landed recipient-picker purchase and arm
    /// the window-31 paint flag.
    ///
    /// Gated on **holding** the card, not on the credit being non-zero:
    /// retail's toast is `FUN_80042F4C(0xFE) != 0`, so a sub-20-gold buy
    /// still shows the window with the bank unchanged. `credit` is the
    /// session's own `(price / 20) * 1`.
    pub(super) fn arm_point_card_toast(&mut self, world: &mut World, credit: i32) {
        if !world.point_card_held() {
            return;
        }
        world.minigames.point_card =
            legaia_engine_menus::shop::apply_point_card(world.minigames.point_card, credit);
        self.point_card_toast = Some(credit);
    }
}
