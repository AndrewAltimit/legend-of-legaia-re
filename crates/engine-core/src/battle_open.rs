//! The battle **open banner** - what the screen says about the formation roll
//! before anyone gets to act.
//!
//! Retail's flow state `0x0A` calls `FUN_801D9D3C`, which lays out the party
//! plates and then reads `ctx[+0x290]` (the formation advantage
//! [`legaia_engine_vm::battle_formulas::FormationAdvantage`] models) to pick a
//! banner string. The arm at `0x801DA234` is three-way:
//!
//! | `ctx[+0x290]` | Banner | What follows |
//! |---|---|---|
//! | `0` | none - the draw at `0x801DA2E4` is skipped | ordinary round |
//! | `1` back attack | `Ambushed!` (`0x801F4D10`) | `0x0B` jumps to `0xFE`: the party gets **no** command that round |
//! | `2` pre-emptive | `<leader>('s team) surprised the enemy.` | ordinary round; the monsters sit it out |
//!
//! The singular / plural pick is `0x801DA274`: the byte at `DAT_8007BD10 + 1`
//! (the present-party list's **second** slot) decides, so a party with nobody
//! in slot 1 gets the solo line. The name is substituted into the `0xC1` token
//! by `FUN_8003CBF8`, with the operand set to `DAT_8007BD10[0] - 1` - the
//! party **leader**, not the acting member.
//!
//! The intro hold is `ctx[+0x6D6]`, which state `0x0A` seeds at `0x5A` (90
//! frames) and re-seeds at `0x78` (120) whenever `ctx[+0x290]` is non-zero -
//! i.e. the banner buys itself an extra half-second.
//!
//! ## What is committed here and what is not
//!
//! The two pre-emptive lines are retail sentences; their disc coordinates are
//! pinned in [`legaia_asset::battle_ui_strings`] and the text is read off the
//! user's own image, never stored. [`FormationBanner::line`] therefore takes an
//! optional resolved template and falls back to the port's own wording, which
//! is what a host without a disc-read string table shows.

use legaia_asset::battle_ui_strings::{BattleUiStrings, OVERLAY_BASE_VA};
use legaia_engine_vm::battle_formulas::FormationAdvantage;

/// Read the battle screen's overlay-resident labels - the banner sentences,
/// `Spirit`, `Escape` and the per-character Ra-Seru magic-command names - off
/// the user's own `PROT.DAT`.
///
/// The SCUS-resident half (`Begin` / `Run` / `Attack` / `Item` / `Auto` /
/// `Command`) is not read here: those words are already the port's own chip
/// labels, so the only thing a disc read would add is a second copy. Hosts
/// that want them can call
/// [`legaia_asset::battle_ui_strings::BattleUiStrings::merge_scus`] on top.
///
/// An entry that will not resolve yields an empty table rather than an error,
/// so a host with a partial extraction falls back to the port's wording.
pub fn battle_ui_strings_from_prot(index: &crate::scene::ProtIndex) -> BattleUiStrings {
    let mut out = BattleUiStrings::default();
    let Some(rec) = legaia_asset::static_overlay::overlay_map().by_label("battle_action") else {
        return out;
    };
    let Ok(bytes) = index.entry_bytes(rec.prot_index) else {
        return out;
    };
    match legaia_asset::static_overlay::as_loaded(&bytes, rec) {
        Ok(loaded) => out.merge_overlay(&loaded, rec.base_va),
        Err(_) => out.merge_overlay(&bytes, OVERLAY_BASE_VA),
    }
    out
}

/// Frames the open banner stays up, matching the `ctx[+0x6D6]` reseed at
/// `0x801D0E34`: retail holds the intro `0x78` frames whenever the formation
/// roll produced an advantage, against `0x5A` for an ordinary open.
pub const BANNER_FRAMES: u16 = 0x78;

/// Frames an ordinary (no-advantage) battle open holds - retail's `0x5A`.
/// Nothing is drawn during it; kept here because it is the same field.
pub const PLAIN_OPEN_FRAMES: u16 = 0x5A;

/// The tutorial-box style the port raises the banner with: left margin, top
/// anchor, self-dismissing. Retail's own banner is a different emitter
/// (`FUN_8003541C` direct, `x = 0x10`, `y = 0x0C`, `w = 0x120`), so the port's
/// `y` sits two rows lower than retail's; the corner and the dismissal
/// behaviour match.
pub const BANNER_BOX_STYLE: u8 = 0;

/// The banner one formation roll produces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormationBanner {
    /// `ctx[+0x290] == 1` - the monsters got the drop.
    Ambushed,
    /// `ctx[+0x290] == 2` with two or more party members present.
    TeamSurprisedEnemy,
    /// `ctx[+0x290] == 2` with a solo party (retail: nothing in present-party
    /// slot 1).
    SoloSurprisedEnemy,
}

impl FormationBanner {
    /// The banner a roll + party size calls for, or `None` for an ordinary
    /// open - which is retail's own `beqz` skip at `0x801DA2E4`, not an
    /// omission.
    pub fn for_formation(advantage: FormationAdvantage, party_count: u8) -> Option<Self> {
        match advantage {
            FormationAdvantage::None => None,
            FormationAdvantage::BackAttack => Some(Self::Ambushed),
            FormationAdvantage::Preemptive if party_count >= 2 => Some(Self::TeamSurprisedEnemy),
            FormationAdvantage::Preemptive => Some(Self::SoloSurprisedEnemy),
        }
    }

