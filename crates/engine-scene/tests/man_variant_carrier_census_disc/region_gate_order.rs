//! Region gates: bracket write order, and the unordered flags no single record reads jointly.

use super::*;

/// The write order **inside** a card bracket, where the scripts fix it.
///
/// `crates/save/tests/region_gate_card_brackets.rs` places each residual
/// gate family between two milestone saves; this pins the order of the writes
/// that share one bracket, from the MAN bytes alone:
///
/// - **`rayman`'s chain is forced by its own gates.** Each link has exactly one
///   story SET site disc-wide, and each setter's C2 list is the previous link:
///   `0x201` (`P1[47]`, a talk beat) -> `0x1FB` (`P2[12]`, C2 `0x201`) ->
///   `0x200` (`P2[18]`, C2 `0x1FB`) -> `0x1FC` (`P2[19]`, C2 `0x200`).
/// - **`bubu2`'s pair is one beat.** `P2[0]` (C1 `0x3D3`, C2 `0x608`) SETs
///   `0x609` then `0x3D3` on adjacent instructions. `P2[2]` (C1 carries
///   `0x608`) is dead once `0x608` is set, which the bracket before this one
///   already guarantees, and `P2[3]` (C1 `0x609`) is retired by `P2[0]`.
/// - **retock's `0x502` precedes `0x33B`.** `0x502`'s only story writer is
///   `retock P2[33]`, which only Eliza's talk dispatch (`P1[31]`) spawns, and
///   the dispatch tests `0x33B` first and diverts to her other dialogue when it
///   is set. Once jagaroom's `P2[8]` latches `0x33B`, `0x502` can no longer be
///   written; both are set in the Dohati save, so it was written first.
/// - **deroa's `0x46D` follows `0x3E1`** (`P2[4]`, C2 `0x3E1`); `0x46E` /
///   `0x46F` (`P2[5]` / `P2[6]`) carry only their own C1 latch, so their order
///   against `0x46D` is where the player walks, not a script fact.
#[test]
fn region_gate_in_bracket_write_order() {
    use legaia_engine_field::man_field_scripts::{
        partition2_record_gates, walk_partition_gflag_sites,
    };
    let Some(index) = open_index() else { return };
    let scenes = index.cdname_scene_names();
    let census = system_flag_census(&index, &scenes);
    // Story SET sites: clean, not a prose alias, not a developer flag menu.
    let story_sets = |flag: u16| -> BTreeSet<(String, usize, usize)> {
        census
            .get(&flag)
            .map(|s| {
                s.iter()
                    .filter(|h| {
                        h.kind == FlagKind::Set && h.clean && !h.text_alias && !h.debug_menu
                    })
                    .map(|h| (h.scene_name.clone(), h.partition, h.record))
                    .collect()
            })
            .unwrap_or_default()
    };
    let one = |s: &str, p: usize, r: usize| BTreeSet::from([(s.to_string(), p, r)]);
    let man_of = |scene_name: &str| {
        let scene = Scene::load(&index, scene_name).expect("load");
        let man = scene
            .field_man_payload(&index)
            .expect("payload")
            .expect("MAN");
        let mf = legaia_scene_formats::man_section::parse(&man).expect("parse");
        (mf, man)
    };
    let gates = |scene_name: &str, rec: usize| {
        let (mf, man) = man_of(scene_name);
        partition2_record_gates(&mf, &man, rec).expect("gates")
    };

    // rayman: one writer per link, each gated on the link before it.
    assert_eq!(story_sets(0x201), one("rayman", 1, 47));
    assert_eq!(story_sets(0x1FB), one("rayman", 2, 12));
    assert_eq!(story_sets(0x200), one("rayman", 2, 18));
    assert_eq!(story_sets(0x1FC), one("rayman", 2, 19));
    assert_eq!(gates("rayman", 12), (vec![0x1FB], vec![0x201]));
    assert_eq!(gates("rayman", 18), (vec![0x200], vec![0x1FB]));
    assert_eq!(gates("rayman", 19), (vec![0x1FC], vec![0x200]));

    // bubu2: P2[0] writes 0x609 then 0x3D3 on adjacent instructions.
    assert_eq!(gates("bubu2", 0), (vec![0x3D3], vec![0x608]));
    assert_eq!(gates("bubu2", 2), (vec![0x608, 0x3D3], vec![]));
    assert_eq!(gates("bubu2", 3), (vec![0x609], vec![0x608, 0x3D3]));
    let (mf, man) = man_of("bubu2");
    let p2_0: Vec<_> = walk_partition_gflag_sites(&mf, &man, 2)
        .into_iter()
        .filter(|s| s.record == 0 && s.kind == FlagKind::Set && s.clean)
        .filter(|s| s.flag == 0x609 || s.flag == 0x3D3)
        .collect();
    assert_eq!(p2_0.len(), 2, "bubu2 P2[0] carries the pair once");
    assert_eq!((p2_0[0].flag, p2_0[1].flag), (0x609, 0x3D3));
    assert_eq!(p2_0[1].abs_pc, p2_0[0].abs_pc + 2, "adjacent SETs");

    // retock: 0x502's only story writer is P2[33]; 0x33B's is jagaroom P2[8].
    assert_eq!(story_sets(0x502), one("retock", 2, 33));
    assert_eq!(story_sets(0x33B), one("jagaroom", 2, 8));
    assert_eq!(gates("retock", 33), (vec![0x502], vec![0x357]));
    assert_eq!(gates("jagaroom", 8), (vec![0x33B], vec![0x351]));
    // Eliza's talk dispatch (P1[31]): Test 0x33B -> her other dialogue,
    // Test 0x63C -> SpawnRecord P2[33], else SpawnRecord P2[16] and loop.
    let (mf, man) = man_of("retock");
    let counts = &mf.header.partition_counts;
    let p2_33 = (counts[0] + counts[1] + 33) as u8;
    let p2_16 = (counts[0] + counts[1] + 16) as u8;
    let dispatch = [
        0x73, 0x3B, 0x12, 0x00, // Test 0x33B -> Eliza's post-0x33B lines
        0x76, 0x3C, 0x08, 0x00, // Test 0x63C -> spawn P2[33]
        0x44, p2_16, 0x21, 0x26, 0xF0, 0xFF, // spawn P2[16], loop
        0x44, p2_33, // spawn P2[33]
    ];
    assert!(
        man.windows(dispatch.len()).any(|w| w == dispatch),
        "retock's P2[33] spawn sits behind a 0x33B-clear test"
    );

    // deroa: 0x46D waits on 0x3E1; 0x46E / 0x46F carry no C2.
    assert_eq!(story_sets(0x46D), one("deroa", 2, 4));
    assert_eq!(gates("deroa", 4), (vec![0x46D], vec![0x3E1]));
    assert_eq!(gates("deroa", 5), (vec![0x46E], vec![]));
    assert_eq!(gates("deroa", 6), (vec![0x46F], vec![]));
    eprintln!("[ran] in-bracket write order: rayman chain, bubu2 pair, retock 0x502 < 0x33B");
}

