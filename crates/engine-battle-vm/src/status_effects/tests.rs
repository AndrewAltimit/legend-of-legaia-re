use super::*;

#[test]
fn status_icon_base_marker_needs_a_live_empty_slot() {
    assert_eq!(status_icon(0, true), StatusIcon::BaseWithCount);
    // Empty and not live: the gone-marker, never the base+count.
    assert_eq!(status_icon(0, false), StatusIcon::Sprite(0x20));
}

#[test]
fn status_icon_absent_slot_wins_over_flag_bits() {
    // `present == 0` is tested before the ladder: 0x20 regardless of bits.
    assert_eq!(status_icon(0x0004, false), StatusIcon::Sprite(0x20));
    assert_eq!(status_icon(0x1000, false), StatusIcon::Sprite(0x20));
}

#[test]
fn status_icon_ladder_is_priority_ordered() {
    assert_eq!(status_icon(0x0004, true), StatusIcon::Sprite(0x1A));
    assert_eq!(status_icon(0x0400, true), StatusIcon::Sprite(0x1D));
    assert_eq!(status_icon(0x0800, true), StatusIcon::Sprite(0x1E));
    assert_eq!(status_icon(0x0380, true), StatusIcon::Sprite(0x1C));
    assert_eq!(status_icon(0x0078, true), StatusIcon::Sprite(0x1B));
    assert_eq!(status_icon(0x1000, true), StatusIcon::Sprite(0x1F));
    assert_eq!(status_icon(0x0002, true), StatusIcon::Sprite(0x19));
    assert_eq!(status_icon(0x0001, true), StatusIcon::Sprite(0x18));
    // 0x0004 outranks a lower-priority bit set at the same time.
    assert_eq!(status_icon(0x0004 | 0x0001, true), StatusIcon::Sprite(0x1A));
    // 0x0380 (a group mask) outranks 0x0078.
    assert_eq!(status_icon(0x0380 | 0x0078, true), StatusIcon::Sprite(0x1C));
}

#[test]
fn every_tracked_kind_round_trips_typed_set_to_word_to_element() {
    // The wiring contract in one test: a typed status set packs into the
    // retail word, the retail ladder picks one sprite, and the sprite
    // names the kind back. Faint is the one kind with no bit - it is the
    // zero-HP arm instead.
    for kind in [
        StatusKind::Venom,
        StatusKind::Toxic,
        StatusKind::Stone,
        StatusKind::Rot,
        StatusKind::Confuse,
        StatusKind::Numb,
        StatusKind::Sleep,
        StatusKind::Curse,
    ] {
        let mut t = StatusEffectTracker::new();
        t.apply(0, kind);
        let word = t.display_flags(0);
        assert_ne!(word, 0, "{kind:?} must set a bit");
        let StatusIcon::Sprite(id) = status_icon(word, true) else {
            panic!("{kind:?} -> word {word:#06x} selected no sprite");
        };
        assert_eq!(
            StatusKind::from_icon_sprite(id),
            Some(kind),
            "{kind:?} -> {word:#06x} -> sprite {id:#04x}"
        );
    }
    // Faint: no bit, and the zero-HP arm draws the KO marker.
    assert_eq!(StatusKind::Faint.display_bit(), 0);
    let mut t = StatusEffectTracker::new();
    t.apply(0, StatusKind::Faint);
    assert_eq!(t.display_flags(0), 0);
    assert_eq!(
        status_icon(t.display_flags(0), false),
        StatusIcon::Sprite(0x20)
    );
    assert_eq!(StatusKind::from_icon_sprite(0x20), Some(StatusKind::Faint));
}

#[test]
fn rot_packs_each_rolled_limb_bit() {
    // Each roll ORs its limb in (`0x801E1734..0x801E1740`), so the word
    // grows by one limb bit per roll rather than moving between them.
    let mut t = StatusEffectTracker::new();
    t.apply(1, StatusKind::Rot);
    let mut expect = 0;
    for limb in 0..3u8 {
        t.set_rot_limb(1, limb);
        expect |= display_flags::ROT_LIMBS[limb as usize];
        let word = t.display_flags(1);
        assert_eq!(word, expect);
        // One limb bit still lands on the group mask the ladder tests.
        assert_ne!(word & display_flags::ROT_MASK, 0);
        assert_eq!(status_icon(word, true), StatusIcon::Sprite(0x1B));
    }
    // The kind-level bit is the whole group (what Rot Guard clears).
    assert_eq!(StatusKind::Rot.display_bit(), display_flags::ROT_MASK);
}

