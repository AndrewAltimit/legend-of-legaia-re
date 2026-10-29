//! The pause menu's **Journal**: what the active pack's progress looks like
//! to the player, and the small session that pages through it.
//!
//! [`journal_view`] projects the pack and its [`QuestRuntime`] into plain
//! rows - quests (active first, then completed) with their objectives and
//! rewards, challenges with their best scores, the trading-card collection
//! and the pack's maps - and [`JournalSession`] is the cursor over them. Both are
//! `World`-free; the menu dispatcher seats the session as a pause-menu
//! sub-screen and the draw layer takes the rows as text.
//!
//! The Journal is an engine extension, reachable only while a pack is
//! active, so a game with no pack has the retail pause menu.

use legaia_engine_system::input::PadButton;
use legaia_expansion_format::ExpansionPack;
use legaia_expansion_format::quest::{FormationRef, Goal, Rewards};

use crate::quest::{QuestRuntime, QuestStatus};

/// One objective row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectiveRow {
    /// The objective's journal line, or a description of its goal.
    pub text: String,
    pub progress: u32,
    pub target: u32,
}

impl ObjectiveRow {
    /// Whether the objective is complete.
    pub fn done(&self) -> bool {
        self.progress >= self.target
    }
}

/// One quest row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuestRow {
    pub id: String,
    pub title: String,
    pub description: String,
    pub status: QuestStatus,
    /// Times completed.
    pub completions: u32,
    pub objectives: Vec<ObjectiveRow>,
    /// The reward, as one line (empty when the quest grants nothing).
    pub rewards: String,
}

/// One challenge row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChallengeRow {
    pub id: String,
    pub title: String,
    pub description: String,
    /// The best score, once cleared.
    pub best: Option<u32>,
    pub clears: u32,
    /// Armed for the next battle it applies to.
    pub armed: bool,
}

/// One map the Travel page can enter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TravelRow {
    pub map_id: String,
    pub name: String,
}

/// The Journal's content.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct JournalView {
    pub pack_name: String,
    pub pack_version: String,
    pub pack_authors: String,
    /// Started quests: active ones first, then completed, each in pack
    /// order.
    pub quests: Vec<QuestRow>,
    pub challenges: Vec<ChallengeRow>,
    /// The pack defines trading cards (the Cards page opens the card menu).
    pub has_cards: bool,
    /// Distinct cards in the collection.
    pub cards_owned: usize,
    /// Owned cards the binder has not shown yet.
    pub cards_unseen: usize,
    /// Unopened boosters.
    pub boosters: u32,
    /// The pack's maps, in id order.
    pub maps: Vec<TravelRow>,
}

impl JournalView {
    /// How many rows page `page` has.
    pub fn rows(&self, page: JournalPage) -> usize {
        match page {
            JournalPage::Quests => self.quests.len(),
            JournalPage::Challenges => self.challenges.len(),
            JournalPage::Cards => usize::from(self.has_cards),
            JournalPage::Travel => self.maps.len(),
        }
    }
}

/// Project `pack` and its progress into the Journal's rows.
pub fn journal_view(pack: &ExpansionPack, rt: &QuestRuntime) -> JournalView {
    let mut quests: Vec<QuestRow> = rt
        .journal(&pack.quests)
        .into_iter()
        .map(|e| QuestRow {
            id: e.quest.id.clone(),
            title: non_empty(&e.quest.title, &e.quest.id),
            description: e.quest.description.clone(),
            status: e.status,
            completions: e.completions,
            objectives: e
                .objectives
                .iter()
                .map(|o| ObjectiveRow {
                    text: o.text.map_or_else(|| goal_text(o.goal), str::to_string),
                    progress: o.progress.min(o.target),
                    target: o.target,
                })
                .collect(),
            rewards: rewards_text(&e.quest.rewards),
        })
        .collect();
    // Active before completed; the sort is stable, so each keeps pack order.
    quests.sort_by_key(|q| q.status == QuestStatus::Completed);
    let armed = rt.challenge.as_ref().map(|c| c.id.as_str());
    let challenges = pack
        .quests
        .challenges
        .iter()
        .map(|c| {
            let record = rt.progress.challenges.get(&c.id);
            ChallengeRow {
                id: c.id.clone(),
                title: non_empty(&c.title, &c.id),
                description: c.description.clone(),
                best: record.filter(|r| r.clears > 0).map(|r| r.best),
                clears: record.map_or(0, |r| r.clears),
                armed: armed == Some(c.id.as_str()),
            }
        })
        .collect();
    JournalView {
        pack_name: pack.manifest.name.clone(),
        pack_version: pack.manifest.version.clone(),
        pack_authors: pack.manifest.authors.join(", "),
        quests,
        challenges,
        has_cards: !pack.cards.cards.is_empty(),
        cards_owned: rt.progress.cards.collection.binder().len(),
        cards_unseen: rt.progress.cards.unseen(),
        boosters: rt.progress.cards.total_boosters(),
        maps: pack
            .maps
            .iter()
            .map(|(id, m)| TravelRow {
                map_id: id.clone(),
                name: non_empty(&m.name, id),
            })
            .collect(),
    }
}

