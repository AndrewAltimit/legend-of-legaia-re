//! Disc-gated corpus sweep for the paired gold-charge scan
//! ([`legaia_asset::inn_costs`]).
//!
//! Sweeps every PROT entry that resolves as a scene bundle, scans each
//! decoded MAN for the op-`0x4E` gold-compare + negative `0x3A`
//! `ADD_MONEY` pair, and asserts the structural invariants:
//!
//!  - the corpus is non-vacuous (multiple scenes carry paired charges,
//!    both compare widths are represented);
//!  - every paired cost is positive and below the retail gold clamp;
//!  - every hit indexes real bytes (`0x4E` / `0x3A` at the reported
//!    offsets) and the debit trails its gate inside the pair window.
//!
//! Skips silently when `extracted/PROT.DAT` or `LEGAIA_DISC_BIN` is missing.

use legaia_asset::inn_costs;
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

fn extracted_cdname() -> Option<PathBuf> {
    [
        PathBuf::from("extracted/CDNAME.TXT"),
        PathBuf::from("../../extracted/CDNAME.TXT"),
    ]
    .into_iter()
    .find(|p| p.is_file())
}

#[test]
fn gold_charge_pairs_resolve_across_the_scene_corpus() {
    let Some(prot_dat) = extracted_prot_dat() else {
        eprintln!("[skip] extracted/PROT.DAT missing");
        return;
    };
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    }
    let names = extracted_cdname().and_then(|p| legaia_prot::cdname::parse(&p).ok());

    let mut archive = Archive::open(&prot_dat).expect("open PROT.DAT");
    let entries = archive.entries.clone();
    let mut buf = Vec::new();

    let mut scenes_with_charges = 0usize;
    let mut total_sites = 0usize;
    let mut sub3 = 0usize;
    let mut sub10 = 0usize;

    for (idx, entry) in entries.iter().enumerate() {
        archive.read_entry(entry, &mut buf).expect("read entry");
        let Some(located) = inn_costs::locate(&buf) else {
            continue;
        };
        scenes_with_charges += 1;
        let label = names
            .as_ref()
            .and_then(|n| legaia_prot::cdname::block_for_extraction_index(n, idx as u32))
            .unwrap_or("?");
        for site in &located.charges {
            total_sites += 1;
            match site.sub_op {
                3 => sub3 += 1,
                10 => sub10 += 1,
                other => panic!("unexpected compare sub-op {other}"),
            }
            assert!(site.cost > 0 && site.cost <= 9_999_999);
            assert_eq!(located.decoded[site.compare_off], 0x4E);
            assert_eq!(located.decoded[site.add_money_off], 0x3A);
            assert!(
                site.add_money_off > site.compare_off && site.add_money_off - site.compare_off < 40
            );
            eprintln!(
                "entry {idx:04} ({label}) cost {} sub{} (0x4E at {:#x}, 0x3A at {:#x})",
                site.cost, site.sub_op, site.compare_off, site.add_money_off
            );
        }
    }

    eprintln!("scenes with paired charges: {scenes_with_charges}, sites: {total_sites}");
    assert!(
        scenes_with_charges >= 4,
        "expected several charge scenes; got {scenes_with_charges}"
    );
    assert!(sub3 >= 4, "expected u16 gates (inn class); got {sub3}");
    assert!(
        sub10 >= 1,
        "expected a u32 gate (casino class); got {sub10}"
    );
}
