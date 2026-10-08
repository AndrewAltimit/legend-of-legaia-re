use super::*;
use crate::battle_anim::MonsterAnimPlayer;
use legaia_asset::monster_archive::{MonsterAnimation, PartPose};

/// A one-part, two-frame clip whose entry head carries `impact_class`.
fn clip(impact_class: u8) -> MonsterAnimation {
    MonsterAnimation {
        action_id: 0xC,
        rate: 2,
        attach_key: 0,
        solo_flag: 0,
        impact_class,
        effect_script: Vec::new(),
        part_count: 1,
        frame_count: 2,
        frames: vec![vec![PartPose::default()], vec![PartPose::default()]],
    }
}

/// A battle with one party member and one monster, both alive, the
/// attacker playing `clip(class)`.
fn duel_with_attacker_clip(attacker: usize, class: u8) -> World {
    let mut w = World::new();
    w.enter_battle(1, 1);
    for i in 0..2 {
        w.actors[i].battle.liveness = 1;
        w.actors[i].battle.hp = 500;
        w.actors[i].battle.max_hp = 500;
    }
    w.set_battle_attack(0, 80);
    w.set_battle_attack(1, 80);
    w.actors[0].battle.active_target = 1;
    w.actors[1].battle.active_target = 0;
    w.actors[attacker].battle_animation = MonsterAnimPlayer::new(&clip(class));
    w.battle_ctx.active_actor = attacker as u8;
    w
}

/// A connecting swing stamps the retail triple on the STRUCK actor -
/// `+0x21F = class`, `+0x0C = 0x1000` - from the attacker's committed
/// record `+0x7A` (`FUN_801EC3E4` `0x801EE3D4..0x801EE43C`). No disc
/// impact table is installed here, so the colour word is the one
/// write with nothing to carry.
#[test]
fn a_connecting_swing_arms_the_impact_triple_from_the_attackers_clip() {
    let mut w = duel_with_attacker_clip(0, 1);
    {
        let atk = w.battle_ctx.active_actor;
        w.land_melee_hit(atk, 1 - atk, BASIC_ATTACK_COMMAND, 0, false, true);
    }
    assert_eq!(w.actors[1].battle.impact_state, 1);
    assert_eq!(
        w.actors[1].battle.render_blend,
        legaia_engine_vm::battle_formulas::TINT_BLEND_FULL
    );
    assert_eq!(
        w.actors[0].battle.impact_state, 0,
        "the attacker is untouched"
    );
}

/// The monster's basic swing goes through the same routine with its
/// archive entry as the record.
#[test]
fn a_monster_swing_arms_the_party_target_the_same_way() {
    let mut w = duel_with_attacker_clip(1, 2);
    {
        let atk = w.battle_ctx.active_actor;
        w.land_melee_hit(atk, 1 - atk, BASIC_ATTACK_COMMAND, 0, false, true);
    }
    assert_eq!(w.actors[0].battle.impact_state, 2);
    assert_eq!(w.actors[0].battle.render_blend, 0x1000);
}

/// Class `0` and a class past the table (`sltiu v0,v0,0x6` at
/// `0x801EE3E0`) arm nothing - the swing still lands.
#[test]
fn class_zero_and_out_of_table_classes_arm_nothing() {
    for class in [0u8, crate::move_power::IMPACT_CLASS_LIMIT, 0xFF] {
        let mut w = duel_with_attacker_clip(0, class);
        let hp_before = w.actors[1].battle.hp;
        w.land_melee_hit(0, 1, BASIC_ATTACK_COMMAND, 0, false, true);
        // Hits accumulate; the combo total is the one live-HP write.
        w.apply_combo_total(1);
        assert!(
            w.actors[1].battle.hp < hp_before,
            "class {class}: the swing landed"
        );
        assert_eq!(w.actors[1].battle.impact_state, 0, "class {class}");
        assert_eq!(w.actors[1].battle.render_blend, 0, "class {class}");
    }
}

/// With no clip playing (a synthetic battle) the class reads `0`.
#[test]
fn no_playing_clip_reads_class_zero() {
    let mut w = duel_with_attacker_clip(0, 3);
    w.actors[0].battle_animation = None;
    {
        let atk = w.battle_ctx.active_actor;
        w.land_melee_hit(atk, 1 - atk, BASIC_ATTACK_COMMAND, 0, false, true);
    }
    assert_eq!(w.actors[1].battle.impact_state, 0);
}
