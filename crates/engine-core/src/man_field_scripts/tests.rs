//! Unit tests for `man_field_scripts`, extracted verbatim.

use super::*;
use legaia_asset::man_section::{ManFile, ManHeader};

/// Build a minimal one-partition-1-record MAN whose single record is a
/// field-VM script: `[N=0][4-byte header][0x37 yield with inline
/// count=1 id=0x4F][...]`. Exercises the record-walk + arm-site decode
/// without disc data.
fn synthetic_man_with_tetsu_arm() -> (ManFile, Vec<u8>) {
    // data_region_offset is arbitrary for the synthetic test; pick a
    // small value and lay the record body right after it.
    let data_region_offset = 0x40usize;
    let p1_0 = 0u32; // record 0 sits at the start of the data region.
    let script_start = data_region_offset + p1_0 as usize;

    // Record prefix: N=0 -> pc0 = 1 + 0 + 4 = 5.
    // Then a 0x37 yield whose inline window is [0x37][s0][s1][count=1][0x4F].
    let mut man = vec![0u8; script_start];
    man.push(0x00); // N = 0
    man.extend_from_slice(&[0xAA, 0xBB, 0xCC, 0xDD]); // 4-byte header
    // pc0 = 5: the yield opcode + inline record.
    man.push(0x37); // +0 yield opcode
    man.push(0x11); // +1 reserved
    man.push(0x22); // +2 reserved
    man.push(0x01); // +3 count = 1
    man.push(0x4F); // +4 monster id = Tetsu
    man.push(0x00); // +5 padding so the window has 8 bytes
    man.push(0x00);
    man.push(0x00);

    let header = ManHeader {
        status_flags: 0,
        low_flag: false,
        depth_lut: [0; 16],
        partition_counts: [0, 1, 0],
        u24_at_28: 0,
    };
    let man_file = ManFile {
        header,
        partitions: [vec![], vec![p1_0], vec![]],
        data_region_offset,
        // Sections all point past the script so they don't bound it.
        sections: std::array::from_fn(|_| legaia_asset::man_section::SectionRef {
            offset: man.len(),
            length: 0,
        }),
    };
    (man_file, man)
}

#[test]
fn walks_partition1_and_decodes_inline_tetsu_arm() {
    let (man_file, man) = synthetic_man_with_tetsu_arm();
    let records = walk_partition1_scripts(&man_file, &man);
    assert_eq!(records.len(), 1);
    let rec = &records[0];
    assert_eq!(rec.index, 0);
    assert_eq!(rec.pc0, 5);
    assert_eq!(rec.arm_sites.len(), 1, "one yield site");
    let site = &rec.arm_sites[0];
    assert_eq!(site.opcode, 0x37);
    assert!(!site.wide);
    let record = site.record.expect("inline window decodes");
    assert_eq!(record.count, 1);
    assert_eq!(record.monster_ids[0], 0x4F);
    assert!(site.matches_tetsu());
}

/// Build a MAN with two partition-1 records: record 0 (the scene
/// controller, skipped by `actor_placements`) and record 1 (a placed actor
/// whose `[N=0][model][actions][tx][tz]` header is followed by `script`).
fn man_with_placement_script(script: &[u8]) -> (ManFile, Vec<u8>) {
    let data_region_offset = 0x40usize;
    // Record 0: a minimal controller (`N=0`, header, halt).
    let rec0: &[u8] = &[0x00, 0, 0, 0, 0, 0x21];
    // Record 1: N=0, model=5, actions=0, tile (3,4), then the script.
    let mut rec1 = vec![0x00, 0x05, 0x00, 0x03, 0x04];
    rec1.extend_from_slice(script);

    let off0 = 0u32;
    let off1 = rec0.len() as u32;
    let mut man = vec![0u8; data_region_offset];
    man.extend_from_slice(rec0);
    man.extend_from_slice(&rec1);

    let header = ManHeader {
        status_flags: 0,
        low_flag: false,
        depth_lut: [0; 16],
        partition_counts: [0, 2, 0],
        u24_at_28: 0,
    };
    let man_file = ManFile {
        header,
        partitions: [vec![], vec![off0, off1], vec![]],
        data_region_offset,
        sections: std::array::from_fn(|_| legaia_asset::man_section::SectionRef {
            offset: man.len(),
            length: 0,
        }),
    };
    (man_file, man)
}

