//! Disc-gated end-to-end test for the "limit strong fights to a solo enemy"
//! encounter option. On a scratch copy of the disc it runs a World-scope random
//! encounter pass (the mode most likely to drop a late-game heavy hitter into an
//! early area) twice — once with the solo-strong option off, once on — then
//! re-decodes every patched scene straight off the image and asserts:
//!
//! - **off:** at least one random formation ends up a *pack* (count >= 2) that
//!   holds a monster much stronger than that area's native average — i.e. the
//!   problem the option exists to fix actually occurs (non-vacuous).
//! - **on:** *no* multi-monster random formation holds such a monster anymore —
//!   every strong pack was collapsed to a lone enemy — and the run reports the
//!   collapses, the patched sectors stay EDC/ECC-valid, and a fixed seed is
//!   byte-deterministic.
//!
//! Skips + passes without `LEGAIA_DISC_BIN`.

use std::collections::HashMap;

use legaia_iso::iso9660::find_file_in_image;
use legaia_iso::raw::{SECTOR_SIZE, USER_DATA_OFFSET, USER_DATA_SIZE};
use legaia_rando::apply::{self, EncounterScope, SoloStrongConfig};
use legaia_rando::disc::DiscPatcher;
use legaia_rando::drops::DropMode;
use legaia_rando::encounter::{MonsterPowerTable, SceneEncounters};

fn load_disc() -> Option<Vec<u8>> {
    let p = std::path::PathBuf::from(std::env::var_os("LEGAIA_DISC_BIN")?);
    p.is_file().then(|| std::fs::read(&p).ok()).flatten()
}

/// The native combat-power baseline of every locatable scene, keyed by PROT entry
/// index — read from the *original* disc, exactly as the apply path does.
fn native_baselines(patcher: &DiscPatcher, table: &MonsterPowerTable) -> HashMap<usize, u32> {
    let mut out = HashMap::new();
    for idx in 0..patcher.entry_count() {
        if let Some(scene) = patcher
            .read_entry(idx)
            .ok()
            .and_then(|e| SceneEncounters::locate(&e, idx))
            && let Some(b) = scene.baseline_power(table)
        {
            out.insert(idx, b);
        }
    }
    out
}

/// Count the random formations (across all scenes) that are a *pack* (>= 2
/// monsters) and hold at least one monster whose combat power clears
/// `threshold_pct`% of that scene's native baseline — the "strong pack" the
/// option targets. `skip` are PROT entries to ignore (re-pack failures).
fn strong_pack_count(
    patcher: &DiscPatcher,
    table: &MonsterPowerTable,
    baselines: &HashMap<usize, u32>,
    threshold_pct: u16,
    skip: &[usize],
) -> usize {
    let mut n = 0;
    for (&idx, &baseline) in baselines {
        if skip.contains(&idx) {
            continue;
        }
        let Some(scene) = patcher
            .read_entry(idx)
            .ok()
            .and_then(|e| SceneEncounters::locate(&e, idx))
        else {
            continue;
        };
        let threshold = (baseline as u64 * threshold_pct as u64) / 100;
        for i in 0..scene.formation_count() {
            if !scene.is_random_formation(i) {
                continue;
            }
            let ids = scene.formation_ids(i);
            if ids.len() < 2 {
                continue;
            }
            if ids.iter().any(|&id| table.power_of(id) as u64 >= threshold) {
                n += 1;
            }
        }
    }
    n
}

#[test]
fn solo_strong_collapses_strong_packs_on_disc() {
    let Some(original) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let seed = 0x50_4C_4F_53_5F_31; // "SOLO_1"
    let pct = apply::DEFAULT_SOLO_STRONG_THRESHOLD_PCT;

    // The power table + native baselines come from the untouched disc.
    let base = DiscPatcher::open(original.clone()).expect("open");
    let table = apply::monster_power_table(&base).expect("power table");
    let baselines = native_baselines(&base, &table);
    assert!(
        baselines.len() > 40,
        "expected many scenes with random encounters, found {}",
        baselines.len()
    );

    // Pass A: World random WITHOUT the solo option. Some strong packs survive.
    let mut off = DiscPatcher::open(original.clone()).expect("open");
    let rep_off = apply::randomize_encounters_full(
        &mut off,
        seed,
        DropMode::Random,
        EncounterScope::World,
        &[],
        None,
    )
    .expect("randomize (solo off)");
    let strong_off = strong_pack_count(&off, &table, &baselines, pct, &rep_off.skipped);
    assert!(
        strong_off > 0,
        "World-scope random should produce at least one strong pack (else the test is vacuous)"
    );
    assert_eq!(rep_off.solo_collapsed, 0, "solo off must not collapse");

    // Pass B: same seed, solo option ON. Every strong pack is now a lone enemy.
    let mut on = DiscPatcher::open(original.clone()).expect("open");
    let rep_on = apply::randomize_encounters_full(
        &mut on,
        seed,
        DropMode::Random,
        EncounterScope::World,
        &[],
        Some(SoloStrongConfig { threshold_pct: pct }),
    )
    .expect("randomize (solo on)");
    assert!(
        rep_on.solo_collapsed > 0,
        "solo on should collapse at least one strong pack"
    );
    let strong_on = strong_pack_count(&on, &table, &baselines, pct, &rep_on.skipped);
    assert_eq!(
        strong_on, 0,
        "no multi-monster random formation may keep a monster >= {pct}% of its area's average"
    );

    // A patched scene's PROT.DAT sectors stay EDC/ECC-valid.
    let img = on.image();
    let (prot_lba, prot_size) = find_file_in_image(img, "PROT.DAT").unwrap();
    let psize = prot_size as usize;
    let psectors = psize.div_ceil(USER_DATA_SIZE);
    let mut payload = Vec::with_capacity(psectors * USER_DATA_SIZE);
    for i in 0..psectors {
        let b = (prot_lba as usize + i) * SECTOR_SIZE + USER_DATA_OFFSET;
        payload.extend_from_slice(&img[b..b + USER_DATA_SIZE]);
    }
    payload.truncate(psize);
    let archive = legaia_prot::archive::Archive::from_bytes(payload).unwrap();
    let changed_idx = *baselines
        .keys()
        .find(|i| !rep_on.skipped.contains(i))
        .unwrap();
    let disc_sector = prot_lba as u64 + archive.entries[changed_idx].start_lba as u64;
    let sb = disc_sector as usize * SECTOR_SIZE;
    assert!(
        legaia_iso::write::mode2_form1_sector_is_valid(&img[sb..sb + SECTOR_SIZE]),
        "patched scene {changed_idx} first sector must be EDC/ECC-valid"
    );

    // Determinism: same seed -> byte-identical patched image.
    let mut on2 = DiscPatcher::open(original.clone()).expect("open");
    let rep_on2 = apply::randomize_encounters_full(
        &mut on2,
        seed,
        DropMode::Random,
        EncounterScope::World,
        &[],
        Some(SoloStrongConfig { threshold_pct: pct }),
    )
    .expect("randomize (solo on, repeat)");
    assert_eq!(rep_on2.solo_collapsed, rep_on.solo_collapsed);
    assert!(
        on2.image() == on.image(),
        "same seed must reproduce the patched image"
    );

    eprintln!(
        "solo-strong seed {seed:#x}: {} strong packs before, {} after; {} formations collapsed",
        strong_off, strong_on, rep_on.solo_collapsed
    );
}
