use super::*;

/// The kernel and the disc-side parser each state the record shape; a
/// silent disagreement would put the parser's records out of phase with the
/// expander's indexing.
#[test]
fn constants_agree_with_the_disc_parser() {
    assert_eq!(
        CUE_GROUP_STRIDE,
        legaia_game_tables::move_power::CUE_GROUP_STRIDE
    );
    assert_eq!(
        CUE_ACTOR_FLAG,
        legaia_game_tables::move_power::CUE_ACTOR_FLAG
    );
}

fn tables() -> (Vec<u8>, Vec<u8>) {
    // Group 0: empty. Group 1: two effect cues (ids 4, 5).
    // Group 2: one actor cue (id 0x80 | 3) and one effect cue (id 9).
    let mut groups = vec![0u8; CUE_GROUP_STRIDE * 3];
    groups[CUE_GROUP_STRIDE] = 2;
    groups[CUE_GROUP_STRIDE + 1] = 4;
    groups[CUE_GROUP_STRIDE + 2] = 5;
    groups[CUE_GROUP_STRIDE * 2] = 2;
    groups[CUE_GROUP_STRIDE * 2 + 1] = 0x83;
    groups[CUE_GROUP_STRIDE * 2 + 2] = 9;
    let mut clut = vec![0u8; 16];
    clut[4] = 0xD0;
    // id 5 and id 9 have no sound.
    (groups, clut)
}

#[test]
fn empty_group_still_writes_the_actor_fields() {
    let (groups, clut_map) = tables();
    let t = CueTables {
        groups: &groups,
        clut_map: &clut_map,
    };
    let out = expand_cue_group(CUE_TINT_NEUTRAL, 0x1234, 0, 0, &t);
    assert!(out.spawns.is_empty());
    assert_eq!(out.actor_state, 0x1234);
    assert_eq!(out.actor_blend, Some(0x2000));
}

#[test]
fn skip_state_suppresses_the_flags_write() {
    let (groups, clut_map) = tables();
    let t = CueTables {
        groups: &groups,
        clut_map: &clut_map,
    };
    let out = expand_cue_group(CUE_TINT_NEUTRAL, CUE_ACTOR_STATE_SKIP, 0, 0, &t);
    assert_eq!(out.actor_blend, None);
}

#[test]
fn effect_cues_carry_their_clut_row_only_when_the_map_byte_is_set() {
    let (groups, clut_map) = tables();
    let t = CueTables {
        groups: &groups,
        clut_map: &clut_map,
    };
    let out = expand_cue_group(CUE_TINT_NEUTRAL, 0, 0, 1, &t);
    assert_eq!(
        out.spawns,
        vec![
            CueSpawn::Effect {
                id: 4,
                clut_x: Some(0xD0),
                effect_index: 4,
                tint: None
            },
            CueSpawn::Effect {
                id: 5,
                clut_x: None,
                effect_index: 5,
                tint: None
            },
        ]
    );
}

#[test]
fn actor_cues_strip_the_flag_and_use_the_unbiased_yaw() {
    let (groups, clut_map) = tables();
    let t = CueTables {
        groups: &groups,
        clut_map: &clut_map,
    };
    let out = expand_cue_group(0x00FF_0000, 0, 0x400, 2, &t);
    assert_eq!(out.spawns[0], CueSpawn::Actor { id: 3, yaw: 0x400 });
    assert_eq!(
        out.spawns[1],
        CueSpawn::Effect {
            id: 9,
            clut_x: None,
            effect_index: 9,
            tint: Some(0x00FF_0000 | CUE_TINT_MODE)
        }
    );
}

#[test]
fn the_three_computed_sites_move_with_the_tier() {
    // class 0: group == tier. class 1: tier + 1. class 2: tier + 3.
    for tier in 0..4u8 {
        assert_eq!(cue_group_for(0, tier).unwrap().group, tier);
        assert_eq!(cue_group_for(1, tier).unwrap().group, tier + 1);
        assert_eq!(cue_group_for(2, tier).unwrap().group, tier + 3);
    }
}

#[test]
fn the_literal_sites_ignore_the_tier_entirely() {
    for tier in [0u8, 1, 9, 0xFF] {
        assert_eq!(cue_group_for(3, tier).unwrap().group, 5);
        assert_eq!(cue_group_for(4, tier).unwrap().group, 6);
        assert_eq!(cue_group_for(5, tier).unwrap().group, 7);
        assert_eq!(cue_group_for(8, tier).unwrap().group, 0xC);
    }
}

#[test]
fn class_seven_has_one_site_per_tier_and_none_outside_one_to_four() {
    for (tier, group) in [(1u8, 8u8), (2, 9), (3, 0xA), (4, 0xB)] {
        assert_eq!(cue_group_for(7, tier).unwrap().group, group);
    }
    assert!(cue_group_for(7, 0).is_none());
    assert!(cue_group_for(7, 5).is_none());
}

#[test]
fn only_the_class_one_loop_arm_is_per_target() {
    assert!(cue_group_for(1, 0).unwrap().per_target);
    for class in [0u8, 2, 3, 4, 5, 8] {
        assert!(
            !cue_group_for(class, 1).unwrap().per_target,
            "class {class}"
        );
    }
}

#[test]
fn the_classes_with_no_expander_branch_select_no_site() {
    // Class 6's inner table only bumps counters; 9..=0xE and 0x82 have no
    // `jal`; the spell-table routing bytes are above the arms entirely.
    for class in [6u8, 9, 0xA, 0xB, 0xC, 0xD, 0xE, 0x14, 0x32, 0x63, 0x82] {
        assert!(cue_group_for(class, 1).is_none(), "class {class:#x}");
    }
}