#[test]
fn packed_word_orders_by_the_retail_ladder_not_the_typed_list() {
    // Venom + Stone: the engine's list order puts Venom first, retail's
    // ladder puts Stone first. The packed word is what decides.
    let mut t = StatusEffectTracker::new();
    t.apply(0, StatusKind::Venom);
    t.apply(0, StatusKind::Stone);
    let word = t.display_flags(0);
    assert_eq!(word, display_flags::VENOM | display_flags::STONE);
    assert_eq!(status_icon(word, true), StatusIcon::Sprite(0x1A));
}

#[test]
fn master_guard_mask_is_every_ailment_bit_and_excludes_delegation() {
    // FUN_8004CE2C's Master Guard arm clears 0x1C7F (`andi 0xe380`).
    let all = display_flags::VENOM
        | display_flags::TOXIC
        | display_flags::STONE
        | display_flags::ROT_MASK
        | display_flags::NUMB
        | display_flags::SLEEP
        | display_flags::CURSE;
    assert_eq!(all, AILMENT_MASK);
    // Rage is a passive, not an ailment: a full cure leaves it standing.
    assert_eq!(AILMENT_MASK & display_flags::CONFUSE_MASK, 0);
    // Damage clears the wake-on-hit family and nothing else.
    assert_eq!(
        display_flags::DAMAGE_CLEARS,
        display_flags::CONFUSE_MASK | display_flags::NUMB | display_flags::SLEEP
    );
    // The whole-actor inert pair is Stone + Numb.
    assert_eq!(
        display_flags::INERT_MASK,
        display_flags::STONE | display_flags::NUMB
    );
}

#[test]
fn status_icon_unlisted_bit_on_a_live_slot_draws_nothing() {
    // 0x8000 is outside every mask; a live slot carrying only it is blank.
    assert_eq!(status_icon(0x8000, true), StatusIcon::None);
}

#[test]
fn enemy_effect_byte_routes() {
    assert_eq!(
        StatusKind::from_enemy_effect(EnemyEffect::Toxic),
        Some(StatusKind::Toxic)
    );
    assert_eq!(
        StatusKind::from_enemy_effect(EnemyEffect::Numb),
        Some(StatusKind::Numb)
    );
    assert_eq!(
        StatusKind::from_enemy_effect(EnemyEffect::Other(3)),
        Some(StatusKind::Venom)
    );
    // Bytes 4/5 follow the pinned appliers: strong DoT (Toxic) and the
    // random limb-disable (Rot) - not the old Sleep/Confuse reading.
    assert_eq!(
        StatusKind::from_enemy_effect(EnemyEffect::Other(4)),
        Some(StatusKind::Toxic)
    );
    assert_eq!(
        StatusKind::from_enemy_effect(EnemyEffect::Other(5)),
        Some(StatusKind::Rot)
    );
    assert_eq!(
        StatusKind::from_enemy_effect(EnemyEffect::Other(6)),
        Some(StatusKind::Curse)
    );
    assert_eq!(
        StatusKind::from_enemy_effect(EnemyEffect::Other(8)),
        Some(StatusKind::Faint)
    );
    assert_eq!(StatusKind::from_enemy_effect(EnemyEffect::None), None);
    assert_eq!(StatusKind::from_enemy_effect(EnemyEffect::Other(99)), None);
}

#[test]
fn rot_limb_roll_round_trips() {
    let mut t = StatusEffectTracker::new();
    assert_eq!(t.rot_limb(3), None);
    t.apply_from_enemy_effect(3, EnemyEffect::Other(5));
    // Applied with a default limb until the applier rolls one.
    assert_eq!(t.rot_limb(3), Some(0));
    t.set_rot_limb(3, 2);
    assert_eq!(t.rot_limb(3), Some(2));
    // The roll is mod-3.
    t.set_rot_limb(3, 7);
    assert_eq!(t.rot_limb(3), Some(1));
    // Rot doesn't block the whole turn (a limb, not the actor).
    assert!(!StatusKind::Rot.blocks_actions());
    // Curing clears the limb.
    t.cure(3, StatusKind::Rot);
    assert_eq!(t.rot_limb(3), None);
}

