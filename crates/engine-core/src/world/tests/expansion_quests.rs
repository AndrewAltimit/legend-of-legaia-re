//! Expansion-pack quests, challenges and pack dialogue on the `World`
//! (`world::expansion_quests`), over a synthetic pack: the hooks are inert
//! with no pack, and with one they drive the tracker from the real battle
//! end, scene entry and inline-dialogue paths.

use super::*;
use legaia_engine_battle::monster_catalog::{FormationDef, FormationSlot};
use legaia_engine_battle_vm::battle_action::BattleEndCause;
use legaia_engine_expansion::quest::{QuestEvent, QuestNotice, QuestStatus};
use legaia_engine_mode::mode::SceneMode;
use legaia_expansion_format::ExpansionPack;

fn pack() -> std::sync::Arc<ExpansionPack> {
    let mut files = legaia_expansion_format::PackFiles::new();
    files.insert(
        "pack.toml".into(),
        b"id = \"t\"\nname = \"T\"\nversion = \"1.0.0\"\n".to_vec(),
    );
    files.insert(
        "quests.toml".into(),
        br#"
[[quests]]
id = "visit"
auto_start = true
objectives = [{ kind = "enter_scene", scene = "town02" }]
rewards = { gold = 250, items = [{ item = 0x10, count = 2 }] }

[[quests]]
id = "fights"
auto_start = true
objectives = [{ kind = "defeat_formation", formation = 7, count = 2 }]
rewards = { xp = 1000000, set_flags = ["fights_done"] }

[[challenges]]
id = "bare"
formation = 7
rules = { no_items = true, no_magic = true }
"#
        .to_vec(),
    );
    files.insert(
        "dialog.toml".into(),
        br#"
[lines.ask]
text = "Climb?"
choices = [
    { label = "Yes", reply = "Go.", set_flags = ["yes"], next = "after" },
    { label = "No", set_flags = ["no"] },
]

[lines.after]
text = "Good luck."
"#
        .to_vec(),
    );
    std::sync::Arc::new(ExpansionPack::from_files(&files).expect("pack parses"))
}

fn battle_world() -> World {
    let mut world = World {
        party: legaia_engine_menus::party_state::PartyState {
            party_count: 1,
            ..Default::default()
        },
        ..World::default()
    };
    world.actors[0].battle.hp = 100;
    world
}

fn fight(world: &mut World, cause: BattleEndCause) {
    let formation = FormationDef::new(7, vec![FormationSlot::new(1)]);
    world.field_return = Some(FieldReturnState {
        actors: world.actors.clone(),
        player_actor_slot: world.player_actor_slot,
        party_count: world.party.party_count,
    });
    world.battle.return_mode = SceneMode::Field;
    world.enter_battle_from_formation(&formation);
    // The encounter paths seat the formation after entry (`field_loop`).
    world.battle.active_formation = Some(formation);
    world.battle.end = Some(cause);
    world.begin_battle_end_sequence();
    let mut frames = 0;
    while world.battle.victory.is_some() && frames < 2000 {
        world.tick_battle_end_sequence();
        frames += 1;
    }
    assert!(world.battle.victory.is_none(), "end sequence never exited");
}

#[test]
fn every_hook_is_inert_with_no_pack() {
    let mut w = battle_world();
    w.set_active_scene_label("town02");
    w.expansion_notify(QuestEvent::Talked("x".into()));
    w.expansion_talked("x");
    assert!(!w.expansion_start_quest("visit"));
    assert!(w.expansion_say("ask").is_err());
    assert!(w.dialog.inline.is_none());
    assert!(w.expansion_item_use_allowed() && w.expansion_magic_allowed());
    fight(&mut w, BattleEndCause::MonsterWipe);
    w.tick();
    assert!(w.take_expansion_notices().is_empty());
    assert!(w.expansion.quests.progress.pack_id.is_empty());
    assert_eq!(w.expansion.quests.save_block(None), None);
}

#[test]
fn scene_entry_completes_an_enter_scene_quest_and_grants_its_reward() {
    let mut w = battle_world();
    w.expansion.pack = Some(pack());
    w.party.money = 100;
    w.set_active_scene_label("town01");
    assert_eq!(
        w.expansion.quests.quest("visit").unwrap().status,
        QuestStatus::Active
    );
    w.set_active_scene_label("town02");
    assert_eq!(
        w.expansion.quests.quest("visit").unwrap().status,
        QuestStatus::Completed
    );
    assert_eq!(w.party.money, 350);
    assert_eq!(w.party.inventory.get(&0x10).copied(), Some(2));
    let notices = w.take_expansion_notices();
    assert!(notices.contains(&QuestNotice::Completed {
        quest: "visit".into(),
        dialog: None
    }));
}

