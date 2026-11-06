//! The retail comparison corpus's result cache: one state's report (and its
//! side-by-side PNG) reused when nothing that could move it has changed.
//!
//! The key is a SHA-256 over everything a state's result is a function of:
//! the engine binary's bytes (which is also the image child's), the disc
//! (`PROT.DAT`, `SCUS_942.54`, `CDNAME.TXT` by content, every other file under
//! the extracted root by path, size and mtime), the state file and memory card
//! by content, the state's manifest row, a resident disc's stamp, the run's
//! frame mode and seed order, and every `LEGAIA_*` / `WGPU_*` variable in the
//! environment. A rebuilt binary, a re-extracted disc or a re-captured state
//! therefore misses; what stays outside the key is the GPU driver.
//!
//! A cache is for iterating. The ratchet never reads one: the CLI refuses
//! `--cache` with `--write-baseline` / `--check-baseline`, and a state that
//! failed for a transient reason (an image child with no frame, a failed seed)
//! is not stored. Entries hold retail pixels and live under the gitignored
//! `target/`; `docs/tooling/retail-compare.md` has the operator's view.

use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::retail_compare::{CorpusEntry, RunOptions, StateReport};

/// Bumped when the entry format or what a report means changes.
const FORMAT: u32 = 1;
/// Per-binary generations kept; older ones are deleted on open.
const KEEP_GENERATIONS: usize = 3;

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Feed a file's bytes into `h`, length-prefixed.
fn hash_file(h: &mut Sha256, path: &Path) -> Result<()> {
    let mut f = std::fs::File::open(path).with_context(|| format!("open {}", path.display()))?;
    let mut buf = vec![0u8; 1 << 20];
    let mut len = 0u64;
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
        len += n as u64;
    }
    h.update(len.to_le_bytes());
    Ok(())
}

fn hash_str(h: &mut Sha256, s: &str) {
    h.update((s.len() as u64).to_le_bytes());
    h.update(s.as_bytes());
}

/// Path, size and mtime of every file under `dir`, sorted, skipping `skip`.
fn hash_tree_stats(h: &mut Sha256, root: &Path, skip: &[&str]) -> Result<()> {
    let mut stack = vec![root.to_path_buf()];
    let mut rows = Vec::new();
    while let Some(dir) = stack.pop() {
        for ent in std::fs::read_dir(&dir).with_context(|| format!("list {}", dir.display()))? {
            let ent = ent?;
            let path = ent.path();
            let meta = std::fs::metadata(&path)?;
            if meta.is_dir() {
                stack.push(path);
                continue;
            }
            let rel = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .display()
                .to_string();
            if skip.contains(&rel.as_str()) {
                continue;
            }
            let mtime = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map_or(0, |d| d.as_nanos());
            rows.push(format!("{rel} {} {mtime}", meta.len()));
        }
    }
    rows.sort();
    for r in rows {
        hash_str(h, &r);
    }
    Ok(())
}

/// An entry on disk. The floats go through their bits as well: JSON parsing
/// may land an ulp off a 17-digit value, and a cached report must print the
/// bytes the measured one printed.
#[derive(Serialize, Deserialize)]
struct Entry {
    format: u32,
    report: StateReport,
    bits: Vec<u64>,
}

fn report_floats(r: &mut StateReport) -> Vec<&mut f64> {
    let mut out: Vec<&mut f64> = r.channels.values_mut().collect();
    out.extend(r.score.as_mut());
    if let Some(i) = r.image.as_mut() {
        out.extend([
            &mut i.mae,
            &mut i.within,
            &mut i.retail_luma,
            &mut i.engine_luma,
        ]);
    }
    out
}

/// A cache generation: one directory per run-wide input hash.
pub struct ResultCache {
    dir: PathBuf,
    base: [u8; 32],
}

