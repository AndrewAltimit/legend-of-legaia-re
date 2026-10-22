//! The disc a patched-disc state was made on.
//!
//! A state tagged `resident_patch` replays a patched build: its RAM holds the
//! patch's executable, overlays and streamed assets. Scored against the
//! engine on the retail disc, every difference the patch makes reads as an
//! engine divergence - a party-swap capture's fighter is the patch's mesh
//! where the engine draws the unpatched member. A scenario that names the
//! image it booted (`resident_disc`) is run on that image's own files
//! instead, so the distance left is the engine's.
//!
//! The image is looked up by file name, never committed: in the directories
//! `LEGAIA_PATCHED_DISCS` lists (`:`-separated), else beside
//! `LEGAIA_DISC_BIN` and one directory up (where `legaia-patcher` writes its
//! output beside the input image or its folder). Its `PROT.DAT`,
//! `CDNAME.TXT` and `SCUS_942.54` are extracted once into a cache directory
//! under the report directory (gitignored, like every other Sony-derived
//! byte the run writes).

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

/// The files an extracted root needs for the engine side of a state.
const ROOT_FILES: [&str; 3] = ["PROT.DAT", "CDNAME.TXT", "SCUS_942.54"];

/// The directories a `resident_disc` name is looked up in.
pub fn resident_disc_dirs() -> Vec<PathBuf> {
    if let Some(v) = std::env::var_os("LEGAIA_PATCHED_DISCS") {
        return std::env::split_paths(&v).collect();
    }
    let Some(bin) = std::env::var_os("LEGAIA_DISC_BIN") else {
        return Vec::new();
    };
    let bin = PathBuf::from(bin);
    let mut out = Vec::new();
    if let Some(dir) = bin.parent() {
        out.push(dir.to_path_buf());
        if let Some(up) = dir.parent() {
            out.push(up.to_path_buf());
        }
    }
    out
}

/// The first `dirs` entry holding a file called `name`.
pub fn locate_resident_disc(name: &str, dirs: &[PathBuf]) -> Option<PathBuf> {
    dirs.iter().map(|d| d.join(name)).find(|p| p.is_file())
}

/// The cache directory for `image` under `cache_root`: one per image name.
fn cache_dir(image: &Path, cache_root: &Path) -> PathBuf {
    let stem = image
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    cache_root.join(stem)
}

/// The stamp a cache directory keeps of the image it was extracted from: a
/// re-written image (same name, new bytes) re-extracts.
fn stamp_of(image: &Path) -> Result<String> {
    let meta = std::fs::metadata(image).with_context(|| format!("stat {}", image.display()))?;
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_secs());
    Ok(format!("{} {} {mtime}\n", image.display(), meta.len()))
}

/// Extract `image`'s engine files into a cache directory under `cache_root`
/// (reused while its stamp matches) and return that extracted root.
pub fn extract_resident_disc(image: &Path, cache_root: &Path) -> Result<PathBuf> {
    let dir = cache_dir(image, cache_root);
    let stamp = stamp_of(image)?;
    let stamp_path = dir.join("source.txt");
    if std::fs::read_to_string(&stamp_path).ok().as_deref() == Some(stamp.as_str())
        && ROOT_FILES.iter().all(|f| dir.join(f).is_file())
    {
        return Ok(dir);
    }
    std::fs::create_dir_all(&dir).with_context(|| format!("create {}", dir.display()))?;
    use legaia_engine_scene::vfs::Vfs;
    let vfs = legaia_engine_scene::vfs::DiscVfs::open(image)
        .with_context(|| format!("open disc image {}", image.display()))?;
    for want in ROOT_FILES {
        let bytes = vfs
            .read(&want.to_ascii_lowercase())
            .with_context(|| format!("{want} on {}", image.display()))?;
        std::fs::write(dir.join(want), &bytes)
            .with_context(|| format!("write {}/{want}", dir.display()))?;
    }
    std::fs::write(&stamp_path, stamp)?;
    Ok(dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locate_takes_the_first_directory_holding_the_name() {
        let base = std::env::temp_dir().join(format!("rc-resident-{}", std::process::id()));
        let (a, b) = (base.join("a"), base.join("b"));
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(&b).unwrap();
        std::fs::write(b.join("x.bin"), b"").unwrap();
        let dirs = vec![a.clone(), b.clone()];
        assert_eq!(locate_resident_disc("x.bin", &dirs), Some(b.join("x.bin")));
        assert_eq!(locate_resident_disc("y.bin", &dirs), None);
        std::fs::write(a.join("x.bin"), b"").unwrap();
        assert_eq!(locate_resident_disc("x.bin", &dirs), Some(a.join("x.bin")));
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn cache_dir_is_keyed_on_the_image_name() {
        let d = cache_dir(Path::new("/d/Legaia.patcher-1.bin"), Path::new("/c"));
        assert_eq!(d, Path::new("/c/Legaia.patcher-1"));
    }
}
