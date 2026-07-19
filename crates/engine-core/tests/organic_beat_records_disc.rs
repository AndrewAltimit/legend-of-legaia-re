//! Disc-gated runtime oracle: the chapter-1 story-spine flags land from
//! **executing the MAN records' own script bytes**, not from engine
//! stand-ins.
//!
//! Three beats, one shared machinery (field-VM record execution + the
//! `FUN_8003BDE0` gated partition-2 record dispatch):
//!
//! 1. **rikuroa `P1[3]`** (the Caruban boss stager in the streaming carrier
//!    MAN, PROT 0157): the parked special-model placement (SJIS locals
//!    ノア/Noa) whose record carries the whole pre-boss beat. Approaching /
//!    touching the placed actor runs the record through the field VM (the
//!    retail touch dispatch resuming the parked stager script -
//!    `FUN_801d5b5c`); its own bytes SET the staged marker `0x289`
//!    (`52 89`) and enter the battle via `3E FF 11` -> MAN formation-table
//!    row 17 = lone Caruban (`0x49`). No engine stamp anywhere: the old
//!    `SCRIPTED_SCENE_BOSSES` battle-entry marker stand-in is retired.
//!
//! 2. **rikuroa `P2[50]`** (the post-victory cutscene record): after the
//!    boss battle the field return re-runs the scene-entry system script
//!    `P1[0]`, whose `SysFlag.Test 0x289` arm issues the op-`0x44` spawn of
//!    global record `0x5C` = `P2[50]` (C1 gate `[0x142]`, the self-latching
//!    one-shot). The record's own `51 42` SET lands system flag `0x142` and
//!    its `62 89` clears the staged marker.
//!
//! 3. **town01 `P2[3]`** (the Rim Elm opening record): its `52 25` SET at the
//!    record head lands system flag `0x225` (549) - the record SETs its own
//!    C1 gate, same self-latch shape - purely by timeline execution.
//!
//! Baseline passes keep every leg non-vacuous: each flag is asserted CLEAR
//! before the record that owns it executes.
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

/// The Caruban first-visit gate flag (`map01` dolk->dolk2 entrance selector) -
/// also the stager record's own park gate (its head `SysFlag.Test`).
const CARUBAN_GATE_FLAG: u16 = 0x142;
/// The transient "boss battle staged" marker the stager `P1[3]`'s own script
/// bytes SET (`52 89`) right before its battle-entry op; tested by `P1[0]` on
/// the post-battle scene re-entry.
const CARUBAN_STAGED_MARKER: u16 = 0x289;
/// The rikuroa first-arrival story flag: `P1[0]`'s intro arm spawns `P2[43]`
/// (op `44 55`) while it is clear, and that record's own `52 FB` SETs it -
/// the same self-latch shape one branch level up.
const RIKUROA_ARRIVAL_FLAG: u16 = 0x2FB;
/// The stager placement / partition-1 record index (the Noa actor).
const CARUBAN_STAGER_SLOT: u8 = 3;
/// The MAN formation-table row `P1[3]`'s `3E FF 11` selects.
const CARUBAN_FORMATION_ROW: u16 = 17;
/// Caruban's monster-archive id (PROT 867; lone slot of formation row 17).
const CARUBAN_MONSTER_ID: u16 = 0x49;
/// The Rim Elm opening one-shot (flag 549), `town01` `P2[3]`'s own C1 gate.
const TOWN01_OPENING_FLAG: u16 = 0x225;

