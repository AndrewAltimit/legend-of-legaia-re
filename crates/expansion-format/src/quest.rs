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
    /// Dialogue line shown when the player talks to the giver while the quest
    /// is active (a reminder, or the offer again).
    pub active_dialog: Option<String>,
    /// A `battle.toml` formation fought when the quest starts (after its
    /// start line).
    pub start_battle: Option<String>,
    /// A `battle.toml` boss fought when the quest starts (after its start
    /// line).
    pub start_boss: Option<String>,
    /// A `cutscenes.toml` cutscene played when the quest starts (after its
    /// start line).
    pub start_cutscene: Option<String>,
    /// A `cutscenes.toml` cutscene played when the quest completes (after
    /// its completion line, once the player is back in the field).
    pub complete_cutscene: Option<String>,
}

fn yes() -> bool {
    true
}

/// A quest's start conditions. Every listed condition must hold. The same
/// shape gates a map's entrances, exits, triggers and NPC lines.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
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
    /// Pack flags that must be clear.
    pub flags_clear: Vec<String>,
    /// Once-only `battle.toml` bosses (`once = true`) that must have been
    /// beaten.
    pub bosses: Vec<String>,
    /// Once-only bosses not yet beaten - the gate that retires a boss's
    /// challenge once it is won.
    pub bosses_unbeaten: Vec<String>,
    /// The highest party level must be at least this.
    pub min_level: Option<u8>,
}

