//! The quest tracker: a deterministic state machine over a pack's quests.
//!
//! The `World` reports what happens in play as [`QuestEvent`]s (a battle won,
//! a scene entered, an NPC talked to, a mode won, ...) together with a
//! read-only view of the state quests test ([`QuestWorld`]: party level,
//! bag counts, retail system flags, the play clock). The tracker advances
//! every active quest's objectives, starts the quests whose giver was talked
//! to or whose prerequisites now hold, completes the quests whose objectives
//! are all done, and leaves two queues behind:
//!
//! - [`QuestNotice`]s a host shows (quest started / updated / completed,
//!   challenge cleared / failed);
//! - [`Grant`]s the `World` applies (gold, EXP, items, cards).
//!
//! Pack flags and counters ([`crate::flags::PackFlags`]) are applied by the
//! tracker itself. Same pack, same progress, same events -> same queues: no
//! RNG and no clock but the one [`QuestWorld::play_seconds`] reports.

use std::collections::BTreeMap;

use legaia_expansion_format::ExpansionPack;
use legaia_expansion_format::quest::{
    FormationRef, Goal, Prerequisites, QuestDef, QuestSection, Rewards,
};

use crate::dialog::DialogRuntime;
use crate::quest_challenge::ActiveChallenge;
use crate::quest_save::ExpansionProgress;

/// Something that happened in play that a quest may be waiting for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QuestEvent {
    /// A battle ended.
    BattleEnded {
        outcome: BattleOutcome,
        /// The runtime formation id (`FormationDef::formation_id`).
        formation_id: u16,
        /// The monster ids that fought.
        monsters: Vec<u16>,
        /// The formation record's boss header byte was non-zero.
        boss: bool,
    },
    /// The player entered a scene (CDNAME name or pack map id).
    SceneEntered(String),
    /// The player talked to the NPC with this id.
    Talked(String),
    /// The player won the pack mode with this id.
    ModeWon(String),
    /// Add to a pack counter (a script or a mode reporting progress).
    CounterAdd { counter: String, add: i64 },
    /// Raise a pack flag.
    FlagSet(String),
    /// Clear a pack flag.
    FlagCleared(String),
    /// Nothing happened, but the state quests read may have moved (a level
    /// up, an item gained, a story flag): re-test the state objectives and
    /// prerequisites.
    Refresh,
}

/// How a battle ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BattleOutcome {
    Won,
    Lost,
    Fled,
}

/// The state a quest reads but does not own. The `World` implements it.
pub trait QuestWorld {
    /// Whether the retail system flag `idx` is set.
    fn story_flag(&self, idx: u16) -> bool;
    /// The present party as `(roster slot, level)` pairs.
    fn party(&self) -> Vec<(u8, u8)>;
    /// How many of item `item` (a retail id or a pack item) the bags hold.
    fn item_count(&self, item: &legaia_expansion_format::items::ItemRef) -> u32;
    /// The save's play clock, in seconds.
    fn play_seconds(&self) -> u32;
    /// The runtime formation id the pack's `battle.toml` formation `id`
    /// compiled to; `None` when there is none.
    fn pack_formation_id(&self, id: &str) -> Option<u16> {
        let _ = id;
        None
    }
    /// The runtime formation id of the pack's `battle.toml` boss `id`;
    /// `None` when there is none.
    fn pack_boss_formation_id(&self, id: &str) -> Option<u16> {
        let _ = id;
        None
    }
    /// Whether the pack's once-only boss `id` has been beaten.
    fn boss_cleared(&self, id: &str) -> bool {
        let _ = id;
        false
    }
    /// The highest level in the present party.
    fn party_level(&self) -> u8 {
        self.party().iter().map(|&(_, l)| l).max().unwrap_or(0)
    }
}

