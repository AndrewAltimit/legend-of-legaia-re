//! Chapter-1 spine flag writers and the self-latching records that carry them.

use super::*;

/// The carrier-complete census surfaces the chapter-1 spine writers.
#[test]
fn spine_flag_writers_surface_in_the_carrier_census() {
    let Some(index) = open_index() else { return };
    let scenes = index.cdname_scene_names();
    let census = system_flag_census(&index, &scenes);

    // 0x142: the story writers are the `dolk2` carrier's entry re-assert
    // (P1[0]) and the `rikuroa` carrier's post-victory latch (P2[50] - the
    // C1 self-latch the firehose caught live, `51 42`, `ra 0x801E3598`).
    //
    // Everything else the census holds for this flag is a **developer
    // flag-menu arm** (`man_field_scripts::debug_flag_menu_arm`): a picker
    // option whose body is a flag-op run ending in an unconditional
    // `JmpRel` back to the picker, two or more side by side. Those decode
    // cleanly and their operands are real, so `clean` and `text_alias` both
    // pass them - the arm block is what separates them. `map01` P1[2] names
    // itself (`Clear all flags`), `rikuroa` P1[10..12] are `Clear`-labelled
    // nine-flag Set/Clear ladders, and the `keikoku` / `suimon` / `town0b` /
    // `town0c` / `dolk2` P1[1] arms are the `On` / `Off` / `=Back=` toggle.
    // See `docs/subsystems/script-vm.md`
    // "Shipped scene scripts carry developer flag-setting menus".
    let s142: BTreeSet<(String, bool, usize, usize)> = census
        .get(&0x142)
        .expect("0x142 sites")
        .iter()
        .filter(|h| h.kind == FlagKind::Set && !h.debug_menu)
        .map(|h| (h.scene_name.clone(), h.variant, h.partition, h.record))
        .collect();
    assert_eq!(
        s142,
        BTreeSet::from([
            ("dolk2".to_string(), true, 1, 0),
            ("rikuroa".to_string(), true, 2, 50),
        ]),
        "0x142 story SET sites (both in streaming variant carriers)"
    );
    // Non-vacuous the other way: the menu arms are still surfaced, and they
    // are the scenes below (`cave01` contributes a TEST arm rather than a
    // write). If this set empties, the classifier stopped firing and the pin
    // above stopped meaning anything.
    let menu142: BTreeSet<String> = census
        .get(&0x142)
        .expect("0x142 sites")
        .iter()
        .filter(|h| h.debug_menu)
        .map(|h| h.scene_name.clone())
        .collect();
    assert_eq!(
        menu142,
        BTreeSet::from([
            "cave01".to_string(),
            "dolk".to_string(),
            "dolk2".to_string(),
            "keikoku".to_string(),
            "map01".to_string(),
            "rikuroa".to_string(),
            "suimon".to_string(),
            "town0b".to_string(),
            "town0c".to_string(),
        ]),
        "scenes whose 0x142 sites are developer flag-menu arms"
    );

    // 0x482 (Drake mist walls): every census site is desync NOISE - the
    // earlier "SET by the other7 script pool P1[15]/P1[39], CLEARed by the
    // edbalden/eddoman epilogue carriers" reading is FALSIFIED by hand
    // disasm of each site class:
    //   - the other7 "SETs" are the full-width SJIS digit run `82 54 82 4F`
    //     (e.g. the text ５００) read one byte off as `54 82` = SET 0x482;
    //   - the edbalden/eddoman "CLEARs" are a repeating full-width label
    //     table (`82 64 82 77 82 68 82 73` = ＥＸＩＴ + counter), 17 aliases
    //     of `64 82` per carrier;
    //   - the edbylon "TEST" is SJIS dialogue (`74 82` with a garbage branch
    //     target 0x8E51).
    // (other7 itself is a dev-leftover earlier revision of koin3's MAN - its
    // 8 partition-2 records are a name-exact prefix subset of koin3's 12 -
    // with no loader anywhere in the captured code.) The motion-VM and
    // op-0x49 censuses carry no 0x482 site either, and every poll-captured
    // 0x482 set sits in a bulk save-load tick, so the flag's writer is
    // presumed a direct code path - a capture target (P7 write-watch around
    // the post-Zeto Drake-revival beat), not a script hunt. (NB flag 549's
    // identical-sounding verdict was since overturned by the `4C ED` width
    // fix - see `flag_549_writer_is_the_rim_elm_p2_3_self_latch` - but 0x482
    // stays all-noise even under the fixed widths: the assert below is the
    // living check.)
    let s482 = census.get(&0x482).expect("0x482 sites");
    assert!(
        s482.iter().all(|h| !h.clean),
        "every 0x482 census site is decode-desynced text noise; a CLEAN site \
         appearing here means a genuine script writer surfaced - re-open the \
         static hunt: {s482:?}"
    );
    // Pin the two site classes the falsified reading was built on, so the
    // census still *surfaces* them (as flagged noise) rather than silently
    // dropping them if the walker changes.
    for (scene, partition, record) in [("other7", 1usize, 15usize), ("other7", 1, 39)] {
        assert!(
            s482.iter().any(|h| h.scene_name == scene
                && h.partition == partition
                && h.record == record
                && h.kind == FlagKind::Set
                && !h.clean),
            "expected the {scene} P{partition}[{record}] phantom-SET alias to \
             surface as non-clean noise"
        );
    }
    assert!(
        s482.iter().any(|h| h.scene_name == "edbalden"
            && h.variant
            && h.kind == FlagKind::Clear
            && !h.clean),
        "expected the edbalden EXIT-label aliases to surface as non-clean noise"
    );

    // The live-confirmed 0x142 writer arms decode CLEAN (the P2[50]
    // post-victory one-shot and the dolk2 P1[0] re-assert), separating them
    // from the 0x482 mirage class. (The rikuroa P1[10..=12] debug-ladder
    // SETs sit a few instructions after inline ASCII labels - "Dummy" /
    // "Clear" - so they stay under the resync window and keep the
    // verify-by-hand tag; hand disasm confirms real `51 42` ladder ops.)
    assert!(
        census
            .get(&0x142)
            .unwrap()
            .iter()
            .any(|h| h.scene_name == "rikuroa"
                && h.partition == 2
                && h.record == 50
                && h.kind == FlagKind::Set
                && h.clean),
        "rikuroa P2[50] 0x142 SET must decode clean"
    );

    // 0x1BE: geremi's arrival one-shot (P2[0] self-latch) - the flag the
    // earlier misattributed frame read as a rikuroa/Zeto gate. `geremi`
    // P1[1] carries the op too, as a developer flag-menu arm.
    let s1be: BTreeSet<(String, usize, usize, bool)> = census
        .get(&0x1BE)
        .expect("0x1BE sites")
        .iter()
        .filter(|h| h.kind == FlagKind::Set && !h.debug_menu)
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
    use legaia_engine_field::man_field_scripts::partition2_record_gates;
    let Some(index) = open_index() else { return };
    let scene = Scene::load(&index, "rikuroa").expect("load rikuroa");
    let man = scene
        .field_man_payload(&index)
        .expect("payload")
        .expect("rikuroa MAN resolves");
    let mf = legaia_scene_formats::man_section::parse(&man).expect("parse");
    assert_eq!(mf.header.partition_counts, [13, 29, 64]);
    let (c1, c2) = partition2_record_gates(&mf, &man, 50).expect("P2[50] gates");
    assert_eq!(
        c1,
        vec![0x142],
        "P2[50] C1 blocks respawn once 0x142 is set"
    );
    assert!(c2.is_empty(), "P2[50] has no requires-all gate");
}

