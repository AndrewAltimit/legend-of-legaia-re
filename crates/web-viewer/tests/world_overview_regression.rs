//! World-overview content regression. Pins per-kingdom slot-1 TMD pack +
//! MAN placements + classification TOML to stable SHA-256 digests so any
//! drift in the extract pipeline, TMD parser, LZS decoder, or
//! classification TOML surfaces as a test failure.
//!
//! Why content hashes instead of canvas pixel snapshots? The world-overview
//! page renders via WebGL in the browser; the rendered pixels depend on
//! GPU driver, OS, and canvas-text font metrics, so a PNG diff isn't
//! reproducible across machines. The *data feeding* the renderer is
//! perfectly deterministic from disc bytes, and changes to that data are
//! what produce visible regressions (different classes → different cell
//! tints, different unplaced counts → missing meshes, different md5s →
//! changed TMD bodies). Hashing the data covers the same surface with no
//! cross-platform fragility.
//!
//! Skipped (passes) when `LEGAIA_DISC_BIN` is unset, same convention as
//! the other disc-gated tests. To rebaseline after an intentional change:
//!
//!     LEGAIA_REGRESSION_UPDATE=1 LEGAIA_DISC_BIN=... \
//!         cargo test -p legaia-web-viewer world_overview_regression
//!
//! That rewrites the fixture at
//! `crates/web-viewer/tests/fixtures/world_overview_regression.toml`
//! with the current digests. Commit the diff after reviewing.

#![cfg(not(target_arch = "wasm32"))]

use legaia_web_viewer::disc::{extract_prot_dat, parse_prot_toc};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

/// Per-kingdom: (PROT base index, classification-TOML section name).
const KINGDOMS: &[(u32, &str)] = &[(85, "drake"), (244, "sebucus"), (391, "karisto")];

const SECTOR_ALIGN: usize = 0x800;
const ASSET_TABLE_COUNT: u32 = 7;
const ASSET_TABLE_HEADER_END: usize = 8 + (ASSET_TABLE_COUNT as usize) * 8;

/// Located 7-asset table inside a kingdom's PROT entry.
struct AssetTable<'a> {
    buf: &'a [u8],
    table_off: usize,
}

impl<'a> AssetTable<'a> {
    fn locate(buf: &'a [u8]) -> Option<Self> {
        let mut off = 0;
        while off + 64 <= buf.len() {
            if read_u32_le(buf, off) == Some(ASSET_TABLE_COUNT)
                && read_u32_le(buf, off + 12) == Some(ASSET_TABLE_HEADER_END as u32)
            {
                return Some(Self {
                    buf,
                    table_off: off,
                });
            }
            off += SECTOR_ALIGN;
        }
        None
    }

    /// `(type_byte, decompressed_size, descriptor_byte_offset)` for slot `slot`.
    fn slot(&self, slot: usize) -> Option<(u8, usize, usize)> {
        let p = self.table_off + 8 + slot * 8;
        let type_size = read_u32_le(self.buf, p)?;
        let offset = read_u32_le(self.buf, p + 4)? as usize;
        let type_byte = ((type_size >> 24) & 0xFF) as u8;
        let size = (type_size & 0x00FF_FFFF) as usize;
        Some((type_byte, size, offset))
    }

    /// LZS-decompress slot `slot`. Asserts the type byte matches.
    fn decompress_slot(&self, slot: usize, expected_type: u8) -> Option<Vec<u8>> {
        let (type_byte, size, offset) = self.slot(slot)?;
        if type_byte != expected_type {
            return None;
        }
        let src = &self.buf[self.table_off + offset..];
        legaia_lzs::decompress(src, size).ok()
    }
}

fn read_u32_le(buf: &[u8], off: usize) -> Option<u32> {
    Some(u32::from_le_bytes(buf.get(off..off + 4)?.try_into().ok()?))
}

fn read_i16_le(buf: &[u8], off: usize) -> Option<i16> {
    Some(i16::from_le_bytes(buf.get(off..off + 2)?.try_into().ok()?))
}

/// One TMD inside the slot-1 pack.
#[derive(Debug)]
struct PackTmd {
    slot: usize,
    byte_offset: usize,
    body_bytes: usize,
    magic_ok: bool,
    nobj: u32,
    md5_prefix: String, // 12-char prefix to match the world-overview JSON convention
}

fn parse_tmd_pack(pack: &[u8]) -> Vec<PackTmd> {
    let Some(count) = read_u32_le(pack, 0) else {
        return Vec::new();
    };
    let count = count as usize;
    let mut offsets = Vec::with_capacity(count);
    for k in 0..count {
        let Some(w) = read_u32_le(pack, 4 + 4 * k) else {
            return Vec::new();
        };
        offsets.push((w as usize) * 4);
    }
    let mut out = Vec::with_capacity(count);
    for k in 0..count {
        let start = offsets[k];
        let end = if k + 1 < count {
            offsets[k + 1]
        } else {
            pack.len()
        };
        if start + 12 > pack.len() || end > pack.len() || end <= start {
            out.push(PackTmd {
                slot: k,
                byte_offset: start,
                body_bytes: 0,
                magic_ok: false,
                nobj: 0,
                md5_prefix: String::new(),
            });
            continue;
        }
        let body = &pack[start..end];
        let magic = read_u32_le(pack, start).unwrap_or(0);
        let nobj = read_u32_le(pack, start + 8).unwrap_or(0);
        let body_md5 = sha256_prefix12(body);
        out.push(PackTmd {
            slot: k,
            byte_offset: start,
            body_bytes: body.len(),
            magic_ok: magic == 0x80000002,
            nobj,
            md5_prefix: body_md5,
        });
    }
    out
}