impl Prerequisites {
    /// Whether nothing is required: the gate is always open.
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// Validate the references of a gate at `at`: the quests it names, and
    /// no empty pack flag ids.
    pub(crate) fn check(&self, cx: &mut crate::validate::Checker<'_>, at: &str) {
        for q in &self.quests {
            cx.reference("quest", q, &format!("{at}.quests"));
        }
        for b in &self.bosses {
            cx.reference("boss", b, &format!("{at}.bosses"));
        }
        for b in &self.bosses_unbeaten {
            cx.reference("boss", b, &format!("{at}.bosses_unbeaten"));
        }
        for f in self.flags.iter().chain(&self.flags_clear) {
            if f.trim().is_empty() {
                cx.error(at, "an empty pack flag id");
            }
        }
        if self.min_level.is_some_and(|l| !(1..=99).contains(&l)) {
            cx.error(&format!("{at}.min_level"), "a level is 1..=99");
        }
    }
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
    /// Win `count` battles against the formation `formation`.
    DefeatFormation {
        formation: FormationRef,
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
    /// Win a boss battle: the `battle.toml` boss `boss`, or the formation
    /// `formation`, or with neither any battle whose formation record
    /// carries a non-zero header byte (retail's scripted / boss rows).
    WinBoss {
        #[serde(default)]
        formation: Option<FormationRef>,
        #[serde(default)]
        boss: Option<String>,
    },
    /// Enter the scene `scene` (a CDNAME scene name, or a pack map id).
    EnterScene { scene: String },
    /// The highest party level reaches `level`.
    ReachLevel { level: u8 },
    /// Hold `count` of item `item`. With `hand_in`, completing the quest
    /// takes them from the bag.
    CollectItem {
        /// A retail id or a pack item ([`crate::items::ItemRef`]).
        item: crate::items::ItemRef,
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

/// A formation: a runtime formation id (`FormationDef::formation_id`, the
/// id space retail encounters use), or the id of a formation the pack's
/// `battle.toml` defines.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum FormationRef {
    Id(u16),
    Pack(String),
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
    /// A retail id or a pack item ([`crate::items::ItemRef`]).
    pub item: crate::items::ItemRef,
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
    pub formation: Option<FormationRef>,
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

impl Rewards {
    fn check(&self, cx: &mut crate::validate::Checker<'_>, at: &str) {
        for (i, r) in self.items.iter().enumerate() {
            let field = format!("{at}.items[{i}]");
            r.item.check(cx, &field);
            if r.count == 0 {
                cx.warning(&field, "count 0 grants nothing");
            }
        }
        for f in &self.set_flags {
            if f.trim().is_empty() {
                cx.error(&format!("{at}.set_flags"), "an empty pack flag id");
            }
        }
        for (i, c) in self.cards.iter().enumerate() {
            if cx.pack().cards.card(c).is_none() {
                cx.error(
                    &format!("{at}.cards[{i}]"),
                    format!("no card {c:?} in cards.toml"),
                );
            }
        }
    }
}

fn reference_formation(cx: &mut crate::validate::Checker<'_>, r: &FormationRef, at: &str) {
    if let FormationRef::Pack(name) = r {
        cx.reference("formation", name, at);
    }
}

impl crate::validate::SectionRules for QuestSection {
    fn declare(&self, cx: &mut crate::validate::Checker<'_>) {
        for (i, q) in self.quests.iter().enumerate() {
            cx.declare("quest", &q.id, &format!("quests[{i}].id"));
        }
        for (i, c) in self.challenges.iter().enumerate() {
            cx.declare("challenge", &c.id, &format!("challenges[{i}].id"));
        }
    }

    fn check(&self, cx: &mut crate::validate::Checker<'_>) {
        for (i, q) in self.quests.iter().enumerate() {
            let at = format!("quests[{i}]");
            if q.title.trim().is_empty() {
                cx.warning(&at, format!("quest {:?} has no title", q.id));
            }
            for p in &q.prerequisites.quests {
                if *p == q.id {
                    cx.error(
                        &format!("{at}.prerequisites"),
                        "a quest cannot require itself",
                    );
                }
                cx.reference("quest", p, &format!("{at}.prerequisites.quests"));
            }
            for (key, line) in [
                ("start_dialog", &q.start_dialog),
                ("complete_dialog", &q.complete_dialog),
                ("active_dialog", &q.active_dialog),
            ] {
                if let Some(l) = line {
                    cx.reference("line", l, &format!("{at}.{key}"));
                }
            }
            if let Some(f) = &q.start_battle {
                cx.reference("formation", f, &format!("{at}.start_battle"));
            }
            if let Some(b) = &q.start_boss {
                cx.reference("boss", b, &format!("{at}.start_boss"));
            }
            for (key, c) in [
                ("start_cutscene", &q.start_cutscene),
                ("complete_cutscene", &q.complete_cutscene),
            ] {
                if let Some(c) = c {
                    cx.reference("cutscene", c, &format!("{at}.{key}"));
                }
            }
            if q.start_battle.is_some() && q.start_boss.is_some() {
                cx.error(&at, "a quest starts a battle or a boss, not both");
            }
            for (k, o) in q.objectives.iter().enumerate() {
                let field = format!("{at}.objectives[{k}]");
                match &o.goal {
                    Goal::DefeatFormation { formation, count } => {
                        reference_formation(cx, formation, &field);
                        zero_count(cx, *count, &field);
                    }
                    Goal::DefeatMonster { monster, count } => {
                        cx.disc_ref(&field, &crate::refs::DiscRef::Monster { id: *monster });
                        zero_count(cx, *count, &field);
                    }
                    Goal::WinBattles { count } | Goal::WinMode { count, .. } => {
                        zero_count(cx, *count, &field);
                    }
                    Goal::WinBoss { formation, boss } => {
                        if let Some(f) = formation {
                            reference_formation(cx, f, &field);
                        }
                        if let Some(b) = boss {
                            cx.reference("boss", b, &field);
                        }
                        if formation.is_some() && boss.is_some() {
                            cx.error(&field, "name the boss or the formation, not both");
                        }
                    }
                    Goal::CollectItem { item, count, .. } => {
                        item.check(cx, &field);
                        zero_count(cx, *count, &field);
                    }
                    Goal::ReachLevel { level } if !(1..=99).contains(level) => {
                        cx.error(&field, format!("level {level} is not 1..=99"));
                    }
                    _ => {}
                }
            }
            if q.giver.is_none() && !q.auto_start {
                cx.warning(
                    &at,
                    format!(
                        "quest {:?} has no giver and no auto_start; only a script starts it",
                        q.id
                    ),
                );
            }
            if !q.repeat.repeatable
                && (q.repeat.cooldown_seconds > 0 || q.repeat.max_completions.is_some())
            {
                cx.warning(
                    &format!("{at}.repeat"),
                    "cooldown and cap apply only to a repeatable quest",
                );
            }
            q.rewards.check(cx, &format!("{at}.rewards"));
        }
        for (i, c) in self.challenges.iter().enumerate() {
            let at = format!("challenges[{i}]");
            if let Some(f) = &c.formation {
                reference_formation(cx, f, &format!("{at}.formation"));
            }
            if c.rules.turn_limit == Some(0) {
                cx.error(
                    &format!("{at}.rules.turn_limit"),
                    "a turn limit of 0 cannot be met",
                );
            }
            if c.rules.solo.is_some_and(|s| s > 3) {
                cx.error(
                    &format!("{at}.rules.solo"),
                    "solo names a roster slot 0..=3",
                );
            }
            if c.rules.enemy_scale_permille == Some(0) {
                cx.error(
                    &format!("{at}.rules.enemy_scale_permille"),
                    "a scale of 0 removes the enemies' stats",
                );
            }
            c.rewards.check(cx, &format!("{at}.rewards"));
        }
    }
}

fn zero_count(cx: &mut crate::validate::Checker<'_>, count: u32, at: &str) {
    if count == 0 {
        cx.warning(at, "count 0 is met at once");
    }
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
                { kind = "win_boss", formation = "sovereign" },
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
                formation: FormationRef::Id(12),
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
        assert_eq!(
            q.objectives[3].goal,
            Goal::WinBoss {
                formation: Some(FormationRef::Pack("sovereign".into())),
                boss: None,
            }
        );
        assert_eq!(q.prerequisites.story_flags, vec![0x142]);
        assert_eq!(q.rewards.items[0].count, 2);
        assert!(q.repeat.repeatable);
    }

    #[test]
    fn a_reward_card_must_be_in_cards_toml() {
        let quests: QuestSection = toml::from_str(
            "[[quests]]\nid = \"a\"\ntitle = \"A\"\nauto_start = true\nrewards = { cards = [\"ghost\", \"real\"] }\n",
        )
        .unwrap();
        let cards = toml::from_str(
            "[[cards]]\nid = \"real\"\nname = \"Real\"\nrarity = \"common\"\nsides = [1, 1, 1, 1]\n",
        )
        .unwrap();
        let pack = crate::ExpansionPack {
            manifest: crate::manifest::PackManifest::new("t", "T", "0.1.0"),
            quests,
            cards,
            ..Default::default()
        };
        let errors: Vec<String> = crate::validate::validate(&pack)
            .iter()
            .filter(|i| i.severity == crate::validate::Severity::Error)
            .map(ToString::to_string)
            .collect();
        assert!(
            errors.iter().any(|e| e.contains("no card \"ghost\"")),
            "{errors:#?}"
        );
        assert!(
            !errors.iter().any(|e| e.contains("\"real\"")),
            "{errors:#?}"
        );
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

    fn pack_with(quests: &str, dialog: &str, battle: &str) -> crate::ExpansionPack {
        let mut files = crate::PackFiles::new();
        files.insert(
            "pack.toml".into(),
            b"id = \"t\"\nname = \"T\"\nversion = \"1.0.0\"\ndescription = \"d\"\nauthors = [\"a\"]\nlicense = \"MIT\"\n".to_vec(),
        );
        files.insert("quests.toml".into(), quests.as_bytes().to_vec());
        files.insert("dialog.toml".into(), dialog.as_bytes().to_vec());
        files.insert("battle.toml".into(), battle.as_bytes().to_vec());
        crate::ExpansionPack::from_files(&files).unwrap()
    }

    fn errors(pack: &crate::ExpansionPack) -> Vec<String> {
        crate::validate::validate(pack)
            .into_iter()
            .filter(|i| i.severity == crate::validate::Severity::Error)
            .map(|i| format!("{} [{}]: {}", i.path, i.field, i.message))
            .collect()
    }

    const BATTLE: &str = r#"
[formations.ogres]
monsters = [{ kind = "monster", id = 12 }]
[bosses.king]
formation = "ogres"
"#;

    #[test]
    fn a_well_formed_quest_and_line_validate_clean() {
        let p = pack_with(
            r#"
            [[quests]]
            id = "q"
            title = "Q"
            giver = "npc"
            start_dialog = "hello"
            start_boss = "king"
            objectives = [
                { kind = "defeat_formation", formation = "ogres" },
                { kind = "win_boss", boss = "king" },
            ]
            [[challenges]]
            id = "c"
            formation = "ogres"
            "#,
            r#"
            [lines.hello]
            text = "Hi."
            choices = [{ label = "Fight", battle = "ogres", next = "bye" }, { label = "No" }]
            [lines.bye]
            text = "Bye."
            "#,
            BATTLE,
        );
        assert!(errors(&p).is_empty(), "{:?}", errors(&p));
    }

    #[test]
    fn dangling_references_and_bad_text_are_errors() {
        let p = pack_with(
            r#"
            [[quests]]
            id = "q"
            auto_start = true
            prerequisites = { quests = ["missing"] }
            complete_dialog = "nope"
            objectives = [
                { kind = "defeat_formation", formation = "ghosts" },
                { kind = "win_boss", boss = "queen" },
                { kind = "reach_level", level = 120 },
            ]
            [[quests]]
            id = "q"
            auto_start = true
            "#,
            r#"
            [lines.hello]
            text = "caf\u00e9"
            choices = [{ label = "a", next = "gone" }]
            "#,
            BATTLE,
        );
        let e = errors(&p);
        for needle in [
            "quest \"missing\"",
            "line \"nope\"",
            "formation \"ghosts\"",
            "boss \"queen\"",
            "level 120",
            "already defined",
            "line \"gone\"",
            "no glyph",
        ] {
            assert!(e.iter().any(|m| m.contains(needle)), "{needle}: {e:#?}");
        }
    }
}
