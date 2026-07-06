//! Arc 2 "Chapter 1 boss spine" oracle: the next spine leg past the Ravine -
//! the Drake overworld (`map01`) down into the first-boss dungeon chain
//! (`rikuroa` / `dolk`).
//!
//! This leg is unblocked by the v12-family bundle fix: `rikuroa` (PROT 164) and
//! `dolk2` (PROT 76) ship their MAN inside a `scene_asset_table` embedded at
//! offset `0x1000` of their `Class::SceneV12Table` entry, so before the
//! `find_bundle` v12 fallback they had no loadable MAN. With the MAN resolved,
//! the scene-entry system script + collision grid + destination table all come
//! up and the overworld-portal transition can land the player inside the
//! dungeon.
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
//! **Part C - the scripted first-boss (Zeto) fight.** Zeto (monster id 75 =
//! `0x4B`) has no on-disc formation record: not in `rikuroa`'s MAN encounter
//! section and not as an inline armed-YIELD window. Retail arms it through the
//! *battle-id path* (`DAT_8007b7fc = 0x4B` -> `FUN_8005567c` collapses ids
//! `0x49..0x4D` to a lone-monster cell), gated by a first-visit story flag. The
//! engine models this as a `rikuroa` scene-entry latch on flag `0x1BE`
//! (`rikuroa` cutscene record P2[0] "Meta's warning" C1) plus the battle-id
//! port [`World::install_boss_encounter`]; the gate flag latches on victory
//! ([`World::apply_battle_loot`]). Part C drives entry -> armed boss -> Field ->
//! Battle seated with Zeto's real archive HP -> victory latches `0x1BE`, and a
//! separate leg proves a post-victory revisit does not re-arm the fight.
//!
//! **Residual (documented, not faked).** The field-side op that writes the
//! battle-id global in retail is not yet recovered from the corpus - it sits in
//! the `rikuroa` scene prescript / event-VM (a different bytecode than the
//! field VM) or a LUI+ADDIU-aliased store in an undumped overlay. The
//! scene-entry flag-`0x1BE` latch is the faithful interim until that writer op
//! is pinned. Past the boss, `dolk2` is reached from a dungeon interior (not a
//! `map01` portal); wiring that interior `0x3F` is the following leg.
//!
//! Skip-pass (CLAUDE.md disc-gated convention): `LEGAIA_DISC_BIN` unset /
//! `extracted/` missing.

use std::path::PathBuf;

use legaia_engine_core::scene::{ProtIndex, Scene, SceneHost, SceneTickEvent};
use legaia_engine_core::scene_bundle::{extract_man_payload, find_bundle};
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
    let mut host = open_host()?;
    host.enter_field_scene("town01", 0).expect("enter town01");
    assert_eq!(host.world.mode, SceneMode::Field, "town01 is a field scene");
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

