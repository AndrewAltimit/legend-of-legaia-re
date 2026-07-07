//! Arc 2 "Chapter 1 Drake hub breadth" oracle: depth-decode + drive the three
//! hub legs the sweep (`chapter1_hub_sweep_oracle.rs`) proved load ungated but
//! never decoded past their partition counts - `cave01`, `vell`, `suimon` -
//! plus the first static decode of `jouinb` (deep Drake Castle, raw
//! destination index 664, reached from `jouina P2[20]`).
//!
//! **Part A - `cave01` (the two-mouth cave).** Scripted bundle, MAN
//! `[1, 13, 18]`. Its `0x3F` chain lists a single named destination (`map01`)
//! but TWO exit records carry it: `P2[0]` (+0x14, gate-0 trigger at `(8,89)`)
//! and `P2[1]` (+0x16, gate-1 walk-on band `(93..94, 96)`) - the cave is a
//! pass-through with a mouth on each side of the Drake range. Nine of the 18
//! partition-2 records are story-gated, forming the longest C1/C2 beat chain
//! decoded on a hub leg so far: six self-latching one-shots (`P2[2]`/`P2[3]`/
//! `P2[8]`/`P2[9]`/`P2[10]`/`P2[11]`, `C1 = [own flag]`) and a three-beat
//! ordered chain `P2[13] -> P2[14] -> P2[15]` (`C2 = [0x15D]` -> `C2 =
//! [0x15E]` -> `C2 = [0x169]`, each latching the next beat's requirement;
//! `P2[16]` SETs the chain's `0x15D` entry key). `P2[15]`'s `C1` also lists
//! `0x142` - the dolk/dolk2 Zeto switch flag - so the final beat stops
//! replaying once Zeto falls. The `P2[13]` gate is proven both ways in-engine
//! (installs-nothing when `0x15D` clear; walk-on spawns the timeline with it
//! set), and the interior hop `cave01 -> map01` is driven to `SceneEntered`.
//!
//! **Part B - `vell` (Vahn's flashback beach / Vidna leg).** Scripted bundle,
//! MAN `[8, 17, 13]`. Single `0x3F` destination (`map01`, exit `P2[10]` +0x14,
//! walk-on band `(88..92, 7)`). Only two gated partition-2 records: `P2[11]`
//! is a plain self-latch one-shot (`C1 = [0x2AF]`, SET at +0x1CF0, tested by
//! `vell P1[3]` - a vell-local pair), and `P2[7]` carries `C1 = [0x63A, 0x7]`,
//! **byte-identical to `vozz P2[7]`'s gate** (pinned as a cross-leg shared
//! pattern). The disc-wide census finds NO script site for `0x63A` anywhere
//! (no set, no clear, no test) - the flag is engine/SCUS-written or simply
//! never set, so the C1 (requires-clear) condition is always satisfied on a
//! fresh save. Round trip `map01 -> vell -> map01` driven.
//!
//! **Part C - `suimon` (the Water Gate corridor).** Shares its 2345-byte
//! `[10, 7, 3]` MAN with `dolk2` (pinned by the sweep oracle). All three
//! partition-2 records are ungated `0x3F` exits to `map01` (+0x1C each) with
//! wide walk-on bands - the Water Gate is a pure pass-through corridor
//! between the two halves of the Drake overworld, story-varied only through
//! the shared controller's `P1[0]` test of the Zeto flag `0x142`. Driven
//! `map01 -> suimon -> map01`.
//!
//! **Part D - `jouinb` (deep Drake Castle; new ground).** Scripted bundle,
//! MAN `[19, 7, 13]`. Loads directly in Field mode. The castle chain does NOT
//! stop here: `P2[10]` chains deeper to `jouinc` (index 672) and `P2[9]`
//! returns to `jouina` (index 655); `jouinc` itself parses (`[43, 18, 60]`)
//! and lists `{jouinb, jouind}` - the Drake Castle interior is a four-deep
//! scene chain `jou -> jouina -> jouinb -> jouinc -> jouind`. Every one of
//! `jouinb`'s 13 partition-2 records is UNGATED (empty C1/C2) - unlike
//! `jouina`'s uniform `C1=[0xF]` busy-latch - so the deep-castle doors are
//! open once the `jou` castle-door gate (`C2=[0x44D]`, depth oracle) is
//! passed. Named decoder asymmetry: the partition-1 destination-table scan
//! (`scene_destinations`) lists only `{jouinc}`, while the resync-capable
//! partition-2 walker and the strict portal-site join BOTH see the `jouina`
//! return door - the reverse of the `jou P2[5]` blind spot, where the strict
//! join was the one that bailed.
//!
//! **Part E - the disc-wide flag census on the new gates.** One-setter facts:
//! `0x2AF` = vell-only pair (`P1[3]` Test / `P2[11]` Set); `0x63A` = zero
//! sites disc-wide; `0x15D` setters all in cave01 (`P1[1]`, `P1[2]`,
//! `P2[16]`); `0x15E`'s only SET is cave01 `P2[13]` and it is read
//! cross-scene by `urudre1 P2[0]` (the Uru Mais dream sequence tests the cave
//! beat); `0x142` has NO SET site in any MAN on the disc (sole non-test site:
//! a Clear in `dolk P1[26]`) - corroborating the wave-5 finding that the
//! dolk/dolk2 switch is written by the battle-id victory path, not by a field
//! script. Census-visible but not asserted (recorded here as reference):
//! `0x159` gains a second setter at `balden P2[3]`; `0x15C` is tested
//! cross-scene by `balden P1[0]`/`P2[12]`; `0x160` is tested by
//! `taiku`/`deene`/`opdeene` `P1[0]`+`P2[5]`.
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
// Discovery + shared drive helpers (patterned on the hub sweep + depth
// oracles; test-local copies by convention)
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

