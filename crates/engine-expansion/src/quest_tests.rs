//! Disc-free tests of the quest tracker over synthetic packs.

use super::*;
use legaia_expansion_format::quest::{ItemReward, ObjectiveDef, Prerequisites, Repeat};

fn pack(quests: &str) -> ExpansionPack {
    let mut p = ExpansionPack::default();
    p.manifest.id = "t".into();
    p.manifest.version = "1.0.0".into();
    p.quests = toml::from_str(quests).expect("quests parse");
    p
}

fn world(level: u8) -> StaticQuestWorld {
    StaticQuestWorld {
        party: vec![(0, level), (1, level.saturating_sub(2))],
        ..Default::default()
    }
}

fn won(formation_id: u16, monsters: &[u16], boss: bool) -> QuestEvent {
    QuestEvent::BattleEnded {
        outcome: BattleOutcome::Won,
        formation_id,
        monsters: monsters.to_vec(),
        boss,
    }
}

fn talked(npc: &str) -> QuestEvent {
    QuestEvent::Talked(npc.into())
}

fn started(rt: &QuestRuntime, q: &str) -> bool {
    rt.notices
        .iter()
        .any(|n| matches!(n, QuestNotice::Started { quest, .. } if quest == q))
}

fn completed(rt: &QuestRuntime, q: &str) -> bool {
    rt.notices
        .iter()
        .any(|n| matches!(n, QuestNotice::Completed { quest, .. } if quest == q))
}

const TOWER: &str = r#"
[[quests]]
id = "tower"
giver = "guide"
start_dialog = "offer"
complete_dialog = "done"
objectives = [
    { kind = "talk", npc = "guide" },
    { kind = "win_battles", count = 3 },
    { kind = "reach_level", level = 40 },
]
rewards = { gold = 500, xp = 100, items = [{ item = 0x10, count = 2 }], cards = ["tower"], set_flags = ["tower_done"] }
"#;

#[test]
fn the_giver_talk_starts_the_quest_without_counting_toward_it() {
    let p = pack(TOWER);
    let mut rt = QuestRuntime::default();
    rt.handle(&p, &world(10), talked("guide"));
    assert!(rt.notices.contains(&QuestNotice::Started {
        quest: "tower".into(),
        dialog: Some("offer".into())
    }));
    assert_eq!(rt.quest("tower").unwrap().objectives, vec![0, 0, 0]);
    rt.handle(&p, &world(10), talked("guide"));
    assert_eq!(rt.quest("tower").unwrap().objectives[0], 1);
}

#[test]
fn an_ordered_quest_counts_only_its_current_objective() {
    let p = pack(TOWER);
    let mut rt = QuestRuntime::default();
    let w = world(45);
    rt.handle(&p, &w, talked("guide"));
    // The level is already 45, but objective 2 is not current yet.
    assert_eq!(rt.quest("tower").unwrap().objectives, vec![0, 0, 0]);
    rt.handle(&p, &w, won(1, &[], false));
    assert_eq!(
        rt.quest("tower").unwrap().objectives,
        vec![0, 0, 0],
        "a win before the talk is not counted"
    );
    rt.handle(&p, &w, talked("guide"));
    for _ in 0..3 {
        rt.handle(&p, &w, won(1, &[], false));
    }
    // The third win completes objective 1; the level objective re-tests at
    // once and the quest completes on the same event.
    assert_eq!(rt.quest("tower").unwrap().status, QuestStatus::Completed);
    assert!(rt.notices.contains(&QuestNotice::Completed {
        quest: "tower".into(),
        dialog: Some("done".into())
    }));
    assert_eq!(
        rt.take_grants(),
        vec![
            Grant::Gold(500),
            Grant::Xp(100),
            Grant::Item {
                item: 0x10,
                count: 2
            },
            Grant::Card("tower".into()),
        ]
    );
    assert!(rt.progress.flags.is_set("tower_done"));
}