#[test]
fn won_battles_count_through_finish_battle_and_a_loss_does_not() {
    let mut w = battle_world();
    w.expansion.pack = Some(pack());
    w.expansion_notify(QuestEvent::Refresh);
    fight(&mut w, BattleEndCause::MonsterWipe);
    assert_eq!(
        w.expansion.quests.quest("fights").unwrap().objectives,
        vec![1]
    );
    w.actors[0].battle.hp = 100;
    fight(&mut w, BattleEndCause::PartyWipe);
    w.resolve_game_over_hold();
    assert_eq!(
        w.expansion.quests.quest("fights").unwrap().objectives,
        vec![1],
        "a loss is not a win"
    );
    w.actors[0].battle.hp = 100;
    let level_before = w.party.level_up_tracker.level[0];
    fight(&mut w, BattleEndCause::MonsterWipe);
    assert_eq!(
        w.expansion.quests.quest("fights").unwrap().status,
        QuestStatus::Completed
    );
    assert!(w.expansion.quests.progress.flags.is_set("fights_done"));
    assert!(
        w.party.level_up_tracker.level[0] > level_before,
        "the quest's EXP reaches the party"
    );
}

#[test]
fn a_challenge_refuses_items_and_magic_only_in_its_battle() {
    let mut w = battle_world();
    w.expansion.pack = Some(pack());
    w.expansion_arm_challenge("bare").unwrap();
    assert!(
        w.expansion_item_use_allowed(),
        "no battle is seated: no rule applies"
    );
    w.battle.active_formation = Some(FormationDef::new(8, vec![FormationSlot::new(1)]));
    assert!(w.expansion_item_use_allowed(), "another formation");
    w.battle.active_formation = Some(FormationDef::new(7, vec![FormationSlot::new(1)]));
    assert!(!w.expansion_item_use_allowed());
    assert!(!w.expansion_magic_allowed());
    w.battle.active_formation = None;
    fight(&mut w, BattleEndCause::MonsterWipe);
    assert!(w.take_expansion_notices().iter().any(
        |n| matches!(n, QuestNotice::ChallengeCleared { challenge, .. } if challenge == "bare")
    ));
    assert!(w.expansion.quests.challenge.is_none());
}

/// Drive the inline runner with a confirm on every other tick, choosing
/// option `pick` when the menu takes input, until the talk ends.
fn run_line(w: &mut World, pick: usize) {
    let mut frames = 0;
    let mut moved = false;
    while w.dialog.inline.as_ref().is_some_and(|id| !id.done) && frames < 4000 {
        let menu = w
            .dialog
            .inline
            .as_ref()
            .and_then(|id| id.panel.as_ref())
            .is_some_and(|p| p.menu_active() && p.picker_takes_input());
        let down = menu && !moved && pick > 0;
        if down {
            moved = true;
        }
        w.step_inline_dialogue(frames % 2 == 0 && !down, false, down);
        frames += 1;
    }
    assert!(frames < 4000, "the line never finished");
}

#[test]
fn a_pack_line_runs_on_the_inline_runner_and_applies_its_choice() {
    let mut w = battle_world();
    w.expansion.pack = Some(pack());
    w.expansion_say("ask").unwrap();
    assert!(w.dialog.inline.is_some());
    run_line(&mut w, 1);
    assert_eq!(w.dialog.inline.as_ref().unwrap().last_choice, Some(1));
    w.tick();
    assert!(w.expansion.quests.progress.flags.is_set("no"));
    assert!(!w.expansion.quests.progress.flags.is_set("yes"));
    assert!(w.expansion.quests.dialog.pending.is_none());
}

#[test]
fn a_choice_chains_its_next_line() {
    let mut w = battle_world();
    w.expansion.pack = Some(pack());
    w.expansion_say("ask").unwrap();
    run_line(&mut w, 0);
    // The host drops the finished talk; the next tick resolves the choice
    // and starts the chained line.
    w.dialog.inline = None;
    w.tick();
    assert!(w.expansion.quests.progress.flags.is_set("yes"));
    assert_eq!(
        w.expansion
            .quests
            .dialog
            .pending
            .as_ref()
            .map(|p| p.line.as_str()),
        Some("after")
    );
    assert!(w.dialog.inline.is_some());
}

#[test]
fn an_unknown_line_is_an_error() {
    let mut w = battle_world();
    w.expansion.pack = Some(pack());
    assert!(w.expansion_say("missing").is_err());
}