/// What a host shows the player.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QuestNotice {
    /// A quest started; `dialog` is its start line, if it names one.
    Started {
        quest: String,
        dialog: Option<String>,
    },
    /// An objective moved: `progress` of `target`.
    Updated {
        quest: String,
        objective: usize,
        progress: u32,
        target: u32,
    },
    /// A quest completed; `dialog` is its completion line, if it names one.
    Completed {
        quest: String,
        dialog: Option<String>,
    },
    /// The player talked to the giver of an active quest; `dialog` is the
    /// quest's `active_dialog`.
    Reminder { quest: String, dialog: String },
    /// A challenge was armed for the next battle it applies to.
    ChallengeArmed { challenge: String },
    /// A challenge battle was won under its rules.
    ChallengeCleared {
        challenge: String,
        score: u32,
        best: u32,
        first_clear: bool,
    },
    /// A challenge battle was lost, fled, or won outside its rules.
    ChallengeFailed {
        challenge: String,
        reason: ChallengeFailure,
    },
}

/// Why a challenge failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChallengeFailure {
    Lost,
    Fled,
    /// Won, but past the turn limit.
    TurnLimit,
    /// Won, but the party broke the solo or level-cap rule.
    PartyRule,
}

/// What the `World` must apply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Grant {
    Gold(u32),
    /// EXP for each present party member.
    Xp(u32),
    /// `count` of a retail or pack item for the bag.
    Item {
        item: legaia_expansion_format::items::ItemRef,
        count: u8,
    },
    /// A hand-in: take `count` of `item` from the bag.
    TakeItem {
        item: legaia_expansion_format::items::ItemRef,
        count: u32,
    },
    Card(String),
    /// A pack fight to start once no dialogue is up.
    Fight(PackFight),
}

/// What a quest or a dialogue choice starts once the talk ends: a pack
/// fight, or a pack mode instance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PackFight {
    /// A `battle.toml` formation, by id.
    Formation(String),
    /// A `battle.toml` boss, by id.
    Boss(String),
    /// A `modes.toml` instance, by id.
    Mode(String),
}

/// Where a quest stands. A quest with no entry has never started.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuestStatus {
    Active,
    Completed,
}

/// One quest's saved progress.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuestState {
    pub status: QuestStatus,
    /// Per-objective progress, parallel to the quest's objectives.
    pub objectives: Vec<u32>,
    /// Times completed.
    pub completions: u32,
    /// Play clock at the last completion.
    pub last_completed_at: u32,
}

/// The tracker's whole runtime state, seated on `ExpansionState::quests`.
#[derive(Debug, Clone, Default)]
pub struct QuestRuntime {
    /// What the save carries.
    pub progress: ExpansionProgress,
    /// A loaded save's expansion block for a pack that is not the active one,
    /// kept verbatim so saving again does not drop it.
    pub dormant: Option<Vec<u8>>,
    /// The armed or running challenge.
    pub challenge: Option<ActiveChallenge>,
    /// The pack dialogue line on screen.
    pub dialog: DialogRuntime,
    /// Notices for the host, oldest first.
    pub notices: Vec<QuestNotice>,
    /// Grants for the `World`, oldest first.
    pub grants: Vec<Grant>,
    /// Trading-card ids the `World` received from [`Grant::Card`], oldest
    /// first, for the card collection to take.
    pub card_inbox: Vec<String>,
    /// Pack fights waiting for the dialogue channel to free up, oldest
    /// first.
    pub fights: Vec<PackFight>,
    /// What adopting saved progress for the active pack dropped or resized
    /// ([`crate::save_compat::reconcile`]), until the `World` takes it for
    /// its notice.
    pub compat: Option<crate::save_compat::CompatReport>,
}

/// One objective's progress target.
pub fn goal_target(goal: &Goal) -> u32 {
    match goal {
        Goal::DefeatFormation { count, .. }
        | Goal::DefeatMonster { count, .. }
        | Goal::WinBattles { count }
        | Goal::WinMode { count, .. }
        | Goal::CollectItem { count, .. } => (*count).max(1),
        _ => 1,
    }
}

/// The runtime formation id `r` names, if it resolves.
pub fn resolve_formation(r: &FormationRef, world: &dyn QuestWorld) -> Option<u16> {
    match r {
        FormationRef::Id(id) => Some(*id),
        FormationRef::Pack(name) => world.pack_formation_id(name),
    }
}

