//! Disc-gated end-to-end tests for the translation / language-pack pipeline:
//!
//! - export -> import of the **unmodified** pack changes zero bytes on the
//!   image (untranslated entries are never written);
//! - a synthetic pack that fills a handful of entries (safe ASCII, within
//!   budget) round-trips: patch a scratch copy, re-export off the patched
//!   image via the same parsers and see the new strings read back, with
//!   every touched sector still EDC/ECC-valid, and the SCUS name tables
//!   resolving the new names;
//! - a second import of the same pack is a no-op (idempotent);
//! - a fixed pack is byte-deterministic across runs.
//!
//! Skips + passes without `LEGAIA_DISC_BIN`.

use std::collections::BTreeMap;

use legaia_iso::raw::SECTOR_SIZE;
use legaia_rando::disc::DiscPatcher;
use legaia_rando::translation::{LanguagePack, export_pack, import_pack, pack::Entry};

fn load_disc() -> Option<Vec<u8>> {
    let p = std::path::PathBuf::from(std::env::var_os("LEGAIA_DISC_BIN")?);
    p.is_file().then(|| std::fs::read(&p).ok()).flatten()
}

fn export(image: &[u8]) -> LanguagePack {
    let patcher = DiscPatcher::open(image.to_vec()).expect("open disc");
    export_pack(&patcher).expect("export pack")
}

#[test]
fn export_covers_every_section_and_unmodified_import_is_a_noop() {
    let Some(original) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let pack = export(&original);

    // Coverage sanity: every section is populated on a retail disc.
    for (name, _translated, total) in pack.coverage() {
        assert!(total > 0, "section {name} exported no entries");
    }
    assert!(
        pack.sections.scene_dialog.len() > 10_000,
        "scene dialog should be the bulk of the text ({} entries)",
        pack.sections.scene_dialog.len()
    );
    // Keys are globally unique: several SCUS tables point into one string
    // pool (arts are also named in the spell-id space), and one key must
    // mean one write.
    let mut keys = std::collections::BTreeSet::new();
    for (name, entries) in pack.sections.iter() {
        for e in entries {
            assert!(
                keys.insert(e.key.clone()),
                "duplicate key {} in {name}",
                e.key
            );
        }
    }
    // YAML pack round-trips bit-exactly through our emitter + parser.
    let yaml = pack.to_yaml().expect("to_yaml");
    let back = LanguagePack::from_yaml(&yaml).expect("from_yaml");
    assert_eq!(pack, back, "pack YAML round-trip");

    // Importing the untouched export changes nothing.
    let mut patcher = DiscPatcher::open(original.clone()).expect("open disc");
    let report = import_pack(&mut patcher, &pack).expect("import");
    assert_eq!(report.applied, 0);
    assert_eq!(report.already_applied, 0);
    assert!(report.issues.is_empty(), "issues: {:?}", report.issues);
    assert_eq!(report.untranslated, pack.sections.total());
    assert_eq!(
        patcher.into_image(),
        original,
        "unmodified import must be byte-identical"
    );
}

/// Pick a translation that certainly encodes and fits: ASCII, `<= budget`.
fn fill(entry: &mut Entry, text: &str) -> String {
    let t: String = text.chars().take(entry.budget).collect();
    entry.translation = t.clone();
    t
}

