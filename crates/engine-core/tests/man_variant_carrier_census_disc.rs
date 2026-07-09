//! Disc-gated: the streaming **variant MAN carriers** and the story-spine
//! flag writers the carrier-complete SYSTEM-flag census surfaces.
//!
//! Thirteen retail blocks ship a second MAN as the type-3 chunk of a
//! standalone `data_field_streaming` entry - story-state variants with
//! different partition counts and different scripts than the bundle MAN
//! (for the v12-family dungeons `rikuroa` / `dolk2` the streaming carrier
//! is the scene's ONLY MAN; the live script heap at the Mt. Rikuroa Caruban
//! beat byte-matches PROT `0157`'s chunk). A bundle-only census is
//! structurally blind to every script op these carriers hold - which is
//! exactly where the chapter-1 story-spine writers turned out to live:
//!
//!   - `0x142` (Caruban beat / dolk->dolk2 switch): SET by the rikuroa
//!     carrier's P1[10..12] + post-victory record P2[50] (the C1 self-latch
//!     the firehose caught live, `51 42`, `ra 0x801E3598`), re-asserted by
//!     dolk2's carrier P1[0..1], CLEARed by dolk's bundle P1[26].
//!   - `0x482` (Drake mist walls): SET by the `other7` script pool
//!     P1[15]/P1[39], CLEARed by the epilogue variant carriers
//!     (`edbalden` / `eddoman`).
//!   - `0x1BE`: SET by `geremi` P2[0] (Jeremi's arrival one-shot "Meta's
//!     warning", C1 = itself) - surfaced by the partition-2 named-record
//!     header fix, NOT a rikuroa/Zeto flag.
//!
//! Skips + passes when `LEGAIA_DISC_BIN` / extracted assets are missing
//! (CLAUDE.md disc-gated convention).

use std::collections::BTreeSet;
use std::path::PathBuf;

use legaia_engine_core::man_field_scripts::{
    flag_test_bytescan, scene_man_carriers, system_flag_census,
};
use legaia_engine_core::scene::{ProtIndex, Scene};
use legaia_engine_vm::field_disasm::FlagKind;

fn extracted_dir() -> Option<PathBuf> {
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

fn open_index() -> Option<ProtIndex> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    let extracted = extracted_dir()?;
    Some(ProtIndex::open_extracted(&extracted).expect("open ProtIndex"))
}

/// The thirteen streaming variant carriers, pinned by `(scene, entry_idx)`.
#[test]
fn thirteen_streaming_variant_carriers_disc_wide() {
    let Some(index) = open_index() else { return };
    let mut found: BTreeSet<(String, u32)> = BTreeSet::new();
    for name in index.cdname_scene_names() {
        let Ok(scene) = Scene::load(&index, &name) else {
            continue;
        };
        for c in scene_man_carriers(&index, &scene) {
            if c.is_variant() {
                found.insert((name.clone(), c.entry_idx));
            }
        }
    }
    eprintln!("[carriers] {found:?}");
    let want: BTreeSet<(String, u32)> = [
        ("dolk2", 70u32),
        ("rikuroa2", 122),
        ("rikuroa", 157),
        ("rayman", 201),
        ("station", 228),
        ("balden2", 320),
        ("ropeway2", 339),
        ("taiku", 373),
        ("doman", 401),
        ("taiku2", 427),
        ("nilboa2", 648),
        ("edbalden", 792),
        ("eddoman", 817),
    ]
    .into_iter()
    .map(|(s, i)| (s.to_string(), i))
    .collect();
    assert_eq!(
        found, want,
        "streaming variant MAN carrier set (scene, entry_idx)"
    );
}

/// The carrier-complete census surfaces the chapter-1 spine writers.
#[test]
fn spine_flag_writers_surface_in_the_carrier_census() {
    let Some(index) = open_index() else { return };
    let scenes = index.cdname_scene_names();
    let census = system_flag_census(&index, &scenes);

    // 0x142: rikuroa carrier SETs (incl. the P2[50] post-victory one-shot
    // the firehose caught live) + dolk2 carrier re-asserts.
    let s142: BTreeSet<(String, bool, usize, usize)> = census
        .get(&0x142)
        .expect("0x142 sites")
        .iter()
        .filter(|h| h.kind == FlagKind::Set)
        .map(|h| (h.scene_name.clone(), h.variant, h.partition, h.record))
        .collect();
    assert_eq!(
        s142,
        BTreeSet::from([
            ("dolk2".to_string(), true, 1, 0),
            ("dolk2".to_string(), true, 1, 1),
            ("rikuroa".to_string(), true, 1, 10),
            ("rikuroa".to_string(), true, 1, 11),
            ("rikuroa".to_string(), true, 1, 12),
            ("rikuroa".to_string(), true, 2, 50),
        ]),
        "0x142 SET sites (all in streaming variant carriers)"
    );

    // 0x482: set in other6/other7, cleared by the epilogue carriers.
    let s482_set: BTreeSet<(String, usize, usize)> = census
        .get(&0x482)
        .expect("0x482 sites")
        .iter()
        .filter(|h| h.kind == FlagKind::Set)
        .map(|h| (h.scene_name.clone(), h.partition, h.record))
        .collect();
    assert_eq!(
        s482_set,
        BTreeSet::from([("other7".to_string(), 1, 15), ("other7".to_string(), 1, 39),]),
        "0x482 SET sites (other7 script pool)"
    );
    assert!(
        census
            .get(&0x482)
            .unwrap()
            .iter()
            .any(|h| { h.scene_name == "edbalden" && h.variant && h.kind == FlagKind::Clear }),
        "0x482 cleared by the edbalden epilogue variant carrier"
    );

    // 0x1BE: geremi's arrival one-shot (P2[0] self-latch) - the flag the
    // earlier misattributed frame read as a rikuroa/Zeto gate.
    let s1be: BTreeSet<(String, usize, usize, bool)> = census
        .get(&0x1BE)
        .expect("0x1BE sites")
        .iter()
        .filter(|h| h.kind == FlagKind::Set)
        .map(|h| (h.scene_name.clone(), h.partition, h.record, h.variant))
        .collect();
    assert_eq!(
        s1be,
        BTreeSet::from([("geremi".to_string(), 2, 0, false)]),
        "0x1BE single SET: geremi P2[0] (Jeremi arrival one-shot)"
    );
}

/// The rikuroa carrier IS the scene's MAN (v12-family: no bundle MAN), and
/// its post-victory record P2[50] gates on `0x142` itself - the C1
/// self-latching one-shot shape.
#[test]
fn rikuroa_carrier_p2_50_is_the_0x142_self_latch() {
    use legaia_engine_core::man_field_scripts::partition2_record_gates;
    let Some(index) = open_index() else { return };
    let scene = Scene::load(&index, "rikuroa").expect("load rikuroa");
    let man = scene
        .field_man_payload(&index)
        .expect("payload")
        .expect("rikuroa MAN resolves");
    let mf = legaia_asset::man_section::parse(&man).expect("parse");
    assert_eq!(mf.header.partition_counts, [13, 29, 64]);
    let (c1, c2) = partition2_record_gates(&mf, &man, 50).expect("P2[50] gates");
    assert_eq!(
        c1,
        vec![0x142],
        "P2[50] C1 blocks respawn once 0x142 is set"
    );
    assert!(c2.is_empty(), "P2[50] has no requires-all gate");
}

/// `geremi`'s Jeremi-arrival cutscene record P2[0] both SETS `0x1BE` (at its
/// script head) and lists `0x1BE` as its own C1 one-shot gate - the same
/// self-latching shape as rikuroa's `0x142`. This is the record whose C1
/// evaluation is the "engine-side reader" a reader-watch capture caught: the
/// gate evaluator `FUN_8003BDE0` tests `0x1BE` on every geremi entry (via the
/// field-overlay scene-init at `0x801D218C`), which is why the flag looked
/// "write-only" to the inline-opcode census - it is read from the record
/// HEADER gate list, not an inline `0x70` TEST.
#[test]
fn geremi_p2_0_is_the_0x1be_self_latch() {
    use legaia_engine_core::man_field_scripts::partition2_record_gates;
    let Some(index) = open_index() else { return };
    let scene = Scene::load(&index, "geremi").expect("load geremi");
    let man = scene
        .field_man_payload(&index)
        .expect("payload")
        .expect("geremi MAN resolves");
    let mf = legaia_asset::man_section::parse(&man).expect("parse");
    assert_eq!(mf.header.partition_counts, [18, 70, 20]);
    let (c1, c2) = partition2_record_gates(&mf, &man, 0).expect("P2[0] gates");
    assert_eq!(
        c1,
        vec![0x1BE],
        "P2[0] C1 blocks the arrival cutscene once 0x1BE is set"
    );
    assert!(c2.is_empty(), "P2[0] has no requires-all gate");
}

