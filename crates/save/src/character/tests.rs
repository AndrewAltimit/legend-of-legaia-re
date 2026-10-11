use super::*;

#[test]
fn level_survives_an_ability_bitfield_rebuild() {
    // The aggregator zeroes and rebuilds `+0xF4..+0x104` every pass; the
    // level must not live inside that window.
    let mut rec = CharacterRecord::zeroed();
    rec.set_level(37);
    rec.set_ability_bits([0; ABILITY_BITS_LEN]);
    rec.set_ability_bits([0xFF; ABILITY_BITS_LEN]);
    assert_eq!(rec.level(), 37);
    assert_eq!(rec.magic_rank(), 37);
    assert_eq!(rec.raw[0x130], 37);
}

#[test]
fn cumulative_xp_round_trip() {
    let mut r = CharacterRecord::zeroed();
    r.set_cumulative_xp(518_462);
    assert_eq!(r.cumulative_xp(), 518_462);
    // Verify the bytes land at +0x00..+0x04 in LE (0x7E93E).
    assert_eq!(r.raw[0x00], 0x3E);
    assert_eq!(r.raw[0x01], 0xE9);
    assert_eq!(r.raw[0x02], 0x07);
    assert_eq!(r.raw[0x03], 0x00);
}

#[test]
fn next_level_xp_round_trip() {
    let mut r = CharacterRecord::zeroed();
    r.set_next_level_xp(121);
    assert_eq!(r.next_level_xp(), 121);
    // Verify the bytes land at +0x04..+0x08 in LE.
    assert_eq!(r.raw[0x04], 121);
    assert_eq!(r.raw[0x05], 0);
    // Disjoint from cumulative XP at +0x00.
    assert_eq!(r.cumulative_xp(), 0);
}

#[test]
fn cumulative_xp_default_is_zero() {
    let r = CharacterRecord::zeroed();
    assert_eq!(r.cumulative_xp(), 0);
}

#[test]
fn zeroed_record_round_trips_to_all_zeros() {
    let r = CharacterRecord::zeroed();
    let bytes = r.write();
    assert_eq!(bytes.len(), CHARACTER_RECORD_SIZE);
    assert!(bytes.iter().all(|&b| b == 0));
}

#[test]
fn parse_rejects_wrong_size() {
    assert!(CharacterRecord::parse(&[0u8; 0x100]).is_err());
    assert!(CharacterRecord::parse(&[0u8; 0x500]).is_err());
    assert!(CharacterRecord::parse(&[]).is_err());
}

#[test]
fn parse_then_write_round_trips_arbitrary_bytes() {
    // Synthesise a buffer with a recognisable pattern in every byte
    // so any field-level corruption shows up in the comparison.
    let buf: Vec<u8> = (0..CHARACTER_RECORD_SIZE)
        .map(|i| ((i * 37) ^ 0x5A) as u8)
        .collect();
    let r = CharacterRecord::parse(&buf).unwrap();
    let out = r.write();
    assert_eq!(buf, out);
}

#[test]
fn typed_setters_round_trip_through_raw_bytes() {
    let mut r = CharacterRecord::zeroed();
    let hms = HpMpSp {
        hp_cur: 250,
        hp_max: 999,
        mp_cur: 30,
        mp_max: 80,
        sp_cur: 10,
        sp_max: 50,
    };
    r.set_hp_mp_sp(hms);
    r.set_stat_cap(STAT_CAP);
    let bits = [0xAB; ABILITY_BITS_LEN];
    r.set_ability_bits(bits);
    let eq = EquipmentSlots {
        slots: [0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17],
    };
    r.set_equipment(eq);
    let mut ids = [0u8; MAX_SPELLS];
    ids[..5].copy_from_slice(&[1, 2, 3, 4, 5]);
    let mut levels = [0u8; MAX_SPELLS];
    levels[..5].copy_from_slice(&[10, 20, 30, 40, 50]);
    let spells = SpellList {
        count: 5,
        ids,
        levels,
    };
    r.set_spell_list(spells);

    // Round-trip via parse/write.
    let bytes = r.write();
    let r2 = CharacterRecord::parse(&bytes).unwrap();
    assert_eq!(r2.hp_mp_sp(), hms);
    assert_eq!(r2.stat_cap(), STAT_CAP);
    assert_eq!(r2.ability_bits(), bits);
    assert_eq!(r2.equipment(), eq);
    let s2 = r2.spell_list();
    assert_eq!(s2.count, 5);
    assert_eq!(&s2.ids[..5], &[1, 2, 3, 4, 5]);
    assert_eq!(&s2.levels[..5], &[10, 20, 30, 40, 50]);
}