#[test]
fn classify_warp_script_is_a_portal() {
    // `0x3E` with op0 = 103 is a genuine door-warp to map id 103 - 100 = 3
    // (within the 7-id `WARP_OP0_RANGE`).
    let (mf, man) = man_with_placement_script(&[0x3E, 103, 0, 0, 0, 0]);
    let placements = mf.actor_placements(&man);
    assert_eq!(placements.len(), 1, "record 0 is the controller");
    assert_eq!(
        classify_placement(&mf, &man, &placements[0]),
        PlacementKind::Portal { target_map: 3 }
    );
}

#[test]
fn is_genuine_warp_gate() {
    // Base opcode, in-range op0 -> genuine (map_id 0..=6 -> op0 100..=106).
    assert!(is_genuine_warp(100, None)); // map_id 0
    assert!(is_genuine_warp(106, None)); // map_id 6
    // Out-of-range op0 (the desync phantoms: 175 / 179 / 200) -> rejected.
    assert!(!is_genuine_warp(107, None));
    assert!(!is_genuine_warp(200, None));
    // Cross-context `0x80`-prefixed warp -> rejected even with in-range op0.
    assert!(!is_genuine_warp(103, Some(0xF8)));
}

#[test]
fn classify_out_of_range_warp_is_not_a_portal() {
    // `0x3E` with op0 = 200 decodes as `is_warp` (op0 >= 100) but lands far
    // outside the 7-id door-warp range - the signature of a text-desynced
    // read (corpus: `geremi` op0=200, `other7` op0=175/179). With no inline
    // text after it, the actor is Plain, never a phantom portal to map 100.
    let (mf, man) = man_with_placement_script(&[0x3E, 200, 0, 0, 0, 0, 0x21]);
    let placements = mf.actor_placements(&man);
    assert_eq!(
        classify_placement(&mf, &man, &placements[0]),
        PlacementKind::Plain,
        "an out-of-range pseudo-warp must not classify as a portal"
    );
}

#[test]
fn scene_destinations_decodes_named_warp() {
    // A script with a 0x3F named scene-change to "dolk" (index 60, entry
    // tile bytes 0x10/0x20, dir 0x30) followed by a halt.
    let mut script = vec![0x3Fu8, 60, 0, 4];
    script.extend_from_slice(b"dolk");
    script.extend_from_slice(&[0x10, 0x20, 0x30, 0x21]);
    let (mf, man) = man_with_placement_script(&script);
    let dests = scene_destinations(&mf, &man);
    assert_eq!(
        dests,
        vec![SceneDestination {
            scene_name: "dolk".to_string(),
            index: 60,
            entry_x: 0x10,
            entry_z: 0x20,
        }]
    );
}

#[test]
fn scene_destinations_rejects_text_desync_name() {
    // A 0x3F whose "name" is uppercase/punctuation (a literal '?' inside
    // message text) is not a clean CDNAME label and is dropped.
    let mut script = vec![0x3Fu8, 0, 0, 4];
    script.extend_from_slice(b"Hi! ");
    script.extend_from_slice(&[0x00, 0x00, 0x00, 0x21]);
    let (mf, man) = man_with_placement_script(&script);
    assert!(scene_destinations(&mf, &man).is_empty());
}

#[test]
fn classify_interact_script_is_an_npc() {
    // `0x3E` with op0 < 100 is a field interact at index op1.
    let (mf, man) = man_with_placement_script(&[0x3E, 0x05, 0x07, 0x21]);
    let placements = mf.actor_placements(&man);
    assert_eq!(
        classify_placement(&mf, &man, &placements[0]),
        PlacementKind::Npc {
            interact_id: Some(0x07),
            dialog_inline: None,
        }
    );
}

#[test]
fn classify_plain_script_has_no_interaction() {
    // A bare halt: no warp / dialog / interact.
    let (mf, man) = man_with_placement_script(&[0x21]);
    let placements = mf.actor_placements(&man);
    assert_eq!(
        classify_placement(&mf, &man, &placements[0]),
        PlacementKind::Plain
    );
}

#[test]
fn first_inline_dialog_offset_finds_a_printable_segment() {
    // `[noise][0x1F "Hello" 0x00]` -> offset of the 0x1F.
    let body = [0x21u8, 0x25, 0x1F, b'H', b'e', b'l', b'l', b'o', 0x00, 0x21];
    assert_eq!(first_inline_dialog_offset(&body, 0), Some(2));
}

