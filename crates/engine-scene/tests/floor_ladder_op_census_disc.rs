//! Disc-gated census: which records arm the floor-height ladder ops, per
//! scene MAN.
//!
//! Op `0x4C` nibble 9 owns the sixteen-rung ladder at `0x1F80035C`: sub-`E`
//! installs every rung (and the MAN-header copy), sub-`0` arms one rung's
//! oscillator, sub-`F` retires them all. On the return from a battle the MAN
//! loader `FUN_8003AEB0` re-seats the ladder from the MAN header and retires
//! every oscillator, and only the scene system script `P1[0]` is re-installed
//! (`FUN_8003AB2C`). So which record carries each site decides what a fight
//! leaves: a `P1[0]` site comes back, a site in any other record does not
//! until that record runs again.
//!
//! Printed with `--nocapture` (one row per scene, record and op).
//!
//! Skips + passes when `LEGAIA_DISC_BIN` / extracted assets are missing.

use legaia_engine_field::man_field_scripts::partition_record_span;
use legaia_engine_scene::man_census::scene_man_carriers;
use legaia_engine_scene::scene::{ProtIndex, Scene};
use legaia_scene_formats::field_disasm::LinearWalker;
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

/// `(scene, carrier entry, partition, record, sub) -> sites`.
type Sites = BTreeMap<(String, u32, usize, usize, u8), usize>;

fn census() -> Option<Sites> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    let extracted = extracted_dir().or_else(|| {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        None
    })?;
    let index = ProtIndex::open_extracted(&extracted).expect("open ProtIndex");
    let mut out = Sites::new();
    for name in index.cdname_scene_names() {
        let Ok(scene) = Scene::load(&index, &name) else {
            continue;
        };
        for carrier in scene_man_carriers(&index, &scene) {
            let man = &carrier.payload;
            let Ok(man_file) = legaia_scene_formats::man_section::parse(man) else {
                continue;
            };
            for partition in 0..man_file.partitions.len() {
                for record in 0..man_file.partitions[partition].len() {
                    let Some((start, pc0, len)) =
                        partition_record_span(&man_file, man, partition, record)
                    else {
                        continue;
                    };
                    let body = &man[start..start + len];
                    for insn in LinearWalker::new(body, pc0).flatten() {
                        if insn.opcode != 0x4C {
                            continue;
                        }
                        let hs = if insn.extended.is_some() { 2 } else { 1 };
                        let Some(&sub) = body.get(insn.pc + hs) else {
                            continue;
                        };
                        if sub >> 4 != 9 {
                            continue;
                        }
                        *out.entry((name.clone(), carrier.entry_idx, partition, record, sub))
                            .or_default() += 1;
                    }
                }
            }
        }
    }
    Some(out)
}

#[test]
fn floor_ladder_op_sites_by_record_or_skip() {
    let Some(sites) = census() else {
        return;
    };
    let mut outside_system = 0usize;
    for ((scene, entry, p, r, sub), n) in &sites {
        let system = *p == 1 && *r == 0;
        if !system && (*sub == 0x90 || *sub == 0x9E) {
            outside_system += n;
        }
        println!(
            "{scene:<10} entry {entry:>4} P{p}[{r:>2}] 4C {sub:02X} x{n}{}",
            if system { "  (system script)" } else { "" }
        );
    }
    println!("install / arm sites outside P1[0]: {outside_system}");
    assert!(
        sites.keys().any(|k| k.4 == 0x90),
        "the disc arms floor-ladder oscillators"
    );
}