/// The two in-bracket orders no gate decides are **unobservable**.
///
/// `doman`'s `0x3FB` (against bubu2's `0x609` / `0x3D3`) and deroa's
/// `0x46E` / `0x46F` (against `0x46D`). Disc-wide, across every scene's bundle
/// and variant MAN:
///
/// - `0x3FB` has no clean, non-alias, non-debug field-VM `TEST`;
/// - `0x46E`'s one reader is deroa `P2[8]`, whose whole body is that test
///   guarding `[4C 38]` (the camera-region query at the player's tile) and an
///   idle loop;
/// - `0x46F`'s two readers are deroa's scene objects `P0[0]` (parks the object
///   at `MoveTo 7F 7F` when set) and `P0[5]` (`[4C 42]` target `-600` when
///   set, `-1500` when clear);
/// - every one of those readers tests that one flag and no other flag at all;
/// - no partition-2 record lists any of the three in a C2 (requires-all) gate;
///   the C1 lists that carry them are the writer records' own plus the two
///   ending epilogues' `P2[3]` (`edbalden`, `eddoman`: C1 `[0x3FB]` alone).
///
/// (The SCUS and overlay images carry no literal call of the flag helpers
/// `FUN_8003CE08` / `_CE34` / `_CE64` with these ids either.) Each flag's
/// *value* is observable and the milestone saves pin it; no reader looks at
/// two of them together, so the order the player's route writes them in
/// changes nothing a script, a record gate or an engine path can observe.
#[test]
fn region_gate_unordered_flags_have_no_joint_reader() {
    use legaia_engine_field::man_field_scripts::partition2_record_gates;
    let Some(index) = open_index() else { return };
    let scenes = index.cdname_scene_names();
    let census = system_flag_census(&index, &scenes);
    let flags: [(u16, &str, usize); 3] = [
        (0x3FB, "doman", 4),
        (0x46E, "deroa", 5),
        (0x46F, "deroa", 6),
    ];
    for (flag, _, _) in flags {
        let readers: BTreeSet<_> = census
            .get(&flag)
            .into_iter()
            .flatten()
            .filter(|h| h.kind == FlagKind::Test && h.clean && !h.text_alias && !h.debug_menu)
            .map(|h| (h.scene_name.clone(), h.partition, h.record))
            .collect();
        let expect: BTreeSet<(String, usize, usize)> = match flag {
            0x46E => BTreeSet::from([("deroa".to_string(), 2, 8)]),
            0x46F => BTreeSet::from([("deroa".to_string(), 0, 0), ("deroa".to_string(), 0, 5)]),
            _ => BTreeSet::new(),
        };
        assert_eq!(readers, expect, "script readers of {flag:#X}");
    }
    // Each reader's whole flag traffic is the one test of its own flag.
    {
        use legaia_engine_field::man_field_scripts::walk_partition_gflag_sites;
        let scene = Scene::load(&index, "deroa").expect("load deroa");
        let man = scene
            .field_man_payload(&index)
            .expect("payload")
            .expect("MAN");
        let mf = legaia_scene_formats::man_section::parse(&man).expect("parse");
        for (p, rec, flag) in [(2usize, 8usize, 0x46Eu16), (0, 0, 0x46F), (0, 5, 0x46F)] {
            let sites: Vec<_> = walk_partition_gflag_sites(&mf, &man, p)
                .into_iter()
                .filter(|s| s.record == rec && s.clean)
                .map(|s| (s.kind, s.flag))
                .collect();
            assert_eq!(sites, vec![(FlagKind::Test, flag)], "deroa P{p}[{rec}]");
        }
    }
    let mut c1_holders: BTreeSet<(u16, String, usize)> = BTreeSet::new();
    for name in &scenes {
        let Ok(scene) = Scene::load(&index, name) else {
            continue;
        };
        for carrier in scene_man_carriers(&index, &scene) {
            let Ok(mf) = legaia_scene_formats::man_section::parse(&carrier.payload) else {
                continue;
            };
            let n2 = mf.header.partition_counts[2] as usize;
            for rec in 0..n2 {
                let Some((c1, c2)) = partition2_record_gates(&mf, &carrier.payload, rec) else {
                    continue;
                };
                for (flag, _, _) in flags {
                    assert!(
                        !c2.contains(&flag),
                        "{name} P2[{rec}] gates on {flag:#X} (C2)"
                    );
                    if c1.contains(&flag) {
                        eprintln!("  C1 holder {flag:#X}: {name} P2[{rec}] C1={c1:X?} C2={c2:X?}");
                        c1_holders.insert((flag, name.to_string(), rec));
                    }
                }
            }
        }
    }
    // The writers' own latches, plus the two ending epilogues' `P2[3]`, whose
    // C1 is `[0x3FB]` alone: a single-flag read at the very end of the game,
    // long after the bracket.
    let mut expect: BTreeSet<(u16, String, usize)> = flags
        .iter()
        .map(|&(f, s, r)| (f, s.to_string(), r))
        .collect();
    expect.insert((0x3FB, "edbalden".to_string(), 3));
    expect.insert((0x3FB, "eddoman".to_string(), 3));
    assert_eq!(c1_holders, expect, "C1 lists carrying the three flags");
    eprintln!("[ran] 0x3FB / 0x46E / 0x46F: single-flag readers only, no C2, C1 = own latch");
}
