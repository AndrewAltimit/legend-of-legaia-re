use super::*;

fn hand(costs: [u16; 4]) -> [MuscleCard; 4] {
    [
        MuscleCard {
            command_id: 0x0C,
            cost: costs[0],
        },
        MuscleCard {
            command_id: 0x0F,
            cost: costs[1],
        },
        MuscleCard {
            command_id: 0x0E,
            cost: costs[2],
        },
        MuscleCard {
            command_id: 0x0D,
            cost: costs[3],
        },
    ]
}

fn session() -> MuscleDomeSession {
    MuscleDomeSession::new(
        hand([0x1E, 0x2A, 0x2A, 0x1E]),
        hand([0x1E, 0x1E, 0x1E, 0x1E]),
        [100, 70],
        [500, 400],
        3,
    )
}

// --- The Ra-Seru command class ----------------------------------------

fn seru(id: u8, mp: u8, power: u16) -> crate::spells::SpellDef {
    crate::spells::SpellDef {
        id,
        name: format!("Seru{id:02x}"),
        mp_cost: mp,
        element: crate::spells::SpellElement::Neutral,
        target: crate::spells::SpellTarget::OneEnemy,
        effect: crate::spells::SpellEffect::Damage {
            base_power: power,
            element: crate::spells::SpellElement::Neutral,
        },
        anim_id: 0,
        effect_class: 0,
    }
}

fn magic(mp: u16) -> DomeMagic {
    DomeMagic {
        ring: DomeRing {
            special: 0,
            status: 0,
            has_raseru: true,
        },
        mp,
        mp_max: 60,
        ability_bits: 0,
        magic_power: 40,
        spells: vec![seru(0x81, 8, 40), seru(0x82, 30, 90)],
    }
}

#[test]
fn the_ra_seru_chip_is_live_exactly_when_retails_three_gates_pass() {
    let mut s = session();
    // No loadout: retail's `-` chip, and no mark rides with it.
    assert!(!s.chip_enabled(0, DomeRingChip::RaSeru));
    assert_eq!(s.chip_mark(0, DomeRingChip::RaSeru), None);
    s.install_magic(0, magic(60));
    assert!(s.chip_enabled(0, DomeRingChip::RaSeru));
    assert_eq!(s.chip_mark(0, DomeRingChip::RaSeru), None);
    // Sealed: the chip keeps its plate and wears `FUN_801DBEC4`'s mark.
    let mut sealed = magic(60);
    sealed.ring.status = STATUS_MAGIC_SEALED;
    s.install_magic(0, sealed);
    assert!(!s.chip_enabled(0, DomeRingChip::RaSeru));
    assert_eq!(s.chip_mark(0, DomeRingChip::RaSeru), Some(ChipMark::Sealed));
    // Forbidden: the red cross-out X, the same emitter Item's uses. The
    // word is the SESSION's, not the loadout's - retail has one per
    // battle, which is why a fighter with no loadout at all still has its
    // Item chip crossed out below.
    s.install_magic(0, magic(60));
    s.set_special_word(SPECIAL_MAGIC_FORBIDDEN);
    assert!(!s.chip_enabled(0, DomeRingChip::RaSeru));
    assert_eq!(
        s.chip_mark(0, DomeRingChip::RaSeru),
        Some(ChipMark::Forbidden)
    );
    // ...and the Item chip is the *other* bit, so a magic ban leaves it
    // alone. Retail's dome crosses out Item, never both.
    assert!(s.chip_enabled(0, DomeRingChip::Item));
    assert_eq!(s.chip_mark(0, DomeRingChip::Item), None);
}

#[test]
fn the_item_chip_carries_its_own_bit() {
    let mut s = session();
    // The word is per battle, so it gates the Item chip whether or not a
    // magic loadout is installed - assert the no-loadout case first,
    // which is the one a per-loadout word could not reach.
    s.set_special_word(SPECIAL_ITEM_FORBIDDEN);
    assert!(!s.chip_enabled(0, DomeRingChip::Item));
    assert_eq!(
        s.chip_mark(0, DomeRingChip::Item),
        Some(ChipMark::Forbidden)
    );
    s.install_magic(0, magic(60));
    assert!(!s.chip_enabled(0, DomeRingChip::Item));
    assert_eq!(
        s.chip_mark(0, DomeRingChip::Item),
        Some(ChipMark::Forbidden)
    );
    // The magic bit is clear, so the Ra-Seru arm still commits - which is
    // exactly the dome's retail cluster.
    assert!(s.chip_enabled(0, DomeRingChip::RaSeru));
}

#[test]
fn a_cast_spends_mp_not_ap_and_replaces_the_direction_string() {
    let mut s = session();
    s.install_magic(0, magic(60));
    assert!(s.commit_card(0, 0));
    let budget_after_a_swing = s.budget(0);
    assert!(budget_after_a_swing < 100);
    let cost = s.commit_cast(0, 0x81).expect("Gimard is affordable");
    assert_eq!(cost, 8);
    // The AP the string spent comes back: retail's magic arm never reads
    // `ctx+0x6D8` / `ctx+0x6DC`, and the queue store is a whole-string
    // replacement at `actor+0x1DF[0]`.
    assert_eq!(s.budget(0), 100);
    assert_eq!(s.spent(0), 0);
    assert!(s.queue(0).is_empty());
    assert_eq!(s.queued_cast(0), Some(0x81));
    assert_eq!(s.turn_action(0), DomeTurnAction::Cast(0x81));
    // Not charged until the turn plays out - retail debits at the shared
    // band's cast-begin, not at the pick.
    assert_eq!(s.mp(0), 60);
    s.ai_commit_all(1);
    s.end_selection();
    let foe_before = s.hp(1);
    s.resolve_turn(|_, _| 7);
    assert_eq!(s.mp(0), 52, "the cast charged its MP once");
    assert!(s.hp(1) < foe_before, "the cast landed");
}

