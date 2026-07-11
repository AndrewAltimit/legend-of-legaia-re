//! Arc 2 "Chapter 1 boss spine" oracle: the next spine leg past the Ravine -
//! the Drake overworld (`map01`) down into the first-boss dungeon chain
//! (`rikuroa` / `dolk`).
//!
//! This leg rides the retail-frame scene windows: `rikuroa` and `dolk2`
//! resolve their MAN from the block's streaming variant carrier (PROT 0157 /
//! 0070 - the payload the live script heap byte-matches at the Caruban
//! beat). The earlier "v12-embedded at PROT 164 / 76" reading decoded the
//! NEXT block's sidecar (geremi's / suimon's) through the unshifted CDNAME
//! window. With the real MAN resolved, the scene-entry system script +
//! collision grid + destination table all come up and the overworld-portal
//! transition can land the player inside the dungeon.
//!
//! **Part A - the overworld lists the dungeon chain.** `map01`'s controller
//! (its MAN `0x3F` named-scene-change ops) lists `rikuroa` / `dolk` / `dolk2`
//! among its destinations, and the world-map entity seeder installs overworld
//! portals for the two directly-reachable ones (`rikuroa`, `dolk`).
//!
//! **Part B - the engine walks into the dungeon.** From a live `SceneHost`,
//! driving `town01 -> map01` (the Arc-1 Leg-1 walk-on trigger) and then stepping
//! onto the `rikuroa` (or `dolk`) overworld portal loads the dungeon in
//! `SceneMode::Field`, seats the player at the portal's `0x3F` entry tile, and
//! the scene's MAN is present + parses with the pinned partition counts.
//!
//! **Part C - the scripted first-boss (Caruban) fight, fully organic.**
//! Mt. Rikuroa's boss is Caruban (monster id 73 = `0x49`). The trigger is
//! the streaming-carrier MAN's boss-stager placement `P1[3]` (the parked
//! Noa actor): approaching / touching it runs the record through the field
//! VM (retail's touch dispatch resuming the parked stager script,
//! `FUN_801d5b5c`), whose own bytes SET the staged marker `0x289` (`52 89`)
//! and enter the battle via the scripted-battle op `3E FF 11` -> MAN
//! formation-table row 17 = lone Caruban. The first-visit gate is flag
//! `0x142` - the stager record's own park gate AND the C1 one-shot of the
//! post-victory record P2[50], which that record itself SETs
//! (firehose-caught live, `51 42`, `ra 0x801E3598`; UNSET in
//! `rikuroa_pre_caruban`, SET in `rikuroa_post_caruban`): the post-battle
//! field return re-runs the entry script, whose `0x289` arm spawns `P2[50]`
//! through the C1-gated dispatch, and its own script bytes SET `0x142` -
//! which also flips the `map01` `dolk`->`dolk2` entrance (same flag).
//! Part C drives entry -> approach the stager -> `P1[3]` execution ->
//! Battle seated with Caruban's real archive HP -> victory -> `P2[50]`
//! execution lands `0x142`, and a separate leg proves a post-victory
//! revisit installs no stager. No engine stamp remains anywhere in the
//! chain. (The organic first-arrival flag `0x2FB` chain is covered by
//! `engine-core/tests/organic_beat_records_disc.rs`; Part C seeds `0x2FB`
//! for isolation.)
//!
//! (Two earlier readings are retired: a Part C that armed **Zeto** (75)
//! here gated on `0x1BE` - misattributed, Zeto is `garmel`'s organic
//! `P2[12]` `3E FF 09` - and the "battle-id global `DAT_8007b7fc`"
//! hypothesis for these fights: the formation is the scene's own MAN row,
//! selected by index from script bytes.)
//!
//! **Part D - the story-conditional dungeon entrance (`dolk` -> `dolk2`).**
//! `dolk2` is NOT reached from a dungeon interior (that earlier reading is
//! falsified - no interior scene lists `dolk2`; only `map01` does). It is the
//! post-boss variant of the *same* `map01` dungeon entrance as `dolk`, selected
//! inside the entrance record (`map01` P2[1]/P2[2]) by an op-`0x70` story-flag
//! branch on system flag `0x142`: clear -> `dolk`, set -> `dolk2` (same trigger
//! tile + arrival tile). The engine resolves the branch in the world-map portal
//! seeder via `World::system_flag_test`; Part D asserts both arms + walks into
//! `dolk2` with the flag set. (Flag `0x142`'s retail setter is pinned: the
//! rikuroa post-victory record P2[50]'s script SET - the same record whose
//! execution Part C drives; the oracle still seeds it directly for isolation.)
//!
//! Skip-pass (CLAUDE.md disc-gated convention): `LEGAIA_DISC_BIN` unset /
//! `extracted/` missing.