#[test]
fn revive_is_the_one_site_that_passes_the_skip_state() {
    let revive = cue_group_for(4, 0).unwrap();
    assert_eq!(revive.actor_state, CUE_ACTOR_STATE_SKIP);
    // ... which is what suppresses the `+0x0C` follow-up write.
    let groups = vec![0u8; CUE_GROUP_STRIDE * 8];
    let clut = vec![0u8; 16];
    let t = CueTables {
        groups: &groups,
        clut_map: &clut,
    };
    let out = expand_cue_group(revive.tint, revive.actor_state, 0, revive.group, &t);
    assert_eq!(out.actor_blend, None);
    // Every other site writes it.
    for class in [0u8, 1, 2, 3, 5, 8] {
        let s = cue_group_for(class, 1).unwrap();
        let out = expand_cue_group(s.tint, s.actor_state, 0, s.group, &t);
        assert_eq!(out.actor_blend, Some(0x2000), "class {class}");
    }
}

#[test]
fn only_the_two_hp_restore_sites_pass_the_neutral_tint() {
    assert_eq!(cue_group_for(0, 0).unwrap().tint, CUE_TINT_NEUTRAL);
    assert_eq!(cue_group_for(1, 0).unwrap().tint, CUE_TINT_NEUTRAL);
    // The class-7 buff sites reuse the neutral word for tiers 1 and 2 but
    // not 3 and 4 - so "neutral" is per-site, not per-class.
    assert_eq!(cue_group_for(7, 1).unwrap().tint, CUE_TINT_NEUTRAL);
    assert_ne!(cue_group_for(7, 3).unwrap().tint, CUE_TINT_NEUTRAL);
    for class in [2u8, 3, 4, 5, 8] {
        assert_ne!(cue_group_for(class, 1).unwrap().tint, CUE_TINT_NEUTRAL);
    }
}

fn inputs() -> BannerInputs {
    BannerInputs {
        active_slot: 0,
        action_category: 1,
        action_id: 0x20,
        target: 3,
        ctx_override_slot: 0,
        monster_alive: [true, true, false, true],
    }
}

#[test]
fn category_five_skips_everything() {
    let plan = plan_target_banner(
        &BannerInputs {
            action_category: CATEGORY_SKIP,
            ..inputs()
        },
        0,
    );
    assert_eq!(plan.layout, BannerLayout::Skipped);
    assert!(!plan.caster_banner);
    assert!(plan.hud_elements.is_empty());
}

#[test]
fn all_enemies_takes_the_multi_arm_and_skips_the_active_slot() {
    let plan = plan_target_banner(
        &BannerInputs {
            active_slot: 4,
            target: TARGET_ALL_ENEMIES,
            ..inputs()
        },
        0,
    );
    // Slots 3..=6 alive as [true, true, false, true]; slot 4 is active.
    assert_eq!(plan.layout, BannerLayout::MultiTarget { slots: vec![3, 6] });
    assert_eq!(plan.hud_elements, vec![HUD_CASTER_BANNER]);
}

#[test]
fn the_three_forced_ids_take_the_multi_arm_from_category_two() {
    for id in MULTI_TARGET_ACTION_IDS {
        let plan = plan_target_banner(
            &BannerInputs {
                action_category: 2,
                action_id: id,
                target: 3,
                ..inputs()
            },
            0,
        );
        assert!(matches!(plan.layout, BannerLayout::MultiTarget { .. }));
    }
    // Same ids under a different category do not force it.
    let plan = plan_target_banner(
        &BannerInputs {
            action_category: 1,
            action_id: MULTI_TARGET_ACTION_IDS[0],
            ..inputs()
        },
        0,
    );
    assert!(matches!(plan.layout, BannerLayout::SingleTarget { .. }));
}

#[test]
fn all_enemies_with_an_override_slot_becomes_single_target() {
    let plan = plan_target_banner(
        &BannerInputs {
            target: TARGET_ALL_ENEMIES,
            ctx_override_slot: 5,
            ..inputs()
        },
        0x30,
    );
    assert_eq!(plan.layout, BannerLayout::SingleTarget { source_slot: 5 });
    assert_eq!(
        plan.hud_elements,
        vec![HUD_CASTER_BANNER, HUD_TARGET_BANNER]
    );
    assert_eq!(plan.banner_width, Some(BANNER_WIDTH_BASE - 0x30));
}

#[test]
fn categories_zero_and_four_stop_after_the_caster_banner() {
    for category in CATEGORY_NO_TARGET_BANNER {
        let plan = plan_target_banner(
            &BannerInputs {
                action_category: category,
                ..inputs()
            },
            0,
        );
        assert_eq!(plan.layout, BannerLayout::Idle);
        assert_eq!(plan.banner_width, None, "the arm returns before the width");
    }
}

#[test]
fn party_wide_needs_a_monster_caster_and_target_eight() {
    let plan = plan_target_banner(
        &BannerInputs {
            active_slot: 3,
            target: TARGET_PARTY_WIDE,
            ..inputs()
        },
        0,
    );
    assert_eq!(plan.layout, BannerLayout::PartyWide);
    assert_eq!(plan.hud_elements, vec![HUD_CASTER_BANNER, 0x06, 0x4E, 0x4F]);
    assert_eq!(plan.ctx_18, Some(CTX_18_PARTY_WIDE));
    assert_eq!(plan.banner_width, Some(BANNER_WIDTH_IDLE));

    // Party caster, same target: idle, no extra HUD.
    let plan = plan_target_banner(
        &BannerInputs {
            active_slot: 1,
            target: TARGET_PARTY_WIDE,
            ..inputs()
        },
        0,
    );
    assert_eq!(plan.layout, BannerLayout::Idle);
    assert_eq!(plan.hud_elements, vec![HUD_CASTER_BANNER]);
    assert_eq!(plan.ctx_18, None);
}
