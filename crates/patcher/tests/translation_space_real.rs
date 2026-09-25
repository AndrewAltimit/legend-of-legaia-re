//! Disc-gated agreement oracle for the translation space report
//! (`legaia_patcher::translation::space`).
//!
//! The report promises that every number is the importer's. This test holds
//! it to that: it builds a synthetic pack that exercises every outcome -
//! names that fit, names that move, names with no free run, a pinned name over
//! its room, monster names in place / grown / refused, a scene scrambled past
//! its footprint (rollbacks), a scene with longer lines (the relocator), a
//! streaming dungeon line that grows into its sector slack, fixed strings in
//! and over their room, an unencodable line - then checks the report's
//! predicted outcome for **every** filled key against what `import_pack`
//! actually does on a fresh copy of the disc, the per-scene rolled-back sets
//! against the import's rollback diagnostics, and the fast paths
//! (`scene_fit`, `NameFitter`) against the full report.
//!
//! The synthetic text is generated (letters and lengths), never the game's;
//! nothing prints or asserts on disc text. Skips + passes without
//! `LEGAIA_DISC_BIN`.

use std::collections::{BTreeMap, BTreeSet};

use legaia_patcher::disc::DiscPatcher;
use legaia_patcher::rng::SplitMix64;
use legaia_patcher::translation::space::{
    self, NameFitter, Outcome, RoomKind, SpaceOptions, SpaceReport,
};
use legaia_patcher::translation::{
    IssueKind, LanguagePack, WritePath, export_pack, import_pack, import_pack_relayout,
};

fn load_disc() -> Option<Vec<u8>> {
    let p = std::path::PathBuf::from(std::env::var_os("LEGAIA_DISC_BIN")?);
    p.is_file().then(|| std::fs::read(&p).ok()).flatten()
}

/// `n` generated letters (and spaces, when `spaces`), hard to compress.
fn noise(rng: &mut SplitMix64, n: usize, spaces: bool) -> String {
    const L: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ";
    (0..n)
        .map(|i| {
            if spaces && i % 6 == 5 {
                ' '
            } else {
                L[rng.below(L.len())] as char
            }
        })
        .collect()
}

/// Every filled key of `pack`.
fn filled(pack: &LanguagePack) -> BTreeSet<String> {
    pack.sections
        .iter()
        .flat_map(|(_, es)| es)
        .filter(|e| e.is_filled())
        .map(|e| e.key.clone())
        .collect()
}