#[test]
fn rot_rolls_accumulate_limbs_as_retail_ors_them() {
    // `0x801E1734..0x801E1740`: `lhu` / `or 1 << (rand % 3 + 3)` / `sh`,
    // so each landed Rot adds a limb bit to `+0x16E`.
    let mut t = StatusEffectTracker::new();
    t.apply(0, StatusKind::Rot);
    t.set_rot_limb(0, 1);
    assert_eq!(t.display_flags(0), 0x0010);
    t.apply(0, StatusKind::Rot);
    t.set_rot_limb(0, 2);
    assert_eq!(t.display_flags(0), 0x0030);
    t.set_rot_limb(0, 0);
    assert_eq!(t.display_flags(0), 0x0038);
    // The latest roll is still what `rot_limb` answers.
    assert_eq!(t.rot_limb(0), Some(0));
    t.cure(0, StatusKind::Rot);
    assert_eq!(t.display_flags(0), 0);
}

#[test]
fn apply_then_has_returns_true() {
    let mut t = StatusEffectTracker::new();
    t.apply(0, StatusKind::Toxic);
    assert!(t.has(0, StatusKind::Toxic));
    assert!(!t.has(0, StatusKind::Numb));
}

#[test]
fn apply_idempotent_takes_longer_duration() {
    let mut t = StatusEffectTracker::new();
    t.apply_with_duration(0, StatusKind::Toxic, 2);
    t.apply_with_duration(0, StatusKind::Toxic, 5);
    let s = t.statuses(0);
    assert_eq!(s.len(), 1);
    assert_eq!(s[0].remaining_turns, 5);
}

#[test]
fn apply_idempotent_keeps_longer_when_new_is_shorter() {
    let mut t = StatusEffectTracker::new();
    t.apply_with_duration(0, StatusKind::Toxic, 5);
    t.apply_with_duration(0, StatusKind::Toxic, 2);
    assert_eq!(t.statuses(0)[0].remaining_turns, 5);
}

#[test]
fn cure_removes_and_emits_event() {
    let mut t = StatusEffectTracker::new();
    t.apply(0, StatusKind::Toxic);
    t.drain_events(); // flush the apply (no apply event but in case)
    assert!(t.cure(0, StatusKind::Toxic));
    assert!(!t.has(0, StatusKind::Toxic));
    let evs = t.drain_events();
    assert_eq!(evs.len(), 1);
    assert!(matches!(evs[0], StatusEvent::Cleared { .. }));
}

#[test]
fn cure_all_clears_every_kind() {
    let mut t = StatusEffectTracker::new();
    t.apply(0, StatusKind::Toxic);
    t.apply(0, StatusKind::Numb);
    t.apply(0, StatusKind::Curse);
    t.cure_all(0);
    assert!(!t.is_afflicted(0));
}

#[test]
fn toxic_tick_dot_is_max_hp_over_16() {
    let mut t = StatusEffectTracker::new();
    t.apply_with_duration(0, StatusKind::Toxic, 3);
    let dmg = t.tick_actor(0, 100, 160);
    assert_eq!(dmg, 10); // 160 >> 4
}

#[test]
fn toxic_never_kills_clamps_to_current_minus_one() {
    // Retail: `if (cur <= raw) raw = cur - 1` BEFORE the cap. A 5-HP
    // actor with a big max takes 4, landing at 1 HP, never 0.
    let mut t = StatusEffectTracker::new();
    t.apply(0, StatusKind::Toxic);
    let dmg = t.tick_actor(0, 5, 4000);
    assert_eq!(dmg, 4);
}

#[test]
fn toxic_caps_at_256_and_venom_at_128() {
    // toxic: 65535 >> 4 = 4095 -> capped to 0x100.
    assert_eq!(toxic_tick_damage(65535, 65535), 0x100);
    // venom: 65535 >> 5 = 2047 -> capped to 0x80.
    assert_eq!(venom_tick_damage(65535, 65535), 0x80);
}

#[test]
fn never_kill_clamp_applies_before_the_cap() {
    // cur=200, max=10000: raw toxic = 625 >= cur -> 199, and the cap
    // does NOT shrink it further (199 < 256). Retail order.
    assert_eq!(toxic_tick_damage(200, 10000), 199);
}

#[test]
fn tiny_max_hp_ticks_zero_no_floor() {
    // Retail has no 1-damage floor: max_hp 5 >> 4 == 0.
    let mut t = StatusEffectTracker::new();
    t.apply(0, StatusKind::Toxic);
    let dmg = t.tick_actor(0, 5, 5);
    assert_eq!(dmg, 0);
    // And a zero tick queues no TickDamage event (no damage popup).
    assert!(
        !t.drain_events()
            .iter()
            .any(|e| matches!(e, StatusEvent::TickDamage { .. }))
    );
}

