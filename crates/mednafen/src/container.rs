//! MDFNSVST container parser.
//!
//! Layout (decompressed):
//!
//! ```text
//! 0x00..0x08: "MDFNSVST" magic
//! 0x08..0x18: 16-byte header (version, payload size, flags)
//! 0x18..   : opaque preamble (mednafen-version-specific) followed by a
//!            sequence of named sections.
//!
//! Each top-level section:
//!     [32-byte name, NUL-padded]
//!     [4-byte LE size of the section body]
//!     [body of `size` bytes — sequence of sub-entries]
//!
//! Each sub-entry:
//!     [1-byte name length N]
//!     [N-byte name]
//!     [4-byte LE size of the value M]
//!     [M-byte value]
//! ```
//!
//! Mednafen interleaves real sections with multi-megabyte raw blobs (like
//! `MAIN` carrying main RAM). Walking the section table linearly from the
//! header is unreliable because the raw RAM contents include false-positive
//! "names". Instead, this module parses sections lazily by scanning the
//! payload for an exact name match — both fast (single linear scan) and
//! immune to byte-aliasing inside large opaque sections.

use anyhow::{Context, Result, bail};
use flate2::read::GzDecoder;
use std::io::Read;
use std::path::Path;

pub const MDFN_MAGIC: &[u8; 8] = b"MDFNSVST";
pub const MDFN_HEADER_LEN: usize = 0x18;
pub const SECTION_NAME_LEN: usize = 32;

/// Parsed mednafen save state.
#[derive(Debug, Clone)]
pub struct SaveState {
    pub payload: Vec<u8>,
    /// Cached known sections (populated on first targeted lookup).
    pub sections: Vec<Section>,
}

/// One top-level section.
#[derive(Debug, Clone)]
pub struct Section {
    pub name: String,
    pub body_offset: usize,
    pub body_len: usize,
    pub entries: Vec<SubEntry>,
}

/// One sub-entry inside a section.
#[derive(Debug, Clone)]
pub struct SubEntry {
    pub name: String,
    pub value_offset: usize,
    pub value_len: usize,
}

/// Top-level section names mednafen typically emits for the PSX module.
/// Used as the seed list for [`SaveState::index_known_sections`].
pub const KNOWN_SECTION_NAMES: &[&str] = &[
    "MAIN",
    "GPU",
    "SPU",
    "CDC",
    "IRQ",
    "TIMER",
    "MDEC",
    "DMA",
    "DRIVE",
    "FIO",
    "MDFNRINP",
    "BIOS_HASH",
    "MDFNDRIVE_00000000",
];

impl SaveState {
    /// Load and parse a `.mc{0..9}` mednafen save state from disk.
    pub fn from_path(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let raw = std::fs::read(path)
            .with_context(|| format!("reading mednafen save state {}", path.display()))?;
        Self::from_compressed(&raw)
    }

    /// Parse a gzipped save state from an in-memory buffer.
    pub fn from_compressed(raw: &[u8]) -> Result<Self> {
        let mut payload = Vec::with_capacity(raw.len() * 2);
        GzDecoder::new(raw)
            .read_to_end(&mut payload)
            .context("decompressing mednafen save state")?;
        Self::from_decompressed(payload)
    }

    /// Parse an already-decompressed save-state payload.
    pub fn from_decompressed(payload: Vec<u8>) -> Result<Self> {
        if payload.len() < MDFN_HEADER_LEN {
            bail!("save state too small ({} bytes)", payload.len());
        }
        if &payload[..8] != MDFN_MAGIC {
            bail!("bad magic: {:?} (expected MDFNSVST)", &payload[..8]);
        }
        let mut s = Self {
            payload,
            sections: Vec::new(),
        };
        s.index_known_sections();
        Ok(s)
    }

    /// Targeted scan for each name in [`KNOWN_SECTION_NAMES`]. Each match
    /// adds a `Section` to the cache. Unknown sections can be discovered
    /// later via [`Self::find_section_by_name`].
    pub fn index_known_sections(&mut self) {
        for &name in KNOWN_SECTION_NAMES {
            if self.sections.iter().any(|s| s.name == name) {
                continue;
            }
            if let Some(sec) = self.find_section_by_name(name) {
                self.sections.push(sec);
            }
        }
        self.sections.sort_by_key(|s| s.body_offset);
    }

