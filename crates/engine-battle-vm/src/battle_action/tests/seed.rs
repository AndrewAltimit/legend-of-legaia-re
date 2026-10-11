//! State-byte round trips, the round prompt and the action-seed band.

use super::*;

#[test]
fn action_state_byte_roundtrip() {
    for s in [
        ActionState::Begin,
        ActionState::ActionSeed,
        ActionState::AttackChain,
        ActionState::DoneCleanup,
        ActionState::EndOfAction,
        ActionState::RoundEnd,
    ] {
        assert_eq!(ActionState::from_byte(s.as_byte()).unwrap(), s);
    }
    // Unmapped byte returns None.
    assert!(ActionState::from_byte(0x07).is_none());
}

#[test]
fn action_category_byte_roundtrip() {
    for c in [
        ActionCategory::TacticalArts,
        ActionCategory::Item,
        ActionCategory::Magic,
        ActionCategory::Attack,
        ActionCategory::Spirit,
        ActionCategory::Run,
    ] {
        assert_eq!(ActionCategory::from_byte(c.as_byte()), c);
    }
    // Reserved bytes fold to TacticalArts.
    assert_eq!(
        ActionCategory::from_byte(0x42),
        ActionCategory::TacticalArts
    );
}

#[test]
fn begin_with_menu_open_routes_to_queued_from_menu() {
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 0);
    ctx.action_state = ActionState::Begin.as_byte();
    ctx.queued_action = 5;
    ctx.menu_open = 1;
    ctx.turn_cursor = 9;
    let out = step(&mut host, &mut ctx);
    assert!(matches!(
        out,
        StepOutcome::Transition {
            to,
            ..
        } if to == ActionState::QueuedFromMenu.as_byte()
    ));
    // `Begin` seeds the turn cursor from the formation advantage, which is
    // `0` here - it does NOT copy `queued_action` anywhere.
    assert_eq!(ctx.turn_cursor, 0);
}

/// The three storing arms of `Begin`'s `ctx[+0x290]` switch, plus the fourth
/// that stores nothing (`0x801E2AC0..0x801E2B24`).
#[test]
fn begin_seeds_the_turn_cursor_from_the_formation_advantage() {
    let seed = |advantage: u8, was: u8| {
        let (mut ctx, mut host) = fresh(ActionCategory::Attack, 0);
        ctx.action_state = ActionState::Begin.as_byte();
        ctx.formation_advantage = advantage;
        ctx.turn_cursor = was;
        step(&mut host, &mut ctx);
        ctx.turn_cursor
    };
    // The default host seats 3 party slots of 8.
    assert_eq!(seed(0, 9), 0, "no advantage: head of the order");
    assert_eq!(seed(1, 9), 3, "party count");
    assert_eq!(seed(2, 9), 5, "monster count");
    assert_eq!(seed(3, 9), 9, "unmapped advantage stores nothing");
}

/// State `0x00` runs once per **round** (flow `0xFE` writes `ctx[7] = 0` at
/// `0x801D3224` each round): within a round the pass is a no-op, and once the
/// host clears the flag at the round boundary the next pass copies the
/// already-cleared `+0x290` over `+0x291`.
/// The auto-fill leg rolls its target over the **seated** monsters
/// (`rand() % ctx[+0x01] + 3`, `0x801F0538..0x801F0570`) and redirects a dead
/// pick until it lands on a living one. With the two front monsters down and
/// the third standing, a roll over the living count alone could only name
/// the first seat, and the unbounded redirect never returned.
#[test]
fn auto_fill_target_roll_spans_the_seated_monsters() {
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 0);
    host.slot_count = 6;
    host.ability_bits_high
        .insert(0, crate::battle_arts_auto_combo::AUTO_FILL_ABILITY_BIT);
    for (slot, hp) in [(3usize, 0u16), (4, 0), (5, 40)] {
        host.actors[slot].max_hp = 40;
        host.actors[slot].liveness = hp;
    }
    host.rng_seq = vec![0, 0, 1, 2];
    round_state_zero(&mut host, &mut ctx);
    assert_eq!(host.actors[0].active_target, 5);
}

