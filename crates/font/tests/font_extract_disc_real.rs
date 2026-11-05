//! `font-extract --disc` end to end: the command the extraction guide prints.
//! Skips and passes without `LEGAIA_DISC_BIN` and an extracted `SCUS_942.54`.

use legaia_font::Font;
use std::path::PathBuf;
use std::process::Command;

#[test]
fn disc_mode_writes_a_font_the_loader_reads() {
    let Ok(disc) = std::env::var("LEGAIA_DISC_BIN") else {
        eprintln!("LEGAIA_DISC_BIN unset - skipping");
        return;
    };
    let scus = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../extracted/SCUS_942.54");
    if !scus.is_file() {
        eprintln!("extracted/SCUS_942.54 not present - skipping");
        return;
    }
    let root = std::env::temp_dir().join(format!("font-extract-disc-{}", std::process::id()));
    let out = root.join("font");
    let status = Command::new(env!("CARGO_BIN_EXE_font-extract"))
        .arg("--disc")
        .arg(&disc)
        .arg("--scus")
        .arg(&scus)
        .arg("--out")
        .arg(&out)
        .status()
        .expect("run font-extract");
    assert!(status.success(), "font-extract --disc failed: {status}");
    Font::load_from_extracted(&root).expect("the loader reads what --disc wrote");
    std::fs::remove_dir_all(&root).ok();
}