/// The READER of flag `0x225` (549, the Rim Elm opening one-shot) is the same
/// record-gate evaluator `FUN_8003BDE0`, disc-wide across the Rim Elm scene
/// variants: `P2[3]` C1 = `[0x225]` (blocks the opening once it has played)
/// and `P2[4]` C2 contains `0x225` (the post-naming beat requires the opening
/// done). It is read ONLY from these record-HEADER C1/C2 gate lists, never an
/// inline `0x70` TEST - which is why the inline-opcode census reports zero
/// test sites. (The WRITER of `0x225` is a separate, still-open question: a
/// direct code path, not carrier bytecode.)
#[test]
fn flag_549_reader_is_the_rim_elm_p2_gate() {
    use legaia_engine_core::man_field_scripts::partition2_record_gates;
    let Some(index) = open_index() else { return };
    // Canonical case pinned exactly; the other variants share the shape.
    let scene = Scene::load(&index, "town01").expect("load town01");
    let man = scene
        .field_man_payload(&index)
        .expect("payload")
        .expect("town01 MAN resolves");
    let mf = legaia_asset::man_section::parse(&man).expect("parse");
    let (c1_3, _) = partition2_record_gates(&mf, &man, 3).expect("P2[3] gates");
    assert_eq!(c1_3, vec![0x225], "town01 P2[3] C1 = the opening one-shot");
    let (c1_4, c2_4) = partition2_record_gates(&mf, &man, 4).expect("P2[4] gates");
    assert_eq!(
        c1_4,
        vec![0x226],
        "town01 P2[4] C1 = its own one-shot (550)"
    );
    assert!(
        c2_4.contains(&0x225),
        "town01 P2[4] C2 requires the opening done (549 set)"
    );
    // Disc-wide: 0x225 is read as a gate in the four Rim Elm variants.
    let mut scenes_gating = 0usize;
    for name in index.cdname_scene_names() {
        let Ok(scene) = Scene::load(&index, &name) else {
            continue;
        };
        let Some(man) = scene.field_man_payload(&index).ok().flatten() else {
            continue;
        };
        let Ok(mf) = legaia_asset::man_section::parse(&man) else {
            continue;
        };
        let n_p2 = *mf.header.partition_counts.get(2).unwrap_or(&0) as usize;
        let gates_549 = (0..n_p2).any(|i| {
            partition2_record_gates(&mf, &man, i)
                .map(|(c1, c2)| c1.contains(&0x225) || c2.contains(&0x225))
                .unwrap_or(false)
        });
        if gates_549 {
            scenes_gating += 1;
        }
    }
    assert!(
        scenes_gating >= 4,
        "0x225 is read as a C1/C2 gate in >= 4 Rim Elm variants, got {scenes_gating}"
    );
}

/// The chapter-2 (Sebucus) progression spine, mined from the chapter-2 poll
/// capture and pinned via the C1/C2 record-gate lists (`FUN_8003BDE0`). The
/// same self-latch + requires-all shape as the chapter-1 gates chains the
/// teien beat sequence into the tower into geremi:
///
/// - `teien` `P2[1]` C1=`[0x1C9]` (one-shot) sets `0x1C8`;
///   `P2[2]` C1=`[0x1C9]` C2=`[0x1C8]` (needs step 1) sets `0x1C9`;
///   `P2[5]` C1=`[0x332]` C2=`[0x1C9]` sets `0x332`.
/// - `tower` `P2[2]` C1=`[0x1C7]` C2=`[0x1C9]`: available once the teien arc
///   is reached (`0x1C9`), sets tower-clear `0x1C7`.
/// - `geremi` `P2[1]` C2=`[0x1C7]`: a geremi beat that requires the tower.
/// - `balden` `P2[19]` C1=`[0x5B3]` self-latch / `P2[18]` C2=`[0x5B3]`
///   successor (the town01/549 shape).
/// - `map02` `P2[9]` C1=`[0x332]` C2=`[0x1C9]`: the overworld-side one-shot
///   mirroring the teien arc.
///
/// (The low flags `0x8C..0x8F` the poll also caught are tower/teien-LOCAL
/// switch state, tested by inline actor scripts only - not progression gates.)
#[test]
fn chapter2_sebucus_gate_spine() {
    use legaia_engine_core::man_field_scripts::partition2_record_gates;
    let Some(index) = open_index() else { return };
    let gates = |scene_name: &str, rec: usize| -> (Vec<u16>, Vec<u16>) {
        let scene = Scene::load(&index, scene_name).expect("load");
        let man = scene
            .field_man_payload(&index)
            .expect("payload")
            .expect("MAN");
        let mf = legaia_asset::man_section::parse(&man).expect("parse");
        partition2_record_gates(&mf, &man, rec).expect("gates")
    };

    // teien beat sequence.
    assert_eq!(gates("teien", 1), (vec![0x1C9], vec![]));
    assert_eq!(gates("teien", 2), (vec![0x1C9], vec![0x1C8]));
    assert_eq!(gates("teien", 5), (vec![0x332], vec![0x1C9]));
    // tower, gated on the teien arc, latches tower-clear.
    assert_eq!(gates("tower", 2), (vec![0x1C7], vec![0x1C9]));
    // geremi beat requiring the tower.
    assert_eq!(gates("geremi", 1), (vec![], vec![0x1C7]));
    // balden self-latch + successor.
    assert_eq!(gates("balden", 19), (vec![0x5B3], vec![]));
    assert_eq!(gates("balden", 18), (vec![], vec![0x5B3]));
    // overworld mirror of the teien arc.
    assert_eq!(gates("map02", 9), (vec![0x332], vec![0x1C9]));
}

/// The chapter-2 **dungeon** gate families - `taiku`/`doman`/`rayman` - mined
/// STATICALLY from the disc C1/C2 record-header gates (`FUN_8003BDE0`), the same
/// self-latch / requires-all shapes as the Sebucus spine.
///
/// IMPORTANT provenance note: unlike the teien/tower/balden spine (which the
/// chapter-2 state-poll walked organically), the poll never traversed these
/// dungeons - the only time their flags appear in any 07-08 capture is a bulk
/// save-state LOAD (1392 flags set then cleared 15 ticks later) plus the map01
/// scene-entry mass-clear, never an in-scene organic set. So these families are
/// disc-static gate STRUCTURE, not poll-confirmed progression order; a live
/// dungeon capture is still owed to pin the play order. The structure itself is
/// robust (record-header fields, not the desync-prone inline bytecode walk).
///
/// - `taiku` story cluster (`0x384..=0x390`): two self-latch pairs
///   (`0x390`: `P2[8]` one-shot / `P2[9]` successor; `0x38F`: `P2[10]` /
///   `P2[15]`) plus the entry beats. The `0x505..=0x519` cluster is dungeon
///   -LOCAL puzzle-switch state (multi-flag C1 groups), not story spine.
/// - `rayman` arc: an "arc-reached" group (`P2[3..]` C2=`[0x1EC]`) fanning off
///   the `P2[7]` one-shot `0x1EB`, plus a linear requires-all sub-chain
///   `0x201` -> `P2[12]` (`0x1FB`) -> `P2[18]` (`0x200`) -> `P2[19]` (`0x1FC`).
/// - `rayman2` = the same MAN gated by a shared C1 on low flag `0x7` across
///   `P2[2..=21]` (the variant discriminator; base `rayman` carries `0x7` on
///   `P2[22]` only). Two carriers for one location, selected by `0x7`.
/// - `doman` (tiny): `P2[4]` one-shot `0x3FB`; `P2[3]` successor needs `0x6E7`
///   (set in a different scene - the cross-scene gate pattern).
/// - `taiku2` is a single-record sub-room variant: no gate family.
#[test]
fn chapter2_dungeon_gate_families() {
    use legaia_engine_core::man_field_scripts::partition2_record_gates;
    let Some(index) = open_index() else { return };
    let gates = |scene_name: &str, rec: usize| -> (Vec<u16>, Vec<u16>) {
        let scene = Scene::load(&index, scene_name).expect("load");
        let man = scene
            .field_man_payload(&index)
            .expect("payload")
            .expect("MAN");
        let mf = legaia_asset::man_section::parse(&man).expect("parse");
        partition2_record_gates(&mf, &man, rec).expect("gates")
    };

    // taiku: 0x390 and 0x38F self-latch pairs (one-shot setter + successor).
    assert_eq!(gates("taiku", 8), (vec![0x390], vec![]));
    assert_eq!(gates("taiku", 9), (vec![], vec![0x390]));
    assert_eq!(gates("taiku", 10), (vec![0x38F], vec![]));
    assert_eq!(gates("taiku", 15), (vec![], vec![0x38F]));

    // rayman: the linear requires-all sub-chain 0x201 -> 0x1FB -> 0x200 -> 0x1FC.
    assert_eq!(gates("rayman", 12), (vec![0x1FB], vec![0x201]));
    assert_eq!(gates("rayman", 18), (vec![0x200], vec![0x1FB]));
    assert_eq!(gates("rayman", 19), (vec![0x1FC], vec![0x200]));
    // the arc-reached fan-out gates on 0x1EC; 0x1EB is the entry one-shot.
    assert_eq!(gates("rayman", 3), (vec![], vec![0x1EC]));
    assert_eq!(gates("rayman", 7), (vec![0x1EB], vec![]));

    // rayman2: the SAME chain, each record additionally C1-gated on 0x7 (the
    // variant discriminator). Base rayman does not carry 0x7 on these records.
    assert_eq!(gates("rayman2", 12), (vec![0x1FB, 0x7], vec![0x201]));
    assert_eq!(gates("rayman2", 3), (vec![0x7], vec![0x1EC]));
    assert!(!gates("rayman", 12).0.contains(&0x7));
    assert!(!gates("rayman", 3).0.contains(&0x7));

    // doman: entry one-shot 0x3FB; a successor gated on the cross-scene 0x6E7.
    assert_eq!(gates("doman", 4), (vec![0x3FB], vec![]));
    assert_eq!(gates("doman", 3), (vec![], vec![0x6E7]));
}