impl ResultCache {
    /// Open the generation for this binary, disc and run mode under `root`,
    /// deleting all but the newest few others.
    pub fn open(root: &Path, opts: &RunOptions<'_>) -> Result<Self> {
        let mut h = Sha256::new();
        h.update(FORMAT.to_le_bytes());
        hash_file(&mut h, &std::env::current_exe()?)?;
        const CONTENT: [&str; 3] = ["PROT.DAT", "SCUS_942.54", "CDNAME.TXT"];
        for f in CONTENT {
            hash_file(&mut h, &opts.extracted.join(f))?;
        }
        hash_tree_stats(&mut h, opts.extracted, &CONTENT)?;
        hash_str(&mut h, &format!("{:?}", opts.order));
        hash_str(&mut h, &format!("{:?}", opts.engine_exe.is_some()));
        hash_str(
            &mut h,
            &format!("{:?}", crate::retail_compare_image::frame_render()),
        );
        let env: BTreeMap<String, String> = std::env::vars_os()
            .filter_map(|(k, v)| Some((k.into_string().ok()?, v.into_string().ok()?)))
            .filter(|(k, _)| k.starts_with("LEGAIA_") || k.starts_with("WGPU_"))
            .collect();
        for (k, v) in env {
            hash_str(&mut h, &k);
            hash_str(&mut h, &v);
        }
        let base: [u8; 32] = h.finalize().into();
        let dir = root.join(&hex(&base)[..16]);
        std::fs::create_dir_all(&dir).with_context(|| format!("create {}", dir.display()))?;
        prune(root, &dir);
        Ok(Self { dir, base })
    }

    /// This state's key, or `None` when an input cannot be read (it then
    /// simply runs).
    pub fn key(&self, entry: &CorpusEntry) -> Option<String> {
        let mut h = Sha256::new();
        h.update(self.base);
        hash_str(&mut h, &entry.label);
        hash_str(&mut h, entry.emulator);
        hash_str(&mut h, &entry.fingerprint);
        hash_str(&mut h, &format!("{:?}", entry.resident_patch));
        hash_str(&mut h, &format!("{:?}", entry.resident_disc));
        hash_str(&mut h, &format!("{:?}", entry.ram_injected));
        hash_file(&mut h, &entry.path).ok()?;
        match &entry.staging_source {
            Some((emulator, source)) => {
                hash_str(&mut h, emulator);
                hash_file(&mut h, source).ok()?;
            }
            None => hash_str(&mut h, "no staging source"),
        }
        match &entry.memory_card {
            Some(Ok(card)) => hash_file(&mut h, card).ok()?,
            other => hash_str(&mut h, &format!("{other:?}")),
        }
        if let Some(name) = &entry.resident_disc {
            let dirs = crate::retail_compare::resident_disc::resident_disc_dirs();
            match crate::retail_compare::resident_disc::locate_resident_disc(name, &dirs) {
                Some(image) => {
                    hash_str(
                        &mut h,
                        &crate::retail_compare::resident_disc::stamp_of(&image).ok()?,
                    );
                }
                None => hash_str(&mut h, "resident disc not found"),
            }
        }
        Some(hex(&h.finalize()))
    }

    /// The stored report for `key`, its PNG restored under the run's report
    /// directory. Anything missing or unreadable is a miss.
    pub fn fetch(
        &self,
        key: &Option<String>,
        entry: &CorpusEntry,
        opts: &RunOptions<'_>,
    ) -> Option<StateReport> {
        let key = key.as_ref()?;
        let text = std::fs::read_to_string(self.dir.join(format!("{key}.json"))).ok()?;
        let mut e: Entry = serde_json::from_str(&text).ok()?;
        if e.format != FORMAT || e.report.label != entry.label {
            return None;
        }
        {
            let floats = report_floats(&mut e.report);
            if floats.len() != e.bits.len() {
                return None;
            }
            for (f, b) in floats.into_iter().zip(&e.bits) {
                *f = f64::from_bits(*b);
            }
        }
        let png = self.dir.join(format!("{key}.png"));
        if png.is_file()
            && let Some(out) = opts.out_dir
        {
            std::fs::copy(&png, out.join(format!("{}.png", entry.label))).ok()?;
        }
        Some(e.report)
    }

