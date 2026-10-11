//! Flag 549: the Rim Elm P2 gate that reads it, the record that latches it, and `town0c` as a
//! Rim Elm state variant.

use super::*;

/// The READER of flag `0x225` (549, the Rim Elm opening one-shot) is the same
/// record-gate evaluator `FUN_8003BDE0`, disc-wide across the Rim Elm scene
/// variants: `P2[3]` C1 = `[0x225]` (blocks the opening once it has played)
/// and `P2[4]` C2 contains `0x225` (the post-naming beat requires the opening
/// done). It is read ONLY from these record-HEADER C1/C2 gate lists, never an
/// inline `0x70` TEST - which is why the inline-opcode census reports zero
/// test sites.
///
/// ## How many scenes gate on it: **three**, and why it is not four
///
/// The gating scenes are `town01` / `town0b` / `town0c` - the three Rim Elm
/// renditions - each with the identical `P2[3]` / `P2[4]` gate pair.
///
/// This census used to read four, the fourth being `gameover_data`. That was
/// `town01`'s MAN counted twice, not a fourth carrier, and the reason is
/// structural rather than statistical:
///
///  - `gameover_data`'s CDNAME block is extraction entries `1..3`, a strict
///    **subset** of `town01`'s `1..10` - the two head defines (`init_data 0`,
///    `gameover_data 1`) sit inside the TOC header rows and keep unshifted
///    legacy windows, so they land on entries the `-2` shift gives to the
///    scene that follows (`docs/formats/cdname.md`);
///  - that window contains no `SceneAssetTable` entry at all. It holds
///    `town01`'s `FieldMap` (entry 1) and `SceneV12Table` (entry 2, one
///    sector). Under the old declared-span PROT size the one-sector v12 entry
///    read `toc[p+5] - toc[p+3] + 4` sectors, running through the event
///    scripts and into entry 4 - `town01`'s bundle, and its MAN;
///  - with entry sizes corrected to `toc[p+3] - toc[p+2]`, entry 2 is exactly
///    its 2048 bytes, `find_bundle` returns `None`, and the block carries no
///    MAN. There was never a "`gameover_data` dev copy of the town01 MAN":
///    it was the town01 MAN, read through a neighbouring block's window.
///
/// The assertions below pin the reading rather than the count: the three
/// carriers are checked to be three *distinct PROT entries at three distinct
/// start LBAs carrying three distinct payloads*, and `gameover_data` is
/// checked to resolve no MAN while its block stays inside `town01`'s. See
/// also `no_two_man_carriers_share_bytes_disc_wide`, the general form.
#[test]
fn flag_549_reader_is_the_rim_elm_p2_gate() {
    use legaia_engine_field::man_field_scripts::partition2_record_gates;
    let Some(index) = open_index() else { return };
    // Canonical case pinned exactly; the other variants share the shape.
    let scene = Scene::load(&index, "town01").expect("load town01");
    let man = scene
        .field_man_payload(&index)
        .expect("payload")
        .expect("town01 MAN resolves");
    let mf = legaia_scene_formats::man_section::parse(&man).expect("parse");
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

    // Disc-wide: which carriers gate on 0x225, and what entry each is.
    let mut gating: Vec<(String, u32, Option<u32>, Vec<u8>)> = Vec::new();
    for name in index.cdname_scene_names() {
        let Ok(scene) = Scene::load(&index, &name) else {
            continue;
        };
        for carrier in scene_man_carriers(&index, &scene) {
            let Ok(mf) = legaia_scene_formats::man_section::parse(&carrier.payload) else {
                continue;
            };
            let n_p2 = (*mf.header.partition_counts.get(2).unwrap_or(&0)).max(0) as usize;
            let gates_549 = (0..n_p2).any(|i| {
                partition2_record_gates(&mf, &carrier.payload, i)
                    .map(|(c1, c2)| c1.contains(&0x225) || c2.contains(&0x225))
                    .unwrap_or(false)
            });
            if gates_549 {
                gating.push((
                    name.clone(),
                    carrier.entry_idx,
                    index.entry_start_lba_retail(carrier.entry_idx as u16),
                    carrier.payload.clone(),
                ));
            }
        }
    }
    let named: Vec<(&str, u32)> = gating.iter().map(|(n, e, _, _)| (n.as_str(), *e)).collect();
    eprintln!("[disc] 0x225 gate carriers: {named:?}");
    assert_eq!(
        named,
        vec![("town01", 4u32), ("town0b", 13), ("town0c", 22)],
        "0x225 is read as a C1/C2 gate in the three Rim Elm renditions"
    );

    // Corroboration that three is three carriers and not one MAN counted
    // three times: distinct PROT entries, distinct start LBAs, distinct bytes.
    let lbas: BTreeSet<Option<u32>> = gating.iter().map(|(_, _, l, _)| *l).collect();
    assert_eq!(lbas.len(), 3, "three distinct start LBAs, got {lbas:?}");
    for (a, b) in [(0usize, 1usize), (0, 2), (1, 2)] {
        assert_ne!(
            gating[a].3, gating[b].3,
            "{} and {} must be different MAN payloads",
            gating[a].0, gating[b].0
        );
    }

    // And the structural reason the fourth is gone: `gameover_data`'s block is
    // inside `town01`'s and carries no asset-table bundle of its own, so it
    // resolves no MAN. It only ever produced one because a one-sector entry
    // read under the old declared-span size ran into town01's bundle.
    let (go_lo, go_hi) = index
        .block_range_extraction("gameover_data")
        .expect("gameover_data block");
    let (t1_lo, t1_hi) = index
        .block_range_extraction("town01")
        .expect("town01 block");
    assert!(
        go_lo >= t1_lo && go_hi <= t1_hi,
        "gameover_data's block ({go_lo}..{go_hi}) sits inside town01's ({t1_lo}..{t1_hi})"
    );
    let go = Scene::load(&index, "gameover_data").expect("load gameover_data");
    assert!(
        go.entries.iter().all(|e| e.class != Class::SceneAssetTable),
        "gameover_data's window holds no asset-table bundle: {:?}",
        go.entries
            .iter()
            .map(|e| (e.idx, e.class))
            .collect::<Vec<_>>()
    );
    assert!(
        scene_man_carriers(&index, &go).is_empty(),
        "gameover_data resolves no MAN carrier of its own"
    );
}