    /// Search the payload for a section header with the exact 32-byte name
    /// `name` (NUL-padded). Returns the parsed [`Section`] or `None`.
    pub fn find_section_by_name(&self, name: &str) -> Option<Section> {
        if name.len() > SECTION_NAME_LEN {
            return None;
        }
        let mut needle = [0u8; SECTION_NAME_LEN];
        needle[..name.len()].copy_from_slice(name.as_bytes());
        // The header is `name(32) + size(4)`. We search for the full 32-byte
        // pattern; both alpha-prefix and NUL-suffix together are very rare
        // matches inside main RAM.
        let mut pos = MDFN_HEADER_LEN;
        while pos + SECTION_NAME_LEN + 4 <= self.payload.len() {
            if let Some(rel) = self.payload[pos..]
                .windows(SECTION_NAME_LEN)
                .position(|w| w == needle)
            {
                let abs = pos + rel;
                let size_off = abs + SECTION_NAME_LEN;
                if size_off + 4 > self.payload.len() {
                    return None;
                }
                let body_len = u32::from_le_bytes([
                    self.payload[size_off],
                    self.payload[size_off + 1],
                    self.payload[size_off + 2],
                    self.payload[size_off + 3],
                ]) as usize;
                let body_offset = size_off + 4;
                if body_offset + body_len > self.payload.len() || body_len > 4 * 1024 * 1024 {
                    // Possible false positive — keep scanning past this hit.
                    pos = abs + SECTION_NAME_LEN;
                    continue;
                }
                let entries = walk_subentries(&self.payload, body_offset, body_len);
                return Some(Section {
                    name: name.to_owned(),
                    body_offset,
                    body_len,
                    entries,
                });
            } else {
                return None;
            }
        }
        None
    }

    /// Look up a top-level section by name (cache + lazy fallback).
    pub fn section(&self, name: &str) -> Option<&Section> {
        self.sections.iter().find(|s| s.name == name)
    }

    /// Look up a `(section, entry)` pair, returning the value slice.
    pub fn entry_bytes(&self, section: &str, entry: &str) -> Option<&[u8]> {
        let s = self.section(section)?;
        let e = s.entries.iter().find(|e| e.name == entry)?;
        Some(&self.payload[e.value_offset..e.value_offset + e.value_len])
    }

    /// 2 MiB of PSX main RAM — index `0` corresponds to `0x80000000`.
    /// Tries the structured `MAIN.MainRAM.data8` path first; falls back to
    /// the SCUS-anchor heuristic if the structured path doesn't pan out.
    pub fn main_ram(&self) -> Result<&[u8]> {
        if let Some(bytes) = self.entry_bytes("MAIN", "MainRAM.data8")
            && bytes.len() == crate::extract::PSX_RAM_SIZE
        {
            return Ok(bytes);
        }
        crate::extract::main_ram_via_anchor(&self.payload)
    }
}

