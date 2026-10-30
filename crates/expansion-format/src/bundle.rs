//! The `.lxpack` single-file bundle: a pack tree in one shareable file.
//!
//! A bundle is a self-describing archive of a [`PackFiles`] tree, nothing
//! more: it adds no metadata of its own (the manifest is `pack.toml`, one of
//! the files) and no timestamps, so the same tree always gives the same
//! bytes. All integers are little-endian.
//!
//! ```text
//! header   0x00  [u8; 8]  magic  "LXPACK\x1A\n"
//!          0x08  u16      format version (BUNDLE_VERSION)
//!          0x0A  u16      flags, 0
//!          0x0C  u32      entry count
//! table    per entry, sorted by path bytes, paths unique:
//!          +0x00 u16      path length in bytes
//!          +0x02 u8       method: 0 stored, 1 raw DEFLATE
//!          +0x03 u8       reserved, 0
//!          +0x04 u32      raw length
//!          +0x08 u32      stored length
//!          +0x0C [u8; 32] SHA-256 of the raw bytes
//!          +0x2C [u8]     path, UTF-8, `/`-separated
//! data     each entry's stored bytes, in table order, back to back
//! trailer  [u8; 32]       SHA-256 of every byte before it
//! ```
//!
//! The writer deflates an entry only when that makes it smaller. The reader
//! verifies the trailer before it trusts the table, bounds every length,
//! and checks each entry's digest after decompression.

use std::io::{Read, Write};

use anyhow::{Context, Result, bail, ensure};
use flate2::Compression;
use flate2::read::DeflateDecoder;
use flate2::write::DeflateEncoder;
use sha2::{Digest, Sha256};

use crate::PackFiles;

/// The bundle magic.
pub const MAGIC: [u8; 8] = *b"LXPACK\x1A\n";
/// The bundle format version this crate writes and reads.
pub const BUNDLE_VERSION: u16 = 1;
/// The conventional file extension (without the dot).
pub const EXTENSION: &str = "lxpack";

/// Most entries a bundle may hold.
pub const MAX_ENTRIES: usize = 16_384;
/// Longest path, in bytes.
pub const MAX_PATH_LEN: usize = 255;
/// Largest single entry once decompressed.
pub const MAX_ENTRY_LEN: usize = 64 << 20;
/// Largest whole tree once decompressed.
pub const MAX_TOTAL_LEN: usize = 512 << 20;

const HEADER_LEN: usize = 16;
const ENTRY_FIXED_LEN: usize = 0x2C;
const DIGEST_LEN: usize = 32;

/// Largest bundle file: every entry at its limits, stored.
pub const MAX_BUNDLE_LEN: usize =
    HEADER_LEN + MAX_ENTRIES * (ENTRY_FIXED_LEN + MAX_PATH_LEN) + MAX_TOTAL_LEN + DIGEST_LEN;

/// What kind of failure a bundle read hit, for a reader that words it for a
/// person (the workbench's import, the play page's library).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BundleErrorKind {
    /// The bytes do not start with the bundle magic: some other file.
    NotABundle,
    /// The file is damaged or truncated: a checksum, length or table fails.
    Corrupt,
    /// The bundle is written in a bundle format this build does not read.
    UnsupportedVersion,
    /// The bundle, an entry or the whole tree is over a size limit.
    TooLarge,
    /// An entry's path could escape the pack directory or is malformed.
    BadPath,
}

/// A bundle read failure with its [`BundleErrorKind`]; carried inside the
/// `anyhow::Error` [`read_bundle`] returns (`downcast_ref::<BundleError>`).
/// A failure with no `BundleError` inside is [`BundleErrorKind::Corrupt`]
/// ([`error_kind`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BundleError {
    pub kind: BundleErrorKind,
    pub message: String,
}

impl std::fmt::Display for BundleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for BundleError {}

/// The kind of a [`read_bundle`] failure.
pub fn error_kind(e: &anyhow::Error) -> BundleErrorKind {
    e.downcast_ref::<BundleError>()
        .map_or(BundleErrorKind::Corrupt, |b| b.kind)
}

macro_rules! refuse {
    ($kind:ident, $($t:tt)*) => {
        return Err(BundleError {
            kind: BundleErrorKind::$kind,
            message: format!($($t)*),
        }
        .into())
    };
}