#[test]
fn first_inline_dialog_offset_rejects_a_stray_marker() {
    // A 0x1F followed by non-printable / too-short data is not a segment.
    let body = [0x1Fu8, 0x01, 0x02, 0x00, 0x1F, 0xAB, 0x00];
    assert_eq!(first_inline_dialog_offset(&body, 0), None);
}

#[test]
fn classify_inline_text_with_phantom_warp_byte_is_an_npc() {
    // A talk-NPC record whose message contains a literal '>' (0x3E, the
    // warp/interact opcode). The structural pass finds the 0x1F text block;
    // the desync gate ignores the '>' byte because it sits inside the text,
    // so the actor classifies as an Npc carrying the inline message - NOT a
    // phantom portal.
    let mut script = vec![0x25u8]; // a benign leading op
    script.extend_from_slice(&[0x1F]); // text-segment lead
    script.extend_from_slice(b"<Go north>"); // contains 0x3E ('>')
    script.push(0x00); // terminator
    let (mf, man) = man_with_placement_script(&script);
    let placements = mf.actor_placements(&man);
    let kind = classify_placement(&mf, &man, &placements[0]);
    match kind {
        PlacementKind::Npc { dialog_inline, .. } => {
            let inline = dialog_inline.expect("inline text captured");
            // Renders the segment text (after the 0x1F lead).
            let panel = crate::dialog::OwnedDialogPanel::from_inline_dialog(&inline);
            assert!(panel.is_some(), "inline buffer is renderable");
        }
        other => panic!("expected Npc, got {other:?}"),
    }
}

#[test]
fn classify_warp_wins_over_a_preceding_dialog() {
    // A talk-then-warp script (interact first, warp after) classifies as a
    // portal - the warp is the defining behaviour.
    let (mf, man) = man_with_placement_script(&[0x3E, 0x01, 0x09, 0x3E, 105, 0, 0, 0, 0]);
    let placements = mf.actor_placements(&man);
    assert_eq!(
        classify_placement(&mf, &man, &placements[0]),
        PlacementKind::Portal { target_map: 5 }
    );
}

/// Build a minimal one-partition-2-record MAN whose single record is a
/// field-VM script ending in `GFLAG_SET 26` (op `0x2E`, operand `0x1A`) -
/// the opening prologue's `town01` hand-off arm.
fn synthetic_man_with_gflag_set_26() -> (ManFile, Vec<u8>) {
    let data_region_offset = 0x40usize;
    let p2_0 = 0u32;
    let script_start = data_region_offset + p2_0 as usize;

    // Record prefix: N=0 -> pc0 = 5. Then GFLAG_SET 26.
    let mut man = vec![0u8; script_start];
    man.push(0x00); // N = 0
    man.extend_from_slice(&[0xAA, 0xBB, 0xCC, 0xDD]); // 4-byte header
    man.push(0x2E); // GFLAG_SET
    man.push(0x1A); // bit 26
    man.push(0x48); // a trailing no-op so the walk has a clean boundary

    let header = ManHeader {
        status_flags: 0,
        low_flag: false,
        depth_lut: [0; 16],
        partition_counts: [0, 0, 1],
        u24_at_28: 0,
    };
    let man_file = ManFile {
        header,
        partitions: [vec![], vec![], vec![p2_0]],
        data_region_offset,
        sections: std::array::from_fn(|_| legaia_asset::man_section::SectionRef {
            offset: man.len(),
            length: 0,
        }),
    };
    (man_file, man)
}

#[test]
fn walks_partition2_and_finds_gflag_set_26() {
    let (man_file, man) = synthetic_man_with_gflag_set_26();
    let sites = walk_partition_gflag_sites(&man_file, &man, 2);
    assert_eq!(sites.len(), 1, "one GFLAG site");
    let site = sites[0];
    assert_eq!(site.partition, 2);
    assert_eq!(site.record, 0);
    assert_eq!(site.opcode, 0x2E);
    assert!(site.set);
    assert_eq!(site.bit, 26);
    // The other partitions carry no records, hence no sites.
    assert!(walk_partition_gflag_sites(&man_file, &man, 0).is_empty());
    assert!(walk_partition_gflag_sites(&man_file, &man, 1).is_empty());
}

