//! Disc + save-library gated: for every PCSX-Redux state in the library, the
//! game-agnostic structural RAM location and the Legaia anchor search land on
//! the same payload offset.
//!
//! The structural walk replaced the anchor search as the reader's primary
//! route; this test is what keeps that change from moving any Legaia caller's
//! RAM. It reads only offsets, never bytes, into its assertions. Skips (passes)
//! when `extracted/SCUS_942.54` or `saves/library/pcsx-redux` is absent.

use std::path::{Path, PathBuf};

use legaia_pcsx_redux::{sstate_payload, structural_ram_offset};

fn find(rel: &str) -> Option<PathBuf> {
    ["", "../", "../../"]
        .iter()
        .map(|p| Path::new(p).join(rel))
        .find(|p| p.exists())
}

#[test]
fn structural_ram_offset_equals_the_anchor_offset_for_every_library_state() {
    let Some(scus) = std::env::var_os("LEGAIA_SCUS")
        .map(PathBuf::from)
        .or_else(|| find("extracted/SCUS_942.54"))
    else {
        eprintln!("[skip] extracted/SCUS_942.54 absent (LEGAIA_SCUS unset)");
        return;
    };
    let Some(dir) = find("saves/library/pcsx-redux") else {
        eprintln!("[skip] saves/library/pcsx-redux absent (capture-gated)");
        return;
    };
    let scus = std::fs::read(scus).expect("read SCUS");
    let mut paths: Vec<PathBuf> = std::fs::read_dir(&dir)
        .expect("list library")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "sstate"))
        .collect();
    paths.sort();

    let (mut agreed, mut anchorless) = (0usize, 0usize);
    for p in &paths {
        let raw = std::fs::read(p).expect("read state");
        let payload = sstate_payload(&raw).expect("payload");
        let structural = structural_ram_offset(&payload)
            .unwrap_or_else(|| panic!("{}: no memory submessage", p.display()));
        match legaia_ram_image::extract::main_ram_via_anchor_with_scus(&payload, &scus) {
            Ok(ram) => {
                let anchor = ram.as_ptr() as usize - payload.as_ptr() as usize;
                assert_eq!(
                    structural,
                    anchor,
                    "{}: structural RAM offset {structural:#x} != anchor offset {anchor:#x}",
                    p.display()
                );
                agreed += 1;
            }
            // A state caught before the executable is resident (BIOS boot)
            // has no anchor; the structural route still opens it.
            Err(_) => anchorless += 1,
        }
    }
    eprintln!(
        "[structural-vs-anchor] {} states: {agreed} agree, {anchorless} carry no anchor",
        paths.len()
    );
    assert!(agreed >= 1, "no library state exercised the comparison");
}
