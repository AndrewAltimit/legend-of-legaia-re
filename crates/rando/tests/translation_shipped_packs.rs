//! The **shipped** language packs (`site/lang/*.yaml`) - the distributable,
//! translation-only shape the site's ROM patcher offers.
//!
//! Two layers:
//!
//! - a **disc-free content gate** (always runs, including in CI): every shipped
//!   pack parses, carries no `source:` / `context:` field (the repo must never
//!   hold the game's own script), keys are well-formed and unique, and every
//!   translation both encodes into the retail glyph set and fits its byte
//!   budget. This is what keeps a bad pack from being committed.
//! - **disc-gated proofs** (skip + pass without `LEGAIA_DISC_BIN`): each pack
//!   applies to a real disc through the same code path the browser uses, the
//!   patched image still parses, its SCUS name table reads back the translated
//!   names, every touched sector stays EDC/ECC-valid, and a translated **and**
//!   randomized image composes (translate first, then randomize).

use std::path::{Path, PathBuf};

use legaia_iso::raw::SECTOR_SIZE;
use legaia_rando::disc::DiscPatcher;
use legaia_rando::drops::DropMode;
use legaia_rando::translation::markup::{self, Target};
use legaia_rando::translation::{LanguagePack, import_pack};

/// Every language pack the repo ships.
fn shipped_packs() -> Vec<(String, LanguagePack)> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("site/lang");
    let mut out = Vec::new();
    for entry in std::fs::read_dir(&dir).expect("site/lang must exist") {
        let path = entry.expect("dir entry").path();
        if path.extension().and_then(|e| e.to_str()) != Some("yaml") {
            continue;
        }
        let text = std::fs::read_to_string(&path).expect("read pack");
        let pack = LanguagePack::from_yaml(&text)
            .unwrap_or_else(|e| panic!("{} is not a valid pack: {e}", path.display()));
        out.push((
            path.file_stem().unwrap().to_string_lossy().into_owned(),
            pack,
        ));
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    assert!(!out.is_empty(), "no shipped packs found in site/lang");
    out
}

fn load_disc() -> Option<Vec<u8>> {
    let p = PathBuf::from(std::env::var_os("LEGAIA_DISC_BIN")?);
    p.is_file().then(|| std::fs::read(&p).ok()).flatten()
}

/// A shipped pack is a `key -> our text` lookup table and nothing else: no
/// `source:`, no `context:`. Both fields quote the game's own script, so their
/// presence in a committed file is a licensing bug, not a style one.
#[test]
fn shipped_packs_carry_no_source_text() {
    for (name, pack) in shipped_packs() {
        for (section, entries) in pack.sections.iter() {
            for e in entries {
                assert!(
                    e.source.is_empty(),
                    "{name}.yaml [{section}] {}: shipped packs must not carry the \
                     original text (run `translate strip`)",
                    e.key
                );
                assert!(
                    e.context.is_empty(),
                    "{name}.yaml [{section}] {}: shipped packs must not carry context",
                    e.key
                );
                assert!(
                    e.is_filled(),
                    "{name}.yaml [{section}] {}: unfilled entry in a shipped pack",
                    e.key
                );
            }
        }
        // The file itself, too - the emitter must never print those keys.
        let yaml = pack.to_yaml().expect("emit");
        assert!(
            !yaml.contains("source:"),
            "{name}.yaml emits a source field"
        );
        assert!(
            !yaml.contains("context:"),
            "{name}.yaml emits a context field"
        );
    }
}

/// Every translation encodes into the retail glyph set (printable ASCII - the
/// font has no accented Latin, so packs must be ASCII-folded) and fits the byte
/// budget of the string it replaces.
#[test]
fn shipped_pack_translations_encode_within_budget() {
    for (name, pack) in shipped_packs() {
        let mut keys = std::collections::BTreeSet::new();
        let mut filled = 0usize;
        for (section, entries) in pack.sections.iter() {
            for e in entries {
                assert!(
                    keys.insert(e.key.clone()),
                    "{name}.yaml: duplicate key {}",
                    e.key
                );
                // `scus:` name-table and `ui:` overlay strings are
                // NUL-terminated C strings (import encodes them with
                // `CString`); the dialog sections are `0x1F`-lead segments.
                let target = if e.key.starts_with("scus:") || e.key.starts_with("ui:") {
                    Target::CString
                } else {
                    Target::Segment
                };
                match markup::encode(&e.translation, target) {
                    Ok(bytes) => assert!(
                        bytes.len() <= e.budget,
                        "{name}.yaml [{section}] {}: {} bytes over its {} byte budget",
                        e.key,
                        bytes.len(),
                        e.budget
                    ),
                    Err(issues) => panic!(
                        "{name}.yaml [{section}] {}: not encodable in the retail glyph \
                         set - {} (ASCII-fold it)",
                        e.key, issues[0]
                    ),
                }
                filled += 1;
            }
        }
        assert!(
            filled > 500,
            "{name}.yaml has only {filled} entries - the name tables should all be filled"
        );
        assert!(
            !pack.notes.is_empty(),
            "{name}.yaml must document what is filled vs skeleton in `notes`"
        );
    }
}

