//! The opened source: a disc image or an extracted folder, reduced to the
//! `PROT.DAT` archive, its CDNAME name map and (for a disc) the ISO9660
//! file list.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use legaia_engine_scene::scene::ProtIndex;
use legaia_iso::iso9660;
use legaia_iso::raw::{RawDisc, USER_DATA_SIZE};
use legaia_prot::archive::Entry;
use legaia_prot::cdname::{self, IndexMap};

/// What the user opened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    /// A raw Mode2/2352 disc image (the `.bin`; a `.cue` is resolved to it).
    Disc(PathBuf),
    /// A `legaia-extract` / `disc-extract` output folder holding `PROT.DAT`.
    Extracted(PathBuf),
}

impl Source {
    pub fn path(&self) -> &Path {
        match self {
            Source::Disc(p) | Source::Extracted(p) => p,
        }
    }
}

/// One ISO9660 file on the disc.
#[derive(Debug, Clone)]
pub struct IsoFile {
    pub path: String,
    pub lba: u32,
    pub size: u32,
}

/// A loaded source, ready to browse.
pub struct Library {
    pub source: Source,
    pub entries: Vec<Entry>,
    pub cdname: Option<IndexMap>,
    /// Empty for an extracted folder.
    pub iso_files: Vec<IsoFile>,
    /// `"NTSC-U (SCUS_942.54)"`-style line from `SYSTEM.CNF`, when present.
    pub region: Option<String>,
    /// The boot executable's name (`SCUS_942.54`, `SCES_019.44`, ...), from
    /// `SYSTEM.CNF` or, for a folder, the executable sitting in it.
    pub exe: Option<String>,
    /// The archive, held as the engine's own index so scene builds
    /// (`legaia_engine_scene`) read from the same single copy of `PROT.DAT`.
    pub index: Arc<ProtIndex>,
}

/// Check that `path` is something [`Library::open`] can read, without
/// loading it: a directory with a `PROT.DAT`, or a Mode2/2352 image (or a
/// `.cue` naming one) whose ISO9660 tree has `PROT.DAT`.
pub fn validate(path: &Path) -> Result<Source> {
    // Said plainly: the OS error ("No such file or directory (os error 2)")
    // reads as a fault in the program rather than a path to correct.
    if !path.exists() {
        bail!(
            "Nothing at {} - check the path, or use Open disc image... to pick \
             your .bin / .cue",
            path.display()
        );
    }
    if path.is_dir() {
        if !path.join("PROT.DAT").is_file() {
            bail!(
                "{} has no PROT.DAT - pick a disc image or a legaia-extract output folder",
                path.display()
            );
        }
        return Ok(Source::Extracted(path.to_path_buf()));
    }
    let bin = legaia_iso::raw::resolve_disc_path(path)
        .with_context(|| format!("resolving {}", path.display()))?;
    // The reader's own message names the file and says what is wrong with
    // it; only an error that does not (permission, I/O) gets the path added.
    let mut disc = RawDisc::open(&bin).map_err(|e| {
        let named = e.to_string().contains(&bin.display().to_string());
        let e = anyhow::Error::new(e);
        if named {
            e
        } else {
            e.context(format!("opening {}", bin.display()))
        }
    })?;
    let vol = iso9660::read_volume(&mut disc).context("reading the ISO9660 volume")?;
    let files = iso9660::walk_files(&mut disc, &vol.root).context("walking the ISO9660 tree")?;
    if !files
        .iter()
        .any(|(p, _)| p.eq_ignore_ascii_case("PROT.DAT"))
    {
        bail!(
            "{} has no PROT.DAT - this is not a Legend of Legaia disc",
            bin.display()
        );
    }
    Ok(Source::Disc(bin))
}

/// The USA boot executable.
pub const USA_EXE: &str = "SCUS_942.54";

/// The boot executable in an extracted folder (`SCUS_942.54`,
/// `SCES_019.44`, ...): the file named like a PlayStation product code.
fn folder_executable(dir: &Path) -> Option<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .ok()?
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .filter(|n| {
            let b = n.as_bytes();
            b.len() == 11
                && b[4] == b'_'
                && b[8] == b'.'
                && b[..4].iter().all(u8::is_ascii_uppercase)
        })
        .collect();
    names.sort();
    names.into_iter().next()
}

fn read_iso_file(disc: &mut RawDisc, f: &IsoFile) -> Result<Vec<u8>> {
    let mut buf = Vec::new();
    disc.read_user_data(f.lba, f.size.div_ceil(USER_DATA_SIZE as u32), &mut buf)?;
    buf.truncate(f.size as usize);
    Ok(buf)
}

impl Library {
    /// Validate and load `path` (disc image, `.cue`, or extracted folder).
    pub fn open(path: &Path) -> Result<Self> {
        match validate(path)? {
            Source::Disc(bin) => Self::open_disc(&bin),
            Source::Extracted(dir) => Self::open_extracted(&dir),
        }
    }

