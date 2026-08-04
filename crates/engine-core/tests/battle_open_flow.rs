//! The battle **open flow** is three prompts and one banner, and the flow byte
//! is what sequences them.
//!
//! Retail's dispatcher `FUN_801D0748` walks `ctx[+0x06]` through
//! `0x0A -> 0x0B -> 0x14 -> 0x1E -> 0x28`, and the round-open prompt at `0x1E`
//! sits between the round starting and any member entering a command. Two
//! things follow that the port used to get wrong, and both are asserted here:
//!
//! 1. the prompt is a property of the **round**, so a second party member of
//!    the same round goes straight to the command ring;
//! 2. an **ambush** (`ctx[+0x290] == 1`) skips the whole selection band -
//!    retail's `0x0B` jumps to `0xFE` - so the party enters no command at all
//!    that round, and the screen says so.
//!
//! A third thing, which is what makes `Run` reachable at all: the prompt is
//! the phase the session **opens in**, not one it is swapped onto a frame
//! later. Retail's `0x14` arm stores `ctx[+0x06] = 0x1E` before anything is on
//! screen (`0x801D0ED4`) and the ring `0x28` is only entered from `0x1E`
//! (`0x801D108C`), so no observer ever sees the ring first. A session built on
//! the ring and rewritten next tick reads as "the prompt never happens" to
//! anything that looks on the frame the session appears - and Run lives only
//! on that prompt.
//!
//! These are disc-free: `World::default()` + `enter_battle` is enough, because
//! the flow byte and the box queue are engine state.

use legaia_engine_core::battle_flow::BattleFlowState;
use legaia_engine_core::battle_input::{
    BattleCommand, BattleCommandSession, CommandPhase, RoundChoice,
};
use legaia_engine_core::battle_open::{BANNER_FRAMES, FormationBanner};
use legaia_engine_core::input::{InputState, PadButton};
use legaia_engine_core::world::World;
use legaia_engine_vm::battle_formulas::FormationAdvantage;

/// A 3-party / 2-monster battle with the player driving.
fn player_driven_battle() -> World {
    let mut world = World {
        battle_player_driven: true,
        ..Default::default()
    };
    world.enter_battle(3, 2);
    world
}

/// One press frame plus one release frame - every battle surface reads
/// `just_pressed`, so a held mask is one event.
fn tap(world: &mut World, button: PadButton) {
    world.set_pad(InputState::mask_of([button]));
    world.tick();
    world.set_pad(0);
    world.tick();
}

/// Tick with a neutral pad until `f` holds. Returns whether it did.
fn settle_until(world: &mut World, ticks: usize, f: impl Fn(&World) -> bool) -> bool {
    for _ in 0..ticks {
        if f(world) {
            return true;
        }
        world.set_pad(0);
        world.tick();
    }
    f(world)
}

/// The round's **first** party command opens on the prompt; a session opened
/// after it (a later member, or a submenu backed out of) opens on the ring.
#[test]
fn the_prompt_is_per_round_not_per_turn() {
    let mut world = player_driven_battle();

    // Round open: the flow byte parks at TurnPrompt, which is what arms the
    // prompt on the next live tick.
    world.battle_flow = BattleFlowState::TurnPrompt;
    world.battle_command = Some(BattleCommandSession::new(0, 0));
    world.tick();
    let session = world.battle_command.as_ref().expect("session still open");
    assert!(
        matches!(session.phase, CommandPhase::RoundPrompt { .. }),
        "the round's first party command should open on Begin | Run, got {:?}",
        session.phase
    );
    assert_eq!(session.round_choice(), Some(RoundChoice::Begin));

    // Take Begin: the flow leaves TurnPrompt and the ring is up.
    world.battle_command.as_mut().unwrap().phase = CommandPhase::Menu { cursor: 0 };
    world.battle_flow = BattleFlowState::CategoryMenu;

    // A second member's session in the same round is left on the ring.
    world.battle_command = Some(BattleCommandSession::new(1, 1));
    world.tick();
    let session = world.battle_command.as_ref().expect("session still open");
    assert!(
        matches!(session.phase, CommandPhase::Menu { .. }),
        "a later member of the same round must not re-ask Begin | Run, got {:?}",
        session.phase
    );
}

