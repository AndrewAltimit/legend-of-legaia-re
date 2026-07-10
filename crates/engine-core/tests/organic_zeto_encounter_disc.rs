//! Disc-gated runtime oracle: the **Zeto** boss fight in `garmel` enters
//! organically through the encounter-record path - the scene's own script
//! bytes select the MAN formation-table row - not through a hard-coded
//! engine stamp.
//!
//! The retail chain (live-capture pinned: `FUN_801DA51C` body
//! `0x801DA620..0x801DA678` copies `count=1, record[4]=0x4B` into
//! `0x8007BD0C` at the battle-launch tick; `0x8007B7FC` stays silent):
//!
//! 1. `garmel`'s beat record `P2[12]` (C1 gate `[0x198]`, the self-latching
//!    one-shot: the record's own head `51 98` SETs it) plays the pre-boss
//!    cutscene and ends in the field-VM scripted-battle op **`3E FF 09`**.
//! 2. The op's case-0x3E interact arm points the system entity's `+0x94` at
//!    formation-table row 9 (`sys[+0x8A] = 1`,
//!    `sys[+0x94] = *(ctrl+0x20) + 9 * stride + 1`) and requests the battle
//!    mode switch; the entity SM's confirm state copies the row into the
//!    formation cell.
//! 3. Row 9 of garmel's MAN encounter section is `[01 00 00][count=1][0x4B]` -
//!    the lone-Zeto formation, sitting OUTSIDE every region's rollable
//!    `[base, base+count)` slice (it can only enter through the scripted op).
//!
//! The engine mirrors the chain end-to-end from disc bytes: scene entry
//! installs the MAN formation rows (+ the PROT 867 archive stats for their
//! monster ids), the walk-on/beat record executes through the field VM, and
//! its `3E FF 09` routes through `World::trigger_scripted_battle` into the
//! formation-table row - no `SCRIPTED_SCENE_BOSSES` entry, no synthetic
//! formation id.
//!
//! Skip-passes without disc data / extracted assets (CLAUDE.md convention).

use legaia_engine_core::scene::SceneHost;
use std::path::PathBuf;

fn extracted_dir() -> Option<PathBuf> {
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

fn gated() -> Option<PathBuf> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        return None;
    };
    Some(extracted)
}

/// Zeto's monster-archive id (PROT 867; lone slot of garmel formation row 9).
const ZETO_MONSTER_ID: u16 = 0x4B;
/// The garmel MAN formation-table row the beat record's `3E FF 09` selects.
const ZETO_FORMATION_ROW: u16 = 9;
/// The Zeto beat record (partition-2 index) carrying the `3E FF 09` op.
const ZETO_BEAT_RECORD: usize = 12;
/// The beat record's C1 gate flag - SET by the record's own head (`51 98`),
/// so the beat (and the fight behind it) is a self-latching one-shot.
const ZETO_GATE_FLAG: u16 = 0x198;

