//! Chapter-3 gate families and writer pins (Karisto castle, Koin, Conkram).

use super::*;

/// The Karisto-castle deep-spoke gate families (`kor`/`kor3`/`kor4`/`kor5`/
/// `korout`, the `koin*` cluster, `chitei2`) - disc-static, mined with the
/// same C1/C2 record-header method as the ch2 sweep. Reached via `korb3`
/// (routes to `korb2`/`kor`); `chitei2` is entered from `deroa` (whose
/// `P2[4]` C2-gates on `0x3E1`, set in Conkram - see the conc test below).
///
/// Structure:
/// - **kor**: three one-shot cutscene beats - `P2[8]` (ungated) SETs `0x408`
///   (read by `korout P2[3]` C1), `P2[9]`/`P2[10]` self-latch `0x409`/`0x40A`
///   (each C1 = its own flag) - plus a four-record **door group**
///   `P2[13..=16]` C2=`[0x612]`. The `0x612` arm flag is SET by the
///   partition-0 entry scripts (`P0[7..=10]`, a SceneFade + `56 12` run,
///   hand-disasm-verified) and cleared back by the door records - the
///   arm-then-consume door mechanic. `kor3` (`P2[7..=8]`) and `kor4`
///   (`P2[2..=3]`) gate their doors on the SAME `0x612`.
/// - **kor3**: `P2[14]` C1=`[0x426]` self-latch.
/// - **kor5**: a three-step chain - `P2[3]` C1=`[0x43A]` self-latch ->
///   `P2[4]` C1=`[0x436]` C2=`[0x43A]` -> `P2[5]` C1=`[0x436]` self-latch ->
///   `P2[8]` C1=`[0x6C4]` C2=`[0x436]` self-latch.
/// - **korb2** / **koin2** / **koin6**: gateless (pure geometry/router
///   scenes).
/// - **koin4**: `P2[3]` C1=`[0x5D6]` - `0x5D6` has NO script writer
///   disc-wide (only koin4 P1 TESTs), a code-path lead like `549`.
/// - **chitei2**: `P2[11]` C1=`[0x470]` C2=`[0x4F0]`, `P2[13]` C1=`[0x4C4]`
///   self-latch, `P2[17]` C1=`[0x4C9]` C2=`[0x4C6]` self-latch, and a
///   `0x4C8` family (`P2[14]`/`P2[15]`/`P2[19]` C1) whose SET has TWO clean
///   writers: `chitei2 P1[0]` and `map03 P2[19]` (the hub co-writes the
///   underground beat).
#[test]
fn chapter3_karisto_castle_gate_families() {
    use legaia_engine_field::man_field_scripts::partition2_record_gates;
    use legaia_scene_formats::man_section::parse as parse_man;
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

    // kor: one-shot latches + the 0x612 door group. (P2[8] is the ungated
    // 0x408 writer beat; only P2[9]/P2[10] self-latch.)
    let kor = all_gates("kor");
    assert_eq!(kor.len(), 24, "kor has 24 partition-2 records");
    assert_eq!(kor[8], (vec![], vec![]));
    assert_eq!(kor[9], (vec![0x409], vec![]));
    assert_eq!(kor[10], (vec![0x40A], vec![]));
    for (i, g) in kor.iter().enumerate().take(17).skip(13) {
        assert_eq!(g, &(vec![], vec![0x612]), "kor P2[{i}] is a 0x612 door");
    }

    // korout gates its P2[3] on kor's 0x408 beat (C1: plays only pre-beat).
    assert_eq!(all_gates("korout")[3], (vec![0x408], vec![]));

    // kor3 + kor4 share kor's 0x612 door-arm flag.
    let kor3 = all_gates("kor3");
    assert_eq!(kor3[7], (vec![], vec![0x612]));
    assert_eq!(kor3[8], (vec![], vec![0x612]));
    assert_eq!(kor3[14], (vec![0x426], vec![]));
    let kor4 = all_gates("kor4");
    assert_eq!(kor4[2], (vec![], vec![0x612]));
    assert_eq!(kor4[3], (vec![], vec![0x612]));

    // kor5: the 0x43A -> 0x436 -> 0x6C4 chain.
    let kor5 = all_gates("kor5");
    assert_eq!(kor5[3], (vec![0x43A], vec![]));
    assert_eq!(kor5[4], (vec![0x436], vec![0x43A]));
    assert_eq!(kor5[5], (vec![0x436], vec![]));
    assert_eq!(kor5[8], (vec![0x6C4], vec![0x436]));

    // Gateless spokes.
    for scene in ["korb2", "koin2", "koin6"] {
        let gated = all_gates(scene)
            .iter()
            .filter(|(c1, c2)| !c1.is_empty() || !c2.is_empty())
            .count();
        assert_eq!(gated, 0, "{scene} has no gated P2 records");
    }

    // koin4: the 0x5D6 gate (writer-less disc-wide; code-path lead).
    assert_eq!(all_gates("koin4")[3], (vec![0x5D6], vec![]));

    // chitei2: the underground families.
    let chitei2 = all_gates("chitei2");
    assert_eq!(chitei2[11], (vec![0x470], vec![0x4F0]));
    assert_eq!(chitei2[13], (vec![0x4C4], vec![]));
    assert_eq!(chitei2[17], (vec![0x4C9], vec![0x4C6]));
    for i in [14usize, 15, 19] {
        assert_eq!(
            chitei2[i].0,
            vec![0x4C8],
            "chitei2 P2[{i}] C1-gates on 0x4C8"
        );
    }
}

