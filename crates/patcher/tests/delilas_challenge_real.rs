//! Disc round-trip oracle for the **Delilas Challenge** mod (the dome-course
//! architecture): the koin1 menu + warp half, and that it composes with the
//! companion arena code injection.
//!
//! Requires `LEGAIA_DISC_BIN`; skips (and passes) without it.

use legaia_patcher::apply;
use legaia_patcher::delilas_challenge::{
    ARENA_ENTER_BGM, COURSE_UNLOCK_FLAGS, DOME_ACTIVE_FLAG, DOME_WARP_OP, DelilasSites,
    KORU_DEFEATED_FLAG, NEVER_SET_FLAGS,
};
use legaia_patcher::delilas_dome::{
    ARENA_BASE_VA, ARENA_OVERLAY_PROT_INDEX, COURSE_FLAG, COURSE3_DESC_VA, ROSTER_VA, ROUTINE_VA,
    SEED_HOOK_VA, descriptor_bytes,
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

/// SET / CLEAR / TEST flag-op byte pairs (op high nibble | flag bits 8-11).
fn set_op(flag: u16) -> [u8; 2] {
    [0x50 | (flag >> 8) as u8, (flag & 0xFF) as u8]
}
fn clear_op(flag: u16) -> [u8; 2] {
    [0x60 | (flag >> 8) as u8, (flag & 0xFF) as u8]
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
    let report = apply::apply_delilas_challenge(&mut patcher, true).expect("apply");
    assert!(report.changed);
    assert!(report.dome_injected, "arena course must be injected too");
    assert!(report.grown_bytes > 0);
    assert!(report.compressed_len <= report.compressed_budget);
    let patched = patcher.into_image();

    // Same-size image; every touched sector still EDC/ECC-valid (DiscPatcher
    // re-encodes on write; re-opening validates the sector forms).
    assert_eq!(patched.len(), disc.len(), "image size must not change");
    let reopen = DiscPatcher::open(patched.clone()).expect("patched image re-parses");

    // --- The companion arena injection landed. ---
    let overlay = reopen
        .read_entry(ARENA_OVERLAY_PROT_INDEX)
        .expect("read arena overlay");
    // Seed hook is now `j ROUTINE_VA` (a jump into the SCUS cave).
    let seed_off = (SEED_HOOK_VA - ARENA_BASE_VA) as usize;
    let seed = u32::from_le_bytes(overlay[seed_off..seed_off + 4].try_into().unwrap());
    assert_eq!(seed >> 26, 0x02, "seed hook detours with a `j`");
    // Course-3 descriptor {round_count=2, roster_ptr} sits at 0x801D1A20.
    let desc_off = (COURSE3_DESC_VA - ARENA_BASE_VA) as usize;
    assert_eq!(
        &overlay[desc_off..desc_off + 8],
        &descriptor_bytes(),
        "course-3 descriptor must be installed"
    );
    // The SCUS cave now carries the routine + roster (no longer all-zero).
    let scus = reopen
        .read_named_file("SCUS_942.54")
        .expect("read SCUS from patched image");
    let routine_off = legaia_asset::item_names::file_offset_for_va(&scus, ROUTINE_VA)
        .expect("resolve routine VA");
    assert!(
        scus[routine_off..routine_off + 8].iter().any(|&b| b != 0),
        "seed routine must be present in the cave"
    );
    let roster_off =
        legaia_asset::item_names::file_offset_for_va(&scus, ROSTER_VA).expect("resolve roster VA");
    assert!(
        scus[roster_off..roster_off + 16].iter().any(|&b| b != 0),
        "Delilas roster must be present in the cave"
    );

    // The Magic-reject masks are widened to 0x300 on the patched image.
    {
        use legaia_patcher::delilas_dome::{
            BATTLE_BASE_VA, BATTLE_OVERLAY_PROT_INDEX, MAGIC_REJECT_NEW, MAGIC_REJECT_SITES,
        };
        let battle = reopen
            .read_entry(BATTLE_OVERLAY_PROT_INDEX)
            .expect("read battle overlay from patched image");
        for va in MAGIC_REJECT_SITES {
            let off = (va - BATTLE_BASE_VA) as usize;
            let got = u32::from_le_bytes(battle[off..off + 4].try_into().unwrap());
            assert_eq!(
                got, MAGIC_REJECT_NEW,
                "widened magic-reject mask at {va:#x}"
            );
        }
    }

    // The patched image locates as applied, and a second apply is a no-op.
    let after = koin1_sites(patched.clone());
    assert!(after.already_applied, "patched disc must read as applied");
    let mut patcher2 = DiscPatcher::open(patched.clone()).expect("re-open");
    let report2 = apply::apply_delilas_challenge(&mut patcher2, true).expect("re-apply");
    assert!(!report2.changed, "second apply must be a no-op");
    assert!(
        !report2.dome_injected,
        "arena injection must be idempotent on a patched image"
    );
    let repatched = patcher2.into_image();
    assert_eq!(repatched, patched, "no-op re-apply must not touch bytes");

    // Determinism: applying to a fresh copy of the original produces the
    // identical image.
    let mut patcher3 = DiscPatcher::open(disc.clone()).expect("open disc again");
    apply::apply_delilas_challenge(&mut patcher3, true).expect("apply again");
    assert_eq!(
        patcher3.into_image(),
        patched,
        "apply must be deterministic"
    );

    // Composes with the sibling koin1-MAN feature (Earth Egg price edit):
    // both rewrite PROT 543's MAN, in either order.
    let mut ab = DiscPatcher::open(disc.clone()).expect("open");
    apply::apply_delilas_challenge(&mut ab, true).expect("delilas first");
    apply::set_earth_egg_price(&mut ab, 500).expect("earth egg second");
    let ab = ab.into_image();
    let mut ba = DiscPatcher::open(disc).expect("open");
    apply::set_earth_egg_price(&mut ba, 500).expect("earth egg first");
    apply::apply_delilas_challenge(&mut ba, true).expect("delilas second");
    let ba = ba.into_image();
    assert!(koin1_sites(ab).already_applied);
    assert!(koin1_sites(ba).already_applied);

    // --- Decode-level assertions on the patched koin1 MAN. ---
    let man = &after.decoded;
    let mf = legaia_asset::man_section::parse(man).expect("patched MAN parses");
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
            // Option 3 targets the branch; the branch gate tests the Koru flag.
            let t = p.jump_target(3).expect("4th option target");
            assert!(t < rec.len());
            let gate_op = 0x70 | (KORU_DEFEATED_FLAG >> 8) as u8;
            assert_eq!(rec[t], gate_op, "branch must open with the Koru test");
            assert_eq!(rec[t + 1], (KORU_DEFEATED_FLAG & 0xFF) as u8);
            let branch = &rec[t..];
            // A WARP, not a battle: no scripted-battle op survives.
            assert!(
                !branch.windows(3).any(|w| w == [0x3E, 0xFF, 0x00]),
                "warp branch must not launch a scripted battle"
            );
            // It mirrors a retail difficulty arm: dome-active set, the three
            // course-unlock flags cleared, the course-3 request set, and the
            // verbatim BGM + arena warp present.
            assert!(branch.windows(2).any(|w| w == set_op(DOME_ACTIVE_FLAG)));
            for &f in &COURSE_UNLOCK_FLAGS {
                assert!(
                    branch.windows(2).any(|w| w == clear_op(f)),
                    "course-unlock flag {f:#x} not cleared"
                );
            }
            assert!(
                branch.windows(2).any(|w| w == set_op(COURSE_FLAG)),
                "branch must request the Delilas course (flag {COURSE_FLAG:#x})"
            );
            assert!(branch.windows(7).any(|w| w == ARENA_ENTER_BGM));
            assert!(
                branch.windows(6).any(|w| w == DOME_WARP_OP),
                "branch must carry the arena warp op"
            );
            // The confirm picker sits between the gate and the warp (its
            // labels are inline text right after the two jump entries).
            let confirm_label = b"Bring them on!";
            assert!(
                branch
                    .windows(confirm_label.len())
                    .any(|w| w == confirm_label),
                "confirm picker label missing from the branch"
            );
            // The retail quick-path skip tests (flags 0x559/0x558) before the
            // who-picker are retargeted at never-set flags (same op shape) -
            // the enrollment menu always shows, and no 0x21 yield-fill exists
            // (the fill broke the clerk dialog mid-interaction live).
            let before = &rec[..open];
            for pat in [[0x75, 0x59], [0x75, 0x58]] {
                assert!(
                    !before.windows(2).any(|w| w == pat),
                    "who-menu skip test survived before the picker"
                );
            }
            for &flag in &NEVER_SET_FLAGS {
                let retargeted = [0x70 | (flag >> 8) as u8, (flag & 0xFF) as u8];
                assert!(
                    before.windows(2).any(|w| w == retargeted),
                    "retargeted skip test {flag:#x} missing before the picker"
                );
            }
            assert!(
                !before.windows(4).any(|w| w == [0x21, 0x21, 0x21, 0x21]),
                "0x21 yield-fill run must not exist before the picker"
            );
        }
    }
    assert!(found_picker, "patched MAN must carry the 4-option picker");
}

