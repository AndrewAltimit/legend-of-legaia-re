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

use legaia_engine_core::man_field_scripts::{scene_man_carriers, system_flag_census};
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