/// The `koin1`/`koin1b` pair + `koin3`, and the census writer pins for the
/// castle families. `koin1b` is `koin1`'s story-state sibling: the same
/// record shape with a `0x00B`-toggled pair spliced in at `P2[11..=12]`,
/// and it owns the `0x3DA` SET (`P2[6]`) that koin1's `P2[6]` C1-gates on.
///
/// koin1's `P2[9]`/`P2[10]` are a `0x50A` **toggle pair** (C2=[0x50A] /
/// C1=[0x50A]: one spawns while the flag is set, the other while clear) -
/// `0x50A` has NO script writer disc-wide (one koin3 P1 TEST aside), a
/// code-path lead like `549`/`0x5D6`/`0x482`.
#[test]
fn chapter3_koin_family_and_writer_pins() {
    use legaia_engine_field::man_field_scripts::partition2_record_gates;
    use legaia_scene_formats::man_section::parse as parse_man;
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

    let koin1 = all_gates("koin1");
    assert_eq!(koin1.len(), 13);
    assert_eq!(koin1[0], (vec![0x3AC], vec![]));
    assert_eq!(koin1[6], (vec![0x3DA], vec![]));
    assert_eq!(koin1[8], (vec![0x4BA], vec![]));
    assert_eq!(
        koin1[9],
        (vec![], vec![0x50A]),
        "koin1 P2[9] plays while 0x50A set"
    );
    assert_eq!(
        koin1[10],
        (vec![0x50A], vec![]),
        "koin1 P2[10] plays while 0x50A clear"
    );

    let koin1b = all_gates("koin1b");
    assert_eq!(koin1b.len(), 14, "koin1b = koin1 + the spliced 0x00B pair");
    assert_eq!(
        koin1b[..11],
        koin1[..11],
        "koin1b's first 11 gates mirror koin1"
    );
    assert_eq!(koin1b[11], (vec![0xB], vec![]));
    assert_eq!(koin1b[12], (vec![], vec![0xB]));

    let koin3 = all_gates("koin3");
    assert_eq!(koin3[2], (vec![0x431], vec![]));
    assert_eq!(koin3[4], (vec![0x4CA], vec![]));

    // Census writer pins: each latch flag's SET is the gated record itself
    // (the standard self-latching one-shot), decoding CLEAN.
    let scenes = index.cdname_scene_names();
    let census = system_flag_census(&index, &scenes);
    let clean_set = |flag: u16, scene: &str, partition: usize, record: usize| -> bool {
        census.get(&flag).is_some_and(|hits| {
            hits.iter().any(|h| {
                h.scene_name == scene
                    && h.partition == partition
                    && h.record == record
                    && h.kind == FlagKind::Set
                    && h.clean
            })
        })
    };
    for (flag, scene, partition, record) in [
        (0x408u16, "kor", 2usize, 8usize),
        (0x409, "kor", 2, 9),
        (0x40A, "kor", 2, 10),
        (0x426, "kor3", 2, 14),
        (0x43A, "kor5", 2, 3),
        (0x436, "kor5", 2, 5),
        (0x6C4, "kor5", 2, 8),
        (0x3AC, "koin1", 2, 0),
        (0x4BA, "koin1", 2, 8),
        (0x3DA, "koin1b", 2, 6),
        (0x431, "koin3", 2, 7),
        (0x4CA, "koin3", 2, 4),
        (0x4C4, "chitei2", 2, 13),
        (0x4C9, "chitei2", 2, 17),
        (0x4C8, "chitei2", 1, 0),
        (0x4C8, "map03", 2, 19),
        (0x470, "chitei2", 2, 11),
    ] {
        assert!(
            clean_set(flag, scene, partition, record),
            "expected a clean SET of 0x{flag:03X} at {scene} P{partition}[{record}]"
        );
    }
    // `0x50A` stays writer-less in script space (a code-path lead, like
    // 549): its writers are the mode-24 minigame overlays' native code.
    let clean_sets_50a = census
        .get(&0x50Au16)
        .map(|hits| {
            hits.iter()
                .filter(|h| h.kind == FlagKind::Set && h.clean)
                .count()
        })
        .unwrap_or(0);
    assert_eq!(
        clean_sets_50a, 0,
        "0x50A has no clean script writer (code-path lead)"
    );
    // `0x5D6` DOES have one, and it is a self-latch: `koin4` P1[15] is both
    // the record the flag gates (its dialog/position variant) and the record
    // that sets it. Behind the "If you have money, go inside / and buy
    // something." line the walk reads `48` (a one-byte no-op) then `55 D6`,
    // and the ops after it are a coherent cross-context actor run - the same
    // shape as the town01 P2[3] and flag-549 self-latches. P2[3] carries the
    // same block. The earlier "no writer in any enumerable space" reading
    // was a walk that stopped at the record's first text segment.
    let sets_5d6: BTreeSet<(String, usize, usize)> = census
        .get(&0x5D6u16)
        .expect("0x5D6 sites")
        .iter()
        .filter(|h| h.kind == FlagKind::Set && h.clean && !h.text_alias && !h.debug_menu)
        .map(|h| (h.scene_name.clone(), h.partition, h.record))
        .collect();
    assert_eq!(
        sets_5d6,
        BTreeSet::from([("koin4".to_string(), 1, 15), ("koin4".to_string(), 2, 3)]),
        "0x5D6's script writers are the koin4 self-latch pair"
    );
}

