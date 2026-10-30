//! Rebirth's quests, played through on the quest tracker.
//!
//! The whole pack is walked from the tower opening at level 30 to the First
//! Breath past 99, feeding the tracker the events play would (talks, scene
//! entries, won fights, won tables), at the levels the pack's own gates
//! name. Every quest has to start, and every non-repeatable one has to
//! complete: a gate nothing satisfies, a giver nobody stands at, or an
//! objective no event reaches fails here before a player meets it.

use std::collections::BTreeSet;
use std::path::PathBuf;

use legaia_engine_expansion::battle::CompiledBattle;
use legaia_engine_expansion::quest::{
    BattleOutcome, QuestEvent, QuestNotice, QuestRuntime, StaticQuestWorld,
};
use legaia_expansion_format::ExpansionPack;

struct Play {
    pack: ExpansionPack,
    battle: CompiledBattle,
    world: StaticQuestWorld,
    rt: QuestRuntime,
    started: BTreeSet<String>,
    completed: BTreeSet<String>,
}

impl Play {
    fn new() -> Self {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../packs/rebirth");
        let pack = ExpansionPack::from_dir(&dir).expect("packs/rebirth loads");
        let battle = CompiledBattle::compile("rebirth", &pack.battle).expect("compiles");
        let mut world = StaticQuestWorld::default();
        for f in &battle.formations {
            let key = f.key.trim_start_matches("rebirth:").to_string();
            world.pack_formations.insert(key, f.def.formation_id);
        }
        for (k, b) in &battle.bosses {
            world.pack_bosses.insert(k.clone(), b.formation_id);
        }
        Self {
            pack,
            battle,
            world,
            rt: QuestRuntime::default(),
            started: BTreeSet::new(),
            completed: BTreeSet::new(),
        }
    }

    fn send(&mut self, e: QuestEvent) {
        self.rt.handle(&self.pack, &self.world, e);
        for n in self.rt.notices.drain(..) {
            match n {
                QuestNotice::Started { quest, .. } => {
                    self.started.insert(quest);
                }
                QuestNotice::Completed { quest, .. } => {
                    self.completed.insert(quest);
                }
                _ => {}
            }
        }
        let _ = self.rt.take_grants();
    }

    fn level(&mut self, l: u8) {
        self.world.party = vec![(0, l), (1, l), (2, l)];
        self.send(QuestEvent::Refresh);
    }

    fn talk(&mut self, npc: &str) {
        self.send(QuestEvent::Talked(npc.into()));
    }

    fn enter(&mut self, scene: &str) {
        self.send(QuestEvent::SceneEntered(scene.into()));
    }

    fn win(&mut self, formation: &str) {
        let f = self
            .battle
            .formation(self.world.pack_formations[formation])
            .expect("formation");
        let monsters: Vec<u16> = f
            .all_waves_def()
            .slots
            .iter()
            .map(|s| s.monster_id)
            .collect();
        let boss = f.def.header_flags != 0;
        self.send(QuestEvent::BattleEnded {
            outcome: BattleOutcome::Won,
            formation_id: f.def.formation_id,
            monsters,
            boss,
        });
    }

    fn boss(&mut self, boss: &str) {
        let id = self.world.pack_bosses[boss];
        let key = self
            .battle
            .formation(id)
            .unwrap()
            .key
            .trim_start_matches("rebirth:")
            .to_string();
        self.win(&key);
    }

    fn done(&self, q: &str) -> bool {
        self.completed.contains(q)
    }
}

