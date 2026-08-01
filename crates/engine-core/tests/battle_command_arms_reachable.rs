//! Disc-free guard on the **command-arm hand-off**: picking a command from
//! the live battle command menu must hand the pad to that command's own
//! surface, and the surface must actually read the pad.
//!
//! This is the shape that broke the arts-shout oracles: a wave re-pointed the
//! Arts arm from the saved-chain list to the retail per-press input session,
//! and every integration driver that pressed Cross expecting a one-press
//! commit silently stopped reaching an executed art. `--lib` was green
//! throughout - the surface's own unit tests all passed; what changed was
//! which surface the *command menu* opens and what it wants from the pad.
//!
//! So the assertions here are deliberately about the hand-off and not about
//! any one surface's internals:
//!
//! - Arts / Magic / Item each open exactly one submenu and none of the others;
//! - the command session is consumed and the action SM stays parked (nothing
//!   is armed behind the player's back, no strike lands);
//! - the Arts entry **consumes a directional press** - it is an input model,
//!   not a confirm-and-go list, and a driver has to type into it.
//!
//! A wave that re-points an arm again fails here first, with the arm named,
//! rather than as a shout count of zero three test files away.

use legaia_engine_core::input::{InputState, PadButton};
use legaia_engine_core::monster_catalog::{vanilla_formation_table, vanilla_monster_catalog};
use legaia_engine_core::world::{Actor, SceneMode, World};

fn build_world() -> World {
    let mut w = World::new();
    while w.actors.len() < 8 {
        w.actors.push(Actor::default());
    }
    w.party_count = 3;
    for i in 0..3 {
        w.actors[i].active = true;
        w.actors[i].battle.hp = 100;
        w.actors[i].battle.max_hp = 100;
        w.actors[i].battle.liveness = 1;
        w.set_battle_attack(i as u8, 90);
    }
    w.load_party(legaia_save::Party::zeroed(3));
    w.set_formation_table(vanilla_formation_table(), vanilla_monster_catalog());

    w.player_actor_slot = Some(0);
    w.actors[0].move_state.world_x = 300;
    w.actors[0].move_state.world_z = 300;
    w.actors[0].move_state.field_72 = 4096;
    w.field_camera_azimuth = 0;

    use legaia_engine_core::encounter::{
        EncounterEntry, EncounterSession, EncounterTable, EncounterTracker,
    };
    let mut table = EncounterTable::new("command_arm_test");
    table.set_trigger_rate(0xFF);
    table.push(EncounterEntry::new(1, 1));
    let mut session = EncounterSession::new(EncounterTracker::new(table));
    session.transition_frames = 2;
    session.grace_frames = 2;
    w.set_encounter_session(Some(session));

    w.mode = SceneMode::Field;
    w.live_gameplay_loop = true;
    w.battle_player_driven = true;
    w
}

/// One press frame followed by one release frame - every battle session reads
/// `just_pressed`, so a held mask is one event, not many.
fn tap(w: &mut World, button: PadButton) {
    w.set_pad(InputState::mask_of([button]));
    w.tick();
    w.set_pad(0);
    w.tick();
}

fn monster_hp_total(w: &World) -> u32 {
    (w.party_count as usize..w.actors.len())
        .map(|i| w.actors[i].battle.hp as u32)
        .sum()
}

/// Walk into the scripted encounter and leave the command menu open.
fn enter_battle(w: &mut World) {
    let up = InputState::mask_of([PadButton::Up]);
    for _ in 0..6000 {
        w.set_pad(up);
        w.tick();
        if w.mode == SceneMode::Battle {
            break;
        }
    }
    assert_eq!(w.mode, SceneMode::Battle, "walking triggers an encounter");
    assert!(w.battle_command.is_some(), "the turn opens a command menu");
}