/// With fewer than three party members the host seats its monsters straight
/// after the party, so the retail seat the roll names is translated before
/// it is tested and stored.
#[test]
fn auto_fill_target_is_stored_in_host_seating() {
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 0);
    host.party_count = 2;
    host.slot_count = 4;
    host.ability_bits_high
        .insert(0, crate::battle_arts_auto_combo::AUTO_FILL_ABILITY_BIT);
    for slot in 2..4usize {
        host.actors[slot].max_hp = 40;
    }
    host.actors[2].liveness = 0;
    host.rng_seq = vec![0, 1];
    round_state_zero(&mut host, &mut ctx);
    assert_eq!(host.actors[0].active_target, 3);
}

#[test]
fn round_state_zero_runs_once_per_round_and_relatches_the_cleared_byte() {
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 0);
    ctx.formation_advantage = 2;
    assert!(round_state_zero(&mut host, &mut ctx));
    assert_eq!(ctx.formation_latched, 2);
    assert_eq!(ctx.formation_advantage, 0);
    ctx.turn_cursor = 4;
    assert!(!round_state_zero(&mut host, &mut ctx), "same round: no-op");
    assert_eq!(ctx.turn_cursor, 4);
    assert_eq!(ctx.formation_latched, 2);
    // Round boundary.
    ctx.formation_armed = false;
    assert!(round_state_zero(&mut host, &mut ctx));
    assert_eq!(
        ctx.formation_latched, 0,
        "round two latches the cleared byte"
    );
    assert_eq!(ctx.turn_cursor, 0);
}

#[test]
fn begin_without_menu_routes_to_pre_action_wait() {
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 0);
    ctx.action_state = ActionState::Begin.as_byte();
    let out = step(&mut host, &mut ctx);
    assert!(matches!(
        out,
        StepOutcome::Transition {
            to,
            ..
        } if to == ActionState::PreActionWait.as_byte()
    ));
}

#[test]
fn pre_action_wait_holds_until_cleared() {
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 0);
    ctx.action_state = ActionState::PreActionWait.as_byte();
    host.prev_cleared = false;
    let out = step(&mut host, &mut ctx);
    assert_eq!(out, StepOutcome::Stay);
    host.prev_cleared = true;
    let out = step(&mut host, &mut ctx);
    assert!(matches!(
        out,
        StepOutcome::Transition {
            to,
            ..
        } if to == ActionState::ActionSeed.as_byte()
    ));
}

#[test]
fn queued_from_menu_holds_then_releases() {
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 0);
    ctx.action_state = ActionState::QueuedFromMenu.as_byte();
    ctx.menu_open = 1;
    assert_eq!(step(&mut host, &mut ctx), StepOutcome::Stay);
    ctx.menu_open = 0;
    let out = step(&mut host, &mut ctx);
    assert!(matches!(
        out,
        StepOutcome::Transition {
            to,
            ..
        } if to == ActionState::PreActionWait.as_byte()
    ));
}

#[test]
fn action_seed_attack_routes_to_attack_face_and_emits_ui() {
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 1);
    ctx.action_state = ActionState::ActionSeed.as_byte();
    let out = step(&mut host, &mut ctx);
    assert!(matches!(
        out,
        StepOutcome::Transition {
            to,
            ..
        } if to == ActionState::AttackFace.as_byte()
    ));
    // Party slot < 3 → fires UI element 7.
    let events = host.take();
    assert!(events.contains(&Event::PartySetup(1)));
    assert!(events.contains(&Event::Camera));
    assert!(events.contains(&Event::Pose(1, Pose::Idle)));
    assert!(events.contains(&Event::Ui(7, 0)));
}

/// The action-dispatch liveness gate: a DEAD acting actor never enters an
/// action band. Retail holds the invariant upstream (`FUN_801DABA4`'s dead
/// sweep zeroes the key, bumps `ctx[+0x25]` and can never pick the slot into
/// `ctx[+0x274]`); the port's hosts can kill an actor between arming and
/// seed, so the seed itself routes the dead actor where retail's
/// cleared-action category-0 arm routes - state `0x50` - with no setup hook,
/// no camera, no attack band.
#[test]
fn action_seed_dead_actor_routes_to_done_cleanup_not_the_attack_band() {
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 5);
    // Stage the multi-strike shape the defect ran: a monster with a queued
    // swing stream, killed before its staged action fires.
    host.actors[5].params[0] = 0x01;
    host.actors[5].liveness = 0;
    ctx.action_state = ActionState::ActionSeed.as_byte();
    let out = step(&mut host, &mut ctx);
    assert!(matches!(
        out,
        StepOutcome::Transition {
            to,
            ..
        } if to == ActionState::DoneCleanup.as_byte()
    ));
    // The gate sits ahead of every seed side effect.
    let events = host.take();
    assert!(!events.contains(&Event::MonsterSetup(5)));
    assert!(!events.contains(&Event::Camera));
    assert!(!events.iter().any(|e| matches!(e, Event::Pose(5, _))));
}