/// Build a one-partition-1-record MAN whose script is a SYSTEM-flag SET of
/// flag `0x193` (op `0x51`, operand `0x93` - `idx = (0x51 & 0x8F) << 8 | 0x93`)
/// followed by a SYSTEM-flag TEST of the same flag (op `0x71`), to exercise
/// the `0x50..=0x7F` walker arm + census tagging without disc data.
fn synthetic_man_with_system_flag_0x193() -> (ManFile, Vec<u8>) {
    let data_region_offset = 0x40usize;
    let p1_0 = 0u32;
    let script_start = data_region_offset + p1_0 as usize;

    let mut man = vec![0u8; script_start];
    man.push(0x00); // N = 0 -> pc0 = 5
    man.extend_from_slice(&[0xAA, 0xBB, 0xCC, 0xDD]); // 4-byte header
    man.push(0x51); // SYSTEM SET, high nibble 0x01
    man.push(0x93); // operand -> idx 0x0193
    // TEST op is 3 operand bytes wide (`[flag][i16 delta]`); give it a delta.
    man.push(0x71); // SYSTEM TEST, high nibble 0x01
    man.push(0x93); // operand -> idx 0x0193
    man.extend_from_slice(&[0x02, 0x00]); // i16 jump delta
    man.push(0x48); // trailing no-op for a clean boundary

    let header = ManHeader {
        status_flags: 0,
        low_flag: false,
        depth_lut: [0; 16],
        partition_counts: [0, 1, 0],
        u24_at_28: 0,
    };
    let man_file = ManFile {
        header,
        partitions: [vec![], vec![p1_0], vec![]],
        data_region_offset,
        sections: std::array::from_fn(|_| legaia_asset::man_section::SectionRef {
            offset: man.len(),
            length: 0,
        }),
    };
    (man_file, man)
}

#[test]
fn walk_surfaces_system_flag_set_and_test_sites() {
    let (man_file, man) = synthetic_man_with_system_flag_0x193();
    let sites = walk_partition_gflag_sites(&man_file, &man, 1);
    assert_eq!(sites.len(), 2, "one SET + one TEST system-flag site");

    let set = sites[0];
    assert_eq!(set.bank, FlagBank::System);
    assert_eq!(set.opcode, 0x51);
    assert_eq!(set.flag, 0x0193);
    assert_eq!(set.bit, 0x93, "low byte of the flag number");
    assert!(set.set);
    assert_eq!(set.kind, FlagKind::Set);

    let test = sites[1];
    assert_eq!(test.bank, FlagBank::System);
    assert_eq!(test.opcode, 0x71);
    assert_eq!(test.flag, 0x0193);
    assert!(!test.set, "TEST is not a SET");
    assert_eq!(test.kind, FlagKind::Test);
}

#[test]
fn scratchpad_gflag_site_is_tagged_scratchpad_bank() {
    // The existing prologue hand-off arm still reports as a scratchpad SET.
    let (man_file, man) = synthetic_man_with_gflag_set_26();
    let sites = walk_partition_gflag_sites(&man_file, &man, 2);
    assert_eq!(sites.len(), 1);
    assert_eq!(sites[0].bank, FlagBank::Scratchpad);
    assert_eq!(sites[0].flag, 26);
    assert_eq!(sites[0].bit, 26);
    assert!(sites[0].set);
}

#[test]
fn partition2_named_record_script_offset_matches_the_formula() {
    // name_len=6 (12 SJIS bytes), all three cond-blocks empty -> 0x10,
    // the opdeene record-18 shape.
    let mut body = vec![0x06];
    body.extend_from_slice(&[0xAA; 12]); // 6 SJIS chars
    body.extend_from_slice(&[0x00, 0x00, 0x00]); // C0=C1=C2=0
    body.push(0x34); // first opcode
    assert_eq!(partition2_record_script_offset(&body), Some(0x10));

    // Non-empty blocks: name_len=2 (4 bytes), C0=3 (3 bytes), C1=1 (2
    // bytes), C2=2 (4 bytes) -> 1 + 4 + (1+3) + (1+2) + (1+4) = 17.
    let mut body = vec![0x02, 0xAA, 0xAA, 0xAA, 0xAA];
    body.push(0x03); // C0 = 3
    body.extend_from_slice(&[0x11, 0x22, 0x33]);
    body.push(0x01); // C1 = 1 u16
    body.extend_from_slice(&[0x44, 0x55]);
    body.push(0x02); // C2 = 2 u16
    body.extend_from_slice(&[0x66, 0x77, 0x88, 0x99]);
    body.push(0x21); // first opcode
    assert_eq!(partition2_record_script_offset(&body), Some(17));
    assert_eq!(body[17], 0x21);

    // A count byte past the end returns None rather than panicking.
    assert_eq!(partition2_record_script_offset(&[0x06]), None);
}

