//! The expansion pack's Journal on the pause menu: Select on the root list
//! opens it while a pack is active ([`journal_on_select`], [`open_journal`]),
//! its Cards page opens the card menu (`World::open_card_menu`), and its outcome (a Travel pick, a
//! challenge armed) folds into the world when it closes.

use super::*;

/// The Journal's content when `edge` asks for it: Select, on the root list,
/// while an expansion pack is active. `None` - leaving the root list to take
/// the edge - with no pack (the retail menu has no Select arm) or on any
/// other edge. A host reads it before it borrows the root list mutably and
/// hands it to [`open_journal`].
pub fn journal_on_select(edge: u16, world: &World) -> Option<JournalView> {
    if edge & PadButton::Select.mask() == 0 {
        return None;
    }
    world.expansion_journal()
}

/// Open the Journal over the root list: the list is suspended on the row
/// under its cursor, so closing the Journal lands the hand where it was.
/// Returns the sub-session for the host to seat; `None` when the root list
/// is not browsing its rows.
pub fn open_journal(
    menu: &mut legaia_engine_menus::field_menu::FieldMenuSession,
    view: JournalView,
) -> Option<FieldMenuSubsession> {
    if !menu.suspend_at_cursor() {
        return None;
    }
    Some(FieldMenuSubsession::Journal {
        session: JournalSession::new(),
        view: Box::new(view),
    })
}

/// Fold a finished Journal into the world: a Travel pick asks the scene
/// host for the map (and closes the menu, so the walk resumes on the map);
/// a challenge pick arms it. Returns whether the whole menu closes.
pub fn apply_journal_outcome(session: &JournalSession, world: &mut World) -> bool {
    match session.outcome() {
        Some(JournalOutcome::Travel { map_id }) => world.request_expansion_travel(map_id, ""),
        Some(JournalOutcome::ArmChallenge { challenge }) => {
            use legaia_engine_expansion::quest_challenge::ArmRefusal;
            if let Err(e) = world.expansion_arm_challenge(challenge) {
                let why = match e {
                    ArmRefusal::Solo => "it is fought alone",
                    ArmRefusal::LevelCap => "the party is over its level cap",
                    ArmRefusal::Unknown | ArmRefusal::UnknownFormation => "it cannot be fought",
                };
                world
                    .expansion
                    .toasts
                    .push(format!("Challenge not armed: {why}"));
            }
            false
        }
        Some(JournalOutcome::OpenCards) => {
            world.expansion.open_cards = world.cards_available();
            world.expansion.open_cards
        }
        Some(JournalOutcome::Closed) | None => false,
    }
}
