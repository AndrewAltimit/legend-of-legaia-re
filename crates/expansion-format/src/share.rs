//! Sharing a pack: what an import checks before a pack is trusted, the
//! summary a pack library shows, and the pack's sources as one zip.
//!
//! [`import_bundle`] is the one gate every reader of a shared `.lxpack`
//! goes through - the workbench's Open and drop, the play page's library -
//! in this order, each with its own [`ImportErrorKind`]:
//!
//! 1. the bundle itself: magic, bundle format version, size limits, the
//!    whole-file checksum, every entry's path and digest ([`crate::bundle`]);
//! 2. trust ([`validate::trust_issues`]): no disc file, no program or
//!    script, every asset an allowed type with that type's signature, the
//!    asset budgets;
//! 3. the pack format and engine gates ([`crate::manifest::check_compatibility`]);
//! 4. the strict parse.
//!
//! Validation errors past those (a dangling reference, a bad field) do not
//! refuse an import: the workbench opens the pack to fix them, and the play
//! page refuses to activate it, listing them.

use std::io::Write;

use flate2::Compression;
use flate2::write::DeflateEncoder;
use serde::Serialize;

use crate::bundle::{self, BundleErrorKind};
use crate::manifest::{Incompatible, ManifestPeek, PackManifest};
use crate::validate::{self, Issue};
use crate::{ExpansionPack, PackFiles};

/// Why an import was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ImportErrorKind {
    /// Not an `.lxpack` at all.
    NotABundle,
    /// The file is damaged or truncated.
    Corrupt,
    /// A bundle format this build does not read.
    UnsupportedVersion,
    /// Over a size limit.
    TooLarge,
    /// A path that could escape the pack directory.
    BadPath,
    /// The pack is written in a newer pack format.
    NewerFormat,
    /// The pack names a pack format older than any this build reads.
    OlderFormat,
    /// The pack needs a newer engine.
    NewerEngine,
    /// The pack carries something a pack may not: disc bytes, a program, a
    /// disallowed file type.
    Unsafe,
    /// `pack.toml` or a section does not parse.
    Unparsable,
}

/// A refused import, worded for the person importing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ImportError {
    pub kind: ImportErrorKind,
    pub message: String,
}

impl std::fmt::Display for ImportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for ImportError {}

/// What a library lists for one pack.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct PackSummary {
    pub id: String,
    pub name: String,
    pub version: String,
    pub authors: Vec<String>,
    pub description: String,
    pub license: String,
    pub homepage: Option<String>,
    pub tags: Vec<String>,
    pub schema: u32,
    pub min_engine: Option<String>,
    pub requires: Vec<String>,
    pub conflicts: Vec<String>,
    /// The cover's path in the tree (`assets/cover.png`), when the manifest
    /// names one the pack carries.
    pub cover: Option<String>,
    /// Files in the tree and their total size, unpacked.
    pub files: usize,
    pub bytes: usize,
    /// Content counts, for the library's one-line description.
    pub maps: usize,
    pub quests: usize,
    pub cards: usize,
}

impl PackSummary {
    /// The summary of a parsed pack and the tree it came from.
    pub fn of(pack: &ExpansionPack, files: &PackFiles) -> Self {
        let m: &PackManifest = &pack.manifest;
        let cover = m
            .cover
            .as_ref()
            .filter(|c| files.contains_key(c.as_str()))
            .cloned();
        Self {
            id: m.id.clone(),
            name: m.name.clone(),
            version: m.version.clone(),
            authors: m.authors.clone(),
            description: m.description.clone(),
            license: m.license.clone(),
            homepage: m.homepage.clone(),
            tags: m.tags.clone(),
            schema: m.schema,
            min_engine: m.min_engine.clone(),
            requires: m.requires.clone(),
            conflicts: m.conflicts.clone(),
            cover,
            files: files.len(),
            bytes: files.values().map(Vec::len).sum(),
            maps: pack.maps.len(),
            quests: pack.quests.quests.len(),
            cards: pack.cards.cards.len(),
        }
    }
}