#[test]
fn an_unaffordable_pick_is_refused_and_charges_nothing() {
    let mut s = session();
    s.install_magic(0, magic(10));
    assert_eq!(
        s.commit_cast(0, 0x82),
        Err(DomeCastRefusal::NotEnoughMp),
        "30 MP against a 10 MP gauge"
    );
    assert_eq!(s.queued_cast(0), None);
    assert_eq!(s.mp(0), 10);
    // The cheap one still goes.
    assert_eq!(s.commit_cast(0, 0x81), Ok(8));
}

#[test]
fn the_ability_bits_discount_the_price_the_arm_compares() {
    let mut s = session();
    let mut m = magic(60);
    // Bit 0x20 halves; retail's arm is `cost - (cost >> 1)`.
    m.ability_bits = 0x20;
    s.install_magic(0, m);
    assert_eq!(s.spell_mp_cost(0, 0x82), Some(15));
    assert_eq!(s.spell_rows(0)[1].mp_cost, 15);
    s.commit_cast(0, 0x82).expect("half price is affordable");
    s.ai_commit_all(1);
    s.end_selection();
    s.resolve_turn(|_, _| 0);
    assert_eq!(s.mp(0), 45);
}

#[test]
fn an_open_ra_seru_list_swallows_the_direction_input() {
    let mut s = session();
    s.install_magic(0, magic(60));
    s.open_magic(0).expect("the gates pass");
    assert!(s.magic_open());
    assert!(!s.can_commit(0, 0), "the list is the surface, not the deck");
    s.move_magic_cursor(0, 1);
    assert_eq!(s.magic_cursor(), 1);
    s.move_magic_cursor(0, 1);
    assert_eq!(s.magic_cursor(), 0, "the cursor wraps");
    s.close_magic();
    assert!(!s.magic_open());
    assert!(s.can_commit(0, 0));
}

#[test]
fn a_sealed_or_forbidden_fighter_cannot_open_the_list() {
    let mut s = session();
    assert_eq!(s.open_magic(0), Err(DomeCastRefusal::NoLoadout));
    let mut m = magic(60);
    m.ring.has_raseru = false;
    s.install_magic(0, m);
    assert_eq!(s.open_magic(0), Err(DomeCastRefusal::NoRaSeru));
    let mut m = magic(60);
    m.ring.status = STATUS_MAGIC_SEALED;
    s.install_magic(0, m);
    assert_eq!(s.open_magic(0), Err(DomeCastRefusal::Sealed));
    s.install_magic(0, magic(60));
    s.set_special_word(SPECIAL_MAGIC_FORBIDDEN);
    assert_eq!(s.open_magic(0), Err(DomeCastRefusal::Forbidden));
    assert!(!s.magic_open());
}

#[test]
fn the_shared_select_input_drives_the_list_on_both_hosts() {
    let mut s = session();
    s.install_magic(0, magic(60));
    let pad = |f: fn(&mut DomeSelectPad)| {
        let mut p = DomeSelectPad::default();
        f(&mut p);
        p
    };
    assert!(!s.select_input(pad(|p| p.magic = true)));
    assert!(s.magic_open());
    assert!(!s.select_input(pad(|p| p.down = true)));
    assert_eq!(s.magic_cursor(), 1);
    // Row 1 costs 30 of the 60-MP gauge, so the confirm commits and the
    // turn closes for both fighters.
    assert!(s.select_input(pad(|p| p.confirm = true)));
    assert_eq!(s.phase(), MusclePhase::Resolve);
    assert_eq!(s.queued_cast(0), Some(0x82));

    // A fresh session: with the list shut a direction press still commits
    // a card, and a turn boundary clears any cast the last one carried.
    let mut s = session();
    s.install_magic(0, magic(60));
    s.commit_cast(0, 0x81).expect("affordable");
    s.ai_commit_all(1);
    s.end_selection();
    s.resolve_turn(|_, _| 0);
    assert_eq!(
        s.phase(),
        MusclePhase::TurnOver,
        "the foe survived a Seru01"
    );
    s.next_turn();
    assert_eq!(s.queued_cast(0), None, "a new turn clears the cast");
    assert!(!s.select_input(pad(|p| p.left = true)));
    assert_eq!(s.queue(0).len(), 1);
}

#[test]
fn a_reselect_throws_the_cast_away_with_the_string() {
    let mut s = session();
    s.install_magic(0, magic(60));
    s.commit_cast(0, 0x81).expect("affordable");
    s.reset_selection(0);
    assert_eq!(s.queued_cast(0), None);
    assert_eq!(s.mp(0), 60, "an uncommitted cast charged nothing");
    assert_eq!(s.phase(), MusclePhase::Select);
}

