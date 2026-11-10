//! The player's screenshot key (`Print Screen`,
//! [`legaia_engine_session::hotkeys::HostHotkey::Screenshot`]): the frame the
//! window presents, saved as a PNG in the player's pictures folder.
//!
//! The browser play page's twin downloads the composited canvas
//! (`site/js/play-view-frame.js`, `_savePlayerShot`).

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use legaia_engine_render::CaptureImage;

/// The folder screenshots land in: `<pictures>/Legend of Legaia`, else
/// `<data dir>/screenshots` (`legaia-engine` under the platform data dir),
/// else `screenshots` beside the working directory.
pub fn screenshot_dir() -> PathBuf {
    if let Some(p) = dirs::picture_dir() {
        return p.join("Legend of Legaia");
    }
    if let Some(d) = crate::launcher::data_dir() {
        return d.join("screenshots");
    }
    PathBuf::from("screenshots")
}

/// The file a shot taken now of `scene` is written to: the scene label and
/// the wall-clock second, with a counter when a second already holds one.
pub fn screenshot_path(dir: &Path, scene: &str, unix_secs: u64) -> PathBuf {
    let scene: String = scene
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '_')
        .collect();
    let stem = if scene.is_empty() {
        format!("legaia-{unix_secs}")
    } else {
        format!("legaia-{scene}-{unix_secs}")
    };
    let mut path = dir.join(format!("{stem}.png"));
    let mut n = 2;
    while path.exists() {
        path = dir.join(format!("{stem}-{n}.png"));
        n += 1;
    }
    path
}

/// Write `img` into [`screenshot_dir`] and answer where it went.
pub fn save(img: &CaptureImage, scene: &str) -> Result<PathBuf> {
    let dir = screenshot_dir();
    std::fs::create_dir_all(&dir).with_context(|| format!("create {}", dir.display()))?;
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let path = screenshot_path(&dir, scene, secs);
    crate::window::write_capture_png(&path, img)?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shots_in_one_second_do_not_overwrite_each_other() {
        let dir = std::env::temp_dir().join(format!("legaia-shot-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let a = screenshot_path(&dir, "town01", 7);
        assert_eq!(a.file_name().unwrap(), "legaia-town01-7.png");
        std::fs::write(&a, b"x").unwrap();
        let b = screenshot_path(&dir, "town01", 7);
        assert_eq!(b.file_name().unwrap(), "legaia-town01-7-2.png");
        assert_eq!(
            screenshot_path(&dir, "", 7).file_name().unwrap(),
            "legaia-7.png"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
