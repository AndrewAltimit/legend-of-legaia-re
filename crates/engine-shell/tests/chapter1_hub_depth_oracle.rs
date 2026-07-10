//! Arc 3 "Chapter 1 hub-leg depth" oracle: push depth on the two
//! story-load-bearing Drake hub legs the hub sweep
//! (`chapter1_hub_sweep_oracle.rs`) proved load ungated from `map01`.
//!
//! **Part V - `vozz` (the Ravine-gate opener).** `vozz` (Scripted bundle
//! PROT 103, MAN partitions `[4, 24, 20]`) carries the one concrete cross-leg
//! ordering fact in chapter 1: the Ravine gate flag `0x193` - the `C1` spawn
//! gate on `map01`'s `keikoku` portal record - is SET here. Decoded from the
//! disc bytes at a real opcode boundary: `P1[7]` (the Zalan interaction
//! script) runs `52 AC` + `51 93` (the `0x5x` system-flag SET family:
//! `idx = (lead & 0x8F) << 8 | operand`) at MAN offsets `0xDA4`/`0xDA6`,
//! one-shot-guarded by the op-`0x72` test of `0x2AC` at `0xDA0`. The
//! disc-wide census pins `vozz P1[7]` as the **only** `0x193` SET site on the
//! whole disc. vozz's own `0x3F` chain lists a single destination - back to
//! `map01` (exit record `P2[10]`, gate-1 trigger tiles `(60..62, 2)`) - so
//! the drivable interior hop is the round trip, which this file drives.
//! The partition-2 census also pins three self-latching one-shot beats
//! (`P2[11..=13]`, `C1 = [own flag]`, each SETs `0x2B2/0x2B3/0x2B4`).
//!
//! **Part J - `jou -> jouina` (the Drake Castle door).** `jou` (PROT 630,
//! `[15, 8, 7]`) lists `jouina` + `map01` as `0x3F` destinations. The
//! `jouina` warp is partition-2 record `P2[5]` (gate-1 trigger tiles
//! `(93..95, 97)`), and it is **story-gated**: `C2 = [0x44D]` (requires-all).
//! The only `0x44D` SET site on the disc is `jou`'s own walk-on beat `P2[4]`
//! (`C1 = [0x44D]` self-latch one-shot, `C2 = [0x44B]` - the izumi / noaru
//! Noa-beat chain); `map01 P0[9]` tests it. So a fresh Drake arrival can
//! enter `jou` but NOT `jouina` - the castle door is the hub's second story
//! gate after `keikoku`'s `0x193`. The drive proves the gate both ways
//! in-engine: the fresh walk-on installs nothing, and with `0x44D` set (the
//! disc-decoded requirement) the same walk-on spawns `P2[5]` as the door
//! cutscene, whose player-channel (`0xF8`) ExecMove/HaltAcquire beats now
//! complete through the engine's completion model, so the drive follows the
//! record's trailing `0x3F` all the way to `SceneEntered("jouina")` (see
//! `part_j_drive_jou_castle_door_gate`). `jouina` is additionally pinned by
//! a direct Field-mode load. `jouina` (`[18, 5, 23]`) then chains
//! **deeper**: its `0x3F` set is `{jou, jouinb}` (`P2[20]` -> `jouinb`,
//! `P2[21]` -> back to `jou`, both ungated) - the castle interior is not
//! terminal.
//!
//! Named finding - the strict portal-site join has a blind spot: jou's
//! `P2[5]` contains an undecodable byte region before its `0x3F` (+0xD0), so
//! [`overworld_portal_sites`]'s strict linear pre-scan bails and reports only
//! the `map01` exit record; the resync-capable [`LinearWalker`] (and the
//! executing VM, which follows jumps) reach the op. The oracle pins the
//! asymmetry so a future decoder change that closes it trips the test.
//!
//! Skip-pass (CLAUDE.md disc-gated convention): `LEGAIA_DISC_BIN` unset /
//! `extracted/` missing.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use legaia_engine_core::man_field_scripts::{
    FlagBank, overworld_portal_sites, partition_record_span, partition2_record_gates,
    scene_destinations, system_flag_census, walk_partition_gflag_sites,
};
use legaia_engine_core::scene::{ProtIndex, Scene, SceneHost, SceneTickEvent};
use legaia_engine_core::scene_bundle::{extract_man_payload, find_bundle};
use legaia_engine_core::world::{SceneMode, WorldMapEntityConfig};
use legaia_engine_vm::field_disasm::{FlagKind, InsnInfo, LinearWalker, scene_change_name};