/// Rim Elm's south-gate exit tile (shared with the spine + hub oracles).
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

/// Drive the interior walk-on band at `tile` and assert it returns the player
/// to the overworld (`map01`, WorldMap mode).
fn drive_exit_band_to_map01(host: &mut SceneHost, tile: (u8, u8), what: &str) {
    for _ in 0..3 {
        if let SceneTickEvent::SceneEntered { name } = host.tick().expect("tick") {
            panic!("unexpected early transition to {name}");
        }
    }
    host.world.seat_player_at_tile(tile.0, tile.1);
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
        "{what} returns to the overworld"
    );
    assert_eq!(host.world.mode, SceneMode::WorldMap);
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

/// The `0x3F` destination-name set from the partition-1 destination-table scan.
fn scene_dest_names(index: &ProtIndex, name: &str) -> BTreeSet<String> {
    let man = load_man(index, name);
    let mf = legaia_asset::man_section::parse(&man).expect("parse MAN");
    scene_destinations(&mf, &man)
        .into_iter()
        .map(|d| d.scene_name)
        .collect()
}

/// Clean-name `0x3F` scene-change sites in one partition-2 record via the
/// resync-capable [`LinearWalker`] (copied from the depth oracle).
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

/// Non-empty C1/C2 gate lists across all `n` partition-2 records.
fn p2_gate_census(
    mf: &legaia_asset::man_section::ManFile,
    man: &[u8],
    n: usize,
    scene: &str,
) -> BTreeMap<usize, (Vec<u16>, Vec<u16>)> {
    let mut gates = BTreeMap::new();
    for r in 0..n {
        let (c1, c2) = partition2_record_gates(mf, man, r)
            .unwrap_or_else(|| panic!("{scene} P2[{r}] gates decode"));
        if !c1.is_empty() || !c2.is_empty() {
            gates.insert(r, (c1, c2));
        }
    }
    gates
}

/// Gate-1 walk-on trigger tiles of one record.
fn gate1_tiles_of(index: &ProtIndex, name: &str, rec: u8) -> BTreeSet<(u8, u8)> {
    let scene = Scene::load(index, name).unwrap_or_else(|e| panic!("load {name}: {e:#}"));
    let (primary, fallback) = scene
        .field_tile_triggers(index)
        .unwrap_or_else(|e| panic!("{name} triggers: {e:#}"));
    let mut triggers = primary;
    triggers.extend(fallback);
    triggers
        .iter()
        .filter(|t| t.gate == 1 && t.record == rec)
        .map(|t| (t.tile_x, t.tile_z))
        .collect()
}

