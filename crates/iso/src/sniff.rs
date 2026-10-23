//! Why a file a user picked is not a disc image the reader can open, said
//! for a picker screen rather than a log.
//!
//! [`crate::raw::RawDisc::open`] rejects anything that is not a raw
//! 2352-byte-sector dump, and its error is written for a terminal: it names
//! the full path and the sector layout. A desktop program wants to say what
//! the file *is* (a 2048-byte `.iso`, a compressed `.chd`, a zip, a patch)
//! and what to choose instead, in two short sentences with no path. That is
//! [`Diagnosis`]: [`diagnose_bytes`] reads a file's head and extension (no
//! `std::fs`, so a browser host can call it on a picked file's first bytes),
//! and [`diagnose_path`] wraps it for a path, following a `.cue` sheet to its
//! `.bin` first.
//!
//! Both return `None` for a file that does look like a raw dump: the problem,
//! if any, is then past the sector layout (a damaged volume, another game).

use std::path::Path;

use crate::raw::{SECTOR_SIZE, SECTOR_SYNC};

/// How many head bytes [`diagnose_bytes`] needs to recognise everything it
/// knows (the ISO9660 volume descriptor of a 2048-byte image sits at
/// `16 * 2048 + 1`).
pub const SNIFF_LEN: usize = 16 * 2048 + 8;

/// What a picked file is, and what to pick instead.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnosis {
    /// One sentence: what the file is ("This is a 2048-byte .iso, ...").
    pub what: String,
    /// One sentence: what to do about it.
    pub fix: String,
}

impl Diagnosis {
    fn new(what: impl Into<String>, fix: impl Into<String>) -> Self {
        Self {
            what: what.into(),
            fix: fix.into(),
        }
    }
}

impl std::fmt::Display for Diagnosis {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} {}", self.what, self.fix)
    }
}

const RAW_FIX: &str = "Choose the .bin of a raw dump (2352-byte sectors), or the .cue beside it.";

/// Diagnose a file from its lower-cased extension (no dot), its first
/// [`SNIFF_LEN`] bytes (fewer when the file is shorter) and its full length.
/// `None` when the head is a raw CD sector.
pub fn diagnose_bytes(ext: &str, head: &[u8], len: u64) -> Option<Diagnosis> {
    if head.len() >= SECTOR_SYNC.len() && head[..SECTOR_SYNC.len()] == SECTOR_SYNC {
        return None;
    }
    let magic = |m: &[u8]| head.starts_with(m);
    if magic(b"MComprHD") || ext == "chd" {
        return Some(Diagnosis::new(
            "This is a compressed .chd image.",
            "Convert it back to .bin + .cue first (chdman extractcd -i game.chd -o game.cue).",
        ));
    }
    if magic(b"\0PBP") || ext == "pbp" {
        return Some(Diagnosis::new(
            "This is a PSP or PlayStation Classic EBOOT (.pbp), not a disc image.",
            "Use the .bin + .cue dump of the original disc.",
        ));
    }
    if magic(b"ECM\0") || ext == "ecm" {
        return Some(Diagnosis::new(
            "This is an ECM-packed image.",
            "Unpack it with unecm to get the .bin, then choose that.",
        ));
    }
    if magic(b"PK\x03\x04")
        || magic(b"7z\xBC\xAF\x27\x1C")
        || magic(b"Rar!")
        || magic(&[0x1F, 0x8B])
        || matches!(ext, "zip" | "7z" | "rar" | "gz" | "xz" | "tar")
    {
        return Some(Diagnosis::new(
            "This is a compressed archive, not the disc image itself.",
            "Extract it first, then choose the .bin or .cue inside.",
        ));
    }
    if magic(b"PPF") || matches!(ext, "ppf" | "ips" | "bps" | "xdelta" | "vcdiff") {
        return Some(Diagnosis::new(
            "This is a patch file, not a disc image.",
            "Choose the disc's .bin or .cue.",
        ));
    }
    if ext == "ccd" || ext == "mds" {
        return Some(Diagnosis::new(
            "This is a disc-image description file, not the image.",
            "Choose the .img (or .mdf) beside it.",
        ));
    }
    if ext == "m3u" {
        return Some(Diagnosis::new(
            "This is a playlist, not a disc image.",
            "Choose the .cue or .bin it lists.",
        ));
    }
    let iso_pvd = 16 * 2048;
    if head.len() >= iso_pvd + 6 && &head[iso_pvd + 1..iso_pvd + 6] == b"CD001" {
        return Some(Diagnosis::new(
            "This is a 2048-byte .iso, which leaves out the movie and voice sectors the game streams.",
            "Use a raw .bin + .cue dump of the disc (2352-byte sectors).",
        ));
    }
    if len < SECTOR_SIZE as u64 {
        return Some(Diagnosis::new(
            "This file is far too small to be a disc image.",
            RAW_FIX,
        ));
    }
    Some(Diagnosis::new("This is not a raw CD image.", RAW_FIX))
}

