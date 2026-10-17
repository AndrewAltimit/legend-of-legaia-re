//! Vera, Orb and Spoon: the restore arm, the level-nine AP doubling and the
//! module heal.

use super::*;

/// PROT 0905's restore arm must not be a **second** HP owner.
///
/// The engine folds a cast's HP outcome exactly once, and that fold already
/// routes this module's own magnitude in (`World::seru_tick_heal_amount`).
/// The driven seam re-enters the module every frame, so an arm that also
/// stored would restore twice per cast and more if the phase lingered. The
/// arm keeps its cure sweep - that half has no second owner.
#[test]
fn the_vera_restore_arm_leaves_hp_to_the_fold_and_still_cures() {
    use legaia_engine_cast::seru_magic_ticks::{
        VERA_CURE_MASKS, VERA_CURE_MIN_LEVEL, VERA_RESTORE_ARM,
    };
    let mut world = module_code_world();
    assert_eq!(world.cast_module_for(0x83), Some(905));
    world.casting.summon_actor_slot = Some(7);
    // A hurt, status-carrying ally in the victim seat.
    world.battle_ctx.active_actor = 0;
    world.actors[0].battle.hp = 40;
    world.actors[0].battle.max_hp = 1000;
    world.actors[0].battle.field_flags = 0x0003;
    // A cure tier the arm will act on, and a caster whose record carries the
    // spell at a level past the cure floor (the arm reads the record, not the
    // actor).
    world.battle_ctx.follow_up_pending = 1;
    let mut member = legaia_save::CharacterRecord::parse(&[0u8; 0x414]).expect("blank record");
    let mut list = member.spell_list();
    list.count = 1;
    list.ids[0] = 0x83;
    list.levels[0] = VERA_CURE_MIN_LEVEL;
    member.set_spell_list(list);
    world.party.roster.members = vec![member];

    // Each arm is re-entered until it lets the phase through: PROT 0905's
    // counted arms hold on the module's own countdown
    // (`legaia_engine_cast::cast_module_camera`), so one call per arm would
    // stop at the first gate.
    let mut ran_restore = false;
    for arm in 0..=VERA_RESTORE_ARM {
        world.casting.module_phase = arm;
        for _ in 0..2048 {
            if world.run_cast_module_code(0x83, arm).is_none() {
                break;
            }
            if world.casting.module_phase != arm {
                break;
            }
        }
        ran_restore |= arm == VERA_RESTORE_ARM && world.casting.module_phase != arm;
    }
    assert!(ran_restore, "the restore arm has to have executed");
    assert_eq!(
        world.actors[0].battle.hp, 40,
        "the module tick is not an HP owner - the fold is"
    );
    assert_eq!(
        world.actors[0].battle.field_flags,
        0x0003 & VERA_CURE_MASKS[0],
        "the cure half still runs"
    );
}

/// A level-9 Vera (cure tier 4, the light row's top band) doubles the
/// target's battle AP gauge `+0x170`, capped at 100 - PROT 0905
/// `0x801F7F24..0x801F7F48`. First reported by the_rabidsquirel from retail
/// save-state testing; a lower tier leaves AP alone.
#[test]
fn a_level_nine_vera_doubles_the_targets_ap() {
    use legaia_engine_cast::seru_magic_ticks::VERA_RESTORE_ARM;
    for (tier, before, after) in [(4u8, 17u16, 34u16), (4, 60, 100), (3, 17, 17)] {
        let mut world = module_code_world();
        world.casting.summon_actor_slot = Some(7);
        world.battle_ctx.active_actor = 0;
        world.actors[0].battle.spirit_gauge = before;
        world.battle_ctx.follow_up_pending = tier;
        roster_with_spell(&mut world, 0x83, 9);
        drive_module_through(&mut world, 0x83, VERA_RESTORE_ARM);
        assert_eq!(
            world.actors[0].battle.spirit_gauge, after,
            "tier {tier}, AP {before}"
        );
    }
}