#[test]
fn poison_tick_is_max_hp_over_32() {
    let mut t = StatusEffectTracker::new();
    t.apply(0, StatusKind::Venom);
    let dmg = t.tick_actor(0, 80, 100);
    assert_eq!(dmg, 3); // 100 >> 5
}

#[test]
fn toxic_suppresses_venom_tick() {
    // Retail's ticker takes the strong-DoT branch first; the weak one is
    // an `else`. With both active only Toxic's max/16 lands.
    let mut t = StatusEffectTracker::new();
    t.apply(0, StatusKind::Toxic);
    t.apply(0, StatusKind::Venom);
    let dmg = t.tick_actor(0, 1000, 1600);
    assert_eq!(dmg, 100); // 1600 >> 4 only, not + 1600 >> 5
    let evs = t.drain_events();
    assert!(!evs.iter().any(|e| matches!(
        e,
        StatusEvent::TickDamage {
            kind: StatusKind::Venom,
            ..
        }
    )));
}

#[test]
fn dead_actor_does_not_tick() {
    // Retail guards the whole DoT arm on `+0x14C != 0`.
    let mut t = StatusEffectTracker::new();
    t.apply(0, StatusKind::Toxic);
    assert_eq!(t.tick_actor(0, 0, 1600), 0);
}

#[test]
fn ticking_decrements_remaining_turns() {
    let mut t = StatusEffectTracker::new();
    t.apply_with_duration(0, StatusKind::Toxic, 2);
    t.tick_actor(0, 100, 160);
    assert_eq!(t.statuses(0)[0].remaining_turns, 1);
    t.tick_actor(0, 100, 160);
    // Cleared at zero
    assert!(!t.has(0, StatusKind::Toxic));
}

#[test]
fn ticking_emits_cleared_event_at_expiry() {
    let mut t = StatusEffectTracker::new();
    t.apply_with_duration(0, StatusKind::Toxic, 1);
    t.drain_events();
    t.tick_actor(0, 100, 160);
    let evs = t.drain_events();
    assert!(evs.iter().any(|e| matches!(
        e,
        StatusEvent::Cleared {
            kind: StatusKind::Toxic,
            ..
        }
    )));
}

#[test]
fn numb_does_not_deal_damage_on_tick() {
    let mut t = StatusEffectTracker::new();
    t.apply(0, StatusKind::Numb);
    let dmg = t.tick_actor(0, 100, 160);
    assert_eq!(dmg, 0);
}

#[test]
fn numb_blocks_actions_and_clears_on_being_hit() {
    // Numb is a full paralysis (not a chance roll): it blocks the turn and,
    // like Sleep, wears off when the unit is attacked.
    let mut t = StatusEffectTracker::new();
    t.apply(0, StatusKind::Numb);
    assert!(!t.check_can_act(0), "Numb blocks the turn");
    assert!(t.drain_events().iter().any(|e| matches!(
        e,
        StatusEvent::Blocked {
            kind: StatusKind::Numb,
            ..
        }
    )));
    t.on_damaged(0);
    assert!(!t.has(0, StatusKind::Numb), "being hit clears Numb");
}

#[test]
fn petrified_actor_takes_no_poison_tick() {
    // Stone makes the unit invulnerable, so its poison DoT doesn't tick.
    let mut t = StatusEffectTracker::new();
    t.apply(0, StatusKind::Stone);
    t.apply(0, StatusKind::Venom);
    assert_eq!(t.tick_actor(0, 80, 160), 0, "Stone absorbs poison ticks");
}

#[test]
fn stone_lasts_the_whole_battle() {
    // Stone has no in-battle cure - its default duration is effectively
    // "until battle end".
    assert_eq!(StatusKind::Stone.default_duration(), 255);
}

#[test]
fn toxic_drains_exactly_twice_venom() {
    // Both arms key on max HP: max/16 vs max/32 (below the caps and the
    // never-kill clamp).
    assert_eq!(toxic_tick_damage(1000, 1600), 100);
    assert_eq!(venom_tick_damage(1000, 1600), 50);
    assert_eq!(
        toxic_tick_damage(1000, 1600),
        2 * venom_tick_damage(1000, 1600)
    );
}

