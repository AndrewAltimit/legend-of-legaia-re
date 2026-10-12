//! Disc-gated oracle for the **Fortress encounter fix**
//! (`legaia_patcher::fortress_encounter_fix` + `apply::apply_fortress_encounter_fix`).
//!
//! Applies the fix to a scratch copy of the real disc and asserts, off the
//! patched image, that:
//!   * the baseline tables are the documented retail shape, and the scenes whose
//!     condition list names flag `0x15` are exactly the Floating Castle's
//!     carriers and the two Fortress scenes (non-vacuous, and the proof that
//!     the Fortress is the only place outside the castle the flag is read
//!     from an encounter table);
//!   * after patching neither Fortress scene tests flag `0x15`, and for every
//!     story-flag state and every tile the roll reads the row a healthy retail
//!     file reads;
//!   * exactly 14 decoded MAN bytes moved per scene, nothing outside each MAN's
//!     compressed stream moved, no other PROT entry moved, and every changed
//!     sector is EDC/ECC-valid;
//!   * the result is byte-deterministic and a second application is a no-op;
//!   * it composes with the MAN-rewriting randomizers in either order;
//!   * a table that is not retail's is refused with nothing written.
//!
//! Gates on `LEGAIA_DISC_BIN`; skips+passes when unset. The patched image lives
//! only in memory.

use std::collections::BTreeSet;

use legaia_disc_patch::disc::DiscPatcher;
use legaia_iso::raw::SECTOR_SIZE;
use legaia_iso::write::mode2_form1_sector_is_valid;
use legaia_patcher::apply;
use legaia_patcher::drops::DropMode;
use legaia_patcher::encounter::SceneEncounters;
use legaia_patcher::fortress_encounter_fix::{
    self as fix, FORTRESS_SCENES, POST_JETTE_FLAG, RETAIL_STALE_ROW, STALE_FLAG, TableState,
};
use legaia_scene_formats::man_section;

/// `deroa`'s third gated group ("Gate of Hell vanished").
const GATE_FLAG: u16 = 0x046D;

fn load_disc() -> Option<Vec<u8>> {
    let p = std::path::PathBuf::from(std::env::var_os("LEGAIA_DISC_BIN")?);
    p.is_file().then(|| std::fs::read(&p).ok()).flatten()
}

fn scene(patcher: &DiscPatcher, idx: usize) -> SceneEncounters {
    let entry = patcher.read_entry(idx).expect("read entry");
    SceneEncounters::locate(&entry, idx).expect("scene bundle MAN")
}

/// The condition list of a decoded MAN as `(flag, region_count)`.
fn conditions(man: &[u8]) -> Vec<(u16, u16)> {
    let file = man_section::parse(man).unwrap();
    let body = file.encounter_section_body(man).unwrap();
    let sec = man_section::parse_encounter_section(body).unwrap();
    man_section::condition_records(body, &sec)
        .map(|c| c.unwrap())
        .map(|c| (c.flag_id, c.region_count))
        .collect()
}

/// The 12 raw region rows of a decoded MAN.
fn rows(man: &[u8]) -> Vec<[u8; 12]> {
    let file = man_section::parse(man).unwrap();
    let body = file.encounter_section_body(man).unwrap();
    let sec = man_section::parse_encounter_section(body).unwrap();
    assert_eq!(sec.region_stride, 12);
    body[sec.region_range.0..sec.region_range.1]
        .as_chunks::<12>()
        .0
        .to_vec()
}

/// What the roll reads at `tile` under `flags`: the winning row's payload
/// (`+4..+12`: rate, stage, formation slice, Door bits, return point), or
/// `None` when no row of the selected group holds the tile.
fn winning_payload(man: &[u8], flags: &[u16], tile: (u8, u8)) -> Option<[u8; 8]> {
    let file = man_section::parse(man).unwrap();
    let body = file.encounter_section_body(man).unwrap();
    let sec = man_section::parse_encounter_section(body).unwrap();
    let (first, count) = man_section::active_region_group(body, &sec, |f| flags.contains(&f))?;
    rows(man)[first..first + count]
        .iter()
        .find(|r| tile.0 >= r[0] && tile.0 <= r[2] && tile.1 >= r[1] && tile.1 <= r[3])
        .map(|r| r[4..].try_into().unwrap())
}