/// Count `event` toward `goal`. Returns whether `progress` moved.
fn apply_event(
    goal: &Goal,
    progress: &mut u32,
    event: &QuestEvent,
    world: &dyn QuestWorld,
) -> bool {
    let hit = match (goal, event) {
        (
            Goal::DefeatFormation { formation, .. },
            QuestEvent::BattleEnded {
                outcome: BattleOutcome::Won,
                formation_id,
                ..
            },
        ) => resolve_formation(formation, world) == Some(*formation_id),
        (
            Goal::DefeatMonster { monster, .. },
            QuestEvent::BattleEnded {
                outcome: BattleOutcome::Won,
                monsters,
                ..
            },
        ) => monsters.contains(monster),
        (
            Goal::WinBattles { .. },
            QuestEvent::BattleEnded {
                outcome: BattleOutcome::Won,
                ..
            },
        ) => true,
        (
            Goal::WinBoss {
                formation,
                boss: named,
            },
            QuestEvent::BattleEnded {
                outcome: BattleOutcome::Won,
                formation_id,
                boss,
                ..
            },
        ) => match (named, formation) {
            (Some(b), _) => world.pack_boss_formation_id(b) == Some(*formation_id),
            (None, Some(f)) => *boss && resolve_formation(f, world) == Some(*formation_id),
            (None, None) => *boss,
        },
        (Goal::EnterScene { scene }, QuestEvent::SceneEntered(s)) => scene == s,
        (Goal::Talk { npc }, QuestEvent::Talked(n)) => npc == n,
        (Goal::WinMode { mode, .. }, QuestEvent::ModeWon(m)) => mode == m,
        _ => false,
    };
    if hit && *progress < goal_target(goal) {
        *progress += 1;
        true
    } else {
        false
    }
}

/// Re-test a state objective against the world. Returns whether `progress`
/// moved. A level, counter or flag objective latches once met; a collect
/// objective tracks the bag live.
fn refresh_goal(
    goal: &Goal,
    progress: &mut u32,
    world: &dyn QuestWorld,
    flags: &crate::flags::PackFlags,
) -> bool {
    let next = match goal {
        Goal::ReachLevel { level } if world.party_level() >= *level => 1,
        Goal::Counter { counter, at_least } if flags.counter(counter) >= *at_least => 1,
        Goal::Flag { flag } if flags.is_set(flag) => 1,
        Goal::CollectItem { item, count, .. } => world.item_count(item).min((*count).max(1)),
        _ => return false,
    };
    let next = if matches!(goal, Goal::CollectItem { .. }) {
        next
    } else {
        next.max(*progress)
    };
    let moved = next != *progress;
    *progress = next;
    moved
}

fn goal_done(goal: &Goal, progress: u32) -> bool {
    progress >= goal_target(goal)
}

impl QuestRuntime {
    /// Bring the progress in line with the active pack: a different pack (or
    /// none before) starts fresh progress, adopting a dormant block saved for
    /// this pack if there is one, and parking the old pack's progress as
    /// dormant. With no pack the progress is left alone.
    pub fn sync_pack(&mut self, pack: &ExpansionPack) {
        let id = &pack.manifest.id;
        if self.progress.pack_id == *id {
            return;
        }
        let adopted = self
            .dormant
            .as_deref()
            .and_then(ExpansionProgress::decode)
            .filter(|p| p.pack_id == *id);
        let previous = std::mem::take(&mut self.progress);
        if adopted.is_some() {
            self.dormant = None;
        }
        if !previous.pack_id.is_empty() && self.dormant.is_none() {
            self.dormant = Some(previous.encode());
        }
        match adopted {
            Some(mut p) => {
                let report = crate::save_compat::reconcile(&mut p, pack);
                self.compat = (!report.is_clean() || report.version_changed()).then_some(report);
                self.progress = p;
            }
            None => self.progress = ExpansionProgress::new(pack),
        }
        self.progress.pack_version = pack.manifest.version.clone();
        self.challenge = None;
    }