#[test]
fn typed_setters_preserve_untouched_bytes() {
    // Fill the record with a non-zero pattern, then write only ONE
    // typed field - every other byte must survive unchanged.
    let mut buf = vec![0xCC; CHARACTER_RECORD_SIZE];
    // Pre-zero the byte we'll write through so the comparison is clean.
    buf[0x120] = 0;
    buf[0x121] = 0;
    let mut r = CharacterRecord::parse(&buf).unwrap();
    r.set_stat_cap(STAT_CAP);
    let out = r.write();
    // stat_cap bytes should reflect the new value.
    assert_eq!(&out[0x120..0x122], &STAT_CAP.to_le_bytes());
    // Every other byte should be the original 0xCC.
    for (i, &b) in out.iter().enumerate() {
        if !(0x120..0x122).contains(&i) {
            assert_eq!(b, 0xCC, "byte at offset 0x{i:X} corrupted by set_stat_cap");
        }
    }
}

/// `stat_cap` aliases the same `+0x120` field as
/// `record_stats().cap_constant` - name and offset agree with the
/// cheat-derived layout (`stat_cap_constant_100(+0x120)`), and the
/// live INT stat at `+0x11A` is untouched by the setter.
#[test]
fn stat_cap_aliases_record_cap_constant() {
    let mut r = CharacterRecord::zeroed();
    r.set_stat_cap(RECORD_CAP_CONSTANT);
    assert_eq!(r.record_stats().cap_constant, RECORD_CAP_CONSTANT);
    assert_eq!(r.live_stats().int, 0, "live INT (+0x11A) must not move");
    let mut s = r.record_stats();
    s.cap_constant = 123;
    r.set_record_stats(s);
    assert_eq!(r.stat_cap(), 123);
}

/// The Fire Book I capture pair (battle command menu parked on Fire
/// Book I → Fire Book I just used on Vahn) shows Vahn's record
/// changing at `+0x185..+0x188` from `[0x01, 0x0C, 0x00]` to
/// `[0x02, 0x03, 0x0C]`. The typed accessor must read those exact
/// bytes back as the same list shape (count + head-inserted ID).
#[test]
fn displayed_skills_reads_fire_book_capture_pattern() {
    let mut r = CharacterRecord::zeroed();
    // Pre-event: count=1, ids=[0x0C, ...].
    r.raw[0x185] = 0x01;
    r.raw[0x186] = 0x0C;
    let before = r.displayed_skills();
    assert_eq!(before.count, 1);
    assert_eq!(before.ids[0], 0x0C);
    assert_eq!(before.ids[1], 0x00);

    // Post-event: count=2, ids=[0x03, 0x0C, ...] (head insert).
    r.raw[0x185] = 0x02;
    r.raw[0x186] = 0x03;
    r.raw[0x187] = 0x0C;
    let after = r.displayed_skills();
    assert_eq!(after.count, 2);
    assert_eq!(after.ids[0], 0x03);
    assert_eq!(after.ids[1], 0x0C);
    assert_eq!(after.count - before.count, 1);
}

/// Setter writes back to `+0x185..+0x196` and round-trips through `parse`.
#[test]
fn displayed_skills_round_trip() {
    let mut r = CharacterRecord::zeroed();
    let mut ids = [0u8; MAX_DISPLAYED_SKILLS];
    ids[..3].copy_from_slice(&[0x03, 0x0C, 0x1F]);
    let list = DisplayedSkillList { count: 3, ids };
    r.set_displayed_skills(list);
    assert_eq!(r.raw[0x185], 0x03);
    assert_eq!(&r.raw[0x186..0x186 + 3], &[0x03, 0x0C, 0x1F]);
    let bytes = r.write();
    let r2 = CharacterRecord::parse(&bytes).unwrap();
    let read_back = r2.displayed_skills();
    assert_eq!(read_back.count, 3);
    assert_eq!(read_back.ids[..3], [0x03, 0x0C, 0x1F]);
}

/// Count clamps at `MAX_DISPLAYED_SKILLS` to keep the byte field
/// within the +0x185..+0x196 window before the equipment slots.
#[test]
fn displayed_skills_count_clamps_at_max() {
    let mut r = CharacterRecord::zeroed();
    let list = DisplayedSkillList {
        count: 200,
        ids: [0xAA; MAX_DISPLAYED_SKILLS],
    };
    r.set_displayed_skills(list);
    assert_eq!(r.raw[0x185], MAX_DISPLAYED_SKILLS as u8);
    // The equipment-slot byte at +0x196 must not be clobbered.
    assert_eq!(r.raw[0x196], 0x00);
}