/// Every flag state over the three flags the Fortress lists name.
fn flag_states() -> Vec<Vec<u16>> {
    (0..8u8)
        .map(|m| {
            [STALE_FLAG, POST_JETTE_FLAG, GATE_FLAG]
                .into_iter()
                .enumerate()
                .filter(|(i, _)| m & (1 << i) != 0)
                .map(|(_, f)| f)
                .collect()
        })
        .collect()
}

/// Every sector that differs between two images is EDC/ECC-valid in `after`.
fn assert_changed_sectors_valid(before: &[u8], after: &[u8]) -> usize {
    assert_eq!(before.len(), after.len(), "image size unchanged");
    let mut changed = 0;
    let (before, _) = before.as_chunks::<SECTOR_SIZE>();
    let (after, _) = after.as_chunks::<SECTOR_SIZE>();
    for (i, (b, a)) in before.iter().zip(after).enumerate() {
        if a != b {
            changed += 1;
            assert!(
                mode2_form1_sector_is_valid(a),
                "changed sector {i} must be EDC/ECC-valid"
            );
        }
    }
    changed
}

#[test]
fn baseline_is_the_documented_retail_shape() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let patcher = DiscPatcher::open(disc).expect("open disc");

    for (idx, name) in FORTRESS_SCENES {
        let s = scene(&patcher, idx);
        let located = fix::locate(&s.decoded).unwrap_or_else(|e| panic!("{name}: {e:#}"));
        assert_eq!(located.state, TableState::Retail, "{name}");
        let conds = conditions(&s.decoded);
        let rows = rows(&s.decoded);
        assert_eq!(conds[0], (STALE_FLAG, 1), "{name} group 0");
        assert_eq!(conds[1].0, POST_JETTE_FLAG, "{name} group 1");
        assert_eq!(rows[0], RETAIL_STALE_ROW, "{name} row 0");
        // The copied row: whole map, rate 0, Door of Light allowed, a real
        // world-map return point - everything the castle's row is not.
        let row = located.edit.new_row;
        assert_eq!(&row[..5], &[0, 0, 128, 128, 0], "{name} quiet row");
        assert_ne!(row[8] & 0x80, 0, "{name}: Door of Light allowed");
        assert_eq!(RETAIL_STALE_ROW[8] & 0x80, 0, "castle row blocks it");
        // No streaming variant MAN: the bundle MAN is the scene's only carrier.
        let entry = patcher.read_entry(idx).unwrap();
        assert!(
            SceneEncounters::locate_streaming_mans(&entry, idx).is_empty(),
            "{name} has no variant MAN"
        );
    }
    // The two scenes differ where the docs say they do.
    assert_eq!(
        conditions(&scene(&patcher, FORTRESS_SCENES[0].0).decoded)[1].1,
        1
    );
    assert_eq!(
        conditions(&scene(&patcher, FORTRESS_SCENES[1].0).decoded)[1].1,
        2
    );

    // Disc-wide: which MAN carriers name flag 0x15 in a condition list.
    let mut carriers = BTreeSet::new();
    for idx in 0..patcher.entry_count() {
        let Ok(entry) = patcher.read_entry(idx) else {
            continue;
        };
        let mut mans: Vec<SceneEncounters> =
            SceneEncounters::locate(&entry, idx).into_iter().collect();
        mans.extend(SceneEncounters::locate_streaming_mans(&entry, idx));
        for m in mans {
            if conditions(&m.decoded).iter().any(|c| c.0 == STALE_FLAG) {
                carriers.insert((idx, m.raw_in_place));
            }
        }
    }
    eprintln!("flag {STALE_FLAG:#06x} condition-list carriers (entry, raw): {carriers:?}");
    let outside_castle: Vec<usize> = carriers
        .iter()
        .map(|c| c.0)
        .filter(|idx| !CASTLE_ENTRIES.contains(idx))
        .collect();
    assert_eq!(
        outside_castle,
        FORTRESS_SCENES.map(|s| s.0),
        "outside the Floating Castle only the two Fortress scenes list flag 0x15"
    );
    assert_eq!(
        carriers,
        BTreeSet::from([
            (373, true),
            (426, false),
            (427, true),
            (606, false),
            (706, false)
        ]),
        "the castle's three carriers (taiku is raw-only) and the two Fortress bundles"
    );
}