    /// Which pinned disc string carries this banner's retail wording.
    pub fn disc_label(self) -> legaia_asset::battle_ui_strings::BattleUiLabel {
        use legaia_asset::battle_ui_strings::BattleUiLabel as L;
        match self {
            Self::Ambushed => L::Ambushed,
            Self::TeamSurprisedEnemy => L::TeamSurprised,
            Self::SoloSurprisedEnemy => L::SoloSurprised,
        }
    }

    /// `true` when the party loses the round the banner announces - the one
    /// case the player never enters a command, which is exactly what makes the
    /// announcement load-bearing rather than decorative.
    pub fn costs_the_party_its_round(self) -> bool {
        matches!(self, Self::Ambushed)
    }

    /// The rendered line.
    ///
    /// `template` is the retail string when the caller has one (read from the
    /// user's image through [`legaia_asset::battle_ui_strings`]): the `0xC1`
    /// token and the character index byte that follows it are replaced with
    /// `leader`. With no template the port's own wording is used, so a host
    /// that never loaded the overlay still says which way the ambush went.
    pub fn line(self, leader: &str, template: Option<&str>) -> String {
        if let Some(t) = template {
            let rendered = substitute_name_token(t, leader);
            if !rendered.trim().is_empty() {
                return rendered;
            }
        }
        match self {
            Self::Ambushed => "Ambushed!".to_string(),
            Self::TeamSurprisedEnemy => format!("{leader}'s team struck first!"),
            Self::SoloSurprisedEnemy => format!("{leader} struck first!"),
        }
    }
}

/// Replace retail's `0xC1` name token - and the one operand byte the
/// substitution routine writes after it - with `name`.
///
/// `FUN_8003CBF8(str, 0xC1, 1)` finds the token and returns its offset; the
/// caller then stores the character index into the byte that follows, so the
/// token is **two** bytes wide in the stored string even though only the first
/// is the marker. A template with no token is returned unchanged.
fn substitute_name_token(template: &str, name: &str) -> String {
    let Some(pos) = template.find('\u{c1}') else {
        return template.to_string();
    };
    let mut it = template[pos..].chars();
    it.next();
    // The operand byte is whatever the runtime last stored; skip it when it is
    // not itself printable text, which is how a live string always reads.
    let skip = match it.next() {
        Some(c) if !c.is_ascii_graphic() => '\u{c1}'.len_utf8() + c.len_utf8(),
        _ => '\u{c1}'.len_utf8(),
    };
    let mut out = String::with_capacity(template.len() + name.len());
    out.push_str(&template[..pos]);
    out.push_str(name);
    out.push_str(&template[pos + skip..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_ordinary_open_raises_no_banner() {
        assert_eq!(
            FormationBanner::for_formation(FormationAdvantage::None, 3),
            None
        );
    }

    /// The plural pick is party size, which is retail's present-party slot-1
    /// test in the port's own seating.
    #[test]
    fn the_preemptive_line_picks_singular_for_a_solo_party() {
        assert_eq!(
            FormationBanner::for_formation(FormationAdvantage::Preemptive, 1),
            Some(FormationBanner::SoloSurprisedEnemy)
        );
        for n in 2..=3 {
            assert_eq!(
                FormationBanner::for_formation(FormationAdvantage::Preemptive, n),
                Some(FormationBanner::TeamSurprisedEnemy)
            );
        }
    }

    /// Only the back attack costs the party its round - the whole reason the
    /// player never enters a command that round.
    #[test]
    fn only_the_back_attack_costs_the_round() {
        let b = FormationBanner::for_formation(FormationAdvantage::BackAttack, 3).unwrap();
        assert_eq!(b, FormationBanner::Ambushed);
        assert!(b.costs_the_party_its_round());
        assert!(
            !FormationBanner::for_formation(FormationAdvantage::Preemptive, 3)
                .unwrap()
                .costs_the_party_its_round()
        );
    }

    /// A disc template's `0xC1` token plus its operand byte become the name.
    #[test]
    fn the_name_token_and_its_operand_are_replaced() {
        let template = "\u{c1}\u{1}'s team surprised the enemy.";
        let line = FormationBanner::TeamSurprisedEnemy.line("Vahn", Some(template));
        assert_eq!(line, "Vahn's team surprised the enemy.");
        // A template with no token survives verbatim.
        assert_eq!(
            FormationBanner::Ambushed.line("Vahn", Some("Ambushed!")),
            "Ambushed!"
        );
    }

    /// Without a template the port supplies its own wording, and it still
    /// names the leader on the pre-emptive lines.
    #[test]
    fn the_fallback_wording_still_names_the_leader() {
        assert_eq!(
            FormationBanner::Ambushed.line("Vahn", None),
            "Ambushed!".to_string()
        );
        assert!(
            FormationBanner::TeamSurprisedEnemy
                .line("Noa", None)
                .starts_with("Noa")
        );
        assert!(
            FormationBanner::SoloSurprisedEnemy
                .line("Gala", None)
                .starts_with("Gala")
        );
        // An empty template falls back rather than drawing a blank banner.
        assert_eq!(
            FormationBanner::Ambushed.line("Vahn", Some("")),
            "Ambushed!".to_string()
        );
    }

    /// The banner's hold is retail's advantage-case intro timer, and it is
    /// longer than the plain one - the extra frames exist to read it.
    #[test]
    fn the_banner_hold_is_the_longer_intro_timer() {
        assert_eq!(BANNER_FRAMES, 0x78);
        assert_eq!(PLAIN_OPEN_FRAMES, 0x5A);
        const _: () = assert!(BANNER_FRAMES > PLAIN_OPEN_FRAMES);
    }
}
