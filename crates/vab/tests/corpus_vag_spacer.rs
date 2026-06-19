//! Disc-gated: `vag_table[0]` is a reserved spacer, not a master pitch shift.
//!
//! The VAB VAG size table is 1-indexed (`vag_table[1..=vs]` hold the sizes), so
//! `vag_table[0]` is a leading spacer. This scans every VAB in the extracted
//! PROT corpus and asserts the spacer is universally `0` - the empirical
//! confirmation that it carries no master pitch / sample-rate correction (see
//! [`legaia_vab::VabReport::vag_table_spacer`]).
//!
//! Skip-passes without disc data / extracted assets (CLAUDE.md convention): it
//! reads only `extracted/PROT/*.BIN`, which exist after `legaia-extract`.

use std::path::PathBuf;

use legaia_vab::{find_vabs, parse};

fn extracted_prot_dir() -> Option<PathBuf> {
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c).join("PROT");
        if d.is_dir() {
            return Some(d);
        }
    }
    None
}

#[test]
fn vag_table_spacer_is_zero_across_corpus() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(prot) = extracted_prot_dir() else {
        eprintln!("[skip] extracted/PROT missing - run `legaia-extract` first");
        return;
    };

    let mut vab_count = 0usize;
    for entry in std::fs::read_dir(&prot).expect("read extracted/PROT") {
        let path = entry.expect("dir entry").path();
        if path.extension().and_then(|e| e.to_str()) != Some("BIN") {
            continue;
        }
        let data = std::fs::read(&path).expect("read PROT entry");
        for off in find_vabs(&data) {
            // A few magic hits are truncated banks (a multi-bank VAB spilling
            // past a single PROT entry, or a false-positive magic match); only
            // assert on VABs that fully parse within this buffer.
            let Ok(report) = parse(&data, off) else {
                continue;
            };
            assert_eq!(
                report.vag_table_spacer,
                0,
                "vag_table[0] is a reserved spacer (expected 0) in {} @ 0x{:08X}",
                path.display(),
                off,
            );
            vab_count += 1;
        }
    }

    assert!(
        vab_count > 100,
        "expected the extracted corpus to surface many VABs, found {vab_count}"
    );
    eprintln!("[ok]    vag_table[0] == 0 across {vab_count} corpus VABs");
}