    /// Set the active pack's progress aside as the dormant block (the pack
    /// is being deactivated), leaving the runtime empty.
    pub fn park(&mut self) {
        let progress = std::mem::take(&mut self.progress);
        let dormant = self.dormant.take();
        *self = Self::default();
        self.dormant = if progress.pack_id.is_empty() {
            dormant
        } else {
            Some(progress.encode())
        };
    }

    /// The save block for this runtime: the active pack's progress (synced
    /// first, so a block loaded before the pack was activated is adopted),
    /// else a dormant block, else nothing.
    pub fn save_block(&mut self, pack: Option<&ExpansionPack>) -> Option<Vec<u8>> {
        match pack {
            Some(p) => {
                self.sync_pack(p);
                Some(self.progress.encode())
            }
            None => self.dormant.clone(),
        }
    }

    /// Restore from a save block (`None` = the save carries none): a block
    /// for the active pack becomes the progress; any other block is kept
    /// dormant. Notices, grants, the challenge and the on-screen line are
    /// dropped either way.
    pub fn load_block(&mut self, pack: Option<&ExpansionPack>, block: Option<&[u8]>) {
        self.notices.clear();
        self.grants.clear();
        self.card_inbox.clear();
        self.fights.clear();
        self.challenge = None;
        self.dialog = DialogRuntime::default();
        self.compat = None;
        let decoded = block.and_then(ExpansionProgress::decode);
        match (pack, decoded) {
            (Some(p), Some(mut d)) if d.pack_id == p.manifest.id => {
                // A save from another version of the pack: drop what the
                // pack no longer defines before anything reads it.
                let report = crate::save_compat::reconcile(&mut d, p);
                self.compat = (!report.is_clean() || report.version_changed()).then_some(report);
                self.progress = d;
                self.progress.pack_version = p.manifest.version.clone();
                self.dormant = None;
            }
            (pack, _) => {
                self.dormant = block.map(<[u8]>::to_vec);
                self.progress = pack.map(ExpansionProgress::new).unwrap_or_default();
            }
        }
    }

    /// The saved state of quest `id`.
    pub fn quest(&self, id: &str) -> Option<&QuestState> {
        self.progress.quests.get(id)
    }

    /// Take the queued notices.
    pub fn take_notices(&mut self) -> Vec<QuestNotice> {
        std::mem::take(&mut self.notices)
    }

    /// Take the queued grants.
    pub fn take_grants(&mut self) -> Vec<Grant> {
        std::mem::take(&mut self.grants)
    }

    /// Feed one event through every quest, then start and complete what the
    /// new state allows.
    pub fn handle(&mut self, pack: &ExpansionPack, world: &dyn QuestWorld, event: QuestEvent) {
        self.sync_pack(pack);
        let section = &pack.quests;
        match &event {
            QuestEvent::CounterAdd { counter, add } => {
                self.progress.flags.add(counter, *add);
            }
            QuestEvent::FlagSet(f) => {
                self.progress.flags.set(f);
            }
            QuestEvent::FlagCleared(f) => {
                self.progress.flags.clear(f);
            }
            QuestEvent::BattleEnded { .. } => self.resolve_challenge(section, world, &event),
            // A pack map entered is a map the Journal's Travel page offers
            // from now on.
            QuestEvent::SceneEntered(map) if pack.maps.contains_key(map) => {
                self.progress.flags.set(&crate::journal::visited_flag(map));
            }
            _ => {}
        }
        let was_active: Vec<String> = self
            .progress
            .quests
            .iter()
            .filter(|(_, s)| s.status == QuestStatus::Active)
            .map(|(id, _)| id.clone())
            .collect();
        // Active quests first, so the talk that starts a quest is not
        // counted toward that quest's own talk objective.
        for q in &section.quests {
            self.advance(q, world, Some(&event));
        }
        if let QuestEvent::Talked(npc) = &event {
            // A talk to the giver of a quest that was already active (and is
            // still: this talk did not complete it) shows its reminder.
            for q in &section.quests {
                if q.giver.as_deref() == Some(npc)
                    && was_active.contains(&q.id)
                    && self
                        .quest(&q.id)
                        .is_some_and(|s| s.status == QuestStatus::Active)
                    && let Some(line) = &q.active_dialog
                {
                    self.notices.push(QuestNotice::Reminder {
                        quest: q.id.clone(),
                        dialog: line.clone(),
                    });
                }
            }
            for q in &section.quests {
                if q.giver.as_deref() == Some(npc) && self.startable(section, q, world) {
                    self.begin(q, world);
                }
            }
        }
        self.settle(section, world);
    }

