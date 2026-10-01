//! Disc-gated: `translate export` carries every dialog line the scene scripts
//! reach, on every retail build, and re-importing the export is a no-op.
//!
//! - USA (`LEGAIA_DISC_BIN`): every script-walked line is exported except the
//!   blank spacer lines; no text sits past a walk's stop; importing the pack
//!   with every `translation` set to its own `source` leaves the image
//!   byte-identical.
//! - Japanese (`LEGAIA_DISC_BIN_JP`, the `SCPS_100.59` disc): the export finds
//!   the count-led Shift-JIS lines (it found none before the build-aware
//!   export), every exported source re-encodes to exactly the disc bytes at
//!   its key, and an import of the pack - unmodified or filled with its own
//!   source - writes nothing.
//! - PAL (`LEGAIA_DISC_BIN_PAL`, any `SCES_*` disc): the export succeeds
//!   (dialog + monster names) and a source-filled import is byte-identical.
//!
//! Each gate skips and passes when its variable is unset.

use legaia_patcher::disc::DiscPatcher;
use legaia_patcher::translation::export::SceneManText;
use legaia_patcher::translation::stream_man::StreamManText;
use legaia_patcher::translation::{LanguagePack, coverage, export_pack, import_pack, sjis};

fn load(var: &str) -> Option<Vec<u8>> {
    let p = std::path::PathBuf::from(std::env::var_os(var)?);
    p.is_file().then(|| std::fs::read(&p).ok()).flatten()
}

/// The pack with every `translation` set to its `source`.
fn source_filled(mut pack: LanguagePack) -> LanguagePack {
    for entries in pack.sections.each_mut() {
        for e in entries.iter_mut() {
            e.translation = e.source.clone();
        }
    }
    pack
}

fn import_is_identity(
    image: &[u8],
    pack: &LanguagePack,
) -> legaia_patcher::translation::ImportReport {
    let mut patcher = DiscPatcher::open(image.to_vec()).expect("open disc");
    let report = import_pack(&mut patcher, pack).expect("import");
    assert!(
        patcher.image() == image,
        "import changed the image (applied {})",
        report.applied
    );
    report
}

#[test]
fn usa_export_carries_every_walked_line() {
    let Some(image) = load("LEGAIA_DISC_BIN") else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let patcher = DiscPatcher::open(image.clone()).expect("open disc");
    let rep = coverage::measure(&patcher).expect("coverage");
    assert_eq!(rep.codec, "latin");
    let (walked, exported, _) = rep.totals();
    assert!(walked > 20_000, "walk found only {walked} lines");
    for c in &rep.carriers {
        for (off, reason) in &c.missing {
            assert_eq!(
                *reason, "blank line",
                "entry {} line 0x{off:x} missing: {reason}",
                c.entry
            );
        }
        assert_eq!(
            c.unreached_candidates, 0,
            "entry {} has unwalked text",
            c.entry
        );
    }
    eprintln!("[ran] USA: {exported} / {walked} walked lines exported");

    let pack = export_pack(&patcher).expect("export");
    let report = import_is_identity(&image, &source_filled(pack));
    assert_eq!(report.applied, 0, "a source-filled import applies nothing");
}

#[test]
fn japanese_export_finds_the_count_led_lines() {
    let Some(image) = load("LEGAIA_DISC_BIN_JP") else {
        eprintln!("[skip] LEGAIA_DISC_BIN_JP unset");
        return;
    };
    let patcher = DiscPatcher::open(image.clone()).expect("open disc");
    let pack = export_pack(&patcher).expect("export must succeed on the Japanese disc");
    assert_eq!(pack.language, "ja");
    assert!(pack.sections.scene_dialog.len() > 20_000);
    assert!(pack.sections.inline_text.len() > 5_000);
    assert!(
        pack.sections.items.is_empty(),
        "USA-keyed sections stay USA-only"
    );

    // Every source re-encodes to the disc bytes at its key.
    let mut checked = 0;
    for (section, entries) in pack.sections.iter() {
        for e in entries {
            let mut parts = e.key.split(':');
            let (kind, idx, off) = (
                parts.next().unwrap(),
                parts.next().unwrap().parse::<usize>().unwrap(),
                usize::from_str_radix(parts.next().unwrap().trim_start_matches("0x"), 16).unwrap(),
            );
            let entry = patcher.read_entry(idx).unwrap();
            let bytes = match kind {
                "man" => SceneManText::locate(&entry).unwrap().decoded,
                "raw" => entry,
                other => panic!("{section}: unexpected key kind {other}"),
            };
            let encoded = sjis::encode(&e.source).expect("source encodes");
            assert_eq!(encoded.len(), e.budget, "{}", e.key);
            assert_eq!(&bytes[off..off + e.budget], encoded.as_slice(), "{}", e.key);
            assert_eq!(
                bytes[off - 1] as usize * 2,
                e.budget,
                "count byte of {}",
                e.key
            );
            checked += 1;
        }
    }
    // The streaming dungeon scenes' lines are keyed in entry space.
    assert!((0..patcher.entry_count()).any(|i| {
        patcher
            .read_entry(i)
            .ok()
            .and_then(|e| StreamManText::locate_structural(&e))
            .is_some()
    }));

    let rep = coverage::measure(&patcher).expect("coverage");
    assert_eq!(rep.codec, "shift_jis");
    let (walked, exported, missing) = rep.totals();
    assert!(
        missing * 1000 <= walked,
        "{missing} of {walked} walked lines missing"
    );
    for c in &rep.carriers {
        assert_eq!(
            c.unreached_candidates, 0,
            "entry {} has unwalked text",
            c.entry
        );
    }
    eprintln!("[ran] JP: {checked} sources byte-exact; {exported} / {walked} walked lines");

    import_is_identity(&image, &pack);
    let report = import_is_identity(&image, &source_filled(pack));
    assert_eq!(report.applied, 0);
    assert!(
        !report.issues.is_empty(),
        "a filled import onto JP is refused per entry"
    );
}

#[test]
fn pal_export_succeeds_and_reimports_identically() {
    let Some(image) = load("LEGAIA_DISC_BIN_PAL") else {
        eprintln!("[skip] LEGAIA_DISC_BIN_PAL unset");
        return;
    };
    let patcher = DiscPatcher::open(image.clone()).expect("open disc");
    let pack = export_pack(&patcher).expect("export must succeed on a PAL disc");
    assert!(pack.sections.scene_dialog.len() > 20_000);
    assert!(!pack.sections.monster_names.is_empty());
    let rep = coverage::measure(&patcher).expect("coverage");
    for c in &rep.carriers {
        for (off, reason) in &c.missing {
            assert_eq!(*reason, "blank line", "entry {} line 0x{off:x}", c.entry);
        }
    }
    eprintln!(
        "[ran] PAL: {} dialog lines",
        pack.sections.scene_dialog.len()
    );
    let report = import_is_identity(&image, &source_filled(pack));
    assert_eq!(report.applied, 0);
}