// ---------------------------------------------------------------------
// Discovery + shared drive helpers (patterned on chapter1_hub_sweep_oracle)
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

/// Rim Elm's south-gate exit tile (shared with the spine + hub-sweep oracles).
const TOWN01_SOUTH_GATE: (u8, u8) = (25, 46);

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
    assert_eq!(entered.as_deref(), Some("map01"));
    assert_eq!(host.world.mode, SceneMode::WorldMap);
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

/// Drive `town01 -> map01 -> <dest>` through the overworld portal.
fn drive_to_leg(dest: &str) -> Option<SceneHost> {
    let mut host = drive_town01_to_map01()?;
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
    assert_eq!(entered.as_deref(), Some(dest), "map01 portal loads {dest}");
    assert_eq!(host.world.mode, SceneMode::Field, "{dest} is a field scene");
    Some(host)
}

/// The named scene's MAN payload through the live `find_bundle` path.
fn load_man(index: &ProtIndex, name: &str) -> Vec<u8> {
    let scene = Scene::load(index, name).unwrap_or_else(|e| panic!("load {name}: {e:#}"));
    let bundle = find_bundle(&scene).unwrap_or_else(|| panic!("{name}: no bundle"));
    let entry_bytes = index
        .entry_bytes_extended(bundle.entry_idx())
        .expect("extended footprint");
    extract_man_payload(&bundle, &entry_bytes)
        .unwrap_or_else(|e| panic!("{name}: extract MAN: {e:#}"))
        .unwrap_or_else(|| panic!("{name}: bundle carries no MAN"))
}

fn scene_dest_names(index: &ProtIndex, name: &str) -> BTreeSet<String> {
    let man = load_man(index, name);
    let mf = legaia_asset::man_section::parse(&man).expect("parse MAN");
    scene_destinations(&mf, &man)
        .into_iter()
        .map(|d| d.scene_name)
        .collect()
}

/// Clean-name `0x3F` scene-change sites in one partition-2 record, as
/// `(record_relative_pc, scene_name, index)` - via the resync-capable
/// [`LinearWalker`] (the strict pre-scan `overworld_portal_sites` uses can
/// bail on an undecodable byte before the op; see the jou `P2[5]` finding).
fn p2_scene_changes(
    mf: &legaia_asset::man_section::ManFile,
    man: &[u8],
    record: usize,
) -> Vec<(usize, String, i16)> {
    let Some((start, pc0, len)) = partition_record_span(mf, man, 2, record) else {
        return Vec::new();
    };
    let body = &man[start..start + len];
    let mut out = Vec::new();
    for insn in LinearWalker::new(body, pc0).flatten() {
        if let InsnInfo::SceneChange { index, .. } = insn.info
            && let Some(nm) = scene_change_name(body, &insn)
        {
            out.push((insn.pc, nm, index));
        }
    }
    out
}

// ---------------------------------------------------------------------
// Part V: vozz depth
// ---------------------------------------------------------------------