/// Assert one partition-2 record SETs `flag` at `abs_pc` (the latch closer).
fn assert_p2_set_site(
    mf: &legaia_asset::man_section::ManFile,
    man: &[u8],
    scene: &str,
    rec: usize,
    flag: u16,
    abs_pc: usize,
) {
    let sites = walk_partition_gflag_sites(mf, man, 2);
    assert!(
        sites.iter().any(|s| s.record == rec
            && s.bank == FlagBank::System
            && s.flag == flag
            && s.kind == FlagKind::Set
            && s.abs_pc == abs_pc),
        "{scene} P2[{rec}] sets flag {flag:#X} at {abs_pc:#X}"
    );
}

// ---------------------------------------------------------------------
// Part A: cave01 - the two-mouth cave + the longest hub-leg beat chain
// ---------------------------------------------------------------------

#[test]
fn part_a_cave01_depth_decode() {
    let Some(host) = open_host() else {
        return;
    };
    let index = host.index.clone();
    let man = load_man(&index, "cave01");
    let mf = legaia_asset::man_section::parse(&man).expect("parse cave01 MAN");
    assert_eq!(mf.header.partition_counts, [1, 13, 18], "cave01 MAN shape");

    // (1) One named 0x3F destination (map01) carried by TWO exit records -
    // the cave has a mouth on each side of the Drake range.
    assert_eq!(
        scene_dest_names(&index, "cave01"),
        BTreeSet::from(["map01".to_string()]),
        "cave01's only 0x3F destination is the overworld"
    );
    assert_eq!(
        p2_scene_changes(&mf, &man, 0),
        vec![(0x14, "map01".to_string(), 85)],
        "cave01 first exit record P2[0]"
    );
    assert_eq!(
        p2_scene_changes(&mf, &man, 1),
        vec![(0x16, "map01".to_string(), 85)],
        "cave01 second exit record P2[1]"
    );
    // P2[1] is the gate-1 walk-on mouth; P2[0]'s trigger is a gate-0 site at
    // (8, 89) (interact-family, one tile - the other mouth).
    assert_eq!(
        gate1_tiles_of(&index, "cave01", 1),
        BTreeSet::from([(93, 96), (94, 96)]),
        "cave01 walk-on mouth band"
    );

    // (2) Gate census: six self-latch one-shots + the ordered three-beat
    // chain P2[13] -> P2[14] -> P2[15] (each C2 = the previous beat's latch).
    let gates = p2_gate_census(&mf, &man, 18, "cave01");
    let expect: BTreeMap<usize, (Vec<u16>, Vec<u16>)> = BTreeMap::from([
        (2, (vec![0x157], vec![])),
        (3, (vec![0x160], vec![])),
        (8, (vec![0x15C], vec![])),
        (9, (vec![0x15A], vec![])),
        (10, (vec![0x15B], vec![])),
        (11, (vec![0x159], vec![])),
        (13, (vec![0x15E], vec![0x15D])),
        (14, (vec![0x169], vec![0x15E])),
        (15, (vec![0x13, 0x142], vec![0x169])),
    ]);
    assert_eq!(gates, expect, "cave01 partition-2 C1/C2 gate census");

    // (3) The latch closers, pinned to the byte: each one-shot SETs its own
    // C1 flag; P2[16] (ungated) SETs 0x15D - the entry key of the beat chain;
    // P2[15] SETs its own 0x13.
    for (rec, flag, pc) in [
        (2usize, 0x157u16, 0x2A62usize),
        (3, 0x160, 0x3214),
        (8, 0x15C, 0x3280),
        (13, 0x15E, 0x35DD),
        (15, 0x13, 0x3BE0),
        (16, 0x15D, 0x3C10),
    ] {
        assert_p2_set_site(&mf, &man, "cave01", rec, flag, pc);
    }

    // (4) The chain beat P2[13]'s walk-on band.
    assert_eq!(
        gate1_tiles_of(&index, "cave01", 13),
        BTreeSet::from([(69, 66), (69, 67), (70, 65), (70, 66), (71, 64), (71, 65)]),
        "cave01 P2[13] beat trigger band"
    );
    eprintln!(
        "[ok] Part A: cave01 two-mouth exits P2[0]/P2[1]; 9 gated P2 records; \
         beat chain 0x15D -> 0x15E -> 0x169 with P2[16] as entry-key setter"
    );
}

#[test]
fn part_a_drive_map01_to_cave01_and_back() {
    let Some(mut host) = drive_to_leg("cave01") else {
        return;
    };
    drive_exit_band_to_map01(&mut host, (94, 96), "cave01's walk-on mouth P2[1]");
    eprintln!("[ok] Part A: drove map01 -> cave01 -> map01 (mouth band at (94,96))");
}