const METHOD_STORED: u8 = 0;
const METHOD_DEFLATE: u8 = 1;

/// Is `path` acceptable as a pack-tree path: non-empty, at most
/// [`MAX_PATH_LEN`] bytes, `/`-separated, relative, with no empty, `.` or
/// `..` component and none of `\`, `:` or NUL. The rule keeps an unpacked
/// bundle inside its output directory on every host.
pub fn is_valid_path(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= MAX_PATH_LEN
        && !path.contains(['\\', ':', '\0'])
        && path
            .split('/')
            .all(|c| !c.is_empty() && c != "." && c != "..")
}

/// Serialise a pack tree into `.lxpack` bytes. Deterministic: the same tree
/// gives the same bytes.
pub fn write_bundle(files: &PackFiles) -> Result<Vec<u8>> {
    ensure!(
        files.len() <= MAX_ENTRIES,
        "{} files exceed the bundle limit of {MAX_ENTRIES}",
        files.len()
    );
    let mut total = 0usize;
    let mut table = Vec::new();
    let mut data = Vec::new();
    // `PackFiles` is a `BTreeMap<String, _>`, so iteration is already in
    // path-byte order; the reader insists on it.
    for (path, raw) in files {
        ensure!(is_valid_path(path), "invalid pack path {path:?}");
        ensure!(
            raw.len() <= MAX_ENTRY_LEN,
            "{path} is {} bytes, over the {MAX_ENTRY_LEN}-byte entry limit",
            raw.len()
        );
        total += raw.len();
        ensure!(
            total <= MAX_TOTAL_LEN,
            "the pack exceeds the {MAX_TOTAL_LEN}-byte bundle limit"
        );
        let deflated = deflate(raw)?;
        let (method, stored) = if deflated.len() < raw.len() {
            (METHOD_DEFLATE, deflated)
        } else {
            (METHOD_STORED, raw.clone())
        };
        table.extend_from_slice(&(path.len() as u16).to_le_bytes());
        table.push(method);
        table.push(0);
        table.extend_from_slice(&(raw.len() as u32).to_le_bytes());
        table.extend_from_slice(&(stored.len() as u32).to_le_bytes());
        table.extend_from_slice(&Sha256::digest(raw));
        table.extend_from_slice(path.as_bytes());
        data.extend_from_slice(&stored);
    }
    let mut out = Vec::with_capacity(HEADER_LEN + table.len() + data.len() + DIGEST_LEN);
    out.extend_from_slice(&MAGIC);
    out.extend_from_slice(&BUNDLE_VERSION.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&(files.len() as u32).to_le_bytes());
    out.extend_from_slice(&table);
    out.extend_from_slice(&data);
    let digest = Sha256::digest(&out);
    out.extend_from_slice(&digest);
    Ok(out)
}

/// Does `bytes` start with the bundle magic.
pub fn is_bundle(bytes: &[u8]) -> bool {
    bytes.starts_with(&MAGIC)
}

