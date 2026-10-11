use super::*;

fn alive_enemy(mag_def: u16, hp: u16) -> SpellSnapshot {
    SpellSnapshot {
        caster_mag: 100,
        caster_hp: 100,
        caster_max_hp: 100,
        caster_mp: 50,
        caster_ability_bits: 0,
        target_mdef: mag_def,
        target_hp: hp,
        target_hp_max: hp,
        target_mp: 0,
        target_alive: hp > 0,
        target_weakness: ElementMask::empty(),
    }
}

fn vanilla_spell(id: u8) -> SpellDef {
    SpellCatalog::vanilla().get(id).cloned().unwrap()
}

#[test]
fn vanilla_catalog_has_expected_count() {
    let cat = SpellCatalog::vanilla();
    // 5 healing + 7 damage + 4 buff + 2 utility = 18.
    assert!(cat.len() >= 16, "vanilla catalog has {} spells", cat.len());
}

#[test]
fn mp_cost_returns_zero_for_unknown_id() {
    let cat = SpellCatalog::vanilla();
    assert_eq!(cat.mp_cost(0xFF), 0);
    assert_eq!(cat.mp_cost(0x10), 4);
}

#[test]
fn is_capture_marks_only_capture_spells() {
    let cat = SpellCatalog::vanilla();
    assert!(cat.is_capture(0x40));
    assert!(!cat.is_capture(0x20));
    assert!(!cat.is_capture(0x10));
}

#[test]
fn cast_damage_spell_fails_when_target_dead() {
    let snap = alive_enemy(10, 0);
    let s = vanilla_spell(0x20); // Flame
    let outcome = cast_spell(&s, 3, &snap);
    assert!(matches!(
        outcome,
        SpellOutcome::Failed {
            reason: SpellFailReason::DeadTarget
        }
    ));
}

#[test]
fn cast_damage_spell_fails_when_caster_low_mp() {
    let mut snap = alive_enemy(0, 100);
    snap.caster_mp = 0;
    let s = vanilla_spell(0x20);
    let outcome = cast_spell(&s, 3, &snap);
    assert!(matches!(
        outcome,
        SpellOutcome::Failed {
            reason: SpellFailReason::NotEnoughMp
        }
    ));
}

#[test]
fn cast_damage_spell_returns_positive_amount() {
    let snap = alive_enemy(5, 100);
    let s = vanilla_spell(0x20); // Flame, base 80, fire
    let outcome = cast_spell(&s, 3, &snap);
    match outcome {
        SpellOutcome::Damage {
            target,
            amount,
            element,
            weakness,
        } => {
            assert_eq!(target, 3);
            // 100 * 80 / 8 - 5 = 995. Clamped to u16 (995).
            assert_eq!(amount, 995);
            assert_eq!(element, SpellElement::Fire);
            assert!(!weakness);
        }
        _ => panic!("expected damage outcome"),
    }
}

#[test]
fn cast_damage_spell_respects_element_weakness() {
    let mut snap = alive_enemy(5, 100);
    snap.target_weakness = ElementMask::empty().with(SpellElement::Fire);
    let s = vanilla_spell(0x20);
    let outcome = cast_spell(&s, 3, &snap);
    match outcome {
        SpellOutcome::Damage {
            amount, weakness, ..
        } => {
            // 1.5x multiplier means roughly 1492 damage.
            assert!(amount > 1400 && amount < 1600);
            assert!(weakness);
        }
        _ => panic!("expected damage outcome"),
    }
}

#[test]
fn cast_damage_spell_clamps_floor_at_one() {
    // Target has more defense than caster has magic - formula goes
    // negative; the floor clamps it to 1.
    let snap = SpellSnapshot {
        caster_mag: 1,
        caster_mp: 50,
        target_mdef: 9999,
        target_hp: 100,
        target_hp_max: 100,
        target_alive: true,
        ..Default::default()
    };
    let s = vanilla_spell(0x20);
    let outcome = cast_spell(&s, 3, &snap);
    match outcome {
        SpellOutcome::Damage { amount, .. } => assert_eq!(amount, 1),
        _ => panic!("expected damage outcome"),
    }
}

#[test]
fn cast_heal_spell_caps_at_hp_max() {
    // HP at 95, max 100; Heal would have given 60 but only 5 effective.
    let snap = SpellSnapshot {
        caster_mp: 50,
        target_alive: true,
        target_hp: 95,
        target_hp_max: 100,
        ..Default::default()
    };
    let s = vanilla_spell(0x10); // Heal
    let outcome = cast_spell(&s, 1, &snap);
    match outcome {
        SpellOutcome::Heal { target, amount } => {
            assert_eq!(target, 1);
            assert_eq!(amount, 5);
        }
        _ => panic!("expected heal outcome"),
    }
}

#[test]
fn cast_heal_spell_returns_no_change_when_full_hp() {
    let snap = SpellSnapshot {
        caster_mp: 50,
        target_alive: true,
        target_hp: 100,
        target_hp_max: 100,
        ..Default::default()
    };
    let s = vanilla_spell(0x10);
    let outcome = cast_spell(&s, 1, &snap);
    assert!(matches!(
        outcome,
        SpellOutcome::Failed {
            reason: SpellFailReason::NoChange
        }
    ));
}