/// The ring is retail's four arms, and the two commands that are not on it are
/// reachable through the prompt above it and the prompt below it. A regression
/// here is the port re-growing an invented row.
#[test]
fn the_ring_carries_exactly_the_four_retail_arms() {
    assert_eq!(BattleCommand::MENU.len(), 4);
    assert_eq!(
        BattleCommand::MENU,
        [
            BattleCommand::Item,
            BattleCommand::Attack,
            BattleCommand::Magic,
            BattleCommand::Spirit,
        ]
    );
    // Run lives on the round prompt.
    assert_eq!(RoundChoice::PROMPT[1], RoundChoice::Run);
    // Arts lives under the Attack arm, via the attack-mode prompt.
    let mut s = BattleCommandSession::new(0, 0);
    s.phase = CommandPhase::Menu { cursor: 1 };
    assert_eq!(s.menu_command(), Some(BattleCommand::Attack));
}

/// An ambush raises the banner and costs the party the round: every party
/// initiative key is zeroed, so the live loop opens no command session at all.
#[test]
fn an_ambush_announces_itself_and_costs_the_party_the_round() {
    // Real SPD on both sides so the initiative path (and therefore the side
    // lockout) is live rather than the round-robin fallback.
    let mut world = player_driven_battle();
    for slot in 0..5 {
        world.battle_speed[slot] = 20;
    }
    world.set_battle_formation(FormationAdvantage::BackAttack);
    world.seed_battle_initiative();
    world.raise_battle_open_banner();

    // The screen says so - one self-dismissing box, holding retail's longer
    // advantage-case intro timer.
    let banner = world
        .battle_tutorial_box()
        .expect("an ambush must put a banner up");
    assert!(!banner.text.trim().is_empty());
    assert!(!banner.waits_for_input);
    assert_eq!(banner.frames_remaining, BANNER_FRAMES);

    // ... and no party member holds a turn this round.
    for slot in 0..3 {
        assert_eq!(
            world.actors[slot].battle.init_key, 0,
            "party slot {slot} kept a turn through an ambush"
        );
    }
    assert!(
        (3..5).any(|slot| world.actors[slot].battle.init_key != 0),
        "the monsters must still hold theirs - otherwise nothing acts"
    );
}

/// A pre-emptive strike is the mirror image: the banner goes up, the party
/// keeps its round, and the monsters lose theirs.
#[test]
fn a_preemptive_strike_announces_itself_and_costs_the_monsters_the_round() {
    let mut world = World::default();
    world.enter_battle(3, 2);
    for slot in 0..5 {
        world.battle_speed[slot] = 20;
    }
    world.set_battle_formation(FormationAdvantage::Preemptive);
    world.seed_battle_initiative();
    world.raise_battle_open_banner();

    assert!(world.battle_tutorial_box().is_some());
    for slot in 3..5 {
        assert_eq!(world.actors[slot].battle.init_key, 0);
    }
    assert!((0..3).any(|slot| world.actors[slot].battle.init_key != 0));
}

/// An ordinary formation raises nothing - retail's own skip, and the check
/// that keeps the two tests above non-vacuous.
#[test]
fn an_ordinary_formation_raises_no_banner() {
    let mut world = World::default();
    world.enter_battle(3, 2);
    world.set_battle_formation(FormationAdvantage::None);
    world.raise_battle_open_banner();
    assert!(world.battle_tutorial_box().is_none());
    assert_eq!(
        FormationBanner::for_formation(FormationAdvantage::None, 3),
        None
    );
}

/// The round prompt is up **on the frame the session appears**, driven by
/// `World::tick` alone.
///
/// This is the assertion the sibling above cannot make: it hands the session
/// in on the ring and lets the next tick rewrite it, which passes whether the
/// prompt is the opening phase or a one-frame-late correction. The difference
/// is not cosmetic - `battle_command.is_some()` is the only edge an observer
/// has, so a prompt that arrives one tick behind it is a prompt nothing ever
/// sees, and `Run` lives on that prompt and nowhere else.
///
/// Retail has no such window: `0x14` stores `0x1E` at `0x801D0ED4`, before the
/// selector is drawn, and the ring `0x28` is entered only from `0x1E`'s
/// confirm arm at `0x801D108C`.
#[test]
fn the_round_prompt_is_up_on_the_frame_the_session_opens() {
    let mut world = player_driven_battle();
    for slot in 0..5 {
        world.actors[slot].battle.max_hp = 4000;
        world.actors[slot].battle.hp = 4000;
    }

    assert!(
        settle_until(&mut world, 600, |w| w.battle_command.is_some()),
        "a player-driven battle never handed the pad a command session"
    );
    let session = world.battle_command.as_ref().expect("session open");
    assert!(
        matches!(session.phase, CommandPhase::RoundPrompt { .. }),
        "the first frame a session exists must already be the round prompt, got {:?}",
        session.phase
    );
    assert_eq!(session.round_choice(), Some(RoundChoice::Begin));
    assert_eq!(world.battle_flow, BattleFlowState::TurnPrompt);
    // Run is on this prompt and on no other surface, which is why a frame that
    // skips it costs the player the command entirely.
    assert_eq!(RoundChoice::PROMPT[1], RoundChoice::Run);

    // Begin walks it to the ring - retail's `0x1E -> 0x28`.
    tap(&mut world, PadButton::Cross);
    let session = world.battle_command.as_ref().expect("session still open");
    assert!(
        matches!(session.phase, CommandPhase::Menu { .. }),
        "Begin must drop into the four-arm ring, got {:?}",
        session.phase
    );
}