    /// Start quest `id` from a script, if its prerequisites hold. Returns
    /// whether it started.
    pub fn start_quest(&mut self, pack: &ExpansionPack, world: &dyn QuestWorld, id: &str) -> bool {
        self.sync_pack(pack);
        let Some(q) = pack.quests.quest(id) else {
            return false;
        };
        if !self.startable(&pack.quests, q, world) {
            return false;
        }
        self.begin(q, world);
        self.settle(&pack.quests, world);
        true
    }

    /// Drop an active quest's progress (it can be taken again). Completed
    /// quests keep their record. Returns whether a quest was dropped.
    pub fn abandon_quest(&mut self, id: &str) -> bool {
        match self.progress.quests.get(id) {
            Some(s) if s.status == QuestStatus::Active && s.completions == 0 => {
                self.progress.quests.remove(id);
                true
            }
            Some(s) if s.status == QuestStatus::Active => {
                let s = self.progress.quests.get_mut(id).expect("present");
                s.status = QuestStatus::Completed;
                s.objectives.iter_mut().for_each(|p| *p = 0);
                true
            }
            _ => false,
        }
    }

    /// Whether `q` may start now.
    fn startable(&self, section: &QuestSection, q: &QuestDef, world: &dyn QuestWorld) -> bool {
        match self.progress.quests.get(&q.id) {
            Some(s) if s.status == QuestStatus::Active => return false,
            Some(s) => {
                let r = &q.repeat;
                if !r.repeatable
                    || r.max_completions.is_some_and(|m| s.completions >= m)
                    || world.play_seconds().saturating_sub(s.last_completed_at) < r.cooldown_seconds
                {
                    return false;
                }
            }
            None => {}
        }
        self.prerequisites_hold(section, &q.prerequisites, world)
    }

    /// Whether every condition of `p` holds: the quests it names completed
    /// at least once, its retail story flags set (or clear), its pack flags
    /// set (or clear), its once-only bosses beaten (or not) and the party
    /// level reached. A quest's start test, and the gate a map entrance,
    /// exit, trigger or NPC line opens on.
    pub fn prerequisites_hold(
        &self,
        section: &QuestSection,
        p: &Prerequisites,
        world: &dyn QuestWorld,
    ) -> bool {
        p.quests.iter().all(|id| {
            section.quest(id).is_some()
                && self
                    .progress
                    .quests
                    .get(id)
                    .is_some_and(|s| s.completions > 0)
        }) && p.story_flags.iter().all(|&f| world.story_flag(f))
            && p.story_flags_clear.iter().all(|&f| !world.story_flag(f))
            && p.flags.iter().all(|f| self.progress.flags.is_set(f))
            && p.flags_clear.iter().all(|f| !self.progress.flags.is_set(f))
            && p.bosses.iter().all(|b| world.boss_cleared(b))
            && p.bosses_unbeaten.iter().all(|b| !world.boss_cleared(b))
            && p.min_level.is_none_or(|l| world.party_level() >= l)
    }

    fn begin(&mut self, q: &QuestDef, world: &dyn QuestWorld) {
        let entry = self
            .progress
            .quests
            .entry(q.id.clone())
            .or_insert(QuestState {
                status: QuestStatus::Active,
                objectives: Vec::new(),
                completions: 0,
                last_completed_at: 0,
            });
        entry.status = QuestStatus::Active;
        entry.objectives = vec![0; q.objectives.len()];
        self.notices.push(QuestNotice::Started {
            quest: q.id.clone(),
            dialog: q.start_dialog.clone(),
        });
        if let Some(f) = &q.start_battle {
            self.grants
                .push(Grant::Fight(PackFight::Formation(f.clone())));
        }
        if let Some(b) = &q.start_boss {
            self.grants.push(Grant::Fight(PackFight::Boss(b.clone())));
        }
        self.advance(q, world, None);
    }

