//! Disc-gated check of `ui_menu` / `system_text` relocation: a menu label or
//! system line longer than its span moves - into its pool's compaction, the
//! space ledger's translation region, or (in the executable) the name pools'
//! free runs - and every instruction or word that formed its address forms
//! the new one.
//!
//! The oracle is semantic, not a byte diff: for every string the pools hold,
//! every site that formed its address on the retail disc is decoded on the
//! patched disc, and the string read there must be the one the import says
//! it wrote (the translation when applied, the retail text otherwise). A
//! string that stayed put is read at its own address. Every touched sector
//! stays EDC/ECC-valid, and the patch is byte-deterministic.
//!
//! Skips + passes without `LEGAIA_DISC_BIN`.

use std::collections::{BTreeMap, BTreeSet};

use legaia_iso::raw::SECTOR_SIZE;
use legaia_patcher::disc::DiscPatcher;
use legaia_patcher::space_ledger::{self, Image, Owner};
use legaia_patcher::translation::code_refs::{self, RETAIL_GP};
use legaia_patcher::translation::code_strings::{self, CodeStrings};
use legaia_patcher::translation::markup::{self, Target};
use legaia_patcher::translation::ui;
use legaia_patcher::translation::{LanguagePack, export_pack, import_pack};

fn load_disc() -> Option<Vec<u8>> {
    let p = std::path::PathBuf::from(std::env::var_os("LEGAIA_DISC_BIN")?);
    p.is_file().then(|| std::fs::read(&p).ok()).flatten()
}

fn va_of(key: &str) -> u32 {
    u32::from_str_radix(key.rsplit(':').next().unwrap().trim_start_matches("0x"), 16).unwrap()
}

/// `(image id, bytes, base)`: the executable (`usize::MAX`) and each overlay
/// the pools cover.
fn images(p: &DiscPatcher) -> Vec<(usize, Vec<u8>, u32)> {
    let mut out = vec![(
        usize::MAX,
        p.read_named_file("SCUS_942.54").unwrap(),
        ui::SCUS_POOL_BASE_VA,
    )];
    let prots: BTreeSet<usize> = ui::UI_STRING_POOLS.iter().map(|x| x.prot_index).collect();
    for prot in prots {
        out.push((
            prot,
            p.read_entry(prot).unwrap(),
            ui::overlay_base_va(prot).unwrap(),
        ));
    }
    out
}

/// The same foreign set the importer uses.
fn build(imgs: &[(usize, Vec<u8>, u32)], id: usize) -> CodeStrings {
    let (_, bytes, base) = imgs.iter().find(|i| i.0 == id).unwrap();
    let foreign: Vec<(&[u8], u32)> = if id == usize::MAX {
        imgs.iter()
            .filter(|i| [897, 898, 899].contains(&i.0))
            .map(|i| (i.1.as_slice(), i.2))
            .collect()
    } else {
        let mut f: Vec<(&[u8], u32)> = vec![(imgs[0].1.as_slice(), imgs[0].2)];
        for &c in code_strings::co_resident(id) {
            if let Some(i) = imgs.iter().find(|i| i.0 == c) {
                f.push((i.1.as_slice(), i.2));
            }
        }
        f
    };
    CodeStrings::build(bytes, *base, code_strings::pools_of(id), &foreign)
}

fn read_str(img: &[u8], base: u32, va: u32, strict: bool) -> Vec<u8> {
    let off = (va - base) as usize;
    let n = ui::pool_strlen(img, off, strict).unwrap_or(0);
    img[off..off + n].to_vec()
}

fn check_sectors(original: &[u8], patched: &[u8]) {
    for (i, (a, b)) in original
        .chunks(SECTOR_SIZE)
        .zip(patched.chunks(SECTOR_SIZE))
        .enumerate()
    {
        if a != b && a.len() == SECTOR_SIZE {
            assert!(
                legaia_iso::write::mode2_form1_sector_is_valid(b),
                "sector {i} invalid"
            );
        }
    }
}

/// Fill every `ui_menu` / `system_text` entry via `f(i, source)`.
fn fill(pack: &mut LanguagePack, f: impl Fn(usize, &str) -> Option<String>) {
    let s = &mut pack.sections;
    for (i, e) in s
        .ui_menu
        .iter_mut()
        .chain(s.system_text.iter_mut())
        .enumerate()
    {
        if let Some(t) = f(i, &e.source) {
            e.translation = t;
        }
    }
}