/// A pack that reaches every outcome the report names.
fn mixed_pack(src: &DiscPatcher, disc_only: &SpaceReport) -> LanguagePack {
    let mut pack = export_pack(src).expect("export");
    let mut rng = SplitMix64::new(0x5A_CE);
    let room: BTreeMap<&str, (usize, RoomKind, Option<usize>)> = disc_only
        .entries
        .iter()
        .map(|e| (e.key.as_str(), (e.room, e.room_kind, e.english_len)))
        .collect();
    let s = &mut pack.sections;

    // Names: every movable one length-shuffled around English (-5..=+3, so a
    // third grow and move, paid for by the ones that shrink), two far too
    // long for any free run, one pinned name over its room.
    let (mut i, mut huge, mut pinned) = (0usize, 0usize, 0usize);
    for entries in [
        &mut s.items,
        &mut s.item_types,
        &mut s.spells,
        &mut s.arts,
        &mut s.accessory_passives,
    ] {
        for e in entries.iter_mut() {
            let Some(&(r, kind, Some(eng))) = room.get(e.key.as_str()) else {
                continue;
            };
            match kind {
                RoomKind::NameMovable if huge < 2 && i % 97 == 50 => {
                    e.translation = noise(&mut rng, 3000, false);
                    huge += 1;
                }
                RoomKind::NameMovable => {
                    let d = (i * 7 % 9) as isize - 5;
                    let n = (eng as isize + d).max(1) as usize;
                    e.translation = noise(&mut rng, n, false);
                }
                RoomKind::StringFixed if pinned < 1 => {
                    e.translation = noise(&mut rng, r + 3, false);
                    pinned += 1;
                }
                _ => continue,
            }
            i += 1;
        }
    }

    // Monsters: in place, grown to the cap, and one past it.
    let mons: BTreeMap<String, (usize, usize)> = disc_only
        .monsters
        .iter()
        .map(|m| (m.key.clone(), (m.room, m.cap)))
        .collect();
    for (j, e) in s.monster_names.iter_mut().enumerate() {
        let Some(&(r, cap)) = mons.get(&e.key) else {
            continue;
        };
        e.translation = match j % 4 {
            0 => noise(&mut rng, r.min(5), false),
            1 if r < cap => noise(&mut rng, cap, false),
            2 => noise(&mut rng, 16, false),
            _ => continue,
        };
    }

    // Fixed strings: shorter, and over the room.
    for (j, e) in s.ui_menu.iter_mut().take(6).enumerate() {
        let r = room[e.key.as_str()].0;
        e.translation = noise(&mut rng, if j % 2 == 0 { r.min(2) } else { r + 2 }, false);
    }
    if let Some(e) = s.party_names.get_mut(0) {
        e.translation = noise(&mut rng, 10, false);
    }
    if let Some(e) = s.party_names.get_mut(1) {
        e.translation = noise(&mut rng, 3, false);
    }
    if let Some(e) = s.place_names.get_mut(0) {
        e.translation = "Plage \u{00e9}t\u{00e9}".to_string();
    }

    // Dialog. A mid-sized scene scrambled at English lengths (it no longer
    // recompresses: rollbacks); in three smaller scenes every walked line
    // shortened to a repetitive run and one line grown past its span (the
    // relocator carries the scene at exact lengths); one streaming dungeon
    // line grown into its sector slack.
    let mut by_scene: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for (j, e) in s.scene_dialog.iter().enumerate() {
        if let Some(p) = e.key.split(':').nth(1).and_then(|p| p.parse().ok()) {
            by_scene.entry(p).or_default().push(j);
        }
    }
    let mut scenes: Vec<(&usize, &Vec<usize>)> = by_scene
        .iter()
        .filter(|(_, v)| (30..=150).contains(&v.len()))
        .collect();
    scenes.sort_by_key(|(p, v)| (std::cmp::Reverse(v.len()), **p));
    assert!(scenes.len() >= 4, "mid-sized scenes: {}", scenes.len());
    for &j in scenes[0].1 {
        let e = &mut s.scene_dialog[j];
        e.translation = noise(&mut rng, e.budget, true);
    }
    for (_, lines) in scenes.iter().skip(1).take(3) {
        let mut grew = false;
        for &j in lines.iter() {
            let e = &mut s.scene_dialog[j];
            if room[e.key.as_str()].1 != RoomKind::DialogGrowable || e.budget < 4 {
                continue;
            }
            let n = if grew { e.budget - 2 } else { e.budget + 6 };
            grew = true;
            e.translation = "ab".repeat(n).chars().take(n).collect();
        }
    }
    if let Some(e) = s.inline_text.iter_mut().find(|e| {
        room.get(e.key.as_str())
            .is_some_and(|r| r.1 == RoomKind::DialogGrowable)
    }) {
        e.translation = noise(&mut rng, e.budget + 4, true);
    }
    pack
}

#[test]
fn disc_only_report_is_not_vacuous() {
    let Some(original) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let src = DiscPatcher::open(original).expect("open disc");
    let r = space::space_report(&src, None, SpaceOptions::default()).expect("report");
    let english = export_pack(&src).expect("export");

    assert_eq!(r.entries.len(), english.sections.total(), "one row per key");
    assert!(
        r.entries
            .iter()
            .all(|e| e.outcome.is_none() && e.pack_len.is_none())
    );
    let movable = r.names.iter().filter(|n| n.movable).count();
    assert!(movable > 600, "movable names: {movable}");
    assert!(r.names.iter().any(|n| !n.movable), "some names are pinned");
    assert!(r.names.iter().all(|n| n.movable == n.pin.is_none()));
    assert!(!r.name_regions.is_empty());
    for g in &r.name_regions {
        assert_eq!(g.english_used + g.english_free, g.total);
    }
    assert!(r.monsters.len() > 150, "monsters: {}", r.monsters.len());
    for m in &r.monsters {
        assert!([7, 11, 15].contains(&m.room), "{}: room {}", m.key, m.room);
        assert!(m.cap >= m.room && m.cap <= m.longest);
        assert!(m.block_len <= m.max_block && m.kept_len <= m.max_kept);
    }
    assert!(r.scenes.len() > 50, "scenes: {}", r.scenes.len());
    for sc in &r.scenes {
        assert!(sc.footprint >= sc.disc_len && sc.lines > 0);
    }
    let streaming = r.carriers.iter().filter(|c| c.streaming).count();
    assert!(streaming >= 10, "streaming carriers: {streaming}");
    assert!(
        r.carriers
            .iter()
            .filter(|c| c.streaming)
            .all(|c| c.sector_slack.is_some_and(|s| s < 2048 * 64))
    );
    assert!(!r.pools.is_empty() && r.pools.iter().all(|p| p.room_bytes >= p.english_bytes));
    // Fixed fields carry their fixed room.
    for e in &r.entries {
        match e.group.as_deref() {
            Some("party") => assert_eq!(e.room, 9),
            Some("place") => assert_eq!(e.room, 31),
            _ => {}
        }
    }
    // The schema round-trips.
    let json = serde_json::to_string(&r).expect("serialize");
    let back: SpaceReport = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back.entries.len(), r.entries.len());
    assert_eq!(back.schema, space::SPACE_SCHEMA);
}