    /// Run `event` (or only the state re-test) over `q`'s objectives and
    /// complete it when they are all done.
    fn advance(&mut self, q: &QuestDef, world: &dyn QuestWorld, event: Option<&QuestEvent>) {
        let Some(state) = self.progress.quests.get_mut(&q.id) else {
            return;
        };
        if state.status != QuestStatus::Active {
            return;
        }
        state.objectives.resize(q.objectives.len(), 0);
        let mut updates = Vec::new();
        let mut event = event;
        for (i, obj) in q.objectives.iter().enumerate() {
            let p = &mut state.objectives[i];
            let mut moved = false;
            if let Some(e) = event {
                moved |= apply_event(&obj.goal, p, e, world);
            }
            moved |= refresh_goal(&obj.goal, p, world, &self.progress.flags);
            if moved {
                updates.push((i, *p, goal_target(&obj.goal)));
            }
            if q.ordered {
                if !goal_done(&obj.goal, *p) {
                    break;
                }
                // An event counts toward one objective of an ordered quest:
                // the ones after it only re-test the state.
                if moved {
                    event = None;
                }
            }
        }
        for (objective, progress, target) in updates {
            self.notices.push(QuestNotice::Updated {
                quest: q.id.clone(),
                objective,
                progress,
                target,
            });
        }
        let state = &self.progress.quests[&q.id];
        let done = q
            .objectives
            .iter()
            .zip(&state.objectives)
            .all(|(o, &p)| goal_done(&o.goal, p));
        if done {
            self.complete(q, world);
        }
    }

    fn complete(&mut self, q: &QuestDef, world: &dyn QuestWorld) {
        let state = self.progress.quests.get_mut(&q.id).expect("active quest");
        state.status = QuestStatus::Completed;
        state.completions = state.completions.saturating_add(1);
        state.last_completed_at = world.play_seconds();
        for o in &q.objectives {
            if let Goal::CollectItem {
                item,
                count,
                hand_in: true,
            } = &o.goal
            {
                self.grants.push(Grant::TakeItem {
                    item: item.clone(),
                    count: *count,
                });
            }
        }
        self.reward(&q.rewards);
        self.notices.push(QuestNotice::Completed {
            quest: q.id.clone(),
            dialog: q.complete_dialog.clone(),
        });
    }

    /// Queue `r`'s grants and apply its flags and counters.
    pub(crate) fn reward(&mut self, r: &Rewards) {
        if r.gold > 0 {
            self.grants.push(Grant::Gold(r.gold));
        }
        if r.xp > 0 {
            self.grants.push(Grant::Xp(r.xp));
        }
        for i in &r.items {
            self.grants.push(Grant::Item {
                item: i.item.clone(),
                count: i.count,
            });
        }
        for c in &r.cards {
            self.grants.push(Grant::Card(c.clone()));
        }
        for f in &r.set_flags {
            self.progress.flags.set(f);
        }
        for c in &r.counters {
            self.progress.flags.add(&c.counter, c.add);
        }
    }

    /// Re-test every active quest and auto-start what may start, until
    /// nothing moves. A completion's flags can unlock the next quest of a
    /// chain within one call; the loop is bounded by the quest count.
    fn settle(&mut self, section: &QuestSection, world: &dyn QuestWorld) {
        for _ in 0..=section.quests.len() {
            let before = self.notices.len();
            for q in &section.quests {
                self.advance(q, world, None);
            }
            for q in &section.quests {
                if q.auto_start && self.startable(section, q, world) {
                    self.begin(q, world);
                }
            }
            if self.notices.len() == before {
                break;
            }
        }
    }

