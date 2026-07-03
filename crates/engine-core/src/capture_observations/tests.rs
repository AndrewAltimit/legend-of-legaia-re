//! Unit tests for `capture_observations`, extracted verbatim.

use super::*;

#[test]
fn byte_delta_signed_delta_arithmetic() {
    let d = ByteDelta {
        addr: 0x80084708 + 0x10E,
        before: 0x3A,
        after: 0x42,
    };
    assert_eq!(d.signed_delta(), 8);

    let neg = ByteDelta {
        addr: 0x80084708 + 0x11C,
        before: 0xDD,
        after: 0x03,
    };
    // 0x03 - 0xDD = -218 (the actual u16 LE field underneath wraps,
    // but the byte-only signed delta is what we surface).
    assert_eq!(neg.signed_delta(), -218);
}

#[test]
fn encounter_trigger_overlay_window_covers_documented_range() {
    let (lo, hi) = encounter_trigger::OVERLAY_WINDOW;
    assert!(lo < hi);
    assert!(lo <= 0x801CE808);
    assert!(hi >= 0x801F3818);
    // Sanity: window spans roughly the documented 150 KB.
    assert!((hi - lo) as usize >= 0x20_000);
    assert!((hi - lo) as usize <= 0x40_000);
}

#[test]
fn encounter_trigger_actor_pool_stride_is_consistent() {
    let (lo, hi) = encounter_trigger::ACTOR_POOL_WINDOW;
    let span = hi - lo;
    let n = encounter_trigger::ACTOR_POOL_SLOT_COUNT as u32;
    let stride = encounter_trigger::ACTOR_POOL_SLOT_STRIDE;
    assert!(span >= n * stride);
}

#[test]
fn vahn_fire_book_changed_addr_is_inside_record() {
    let addr = vahn_fire_book_use::changed_addr();
    assert!(addr >= vahn_fire_book_use::VAHN_RECORD_BASE);
    assert!(addr < vahn_fire_book_use::VAHN_RECORD_BASE + 0x414);
}

#[test]
fn seru_capture_reader_lifts_planted_before_after() {
    let mut ram = vec![0u8; 0x200000];
    let base = (seru_capture::VAHN_RECORD_BASE - 0x80000000) as usize;
    // BEFORE: empty spell list.
    assert_eq!(
        seru_capture::read_spell_head(&ram, seru_capture::VAHN_RECORD_BASE),
        Some(seru_capture::BEFORE)
    );
    // Plant the AFTER bytes: count=1, id[0]=0x81, level[0]=1.
    ram[base + seru_capture::SPELL_COUNT_OFFSET as usize] = 1;
    ram[base + seru_capture::SPELL_IDS_OFFSET as usize] = seru_capture::GIMARD_SPELL_ID;
    ram[base + seru_capture::SPELL_LEVELS_OFFSET as usize] = seru_capture::GRANTED_LEVEL;
    assert_eq!(
        seru_capture::read_spell_head(&ram, seru_capture::VAHN_RECORD_BASE),
        Some(seru_capture::AFTER)
    );
}

#[test]
fn seru_capture_offsets_match_save_crate_spell_list_schema() {
    // The pinned offsets must agree with the typed reader the save crate
    // exposes, so a captured record round-trips through `spell_list`.
    let mut rec = legaia_save::CharacterRecord::zeroed();
    rec.raw[seru_capture::SPELL_COUNT_OFFSET as usize] = 1;
    rec.raw[seru_capture::SPELL_IDS_OFFSET as usize] = seru_capture::GIMARD_SPELL_ID;
    rec.raw[seru_capture::SPELL_LEVELS_OFFSET as usize] = seru_capture::GRANTED_LEVEL;
    let list = rec.spell_list();
    assert_eq!(list.count, 1);
    assert_eq!(list.ids[0], seru_capture::GIMARD_SPELL_ID);
    assert_eq!(list.levels[0], seru_capture::GRANTED_LEVEL);
}

#[test]
fn char_level_up_record_bases_are_stride_consistent() {
    assert_eq!(char_level_up::VAHN_BASE, 0x80084708);
    assert_eq!(char_level_up::NOA_BASE, char_level_up::VAHN_BASE + 0x414);
    assert_eq!(
        char_level_up::GALA_BASE,
        char_level_up::VAHN_BASE + 2 * 0x414
    );
    assert_eq!(
        char_level_up::SLOT3_BASE,
        char_level_up::VAHN_BASE + 3 * 0x414
    );
}