/// Prove the `P2[13]` beat gate both ways in-engine (the depth oracle's
/// part_j pattern): fresh (`0x15D` clear) the walk-on installs nothing; with
/// `0x15D` set - the disc-decoded C2 requirement, normally written by the
/// ungated `P2[16]` beat - the same walk-on spawns the record as the
/// cutscene timeline.
#[test]
fn part_a_drive_cave01_beat_gate_both_ways() {
    let Some(mut host) = drive_to_leg("cave01") else {
        return;
    };
    for _ in 0..3 {
        if let SceneTickEvent::SceneEntered { name } = host.tick().expect("tick") {
            panic!("unexpected early transition to {name}");
        }
    }

    // (1) Fresh chapter-1 arrival: 0x15D is clear, the beat stays dormant.
    assert!(
        !host.world.system_flag_test(0x15D),
        "fresh state: cave01 beat-chain entry flag 0x15D is clear"
    );
    host.world.seat_player_at_tile(70, 65);
    for _ in 0..30 {
        if let SceneTickEvent::SceneEntered { name } = host.tick().expect("tick") {
            panic!("gated-out beat must not transition, but entered {name}");
        }
        assert!(
            !host.world.cutscene_timeline_active(),
            "gated-out beat must not install the P2[13] timeline"
        );
    }
    assert_eq!(host.world.mode, SceneMode::Field, "still in cave01");

    // (2) Set the requirement and re-cross: the walk-on now passes the C2
    // check and installs the beat as the cutscene timeline.
    host.world.system_flag_set(0x15D);
    host.world.seat_player_at_tile(70, 40); // step off the band
    for _ in 0..3 {
        if let SceneTickEvent::SceneEntered { name } = host.tick().expect("tick") {
            panic!("unexpected transition to {name} while stepping off");
        }
    }
    host.world.seat_player_at_tile(70, 65);
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
        "with 0x15D set, the walk-on spawns P2[13] as the cutscene timeline"
    );
    eprintln!("[ok] Part A: cave01 P2[13] honors C2=[0x15D] both ways in-engine");
}

// ---------------------------------------------------------------------
// Part B: vell - one self-latch + the vozz-shared 0x63A gate
// ---------------------------------------------------------------------

#[test]
fn part_b_vell_depth_decode() {
    let Some(host) = open_host() else {
        return;
    };
    let index = host.index.clone();
    let man = load_man(&index, "vell");
    let mf = legaia_asset::man_section::parse(&man).expect("parse vell MAN");
    assert_eq!(mf.header.partition_counts, [8, 17, 13], "vell MAN shape");

    // (1) Single 0x3F destination: the overworld exit P2[10].
    assert_eq!(
        scene_dest_names(&index, "vell"),
        BTreeSet::from(["map01".to_string()]),
        "vell's only 0x3F destination is the overworld"
    );
    assert_eq!(
        p2_scene_changes(&mf, &man, 10),
        vec![(0x14, "map01".to_string(), 85)],
        "vell exit record P2[10]"
    );
    assert_eq!(
        gate1_tiles_of(&index, "vell", 10),
        (88..=92).map(|x| (x, 7u8)).collect::<BTreeSet<_>>(),
        "vell exit trigger band"
    );

    // (2) Gate census: exactly two gated records.
    let gates = p2_gate_census(&mf, &man, 13, "vell");
    let expect: BTreeMap<usize, (Vec<u16>, Vec<u16>)> =
        BTreeMap::from([(7, (vec![0x63A, 0x7], vec![])), (11, (vec![0x2AF], vec![]))]);
    assert_eq!(gates, expect, "vell partition-2 C1/C2 gate census");

    // (3) P2[11] closes its own latch (SET 0x2AF at +0x1CF0).
    assert_p2_set_site(&mf, &man, "vell", 11, 0x2AF, 0x1CF0);

    // (4) Named cross-leg pattern: vell P2[7] and vozz P2[7] carry the
    // byte-identical C1 = [0x63A, 0x7] gate (0x63A has NO script site on the
    // whole disc - see the Part E census - so the requires-clear condition
    // is always satisfied on a fresh save; 0x7 is the shared busy/system
    // slot also gating most vozz records).
    let vozz_man = load_man(&index, "vozz");
    let vozz_mf = legaia_asset::man_section::parse(&vozz_man).expect("parse vozz MAN");
    let (vc1, vc2) = partition2_record_gates(&vozz_mf, &vozz_man, 7).expect("vozz P2[7] gates");
    assert_eq!(
        (vc1, vc2),
        (vec![0x63A, 0x7], vec![]),
        "vozz P2[7] shares vell P2[7]'s gate list"
    );
    eprintln!(
        "[ok] Part B: vell exit P2[10] @ (88..92,7); gates = {{P2[7]: [0x63A,0x7], \
         P2[11]: self-latch 0x2AF}}; 0x63A gate shared with vozz P2[7]"
    );
}

