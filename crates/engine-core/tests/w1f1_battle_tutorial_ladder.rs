//! Pad-driven ladder for the **sparring-tutorial prompt machine** - the
//! overlay-967 hook dispatcher (`FUN_801F6B70`) and the prompt-box emitter
//! (`FUN_801F747C`).
//!
//! `crates/engine-shell/tests/training_battle.rs` reaches the Rim Elm sparring
//! *fight* five ways but never primes the tutorial, so promoting that file to a
//! coverage export would not execute either routine. This ladder primes the
//! machine, walks a real encounter into a battle, and then drives the command
//! flow **by pad only** across the hook states, reading each queued box the way
//! a rendering host reads it (`ActiveTutorialBox::rect`, which is the emitter's
//! measure-then-place arithmetic).
//!
//! The prompt corpus is Sony bytes, so the script here is **synthetic**: every
//! message VA the machine can emit gets a short ASCII marker naming its own
//! address, exactly as `world/tests/battle_tutorial_flow.rs` does. Nothing on
//! this ladder needs a disc, so it runs in CI unconditionally - and that is
//! deliberate, because a disc-gated ladder contributes *nothing* to a coverage
//! export taken without `LEGAIA_DISC_BIN`.
//!
//! What is asserted is the machine's shape, not the port's current output:
//!
//! * a hook fires **once per entry** into a flow state (the `ctx[+0x6AE]`
//!   one-shot latch), so re-entering the same state twice must not re-queue;
//! * every queued box carries a decodable retail style `0..=9`, and its rect is
//!   the measured `(x, y, w, lines*14 - 4)` the emitter registers - the height
//!   is a function of the line count alone, and the bottom-anchored styles put
//!   the box's *bottom* at their anchor;
//! * a box on screen parks the battle loop (`ctx[+0x6B2]`), so no pad-driven
//!   frame can advance the action SM past it.

use legaia_engine_core::battle_flow::BattleFlowState;
use legaia_engine_core::battle_tutorial::{
    BattleTutorial, BattleTutorialScript, BoxStyle, OVERLAY_967_BASE_VA, TutorialLesson, msg,
};
use legaia_engine_core::input::{InputState, PadButton};
use legaia_engine_core::monster_catalog::{vanilla_formation_table, vanilla_monster_catalog};
use legaia_engine_core::world::{Actor, SceneMode, World};

/// A stand-in overlay blob: one ASCII marker per emittable message VA, so a
/// queued box is traceable back to the hook that produced it without shipping
/// any retail text.
fn synthetic_script() -> BattleTutorialScript {
    let base = OVERLAY_967_BASE_VA;
    let mut ids: Vec<u32> = BattleTutorialScript::MESSAGE_IDS.to_vec();
    ids.push(msg::ENTER_HIGH_LOW_HIGH);
    ids.push(msg::WRONG_COMMANDS);
    ids.push(msg::PRACTICE_OVER);
    let span = ids.iter().map(|v| v - base).max().unwrap() as usize + 16;
    let mut bytes = vec![0u8; span];
    for va in ids {
        let off = (va - base) as usize;
        // Two rendered lines for half the corpus: the emitter's height term is
        // `lines * 14 - 4`, and a one-line-only corpus cannot tell a height
        // that ignores the line count from one that honours it.
        let marker = if va % 2 == 0 {
            format!("m{va:08X}")
        } else {
            format!("m{va:08X}|second")
        };
        bytes[off..off + marker.len()].copy_from_slice(marker.as_bytes());
    }
    BattleTutorialScript::from_overlay(&bytes, base)
}

/// A field world one encounter away from the sparring fight, with the tutorial
/// primed so `enter_battle` arms it (the engine's stand-in for retail paging
/// stage overlay 967 into slot B).
fn primed_world() -> World {
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
        w.set_battle_attack(i as u8, 60);
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
    let mut table = EncounterTable::new("w1f1_tutorial_ladder");
    table.set_trigger_rate(0xFF);
    table.push(EncounterEntry::new(1, 1));
    let mut session = EncounterSession::new(EncounterTracker::new(table));
    session.transition_frames = 2;
    session.grace_frames = 2;
    w.set_encounter_session(Some(session));

    w.mode = SceneMode::Field;
    w.live_gameplay_loop = true;
    w.battle_player_driven = true;
    w.prime_battle_tutorial(synthetic_script());
    w
}

