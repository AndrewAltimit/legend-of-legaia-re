//! The two **gates** the retail queue builder and damage kernel read off
//! character / monster records, and the queue passes that depend on them.
//!
//! 1. The Miracle marker `ctx[+0x25F + slot]`. Retail raises it once per
//!    battle, in the party battle-actor seeding routine `FUN_80053CB8`
//!    (`sb v1,0x25f(v0)` at `0x80054270`), from one equipment byte of the
//!    acting character's record - **not** from anything the player types.
//!    With the marker clear the builder's Miracle arm writes nothing, however
//!    exactly the entered string matches.
//! 2. The monster record's `+0x1E` swing class. A class-`2` target is struck
//!    with one low swing instead of two rolled arm swings
//!    (`FUN_801EED1C`, `lbu v1,0x1e(v0)` at `0x801EEFC8`), and the same byte
//!    is the damage kernel's apply-mode look-ahead input.

use super::*;

/// A one-party-member battle world with `member`'s record installed, ready
/// for an arts commit.
fn arts_world(equip: [u8; 8]) -> World {
    let mut w = World::new();
    while w.actors.len() < 4 {
        w.actors.push(Actor::default());
    }
    w.party_count = 1;
    let mut party = legaia_save::Party::zeroed(1);
    let mut eq = party.members[0].equipment();
    eq.slots = equip;
    party.members[0].set_equipment(eq);
    w.load_party(party);
    w.mode = SceneMode::Battle;
    for i in 0..4 {
        w.actors[i].active = true;
        w.actors[i].battle.hp = 500;
        w.actors[i].battle.max_hp = 500;
        w.actors[i].battle.liveness = 1;
    }
    w
}

/// Vahn's Miracle command string, as the shipped table carries it.
fn vahn_miracle() -> Vec<legaia_art::Command> {
    legaia_art::MIRACLE_ARTS
        .iter()
        .find(|m| m.character == legaia_art::Character::Vahn)
        .expect("Vahn has a Miracle row")
        .commands
        .to_vec()
}

/// The Miracle row's own bytes, in the order the resident row stores them
/// (the MSB quirk stripped, which is what `clear_queue_msb` does).
fn vahn_miracle_queue() -> Vec<u8> {
    legaia_art::MIRACLE_ARTS
        .iter()
        .find(|m| m.character == legaia_art::Character::Vahn)
        .expect("Vahn has a Miracle row")
        .replacement
        .iter()
        .map(|a| a.as_byte())
        .collect()
}

#[test]
fn an_empty_ra_seru_slot_leaves_the_miracle_string_unreplaced() {
    // Equipment slot 3 clear: retail's `lbu v0,0x761` reads zero, the marker
    // stays clear and the builder's Miracle arm is never entered.
    let mut w = arts_world([0u8; 8]);
    assert!(
        !w.miracle_marker_armed_for(0),
        "an empty Ra-Seru slot must not arm the marker"
    );
    let (queue, _) = w.build_arts_action_queue(0, &vahn_miracle());
    let row = vahn_miracle_queue();
    assert_ne!(
        &queue[..row.len()],
        &row[..],
        "with the marker clear the queue must NOT become the Miracle row"
    );
}

#[test]
fn an_occupied_ra_seru_slot_arms_the_marker_and_the_row_lands() {
    let mut equip = [0u8; 8];
    equip[3] = 0x40;
    let mut w = arts_world(equip);
    assert!(w.miracle_marker_armed_for(0));
    let (queue, _) = w.build_arts_action_queue(0, &vahn_miracle());
    let row = vahn_miracle_queue();
    assert_eq!(
        &queue[..row.len()],
        &row[..],
        "the armed marker plus the matching string is retail's Miracle arm"
    );
}

#[test]
fn roster_id_two_reads_the_other_slot_of_the_pair() {
    // `_DAT_8007B42C` puts Noa's weapon at index 3, so her Ra-Seru byte is
    // index 2 - and retail's `beq v0,a3` arm reads exactly that.
    let mut only_two = [0u8; 8];
    only_two[2] = 0x40;
    let mut only_three = [0u8; 8];
    only_three[3] = 0x40;
    assert!(
        vm::battle_action::miracle_marker_armed(2, &only_two),
        "id 2 reads equipment slot 2"
    );
    assert!(!vm::battle_action::miracle_marker_armed(2, &only_three));
    assert!(vm::battle_action::miracle_marker_armed(1, &only_three));
    assert!(!vm::battle_action::miracle_marker_armed(1, &only_two));
    assert!(vm::battle_action::miracle_marker_armed(3, &only_three));
}