/// The chapter-2 `balden`/`balden2` and `station`/`station3` gate families,
/// mined the same disc-static way (C1/C2 record headers, `FUN_8003BDE0`).
///
/// - **balden** is a full arc around `0x1D5` (the balden-reached flag): entry
///   `P2[0]` C1=`[0x1D4,0x1D2]` C2=`[0x1D5]`, the `0x1D5` group (`P2[3]`/`[16]`/
///   `[17]` one-shots, `P2[8]`/`[12]` successors), a `0x1C0`/`0x1CB` pair
///   (`P2[9]`), a `0x346` cross-scene successor (`P2[14]`), and the `0x5B3`
///   self-latch pair (`P2[19]`/`P2[18]`) already in the Sebucus spine. NB the
///   `0x5B3` beat is the ONE chapter-2 dungeon gate the poll caught ORGANICALLY
///   (`flagset` in `scene=balden` @tick 89729); the rest are disc-static.
/// - **balden2** is a sibling streaming carrier with a BYTE-different body
///   (differs ~53 bytes) but an IDENTICAL gate family - same 20 partition-2
///   records, same C1/C2 on every one. Unlike `rayman2` (which adds a `0x7`
///   discriminator), the two balden carriers carry no gate-level selector, so
///   the variant is chosen by the streaming slot, not a flag.
/// - **balden `P2[15]`** C1=`[0x3FD,0x359]` gates on the `ropeway2`
///   switch-group flags - a cross-scene link into that puzzle's state.
/// - **station**/`station3` are small and both gate on taiku's `0x38F`
///   (`station P2[24]` C2=`[0x38F]`, `station3 P2[2]` C2=`[0x38F]`), so `0x38F`
///   is a shared taiku/station area-progress flag. Poll never walked them.
#[test]
fn chapter2_balden_station_gate_families() {
    use legaia_asset::man_section::parse as parse_man;
    use legaia_engine_core::man_field_scripts::partition2_record_gates;
    let Some(index) = open_index() else { return };
    let all_gates = |scene_name: &str| -> Vec<(Vec<u16>, Vec<u16>)> {
        let scene = Scene::load(&index, scene_name).expect("load");
        let man = scene
            .field_man_payload(&index)
            .expect("payload")
            .expect("MAN");
        let mf = parse_man(&man).expect("parse");
        let n_p2 = *mf.header.partition_counts.get(2).unwrap_or(&0) as usize;
        (0..n_p2)
            .map(|r| partition2_record_gates(&mf, &man, r).unwrap_or_default())
            .collect()
    };

    let balden = all_gates("balden");
    // balden arc around 0x1D5 + the cross-scene links.
    assert_eq!(balden[0], (vec![0x1D4, 0x1D2], vec![0x1D5]));
    assert_eq!(balden[9], (vec![0x1CB], vec![0x1C0]));
    assert_eq!(balden[14], (vec![], vec![0x346]));
    // P2[15] gates on the ropeway2 switch-group flags (cross-scene link).
    assert_eq!(balden[15], (vec![0x3FD, 0x359], vec![]));
    // the 0x5B3 self-latch pair (also in the Sebucus spine; poll-confirmed).
    assert_eq!(balden[19], (vec![0x5B3], vec![]));
    assert_eq!(balden[18], (vec![], vec![0x5B3]));

    // balden2 is a sibling carrier: identical gate family, no 0x7-style selector.
    assert_eq!(
        all_gates("balden2"),
        balden,
        "balden2 gate family mirrors balden"
    );

    // station / station3: small, both gate on taiku's 0x38F (shared area flag).
    let station = all_gates("station");
    assert_eq!(station[19], (vec![0x36B], vec![]));
    assert_eq!(station[24], (vec![], vec![0x38F]));
    let station3 = all_gates("station3");
    assert_eq!(station3[2], (vec![0x467], vec![0x38F]));
}

/// The chapter-2 `ropeway`/`ropeway2` and `jiji` gate families - the ONLY
/// chapter-2 dungeons the poll walked ORGANICALLY (every flag below was caught
/// as an in-scene `flagset`, not a bulk-load artifact), so both the structure
/// AND the play order are poll-confirmed here.
///
/// - **ropeway** is sparse: one-shot entry gates `0x1D6` (`P2[5]`/`P2[6]`; the
///   ropeway-reached flag, contiguous with balden's `0x1D5`), `0x321`
///   (`P2[18]`), `0x514` (`P2[30]`, adjacent to taiku's `0x505..0x519`).
/// - **ropeway2** (the streaming variant) shares `0x1D6`/`0x321`, adds a
///   `0x5A8` cross-scene successor (`P2[23]`), and - the payoff - **resolves
///   the switch-group consumer**: `P2[31..=34]` each C1=`[0x359]`
///   C2=`[0x3FF,0x400,0x401,0x402]`, i.e. they spawn only when all four switch
///   bits are set AND the `0x359` commit is still clear, then `0x359` latches
///   them shut. The consumer is INTERNAL (same scene, header C2 gate), not the
///   "external" site the inline census hinted at. Poll order confirmed: the
///   four switches flip, then `0x359` commits.
/// - **jiji** (small): one-shot pairs `0x305` (`P2[2]`), `0x306`
///   (`P2[3]`/`P2[4]`), `0x3BD` (`P2[6]`/`P2[7]`).
#[test]
fn chapter2_ropeway_jiji_gate_families() {
    use legaia_asset::man_section::parse as parse_man;
    use legaia_engine_core::man_field_scripts::partition2_record_gates;
    let Some(index) = open_index() else { return };
    let gates = |scene_name: &str, rec: usize| -> (Vec<u16>, Vec<u16>) {
        let scene = Scene::load(&index, scene_name).expect("load");
        let man = scene
            .field_man_payload(&index)
            .expect("payload")
            .expect("MAN");
        let mf = parse_man(&man).expect("parse");
        partition2_record_gates(&mf, &man, rec).unwrap_or_default()
    };

    // ropeway one-shot entry gates.
    assert_eq!(gates("ropeway", 5), (vec![0x1D6], vec![]));
    assert_eq!(gates("ropeway", 6), (vec![0x1D6], vec![]));
    assert_eq!(gates("ropeway", 18), (vec![0x321], vec![]));
    assert_eq!(gates("ropeway", 30), (vec![0x514], vec![]));

    // ropeway2: the switch-group CONSUMER - P2[31..=34] all require the four
    // switch bits (C2) and block once the 0x359 commit is set (C1).
    let switches = vec![0x3FF, 0x400, 0x401, 0x402];
    for rec in 31..=34 {
        assert_eq!(
            gates("ropeway2", rec),
            (vec![0x359], switches.clone()),
            "ropeway2 P2[{rec}] is a switch-group payoff consumer"
        );
    }
    // shared entry gates + the cross-scene successor.
    assert_eq!(gates("ropeway2", 5), (vec![0x1D6], vec![]));
    assert_eq!(gates("ropeway2", 23), (vec![], vec![0x5A8]));

    // jiji one-shot pairs.
    assert_eq!(gates("jiji", 2), (vec![0x305], vec![]));
    assert_eq!(gates("jiji", 3), (vec![0x306], vec![]));
    assert_eq!(gates("jiji", 4), (vec![0x306], vec![]));
    assert_eq!(gates("jiji", 6), (vec![0x3BD], vec![]));
    assert_eq!(gates("jiji", 7), (vec![0x3BD], vec![]));
}