/// Every shipped pack fills the overlay UI-menu corpus (`ui:` keys): the
/// pause-menu / options / shop / equip / status command labels and the
/// in-battle system messages. Locks the multi-language UI-menu fill in place so
/// a regression (a pack shipped without it) fails the build. Keyed by disc
/// coordinate, so this asserts nothing about the game's own text.
#[test]
fn shipped_packs_cover_ui_menu() {
    // A representative spread of menu (0899) + battle (0898) command labels
    // every pack must translate. Coordinates, not text.
    const REQUIRED: &[&str] = &[
        "ui:899:0x801ce9d0", // @Items
        "ui:899:0x801ce9d8", // @Magic
        "ui:899:0x801ce9e0", // @Equip
        "ui:899:0x801ce9e8", // @Status
        "ui:899:0x801cea08", // @Save
        "ui:899:0x801ceb94", // @Buy
        "ui:899:0x801ceb9c", // @Sell
        "ui:898:0x801f4d24", // Escape
    ];
    for (name, pack) in shipped_packs() {
        let keys: std::collections::BTreeSet<&str> = pack
            .sections
            .ui_menu
            .iter()
            .map(|e| e.key.as_str())
            .collect();
        assert!(
            pack.sections.ui_menu.len() >= 90,
            "{name}.yaml ships only {} ui_menu entries - the overlay UI corpus \
             should be filled",
            pack.sections.ui_menu.len()
        );
        for want in REQUIRED {
            assert!(
                keys.contains(want),
                "{name}.yaml is missing UI-menu key {want}"
            );
        }
    }
}

/// Key shapes are the four disc coordinates the importer understands.
#[test]
fn shipped_pack_keys_are_disc_coordinates() {
    for (name, pack) in shipped_packs() {
        for (_, entries) in pack.sections.iter() {
            for e in entries {
                let parts: Vec<&str> = e.key.split(':').collect();
                let ok = match parts.as_slice() {
                    ["scus", "str", va] => va
                        .strip_prefix("0x")
                        .and_then(|h| u32::from_str_radix(h, 16).ok())
                        .is_some(),
                    ["scus", "party", n] => n.parse::<usize>().is_ok(),
                    // `ui:<prot>:0x<va>` - overlay UI string at a virtual
                    // address inside PROT overlay entry `prot`.
                    ["ui", entry, va] => {
                        entry.parse::<usize>().is_ok()
                            && va
                                .strip_prefix("0x")
                                .and_then(|h| u32::from_str_radix(h, 16).ok())
                                .is_some()
                    }
                    [kind @ ("man" | "raw"), entry, off] => {
                        let _ = kind;
                        entry.parse::<usize>().is_ok()
                            && off
                                .strip_prefix("0x")
                                .and_then(|h| usize::from_str_radix(h, 16).ok())
                                .is_some()
                    }
                    _ => false,
                };
                assert!(ok, "{name}.yaml: malformed key {}", e.key);
            }
        }
    }
}

/// Each shipped pack applies to a real disc: the image stays the same size and
/// still parses, the SCUS item table reads back translated names, and every
/// sector the import touched is still EDC/ECC-valid.
#[test]
fn shipped_packs_apply_to_a_real_disc() {
    let Some(original) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    for (name, pack) in shipped_packs() {
        let mut patcher = DiscPatcher::open(original.clone()).expect("open disc");
        let report = import_pack(&mut patcher, &pack).expect("import");
        assert!(
            report.applied > 500,
            "{name}: only {} entries applied",
            report.applied
        );
        let patched = patcher.into_image();
        assert_eq!(patched.len(), original.len(), "{name}: same-size image");

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
                    "{name}: sector {i} invalid after import"
                );
            }
        }
        assert!(touched > 0, "{name}: nothing was written");

        // The engine-facing parser sees the translated names: at least one item
        // name on the patched disc must equal a `scus:str` translation, and none
        // may be empty (an over-long write would have eaten a neighbour).
        let scus = legaia_iso::iso9660::read_file_in_image(&patched, "SCUS_942.54").expect("scus");
        let items =
            legaia_asset::item_names::ItemNameTable::from_scus(&scus).expect("item name table");
        let translated: std::collections::BTreeSet<&str> = pack
            .sections
            .items
            .iter()
            .map(|e| e.translation.as_str())
            .collect();
        let hits = (0..=255u8)
            .filter_map(|id| items.name(id))
            .filter(|n| translated.contains(n))
            .count();
        assert!(
            hits > 100,
            "{name}: only {hits} translated item names visible through ItemNameTable"
        );

        // Idempotent: re-importing writes nothing.
        let mut again = DiscPatcher::open(patched.clone()).expect("reopen");
        let r2 = import_pack(&mut again, &pack).expect("re-import");
        assert_eq!(
            r2.applied, 0,
            "{name}: re-import wrote {} entries",
            r2.applied
        );
        assert_eq!(
            again.into_image(),
            patched,
            "{name}: re-import changed bytes"
        );
    }
}