/// A bundle that passed every import gate.
#[derive(Debug, Clone)]
pub struct Imported {
    pub files: PackFiles,
    pub pack: ExpansionPack,
    pub summary: PackSummary,
}

impl Imported {
    /// The cover image's bytes, if the pack has one.
    pub fn cover(&self) -> Option<&[u8]> {
        self.summary
            .cover
            .as_ref()
            .and_then(|c| self.files.get(c))
            .map(Vec::as_slice)
    }
}

fn refuse(kind: ImportErrorKind, message: impl Into<String>) -> ImportError {
    ImportError {
        kind,
        message: message.into(),
    }
}

/// Read `.lxpack` bytes through every import gate.
pub fn import_bundle(bytes: &[u8]) -> Result<Imported, ImportError> {
    let files = bundle::read_bundle(bytes).map_err(|e| {
        let kind = match bundle::error_kind(&e) {
            BundleErrorKind::NotABundle => ImportErrorKind::NotABundle,
            BundleErrorKind::Corrupt => ImportErrorKind::Corrupt,
            BundleErrorKind::UnsupportedVersion => ImportErrorKind::UnsupportedVersion,
            BundleErrorKind::TooLarge => ImportErrorKind::TooLarge,
            BundleErrorKind::BadPath => ImportErrorKind::BadPath,
        };
        refuse(kind, format!("{e:#}"))
    })?;
    import_files(files)
}

/// The gates past the bundle itself, over a tree from anywhere.
pub fn import_files(files: PackFiles) -> Result<Imported, ImportError> {
    let trust = validate::trust_issues(&files);
    if let Some(first) = trust.first() {
        let more = match trust.len() {
            1 => String::new(),
            n => format!(" (and {} more)", n - 1),
        };
        return Err(refuse(
            ImportErrorKind::Unsafe,
            format!(
                "the pack was refused: {}: {}{more}",
                first.path, first.message
            ),
        ));
    }
    let text = files
        .get("pack.toml")
        .map(|b| String::from_utf8_lossy(b).into_owned())
        .ok_or_else(|| refuse(ImportErrorKind::Unparsable, "pack.toml is missing"))?;
    if let Some(peek) = ManifestPeek::read(&text)
        && let Err(why) = peek.compatibility()
    {
        let kind = match why {
            Incompatible::NewerFormat { .. } => ImportErrorKind::NewerFormat,
            Incompatible::OlderFormat { .. } => ImportErrorKind::OlderFormat,
            Incompatible::NewerEngine { .. } => ImportErrorKind::NewerEngine,
            Incompatible::BadMinEngine { .. } => ImportErrorKind::Unparsable,
        };
        let who = if peek.id.is_empty() {
            "the pack".to_string()
        } else {
            format!("{} {}", peek.id, peek.version)
        };
        return Err(refuse(kind, format!("{who}: {why}")));
    }
    let pack = ExpansionPack::from_files(&files)
        .map_err(|e| refuse(ImportErrorKind::Unparsable, format!("{e:#}")))?;
    let summary = PackSummary::of(&pack, &files);
    Ok(Imported {
        files,
        pack,
        summary,
    })
}