#[test]
fn part_b_drive_map01_to_vell_and_back() {
    let Some(mut host) = drive_to_leg("vell") else {
        return;
    };
    drive_exit_band_to_map01(&mut host, (90, 7), "vell's exit record P2[10]");
    eprintln!("[ok] Part B: drove map01 -> vell -> map01 (exit band at (90,7))");
}

// ---------------------------------------------------------------------
// Part C: suimon - the ungated Water Gate pass-through corridor
// ---------------------------------------------------------------------

#[test]
fn part_c_suimon_depth_decode() {
    let Some(host) = open_host() else {
        return;
    };
    let index = host.index.clone();
    let man = load_man(&index, "suimon");
    let mf = legaia_asset::man_section::parse(&man).expect("parse suimon MAN");
    assert_eq!(mf.header.partition_counts, [10, 7, 3], "suimon MAN shape");

    // (1) All three partition-2 records are 0x3F exits to map01 (+0x1C each)
    // and every one is ungated - the Water Gate is a pure corridor.
    assert_eq!(
        scene_dest_names(&index, "suimon"),
        BTreeSet::from(["map01".to_string()]),
        "suimon's only 0x3F destination is the overworld"
    );
    for r in 0..3usize {
        assert_eq!(
            p2_scene_changes(&mf, &man, r),
            vec![(0x1C, "map01".to_string(), 85)],
            "suimon exit record P2[{r}]"
        );
    }
    assert!(
        p2_gate_census(&mf, &man, 3, "suimon").is_empty(),
        "every suimon partition-2 record is ungated"
    );

    // (2) The exit bands: wide walk-on strips on both ends plus the ramp.
    let r0 = gate1_tiles_of(&index, "suimon", 0);
    let r1 = gate1_tiles_of(&index, "suimon", 1);
    let r2 = gate1_tiles_of(&index, "suimon", 2);
    assert_eq!(r0.len(), 28, "suimon P2[0] band width");
    assert_eq!(r1.len(), 25, "suimon P2[1] band width");
    assert_eq!(r2.len(), 20, "suimon P2[2] band width");
    for (band, tile) in [
        (&r0, (19, 85)),
        (&r0, (66, 45)),
        (&r1, (28, 79)),
        (&r2, (33, 84)),
    ] {
        assert!(band.contains(&tile), "suimon band contains {tile:?}");
    }
    eprintln!(
        "[ok] Part C: suimon = 3 ungated map01 exits (bands 28/25/20 tiles); \
         story variation only via the dolk2-shared controller's 0x142 test"
    );
}

#[test]
fn part_c_drive_map01_to_suimon_and_through() {
    let Some(mut host) = drive_to_leg("suimon") else {
        return;
    };
    drive_exit_band_to_map01(&mut host, (19, 85), "suimon's P2[0] exit band");
    eprintln!("[ok] Part C: drove map01 -> suimon -> map01 (band at (19,85))");
}

// ---------------------------------------------------------------------
// Part D: jouinb - deep Drake Castle, first static decode
// ---------------------------------------------------------------------