/// 12-hex-char SHA-256 prefix (replaces the world-overview's md5 prefix in
/// fingerprinting role - we don't need cross-tool md5 compatibility, just
/// a stable per-body fingerprint).
fn sha256_prefix12(buf: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(buf);
    let d = h.finalize();
    let mut s = String::with_capacity(12);
    for b in d.iter().take(6) {
        s.push_str(&format!("{:02x}", b));
    }
    s
}

/// Walked MAN placement record (the world-position-bearing subset of the
/// per-kingdom placement asset).
#[derive(Debug)]
struct Placement {
    id: i16,
    tmd_slot: u8,
    flag: u8,
    x_enc: u8,
    z_enc: u8,
    script_positioned: bool,
}

fn parse_placements(man: &[u8]) -> Option<Vec<Placement>> {
    let hdr = 0x22;
    let a = read_i16_le(man, hdr)?;
    let b = read_i16_le(man, hdr + 2)?;
    let c = read_i16_le(man, hdr + 4)?;
    let total = (a + b + c) as usize;
    let off_tbl = hdr + 9;
    if off_tbl + total * 3 > man.len() {
        return None;
    }
    let mut offsets = Vec::with_capacity(total);
    for i in 0..total {
        let p = off_tbl + i * 3;
        let lo = man[p] as u32;
        let mid = man[p + 1] as u32;
        let hi = man[p + 2] as u32;
        offsets.push((lo | (mid << 8) | (hi << 16)) as usize);
    }
    let data_area = off_tbl + total * 3;
    let mut out = Vec::new();
    let a_pos = a as usize;
    for s4 in 1..(total - a_pos) {
        let a3 = a_pos + s4;
        if a3 >= total {
            break;
        }
        let rec_off = data_area + offsets[a3];
        if rec_off >= man.len() {
            break;
        }
        let n_chars = man[rec_off] as usize;
        let name_end = rec_off + 1 + 2 * n_chars;
        if name_end + 4 > man.len() {
            break;
        }
        let tmd = man[name_end];
        let flag = man[name_end + 1];
        let x_enc = man[name_end + 2];
        let z_enc = man[name_end + 3];
        let script_positioned = x_enc == 0x7F && z_enc == 0x7F;
        out.push(Placement {
            id: s4 as i16,
            tmd_slot: tmd,
            flag,
            x_enc,
            z_enc,
            script_positioned,
        });
    }
    Some(out)
}

/// Build the canonical text representation for one kingdom. Stable byte
/// ordering + fixed-width formatting so the SHA-256 is reproducible across
/// machines. Includes EVERY field the world-overview page actually surfaces,
/// so any drift in the rendered viewer reflects as a digest change.
fn kingdom_digest_input(
    key: &str,
    prot_base: u32,
    pack: &[PackTmd],
    placements: &[Placement],
    classes: &BTreeMap<usize, String>,
) -> String {
    let mut s = String::with_capacity(8192);
    s.push_str(&format!("kingdom={} prot_base={}\n", key, prot_base));
    s.push_str(&format!("pack_count={}\n", pack.len()));
    for t in pack {
        s.push_str(&format!(
            "  pack slot={:>3} off=0x{:06X} bytes={:>6} nobj={:>3} magic_ok={} md5={}\n",
            t.slot, t.byte_offset, t.body_bytes, t.nobj, t.magic_ok, t.md5_prefix
        ));
    }
    s.push_str(&format!("placements={}\n", placements.len()));
    for p in placements {
        s.push_str(&format!(
            "  pl id={:>3} tmd={:>3} flag=0x{:02X} x=0x{:02X} z=0x{:02X} scripted={}\n",
            p.id, p.tmd_slot, p.flag, p.x_enc, p.z_enc, p.script_positioned as u8
        ));
    }
    s.push_str("classifications=\n");
    for (slot, cls) in classes {
        s.push_str(&format!("  cls slot={:>3} class={}\n", slot, cls));
    }
    s
}

fn sha256_hex(s: &str) -> String {
    let mut h = Sha256::new();
    h.update(s.as_bytes());
    let d = h.finalize();
    let mut out = String::with_capacity(64);
    for b in d.iter() {
        out.push_str(&format!("{:02x}", b));
    }
    out
}

