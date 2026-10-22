//! Which screen a title sub-mode word selects: the one table both play
//! hosts ask before they build the title card's draws.

/// Which of the front-end's screens a **retail title sub-mode** selects.
///
/// PORT: FUN_801DD35C (the sub-mode -> screen half of the dispatcher)
///
/// The front end is one function with a 25-entry jump table
/// ([`super::SUBMODE_TABLE`]) and one selector word
/// (`state[+0x204]`). The port's title, save-select and options screens are
/// separate sessions, so each host used to pick its screen off its own enum
/// and the sub-mode word was carried but never read. This is the one place
/// that maps the word to a screen, and every host calls it.
///
/// **The word does not carry the prompt / rows split.** Retail's `0x10`
/// `AttractIdle` is a single state that draws the title card, polls
/// `Start | L1 | Cross` and steps the menu cursor on `Up` / `Down`
/// ([`super::PADMASK_START_L1_CROSS`] and the
/// cursor masks beside it), and the port's title session stays on that word
/// from the prompt through the menu - only a confirm moves it, to `0x16` or
/// `0x18`. So "Press Start" versus "NEW GAME / CONTINUE" is a port-side
/// phase, not a sub-mode, and [`title_text_phase`] takes it as a second
/// argument rather than inventing a word retail does not write.
///
/// The rest of the mapping is the evidence line: the modes the port's title
/// reaches are named, every other in-range mode is the **memory-card
/// manager** band that the port draws through the save-select builders, and
/// `>= 0x19` is outside the dispatcher's own `sltiu 0x19` bound.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TitleDrawList {
    /// Nothing of the title's own - the entry pass, the idle tail, and every
    /// out-of-range word.
    Blank,
    /// The title card: the wordmark plus either the prompt or the menu rows
    /// (`0x10` `AttractIdle`, `0x11` `AttractDelay`, and the retail-
    /// unreachable `0x02` `TextMenu` / `0x14` `MainMenu`).
    TitleCard,
    /// A row has been confirmed and the card is on its way out (`0x06`
    /// `LaunchGame`, `0x16` `LaunchFade`, `0x18` `ContinueFadeIn`).
    Launching,
    /// The memory-card manager band (`0x03`..=`0x0F`, `0x12`, `0x13`, `0x15`,
    /// `0x17`): slot grids, block scans, card prompts and their notices. The
    /// port draws these from the save-select builders
    /// (`save_select_draws_for` and
    /// siblings), so the title layer contributes nothing to them.
    CardManager,
}

/// Map a retail title sub-mode word to its [`TitleDrawList`].
pub fn title_draw_list(submode: u8) -> TitleDrawList {
    use super::TitleOverlaySubMode as S;
    let Some(m) = S::from_u8(submode) else {
        return TitleDrawList::Blank;
    };
    match m {
        S::Init | S::Idle => TitleDrawList::Blank,
        S::TextMenu | S::MainMenu | S::AttractIdle | S::AttractDelay => TitleDrawList::TitleCard,
        S::LaunchGame | S::LaunchFade | S::ContinueFadeIn => TitleDrawList::Launching,
        _ => TitleDrawList::CardManager,
    }
}

/// The `phase` argument [`title_draws_for`] / [`title_menu_draws_for`] take,
/// resolved from the retail sub-mode word instead of from a host's own enum.
///
/// `0` draws nothing, `1` is the prompt, `2` is the menu rows - the builders'
/// existing contract, with one selector behind it on every host.
/// `menu_open` is the port-side half of the split retail keeps outside the
/// sub-mode word (see [`TitleDrawList`]).
pub fn title_text_phase(submode: u8, menu_open: bool) -> u8 {
    match title_draw_list(submode) {
        TitleDrawList::TitleCard if menu_open => 2,
        TitleDrawList::TitleCard => 1,
        TitleDrawList::Blank | TitleDrawList::Launching | TitleDrawList::CardManager => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::super::TitleOverlaySubMode as S;
    use super::*;

    #[test]
    fn every_in_range_submode_selects_one_list_and_the_bound_holds() {
        for b in 0..=0x18u8 {
            assert!(S::is_in_range(b), "{b:#x} should be in the table");
            let _ = title_draw_list(b);
        }
        // The dispatcher's own bound: 0x19 and up fall to the epilogue.
        for b in [0x19u8, 0x20, 0xFF] {
            assert_eq!(title_draw_list(b), TitleDrawList::Blank);
        }
    }

    #[test]
    fn the_modes_the_port_reaches_map_to_the_screens_it_draws() {
        // A cold boot sits on 0x10 from the prompt through the menu, so the
        // same word has to serve both phases.
        assert_eq!(
            title_draw_list(S::AttractIdle as u8),
            TitleDrawList::TitleCard
        );
        assert_eq!(title_text_phase(S::AttractIdle as u8, false), 1);
        assert_eq!(title_text_phase(S::AttractIdle as u8, true), 2);
        // Confirming a row leaves the card.
        assert_eq!(
            title_draw_list(S::LaunchFade as u8),
            TitleDrawList::Launching
        );
        assert_eq!(
            title_draw_list(S::ContinueFadeIn as u8),
            TitleDrawList::Launching
        );
        assert_eq!(title_text_phase(S::LaunchFade as u8, true), 0);
        // The card-manager band belongs to the save-select builders.
        assert_eq!(
            title_draw_list(S::SlotGrid as u8),
            TitleDrawList::CardManager
        );
        assert_eq!(title_text_phase(S::SlotGrid as u8, true), 0);
    }
}
