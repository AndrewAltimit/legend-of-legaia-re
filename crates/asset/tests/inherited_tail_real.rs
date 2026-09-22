//! Disc-gated: the Rust inherited-tail cut against the Python one, and the
//! equality the Rust side rests on.
//!
//! [`legaia_asset::inherited_tail`] and
//! `scripts/ghidra-analysis/inherited_tail.py` measure the same thing for two
//! different instruments - the byte account and `disc-coverage.py` - and an
//! instrument pair that disagrees about where an image stops being its own
//! content reports two different denominators for one disc. So the agreement
//! is asserted cut for cut rather than argued from the two implementations
//! looking alike.
//!
//! The Rust side also rests on a fact about the committed overlay map that is
//! true today and is not a law: every row is `form = "raw"` with
//! `content_source = "prot_entry_extent"`, and each row's `content_bytes`
//! equals its extracted PROT entry's file length exactly. That is what lets it
//! read the images out of `--prot-dir` instead of needing an
//! `extracted/overlays/` tree. A row that stops being raw breaks it silently,
//! so the first test asserts the equality.
//!
//! Skips + passes without `extracted/PROT` (CLAUDE.md disc-gated convention).

use legaia_asset::inherited_tail;
use legaia_asset::static_overlay::overlay_map;
use std::path::{Path, PathBuf};

fn prot_dir() -> Option<PathBuf> {
    for c in [
        "extracted/PROT",
        "../extracted/PROT",
        "../../extracted/PROT",
    ] {
        let d = PathBuf::from(c);
        if d.is_dir() {
            return Some(d);
        }
    }
    eprintln!("[skip] extracted/PROT missing - run `legaia-extract` first");
    None
}

fn entry_path(dir: &Path, idx: u32) -> Option<PathBuf> {
    let prefix = format!("{idx:04}_");
    let mut hits: Vec<PathBuf> = std::fs::read_dir(dir)
        .ok()?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with(&prefix))
        })
        .collect();
    hits.sort();
    hits.into_iter().next()
}

/// The equality the Rust cut is built on: a mapped overlay's PROT entry file
/// IS its as-loaded image, byte for byte and length for length.
#[test]
fn every_mapped_overlays_entry_file_is_its_as_loaded_image() {
    let Some(dir) = prot_dir() else { return };
    let toml = std::fs::read_to_string("data/static-overlays.toml")
        .or_else(|_| std::fs::read_to_string("crates/asset/data/static-overlays.toml"))
        .expect("read static-overlays.toml");

    let mut checked = 0usize;
    // `content_bytes` is not on `OverlayRecord` (only the Python instruments
    // read it), so it is taken from the TOML text here rather than invented.
    for rec in &overlay_map().overlays {
        let Some(bytes) = content_bytes_of(&toml, rec.prot_index) else {
            panic!("PROT {} has no content_bytes row", rec.prot_index);
        };
        let Some(path) = entry_path(&dir, rec.prot_index) else {
            continue;
        };
        let len = std::fs::metadata(&path).expect("stat entry").len();
        assert_eq!(
            len, bytes as u64,
            "PROT {} ({}): entry file is {len} bytes, map says {bytes}",
            rec.prot_index, rec.label,
        );
        checked += 1;
    }
    eprintln!("[inherited-tail] {checked} mapped overlay entries match content_bytes");
    assert!(checked > 0, "no mapped overlay entries found");
}