/// The errors [`validate::validate_files`] finds, as `file: field: message`
/// lines.
pub fn error_lines(issues: &[Issue]) -> Vec<String> {
    issues
        .iter()
        .filter(|i| i.severity == validate::Severity::Error)
        .map(|i| {
            if i.field.is_empty() {
                format!("{}: {}", i.path, i.message)
            } else {
                format!("{}: {}: {}", i.path, i.field, i.message)
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// The sources as a zip.
// ---------------------------------------------------------------------------

/// The pack's source tree as a zip under one folder `root/`, so it unpacks
/// into a pack directory the workbench's Open folder (or `asset expansion
/// validate`) reads back. Deterministic: entries in path order, a fixed
/// timestamp (1980-01-01), DEFLATE only where it is smaller.
pub fn sources_zip(root: &str, files: &PackFiles) -> std::io::Result<Vec<u8>> {
    const DOS_DATE: u16 = (1 << 5) | 1; // 1980-01-01
    const UTF8_NAMES: u16 = 1 << 11;
    let mut out = Vec::new();
    let mut central = Vec::new();
    let mut count = 0u16;
    for (path, raw) in files {
        let name = format!("{root}/{path}");
        let mut crc = flate2::Crc::new();
        crc.update(raw);
        let crc = crc.sum();
        let mut enc = DeflateEncoder::new(Vec::new(), Compression::best());
        enc.write_all(raw)?;
        let deflated = enc.finish()?;
        let (method, stored): (u16, &[u8]) = if deflated.len() < raw.len() {
            (8, &deflated)
        } else {
            (0, raw)
        };
        let offset = out.len() as u32;
        let le16 = |v: &mut Vec<u8>, x: u16| v.extend_from_slice(&x.to_le_bytes());
        let le32 = |v: &mut Vec<u8>, x: u32| v.extend_from_slice(&x.to_le_bytes());
        // Local file header.
        le32(&mut out, 0x0403_4B50);
        le16(&mut out, 20);
        le16(&mut out, UTF8_NAMES);
        le16(&mut out, method);
        le16(&mut out, 0);
        le16(&mut out, DOS_DATE);
        le32(&mut out, crc);
        le32(&mut out, stored.len() as u32);
        le32(&mut out, raw.len() as u32);
        le16(&mut out, name.len() as u16);
        le16(&mut out, 0);
        out.extend_from_slice(name.as_bytes());
        out.extend_from_slice(stored);
        // Central directory record.
        le32(&mut central, 0x0201_4B50);
        le16(&mut central, 20);
        le16(&mut central, 20);
        le16(&mut central, UTF8_NAMES);
        le16(&mut central, method);
        le16(&mut central, 0);
        le16(&mut central, DOS_DATE);
        le32(&mut central, crc);
        le32(&mut central, stored.len() as u32);
        le32(&mut central, raw.len() as u32);
        le16(&mut central, name.len() as u16);
        le16(&mut central, 0);
        le16(&mut central, 0);
        le16(&mut central, 0);
        le16(&mut central, 0);
        le32(&mut central, 0);
        le32(&mut central, offset);
        central.extend_from_slice(name.as_bytes());
        count += 1;
    }
    let cd_offset = out.len() as u32;
    let cd_len = central.len() as u32;
    out.extend_from_slice(&central);
    out.extend_from_slice(&0x0605_4B50u32.to_le_bytes());
    out.extend_from_slice(&[0; 4]);
    out.extend_from_slice(&count.to_le_bytes());
    out.extend_from_slice(&count.to_le_bytes());
    out.extend_from_slice(&cd_len.to_le_bytes());
    out.extend_from_slice(&cd_offset.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    Ok(out)
}

#[cfg(test)]
mod tests {
    use std::io::Read;

    use super::*;

    fn tree() -> PackFiles {
        let mut f = PackFiles::new();
        f.insert(
            "pack.toml".into(),
            b"id = \"t\"\nname = \"T\"\nversion = \"0.1.0\"\n".to_vec(),
        );
        f.insert(
            "maps/a.toml".into(),
            format!("name = \"A\"\n{}", "# padding\n".repeat(100)).into_bytes(),
        );
        f.insert("assets/cover.png".into(), b"\x89PNG\r\n\x1a\nrest".to_vec());
        f
    }

    #[test]
    fn a_good_bundle_imports() {
        let b = bundle::write_bundle(&tree()).unwrap();
        let i = import_bundle(&b).unwrap();
        assert_eq!(i.summary.id, "t");
        assert_eq!(i.summary.files, 3);
        assert!(i.cover().is_none(), "the manifest names no cover");
    }

    #[test]
    fn each_gate_has_its_kind() {
        let kind = |b: &[u8]| import_bundle(b).unwrap_err().kind;
        assert_eq!(kind(b"not a pack"), ImportErrorKind::NotABundle);
        let good = bundle::write_bundle(&tree()).unwrap();
        assert_eq!(kind(&good[..good.len() - 1]), ImportErrorKind::Corrupt);

        let mut t = tree();
        t.insert("assets/x.png".into(), vec![0x10, 0, 0, 0, 8, 0, 0, 0]);
        let e = import_bundle(&bundle::write_bundle(&t).unwrap()).unwrap_err();
        assert_eq!(e.kind, ImportErrorKind::Unsafe);
        assert!(e.message.contains("TIM"), "{e}");

        let mut t = tree();
        t.insert("setup.js".into(), b"alert(1)".to_vec());
        assert_eq!(
            kind(&bundle::write_bundle(&t).unwrap()),
            ImportErrorKind::Unsafe
        );

        let mut t = tree();
        t.insert(
            "pack.toml".into(),
            b"id = \"t\"\nname = \"T\"\nversion = \"2.0.0\"\nschema = 7\nnew_key = true\n".to_vec(),
        );
        let e = import_bundle(&bundle::write_bundle(&t).unwrap()).unwrap_err();
        assert_eq!(e.kind, ImportErrorKind::NewerFormat);
        assert!(e.message.contains("update the engine"), "{e}");

        let mut t = tree();
        t.insert(
            "pack.toml".into(),
            b"id = \"t\"\nname = \"T\"\nversion = \"2.0.0\"\nmin_engine = \"42.0.0\"\n".to_vec(),
        );
        assert_eq!(
            kind(&bundle::write_bundle(&t).unwrap()),
            ImportErrorKind::NewerEngine
        );

        let mut t = tree();
        t.insert("quests.toml".into(), b"quests = 3".to_vec());
        assert_eq!(
            kind(&bundle::write_bundle(&t).unwrap()),
            ImportErrorKind::Unparsable
        );
    }

    /// Read the zip back with a minimal reader: every entry's name, CRC and
    /// bytes, and the central directory agreeing with the local headers.
    #[test]
    fn the_sources_zip_holds_the_tree() {
        let t = tree();
        let z = sources_zip("t", &t).unwrap();
        assert_eq!(z, sources_zip("t", &t).unwrap(), "deterministic");
        let u16_at = |at: usize| u16::from_le_bytes([z[at], z[at + 1]]) as usize;
        let u32_at = |at: usize| u32::from_le_bytes(z[at..at + 4].try_into().unwrap()) as usize;
        let eocd = z.len() - 22;
        assert_eq!(u32_at(eocd), 0x0605_4B50);
        assert_eq!(u16_at(eocd + 10), t.len());
        let mut at = u32_at(eocd + 16);
        let mut back = PackFiles::new();
        for _ in 0..t.len() {
            assert_eq!(u32_at(at), 0x0201_4B50);
            let method = u16_at(at + 10);
            let crc = u32_at(at + 16) as u32;
            let stored = u32_at(at + 20);
            let raw_len = u32_at(at + 24);
            let name_len = u16_at(at + 28);
            let local = u32_at(at + 42);
            let name = std::str::from_utf8(&z[at + 46..at + 46 + name_len]).unwrap();
            assert_eq!(u32_at(local), 0x0403_4B50);
            let data = local + 30 + u16_at(local + 26);
            let body = &z[data..data + stored];
            let raw = if method == 8 {
                let mut v = Vec::new();
                flate2::read::DeflateDecoder::new(body)
                    .read_to_end(&mut v)
                    .unwrap();
                v
            } else {
                body.to_vec()
            };
            assert_eq!(raw.len(), raw_len);
            let mut c = flate2::Crc::new();
            c.update(&raw);
            assert_eq!(c.sum(), crc, "{name}");
            back.insert(name.strip_prefix("t/").unwrap().to_string(), raw);
            at += 46 + name_len;
        }
        assert_eq!(back, t);
    }
}