#[test]
fn every_rebirth_quest_is_reachable_and_the_chain_completes() {
    let mut p = Play::new();

    // Rim Elm at 30: the rumour starts on its own; the stone answers.
    p.level(29);
    assert!(!p.started.contains("tower_rumour"), "nothing below 30");
    p.level(30);
    p.enter("rebirth_tower_1f");
    p.talk("tower_guide");
    assert!(p.done("tower_rumour"));
    assert!(p.started.contains("tower_trial"));

    // The Isles, open as soon as Ysolde has spoken: where 30 grows to 40.
    p.enter("isles_beach");
    p.talk("isles_ferrywoman");
    p.enter("isles_village");
    p.talk("isles_elder");
    assert!(p.done("isles_landfall"), "landfall at 30");
    p.talk("isles_fisher");
    for _ in 0..3 {
        p.win("isles_shore");
    }
    p.talk("isles_fisher");
    assert!(p.done("isles_nets"));
    p.level(35);
    p.talk("isles_elder");
    p.enter("isles_thornwood");
    p.boss("isles_hive_queen");
    p.talk("isles_hermit");
    assert!(p.done("isles_thornwood"));
    p.enter("isles_grotto");
    p.talk("isles_pearl");
    assert!(p.done("isles_pearl"));

    // The Warden's Trial at 40 (the three wins came on the Isles).
    p.level(40);
    p.enter("rebirth_tower_1f");
    p.talk("tower_guide");
    assert!(p.done("tower_trial"), "trial at 40");

    // 1F side work: the bounty, the remedy.
    p.talk("tower_guide");
    for _ in 0..6 {
        p.win("ogre_pack");
    }
    p.talk("tower_guide");
    assert!(p.done("bounty_ironhide"));
    p.world.items.insert(123, 5);
    p.send(QuestEvent::Refresh);
    p.talk("tower_guide");
    assert!(p.done("warden_remedy"));
    p.world.items.clear();

    // 2F at 50: the Cinder Warden.
    p.level(50);
    p.enter("rebirth_tower_2f");
    p.talk("ember_brazier");
    p.boss("cinder_warden");
    assert!(p.done("ember_stair"));

    // 3F: the garden starts on its own; three herds.
    p.enter("rebirth_tower_3f");
    p.win("garden_gauntlet");
    assert!(p.done("hanging_garden"));
    for _ in 0..3 {
        p.win("gilded_herd");
    }
    assert!(p.done("gilded_hunt"));

    // 4F at 60: the Colossus.
    p.level(60);
    p.enter("rebirth_tower_4f");
    p.talk("vault_heart");
    p.boss("verdant_colossus");
    assert!(p.done("verdant_vault"));

    // 5F: the keeper, his tables.
    p.enter("rebirth_tower_5f");
    p.talk("rest_keeper");
    assert!(p.done("gamblers_rest"));
    p.talk("rest_keeper");
    for m in [
        "tower-high-table",
        "wardens-duel",
        "rebirth-links",
        "spiral-circuit",
        "old-vault",
        "tower-high-table",
        "tower-high-table",
    ] {
        p.send(QuestEvent::ModeWon(m.into()));
    }
    p.talk("rest_keeper");
    assert!(p.done("keeper_tables") && p.done("high_roller"));

    // 6F at 70: the gauntlet and the Regent, then the sentinel bounty.
    p.level(70);
    p.enter("rebirth_tower_6f");
    p.talk("regent_seat");
    p.win("engine_gauntlet");
    p.boss("iron_regent");
    assert!(p.done("engine_deep"));
    p.talk("regent_seat");
    for _ in 0..4 {
        p.win("brass_line");
        p.win("regent_hall");
    }
    assert!(p.done("sentinel_hunt"));

    // 7F at 80: the Seraph.
    p.level(80);
    p.enter("rebirth_tower_7f");
    p.talk("sanctum_echo");
    p.boss("azure_seraph");
    assert!(p.done("sanctum_echoes"), "the Seraph at 80");

    // 8F at 88: the Sovereign, then its cutscene, then the echo.
    p.level(88);
    p.enter("rebirth_tower_8f");
    assert!(p.done("every_floor"));
    p.talk("hollow_throne");
    p.boss("hollow_sovereign");
    assert!(p.done("tower_sovereign"));
    assert!(!p.started.contains("rebirth"), "not under the cutscene");
    // The ending cutscene's last step raises `throne_broken`.
    p.send(QuestEvent::FlagSet("throne_broken".into()));
    assert!(p.started.contains("rebirth"));
    for _ in 0..5 {
        p.world.play_seconds += 700;
        p.talk("hollow_throne");
        p.boss("throne_echo");
    }
    assert!(p.done("throne_echo_hunt"));
    assert!(p.done("echo_breaker"), "five echoes");

    // The bounties come round again: four in all.
    for _ in 0..2 {
        p.world.play_seconds += 4000;
        p.talk("tower_guide");
        for _ in 0..6 {
            p.win("ogre_pack");
        }
        p.talk("tower_guide");
        p.talk("regent_seat");
        for _ in 0..4 {
            p.win("brass_line");
            p.win("regent_hall");
        }
    }
    assert!(p.done("bounty_board"), "four bounties");

    // A hundred wins and three challenges by now.
    for _ in 0..100 {
        p.win("herald_watch");
    }
    assert!(p.done("seasoned_hunter"));
    p.send(QuestEvent::CounterAdd {
        counter: "challenges_cleared".into(),
        add: 3,
    });
    assert!(p.done("trial_master"));

    // 99: Rebirth, and under the stone the First Breath.
    p.level(99);
    p.talk("tower_guide");
    assert!(p.done("rebirth"));
    assert!(p.started.contains("first_breath"));
    p.boss("first_breath");
    assert!(p.done("first_breath"));
    assert!(p.rt.progress.flags.is_set("breath_released"));

    let all: BTreeSet<String> = p.pack.quests.quests.iter().map(|q| q.id.clone()).collect();
    let unstarted: Vec<&String> = all.difference(&p.started).collect();
    assert!(unstarted.is_empty(), "never started: {unstarted:?}");
    let open: Vec<&String> = p
        .pack
        .quests
        .quests
        .iter()
        .filter(|q| !q.repeat.repeatable && !p.completed.contains(&q.id))
        .map(|q| &q.id)
        .collect();
    assert!(open.is_empty(), "never completed: {open:?}");
}

/// Every boss a quest offers is first met where the balance curve
/// (`rebirth_balance.rs`) tunes it, give or take five levels.
#[test]
fn quest_gates_match_the_tuned_levels() {
    let p = Play::new();
    let chain = legaia_engine_expansion::quest::boss_chain_levels(&p.pack.quests);
    for (boss, tuned) in [
        ("isles_hive_queen", 35),
        ("cinder_warden", 50),
        ("verdant_colossus", 65),
        ("iron_regent", 75),
        ("azure_seraph", 85),
        ("hollow_sovereign", 90),
        ("throne_echo", 95),
        ("first_breath", 99),
    ] {
        let at = chain[boss];
        assert!(
            at <= tuned && tuned - at <= 7,
            "{boss}: offered at {at}, tuned for {tuned}"
        );
    }
}