#[test]
fn predicted_outcomes_match_the_import() {
    let Some(original) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let src = DiscPatcher::open(original.clone()).expect("open disc");
    let disc_only = space::space_report(&src, None, SpaceOptions::default()).expect("report");
    let pack = mixed_pack(&src, &disc_only);
    let report = space::space_report(&src, Some(&pack), SpaceOptions::default()).expect("report");

    let mut fresh = DiscPatcher::open(original).expect("open disc");
    let imp = import_pack(&mut fresh, &pack).expect("import");
    let applied: BTreeSet<&str> = imp.applied_keys.iter().map(String::as_str).collect();
    let already: BTreeSet<&str> = imp.already_keys.iter().map(String::as_str).collect();
    let issues: BTreeMap<&str, &str> = imp
        .issues
        .iter()
        .map(|(k, m)| (k.as_str(), m.as_str()))
        .collect();

    let keys = filled(&pack);
    let rows: BTreeMap<&str, &space::EntrySpace> =
        report.entries.iter().map(|e| (e.key.as_str(), e)).collect();
    let mut seen: BTreeMap<Outcome, usize> = BTreeMap::new();
    for key in &keys {
        let row = rows[key.as_str()];
        let o = row.outcome.expect("pack report has outcomes");
        *seen.entry(o).or_default() += 1;
        let landed = applied.contains(key.as_str()) || already.contains(key.as_str());
        assert_eq!(o.lands(), landed, "{key}: predicted {o:?}");
        assert_eq!(
            o == Outcome::AlreadyApplied,
            already.contains(key.as_str()),
            "{key}"
        );
        assert_eq!(
            o == Outcome::Moved,
            imp.trace.moved.contains_key(key),
            "{key}"
        );
        assert_eq!(
            o == Outcome::Grown,
            imp.trace.grown_monsters.contains(key),
            "{key}"
        );
        assert_eq!(!o.lands(), issues.contains_key(key.as_str()), "{key}");
        if !o.lands() {
            assert!(row.issue.is_some() && row.issue_kind.is_some(), "{key}");
        }
        // The room the report shows is the room the import measured, and
        // equals the disc-only (export) room.
        if let Some(&measured) = imp.trace.rooms.get(key) {
            assert_eq!(row.room, measured, "{key}: room");
            let d = disc_only.entries.iter().find(|e| &e.key == key).unwrap();
            assert_eq!(d.room, measured, "{key}: disc-only room");
        }
    }
    assert_eq!(
        seen.get(&Outcome::Moved).copied().unwrap_or(0),
        imp.relocated_names
    );
    assert_eq!(
        seen.get(&Outcome::Grown).copied().unwrap_or(0),
        imp.grown_monster_names
    );
    // Non-vacuous: the pack reached every outcome it was built for.
    for o in [
        Outcome::InPlace,
        Outcome::Moved,
        Outcome::NoFreeRun,
        Outcome::OverBudget,
        Outcome::Grown,
        Outcome::Refused,
        Outcome::Relocated,
        Outcome::RolledBack,
        Outcome::NotEncodable,
    ] {
        assert!(
            seen.get(&o).copied().unwrap_or(0) > 0,
            "no {o:?} in {seen:?}"
        );
    }
    assert!(seen.values().sum::<usize>() > 1000, "{seen:?}");

    // Per-scene rollback sets equal the import's rollback diagnostics.
    let mut rolled: BTreeMap<usize, BTreeSet<&str>> = BTreeMap::new();
    for (k, _) in &imp.issues {
        if imp.trace.issue_kinds.get(k) == Some(&IssueKind::RolledBack) {
            let p: usize = k.split(':').nth(1).unwrap().parse().unwrap();
            rolled.entry(p).or_default().insert(k.as_str());
        }
    }
    assert!(!rolled.is_empty(), "a scene rolled back");
    for sc in &report.scenes {
        let want = rolled.remove(&sc.prot).unwrap_or_default();
        let got: BTreeSet<&str> = sc.rolled_back.iter().map(String::as_str).collect();
        assert_eq!(got, want, "scene {}", sc.prot);
        if !sc.rolled_back.is_empty() {
            assert!(sc.full_overflow.or(sc.padded_overflow).is_some());
        }
    }
    assert!(
        rolled.is_empty(),
        "rollbacks outside the report: {rolled:?}"
    );

    // Fast paths agree with the full report.
    for sc in report.scenes.iter().filter(|s| s.filled.unwrap_or(0) > 0) {
        let t0 = std::time::Instant::now();
        let fit = space::scene_fit(&src, &pack, sc.prot, false);
        let dt = t0.elapsed();
        assert_eq!(fit.scene.rolled_back, sc.rolled_back, "scene {}", sc.prot);
        assert_eq!(fit.scene.path, sc.path, "scene {}", sc.prot);
        assert_eq!(fit.scene.written_len, sc.written_len, "scene {}", sc.prot);
        for e in fit.entries.iter().filter(|e| keys.contains(&e.key)) {
            assert_eq!(e.outcome, rows[e.key.as_str()].outcome, "{}", e.key);
        }
        eprintln!(
            "scene_fit {}: {} filled, {:.1} ms",
            sc.prot,
            fit.scene.filled.unwrap_or(0),
            dt.as_secs_f64() * 1000.0
        );
    }
    let names = NameFitter::new(&src).expect("fitter").fit(&pack);
    for e in &names.entries {
        assert_eq!(e.outcome, rows[e.key.as_str()].outcome, "{}", e.key);
        assert_eq!(e.room, rows[e.key.as_str()].room, "{}", e.key);
    }
    for (a, b) in names.regions.iter().zip(&report.name_regions) {
        assert_eq!((a.pack_used, a.pack_free), (b.pack_used, b.pack_free));
    }
    assert!(
        report.summary.name_free_pack.unwrap() > 0,
        "the shuffle frees bytes"
    );
}