#[test]
fn every_ring_chip_names_its_retail_anchor_and_action_byte() {
    // The element table's arrived endpoints, and the `actor+0x1DE` byte
    // each arm stores.
    assert_eq!(DomeRingChip::Item.anchor(), (204, 34));
    assert_eq!(DomeRingChip::Attack.anchor(), (160, 66));
    assert_eq!(DomeRingChip::RaSeru.anchor(), (248, 66));
    assert_eq!(DomeRingChip::Spirit.anchor(), (204, 98));
    assert_eq!(DomeRingChip::Item.action_state(), 1);
    assert_eq!(DomeRingChip::RaSeru.action_state(), 2);
    assert_eq!(DomeRingChip::Attack.action_state(), 3);
    assert_eq!(DomeRingChip::Spirit.action_state(), 4);
    // The three mark emitters differ only in source rect and palette.
    assert_eq!(ChipMark::Forbidden.source_rect(), (0, 96, 64, 16));
    assert_eq!(ChipMark::Blocked.source_rect(), (80, 96, 32, 24));
    assert_eq!(ChipMark::Sealed.source_rect(), (120, 96, 64, 16));
    assert_eq!(ChipMark::Forbidden.clut(), 0x7704);
    assert_eq!(ChipMark::Blocked.clut(), 0x770B);
    assert_eq!(ChipMark::Sealed.clut(), 0x7700);
}

#[test]
fn the_attack_chip_needs_all_three_status_bits_to_be_blocked() {
    let mut s = session();
    let mut m = magic(60);
    m.ring.status = 0x18; // two of the three
    s.install_magic(0, m);
    assert!(s.chip_enabled(0, DomeRingChip::Attack));
    assert_eq!(s.chip_mark(0, DomeRingChip::Attack), None);
    let mut m = magic(60);
    m.ring.status = STATUS_ATTACK_BLOCKED;
    s.install_magic(0, m);
    assert!(!s.chip_enabled(0, DomeRingChip::Attack));
    assert_eq!(
        s.chip_mark(0, DomeRingChip::Attack),
        Some(ChipMark::Blocked)
    );
}

#[test]
fn commit_respects_the_budget() {
    let mut s = session();
    assert!(s.commit_card(0, 0)); // 0x1E = 30, budget 70 left
    assert!(s.commit_card(0, 1)); // 0x2A = 42, budget 28 left
    assert_eq!(s.spent(0), 72);
    assert_eq!(s.budget(0), 28);
    assert!(!s.commit_card(0, 2), "42 > 28 rejected");
    assert!(!s.commit_card(0, 3), "30 > 28 rejected");
}

#[test]
fn queue_carries_command_ids() {
    let mut s = session();
    s.commit_card(0, 0);
    s.commit_card(0, 3);
    assert_eq!(s.queue(0), &[0x0C, 0x0D]);
}

#[test]
fn ai_commits_greedily_under_budget() {
    let mut s = session();
    s.ai_commit_all(1);
    // Pool 70, all cards 30: two commits (60), third rejected.
    assert_eq!(s.queue(1).len(), 2);
    assert_eq!(s.spent(1), 60);
}

#[test]
fn resolution_plays_whole_strings_and_reads_hp_left() {
    let mut s = session();
    s.commit_card(0, 0);
    s.commit_card(0, 1);
    s.ai_commit_all(1);
    s.end_selection();
    assert_eq!(s.phase(), MusclePhase::Resolve);
    s.resolve_turn(|_, _| 50);
    // Player queued 2, opponent 2: both take 100.
    assert_eq!(s.hp(0), 400);
    assert_eq!(s.hp(1), 300);
    assert_eq!(s.last_turn_damage(), [100, 100]);
    assert_eq!(s.phase(), MusclePhase::TurnOver);
    // The readout is a plain percentage (scale 100, not 0x6C), and the
    // HUD's own number is the OPPONENT's.
    assert_eq!(s.hp_left_percent(0), 400 * 100 / 500);
    assert_eq!(s.hp_left_percent(1), 300 * 100 / 400);
    assert_eq!(s.hp_left(), 75, "HUD reads slot 1 = the opponent");
    // The turn counter advanced; nothing is counting down against it.
    assert_eq!(s.turn(), 1);
    // Next turn reseeds budgets + clears queues.
    s.next_turn();
    assert_eq!(s.phase(), MusclePhase::Select);
    assert_eq!(s.budget(0), 100);
    assert!(s.queue(0).is_empty());
}

#[test]
fn a_turn_plays_each_string_whole_not_interleaved() {
    // Player queues two commands, the opponent one. Interleaved play
    // would order them p0, o0, p1; a real turn is p0, p1, o0.
    let mut s = MuscleDomeSession::new(
        hand([1, 1, 1, 1]),
        hand([1, 0xFFFF, 0xFFFF, 0xFFFF]),
        [2, 1],
        [500, 500],
        0,
    );
    s.commit_card(0, 0);
    s.commit_card(0, 3);
    s.ai_commit_all(1);
    assert_eq!(s.queue(0), &[0x0C, 0x0D]);
    assert_eq!(s.queue(1), &[0x0C]);
    s.end_selection();
    let mut order = Vec::new();
    s.resolve_turn(|attacker, cmd| {
        order.push((attacker, cmd));
        1
    });
    assert_eq!(order, vec![(0, 0x0C), (0, 0x0D), (1, 0x0C)]);
}