#[test]
fn one_event_counts_toward_one_ordered_objective() {
    let p = pack(
        r#"
        [[quests]]
        id = "q"
        auto_start = true
        objectives = [{ kind = "win_battles" }, { kind = "win_battles" }]
        "#,
    );
    let mut rt = QuestRuntime::default();
    rt.handle(&p, &world(1), QuestEvent::Refresh);
    rt.handle(&p, &world(1), won(1, &[], false));
    assert_eq!(rt.quest("q").unwrap().objectives, vec![1, 0]);
    rt.handle(&p, &world(1), won(1, &[], false));
    assert_eq!(rt.quest("q").unwrap().status, QuestStatus::Completed);
}

#[test]
fn an_unordered_quest_counts_every_objective_at_once() {
    let p = pack(
        r#"
        [[quests]]
        id = "q"
        auto_start = true
        ordered = false
        objectives = [
            { kind = "defeat_formation", formation = 5, count = 2 },
            { kind = "defeat_monster", monster = 9 },
            { kind = "win_boss" },
            { kind = "enter_scene", scene = "town01" },
        ]
        "#,
    );
    let mut rt = QuestRuntime::default();
    let w = world(1);
    rt.handle(&p, &w, QuestEvent::Refresh);
    rt.handle(&p, &w, won(5, &[9, 9], false));
    assert_eq!(rt.quest("q").unwrap().objectives, vec![1, 1, 0, 0]);
    rt.handle(&p, &w, won(6, &[3], true));
    assert_eq!(rt.quest("q").unwrap().objectives, vec![1, 1, 1, 0]);
    rt.handle(&p, &w, QuestEvent::SceneEntered("town02".into()));
    rt.handle(&p, &w, QuestEvent::SceneEntered("town01".into()));
    assert_eq!(rt.quest("q").unwrap().objectives, vec![1, 1, 1, 1]);
    assert_eq!(rt.quest("q").unwrap().status, QuestStatus::Active);
    rt.handle(&p, &w, won(5, &[], false));
    assert_eq!(rt.quest("q").unwrap().status, QuestStatus::Completed);
    let updates = rt
        .notices
        .iter()
        .filter(|n| matches!(n, QuestNotice::Updated { .. }))
        .count();
    assert_eq!(updates, 5, "one notice per objective step");
}

#[test]
fn a_loss_counts_for_nothing() {
    let p = pack(
        "[[quests]]\nid = \"q\"\nauto_start = true\nobjectives = [{ kind = \"win_battles\" }]\n",
    );
    let mut rt = QuestRuntime::default();
    let lost = QuestEvent::BattleEnded {
        outcome: BattleOutcome::Lost,
        formation_id: 1,
        monsters: vec![],
        boss: false,
    };
    rt.handle(&p, &world(1), lost);
    assert_eq!(rt.quest("q").unwrap().objectives, vec![0]);
}

#[test]
fn prerequisites_gate_the_start() {
    let p = pack(
        r#"
        [[quests]]
        id = "first"
        auto_start = true
        objectives = [{ kind = "flag", flag = "go" }]

        [[quests]]
        id = "second"
        giver = "npc"
        prerequisites = { quests = ["first"], story_flags = [0x142], story_flags_clear = [7], min_level = 20 }
        "#,
    );
    let mut rt = QuestRuntime::default();
    let mut w = world(25);
    rt.handle(&p, &w, talked("npc"));
    assert!(!started(&rt, "second"), "first is not complete");
    rt.handle(&p, &w, QuestEvent::FlagSet("go".into()));
    assert!(completed(&rt, "first"));
    rt.handle(&p, &w, talked("npc"));
    assert!(!started(&rt, "second"), "story flag 0x142 is clear");
    w.story_flags = vec![0x142, 7];
    rt.handle(&p, &w, talked("npc"));
    assert!(!started(&rt, "second"), "story flag 7 must be clear");
    w.story_flags = vec![0x142];
    rt.handle(&p, &world(19), talked("npc"));
    assert!(!started(&rt, "second"), "level 19 is under the minimum");
    rt.handle(&p, &w, talked("npc"));
    assert!(started(&rt, "second"));
    // No objectives: it completes on the start.
    assert_eq!(rt.quest("second").unwrap().status, QuestStatus::Completed);
}