#[test]
fn the_swing_class_reaches_the_no_input_attack_queue() {
    use crate::monster_catalog::{MonsterCatalog, MonsterDef};
    let mut w = arts_world([0u8; 8]);
    let mut cat = MonsterCatalog::new();
    let mut low = MonsterDef::new(7, "Low", 100, 10);
    low.swing_class = vm::battle_action::LOW_SWING_TARGET_CLASS;
    cat.insert(low);
    let mut tall = MonsterDef::new(8, "Tall", 100, 10);
    tall.swing_class = 0;
    cat.insert(tall);
    w.set_monster_catalog(cat);

    w.actors[1].battle_monster_id = Some(7);
    w.actors[2].battle_monster_id = Some(8);
    assert_eq!(
        w.attack_swing_class_of(1),
        vm::battle_action::LOW_SWING_TARGET_CLASS
    );
    assert_eq!(w.attack_swing_class_of(2), 0);
    // A party slot has no record-pointer-table row in retail; the port
    // answers with the ordinary class.
    assert_eq!(w.attack_swing_class_of(0), 0);

    // ...and the queue the builder writes follows: one low swing for the
    // class-2 target, two rolled arm swings for the other.
    let n = w.seed_basic_attack_queue(0, 1);
    assert_eq!(n, 1);
    assert_eq!(w.actors[0].battle.params[0], vm::battle_action::SWING_LOW);
    let n = w.seed_basic_attack_queue(0, 2);
    assert_eq!(n, vm::battle_action::BASIC_ATTACK_SWINGS);
    for b in &w.actors[0].battle.params[..2] {
        assert!(
            vm::battle_action::is_swing_command(*b) && *b != vm::battle_action::SWING_LOW,
            "arm swing expected, got {b:#04x}"
        );
    }
}

#[test]
fn the_apply_mode_early_arm_fires_at_the_hit_ticker_seat() {
    use crate::monster_catalog::{MonsterCatalog, MonsterDef};
    let mut w = arts_world([0u8; 8]);
    // Party width 1, so slots 1..3 are monster seats.
    let mut cat = MonsterCatalog::new();
    let mut tall = MonsterDef::new(9, "Tall", 100, 10);
    // Class 3: only power bytes 0x11..=0x15 can connect with it.
    tall.swing_class = vm::battle_action::MISS_CLASS_HIGH;
    cat.insert(tall);
    let mut ordinary = MonsterDef::new(10, "Ordinary", 100, 10);
    ordinary.swing_class = 0;
    cat.insert(ordinary);
    w.set_monster_catalog(cat);
    w.actors[1].battle_monster_id = Some(9);
    w.actors[2].battle_monster_id = Some(10);
    // Attacker: party slot 0, cursor parked (so the look-ahead's stream walk
    // is skipped, exactly as retail's `0xFF - 1` bound does).
    w.actors[0].battle.strike_index = vm::battle_action::STRIKE_CURSOR_PARKED;

    // Every remaining power byte is class LOW - nothing left can connect with
    // a class-3 target, so the total lands on this hit.
    let low_run = [0x05u8, 0x05, 0, 0];
    assert_eq!(
        w.hit_apply_mode(0, 1, &low_run, 0),
        vm::battle_action::APPLY_MODE_EARLY
    );
    // A class-HIGH byte still ahead keeps it on the ordinary arm...
    let mixed = [0x05u8, 0x12, 0, 0];
    assert_eq!(
        w.hit_apply_mode(0, 1, &mixed, 0),
        vm::battle_action::APPLY_MODE_NORMAL
    );
    // ...and an ordinary-class target is never on the early arm.
    assert_eq!(
        w.hit_apply_mode(0, 2, &low_run, 0),
        vm::battle_action::APPLY_MODE_NORMAL
    );
}