/// Assert the named scene's bundle carries a MAN that parses with the given
/// partition counts (driven off the same `find_bundle` path the host uses).
fn assert_scene_man(index: &ProtIndex, name: &str, want_counts: [i16; 3]) {
    let scene = Scene::load(index, name).unwrap_or_else(|e| panic!("load {name}: {e:#}"));
    let bundle = find_bundle(&scene).unwrap_or_else(|| panic!("{name}: no bundle"));
    let entry_bytes = index
        .entry_bytes_extended(bundle.entry_idx())
        .expect("extended footprint");
    let man = extract_man_payload(&bundle, &entry_bytes)
        .unwrap_or_else(|e| panic!("{name}: extract MAN: {e:#}"))
        .unwrap_or_else(|| panic!("{name}: bundle carries no MAN"));
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
    let mut host = drive_town01_to_map01()?;
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
    // The dungeon's MAN is resolved (the v12-embedded fix) - partitions
    // [18, 70, 20]. This is the source the scripted-boss trigger will read.
    let index = host.index.clone();
    assert_scene_man(&index, "rikuroa", [18, 70, 20]);
    eprintln!(
        "[ok] Leg (Arc-2): map01 -> rikuroa overworld portal -> rikuroa (Field), \
         MAN present. NEXT LEG: the Zeto scripted boss is gated by rikuroa's \
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
// Part C: the scripted first-boss (Zeto) fight, end to end
// ---------------------------------------------------------------------
//
// Zeto (monster id 75 = 0x4B) has NO on-disc formation record - not in
// rikuroa's MAN encounter section and not as an inline armed-YIELD window.
// Retail arms it through the battle-id path (`DAT_8007b7fc = 0x4B` ->
// `FUN_8005567c` collapses ids 0x49..0x4D to a lone-monster cell), gated by a
// first-visit story flag. The engine models this as a rikuroa scene-entry latch
// on flag `0x1BE` (rikuroa cutscene P2[0] "Meta's warning" C1) + the battle-id
// port `World::install_boss_encounter`; the gate flag latches on victory. The
// field-side op that writes the battle-id global in retail is not yet recovered
// (it sits in the scene prescript / an undumped overlay) - this leg exercises
// the faithful interim.

/// Zeto = monster id 75 (0x4B); rikuroa boss gate flag 0x1BE. Mirrors
/// `legaia_engine_core::world::SCRIPTED_SCENE_BOSSES`.
const ZETO_MONSTER_ID: u16 = 75;
const RIKUROA_BOSS_GATE_FLAG: u16 = 0x1BE;

#[test]
fn part_c_rikuroa_arms_and_fights_the_zeto_scripted_boss() {
    use legaia_engine_core::world::BOSS_FORMATION_ID_BASE;

    let Some(mut host) = drive_map01_portal_into("rikuroa") else {
        return;
    };

    // (1) Entering rikuroa (first visit, gate flag clear) armed the boss.
    assert!(
        !host.world.system_flag_test(RIKUROA_BOSS_GATE_FLAG),
        "first visit: the Zeto gate flag starts clear"
    );
    assert_eq!(
        host.world.boss_formation_id,
        Some(BOSS_FORMATION_ID_BASE | ZETO_MONSTER_ID),
        "rikuroa entry arms the Zeto boss formation"
    );
    assert_eq!(
        host.world.pending_boss_victory_flag,
        Some(RIKUROA_BOSS_GATE_FLAG),
        "the victory latch is pending on the gate flag"
    );
    assert!(
        host.world.scripted_formation_pending,
        "the boss fires on the next field step"
    );
    // Real Zeto stats were seeded from the PROT 867 archive.
    let zeto = host
        .world
        .monster_catalog
        .get(ZETO_MONSTER_ID)
        .expect("Zeto stats seeded from the monster archive");
    assert!(zeto.hp > 0, "Zeto carries real archive HP");

    // (2) Step -> the scripted boss flips Field -> Battle.
    host.world.live_gameplay_loop = true;
    assert!(
        host.world.on_field_step(),
        "the forced boss formation triggers on the field step"
    );
    let mut reached_battle = false;
    for _ in 0..240 {
        let _ = host.tick().expect("tick");
        if host.world.mode == SceneMode::Battle {
            reached_battle = true;
            break;
        }
    }
    assert!(reached_battle, "the Zeto cue flips Field -> Battle");
    let monster_slot = host.world.party_count.clamp(1, 3) as usize;
    assert_eq!(
        host.world.actors[monster_slot].battle_monster_id,
        Some(ZETO_MONSTER_ID),
        "the lone enemy slot is Zeto"
    );
    assert!(
        host.world.actors[monster_slot].battle.max_hp > 0,
        "the Zeto slot is seeded with its real HP"
    );

    // (3) Winning latches the first-visit gate flag so the boss does not
    // re-arm. Resolving the boss formation's loot IS the victory in the engine
    // model (it grants XP/gold); drive it directly.
    let boss_formation = host
        .world
        .active_formation
        .clone()
        .expect("boss formation captured at the Field -> Battle transition");
    assert_eq!(
        boss_formation.formation_id,
        BOSS_FORMATION_ID_BASE | ZETO_MONSTER_ID
    );
    let catalog = host.world.monster_catalog.clone();
    let _ = host.world.apply_battle_loot(&boss_formation, &catalog);
    assert!(
        host.world.system_flag_test(RIKUROA_BOSS_GATE_FLAG),
        "beating Zeto latches the rikuroa first-visit gate flag 0x1BE"
    );
    assert_eq!(
        host.world.boss_formation_id, None,
        "the boss latch is consumed on victory"
    );

    eprintln!(
        "[ok] Part C: rikuroa -> armed Zeto (id 75, battle-id path) -> Battle \
         seated with real HP -> victory latches gate flag 0x1BE"
    );
}

#[test]
fn part_c_rikuroa_does_not_rearm_zeto_once_the_gate_flag_is_set() {
    let Some(mut host) = drive_map01_portal_into("rikuroa") else {
        return;
    };
    // Simulate the post-victory world state: the gate flag is already latched.
    host.world.system_flag_set(RIKUROA_BOSS_GATE_FLAG);
    // Re-enter rikuroa from scratch - the latch must see the set flag and skip.
    host.enter_field_scene("rikuroa", 0)
        .expect("re-enter rikuroa");
    assert_eq!(
        host.world.boss_formation_id, None,
        "with the gate flag set, re-entering rikuroa does not re-arm Zeto"
    );
    eprintln!("[ok] Part C: post-victory rikuroa revisit does not re-arm the boss");
}