/// `rikuroa`'s post-Caruban record `P2[50]` starts no track itself: the entry
/// arm that spawns it (`P1[0]`, `44 5C` behind the `0x289` test) starts the
/// one it plays over. The spawn-score walk finds that arm's start.
#[test]
fn rikuroa_p2_50_is_scored_by_its_spawning_arm() {
    use legaia_engine_field::man_field_scripts::walk_spawn_scores;
    let Some(index) = open_index() else { return };
    let scene = Scene::load(&index, "rikuroa").expect("load rikuroa");
    let man = scene
        .field_man_payload(&index)
        .expect("payload")
        .expect("rikuroa MAN resolves");
    let mf = legaia_scene_formats::man_section::parse(&man).expect("parse");
    let scores = walk_spawn_scores(&mf, &man);
    let s = scores
        .iter()
        .find(|s| s.global_index == 0x5C)
        .expect("a scored spawn of global record 0x5C = P2[50]");
    assert_eq!(
        (s.partition, s.record),
        (1, 0),
        "spawned by the entry script"
    );
    assert!(
        matches!(s.words.first(), Some(&(2025, 1 | 9))),
        "the arm starts track 2025: {:?}",
        s.words
    );
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
    use legaia_engine_field::man_field_scripts::partition2_record_gates;
    let Some(index) = open_index() else { return };
    let scene = Scene::load(&index, "geremi").expect("load geremi");
    let man = scene
        .field_man_payload(&index)
        .expect("payload")
        .expect("geremi MAN resolves");
    let mf = legaia_scene_formats::man_section::parse(&man).expect("parse");
    assert_eq!(mf.header.partition_counts, [18, 70, 20]);
    let (c1, c2) = partition2_record_gates(&mf, &man, 0).expect("P2[0] gates");
    assert_eq!(
        c1,
        vec![0x1BE],
        "P2[0] C1 blocks the arrival cutscene once 0x1BE is set"
    );
    assert!(c2.is_empty(), "P2[0] has no requires-all gate");
}

