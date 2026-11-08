//! How the desktop programs treat their own settings files: Explorer's
//! `settings.json` and the launcher's `launcher.toml`.
//!
//! Two rules, shared so the programs cannot drift apart:
//!
//! - A file that exists but does not parse (a hand edit gone wrong, a write
//!   cut off by a crash or a full disk) is never silently replaced by the
//!   defaults. [`set_aside`] renames it to `<name>.broken` (or
//!   `<name>.broken-2`, ... when an older one is still there) before the
//!   program carries on with the defaults, so the next save cannot overwrite
//!   the player's recent list and switches; [`set_aside_notice`] is the
//!   sentence that says so.
//! - [`write_atomic`] writes the new contents beside the file and renames
//!   them over it, so a crash mid-write leaves the old file whole instead of
//!   half of the new one.

use std::format;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::string::{String, ToString};

/// The suffix [`set_aside`] gives a file that did not parse.
pub const BROKEN_SUFFIX: &str = ".broken";

/// How many numbered backups [`set_aside`] tries before it reuses the last.
const MAX_BACKUPS: u32 = 20;

/// `path` with `suffix` appended to its file name.
fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(suffix);
    path.with_file_name(name)
}

/// Rename the unreadable settings file at `path` out of the way, to the
/// first free name of `<name>.broken`, `<name>.broken-2`, ... Returns the
/// name it now has.
pub fn set_aside(path: &Path) -> io::Result<PathBuf> {
    let mut target = with_suffix(path, BROKEN_SUFFIX);
    let mut n = 2;
    while target.exists() && n <= MAX_BACKUPS {
        target = with_suffix(path, &format!("{BROKEN_SUFFIX}-{n}"));
        n += 1;
    }
    fs::rename(path, &target)?;
    Ok(target)
}

/// The sentence a program shows once it has set its settings file aside:
/// what happened and where the old contents are. `why` is the parser's
/// reason; `kept` is [`set_aside`]'s result, `None` when even the rename
/// failed (the file then stays where it was and is not written over).
pub fn set_aside_notice(path: &Path, why: &str, kept: Option<&Path>) -> String {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string());
    let why = why.trim().trim_end_matches('.');
    match kept {
        Some(kept) => format!(
            "Your saved settings ({name}) could not be read ({why}), so this start uses the \
             defaults. The old file was kept as {}.",
            kept.display()
        ),
        None => format!(
            "Your saved settings ({}) could not be read ({why}), so this start uses the \
             defaults and leaves that file as it is.",
            path.display()
        ),
    }
}

/// Write `contents` to `path` without ever leaving it half-written: into
/// `<name>.tmp` beside it first, flushed to disk, then renamed over it (a
/// rename within one folder replaces the old file in one step, on Windows
/// too). The folder must exist.
pub fn write_atomic(path: &Path, contents: &[u8]) -> io::Result<()> {
    let tmp = with_suffix(path, ".tmp");
    let result = (|| {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(contents)?;
        f.sync_all()?;
        drop(f);
        fs::rename(&tmp, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::vec::Vec;

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "legaia-app-theme-settings-{tag}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A broken file moves aside under a free name, and an older backup is
    /// never overwritten.
    #[test]
    fn set_aside_keeps_every_backup() {
        let dir = scratch("aside");
        let path = dir.join("launcher.toml");
        fs::write(&path, "disc = ").unwrap();
        let first = set_aside(&path).unwrap();
        assert_eq!(first, dir.join("launcher.toml.broken"));
        assert!(!path.exists());
        assert_eq!(fs::read_to_string(&first).unwrap(), "disc = ");
        fs::write(&path, "second").unwrap();
        let second = set_aside(&path).unwrap();
        assert_eq!(second, dir.join("launcher.toml.broken-2"));
        assert_eq!(fs::read_to_string(&first).unwrap(), "disc = ");
        assert_eq!(fs::read_to_string(&second).unwrap(), "second");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn write_atomic_replaces_and_leaves_no_temp_file() {
        let dir = scratch("atomic");
        let path = dir.join("settings.json");
        write_atomic(&path, b"one").unwrap();
        write_atomic(&path, b"two").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"two");
        let names: Vec<_> = fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names, [std::ffi::OsString::from("settings.json")]);
        // A folder that is not there fails cleanly, with nothing left behind.
        assert!(write_atomic(&dir.join("absent").join("x.json"), b"x").is_err());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_notice_names_the_backup_or_says_the_file_stays() {
        let path = Path::new("/home/p/.config/legaia-engine/launcher.toml");
        let kept = Path::new("/home/p/.config/legaia-engine/launcher.toml.broken");
        let s = set_aside_notice(path, "expected `=`, found newline at line 1.", Some(kept));
        assert!(s.contains("(launcher.toml)"), "{s}");
        assert!(s.contains("found newline at line 1)"), "{s}");
        assert!(s.ends_with("launcher.toml.broken."), "{s}");
        let s = set_aside_notice(path, "bad", None);
        assert!(s.contains("leaves that file as it is"), "{s}");
    }
}
