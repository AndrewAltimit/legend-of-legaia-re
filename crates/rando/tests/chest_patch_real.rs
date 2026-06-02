//! Disc-gated end-to-end test for the chest randomizer: shuffle every chest's
//! item id on a scratch copy, then re-decode each patched scene MAN off the disc
//! and confirm the edit is faithful — the give-item site offsets are unchanged,
//! the global chest-item multiset is preserved (shuffle), sectors stay
//! EDC/ECC-valid, and a fixed seed is byte-deterministic. Skips without
//! `LEGAIA_DISC_BIN`.

use legaia_iso::iso9660::find_file_in_image;
use legaia_iso::raw::{SECTOR_SIZE, USER_DATA_OFFSET, USER_DATA_SIZE};
use legaia_rando::apply;
use legaia_rando::chest::SceneChests;
use legaia_rando::disc::DiscPatcher;
use legaia_rando::drops::DropMode;

fn load_disc() -> Option<Vec<u8>> {
    let p = std::path::PathBuf::from(std::env::var_os("LEGAIA_DISC_BIN")?);
    p.is_file().then(|| std::fs::read(&p).ok()).flatten()
}

/// (scene idx, site offsets, current items) for every scene with chest sites.
fn snapshot(patcher: &DiscPatcher) -> Vec<(usize, Vec<usize>, Vec<u8>)> {
    let mut out = Vec::new();
    for idx in 0..patcher.entry_count() {
        let Ok(entry) = patcher.read_entry(idx) else {
            continue;
        };
        if let Some(sc) = SceneChests::locate(&entry, idx) {
            out.push((idx, sc.sites.clone(), sc.current_items()));
        }
    }
    out
}