/// The whole arc: a dead monster's staged attack is consumed as a spent turn
/// (the SM walks the Done band to `EndOfAction`) and the attack band is never
/// entered - so no strike can ever land from a corpse.
#[test]
fn dead_actor_staged_action_is_spent_without_a_single_attack_state() {
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 5);
    host.actors[5].liveness = 0;
    ctx.action_state = ActionState::Begin.as_byte();
    let attack_band = [
        ActionState::AttackFace.as_byte(),
        ActionState::AttackWindup.as_byte(),
        ActionState::AttackAdvance.as_byte(),
        ActionState::AttackCloseRange.as_byte(),
        ActionState::AttackStrike.as_byte(),
        ActionState::AttackShortStep.as_byte(),
        ActionState::AttackChain.as_byte(),
        ActionState::AttackRecovery.as_byte(),
        ActionState::AttackReturn.as_byte(),
    ];
    for _ in 0..400 {
        step(&mut host, &mut ctx);
        assert!(
            !attack_band.contains(&ctx.action_state),
            "dead actor reached attack state {:#04x}",
            ctx.action_state
        );
        if ctx.action_state == ActionState::EndOfAction.as_byte() {
            break;
        }
    }
    assert_eq!(
        ctx.action_state,
        ActionState::EndOfAction.as_byte(),
        "the dead actor's turn is spent through the Done band, not parked"
    );
    assert!(
        !host
            .take()
            .iter()
            .any(|e| matches!(e, Event::ApplyDamage(..) | Event::ApplyArtStrike(_))),
        "no damage may originate from a dead acting actor"
    );
}

/// A LIVING actor with the same staged action still dispatches into the
/// attack band - the gate reads liveness, not the staging.
#[test]
fn action_seed_living_actor_still_reaches_the_attack_band() {
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 5);
    host.actors[5].params[0] = 0x01;
    ctx.action_state = ActionState::ActionSeed.as_byte();
    let out = step(&mut host, &mut ctx);
    assert!(matches!(
        out,
        StepOutcome::Transition {
            to,
            ..
        } if to == ActionState::AttackFace.as_byte()
    ));
}

#[test]
fn action_seed_run_party_routes_to_run_begin() {
    let (mut ctx, mut host) = fresh(ActionCategory::Run, 1);
    ctx.action_state = ActionState::ActionSeed.as_byte();
    let out = step(&mut host, &mut ctx);
    assert!(matches!(
        out,
        StepOutcome::Transition {
            to,
            ..
        } if to == ActionState::RunBegin.as_byte()
    ));
    // Camera not called for run actions.
    assert!(!host.take().contains(&Event::Camera));
}

#[test]
fn action_seed_run_monster_routes_to_capture_start() {
    let (mut ctx, mut host) = fresh(ActionCategory::Run, 5);
    ctx.action_state = ActionState::ActionSeed.as_byte();
    let out = step(&mut host, &mut ctx);
    assert!(matches!(
        out,
        StepOutcome::Transition {
            to,
            ..
        } if to == ActionState::CaptureStart.as_byte()
    ));
}

#[test]
fn action_seed_magic_routes_to_magic_cast_begin() {
    let (mut ctx, mut host) = fresh(ActionCategory::Magic, 1);
    ctx.action_state = ActionState::ActionSeed.as_byte();
    let out = step(&mut host, &mut ctx);
    assert!(matches!(
        out,
        StepOutcome::Transition {
            to,
            ..
        } if to == ActionState::MagicCastBegin.as_byte()
    ));
}