#[test]
fn active_spell_slot_round_trips_and_rejects_out_of_range() {
    let mut r = CharacterRecord::zeroed();
    let payload = [0x42u8; 0x14];
    assert!(r.set_active_spell_slot(0, payload));
    assert!(r.set_active_spell_slot(MAX_ACTIVE_SPELL_SLOTS - 1, payload));
    assert!(!r.set_active_spell_slot(MAX_ACTIVE_SPELL_SLOTS, payload));
    assert_eq!(r.active_spell_slot(0), Some(payload));
    assert_eq!(r.active_spell_slot(MAX_ACTIVE_SPELL_SLOTS), None);
}

#[test]
fn party_round_trip_preserves_every_member() {
    let mut p = Party::zeroed(4);
    for (i, m) in p.members.iter_mut().enumerate() {
        m.set_stat_cap((i as u16 + 1) * 100);
        let spells = SpellList {
            count: i as u8,
            ..SpellList::default()
        };
        m.set_spell_list(spells);
    }
    let bytes = p.write();
    assert_eq!(bytes.len(), 4 * CHARACTER_RECORD_SIZE);
    let p2 = Party::parse(&bytes).unwrap();
    assert_eq!(p2.members.len(), 4);
    for (i, m) in p2.members.iter().enumerate() {
        assert_eq!(m.stat_cap(), (i as u16 + 1) * 100);
        assert_eq!(m.spell_list().count, i as u8);
    }
}

#[test]
fn party_parse_rejects_misaligned_buffer() {
    assert!(Party::parse(&[]).is_err());
    assert!(Party::parse(&[0u8; 0x100]).is_err());
    assert!(Party::parse(&[0u8; 0x415]).is_err());
}

/// The "Max AGL / ATK / UDF / LDF / SPD / INT" GameShark cheats
/// each have two address sites per character: one in the live
/// window (`+0x110..+0x11B`) and one in the record window
/// (`+0x122..+0x12D`). Verify that writing the live window and
/// reading it back round-trips through the typed accessor.
#[test]
fn live_stats_round_trip() {
    let mut r = CharacterRecord::zeroed();
    let s = LiveStats {
        agl: 0x3E7,
        atk: 0x3E7,
        udf: 0x3E7,
        ldf: 0x3E7,
        spd: 0x3E7,
        int: 0x3E7,
    };
    r.set_live_stats(s);
    let read_back = r.live_stats();
    assert_eq!(read_back, s);
    // The bytes should land at the cheat-pinned offsets.
    assert_eq!(
        &r.raw[0x110..0x11C],
        &[
            0xE7, 0x03, 0xE7, 0x03, 0xE7, 0x03, 0xE7, 0x03, 0xE7, 0x03, 0xE7, 0x03,
        ]
    );
}

/// Record-side stats round-trip including the cap constant at
/// `+0x120`.
#[test]
fn record_stats_round_trip_pins_cap_constant() {
    let mut r = CharacterRecord::zeroed();
    let s = RecordStats {
        hp_max: 0x270F,
        mp_max: 0x03E7,
        cap_constant: RECORD_CAP_CONSTANT,
        agl: 50,
        atk: 60,
        udf: 70,
        ldf: 80,
        spd: 90,
        int: 100,
    };
    r.set_record_stats(s);
    assert_eq!(r.record_stats(), s);
    // Cap constant lands at +0x120 LE.
    assert_eq!(&r.raw[0x120..0x122], &[100, 0]);
}

/// The "Level 99" GameShark cheat writes `0x63` to `+0x130`.
/// Our accessor calls this byte `magic_rank`; the test asserts
/// the read/write reaches the same bytes the cheat targets.
#[test]
fn magic_rank_writes_byte_at_offset_0x130() {
    let mut r = CharacterRecord::zeroed();
    r.set_magic_rank(99);
    assert_eq!(r.raw[0x130], 99);
    assert_eq!(r.magic_rank(), 99);
}

/// "All Summons Level 9" stamps `0x09` into 16 bytes at
/// `+0x161..+0x178`.
#[test]
fn summon_levels_round_trip_16_bytes() {
    let mut r = CharacterRecord::zeroed();
    let levels = [0x09u8; SUMMON_SLOT_COUNT];
    r.set_summon_levels(levels);
    assert_eq!(r.summon_levels(), levels);
    assert!(
        r.raw[0x161..0x161 + SUMMON_SLOT_COUNT]
            .iter()
            .all(|&b| b == 0x09)
    );
    // Bytes outside the window must be untouched.
    assert_eq!(r.raw[0x160], 0);
    assert_eq!(r.raw[0x161 + SUMMON_SLOT_COUNT], 0);
}