#[test]
fn dome_leg_runs_past_four_turns_and_ends_only_on_a_ko() {
    let mut s = session();
    // The opponent has 400 HP; 40 a turn needs ten turns to drop it. A
    // four-turn bound would have ended this leg at turn 4 with the
    // opponent still standing on 240 HP.
    for turn in 1..=10 {
        assert_eq!(s.phase(), MusclePhase::Select, "turn {turn} is playable");
        assert!(!s.decided(), "turn {turn}: nobody has dropped yet");
        s.commit_card(0, 0);
        s.end_selection();
        s.resolve_turn(|attacker, _| if attacker == 0 { 40 } else { 0 });
        assert_eq!(s.turn(), turn);
        if turn < 10 {
            assert_eq!(
                s.phase(),
                MusclePhase::TurnOver,
                "turn {turn}: the leg continues"
            );
            s.next_turn();
        }
    }
    // Turn 10 lands the KO - the only thing that ends a leg.
    assert_eq!(s.hp(1), 0);
    assert_eq!(s.phase(), MusclePhase::Won);
    assert!(s.decided());
}

#[test]
fn dome_turns_left_is_korus_hud_not_a_dome_rule() {
    // The strip's arithmetic is still decoded - as a free function keyed
    // on the battle turn counter, reachable only by the fight whose
    // formation slot 0 is the timed-fight monster id.
    assert_eq!(timed_fight_turns_left(0), 4);
    assert_eq!(timed_fight_turns_left(3), 1);
    assert_eq!(timed_fight_turns_left(4), 0);
    assert_eq!(timed_fight_turns_left(99), 0, "floored, not wrapped");
    // And the dome's own ladder can never reach that fight.
    assert_eq!(TIMED_FIGHT_MONSTER_ID, 0xB6);
}

#[test]
fn retail_kernel_is_the_shared_resolution_path() {
    // No model installed: the retail path declines rather than inventing
    // a damage rule of its own.
    let mut bare = session();
    bare.commit_card(0, 0);
    bare.end_selection();
    assert!(!bare.resolve_turn_retail(), "no model, no resolution");
    assert_eq!(bare.phase(), MusclePhase::Resolve, "phase untouched");

    // With a model installed the same call drives the turn, logging each
    // play in whole-string order and advancing the rand cursor.
    let mut s = session();
    s.install_damage_model(DomeDamageModel::new(
        Vec::new(),
        [0u8; move_power::MOVE_ID_INDEX_MAP_LEN],
        None,
        [
            DomeCombatant {
                hp_max: 500,
                int: 60,
                udf: 20,
                ldf: 20,
                element: 0,
            },
            DomeCombatant {
                hp_max: 400,
                int: 50,
                udf: 15,
                ldf: 15,
                element: 0,
            },
        ],
        [500, 400],
        0x1234_5678,
    ));
    let seed_before = s.damage_model().unwrap().rng_seed();
    s.commit_card(0, 0);
    s.commit_card(0, 3);
    s.ai_commit_all(1);
    s.end_selection();
    assert!(s.resolve_turn_retail());
    let plays = s.last_turn_plays();
    assert_eq!(plays.len(), s.queue(0).len() + s.queue(1).len());
    let order: Vec<usize> = plays.iter().map(|p| p.attacker).collect();
    assert_eq!(
        order,
        vec![0, 0, 1, 1],
        "each string plays whole, player first"
    );
    assert_ne!(
        s.damage_model().unwrap().rng_seed(),
        seed_before,
        "the PsyQ rand cursor advanced"
    );
    // The model's HP mirror tracks the session's own HP.
    assert_eq!(plays.last().unwrap().hp_after, [s.hp(0), s.hp(1)]);
    assert_eq!(s.turn(), 1);
}

#[test]
fn a_matched_art_replaces_its_swings_in_the_resolved_queue() {
    use legaia_art::{ActionConstant, Command};
    let mut s = session();
    // Right, Left, Right - three 30-cost cards inside the 100 budget,
    // the shape a two-arrow art overlaps into.
    for card in [3usize, 0, 3] {
        assert!(s.commit_card(0, card), "card {card} fits the budget");
    }
    let raw = s.queue(0).to_vec();
    assert_eq!(
        s.tokenized_queue(0),
        raw,
        "no catalog: the raw direction string is the queue"
    );

    // `Up, Down` is an art. The tokenizer writes the starter over the
    // art's LAST arrow and inserts the constant after it, leaving the
    // leading arrow in place.
    s.install_art_catalog(
        0,
        vec![(
            ActionConstant::from_byte(0x1F).unwrap(),
            vec![Command::Right, Command::Left],
        )],
    );
    let tokens = s.tokenized_queue(0);
    assert!(
        tokens.contains(&ActionConstant::RegularStarter.as_byte()),
        "the art starter is in the queue: {tokens:02x?}"
    );
    assert!(tokens.contains(&0x1F), "the art constant is: {tokens:02x?}");

    // A one-arrow row is refused, and a non-art constant never enters.
    let mut t = session();
    t.install_art_catalog(
        0,
        vec![
            (
                ActionConstant::from_byte(0x1F).unwrap(),
                vec![Command::Right],
            ),
            (
                ActionConstant::RegularStarter,
                vec![Command::Right, Command::Right],
            ),
        ],
    );
    t.commit_card(0, 3);
    t.commit_card(0, 3);
    assert_eq!(
        t.tokenized_queue(0),
        t.queue(0).to_vec(),
        "a one-arrow row and a starter row are both refused"
    );
}