fn walk_subentries(payload: &[u8], body_offset: usize, body_len: usize) -> Vec<SubEntry> {
    let mut out = Vec::new();
    let mut pos = body_offset;
    let end = body_offset + body_len;
    while pos < end {
        if pos >= payload.len() {
            break;
        }
        let name_len = payload[pos] as usize;
        if name_len == 0 || pos + 1 + name_len + 4 > end {
            break;
        }
        let name_bytes = &payload[pos + 1..pos + 1 + name_len];
        if !name_bytes.iter().all(|&b| (0x20..=0x7E).contains(&b)) {
            break;
        }
        let Ok(name) = std::str::from_utf8(name_bytes) else {
            break;
        };
        let value_size_off = pos + 1 + name_len;
        let value_len = u32::from_le_bytes([
            payload[value_size_off],
            payload[value_size_off + 1],
            payload[value_size_off + 2],
            payload[value_size_off + 3],
        ]) as usize;
        let value_offset = value_size_off + 4;
        if value_offset + value_len > end {
            break;
        }
        out.push(SubEntry {
            name: name.to_owned(),
            value_offset,
            value_len,
        });
        pos = value_offset + value_len;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_section_bytes(name: &str, entries: &[(&str, Vec<u8>)]) -> Vec<u8> {
        let mut body = Vec::new();
        for (entry_name, data) in entries {
            body.push(entry_name.len() as u8);
            body.extend_from_slice(entry_name.as_bytes());
            body.extend_from_slice(&(data.len() as u32).to_le_bytes());
            body.extend_from_slice(data);
        }
        let mut name_buf = [0u8; SECTION_NAME_LEN];
        let n = name.len().min(SECTION_NAME_LEN);
        name_buf[..n].copy_from_slice(&name.as_bytes()[..n]);
        let mut out = Vec::new();
        out.extend_from_slice(&name_buf);
        out.extend_from_slice(&(body.len() as u32).to_le_bytes());
        out.extend_from_slice(&body);
        out
    }

    fn make_save(sections: Vec<Vec<u8>>) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(MDFN_MAGIC);
        out.extend_from_slice(&[0u8; MDFN_HEADER_LEN - MDFN_MAGIC.len()]);
        for s in sections {
            out.extend_from_slice(&s);
        }
        out
    }

    #[test]
    fn rejects_bad_magic() {
        let bad = vec![0u8; 64];
        assert!(SaveState::from_decompressed(bad).is_err());
    }

    #[test]
    fn finds_known_section_by_targeted_scan() {
        let s = make_section_bytes("MAIN", &[("MainRAM.data8", vec![0xAB; 16])]);
        let payload = make_save(vec![s]);
        let parsed = SaveState::from_decompressed(payload).unwrap();
        let entry = parsed.entry_bytes("MAIN", "MainRAM.data8").unwrap();
        assert_eq!(entry, &[0xAB; 16]);
    }

    #[test]
    fn finds_section_through_random_preamble() {
        // Synthesize a save state with random-looking prefix bytes before
        // the real MAIN section. The targeted scan must still find MAIN.
        let mut payload = Vec::new();
        payload.extend_from_slice(MDFN_MAGIC);
        payload.extend_from_slice(&[0u8; MDFN_HEADER_LEN - MDFN_MAGIC.len()]);
        // 200 bytes of random ASCII printable noise.
        let noise: Vec<u8> = (0..200).map(|i| 0x40 + (i % 26) as u8).collect();
        payload.extend_from_slice(&noise);
        // Real MAIN section.
        payload.extend_from_slice(&make_section_bytes("MAIN", &[("X", vec![0xCD; 4])]));
        let parsed = SaveState::from_decompressed(payload).unwrap();
        assert_eq!(
            parsed.entry_bytes("MAIN", "X").unwrap(),
            &[0xCD, 0xCD, 0xCD, 0xCD]
        );
    }

    #[test]
    fn parses_multiple_sections() {
        let main = make_section_bytes(
            "MAIN",
            &[("MainRAM.data8", vec![0xCC; 32]), ("CPU.PC", vec![0; 4])],
        );
        let drive = make_section_bytes("MDFNDRIVE_00000000", &[("counter", vec![1, 2, 3, 4])]);
        let payload = make_save(vec![main, drive]);
        let parsed = SaveState::from_decompressed(payload).unwrap();
        assert!(parsed.section("MAIN").is_some());
        assert!(parsed.section("MDFNDRIVE_00000000").is_some());
        assert_eq!(
            parsed.entry_bytes("MDFNDRIVE_00000000", "counter").unwrap(),
            &[1, 2, 3, 4]
        );
    }

    #[test]
    fn returns_none_for_missing_section() {
        let s = make_section_bytes("MAIN", &[]);
        let payload = make_save(vec![s]);
        let parsed = SaveState::from_decompressed(payload).unwrap();
        assert!(parsed.find_section_by_name("ZZZZ").is_none());
    }
}