/// The Conkram (`conc*`) gate families - the chapter-3 "past" arc around
/// `0x3E1`/`0x3E5`. `conc2` `P2[12]` SETs `0x3E1` (the flag `deroa P2[4]`
/// C2-gates the `chitei2` descent on - the cross-region bridge); `conc3`
/// `P2[10]` self-latches `0x3E5` (site verified by hand: coherent channel
/// ops after an embedded ASCII string) and its ungated `P2[9]` SETs
/// `0x3F9`; `conc P2[10]` chains C1=`[0x3E5]` C2=`[0x3F9]`.
///
/// Shared shapes:
/// - **soldier groups**: in `conc` and `concnow` every `r1..rN` soldier
///   record C1-gates on the low system flag `0x007` (SET clean by
///   `concnow P0[34]` + `conc2 P0[21]` - a "soldiers disperse" beat).
/// - **door group**: `conc`'s C2=`[0x6DE]` records - the same
///   arm-then-consume mechanic as kor's `0x612` (armed by the P1[0] entry
///   script's player-position BBoxTest run, hand-disasm-verified `56 DE`).
/// - **concnow**: a one-shot ladder over `0x3ED`/`0x3EE`/`0x3D2`/`0x3CE`/
///   `0x3E1`/`0x423`.
/// - **conc2**: the `0x3E1`+`0x3E5` C1 family (pre-beat records that stop
///   spawning once the past-arc flags land).
/// - **concend**: a single ungated epilogue record.
#[test]
fn chapter3_conkram_gate_families() {
    use legaia_engine_field::man_field_scripts::partition2_record_gates;
    use legaia_scene_formats::man_section::parse as parse_man;
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

    // conc: the 0x007 soldier group + the 0x3E5/0x3F9 chain record + the
    // 0x6DE door group.
    let conc = all_gates("conc");
    assert_eq!(conc.len(), 35);
    for (i, g) in conc.iter().enumerate().take(9).skip(1) {
        assert_eq!(
            g,
            &(vec![0x7], vec![]),
            "conc P2[{i}] soldier C1-gates on 0x007"
        );
    }
    assert_eq!(conc[10], (vec![0x3E5], vec![0x3F9]));
    let conc_doors = conc.iter().filter(|(_, c2)| c2 == &vec![0x6DE]).count();
    assert_eq!(
        conc_doors, 11,
        "conc carries eleven 0x6DE-armed door records"
    );

    // concnow: soldier group + the one-shot ladder.
    let concnow = all_gates("concnow");
    for (i, g) in concnow.iter().enumerate().take(11).skip(3) {
        assert_eq!(g, &(vec![0x7], vec![]), "concnow P2[{i}] soldier");
    }
    assert_eq!(concnow[13], (vec![0x3ED], vec![]));
    assert_eq!(concnow[16], (vec![0x3CE], vec![]));
    assert_eq!(concnow[17], (vec![0x3E1], vec![0x3CE]));

    // conc2: the 0x3E1+0x3E5 pre-beat family.
    let conc2 = all_gates("conc2");
    assert_eq!(conc2[10], (vec![0x425], vec![0x3E5]));
    // Two shapes: both flags in C1 (pre-beat one-shots), and C1=[0x3E1]
    // C2=[0x3E5] (records that need the 0x3E5 beat but not the 0x3E1 one).
    let both_c1 = conc2
        .iter()
        .filter(|(c1, _)| c1.contains(&0x3E1) && c1.contains(&0x3E5))
        .count();
    let c1_c2 = conc2
        .iter()
        .filter(|(c1, c2)| c1 == &vec![0x3E1] && c2 == &vec![0x3E5])
        .count();
    assert_eq!(both_c1, 3, "conc2 0x3E1+0x3E5 both-C1 pre-beat records");
    assert_eq!(c1_c2, 9, "conc2 C1=[0x3E1] C2=[0x3E5] records");

    // conc3: the past-arc writers - P2[9] (ungated) SETs 0x3F9, P2[10]
    // self-latches 0x3E5.
    let conc3 = all_gates("conc3");
    assert_eq!(conc3[9], (vec![], vec![]));
    assert_eq!(conc3[10], (vec![0x3E5], vec![]));

    // concend: single ungated epilogue record.
    let concend = all_gates("concend");
    assert_eq!(concend.len(), 1);
    assert_eq!(concend[0], (vec![], vec![]));

    // Census writer pins (clean SETs): the cross-region 0x3E1 bridge writer
    // + the conc3 0x3F9 self-latch + the 0x007 soldier-disperse writers.
    let scenes = index.cdname_scene_names();
    let census = system_flag_census(&index, &scenes);
    let clean_set = |flag: u16, scene: &str, partition: usize, record: usize| -> bool {
        census.get(&flag).is_some_and(|hits| {
            hits.iter().any(|h| {
                h.scene_name == scene
                    && h.partition == partition
                    && h.record == record
                    && h.kind == FlagKind::Set
                    && h.clean
            })
        })
    };
    assert!(clean_set(0x3E1, "conc2", 2, 12), "0x3E1 bridge writer");
    assert!(clean_set(0x3F9, "conc3", 2, 9), "0x3F9 self-latch writer");
    assert!(
        clean_set(0x7, "concnow", 0, 34),
        "0x007 disperse writer (now)"
    );
    assert!(
        clean_set(0x7, "conc2", 0, 21),
        "0x007 disperse writer (past)"
    );
}