    /// The journal: every quest that has started, in pack order.
    pub fn journal<'a>(&self, section: &'a QuestSection) -> Vec<JournalEntry<'a>> {
        section
            .quests
            .iter()
            .filter_map(|q| {
                let s = self.progress.quests.get(&q.id)?;
                let objectives = q
                    .objectives
                    .iter()
                    .enumerate()
                    .map(|(i, o)| {
                        let progress = s.objectives.get(i).copied().unwrap_or(0);
                        let target = goal_target(&o.goal);
                        JournalObjective {
                            text: o.text.as_deref(),
                            goal: &o.goal,
                            progress: if s.status == QuestStatus::Completed && s.completions > 0 {
                                target
                            } else {
                                progress
                            },
                            target,
                        }
                    })
                    .collect();
                Some(JournalEntry {
                    quest: q,
                    status: s.status,
                    completions: s.completions,
                    objectives,
                })
            })
            .collect()
    }
}

/// One quest as the journal shows it.
#[derive(Debug, Clone, PartialEq)]
pub struct JournalEntry<'a> {
    pub quest: &'a QuestDef,
    pub status: QuestStatus,
    pub completions: u32,
    pub objectives: Vec<JournalObjective<'a>>,
}

/// One objective as the journal shows it.
#[derive(Debug, Clone, PartialEq)]
pub struct JournalObjective<'a> {
    pub text: Option<&'a str>,
    pub goal: &'a Goal,
    pub progress: u32,
    pub target: u32,
}

/// A fixed [`QuestWorld`] for tests and tools.
#[derive(Debug, Clone, Default)]
pub struct StaticQuestWorld {
    pub story_flags: Vec<u16>,
    pub pack_formations: BTreeMap<String, u16>,
    pub pack_bosses: BTreeMap<String, u16>,
    pub party: Vec<(u8, u8)>,
    pub items: BTreeMap<u8, u32>,
    pub play_seconds: u32,
}

impl QuestWorld for StaticQuestWorld {
    fn story_flag(&self, idx: u16) -> bool {
        self.story_flags.contains(&idx)
    }
    fn party(&self) -> Vec<(u8, u8)> {
        self.party.clone()
    }
    fn item_count(&self, item: &legaia_expansion_format::items::ItemRef) -> u32 {
        item.retail_id()
            .and_then(|id| self.items.get(&id))
            .copied()
            .unwrap_or(0)
    }
    fn play_seconds(&self) -> u32 {
        self.play_seconds
    }
    fn pack_formation_id(&self, id: &str) -> Option<u16> {
        self.pack_formations.get(id).copied()
    }
    fn pack_boss_formation_id(&self, id: &str) -> Option<u16> {
        self.pack_bosses.get(id).copied()
    }
}

/// The party level each `battle.toml` boss a quest asks for is first met
/// at: the highest `min_level` along the quest's prerequisite chain, and the
/// `reach_level` objectives of the quests that chain completes. A boss more
/// than one quest names takes the lowest. Bosses no quest names are absent.
///
/// The pack's own pacing, read from data it already carries - what a Boss
/// Rush stages its pack bosses at.
pub fn boss_chain_levels(section: &QuestSection) -> BTreeMap<String, u8> {
    fn chain(section: &QuestSection, q: &QuestDef, seen: &mut Vec<String>) -> u8 {
        if seen.contains(&q.id) {
            return 0;
        }
        seen.push(q.id.clone());
        let mut level = q.prerequisites.min_level.unwrap_or(0);
        for id in &q.prerequisites.quests {
            let Some(p) = section.quest(id) else {
                continue;
            };
            let reached = p
                .objectives
                .iter()
                .filter_map(|o| match o.goal {
                    Goal::ReachLevel { level } => Some(level),
                    _ => None,
                })
                .max()
                .unwrap_or(0);
            level = level.max(reached).max(chain(section, p, seen));
        }
        level
    }
    let mut out: BTreeMap<String, u8> = BTreeMap::new();
    for q in &section.quests {
        for o in &q.objectives {
            if let Goal::WinBoss { boss: Some(b), .. } = &o.goal {
                let level = chain(section, q, &mut Vec::new());
                out.entry(b.clone())
                    .and_modify(|l| *l = (*l).min(level))
                    .or_insert(level);
            }
        }
    }
    out
}

#[cfg(test)]
#[path = "quest_tests.rs"]
mod tests;