#[test]
fn a_completion_flag_auto_starts_the_next_quest_of_a_chain() {
    let p = pack(
        r#"
        [[quests]]
        id = "a"
        auto_start = true
        objectives = [{ kind = "talk", npc = "x" }]
        rewards = { set_flags = ["a_done"] }

        [[quests]]
        id = "b"
        auto_start = true
        prerequisites = { flags = ["a_done"] }
        objectives = [{ kind = "counter", counter = "k", at_least = 2 }]
        "#,
    );
    let mut rt = QuestRuntime::default();
    let w = world(1);
    rt.handle(&p, &w, QuestEvent::Refresh);
    assert!(started(&rt, "a") && !started(&rt, "b"));
    rt.handle(&p, &w, talked("x"));
    assert!(completed(&rt, "a") && started(&rt, "b"));
    rt.handle(
        &p,
        &w,
        QuestEvent::CounterAdd {
            counter: "k".into(),
            add: 2,
        },
    );
    assert!(completed(&rt, "b"));
}

#[test]
fn a_hand_in_takes_the_items_on_completion() {
    let p = pack(
        r#"
        [[quests]]
        id = "q"
        auto_start = true
        objectives = [{ kind = "collect_item", item = 0x20, count = 3, hand_in = true }]
        "#,
    );
    let mut rt = QuestRuntime::default();
    let mut w = world(1);
    w.items.insert(0x20, 1);
    rt.handle(&p, &w, QuestEvent::Refresh);
    assert_eq!(rt.quest("q").unwrap().objectives, vec![1]);
    w.items.insert(0x20, 0);
    rt.handle(&p, &w, QuestEvent::Refresh);
    assert_eq!(
        rt.quest("q").unwrap().objectives,
        vec![0],
        "a collect objective tracks the bag, it does not latch"
    );
    w.items.insert(0x20, 5);
    rt.handle(&p, &w, QuestEvent::Refresh);
    assert_eq!(rt.quest("q").unwrap().status, QuestStatus::Completed);
    assert_eq!(
        rt.take_grants(),
        vec![Grant::TakeItem {
            item: 0x20,
            count: 3
        }]
    );
}

#[test]
fn a_repeatable_quest_waits_out_its_cooldown() {
    let p = pack(
        r#"
        [[quests]]
        id = "daily"
        giver = "npc"
        objectives = [{ kind = "win_mode", mode = "poker" }]
        rewards = { gold = 10 }
        repeat = { repeatable = true, cooldown_seconds = 100, max_completions = 2 }
        "#,
    );
    let mut rt = QuestRuntime::default();
    let mut w = world(1);
    w.play_seconds = 1000;
    rt.handle(&p, &w, talked("npc"));
    rt.handle(&p, &w, QuestEvent::ModeWon("poker".into()));
    assert_eq!(rt.quest("daily").unwrap().completions, 1);
    rt.notices.clear();
    w.play_seconds = 1099;
    rt.handle(&p, &w, talked("npc"));
    assert!(!started(&rt, "daily"), "inside the cooldown");
    w.play_seconds = 1100;
    rt.handle(&p, &w, talked("npc"));
    assert!(started(&rt, "daily"));
    assert_eq!(
        rt.quest("daily").unwrap().objectives,
        vec![0],
        "a restart resets"
    );
    rt.handle(&p, &w, QuestEvent::ModeWon("poker".into()));
    assert_eq!(rt.quest("daily").unwrap().completions, 2);
    rt.notices.clear();
    w.play_seconds = 5000;
    rt.handle(&p, &w, talked("npc"));
    assert!(!started(&rt, "daily"), "the completion cap holds");
}

#[test]
fn a_one_shot_quest_does_not_restart() {
    let p = pack(TOWER);
    let mut rt = QuestRuntime::default();
    let w = world(50);
    rt.handle(&p, &w, talked("guide"));
    rt.handle(&p, &w, talked("guide"));
    for _ in 0..3 {
        rt.handle(&p, &w, won(1, &[], false));
    }
    assert!(completed(&rt, "tower"));
    rt.notices.clear();
    rt.handle(&p, &w, talked("guide"));
    assert!(rt.notices.is_empty());
}