#[test]
fn relayout_dry_run_matches_the_import() {
    let Some(original) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let src = DiscPatcher::open(original.clone()).expect("open disc");
    let disc_only = space::space_report(&src, None, SpaceOptions::default()).expect("report");
    // One mid-sized scene, every walked line grown past its span with
    // incompressible text: the full-length dialog overflows the footprint,
    // so only a relayout carries it.
    let mut pack = export_pack(&src).expect("export");
    let growable: BTreeSet<&str> = disc_only
        .entries
        .iter()
        .filter(|e| e.room_kind == RoomKind::DialogGrowable)
        .map(|e| e.key.as_str())
        .collect();
    let target = disc_only
        .scenes
        .iter()
        .filter(|s| (30..=150).contains(&s.lines))
        .map(|s| s.prot)
        .min()
        .expect("a mid-sized scene");
    let prefix = format!("man:{target}:");
    let mut rng = SplitMix64::new(7);
    for e in pack.sections.scene_dialog.iter_mut() {
        if e.key.starts_with(&prefix) && growable.contains(e.key.as_str()) {
            e.translation = noise(&mut rng, e.budget + 3, true);
        }
    }

    let off = space::space_report(&src, Some(&pack), SpaceOptions::default()).expect("report");
    let sc = off.scenes.iter().find(|s| s.prot == target).unwrap();
    let would = sc
        .relayout_would_add
        .expect("overflow measured without relayout");
    assert!(sc.full_overflow.is_some());

    let on =
        space::space_report(&src, Some(&pack), SpaceOptions { relayout: true }).expect("report");
    let sc = on.scenes.iter().find(|s| s.prot == target).unwrap();
    assert_eq!(sc.path, Some(WritePath::Relayout));
    assert_eq!(
        sc.relayout_sectors,
        Some(would),
        "dry run predicts the relayout"
    );

    let mut fresh = DiscPatcher::open(original).expect("open disc");
    let imp = import_pack_relayout(&mut fresh, &pack).expect("import");
    assert_eq!(imp.relayout_entries, on.summary.relayout_entries);
    assert_eq!(imp.relayout_sectors_added, on.summary.relayout_sectors);
    assert!(imp.relayout_entries >= 1);
    let applied: BTreeSet<&str> = imp.applied_keys.iter().map(String::as_str).collect();
    for e in on.entries.iter().filter(|e| e.key.starts_with(&prefix)) {
        let Some(o) = e.outcome.filter(|o| *o != Outcome::Untranslated) else {
            continue;
        };
        assert_eq!(
            o == Outcome::Relayout,
            applied.contains(e.key.as_str()),
            "{}",
            e.key
        );
    }
}