#[test]
fn shuffle_chests_round_trips_on_disc() {
    let Some(original) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let seed = 0xC0FFEE_u64;

    let before = snapshot(&DiscPatcher::open(original.clone()).unwrap());
    let total_sites: usize = before.iter().map(|(_, s, _)| s.len()).sum();
    assert!(total_sites > 0, "expected chest give-item sites");

    // The walk skips inline dialogue and reaches post-announcement give-item
    // ops, so it must recover far more than the old first-dialogue-stops lower
    // bound (38). A floor well above that catches a regression to that bug.
    assert!(
        total_sites > 150,
        "expected the dialogue-skipping walk to find many post-text give sites, got {total_sites}"
    );

    // Ground truth from the keikoku (Ravine, PROT entry 112) chest savestate
    // pair: 4 chest give-item ops; the chest the player opened gives Phoenix
    // (item 0x80). The old walk found ZERO sites in this scene because every
    // chest record opens with its announcement dialogue.
    let keikoku = before
        .iter()
        .find(|(idx, _, _)| *idx == 112)
        .expect("keikoku (entry 112) must have chest give-item sites");
    assert_eq!(keikoku.1.len(), 4, "keikoku has 4 chests");
    assert!(
        keikoku.2.contains(&0x80),
        "keikoku chest set includes Phoenix (0x80); got {:02x?}",
        keikoku.2
    );

    let mut patcher = DiscPatcher::open(original.clone()).unwrap();
    // Empty keep-static set: this test asserts the *global* multiset is preserved,
    // which only holds when every site participates in the shuffle.
    let no_static = std::collections::BTreeSet::<u8>::new();
    let report = apply::randomize_chests(&mut patcher, &[], seed, DropMode::Shuffle, &no_static)
        .expect("randomize");
    assert_eq!(report.sites_total, total_sites);
    assert!(report.items_changed > 0);

    let after = snapshot(&patcher);

    // Same scenes, same site offsets (only operand bytes changed; widths intact).
    assert_eq!(before.len(), after.len(), "scene set changed");
    for ((bi, bsites, _), (ai, asites, _)) in before.iter().zip(&after) {
        assert_eq!(bi, ai, "scene order changed");
        assert_eq!(bsites, asites, "chest site offsets changed in scene {bi}");
    }

    // Global multiset of chest items preserved (minus skipped scenes).
    let skipped: std::collections::HashSet<usize> = report.skipped.iter().copied().collect();
    let mut mb: Vec<u8> = before
        .iter()
        .filter(|(i, _, _)| !skipped.contains(i))
        .flat_map(|(_, _, items)| items.clone())
        .collect();
    let mut ma: Vec<u8> = after
        .iter()
        .filter(|(i, _, _)| !skipped.contains(i))
        .flat_map(|(_, _, items)| items.clone())
        .collect();
    mb.sort_unstable();
    ma.sort_unstable();
    assert_eq!(mb, ma, "shuffle must preserve the chest-item multiset");

    // A patched scene's first PROT.DAT sector stays EDC/ECC-valid.
    let changed = after
        .iter()
        .map(|(i, _, _)| *i)
        .find(|i| !skipped.contains(i))
        .unwrap();
    let img = patcher.image();
    let (prot_lba, prot_size) = find_file_in_image(img, "PROT.DAT").unwrap();
    let psectors = (prot_size as usize).div_ceil(USER_DATA_SIZE);
    let mut payload = Vec::with_capacity(psectors * USER_DATA_SIZE);
    for i in 0..psectors {
        let b = (prot_lba as usize + i) * SECTOR_SIZE + USER_DATA_OFFSET;
        payload.extend_from_slice(&img[b..b + USER_DATA_SIZE]);
    }
    payload.truncate(prot_size as usize);
    let archive = legaia_prot::archive::Archive::from_bytes(payload).unwrap();
    let lba = archive.entries[changed].start_lba;
    let sb = (prot_lba as u64 + lba as u64) as usize * SECTOR_SIZE;
    assert!(
        legaia_iso::write::mode2_form1_sector_is_valid(&img[sb..sb + SECTOR_SIZE]),
        "patched chest scene {changed} sector must be EDC/ECC-valid"
    );

    // Determinism.
    let mut p2 = DiscPatcher::open(original.clone()).unwrap();
    let r2 = apply::randomize_chests(&mut p2, &[], seed, DropMode::Shuffle, &no_static).unwrap();
    assert_eq!(r2.skipped, report.skipped);
    assert!(
        p2.image() == patcher.image(),
        "same seed -> identical image"
    );

    eprintln!(
        "chests shuffle seed {seed:#x}: {} sites, {} changed, {} scenes, {} skipped",
        report.sites_total,
        report.items_changed,
        report.scenes_changed,
        report.skipped.len()
    );
}

/// A chest's announcement text ("There is a {item}…" / "{name} now has the
/// {item}!") renders the item name from a separate `0xC2 <id>` dialogue token,
/// distinct from the `0x39` give operand. The randomizer must rewrite both so the
/// flavor text names the item it actually grants — otherwise a patched chest
/// gives the new item but still *reads* as the old one. Pinned on keikoku's
/// Phoenix chest: change it and re-decode off the patched image; the give operand
/// and every item-name token in that record must both carry the new id.
#[test]
fn chest_display_tokens_track_the_give_item() {
    let Some(original) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    // keikoku (entry 112) Phoenix (0x80) chest -> a distinct, named item.
    const PHOENIX: u8 = 0x80;
    const NEW_ID: u8 = 0x8e; // Wonder Elixir

    let mut patcher = DiscPatcher::open(original).unwrap();
    let mut sc = SceneChests::locate(&patcher.read_entry(112).unwrap(), 112).unwrap();
    let k = sc
        .current_items()
        .iter()
        .position(|&b| b == PHOENIX)
        .expect("keikoku has the Phoenix chest");
    // The Phoenix chest names its item in dialogue (announcement + "now has").
    assert!(
        !sc.display_tokens[k].is_empty(),
        "the Phoenix chest record carries item-name display tokens"
    );
    let token_count = sc.display_tokens[k].len();

    sc.set_site(k, NEW_ID);
    // In-memory: operand + every display token updated together.
    assert_eq!(sc.decoded[sc.sites[k]], NEW_ID, "give operand updated");
    for &t in &sc.display_tokens[k] {
        assert_eq!(
            sc.decoded[t], NEW_ID,
            "item-name display token must match the give"
        );
    }

    let stream = sc.repack().expect("keikoku MAN re-packs");
    patcher
        .patch_prot_entry(112, sc.man_offset as u64, &stream)
        .unwrap();

    // Re-decode off the patched image: text == grant on disc, not just in memory.
    let after = SceneChests::locate(&patcher.read_entry(112).unwrap(), 112).unwrap();
    let ak = after
        .current_items()
        .iter()
        .position(|&b| b == NEW_ID)
        .expect("patched chest grants the new id");
    assert_eq!(
        after.display_tokens[ak].len(),
        token_count,
        "the same item-name tokens are recovered after patching"
    );
    for &t in &after.display_tokens[ak] {
        assert_eq!(
            after.decoded[t], NEW_ID,
            "on-disc announcement token names the granted item"
        );
    }
    // No stray Phoenix id left in this chest's operand or its display tokens.
    assert_ne!(after.decoded[after.sites[ak]], PHOENIX);
    eprintln!("keikoku Phoenix chest: give + {token_count} display token(s) all -> 0x{NEW_ID:02x}");
}