/// Static disc facts: the rikuroa streaming-carrier MAN carries the Caruban
/// boss-stager placement (record `3E FF 11`, park gate `0x142`, station leg)
/// and the lone-Caruban formation row it selects - all as disc bytes, none
/// as engine constants.
#[test]
fn rikuroa_man_carries_the_caruban_stager_placement_and_formation_row() {
    let Some(extracted) = gated() else { return };
    let mut host = SceneHost::open_extracted(&extracted).expect("open SceneHost");
    host.enter_field_scene("rikuroa", 0).expect("enter rikuroa");
    let man_bytes = host
        .scene
        .as_ref()
        .unwrap()
        .field_man_payload(&host.index)
        .expect("MAN payload read")
        .expect("rikuroa resolves its streaming carrier MAN");
    let man_file = legaia_asset::man_section::parse(&man_bytes).expect("MAN parses");

    // The stager detection recovers exactly one site: P1[3], formation row
    // 17, park gate 0x142, stationed at the nest tile (69, 75) by its own
    // `0x4C 0x51` leg.
    let sites =
        legaia_engine_core::man_field_scripts::boss_stager_placements(&man_file, &man_bytes);
    assert_eq!(sites.len(), 1, "rikuroa carries one boss-stager placement");
    let site = sites[0];
    assert_eq!(site.placement_index, CARUBAN_STAGER_SLOT as usize);
    assert_eq!(u16::from(site.formation_row), CARUBAN_FORMATION_ROW);
    assert_eq!(site.park_gate_flag, Some(CARUBAN_GATE_FLAG));
    assert!(site.spawn_parked, "the Noa placement spawns parked");
    let station = site.station_world.expect("the record stations its actor");
    assert_eq!(
        ((station.0 - 0x40) >> 7, (station.1 - 0x40) >> 7),
        (69, 75),
        "stationed at the nest tile"
    );

    // Formation row 17 = lone monster 0x49 (count 1), read straight off the
    // variant MAN's encounter section.
    let record = legaia_engine_core::encounter_man::formation_record_for_row(
        &man_bytes,
        CARUBAN_FORMATION_ROW as usize,
    )
    .expect("rikuroa formation row 17 decodes");
    assert_eq!(record.count, 1, "row 17 is a lone-monster formation");
    assert_eq!(
        record.monster_ids[0], CARUBAN_MONSTER_ID as u8,
        "row 17's lone slot is Caruban"
    );

    // The record body carries the staged-marker SET and the battle op as
    // adjacent script bytes (`52 89` .. `3E FF 11`).
    let (start, _pc0, len) = legaia_engine_core::man_field_scripts::partition_record_span(
        &man_file,
        &man_bytes,
        1,
        CARUBAN_STAGER_SLOT as usize,
    )
    .expect("P1[3] span resolves");
    let body = &man_bytes[start..start + len];
    assert_eq!(
        body.windows(3)
            .filter(|w| *w == [0x3E, 0xFF, CARUBAN_FORMATION_ROW as u8])
            .count(),
        1,
        "P1[3] carries `3E FF 11` exactly once"
    );
    assert!(
        body.windows(2).any(|w| w == [0x52, 0x89]),
        "P1[3] carries the staged-marker SET `52 89`"
    );
    eprintln!(
        "[rikuroa] static chain verified: P1[3] stager (gate 0x142, station (69,75)) \
         -> `52 89` + `3E FF 11` -> row 17 = [0x49]"
    );
}

