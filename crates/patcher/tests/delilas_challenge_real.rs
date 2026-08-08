//! Disc round-trip oracle for the **Delilas Challenge** mod.
//!
//! Requires `LEGAIA_DISC_BIN`; skips (and passes) without it.

use legaia_patcher::apply;
use legaia_patcher::delilas_challenge::{
    DEFAULT_REWARD_ITEM, DELILAS_IDS, DelilasSites, KORU_DEFEATED_FLAG, MARKER_GROUP, MARKER_SOLO,
    OUTCOME_SURVIVED_FLAG, SCRIPTED_LOSS_FLAG,
};
use legaia_patcher::disc::DiscPatcher;

fn load_disc() -> Option<Vec<u8>> {
    let p = std::path::PathBuf::from(std::env::var_os("LEGAIA_DISC_BIN")?);
    p.is_file().then(|| std::fs::read(&p).ok()).flatten()
}

/// Decode the koin1 MAN off a (possibly patched) image.
fn koin1_sites(image: Vec<u8>) -> DelilasSites {
    let patcher = DiscPatcher::open(image).expect("open image");
    for idx in 0..patcher.entry_count() {
        let entry = patcher.read_entry(idx).expect("read entry");
        if let Some(sites) = DelilasSites::locate(&entry, idx) {
            return sites;
        }
    }
    panic!("no entry carries the Muscle Dome enrollment script");
}