/// Static disc facts: garmel's MAN carries the lone-Zeto formation row, the
/// beat record that selects it by index, and the self-latch gate - all as
/// disc bytes, none as engine constants.
#[test]
fn garmel_man_carries_the_zeto_formation_row_and_beat_record() {
    let Some(extracted) = gated() else { return };
    let mut host = SceneHost::open_extracted(&extracted).expect("open SceneHost");
    host.enter_field_scene("garmel", 0).expect("enter garmel");
    let man_bytes = host
        .scene
        .as_ref()
        .unwrap()
        .field_man_payload(&host.index)
        .expect("MAN payload read")
        .expect("garmel resolves its bundle MAN");
    let man_file = legaia_asset::man_section::parse(&man_bytes).expect("MAN parses");

    // Formation row 9 = lone monster 0x4B (count 1). Read straight off the
    // encounter section.
    let record = legaia_engine_core::encounter_man::formation_record_for_row(
        &man_bytes,
        ZETO_FORMATION_ROW as usize,
    )
    .expect("garmel formation row 9 decodes");
    assert_eq!(record.count, 1, "row 9 is a lone-monster formation");
    assert_eq!(
        record.monster_ids[0], ZETO_MONSTER_ID as u8,
        "row 9's lone slot is Zeto"
    );

    // The row sits OUTSIDE every region's rollable slice: it can never come
    // from the random roll, only from the scripted op.
    let region_table =
        legaia_engine_core::region_encounter::region_encounter_table_from_man("garmel", &man_bytes)
            .expect("garmel region table decodes");
    for region in &region_table.regions {
        let base = u16::from(region.formation_base);
        let end = base + u16::from(region.formation_count);
        assert!(
            !(base..end).contains(&ZETO_FORMATION_ROW),
            "row 9 is outside region slice [{base}, {end})"
        );
    }

    // The beat record P2[12]: C1 gate [0x198], and its body carries the
    // `3E FF 09` scripted-battle op exactly once.
    let (c1, c2) = legaia_engine_core::man_field_scripts::partition2_record_gates(
        &man_file,
        &man_bytes,
        ZETO_BEAT_RECORD,
    )
    .expect("P2[12] gates decode");
    assert_eq!(c1, vec![ZETO_GATE_FLAG], "P2[12] C1 gate is [0x198]");
    assert!(c2.is_empty(), "P2[12] has no C2 gate");
    let (start, pc0, len) = legaia_engine_core::man_field_scripts::partition_record_span(
        &man_file,
        &man_bytes,
        2,
        ZETO_BEAT_RECORD,
    )
    .expect("P2[12] span resolves");
    let body = &man_bytes[start..start + len];
    let hits = body
        .windows(3)
        .filter(|w| *w == [0x3E, 0xFF, ZETO_FORMATION_ROW as u8])
        .count();
    assert_eq!(hits, 1, "P2[12] carries `3E FF 09` exactly once");
    // The record's first opcode is its own C1 SET (`51 98`) - the self-latch.
    assert_eq!(
        &body[pc0..pc0 + 2],
        &[0x51, 0x98],
        "P2[12] head SETs its own gate flag 0x198"
    );
    eprintln!("[garmel] static chain verified: row 9 = [0x4B], P2[12] `3E FF 09`, C1 [0x198]");
}