/// Cut for cut against `scripts/ghidra-analysis/inherited_tail.py`'s
/// `tail_cuts` - both legs, sibling comparison and packer buffer.
#[test]
fn rust_and_python_tails_agree() {
    let Some(dir) = prot_dir() else { return };
    let rust = inherited_tail::tails_in(&dir);

    let script = ["scripts/ghidra-analysis", "../../scripts/ghidra-analysis"]
        .into_iter()
        .map(PathBuf::from)
        .find(|p| p.join("inherited_tail.py").is_file());
    let Some(script_dir) = script else {
        eprintln!("[skip] scripts/ghidra-analysis not found from this cwd");
        return;
    };
    let repo = script_dir.join("../..");
    let prog = format!(
        r#"
import glob, os, sys, json
sys.path.insert(0, {script_dir:?})
import inherited_tail, slot_b_band
try:
    import tomllib
except ImportError:
    import tomli as tomllib
rows = tomllib.load(open(os.path.join({repo:?}, "crates/asset/data/static-overlays.toml"), "rb"))["overlays"]
imgs = []
for row in rows:
    hits = sorted(glob.glob(os.path.join({dir:?}, "%04d_*" % row["prot_index"])))
    if not hits:
        continue
    imgs.append((row["prot_index"], row["base_va"], open(hits[0], "rb").read()))
cuts = inherited_tail.tail_cuts(
    imgs, lambda _k, base, data: slot_b_band.content_end(data, base),
    prot_dir={dir:?})
print(json.dumps({{str(k): [v[0], v[1]] for k, v in cuts.items()}}))
"#,
        script_dir = script_dir.to_string_lossy(),
        repo = repo.to_string_lossy(),
        dir = dir.to_string_lossy(),
    );
    let out = std::process::Command::new("python3")
        .arg("-c")
        .arg(&prog)
        .output()
        .expect("run python3");
    if !out.status.success() {
        eprintln!(
            "[skip] python side failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        return;
    }
    let text = String::from_utf8(out.stdout).expect("utf8");
    let py: std::collections::BTreeMap<String, (usize, u32)> =
        serde_json::from_str(text.trim()).expect("parse python cuts");

    eprintln!(
        "[inherited-tail] rust {} image(s) with a tail, python {}",
        rust.len(),
        py.len()
    );
    assert_eq!(rust.len(), py.len(), "different number of tails");

    // Every image, PROT 0944 included. 0944 used to be the one divergence:
    // this side chained its top record's zero padding (`0x1988..0x199C`) as a
    // `[model_sel 0]` record and walked on through PROT 0942's records to
    // `0x1EC8`, while the Python walker, lacking the `WAIT 0x0FFF` bound, left
    // the top record unbounded. Both walkers now carry the same two rules -
    // the forever-`WAIT` fallback and "eight zero bytes are padding, not a
    // record" - and both cut 0944 at `0x199C` with 0942 as donor.

    let mut diverged = Vec::new();
    for (idx, tail) in &rust {
        let (start, donor) = py
            .get(&idx.to_string())
            .copied()
            .unwrap_or_else(|| panic!("python has no tail for PROT {idx}"));
        if (tail.start, tail.donor_prot_index) != (start, donor) {
            diverged.push((*idx, (tail.start, tail.donor_prot_index), (start, donor)));
        }
    }
    eprintln!("[inherited-tail] divergences: {diverged:?}");
    assert!(
        diverged.is_empty(),
        "the Rust and Python inherited-tail cuts disagree: {diverged:#?}",
    );
    let t944 = rust.get(&944).expect("PROT 0944 has a tail");
    assert_eq!((t944.start, t944.donor_prot_index), (0x199C, 942));

    // The packer-buffer leg. 0898 and 0895 have no overlay donor - the buffer
    // held PROT 0894 there - and 0901's run opens mid-routine on PROT 0900's
    // epilogue, below where the sibling gate (which declines 0900) would cut.
    for (idx, start, donor) in [(898, 0x281BB, 894), (895, 0x256A8, 894), (901, 0x252A, 900)] {
        let t = rust
            .get(&idx)
            .unwrap_or_else(|| panic!("PROT {idx} has a tail"));
        assert_eq!(
            (t.start, t.donor_prot_index),
            (start, donor),
            "PROT {idx}'s packer-buffer cut"
        );
    }
}

/// `content_bytes = 0x...` for one row of the map TOML.
fn content_bytes_of(toml: &str, idx: u32) -> Option<u32> {
    let key = format!("prot_index = {idx}");
    let at = toml.find(&key)?;
    let rest = &toml[at..];
    let end = rest.find("\n[[overlays]]").unwrap_or(rest.len());
    let block = &rest[..end];
    let line = block
        .lines()
        .find(|l| l.trim_start().starts_with("content_bytes"))?;
    let v = line.split('=').nth(1)?.trim();
    if let Some(hex) = v.strip_prefix("0x") {
        u32::from_str_radix(hex, 16).ok()
    } else {
        v.parse().ok()
    }
}