/// The full disc-verified `0x193` site list inside vozz's partition 1, with
/// the SET site pinned to the byte: `P1[7]` op `0x51` at MAN offset `0xDA6`,
/// immediately after the companion `52 AC` (SET `0x2AC`) at `0xDA4` and the
/// op-`0x72` one-shot guard testing `0x2AC` at `0xDA0`.
#[test]
fn part_v_vozz_flag_0x193_sites_pinned() {
    let Some(host) = open_host() else {
        return;
    };
    let index = host.index.clone();
    let man = load_man(&index, "vozz");
    let mf = legaia_asset::man_section::parse(&man).expect("parse vozz MAN");
    assert_eq!(mf.header.partition_counts, [4, 24, 20], "vozz MAN shape");

    let sites = walk_partition_gflag_sites(&mf, &man, 1);
    let s193: Vec<(usize, FlagKind, u8, usize)> = sites
        .iter()
        .filter(|s| s.bank == FlagBank::System && s.flag == 0x193)
        .map(|s| (s.record, s.kind, s.opcode, s.abs_pc))
        .collect();
    assert_eq!(
        s193,
        vec![
            (0, FlagKind::Test, 0x71, 0x19C),
            (7, FlagKind::Set, 0x51, 0xDA6),
            (8, FlagKind::Test, 0x71, 0x11F9),
            (9, FlagKind::Test, 0x71, 0x1227),
            (12, FlagKind::Clear, 0x61, 0x12BD),
        ],
        "the five 0x193 sites in vozz P1 (Test/Set/Test/Test/Clear)"
    );

    // The setter's immediate context in P1[7]: the op-0x72 one-shot guard on
    // 0x2AC and the companion SET 0x2AC two bytes before the 0x193 SET.
    let ctx: Vec<(FlagKind, usize)> = sites
        .iter()
        .filter(|s| s.record == 7 && s.bank == FlagBank::System && s.flag == 0x2AC)
        .map(|s| (s.kind, s.abs_pc))
        .collect();
    assert!(
        ctx.contains(&(FlagKind::Test, 0xDA0)),
        "P1[7] one-shot guard: op-0x72 Test 0x2AC at 0xDA0, got {ctx:?}"
    );
    assert!(
        ctx.contains(&(FlagKind::Set, 0xDA4)),
        "P1[7] companion SET 0x2AC at 0xDA4, got {ctx:?}"
    );
    eprintln!("[ok] Part V: vozz P1[7] `51 93` @ 0xDA6 sets the Ravine gate flag 0x193");
}

/// vozz's `0x3F` destination set, exit record, and partition-2 gate census.
/// The three one-shot beats `P2[11..=13]` self-latch: each record's `C1` is
/// exactly the flag its own script SETs (`0x2B2`/`0x2B3`/`0x2B4`).
#[test]
fn part_v_vozz_destinations_and_gate_census() {
    let Some(host) = open_host() else {
        return;
    };
    let index = host.index.clone();
    let man = load_man(&index, "vozz");
    let mf = legaia_asset::man_section::parse(&man).expect("parse vozz MAN");

    // (1) The 0x3F chain offers exactly one destination: back to map01.
    let dests = scene_dest_names(&index, "vozz");
    assert_eq!(
        dests,
        BTreeSet::from(["map01".to_string()]),
        "vozz's only 0x3F destination is the overworld exit"
    );

    // (2) The exit record: P2[10] scene-changes to map01 (index 85) at +0x14,
    // and its gate-1 trigger tiles are (60..62, 2).
    let ex = p2_scene_changes(&mf, &man, 10);
    assert_eq!(
        ex,
        vec![(0x14, "map01".to_string(), 85)],
        "vozz exit record P2[10]"
    );
    let scene = Scene::load(&index, "vozz").expect("load vozz");
    let (primary, fallback) = scene.field_tile_triggers(&index).expect("vozz triggers");
    let mut triggers = primary;
    triggers.extend(fallback);
    let exit_tiles: BTreeSet<(u8, u8)> = triggers
        .iter()
        .filter(|t| t.gate == 1 && t.record == 10)
        .map(|t| (t.tile_x, t.tile_z))
        .collect();
    assert_eq!(
        exit_tiles,
        BTreeSet::from([(60, 2), (61, 2), (62, 2)]),
        "vozz exit trigger tiles"
    );
    // The strict portal-site join agrees here (no blind spot on vozz): the
    // only entrance-shaped record is the map01 exit.
    let sites = overworld_portal_sites(&mf, &man, &triggers);
    let site_dests: BTreeSet<(String, u8)> = sites
        .iter()
        .map(|s| (s.scene_name.clone(), s.record))
        .collect();
    assert_eq!(
        site_dests,
        BTreeSet::from([("map01".to_string(), 10)]),
        "vozz portal-site join: only the exit record"
    );

    // (3) Gate census across all 20 partition-2 records.
    let mut gates = BTreeMap::new();
    for r in 0..20usize {
        let (c1, c2) = partition2_record_gates(&mf, &man, r)
            .unwrap_or_else(|| panic!("vozz P2[{r}] gates decode"));
        if !c1.is_empty() || !c2.is_empty() {
            gates.insert(r, (c1, c2));
        }
    }
    let expect: BTreeMap<usize, (Vec<u16>, Vec<u16>)> = BTreeMap::from([
        (0, (vec![0x7], vec![])),
        (1, (vec![0x7], vec![])),
        (3, (vec![0x7], vec![])),
        (6, (vec![0x7], vec![])),
        (7, (vec![0x63A, 0x7], vec![])),
        (8, (vec![0x7], vec![])),
        (9, (vec![0x7], vec![])),
        (11, (vec![0x2B2], vec![])),
        (12, (vec![0x2B3], vec![])),
        (13, (vec![0x2B4], vec![])),
        (16, (vec![0x7], vec![])),
        (18, (vec![0x2AC], vec![0x2B3])),
    ]);
    assert_eq!(gates, expect, "vozz partition-2 C1/C2 gate census");

    // (4) The one-shot latch structure: P2[11..=13] each SET exactly the flag
    // their own C1 lists, so the beat plays once.
    let p2_sites = walk_partition_gflag_sites(&mf, &man, 2);
    for (rec, flag) in [(11usize, 0x2B2u16), (12, 0x2B3), (13, 0x2B4)] {
        assert!(
            p2_sites.iter().any(|s| s.record == rec
                && s.bank == FlagBank::System
                && s.flag == flag
                && s.kind == FlagKind::Set),
            "vozz P2[{rec}] sets its own C1 latch flag {flag:#X}"
        );
    }
    eprintln!("[ok] Part V: vozz dests={{map01}}, exit P2[10] @ (60..62,2), 12 gated P2 records");
}