/// Flag 549 (`0x225`) WRITER: the Rim Elm opening one-shot is a SELF-LATCH -
/// town01 `P2[3]` (the very record whose C1 gate reads 549) SETs it from its
/// own script bytes (`52 25` at body offset `+0x3`), the same shape as the
/// rikuroa `P2[50]`/`0x142` post-victory latch.
///
/// Runtime-pinned first (reader-watch burst from the `s2_rimelm_town01`
/// anchor, where 549 is still clear; it is set by `s3_rimelm_freeroam`): the
/// SET fired via `FUN_8003CE08` with `ra 0x801E3598` - the field-VM
/// dispatcher's own `0x5x` arm - and the captured script-PC
/// (`vm=0x800EAFDF vmo=0xF`) resolves to town01 P2[3] body `+0x3`
/// (record start `0x6FC8`, pc0 12). The census had missed it because the
/// `4C ED` op (`_DAT_8007BA66` write, retail `param_2 + 3`) had no width in
/// the disassembler, desyncing the walk one byte before the SET. So the old
/// "direct code path, capture-only" verdict is FALSIFIED - the writer is
/// plain script bytes in the PRIMARY bundle MAN; the census was width-blind,
/// not carrier-blind.
///
/// The SET is a **single** site. This census used to report a second, in a
/// `gameover_data` "dev copy" of the town01 MAN; there is no such copy. That
/// block's window is a subset of `town01`'s and carries no asset-table bundle
/// of its own, so the "copy" was town01's own sectors read through the old
/// declared-span entry size - see the reader test above for the full
/// derivation, and `no_two_man_carriers_share_bytes_disc_wide` for the
/// disc-wide property that rules the shape out generally.
#[test]
fn flag_549_writer_is_the_rim_elm_p2_3_self_latch() {
    let Some(index) = open_index() else { return };
    let scenes = index.cdname_scene_names();
    let census = system_flag_census(&index, &scenes);

    let sites = census.get(&0x225).expect("0x225 sites");
    let sets: BTreeSet<(String, bool, usize, usize, bool)> = sites
        .iter()
        .filter(|h| h.kind == FlagKind::Set)
        .map(|h| {
            (
                h.scene_name.clone(),
                h.variant,
                h.partition,
                h.record,
                h.clean,
            )
        })
        .collect();
    assert_eq!(
        sets,
        BTreeSet::from([("town01".to_string(), false, 2, 3, true)]),
        "0x225 SET sites: exactly one - the town01 P2[3] self-latch, clean decode"
    );

    // The runtime-confirmed inline TEST (P1[0], the scene-entry system
    // script, live `vm` offset +0x5D) also surfaces - decode-flagged, since
    // P1[0] walks through dialogue upstream, but present.
    assert!(
        sites.iter().any(|h| h.scene_name == "town01"
            && h.partition == 1
            && h.record == 0
            && h.kind == FlagKind::Test),
        "town01 P1[0] inline TEST of 0x225 surfaces in the census"
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