use std::path::PathBuf;

use legaia_engine_core::scene::{ProtIndex, Scene, SceneHost, SceneTickEvent};
use legaia_engine_core::world::{SceneMode, WorldMapEntityConfig};

fn extracted_dir() -> Option<PathBuf> {
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

/// Rim Elm's south-gate exit tile (the gate-1 walk-on trigger whose partition-2
/// record's `0x3F` leaves for `map01`); shared with the Arc-1 spine oracle.
const TOWN01_SOUTH_GATE: (u8, u8) = (25, 46);

fn open_host() -> Option<SceneHost> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    let extracted = extracted_dir()?;
    Some(SceneHost::open_extracted(&extracted).expect("open SceneHost"))
}

/// Drive `town01` free-roam onto the Drake overworld (`map01`, WorldMap) via the
/// south-gate walk-on trigger. `None` when the disc gate skips.
fn drive_town01_to_map01() -> Option<SceneHost> {
    drive_town01_to_map01_with_flags(&[])
}

/// Like [`drive_town01_to_map01`], but latches each system story flag in `flags`
/// while still in `town01` - before the `map01` seeder runs - so the overworld's
/// story-conditional entrances (e.g. the dolk/dolk2 dungeon variant on flag
/// `0x142`) resolve against a post-beat flag state. The flags survive the
/// scene transition (they are world story state, not per-scene).
fn drive_town01_to_map01_with_flags(flags: &[u16]) -> Option<SceneHost> {
    let mut host = open_host()?;
    host.enter_field_scene("town01", 0).expect("enter town01");
    assert_eq!(host.world.mode, SceneMode::Field, "town01 is a field scene");
    for &f in flags {
        host.world.system_flag_set(f);
    }
    for _ in 0..3 {
        if let SceneTickEvent::SceneEntered { name } = host.tick().expect("tick") {
            panic!("unexpected early transition to {name}");
        }
    }
    host.world
        .seat_player_at_tile(TOWN01_SOUTH_GATE.0, TOWN01_SOUTH_GATE.1);
    let mut entered = None;
    for _ in 0..120 {
        if let SceneTickEvent::SceneEntered { name } = host.tick().expect("tick") {
            entered = Some(name);
            break;
        }
    }
    assert_eq!(
        entered.as_deref(),
        Some("map01"),
        "the south-gate trigger's 0x3F record leaves Rim Elm for the overworld"
    );
    assert_eq!(
        host.world.mode,
        SceneMode::WorldMap,
        "map01 routes through the world-map entry"
    );
    Some(host)
}

/// Tile of the first overworld portal to `dest` on the currently-loaded map.
fn find_portal_tile(host: &SceneHost, dest: &str) -> Option<(u8, u8)> {
    host.world
        .world_map_entity_configs
        .iter()
        .zip(host.world.world_map_entity_positions.iter())
        .find_map(|(cfg, &(x, z))| match cfg {
            WorldMapEntityConfig::OverworldPortal { scene_name, .. } if scene_name == dest => {
                Some(((x >> 7) as u8, (z >> 7) as u8))
            }
            _ => None,
        })
}