/// Walk the field until an encounter flips the world into battle.
fn walk_into_battle(w: &mut World) {
    let up = InputState::mask_of([PadButton::Up]);
    for _ in 0..6000 {
        w.set_pad(up);
        let _ = w.tick();
        if w.mode == SceneMode::Battle {
            return;
        }
    }
    panic!("no encounter triggered in 6000 field ticks");
}

/// Read every queued box the way a rendering host does: measure the text (a
/// host font here is stubbed as 6 px per column), apply the emitter's placement
/// + sizing arithmetic, and check the invariants that arithmetic must hold.
///
/// Returns the number of boxes inspected, so a caller can prove the read was
/// not vacuous.
fn inspect_boxes(w: &World) -> usize {
    let mut n = 0;
    for b in &w.battle_tutorial_boxes {
        let width: i16 = b
            .text
            .lines()
            .map(|l| l.chars().count() as i16 * 6)
            .max()
            .unwrap_or(0);
        let style = BoxStyle::from_raw(b.style)
            .unwrap_or_else(|| panic!("style {} outside the retail 0..=9 table", b.style));
        let (x, y, rw, rh) = b.rect(width).expect("a decodable style has a rect");
        let lines = b.lines();

        // The box is sized to its text: width straight through, height purely a
        // function of the rendered line count.
        assert_eq!(rw, width, "box width is the measured text width");
        assert_eq!(rh, lines * 14 - 4, "box height is lines*14 - 4");

        // Placement: centred styles put the text's centre on the 320-px stage
        // centre; the others sit at the fixed 0x10 left margin.
        if style.centred {
            assert_eq!(x, 0xA0 - width / 2, "centred style {}", b.style);
        } else {
            assert_eq!(x, 0x10, "left-margin style {}", b.style);
        }
        // A bottom-anchored style anchors the box's BOTTOM, not its top - the
        // reading that makes a two-line prompt grow upward instead of off the
        // bottom of the screen.
        match style.bottom_anchor {
            Some(base) => assert_eq!(y + rh, base, "bottom anchor of style {}", b.style),
            None => assert_eq!(y, 0x0E, "top-anchored style {}", b.style),
        }

        // `waits_for_input` is the emitter's `s4` latch, and the queue must
        // carry the same reading the style decodes to.
        assert_eq!(
            b.waits_for_input, style.waits_for_input,
            "queue disagrees with the style table on style {}",
            b.style
        );
        n += 1;
    }
    n
}

/// Dismiss every queued box with Cross, inspecting each before it goes.
/// Returns the total number of boxes seen.
fn drain_boxes(w: &mut World) -> usize {
    let mut seen = 0;
    for _ in 0..600 {
        if w.battle_tutorial_boxes.is_empty() {
            break;
        }
        seen += inspect_boxes(w);
        w.set_pad(InputState::mask_of([PadButton::Cross]));
        let _ = w.tick();
        w.set_pad(0);
        let _ = w.tick();
    }
    assert!(
        w.battle_tutorial_boxes.is_empty(),
        "box queue never drained: {:?}",
        w.battle_tutorial_boxes
    );
    seen
}