#[test]
fn a_direction_swing_does_not_resolve_at_power_zero() {
    // The four direction ids map to move-power index 0, and the disc
    // ships row 0 as 26 zero bytes - so the table cannot be a swing's
    // power source. The kernel falls back to the melee scalar.
    let map = [0u8; move_power::MOVE_ID_INDEX_MAP_LEN];
    let mut m = DomeDamageModel::new(
        Vec::new(),
        map,
        None,
        [
            DomeCombatant {
                hp_max: 500,
                int: 60,
                udf: 20,
                ldf: 20,
                element: 0,
            },
            DomeCombatant {
                hp_max: 400,
                int: 50,
                udf: 15,
                ldf: 15,
                element: 0,
            },
        ],
        [500, 400],
        0x1234_5678,
    );
    m.begin_turn([500, 400]);
    m.damage(0, 0x0F);
    let play = m.plays().last().copied().expect("one play logged");
    assert_eq!(
        play.power,
        legaia_engine_vm::battle_formulas::command_power_scalar(0x0F) as i32,
        "the swing's tier is the melee scalar, not the empty table row"
    );
    assert!(play.power > 0);
}

#[test]
fn hub_screens_fade_hold_and_fade_at_the_measured_literals() {
    // The intro strip: 32 ticks up at the fast rate, 123 held, 32 down.
    let mut c = HubScreen::intro_card();
    assert_eq!(c.brightness(), 0);
    for _ in 0..32 {
        c.tick(1, 0);
    }
    assert_eq!(c.brightness(), HUB_FADE_FULL, "clamps at the neutral byte");
    assert_eq!(c.stage(), HubScreenStage::Hold);
    // Its hold is NOT skippable - a full pad word does not shorten it.
    for _ in 0..HUB_INTRO_HOLD_TICKS - 1 {
        c.tick(1, 0xFFFF);
    }
    assert_eq!(c.stage(), HubScreenStage::Hold, "123 ticks, no skip");
    c.tick(1, 0);
    assert_eq!(c.stage(), HubScreenStage::FadeOut);
    assert_eq!(
        c.total_ticks(),
        32 + HUB_INTRO_HOLD_TICKS + 32,
        "187 ticks unskipped"
    );

    // The ROUND banner: slow fade-in (64), a 180-tick hold that a pad
    // press ends early.
    let mut b = HubScreen::round_banner();
    for _ in 0..64 {
        b.tick(1, 0);
    }
    assert_eq!(b.stage(), HubScreenStage::Hold);
    b.tick(1, HUB_SKIP_PAD_MASK);
    assert_eq!(b.stage(), HubScreenStage::FadeOut, "the & 0xF4 skip");
    // A pad word with no mask bit does not skip.
    let mut b2 = HubScreen::round_banner();
    for _ in 0..64 {
        b2.tick(1, 0);
    }
    b2.tick(1, !HUB_SKIP_PAD_MASK);
    assert_eq!(b2.stage(), HubScreenStage::Hold);

    // The frame delta scales every step, so a dropped frame halves the
    // tick count rather than stretching the screen.
    let mut d = HubScreen::intro_card();
    for _ in 0..16 {
        d.tick(2, 0);
    }
    assert_eq!(d.brightness(), HUB_FADE_FULL);

    // The opponent card's hold is the short one, and also skippable.
    let mut o = HubScreen::opponent_card();
    while o.stage() == HubScreenStage::FadeIn {
        o.tick(1, 0);
    }
    for _ in 0..HUB_OPPONENT_CARD_HOLD_TICKS - 1 {
        o.tick(1, 0);
    }
    assert_eq!(o.stage(), HubScreenStage::Hold);
    o.tick(1, 0);
    assert_eq!(o.stage(), HubScreenStage::FadeOut);

    // Every screen terminates within its own advertised length.
    for mut e in [
        HubScreen::intro_card(),
        HubScreen::round_banner(),
        HubScreen::opponent_card(),
        HubScreen::interval(HUB_TALLY_ROLL_LEAD_TICKS),
    ] {
        let total = e.total_ticks();
        for _ in 0..total {
            e.tick(1, 0);
        }
        assert!(e.done(), "finished within {total} ticks");
        assert_eq!(e.brightness(), 0);
    }
}

#[test]
fn selection_exhausts_when_no_card_is_affordable() {
    let mut s = session();
    assert!(!s.selection_exhausted(0));
    s.commit_card(0, 0); // 30, budget 70
    s.commit_card(0, 0); // 30, budget 40
    assert!(!s.selection_exhausted(0), "a 30-cost card still fits in 40");
    s.commit_card(0, 0); // 30, budget 10
    assert!(
        s.selection_exhausted(0),
        "cheapest card is 30, budget 10: retail ends the input here"
    );
}

#[test]
fn reset_selection_clears_the_queue_and_refunds_the_budget() {
    let mut s = session();
    s.commit_card(0, 0);
    s.commit_card(0, 1);
    assert_eq!(s.budget(0), 28);
    s.end_selection();
    s.reset_selection(0);
    assert_eq!(s.phase(), MusclePhase::Select);
    assert!(s.queue(0).is_empty());
    assert_eq!(s.budget(0), 100);
    assert_eq!(s.spent(0), 0);
}

