use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

pub const SECTOR_SIZE: usize = 2352;
pub const USER_DATA_OFFSET: usize = 24;
pub const USER_DATA_SIZE: usize = 2048;

pub struct RawDisc {
    file: File,
    sector_count: u64,
}

impl RawDisc {
    pub fn open(path: &Path) -> io::Result<Self> {
        let resolved = resolve_disc_path(path)?;
        let file = File::open(&resolved)?;
        let len = file.metadata()?.len();
        Ok(Self {
            file,
            sector_count: len / SECTOR_SIZE as u64,
        })
    }

    pub fn sector_count(&self) -> u64 {
        self.sector_count
    }

    pub fn read_sector(&mut self, lba: u32) -> io::Result<[u8; USER_DATA_SIZE]> {
        let mut sector = [0u8; SECTOR_SIZE];
        self.file
            .seek(SeekFrom::Start(lba as u64 * SECTOR_SIZE as u64))?;
        self.file.read_exact(&mut sector)?;
        let mut out = [0u8; USER_DATA_SIZE];
        out.copy_from_slice(&sector[USER_DATA_OFFSET..USER_DATA_OFFSET + USER_DATA_SIZE]);
        Ok(out)
    }

    pub fn read_user_data(&mut self, lba: u32, count: u32, out: &mut Vec<u8>) -> io::Result<()> {
        out.clear();
        out.reserve(count as usize * USER_DATA_SIZE);
        let mut sector = [0u8; SECTOR_SIZE];
        self.file
            .seek(SeekFrom::Start(lba as u64 * SECTOR_SIZE as u64))?;
        for _ in 0..count {
            self.file.read_exact(&mut sector)?;
            out.extend_from_slice(&sector[USER_DATA_OFFSET..USER_DATA_OFFSET + USER_DATA_SIZE]);
        }
        Ok(())
    }

    /// Read one raw 2352-byte sector. Use this when you need the CD-XA
    /// subheader (bytes 16..24) and full Form 2 user data (bytes 24..2348),
    /// which the Form 1 view at [`Self::read_sector`] truncates. The XA
    /// audio path uses this to demux multiplexed channels.
    pub fn read_raw_sector(&mut self, lba: u32) -> io::Result<[u8; SECTOR_SIZE]> {
        let mut sector = [0u8; SECTOR_SIZE];
        self.file
            .seek(SeekFrom::Start(lba as u64 * SECTOR_SIZE as u64))?;
        self.file.read_exact(&mut sector)?;
        Ok(sector)
    }
}

/// If `path` is a `.cue` sheet, parse it and return the absolute path to
/// the referenced binary track. Otherwise return `path` unchanged. Only
/// the FIRST `FILE "<name>" BINARY` line is honoured - Legaia ships a
/// single-track Mode2/2352 image, so multi-track cue layouts aren't a
/// concern in this project.
pub fn resolve_disc_path(path: &Path) -> io::Result<PathBuf> {
    let is_cue = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case("cue"))
        .unwrap_or(false);
    if !is_cue {
        return Ok(path.to_path_buf());
    }
    let text = std::fs::read_to_string(path)?;
    let bin_name = parse_cue_first_file(&text).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "cue sheet has no FILE \"...\" BINARY line",
        )
    })?;
    let parent = path.parent().unwrap_or_else(|| Path::new(""));
    let resolved = parent.join(&bin_name);
    if !resolved.exists() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!(
                "cue references {bin_name:?} but {} does not exist",
                resolved.display()
            ),
        ));
    }
    Ok(resolved)
}

/// Pull the first `FILE "<name>" BINARY` token out of a cue sheet body.
/// Lenient about whitespace and accepts both quoted and unquoted file
/// names. Returns `None` if no FILE line is present.
fn parse_cue_first_file(text: &str) -> Option<String> {
    for raw in text.lines() {
        let line = raw.trim();
        if !line.to_ascii_uppercase().starts_with("FILE") {
            continue;
        }
        // After "FILE", find a quoted name first; fall back to the first
        // whitespace-delimited token if quotes are missing.
        let after = line[4..].trim_start();
        if let Some(rest) = after.strip_prefix('"')
            && let Some(end) = rest.find('"')
        {
            return Some(rest[..end].to_string());
        }
        // Unquoted: take until next whitespace.
        let unquoted: String = after.chars().take_while(|c| !c.is_whitespace()).collect();
        if !unquoted.is_empty() {
            return Some(unquoted);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_cue_quoted_name() {
        let cue = "FILE \"Legend of Legaia (USA).bin\" BINARY\n  TRACK 01 MODE2/2352\n";
        assert_eq!(
            parse_cue_first_file(cue).as_deref(),
            Some("Legend of Legaia (USA).bin")
        );
    }

    #[test]
    fn parse_cue_unquoted_name() {
        let cue = "FILE legaia.bin BINARY\n  TRACK 01 MODE2/2352\n";
        assert_eq!(parse_cue_first_file(cue).as_deref(), Some("legaia.bin"));
    }

    #[test]
    fn parse_cue_with_leading_indent() {
        let cue = "  REM hello\n   FILE \"x.bin\" BINARY\n";
        assert_eq!(parse_cue_first_file(cue).as_deref(), Some("x.bin"));
    }

    #[test]
    fn parse_cue_returns_none_when_missing() {
        assert!(parse_cue_first_file("REM no file line\n").is_none());
    }

    #[test]
    fn resolve_returns_unchanged_for_bin() {
        let p = Path::new("/tmp/foo.bin");
        assert_eq!(resolve_disc_path(p).unwrap(), p);
    }

    #[test]
    fn resolve_reads_cue_when_present() {
        let dir = std::env::temp_dir().join("legaia-cue-resolve-test");
        let _ = std::fs::create_dir_all(&dir);
        let bin_path = dir.join("game.bin");
        std::fs::write(&bin_path, b"").unwrap();
        let cue_path = dir.join("game.cue");
        std::fs::write(&cue_path, "FILE \"game.bin\" BINARY\n").unwrap();
        let resolved = resolve_disc_path(&cue_path).unwrap();
        assert_eq!(resolved, bin_path);
    }

    #[test]
    fn resolve_errors_when_referenced_bin_missing() {
        let dir = std::env::temp_dir().join("legaia-cue-resolve-test-missing");
        let _ = std::fs::create_dir_all(&dir);
        let cue_path = dir.join("orphan.cue");
        std::fs::write(&cue_path, "FILE \"nope.bin\" BINARY\n").unwrap();
        let err = resolve_disc_path(&cue_path).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::NotFound);
    }
}