/// A level-9 Orb doubles every living party seat's AP gauge the same way -
/// PROT 0911 `0x801F7E10..0x801F7E3C`.
#[test]
fn a_level_nine_orb_doubles_the_party_ap() {
    use legaia_engine_cast::seru_magic_ticks::ORB_ARM5_TARGET_PHASE;
    let mut world = module_code_world();
    assert_eq!(world.cast_module_for(0x89), Some(911));
    world.casting.summon_actor_slot = Some(7);
    world.battle_ctx.active_actor = 0;
    world.actors[0].battle.spirit_gauge = 26;
    world.battle_ctx.follow_up_pending = 4;
    roster_with_spell(&mut world, 0x89, 9);
    drive_module_through(&mut world, 0x89, ORB_ARM5_TARGET_PHASE);
    assert_eq!(world.actors[0].battle.spirit_gauge, 52);
}

/// A level-9 Spoon doubles the party's AP too - PROT 0919 arm 7
/// (`0x801F8394..0x801F83C0`) - once, as the arm lets the phase through, and
/// cures nothing below tier 4's selector beyond its mask.
#[test]
fn a_level_nine_spoon_doubles_the_party_ap_once() {
    use legaia_engine_cast::seru_magic_ticks::SPOON_HEAL_ARM;
    let mut world = module_code_world();
    assert_eq!(world.cast_module_for(0x91), Some(919));
    world.casting.summon_actor_slot = Some(7);
    world.battle_ctx.active_actor = 0;
    world.actors[0].battle.spirit_gauge = 9;
    world.actors[0].battle.field_flags = 0x0403;
    world.battle_ctx.follow_up_pending = 4;
    roster_with_spell(&mut world, 0x91, 9);
    drive_module_through(&mut world, 0x91, SPOON_HEAL_ARM);
    assert_ne!(world.casting.module_phase, SPOON_HEAL_ARM, "arm 7 ran");
    assert_eq!(world.actors[0].battle.spirit_gauge, 18);
    assert_eq!(world.actors[0].battle.field_flags, 0, "tier 4 mask 0xFB84");
    // A later frame past the arm does not double again.
    let phase = world.casting.module_phase;
    let _ = world.run_cast_module_code(0x91, phase);
    assert_eq!(world.actors[0].battle.spirit_gauge, 18);

    // Below the level gate the ladder never runs.
    let mut world = module_code_world();
    world.casting.summon_actor_slot = Some(7);
    world.actors[0].battle.spirit_gauge = 9;
    world.battle_ctx.follow_up_pending = 4;
    roster_with_spell(&mut world, 0x91, 2);
    drive_module_through(&mut world, 0x91, SPOON_HEAL_ARM);
    assert_eq!(world.actors[0].battle.spirit_gauge, 9);
}

/// The evolved block now folds like the base block: a Spoon cast restores
/// PROT 0919's own `(level << 7) + 0x380` (not the catalog placeholder) and
/// credits the module's `+4` XP for a seat whose missing HP covered it
/// (`0x801F80D4`).
#[test]
fn a_spoon_cast_heals_its_module_amount_and_trains_the_spell() {
    use legaia_engine_battle::spells::{SpellDef, SpellEffect, SpellTarget};
    let mut world = module_code_world();
    world.actors[0].battle.hp = 1;
    world.actors[0].battle.max_hp = 0x500;
    roster_with_spell(&mut world, 0x91, 1);
    let def = SpellDef {
        id: 0x91,
        name: "Spoon".into(),
        mp_cost: 0,
        target: SpellTarget::AllAllies,
        effect: SpellEffect::HealAll { amount: 7 },
        ..SpellDef::default()
    };
    assert!(world.cast_spell_on_slots_prepaid(0, &def, &[0]));
    assert_eq!(
        world.actors[0].battle.hp,
        1 + 0x400,
        "level 1: 0x80 + 0x380"
    );
    let rec = &world.party.roster.members[0];
    let slot = legaia_engine_battle::magic_xp::spell_slot(rec, 0x91).expect("learned");
    assert_eq!(legaia_engine_battle::magic_xp::spell_xp(rec, slot), 4);
}
