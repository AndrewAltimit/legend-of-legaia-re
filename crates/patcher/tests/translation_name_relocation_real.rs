//! Disc-gated check of SCUS name relocation: a translated item / spell / art /
//! accessory name longer than its in-place span moves into the name pools'
//! free bytes and every table slot is repointed.
//!
//! Fills every movable name with a length-shuffled translation (some shorter,
//! some longer than retail - the shape a real language pack has), imports it,
//! and re-reads the patched executable through the **retail** table slots:
//! each slot resolves to its translation (or its retail text when unfilled),
//! no executable byte outside the movable spans and the slots changes, and
//! every touched sector stays EDC/ECC-valid. A second pass grows every name
//! and checks the ones that cannot fit are reported and left retail.
//!
//! Skips + passes without `LEGAIA_DISC_BIN`.

use std::collections::{BTreeMap, BTreeSet};

use legaia_asset::item_names;
use legaia_iso::raw::SECTOR_SIZE;
use legaia_patcher::disc::DiscPatcher;
use legaia_patcher::translation::export::name_table_refs;
use legaia_patcher::translation::markup::{self, Target};
use legaia_patcher::translation::name_pool::NamePool;
use legaia_patcher::translation::{LanguagePack, export_pack, import_pack};

const SECTIONS: [&str; 5] = [
    "items",
    "item_types",
    "spells",
    "arts",
    "accessory_passives",
];

fn load_disc() -> Option<Vec<u8>> {
    let p = std::path::PathBuf::from(std::env::var_os("LEGAIA_DISC_BIN")?);
    p.is_file().then(|| std::fs::read(&p).ok()).flatten()
}

fn cstr(scus: &[u8], va: u32) -> Vec<u8> {
    let off = item_names::file_offset_for_va(scus, va).expect("va maps");
    let n = scus[off..]
        .iter()
        .position(|&b| b == 0)
        .expect("terminated");
    scus[off..off + n].to_vec()
}

fn va_of(key: &str) -> u32 {
    u32::from_str_radix(key.rsplit(':').next().unwrap().trim_start_matches("0x"), 16).unwrap()
}

/// Fill every movable name of the five SCUS name sections via `f(i, source)`.
fn fill(pack: &mut LanguagePack, pool: &NamePool, f: impl Fn(usize, &str) -> String) {
    let movable: BTreeSet<u32> = pool.movable_spans().iter().map(|s| s.0).collect();
    let mut i = 0;
    let s = &mut pack.sections;
    for entries in [
        &mut s.items,
        &mut s.item_types,
        &mut s.spells,
        &mut s.arts,
        &mut s.accessory_passives,
    ] {
        for e in entries.iter_mut() {
            // Plain-text names only, so the length shuffle can't split a token.
            if !e.key.starts_with("scus:str:")
                || !movable.contains(&va_of(&e.key))
                || e.source.contains(['{', '|'])
            {
                continue;
            }
            e.translation = f(i, &e.source);
            i += 1;
        }
    }
}