fn non_empty(s: &str, fallback: &str) -> String {
    if s.trim().is_empty() {
        fallback.to_string()
    } else {
        s.to_string()
    }
}

fn formation_text(f: &FormationRef) -> String {
    match f {
        FormationRef::Id(id) => format!("formation {id}"),
        FormationRef::Pack(name) => name.replace('_', " "),
    }
}

/// A one-line description of `goal`, for an objective with no `text`.
pub fn goal_text(goal: &Goal) -> String {
    let times = |n: u32| {
        if n > 1 {
            format!(" {n} times")
        } else {
            String::new()
        }
    };
    match goal {
        Goal::DefeatFormation { formation, count } => {
            format!("Defeat {}{}", formation_text(formation), times(*count))
        }
        Goal::DefeatMonster { monster, count } => {
            format!("Defeat monster {monster}{}", times(*count))
        }
        Goal::WinBattles { count: 1 } => "Win a battle".into(),
        Goal::WinBattles { count } => format!("Win {count} battles"),
        Goal::WinBoss { boss: Some(b), .. } => format!("Defeat {}", b.replace('_', " ")),
        Goal::WinBoss {
            formation: Some(f), ..
        } => format!("Defeat {}", formation_text(f)),
        Goal::WinBoss { .. } => "Win a boss battle".into(),
        Goal::EnterScene { scene } => format!("Go to {scene}"),
        Goal::ReachLevel { level } => format!("Reach level {level}"),
        Goal::CollectItem {
            item,
            count,
            hand_in,
        } => format!(
            "{} {count} of item {item}",
            if *hand_in { "Bring" } else { "Collect" }
        ),
        Goal::Talk { npc } => format!("Talk to {}", npc.replace('_', " ")),
        Goal::WinMode { mode, count } => format!("Win {}{}", mode.replace('_', " "), times(*count)),
        Goal::Counter { counter, at_least } => {
            format!("{} reaches {at_least}", counter.replace('_', " "))
        }
        Goal::Flag { flag } => format!("Raise {}", flag.replace('_', " ")),
    }
}

/// A reward as one line: `10000 G, 500 EXP each, 2 items, 1 card`.
pub fn rewards_text(r: &Rewards) -> String {
    let mut parts = Vec::new();
    if r.gold > 0 {
        parts.push(format!("{} G", r.gold));
    }
    if r.xp > 0 {
        parts.push(format!("{} EXP each", r.xp));
    }
    let items: u32 = r.items.iter().map(|i| u32::from(i.count)).sum();
    match items {
        0 => {}
        1 => parts.push("1 item".into()),
        n => parts.push(format!("{n} items")),
    }
    match r.cards.len() {
        0 => {}
        1 => parts.push("1 card".into()),
        n => parts.push(format!("{n} cards")),
    }
    parts.join(", ")
}

/// The Journal's pages, in Left / Right order.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum JournalPage {
    #[default]
    Quests,
    Challenges,
    /// The trading-card collection; Cross opens the card menu (binder,
    /// boosters, sell, deck).
    Cards,
    /// Enter one of the pack's maps directly - the testing fallback for a
    /// map whose entrance is out of reach.
    Travel,
}

impl JournalPage {
    pub const ALL: [Self; 4] = [Self::Quests, Self::Challenges, Self::Cards, Self::Travel];

    pub fn label(self) -> &'static str {
        match self {
            Self::Quests => "Quests",
            Self::Challenges => "Challenges",
            Self::Cards => "Cards",
            Self::Travel => "Travel",
        }
    }
}

/// How a Journal session ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JournalOutcome {
    /// Circle: back to the pause menu.
    Closed,
    /// Cross on a Travel row: enter that map (the menu closes).
    Travel { map_id: String },
    /// Cross on a challenge: arm it for its next battle.
    ArmChallenge { challenge: String },
    /// Cross on the Cards page: open the card menu (the pause menu closes
    /// first).
    OpenCards,
}

/// The Journal's cursor.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct JournalSession {
    pub page: JournalPage,
    /// The selected row of [`Self::page`].
    pub cursor: usize,
    outcome: Option<JournalOutcome>,
}

impl JournalSession {
    pub fn new() -> Self {
        Self::default()
    }