/// Full organic chapter-1 rikuroa slice, no flags pre-seeded, no engine
/// stamps:
///
/// 1. first entry runs `P1[0]`, whose intro arm spawns `P2[43]` - its script
///    bytes SET the arrival flag `0x2FB`;
/// 2. re-entering (the scene reload that follows the retail first-arrival
///    cutscene) re-derives the stager binding from the MAN (nothing is
///    pre-armed: no forced formation, marker clear);
/// 3. the player approaches the stager's station tile - the touch dispatch
///    runs `P1[3]` through the field VM, whose own `52 89` SETs the staged
///    marker and whose `3E FF 11` enters battle on formation row 17 (lone
///    Caruban, real archive stats);
/// 4. winning returns to the field, the entry script re-runs, its `0x289`
///    arm spawns `P2[50]` through the C1-gated dispatch, and that record's
///    own `51 42` / `62 89` land `0x142` and clear the marker.
///
/// Every flag in the chain lands from script bytes - the old battle-entry
/// `0x289` stamp (`SCRIPTED_SCENE_BOSSES`) is deleted.
#[test]
fn rikuroa_caruban_chain_runs_organically_from_p1_3_to_p2_50() {
    let Some(extracted) = gated() else { return };
    let mut host = SceneHost::open_extracted(&extracted).expect("open SceneHost");
    host.world.live_gameplay_loop = true;
    host.enter_field_scene("rikuroa", 0).expect("enter rikuroa");

    // Baseline (non-vacuous): every flag in the chain is clear.
    for (flag, what) in [
        (CARUBAN_GATE_FLAG, "gate flag 0x142"),
        (CARUBAN_STAGED_MARKER, "staged marker 0x289"),
        (RIKUROA_ARRIVAL_FLAG, "arrival flag 0x2FB"),
    ] {
        assert!(
            !host.world.system_flag_test(flag),
            "baseline: {what} clear on first visit"
        );
    }

    // Phase 1 - first arrival: P1[0]'s intro arm spawns P2[43], whose script
    // bytes SET 0x2FB (organic; the record then owns the scene like the
    // retail arrival cutscene).
    let mut ticks = 0u32;
    while !host.world.system_flag_test(RIKUROA_ARRIVAL_FLAG) && ticks < 2000 {
        host.tick().expect("tick");
        ticks += 1;
    }
    assert!(
        host.world.system_flag_test(RIKUROA_ARRIVAL_FLAG),
        "P2[43]'s `52 FB` SET lands the arrival flag 0x2FB organically (waited {ticks} ticks)"
    );

    // Phase 2 - re-enter (the scene reload after the arrival cutscene):
    // the stager binding is re-derived from the MAN; nothing is pre-armed.
    host.enter_field_scene("rikuroa", 0)
        .expect("re-enter rikuroa");
    assert!(
        !host.world.scripted_formation_pending,
        "no forced formation is pre-armed at scene entry"
    );
    assert!(
        !host.world.system_flag_test(CARUBAN_STAGED_MARKER),
        "the staged marker stays clear until P1[3] itself runs"
    );
    assert!(
        host.world
            .field_boss_stagers
            .contains_key(&CARUBAN_STAGER_SLOT),
        "the P1[3] stager binding is installed from the MAN"
    );

    // Phase 3 - approach: seat the player at the stager's station tile (the
    // nest); the walk-touch dispatch runs P1[3] as the beat timeline, whose
    // own bytes stage + enter the fight. Toggle confirm so the record's
    // inline dialog pages advance on fresh press edges.
    host.world.seat_player_at_tile(69, 75);
    let cross_mask = legaia_engine_core::input::PadButton::Cross.mask();
    let mut ticks = 0u32;
    let mut entered_battle = false;
    while ticks < 4000 {
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
    host.world.input.set_pad(0);
    assert!(
        entered_battle,
        "P1[3]'s `3E FF 11` entered battle within {ticks} ticks of the approach"
    );
    // The staged marker landed from the record's own `52 89` (no engine
    // stamp exists any more).
    assert!(
        host.world.system_flag_test(CARUBAN_STAGED_MARKER),
        "P1[3]'s `52 89` SET landed the staged marker 0x289 by record execution"
    );
    assert!(
        !host.world.system_flag_test(CARUBAN_GATE_FLAG),
        "gate flag 0x142 still clear during the battle"
    );
    // The active formation is the MAN table row - id 17, lone Caruban slot -
    // not a synthetic boss id.
    let formation = host
        .world
        .active_formation
        .as_ref()
        .expect("active formation set");
    assert_eq!(
        formation.formation_id, CARUBAN_FORMATION_ROW,
        "the battle formation is MAN row 17, not a synthetic boss id"
    );
    let slots: Vec<u16> = formation.slots.iter().map(|s| s.monster_id).collect();
    assert_eq!(
        slots,
        vec![CARUBAN_MONSTER_ID],
        "lone Caruban formation [0x49]"
    );
    // The monster actor carries the PROT 867 archive stats (merged at scene
    // entry for every MAN-formation monster id, Caruban included).
    let party = host.world.party_count as usize;
    let caruban = host
        .world
        .actors
        .iter()
        .skip(party)
        .find(|a| a.battle_monster_id == Some(CARUBAN_MONSTER_ID))
        .expect("Caruban battle actor spawned");
    let archive = host
        .world
        .monster_catalog
        .get(CARUBAN_MONSTER_ID)
        .expect("archive stats merged for Caruban");
    assert!(
        archive.hp > 100,
        "boss-class HP from the archive (got {})",
        archive.hp
    );
    assert_eq!(caruban.battle.hp, archive.hp, "actor HP = archive HP");

    // Phase 4 - win: wipe the monsters and let the live loop tear the battle
    // down; the post-battle entry-script re-run + P2[50] land the gate flag.
    for a in host.world.actors.iter_mut().skip(party) {
        if a.battle_monster_id.is_some() {
            a.battle.hp = 0;
            a.battle.liveness = 0;
        }
    }
    let mut ticks = 0u32;
    while matches!(
        host.world.mode,
        legaia_engine_core::world::SceneMode::Battle
    ) && ticks < 2000
    {
        host.tick().expect("tick");
        ticks += 1;
    }
    assert!(
        matches!(host.world.mode, legaia_engine_core::world::SceneMode::Field),
        "battle tears down to the field"
    );
    let mut ticks = 0u32;
    while !host.world.system_flag_test(CARUBAN_GATE_FLAG) && ticks < 2000 {
        host.tick().expect("tick");
        ticks += 1;
    }
    assert!(
        host.world.system_flag_test(CARUBAN_GATE_FLAG),
        "P2[50]'s `51 42` SET lands 0x142 on the post-battle field return (waited {ticks} ticks)"
    );
    // P2[50]'s `62 89` clears the staged marker as part of the same record.
    let mut ticks = 0u32;
    while host.world.system_flag_test(CARUBAN_STAGED_MARKER) && ticks < 2000 {
        host.tick().expect("tick");
        ticks += 1;
    }
    assert!(
        !host.world.system_flag_test(CARUBAN_STAGED_MARKER),
        "P2[50]'s `62 89` clears the staged marker 0x289"
    );
    eprintln!(
        "[rikuroa] full organic chain: P1[3] approach -> 0x289 by `52 89` -> battle row 17 \
         [0x49] -> victory -> P2[50] lands 0x142 + clears 0x289"
    );
}

/// The one-shot: with `0x142` already set (the boss beaten), re-entering
/// rikuroa installs NO stager binding (the record's own park gate refuses),
/// and returning from a battle must NOT replay `P2[50]` (its C1 gate blocks
/// the spawn).
#[test]
fn rikuroa_stager_and_p2_50_are_blocked_by_the_gate_flag_once_set() {
    let Some(extracted) = gated() else { return };
    let mut host = SceneHost::open_extracted(&extracted).expect("open SceneHost");
    host.world.system_flag_set(CARUBAN_GATE_FLAG);
    host.enter_field_scene("rikuroa", 0).expect("enter rikuroa");
    assert!(
        !host
            .world
            .field_boss_stagers
            .contains_key(&CARUBAN_STAGER_SLOT),
        "the beaten boss's stager does not re-arm (park gate 0x142 set)"
    );
    assert!(
        !host.world.scripted_formation_pending,
        "nothing is pre-armed either"
    );
    // Simulate a stale staged marker (the state a fled/lost fight would leave):
    // the P1[0] arm fires, but P2[50]'s C1 gate `[0x142]` blocks the spawn.
    // The blocked-spawn observable is P2[50]'s own effect: its `62 89` clears
    // the staged marker, so the marker STAYING set proves the record never
    // ran. (Ambient helper contexts from engaged channel windows may
    // legitimately exist, so "no helper contexts at all" is not the signal.)
    host.world.system_flag_set(CARUBAN_STAGED_MARKER);
    for _ in 0..600 {
        host.tick().expect("tick");
    }
    assert!(
        host.world.system_flag_test(CARUBAN_STAGED_MARKER),
        "P2[50] does not spawn once its C1 gate flag is set (0x289 stays set)"
    );
    // Non-vacuous: drive the gated dispatch directly against the real record
    // (P2[50] = global index 0x5C, partition counts [13, 29, ..]) in both
    // gate polarities.
    let man_bytes = host
        .scene
        .as_ref()
        .unwrap()
        .field_man_payload(&host.index)
        .expect("MAN payload read")
        .expect("rikuroa resolves its streaming carrier MAN");
    let man_file = legaia_asset::man_section::parse(&man_bytes).expect("MAN parses");
    assert!(
        !host
            .world
            .install_spawned_helper_record(&man_file, &man_bytes, 0x5C),
        "the dispatcher refuses P2[50] while its C1 flag 0x142 is set"
    );
    host.world.system_flag_clear(CARUBAN_GATE_FLAG);
    assert!(
        host.world
            .install_spawned_helper_record(&man_file, &man_bytes, 0x5C),
        "the dispatcher admits P2[50] once 0x142 is clear (same record, same call)"
    );
}

/// town01's opening timeline record `P2[3]` SETs its own C1 gate flag `0x225`
/// (549) from its script bytes when the opening executes.
#[test]
fn town01_p2_3_sets_flag_549_by_record_execution() {
    let Some(extracted) = gated() else { return };
    let mut host = SceneHost::open_extracted(&extracted).expect("open SceneHost");
    // New-game prologue hand-off into Rim Elm (the path that installs the
    // opening cutscene timeline record P2[3]).
    host.world.entering_town01_opening = true;
    host.enter_field_scene(legaia_asset::new_game::OPENING_SCENE, 0)
        .expect("enter town01");
    assert!(
        host.world.cutscene_timeline_active(),
        "the opening timeline installs on the prologue hand-off"
    );
    // Baseline (non-vacuous): 549 is clear until the record's own `52 25` runs.
    assert!(
        !host.world.system_flag_test(TOWN01_OPENING_FLAG),
        "baseline: flag 549 clear at scene entry"
    );
    let mut ticks = 0u32;
    while !host.world.system_flag_test(TOWN01_OPENING_FLAG) && ticks < 4000 {
        host.world.tick();
        ticks += 1;
    }
    assert!(
        host.world.system_flag_test(TOWN01_OPENING_FLAG),
        "P2[3]'s `52 25` SET lands flag 549 by timeline execution (ticked {ticks})"
    );
    eprintln!("[town01] flag 549 landed organically from P2[3] execution at tick {ticks}");
}