/// Read the classification TOML and return `{slot: class}` for the given
/// kingdom. Missing-file / missing-section yields an empty map (so the
/// digest is still defined and any future TOML addition surfaces clearly).
fn load_classifications(repo_root: &Path, kingdom_key: &str) -> BTreeMap<usize, String> {
    let path = repo_root
        .join("site")
        .join("world-overview")
        .join("slot1_classification.toml");
    let Ok(text) = fs::read_to_string(&path) else {
        return BTreeMap::new();
    };
    let Ok(value): Result<toml::Value, _> = text.parse() else {
        return BTreeMap::new();
    };
    let mut out = BTreeMap::new();
    let Some(kingdom_tbl) = value.get(kingdom_key).and_then(|v| v.as_table()) else {
        return out;
    };
    for (slot_str, entry) in kingdom_tbl {
        let Ok(slot) = slot_str.parse::<usize>() else {
            continue;
        };
        let class = entry
            .get("class")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string();
        out.insert(slot, class);
    }
    out
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

fn fixture_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("world_overview_regression.toml")
}

#[test]
fn world_overview_data_digest_matches_fixture() {
    let Some(disc_path) = env::var_os("LEGAIA_DISC_BIN") else {
        eprintln!("LEGAIA_DISC_BIN unset; skipping world-overview regression");
        return;
    };
    let disc = fs::read(&disc_path).expect("read disc image");
    let prot = extract_prot_dat(&disc).expect("PROT.DAT");
    let entries = parse_prot_toc(&prot).expect("PROT TOC");
    let by_index: BTreeMap<u32, &legaia_web_viewer::disc::EntryMeta> =
        entries.iter().map(|e| (e.index, e)).collect();

    let repo = repo_root();
    let mut current = BTreeMap::new();
    for &(base, key) in KINGDOMS {
        let entry = by_index
            .get(&base)
            .unwrap_or_else(|| panic!("PROT entry {} missing", base));
        let start = entry.byte_offset as usize;
        let end = start + entry.size_bytes as usize;
        let buf = &prot[start..end];

        let at =
            AssetTable::locate(buf).unwrap_or_else(|| panic!("no 7-asset table in PROT {}", base));
        let pack_bytes = at
            .decompress_slot(1, 0x02)
            .unwrap_or_else(|| panic!("PROT {} slot-1 LZS decompress failed", base));
        let man_bytes = at
            .decompress_slot(2, 0x03)
            .unwrap_or_else(|| panic!("PROT {} slot-2 LZS decompress failed", base));

        let pack = parse_tmd_pack(&pack_bytes);
        let placements = parse_placements(&man_bytes)
            .unwrap_or_else(|| panic!("PROT {} MAN parse failed", base));
        let classes = load_classifications(&repo, key);

        let digest_input = kingdom_digest_input(key, base, &pack, &placements, &classes);
        let digest = sha256_hex(&digest_input);
        eprintln!(
            "[ok] {:9} prot={:>3} pack={:>2} placed={:>2} classes={:>2} digest={}",
            key,
            base,
            pack.len(),
            placements.len(),
            classes.len(),
            &digest[..16]
        );
        current.insert(key.to_string(), digest);
    }

    let update = env::var("LEGAIA_REGRESSION_UPDATE")
        .map(|v| v == "1")
        .unwrap_or(false);

    if update {
        let mut out = String::new();
        out.push_str(
            "# World-overview content regression fixture. Auto-rewritten by\n\
             # `LEGAIA_REGRESSION_UPDATE=1 cargo test -p legaia-web-viewer \\\n\
             #     world_overview_regression`. Commit the diff after reviewing.\n\
             #\n\
             # Each digest is a SHA-256 over (pack-TMD fingerprints +\n\
             # MAN-placement records + classification TOML entries) for one\n\
             # kingdom. See crates/web-viewer/tests/world_overview_regression.rs\n\
             # for the canonical text format the digest is computed over.\n\n",
        );
        for (key, digest) in &current {
            out.push_str(&format!("[kingdom.{}]\ndigest = \"{}\"\n\n", key, digest));
        }
        fs::create_dir_all(fixture_path().parent().unwrap()).expect("fixture dir");
        fs::write(fixture_path(), out).expect("write fixture");
        eprintln!("[update] rewrote {}", fixture_path().display());
        return;
    }

    let fixture_text = fs::read_to_string(fixture_path()).unwrap_or_else(|_| {
        panic!(
            "fixture missing at {}; run with LEGAIA_REGRESSION_UPDATE=1 to seed",
            fixture_path().display()
        )
    });
    let fixture: toml::Value = fixture_text.parse().expect("fixture parses");
    let expected_map = fixture
        .get("kingdom")
        .and_then(|v| v.as_table())
        .expect("fixture has [kingdom] table");

    let mut failures = Vec::new();
    for (key, got) in &current {
        let expected = expected_map
            .get(key)
            .and_then(|v| v.get("digest"))
            .and_then(|v| v.as_str())
            .unwrap_or("<missing>");
        if expected != got {
            failures.push(format!(
                "  {}: expected {}\n         got      {}",
                key, expected, got
            ));
        }
    }

    assert!(
        failures.is_empty(),
        "world-overview data digest drift in {} kingdom(s):\n{}\n\
         Re-run with LEGAIA_REGRESSION_UPDATE=1 to rebaseline if intentional.",
        failures.len(),
        failures.join("\n")
    );
}