/// PROT entries of the Floating Castle's MAN carriers: `taiku`'s raw streaming
/// MAN (it has no bundle MAN), and `taiku2`'s bundle MAN + raw variant.
const CASTLE_ENTRIES: [usize; 3] = [373, 426, 427];

#[test]
fn fix_drops_the_stale_group_and_nothing_else() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let original = DiscPatcher::open(disc.clone()).expect("open original");
    let mut patcher = DiscPatcher::open(disc).expect("open disc");

    let report = apply::apply_fortress_encounter_fix(&mut patcher).expect("apply");
    assert_eq!(report.scenes_changed, 2);
    assert!(report.changed());

    for (idx, name) in FORTRESS_SCENES {
        let before = scene(&original, idx);
        let after = scene(&patcher, idx);
        assert_eq!(
            fix::locate(&after.decoded).unwrap().state,
            TableState::Fixed
        );
        assert!(
            conditions(&after.decoded).iter().all(|c| c.0 != STALE_FLAG),
            "{name} no longer tests flag 0x15"
        );
        // Group sizes, and every row but row 0, are untouched.
        assert_eq!(
            conditions(&before.decoded)
                .iter()
                .map(|c| c.1)
                .collect::<Vec<_>>(),
            conditions(&after.decoded)
                .iter()
                .map(|c| c.1)
                .collect::<Vec<_>>()
        );
        assert_eq!(
            conditions(&before.decoded)[1..],
            conditions(&after.decoded)[1..]
        );
        assert_eq!(rows(&before.decoded)[1..], rows(&after.decoded)[1..]);

        // Surgical in the decoded MAN: the flag id and row 0, nothing else.
        assert_eq!(before.decoded.len(), after.decoded.len());
        let edit = fix::locate(&before.decoded).unwrap().edit;
        let allowed: BTreeSet<usize> = (edit.condition_off..edit.condition_off + 2)
            .chain(edit.region_off..edit.region_off + 12)
            .collect();
        let changed: BTreeSet<usize> = before
            .decoded
            .iter()
            .zip(&after.decoded)
            .enumerate()
            .filter(|(_, (a, b))| a != b)
            .map(|(i, _)| i)
            .collect();
        assert!(
            !changed.is_empty() && changed.is_subset(&allowed),
            "{name}: changed {changed:?}, allowed {allowed:?}"
        );

        // Meaning: every flag state, every tile.
        let mut healed_tiles = 0usize;
        for flags in flag_states() {
            let healthy: Vec<u16> = flags.iter().copied().filter(|&f| f != STALE_FLAG).collect();
            for x in 0..=128u8 {
                for z in 0..=128u8 {
                    let want = winning_payload(&before.decoded, &healthy, (x, z));
                    assert_eq!(
                        winning_payload(&after.decoded, &flags, (x, z)),
                        want,
                        "{name} flags {flags:?} tile ({x}, {z})"
                    );
                    if flags == [STALE_FLAG] {
                        // Retail on an affected file: rate 0 everywhere.
                        let retail = winning_payload(&before.decoded, &flags, (x, z)).unwrap();
                        assert_eq!(retail[0], 0);
                        if want.is_some_and(|p| p[0] != 0) {
                            healed_tiles += 1;
                        }
                    }
                }
            }
        }
        assert!(
            healed_tiles > 1000,
            "{name}: {healed_tiles} tiles roll again"
        );

        // Nothing outside the MAN's own compressed stream moved in the entry.
        let (e0, e1) = (
            original.read_entry(idx).unwrap(),
            patcher.read_entry(idx).unwrap(),
        );
        assert_eq!(e0.len(), e1.len());
        let stream = before.man_offset..before.man_offset + before.compressed_budget;
        for i in 0..e0.len() {
            if !stream.contains(&i) {
                assert_eq!(
                    e0[i], e1[i],
                    "{name}: byte {i:#x} outside the MAN stream changed"
                );
            }
        }
    }

    // No other PROT entry moved - the castle's tables in particular.
    for idx in 0..original.entry_count() {
        if FORTRESS_SCENES.iter().any(|s| s.0 == idx) {
            continue;
        }
        let (Ok(a), Ok(b)) = (original.read_entry(idx), patcher.read_entry(idx)) else {
            continue;
        };
        assert!(a == b, "PROT entry {idx} changed");
    }

    let changed = assert_changed_sectors_valid(original.image(), patcher.image());
    assert!(changed > 0, "the fix wrote sectors");
}