#[test]
fn char_level_up_record_window_spans_18_bytes() {
    let (lo, hi) = char_level_up::RECORD_WINDOW;
    assert_eq!(hi - lo, 18);
}

#[test]
fn char_level_up_readers_lift_from_synthesised_main_ram() {
    let mut ram = vec![0u8; 0x200000];
    let off = (char_level_up::NOA_BASE - 0x80000000) as usize;
    // Plant XP = 336 at +0x004.
    ram[off + 0x004] = 0x50;
    ram[off + 0x005] = 0x01;
    // Plant a record stat window: HP_max = 182, MP_max = 16, cap = 100,
    // six stats = 124, 24, 16, 13, 34, 6.
    let stats: [u16; 9] = [182, 16, 100, 124, 24, 16, 13, 34, 6];
    for (i, s) in stats.iter().enumerate() {
        let lo = (*s & 0xFF) as u8;
        let hi = (*s >> 8) as u8;
        ram[off + 0x11C + i * 2] = lo;
        ram[off + 0x11C + i * 2 + 1] = hi;
    }
    // Plant rank = 2.
    ram[off + 0x130] = 2;

    assert_eq!(
        char_level_up::read_xp_u16(&ram, char_level_up::NOA_BASE),
        Some(336)
    );
    assert_eq!(
        char_level_up::read_rank_counter(&ram, char_level_up::NOA_BASE),
        Some(2)
    );
    let lifted = char_level_up::read_record_stats(&ram, char_level_up::NOA_BASE).unwrap();
    assert_eq!(lifted, stats);
    assert_eq!(lifted[2], char_level_up::RECORD_STAT_CAP);
}

#[test]
fn field_pack_recover_base_handles_zero_and_below_offset() {
    // Empty RAM: load-dest pointer is zero, recovery should fail.
    let zero = vec![0u8; 0x100000];
    assert!(field_pack_load::recover_base(&zero).is_none());

    // Plant the pinned `town01` settled value (`0x8014BD30`) at
    // the right offset.
    let mut ram = vec![0u8; 0x100000];
    let off = (field_pack_load::LOAD_DEST_PLUS_OFFSET_PTR - 0x80000000) as usize;
    ram[off..off + 4].copy_from_slice(&0x8014BD30u32.to_le_bytes());
    let base = field_pack_load::recover_base(&ram).expect("should recover");
    assert_eq!(base, field_pack_load::TOWN01_FIELD_PACK_BASE);
}

#[test]
fn field_pack_constants_round_trip_through_recover() {
    assert_eq!(
        field_pack_load::TOWN01_FIELD_PACK_BASE + field_pack_load::EFFECT_OFFSET,
        0x8014BD30
    );
    assert_eq!(
        field_pack_load::TOWN0C_FIELD_PACK_BASE + field_pack_load::EFFECT_OFFSET,
        0x800B4DF0
    );
}

#[test]
fn intra_transition_pool_slot_name_round_trips() {
    // Build a synthetic main-RAM image with "town0c" planted in slot 0.
    let mut ram = vec![0u8; 0x100000];
    let off = (field_pack_intra_transition::SCENE_NAME_TABLE_ADDR
        + field_pack_intra_transition::SCENE_NAME_OFFSET_IN_SLOT
        - 0x80000000) as usize;
    ram[off..off + 6].copy_from_slice(b"town0c");
    let label = field_pack_intra_transition::read_pool_slot_name(&ram, 0);
    assert_eq!(label.as_deref(), Some("town0c"));
    // Slot 1 is empty (no name) - reading should fail gracefully.
    assert!(field_pack_intra_transition::read_pool_slot_name(&ram, 1).is_none());
    // Slot 2 doesn't exist.
    assert!(field_pack_intra_transition::read_pool_slot_name(&ram, 2).is_none());
}

