//! `quests.toml`: quests and challenges.
//!
//! A **quest** is a goal the player is given (by an NPC, by a script, or on
//! its own once its prerequisites hold), a list of objectives the engine
//! tracks from what happens in play, and a reward. A **challenge** is a rule
//! set laid over one battle - no items, a turn limit, one character - with a
//! score the engine computes when the battle is won.
//!
//! Everything a quest names is an id: retail content by its coordinate (a
//! formation id, a monster id, a CDNAME scene name, an item id, a system
//! flag), pack content by the string id the pack gives it (an NPC, a
//! dialogue line, a mode, a counter, a flag). Pack flags and counters are
//! the pack's own namespace and never alias a retail story flag.
//!
//! ```toml
//! [[quests]]
//! id = "tower_intro"
//! title = "The Rebirth Tower"
//! giver = "tower_guide"
//! start_dialog = "guide_offer"
//! prerequisites = { min_level = 20 }
//! objectives = [
//!     { kind = "win_battles", count = 3 },
//!     { kind = "reach_level", level = 40 },
//! ]
//! rewards = { gold = 5000, set_flags = ["tower_open"] }
//! ```

use serde::{Deserialize, Serialize};

/// The `quests.toml` section.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct QuestSection {
    /// Every quest, in journal order (`[[quests]]`).
    pub quests: Vec<QuestDef>,
    /// Every challenge (`[[challenges]]`).
    pub challenges: Vec<ChallengeDef>,
}

impl QuestSection {
    /// The quest with id `id`.
    pub fn quest(&self, id: &str) -> Option<&QuestDef> {
        self.quests.iter().find(|q| q.id == id)
    }

    /// The challenge with id `id`.
    pub fn challenge(&self, id: &str) -> Option<&ChallengeDef> {
        self.challenges.iter().find(|c| c.id == id)
    }
}

/// One quest.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct QuestDef {
    /// Pack-unique id; the key the save records progress under.
    pub id: String,
    /// Journal title.
    pub title: String,
    /// Journal description.
    pub description: String,
    /// The NPC id that offers the quest. Talking to it while the
    /// prerequisites hold starts the quest. `None` = started by
    /// [`Self::auto_start`] or by a script.
    pub giver: Option<String>,
    /// Start as soon as the prerequisites hold, with no giver.
    pub auto_start: bool,
    /// What must hold before the quest can start.
    pub prerequisites: Prerequisites,
    /// `true` (the default): objectives complete in listed order, and only
    /// the current one takes progress. `false`: all of them run at once.
    #[serde(default = "yes")]
    pub ordered: bool,
    /// The objectives; the quest completes when every one is done.
    pub objectives: Vec<ObjectiveDef>,
    /// What completing the quest grants.
    pub rewards: Rewards,
    /// Whether, and how often, the quest can run again.
    pub repeat: Repeat,
    /// Dialogue line shown when the quest starts.
    pub start_dialog: Option<String>,
    /// Dialogue line shown when the quest completes.
    pub complete_dialog: Option<String>,
}

fn yes() -> bool {
    true
}

/// A quest's start conditions. Every listed condition must hold.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Prerequisites {
    /// Quests that must have been completed at least once.
    pub quests: Vec<String>,
    /// Retail system flags (the field VM's flag bank, the `0x50` / `0x60` /
    /// `0x70` ops) that must be set - how a pack keys a quest to a point in
    /// the story.
    pub story_flags: Vec<u16>,
    /// Retail system flags that must be clear.
    pub story_flags_clear: Vec<u16>,
    /// Pack flags that must be set.
    pub flags: Vec<String>,
    /// The highest party level must be at least this.
    pub min_level: Option<u8>,
}

/// One objective: a goal plus an optional journal line.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ObjectiveDef {
    /// Journal text; hosts fall back to a description of the goal.
    #[serde(default)]
    pub text: Option<String>,
    /// What the player must do.
    #[serde(flatten)]
    pub goal: Goal,
}

/// What an objective asks for. The `kind` key selects the variant.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Goal {
    /// Win `count` battles against the formation `formation` (the runtime
    /// formation id, `FormationDef::formation_id`).
    DefeatFormation {
        formation: u16,
        #[serde(default = "one")]
        count: u32,
    },
    /// Win `count` battles in which the monster `monster` (1-based PROT 867
    /// record id) fought.
    DefeatMonster {
        monster: u16,
        #[serde(default = "one")]
        count: u32,
    },
    /// Win `count` battles of any kind.
    WinBattles {
        #[serde(default = "one")]
        count: u32,
    },
    /// Win a boss battle: one whose formation record carries a non-zero
    /// header byte (retail's scripted / boss rows), optionally a specific
    /// one.
    WinBoss {
        #[serde(default)]
        formation: Option<u16>,
    },
    /// Enter the scene `scene` (a CDNAME scene name, or a pack map id).
    EnterScene { scene: String },
    /// The highest party level reaches `level`.
    ReachLevel { level: u8 },
    /// Hold `count` of item `item`. With `hand_in`, completing the quest
    /// takes them from the bag.
    CollectItem {
        item: u8,
        #[serde(default = "one")]
        count: u32,
        #[serde(default)]
        hand_in: bool,
    },
    /// Talk to the NPC `npc`.
    Talk { npc: String },
    /// Win the game mode `mode` (a pack mode id) `count` times.
    WinMode {
        mode: String,
        #[serde(default = "one")]
        count: u32,
    },
    /// The pack counter `counter` reaches at least `at_least`.
    Counter { counter: String, at_least: i64 },
    /// The pack flag `flag` is set.
    Flag { flag: String },
}

fn one() -> u32 {
    1
}