#[test]
fn part_d_jouinb_direct_load_and_decode() {
    let Some(mut host) = open_host() else {
        return;
    };
    // (1) Direct Field-mode load (the driven jou -> jouina hop is Arc 1's
    // player-channel work; this scene is pinned statically like jouina).
    host.enter_field_scene("jouinb", 0).expect("enter jouinb");
    assert_eq!(
        host.world.mode,
        SceneMode::Field,
        "jouinb is a field-mode scene"
    );
    for _ in 0..3 {
        if let SceneTickEvent::SceneEntered { name } = host.tick().expect("tick") {
            panic!("unexpected early transition to {name}");
        }
    }

    let index = host.index.clone();
    let man = load_man(&index, "jouinb");
    let mf = legaia_asset::man_section::parse(&man).expect("parse jouinb MAN");
    assert_eq!(mf.header.partition_counts, [19, 7, 13], "jouinb MAN shape");

    // (2) The chain: P2[10] chains DEEPER to jouinc; P2[9] returns to jouina.
    assert_eq!(
        p2_scene_changes(&mf, &man, 9),
        vec![(0x16, "jouina".to_string(), 655)],
        "jouinb P2[9] -> back to jouina"
    );
    assert_eq!(
        p2_scene_changes(&mf, &man, 10),
        vec![(0x16, "jouinc".to_string(), 672)],
        "jouinb P2[10] -> deeper to jouinc"
    );
    assert_eq!(
        gate1_tiles_of(&index, "jouinb", 9),
        [(7, 15), (7, 16), (8, 15), (8, 16), (9, 15), (9, 16)]
            .into_iter()
            .collect::<BTreeSet<_>>(),
        "jouina return-door band"
    );
    assert_eq!(
        gate1_tiles_of(&index, "jouinb", 10),
        BTreeSet::from([(18, 102), (18, 103)]),
        "jouinc onward-door band"
    );

    // (3) Every partition-2 record is UNGATED - no busy-latch (jouina's
    // uniform C1=[0xF]) and no story gate; the deep castle opens once the
    // jou castle door (C2=[0x44D]) is passed.
    assert!(
        p2_gate_census(&mf, &man, 13, "jouinb").is_empty(),
        "all 13 jouinb partition-2 records are ungated"
    );

    // (4) Named decoder asymmetry (reverse of the jou P2[5] blind spot): the
    // partition-1 destination-table scan lists only jouinc, while BOTH the
    // partition-2 walker (2) and the strict portal-site join see the jouina
    // return door.
    assert_eq!(
        scene_dest_names(&index, "jouinb"),
        BTreeSet::from(["jouinc".to_string()]),
        "P1 destination-table scan misses the jouina return door"
    );
    let scene = Scene::load(&index, "jouinb").expect("load jouinb");
    let (primary, fallback) = scene.field_tile_triggers(&index).expect("jouinb triggers");
    let mut triggers = primary;
    triggers.extend(fallback);
    let site_dests: BTreeSet<(String, u8)> = overworld_portal_sites(&mf, &man, &triggers)
        .iter()
        .map(|s| (s.scene_name.clone(), s.record))
        .collect();
    assert_eq!(
        site_dests,
        BTreeSet::from([("jouina".to_string(), 9), ("jouinc".to_string(), 10)]),
        "strict portal-site join sees both castle doors"
    );

    // (5) Chain shape onward: jouinc parses and lists {jouinb, jouind} - the
    // castle is a four-deep interior chain jou -> jouina -> jouinb -> jouinc
    // -> jouind.
    let jouinc_man = load_man(&index, "jouinc");
    let jouinc_mf = legaia_asset::man_section::parse(&jouinc_man).expect("parse jouinc MAN");
    assert_eq!(
        jouinc_mf.header.partition_counts,
        [43, 18, 60],
        "jouinc MAN shape"
    );
    assert_eq!(
        scene_dest_names(&index, "jouinc"),
        BTreeSet::from(["jouinb".to_string(), "jouind".to_string()]),
        "jouinc chains deeper still (jouind) besides the way back"
    );
    eprintln!(
        "[ok] Part D: jouinb loads (Field), [19,7,13], ungated; doors P2[9]->jouina / \
         P2[10]->jouinc; chain continues jouinc [43,18,60] -> {{jouinb, jouind}}"
    );
}

// ---------------------------------------------------------------------
// Part E: disc-wide census on the newly-decoded gate flags
// ---------------------------------------------------------------------