#[test]
fn delilas_challenge_round_trips_on_the_real_disc() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };

    // Baseline: the retail disc locates as not-applied.
    let base = koin1_sites(disc.clone());
    assert!(!base.already_applied, "retail disc must read as unpatched");

    // Apply.
    let mut patcher = DiscPatcher::open(disc.clone()).expect("open disc");
    let report = apply::apply_delilas_challenge(&mut patcher).expect("apply");
    assert!(report.changed);
    assert!(report.grown_bytes > 0);
    assert!(report.compressed_len <= report.compressed_budget);
    let patched = patcher.into_image();

    // Same-size image; every touched sector still EDC/ECC-valid (DiscPatcher
    // re-encodes on write; re-opening validates the sector forms).
    assert_eq!(patched.len(), disc.len(), "image size must not change");
    let _reopen = DiscPatcher::open(patched.clone()).expect("patched image re-parses");

    // The patched image locates as applied, and a second apply is a no-op.
    let after = koin1_sites(patched.clone());
    assert!(after.already_applied, "patched disc must read as applied");
    let mut patcher2 = DiscPatcher::open(patched.clone()).expect("re-open");
    let report2 = apply::apply_delilas_challenge(&mut patcher2).expect("re-apply");
    assert!(!report2.changed, "second apply must be a no-op");
    let repatched = patcher2.into_image();
    assert_eq!(repatched, patched, "no-op re-apply must not touch bytes");

    // Determinism: applying to a fresh copy of the original produces the
    // identical image.
    let mut patcher3 = DiscPatcher::open(disc.clone()).expect("open disc again");
    apply::apply_delilas_challenge(&mut patcher3).expect("apply again");
    assert_eq!(
        patcher3.into_image(),
        patched,
        "apply must be deterministic"
    );

    // Composes with the sibling koin1-MAN feature (Earth Egg price edit):
    // both rewrite PROT 543's MAN, in either order.
    let mut ab = DiscPatcher::open(disc.clone()).expect("open");
    apply::apply_delilas_challenge(&mut ab).expect("delilas first");
    apply::set_earth_egg_price(&mut ab, 500).expect("earth egg second");
    let ab = ab.into_image();
    let mut ba = DiscPatcher::open(disc).expect("open");
    apply::set_earth_egg_price(&mut ba, 500).expect("earth egg first");
    apply::apply_delilas_challenge(&mut ba).expect("delilas second");
    let ba = ba.into_image();
    let ab_sites = koin1_sites(ab);
    assert!(ab_sites.already_applied);
    let ba_sites = koin1_sites(ba);
    assert!(ba_sites.already_applied);

    // Decode-level assertions on the patched MAN.
    let man = &after.decoded;
    // 1. Formation row 0 is the boss-header Delilas trio.
    let mf = legaia_asset::man_section::parse(man).expect("patched MAN parses");
    let enc = mf.sections[0];
    let row = &man[enc.body_offset() + 4..enc.body_offset() + 12];
    assert_eq!(&row[..4], &[1, 0, 0, 3]);
    assert_eq!(&row[4..7], &DELILAS_IDS);
    // 2. The clerk record carries a 4-option who-picker whose labels are the
    //    three name tokens plus the challenge label, and whose branch opens
    //    with the Koru gate, latches the marker in the solo arms, and fights
    //    formation row 0.
    let dro = mf.data_region_offset;
    let mut found_picker = false;
    for &off in &mf.partitions[1] {
        let start = dro + off as usize;
        let Some(&locals) = man.get(start) else {
            continue;
        };
        let pc0 = 1 + locals as usize * 2 + 4;
        let mut end = man.len();
        for part in &mf.partitions {
            for &o in part {
                let s = dro + o as usize;
                if s > start && s < end {
                    end = s;
                }
            }
        }
        for s in &mf.sections {
            if s.offset > start && s.offset < end {
                end = s.offset;
            }
        }
        if start + pc0 >= end {
            continue;
        }
        let rec = &man[start..end];
        for open in pc0..rec.len() {
            if (rec[open] & 0x7F) != 0x29 || rec[open - 1] != 0x00 {
                continue;
            }
            let Some(p) = legaia_mes::parse_picker_at(rec, open) else {
                continue;
            };
            let labels_at = open + 1 + p.n * 2;
            if !rec[labels_at..].starts_with(&[0x1F, 0xC1, 0x00, 0x00]) {
                continue;
            }
            found_picker = true;
            // Option 3 targets the branch; the branch gate tests 0x378.
            let t = p.jump_target(3).expect("4th option target");
            assert!(t < rec.len());
            let gate_op = 0x70 | (KORU_DEFEATED_FLAG >> 8) as u8;
            assert_eq!(rec[t], gate_op, "branch must open with the Koru test");
            assert_eq!(rec[t + 1], (KORU_DEFEATED_FLAG & 0xFF) as u8);
            let branch = &rec[t..];
            // Battle op against formation row 0.
            assert!(branch.windows(3).any(|w| w == [0x3E, 0xFF, 0x00]));
            // Solo marker in each of the three solo arms, one group marker,
            // one scripted-loss latch (the no-game-over idiom).
            let solo = [0x50 | (MARKER_SOLO >> 8) as u8, (MARKER_SOLO & 0xFF) as u8];
            assert_eq!(branch.windows(2).filter(|w| *w == solo).count(), 3);
            let group = [
                0x50 | (MARKER_GROUP >> 8) as u8,
                (MARKER_GROUP & 0xFF) as u8,
            ];
            assert_eq!(branch.windows(2).filter(|w| *w == group).count(), 1);
            let loss = [
                0x50 | (SCRIPTED_LOSS_FLAG >> 8) as u8,
                (SCRIPTED_LOSS_FLAG & 0xFF) as u8,
            ];
            assert_eq!(branch.windows(2).filter(|w| *w == loss).count(), 1);
            // Party strips for each fighter choice.
            for strip in [
                [0x3D, 0x01, 0x3D, 0x02],
                [0x3D, 0x00, 0x3D, 0x02],
                [0x3D, 0x00, 0x3D, 0x01],
            ] {
                assert!(branch.windows(4).any(|w| w == strip));
            }
            // The retail quick-path skip tests (flags 0x559/0x558) before the
            // who-picker are NOPed - the enrollment menu always shows. The
            // arms' own copies of those tests (inside the refusal scenes,
            // after the picker) survive.
            let before = &rec[..open];
            for pat in [[0x75, 0x59], [0x75, 0x58]] {
                assert!(
                    !before.windows(2).any(|w| w == pat),
                    "who-menu skip test survived before the picker"
                );
            }
        }
    }
    assert!(found_picker, "patched MAN must carry the 4-option picker");
    // 3. The entry script opens with the guarded outcome block: both marker
    // tests, the party recompose, six alive-tests, the prize grants, and the
    // loss-path full restores.
    let p10 = dro + mf.partitions[1][0] as usize;
    let locals = man[p10] as usize;
    let blk = p10 + 1 + locals * 2 + 4;
    let test_solo = 0x70 | (MARKER_SOLO >> 8) as u8;
    let test_group = 0x70 | (MARKER_GROUP >> 8) as u8;
    assert_eq!(
        &man[blk..blk + 2],
        &[test_solo, (MARKER_SOLO & 0xFF) as u8],
        "entry script must open with the solo-marker test"
    );
    assert_eq!(
        &man[blk + 4..blk + 6],
        &[test_group, (MARKER_GROUP & 0xFF) as u8]
    );
    let window = &man[blk..blk + 96];
    assert!(
        window.windows(12).any(|w| w
            == [
                0x3D, 0x00, 0x3D, 0x01, 0x3D, 0x02, 0x3C, 0x00, 0x3C, 0x01, 0x3C, 0x02
            ]),
        "party recompose missing"
    );
    let outcome = [
        0x70 | (OUTCOME_SURVIVED_FLAG >> 8) as u8,
        (OUTCOME_SURVIVED_FLAG & 0xFF) as u8,
    ];
    assert_eq!(
        window.windows(2).filter(|w| *w == outcome).count(),
        2,
        "solo + group battle-outcome tests"
    );
    let give = [0x39, DEFAULT_REWARD_ITEM];
    assert_eq!(
        window.windows(2).filter(|w| *w == give).count(),
        3,
        "solo surplus + shared group Honey grants"
    );
    assert!(
        window
            .windows(9)
            .any(|w| w == [0x4C, 0x82, 0x00, 0x4C, 0x82, 0x01, 0x4C, 0x82, 0x02]),
        "loss-path full restore missing"
    );
}

/// Sector-level integrity: every 2352-byte sector of the patched image keeps a
/// valid Mode 2 sync + EDC/ECC where it was touched.
#[test]
fn delilas_challenge_keeps_touched_sectors_valid() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut patcher = DiscPatcher::open(disc.clone()).expect("open disc");
    apply::apply_delilas_challenge(&mut patcher).expect("apply");
    let patched = patcher.into_image();
    assert_eq!(patched.len(), disc.len());
    let mut touched = 0usize;
    for (i, (a, b)) in disc
        .chunks_exact(2352)
        .zip(patched.chunks_exact(2352))
        .enumerate()
    {
        if a != b {
            touched += 1;
            assert!(
                legaia_iso::write::mode2_form1_sector_is_valid(b),
                "sector {i} invalid after patch"
            );
        }
    }
    assert!(touched > 0, "patch must touch at least one sector");
    // The whole edit stays inside one PROT entry's footprint (176 sectors).
    assert!(touched <= 176, "patch leaked outside the koin1 entry");
}
