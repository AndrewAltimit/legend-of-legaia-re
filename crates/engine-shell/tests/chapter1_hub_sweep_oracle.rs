//! Arc 2 "Chapter 1 Drake hub sweep" oracle: the remaining `map01` overworld
//! legs past the covered spine (`town01 -> map01 -> keikoku` and the boss leg
//! `map01 -> rikuroa / dolk -> dolk2`), decoded and driven statically from disc
//! bytes - no emulator capture.
//!
//! The Drake overworld (`map01`) is a hub: its controller lists a dozen
//! destinations and its `.MAP` walk-on tile triggers install one overworld
//! portal per town/dungeon entrance (see
//! [`legaia_engine_core::man_field_scripts::overworld_portal_sites`]). The
//! boss-spine oracle already covers `rikuroa` / `dolk` / `dolk2`; this file
//! sweeps the other five interior legs - `cave01`, `vell`, `vozz`, `suimon`,
//! `jou` - plus `dolk2`'s own onward destinations.
//!
//! **Part A - `dolk2` onward destinations.** `dolk2` (the post-boss `dolk`
//! variant, PROT 76, v12-embedded MAN, partitions `[10, 7, 3]`) lists a single
//! `0x3F` named-scene-change: back to `map01`. It is a terminal interior with no
//! deeper leg - the boss chain returns the player to the overworld.
//!
//! **Part B - the hub sweep.** For each of `cave01`, `vell`, `vozz`, `suimon`,
//! `jou`: driving `town01 -> map01` and stepping onto that leg's overworld portal
//! tile loads the scene in [`SceneMode::Field`], and the scene's MAN parses with
//! its pinned partition counts. All five are first-class `Scripted`-bundle
//! scenes (unlike `rikuroa` / `dolk2`, whose MAN is v12-embedded) and all five
//! come up cleanly - no load failures in this sweep.
//!
//! **Part C - the gate census.** The overworld-entrance story gates are the
//! partition-2 record's C1/C2 flag lists (retail `FUN_8003BDE0`). On `map01`
//! only the Ravine (`keikoku`) entrance carries a gate (`C1 = [0x193]`); every
//! one of the five swept legs installs **unconditionally** (empty C1/C2). The
//! `0x482` mist-wall pattern is a *different* structure (a partition-2 band
//! record with no `0x3F`, covered by the spine oracle), and the one op-`0x70`
//! branch on `map01` is the `dolk`/`dolk2` destination switch on flag `0x142` -
//! a destination selector inside the `dolk` entrance record, not a spawn gate on
//! a swept leg. So no C1/C2 progression gate stands between a fresh Drake arrival
//! and any of the five legs.
//!
//! **Named finding - `suimon` and `dolk2` share a MAN.** `suimon`'s Scripted
//! bundle (PROT 77) and `dolk2`'s v12-embedded bundle (PROT 76) carry a
//! byte-identical MAN payload (partitions `[10, 7, 3]`, 2345 bytes, single
//! destination back to `map01`). This is pinned as an invariant so a future
//! change that de-duplicates or mis-resolves one of the two bundles trips the
//! oracle. It is an observed byte-identity, not a claim about why the disc
//! reuses the payload.
//!
//! Skip-pass (CLAUDE.md disc-gated convention): `LEGAIA_DISC_BIN` unset /
//! `extracted/` missing.

use std::collections::BTreeSet;
use std::path::PathBuf;

use legaia_engine_core::man_field_scripts::{
    overworld_portal_sites, partition2_record_gates, scene_destinations,
};
use legaia_engine_core::scene::{ProtIndex, Scene, SceneHost, SceneTickEvent};
use legaia_engine_core::world::{SceneMode, WorldMapEntityConfig};

// ---------------------------------------------------------------------
// Discovery + shared drive helpers
// ---------------------------------------------------------------------

fn extracted_dir() -> Option<PathBuf> {
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

fn open_host() -> Option<SceneHost> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    let extracted = extracted_dir()?;
    Some(SceneHost::open_extracted(&extracted).expect("open SceneHost"))
}

/// Rim Elm's south-gate exit tile - the gate-1 walk-on trigger whose
/// partition-2 record's `0x3F` leaves for `map01` (shared with the spine
/// oracles).
const TOWN01_SOUTH_GATE: (u8, u8) = (25, 46);

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

/// The named scene's MAN payload through the engine's resolution order
/// (bundle first, streaming variant carrier fallback) - the same
/// `field_man_payload` path the live host uses.
fn load_man(index: &ProtIndex, name: &str) -> Vec<u8> {
    let scene = Scene::load(index, name).unwrap_or_else(|e| panic!("load {name}: {e:#}"));
    scene
        .field_man_payload(index)
        .unwrap_or_else(|e| panic!("{name}: field_man_payload: {e:#}"))
        .unwrap_or_else(|| panic!("{name}: no MAN resolves"))
}