/// Every retail table slot -> the text it should now reach.
fn expected(original_scus: &[u8], pack: &LanguagePack) -> BTreeMap<u32, Vec<u8>> {
    let filled: BTreeMap<u32, Vec<u8>> = pack
        .sections
        .iter()
        .filter(|(n, _)| SECTIONS.contains(n))
        .flat_map(|(_, es)| es)
        .filter(|e| e.is_filled())
        .map(|e| {
            (
                va_of(&e.key),
                markup::encode(&e.translation, Target::CString).unwrap(),
            )
        })
        .collect();
    let mut out = BTreeMap::new();
    for (va, refs) in name_table_refs(original_scus) {
        if item_names::file_offset_for_va(original_scus, va).is_none() {
            continue;
        }
        let want = filled
            .get(&va)
            .cloned()
            .unwrap_or_else(|| cstr(original_scus, va));
        for slot in refs.slots {
            out.insert(slot, want.clone());
        }
    }
    out
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

#[test]
fn longer_names_relocate_and_every_slot_follows() {
    let Some(original) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let src = DiscPatcher::open(original.clone()).expect("open disc");
    let scus0 = src.read_named_file("SCUS_942.54").unwrap();
    let pool = NamePool::build(&scus0);
    assert!(
        pool.movable_spans().len() > 600,
        "movable names: {}",
        pool.movable_spans().len()
    );

    // Lengths shuffled by -5..=+3 around retail, deterministic: every third
    // name or so grows past its span, paid for by the ones that shrink (a
    // relocated string starts 4-byte aligned, so growth and shrinkage of equal
    // character counts do not cancel byte for byte).
    let mut pack = export_pack(&src).expect("export");
    fill(&mut pack, &pool, |i, s| {
        let d = (i * 7 % 9) as isize - 5;
        let mut t: String = s.chars().rev().collect();
        if d < 0 {
            t.truncate((t.len() as isize + d).max(1) as usize);
        } else {
            t.push_str(&"q".repeat(d as usize));
        }
        t
    });
    let mut patcher = DiscPatcher::open(original.clone()).expect("open disc");
    let report = import_pack(&mut patcher, &pack).expect("import");
    let name_issues: Vec<_> = report
        .issues
        .iter()
        .filter(|(k, _)| k.starts_with("scus:str:"))
        .collect();
    assert!(name_issues.is_empty(), "skipped: {name_issues:?}");
    assert!(
        report.relocated_names > 50,
        "relocated {}",
        report.relocated_names
    );

    let patched = patcher.into_image();
    let post = DiscPatcher::open(patched.clone()).expect("open patched");
    let scus1 = post.read_named_file("SCUS_942.54").unwrap();
    assert_eq!(scus0.len(), scus1.len(), "same-size edit");
    let want = expected(&scus0, &pack);
    for (slot, text) in &want {
        let off = item_names::file_offset_for_va(&scus1, *slot).unwrap();
        let ptr = u32::from_le_bytes(scus1[off..off + 4].try_into().unwrap());
        assert_eq!(&cstr(&scus1, ptr), text, "slot 0x{slot:08x} -> 0x{ptr:08x}");
    }
    // Nothing outside the movable spans and the slot words changed.
    let mut allowed = vec![false; scus0.len()];
    for (_, off, len) in pool.movable_spans() {
        allowed[off..off + len].fill(true);
    }
    for slot in want.keys() {
        let off = item_names::file_offset_for_va(&scus0, *slot).unwrap();
        allowed[off..off + 4].fill(true);
    }
    for (i, (a, b)) in scus0.iter().zip(&scus1).enumerate() {
        assert!(
            a == b || allowed[i],
            "SCUS byte 0x{i:x} changed outside the pools"
        );
    }
    check_sectors(&original, &patched);
}

#[test]
fn a_name_with_no_room_is_reported_and_left_retail() {
    let Some(original) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let src = DiscPatcher::open(original.clone()).expect("open disc");
    let scus0 = src.read_named_file("SCUS_942.54").unwrap();
    let pool = NamePool::build(&scus0);
    // Every name grows: the pools have no spare bytes to give.
    let mut pack = export_pack(&src).expect("export");
    fill(&mut pack, &pool, |_, s| format!("{s}xxxx"));
    let mut patcher = DiscPatcher::open(original.clone()).expect("open disc");
    let report = import_pack(&mut patcher, &pack).expect("import");
    let refused: BTreeSet<String> = report
        .issues
        .iter()
        .filter(|(_, m)| m.contains("no free run"))
        .map(|(k, _)| k.clone())
        .collect();
    assert!(!refused.is_empty());

    // The refused names still read retail; the placed ones read translated.
    let patched = patcher.into_image();
    let post = DiscPatcher::open(patched.clone()).expect("open patched");
    let scus1 = post.read_named_file("SCUS_942.54").unwrap();
    let refs = name_table_refs(&scus0);
    for (name, entries) in pack.sections.iter() {
        if !SECTIONS.contains(&name) {
            continue;
        }
        for e in entries.iter().filter(|e| e.is_filled()) {
            let va = va_of(&e.key);
            let slot = refs[&va].slots[0];
            let off = item_names::file_offset_for_va(&scus1, slot).unwrap();
            let ptr = u32::from_le_bytes(scus1[off..off + 4].try_into().unwrap());
            let got = cstr(&scus1, ptr);
            let want = if refused.contains(&e.key) {
                cstr(&scus0, va)
            } else {
                markup::encode(&e.translation, Target::CString).unwrap()
            };
            assert_eq!(got, want, "{}", e.key);
        }
    }
    check_sectors(&original, &patched);
}
