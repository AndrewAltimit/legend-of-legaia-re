//! Disc + save-library gated: every catalogued save state's `resident_patch`
//! field in `scripts/scenarios.toml` agrees with the executable the state
//! actually holds in RAM.
//!
//! `SCUS_942.54` is read from the disc once, at boot, so a save state made on
//! a patched disc keeps the patched executable in RAM through every later
//! load of that state, whatever disc it is loaded onto. The manifest records
//! which catalogued states do (`resident_patch = "<family>"`); this test
//! re-measures it from the state bytes at the patcher's own hook sites, both
//! ways: a state marked patched must differ from the retail executable at a
//! site, and a state left unmarked must match it at every site. The same site
//! table drives `scripts/pcsx-redux/patch_taint_audit.py states`; the method
//! is on docs/tooling/pcsx-redux-automation.md#patched-disc-taint.
//!
//! Only site addresses and lengths appear here - no disc bytes. Skips (passes)
//! when `extracted/SCUS_942.54`, the manifest or `saves/library` is absent.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const SCUS_VA: u32 = 0x8001_0000;
const SCUS_HEADER: usize = 0x800;
const RAM_MASK: u32 = 0x001F_FFFF;

/// `(va, len)` patcher sites - each one a window a patcher feature rewrites in
/// the resident executable (hooks, the rewritten new-game seed code, the
/// quick-travel name cells, and the shared verified-dead arenas the
/// hand-assembled routines live in). Mirrors `SITES` in
/// `scripts/pcsx-redux/patch_taint_audit.py`.
const SITES: &[(u32, usize)] = &[
    (0x8003_21D4, 8),
    (0x8004_AD0C, 4),
    (0x8005_1990, 8),
    (0x8005_1A20, 8),
    (0x8003_44D8, 8),
    (0x8003_4ADC, 16),
    (0x8003_4B04, 40),
    (0x8007_3B18, 0x100),
    (0x8001_2DD0, 45),
    (0x8005_4008, 1),
    (0x8007_7728, 0x100),
    (0x8007_8A88, 0x44),
    (0x8007_ACA0, 0x60),
    (0x8007_AE00, 0x100),
];

fn find_up(rel: &str) -> Option<PathBuf> {
    ["", "../", "../../"]
        .iter()
        .map(|p| PathBuf::from(format!("{p}{rel}")))
        .find(|p| p.exists())
}

/// `fingerprint -> (label, resident_patch)` for every manifest line carrying
/// a 64-hex fingerprint. The manifest writes `resident_patch` on the line
/// right after the fingerprint it describes.
fn manifest_states(text: &str) -> BTreeMap<String, (String, Option<String>)> {
    let mut out = BTreeMap::new();
    let lines: Vec<&str> = text.lines().collect();
    let mut label = String::new();
    for (i, line) in lines.iter().enumerate() {
        let t = line.trim();
        if let Some(rest) = t.strip_prefix("label = \"") {
            label = rest.trim_end_matches('"').to_string();
        }
        if !t.starts_with("backup_fingerprint") {
            continue;
        }
        let Some(fp) = t.split('"').nth(1) else {
            continue;
        };
        if fp.len() != 64 {
            continue;
        }
        let patch = lines
            .get(i + 1)
            .and_then(|n| n.trim().strip_prefix("resident_patch = \""))
            .map(|v| v.trim_end_matches('"').to_string());
        out.insert(fp.to_string(), (label.clone(), patch));
    }
    out
}

fn patched_sites(ram: &[u8], retail: &[u8]) -> Vec<u32> {
    SITES
        .iter()
        .filter(|&&(va, len)| {
            let r = (va - SCUS_VA) as usize;
            let m = (va & RAM_MASK) as usize;
            ram[m..m + len] != retail[r..r + len]
        })
        .map(|&(va, _)| va)
        .collect()
}

fn state_ram(path: &Path) -> Option<Vec<u8>> {
    match path.extension().and_then(|e| e.to_str()) {
        Some("sstate") => legaia_pcsxr::SaveState::from_path(path)
            .ok()
            .map(|s| s.main_ram().to_vec()),
        Some("mcr") => legaia_mednafen::container::SaveState::from_path(path)
            .ok()
            .and_then(|s| s.main_ram().ok().map(<[u8]>::to_vec)),
        _ => None,
    }
}

#[test]
fn resident_patch_field_matches_the_resident_executable() {
    let Some(scus_path) = find_up("extracted/SCUS_942.54") else {
        eprintln!("[skip] extracted/SCUS_942.54 missing");
        return;
    };
    let Some(manifest) = find_up("scripts/scenarios.toml") else {
        eprintln!("[skip] scripts/scenarios.toml missing");
        return;
    };
    let Some(library) = find_up("saves/library") else {
        eprintln!("[skip] saves/library missing (capture-gated)");
        return;
    };
    if std::env::var_os("LEGAIA_SCUS").is_none() {
        // SAFETY: single-threaded test setup before any SaveState load.
        unsafe { std::env::set_var("LEGAIA_SCUS", &scus_path) };
    }
    let scus = std::fs::read(&scus_path).expect("read SCUS");
    let retail = &scus[SCUS_HEADER..];
    let states = manifest_states(&std::fs::read_to_string(&manifest).expect("read manifest"));

    let mut checked = 0usize;
    let mut marked = 0usize;
    let mut errors = Vec::new();
    for (fp, (label, patch)) in &states {
        let path = [
            library.join(format!("pcsx-redux/{fp}.sstate")),
            library.join(format!("mednafen/{fp}.mcr")),
        ]
        .into_iter()
        .find(|p| p.exists());
        let Some(path) = path else { continue };
        let Some(ram) = state_ram(&path) else {
            continue;
        };
        checked += 1;
        let sites = patched_sites(&ram, retail);
        match (patch, sites.is_empty()) {
            (Some(_), false) => marked += 1,
            (None, true) => {}
            (Some(p), true) => errors.push(format!(
                "{label}: marked resident_patch = {p:?} but every patcher site is retail"
            )),
            (None, false) => errors.push(format!(
                "{label}: unmarked, but the resident executable differs from retail at {:x?}",
                sites
            )),
        }
    }
    eprintln!("[ok] {checked} catalogued states measured, {marked} carry a patched executable");
    assert!(
        checked >= 1,
        "library present but no catalogued state readable"
    );
    assert!(errors.is_empty(), "{}", errors.join("\n"));
}