/// The Magic arm's class-byte discriminator (`0x801E2EEC..0x801E2EF8`): the
/// override to the Spirit band needs **both** `class < 0x14` *and* `id <
/// 0x65`, so each test on its own must leave the default `0x28` store alone.
#[test]
fn action_seed_magic_class_discriminator_takes_both_tests() {
    let band = |class: Option<u8>, spell_id: u8| -> u8 {
        let (mut ctx, mut host) = fresh(ActionCategory::Magic, 1);
        ctx.action_state = ActionState::ActionSeed.as_byte();
        host.actors[1].params[0] = spell_id;
        if let Some(c) = class {
            host.spell_classes.insert(spell_id, c);
        }
        step(&mut host, &mut ctx);
        ctx.action_state
    };
    let cast_begin = ActionState::MagicCastBegin.as_byte();
    let pre_arm = ActionState::SpiritPreArm.as_byte();

    // Both tests pass -> 0x3C.
    assert_eq!(band(Some(0x02), 0x10), pre_arm);
    assert_eq!(band(Some(0x13), 0x64), pre_arm, "0x13 / 0x64 are inclusive");
    // Class fails the `< 0x14` test -> the default 0x28 store stands.
    assert_eq!(band(Some(0x14), 0x10), cast_begin);
    assert_eq!(band(Some(0x32), 0x10), cast_begin);
    // Id fails the `< 0x65` test -> likewise. The player Seru block is here.
    assert_eq!(band(Some(0x02), 0x65), cast_begin);
    assert_eq!(band(Some(0x02), 0x81), cast_begin);
}

/// A host that supplies no spell table cannot evaluate the class test, so it
/// keeps retail's non-override branch for every id - which is what every
/// disc-free battle in this engine does.
#[test]
fn action_seed_magic_without_a_spell_table_always_takes_the_magic_band() {
    for spell_id in [0x00u8, 0x10, 0x64, 0x81, 0xFF] {
        let (mut ctx, mut host) = fresh(ActionCategory::Magic, 1);
        ctx.action_state = ActionState::ActionSeed.as_byte();
        host.actors[1].params[0] = spell_id;
        step(&mut host, &mut ctx);
        assert_eq!(ctx.action_state, ActionState::MagicCastBegin.as_byte());
    }
}

/// With no override, `is_capture_spell` falls out of the same class byte the
/// band pick reads - one record, one answer.
#[test]
fn the_default_capture_predicate_derives_from_the_class_byte() {
    struct ClassOnly(std::collections::HashMap<u8, u8>);
    impl BattleActionHost for ClassOnly {
        fn actor(&self, _: u8) -> Option<&BattleActor> {
            None
        }
        fn actor_mut(&mut self, _: u8) -> Option<&mut BattleActor> {
            None
        }
        fn spell_class_byte(&self, id: u8) -> Option<u8> {
            self.0.get(&id).copied()
        }
    }
    let mut classes = std::collections::HashMap::new();
    classes.insert(0x37u8, legaia_game_tables::spell_names::CAPTURE_CLASS);
    classes.insert(0x38u8, 0x32u8);
    let host = ClassOnly(classes);
    assert!(host.is_capture_spell(0x37));
    assert!(
        !host.is_capture_spell(0x38),
        "a non-'c' record is not capture"
    );
    assert!(
        !host.is_capture_spell(0x39),
        "no record at all is not capture"
    );
}

/// The Item arm's own override (`0x801E2E6C..0x801E2EAC`): item ids `0x98` /
/// `0x99` leave the Spirit band for the cast band, staged as a summon.
#[test]
fn action_seed_item_summon_ids_take_the_cast_band_as_a_summon() {
    for (item_id, staged) in [(0x98u8, 0x96u8), (0x99, 0x97)] {
        let (mut ctx, mut host) = fresh(ActionCategory::Item, 1);
        ctx.action_state = ActionState::ActionSeed.as_byte();
        host.actors[1].params[0] = item_id;
        step(&mut host, &mut ctx);
        assert_eq!(ctx.action_state, ActionState::MagicCastBegin.as_byte());
        assert_eq!(host.actors[1].sub_route, 9, "staged as a summon");
        assert_eq!(host.actors[1].params[0], staged, "id is rebased by -2");
    }
}

/// Every other item id keeps the arm's default `0x3C` store, untouched.
#[test]
fn action_seed_ordinary_items_stay_on_the_spirit_band() {
    for item_id in [0x00u8, 0x01, 0x97, 0x9A, 0xFF] {
        let (mut ctx, mut host) = fresh(ActionCategory::Item, 1);
        ctx.action_state = ActionState::ActionSeed.as_byte();
        host.actors[1].params[0] = item_id;
        step(&mut host, &mut ctx);
        assert_eq!(
            ctx.action_state,
            ActionState::SpiritPreArm.as_byte(),
            "item {item_id:#04x}"
        );
        assert_eq!(host.actors[1].params[0], item_id, "and is not rewritten");
    }
}