/// Magic-slot-activator gate at `+0x13C`. Cheat sets to 0x24.
#[test]
fn magic_slot_activator_writes_byte_at_offset_0x13c() {
    let mut r = CharacterRecord::zeroed();
    r.set_magic_slot_activator(0x24);
    assert_eq!(r.raw[0x13C], 0x24);
    assert_eq!(r.magic_slot_activator(), 0x24);
}

/// Magic groups: three 12-byte windows starting at `+0x13D`.
#[test]
fn magic_groups_round_trip_three_windows() {
    let mut r = CharacterRecord::zeroed();
    let mut groups = [[0u8; MAGIC_GROUP_STRIDE]; MAGIC_GROUP_COUNT];
    for (i, group) in groups.iter_mut().enumerate() {
        for (j, slot) in group.iter_mut().enumerate() {
            *slot = (i as u8) * 16 + (j as u8) + 1;
        }
    }
    r.set_magic_groups(groups);
    let read_back = r.magic_groups();
    assert_eq!(read_back, groups);
    // First byte of group 0 lands at +0x13D.
    assert_eq!(r.raw[0x13D], 1);
    // First byte of group 1 lands at +0x149.
    assert_eq!(r.raw[0x149], 17);
    // First byte of group 2 lands at +0x155.
    assert_eq!(r.raw[0x155], 33);
    // Last byte of group 2 lands at +0x160.
    assert_eq!(r.raw[0x160], 33 + 11);
}

/// Snapshot now includes the cheat-derived fields.
#[test]
fn snapshot_includes_new_cheat_derived_fields() {
    let mut r = CharacterRecord::zeroed();
    r.set_magic_rank(42);
    r.set_magic_slot_activator(0x24);
    r.set_record_stats(RecordStats {
        cap_constant: RECORD_CAP_CONSTANT,
        ..RecordStats::default()
    });
    let snap = r.snapshot();
    assert_eq!(snap.magic_rank, 42);
    assert_eq!(snap.magic_slot_activator, 0x24);
    assert_eq!(snap.record_stats.cap_constant, RECORD_CAP_CONSTANT);
    assert_eq!(snap.summon_levels.len(), SUMMON_SLOT_COUNT);
}

/// The two auto command strings round-trip through the raw buffer and do
/// not overlap each other or the equipment slots that precede them.
#[test]
fn auto_command_strings_round_trip_and_stay_disjoint() {
    let mut r = CharacterRecord::zeroed();
    let a: [u8; AUTO_COMMAND_STRING_LEN] = core::array::from_fn(|i| (i as u8) + 1);
    let b: [u8; AUTO_COMMAND_STRING_LEN] = core::array::from_fn(|i| 0x80 + i as u8);
    r.set_equipment(EquipmentSlots { slots: [9; 8] });
    r.set_auto_command_string(AutoCommandBand::Primary, a);
    r.set_auto_command_string(AutoCommandBand::Secondary, b);
    let back = CharacterRecord::parse(&r.write()).expect("round-trip");
    assert_eq!(back.auto_command_string(AutoCommandBand::Primary), a);
    assert_eq!(back.auto_command_string(AutoCommandBand::Secondary), b);
    assert_eq!(back.equipment().slots, [9; 8]);
    assert_eq!(
        AUTO_COMMAND_STRING_A_OFFSET + AUTO_COMMAND_STRING_LEN,
        AUTO_COMMAND_STRING_B_OFFSET
    );
    const { assert!(AUTO_COMMAND_STRING_A_OFFSET >= 0x196 + EQUIPMENT_SLOT_COUNT) };
    const { assert!(AUTO_COMMAND_STRING_B_OFFSET + AUTO_COMMAND_STRING_LEN <= NAME_OFFSET) };
}

/// The band selector is the retail `sltu(base, live)` at `0x801DA3A4`:
/// strictly-below picks the primary slot, everything else the secondary.
#[test]
fn auto_command_band_follows_the_gauge_compare() {
    assert_eq!(
        AutoCommandBand::for_gauge(100, 80),
        AutoCommandBand::Primary
    );
    assert_eq!(
        AutoCommandBand::for_gauge(80, 80),
        AutoCommandBand::Secondary
    );
    assert_eq!(
        AutoCommandBand::for_gauge(60, 80),
        AutoCommandBand::Secondary
    );
}