/// Parse `.lxpack` bytes back into the pack tree.
pub fn read_bundle(bytes: &[u8]) -> Result<PackFiles> {
    if !is_bundle(bytes) {
        refuse!(
            NotABundle,
            "not an .lxpack bundle (the file does not start with the bundle magic)"
        );
    }
    if bytes.len() > MAX_BUNDLE_LEN {
        refuse!(
            TooLarge,
            "the file is {} bytes, over the {MAX_BUNDLE_LEN}-byte bundle limit",
            bytes.len()
        );
    }
    if bytes.len() < HEADER_LEN + DIGEST_LEN {
        refuse!(Corrupt, "bundle is truncated");
    }
    let (body, trailer) = bytes.split_at(bytes.len() - DIGEST_LEN);
    // The version is read before the checksum so a bundle from a newer
    // writer says so, whatever else that format changed.
    let version = u16::from_le_bytes([body[8], body[9]]);
    if version != BUNDLE_VERSION {
        refuse!(
            UnsupportedVersion,
            "bundle format version {version} is not supported (this build reads version {BUNDLE_VERSION}); update the engine"
        );
    }
    if Sha256::digest(body).as_slice() != trailer {
        refuse!(
            Corrupt,
            "bundle checksum mismatch (the file is corrupt or truncated)"
        );
    }
    let mut r = Cursor {
        bytes: body,
        pos: 10,
    };
    let flags = r.u16()?;
    ensure!(flags == 0, "unknown bundle flags 0x{flags:04X}");
    let count = r.u32()? as usize;
    if count > MAX_ENTRIES {
        refuse!(
            TooLarge,
            "{count} entries exceed the bundle limit of {MAX_ENTRIES}"
        );
    }
    ensure!(
        count * ENTRY_FIXED_LEN <= body.len() - HEADER_LEN,
        "{count} entries cannot fit in a {}-byte bundle",
        bytes.len()
    );

    struct Entry {
        path: String,
        method: u8,
        raw_len: usize,
        stored_len: usize,
        digest: [u8; DIGEST_LEN],
    }
    let mut entries = Vec::with_capacity(count);
    let mut total = 0usize;
    for i in 0..count {
        let path_len = r.u16()? as usize;
        let method = r.u8()?;
        let reserved = r.u8()?;
        let raw_len = r.u32()? as usize;
        let stored_len = r.u32()? as usize;
        let digest: [u8; DIGEST_LEN] = r.take(DIGEST_LEN)?.try_into().expect("32 bytes");
        let path = std::str::from_utf8(r.take(path_len)?)
            .with_context(|| format!("entry {i}: path is not UTF-8"))?
            .to_string();
        if !is_valid_path(&path) {
            refuse!(BadPath, "entry {i}: invalid path {path:?}");
        }
        ensure!(reserved == 0, "{path}: reserved byte is {reserved}");
        if raw_len > MAX_ENTRY_LEN {
            refuse!(
                TooLarge,
                "{path}: {raw_len} bytes exceed the {MAX_ENTRY_LEN}-byte entry limit"
            );
        }
        total += raw_len;
        if total > MAX_TOTAL_LEN {
            refuse!(
                TooLarge,
                "the pack unpacks to over the {MAX_TOTAL_LEN}-byte bundle limit"
            );
        }
        match method {
            METHOD_STORED => ensure!(
                stored_len == raw_len,
                "{path}: stored entry has mismatched lengths"
            ),
            METHOD_DEFLATE => {}
            m => bail!("{path}: unknown compression method {m}"),
        }
        if let Some(prev) = entries.last().map(|e: &Entry| &e.path) {
            ensure!(
                prev.as_bytes() < path.as_bytes(),
                "{path}: entries are not sorted and unique"
            );
        }
        entries.push(Entry {
            path,
            method,
            raw_len,
            stored_len,
            digest,
        });
    }

    let mut files = PackFiles::new();
    for e in entries {
        let stored = r
            .take(e.stored_len)
            .with_context(|| format!("{}: data runs past the end", e.path))?;
        let raw = if e.method == METHOD_DEFLATE {
            inflate(stored, e.raw_len).with_context(|| format!("{}: bad DEFLATE data", e.path))?
        } else {
            stored.to_vec()
        };
        ensure!(
            Sha256::digest(&raw).as_slice() == e.digest,
            "{}: checksum mismatch",
            e.path
        );
        files.insert(e.path, raw);
    }
    ensure!(
        r.pos == body.len(),
        "{} trailing bytes after the last entry",
        body.len() - r.pos
    );
    Ok(files)
}

fn deflate(raw: &[u8]) -> Result<Vec<u8>> {
    let mut enc = DeflateEncoder::new(Vec::new(), Compression::best());
    enc.write_all(raw)?;
    Ok(enc.finish()?)
}

/// Inflate `stored`, accepting exactly `raw_len` output bytes; reading one
/// byte past the declared length bounds a decompression bomb.
fn inflate(stored: &[u8], raw_len: usize) -> Result<Vec<u8>> {
    let mut out = Vec::with_capacity(raw_len);
    DeflateDecoder::new(stored)
        .take(raw_len as u64 + 1)
        .read_to_end(&mut out)?;
    ensure!(
        out.len() == raw_len,
        "inflates to {} bytes, header says {raw_len}",
        out.len()
    );
    Ok(out)
}