/// Full organic runtime slice: enter garmel with no flags seeded, spawn the
/// beat record through the gated partition-2 dispatch, and let its own script
/// bytes select + enter the Zeto fight. Asserts the battle formation is the
/// MAN row (id 9, lone 0x4B) with the archive's real stats - and that the
/// gate flag landed from the record head, not from any engine latch.
#[test]
fn zeto_battle_enters_organically_from_the_beat_record() {
    let Some(extracted) = gated() else { return };
    let mut host = SceneHost::open_extracted(&extracted).expect("open SceneHost");
    host.world.live_gameplay_loop = true;
    host.enter_field_scene("garmel", 0).expect("enter garmel");

    // Baseline (non-vacuous): the gate flag is clear, no boss pre-armed, and
    // the engine's scripted-boss table has NO garmel row.
    assert!(
        !host.world.system_flag_test(ZETO_GATE_FLAG),
        "baseline: gate flag 0x198 clear on first visit"
    );
    assert!(
        !host.world.scripted_formation_pending,
        "baseline: nothing pre-armed at scene entry"
    );
    assert!(
        !legaia_engine_core::world::SCRIPTED_SCENE_BOSSES
            .iter()
            .any(|&(scene, ..)| scene == "garmel"),
        "garmel has no synthetic scripted-boss row - the fight is record-borne"
    );

    // Spawn the beat record through the real gated dispatch (the walk-on
    // trigger's install path, C1/C2 evaluated against the live flag bank).
    let man_bytes = host
        .scene
        .as_ref()
        .unwrap()
        .field_man_payload(&host.index)
        .expect("MAN payload read")
        .expect("garmel resolves its bundle MAN");
    let man_file = legaia_asset::man_section::parse(&man_bytes).expect("MAN parses");
    assert!(
        host.world
            .install_gated_p2_record(&man_file, &man_bytes, ZETO_BEAT_RECORD),
        "the gated dispatch admits P2[12] while 0x198 is clear"
    );

    // Drive the record. Toggle confirm across ticks so its inline dialog
    // pages advance on fresh press edges (the record is a conversation-heavy
    // cutscene).
    let cross_mask = legaia_engine_core::input::PadButton::Cross.mask();
    let mut ticks = 0u32;
    let mut entered_battle = false;
    while ticks < 8000 {
        host.world.input.set_pad(if ticks.is_multiple_of(2) {
            cross_mask
        } else {
            0
        });
        host.tick().expect("tick");
        ticks += 1;
        if matches!(
            host.world.mode,
            legaia_engine_core::world::SceneMode::Battle
        ) {
            entered_battle = true;
            break;
        }
    }
    assert!(
        entered_battle,
        "P2[12]'s `3E FF 09` entered battle within {ticks} ticks"
    );
    // The gate flag landed from the record's own head SET (`51 98`).
    assert!(
        host.world.system_flag_test(ZETO_GATE_FLAG),
        "P2[12]'s `51 98` SET landed 0x198 by record execution"
    );
    // The active formation is the MAN table row - id 9, lone Zeto slot.
    let formation = host
        .world
        .active_formation
        .as_ref()
        .expect("active formation set");
    assert_eq!(
        formation.formation_id, ZETO_FORMATION_ROW,
        "the battle formation is MAN row 9, not a synthetic boss id"
    );
    let slots: Vec<u16> = formation.slots.iter().map(|s| s.monster_id).collect();
    assert_eq!(slots, vec![ZETO_MONSTER_ID], "lone Zeto formation [0x4B]");

    // The monster actor carries the PROT 867 archive stats (merged at scene
    // entry for every MAN-formation monster id, Zeto included).
    let party = host.world.party_count as usize;
    let zeto = host
        .world
        .actors
        .iter()
        .skip(party)
        .find(|a| a.battle_monster_id == Some(ZETO_MONSTER_ID))
        .expect("Zeto battle actor spawned");
    let archive = host
        .world
        .monster_catalog
        .get(ZETO_MONSTER_ID)
        .expect("archive stats merged for Zeto");
    assert!(
        archive.hp > 1000,
        "boss-class HP from the archive (got {})",
        archive.hp
    );
    assert_eq!(zeto.battle.hp, archive.hp, "actor HP = archive HP");
    eprintln!(
        "[garmel] Zeto entered organically: formation row {} = [{:#x}], HP {} (tick {ticks})",
        formation.formation_id, ZETO_MONSTER_ID, archive.hp
    );
}

/// The C1 one-shot: once 0x198 is latched (the beat played), the gated
/// dispatch refuses the record - the fight cannot replay.
#[test]
fn zeto_beat_record_is_blocked_by_its_own_c1_gate_once_set() {
    let Some(extracted) = gated() else { return };
    let mut host = SceneHost::open_extracted(&extracted).expect("open SceneHost");
    host.world.system_flag_set(ZETO_GATE_FLAG);
    host.enter_field_scene("garmel", 0).expect("enter garmel");
    let man_bytes = host
        .scene
        .as_ref()
        .unwrap()
        .field_man_payload(&host.index)
        .expect("MAN payload read")
        .expect("garmel resolves its bundle MAN");
    let man_file = legaia_asset::man_section::parse(&man_bytes).expect("MAN parses");
    assert!(
        !host
            .world
            .install_gated_p2_record(&man_file, &man_bytes, ZETO_BEAT_RECORD),
        "the dispatcher refuses P2[12] while its C1 flag 0x198 is set"
    );
    host.world.system_flag_clear(ZETO_GATE_FLAG);
    assert!(
        host.world
            .install_gated_p2_record(&man_file, &man_bytes, ZETO_BEAT_RECORD),
        "the dispatcher admits P2[12] once 0x198 is clear (same record, same call)"
    );
}
