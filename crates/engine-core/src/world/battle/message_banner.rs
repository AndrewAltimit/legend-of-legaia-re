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
//! A third element shares the widget: `0x66`, the **timed** message. Its
//! raisers point record `0x66`'s content word (`0x800775B4`) at a fixed
//! string in the battle overlay's own data - the counterattack swap of the
//! strike loop at `s_Counterattack_successful_801CED18`, the "No effect."
//! pass `FUN_801F3C34` at `0x801CFA20` - set the hold `0x801F6964`, and raise
//! it with `FUN_801D8DE8(0x66, 0)`. The battle frame driver `FUN_80046A20`
//! counts the hold down by the frame byte after every action-SM step and
//! unloads the element when it runs out (`0x80047058..0x80047098`).
//!
//! The port keeps the composed line on [`crate::world::BattleState::message_banner`]
//! from the raise to the matching unload, and both hosts draw it through
//! [`crate::battle_hud::battle_banner_message`] into the top banner widget.

use super::*;

/// Screen element of the Seru-absorb banner.
pub const ABSORB_BANNER_ELEMENT: u8 = 0x59;

/// Screen element of the magic-level-increased banner.
pub const MAGIC_LEVEL_BANNER_ELEMENT: u8 = 0x65;

/// Screen element of the timed message (the counterattack line, "No
/// effect.").
pub const TIMED_MESSAGE_ELEMENT: u8 = 0x66;

/// The battle overlay's (PROT 0898) load base: a timed message's content word
/// is a pointer into its data.
const BATTLE_OVERLAY_BASE: u32 = 0x801C_E818;

/// The counterattack line, `s_Counterattack_successful_801CED18`.
pub const COUNTER_MESSAGE_VA: u32 = 0x801C_ED18;

/// The hold the counterattack swap seeds (`li v0,0x78` / `sw v0,0x6964` at
/// `0x801E3650` / `0x801E3658`).
pub const COUNTER_MESSAGE_HOLD: i32 = 0x78;

/// One raised message banner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BattleMessageBanner {
    /// The screen element that raised it (`0x59` / `0x65` / `0x66`) - the
    /// unload that retires it names the same id.
    pub element: u8,
    /// The composed line (retail's `ctx + 0x1F9` buffer, or the overlay
    /// string a timed message points at).
    pub text: String,
    /// The timed message's remaining hold `0x801F6964`, in frames; `0` for
    /// the two elements that stay until their own unload.
    pub hold: i32,
}

impl World {
    /// The `FUN_801D8DE8` raise / unload for the two message elements. Other
    /// element ids are not this banner's and are ignored.
    ///
    /// REF: FUN_801D8DE8 (`0x801D9154..0x801D91D0`, the `0x59` compose arm)
    pub(in crate::world) fn message_banner_ui_element(&mut self, element: u8, mode: u8) {
        if element != ABSORB_BANNER_ELEMENT
            && element != MAGIC_LEVEL_BANNER_ELEMENT
            && element != TIMED_MESSAGE_ELEMENT
        {
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
            self.battle.message_banner = Some(BattleMessageBanner {
                element,
                text,
                hold: 0,
            });
        }
    }

    /// Stage the magic-level line and raise element `0x65` - the port's seat
    /// of `FUN_801F452C`'s compose followed by `FUN_801E70BC`'s raise.
    pub(in crate::world) fn raise_magic_level_banner(&mut self, text: String) {
        self.battle.message_banner = Some(BattleMessageBanner {
            element: MAGIC_LEVEL_BANNER_ELEMENT,
            text,
            hold: 0,
        });
    }

    /// Raise the timed message: record `0x66`'s content word pointed at the
    /// battle-overlay string `va`, the hold `0x801F6964 = hold`, and the
    /// element raised. The text is read off the loaded PROT 0898 image (the
    /// string is game text, so the port never carries it); without the
    /// image, or for a pointer outside it, nothing is raised.
    pub fn raise_timed_message(&mut self, va: u32, hold: i32) -> bool {
        let Some(text) = self.battle_overlay_string(va) else {
            return false;
        };
        self.battle.message_banner = Some(BattleMessageBanner {
            element: TIMED_MESSAGE_ELEMENT,
            text,
            hold,
        });
        true
    }

    /// The NUL-terminated string at `va` in the battle overlay image.
    fn battle_overlay_string(&self, va: u32) -> Option<String> {
        let off = va.checked_sub(BATTLE_OVERLAY_BASE)? as usize;
        let bytes = self.tables.move_power_overlay.as_deref()?.get(off..)?;
        let end = bytes.iter().position(|&b| b == 0)?;
        let s = bytes.get(..end)?;
        (!s.is_empty() && s.iter().all(|b| b.is_ascii() && !b.is_ascii_control()))
            .then(|| String::from_utf8_lossy(s).into_owned())
    }

    /// The battle frame driver's hold countdown for the timed message
    /// (`FUN_80046A20`, `0x80047058..0x80047098`): run after the action-SM
    /// step, `hold -= DAT_1F800393` while it is positive, and the element is
    /// unloaded (`FUN_801D8DE8(0x66, 1)`) once it reaches `0`.
    ///
    /// PORT: FUN_80046A20 (`0x80047058..0x80047098`, the `0x801F6964` hold)
    pub(in crate::world) fn tick_timed_message(&mut self) {
        // Per battle pass by the frame step, so one a vsync tick
        // ([`super::commit_log_launch::BATTLE_PASS_STEP_PER_TICK`]).
        let step = i32::from(super::commit_log_launch::BATTLE_PASS_STEP_PER_TICK);
        let Some(b) = self.battle.message_banner.as_mut() else {
            return;
        };
        if b.element != TIMED_MESSAGE_ELEMENT || b.hold <= 0 {
            return;
        }
        b.hold -= step;
        if b.hold <= 0 {
            self.battle.message_banner = None;
        }
    }

    /// The `0x59` line for the Seru staged in `ctx[+0x269]` (the engine's
    /// `multi_cast_gate`) and the acting seat's character. `None` without a
    /// staged Seru. Without the disc caption table the line degrades to the
    /// Seru's name alone rather than to invented prose.
    /// Spell `id`'s name as the banner composers copy it: the raw table
    /// string, element plate included. Retail's name opens with the `0xCE`
    /// icon escape (`0xCE 0x14 0x20 'G' ...`), the string copy
    /// `FUN_8003CA78` carries it into the message buffer, and the banner
    /// draws the plate in front of the text. The port's names are the
    /// escape-free display form, so the plate goes back in as the authoring
    /// markup it was written in (`^A ` - the preprocessor `FUN_80036514`
    /// turns `^X` into `0xCE (X - 0x2D)`), which the banner's layout
    /// expands again (`engine-ui::battle_hud_chrome`).
    pub(in crate::world) fn spell_banner_name(&self, spell: u8) -> String {
        let name = self
            .tables
            .spell_catalog
            .get(spell)
            .map(|d| d.name.clone())
            .unwrap_or_else(|| format!("Spell {spell:#04X}"));
        let icon = self
            .menu
            .text
            .as_ref()
            .and_then(|t| t.spell_names.as_ref())
            .and_then(|t| t.icon(spell))
            .and_then(|op| op.checked_add(0x2D))
            .filter(u8::is_ascii_uppercase);
        match icon {
            Some(letter) => format!("^{} {name}", letter as char),
            None => name,
        }
    }

    fn absorb_banner_text(&self) -> Option<String> {
        let seru = self.battle_ctx.multi_cast_gate;
        if seru == 0 {
            return None;
        }
        let spell = seru.wrapping_add(0x80);
        let name = self.spell_banner_name(spell);
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