#[test]
fn synthetic_pack_round_trips_on_disc() {
    let Some(original) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut pack = export(&original);

    // Fill a spread of entries across every mechanism. Keyed expectations:
    // key -> expected re-exported source (segments read back space-padded).
    let mut expect: BTreeMap<String, String> = BTreeMap::new();

    let e = &mut pack.sections.items[0];
    let t = fill(e, "Beere X");
    expect.insert(e.key.clone(), t);

    let e = &mut pack.sections.spells[0];
    let t = fill(e, "Zauber Y");
    expect.insert(e.key.clone(), t);

    let e = &mut pack.sections.arts[0];
    let t = fill(e, "Kunst Z");
    expect.insert(e.key.clone(), t);

    let e = &mut pack.sections.accessory_passives[0];
    let t = fill(e, "Passiv W");
    expect.insert(e.key.clone(), t);

    let e = &mut pack.sections.party_names[0];
    let t = fill(e, "Vahnja");
    expect.insert(e.key.clone(), t);

    // Three dialog segments in different scenes + two raw carriers. Prose-ish
    // text so the re-export quality gate still accepts the patched segments.
    let dialog_picks: Vec<usize> = {
        let mut seen = std::collections::BTreeSet::new();
        let mut picks = Vec::new();
        for (i, e) in pack.sections.scene_dialog.iter().enumerate() {
            let entry_id = e.key.split(':').nth(1).unwrap().to_string();
            if e.budget >= 12 && seen.insert(entry_id) {
                picks.push(i);
                if picks.len() == 3 {
                    break;
                }
            }
        }
        picks
    };
    assert_eq!(dialog_picks.len(), 3, "need three scenes with room");
    for (n, i) in dialog_picks.into_iter().enumerate() {
        let e = &mut pack.sections.scene_dialog[i];
        let t = fill(
            e,
            match n {
                0 => "Das ist ein Test.",
                1 => "Voici un essai.",
                _ => "Prueba de texto.",
            },
        );
        expect.insert(e.key.clone(), t);
    }
    let raw_picks: Vec<usize> = pack
        .sections
        .inline_text
        .iter()
        .enumerate()
        .filter(|(_, e)| e.budget >= 12)
        .map(|(i, _)| i)
        .take(2)
        .collect();
    assert_eq!(raw_picks.len(), 2, "need two raw segments with room");
    for (n, i) in raw_picks.into_iter().enumerate() {
        let e = &mut pack.sections.inline_text[i];
        let t = fill(
            e,
            if n == 0 {
                "Testo uno qui."
            } else {
                "Outro texto."
            },
        );
        expect.insert(e.key.clone(), t);
    }

    // Import onto a scratch copy.
    let mut patcher = DiscPatcher::open(original.clone()).expect("open disc");
    let report = import_pack(&mut patcher, &pack).expect("import");
    assert!(report.issues.is_empty(), "issues: {:?}", report.issues);
    assert_eq!(report.applied, expect.len(), "all filled entries applied");
    let patched = patcher.into_image();
    assert_eq!(patched.len(), original.len(), "same-size image");

    // Every touched sector is still EDC/ECC-valid.
    let mut touched = 0usize;
    for (i, (a, b)) in original
        .chunks(SECTOR_SIZE)
        .zip(patched.chunks(SECTOR_SIZE))
        .enumerate()
    {
        if a != b && a.len() == SECTOR_SIZE {
            touched += 1;
            assert!(
                legaia_iso::write::mode2_form1_sector_is_valid(b),
                "sector {i} invalid after patch"
            );
        }
    }
    assert!(touched > 0, "the import must have touched sectors");

    // Re-export from the patched image with the same parsers: the filled
    // entries read back as their translations (segments space-padded).
    let re = export(&patched);
    let mut found = 0usize;
    for (_, entries) in re.sections.iter() {
        for e in entries {
            if let Some(want) = expect.get(&e.key) {
                assert_eq!(
                    e.source.trim_end_matches(' '),
                    want.trim_end_matches(' '),
                    "key {} must read back translated",
                    e.key
                );
                found += 1;
            }
        }
    }
    assert_eq!(found, expect.len(), "every patched key re-exports");

    // The engine-facing SCUS parsers resolve the new names too.
    let scus = legaia_iso::iso9660::read_file_in_image(&patched, "SCUS_942.54").expect("scus");
    let items = legaia_asset::item_names::ItemNameTable::from_scus(&scus).expect("item table");
    assert!(
        (0..=255u8).any(|id| items.name(id) == Some("Beere X")),
        "patched item name visible through ItemNameTable"
    );

    // Idempotency: importing the same pack over the patched image is a no-op.
    let mut patcher2 = DiscPatcher::open(patched.clone()).expect("open patched");
    let report2 = import_pack(&mut patcher2, &pack).expect("re-import");
    assert_eq!(report2.applied, 0, "issues: {:?}", report2.issues);
    assert_eq!(report2.already_applied, expect.len());
    assert!(report2.issues.is_empty(), "issues: {:?}", report2.issues);
    assert_eq!(
        patcher2.into_image(),
        patched,
        "re-import is byte-identical"
    );

    // Determinism: a fresh import of the same pack produces the same image.
    let mut patcher3 = DiscPatcher::open(original.clone()).expect("open disc");
    import_pack(&mut patcher3, &pack).expect("import again");
    assert_eq!(patcher3.into_image(), patched, "import is deterministic");
}

#[test]
fn over_budget_and_non_latin_report_per_entry_and_touch_nothing() {
    let Some(original) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut pack = export(&original);

    // One over-budget entry, one non-encodable entry.
    let key_long = {
        let e = &mut pack.sections.items[0];
        e.translation = "X".repeat(e.budget + 5);
        e.key.clone()
    };
    let key_cyr = {
        let e = &mut pack.sections.spells[0];
        e.translation = "Заклинание".to_string();
        e.key.clone()
    };

    let mut patcher = DiscPatcher::open(original.clone()).expect("open disc");
    let report = import_pack(&mut patcher, &pack).expect("import");
    assert_eq!(report.applied, 0);
    assert_eq!(report.issues.len(), 2);
    let msg = |k: &str| {
        report
            .issues
            .iter()
            .find(|(key, _)| key == k)
            .map(|(_, m)| m.clone())
            .unwrap_or_default()
    };
    assert!(msg(&key_long).contains("budget"), "{}", msg(&key_long));
    assert!(
        msg(&key_cyr).contains("not in the retail glyph set"),
        "{}",
        msg(&key_cyr)
    );
    assert_eq!(
        patcher.into_image(),
        original,
        "failed entries must leave the image untouched"
    );
}