/// Assert the named scene resolves a MAN that parses with the given
/// partition counts (through the same `field_man_payload` resolution order
/// the host uses - bundle first, streaming variant carrier fallback).
fn assert_scene_man(index: &ProtIndex, name: &str, want_counts: [i16; 3]) {
    let scene = Scene::load(index, name).unwrap_or_else(|e| panic!("load {name}: {e:#}"));
    let man = scene
        .field_man_payload(index)
        .unwrap_or_else(|e| panic!("{name}: field_man_payload: {e:#}"))
        .unwrap_or_else(|| panic!("{name}: no MAN resolves"));
    let mf = legaia_asset::man_section::parse(&man)
        .unwrap_or_else(|e| panic!("{name}: parse MAN: {e:#}"));
    assert_eq!(
        mf.header.partition_counts, want_counts,
        "{name} MAN partition counts"
    );
}

// ---------------------------------------------------------------------
// Part A: the overworld lists the dungeon chain
// ---------------------------------------------------------------------

#[test]
fn part_a_map01_lists_the_first_boss_dungeon_chain() {
    let Some(mut host) = open_host() else {
        return;
    };
    host.load_scene("map01").expect("load map01");
    let dests: Vec<String> = host
        .scene_destinations()
        .iter()
        .map(|d| d.scene_name.clone())
        .collect();
    eprintln!("[map01] {} destinations: {dests:?}", dests.len());
    for expected in ["rikuroa", "dolk", "dolk2"] {
        assert!(
            dests.iter().any(|d| d == expected),
            "map01 controller lists {expected}; got {dests:?}"
        );
    }
    eprintln!("[ok] Part A: map01 lists rikuroa / dolk / dolk2");
}

// ---------------------------------------------------------------------
// Part B: the engine walks into the boss dungeon
// ---------------------------------------------------------------------

/// Drive `map01 -> <dest>` through the overworld portal and assert the dungeon
/// loads in Field mode with the player seated at the portal's entry tile.
fn drive_map01_portal_into(dest: &str) -> Option<SceneHost> {
    drive_map01_portal_into_with_flags(dest, &[])
}

/// Like [`drive_map01_portal_into`], but latches `flags` before the `map01`
/// seeder runs (see [`drive_town01_to_map01_with_flags`]) so a story-conditional
/// entrance resolves to its post-beat destination.
fn drive_map01_portal_into_with_flags(dest: &str, flags: &[u16]) -> Option<SceneHost> {
    let mut host = drive_town01_to_map01_with_flags(flags)?;
    let tile = find_portal_tile(&host, dest)
        .unwrap_or_else(|| panic!("map01 installs a {dest} overworld portal"));
    host.world.seat_player_at_tile(tile.0, tile.1);
    host.world.set_pad(0);
    let mut entered = None;
    for _ in 0..8 {
        if let SceneTickEvent::SceneEntered { name } = host.tick().expect("tick") {
            entered = Some(name);
            break;
        }
    }
    assert_eq!(
        entered.as_deref(),
        Some(dest),
        "the {dest} overworld portal loads the dungeon"
    );
    assert_eq!(
        host.world.mode,
        SceneMode::Field,
        "{dest} is a field-mode dungeon scene"
    );
    let slot = host
        .world
        .player_actor_slot
        .expect("player actor seated on dungeon entry");
    let ms = &host.world.actors[slot as usize].move_state;
    let seat = ((ms.world_x - 0x40) >> 7, (ms.world_z - 0x40) >> 7);
    eprintln!("[{dest}] portal at {tile:?} -> entered, seated at tile {seat:?}");
    Some(host)
}

#[test]
fn part_b_map01_portal_into_rikuroa_first_boss_dungeon() {
    let Some(host) = drive_map01_portal_into("rikuroa") else {
        return;
    };
    // The dungeon's real MAN is resolved (streaming carrier PROT 0157) -
    // partitions [13, 29, 64]. This is the source the scripted-boss trigger
    // and the post-victory 0x142 record read.
    let index = host.index.clone();
    assert_scene_man(&index, "rikuroa", [13, 29, 64]);
    eprintln!(
        "[ok] Leg (Arc-2): map01 -> rikuroa overworld portal -> rikuroa (Field), \
         MAN present. NEXT LEG: the Caruban scripted boss is gated by rikuroa's \
         partition-2 cutscene timeline, not the encounter registry."
    );
}