/// Diagnose the file at `path`. A `.cue` is followed to the file it names
/// (and a missing or unnamed one is the diagnosis). `None` when the file
/// looks like a raw dump, or cannot be read at all (the caller's own I/O
/// error says more than a guess would).
pub fn diagnose_path(path: &Path) -> Option<Diagnosis> {
    let ext = lower_ext(path);
    let target = if ext == "cue" {
        match cue_target(path) {
            Ok(bin) => bin,
            Err(d) => return Some(d),
        }
    } else {
        path.to_path_buf()
    };
    let mut head = Vec::with_capacity(SNIFF_LEN);
    let len = std::fs::metadata(&target).ok()?.len();
    {
        use std::io::Read;
        std::fs::File::open(&target)
            .ok()?
            .take(SNIFF_LEN as u64)
            .read_to_end(&mut head)
            .ok()?;
    }
    diagnose_bytes(&lower_ext(&target), &head, len)
}

fn lower_ext(path: &Path) -> String {
    path.extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default()
}

/// The file a cue sheet names, or why it cannot be used.
fn cue_target(cue: &Path) -> Result<std::path::PathBuf, Diagnosis> {
    let Ok(text) = std::fs::read_to_string(cue) else {
        return Err(Diagnosis::new(
            "This .cue sheet cannot be read as text.",
            "Choose the .bin beside it instead.",
        ));
    };
    let Some(name) = crate::raw::parse_cue_first_file(&text) else {
        return Err(Diagnosis::new(
            "This .cue sheet names no disc file.",
            "Choose the .bin beside it instead.",
        ));
    };
    let dir = cue.parent().unwrap_or_else(|| Path::new(""));
    let bin = dir.join(&name);
    if bin.exists() {
        return Ok(bin);
    }
    // The usual cause: the .bin was renamed and the sheet still names the
    // old file. One .bin in the folder is almost certainly the one meant.
    let bins: Vec<String> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.path()
                .extension()
                .is_some_and(|x| x.eq_ignore_ascii_case("bin"))
        })
        .filter_map(|e| e.file_name().into_string().ok())
        .collect();
    let what = format!("This .cue names \"{name}\", which is not in its folder.");
    Err(match bins.as_slice() {
        [only] => Diagnosis::new(
            what,
            format!("If the .bin was renamed, choose \"{only}\" itself."),
        ),
        _ => Diagnosis::new(
            what,
            "Keep the .bin beside the .cue under that name, or choose the .bin itself.",
        ),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw_head() -> Vec<u8> {
        let mut h = SECTOR_SYNC.to_vec();
        h.resize(SNIFF_LEN, 0);
        h
    }

    #[test]
    fn a_raw_sector_head_is_not_diagnosed() {
        assert_eq!(diagnose_bytes("bin", &raw_head(), 700 << 20), None);
        // The extension does not override the bytes: a raw dump named .img
        // or .iso still opens.
        assert_eq!(diagnose_bytes("iso", &raw_head(), 700 << 20), None);
    }

    #[test]
    fn a_cooked_iso_is_named_as_one() {
        let mut h = vec![0u8; SNIFF_LEN];
        h[16 * 2048] = 1;
        h[16 * 2048 + 1..16 * 2048 + 6].copy_from_slice(b"CD001");
        let d = diagnose_bytes("iso", &h, 600 << 20).unwrap();
        assert!(d.what.contains("2048-byte .iso"), "{d}");
    }

    #[test]
    fn containers_and_patches_by_magic_or_extension() {
        let cases: [(&str, &[u8], &str); 7] = [
            ("chd", b"MComprHD", ".chd"),
            ("bin", b"MComprHD\0\0", ".chd"),
            ("pbp", b"\0PBP", "EBOOT"),
            ("ecm", b"ECM\0", "ECM"),
            ("zip", b"PK\x03\x04", "archive"),
            ("7z", b"7z\xBC\xAF\x27\x1C", "archive"),
            ("ppf", b"PPF30", "patch"),
        ];
        for (ext, head, want) in cases {
            let d = diagnose_bytes(ext, head, 10 << 20).unwrap();
            assert!(d.what.contains(want), "{ext}: {d}");
        }
    }

    #[test]
    fn anything_else_is_not_a_raw_image() {
        let d = diagnose_bytes("bin", &[7u8; 4096], 4096).unwrap();
        assert_eq!(d.what, "This is not a raw CD image.");
        let d = diagnose_bytes("bin", &[7u8; 10], 10).unwrap();
        assert!(d.what.contains("too small"), "{d}");
    }

    #[test]
    fn every_sentence_is_short_and_pathless() {
        // A picker shows these in a narrow box: two sentences, no paths.
        let mut iso = vec![0u8; SNIFF_LEN];
        iso[16 * 2048 + 1..16 * 2048 + 6].copy_from_slice(b"CD001");
        for (ext, head) in [
            ("chd", &b"MComprHD"[..]),
            ("pbp", b"\0PBP"),
            ("ecm", b"ECM\0"),
            ("zip", b"PK\x03\x04"),
            ("ppf", b"PPF"),
            ("ccd", b"[CloneCD]"),
            ("m3u", b"game.cue"),
            ("iso", &iso),
            ("bin", &[1u8; 8]),
        ] {
            let d = diagnose_bytes(ext, head, 1 << 20).unwrap();
            for s in [&d.what, &d.fix] {
                assert!(s.len() <= 100, "{ext}: {s:?} is {} chars", s.len());
                assert!(s.ends_with('.') || s.ends_with(')'), "{ext}: {s:?}");
                assert!(!s.contains('/') || s.contains(".bin"), "{ext}: {s:?}");
            }
        }
    }

    #[test]
    fn a_cue_whose_bin_was_renamed_names_the_one_beside_it() {
        let dir = tempfile::tempdir().unwrap();
        let cue = dir.path().join("game.cue");
        std::fs::write(
            &cue,
            "FILE \"Old Name.bin\" BINARY\n  TRACK 01 MODE2/2352\n",
        )
        .unwrap();
        std::fs::write(dir.path().join("New Name.bin"), raw_head()).unwrap();
        let d = diagnose_path(&cue).unwrap();
        assert!(d.what.contains("\"Old Name.bin\""), "{d}");
        assert!(d.fix.contains("\"New Name.bin\""), "{d}");
        // With the name fixed, the cue resolves and its .bin is raw.
        std::fs::write(
            &cue,
            "FILE \"New Name.bin\" BINARY\n  TRACK 01 MODE2/2352\n",
        )
        .unwrap();
        assert_eq!(diagnose_path(&cue), None);
    }

    #[test]
    fn a_cue_with_no_file_line() {
        let dir = tempfile::tempdir().unwrap();
        let cue = dir.path().join("game.cue");
        std::fs::write(&cue, "REM nothing here\n").unwrap();
        assert!(
            diagnose_path(&cue)
                .unwrap()
                .what
                .contains("names no disc file")
        );
    }

    #[test]
    fn an_unreadable_path_is_left_to_the_caller() {
        assert_eq!(diagnose_path(Path::new("/nonexistent/legaia.bin")), None);
    }
}