#[test]
fn cure_stone_on_escape_restores_only_stone() {
    let mut t = StatusEffectTracker::new();
    t.apply(0, StatusKind::Stone);
    t.apply(0, StatusKind::Venom);
    t.apply(2, StatusKind::Stone);
    t.apply(1, StatusKind::Toxic);
    t.drain_events();
    assert_eq!(t.cure_stone_on_escape(), 2);
    assert!(!t.has(0, StatusKind::Stone));
    assert!(!t.has(2, StatusKind::Stone));
    assert!(t.has(0, StatusKind::Venom), "escape cures only Stone");
    assert!(t.has(1, StatusKind::Toxic), "escape cures only Stone");
    let evs = t.drain_events();
    assert_eq!(
        evs.iter()
            .filter(|e| matches!(
                e,
                StatusEvent::Cleared {
                    kind: StatusKind::Stone,
                    ..
                }
            ))
            .count(),
        2
    );
}

#[test]
fn cure_stone_on_escape_is_noop_without_stone() {
    let mut t = StatusEffectTracker::new();
    t.apply(0, StatusKind::Venom);
    assert_eq!(t.cure_stone_on_escape(), 0);
    assert!(t.has(0, StatusKind::Venom));
}

#[test]
fn check_can_act_emits_blocked_when_asleep() {
    let mut t = StatusEffectTracker::new();
    t.apply(0, StatusKind::Sleep);
    assert!(!t.check_can_act(0));
    let evs = t.drain_events();
    assert_eq!(evs.len(), 1);
    assert!(matches!(
        evs[0],
        StatusEvent::Blocked {
            kind: StatusKind::Sleep,
            ..
        }
    ));
}

#[test]
fn check_can_act_passes_when_only_toxic() {
    let mut t = StatusEffectTracker::new();
    t.apply(0, StatusKind::Toxic);
    assert!(t.check_can_act(0));
}

#[test]
fn check_can_cast_magic_blocked_by_silence() {
    let mut t = StatusEffectTracker::new();
    t.apply(0, StatusKind::Curse);
    assert!(!t.check_can_cast_magic(0));
    let evs = t.drain_events();
    assert!(
        evs.iter()
            .any(|e| matches!(e, StatusEvent::BlockedMagic { .. }))
    );
}

#[test]
fn check_can_cast_magic_blocked_by_petrify() {
    let mut t = StatusEffectTracker::new();
    t.apply(0, StatusKind::Faint);
    assert!(!t.check_can_cast_magic(0));
}

#[test]
fn on_damaged_clears_sleep() {
    let mut t = StatusEffectTracker::new();
    t.apply(0, StatusKind::Sleep);
    t.apply(0, StatusKind::Toxic);
    t.on_damaged(0);
    assert!(!t.has(0, StatusKind::Sleep));
    assert!(t.has(0, StatusKind::Toxic));
}

#[test]
fn apply_from_enemy_effect_routes_burned() {
    let mut t = StatusEffectTracker::new();
    let kind = t.apply_from_enemy_effect(2, EnemyEffect::Toxic);
    assert_eq!(kind, Some(StatusKind::Toxic));
    assert!(t.has(2, StatusKind::Toxic));
}

#[test]
fn apply_from_enemy_effect_skips_none() {
    let mut t = StatusEffectTracker::new();
    let kind = t.apply_from_enemy_effect(0, EnemyEffect::None);
    assert_eq!(kind, None);
    assert!(!t.is_afflicted(0));
}

#[test]
fn multiple_actors_tracked_independently() {
    let mut t = StatusEffectTracker::new();
    t.apply(0, StatusKind::Toxic);
    t.apply(3, StatusKind::Numb);
    assert!(t.has(0, StatusKind::Toxic));
    assert!(t.has(3, StatusKind::Numb));
    assert!(!t.has(0, StatusKind::Numb));
    assert!(!t.has(3, StatusKind::Toxic));
}

#[test]
fn petrify_default_duration_is_huge() {
    let mut t = StatusEffectTracker::new();
    t.apply(0, StatusKind::Faint);
    let inst = t.statuses(0)[0];
    assert_eq!(inst.remaining_turns, 255);
}

#[test]
fn no_op_for_empty_slot() {
    let mut t = StatusEffectTracker::new();
    let dmg = t.tick_actor(7, 100, 100);
    assert_eq!(dmg, 0);
    assert!(t.drain_events().is_empty());
}

#[test]
fn stone_persists_across_turns() {
    // Stone has no in-battle expiry (whole-battle duration), so a single
    // turn tick does not clear it.
    let mut t = StatusEffectTracker::new();
    t.apply(0, StatusKind::Stone);
    assert!(t.has(0, StatusKind::Stone));
    t.tick_actor(0, 100, 100);
    assert!(t.has(0, StatusKind::Stone), "Stone lasts the whole battle");
}
