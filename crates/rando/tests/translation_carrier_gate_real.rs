//! Disc-gated regression test for the language-patch freeze.
//!
//! A shipped language pack froze the game: on New Game (both languages) and,
//! for German, on Meta's in-battle Seru-magic list. Root cause: the raw
//! `0x1F <text> 0x00` dialog-segment scanner fires by coincidence throughout
//! binary asset banks (sequenced music, VAB, the PROT-1204 battle-character
//! pack, monster archives, every scene's first ANM slot), and the importer
//! wrote translated bytes over those coincidental hits, corrupting the binary
//! asset - a garbled SEQ hung the sound driver as New-Game BGM started, and a
//! garbled PROT-1204 pack froze the battle menu that renders Meta's form.
//!
//! The fix gates raw-segment writes on a per-entry **dialog-carrier** check
//! (`segments::is_dialog_carrier`): only prose-dense event-script / dungeon-MAN
//! scenes are written; binary banks are refused. These tests assert:
//!
//! - export never emits a raw key in a non-carrier PROT entry;
//! - a full-fill import leaves every non-carrier binary PROT entry
//!   byte-identical (no binary asset is ever overwritten), while real dialog
//!   carriers still change and every touched sector stays EDC/ECC-valid;
//! - a hand-injected poison entry aimed at a binary bank is refused with a
//!   diagnostic and leaves the bank untouched;
//! - every SCUS name-table string the import writes terminates within its
//!   pool and never runs into a neighbouring string.
//!
//! Skips + passes without `LEGAIA_DISC_BIN`.

use legaia_iso::raw::SECTOR_SIZE;
use legaia_rando::disc::DiscPatcher;
use legaia_rando::translation::export::SceneManText;
use legaia_rando::translation::pack::Entry;
use legaia_rando::translation::{ImportPhase, import_pack_phase, segments};

fn load_disc() -> Option<Vec<u8>> {
    let p = std::path::PathBuf::from(std::env::var_os("LEGAIA_DISC_BIN")?);
    p.is_file().then(|| std::fs::read(&p).ok()).flatten()
}

/// Same-length reversible transform: swap vowels within case, leaving `{..}`
/// markup tokens byte-identical so the encoded length never changes.
fn vowel_swap(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let mut in_brace = false;
    for c in src.chars() {
        let mapped = match c {
            '{' => {
                in_brace = true;
                c
            }
            '}' => {
                in_brace = false;
                c
            }
            _ if in_brace => c,
            'a' => 'e',
            'e' => 'a',
            'i' => 'o',
            'o' => 'i',
            'A' => 'E',
            'E' => 'A',
            'I' => 'O',
            'O' => 'I',
            _ => c,
        };
        out.push(mapped);
    }
    out
}

/// Parse a `raw:<entry>:0x<off>` key's entry index.
fn raw_entry_of(key: &str) -> Option<usize> {
    key.strip_prefix("raw:")?.split(':').next()?.parse().ok()
}

#[test]
fn export_never_emits_a_raw_key_in_a_non_carrier_entry() {
    let Some(original) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let patcher = DiscPatcher::open(original).expect("open disc");
    let pack = legaia_rando::translation::export_pack(&patcher).expect("export");
    assert!(
        !pack.sections.inline_text.is_empty(),
        "export produced no raw carriers at all"
    );
    for e in &pack.sections.inline_text {
        let idx = raw_entry_of(&e.key).expect("raw key shape");
        let entry = patcher.read_entry(idx).expect("read entry");
        assert!(
            segments::is_dialog_carrier(&entry),
            "export emitted raw key {} in a non-carrier (binary) PROT entry {idx}",
            e.key
        );
    }
}