/// What completing a quest (or clearing a challenge) grants.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Rewards {
    /// Gold added to the party purse.
    pub gold: u32,
    /// EXP granted to each party member.
    pub xp: u32,
    /// Items added to the bag.
    pub items: Vec<ItemReward>,
    /// Trading-card ids granted (see `cards.toml`).
    pub cards: Vec<String>,
    /// Pack flags raised.
    pub set_flags: Vec<String>,
    /// Pack counters bumped.
    pub counters: Vec<CounterAdd>,
}

/// `count` of item `item`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ItemReward {
    pub item: u8,
    #[serde(default = "one_u8")]
    pub count: u8,
}

fn one_u8() -> u8 {
    1
}

/// Add `add` to the pack counter `counter`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CounterAdd {
    pub counter: String,
    #[serde(default = "one_i64")]
    pub add: i64,
}

fn one_i64() -> i64 {
    1
}

/// Whether a completed quest can run again.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Repeat {
    /// The quest can be taken again after completing.
    pub repeatable: bool,
    /// Play-clock seconds that must pass after a completion before the quest
    /// can start again (the "daily" shape, on the save's play clock).
    pub cooldown_seconds: u32,
    /// Cap on completions; `None` = no cap.
    pub max_completions: Option<u32>,
}

/// One challenge: a rule set over a battle, a score, and a reward.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ChallengeDef {
    /// Pack-unique id; the key the save records the best score under.
    pub id: String,
    pub title: String,
    pub description: String,
    /// The formation the challenge is fought against; `None` = the next
    /// battle after it is armed, whatever it is.
    pub formation: Option<u16>,
    /// The rules.
    pub rules: ChallengeRules,
    /// How the clear is scored.
    pub score: ScoreRule,
    /// Granted on the first clear.
    pub rewards: Rewards,
}

/// The rules a challenge lays over its battle.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ChallengeRules {
    /// The battle must be won within this many rounds.
    pub turn_limit: Option<u16>,
    /// The battle Item command is refused.
    pub no_items: bool,
    /// The battle Magic (Ra-Seru) command is refused.
    pub no_magic: bool,
    /// Only one party member may be present: its roster slot.
    pub solo: Option<u8>,
    /// No party member may be above this level.
    pub level_cap: Option<u8>,
    /// Enemy HP and attack scale, per mille (`1000` = retail).
    pub enemy_scale_permille: Option<u16>,
}

/// Score for a cleared challenge:
/// `base + turn_bonus * (turn_limit - rounds) + level_bonus * (level_cap - party level)`,
/// each bonus term counted only when its rule is set and never below zero.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ScoreRule {
    pub base: u32,
    /// Per round left under the turn limit.
    pub turn_bonus: u32,
    /// Per level the party's highest member sits under the level cap.
    pub level_bonus: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_quest_with_every_goal_kind_parses() {
        let text = r#"
            [[quests]]
            id = "q"
            title = "Q"
            giver = "npc"
            ordered = false
            prerequisites = { quests = ["p"], story_flags = [0x142], min_level = 5 }
            objectives = [
                { kind = "defeat_formation", formation = 12, count = 3 },
                { kind = "defeat_monster", monster = 0x2A },
                { kind = "win_battles", count = 2, text = "Win two fights" },
                { kind = "win_boss" },
                { kind = "enter_scene", scene = "town01" },
                { kind = "reach_level", level = 40 },
                { kind = "collect_item", item = 0x10, count = 3, hand_in = true },
                { kind = "talk", npc = "guide" },
                { kind = "win_mode", mode = "poker" },
                { kind = "counter", counter = "kills", at_least = 10 },
                { kind = "flag", flag = "door" },
            ]
            rewards = { gold = 100, items = [{ item = 0x10, count = 2 }], cards = ["c"] }
            repeat = { repeatable = true, cooldown_seconds = 3600 }
        "#;
        let s: QuestSection = toml::from_str(text).unwrap();
        let q = s.quest("q").unwrap();
        assert!(!q.ordered);
        assert_eq!(q.objectives.len(), 11);
        assert_eq!(
            q.objectives[0].goal,
            Goal::DefeatFormation {
                formation: 12,
                count: 3
            }
        );
        assert_eq!(
            q.objectives[1].goal,
            Goal::DefeatMonster {
                monster: 0x2A,
                count: 1
            }
        );
        assert_eq!(q.objectives[2].text.as_deref(), Some("Win two fights"));
        assert_eq!(q.prerequisites.story_flags, vec![0x142]);
        assert_eq!(q.rewards.items[0].count, 2);
        assert!(q.repeat.repeatable);
    }

    #[test]
    fn a_bare_quest_defaults_to_ordered() {
        let s: QuestSection = toml::from_str("[[quests]]\nid = \"a\"\n").unwrap();
        assert!(s.quests[0].ordered);
        assert!(s.quests[0].objectives.is_empty());
    }

    #[test]
    fn a_challenge_parses() {
        let text = r#"
            [[challenges]]
            id = "c"
            formation = 7
            rules = { turn_limit = 5, no_items = true, level_cap = 30 }
            score = { base = 1000, turn_bonus = 100 }
        "#;
        let s: QuestSection = toml::from_str(text).unwrap();
        let c = s.challenge("c").unwrap();
        assert_eq!(c.rules.turn_limit, Some(5));
        assert!(c.rules.no_items && !c.rules.no_magic);
        assert_eq!(c.score.turn_bonus, 100);
    }

    #[test]
    fn an_unknown_goal_kind_is_an_error() {
        let text = "[[quests]]\nid = \"a\"\nobjectives = [{ kind = \"fly\" }]\n";
        assert!(toml::from_str::<QuestSection>(text).is_err());
    }
}