#[test]
fn part_b_map01_portal_into_dolk() {
    let Some(host) = drive_map01_portal_into("dolk") else {
        return;
    };
    // dolk keeps its first-class scripted bundle; its MAN lists rikuroa as an
    // interior destination and parses with partitions [26, 41, 12].
    let index = host.index.clone();
    assert_scene_man(&index, "dolk", [26, 41, 12]);
    let dests: Vec<String> = host
        .scene_destinations()
        .iter()
        .map(|d| d.scene_name.clone())
        .collect();
    eprintln!("[dolk] destinations: {dests:?}");
    assert!(
        dests.iter().any(|d| d == "rikuroa"),
        "dolk lists its interior destination rikuroa; got {dests:?}"
    );
    eprintln!("[ok] Leg (Arc-2): map01 -> dolk overworld portal -> dolk (Field), MAN present");
}

// ---------------------------------------------------------------------
// Part C: the scripted first-boss (Caruban) fight, end to end
// ---------------------------------------------------------------------
//
// Caruban (monster id 73 = 0x49) is Mt. Rikuroa's boss (gamedata + the
// operator's live playthrough). The trigger is the MAN's own boss-stager
// placement P1[3] (`3E FF 11` -> formation row 17), gated by first-visit
// flag 0x142 (the record's own head test + the post-victory record P2[50]'s
// C1 one-shot and in-record SET). The engine derives the stager binding
// from the MAN at scene entry (`World::install_boss_stagers_from_man`) and
// runs the record itself on approach - every flag lands from script bytes.

/// Caruban = monster id 73 (0x49); rikuroa boss gate flag 0x142, staged
/// marker 0x289 (SET by P1[3]'s own `52 89`), first-arrival flag 0x2FB.
/// Mirrors the rikuroa MAN's P1[0] / P1[3] dispatch arms.
const CARUBAN_MONSTER_ID: u16 = 73;
const RIKUROA_BOSS_GATE_FLAG: u16 = 0x142;
const RIKUROA_BOSS_STAGED_MARKER: u16 = 0x289;
const RIKUROA_ARRIVAL_FLAG: u16 = 0x2FB;
/// The stager placement slot (partition-1 record index of the Noa actor).
const RIKUROA_STAGER_SLOT: u8 = 3;
/// The MAN formation-table row P1[3]'s `3E FF 11` selects.
const RIKUROA_BOSS_FORMATION_ROW: u16 = 17;
/// The nest tile the stager record's own `0x4C 0x51` leg stations its actor
/// at - the approach point.
const RIKUROA_STAGER_STATION_TILE: (u8, u8) = (69, 75);