/// The chapter-2 `dohaty`/`retock`/`retockin`/`stone` gate families - the
/// remaining `map02`-hub spokes, mined disc-static (C1/C2 record headers). None
/// were walked in the poll, so this is structure, not confirmed play order.
///
/// - **dohaty**: a six-record group `P2[3..=8]` all C1=`[0xF]` (a low-flag
///   first-visit / variant gate, like retockin's `0x7`), a `0x344` one-shot
///   (`P2[10]`), and a `0x63D` self-latch pair (`P2[14]`/`P2[15]`).
/// - **retock** (rich): its progression depends CROSS-SCENE on balden's
///   `0x1D5` - the `P2[13/14/15]` chain over `0x337`/`0x339`/`0x33A` all gate
///   on `0x1D5` (C2), and `P2[28]` needs it outright. A second internal chain
///   runs `0x357` -> `P2[33]` (`0x502`) -> `P2[31]`/`P2[32]` (C2=`0x502`).
/// - **retockin** = the `0x7`-gated interior variant (~30 records C1-carry
///   `0x7`, the variant discriminator, like rayman2); it SHARES `0x502` and
///   `0x357` with retock (same dungeon, exterior/interior), plus a `0x205`
///   self-latch pair (`P2[4]`/`P2[34]`).
/// - **stone** is trivial: a single `0x590` one-shot (`P2[6]`).
#[test]
fn chapter2_dohaty_retock_stone_gate_families() {
    use legaia_asset::man_section::parse as parse_man;
    use legaia_engine_core::man_field_scripts::partition2_record_gates;
    let Some(index) = open_index() else { return };
    let all_gates = |scene_name: &str| -> Vec<(Vec<u16>, Vec<u16>)> {
        let scene = Scene::load(&index, scene_name).expect("load");
        let man = scene
            .field_man_payload(&index)
            .expect("payload")
            .expect("MAN");
        let mf = parse_man(&man).expect("parse");
        let n_p2 = *mf.header.partition_counts.get(2).unwrap_or(&0) as usize;
        (0..n_p2)
            .map(|r| partition2_record_gates(&mf, &man, r).unwrap_or_default())
            .collect()
    };

    // dohaty: the 0xF first-visit group, the 0x344 one-shot, the 0x63D pair.
    let dohaty = all_gates("dohaty");
    for (i, g) in dohaty[3..=8].iter().enumerate() {
        let rec = i + 3;
        assert_eq!(
            *g,
            (vec![0xF], vec![]),
            "dohaty P2[{rec}] is in the 0xF group"
        );
    }
    assert_eq!(dohaty[10], (vec![0x344], vec![]));
    assert_eq!(dohaty[14], (vec![0x63D], vec![]));
    assert_eq!(dohaty[15], (vec![], vec![0x63D]));

    // retock: the balden-0x1D5 cross-scene dependency + two internal chains.
    let retock = all_gates("retock");
    assert_eq!(retock[13], (vec![0x339, 0x1D5], vec![]));
    assert_eq!(retock[14], (vec![0x33A, 0x337], vec![0x1D5, 0x339]));
    assert_eq!(retock[15], (vec![0x337, 0x33A, 0x339], vec![0x1D5]));
    assert_eq!(
        retock[28],
        (vec![], vec![0x1D5]),
        "retock needs balden's 0x1D5"
    );
    assert_eq!(retock[33], (vec![0x502], vec![0x357]));
    assert_eq!(retock[31], (vec![0x33E], vec![0x502]));

    // retockin: 0x7-gated variant, shares 0x502/0x357 with retock, 0x205 pair.
    let retockin = all_gates("retockin");
    let with_0x7 = retockin.iter().filter(|(c1, _)| c1.contains(&0x7)).count();
    assert!(
        with_0x7 > 20,
        "retockin is the 0x7-gated variant ({with_0x7} records carry 0x7)"
    );
    assert_eq!(retockin[4], (vec![0x205], vec![]));
    assert_eq!(retockin[34], (vec![], vec![0x205]));
    assert_eq!(
        retockin[42],
        (vec![0x357, 0x43B], vec![]),
        "shares retock's 0x357"
    );
    assert_eq!(
        retockin[45],
        (vec![0x617], vec![0x502]),
        "shares retock's 0x502"
    );

    // stone: a single one-shot.
    let stone = all_gates("stone");
    assert_eq!(stone[6], (vec![0x590], vec![]));
}

/// The `map02` Sebucus HUB's own gate family. The hub is deliberately sparse -
/// a router (33 partition-1 door/NPC scripts) whose partition-2 layer carries
/// only TWO gated records, both **overworld-mirror one-shots** of a dungeon-arc
/// completion (the same shape: C1 = the mirror flag the record sets, C2 = the
/// dungeon prerequisite it waits on):
///
/// - `P2[9]` C1=`[0x332]` C2=`[0x1C9]` - the teien-arc mirror (also pinned in
///   `chapter2_sebucus_gate_spine`).
/// - `P2[10]` C1=`[0x357]` C2=`[0x3AD]` - the retock-area mirror; its `0x357`
///   is the SAME flag retock/retockin gate on (`retock P2[33]`,
///   `retockin P2[42]`), so this is the hub-side record that reflects the
///   retock dungeon back onto the overworld.
///
/// Every OTHER progression gate for the chapter lives in a spoke dungeon's MAN,
/// not the hub - the hub just mirrors two arc-completions.
#[test]
fn chapter2_map02_hub_gate_family() {
    use legaia_asset::man_section::parse as parse_man;
    use legaia_engine_core::man_field_scripts::partition2_record_gates;
    let Some(index) = open_index() else { return };
    let scene = Scene::load(&index, "map02").expect("load map02");
    let man = scene
        .field_man_payload(&index)
        .expect("payload")
        .expect("MAN");
    let mf = parse_man(&man).expect("parse");
    let n_p2 = *mf.header.partition_counts.get(2).unwrap_or(&0) as usize;
    let mut gated_recs = Vec::new();
    let mut gates = Vec::new();
    for r in 0..n_p2 {
        let Some((c1, c2)) = partition2_record_gates(&mf, &man, r) else {
            continue;
        };
        if c1.is_empty() && c2.is_empty() {
            continue;
        }
        gated_recs.push(r);
        gates.push((c1, c2));
    }

    // The hub has exactly two gated partition-2 records.
    assert_eq!(
        gated_recs,
        [9, 10],
        "map02 hub carries only the two overworld-mirror beats"
    );
    assert_eq!(gates[0], (vec![0x332], vec![0x1C9]), "teien-arc mirror");
    assert_eq!(
        gates[1],
        (vec![0x357], vec![0x3AD]),
        "retock-area mirror; 0x357 shared with retock/retockin"
    );
}

/// `town0c` is NOT a chapter-2 Sebucus spoke - it is a **Rim Elm (Vahn's
/// hometown, `town01`) story-state VARIANT**. (The poll's "scene" spoke is a
/// non-entity: it is the state-poll CSV's column header, not a CDNAME map.)
///
/// `town01`/`town0b`/`town0c` all carry the same Rim Elm opening-chain block at
/// `P2[3..=11]` - the `0x225` (=549) opening one-shot -> `0x226` -> `0x227`
/// chain plus the `0x231/0x232/0x233/0x141` sub-chains - which is exactly the
/// 549-reader gate family (`flag_549_reader_is_the_rim_elm_p2_gate`). So these
/// are progressive story-state renditions of the one town, keyed by that chain,
/// and `town0c` being in the chapter-2 poll is a hometown REVISIT (its opening
/// chain was long-since latched: no organic set of it appears in the poll).
/// `town0d` is the fully `0x7`-gated later variant (the low-flag discriminator
/// also seen on rayman2/retockin/dohaty). This corrects the "ch2 spoke" reading.
#[test]
fn town0c_is_a_rim_elm_state_variant_not_a_ch2_spoke() {
    use legaia_asset::man_section::parse as parse_man;
    use legaia_engine_core::man_field_scripts::partition2_record_gates;
    let Some(index) = open_index() else { return };
    let gates = |scene_name: &str, rec: usize| -> (Vec<u16>, Vec<u16>) {
        let scene = Scene::load(&index, scene_name).expect("load");
        let man = scene
            .field_man_payload(&index)
            .expect("payload")
            .expect("MAN");
        let mf = parse_man(&man).expect("parse");
        partition2_record_gates(&mf, &man, rec).unwrap_or_default()
    };

    // The Rim Elm opening-chain block is byte-identical across the three
    // variants at P2[3,4,5,8,9,10,11] (P2[7] town0b adds 0x141 to its C1).
    for &rec in &[3usize, 4, 5, 8, 9, 10, 11] {
        let base = gates("town01", rec);
        assert_eq!(
            gates("town0b", rec),
            base,
            "town0b P2[{rec}] mirrors town01"
        );
        assert_eq!(
            gates("town0c", rec),
            base,
            "town0c P2[{rec}] mirrors town01"
        );
    }
    // The chain head is the 549 (0x225) opening one-shot -> 0x226 -> 0x227.
    assert_eq!(gates("town0c", 3), (vec![0x225], vec![]));
    assert_eq!(gates("town0c", 4), (vec![0x226], vec![0x225]));
    assert_eq!(gates("town0c", 5), (vec![0x227], vec![0x226]));

    // town0d is the 0x7-gated later variant.
    let scene = Scene::load(&index, "town0d").expect("load town0d");
    let man = scene
        .field_man_payload(&index)
        .expect("payload")
        .expect("MAN");
    let mf = parse_man(&man).expect("parse");
    let n_p2 = *mf.header.partition_counts.get(2).unwrap_or(&0) as usize;
    let with_0x7 = (0..n_p2)
        .filter_map(|r| partition2_record_gates(&mf, &man, r))
        .filter(|(c1, _)| c1.contains(&0x7))
        .count();
    assert!(
        with_0x7 > 10,
        "town0d is the 0x7-gated Rim Elm variant ({with_0x7} records carry 0x7)"
    );
}