#[test]
fn part_e_flag_census_setters_disc_wide() {
    let Some(host) = open_host() else {
        return;
    };
    let index = host.index.clone();
    let scenes = index.cdname_scene_names();
    let census = system_flag_census(&index, &scenes);

    let set_sites = |flag: u16| -> BTreeSet<(String, usize, usize)> {
        census
            .get(&flag)
            .map(|hits| {
                hits.iter()
                    .filter(|h| h.kind == FlagKind::Set)
                    .map(|h| (h.scene_name.clone(), h.partition, h.record))
                    .collect()
            })
            .unwrap_or_default()
    };

    // (1) vell's 0x2AF is a strictly scene-local pair: one Test (P1[3]), one
    // Set (P2[11]), nothing else disc-wide.
    let s2af: Vec<(String, usize, usize, FlagKind)> = census
        .get(&0x2AF)
        .map(|hits| {
            hits.iter()
                .map(|h| (h.scene_name.clone(), h.partition, h.record, h.kind))
                .collect()
        })
        .unwrap_or_default();
    assert_eq!(
        s2af,
        vec![
            ("vell".to_string(), 1, 3, FlagKind::Test),
            ("vell".to_string(), 2, 11, FlagKind::Set),
        ],
        "flag 0x2AF census: vell-local Test/Set pair"
    );

    // (2) 0x63A - the shared vell/vozz P2[7] C1 gate - IS script-written:
    // the retail-frame census (variant carriers + correct scene windows)
    // surfaces its setters, which the bundle-only shifted-window sweep was
    // structurally blind to. The SETs live in `retockin` P2[8] (+ the
    // `edretoin` epilogue twin) and the rikuroa / rikuroa2 variant MANs'
    // P2[29] (paired with a P2[30] CLEAR). The earlier "no script site
    // disc-wide -> engine/SCUS-side flag" reading is falsified.
    let s63a: BTreeSet<(String, usize, usize, &str)> = census
        .get(&0x63A)
        .map(|hits| {
            hits.iter()
                .map(|h| {
                    let kind = match h.kind {
                        FlagKind::Set => "set",
                        FlagKind::Clear => "clear",
                        FlagKind::Test => "test",
                    };
                    (h.scene_name.clone(), h.partition, h.record, kind)
                })
                .collect()
        })
        .unwrap_or_default();
    assert_eq!(
        s63a,
        BTreeSet::from([
            ("edretoin".to_string(), 2, 7, "clear"),
            ("edretoin".to_string(), 2, 8, "set"),
            ("retockin".to_string(), 2, 7, "clear"),
            ("retockin".to_string(), 2, 8, "set"),
            ("rikuroa".to_string(), 2, 29, "set"),
            ("rikuroa".to_string(), 2, 30, "clear"),
            ("rikuroa2".to_string(), 2, 29, "set"),
            ("rikuroa2".to_string(), 2, 30, "clear"),
        ]),
        "flag 0x63A census: retockin/edretoin + rikuroa/rikuroa2 variant sites"
    );

    // (3) The cave01 beat chain's flags are cave01-owned: every 0x15D setter
    // lives in cave01 (P1[1] / P1[2] / P2[16]); 0x15E's single SET is the
    // P2[13] self-latch, and urudre1 P2[0] reads it cross-scene (the Uru Mais
    // dream tests the cave beat).
    assert_eq!(
        set_sites(0x15D),
        BTreeSet::from([
            ("cave01".to_string(), 1, 1),
            ("cave01".to_string(), 1, 2),
            ("cave01".to_string(), 2, 16),
        ]),
        "flag 0x15D setters: cave01-only"
    );
    assert_eq!(
        set_sites(0x15E),
        BTreeSet::from([("cave01".to_string(), 2, 13)]),
        "flag 0x15E single setter: cave01 P2[13]"
    );
    // 0x15E is cave01-local in the retail frame: the earlier "urudre1 P2[0]
    // tests it cross-scene" attribution came through the shifted scene
    // window (the tester record belonged to a neighbouring block's MAN read
    // under the urudre1 label). The corrected census shows every 0x15E site
    // in cave01.
    assert!(
        census
            .get(&0x15E)
            .is_some_and(|hits| hits.iter().all(|h| h.scene_name == "cave01")),
        "flag 0x15E sites are cave01-local"
    );
    assert_eq!(
        set_sites(0x157),
        BTreeSet::from([("cave01".to_string(), 1, 1), ("cave01".to_string(), 2, 2)]),
        "flag 0x157 setters: cave01-only"
    );

    // (4) 0x142 - the Caruban-beat / dolk->dolk2 switch - IS script-SET:
    // the retail-frame census surfaces the writers the bundle-only shifted
    // sweep missed. The setters live in the rikuroa streaming-carrier MAN
    // (P1[10..12] + the post-victory record P2[50], the self-latching C1
    // one-shot the firehose caught live: `51 42`, `ra 0x801E3598`) and in
    // dolk2's own streaming MAN (P1[0]/P1[1], re-asserting on entry). The
    // wave-earlier "no SET site disc-wide -> battle-path write" reading is
    // falsified. dolk's bundle still carries the P1[26] Clear.
    let s142 = census.get(&0x142).expect("0x142 has census sites");
    assert_eq!(
        set_sites(0x142),
        BTreeSet::from([
            ("dolk2".to_string(), 1, 0),
            ("dolk2".to_string(), 1, 1),
            ("rikuroa".to_string(), 1, 10),
            ("rikuroa".to_string(), 1, 11),
            ("rikuroa".to_string(), 1, 12),
            ("rikuroa".to_string(), 2, 50),
        ]),
        "flag 0x142 setters: rikuroa post-Caruban records + dolk2 re-assert"
    );
    assert!(
        s142.iter().any(|h| h.scene_name == "dolk"
            && h.partition == 1
            && h.record == 26
            && h.kind == FlagKind::Clear),
        "dolk P1[26] clears 0x142"
    );

    eprintln!(
        "[ok] Part E: 0x2AF vell-local; 0x63A retockin/rikuroa-written; \
         0x15D/0x15E/0x157 cave01-owned; 0x142 SET by the rikuroa \
         post-Caruban records (pool census)"
    );
}

