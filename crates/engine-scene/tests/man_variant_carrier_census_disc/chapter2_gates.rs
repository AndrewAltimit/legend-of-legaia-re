//! Chapter-2 gate families, scene by scene.

use super::*;

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
    use legaia_engine_field::man_field_scripts::partition2_record_gates;
    let Some(index) = open_index() else { return };
    let gates = |scene_name: &str, rec: usize| -> (Vec<u16>, Vec<u16>) {
        let scene = Scene::load(&index, scene_name).expect("load");
        let man = scene
            .field_man_payload(&index)
            .expect("payload")
            .expect("MAN");
        let mf = legaia_scene_formats::man_section::parse(&man).expect("parse");
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
    use legaia_engine_field::man_field_scripts::partition2_record_gates;
    let Some(index) = open_index() else { return };
    let gates = |scene_name: &str, rec: usize| -> (Vec<u16>, Vec<u16>) {
        let scene = Scene::load(&index, scene_name).expect("load");
        let man = scene
            .field_man_payload(&index)
            .expect("payload")
            .expect("MAN");
        let mf = legaia_scene_formats::man_section::parse(&man).expect("parse");
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
    use legaia_engine_field::man_field_scripts::partition2_record_gates;
    use legaia_scene_formats::man_section::parse as parse_man;
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
    use legaia_engine_field::man_field_scripts::partition2_record_gates;
    use legaia_scene_formats::man_section::parse as parse_man;
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
    use legaia_engine_field::man_field_scripts::partition2_record_gates;
    use legaia_scene_formats::man_section::{ManFile, parse as parse_man};
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

    // bubu1 ships a count-5 scene asset table with a MAN; the detector used
    // to bound `count` to 6 or 7 and read it as MAN-less (`scene-bundles.md`).
    assert!(payload("bubu1").is_some(), "bubu1 has a field MAN payload");

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
