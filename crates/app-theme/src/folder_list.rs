//! The folder listing behind the in-app file finders: the game launcher's
//! disc / memory card / expansion finder and Legaia Explorer's file browser.
//! Both exist because a controller cannot work the desktop's file dialog;
//! each draws the rows its own way and lists them from here, so the two
//! agree on what a folder shows.
//!
//! [`list`] gives a folder's subfolders, then its files with one of the
//! asked-for extensions, each by name ignoring case. Hidden entries (a
//! leading `.`) are left out. [`drive_places`] gives the tops of the
//! computer's drives, for the finders' "places" list (the per-user folders,
//! Downloads and the like, come from the caller, which knows them).

use std::path::{Path, PathBuf};
use std::string::{String, ToString};
use std::vec::Vec;

/// What one row of [`list`] is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    /// A folder to open.
    Folder,
    /// A file with one of the asked-for extensions.
    File,
}

/// One row of [`list`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub path: PathBuf,
    pub name: String,
    pub kind: EntryKind,
}

/// Whether `path`'s extension is one of `extensions` (ignoring case; no
/// leading dot).
pub fn has_extension(path: &Path, extensions: &[&str]) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| extensions.iter().any(|x| e.eq_ignore_ascii_case(x)))
}

/// The subfolders of `dir`, then its files whose extension is one of
/// `extensions` (an empty list: folders only), each group by name ignoring
/// case. Reads at most `limit` directory entries, so a huge folder never
/// stalls a frame.
pub fn list(dir: &Path, extensions: &[&str], limit: usize) -> std::io::Result<Vec<Entry>> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(dir)?.flatten().take(limit) {
        let path = e.path();
        let Some(name) = path
            .file_name()
            .and_then(|n| n.to_str())
            .map(str::to_string)
        else {
            continue;
        };
        if name.starts_with('.') {
            continue;
        }
        let kind = if path.is_dir() {
            EntryKind::Folder
        } else if has_extension(&path, extensions) {
            EntryKind::File
        } else {
            continue;
        };
        out.push(Entry { path, name, kind });
    }
    out.sort_by(|a, b| {
        (a.kind != EntryKind::Folder, a.name.to_lowercase())
            .cmp(&(b.kind != EntryKind::Folder, b.name.to_lowercase()))
    });
    Ok(out)
}

/// The tops of the computer's drives that exist, labelled for a player:
/// `C:\` and on (*Drive C:*) on Windows; elsewhere the whole file system
/// (*Whole computer*) and the removable drives mounted under `/media/<user>`
/// and `/run/media/<user>` (*Drive NAME*).
pub fn drive_places() -> Vec<(String, PathBuf)> {
    let mut out: Vec<(String, PathBuf)> = Vec::new();
    if cfg!(windows) {
        for letter in b'C'..=b'Z' {
            let root = PathBuf::from(std::format!("{}:\\", letter as char));
            if root.is_dir() {
                out.push((std::format!("Drive {}:", letter as char), root));
            }
        }
    } else {
        out.push(("Whole computer".to_string(), PathBuf::from("/")));
        let user = std::env::var("USER").unwrap_or_default();
        for base in ["/media", "/run/media"] {
            let base = Path::new(base).join(&user);
            if let Ok(rd) = std::fs::read_dir(&base) {
                for e in rd.flatten().take(16) {
                    let name = e.file_name().to_string_lossy().into_owned();
                    out.push((std::format!("Drive {name}"), e.path()));
                }
            }
        }
    }
    out
}

/// Whether `path` can be a dump of Legend of Legaia by its name: a `.cue`
/// or `.bin` with "legaia" in its own name or in the name of the folder
/// holding it.
pub fn looks_like_legaia_dump(path: &Path) -> bool {
    let legaia = |p: Option<&std::ffi::OsStr>| {
        p.and_then(|n| n.to_str())
            .is_some_and(|n| n.to_ascii_lowercase().contains("legaia"))
    };
    has_extension(path, &["bin", "cue"])
        && (legaia(path.file_name()) || legaia(path.parent().and_then(Path::file_name)))
}

/// How many entries of one folder [`find_dumps`] reads, and how many of a
/// root's subfolders it opens: a bound, so a huge Downloads folder never
/// slows a program's first screen.
pub const FIND_ENTRY_LIMIT: usize = 4096;
const FIND_SUBFOLDER_LIMIT: usize = 256;

/// The files named like a Legend of Legaia dump ([`looks_like_legaia_dump`])
/// in `dirs`, each folder and its direct subfolders, at most `cap` of them;
/// a `.cue` sorts ahead of the `.bin` beside it. What the launcher and
/// Explorer offer on a first run, so a player whose dump sits in Downloads
/// is not sent hunting through a file dialog. Nothing is written.
pub fn find_dumps(dirs: &[PathBuf], cap: usize) -> Vec<PathBuf> {
    fn scan(dir: &Path, out: &mut Vec<PathBuf>, sub: Option<&mut Vec<PathBuf>>) {
        let Ok(rd) = std::fs::read_dir(dir) else {
            return;
        };
        let mut sub = sub;
        for e in rd.flatten().take(FIND_ENTRY_LIMIT) {
            let path = e.path();
            if path.is_dir() {
                if let Some(s) = sub.as_deref_mut()
                    && s.len() < FIND_SUBFOLDER_LIMIT
                {
                    s.push(path);
                }
            } else if looks_like_legaia_dump(&path) {
                out.push(path);
            }
        }
    }
    let mut found = Vec::new();
    for dir in dirs {
        let mut sub = Vec::new();
        scan(dir, &mut found, Some(&mut sub));
        for s in &sub {
            scan(s, &mut found, None);
        }
        if found.len() >= cap {
            break;
        }
    }
    found.sort_by_key(|p| (has_extension(p, &["bin"]), p.clone()));
    found.truncate(cap);
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folders_first_then_matching_files_by_name() {
        let root = std::env::temp_dir().join(std::format!(
            "legaia-app-theme-folder-list-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        for f in [
            "zeta.CUE",
            "alpha.bin",
            "notes.txt",
            ".hidden.bin",
            "card.mcr",
        ] {
            std::fs::write(root.join(f), b"x").unwrap();
        }
        for d in ["Games", "apps", ".cache"] {
            std::fs::create_dir(root.join(d)).unwrap();
        }
        let names = |exts: &[&str]| -> Vec<(String, EntryKind)> {
            list(&root, exts, 4096)
                .unwrap()
                .into_iter()
                .map(|e| (e.name, e.kind))
                .collect()
        };
        use EntryKind::*;
        assert_eq!(
            names(&["bin", "cue"]),
            [
                ("apps".to_string(), Folder),
                ("Games".to_string(), Folder),
                ("alpha.bin".to_string(), File),
                ("zeta.CUE".to_string(), File),
            ]
        );
        assert_eq!(
            names(&[]),
            [("apps".to_string(), Folder), ("Games".to_string(), Folder)]
        );
        assert_eq!(names(&["mcr"]).last().unwrap().0, "card.mcr");
        assert!(list(&root.join("missing"), &[], 16).is_err());
        let _ = std::fs::remove_dir_all(&root);
    }
}
