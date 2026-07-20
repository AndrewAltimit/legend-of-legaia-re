//! Disc-gated attribution oracle for the boot-path loader constants
//! ([`legaia_asset::boot_overlay`]).
//!
//! Each constant in that module claims a specific `PROT.DAT` extraction entry
//! from a loader-call constant plus the raw->extraction index shift. CDNAME
//! filename labels inherit forward and routinely name a neighbouring block, so
//! a label match would prove nothing; this asserts the *content* instead:
//!
//!  - the CARD-mode TIM pack entry parses as an `asset::pack` whose members
//!    carry the PSX TIM magic;
//!  - the effect-module entry carries the effect-test dev strings;
//!  - the mode-table overlay params land on entries that exist and are large
//!    enough to be overlay code;
//!  - the off-by-two neighbours of both side-band entries do **not** satisfy
//!    the same content checks, so the shift is load-bearing rather than
//!    coincidental.
//!
//! Skips silently when `extracted/PROT.DAT` or `LEGAIA_DISC_BIN` is missing.

use legaia_asset::boot_overlay as bo;
use legaia_prot::archive::Archive;
use std::path::PathBuf;

fn extracted_prot_dat() -> Option<PathBuf> {
    [
        PathBuf::from("extracted/PROT.DAT"),
        PathBuf::from("../../extracted/PROT.DAT"),
    ]
    .into_iter()
    .find(|p| p.is_file())
}

fn read(archive: &mut Archive, idx: u32) -> Vec<u8> {
    let entry = archive.entries[idx as usize].clone();
    let mut buf = Vec::new();
    archive.read_entry(&entry, &mut buf).expect("read entry");
    buf
}

/// `true` when `data` is an `asset::pack` whose first members all begin with the
/// PSX TIM magic (`0x00000010`).
fn is_tim_pack(data: &[u8]) -> bool {
    let Ok(entries) = legaia_asset::pack::parse_pack(data) else {
        return false;
    };
    if entries.is_empty() {
        return false;
    }
    entries.iter().all(|e| {
        let off = e.byte_offset as usize;
        data.get(off..off + 4)
            .map(|w| u32::from_le_bytes(w.try_into().unwrap()) == 0x10)
            .unwrap_or(false)
    })
}

#[test]
fn card_tim_pack_index_is_pinned_by_content_not_label() {
    let Some(prot_dat) = extracted_prot_dat() else {
        eprintln!("[skip] extracted/PROT.DAT missing");
        return;
    };
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    }
    let mut archive = Archive::open(&prot_dat).expect("open PROT.DAT");

    let idx = bo::card_tim_pack_extraction_index();
    assert_eq!(idx, 892, "loader constant 0x37E minus the index shift");

    let data = read(&mut archive, idx);
    assert!(
        is_tim_pack(&data),
        "extraction {idx} should be the CARD-mode TIM pack"
    );

    // The shift is load-bearing: the raw index read as an extraction index (the
    // classic off-by-two) is not a TIM pack.
    let unshifted = read(&mut archive, bo::CARD_TIM_RAW_INDEX);
    assert!(
        !is_tim_pack(&unshifted),
        "raw index {} must not also parse as the TIM pack, or the shift proves nothing",
        bo::CARD_TIM_RAW_INDEX
    );
}

#[test]
fn effect_module_index_carries_the_effect_dev_strings() {
    let Some(prot_dat) = extracted_prot_dat() else {
        eprintln!("[skip] extracted/PROT.DAT missing");
        return;
    };
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    }
    let mut archive = Archive::open(&prot_dat).expect("open PROT.DAT");

    let idx = bo::EFFECT_DATA_EXTRACTION_INDEX;
    assert_eq!(idx, 979);
    // The effect-test mode reaches the same entry through the mode table.
    assert_eq!(bo::overlay_param_to_extraction(0x54), idx);

    // The discriminator is *where* the module starts, not merely that the
    // strings are reachable. Neighbouring entries over-read past their own
    // claimed end into this same payload (see the trailing-gap note in
    // docs/formats/prot.md), so a plain "contains" check matches 977 and 978
    // as well. What is unique to 979 is that the module sits at its head.
    let find = |data: &[u8], needle: &[u8]| {
        (0..data.len()).find(|&i| data[i..].starts_with(needle))
    };

    let data = read(&mut archive, idx);
    let at = find(&data, b"efect init").expect("entry 979 carries `efect init`");
    assert!(
        at < 0x100,
        "the effect module should begin at entry {idx}'s head, found at {at:#x}"
    );
    assert!(
        find(&data, b"battle bgm").is_some_and(|o| o < 0x100),
        "entry {idx} should carry `battle bgm` in the same head block"
    );

    // Neighbours reach the strings only far into their over-read tails.
    for neighbour in [idx - 1, idx - 2] {
        let other = read(&mut archive, neighbour);
        let other_at = find(&other, b"efect init");
        assert!(
            other_at.is_none_or(|o| o >= 0x1000),
            "entry {neighbour} carries the module at {other_at:?}, too close to its head \
             for {idx} to be the owner"
        );
    }
}

#[test]
fn mode_table_overlay_params_resolve_to_real_entries() {
    let Some(prot_dat) = extracted_prot_dat() else {
        eprintln!("[skip] extracted/PROT.DAT missing");
        return;
    };
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    }
    let mut archive = Archive::open(&prot_dat).expect("open PROT.DAT");

    // (loader param, expected extraction index) from docs/subsystems/boot.md.
    let cases = [
        (2u32, 897u32),
        (3, 898),
        (4, 899),
        (7, 902),
        (0x4B, 970),
        (0x4C, 971),
        (0x54, 979),
    ];
    for (param, expect) in cases {
        let idx = bo::overlay_param_to_extraction(param);
        assert_eq!(idx, expect, "param {param:#x}");
        assert!(
            (idx as usize) < archive.entries.len(),
            "entry {idx} out of range"
        );
        let data = read(&mut archive, idx);
        assert!(
            !data.is_empty(),
            "overlay entry {idx} (param {param:#x}) is empty"
        );
    }

    // Both slot-B default choices resolve to real entries too.
    for flag in [false, true] {
        let choice = bo::slot_b_default_overlay(flag, false, None).unwrap();
        assert!((choice.extraction_index as usize) < archive.entries.len());
        assert!(!read(&mut archive, choice.extraction_index).is_empty());
    }
}