/// Assert the named scene's MAN parses with the given partition counts.
fn assert_scene_man(index: &ProtIndex, name: &str, want_counts: [i16; 3]) -> Vec<u8> {
    let man = load_man(index, name);
    let mf = legaia_asset::man_section::parse(&man)
        .unwrap_or_else(|e| panic!("{name}: parse MAN: {e:#}"));
    assert_eq!(
        mf.header.partition_counts, want_counts,
        "{name} MAN partition counts"
    );
    man
}

/// The set of `0x3F` named-scene-change destinations decoded from a scene's MAN.
fn scene_dest_names(index: &ProtIndex, name: &str) -> BTreeSet<String> {
    let man = load_man(index, name);
    let mf = legaia_asset::man_section::parse(&man).expect("parse MAN");
    scene_destinations(&mf, &man)
        .into_iter()
        .map(|d| d.scene_name)
        .collect()
}

/// Every swept hub leg and its pinned MAN partition counts.
const HUB_LEGS: &[(&str, [i16; 3])] = &[
    ("cave01", [1, 13, 18]),
    ("vell", [8, 17, 13]),
    ("vozz", [4, 24, 20]),
    ("suimon", [10, 7, 3]),
    ("jou", [15, 8, 7]),
];

// ---------------------------------------------------------------------
// Part A: dolk2's onward destinations
// ---------------------------------------------------------------------

#[test]
fn part_a_dolk2_onward_destination_is_map01_only() {
    let Some(host) = open_host() else {
        return;
    };
    let index = host.index.clone();
    // dolk2's real MAN is the streaming variant carrier (ext 70, partitions
    // [29, 73, 17]); its only named 0x3F destination is back to map01.
    assert_scene_man(&index, "dolk2", [29, 73, 17]);
    let dests = scene_dest_names(&index, "dolk2");
    assert_eq!(
        dests,
        BTreeSet::from(["map01".to_string()]),
        "dolk2 is a terminal interior: its single 0x3F leads back to the \
         overworld, got {dests:?}"
    );
    eprintln!("[ok] Part A: dolk2 onward destination set = {{map01}}");
}

// ---------------------------------------------------------------------
// Part B: the hub sweep - every remaining map01 interior leg loads
// ---------------------------------------------------------------------

#[test]
fn part_b_map01_installs_a_portal_for_every_hub_leg() {
    let Some(host) = drive_town01_to_map01() else {
        return;
    };
    for (dest, _) in HUB_LEGS {
        assert!(
            find_portal_tile(&host, dest).is_some(),
            "map01 installs an overworld portal for {dest}"
        );
    }
    eprintln!("[ok] Part B: map01 installs portals for cave01/vell/vozz/suimon/jou");
}

/// Drive `map01 -> <dest>` through the overworld portal; assert the leg loads in
/// Field mode with its MAN present + the pinned partition shape.
fn drive_and_assert_leg(dest: &str, want_counts: [i16; 3]) {
    let Some(mut host) = drive_town01_to_map01() else {
        return;
    };
    let tile = find_portal_tile(&host, dest)
        .unwrap_or_else(|| panic!("map01 installs a {dest} overworld portal"));
    host.world.seat_player_at_tile(tile.0, tile.1);
    host.world.set_pad(0);
    let mut entered = None;
    for _ in 0..12 {
        if let SceneTickEvent::SceneEntered { name } = host.tick().expect("tick") {
            entered = Some(name);
            break;
        }
    }
    assert_eq!(
        entered.as_deref(),
        Some(dest),
        "the {dest} overworld portal loads the scene"
    );
    assert_eq!(
        host.world.mode,
        SceneMode::Field,
        "{dest} is a field-mode scene"
    );
    let index = host.index.clone();
    assert_scene_man(&index, dest, want_counts);
    eprintln!(
        "[ok] Leg: map01 -> {dest} portal at {tile:?} -> {dest} (Field), MAN {want_counts:?}"
    );
}

#[test]
fn part_b_leg_cave01() {
    drive_and_assert_leg("cave01", [1, 13, 18]);
}

#[test]
fn part_b_leg_vell() {
    drive_and_assert_leg("vell", [8, 17, 13]);
}

#[test]
fn part_b_leg_vozz() {
    drive_and_assert_leg("vozz", [4, 24, 20]);
}

#[test]
fn part_b_leg_suimon() {
    drive_and_assert_leg("suimon", [10, 7, 3]);
}

#[test]
fn part_b_leg_jou() {
    drive_and_assert_leg("jou", [15, 8, 7]);
    // jou also lists an interior onward leg to jouina (its inn/interior variant)
    // alongside the map01 exit.
    let Some(host) = open_host() else {
        return;
    };
    let index = host.index.clone();
    let dests = scene_dest_names(&index, "jou");
    for expected in ["map01", "jouina"] {
        assert!(
            dests.contains(expected),
            "jou lists {expected} among its 0x3F destinations; got {dests:?}"
        );
    }
}

// ---------------------------------------------------------------------
// Part C: the story-gate census on the swept legs
// ---------------------------------------------------------------------

