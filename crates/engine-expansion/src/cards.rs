//! The trading-card economy's seat on the `World`: where every card grant
//! lands, the battle drops, the card screen while it owns the frame, and
//! the monster portraits the screens draw.
//!
//! The player's cards themselves are pack progress
//! ([`crate::quest_save::ExpansionProgress::cards`], the save block's card
//! record), so they follow the pack through activation, saves and the
//! dormant block exactly as quests do. This module is the plumbing around
//! them:
//!
//! - **Grants.** Quest rewards ([`crate::quest::QuestRuntime::card_inbox`])
//!   and mode wins ([`crate::ExpansionState::pending_card_rewards`]) queue
//!   ids; [`collect`] drains both queues into the collection and raises one
//!   [`CardNotice`] per card for the host's toast.
//! - **Battle drops.** [`battle_sources`] names the reward sources a won
//!   fight offers - each defeated seat's retail monster id, the pack
//!   formation, the pack boss - and the card stream rolls them. Nothing here
//!   reads or advances the game's RNG.
//! - **Stakes.** A Grid Duel the opponent wins under the stake rule names
//!   the player's card it takes; [`forfeit`] removes it.

use std::collections::BTreeMap;

use legaia_engine_mode::mode::SceneMode;
use legaia_engine_modes::cards::economy::CardProgress;
use legaia_engine_modes::cards::screen::{CardScreen, booster_name, card_name};
use legaia_engine_modes::cards::{CardBook, CardGrant};
use legaia_expansion_format::cards::RewardSource;

use crate::modes::PendingCardReward;

/// Something the card economy tells the player.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CardNotice {
    /// A card joined the collection. `new`: never owned before.
    Card { id: String, name: String, new: bool },
    /// An unopened booster joined the inventory.
    Booster { id: String, name: String },
    /// A card left the collection (a lost stake).
    Lost { id: String, name: String },
}

impl CardNotice {
    /// The toast line.
    pub fn line(&self) -> String {
        match self {
            Self::Card {
                name, new: true, ..
            } => format!("Got card: {name} (new!)"),
            Self::Card { name, .. } => format!("Got card: {name}"),
            Self::Booster { name, .. } => format!("Got booster: {name}"),
            Self::Lost { name, .. } => format!("Lost card: {name}"),
        }
    }
}

/// One cached portrait: RGBA8, `w * h * 4` bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Portrait {
    pub w: u16,
    pub h: u16,
    pub rgba: Vec<u8>,
}

/// Portraits rendered for one pack, by retail monster id. Built by the scene
/// host from the player's own disc and held only in memory.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PortraitCache {
    /// The pack the cache was built for; empty before the first build.
    pub pack_id: String,
    pub by_monster: BTreeMap<u16, Portrait>,
}

impl PortraitCache {
    pub fn get(&self, monster: u16) -> Option<&Portrait> {
        self.by_monster.get(&monster)
    }
}

/// The card screen while it owns the frame.
#[derive(Debug, Clone)]
pub struct CardSeat {
    /// The mode the world returns to when the screen closes.
    pub return_mode: SceneMode,
    pub screen: CardScreen,
}

/// Frames a toast line stays up.
pub const TOAST_FRAMES: u16 = 150;
/// Toast lines on screen at once.
pub const TOAST_LINES: usize = 3;
/// Notices kept for a host that has not taken them.
const RECENT_CAP: usize = 32;

/// One toast line on screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CardToast {
    pub line: String,
    pub frames_left: u16,
}

/// The card economy's runtime, seated on `ExpansionState::cards`.
#[derive(Debug, Clone, Default)]
pub struct CardRuntime {
    /// Notices raised since the last [`Self::tick_toasts`], oldest first.
    pub notices: Vec<CardNotice>,
    /// Notices already toasted, kept (capped) for a host to take.
    pub recent: Vec<CardNotice>,
    /// The toast lines, oldest first; the first [`TOAST_LINES`] show.
    pub toasts: Vec<CardToast>,
    /// The open card screen.
    pub seat: Option<Box<CardSeat>>,
    pub portraits: PortraitCache,
    /// The Grid Duel card page while a duel draws from it.
    pub duel_page: DuelPage,
}