#[test]
fn a_script_can_start_and_abandon_a_quest() {
    let p = pack(TOWER);
    let mut rt = QuestRuntime::default();
    assert!(!rt.start_quest(&p, &world(1), "missing"));
    assert!(rt.start_quest(&p, &world(1), "tower"));
    assert!(!rt.start_quest(&p, &world(1), "tower"), "already active");
    assert!(rt.abandon_quest("tower"));
    assert!(rt.quest("tower").is_none());
    assert!(!rt.abandon_quest("tower"));
}

#[test]
fn the_same_events_give_the_same_queues() {
    let p = pack(TOWER);
    let run = || {
        let mut rt = QuestRuntime::default();
        let w = world(45);
        for e in [
            talked("guide"),
            talked("guide"),
            won(1, &[2], false),
            won(1, &[2], false),
            won(1, &[2], false),
        ] {
            rt.handle(&p, &w, e);
        }
        (rt.take_notices(), rt.take_grants(), rt.progress.encode())
    };
    assert_eq!(run(), run());
}

#[test]
fn progress_survives_a_save_block_round_trip() {
    let p = pack(TOWER);
    let mut rt = QuestRuntime::default();
    rt.handle(&p, &world(10), talked("guide"));
    rt.handle(&p, &world(10), talked("guide"));
    rt.handle(&p, &world(10), won(1, &[], false));
    let block = rt.save_block(Some(&p)).unwrap();
    let mut back = QuestRuntime::default();
    back.load_block(Some(&p), Some(&block));
    assert_eq!(back.progress, rt.progress);
    assert!(back.dormant.is_none());
}

#[test]
fn no_pack_and_no_block_saves_nothing() {
    let mut rt = QuestRuntime::default();
    assert_eq!(rt.save_block(None), None);
    rt.load_block(None, None);
    assert_eq!(rt.save_block(None), None);
}

#[test]
fn another_packs_block_stays_dormant_and_is_saved_back() {
    let p = pack(TOWER);
    let mut rt = QuestRuntime::default();
    rt.handle(&p, &world(10), talked("guide"));
    let block = rt.save_block(Some(&p)).unwrap();

    // Loaded with no pack: kept verbatim, saved back verbatim.
    let mut none = QuestRuntime::default();
    none.load_block(None, Some(&block));
    assert_eq!(none.save_block(None), Some(block.clone()));

    // Loaded under another pack: that pack starts fresh, the block waits.
    let mut other = pack("");
    other.manifest.id = "other".into();
    let mut rt2 = QuestRuntime::default();
    rt2.load_block(Some(&other), Some(&block));
    assert!(rt2.progress.quests.is_empty());
    assert_eq!(rt2.progress.pack_id, "other");
    assert_eq!(rt2.dormant.as_deref(), Some(&block[..]));

    // Activating the block's own pack later adopts it.
    let mut late = QuestRuntime::default();
    late.load_block(None, Some(&block));
    late.sync_pack(&p);
    assert!(late.quest("tower").is_some());
    assert!(late.dormant.is_none());
}

#[test]
fn the_journal_lists_started_quests_with_progress() {
    let p = pack(TOWER);
    let mut rt = QuestRuntime::default();
    assert!(rt.journal(&p.quests).is_empty());
    rt.handle(&p, &world(10), talked("guide"));
    rt.handle(&p, &world(10), talked("guide"));
    rt.handle(&p, &world(10), won(1, &[], false));
    let j = rt.journal(&p.quests);
    assert_eq!(j.len(), 1);
    assert_eq!(j[0].status, QuestStatus::Active);
    let prog: Vec<(u32, u32)> = j[0]
        .objectives
        .iter()
        .map(|o| (o.progress, o.target))
        .collect();
    assert_eq!(prog, vec![(1, 1), (1, 3), (0, 1)]);
}