#[test]
fn part_c_hub_leg_entrances_are_ungated() {
    let Some(host) = open_host() else {
        return;
    };
    let index = host.index.clone();
    let man = load_man(&index, "map01");
    let mf = legaia_asset::man_section::parse(&man).expect("parse map01 MAN");

    // Join the .MAP tile triggers to the partition-2 records to recover, per
    // destination, the record index the entrance spawns.
    let scene = Scene::load(&index, "map01").expect("load map01");
    let (primary, fallback) = scene.field_tile_triggers(&index).expect("map01 triggers");
    let mut triggers = primary;
    triggers.extend(fallback);
    let sites = overworld_portal_sites(&mf, &man, &triggers);

    let record_for = |dest: &str| -> Option<u8> {
        sites
            .iter()
            .find(|s| s.scene_name == dest)
            .map(|s| s.record)
    };

    // (1) Each swept leg's entrance record installs unconditionally: empty C1/C2.
    for (dest, _) in HUB_LEGS {
        let rec = record_for(dest).unwrap_or_else(|| panic!("map01 has a {dest} portal record"));
        let (c1, c2) = partition2_record_gates(&mf, &man, rec as usize)
            .unwrap_or_else(|| panic!("{dest} portal record {rec} gates decode"));
        assert!(
            c1.is_empty() && c2.is_empty(),
            "{dest} entrance (rec {rec}) is ungated, got C1={c1:?} C2={c2:?}"
        );
    }

    // (2) Contrast: the covered keikoku entrance DOES carry the 0x193 C1 gate -
    // this pins that the census above is discriminating, not vacuous.
    let keikoku_rec = record_for("keikoku").expect("map01 has a keikoku portal record");
    let (kc1, _kc2) =
        partition2_record_gates(&mf, &man, keikoku_rec as usize).expect("keikoku gates decode");
    assert_eq!(
        kc1,
        vec![0x193],
        "keikoku entrance (rec {keikoku_rec}) carries the C1=[0x193] Ravine gate"
    );

    // (3) The one op-0x70 branch among the entrances is the dolk/dolk2
    // destination switch on flag 0x142 - a destination selector, not a spawn
    // gate. Its record installs unconditionally (empty C1/C2) and carries the
    // conditional alternative.
    let dolk_site = sites
        .iter()
        .find(|s| s.scene_name == "dolk")
        .expect("map01 has a dolk portal");
    let cond = dolk_site
        .conditional
        .as_ref()
        .expect("dolk entrance carries the op-0x70 dolk2 alternative");
    assert_eq!(cond.flag, 0x142, "dolk/dolk2 switch is on flag 0x142");
    assert_eq!(cond.scene_name, "dolk2", "flag 0x142 set -> dolk2");
    let (dc1, dc2) =
        partition2_record_gates(&mf, &man, dolk_site.record as usize).expect("dolk gates decode");
    assert!(
        dc1.is_empty() && dc2.is_empty(),
        "dolk entrance record is ungated (the flag test is a destination switch, \
         not a spawn gate); got C1={dc1:?} C2={dc2:?}"
    );

    eprintln!(
        "[ok] Part C: cave01/vell/vozz/suimon/jou are ungated; keikoku=C1[0x193]; \
         dolk->dolk2 op-0x70 flag 0x142 is a destination switch"
    );
}

// ---------------------------------------------------------------------
// Named finding (corrected): the "suimon == dolk2 MAN" identity was frame
// bleed - dolk2's real MAN is its streaming carrier
// ---------------------------------------------------------------------

#[test]
fn named_finding_suimon_and_dolk2_share_man_payload() {
    let Some(host) = open_host() else {
        return;
    };
    let index = host.index.clone();
    // The earlier "suimon and dolk2 ship a byte-identical [10, 7, 3] MAN"
    // identity was the CDNAME-shifted scene window: dolk2's unshifted window
    // bled into suimon's block and picked up suimon's v12 sidecar (ext 76),
    // so both labels decoded suimon's 2345-byte MAN. In the retail frame,
    // suimon keeps that MAN (its scripted bundle) and dolk2 resolves its own
    // streaming carrier (ext 70, [29, 73, 17]).
    let suimon = load_man(&index, "suimon");
    let dolk2 = load_man(&index, "dolk2");
    assert_eq!(suimon.len(), 2345, "suimon's MAN is 2345 bytes");
    let suimon_mf = legaia_asset::man_section::parse(&suimon).expect("parse suimon MAN");
    assert_eq!(suimon_mf.header.partition_counts, [10, 7, 3]);
    let dolk2_mf = legaia_asset::man_section::parse(&dolk2).expect("parse dolk2 MAN");
    assert_eq!(dolk2_mf.header.partition_counts, [29, 73, 17]);
    assert_ne!(
        suimon, dolk2,
        "dolk2's real MAN is its own streaming carrier, not suimon's sidecar"
    );
    eprintln!(
        "[ok] corrected finding: suimon MAN ({} B) != dolk2 MAN ({} B)",
        suimon.len(),
        dolk2.len()
    );
}