/// The Grid Duel's card page in VRAM
/// (`legaia_engine_modes::cards::table_art`): what was written, and the
/// words it covered, put back when the duel closes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DuelPage {
    /// A digest of the hands and portraits last written.
    pub key: Option<u64>,
    /// The scene label and the covered words before the first write.
    pub saved: Option<(String, Vec<u16>)>,
}

impl CardRuntime {
    /// Take every notice not yet taken, oldest first.
    pub fn take_notices(&mut self) -> Vec<CardNotice> {
        let mut out = std::mem::take(&mut self.recent);
        out.append(&mut self.notices);
        out
    }

    /// One frame of the toasts: new notices become lines, the showing lines
    /// count down and expire.
    pub fn tick_toasts(&mut self) {
        for n in self.notices.drain(..) {
            self.toasts.push(CardToast {
                line: n.line(),
                frames_left: TOAST_FRAMES,
            });
            self.recent.push(n);
        }
        let excess = self.recent.len().saturating_sub(RECENT_CAP);
        self.recent.drain(..excess);
        for t in self.toasts.iter_mut().take(TOAST_LINES) {
            t.frames_left = t.frames_left.saturating_sub(1);
        }
        self.toasts.retain(|t| t.frames_left > 0);
    }

    /// The toast lines showing now.
    pub fn toast_lines(&self) -> Vec<&str> {
        self.toasts
            .iter()
            .take(TOAST_LINES)
            .map(|t| t.line.as_str())
            .collect()
    }
}

/// The notice for a credited grant. `known_before`: the id was owned or
/// shown before this grant.
fn notice(book: &CardBook, g: &CardGrant, known_before: bool) -> CardNotice {
    match g {
        CardGrant::Card(id) => CardNotice::Card {
            id: id.clone(),
            name: card_name(book, id),
            new: !known_before,
        },
        CardGrant::Booster(id) => CardNotice::Booster {
            id: id.clone(),
            name: booster_name(book, id),
        },
    }
}

/// Credit `grants` to `progress`, raising a notice per credited one.
pub fn credit(
    progress: &mut CardProgress,
    book: &CardBook,
    grants: &[CardGrant],
    notices: &mut Vec<CardNotice>,
) {
    for g in grants {
        let known = match g {
            CardGrant::Card(id) => progress.collection.count(id) > 0 || progress.seen.contains(id),
            CardGrant::Booster(_) => true,
        };
        if progress.grant(book, g) {
            notices.push(notice(book, g, known));
        }
    }
}

/// Drain the quest inbox and the mode-win queue into `progress`.
pub fn collect(
    progress: &mut CardProgress,
    book: &CardBook,
    inbox: &mut Vec<String>,
    pending: &mut Vec<PendingCardReward>,
    notices: &mut Vec<CardNotice>,
) {
    let mut grants: Vec<CardGrant> = inbox.drain(..).map(CardGrant::Card).collect();
    grants.extend(pending.drain(..).map(|p| match p {
        PendingCardReward::Booster { id, .. } => CardGrant::Booster(id),
        PendingCardReward::Card { id, .. } => CardGrant::Card(id),
    }));
    credit(progress, book, &grants, notices);
}

/// Hand over the pack's starter cards the first time its collection is
/// used.
pub fn grant_starter(progress: &mut CardProgress, book: &CardBook, notices: &mut Vec<CardNotice>) {
    for id in progress.grant_starter(book) {
        notices.push(CardNotice::Card {
            name: card_name(book, &id),
            id,
            new: true,
        });
    }
}

/// Remove a card the player staked and lost. A card the collection does
/// not hold (the duel lent its starter deck) costs nothing.
pub fn forfeit(
    progress: &mut CardProgress,
    book: &CardBook,
    id: &str,
    notices: &mut Vec<CardNotice>,
) {
    if progress.collection.remove(id, 1).is_ok() {
        notices.push(CardNotice::Lost {
            id: id.to_string(),
            name: card_name(book, id),
        });
    }
}