/// The seed body's own `rand()` (`0x801E2D04`) plus the Item arm's
/// (`0x801E2E3C`) are two draws off the shared cursor, in that order - the
/// property the RNG stream depends on, independent of what the values mean.
/// A Magic action makes only the seed draw, so the two categories are not
/// interchangeable in the stream.
#[test]
fn action_seed_item_draws_twice_and_magic_once() {
    for (category, expected) in [
        (ActionCategory::Item, 2usize),
        // The Spirit arm's own draw sits at `0x801E2FFC`, the same
        // unconditional shape the Item arm's has.
        (ActionCategory::Spirit, 2),
        (ActionCategory::Magic, 1),
        (ActionCategory::Attack, 1),
        (ActionCategory::TacticalArts, 1),
    ] {
        let (mut ctx, mut host) = fresh(category, 1);
        ctx.action_state = ActionState::ActionSeed.as_byte();
        host.rng_seq = vec![7, 7, 7, 7];
        step(&mut host, &mut ctx);
        assert_eq!(
            *host.rng_pos.borrow(),
            expected,
            "{category:?} draws off the shared cursor"
        );
    }
}

/// `ctx[+0xD]`, the camera-angle variant: the seed rolls `rand() % 4`, then
/// the category arm narrows it - Item to `(rand % 2) * 2` (so `0` or `2`),
/// Magic and Tactical Arts to `0` outright.
#[test]
fn action_seed_camera_variant_follows_the_category_arm() {
    // Item, second draw odd -> (1 % 2) * 2 == 2.
    let (mut ctx, mut host) = fresh(ActionCategory::Item, 1);
    ctx.action_state = ActionState::ActionSeed.as_byte();
    host.rng_seq = vec![3, 1];
    step(&mut host, &mut ctx);
    assert_eq!(ctx.camera_variant, 2);

    // Item, second draw even -> 0.
    let (mut ctx, mut host) = fresh(ActionCategory::Item, 1);
    ctx.action_state = ActionState::ActionSeed.as_byte();
    host.rng_seq = vec![3, 4];
    step(&mut host, &mut ctx);
    assert_eq!(ctx.camera_variant, 0);

    // The summon route re-stamps `0` even on an odd draw (`0x801E2E94`).
    let (mut ctx, mut host) = fresh(ActionCategory::Item, 1);
    ctx.action_state = ActionState::ActionSeed.as_byte();
    host.actors[1].params[0] = 0x98;
    host.rng_seq = vec![3, 1];
    step(&mut host, &mut ctx);
    assert_eq!(ctx.action_state, ActionState::MagicCastBegin.as_byte());
    assert_eq!(ctx.camera_variant, 0);

    // Magic / Tactical Arts pin `0` whatever the seed rolled.
    for category in [ActionCategory::Magic, ActionCategory::TacticalArts] {
        let (mut ctx, mut host) = fresh(category, 1);
        ctx.action_state = ActionState::ActionSeed.as_byte();
        host.rng_seq = vec![3];
        step(&mut host, &mut ctx);
        assert_eq!(ctx.camera_variant, 0, "{category:?}");
    }
}

/// The capture harness's variant pin lands over the seed's roll and the
/// category arm's narrowing, and leaves the draws themselves in place.
#[test]
fn action_seed_camera_variant_pin_overrides_the_roll_not_the_draws() {
    let (mut ctx, mut host) = fresh(ActionCategory::Item, 1);
    ctx.action_state = ActionState::ActionSeed.as_byte();
    host.rng_seq = vec![3, 1];
    ctx.camera_variant_pin = Some(3);
    step(&mut host, &mut ctx);
    assert_eq!(ctx.camera_variant, 3);
    let (mut unpinned, mut host2) = fresh(ActionCategory::Item, 1);
    unpinned.action_state = ActionState::ActionSeed.as_byte();
    host2.rng_seq = vec![3, 1];
    step(&mut host2, &mut unpinned);
    assert_eq!(*host.rng_pos.borrow(), *host2.rng_pos.borrow());
    assert_eq!(ctx.action_state, unpinned.action_state);
}

/// The banner byte is cleared at the head of every action, so a tail extended
/// by one action's level-up cannot leak into the next one.
#[test]
fn action_seed_clears_the_level_up_banner_byte() {
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 1);
    ctx.action_state = ActionState::ActionSeed.as_byte();
    ctx.levelup_banner_element = 0x65;
    step(&mut host, &mut ctx);
    assert_eq!(ctx.levelup_banner_element, 0);
}