#[test]
fn cast_revive_returns_no_change_for_alive_target() {
    let snap = SpellSnapshot {
        caster_mp: 50,
        target_alive: true,
        target_hp: 50,
        target_hp_max: 100,
        ..Default::default()
    };
    let s = vanilla_spell(0x14);
    let outcome = cast_spell(&s, 1, &snap);
    assert!(matches!(
        outcome,
        SpellOutcome::Failed {
            reason: SpellFailReason::NoChange
        }
    ));
}

#[test]
fn cast_revive_returns_correct_hp_for_dead_target() {
    let snap = SpellSnapshot {
        caster_mp: 50,
        target_alive: false,
        target_hp: 0,
        target_hp_max: 100,
        ..Default::default()
    };
    let s = vanilla_spell(0x14); // Vital - hp_pct: 50
    let outcome = cast_spell(&s, 1, &snap);
    match outcome {
        SpellOutcome::Revive { target, hp } => {
            assert_eq!(target, 1);
            assert_eq!(hp, 50);
        }
        _ => panic!("expected revive outcome"),
    }
}

#[test]
fn cast_buff_spell_returns_buff_outcome() {
    let snap = SpellSnapshot {
        caster_mp: 50,
        target_alive: true,
        ..Default::default()
    };
    let s = vanilla_spell(0x30); // Power Up
    let outcome = cast_spell(&s, 0, &snap);
    match outcome {
        SpellOutcome::Buff {
            target,
            stat,
            magnitude,
            turns,
        } => {
            assert_eq!(target, 0);
            assert_eq!(stat, BuffStat::Attack);
            assert_eq!(magnitude, 20);
            assert_eq!(turns, 4);
        }
        _ => panic!("expected buff outcome"),
    }
}

#[test]
fn cast_debuff_returns_negative_magnitude() {
    let snap = SpellSnapshot {
        caster_mp: 50,
        target_alive: true,
        ..Default::default()
    };
    let s = vanilla_spell(0x33); // Power Down
    let outcome = cast_spell(&s, 3, &snap);
    match outcome {
        SpellOutcome::Buff { magnitude, .. } => assert_eq!(magnitude, -20),
        _ => panic!("expected buff outcome"),
    }
}

#[test]
fn cast_capture_spell_returns_capture_roll() {
    let snap = alive_enemy(5, 50);
    let s = vanilla_spell(0x40); // Reseal
    let outcome = cast_spell(&s, 3, &snap);
    match outcome {
        SpellOutcome::CaptureRoll { target, hit_pct } => {
            assert_eq!(target, 3);
            assert_eq!(hit_pct, 60);
        }
        _ => panic!("expected capture outcome"),
    }
}

#[test]
fn cast_escape_spell_returns_escape() {
    let snap = SpellSnapshot {
        caster_mp: 50,
        ..Default::default()
    };
    let s = vanilla_spell(0x41); // Warp
    let outcome = cast_spell(&s, 0, &snap);
    assert!(matches!(outcome, SpellOutcome::Escape));
}

#[test]
fn element_mask_with_and_contains_round_trip() {
    let m = ElementMask::empty()
        .with(SpellElement::Fire)
        .with(SpellElement::Ice);
    assert!(m.contains(SpellElement::Fire));
    assert!(m.contains(SpellElement::Ice));
    assert!(!m.contains(SpellElement::Water));
}

#[test]
fn element_neutral_never_matches_weakness() {
    let m = ElementMask::empty().with(SpellElement::Fire);
    // Neutral element passthrough means multiplier = 1.0.
    assert_eq!(SpellElement::Neutral.multiplier_against(m), 1.0);
}

#[test]
fn cure_outcome_count_returns_placeholder_zero() {
    let snap = SpellSnapshot {
        caster_mp: 50,
        target_alive: true,
        ..Default::default()
    };
    let s = vanilla_spell(0x13);
    let outcome = cast_spell(&s, 0, &snap);
    match outcome {
        SpellOutcome::Cure { removed, .. } => assert_eq!(removed, 0),
        _ => panic!("expected cure outcome"),
    }
}

#[test]
fn catalog_iter_yields_every_inserted_spell() {
    let mut cat = SpellCatalog::new();
    cat.insert(SpellDef {
        id: 0x01,
        name: "Spark".into(),
        mp_cost: 3,
        ..Default::default()
    });
    cat.insert(SpellDef {
        id: 0x02,
        name: "Mist".into(),
        mp_cost: 4,
        ..Default::default()
    });
    let names: Vec<&str> = cat.iter().map(|s| s.name.as_str()).collect();
    assert!(names.contains(&"Spark"));
    assert!(names.contains(&"Mist"));
}

#[test]
fn unknown_spell_id_returns_none() {
    let cat = SpellCatalog::vanilla();
    assert!(cat.get(0xFE).is_none());
}

#[test]
fn spell_def_default_is_neutral_one_enemy_zero_cost() {
    let s = SpellDef::default();
    assert_eq!(s.id, 0);
    assert_eq!(s.mp_cost, 0);
    assert_eq!(s.element, SpellElement::Neutral);
    assert_eq!(s.target, SpellTarget::OneEnemy);
}