#[test]
fn a_primed_sparring_fight_raises_prompts_and_every_box_places_by_the_emitter() {
    let mut w = primed_world();
    walk_into_battle(&mut w);
    assert!(
        w.battle_tutorial.is_some(),
        "entering battle arms the primed machine"
    );
    assert_eq!(
        w.battle_tutorial_lesson(),
        Some(TutorialLesson::Attacks),
        "the sparring fight opens on lesson 0"
    );

    // Let the round prompt open; the turn-start hook (state 30) fires with it.
    let mut boxes = 0;
    for _ in 0..0x200 {
        if w.battle_tutorial_box_up() {
            break;
        }
        w.set_pad(0);
        let _ = w.tick();
    }
    assert!(
        w.battle_tutorial_box_up(),
        "the turn-start hook never raised a prompt"
    );
    assert_eq!(
        w.battle_flow,
        BattleFlowState::TurnPrompt,
        "the first hook state is the round prompt"
    );
    boxes += drain_boxes(&mut w);

    // Walk the command flow forward by pad. Cross takes Begin off the round
    // prompt and then commits an Attack; each transition is a fresh hook state,
    // and every box it queues is inspected on the way through.
    for _ in 0..0x400 {
        w.set_pad(InputState::mask_of([PadButton::Cross]));
        let _ = w.tick();
        w.set_pad(0);
        let _ = w.tick();
        if w.battle_tutorial_box_up() {
            boxes += drain_boxes(&mut w);
        }
        if w.battle_tutorial.is_none() || w.mode != SceneMode::Battle {
            break;
        }
    }

    assert!(
        boxes >= 4,
        "the pad walk only ever saw {boxes} prompt boxes - the hook table is \
         not being reached"
    );
}

#[test]
fn a_box_on_screen_parks_the_pad_driven_battle_loop() {
    let mut w = primed_world();
    walk_into_battle(&mut w);
    for _ in 0..0x200 {
        if w.battle_tutorial_box_up() {
            break;
        }
        w.set_pad(0);
        let _ = w.tick();
    }
    assert!(w.battle_tutorial_box_up(), "no prompt raised");

    // Retail's `ctx[+0x6B2]` guard: `FUN_801D0748` returns before it looks at
    // the flow state at all while a box is up, so nothing downstream may move.
    let action_state = w.battle_ctx.action_state;
    let flow = w.battle_flow;
    for _ in 0..30 {
        w.set_pad(0);
        let _ = w.tick();
    }
    assert_eq!(
        w.battle_ctx.action_state, action_state,
        "the action SM advanced while a tutorial box was up"
    );
    assert_eq!(w.battle_flow, flow, "the command flow advanced under a box");
    assert!(
        w.battle_tutorial_box_up(),
        "a waiting box must not time out"
    );
}

#[test]
fn a_hook_fires_once_per_entry_into_its_flow_state() {
    // The dispatcher half on its own: re-entering the same state without an
    // intervening `enter_flow_state` must not re-emit (the `ctx[+0x6AE]`
    // one-shot latch), and re-arming it must.
    let mut t = BattleTutorial::new();
    let first = t.tick(BattleFlowState::TurnPrompt.raw());
    assert!(
        !first.emission.boxes.is_empty(),
        "the turn-start hook emits on first entry"
    );
    let second = t.tick(BattleFlowState::TurnPrompt.raw());
    assert!(
        second.emission.boxes.is_empty(),
        "latched: a second tick in the same state must not re-emit"
    );
    t.enter_flow_state();
    let third = t.tick(BattleFlowState::TurnPrompt.raw());
    assert_eq!(
        third.emission.boxes, first.emission.boxes,
        "re-entering the state re-arms the same hook"
    );

    // The box-up guard suppresses the dispatcher entirely.
    t.enter_flow_state();
    t.box_up = true;
    assert!(
        t.tick(BattleFlowState::CategoryMenu.raw())
            .emission
            .boxes
            .is_empty(),
        "a box on screen suppresses the whole dispatch"
    );
}

#[test]
fn the_style_table_covers_the_retail_range_and_nothing_else() {
    // The emitter's jump table is 10 entries; anything past it is the retail
    // fall-through with no placement applied.
    for raw in 0u8..=9 {
        let s = BoxStyle::from_raw(raw).expect("style inside the table");
        // Styles 2..=7 are the waiting arms; 0/1/8/9 self-dismiss. `s4` is
        // initialised to 1 and only those four clear it.
        assert_eq!(
            s.waits_for_input,
            (2..=7).contains(&raw),
            "wait latch of style {raw}"
        );
        // Odd styles are the centred arms of each pair.
        assert_eq!(s.centred, raw % 2 == 1, "centring of style {raw}");
    }
    for raw in 10u8..=32 {
        assert!(
            BoxStyle::from_raw(raw).is_none(),
            "style {raw} is past the jump table"
        );
    }
}