    /// Store a finished state, unless it failed for a reason a re-run might
    /// not repeat. The rename makes a concurrent or interrupted run safe.
    pub fn store(
        &self,
        key: &Option<String>,
        entry: &CorpusEntry,
        report: &StateReport,
        opts: &RunOptions<'_>,
    ) {
        let Some(key) = key.as_ref() else { return };
        let frame_failed = report
            .detail
            .get("image")
            .is_some_and(|d| d.starts_with("engine frame failed"));
        if frame_failed || report.unseeded.starts_with("seeding failed") {
            return;
        }
        let mut copy = report.clone();
        let bits = report_floats(&mut copy)
            .iter()
            .map(|f| f.to_bits())
            .collect();
        let stored = Entry {
            format: FORMAT,
            report: copy,
            bits,
        };
        let Ok(text) = serde_json::to_string(&stored) else {
            return;
        };
        if let Some(out) = opts.out_dir {
            let png = out.join(format!("{}.png", entry.label));
            if png.is_file() {
                let tmp = self.dir.join(format!("{key}.png.tmp"));
                if std::fs::copy(&png, &tmp).is_err()
                    || std::fs::rename(&tmp, self.dir.join(format!("{key}.png"))).is_err()
                {
                    return;
                }
            }
        }
        let tmp = self.dir.join(format!("{key}.json.tmp"));
        if std::fs::write(&tmp, text).is_ok() {
            let _ = std::fs::rename(&tmp, self.dir.join(format!("{key}.json")));
        }
    }
}

/// Delete the generations under `root` beyond the newest few, `current` kept.
fn prune(root: &Path, current: &Path) {
    let Ok(rd) = std::fs::read_dir(root) else {
        return;
    };
    let mut gens: Vec<(std::time::SystemTime, PathBuf)> = rd
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir() && p != current)
        .filter_map(|p| Some((std::fs::metadata(&p).ok()?.modified().ok()?, p)))
        .collect();
    gens.sort();
    gens.reverse();
    for (_, p) in gens.into_iter().skip(KEEP_GENERATIONS - 1) {
        let _ = std::fs::remove_dir_all(p);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::retail_compare::StateClass;
    use crate::retail_compare_image::ImageScore;

    fn report() -> StateReport {
        StateReport {
            label: "x".into(),
            emulator: "pcsx-redux".into(),
            fingerprint: "f".into(),
            scene: "town01".into(),
            game_mode: 3,
            class: StateClass::Field,
            unseeded: String::new(),
            channels: BTreeMap::from([("camera".to_string(), 1.0 / 3.0)]),
            detail: BTreeMap::new(),
            score: Some(2.0 / 3.0),
            image: Some(ImageScore {
                mae: 3.989_699_590_773_809_6,
                within: 0.1,
                retail_luma: 81.632_577_357_687_45,
                engine_luma: 83.973_537_890_611_55,
                note: String::new(),
            }),
            resident_patch: None,
            resident_disc_missing: None,
        }
    }

    /// An entry's floats come back bit for bit, so a cached report prints the
    /// bytes the measured one did.
    #[test]
    fn floats_survive_the_entry_bit_for_bit() {
        let mut r = report();
        let bits: Vec<u64> = report_floats(&mut r).iter().map(|f| f.to_bits()).collect();
        let text = serde_json::to_string(&Entry {
            format: FORMAT,
            report: r.clone(),
            bits,
        })
        .unwrap();
        let mut e: Entry = serde_json::from_str(&text).unwrap();
        for (f, b) in report_floats(&mut e.report).into_iter().zip(&e.bits) {
            *f = f64::from_bits(*b);
        }
        assert_eq!(
            serde_json::to_string(&e.report).unwrap(),
            serde_json::to_string(&r).unwrap()
        );
    }

    #[test]
    fn prune_keeps_the_current_generation_and_the_newest_others() {
        let dir = tempfile::tempdir().unwrap();
        let cur = dir.path().join("cur");
        std::fs::create_dir(&cur).unwrap();
        for n in 0..5 {
            std::fs::create_dir(dir.path().join(format!("old{n}"))).unwrap();
        }
        prune(dir.path(), &cur);
        let left = std::fs::read_dir(dir.path()).unwrap().count();
        assert_eq!(left, KEEP_GENERATIONS);
        assert!(cur.is_dir());
    }
}