/// The `nilboa` (Nivora Ravine; music track M111 ニルボア/NIRUBOA) gate family -
/// the first scene mined for the NEXT region beyond Sebucus, disc-static.
///
/// - A first-visit **entry group** `P2[0..=3]` each C1-shares `0x456` (blocked
///   once the area is entered) with a per-record switch flag
///   (`0xD`/`0x50C`/`0x50D`/`0x50E`); `P2[4..=6]` are the `0xD` successors.
/// - A `0x47x` puzzle cluster (`0x475`/`0x476` at `P2[14]`; `P2[18]`/`P2[19]`
///   C2-gate on `0x475`) and a `0x45x` cluster (`0x456`/`0x457`/`0x458`).
/// - `P2[32]` C1=`[0x47E]` C2=`[0x370]` - a cross-scene successor.
/// - The same `0xF` low-flag variant gate as the Sebucus dungeons
///   (`P2[8/9/12/13/15/16/36/37]`); `nilboa2` (variant carrier, entry 648) is
///   the fully `0xF`-gated trimmed rendition.
///
/// (`map03`, the chapter-3 hub, routes to `taiku`/`station3`/`korb3`/`bubu1`/
/// `son`/`deroa` - none of them "jankeria", which resolves to no CDNAME scene.)
#[test]
fn nilboa_nivora_ravine_gate_family() {
    use legaia_asset::man_section::parse as parse_man;
    use legaia_engine_core::man_field_scripts::partition2_record_gates;
    let Some(index) = open_index() else { return };
    let all_gates = |scene_name: &str| -> Vec<(Vec<u16>, Vec<u16>)> {
        let scene = Scene::load(&index, scene_name).expect("load");
        let man = scene
            .field_man_payload(&index)
            .expect("payload")
            .expect("MAN");
        let mf = parse_man(&man).expect("parse");
        let n_p2 = *mf.header.partition_counts.get(2).unwrap_or(&0) as usize;
        (0..n_p2)
            .map(|r| partition2_record_gates(&mf, &man, r).unwrap_or_default())
            .collect()
    };

    let nilboa = all_gates("nilboa");
    // the 0x456 entry group + its 0xD successors.
    assert_eq!(nilboa[0], (vec![0xD, 0x456], vec![]));
    assert_eq!(nilboa[1], (vec![0x50C, 0x456], vec![]));
    assert!(nilboa[2].0.contains(&0x456) && nilboa[3].0.contains(&0x456));
    assert_eq!(nilboa[4], (vec![], vec![0xD]));
    // the 0x475 puzzle cluster.
    assert_eq!(nilboa[14], (vec![0x475, 0x476], vec![]));
    assert_eq!(nilboa[18], (vec![], vec![0x475]));
    // a cross-scene successor.
    assert_eq!(nilboa[32], (vec![0x47E], vec![0x370]));

    // nilboa2 = the 0xF-gated variant carrier.
    let nilboa2 = all_gates("nilboa2");
    let with_0xf = nilboa2.iter().filter(|(c1, _)| c1.contains(&0xF)).count();
    assert!(
        with_0xf > 6,
        "nilboa2 is the 0xF-gated Nivora variant ({with_0xf} records carry 0xF)"
    );
}

/// The `map03` (chapter-3 / Karisto) hub region gate families - disc-static
/// (none walked in the poll). `map03`'s `0x3F` doors route to `taiku`/
/// `station3`/`korb3`/`bubu1`/`son`/`deroa` (taiku + station3 already mined
/// as ch2 spokes; "jankeria" is not among them and resolves to no CDNAME
/// scene).
///
/// - **map03** is an even purer router than `map02`: ZERO gated partition-2
///   records - all its logic is the `0x3F` door table + partition-1 scripts.
/// - **bubu1** (Buma) has NO field MAN payload (a streamed/special scene).
/// - **bubu2**: a small requires-all chain over `0x608`/`0x3D3`/`0x609`.
/// - **son**: sparse one-shots (`0x3A6`/`0x3A7`/`0x60D`); routes back to `taiku`.
/// - **deroa**: a `0x46D`/`0x46E`/`0x46F` one-shot group + `0x3E1` gate; routes
///   to the underground `chitei2`.
/// - **korb3** (Karisto castle approach) is the RICH one: a nine-record
///   **collection group** `P2[5..=13]`, every one C1=`[0x403]` (the "all done"
///   latch) and C2 a distinct flag `0x43E..=0x45C`, plus a `0x41x` requires-all
///   cluster (`P2[2]` C2=`[0x41B,0x41C,0x436]`); routes to `korb2`/`kor`.
#[test]
fn map03_karisto_region_gate_families() {
    use legaia_asset::man_section::{ManFile, parse as parse_man};
    use legaia_engine_core::man_field_scripts::partition2_record_gates;
    let Some(index) = open_index() else { return };
    let payload = |scene_name: &str| -> Option<(ManFile, Vec<u8>)> {
        let scene = Scene::load(&index, scene_name).ok()?;
        let man = scene.field_man_payload(&index).ok().flatten()?;
        let mf = parse_man(&man).ok()?;
        Some((mf, man))
    };
    let all_gates = |scene_name: &str| -> Vec<(Vec<u16>, Vec<u16>)> {
        let (mf, man) = payload(scene_name).expect("payload");
        let n_p2 = *mf.header.partition_counts.get(2).unwrap_or(&0) as usize;
        (0..n_p2)
            .map(|r| partition2_record_gates(&mf, &man, r).unwrap_or_default())
            .collect()
    };

    // map03: a pure router - no gated partition-2 records at all.
    let map03_gated = all_gates("map03")
        .iter()
        .filter(|(c1, c2)| !c1.is_empty() || !c2.is_empty())
        .count();
    assert_eq!(
        map03_gated, 0,
        "map03 hub has no gated P2 records (pure router)"
    );

    // bubu1 has no field MAN payload.
    assert!(payload("bubu1").is_none(), "bubu1 has no field MAN payload");

    // bubu2 requires-all chain tail; son / deroa one-shots.
    assert_eq!(all_gates("bubu2")[3], (vec![0x609], vec![0x608, 0x3D3]));
    assert_eq!(all_gates("son")[3], (vec![0x3A6], vec![]));
    assert_eq!(all_gates("deroa")[4], (vec![0x46D], vec![0x3E1]));

    // korb3: the nine-record 0x403 collection group + the 0x41x requires-all.
    let korb3 = all_gates("korb3");
    assert_eq!(korb3[2], (vec![0x41D], vec![0x41B, 0x41C, 0x436]));
    let mut collected = Vec::new();
    for (i, g) in korb3.iter().enumerate().take(14).skip(5) {
        assert_eq!(g.0, vec![0x403], "korb3 P2[{i}] C1 is the 0x403 latch");
        assert_eq!(g.1.len(), 1, "korb3 P2[{i}] C2 is a single collection flag");
        collected.push(g.1[0]);
    }
    collected.sort_unstable();
    assert_eq!(
        collected,
        vec![
            0x43E, 0x43F, 0x440, 0x441, 0x442, 0x443, 0x444, 0x459, 0x45C
        ],
        "korb3 P2[5..=13] are nine distinct 0x403-gated collection records"
    );
}

/// The `tunnela` `0x96..=0x9C` chain - a **seven-chest treasure set**, NOT a
/// story-spine gate family. A poll traversal caught all seven set in-scene in
/// strict order at distinct tiles; static mining resolves what they are.
///
/// tunnela carries ONE (bundle) MAN, no streaming variant, and its 12
/// partition-2 records hold **zero C1/C2 header gates** - so `0x96..=0x9C` are
/// not `FUN_8003BDE0` record gates. They live inline in seven byte-identical
/// partition-1 walk-on-trigger records (`P1[1..=7]`), one per flag, each a
/// treasure-chest: **TEST own flag (already opened?) -> give item + SET own
/// flag**. The records are laid out on a fixed `0xA2` stride with TEST->SET at
/// `+0x5D` (`0x96`: TEST @0x457 / SET @0x4B4; `0x9C`: TEST @0x823 / SET @0x880);
/// none carry a CLEAR (a chest never re-locks). The field-VM disassembler
/// desyncs into each record's chest dialogue after the opening TEST, so the
/// SET for `0x96`/`0x97`/`0x9A` is invisible to `system_flag_census` (which
/// reports it for the other four) - the byte-exact `50 XX` op is the robust
/// signal, and the poll confirmed all seven fire. The ending scene `edlast`
/// (`P2[1]`) TESTs all seven together in a collection/completion battery - the
/// only cross-scene consumer, and why these persist to the endgame.
///
/// This is the "dungeon-LOCAL puzzle/objective state" class (cf. taiku's
/// `0x505..=0x519`), the mining frontier the poll's `[lead]` markers surface.
#[test]
fn chapter2_tunnela_treasure_chest_flags() {
    let Some(index) = open_index() else { return };
    let man_of = |scene_name: &str| -> Vec<u8> {
        let scene = Scene::load(&index, scene_name).expect("load");
        scene
            .field_man_payload(&index)
            .expect("payload")
            .expect("MAN")
    };
    let count = |hay: &[u8], needle: [u8; 2]| hay.windows(2).filter(|w| *w == needle).count();

    // tunnela: each 0x96+k is SET (`50 XX`) and TEST (`70 XX`) exactly once,
    // never CLEARed - seven one-way treasure-chest flags.
    let tunnela = man_of("tunnela");
    for f in 0x96u8..=0x9C {
        assert_eq!(count(&tunnela, [0x50, f]), 1, "tunnela SET 0x{f:02X} once");
        assert_eq!(count(&tunnela, [0x70, f]), 1, "tunnela TEST 0x{f:02X} once");
        assert_eq!(
            count(&tunnela, [0x60, f]),
            0,
            "tunnela never CLEAR 0x{f:02X}"
        );
    }

    // edlast (the ending): TESTs all seven (the completion battery), sets none.
    let edlast = man_of("edlast");
    for f in 0x96u8..=0x9C {
        assert_eq!(count(&edlast, [0x70, f]), 1, "edlast TEST 0x{f:02X}");
        assert_eq!(count(&edlast, [0x50, f]), 0, "edlast never SET 0x{f:02X}");
    }

    // Structural: tunnela's partition-2 records carry NO header gates (this is
    // inline chest state, not a spine gate family).
    let mf = legaia_asset::man_section::parse(&tunnela).expect("parse");
    let n_p2 = *mf.header.partition_counts.get(2).unwrap_or(&0) as usize;
    for r in 0..n_p2 {
        let (c1, c2) =
            legaia_engine_core::man_field_scripts::partition2_record_gates(&mf, &tunnela, r)
                .unwrap_or_default();
        assert!(
            c1.is_empty() && c2.is_empty(),
            "tunnela P2[{r}] has no gates"
        );
    }
}