/// A **battle** item use has to seed the HP readout, and the fight has to keep
/// running afterwards.
///
/// `World::use_item` writes live HP and stops there, which is complete out of
/// battle. Retail's in-battle applier `FUN_800402F4` also assigns the readout's
/// pending accumulator `-delta` (`0x800408FC` / `0x80040D28` / `0x800410BC`).
/// Skipping that leaves `hp != hp_display` with a **zero** accumulator, and the
/// ramp's only guard is `+0x10 != 0` (`0x800474E8`) - so nothing ever moves the
/// bar again. The action SM's `0x51` exit waits on exactly that bar for any
/// party-targeted action (`FUN_801E7250`), so the next monster swing at the
/// healed member parks the battle with no in-battle exit at all: not even
/// winning it, because the turn pump that would notice a KO is what stopped.
///
/// Pad-driven end to end, so it fails if the seed is dropped anywhere between
/// the ring and the applier.
#[test]
fn a_battle_item_heal_keeps_the_readout_and_the_turn_pump_alive() {
    use legaia_engine_core::items::ItemCatalog;

    /// Healing Leaf - the retail id, from `ItemCatalog::vanilla`.
    const HEALING_LEAF: u8 = 0x77;

    let mut world = player_driven_battle();
    world.set_item_catalog(ItemCatalog::vanilla());
    world.inventory.insert(HEALING_LEAF, 3);
    // Durable on both sides: the assertion below is "the fight keeps handing
    // out commands", and a fight that simply ended would satisfy a weaker one.
    for slot in 0..5 {
        world.actors[slot].battle.max_hp = 40000;
        world.actors[slot].battle.hp = 40000;
    }
    // Wound the member the heal lands on, and arm every readout in sync - the
    // state a battle is in the moment anyone has been hit once, and the state
    // the ramp guard is written against.
    world.actors[0].battle.hp = 20000;
    for actor in world.actors.iter_mut() {
        actor.battle.arm_hp_bar();
    }

    assert!(
        settle_until(&mut world, 600, |w| w.battle_command.is_some()),
        "no command session to drive"
    );
    // Begin -> the ring, whose first arm is Item.
    tap(&mut world, PadButton::Cross);
    assert_eq!(
        world.battle_command.as_ref().and_then(|s| s.menu_command()),
        Some(BattleCommand::Item),
        "the ring's seated arm should be Item"
    );
    tap(&mut world, PadButton::Cross);
    assert!(
        world.battle_item_menu.is_some(),
        "the Item arm opened no item menu"
    );
    for _ in 0..8 {
        if world.battle_item_menu.is_none() {
            break;
        }
        tap(&mut world, PadButton::Cross);
    }

    let healed = world.actors[0].battle.hp;
    assert!(healed > 20000, "the item healed nothing (hp {healed})");
    assert_eq!(
        world.inventory.get(&HEALING_LEAF).copied(),
        Some(2),
        "one Healing Leaf consumed"
    );
    // Non-vacuity: the readout really is behind here, so the convergence
    // below is the ramp doing work rather than nothing having moved.
    assert_ne!(
        world.actors[0].battle.hp_display,
        Some(healed),
        "the readout should still be catching up right after the heal"
    );
    assert!(
        settle_until(&mut world, 600, |w| {
            w.actors[0].battle.hp_display == Some(w.actors[0].battle.hp)
        }),
        "the readout never caught up to live HP - the absorbing pair is back"
    );
    // ... and the turn pump survived it.
    assert!(
        settle_until(&mut world, 9000, |w| w.battle_command.is_some()),
        "no further command session after an item use - the fight parked"
    );
}