/// Translation and randomization compose, in that order.
///
/// Translate first: the randomizer's door / starting-bag passes *relocate* MAN
/// records, which would move the byte offsets the dialog keys address. The
/// reverse (translate onto an already-randomized image) is safe but lossy - the
/// moved scenes' lines are skipped. The randomizer itself only reads structure,
/// never text, so it is unbothered by translated strings.
#[test]
fn translated_then_randomized_composes_and_stays_valid() {
    let Some(original) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let (_, pack) = shipped_packs()
        .into_iter()
        .find(|(n, _)| n == "es")
        .expect("the Spanish pack ships");

    let mut patcher = DiscPatcher::open(original.clone()).expect("open disc");
    // 1. Language first.
    let report = import_pack(&mut patcher, &pack).expect("import");
    assert!(report.applied > 500);

    // 2. Then the randomizer, over the translated image.
    let scus =
        legaia_iso::iso9660::read_file_in_image(patcher.image(), "SCUS_942.54").expect("scus");
    let pool = legaia_rando::items::valid_item_pool(&scus).expect("item pool");
    // The name-keyed passes only ask whether an item is *named*, which a
    // translation preserves - so the pool is still the full one.
    assert!(
        pool.len() > 100,
        "translated names must still resolve as named items ({} in pool)",
        pool.len()
    );
    let seed = 0xC0FFEEu64;
    legaia_rando::apply::randomize_drops(&mut patcher, &pool, seed, DropMode::Shuffle)
        .expect("drops");
    legaia_rando::apply::randomize_encounters(&mut patcher, seed, DropMode::Shuffle, &[])
        .expect("encounters");
    let both = patcher.into_image();
    assert_eq!(both.len(), original.len());

    // The composed image still parses as a disc, and the translation survived
    // the randomizer's writes.
    let check = DiscPatcher::open(both.clone()).expect("re-parse composed image");
    let _ = legaia_rando::apply::current_drops(&check).expect("drops still readable");
    let scus2 = legaia_iso::iso9660::read_file_in_image(&both, "SCUS_942.54").expect("scus");
    let items = legaia_asset::item_names::ItemNameTable::from_scus(&scus2).expect("item table");
    let translated: std::collections::BTreeSet<&str> = pack
        .sections
        .items
        .iter()
        .map(|e| e.translation.as_str())
        .collect();
    let hits = (0..=255u8)
        .filter_map(|id| items.name(id))
        .filter(|n| translated.contains(n))
        .count();
    assert!(
        hits > 100,
        "translation survived randomization ({hits} names)"
    );

    // Every sector either side touched is still EDC/ECC-valid.
    for (i, (a, b)) in original
        .chunks(SECTOR_SIZE)
        .zip(both.chunks(SECTOR_SIZE))
        .enumerate()
    {
        if a != b && a.len() == SECTOR_SIZE {
            assert!(
                legaia_iso::write::mode2_form1_sector_is_valid(b),
                "sector {i} invalid after translate+randomize"
            );
        }
    }
}

/// The resume path: a shipped (source-less) pack merges back onto a fresh
/// working pack exported from the user's own disc, so a translator can keep
/// editing without anyone ever redistributing the source text.
#[test]
fn shipped_pack_resumes_into_a_working_pack() {
    let Some(original) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let (_, shipped) = shipped_packs()
        .into_iter()
        .find(|(n, _)| n == "fr")
        .expect("the French pack ships");
    let patcher = DiscPatcher::open(original).expect("open disc");
    let base = legaia_rando::translation::export_pack(&patcher).expect("export");

    let mut working = base.into_skeleton("fr", vec!["someone".into()]);
    let merged = working.merge_translations(&shipped);
    assert_eq!(
        merged,
        shipped.sections.total(),
        "every shipped translation must land on a key of a fresh export"
    );
    // ... and the working pack now carries both the source (for the translator)
    // and the shipped translation (to keep editing).
    let filled: Vec<_> = working
        .sections
        .items
        .iter()
        .filter(|e| e.is_filled())
        .collect();
    assert!(!filled.is_empty());
    assert!(
        filled.iter().all(|e| !e.source.is_empty()),
        "a working pack keeps the source text (locally, never committed)"
    );
}