/// Sector-level integrity: every 2352-byte sector of the patched image keeps a
/// valid Mode 2 sync + EDC/ECC where it was touched. The dome architecture
/// spans three regions (koin1 MAN, the arena overlay, the SCUS cave), so the
/// patch legitimately touches sectors in more than one PROT entry.
#[test]
fn delilas_challenge_keeps_touched_sectors_valid() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut patcher = DiscPatcher::open(disc.clone()).expect("open disc");
    apply::apply_delilas_challenge(&mut patcher, true).expect("apply");
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
}

/// The Honey fallback: applying the challenge with `custom_items = false`
/// installs the same arena grant hook but a Honey-only grant cave, and
/// leaves every custom-items-specific site (item records, jump tables,
/// battle-overlay hooks) at its retail bytes.
#[test]
fn delilas_challenge_honey_fallback_skips_the_custom_items() {
    use legaia_patcher::custom_items::{
        APPLY_DEFAULT_ARM, APPLY_JT_VA, ELIXIR_CLASS, GRANT_HOOK_VA, GRANT_VA, HONEY_ITEM_ID,
        SEED_HOOK_ORIG, SEED_HOOK_VA as ITEM_SEED_HOOK_VA, assemble_grant_routine_for,
    };
    use legaia_patcher::delilas_dome::{BATTLE_BASE_VA, BATTLE_OVERLAY_PROT_INDEX};

    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };

    let mut patcher = DiscPatcher::open(disc.clone()).expect("open disc");
    let report = apply::apply_delilas_challenge(&mut patcher, false).expect("apply honey variant");
    assert!(report.changed);
    assert!(report.dome_injected);
    let patched = patcher.into_image();
    assert_eq!(patched.len(), disc.len());
    let reopen = DiscPatcher::open(patched.clone()).expect("patched image re-parses");

    // The grant cave carries exactly the Honey-only routine.
    let scus = reopen.read_named_file("SCUS_942.54").expect("read SCUS");
    let grant_off =
        legaia_asset::item_names::file_offset_for_va(&scus, GRANT_VA).expect("resolve grant VA");
    let want: Vec<u8> = assemble_grant_routine_for(&[HONEY_ITEM_ID])
        .iter()
        .flat_map(|w| w.to_le_bytes())
        .collect();
    assert_eq!(
        &scus[grant_off..grant_off + want.len()],
        &want[..],
        "grant cave must hold the Honey grant"
    );

    // The arena settle hook detours into the grant cave.
    let overlay = reopen
        .read_entry(ARENA_OVERLAY_PROT_INDEX)
        .expect("read arena overlay");
    let hook_off = (GRANT_HOOK_VA - ARENA_BASE_VA) as usize;
    let hook = u32::from_le_bytes(overlay[hook_off..hook_off + 4].try_into().unwrap());
    assert_eq!(
        hook,
        0x0800_0000 | ((GRANT_VA >> 2) & 0x03FF_FFFF),
        "j GRANT_VA"
    );

    // Custom-items sites stay retail: the applier jump-table words and the
    // battle-overlay seed-dispatch hook.
    let jt_off =
        legaia_asset::item_names::file_offset_for_va(&scus, APPLY_JT_VA + ELIXIR_CLASS as u32 * 4)
            .expect("resolve applier JT");
    for k in 0..2 {
        let got = u32::from_le_bytes(scus[jt_off + k * 4..jt_off + k * 4 + 4].try_into().unwrap());
        assert_eq!(got, APPLY_DEFAULT_ARM, "applier JT must stay retail");
    }
    let battle = reopen
        .read_entry(BATTLE_OVERLAY_PROT_INDEX)
        .expect("read battle overlay");
    let seed_off = (ITEM_SEED_HOOK_VA - BATTLE_BASE_VA) as usize;
    let seed = u32::from_le_bytes(battle[seed_off..seed_off + 4].try_into().unwrap());
    assert_eq!(
        seed, SEED_HOOK_ORIG,
        "item seed-dispatch hook must stay retail"
    );

    // Idempotent, and deterministic on a fresh copy.
    let mut patcher2 = DiscPatcher::open(patched.clone()).expect("re-open");
    let report2 = apply::apply_delilas_challenge(&mut patcher2, false).expect("re-apply");
    assert!(!report2.changed);
    assert_eq!(
        patcher2.into_image(),
        patched,
        "no-op re-apply must not touch bytes"
    );
}