/// The `stone` `0x1D8..=0x1E8` cluster - the in-game **"Gate of Shadows"
/// symbol-code puzzle**, the richest dungeon-LOCAL objective family the poll
/// surfaced. Static mining decodes both the mechanic and the flag layout.
///
/// The puzzle (from the scene's own dialogue): a statue with a **row of four
/// symbols** you set via four directional **Keys** (North/South/East/West),
/// each key pressing one of four elemental **Symbols** (Wind/Fire/Water/Earth).
/// Enter the right four-symbol code to open the Gate of Shadows ("to visit the
/// past"); a wrong answer spawns a punishment battle.
///
/// Flag layout - `0x1D8..=0x1E7` = **16 state flags = 4 keys x 4 symbols**, in
/// four contiguous mutually-exclusive groups of four
/// (`0x1D8..=0x1DB` / `0x1DC..=0x1DF` / `0x1E0..=0x1E3` / `0x1E4..=0x1E7`).
/// Each flag is SET when pressed and CLEARed **six** times: pressing a symbol
/// batch-clears its key's whole group then sets the chosen one, so each group
/// always holds exactly one bit = "the symbol currently on this key".
///
/// The disc **encodes the solution**: the solved record contains a block of
/// four `51 XX` SET ops re-asserting exactly one symbol per key -
/// `{0x1D9, 0x1DE, 0x1E3, 0x1E4}` (so those four flags are SET twice; the
/// other twelve once). This is the four-symbol code, and the poll caught
/// exactly this as the solved resting state (+ `0x1E8`). `0x1E8` = the
/// **puzzle-SOLVED gate**: set by the
/// solved-cutscene record `P2[5]`, TESTed ~20x across stone's scene-entry
/// scripts (`P0[0..5]`, `P1[0]`) to gate the opened-gate behavior, and - the
/// only cross-scene consumer - TESTed once in `map02 P0[32]` (the Sebucus hub
/// reads whether the Gate is open). `0x492` is a shared region-progress flag
/// (SET in both stone `P1[0]` and `map02 P2[13]`), NOT part of the mechanic.
///
/// Not story spine - inline puzzle state (the taiku-`0x505` class), but far
/// richer, and with a real cross-scene payoff gate (`0x1E8` in `map02`).
#[test]
fn chapter2_stone_symbol_puzzle_flags() {
    let Some(index) = open_index() else { return };
    let man_of = |scene_name: &str| -> Vec<u8> {
        let scene = Scene::load(&index, scene_name).expect("load");
        scene
            .field_man_payload(&index)
            .expect("payload")
            .expect("MAN")
    };
    // Count a 2-byte field-VM flag op. The opcode carries the flag's high byte
    // in its low nibble (`base | (idx>>8)`), the next byte is `idx & 0xFF`.
    let count_op = |hay: &[u8], base: u8, idx: u16| -> usize {
        let op = [base | (idx >> 8) as u8, (idx & 0xFF) as u8];
        hay.windows(2).filter(|w| *w == op).count()
    };

    let stone = man_of("stone");
    // The 16 selector flags: each SET (once when pressed; the four SOLUTION
    // symbols a second time in the solved record) and CLEARed six times
    // (batch-reset = the four 4-way mutually-exclusive symbol groups).
    let mut solution: Vec<u16> = Vec::new();
    for f in 0x1D8u16..=0x1E7 {
        let sets = count_op(&stone, 0x50, f);
        assert!(sets >= 1, "stone SETs selector 0x{f:X}");
        assert_eq!(
            count_op(&stone, 0x60, f),
            6,
            "stone CLEAR 0x{f:X} x6 (batch)"
        );
        if sets == 2 {
            solution.push(f);
        }
    }
    // The disc ENCODES the answer: the solved record re-asserts exactly one
    // symbol per key group -> the four-symbol code. (Poll-confirmed: this is
    // the solved resting state the traversal reached.)
    assert_eq!(
        solution,
        vec![0x1D9, 0x1DE, 0x1E3, 0x1E4],
        "encoded solution: one symbol per key (A..D)"
    );
    // The SOLVED gate is a one-way completion flag, NOT a batch-cleared
    // selector: heavily tested, at most one clear.
    assert!(
        count_op(&stone, 0x50, 0x1E8) >= 1,
        "stone SETs 0x1E8 (solved)"
    );
    assert!(
        count_op(&stone, 0x60, 0x1E8) <= 1,
        "0x1E8 is not batch-cleared"
    );
    assert!(
        count_op(&stone, 0x70, 0x1E8) >= 10,
        "0x1E8 tested pervasively"
    );

    // Cross-scene: the Sebucus hub map02 reads the solved gate, shares 0x492,
    // and carries NONE of the 16 stone-local selector flags.
    let map02 = man_of("map02");
    assert!(
        count_op(&map02, 0x70, 0x1E8) >= 1,
        "map02 TESTs the solved gate"
    );
    assert!(count_op(&map02, 0x50, 0x492) >= 1, "map02 shares 0x492");
    for f in 0x1D8u16..=0x1E7 {
        assert_eq!(count_op(&map02, 0x50, f), 0, "0x{f:X} is stone-local");
    }
}

/// The `rayman` `0x1FD..=0x1FF` cluster - the **"three Haris" oracle
/// interaction hub**. Distinct from rayman's header-gate spine chain
/// (`0x201`/`0x1FB`/`0x200`/`0x1FC` off the `0x1EB`/`0x1EC` arc, covered by
/// `chapter2_dungeon_gate_families`): this is a self-contained **field-VM
/// AND-gate**, not a C1/C2 header chain.
///
/// The mechanic (from the scene's own dialogue): three "Hari" statues -
/// "from the left, teller of the past, of the present and of the future" -
/// that "remain awake only for a short time". Talking to each of the three
/// oracles sets one flag; combined gates then check that all three have been
/// consulted before advancing ("Hari has entrusted you with the future").
///
/// Flag layout (byte-exact off the decoded MAN):
/// - `0x1FD`/`0x1FE`/`0x1FF` = one flag per oracle. Each is SET **twice**:
///   once in its own per-actor NPC script (census: `P1[44]`/`P1[45]`/`P1[46]`,
///   three consecutive actor records) and once in a dev **reset arm** ("End
///   flag setting" / "Back=") that SETs all three consecutively
///   (`51 FF 51 FE 51 FD`) then CLEARs all three consecutively
///   (`61 FF 61 FE 61 FD`) - hence CLEAR exactly once each.
/// - All three are TESTed together in tight AND-gate clusters (census records
///   `P1[42]`/`P1[43]`/`P1[47]` + the completion cutscene `P2[18]`), the
///   "have you consulted all three oracles" check. Six TESTs per flag.
///
/// **Self-contained**: no genuine cross-scene consumer. The one out-of-scene
/// census hit (`geremi P1[22]` "CLEAR 0x1FE") is a disassembler-desync false
/// positive - the byte pair `61 FE` is consumed as the operand of a 3-byte
/// `0x26` op sitting inside Shift-JIS text (`26 61 FE 0D 83 4A ...`), not a
/// standalone CLEAR at an instruction boundary. Unlike `stone`'s `0x1E8`, this
/// hub has no cross-scene payoff gate.
#[test]
fn chapter2_rayman_three_haris_hub_flags() {
    let Some(index) = open_index() else { return };
    let man_of = |scene_name: &str| -> Vec<u8> {
        let scene = Scene::load(&index, scene_name).expect("load");
        scene
            .field_man_payload(&index)
            .expect("payload")
            .expect("MAN")
    };
    let count_op = |hay: &[u8], base: u8, idx: u16| -> usize {
        let op = [base | (idx >> 8) as u8, (idx & 0xFF) as u8];
        hay.windows(2).filter(|w| *w == op).count()
    };
    let count_sub = |hay: &[u8], sub: &[u8]| hay.windows(sub.len()).filter(|w| *w == sub).count();

    let rayman = man_of("rayman");

    // Each oracle flag: SET twice (own NPC script + the reset arm), CLEARed
    // exactly once (the reset arm), TESTed pervasively by the AND-gates.
    for f in 0x1FDu16..=0x1FF {
        assert_eq!(count_op(&rayman, 0x50, f), 2, "rayman SET 0x{f:X} twice");
        assert_eq!(count_op(&rayman, 0x60, f), 1, "rayman CLEAR 0x{f:X} once");
        assert!(
            count_op(&rayman, 0x70, f) >= 4,
            "rayman TESTs 0x{f:X} in the AND-gates"
        );
    }

    // The dev reset arm SETs all three consecutively then CLEARs all three
    // consecutively - one such block each.
    assert_eq!(
        count_sub(&rayman, &[0x51, 0xFF, 0x51, 0xFE, 0x51, 0xFD]),
        1,
        "reset arm SETs all three oracles"
    );
    assert_eq!(
        count_sub(&rayman, &[0x61, 0xFF, 0x61, 0xFE, 0x61, 0xFD]),
        1,
        "reset arm CLEARs all three oracles"
    );

    // The AND-gate: distinct sites where all three flags are TESTed together
    // (the "consulted all three oracles" conjunction). Count non-overlapping
    // 24-byte windows carrying a TEST of each of the three.
    let tf: [[u8; 2]; 3] = [[0x71, 0xFD], [0x71, 0xFE], [0x71, 0xFF]];
    let has = |w: &[u8], s: &[u8; 2]| w.windows(2).any(|x| x == *s);
    let mut clusters = 0usize;
    let mut o = 0usize;
    while o + 24 <= rayman.len() {
        let w = &rayman[o..o + 24];
        if tf.iter().all(|s| has(w, s)) {
            clusters += 1;
            o += 24;
        } else {
            o += 1;
        }
    }
    assert!(
        clusters >= 4,
        "at least four AND-gate sites, got {clusters}"
    );
}

