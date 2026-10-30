//! The pause menu's **Journal**: what the active pack's progress looks like
//! to the player, and the small session that pages through it.
//!
//! [`journal_view`] projects the pack and its [`QuestRuntime`] into plain
//! rows - quests (active first, then completed) with their objectives and
//! rewards, challenges with their best scores, the trading-card collection
//! and the pack's maps the party has been to - and [`JournalSession`] is the
//! cursor over them. Both are
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

/// The pack flag that records a visit to pack map `map_id`: raised by
/// [`QuestRuntime::handle`] when the party first enters the map, carried by
/// the save with the other pack flags, and read by the Travel page. The `@`
/// keeps it apart from every flag a pack can name (a pack flag id has the
/// syntax of a pack id).
pub fn visited_flag(map_id: &str) -> String {
    format!("{VISITED_FLAG_PREFIX}{map_id}")
}

/// The prefix of [`visited_flag`].
pub const VISITED_FLAG_PREFIX: &str = "@visited:";

/// One map the Travel page can enter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TravelRow {
    pub map_id: String,
    pub name: String,
}

/// One pack item the party holds or wears (the Items page).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemRow {
    pub key: String,
    pub name: String,
    pub description: String,
    /// `Weapon`, `Item`, ...
    pub kind: &'static str,
    pub equipment: bool,
    pub held: u8,
    /// How many the heroes wear.
    pub worn: usize,
}

/// One pack art or spell a hero knows (the Arts page).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtListRow {
    /// `Vahn: Ember Cascade`.
    pub title: String,
    /// `30 AP`, `Lv 3`.
    pub tag: String,
    /// The arrows (`L R D U R`) or `chain finisher`, or the spell's MP and
    /// description.
    pub detail: String,
    /// A spell row (an art row otherwise).
    pub spell: bool,
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
    /// The pack's maps the Travel page offers, in id order: the ones the
    /// party has been to, or every map when Travel is unlocked.
    pub maps: Vec<TravelRow>,
    /// How many maps the pack defines (so an empty Travel page can say
    /// whether the pack has none or none has been visited yet).
    pub maps_total: usize,
    /// The pack defines items (the Items page opens the item screen).
    pub has_items: bool,
    /// The pack items held or worn, consumables first.
    pub items: Vec<ItemRow>,
    /// The pack defines arts or spells (the Arts page lists them).
    pub has_arts: bool,
    /// The pack arts and spells the heroes know: arts first.
    pub arts: Vec<ArtListRow>,
}

impl JournalView {
    /// How many rows page `page` has.
    pub fn rows(&self, page: JournalPage) -> usize {
        match page {
            JournalPage::Quests => self.quests.len(),
            JournalPage::Challenges => self.challenges.len(),
            JournalPage::Cards => usize::from(self.has_cards),
            JournalPage::Travel => self.maps.len(),
            JournalPage::Items => self.items.len().max(usize::from(self.has_items)),
            JournalPage::Arts => self.arts.len(),
        }
    }
}

/// Project `pack` and its progress into the Journal's rows.
///
/// The Travel page lists the maps the party has entered on this save
/// ([`visited_flag`]); `all_maps` lists every map instead (the "Unlock
/// Travel" cheat, for testing a map whose entrance is out of reach).
pub fn journal_view(pack: &ExpansionPack, rt: &QuestRuntime, all_maps: bool) -> JournalView {
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
                    text: o
                        .text
                        .map_or_else(|| goal_text_in(pack, o.goal), str::to_string),
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
            .filter(|(id, _)| all_maps || rt.progress.flags.is_set(&visited_flag(id)))
            .map(|(id, m)| TravelRow {
                map_id: id.clone(),
                name: non_empty(&m.name, id),
            })
            .collect(),
        maps_total: pack.maps.len(),
        has_items: !pack.items.items.is_empty(),
        items: item_rows(pack, &rt.progress.items),
        has_arts: !pack.arts.arts.is_empty() || !pack.spells.spells.is_empty(),
        arts: art_rows(pack, &rt.progress),
    }
}

const HERO_NAMES: [&str; 3] = ["Vahn", "Noa", "Gala"];

fn art_rows(
    pack: &ExpansionPack,
    progress: &crate::quest_save::ExpansionProgress,
) -> Vec<ArtListRow> {
    let mut rows: Vec<ArtListRow> = pack
        .arts
        .arts
        .iter()
        .filter(|(k, _)| progress.arts.learned.contains(*k))
        .map(|(k, a)| {
            let cmds = a.command_bytes().unwrap_or_default();
            let arrows: Vec<String> = legaia_expansion_format::arts::commands_string(&cmds)
                .chars()
                .map(String::from)
                .collect();
            ArtListRow {
                title: format!(
                    "{}: {}",
                    HERO_NAMES[usize::from(a.character.slot())],
                    non_empty(&a.name, k)
                ),
                tag: format!("{} AP", a.ap),
                detail: if arrows.is_empty() {
                    "Chain finisher".to_string()
                } else {
                    arrows.join(" ")
                },
                spell: false,
            }
        })
        .collect();
    for ((roster, k), lv) in &progress.spells.known {
        let Some(d) = pack.spells.spells.get(k) else {
            continue;
        };
        let who = HERO_NAMES.get(usize::from(*roster)).copied().unwrap_or("?");
        rows.push(ArtListRow {
            title: format!("{who}: {}", non_empty(&d.name, k)),
            tag: format!("Lv {}", lv.level),
            detail: match d.mp {
                Some(mp) => format!("{mp} MP. {}", d.description),
                None => d.description.clone(),
            },
            spell: true,
        });
    }
    rows
}