/// The two koin gates, one writer-less and one not. `0x50A` holds clean
/// TESTs only (`koin3` P2[9]/P2[10]) and no SET/CLEAR anywhere: its writers
/// are the mode-24 minigame overlays' native code, the capture-target class.
/// `0x5D6` reads the same way until the walk crosses the record's text -
/// behind it `koin4` P1[15] (and its P2[3] twin) SET the very flag P1[15]
/// gates on, the self-latch shape. Both are pinned below; a change on either
/// side wants re-verifying (the 549 / `0x370` pattern) before it is trusted.
#[test]
fn koin_gates_0x50a_writer_less_0x5d6_self_latched() {
    let Some(index) = open_index() else { return };
    let scenes = index.cdname_scene_names();
    let census = system_flag_census(&index, &scenes);

    for (flag, scene, sites_want, writers_want) in [
        (
            0x50Au16,
            "koin3",
            vec![(2usize, 9usize), (2, 10)],
            Vec::<(String, usize, usize)>::new(),
        ),
        (
            0x5D6,
            "koin4",
            vec![(1, 15), (1, 15)],
            vec![
                ("koin4".to_string(), 1usize, 15usize),
                ("koin4".to_string(), 2, 3),
            ],
        ),
    ] {
        let sites = census.get(&flag).expect("gate sites");
        let writers: Vec<_> = sites
            .iter()
            .filter(|h| h.kind != FlagKind::Test && h.clean && !h.text_alias && !h.debug_menu)
            .map(|h| (h.scene_name.clone(), h.partition, h.record))
            .collect();
        assert_eq!(
            writers, writers_want,
            "0x{flag:X} script writers changed - re-verify"
        );
        let tests: Vec<(usize, usize)> = sites
            .iter()
            .filter(|h| {
                h.scene_name == scene && h.kind == FlagKind::Test && h.clean && !h.text_alias
            })
            .map(|h| (h.partition, h.record))
            .collect();
        assert_eq!(tests, sites_want, "0x{flag:X} clean TEST sites in {scene}");
    }
}