#[test]
fn the_war_god_carry_arm_fires_from_the_attackers_own_ability_word() {
    use crate::monster_catalog::{MonsterCatalog, MonsterDef};
    let mut w = arts_world([0u8; 8]);
    let mut cat = MonsterCatalog::new();
    cat.insert(MonsterDef::new(11, "Ordinary", 100, 10));
    w.set_monster_catalog(cat);
    w.actors[1].battle_monster_id = Some(11);
    w.actors[0].battle.strike_index = vm::battle_action::STRIKE_CURSOR_PARKED;
    let run = [0x05u8, 0, 0, 0];

    assert_eq!(
        w.hit_apply_mode(0, 1, &run, 0),
        vm::battle_action::APPLY_MODE_NORMAL
    );
    w.character_ability_bits[0] = vm::battle_action::WAR_GOD_ATTACK_X2_BIT;
    assert_eq!(
        w.hit_apply_mode(0, 1, &run, 0),
        vm::battle_action::APPLY_MODE_CARRY,
        "first pass of the Attack x2 pair applies nothing"
    );
    w.battle_ctx.attack_x2_pass = 1;
    assert_eq!(
        w.hit_apply_mode(0, 1, &run, 0),
        vm::battle_action::APPLY_MODE_CARRY,
        "the second pass is still under the bound"
    );
    w.battle_ctx.attack_x2_pass = vm::battle_action::ATTACK_X2_PASS_BOUND;
    assert_eq!(
        w.hit_apply_mode(0, 1, &run, 0),
        vm::battle_action::APPLY_MODE_NORMAL
    );
}

#[test]
fn a_doubled_art_keeps_the_learn_verdict_on_its_first_performance() {
    // The builder walks tail-first, so `FUN_801EFBFC` sees the *last*
    // occurrence of a repeated art first and marks it `0x1A`; the
    // marked-starter reorder (`0x801EF8A0`) then swaps it with the `0x19`
    // starter of the same art earlier in the queue. Net: the learn verdict
    // lands on the art's first performance of the turn.
    use legaia_art::{ActionConstant, Character, Command};
    let mut w = arts_world([0u8; 8]);
    // One two-arrow art, entered twice.
    let art = ActionConstant::from_byte(0x1F).expect("art constant");
    let combo = vec![Command::Up, Command::Down];
    w.set_art_record(
        Character::Vahn,
        art,
        legaia_art::ArtRecord {
            action: art,
            commands: combo.clone(),
            anim_index: 0,
            anim_extra: vec![],
            name: None,
            power: vec![],
            dmg_timing: vec![],
            effect_cues: Default::default(),
            hit_cues: vec![],
            identifier: 0,
            anim_speed: 0,
            enemy_effect: legaia_art::EnemyEffect::default(),
            repeat_frames: Default::default(),
            background: 0,
            runtime_address: None,
        },
    );
    let mut input = combo.clone();
    input.extend(combo.clone());
    let (queue, _) = w.build_arts_action_queue(0, &input);
    let starters: Vec<(usize, u8)> = queue
        .iter()
        .enumerate()
        .filter(|(_, b)| **b == 0x19 || **b == 0x1A)
        .map(|(i, b)| (i, *b))
        .collect();
    assert_eq!(starters.len(), 2, "two starters expected: {queue:02X?}");
    assert_eq!(
        starters[0].1, 0x1A,
        "the newly-learned starter belongs on the first performance: {queue:02X?}"
    );
    assert_eq!(starters[1].1, 0x19, "{queue:02X?}");
}

#[test]
fn a_party_target_never_leaves_the_ordinary_apply_arm() {
    // Retail gates both copies of the kernel on `target >= 3` before the
    // look-ahead and before the War God arm, so a counter / confused hit on
    // an ally is always the ordinary arm.
    let mut w = arts_world([0u8; 8]);
    w.party_count = 3;
    w.character_ability_bits[0] = vm::battle_action::WAR_GOD_ATTACK_X2_BIT;
    w.actors[0].battle.strike_index = vm::battle_action::STRIKE_CURSOR_PARKED;
    assert_eq!(
        w.hit_apply_mode(0, 1, &[0x05, 0, 0, 0], 0),
        vm::battle_action::APPLY_MODE_NORMAL
    );
}