#[test]
fn intra_transition_detector_flags_label_base_disagreement() {
    // Plant the mid-transition shape: slot 0 says "town0c",
    // _DAT_8007B8D0 still says PREV_BASE+0x12800 (the old town01 base).
    let mut ram = vec![0u8; 0x200000];
    let pool_off = (field_pack_intra_transition::SCENE_NAME_TABLE_ADDR
        + field_pack_intra_transition::SCENE_NAME_OFFSET_IN_SLOT
        - 0x80000000) as usize;
    ram[pool_off..pool_off + 6].copy_from_slice(b"town0c");
    let load_dest_off = (field_pack_load::LOAD_DEST_PLUS_OFFSET_PTR - 0x80000000) as usize;
    let stale_load_dest = field_pack_intra_transition::PREV_BASE + field_pack_load::EFFECT_OFFSET;
    ram[load_dest_off..load_dest_off + 4].copy_from_slice(&stale_load_dest.to_le_bytes());

    let mid = field_pack_intra_transition::detect_mid_transition(&ram);
    assert_eq!(
        mid,
        Some(("town0c".to_string(), field_pack_intra_transition::PREV_BASE))
    );

    // Settled state: slot 0 says "town01" + base = PREV_BASE.
    // detector should NOT flag this case.
    ram[pool_off..pool_off + 6].copy_from_slice(b"town01");
    assert!(field_pack_intra_transition::detect_mid_transition(&ram).is_none());
}

#[test]
fn fmv_overlay_resident_check_passes_on_planted_signature() {
    // FMV overlay residency is detected by the "MV1.STR" prefix at the
    // pinned compact-table address.
    let mut ram = vec![0u8; 0x200000];
    assert!(!str_fmv_overlay::is_resident(&ram));
    let off = (str_fmv_overlay::COMPACT_TABLE_ADDR - 0x80000000) as usize;
    ram[off..off + 9].copy_from_slice(b"MV1.STR;1");
    assert!(str_fmv_overlay::is_resident(&ram));
}

#[test]
fn fmv_overlay_mid_game_labels_are_lowercase_cdname_shape() {
    for label in str_fmv_overlay::MID_GAME_LABELS {
        assert!(!label.is_empty());
        assert!(label.len() <= 8, "{label} exceeds CDNAME slot width");
        assert!(
            label
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit()),
            "{label} not CDNAME-shape"
        );
    }
}

#[test]
fn fmv_overlay_mv_basenames_are_canonical_order() {
    let last_digit = |s: &str| s.chars().nth(2).unwrap().to_digit(10).unwrap();
    for (i, name) in str_fmv_overlay::MV_BASENAMES.iter().enumerate() {
        assert_eq!(last_digit(name), (i as u32) + 1);
    }
}

#[test]
fn vahn_fire_book_pattern_matches_pinned_capture() {
    // Pre-event has count=1, list=[0x0C], slot[1]=0x00.
    assert_eq!(vahn_fire_book_use::BEFORE, [0x01, 0x0C, 0x00]);
    // Post-event has count=2, list=[0x03, 0x0C].
    assert_eq!(vahn_fire_book_use::AFTER, [0x02, 0x03, 0x0C]);
    // Count byte incremented by 1 (regardless of interpretation).
    assert_eq!(
        vahn_fire_book_use::AFTER[0] - vahn_fire_book_use::BEFORE[0],
        1
    );
    // Pre-event entry at position 0 (`0x0C`) appears at position 1
    // post-event - consistent with insertion at the front.
    assert_eq!(vahn_fire_book_use::AFTER[2], vahn_fire_book_use::BEFORE[1]);
}

#[test]
fn battle_init_window_extents_consistent() {
    let (lo, hi) = battle_init_overlay::OVERLAY_SCRATCH_WINDOW;
    assert!(hi > lo);
    assert_eq!(hi - lo, 0x4810); // ~16 KB, matches captured diff extent
    let (lo, hi) = battle_init_overlay::BATTLE_BUNDLE_WINDOW;
    assert_eq!(hi - lo, 0x2BD34); // ~168 KB, matches captured diff extent
}

#[test]
fn battle_action_anim_offsets_are_in_actor_record() {
    // Actor record stride = 0x2D4. All anim-state offsets must fall
    // within a single record.
    let stride = 0x2D4u32;
    for &off in &[
        battle_action_animation::ANIM_PC_FIELD_OFFSET,
        battle_action_animation::ANIM_FRAME_FLAGS_OFFSET,
        battle_action_animation::ANIM_DISPATCH_PTR_TABLE_OFFSET,
    ] {
        assert!(
            off < stride,
            "+0x{off:X} should fit in the 0x{stride:X}-byte record"
        );
    }
}