#[test]
fn ko_decides_the_contest_and_names_the_reward() {
    let mut s = session();
    s.commit_card(0, 0);
    s.end_selection();
    s.resolve_turn(|attacker, _| if attacker == 0 { 1000 } else { 0 });
    assert_eq!(s.phase(), MusclePhase::Won);
    assert!(s.decided());
    assert_eq!(s.reward_spell_id(), 0x83);
    assert_eq!(s.hp_left(), 0, "the opponent has nothing left");
}

#[test]
fn player_ko_loses() {
    let mut s = session();
    s.ai_commit_all(1);
    s.end_selection();
    // The opponent's string still runs whole after the player's, so a
    // KO lands even though the player queued nothing this turn.
    s.resolve_turn(|attacker, _| if attacker == 1 { 1000 } else { 0 });
    assert_eq!(s.phase(), MusclePhase::Lost);
}

#[test]
fn settlement_halves_the_tally_when_not_continuing() {
    // 801d102c..801d1034: signed /2, rounding toward zero.
    let s = settle_contest(101, false, false, 0, 5, 40, false);
    assert_eq!(s.score, 50);
    assert!(!s.continuing);
    assert!(!s.award_prize);
    let s = settle_contest(-101, false, false, 0, 5, 40, false);
    assert_eq!(s.score, -50, "MIPS srl/addu/sra idiom rounds toward zero");
}

#[test]
fn settlement_adds_the_score_table_cell_on_continue() {
    let s = settle_contest(100, true, false, 0, 5, 40, false);
    assert_eq!(s.score, 140);
    assert!(s.continuing);
    assert!(!s.award_prize, "prize gates on the Master-course final");
}

#[test]
fn contest_over_zeroes_score_and_latch() {
    let s = settle_contest(100, true, true, 2, 13, 40, false);
    assert_eq!(s.score, 0);
    assert!(!s.continuing);
    assert!(!s.award_prize, "dropped latch skips the prize branch");
}

#[test]
fn glide_arrives_snaps_and_deactivates() {
    let mut g = SpriteGlide {
        total: 10,
        elapsed: 8,
        target: (100, 50),
        start: (0, 0),
    };
    // dt >= total - elapsed: snap to target, slot deactivates.
    assert_eq!(g.step(2), GlideStep::Arrived { pos: (100, 50) });
    assert_eq!(g.total, 0);
    assert_eq!(g.step(1), GlideStep::Idle);
}

#[test]
fn glide_eases_linearly_with_signed_division() {
    let mut g = SpriteGlide {
        total: 10,
        elapsed: 0,
        target: (-100, 40),
        start: (0, 0),
    };
    assert_eq!(
        g.step(5),
        GlideStep::Moving {
            pos: (-50, 20),
            remaining: 6
        }
    );
    assert_eq!(g.elapsed, 5);
}

#[test]
fn time_meter_ramps_in_select_phase_and_drains_otherwise() {
    // Ramp clamps at 0xC.
    assert_eq!(time_meter_step(0xB, 3, true, true), (0xC, 0xE));
    // Outside the select phase the same flags drain.
    assert_eq!(time_meter_step(5, 2, false, true).0, 3);
    // Drain floors at zero; empty bar sits at -0x92.
    assert_eq!(time_meter_step(1, 3, true, false), (0, -0x92));
}

// --- the contest layer ------------------------------------------------

/// Synthetic score rows shaped like the ladder's (8 / 8 / 13 populated
/// cells) but not its values - the disc's own numbers are read off the
/// disc by `tests/muscle_contest_real.rs`, which is where they belong.
fn score_rows() -> [ScoreRow; COURSE_COUNT] {
    let mut s = [[0i32; MAX_ROUNDS_PER_COURSE]; COURSE_COUNT];
    let lens = [8usize, 8, 13];
    for (c, row) in s.iter_mut().enumerate() {
        for (r, cell) in row.iter_mut().enumerate().take(lens[c]) {
            *cell = (c as i32 + 1) * 10 + r as i32;
        }
    }
    s
}

fn all_gates() -> ContestFlags {
    ContestFlags {
        course_unlock: [false; 3],
        master_gates: [true; 3],
        prize_awarded: false,
    }
}

fn contest(flags: &ContestFlags) -> DomeContest {
    DomeContest::enter(flags, [8, 8, 13], score_rows())
}

fn cleared(turns: u32) -> LegReport {
    LegReport {
        survived: true,
        outcome: 0,
        turns_taken: turns,
    }
}

#[test]
fn the_sub_id_word_carries_course_and_round_in_its_low_byte() {
    // The three unlock seeds, decoded.
    assert_eq!((cursor_course(0x001), cursor_round(0x001)), (0, 0));
    assert_eq!((cursor_course(0x101), cursor_round(0x101)), (0, 0));
    assert_eq!((cursor_course(0x111), cursor_round(0x111)), (1, 0));
    assert_eq!((cursor_course(0x321), cursor_round(0x321)), (2, 0));
    // A leg advance is +1, and the round walks with it.
    let mut w = 0x321;
    for round in 1..=5 {
        w = cursor_next_leg(w);
        assert_eq!((cursor_course(w), cursor_round(w)), (2, round));
    }
    // The repack rewrites only the low byte - the high bytes are what
    // let 0x321 mean "course 2" and survive a whole contest.
    assert_eq!(cursor_repack(0x321, 2, 5), 0x326);
    assert_eq!(cursor_repack(0x326, 2, 5) & !0xFF, 0x300);
}