struct Cursor<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self
            .pos
            .checked_add(n)
            .filter(|&e| e <= self.bytes.len())
            .context("bundle is truncated")?;
        let s = &self.bytes[self.pos..end];
        self.pos = end;
        Ok(s)
    }
    fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_le_bytes(
            self.take(2)?.try_into().expect("2 bytes"),
        ))
    }
    fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(
            self.take(4)?.try_into().expect("4 bytes"),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> PackFiles {
        let mut f = PackFiles::new();
        f.insert(
            "pack.toml".into(),
            b"id = \"t\"\nname = \"T\"\nversion = \"0.1.0\"\n".to_vec(),
        );
        f.insert("maps/a.toml".into(), "x = 1\n".repeat(200).into_bytes());
        f.insert("assets/notes.txt".into(), b"hi".to_vec());
        f.insert("assets/empty.txt".into(), Vec::new());
        f
    }

    #[test]
    fn round_trip_and_determinism() {
        let files = sample();
        let a = write_bundle(&files).unwrap();
        let b = write_bundle(&files.clone()).unwrap();
        assert_eq!(a, b, "same tree, same bytes");
        assert!(is_bundle(&a));
        assert_eq!(read_bundle(&a).unwrap(), files);
        // The repetitive map compressed; the 2-byte note was stored.
        assert!(a.len() < files.values().map(Vec::len).sum::<usize>() + 400);
    }

    #[test]
    fn empty_tree_round_trips() {
        let b = write_bundle(&PackFiles::new()).unwrap();
        assert_eq!(b.len(), HEADER_LEN + DIGEST_LEN);
        assert!(read_bundle(&b).unwrap().is_empty());
    }

    #[test]
    fn corruption_is_rejected() {
        let good = write_bundle(&sample()).unwrap();
        for at in [9, HEADER_LEN + 3, good.len() / 2, good.len() - 1] {
            let mut bad = good.clone();
            bad[at] ^= 0x40;
            assert!(read_bundle(&bad).is_err(), "flip at {at}");
        }
        assert!(read_bundle(&good[..good.len() - 1]).is_err());
        assert!(read_bundle(b"PK\x03\x04").is_err());
    }

    #[test]
    fn failures_carry_their_kind() {
        let good = write_bundle(&sample()).unwrap();
        let kind = |b: &[u8]| error_kind(&read_bundle(b).unwrap_err());
        assert_eq!(kind(b"PK\x03\x04"), BundleErrorKind::NotABundle);
        assert_eq!(kind(&good[..good.len() - 3]), BundleErrorKind::Corrupt);
        let mut flipped = good.clone();
        flipped[HEADER_LEN + 20] ^= 1;
        assert_eq!(kind(&flipped), BundleErrorKind::Corrupt);
        let mut newer = good.clone();
        newer[8] = 2;
        let e = read_bundle(&newer).unwrap_err();
        assert_eq!(error_kind(&e), BundleErrorKind::UnsupportedVersion);
        assert!(e.to_string().contains("update the engine"), "{e}");
    }

    #[test]
    fn hostile_paths_are_refused() {
        for p in ["", "/abs", "a/../b", "./a", "a//b", "c:\\x", "a\\b", "a/"] {
            assert!(!is_valid_path(p), "{p:?}");
            let mut f = PackFiles::new();
            f.insert(p.to_string(), vec![1]);
            assert!(write_bundle(&f).is_err(), "{p:?}");
        }
        assert!(is_valid_path("assets/art/hero.png"));
    }

    /// A table whose digest is valid but whose declared lengths lie must
    /// still fail cleanly: rebuild the trailer after editing the table.
    #[test]
    fn lying_lengths_fail_cleanly() {
        let mut f = PackFiles::new();
        f.insert("a.txt".into(), "z".repeat(4096).into_bytes());
        let mut b = write_bundle(&f).unwrap();
        // Entry 0's raw length sits at header + 4.
        let at = HEADER_LEN + 4;
        b[at..at + 4].copy_from_slice(&(10u32 << 20).to_le_bytes());
        let n = b.len() - DIGEST_LEN;
        let d = Sha256::digest(&b[..n]);
        b[n..].copy_from_slice(&d);
        assert!(read_bundle(&b).is_err());
    }
}