/// `0x528` is NOT a story-progression flag - it is one bit of the
/// **`0x527..=0x52E` scratch-register bank** that field scripts bulk-reset.
/// The poll surfaced it as a churn=125 "[lead]", but it is scratch traffic.
///
/// The bank is reset as one contiguous eight-flag run
/// (`65 27 65 28 65 29 65 2A 65 2B 65 2C 65 2D 65 2E` = CLEAR `0x527..=0x52E`)
/// in ~70-90 scenes' scene-entry / per-actor setup - which is exactly the high
/// churn the poll sees on every scene transition.
///
/// CORRECTION (runtime-confirmed): an earlier static-only pass called `0x528`
/// "write-only to the VM" because the disc-wide opcode census reports ZERO
/// `0x70` TEST sites for it. **That was a census desync miss, not the truth.**
/// A read-watch capture (`autorun_flag_reader_watch.lua`, `LEGAIA_FLAG=0x528`,
/// balden) caught the field-VM TEST-opcode handler reading it 1951x in one idle
/// minute: `jal FUN_8003CE64` from `ra=0x801E35E8` (the op-`0x70` case; SET is
/// `ra=0x801E3598`, CLEAR `ra=0x801E35C0`). The consumer is balden's
/// scene-entry script, which TESTs `0x528` to guard its own scratch-bank reset
/// (`75 28 5D 00` immediately before the batch-clear run at balden MAN 0xAC54):
/// a per-scene init guard, not a story gate. The static census walker
/// desyncs past that `75 28` in balden's dialogue-heavy record, so the census
/// UNDERCOUNTS here: for `0x528` the census's zero is a floor, not the answer.
#[test]
fn flag_0x528_is_scratch_bank_tested_only_as_an_init_guard() {
    let Some(index) = open_index() else { return };
    let scenes = index.cdname_scene_names();
    let census = system_flag_census(&index, &scenes);
    let man_of = |name: &str| -> Vec<u8> {
        Scene::load(&index, name)
            .expect("load")
            .field_man_payload(&index)
            .expect("payload")
            .expect("MAN")
    };
    let kinds = |flag: u16| -> (usize, usize, usize) {
        let sites = census.get(&flag).map(Vec::as_slice).unwrap_or(&[]);
        let n = |k: FlagKind| sites.iter().filter(|h| h.kind == k).count();
        (n(FlagKind::Set), n(FlagKind::Clear), n(FlagKind::Test))
    };
    let count_sub = |hay: &[u8], sub: &[u8]| hay.windows(sub.len()).filter(|w| *w == sub).count();
    // Genuine field-VM TEST: the op followed by a small `[lo, 0x00]` branch word.
    let genuine_tests = |hay: &[u8], op: [u8; 2]| -> usize {
        (0..hay.len().saturating_sub(3))
            .filter(|&o| hay[o] == op[0] && hay[o + 1] == op[1] && hay[o + 3] == 0x00)
            .count()
    };

    // 0x528 is written both ways (its 64 SETs cluster in balden).
    let (set, clr, _tst) = kinds(0x528);
    assert!(set > 0 && clr > 0, "0x528 is written (SET+CLEAR)");

    // The census UNDERCOUNTS its TEST (reports zero via desync), but balden
    // carries a genuine `75 28` TEST - the reader the runtime capture confirmed
    // (field-VM op-0x70 handler, ra 0x801E35E8), gating the bank reset.
    assert_eq!(
        kinds(0x528).2,
        0,
        "census reports zero TEST (a desync floor)"
    );
    let balden = man_of("balden");
    assert!(
        genuine_tests(&balden, [0x75, 0x28]) >= 1,
        "but balden genuinely TESTs 0x528 (the census missed it)"
    );

    // The bank is live scratch, not dead: sibling bits are TESTed too.
    for sib in [0x527u16, 0x52C, 0x52E] {
        assert!(kinds(sib).2 > 0, "sibling 0x{sib:X} is VM-tested scratch");
    }

    // Structural: the eight-flag batch-clear run resets the whole bank at once
    // (town01 P1[0]); in balden the TEST guards exactly this run.
    let batch: [u8; 16] = [
        0x65, 0x27, 0x65, 0x28, 0x65, 0x29, 0x65, 0x2A, 0x65, 0x2B, 0x65, 0x2C, 0x65, 0x2D, 0x65,
        0x2E,
    ];
    assert!(
        count_sub(&man_of("town01"), &batch) >= 1,
        "town01 bulk-clears the 0x527..=0x52E scratch bank"
    );
    assert!(
        count_sub(&balden, &batch) >= 1,
        "balden also carries the bank-reset run (guarded by the 0x528 TEST)"
    );
}

/// The other two poll "high-churn" leads - `0x5A1` and `0x6C3` - are also
/// write-only cutscene toggles, NOT gates. Both are zero-TEST to the field VM.
///
/// - **`0x5A1` is half of a flip-flop pair with `0x5A0`**: the two are toggled
///   mutually exclusively (`CLEAR 0x5A0 ; SET 0x5A1` = `65 A0 55 A1`, and the
///   reverse `55 A0 65 A1`), a two-state binary variable held as two flags
///   inside rayman's Hari cutscene records (`P2[7]`/`P2[19]`). Confined to
///   `rayman`; engine-read display/sequence state, never an inline script test.
///
/// - **`0x6C3` is a stone SOLVED-cutscene toggle** (`P2[5]`), write-only.
///   Its census hit list names six scenes, but that is a DESYNC artifact: the
///   flag operand byte `0xC3` collides with a repeating `C3 CC nn` data table
///   in the town-family / rayman records, so `56 C3` matches across a table
///   record boundary. Filtering to genuine ops (a `56 C3` NOT adjacent to a
///   `0xCC`) leaves `stone` as the only real user. A sharper form of the
///   `0x528` note: even the opcode-boundary census's SET/CLEAR *attributions*
///   can be desync false positives when the operand byte aliases table data.
#[test]
fn flags_0x5a1_and_0x6c3_are_write_only_cutscene_toggles() {
    let Some(index) = open_index() else { return };
    let scenes = index.cdname_scene_names();
    let census = system_flag_census(&index, &scenes);
    let man_of = |name: &str| -> Vec<u8> {
        Scene::load(&index, name)
            .expect("load")
            .field_man_payload(&index)
            .expect("payload")
            .expect("MAN")
    };
    let test_count = |flag: u16| -> usize {
        census
            .get(&flag)
            .map(|s| s.iter().filter(|h| h.kind == FlagKind::Test).count())
            .unwrap_or(0)
    };
    let census_scenes = |flag: u16| -> BTreeSet<String> {
        census
            .get(&flag)
            .map(|s| s.iter().map(|h| h.scene_name.clone()).collect())
            .unwrap_or_default()
    };
    let count_sub = |hay: &[u8], sub: &[u8]| hay.windows(sub.len()).filter(|w| *w == sub).count();

    // 0x5A0 / 0x5A1: both write-only; 0x5A1 confined to rayman; the two toggle
    // as a mutually-exclusive flip-flop.
    assert_eq!(test_count(0x5A0), 0, "0x5A0 write-only");
    assert_eq!(test_count(0x5A1), 0, "0x5A1 write-only");
    assert_eq!(
        census_scenes(0x5A1),
        BTreeSet::from(["rayman".to_string()]),
        "0x5A1 lives only in rayman"
    );
    let rayman = man_of("rayman");
    assert!(
        count_sub(&rayman, &[0x65, 0xA0, 0x55, 0xA1]) > 0
            && count_sub(&rayman, &[0x55, 0xA0, 0x65, 0xA1]) > 0,
        "0x5A0/0x5A1 toggle as a flip-flop (both directions present)"
    );

    // 0x6C3: write-only, and genuinely stone-local despite the inflated census
    // scene list. A genuine op is a `56 C3` (SET 0x6C3) not adjacent to a 0xCC
    // (which would make it a `C3 CC nn` data-table boundary match).
    assert_eq!(test_count(0x6C3), 0, "0x6C3 write-only");
    let genuine_6c3 = |hay: &[u8]| -> usize {
        (0..hay.len().saturating_sub(1))
            .filter(|&o| hay[o] == 0x56 && hay[o + 1] == 0xC3)
            .filter(|&o| !(o >= 1 && hay[o - 1] == 0xCC || o + 2 < hay.len() && hay[o + 2] == 0xCC))
            .count()
    };
    assert!(
        genuine_6c3(&man_of("stone")) > 0,
        "0x6C3 real user is stone"
    );
    for other in ["town01", "town0b", "gameover_data"] {
        assert_eq!(
            genuine_6c3(&man_of(other)),
            0,
            "0x6C3 in {other} is a C3-CC data-table desync, not a real op"
        );
    }
}