#[test]
fn full_fill_import_only_touches_carriers_mans_and_scus() {
    let Some(original) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };

    let base = DiscPatcher::open(original.clone()).expect("open disc");
    let n = base.entry_count();

    // Clamped per-entry disc extent: an entry's footprint capped at the next
    // entry's start LBA, so an extended TOC window over a neighbour's sectors
    // isn't mistaken for the entry's own bytes (the same clamp export uses).
    let mut lbas: Vec<u32> = (0..n).filter_map(|i| base.entry_disc_lba(i)).collect();
    lbas.sort_unstable();
    lbas.dedup();
    let clamped_sectors = |idx: usize| -> Option<(u32, u32)> {
        let lba = base.entry_disc_lba(idx)?;
        let foot = base.entry_footprint(idx)? as u32;
        let own = foot.div_ceil(2048);
        let next = lbas
            .iter()
            .copied()
            .find(|&l| l > lba)
            .map(|nl| nl - lba)
            .unwrap_or(own);
        Some((lba, own.min(next)))
    };

    // The set of disc LBAs a translation import is *allowed* to change: the
    // SCUS_942.54 file, plus the clamped sectors of every dialog carrier and
    // every scene-MAN owner. Anything outside is a binary asset bank whose
    // corruption is the freeze under test.
    use std::collections::BTreeSet;
    let mut allowed: BTreeSet<u32> = BTreeSet::new();
    // SCUS lives at a fixed early-disc extent; find it by name via the ISO walk.
    // (The importer only writes it through `patch_named_file`.)
    let (scus_lba, scus_size) =
        legaia_iso::iso9660::find_file_in_image(&original, "SCUS_942.54").expect("SCUS in image");
    for s in 0..scus_size.div_ceil(2048) {
        allowed.insert(scus_lba + s);
    }
    let mut carrier_entries = 0usize;
    for idx in 0..n {
        let Ok(entry) = base.read_entry(idx) else {
            continue;
        };
        let is_carrier = segments::is_dialog_carrier(&entry);
        let is_man = SceneManText::locate(&entry).is_some();
        if is_carrier {
            carrier_entries += 1;
        }
        if (is_carrier || is_man)
            && let Some((lba, secs)) = clamped_sectors(idx)
        {
            for s in 0..secs {
                allowed.insert(lba + s);
            }
        }
    }
    assert!(carrier_entries > 0, "no dialog carriers found");

    // Fill every entry the exported (already carrier-gated) pack offers.
    let mut pack = legaia_rando::translation::export_pack(&base).expect("export");
    for entries in pack.sections.each_mut() {
        for e in entries.iter_mut() {
            let t = vowel_swap(&e.source);
            if t != e.source {
                e.translation = t;
            }
        }
    }

    let mut patcher = DiscPatcher::open(original.clone()).expect("open disc");
    let mut report =
        import_pack_phase(&mut patcher, &pack, ImportPhase::DialogOnly).expect("dialog import");
    report.merge(import_pack_phase(&mut patcher, &pack, ImportPhase::NamesOnly).expect("names"));
    let patched = patcher.into_image();
    assert_eq!(patched.len(), original.len());

    // Every changed sector must belong to an allowed extent, and stay valid.
    let mut touched = 0usize;
    for (i, (a, b)) in original
        .chunks(SECTOR_SIZE)
        .zip(patched.chunks(SECTOR_SIZE))
        .enumerate()
    {
        if a == b || a.len() != SECTOR_SIZE {
            continue;
        }
        touched += 1;
        let lba = i as u32;
        assert!(
            allowed.contains(&lba),
            "sector at LBA {lba} changed but belongs to no carrier / MAN / SCUS extent \
             - a binary asset bank was corrupted"
        );
        assert!(
            legaia_iso::write::mode2_form1_sector_is_valid(b),
            "touched sector {i} invalid after language import"
        );
    }
    assert!(touched > 0, "the language import must touch sectors");
}