/// Build a MAN whose partition 1 is `[controller, records...]`. Each
/// `records[i]` is a full placement record body
/// (`[N=0][model][actions][tx][tz][script...]`); `records[0]` is the
/// scene controller (skipped by `actor_placements`).
fn man_with_placements(records: &[Vec<u8>]) -> (ManFile, Vec<u8>) {
    let data_region_offset = 0x40usize;
    let mut man = vec![0u8; data_region_offset];
    let mut offsets = Vec::new();
    for rec in records {
        offsets.push((man.len() - data_region_offset) as u32);
        man.extend_from_slice(rec);
    }
    let header = ManHeader {
        status_flags: 0,
        low_flag: false,
        depth_lut: [0; 16],
        partition_counts: [0, records.len() as i16, 0],
        u24_at_28: 0,
    };
    let man_file = ManFile {
        header,
        partitions: [vec![], offsets, vec![]],
        data_region_offset,
        sections: std::array::from_fn(|_| legaia_asset::man_section::SectionRef {
            offset: man.len(),
            length: 0,
        }),
    };
    (man_file, man)
}

#[test]
fn derive_field_carriers_maps_sparring_carrier_and_npcs() {
    use crate::encounter_record::{
        RIM_ELM_SPARRING_CARRIER_MODEL, RIM_ELM_SPARRING_CARRIER_TILE,
        RIM_ELM_TRAINING_FORMATION_ID,
    };
    let (tx, tz) = RIM_ELM_SPARRING_CARRIER_TILE;
    // controller (idx 0), sparring carrier (idx 1, pinned tile/model + dialog),
    // a plain talk NPC (idx 2, dialog), a portal (idx 3), a decorative actor
    // (idx 4, halt only).
    let controller = vec![0x00, 0x00, 0x00, 0x00, 0x00, 0x21];
    let mut sparring = vec![0x00, RIM_ELM_SPARRING_CARRIER_MODEL, 0x00, tx, tz];
    sparring.extend_from_slice(&[0x1F, b's', b'p', b'a', b'r', 0x00]);
    let mut npc = vec![0x00, 0x10, 0x00, 10, 12];
    npc.extend_from_slice(&[0x1F, b'h', b'i', b'!', 0x00]);
    let portal = vec![0x00, 0x11, 0x00, 5, 5, 0x3E, 103, 0, 0, 0, 0];
    let decorative = vec![0x00, 0x12, 0x00, 6, 6, 0x21];
    let (mf, man) = man_with_placements(&[controller, sparring, npc, portal, decorative]);

    let carriers = derive_field_carriers(&mf, &man);
    // Portal + decorative carry no engageable carrier; only the sparring
    // partner and the talk NPC survive.
    assert_eq!(carriers.len(), 2, "portal + decorative are skipped");

    // The sparring carrier is first and maps to the training formation.
    assert_eq!(carriers[0].placement_index, 1);
    assert_eq!(carriers[0].tile, RIM_ELM_SPARRING_CARRIER_TILE);
    assert_eq!(carriers[0].model, RIM_ELM_SPARRING_CARRIER_MODEL);
    assert_eq!(
        carriers[0].config,
        FieldCarrierConfig::ScriptedEncounter {
            formation_id: RIM_ELM_TRAINING_FORMATION_ID
        }
    );

    // The plain talk NPC maps to an Npc carrier keyed by its record index.
    assert_eq!(carriers[1].placement_index, 2);
    assert_eq!(
        carriers[1].config,
        FieldCarrierConfig::Npc { interact_id: 2 }
    );
}