#[test]
fn part_c_rikuroa_arms_and_fights_the_caruban_scripted_boss() {
    let Some(mut host) = drive_map01_portal_into("rikuroa") else {
        return;
    };

    // (1) Entering rikuroa (first visit, gate flag clear) derived the stager
    // binding from the MAN - and pre-armed nothing.
    assert!(
        !host.world.system_flag_test(RIKUROA_BOSS_GATE_FLAG),
        "first visit: the Caruban gate flag starts clear"
    );
    assert!(
        !host.world.system_flag_test(RIKUROA_BOSS_STAGED_MARKER),
        "the staged marker stays clear until P1[3] itself runs"
    );
    assert!(
        !host.world.scripted_formation_pending,
        "no forced formation is pre-armed at scene entry"
    );
    let binding = host
        .world
        .field_boss_stagers
        .get(&RIKUROA_STAGER_SLOT)
        .copied()
        .expect("rikuroa entry installs the P1[3] stager binding");
    assert_eq!(binding.record, RIKUROA_STAGER_SLOT);
    assert_eq!(binding.park_gate, Some(RIKUROA_BOSS_GATE_FLAG));
    // Real Caruban stats were merged from the PROT 867 archive (the MAN
    // formation install covers every row's monster ids, row 17 included).
    let caruban = host
        .world
        .monster_catalog
        .get(CARUBAN_MONSTER_ID)
        .expect("Caruban stats seeded from the monster archive");
    assert!(caruban.hp > 0, "Caruban carries real archive HP");
    let archive_hp = caruban.hp;

    // (2) Approach the stager's station tile: the touch dispatch runs P1[3]
    // through the field VM, whose own bytes stage + enter the fight. Seed
    // the first-arrival flag 0x2FB (the story state a real playthrough
    // carries at the Caruban beat; its organic landing from P2[43] is
    // covered by `organic_beat_records_disc.rs`). Toggle confirm so the
    // record's inline dialog pages advance.
    host.world.system_flag_set(RIKUROA_ARRIVAL_FLAG);
    host.world.live_gameplay_loop = true;
    host.world
        .seat_player_at_tile(RIKUROA_STAGER_STATION_TILE.0, RIKUROA_STAGER_STATION_TILE.1);
    let cross = legaia_engine_core::input::PadButton::Cross.mask();
    let mut reached_battle = false;
    let mut ticks = 0u32;
    while ticks < 4000 {
        host.world
            .set_pad(if ticks.is_multiple_of(2) { cross } else { 0 });
        let _ = host.tick().expect("tick");
        ticks += 1;
        if host.world.mode == SceneMode::Battle {
            reached_battle = true;
            break;
        }
    }
    host.world.set_pad(0);
    assert!(
        reached_battle,
        "approaching the stager runs P1[3] and its `3E FF 11` flips Field -> Battle"
    );
    let monster_slot = host.world.party_count.clamp(1, 3) as usize;
    assert_eq!(
        host.world.actors[monster_slot].battle_monster_id,
        Some(CARUBAN_MONSTER_ID),
        "the lone enemy slot is Caruban"
    );
    assert_eq!(
        host.world.actors[monster_slot].battle.hp, archive_hp,
        "the Caruban slot is seeded with its real archive HP"
    );

    // The staged marker landed from the record's own `52 89` (there is no
    // engine stamp); the gate flag stays clear.
    assert!(
        host.world.system_flag_test(RIKUROA_BOSS_STAGED_MARKER),
        "P1[3]'s `52 89` SET landed the staged marker 0x289"
    );
    assert!(
        !host.world.system_flag_test(RIKUROA_BOSS_GATE_FLAG),
        "the gate flag 0x142 stays clear during the fight (no victory latch)"
    );
    assert_eq!(
        host.world.active_formation.as_ref().map(|f| f.formation_id),
        Some(RIKUROA_BOSS_FORMATION_ROW),
        "the battle formation is MAN row 17, not a synthetic boss id"
    );

    // (3) Win through the live loop: the post-battle field return re-runs the
    // scene-entry script, whose staged-marker arm spawns the post-victory
    // record P2[50] through the C1-gated dispatch - its own `51 42` script
    // bytes SET the gate flag (and `62 89` clears the marker).
    let party = host.world.party_count as usize;
    for a in host.world.actors.iter_mut().skip(party) {
        if a.battle_monster_id.is_some() {
            a.battle.hp = 0;
            a.battle.liveness = 0;
        }
    }
    let mut ticks = 0u32;
    while host.world.mode == SceneMode::Battle && ticks < 2000 {
        let _ = host.tick().expect("tick");
        ticks += 1;
    }
    assert_eq!(
        host.world.mode,
        SceneMode::Field,
        "the won battle tears down to the field"
    );
    let mut ticks = 0u32;
    while !host.world.system_flag_test(RIKUROA_BOSS_GATE_FLAG) && ticks < 2000 {
        let _ = host.tick().expect("tick");
        ticks += 1;
    }
    assert!(
        host.world.system_flag_test(RIKUROA_BOSS_GATE_FLAG),
        "P2[50]'s execution lands the rikuroa first-visit gate flag 0x142"
    );

    eprintln!(
        "[ok] Part C: rikuroa -> P1[3] stager approach -> Battle (MAN row 17) seated \
         with real HP -> victory -> P2[50] execution lands gate flag 0x142"
    );
}