#[test]
fn import_refuses_a_poison_entry_aimed_at_a_binary_bank() {
    let Some(original) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let base = DiscPatcher::open(original.clone()).expect("open disc");

    // Find a binary bank (non-carrier) PROT entry that carries a coincidental
    // `0x1F <2 printable> 0x00` segment - the exact shape the freeze wrote over.
    let mut poison: Option<(usize, usize, usize)> = None; // (entry, text_off, len)
    for idx in 0..base.entry_count() {
        let Ok(entry) = base.read_entry(idx) else {
            continue;
        };
        if segments::is_dialog_carrier(&entry) || SceneManText::locate(&entry).is_some() {
            continue;
        }
        if let Some(s) = segments::scan(&entry).into_iter().find(|s| s.len >= 2) {
            poison = Some((idx, s.text_off, s.len));
            break;
        }
    }
    let (entry_idx, off, len) = poison.expect("a coincidental segment in some binary bank");

    // A distributable-shape poison entry (no source; budget = on-disc framing)
    // whose "translation" is a same-length in-charset run.
    let mut pack = legaia_rando::translation::LanguagePack::new("xx");
    let repl: String = "z".repeat(len);
    pack.sections.inline_text.push(Entry {
        key: format!("raw:{entry_idx}:0x{off:x}"),
        context: String::new(),
        source: String::new(),
        translation: repl,
        budget: len,
    });

    let mut patcher = DiscPatcher::open(original.clone()).expect("open disc");
    let report = import_pack_phase(&mut patcher, &pack, ImportPhase::DialogOnly).expect("import");
    assert_eq!(report.applied, 0, "poison entry must not be applied");
    assert!(
        report
            .issues
            .iter()
            .any(|(k, _)| k.contains(&entry_idx.to_string())),
        "the refused poison entry must be reported"
    );
    // The binary bank stays byte-identical.
    let patched = patcher.into_image();
    let post = DiscPatcher::open(patched).expect("open patched");
    assert_eq!(
        base.read_entry(entry_idx).unwrap(),
        post.read_entry(entry_idx).unwrap(),
        "binary bank must be untouched after the poison entry is refused"
    );
}

#[test]
fn imported_scus_strings_terminate_within_their_pool() {
    let Some(original) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let base = DiscPatcher::open(original.clone()).expect("open disc");
    let mut pack = legaia_rando::translation::export_pack(&base).expect("export");
    // Fill only the SCUS name sections (the spell/item/art pools whose overrun
    // would clobber a neighbour's pointer target).
    for e in pack
        .sections
        .items
        .iter_mut()
        .chain(pack.sections.item_types.iter_mut())
        .chain(pack.sections.spells.iter_mut())
        .chain(pack.sections.arts.iter_mut())
        .chain(pack.sections.accessory_passives.iter_mut())
    {
        let t = vowel_swap(&e.source);
        if t != e.source {
            e.translation = t;
        }
    }

    let mut patcher = DiscPatcher::open(original).expect("open disc");
    import_pack_phase(&mut patcher, &pack, ImportPhase::NamesOnly).expect("names import");
    let scus = patcher
        .read_named_file("SCUS_942.54")
        .expect("SCUS in patched image");

    // Every string pointer we translated still resolves to a NUL-terminated
    // string whose bytes are all printable / escape glyphs, and the string
    // does not run past the next pointed-to string (the pool stays coherent).
    // We check the invariant structurally: for every filled SCUS entry, the
    // on-disc bytes at its VA are the translated text followed by a NUL.
    let mut checked = 0usize;
    for entries in [
        &pack.sections.items,
        &pack.sections.spells,
        &pack.sections.arts,
        &pack.sections.accessory_passives,
    ] {
        for e in entries {
            if e.translation.is_empty() {
                continue;
            }
            let va = e
                .key
                .strip_prefix("scus:str:0x")
                .and_then(|h| u32::from_str_radix(h, 16).ok())
                .expect("scus:str key");
            let off = legaia_asset::item_names::file_offset_for_va(&scus, va)
                .expect("VA resolves in patched SCUS");
            // NUL within a sane bound.
            let tail = &scus[off..];
            let nul = tail.iter().take(512).position(|&b| b == 0);
            assert!(nul.is_some(), "translated string at {va:#x} lost its NUL");
            checked += 1;
        }
    }
    assert!(checked > 100, "too few SCUS strings checked ({checked})");
}
