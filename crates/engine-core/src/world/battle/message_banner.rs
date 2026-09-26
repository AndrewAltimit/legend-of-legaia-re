//! The battle **message banner**: the top-of-screen line two HUD screen
//! elements carry - `0x59` (a Seru absorbed on a killing blow) and `0x65` (a
//! Seru spell's magic level went up).
//!
//! Both are ordinary screen-element records of the `0x80076C10` placement
//! table, raised and unloaded through the element spawner `FUN_801D8DE8`
//! like every other HUD element, and both share one string: the battle-result
//! message builder `FUN_801D84C0` points each record's `+0x14` content word at
//! the context's message buffer `ctx + 0x1F9` (`sw v1,0x86c(a0)` and
//! `sw v1,0x98c(a0)` at `0x801D850C` / `0x801D8514`, `a0 = 0x80076C10`, i.e.
//! record `0x59 * 0x18 + 0x14` and record `0x65 * 0x18 + 0x14`). The two
//! records carry the same geometry - seat A `(16, -24)`, seat B `(16, 14)`,
//! width `280`, kind `3` - so a raise (`mode 0`, spawn at seat A) glides the
//! framed line down onto the pen the port's top banner already uses, and the
//! unload (`mode 1`) glides it back out.
//!
//! What differs is who writes the buffer:
//!
//! * `0x59`: the spawner's own `0x59` arm, on `mode == 0` only
//!   (`bne s5,zero` at `0x801D914C`), composes `prefix[char - 1] +
//!   spell_name[seru + 0x80] + suffix` ([`legaia_asset::absorb_caption`]).
//! * `0x65`: `FUN_801F452C` composes `<spell>'s magic level increased.`
//!   before the raise ([`crate::magic_xp::magic_level_increased_message`]).
//!
//! The port keeps the composed line on [`crate::world::BattleState::message_banner`]
//! from the raise to the matching unload, and both hosts draw it through
//! [`crate::battle_hud::battle_banner_message`] into the top banner widget.

use super::*;

/// Screen element of the Seru-absorb banner.
pub const ABSORB_BANNER_ELEMENT: u8 = 0x59;

/// Screen element of the magic-level-increased banner.
pub const MAGIC_LEVEL_BANNER_ELEMENT: u8 = 0x65;

/// One raised message banner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BattleMessageBanner {
    /// The screen element that raised it (`0x59` / `0x65`) - the unload that
    /// retires it names the same id.
    pub element: u8,
    /// The composed line (retail's `ctx + 0x1F9` buffer).
    pub text: String,
}

impl World {
    /// The `FUN_801D8DE8` raise / unload for the two message elements. Other
    /// element ids are not this banner's and are ignored.
    ///
    /// REF: FUN_801D8DE8 (`0x801D9154..0x801D91D0`, the `0x59` compose arm)
    pub(in crate::world) fn message_banner_ui_element(&mut self, element: u8, mode: u8) {
        if element != ABSORB_BANNER_ELEMENT && element != MAGIC_LEVEL_BANNER_ELEMENT {
            return;
        }
        if mode & 1 != 0 {
            if self
                .battle
                .message_banner
                .as_ref()
                .is_some_and(|b| b.element == element)
            {
                self.battle.message_banner = None;
            }
            return;
        }
        if element == ABSORB_BANNER_ELEMENT
            && let Some(text) = self.absorb_banner_text()
        {
            self.battle.message_banner = Some(BattleMessageBanner { element, text });
        }
    }

    /// Stage the magic-level line and raise element `0x65` - the port's seat
    /// of `FUN_801F452C`'s compose followed by `FUN_801E70BC`'s raise.
    pub(in crate::world) fn raise_magic_level_banner(&mut self, text: String) {
        self.battle.message_banner = Some(BattleMessageBanner {
            element: MAGIC_LEVEL_BANNER_ELEMENT,
            text,
        });
    }

    /// The `0x59` line for the Seru staged in `ctx[+0x269]` (the engine's
    /// `multi_cast_gate`) and the acting seat's character. `None` without a
    /// staged Seru. Without the disc caption table the line degrades to the
    /// Seru's name alone rather than to invented prose.
    fn absorb_banner_text(&self) -> Option<String> {
        let seru = self.battle_ctx.multi_cast_gate;
        if seru == 0 {
            return None;
        }
        let spell = seru.wrapping_add(0x80);
        let name = self
            .tables
            .spell_catalog
            .get(spell)
            .map(|d| d.name.clone())
            .unwrap_or_else(|| format!("Spell {spell:#04X}"));
        // `0x8007BD10[seat]` is a 1-based character id; the roster slot is
        // that id minus one.
        let char_id = self.party_roster_slot(usize::from(self.battle_ctx.active_actor)) as u8 + 1;
        Some(
            self.tables
                .absorb_caption
                .as_ref()
                .and_then(|c| c.compose(char_id, &name))
                .unwrap_or(name),
        )
    }
}