#[test]
fn part_c_rikuroa_does_not_rearm_caruban_once_the_gate_flag_is_set() {
    let Some(mut host) = drive_map01_portal_into("rikuroa") else {
        return;
    };
    // Simulate the post-victory world state: the gate flag is already latched.
    host.world.system_flag_set(RIKUROA_BOSS_GATE_FLAG);
    // Re-enter rikuroa from scratch - the stager derive must see the set park
    // gate and install no binding.
    host.enter_field_scene("rikuroa", 0)
        .expect("re-enter rikuroa");
    assert!(
        !host
            .world
            .field_boss_stagers
            .contains_key(&RIKUROA_STAGER_SLOT),
        "with the gate flag set, re-entering rikuroa installs no Caruban stager"
    );
    assert!(
        !host.world.scripted_formation_pending,
        "nothing is pre-armed either"
    );
    eprintln!("[ok] Part C: post-victory rikuroa revisit does not re-arm the boss");
}

// ---------------------------------------------------------------------
// Part D: the story-conditional dungeon entrance (dolk -> dolk2)
// ---------------------------------------------------------------------
//
// `map01`'s dungeon-entrance records (P2[1]/P2[2]) select their `0x3F`
// destination by an op-0x70 story-flag branch on system flag 0x142: flag CLEAR
// -> `dolk` (pre-boss), flag SET -> `dolk2` (post-boss). The two arms share the
// same overworld trigger tile + arrival tile - it is the same entrance, gated by
// story progress. (This falsifies the earlier "dolk2 is reached from a dungeon
// interior" reading: no interior scene lists dolk2; only `map01` does.) The
// engine resolves the branch in the world-map portal seeder via
// `World::system_flag_test`.

const DOLK_VARIANT_FLAG: u16 = 0x142;

#[test]
fn part_d_dungeon_entrance_is_dolk_before_the_flag_and_dolk2_after() {
    // Baseline (flag CLEAR): the entrance is `dolk`; no `dolk2` portal exists.
    let Some(host) = drive_town01_to_map01() else {
        return;
    };
    assert!(
        find_portal_tile(&host, "dolk").is_some(),
        "flag clear: the map01 dungeon entrance resolves to dolk"
    );
    assert!(
        find_portal_tile(&host, "dolk2").is_none(),
        "flag clear: no dolk2 portal is installed"
    );
    let dolk_tile = find_portal_tile(&host, "dolk").unwrap();

    // Post-beat (flag 0x142 SET): the SAME entrance tile now resolves to `dolk2`.
    let Some(host2) = drive_town01_to_map01_with_flags(&[DOLK_VARIANT_FLAG]) else {
        return;
    };
    let dolk2_tile = find_portal_tile(&host2, "dolk2")
        .expect("flag 0x142 set: the map01 dungeon entrance resolves to dolk2");
    assert!(
        find_portal_tile(&host2, "dolk").is_none(),
        "flag set: the pre-boss dolk portal is replaced by dolk2"
    );
    assert_eq!(
        dolk2_tile, dolk_tile,
        "dolk and dolk2 are the same entrance tile, chosen by flag 0x142"
    );
    eprintln!(
        "[ok] Part D: map01 dungeon entrance at {dolk_tile:?} = dolk (flag clear) \
         / dolk2 (flag 0x142 set)"
    );
}

#[test]
fn part_d_engine_walks_into_dolk2_after_the_story_flag() {
    // With flag 0x142 latched, walking the dungeon entrance loads dolk2 (Field).
    let Some(host) = drive_map01_portal_into_with_flags("dolk2", &[DOLK_VARIANT_FLAG]) else {
        return;
    };
    assert_eq!(host.world.mode, SceneMode::Field, "dolk2 is a field scene");
    let index = host.index.clone();
    // dolk2 ships its MAN inside the v12-embedded scene_asset_table.
    assert_scene_man(&index, "dolk2", [29, 73, 17]);
    eprintln!(
        "[ok] Part D: map01 (flag 0x142) -> dolk2 overworld portal -> dolk2 (Field), MAN present"
    );
}