/// Unlike the write-only churn flags, the last two poll leads - `0x32A` and
/// `0x590` - ARE real stone-LOCAL progress gates: both are SET and TESTed by
/// the field VM (a story gate is SET + TESTed; a scratch/toggle is
/// SET/CLEAR-many + zero-TEST). Checking for TEST sites first is what
/// separates them from `0x528`/`0x5A1`/`0x6C3`.
///
/// - **`0x32A` = the stone "code clue" marker**: SET (`53 2A`) at the
///   "Some sort of code is..." hint, TESTed three times in the SOLVED cutscene
///   (`P2[5]`) via the clean `73 2A 05 00 26` test-and-branch idiom. Distinct
///   from the 16 selector flags (`0x1D8..=0x1E7`) and the solved gate
///   (`0x1E8`). The census misses the SET (its walker desyncs in the
///   dialogue-heavy record) but decodes the three TESTs.
/// - **`0x590` = a stone scene-entry progress gate**: SET in a stone cutscene
///   (`P2[6]`), TESTed at scene-entry (`P1[0]`, right before the scratch-bank
///   clear) to branch opened-state behavior. Stone-LOCAL: the census's
///   `other7 P1[15]` "cross-scene TEST" is FALSIFIED - those `75 90` bytes sit
///   inside Shift-JIS dialogue (`75 90 B6 82 ...` = きてる), so the branch word
///   high byte is `0x82`, not `0x00`. A real field-VM TEST is followed by a
///   small `[lo, 0x00]` branch offset; that high-byte==0 test is the
///   real-op-vs-text-desync discriminator.
#[test]
fn stone_0x32a_and_0x590_are_real_local_progress_gates() {
    let Some(index) = open_index() else { return };
    let man_of = |name: &str| -> Vec<u8> {
        Scene::load(&index, name)
            .expect("load")
            .field_man_payload(&index)
            .expect("payload")
            .expect("MAN")
    };
    let count_sub = |hay: &[u8], sub: &[u8]| hay.windows(sub.len()).filter(|w| *w == sub).count();
    // A genuine field-VM TEST: the two-byte TEST op followed by a `[lo, 0x00]`
    // branch word (real jump offsets are small; text desync gives a high byte).
    let genuine_tests = |hay: &[u8], op: [u8; 2]| -> usize {
        (0..hay.len().saturating_sub(3))
            .filter(|&o| hay[o] == op[0] && hay[o + 1] == op[1] && hay[o + 3] == 0x00)
            .count()
    };

    let stone = man_of("stone");
    // 0x32A: real SET (53 2A) + real TESTs (73 2A .. 00) in stone.
    assert!(count_sub(&stone, &[0x53, 0x2A]) >= 1, "stone SETs 0x32A");
    assert!(
        genuine_tests(&stone, [0x73, 0x2A]) >= 3,
        "stone TESTs 0x32A in the solved cutscene"
    );
    // 0x590: real SET (55 90) + real TEST (75 90 .. 00) in stone.
    assert!(count_sub(&stone, &[0x55, 0x90]) >= 1, "stone SETs 0x590");
    assert!(
        genuine_tests(&stone, [0x75, 0x90]) >= 1,
        "stone TESTs 0x590 at scene entry"
    );

    // Cross-scene reader FALSIFIED: other7's census 0x590 TESTs are Shift-JIS
    // text - `75 90` occurs, but never as a genuine `[lo, 0x00]` branch.
    let other7 = man_of("other7");
    assert!(
        count_sub(&other7, &[0x75, 0x90]) > 0,
        "other7 has the 75 90 byte pair (the census hit)"
    );
    assert_eq!(
        genuine_tests(&other7, [0x75, 0x90]),
        0,
        "but none are real TESTs - 0x590 is stone-local, not cross-scene"
    );
}

/// Desync cross-check over every mined flag: the systemic guard the `0x528`
/// correction earned. [`system_flag_census`]'s opcode walker is NOT
/// authoritative for TEST in dialogue-heavy records - it desyncs into
/// Shift-JIS text and silently drops real ops. The walker-independent
/// [`flag_test_bytescan`] (the `[0x70|hi][lo][blo][0x00]` branch idiom)
/// cross-checks it. This test re-validates all seven poll minings against that
/// side channel; it FAILS if any flag we asserted "write-only" elsewhere turns
/// out to carry a hidden TEST reader (a second `0x528`).
///
/// Key results (see [`flag_test_bytescan`] for the noise-floor model):
/// - **Write-only is robust, not a desync floor**: `0x5A0` / `0x5A1` / `0x6C3`
///   have `raw == 0` - the TEST byte-pair is *absent disc-wide*, so no reader
///   can hide. This is the strongest possible confirmation of the cutscene-
///   toggle finding, far beyond "census TEST == 0".
/// - **`0x528` is the lone census-zero-but-real case** (already corrected +
///   firehose-confirmed): census TEST == 0 yet `genuine` sits ~67 vs a `0.4`
///   noise floor. The scratch bank `0x527..=0x52E` is where both static tools
///   disagree wildly - inherently runtime-only, consistent with scratch churn.
/// - **The real gates confirm**: the stone puzzle (`0x1D8..=0x1E8`), rayman
///   Haris hub (`0x1FD..=0x1FF`), and stone-local `0x32A` / `0x590` all show
///   `genuine` far above their (~0) noise floors; `0x1E8` even surfaces its
///   `map02` cross-scene reader.
#[test]
fn desync_crosscheck_revalidates_every_mined_flag() {
    let Some(index) = open_index() else { return };
    let scenes: Vec<String> = index.cdname_scene_names();
    let census = system_flag_census(&index, &scenes);

    // Cache every scene's concatenated MAN-carrier payloads once (a 0xFF
    // separator keeps a cross-carrier window from matching a spurious pair).
    let mut man_cache: Vec<(String, Vec<u8>)> = Vec::new();
    for name in &scenes {
        let Ok(scene) = Scene::load(&index, name) else {
            continue;
        };
        let mut buf: Vec<u8> = Vec::new();
        for c in scene_man_carriers(&index, &scene) {
            buf.extend_from_slice(&c.payload);
            buf.push(0xFF);
        }
        if !buf.is_empty() {
            man_cache.push((name.clone(), buf));
        }
    }
    assert!(!man_cache.is_empty(), "no MAN carriers decoded");

    // Disc-wide (raw, genuine) for a flag's TEST branch idiom.
    let bytescan = |flag: u16| -> (usize, usize) {
        man_cache.iter().fold((0, 0), |(r, g), (_, hay)| {
            let (dr, dg) = flag_test_bytescan(hay, flag);
            (r + dr, g + dg)
        })
    };
    let census_test = |flag: u16| -> usize {
        census
            .get(&flag)
            .map(|s| s.iter().filter(|h| h.kind == FlagKind::Test).count())
            .unwrap_or(0)
    };

    // (1) THE GUARD: every flag asserted "write-only" elsewhere must have a
    // truly absent TEST byte-pair disc-wide (raw == 0). A future mining that
    // mislabels a real gate as write-only trips this.
    for wo in [0x5A0u16, 0x5A1, 0x6C3] {
        let (raw, genuine) = bytescan(wo);
        assert_eq!(
            (raw, genuine),
            (0, 0),
            "0x{wo:X} claimed write-only but its TEST byte-pair appears disc-wide (possible hidden reader)"
        );
    }

    // (2) 0x528: the census reports zero TEST (a desync floor), but the
    // byte-scan finds the reader far above the noise floor. Locks the
    // correction + the "census not authoritative" lesson.
    assert_eq!(census_test(0x528), 0, "0x528 census TEST is a desync floor");
    let (raw_528, gen_528) = bytescan(0x528);
    assert!(
        gen_528 >= 10 && gen_528 as f64 > (raw_528 as f64 / 256.0) * 6.0,
        "0x528 carries a real TEST reader the census missed (genuine {gen_528} vs raw {raw_528})"
    );

    // (3) The real gates: genuine TEST far above a ~0 noise floor. Each has a
    // tiny raw pool (rare op-pair), so the branch-idiom count is decisive.
    let real = |flag: u16, min: usize| {
        let (raw, genuine) = bytescan(flag);
        assert!(
            genuine >= min && genuine as f64 > (raw as f64 / 256.0) * 3.0 + 0.5,
            "0x{flag:X} is a real TEST gate (genuine {genuine} >= {min}, raw {raw})"
        );
    };
    for chest in 0x96u16..=0x9C {
        real(chest, 2); // tunnela treasure-chest gates
    }
    for sym in 0x1D8u16..=0x1E8 {
        real(sym, 4); // stone symbol-puzzle selectors + solved gate
    }
    for hari in [0x1FDu16, 0x1FE, 0x1FF] {
        real(hari, 4); // rayman three-Haris hub
    }
    real(0x32A, 3); // stone code-clue marker
    real(0x590, 1); // stone scene-entry progress gate

    // 0x1E8 (stone SOLVED) is read cross-scene in map02 - the byte-scan sees
    // it even though it is a different scene than the setter.
    let (_, gen_1e8) = bytescan(0x1E8);
    assert!(gen_1e8 >= 15, "0x1E8 solved gate is heavily TESTed");
}
