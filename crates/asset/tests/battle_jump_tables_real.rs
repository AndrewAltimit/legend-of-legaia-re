//! Disc-gated: the battle overlay's head jump tables against the retail image
//! and against the dump corpus's function extents.
//!
//! [`legaia_asset::battle_jump_tables::check`] re-derives every row from the
//! image's own `lui`/`addiu` / `sltiu` / `jr` words. The second test asks the
//! question a table descriptor is worth nothing without: does every arm land in
//! the same dumped function as the `jr` that dispatches it? The extents come
//! from the committed `scripts/ghidra-analysis/dump-extent-attribution.csv`, so
//! the test needs no dump corpus. Skips + passes without `extracted/PROT`.

use legaia_asset::battle_jump_tables::{JUMP_TABLES, OVERLAY_BASE_VA, check};
use std::path::PathBuf;

fn image() -> Option<Vec<u8>> {
    for c in [
        "extracted/PROT",
        "../extracted/PROT",
        "../../extracted/PROT",
    ] {
        let d = PathBuf::from(c);
        let Ok(rd) = std::fs::read_dir(&d) else {
            continue;
        };
        for e in rd.flatten() {
            let p = e.path();
            if p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("0898_"))
            {
                return std::fs::read(p).ok();
            }
        }
    }
    eprintln!("[skip] extracted/PROT/0898_* missing");
    None
}

#[test]
fn every_row_is_re_derived_from_the_retail_image() {
    let Some(img) = image() else { return };
    let errs = check(&img);
    assert!(errs.is_empty(), "{errs:#?}");
    eprintln!("[battle-jump-tables] {} tables verified", JUMP_TABLES.len());
}

#[test]
fn every_arm_lands_in_the_dumped_function_that_dispatches_it() {
    let Some(img) = image() else { return };
    let csv = ["scripts/ghidra-analysis", "../../scripts/ghidra-analysis"]
        .into_iter()
        .map(|d| PathBuf::from(d).join("dump-extent-attribution.csv"))
        .find(|p| p.is_file())
        .expect("dump-extent-attribution.csv");
    let text = std::fs::read_to_string(csv).expect("read csv");
    let extents: Vec<(u32, u32)> = text
        .lines()
        .skip(1)
        .filter_map(|l| {
            let f: Vec<&str> = l.splitn(5, ',').collect();
            (f.get(2) == Some(&"battle_action(898)")).then(|| {
                let s = u32::from_str_radix(f[0], 16).ok()?;
                let n: u32 = f[1].parse().ok()?;
                Some((s, s + n))
            })?
        })
        .collect();
    assert!(!extents.is_empty(), "no battle_action(898) rows");
    let mut arms = 0usize;
    for t in &JUMP_TABLES {
        let words: Vec<u32> = (0..t.arms as usize)
            .map(|i| {
                let o = (t.va - OVERLAY_BASE_VA) as usize + 4 * i;
                u32::from_le_bytes(img[o..o + 4].try_into().unwrap())
            })
            .collect();
        arms += words.len();
        assert!(
            extents
                .iter()
                .any(|&(s, e)| (s..e).contains(&t.jr) && words.iter().all(|a| (s..e).contains(a))),
            "table {:#010x}: no dumped extent holds both its jr and all its arms",
            t.va
        );
    }
    eprintln!("[battle-jump-tables] {arms} arms inside their dispatcher's dumped extent");
}