#[test]
fn fix_is_byte_deterministic_and_idempotent() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut a = DiscPatcher::open(disc.clone()).expect("open a");
    let mut b = DiscPatcher::open(disc).expect("open b");
    apply::apply_fortress_encounter_fix(&mut a).unwrap();
    apply::apply_fortress_encounter_fix(&mut b).unwrap();
    assert!(a.image() == b.image(), "two runs produce the same image");

    let once = a.image().to_vec();
    let again = apply::apply_fortress_encounter_fix(&mut a).expect("re-apply");
    assert_eq!(again.scenes_changed, 0);
    assert!(!again.changed());
    assert!(a.image() == once.as_slice(), "re-apply writes nothing");
}

#[test]
fn fix_composes_with_the_man_rewriting_randomizers() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let original = DiscPatcher::open(disc.clone()).expect("open original");
    let seed = 0x0F02_7E55_u64;
    let no_static = BTreeSet::new();
    let randomize = |p: &mut DiscPatcher| {
        apply::randomize_encounters(p, seed, DropMode::Random, &[]).expect("encounters");
        apply::randomize_chests(p, &[], seed, DropMode::Shuffle, &no_static).expect("chests");
        apply::randomize_doors(p, seed, DropMode::Shuffle, apply::DoorCoupling::Coupled)
            .expect("doors");
    };

    // The CLI / browser order: the fix first, the randomizers re-pack after.
    let mut fix_first = DiscPatcher::open(disc.clone()).expect("open");
    apply::apply_fortress_encounter_fix(&mut fix_first).unwrap();
    randomize(&mut fix_first);
    // And the reverse: the fix lands on already-randomized MANs.
    let mut fix_last = DiscPatcher::open(disc).expect("open");
    randomize(&mut fix_last);
    let report = apply::apply_fortress_encounter_fix(&mut fix_last).expect("fix after randomizers");
    assert_eq!(report.scenes_changed, 2);

    for (label, patched) in [("fix first", &fix_first), ("fix last", &fix_last)] {
        for (idx, name) in FORTRESS_SCENES {
            let after = scene(patched, idx);
            assert_eq!(
                fix::locate(&after.decoded).unwrap().state,
                TableState::Fixed,
                "{label}: {name} keeps the fix"
            );
            // The randomizers never touch the condition or region arrays, so
            // the healed table reads as it does without them.
            let before = scene(&original, idx);
            assert_eq!(rows(&before.decoded)[1..], rows(&after.decoded)[1..]);
            for (x, z) in [(49u8, 37u8), (10, 10), (100, 100)] {
                assert_eq!(
                    winning_payload(&after.decoded, &[STALE_FLAG], (x, z)),
                    winning_payload(&before.decoded, &[], (x, z)),
                    "{label}: {name} tile ({x}, {z})"
                );
            }
        }
        assert_changed_sectors_valid(original.image(), patched.image());
    }
}

#[test]
fn a_table_that_is_not_retail_is_refused_with_nothing_written() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut patcher = DiscPatcher::open(disc).expect("open disc");

    // Give the SECOND scene's group-0 row a rate retail does not have, so the
    // first scene plans cleanly and the failure has something to roll back.
    let (idx, _) = FORTRESS_SCENES[1];
    let mut s = scene(&patcher, idx);
    let off = fix::locate(&s.decoded).unwrap().edit.region_off;
    s.decoded[off + 4] = 18;
    let stream = s.repack().expect("tampered MAN re-packs");
    patcher
        .patch_prot_entry(idx, s.man_offset as u64, &stream)
        .expect("write tampered MAN");

    let tampered = patcher.image().to_vec();
    let err = apply::apply_fortress_encounter_fix(&mut patcher).expect_err("must refuse");
    let msg = format!("{err:#}");
    assert!(msg.contains("chitei2"), "error names the scene: {msg}");
    assert!(
        patcher.image() == tampered.as_slice(),
        "a refused run writes nothing - the first scene included"
    );
    assert_eq!(
        fix::locate(&scene(&patcher, FORTRESS_SCENES[0].0).decoded)
            .unwrap()
            .state,
        TableState::Retail
    );
}