/// Part F - the full Drake Castle interior chain, driven end-to-end in one
/// session: `jou -> jouina -> jouinb -> jouinc`. The first hop is the
/// C2=[0x44D] door cutscene (its player-channel `0xF8` ExecMove/HaltAcquire
/// beats complete through the timeline stepper's completion model); the
/// deeper hops are the ungated castle doors pinned by parts D/J. This is the
/// deepest driven interior chain in the engine.
#[test]
fn part_f_castle_chain_e2e() {
    let Some(mut host) = drive_to_leg("jou") else {
        return;
    };
    let index = host.index.clone();
    for _ in 0..3 {
        if let SceneTickEvent::SceneEntered { name } = host.tick().expect("tick") {
            panic!("unexpected early transition to {name}");
        }
    }

    // Hop 1: jou -> jouina through the story-gated door cutscene.
    host.world.system_flag_set(0x44D);
    host.world.seat_player_at_tile(94, 97);
    let mut entered = None;
    for _ in 0..900 {
        if let SceneTickEvent::SceneEntered { name } = host.tick().expect("tick") {
            entered = Some(name);
            break;
        }
    }
    assert_eq!(entered.as_deref(), Some("jouina"), "castle door hop");
    assert_eq!(host.world.mode, SceneMode::Field);

    // Hop 2: jouina -> jouinb (ungated exit record P2[20]; band discovered
    // from the live trigger table rather than hardcoded).
    for _ in 0..3 {
        if let SceneTickEvent::SceneEntered { name } = host.tick().expect("tick") {
            panic!("arrival spawn bounced straight to {name}");
        }
    }
    let band = gate1_tiles_of(&index, "jouina", 20);
    let &(bx, bz) = band.iter().next().expect("jouina P2[20] has a door band");
    host.world.seat_player_at_tile(bx, bz);
    let mut entered = None;
    for _ in 0..900 {
        if let SceneTickEvent::SceneEntered { name } = host.tick().expect("tick") {
            entered = Some(name);
            break;
        }
    }
    assert_eq!(entered.as_deref(), Some("jouinb"), "jouina onward door");
    assert_eq!(host.world.mode, SceneMode::Field);

    // Hop 3: jouinb -> jouinc (ungated onward band pinned in part D).
    for _ in 0..3 {
        if let SceneTickEvent::SceneEntered { name } = host.tick().expect("tick") {
            panic!("arrival spawn bounced straight to {name}");
        }
    }
    host.world.seat_player_at_tile(18, 102);
    let mut entered = None;
    for _ in 0..900 {
        if let SceneTickEvent::SceneEntered { name } = host.tick().expect("tick") {
            entered = Some(name);
            break;
        }
    }
    assert_eq!(entered.as_deref(), Some("jouinc"), "jouinb onward door");
    assert_eq!(host.world.mode, SceneMode::Field);

    eprintln!("[ok] Part F: drove jou -> jouina -> jouinb -> jouinc end-to-end");
}