/// Drive the vozz round trip: `town01 -> map01 -> vozz` through the overworld
/// portal, then the interior hop vozz's `0x3F` chain offers - the exit
/// trigger back to `map01` (WorldMap mode).
#[test]
fn part_v_drive_map01_to_vozz_and_back() {
    let Some(mut host) = drive_to_leg("vozz") else {
        return;
    };
    let index = host.index.clone();
    let man = load_man(&index, "vozz");
    let mf = legaia_asset::man_section::parse(&man).expect("parse vozz MAN");
    assert_eq!(mf.header.partition_counts, [4, 24, 20]);

    // Interior hop: walk onto the exit band at (61, 2) -> back to map01.
    for _ in 0..3 {
        if let SceneTickEvent::SceneEntered { name } = host.tick().expect("tick") {
            panic!("unexpected early transition to {name}");
        }
    }
    host.world.seat_player_at_tile(61, 2);
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
        "vozz's exit record P2[10] returns to the overworld"
    );
    assert_eq!(host.world.mode, SceneMode::WorldMap);
    eprintln!("[ok] Part V: drove map01 -> vozz -> map01 (exit band at (61,2))");
}

// ---------------------------------------------------------------------
// Part J: jou -> jouina
// ---------------------------------------------------------------------

/// Disc-wide flag census: the two chapter-1 hub story gates have exactly one
/// setter each - `0x193` = vozz `P1[7]`, `0x44D` = jou `P2[4]`.
#[test]
fn part_j_census_pins_gate_setters_disc_wide() {
    let Some(host) = open_host() else {
        return;
    };
    let index = host.index.clone();
    let scenes = index.cdname_scene_names();
    let census = system_flag_census(&index, &scenes);

    let sites = |flag: u16| -> Vec<(String, usize, usize, FlagKind)> {
        census
            .get(&flag)
            .map(|hits| {
                hits.iter()
                    .map(|h| (h.scene_name.clone(), h.partition, h.record, h.kind))
                    .collect()
            })
            .unwrap_or_default()
    };

    // 0x193 (the map01 keikoku C1 gate): all five op sites live in vozz P1;
    // the single SET is P1[7].
    let s193 = sites(0x193);
    assert_eq!(
        s193,
        vec![
            ("vozz".to_string(), 1, 0, FlagKind::Test),
            ("vozz".to_string(), 1, 7, FlagKind::Set),
            ("vozz".to_string(), 1, 8, FlagKind::Test),
            ("vozz".to_string(), 1, 9, FlagKind::Test),
            ("vozz".to_string(), 1, 12, FlagKind::Clear),
        ],
        "flag 0x193 census: vozz-only, one SET (P1[7])"
    );

    // 0x44D (the jou -> jouina C2 gate): tested by map01's controller-side
    // P0[9] and by jou's own entry script P1[0] (a scene-state dispatch
    // test the census sees now that the whole-nibble 0x4C widths are
    // pinned), set only by jou's walk-on beat P2[4].
    let s44d = sites(0x44D);
    assert_eq!(
        s44d,
        vec![
            ("map01".to_string(), 0, 9, FlagKind::Test),
            ("jou".to_string(), 1, 0, FlagKind::Test),
            ("jou".to_string(), 2, 4, FlagKind::Set),
        ],
        "flag 0x44D census: one SET (jou P2[4]), tested by map01 P0[9] + jou P1[0]"
    );
    eprintln!("[ok] Part J: disc-wide census pins 0x193 -> vozz P1[7], 0x44D -> jou P2[4]");
}