#[test]
fn action_seed_monster_with_ai_flag_calls_monster_setup() {
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 4);
    host.actors[4].field_flags = 0x380;
    ctx.action_state = ActionState::ActionSeed.as_byte();
    step(&mut host, &mut ctx);
    let events = host.take();
    assert!(events.contains(&Event::MonsterSetup(4)));
    assert!(!events.iter().any(|e| matches!(e, Event::PartySetup(_))));
}

// --- target banner raised at the ActionSeed tail (FUN_801E6D84) ------------

/// Every category arm of retail's seed body falls into the same
/// `jal 0x801e6d84`, and the caster banner `0x44` is what that routine raises
/// before any of the target arms - so it is unconditional on the seed path,
/// for every category except the one that returns first.
#[test]
fn action_seed_raises_the_caster_banner_for_every_category_but_run() {
    for category in [
        ActionCategory::TacticalArts,
        ActionCategory::Item,
        ActionCategory::Magic,
        ActionCategory::Attack,
        ActionCategory::Spirit,
    ] {
        let (mut ctx, mut host) = fresh(category, 0);
        host.party_count = 3;
        ctx.action_state = ActionState::ActionSeed.as_byte();
        step(&mut host, &mut ctx);
        assert!(
            host.take().contains(&Event::Ui(0x44, 0)),
            "no caster banner for {category:?}"
        );
    }
}

/// The one category `FUN_801E6D84` returns from before raising anything is
/// `5` - which is **Run / Defend**, not Spirit (`4`).
#[test]
fn action_seed_raises_no_banner_for_the_run_category() {
    let (mut ctx, mut host) = fresh(ActionCategory::Run, 0);
    host.party_count = 3;
    ctx.action_state = ActionState::ActionSeed.as_byte();
    step(&mut host, &mut ctx);
    assert!(!host.take().iter().any(|e| matches!(e, Event::Ui(0x44, _))));
}

/// Categories `0` and `4` raise the caster banner and then skip the target
/// arm outright - the caster banner has already been raised by then.
#[test]
fn spirit_and_arts_raise_only_the_caster_banner() {
    for category in [ActionCategory::TacticalArts, ActionCategory::Spirit] {
        let (mut ctx, mut host) = fresh(category, 0);
        host.party_count = 3;
        host.actors[0].active_target = 4; // a named monster
        ctx.action_state = ActionState::ActionSeed.as_byte();
        step(&mut host, &mut ctx);
        let events = host.take();
        assert!(events.contains(&Event::Ui(0x44, 0)), "{category:?}");
        assert!(!events.contains(&Event::Ui(0x51, 0)), "{category:?}");
    }
}

/// A named monster target adds the single-target banner `0x51` on top of the
/// caster banner; a party-slot target does not.
#[test]
fn a_named_monster_target_adds_the_target_banner() {
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 0);
    host.party_count = 3;
    host.actors[0].active_target = 4;
    ctx.action_state = ActionState::ActionSeed.as_byte();
    step(&mut host, &mut ctx);
    let events = host.take();
    assert!(events.contains(&Event::Ui(0x44, 0)));
    assert!(events.contains(&Event::Ui(0x51, 0)));

    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 0);
    host.party_count = 3;
    host.actors[0].active_target = 1; // an ally
    ctx.action_state = ActionState::ActionSeed.as_byte();
    step(&mut host, &mut ctx);
    let events = host.take();
    assert!(events.contains(&Event::Ui(0x44, 0)));
    assert!(!events.contains(&Event::Ui(0x51, 0)));
}

/// `PreActionWait` is gated on `previous_action_cleared`. With the gate
/// closed, the state holds; flipping the gate transitions to `ActionSeed`
/// on the next step.
#[test]
fn pre_action_wait_holds_until_prev_cleared_flips() {
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 1);
    ctx.action_state = ActionState::PreActionWait.as_byte();
    host.prev_cleared = false;

    // Several steps with the gate closed must not transition.
    for _ in 0..8 {
        assert_eq!(step(&mut host, &mut ctx), StepOutcome::Stay);
        assert_eq!(ctx.action_state, ActionState::PreActionWait.as_byte());
    }

    // Flip the gate. Next step transitions.
    host.prev_cleared = true;
    let out = step(&mut host, &mut ctx);
    assert!(matches!(
        out,
        StepOutcome::Transition {
            to,
            ..
        } if to == ActionState::ActionSeed.as_byte()
    ));
}