#[test]
fn placement_motion_route_keeps_local_own_context_runs_only() {
    // Placement at tile (10, 10) -> world (1344, 1344). Script:
    //   NPC_RUN -> (11, 10)        kept (local)
    //   NPC_RUN -> (11, 10)        dropped (consecutive duplicate)
    //   NPC_RUN -> (10, 11)        kept (local)
    //   NPC_RUN -> (127, 127)      dropped (park sentinel)
    //   NPC_RUN -> (60, 60)        dropped (beyond NPC_ROUTE_LOCALITY)
    //   cross-context NPC_RUN      dropped (drives another channel)
    let script = [
        0x4C, 0x51, 11, 10, 0, 5, //
        0x4C, 0x51, 11, 10, 3, 5, //
        0x4C, 0x51, 10, 11, 0, 5, //
        0x4C, 0x51, 0x7F, 0x7F, 0, 5, //
        0x4C, 0x51, 60, 60, 0, 5, //
        0xCC, 0x07, 0x51, 11, 11, 0, 5, // 0x4C | 0x80 prefix, target 0x07
        0x21,
    ];
    let (mf, man) = man_with_placement_script(&script);
    let placements = mf.actor_placements(&man);
    // Re-anchor the placement world position for the test: the helper
    // places it at tile (3, 4); use a placement-local route instead.
    let mut p = placements[0].clone();
    p.world_x = 10 * 0x80 + 0x40;
    p.world_z = 10 * 0x80 + 0x40;
    let route = placement_motion_route(&mf, &man, &p);
    assert_eq!(
        route,
        vec![
            (grid_byte_to_world(11), grid_byte_to_world(10)),
            (grid_byte_to_world(10), grid_byte_to_world(11)),
        ]
    );
}

#[test]
fn grid_byte_to_world_decodes_half_tiles() {
    assert_eq!(grid_byte_to_world(0), 0x40);
    assert_eq!(grid_byte_to_world(10), 10 * 0x80 + 0x40);
    assert_eq!(grid_byte_to_world(10 | 0x80), 10 * 0x80 + 0x80);
}

#[test]
fn walk_touch_event_classifies_portal_and_player_moveto() {
    // A genuine door-warp placement -> Warp.
    let (mf, man) = man_with_placement_script(&[0x3E, 103, 0, 0, 0, 0]);
    let placements = mf.actor_placements(&man);
    assert_eq!(
        placement_walk_touch_event(&mf, &man, &placements[0]),
        Some(WalkTouchEvent::Warp { target_map: 3 })
    );

    // A cross-context player-channel MOVE_TO (`0xA3 0xF8 xb zb`) ->
    // PlayerMoveTo at the decoded world coords.
    let (mf, man) = man_with_placement_script(&[0xA3, 0xF8, 20, 30, 0x21]);
    let placements = mf.actor_placements(&man);
    assert_eq!(
        placement_walk_touch_event(&mf, &man, &placements[0]),
        Some(WalkTouchEvent::PlayerMoveTo {
            world_x: grid_byte_to_world(20),
            world_z: grid_byte_to_world(30),
        })
    );

    // An own-context MOVE_TO (the actor repositioning itself) is NOT a
    // walk-touch event; neither is a bare halt.
    let (mf, man) = man_with_placement_script(&[0x23, 20, 30, 0x21]);
    let placements = mf.actor_placements(&man);
    assert_eq!(placement_walk_touch_event(&mf, &man, &placements[0]), None);
    let (mf, man) = man_with_placement_script(&[0x21]);
    let placements = mf.actor_placements(&man);
    assert_eq!(placement_walk_touch_event(&mf, &man, &placements[0]), None);
}

#[test]
fn parked_placement_carries_no_walk_touch() {
    // Same warp script, but the placement itself parks at the (127, 127)
    // sentinel tile - no touchable body, so no walk-touch event.
    let (mf, man) = man_with_placement_script(&[0x3E, 103, 0, 0, 0, 0]);
    let placements = mf.actor_placements(&man);
    let mut p = placements[0].clone();
    p.tile_x = 0x7F;
    p.tile_z = 0x7F;
    assert_eq!(placement_walk_touch_event(&mf, &man, &p), None);
}

#[test]
fn empty_partition1_yields_no_records() {
    let header = ManHeader {
        status_flags: 0,
        low_flag: false,
        depth_lut: [0; 16],
        partition_counts: [0, 0, 0],
        u24_at_28: 0,
    };
    let man_file = ManFile {
        header,
        partitions: [vec![], vec![], vec![]],
        data_region_offset: 0x2B,
        sections: std::array::from_fn(|_| legaia_asset::man_section::SectionRef {
            offset: 0x2B,
            length: 0,
        }),
    };
    let man = vec![0u8; 0x80];
    assert!(walk_partition1_scripts(&man_file, &man).is_empty());
}