/// Import `pack` onto a copy of `original` and check every retail reference
/// of every pool string reaches the text the import says it wrote. Returns
/// `(patched image, moved count, moved into the ledger region)`.
fn import_and_check(original: &[u8], pack: &LanguagePack) -> (Vec<u8>, usize, usize, usize) {
    let src = DiscPatcher::open(original.to_vec()).unwrap();
    let before = images(&src);
    let mut dst = DiscPatcher::open(original.to_vec()).unwrap();
    let report = import_pack(&mut dst, pack).expect("import");
    let after = images(&dst);
    let applied: BTreeSet<&str> = report.applied_keys.iter().map(String::as_str).collect();
    let filled: BTreeMap<(usize, u32), Vec<u8>> = pack
        .sections
        .ui_menu
        .iter()
        .chain(&pack.sections.system_text)
        .filter(|e| e.is_filled() && applied.contains(e.key.as_str()))
        .map(|e| {
            let prot = if e.key.starts_with("scus:") {
                usize::MAX
            } else {
                e.key.split(':').nth(1).unwrap().parse().unwrap()
            };
            (
                (prot, va_of(&e.key)),
                markup::encode(&e.translation, Target::CString).unwrap(),
            )
        })
        .collect();
    let ledger: Vec<(u32, u32)> = space_ledger::REGIONS
        .iter()
        .filter(|r| r.owner == Owner::Translation)
        .map(|r| (r.start_va, r.end_va))
        .collect();
    let mut in_ledger = 0;
    for &(_, to) in report.trace.moved.values() {
        if ledger.iter().any(|&(s, e)| (s..e).contains(&to)) {
            in_ledger += 1;
        }
    }
    let mut checked = 0;
    for (id, retail, base) in &before {
        let cs = build(&before, *id);
        let patched = &after.iter().find(|i| i.0 == *id).unwrap().1;
        for va in cs.vas() {
            let strict = ui::pool_for(*id, va).is_some_and(|p| p.strict);
            let want = filled
                .get(&(*id, va))
                .cloned()
                .unwrap_or_else(|| read_str(retail, *base, va, strict));
            let sites = cs.sites(va);
            if sites.is_empty() {
                // Never moves: read in place.
                assert_eq!(
                    read_str(patched, *base, va, strict),
                    want,
                    "image {id} string {va:#x} (unreferenced) changed"
                );
                continue;
            }
            for site in sites {
                let at = code_refs::site_address(patched, site, RETAIL_GP).unwrap_or_else(|| {
                    panic!("image {id} {va:#x}: site {site:?} no longer decodes")
                });
                assert_eq!(
                    read_str(patched, *base, at, strict),
                    want,
                    "image {id} string {va:#x}: site {site:?} now forms {at:#x}"
                );
                checked += 1;
            }
        }
    }
    assert!(checked > 100, "only {checked} sites checked");
    check_sectors(original, dst.image());
    let moved_system = report
        .trace
        .moved
        .keys()
        .filter(|k| {
            k.starts_with("scus:") && pack.sections.system_text.iter().any(|e| &e.key == *k)
        })
        .count();
    (
        dst.image().to_vec(),
        report.relocated_strings,
        in_ledger,
        moved_system,
    )
}

#[test]
fn longer_ui_and_system_strings_move_and_every_reference_follows() {
    let Some(original) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let src = DiscPatcher::open(original.clone()).expect("open disc");
    let english = export_pack(&src).expect("export");

    // The ledger's translation region is zero and unreferenced on retail.
    let ov = src.read_entry(space_ledger::MENU_OVERLAY).unwrap();
    let spans = space_ledger::translation_spans(
        Image::Prot(space_ledger::MENU_OVERLAY),
        &ov,
        ui::overlay_base_va(space_ledger::MENU_OVERLAY).unwrap(),
    );
    let r = space_ledger::REGIONS
        .iter()
        .find(|r| r.owner == Owner::Translation)
        .unwrap();
    assert_eq!(
        spans,
        vec![(r.start_va, r.end_va)],
        "ledger region not zero on retail"
    );

    // Pass 1: a length-shuffled pack - a third shorter, a third the same, a
    // third longer than English.
    let mut pack = english.clone();
    fill(&mut pack, |i, s| match i % 3 {
        0 => Some(s.chars().take((s.chars().count() / 2).max(1)).collect()),
        1 => None,
        _ => Some(format!("{s} extra")),
    });
    let (img1, moved1, ledger1, sys1) = import_and_check(&original, &pack);
    eprintln!(
        "[ran] shuffled: {moved1} strings moved ({sys1} system_text), {ledger1} into the ledger region"
    );
    assert!(moved1 > 0, "nothing moved");
    assert!(sys1 > 0, "no system_text string moved");

    // Determinism.
    let (img1b, _, _, _) = import_and_check(&original, &pack);
    assert!(img1 == img1b, "import is not byte-deterministic");

    // Pass 2: everything longer. Most cannot all fit; the ones that do not
    // stay retail (the oracle checks that too).
    let mut pack = english.clone();
    fill(&mut pack, |_, s| Some(format!("{s} (traduzido)")));
    let (_, moved2, ledger2, _) = import_and_check(&original, &pack);
    eprintln!("[ran] all-longer: {moved2} strings moved, {ledger2} into the ledger region");
    assert!(
        ledger2 > 0,
        "the menu overlay's translation region took nothing"
    );
}