    /// One frame of pad edges (the [`PadButton`] layout): Left / Right (or
    /// L1 / R1) turn the page, Up / Down move the cursor, Cross picks a
    /// Travel row or arms a challenge, Circle closes.
    pub fn tick(&mut self, view: &JournalView, edge: u16) {
        if self.outcome.is_some() {
            return;
        }
        let pressed = |b: PadButton| edge & b.mask() != 0;
        let turn = if pressed(PadButton::Right) || pressed(PadButton::R1) {
            1
        } else if pressed(PadButton::Left) || pressed(PadButton::L1) {
            -1
        } else {
            0
        };
        if turn != 0 {
            let n = JournalPage::ALL.len() as i32;
            let at = JournalPage::ALL
                .iter()
                .position(|p| *p == self.page)
                .unwrap_or(0) as i32;
            self.page = JournalPage::ALL[(at + turn).rem_euclid(n) as usize];
            self.cursor = 0;
            return;
        }
        let rows = view.rows(self.page);
        if pressed(PadButton::Down) && rows > 0 {
            self.cursor = (self.cursor + 1) % rows;
        } else if pressed(PadButton::Up) && rows > 0 {
            self.cursor = (self.cursor + rows - 1) % rows;
        } else if pressed(PadButton::Circle) {
            self.outcome = Some(JournalOutcome::Closed);
        } else if pressed(PadButton::Cross) {
            self.outcome = match self.page {
                JournalPage::Travel => view.maps.get(self.cursor).map(|m| JournalOutcome::Travel {
                    map_id: m.map_id.clone(),
                }),
                JournalPage::Challenges => {
                    view.challenges
                        .get(self.cursor)
                        .map(|c| JournalOutcome::ArmChallenge {
                            challenge: c.id.clone(),
                        })
                }
                JournalPage::Cards => view.has_cards.then_some(JournalOutcome::OpenCards),
                JournalPage::Quests => None,
            };
        }
        self.cursor = self.cursor.min(rows.saturating_sub(1));
    }

    /// How the session ended, once it has.
    pub fn outcome(&self) -> Option<&JournalOutcome> {
        self.outcome.as_ref()
    }

    pub fn is_done(&self) -> bool {
        self.outcome.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::quest::QuestState;
    use crate::quest_challenge::ChallengeRecord;

    fn pack() -> ExpansionPack {
        let mut p = ExpansionPack {
            quests: toml::from_str(
            r#"
[[quests]]
id = "one"
title = "First"
objectives = [{ kind = "win_battles", count = 3 }, { kind = "talk", npc = "old_man", text = "See the old man" }]
rewards = { gold = 100, items = [{ item = 4, count = 2 }] }
[[quests]]
id = "two"
title = "Second"
objectives = [{ kind = "reach_level", level = 9 }]
[[challenges]]
id = "c"
title = "Chal"
"#,
        )
        .expect("quests parse"),
            ..Default::default()
        };
        p.manifest.id = "t".into();
        p.manifest.name = "Test".into();
        p.manifest.version = "1.2.3".into();
        p.manifest.authors = vec!["A".into(), "B".into()];
        p
    }

    #[test]
    fn the_view_lists_started_quests_active_first() {
        let p = pack();
        let mut rt = QuestRuntime::default();
        rt.sync_pack(&p);
        rt.progress.quests.insert(
            "one".into(),
            QuestState {
                status: QuestStatus::Completed,
                objectives: vec![3, 1],
                completions: 1,
                last_completed_at: 0,
            },
        );
        rt.progress.quests.insert(
            "two".into(),
            QuestState {
                status: QuestStatus::Active,
                objectives: vec![0],
                completions: 0,
                last_completed_at: 0,
            },
        );
        rt.progress.challenges.insert(
            "c".into(),
            ChallengeRecord {
                best: 900,
                clears: 2,
            },
        );
        let v = journal_view(&p, &rt);
        assert_eq!(v.pack_authors, "A, B");
        let ids: Vec<_> = v.quests.iter().map(|q| q.id.as_str()).collect();
        assert_eq!(ids, ["two", "one"]);
        assert_eq!(v.quests[0].objectives[0].text, "Reach level 9");
        assert_eq!(v.quests[1].objectives[0].text, "Win 3 battles");
        assert_eq!(v.quests[1].objectives[1].text, "See the old man");
        assert_eq!(v.quests[1].rewards, "100 G, 2 items");
        assert_eq!(v.challenges[0].best, Some(900));
        assert!(!v.has_cards);
        assert_eq!(v.cards_unseen, 0);
    }

    #[test]
    fn the_session_pages_and_picks() {
        let mut v = journal_view(&pack(), &QuestRuntime::default());
        v.maps.push(TravelRow {
            map_id: "m".into(),
            name: "M".into(),
        });
        let mut s = JournalSession::new();
        s.tick(&v, PadButton::Right.mask());
        assert_eq!(s.page, JournalPage::Challenges);
        s.tick(&v, PadButton::Right.mask());
        assert_eq!(s.page, JournalPage::Cards);
        s.tick(&v, PadButton::Cross.mask());
        assert!(!s.is_done(), "no cards in the pack: nothing to open");
        s.tick(&v, PadButton::Right.mask());
        assert_eq!(s.page, JournalPage::Travel);
        s.tick(&v, PadButton::Cross.mask());
        assert_eq!(
            s.outcome(),
            Some(&JournalOutcome::Travel { map_id: "m".into() })
        );
        let mut s = JournalSession::new();
        s.tick(&v, PadButton::Cross.mask());
        assert!(!s.is_done(), "Cross on a quest does nothing");
        s.tick(&v, PadButton::Circle.mask());
        assert_eq!(s.outcome(), Some(&JournalOutcome::Closed));
    }
}