/// Drive the open command session all the way to `want`, through whichever of
/// retail's three selection surfaces stand between it and the pad.
///
/// The flow is `Begin | Run` (the round prompt), then the four-arm ring, and
/// for the two attack modes a third `Auto | Command` prompt under the ring's
/// `Attack` arm - so `Arts` is reached *through* `Attack`, which is where
/// retail puts the directional command entry. Panics if the walk never gets
/// there: a miss means the arm was removed, not that the driver was unlucky.
fn pick_command(w: &mut World, want: legaia_engine_core::battle_input::BattleCommand) {
    use legaia_engine_core::battle_input::{AttackMode, BattleCommand, CommandPhase, RoundChoice};
    let ring_arm = match want {
        BattleCommand::Arts => BattleCommand::Attack,
        other => other,
    };
    let want_mode = match want {
        BattleCommand::Arts => AttackMode::Command,
        _ => AttackMode::Auto,
    };
    for _ in 0..40 {
        let Some(session) = w.battle_command.as_ref() else {
            return;
        };
        match session.phase {
            CommandPhase::RoundPrompt { .. } => {
                assert_eq!(
                    session.round_choice(),
                    Some(RoundChoice::Begin),
                    "the round prompt opens on Begin"
                );
                tap(w, PadButton::Cross);
            }
            CommandPhase::Menu { .. } => {
                if session.menu_command() == Some(ring_arm) {
                    tap(w, PadButton::Cross);
                } else {
                    tap(w, PadButton::Down);
                }
            }
            CommandPhase::AttackMode { .. } => {
                if session.attack_mode() == Some(want_mode) {
                    tap(w, PadButton::Cross);
                } else {
                    tap(w, PadButton::Right);
                }
            }
            // Targeting / resolved: the walk is done.
            _ => return,
        }
    }
    panic!("the command cursor never reached {want:?}");
}

/// Which submenu surfaces are open, as a labelled tuple so a failure names the
/// arm rather than printing a bare `false`.
fn open_surfaces(w: &World) -> Vec<&'static str> {
    let mut out = Vec::new();
    if w.arts_input_active() {
        out.push("arts_input");
    }
    if w.battle_arts_menu.is_some() {
        out.push("arts_menu");
    }
    if w.battle_spell_menu.is_some() {
        out.push("spell_menu");
    }
    if w.battle_item_menu.is_some() {
        out.push("item_menu");
    }
    out
}

#[test]
fn arts_command_opens_an_input_session_that_reads_directions() {
    use legaia_engine_core::battle_input::BattleCommand;

    let mut w = build_world();
    enter_battle(&mut w);
    let hp_before = monster_hp_total(&w);

    pick_command(&mut w, BattleCommand::Arts);

    assert_eq!(
        open_surfaces(&w),
        vec!["arts_input"],
        "Arts opens the per-press input session and nothing else"
    );
    assert!(
        w.battle_command.is_none(),
        "the command session hands the pad over rather than staying up"
    );
    assert_eq!(
        monster_hp_total(&w),
        hp_before,
        "no strike may land from opening a submenu"
    );

    // The surface is an input model: a direction press has to reach it and
    // cost AP. A confirm-and-go list would leave both untouched.
    let before = w.arts_input_view().expect("the input view is published");
    let (buffered, pool) = (before.buffer.len(), before.pool);
    assert_eq!(buffered, 0, "a fresh entry starts empty");
    assert!(pool > 0, "the entry seeds an AP pool");

    tap(&mut w, PadButton::Up);

    let after = w.arts_input_view().expect("the entry is still open");
    assert_eq!(after.buffer, &[4], "the Up press appended its command byte");
    assert!(
        after.pool < pool,
        "the press debits its own AP cost ({pool} -> {})",
        after.pool
    );
    assert_eq!(
        monster_hp_total(&w),
        hp_before,
        "entering a direction is not an attack"
    );
}

#[test]
fn magic_and_item_arms_each_open_their_own_surface() {
    use legaia_engine_core::battle_input::BattleCommand;

    for (command, expected) in [
        (BattleCommand::Magic, "spell_menu"),
        (BattleCommand::Item, "item_menu"),
    ] {
        let mut w = build_world();
        enter_battle(&mut w);
        let hp_before = monster_hp_total(&w);

        pick_command(&mut w, command);

        assert_eq!(
            open_surfaces(&w),
            vec![expected],
            "{command:?} opens exactly its own surface"
        );
        assert!(
            w.battle_command.is_none(),
            "{command:?} hands the pad to its submenu"
        );
        assert_eq!(
            monster_hp_total(&w),
            hp_before,
            "{command:?} may not land a strike on the way in"
        );
    }
}
