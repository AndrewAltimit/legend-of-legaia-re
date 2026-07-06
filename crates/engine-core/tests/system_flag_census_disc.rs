//! Disc-gated: the disc-wide SYSTEM-flag census over every scene MAN.
//!
//! The field VM's `0x50..=0x7F` op family drives the wide SYSTEM-flag bitmap
//! (`0x5x` SET / `0x6x` CLEAR / `0x7x` TEST) - the id space of the overworld
//! progress gates. A gate like `system_flag_test(0x193)` lives in one scene,
//! but the *setter* that opens it almost always lives in a different scene's
//! MAN. [`system_flag_census`] walks every CDNAME scene's MAN across all three
//! partitions and maps each flag to the sites that touch it, so the setters can
//! be found regardless of which scene gates on the flag.
//!
//! Skips + passes when `LEGAIA_DISC_BIN` / extracted assets are missing
//! (CLAUDE.md disc-gated convention).

use legaia_engine_core::man_field_scripts::{FlagCensusSite, system_flag_census};
use legaia_engine_core::scene::ProtIndex;
use legaia_engine_vm::field_disasm::FlagKind;
use std::collections::BTreeMap;
use std::path::PathBuf;

fn extracted_dir() -> Option<PathBuf> {
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

fn run_census() -> Option<BTreeMap<u16, Vec<FlagCensusSite>>> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    let extracted = extracted_dir().or_else(|| {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        None
    })?;
    let index = ProtIndex::open_extracted(&extracted).expect("open ProtIndex");
    let scenes = index.cdname_scene_names();
    eprintln!("[census] scanning {} CDNAME scenes", scenes.len());
    Some(system_flag_census(&index, &scenes))
}

#[test]
fn system_flag_census_finds_setters_across_scenes() {
    let Some(census) = run_census() else { return };

    assert!(
        !census.is_empty(),
        "the disc-wide SYSTEM-flag census must surface at least one flag site",
    );

    // Non-vacuous: at least one flag has a SET site (a genuine writer, not
    // only gates). The overworld progress-gate RE needs setters, so prove the
    // census actually recovers them.
    let total_sites: usize = census.values().map(Vec::len).sum();
    let setter_flags: Vec<u16> = census
        .iter()
        .filter(|(_, hits)| hits.iter().any(|h| h.kind == FlagKind::Set))
        .map(|(f, _)| *f)
        .collect();
    eprintln!(
        "[census] {} distinct flags, {} total sites, {} flags with a SET site",
        census.len(),
        total_sites,
        setter_flags.len(),
    );
    assert!(
        !setter_flags.is_empty(),
        "census must find at least one SYSTEM-flag SET site (a real setter)",
    );

    // Report the setters for the overworld progress gates the RE is chasing.
    for &target in &[0x193u16, 0x482, 0x2FC, 549, 550, 551] {
        match census.get(&target) {
            Some(hits) => {
                eprintln!(
                    "[census] flag 0x{target:04X} ({target}): {} site(s)",
                    hits.len()
                );
                for h in hits {
                    eprintln!(
                        "    {:?} scene={} P{}[{}] op=0x{:02X}",
                        h.kind, h.scene_name, h.partition, h.record, h.opcode,
                    );
                }
            }
            None => eprintln!("[census] flag 0x{target:04X} ({target}): no sites"),
        }
    }
}