    fn open_disc(bin: &Path) -> Result<Self> {
        let mut disc = RawDisc::open(bin)?;
        let vol = iso9660::read_volume(&mut disc)?;
        let iso_files: Vec<IsoFile> = iso9660::walk_files(&mut disc, &vol.root)?
            .into_iter()
            .map(|(path, e)| IsoFile {
                path,
                lba: e.lba,
                size: e.size,
            })
            .collect();
        let find = |name: &str| iso_files.iter().find(|f| f.path.eq_ignore_ascii_case(name));
        let prot = find("PROT.DAT").context("no PROT.DAT on the disc")?.clone();
        let prot_bytes = read_iso_file(&mut disc, &prot).context("reading PROT.DAT")?;
        let cdname_text = match find("CDNAME.TXT").cloned() {
            Some(f) => Some(String::from_utf8_lossy(&read_iso_file(&mut disc, &f)?).into_owned()),
            None => None,
        };
        let cdname = cdname_text
            .as_deref()
            .and_then(|t| cdname::parse_str(t).ok());
        let detected = legaia_extract::disc_region(bin);
        let region = detected
            .as_ref()
            .map(|d| format!("{} ({})", d.region.name(), d.executable));
        let exe = detected.map(|d| d.executable);
        let index = ProtIndex::from_bytes(prot_bytes, cdname_text.as_deref())
            .context("parsing the PROT.DAT TOC")?;
        Ok(Self {
            source: Source::Disc(bin.to_path_buf()),
            entries: index.entries().to_vec(),
            cdname,
            iso_files,
            region,
            exe,
            index: Arc::new(index),
        })
    }

    fn open_extracted(dir: &Path) -> Result<Self> {
        let index = ProtIndex::open_extracted(dir).context("parsing the PROT.DAT TOC")?;
        let cdname = index.cdname_map().cloned();
        Ok(Self {
            source: Source::Extracted(dir.to_path_buf()),
            entries: index.entries().to_vec(),
            cdname,
            iso_files: Vec::new(),
            region: None,
            exe: folder_executable(dir),
            index: Arc::new(index),
        })
    }

    /// The bytes of PROT entry at position `pos` in [`Self::entries`].
    pub fn entry_bytes(&self, pos: usize) -> Result<Vec<u8>> {
        let entry = self.entries.get(pos).context("entry out of range")?;
        // A raw read, not `ProtIndex::entry_bytes`: that one caches every
        // entry it returns, and browsing (plus the background classifier)
        // would end up holding a second copy of the whole archive.
        self.index
            .prot_dat_raw_bytes(entry.byte_offset, entry.size_bytes as usize)
    }

    /// The boot executable (`SCUS_942.54` on the USA disc): its static
    /// tables carry the item and equipment names.
    pub fn executable(&self) -> Result<Vec<u8>> {
        let name = self.exe.as_deref().unwrap_or(USA_EXE);
        match &self.source {
            Source::Extracted(dir) => {
                std::fs::read(dir.join(name)).with_context(|| format!("reading {name}"))
            }
            Source::Disc(_) => {
                let f = self
                    .iso_files
                    .iter()
                    .find(|f| f.path.eq_ignore_ascii_case(name))
                    .with_context(|| format!("no {name} on the disc"))?;
                self.iso_file_bytes(f)
            }
        }
    }

    /// `true` for the USA build (or an unknown one), the build whose
    /// executable the static-table readers address.
    pub fn is_usa(&self) -> bool {
        self.exe.as_deref().is_none_or(|e| e == USA_EXE)
    }

    /// The dialog text codec of this build, chosen the way
    /// `legaia_translate::build::detect` chooses it: Shift-JIS on a Japanese
    /// executable, Latin otherwise, with the high accented cells read as
    /// letters on every Latin build except the USA one.
    pub fn text_codec(&self) -> legaia_translate::build::TextCodec {
        use legaia_translate::build::TextCodec;
        let exe = self.exe.as_deref().unwrap_or(USA_EXE);
        if ["SCPS", "SLPS", "SCPM", "PAPX"]
            .iter()
            .any(|p| exe.starts_with(p))
        {
            TextCodec::ShiftJis
        } else {
            TextCodec::Latin {
                allow_high: exe != USA_EXE,
            }
        }
    }

    /// Position in [`Self::entries`] of the entry with PROT index `index`.
    pub fn position_of(&self, index: u32) -> Option<usize> {
        self.entries.iter().position(|e| e.index == index)
    }

    /// File stem the extractor gives this entry (`0865_battle_data`).
    pub fn stem(&self, index: u32) -> String {
        legaia_extract::entry_stem(self.cdname.as_ref(), index)
    }

    /// The CDNAME block label as the extracted filenames carry it.
    pub fn label(&self, index: u32) -> Option<&str> {
        self.cdname
            .as_ref()
            .and_then(|m| cdname::block_for(m, index))
    }

    /// The CDNAME block that retail-semantically covers this entry (the
    /// `#define` numbers are raw-TOC indices, two above extraction indices).
    pub fn retail_block(&self, index: u32) -> Option<&str> {
        self.cdname
            .as_ref()
            .and_then(|m| cdname::block_for_extraction_index(m, index))
    }

    /// Read a whole ISO9660 file (disc sources only).
    pub fn iso_file_bytes(&self, f: &IsoFile) -> Result<Vec<u8>> {
        let Source::Disc(bin) = &self.source else {
            bail!("not a disc source");
        };
        let mut disc = RawDisc::open(bin)?;
        read_iso_file(&mut disc, f)
    }
}

#[cfg(test)]
mod validate_tests {
    use super::validate;

    #[test]
    fn a_missing_path_is_named_plainly() {
        let dir = tempfile::tempdir().unwrap();
        let e = format!(
            "{:#}",
            validate(&dir.path().join("Legaia.bin")).unwrap_err()
        );
        assert!(e.starts_with("Nothing at "), "{e}");
        assert!(!e.contains("os error"), "{e}");
    }

    #[test]
    fn a_folder_without_prot_says_what_it_wants() {
        let dir = tempfile::tempdir().unwrap();
        let e = format!("{:#}", validate(dir.path()).unwrap_err());
        assert!(e.contains("has no PROT.DAT"), "{e}");
    }
}