/// Flag `0x370` (the Nivora-side successor gate): the "writer-less after
/// alias rejection - capture-only" verdict is FALSIFIED by the full-nibble
/// `0x4C` width pinning, the same wave that resolved flag 549. The census
/// surfaces exactly ONE genuine (clean-decode, non-text-alias) script SET
/// disc-wide: `doman`'s variant MAN (PROT 0401) `P1[15]` at MAN offset
/// `0x06397` - `53 70` at a coherent opcode boundary inside a real
/// choreography run (`CFlag.Test` / `CamCfg` / `WaitFrames` / `MoveTo` /
/// `Effect AnimTrigger` / `4C CD`), followed by a `JmpRel` back to the
/// record's gate-test head and the Dr. Usha "Do you understand? The first
/// TimeSpace..." dialogue segment - the research-center briefing latch.
/// The prose sites ("Time**Sp**ace Bomb" = `53 70`) stay rejected as
/// text aliases. The doman NPC records `P1[3..=18]` carry the clean head
/// TESTs of the same flag (the arc-gate dispatch chain, non-printable jump
/// operands = alias-immune).
#[test]
fn flag_0x370_writer_is_the_doman_p1_15_usha_latch() {
    let Some(index) = open_index() else { return };
    let scenes = index.cdname_scene_names();
    let census = system_flag_census(&index, &scenes);

    let sites = census.get(&0x370).expect("0x370 sites");
    let sets: BTreeSet<(String, bool, usize, usize)> = sites
        .iter()
        .filter(|h| h.kind == FlagKind::Set && h.clean && !h.text_alias && !h.debug_menu)
        .map(|h| (h.scene_name.clone(), h.variant, h.partition, h.record))
        .collect();
    assert_eq!(
        sets,
        BTreeSet::from([("doman".to_string(), true, 1, 15)]),
        "0x370's one genuine script SET = doman variant P1[15] (the Usha \
         briefing latch); anything else appearing here is a new surfaced \
         writer - re-verify before trusting"
    );

    // The reader family: clean head TESTs across doman's NPC records (the
    // per-record arc-gate dispatch chain).
    let doman_tests: BTreeSet<usize> = sites
        .iter()
        .filter(|h| {
            h.scene_name == "doman"
                && h.kind == FlagKind::Test
                && h.clean
                && !h.text_alias
                && h.partition == 1
        })
        .map(|h| h.record)
        .collect();
    for rec in [3usize, 4, 7, 8, 9, 15] {
        assert!(
            doman_tests.contains(&rec),
            "doman P1[{rec}] carries a clean head TEST of 0x370 (got {doman_tests:?})"
        );
    }
}