/// The jou castle-door decode: `P2[5]` carries the `0x3F` to `jouina` behind
/// a `C2 = [0x44D]` requires-all gate; the walk-on beat chain that opens it
/// (`P2[3]` / `P2[4]`) is itself gated on the Noa beat (`C2 = [0x44B]`).
#[test]
fn part_j_jou_jouina_door_decode() {
    let Some(host) = open_host() else {
        return;
    };
    let index = host.index.clone();
    let man = load_man(&index, "jou");
    let mf = legaia_asset::man_section::parse(&man).expect("parse jou MAN");
    assert_eq!(mf.header.partition_counts, [15, 8, 7], "jou MAN shape");

    // (1) 0x3F destination set: the castle interior + the overworld exit.
    let dests = scene_dest_names(&index, "jou");
    assert_eq!(
        dests,
        BTreeSet::from(["jouina".to_string(), "map01".to_string()]),
        "jou 0x3F destinations"
    );

    // (2) The jouina warp is P2[5] (+0xD0, destination index 655); the
    // overworld exit is P2[6] (+0x1A, index 85).
    assert_eq!(
        p2_scene_changes(&mf, &man, 5),
        vec![(0xD0, "jouina".to_string(), 655)],
        "jou P2[5] scene change"
    );
    assert_eq!(
        p2_scene_changes(&mf, &man, 6),
        vec![(0x1A, "map01".to_string(), 85)],
        "jou P2[6] scene change"
    );

    // (3) Gate census: the door P2[5] requires 0x44D; the beat chain P2[3] /
    // P2[4] self-latches (C1 = the flag the beat sets) and requires the Noa
    // beat flag 0x44B; the arrival beat P2[2] one-shots on 0x3E7; the
    // overworld exit P2[6] is ungated.
    let mut gates = BTreeMap::new();
    for r in 0..7usize {
        let (c1, c2) = partition2_record_gates(&mf, &man, r)
            .unwrap_or_else(|| panic!("jou P2[{r}] gates decode"));
        if !c1.is_empty() || !c2.is_empty() {
            gates.insert(r, (c1, c2));
        }
    }
    let expect: BTreeMap<usize, (Vec<u16>, Vec<u16>)> = BTreeMap::from([
        (2, (vec![0x3E7], vec![])),
        (3, (vec![0x44C], vec![0x44B])),
        (4, (vec![0x44D], vec![0x44B])),
        (5, (vec![], vec![0x44D])),
    ]);
    assert_eq!(gates, expect, "jou partition-2 C1/C2 gate census");

    // (4) The beats' own SET ops close the latch loop: P2[2] sets 0x3E7
    // (map01's controller tests it), P2[3] sets 0x44C, P2[4] sets 0x44D.
    let p2_sites = walk_partition_gflag_sites(&mf, &man, 2);
    for (rec, flag) in [(2usize, 0x3E7u16), (3, 0x44C), (4, 0x44D)] {
        assert!(
            p2_sites.iter().any(|s| s.record == rec
                && s.bank == FlagBank::System
                && s.flag == flag
                && s.kind == FlagKind::Set),
            "jou P2[{rec}] sets flag {flag:#X}"
        );
    }

    // (5) The door + exit trigger tiles.
    let scene = Scene::load(&index, "jou").expect("load jou");
    let (primary, fallback) = scene.field_tile_triggers(&index).expect("jou triggers");
    let mut triggers = primary;
    triggers.extend(fallback);
    let tiles_of = |rec: u8| -> BTreeSet<(u8, u8)> {
        triggers
            .iter()
            .filter(|t| t.gate == 1 && t.record == rec)
            .map(|t| (t.tile_x, t.tile_z))
            .collect()
    };
    assert_eq!(
        tiles_of(5),
        BTreeSet::from([(93, 97), (94, 97), (95, 97)]),
        "jouina door trigger tiles"
    );
    assert_eq!(
        tiles_of(6),
        (19..=24).map(|z| (20u8, z)).collect::<BTreeSet<_>>(),
        "map01 exit trigger tiles"
    );

    // (6) Named finding: the strict portal-site join misses the jouina door -
    // P2[5] has an undecodable byte region before +0xD0, so the strict linear
    // pre-scan bails while the resync walker (2) above reaches the op. Pinned
    // so a decoder change that closes the blind spot trips this oracle.
    let sites = overworld_portal_sites(&mf, &man, &triggers);
    let site_dests: BTreeSet<(String, u8)> = sites
        .iter()
        .map(|s| (s.scene_name.clone(), s.record))
        .collect();
    assert_eq!(
        site_dests,
        BTreeSet::from([("map01".to_string(), 6)]),
        "strict join sees only the exit record (the P2[5] blind spot)"
    );
    eprintln!("[ok] Part J: jou P2[5] -> jouina behind C2=[0x44D]; beat chain P2[2..=4] decoded");
}