/// The reward sources a won fight offers: one [`RewardSource::Monster`] per
/// defeated seat (by its retail monster id, so three of a monster roll three
/// times), then the pack formation (`formation`, its local key) and the pack
/// boss (`boss`, local key) when the fight was one.
pub fn battle_sources(
    monster_ids: &[u16],
    formation: Option<&str>,
    boss: Option<&str>,
) -> Vec<RewardSource> {
    let mut out: Vec<RewardSource> = monster_ids
        .iter()
        .map(|&id| RewardSource::Monster { id })
        .collect();
    if let Some(f) = formation {
        out.push(RewardSource::Formation { id: f.to_string() });
    }
    if let Some(b) = boss {
        out.push(RewardSource::Boss { id: b.to_string() });
    }
    out
}

/// A local key with any `pack:` qualifier dropped.
pub fn local_key(key: &str) -> &str {
    key.split_once(':').map_or(key, |(_, k)| k)
}

#[cfg(test)]
mod tests {
    use super::*;
    use legaia_expansion_format::cards::{CardDef, CardRewardRule, CardSection};

    fn book() -> CardBook {
        let card = |id: &str| CardDef {
            id: id.into(),
            name: id.to_uppercase(),
            sides: [1; 4],
            ..Default::default()
        };
        let rule = |source, card: &str| CardRewardRule {
            source,
            booster: None,
            card: Some(card.into()),
            chance_percent: 100,
            count: 1,
        };
        CardBook::new(CardSection {
            cards: vec![card("a"), card("b"), card("c")],
            rewards: vec![
                rule(RewardSource::Monster { id: 4 }, "a"),
                rule(RewardSource::Formation { id: "pit".into() }, "b"),
                rule(RewardSource::Boss { id: "king".into() }, "c"),
            ],
            starter: vec!["a".into()],
            ..Default::default()
        })
    }

    #[test]
    fn every_grant_path_drains_into_the_collection_with_notices() {
        let b = book();
        let mut p = CardProgress::default();
        let mut notices = Vec::new();
        let mut inbox = vec!["a".to_string(), "zz".to_string()];
        let mut pending = vec![PendingCardReward::Card {
            id: "a".into(),
            from_mode: "m".into(),
        }];
        collect(&mut p, &b, &mut inbox, &mut pending, &mut notices);
        assert!(inbox.is_empty() && pending.is_empty());
        assert_eq!(p.collection.count("a"), 2);
        assert_eq!(
            notices.iter().map(CardNotice::line).collect::<Vec<_>>(),
            ["Got card: A (new!)", "Got card: A"]
        );
    }

    #[test]
    fn battle_sources_roll_seats_formation_and_boss() {
        let b = book();
        let mut p = CardProgress::default();
        p.seed_once(1);
        let src = battle_sources(&[4, 4, 7], Some(local_key("rebirth:pit")), Some("king"));
        assert_eq!(src.len(), 5);
        let got = p.roll_and_grant(&b, &src);
        assert_eq!(
            got.len(),
            4,
            "two seats of monster 4, the formation, the boss"
        );
        assert_eq!(
            (
                p.collection.count("a"),
                p.collection.count("b"),
                p.collection.count("c")
            ),
            (2, 1, 1)
        );
    }

    #[test]
    fn notices_toast_for_a_while_and_stay_takeable() {
        let mut rt = CardRuntime::default();
        for i in 0..4 {
            rt.notices.push(CardNotice::Booster {
                id: format!("b{i}"),
                name: format!("B{i}"),
            });
        }
        rt.tick_toasts();
        assert_eq!(
            rt.toast_lines(),
            ["Got booster: B0", "Got booster: B1", "Got booster: B2"]
        );
        for _ in 0..TOAST_FRAMES {
            rt.tick_toasts();
        }
        assert_eq!(
            rt.toast_lines(),
            ["Got booster: B3"],
            "the fourth waits its turn"
        );
        assert_eq!(rt.take_notices().len(), 4);
        assert!(rt.take_notices().is_empty());
    }

    #[test]
    fn a_lost_stake_removes_only_an_owned_card() {
        let b = book();
        let mut p = CardProgress::default();
        let mut notices = Vec::new();
        grant_starter(&mut p, &b, &mut notices);
        forfeit(&mut p, &b, "b", &mut notices);
        assert_eq!(notices.len(), 1, "b was never owned");
        forfeit(&mut p, &b, "a", &mut notices);
        assert_eq!(p.collection.count("a"), 0);
        assert_eq!(notices[1].line(), "Lost card: A");
    }
}