#[test]
fn the_entry_word_takes_the_highest_unlocked_course() {
    let mut f = ContestFlags::default();
    assert_eq!(contest_entry_word(&f), CONTEST_ENTRY_WORD_DEFAULT);
    f.course_unlock = [true, false, false];
    assert_eq!(cursor_course(contest_entry_word(&f)), 0);
    f.course_unlock = [true, true, false];
    assert_eq!(cursor_course(contest_entry_word(&f)), 1);
    // Retail tests all three in order and lets the last set one win.
    f.course_unlock = [true, true, true];
    assert_eq!(cursor_course(contest_entry_word(&f)), 2);
    f.course_unlock = [false, false, true];
    assert_eq!(cursor_course(contest_entry_word(&f)), 2);
}

#[test]
fn only_the_master_course_is_story_gated() {
    let mut f = all_gates();
    // Beginner / Expert never clamp, whatever the flags say.
    f.master_gates = [false; 3];
    assert_eq!(course_length(0, 8, 7, &f), 8);
    assert_eq!(course_length(1, 8, 7, &f), 8);
    // Master clamps, but only once the run has reached the threshold -
    // the gate below the round you are on is not consulted.
    assert_eq!(course_length(2, 13, 7, &f), 13, "round 7 is under gate 8");
    assert_eq!(course_length(2, 13, 8, &f), 8);
    f.master_gates = [true, false, false];
    assert_eq!(course_length(2, 13, 8, &f), 13);
    assert_eq!(course_length(2, 13, 11, &f), 11);
    f.master_gates = [true, true, false];
    assert_eq!(course_length(2, 13, 11, &f), 13);
    assert_eq!(course_length(2, 13, 12, &f), 12);
    f.master_gates = [true; 3];
    assert_eq!(course_length(2, 13, 12, &f), 13, "all gates open: full 13");
}

#[test]
fn the_four_lanes_scale_by_max_hp_except_the_money_one() {
    // 500 HP: each lane is `n * 500 / 100` = `n * 5`.
    let r = leg_score_rows(3, 5, 1, 500, 40);
    assert_eq!(r.round_lane, 3 * 2 * 5);
    assert_eq!(r.turns_lane, 5 * 5);
    assert_eq!(r.outcome_lane, LEG_OUTCOME_TABLE[1] * 5);
    assert_eq!(r.score_cell, 40, "the score cell is not scaled");
    // The turns lane caps at 8.
    assert_eq!(leg_score_rows(0, 99, 0, 500, 0).turns_lane, 8 * 5);
    // The outcome index saturates at 3.
    assert_eq!(
        leg_score_rows(0, 0, 9, 500, 0).outcome_lane,
        LEG_OUTCOME_TABLE[3] * 5
    );
    // The three recovery lanes are what the restore state hands back;
    // the money row is not part of it.
    assert_eq!(r.hp_restore(), r.round_lane + r.turns_lane + r.outcome_lane);
}

#[test]
fn a_cleared_leg_advances_the_ladder_scores_and_heals() {
    let f = all_gates();
    let mut c = contest(&f);
    assert_eq!((c.course(), c.round()), (0, 0));
    assert_eq!(c.state(), ContestState::Fight);

    c.finish_leg(cleared(4), 500, &f);
    // The ladder advanced and the cleared leg's cell is row 0 cell 0.
    assert_eq!((c.course(), c.round()), (0, 1));
    assert_eq!(c.state(), ContestState::LegScore);
    let cell0 = score_rows()[0][0];
    assert_eq!(c.rows().score_cell, cell0);
    assert_eq!(c.tally(), 0, "nothing banks until the tally drains");

    assert_eq!(c.advance(), ContestState::Tally);
    assert_eq!(c.advance(), ContestState::Restore);
    assert_eq!(c.tally(), cell0);
    assert!(c.pending_hp_restore() > 0);
    // The restore is capped by max HP.
    assert_eq!(c.take_hp_restore(499, 500), 500);
    assert_eq!(c.pending_hp_restore(), 0);
    assert_eq!(c.advance(), ContestState::Fight);
    assert!(!c.over());
}

#[test]
fn a_cleared_course_pays_the_whole_row_and_a_lost_one_pays_half() {
    let f = all_gates();
    // Run the Beginner course to its end.
    let mut c = contest(&f);
    for _ in 0..8 {
        c.finish_leg(cleared(3), 400, &f);
        if c.over() {
            break;
        }
        c.advance();
        c.advance();
        c.advance();
    }
    assert_eq!(c.round(), 8, "eight legs cleared");
    assert!(c.over());
    assert!(c.continue_latch(), "course run out, party standing");
    let out = c.settle(&f);
    // Cells 1..=7 banked through the tally screen, cell 8 added at
    // settlement: the whole row, which is what the curated table calls
    // the course's reward.
    assert_eq!(out.score, score_rows()[0][..8].iter().sum::<i32>());
    assert!(out.set_continue_flag);
    assert!(!out.set_gave_up_flag);

    // The same run, lost on the last leg: no latch, tally halved.
    let mut c = contest(&f);
    for leg in 0..8 {
        let survived = leg < 7;
        c.finish_leg(
            LegReport {
                survived,
                outcome: 0,
                turns_taken: 3,
            },
            400,
            &f,
        );
        if c.over() {
            break;
        }
        c.advance();
        c.advance();
        c.advance();
    }
    assert!(!c.continue_latch());
    let banked: i32 = score_rows()[0][..7].iter().sum();
    assert_eq!(c.settle(&f).score, banked / 2);
}