/// Drive the castle door's **gate** both ways in-engine: fresh (flag clear)
/// the walk-on refuses to spawn `P2[5]`; with `0x44D` set - the disc-decoded
/// requirement - the same walk-on installs the door-cutscene record as the
/// timeline and the drive follows it through to `SceneEntered("jouina")`.
/// A sibling test then pins `jouina` itself (direct Field-mode load + MAN
/// shape + its own deeper destination chain).
///
/// **Player-channel completion model (the driven hop's mechanism):** the
/// `P2[5]` record is a real door cutscene - after its fade / camera / waits
/// it runs the retail halt-acquire handshake against the **player anchor
/// channel** `0xF8` (`A2 F8 06` ExecMove + `C3 F8 00 5E E2 50` HaltAcquire at
/// +0x60, whose operand s16 state-resumes BACKWARD at +0x50).
/// [`legaia_engine_core::field_channels::resolve_target`] keeps its `None`
/// contract for the special `0xF8` target (retail resolves it to the live
/// player object, `FUN_8003C83C`); the timeline stepper instead models the
/// two ops directly: the ExecMove arms a short in-flight countdown
/// (`CutsceneTimeline::player_move_frames`) and the HaltAcquire PARKS at the
/// op until it drains, then steps PAST by encoded width - so the record
/// reaches its trailing `0x3F` at +0xD0 and the driven hop fires the scene
/// change instead of spinning `pc 0x50 -> 0x60` into the frame cap.
#[test]
fn part_j_drive_jou_castle_door_gate() {
    let Some(mut host) = drive_to_leg("jou") else {
        return;
    };
    for _ in 0..3 {
        if let SceneTickEvent::SceneEntered { name } = host.tick().expect("tick") {
            panic!("unexpected early transition to {name}");
        }
    }

    // (1) Fresh arrival: 0x44D is clear, so the door walk-on installs
    // nothing and no transition happens - the castle is story-locked.
    assert!(
        !host.world.system_flag_test(0x44D),
        "fresh chapter-1 state: castle-door gate flag 0x44D is clear"
    );
    host.world.seat_player_at_tile(94, 97);
    for _ in 0..30 {
        if let SceneTickEvent::SceneEntered { name } = host.tick().expect("tick") {
            panic!("gated-out door must not transition, but entered {name}");
        }
        assert!(
            !host.world.cutscene_timeline_active(),
            "gated-out door must not install the P2[5] timeline"
        );
    }
    assert_eq!(host.world.mode, SceneMode::Field, "still in jou");

    // (2) Set the gate (the disc-decoded requirement, normally written by
    // jou's own P2[4] beat once the Noa chain has played) and re-cross: the
    // same walk-on now passes the C2 check and installs the door cutscene.
    host.world.system_flag_set(0x44D);
    host.world.seat_player_at_tile(94, 23); // step off the door band
    for _ in 0..3 {
        if let SceneTickEvent::SceneEntered { name } = host.tick().expect("tick") {
            panic!("unexpected transition to {name} while stepping off");
        }
    }
    host.world.seat_player_at_tile(94, 97);
    let mut installed = false;
    for _ in 0..10 {
        let _ = host.tick().expect("tick");
        if host.world.cutscene_timeline_active() {
            installed = true;
            break;
        }
    }
    assert!(
        installed,
        "with 0x44D set, the door walk-on spawns P2[5] as the cutscene timeline"
    );

    // (3) Drive the installed door cutscene to completion: the record's
    // fades / camera beats / waits play out, the player-channel (0xF8)
    // ExecMove/HaltAcquire beats complete through the engine's completion
    // model, and the trailing 0x3F at +0xD0 fires the driven hop.
    let mut entered = None;
    for _ in 0..900 {
        if let SceneTickEvent::SceneEntered { name } = host.tick().expect("tick") {
            entered = Some(name);
            break;
        }
    }
    assert_eq!(
        entered.as_deref(),
        Some("jouina"),
        "the door cutscene drives the jou -> jouina hop to SceneEntered"
    );
    assert_eq!(
        host.world.mode,
        SceneMode::Field,
        "jouina arrives in Field mode"
    );
    eprintln!("[ok] Part J: jou castle door honors C2=[0x44D] both ways and drives the jouina hop");
}