/// The curated keep-static set is honored: every chest whose original item is a
/// static id keeps that exact item at its exact site, and no static item ever
/// migrates into another chest. Uses the default quest / key-item set.
#[test]
fn keep_static_items_never_move() {
    let Some(original) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let seed = 0x5EED_u64;
    let keep_static: std::collections::BTreeSet<u8> =
        legaia_rando::items::DEFAULT_STATIC_CHEST_ITEMS
            .iter()
            .copied()
            .collect();

    let before = snapshot(&DiscPatcher::open(original.clone()).unwrap());

    // The static set must actually be present in the corpus, or the test is
    // vacuous. Map each static id -> the set of (scene, offset) sites holding it.
    let static_sites = |snap: &[(usize, Vec<usize>, Vec<u8>)]| {
        let mut m: std::collections::BTreeMap<u8, std::collections::BTreeSet<(usize, usize)>> =
            std::collections::BTreeMap::new();
        for (idx, offs, items) in snap {
            for (o, it) in offs.iter().zip(items) {
                if keep_static.contains(it) {
                    m.entry(*it).or_default().insert((*idx, *o));
                }
            }
        }
        m
    };
    let before_static = static_sites(&before);
    assert!(
        !before_static.is_empty(),
        "expected at least one curated static item in the chest corpus"
    );
    let before_static_count: usize = before_static.values().map(|s| s.len()).sum();

    let mut patcher = DiscPatcher::open(original.clone()).unwrap();
    apply::randomize_chests(&mut patcher, &[], seed, DropMode::Shuffle, &keep_static)
        .expect("randomize");
    let after = snapshot(&patcher);
    let after_static = static_sites(&after);

    // Each static id occupies exactly the same sites before and after — it never
    // moved, vanished, or appeared anywhere new.
    assert_eq!(
        before_static, after_static,
        "static items must stay at their original chest sites and nowhere else"
    );

    // And the on-disc item byte at each static site is unchanged.
    let after_by_idx: std::collections::BTreeMap<usize, (&Vec<usize>, &Vec<u8>)> =
        after.iter().map(|(i, o, it)| (*i, (o, it))).collect();
    for (idx, offs, items) in &before {
        let (aoffs, aitems) = after_by_idx[idx];
        assert_eq!(offs, aoffs, "site offsets must be stable");
        for (k, it) in items.iter().enumerate() {
            if keep_static.contains(it) {
                assert_eq!(aitems[k], *it, "static chest item must be unchanged");
            }
        }
    }
    eprintln!(
        "keep-static honored: {} static-item chest(s) ({} distinct id) unchanged after shuffle",
        before_static_count,
        before_static.len()
    );
}