#[test]
fn running_voids_the_tally_and_latches_the_courses_own_flag() {
    let f = all_gates();
    let mut c = contest(&f);
    // Bank a leg first, so there is something to void.
    c.finish_leg(cleared(3), 400, &f);
    c.advance();
    c.advance();
    c.advance();
    assert_eq!(c.tally(), score_rows()[0][0]);
    // Now run from the second fight.
    c.finish_leg(
        LegReport {
            survived: true,
            outcome: LEG_OUTCOME_RAN,
            turns_taken: 1,
        },
        400,
        &f,
    );
    assert!(c.gave_up());
    assert!(c.over());
    let out = c.settle(&f);
    assert_eq!(out.score, 0, "a give-up pays nothing");
    assert!(out.set_gave_up_flag);
    // Round 2, not 1: the Muscle Paradise flag only latches on the
    // course's first fight.
    assert_eq!(out.set_ran_first_flag, None);

    // Running from the very first fight does latch it.
    let mut c = contest(&f);
    c.finish_leg(
        LegReport {
            survived: true,
            outcome: LEG_OUTCOME_RAN,
            turns_taken: 1,
        },
        400,
        &f,
    );
    assert_eq!(
        c.settle(&f).set_ran_first_flag,
        Some(COURSE_RAN_FIRST_FLAG_BASE)
    );
}

#[test]
fn a_story_gated_master_course_ends_early_at_its_cap() {
    // No gate flags: the Master course stops at round 8 rather than 13,
    // so the run settles eight legs in with the latch up.
    let f = ContestFlags {
        course_unlock: [false, false, true],
        master_gates: [false; 3],
        prize_awarded: false,
    };
    let mut c = contest(&f);
    assert_eq!(c.course(), 2);
    for _ in 0..13 {
        c.finish_leg(cleared(2), 600, &f);
        if c.over() {
            break;
        }
        c.advance();
        c.advance();
        c.advance();
    }
    assert_eq!(c.round(), 8, "clamped to 8 by the missing 0x378 flag");
    assert!(c.continue_latch());
    let out = c.settle(&f);
    assert!(!out.award_prize, "the prize needs the full 13-round run");
    assert_eq!(out.score, score_rows()[2][..8].iter().sum::<i32>());
}

#[test]
fn the_full_master_run_pays_the_row_sum_and_the_one_shot_prize() {
    let f = ContestFlags {
        course_unlock: [false, false, true],
        master_gates: [true; 3],
        prize_awarded: false,
    };
    let mut c = contest(&f);
    for _ in 0..13 {
        c.finish_leg(cleared(2), 600, &f);
        if c.over() {
            break;
        }
        c.advance();
        c.advance();
        c.advance();
    }
    assert_eq!(c.round(), 13);
    let out = c.settle(&f);
    assert!(out.award_prize);
    let row_sum: i32 = score_rows()[2][..13].iter().sum();
    assert_eq!(out.score, row_sum, "the whole row, every cell once");
    // Settlement is idempotent: the second call cannot pay twice.
    let again = c.settle(&f);
    assert_eq!(again.score, row_sum);
    assert!(!again.award_prize);
}

#[test]
fn the_coin_credit_saturates_at_the_bank_ceiling() {
    assert_eq!(credit_casino_coins(0, 818), 818);
    assert_eq!(credit_casino_coins(100, 13830), 13930);
    assert_eq!(
        credit_casino_coins(COIN_BANK_MAX as u32, 1),
        COIN_BANK_MAX as u32
    );
    assert_eq!(credit_casino_coins(9_999_990, 100), COIN_BANK_MAX as u32);
    // The port's own lower clamp: retail's bank is a signed word, the
    // engine's is unsigned.
    assert_eq!(credit_casino_coins(10, -100), 0);
}

#[test]
fn the_zero_damage_fallback_closes_the_turn_instead_of_hanging() {
    let mut s = session();
    s.commit_card(0, 0);
    s.end_selection();
    assert_eq!(s.phase(), MusclePhase::Resolve);
    // No model installed: the shared path still moves the turn on, which
    // is the difference between a degraded contest and a hang.
    assert!(!s.resolve_turn_or_zero(), "the retail kernel did not drive");
    assert_eq!(s.phase(), MusclePhase::TurnOver);
    assert_eq!(s.turn(), 1);
    assert_eq!(s.last_turn_damage(), [0, 0]);
}

#[test]
fn prize_awards_once_at_the_master_course_final() {
    let s = settle_contest(100, true, false, 2, 13, 40, false);
    assert!(s.award_prize);
    assert_eq!(s.score, 140);
    // One-shot: the 0x6CB flag suppresses the re-award.
    let s = settle_contest(100, true, false, 2, 13, 40, true);
    assert!(!s.award_prize);
}