/// `jouina` itself: loads in Field mode, and its decode shows the castle
/// interior is NOT terminal - it lists `jouinb` (deeper castle) alongside
/// the way back to `jou`; both door records are ungated, and every timeline
/// record `P2[0..=19]` carries the `C1=[0xF]` busy-latch pattern.
#[test]
fn part_j_jouina_loads_and_chains_deeper() {
    let Some(mut host) = open_host() else {
        return;
    };
    host.enter_field_scene("jouina", 0).expect("enter jouina");
    assert_eq!(
        host.world.mode,
        SceneMode::Field,
        "jouina is a field-mode scene"
    );
    for _ in 0..3 {
        if let SceneTickEvent::SceneEntered { name } = host.tick().expect("tick") {
            panic!("unexpected early transition to {name}");
        }
    }
    let index = host.index.clone();
    let man = load_man(&index, "jouina");
    let mf = legaia_asset::man_section::parse(&man).expect("parse jouina MAN");
    assert_eq!(mf.header.partition_counts, [18, 5, 23], "jouina MAN shape");
    let dests = scene_dest_names(&index, "jouina");
    assert_eq!(
        dests,
        BTreeSet::from(["jou".to_string(), "jouinb".to_string()]),
        "jouina chains deeper (jouinb) besides the way back"
    );
    assert_eq!(
        p2_scene_changes(&mf, &man, 20),
        vec![(0x16, "jouinb".to_string(), 664)],
        "jouina P2[20] -> jouinb"
    );
    assert_eq!(
        p2_scene_changes(&mf, &man, 21),
        vec![(0x12, "jou".to_string(), 630)],
        "jouina P2[21] -> jou"
    );
    for r in 0..23usize {
        let (c1, c2) = partition2_record_gates(&mf, &man, r)
            .unwrap_or_else(|| panic!("jouina P2[{r}] gates decode"));
        if r < 20 {
            assert_eq!(
                (c1, c2),
                (vec![0xF], vec![]),
                "jouina P2[{r}] carries the C1=[0xF] busy-latch"
            );
        } else {
            assert!(
                c1.is_empty() && c2.is_empty(),
                "jouina door record P2[{r}] is ungated, got C1={c1:?} C2={c2:?}"
            );
        }
    }
    eprintln!("[ok] Part J: jouina loads (Field); chains deeper -> {{jou, jouinb}}");
}