#[test]
fn battle_action_anim_dispatch_table_size_is_4_pointers() {
    assert_eq!(
        battle_action_animation::ANIM_DISPATCH_PTR_TABLE_LEN,
        4 * std::mem::size_of::<u32>()
    );
}

#[test]
fn cutscene_corpus_covers_consecutive_fmv_ids_0_through_8() {
    let mut seen = std::collections::BTreeSet::new();
    for entry in cutscene_trigger_corpus::CORPUS {
        seen.insert(entry.expected_fmv_id);
    }
    let expected: std::collections::BTreeSet<i16> = (0..=8).collect();
    assert_eq!(seen, expected);
}

#[test]
fn cutscene_corpus_user_slot_assignments_match_capture_intent() {
    // The user captured slot 2 → STR 0, slot 3 → STR 1, ...,
    // slot 0 → STR 8. Encode that fingerprint here so the
    // corpus indices stay synchronised with the on-disc saves.
    let want = [
        (2u32, 0i16),
        (3, 1),
        (4, 2),
        (5, 3),
        (6, 4),
        (7, 5),
        (8, 6),
        (9, 7),
        (0, 8),
    ];
    for (i, entry) in cutscene_trigger_corpus::CORPUS.iter().enumerate() {
        assert_eq!(entry.slot, want[i].0);
        assert_eq!(entry.expected_fmv_id, want[i].1);
    }
}

#[test]
fn cutscene_corpus_readers_lift_planted_values() {
    let mut ram = vec![0u8; 0x200000];
    // Plant fmv_id = 5 (s16 LE).
    let off = (cutscene_trigger_corpus::FMV_ID_ADDR - 0x80000000) as usize;
    ram[off..off + 2].copy_from_slice(&5i16.to_le_bytes());
    assert_eq!(cutscene_trigger_corpus::read_fmv_id(&ram), Some(5));

    // Plant game mode = 0x1A.
    let off = (cutscene_trigger_corpus::GAME_MODE_ADDR - 0x80000000) as usize;
    ram[off] = 0x1A;
    assert_eq!(
        cutscene_trigger_corpus::read_game_mode(&ram),
        Some(cutscene_trigger_corpus::EXPECTED_GAME_MODE)
    );

    // Plant BGM id = 2000.
    let off = (cutscene_trigger_corpus::BGM_ID_ADDR - 0x80000000) as usize;
    ram[off..off + 2].copy_from_slice(&2000u16.to_le_bytes());
    assert_eq!(
        cutscene_trigger_corpus::read_bgm_id(&ram),
        Some(cutscene_trigger_corpus::EXPECTED_BGM_ID)
    );
}

#[test]
fn cutscene_corpus_field_pack_scan_finds_planted_trigger() {
    let mut ram = vec![0u8; 0x200000];
    let base = cutscene_trigger_corpus::MAP01_FIELD_PACK_BASE;
    let off = (base - 0x80000000) as usize;
    // Plant a `0x4C 0xE2 0x05 0x00` trigger op at base + 0x100.
    ram[off + 0x100] = 0x4C;
    ram[off + 0x101] = 0xE2;
    ram[off + 0x102] = 0x05;
    ram[off + 0x103] = 0x00;
    let hits = cutscene_trigger_corpus::scan_field_pack_for_trigger_ops(&ram, base, 0x200);
    assert_eq!(hits, vec![(base + 0x100, 5)]);

    // No matches in a zero-filled RAM image - confirming the
    // corpus's empirical "no trigger op found in field-pack"
    // observation when no op is planted.
    let zero = vec![0u8; 0x200000];
    let no_hits = cutscene_trigger_corpus::scan_field_pack_for_trigger_ops(&zero, base, 0x200);
    assert!(no_hits.is_empty());
}

#[test]
fn cutscene_corpus_map01_field_pack_base_round_trips() {
    // The corpus's pinned map01 field-pack base, plus the
    // EFFECT_OFFSET, should match the load-dest pointer value
    // observed in every save.
    let load_dest = cutscene_trigger_corpus::MAP01_FIELD_PACK_BASE + field_pack_load::EFFECT_OFFSET;
    assert_eq!(load_dest, 0x8014BD30);
}