#[test]
fn rewards_from_code_built_defs_match_toml() {
    // The struct form a tool builds is the same quest the TOML names.
    let mut p = pack("");
    p.quests.quests.push(QuestDef {
        id: "q".into(),
        auto_start: true,
        ordered: true,
        prerequisites: Prerequisites::default(),
        objectives: vec![ObjectiveDef {
            text: None,
            goal: Goal::ReachLevel { level: 5 },
        }],
        rewards: Rewards {
            items: vec![ItemReward { item: 1, count: 1 }],
            ..Default::default()
        },
        repeat: Repeat::default(),
        ..Default::default()
    });
    let mut rt = QuestRuntime::default();
    rt.handle(&p, &world(5), QuestEvent::Refresh);
    assert_eq!(rt.take_grants(), vec![Grant::Item { item: 1, count: 1 }]);
}

#[test]
fn a_pack_formation_resolves_through_the_world() {
    let p = pack(
        r#"
        [[quests]]
        id = "q"
        auto_start = true
        ordered = false
        objectives = [
            { kind = "defeat_formation", formation = "ogres" },
            { kind = "win_boss", formation = "sovereign" },
        ]
        "#,
    );
    let mut rt = QuestRuntime::default();
    let mut w = world(1);
    rt.handle(&p, &w, won(0xE000, &[], false));
    assert_eq!(
        rt.quest("q").unwrap().objectives,
        vec![0, 0],
        "an unresolved pack formation matches nothing"
    );
    w.pack_formations.insert("ogres".into(), 0xE000);
    w.pack_formations.insert("sovereign".into(), 0xE002);
    rt.handle(&p, &w, won(0xE000, &[], false));
    rt.handle(&p, &w, won(0xE001, &[], true));
    assert_eq!(rt.quest("q").unwrap().objectives, vec![1, 0]);
    rt.handle(&p, &w, won(0xE002, &[], true));
    assert_eq!(rt.quest("q").unwrap().status, QuestStatus::Completed);
}

/// The Rebirth pack's quest chain parses, every line it names compiles, and
/// the chain plays through on the events it waits for.
#[test]
fn the_rebirth_tower_chain_plays_through() {
    let mut files = legaia_expansion_format::PackFiles::new();
    files.insert(
        "pack.toml".into(),
        b"id = \"rebirth\"\nname = \"Rebirth\"\nversion = \"0.1.0\"\n".to_vec(),
    );
    files.insert(
        "quests.toml".into(),
        include_bytes!("../../../packs/rebirth/quests.toml").to_vec(),
    );
    files.insert(
        "dialog.toml".into(),
        include_bytes!("../../../packs/rebirth/dialog.toml").to_vec(),
    );
    let p = ExpansionPack::from_files(&files).expect("rebirth parses");
    for (id, line) in &p.dialog.lines {
        crate::dialog::compile_line(line).unwrap_or_else(|e| panic!("{id}: {e:?}"));
    }
    for q in &p.quests.quests {
        for l in q.start_dialog.iter().chain(&q.complete_dialog) {
            assert!(p.dialog.lines.contains_key(l), "{}: line {l} missing", q.id);
        }
    }

    let mut rt = QuestRuntime::default();
    rt.handle(&p, &world(29), QuestEvent::Refresh);
    assert!(rt.quest("tower_rumour").is_none(), "level 29 is too low");
    let mut w = world(30);
    rt.handle(&p, &w, QuestEvent::Refresh);
    assert!(started(&rt, "tower_rumour"));
    rt.handle(&p, &w, talked("tower_guide"));
    assert!(completed(&rt, "tower_rumour") && started(&rt, "tower_trial"));
    for _ in 0..3 {
        rt.handle(&p, &w, won(1, &[], false));
    }
    rt.handle(&p, &w, talked("tower_guide"));
    assert!(!completed(&rt, "tower_trial"), "level 40 not reached");
    w.party = vec![(0, 40)];
    rt.handle(&p, &w, QuestEvent::Refresh);
    rt.handle(&p, &w, talked("tower_guide"));
    assert!(completed(&rt, "tower_trial"));
    assert!(rt.progress.flags.is_set("tower_open"));
    assert!(rt.take_grants().contains(&Grant::Gold(10000)));

    rt.arm_challenge(&p, &w, "bare_hands").unwrap();
    assert!(!rt.item_use_allowed(Some(1)) && !rt.magic_allowed(Some(1)));
}