fn item_rows(pack: &ExpansionPack, progress: &crate::items::ItemProgress) -> Vec<ItemRow> {
    let mut rows: Vec<ItemRow> = pack
        .items
        .items
        .iter()
        .filter(|(k, _)| progress.count(k) > 0 || progress.worn_count(k) > 0)
        .map(|(k, d)| ItemRow {
            key: k.clone(),
            name: non_empty(&d.name, k),
            description: d.description.replace('|', " "),
            kind: d.kind.label(),
            equipment: d.kind.is_equipment(),
            held: progress.count(k),
            worn: progress.worn_count(k),
        })
        .collect();
    rows.sort_by_key(|r| r.equipment);
    rows
}

/// The name a pack names item `r` by: a pack item's own name, `item 0xNN`
/// for a retail id (the Journal has no retail name table).
fn item_name(pack: &ExpansionPack, r: &legaia_expansion_format::items::ItemRef) -> String {
    r.pack_key()
        .map(|k| k.rsplit(':').next().unwrap_or(k))
        .and_then(|k| pack.items.items.get(k))
        .map_or_else(|| r.to_string(), |d| d.name.clone())
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

/// [`goal_text`], naming a collect objective's pack item by its name.
pub fn goal_text_in(pack: &ExpansionPack, goal: &Goal) -> String {
    match goal {
        Goal::CollectItem {
            item,
            count,
            hand_in,
        } if item.pack_key().is_some() => format!(
            "{} {count} {}",
            if *hand_in { "Bring" } else { "Collect" },
            item_name(pack, item)
        ),
        _ => goal_text(goal),
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
    /// Enter one of the pack's maps the party has been to directly (every
    /// map under the "Unlock Travel" cheat).
    Travel,
    /// The pack items held and worn; Cross opens the pack item screen.
    Items,
    /// The pack arts and spells the heroes know.
    Arts,
}

impl JournalPage {
    pub const ALL: [Self; 6] = [
        Self::Quests,
        Self::Challenges,
        Self::Cards,
        Self::Travel,
        Self::Items,
        Self::Arts,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Quests => "Quests",
            Self::Challenges => "Challenges",
            Self::Cards => "Cards",
            Self::Travel => "Travel",
            Self::Items => "Items",
            Self::Arts => "Arts",
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
    /// Cross on the Items page: open the pack item screen, on its Gear tab
    /// for a worn item (the pause menu closes first).
    OpenItems { gear: bool },
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
                JournalPage::Items => view.has_items.then(|| JournalOutcome::OpenItems {
                    gear: view.items.get(self.cursor).is_some_and(|i| i.equipment),
                }),
                JournalPage::Quests | JournalPage::Arts => None,
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
        let v = journal_view(&p, &rt, false);
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
    fn travel_lists_the_maps_the_party_has_entered() {
        let mut p = pack();
        for id in ["a_map", "b_map"] {
            p.maps.insert(id.into(), Default::default());
        }
        let w = crate::quest::StaticQuestWorld::default();
        let mut rt = QuestRuntime::default();
        rt.sync_pack(&p);
        let v = journal_view(&p, &rt, false);
        assert!(v.maps.is_empty(), "nothing visited yet");
        assert_eq!(v.maps_total, 2);
        // A retail scene is not a pack map; a pack map entered is recorded.
        rt.handle(
            &p,
            &w,
            crate::quest::QuestEvent::SceneEntered("town01".into()),
        );
        rt.handle(
            &p,
            &w,
            crate::quest::QuestEvent::SceneEntered("b_map".into()),
        );
        let ids: Vec<_> = journal_view(&p, &rt, false)
            .maps
            .into_iter()
            .map(|m| m.map_id)
            .collect();
        assert_eq!(ids, ["b_map"]);
        assert!(!rt.progress.flags.is_set(&visited_flag("town01")));
        // The visit survives the save block.
        let back = crate::quest_save::ExpansionProgress::decode(&rt.progress.encode()).unwrap();
        assert!(back.flags.is_set(&visited_flag("b_map")));
        // Unlocked, every map is offered.
        assert_eq!(journal_view(&p, &rt, true).maps.len(), 2);
    }

    #[test]
    fn the_session_pages_and_picks() {
        let mut v = journal_view(&pack(), &QuestRuntime::default(), false);
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
