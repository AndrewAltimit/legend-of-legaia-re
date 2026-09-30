//! Ghidra dump corpus: header parse and byte-level attribution.
//! Split out of `byte_account.rs`.

use super::*;

// ---------------------------------------------------------------------------
// Ghidra dump corpus (header parse + byte-level attribution)
// ---------------------------------------------------------------------------

/// One dumped function's stated extent, plus what the dump prints first.
#[derive(Debug, Clone)]
pub struct DumpExtent {
    /// Entry virtual address from the header's `entry=` / VA field.
    pub entry_va: u32,
    /// Byte length from the `size=N bytes` line (or `max - min + 4`).
    pub bytes: u32,
    /// Filename label: `overlay_<label>_<addr>.txt` -> `<label>`; `None` for a
    /// bare `<addr>.txt`.
    pub label: Option<String>,
    /// The first printed instruction texts, in order (up to 4).
    pub head_insns: Vec<String>,
}

/// Header shapes that are the corpus recording an *answer* rather than a body
/// dump. Mirrors `dump_header.py`'s `_NOT_A_DUMP_MARKERS`.
pub(super) const NOT_A_DUMP: [&str; 5] = [
    "citation pointer",
    "cite of",
    "NOFUNC",
    "DATA REGION",
    "DATA WINDOW",
];

pub(super) fn hex8(tok: &str) -> Option<u32> {
    let t = tok.trim_start_matches("0x").trim_start_matches("0X");
    if t.len() != 8 || !t.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    u32::from_str_radix(t, 16).ok()
}

/// Parse one dump file's header + first instructions.
///
/// Re-implements `scripts/ghidra-analysis/dump_header.py`'s accepted
/// spellings: the VA may be bare or `0x`-prefixed, the entry may be
/// `(entry=VA)`, `(entry VA)`, `(entry=VA, label=..)` or absent, and the size
/// line may or may not carry an instruction count. A `min=..  max=..` header
/// states the same extent a different way (`max` inclusive).
pub fn parse_dump_header(text: &str, file_stem: &str) -> Option<DumpExtent> {
    let mut lines = text.lines();
    let head = lines.next()?;
    if NOT_A_DUMP.iter().any(|m| head.contains(m)) {
        return None;
    }
    // Entry VA: prefer the explicit `entry` field, else the first 8-hex token
    // outside a `[image]` bracket.
    let mut entry_va = None;
    if let Some(p) = head.find("entry") {
        let rest = head[p + 5..].trim_start_matches(['=', ' ']);
        entry_va = rest.split([',', ')', ' ']).next().and_then(hex8);
    }
    if entry_va.is_none() {
        let mut stripped = String::new();
        let mut depth = 0i32;
        for ch in head.chars() {
            match ch {
                '[' => depth += 1,
                ']' => depth -= 1,
                c if depth == 0 => stripped.push(c),
                _ => {}
            }
        }
        entry_va = stripped
            .split(|c: char| !(c.is_ascii_alphanumeric()))
            .find_map(hex8);
    }
    let entry_va = entry_va?;

    let mut bytes = None;
    for line in lines.by_ref().take(3) {
        if let Some(rest) = line.strip_prefix("size=") {
            bytes = rest.split_whitespace().next().and_then(|n| n.parse().ok());
            break;
        }
        if let (Some(mi), Some(ma)) = (line.find("min="), line.find("max=")) {
            let lo = line[mi + 4..].split_whitespace().next().and_then(hex8);
            let hi = line[ma + 4..].split_whitespace().next().and_then(hex8);
            if let (Some(lo), Some(hi)) = (lo, hi) {
                bytes = hi.checked_sub(lo).map(|d| d + 4);
                break;
            }
        }
    }
    let bytes = bytes.filter(|&b: &u32| b > 0)?;

    // First few printed instruction texts: `<addr>  <mnemonic ...>`, with a
    // leading `_` marking a delay slot.
    let mut head_insns = Vec::new();
    for line in text.lines() {
        let t = line.trim_start_matches('_');
        let Some((addr, rest)) = t.split_once("  ") else {
            continue;
        };
        if hex8(addr.trim()).is_none() {
            continue;
        }
        let rest = rest.trim();
        if rest.is_empty() {
            continue;
        }
        head_insns.push(rest.to_string());
        if head_insns.len() == 4 {
            break;
        }
    }

    // `overlay_<label>_<8 hex>` -> label; `<8 hex>` -> None.
    let label = file_stem.strip_prefix("overlay_").and_then(|s| {
        s.rsplit_once('_')
            .filter(|(_, tail)| hex8(tail).is_some())
            .map(|(lead, _)| lead.to_string())
    });

    Some(DumpExtent {
        entry_va,
        bytes,
        label,
        head_insns,
    })
}

pub(super) fn read_prefix(path: &Path, n: usize) -> std::io::Result<String> {
    use std::io::Read;
    let mut f = std::fs::File::open(path)?;
    let mut buf = vec![0u8; n];
    let got = f.read(&mut buf)?;
    buf.truncate(got);
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

/// Read every dump in `dir` whose header parses.
pub fn read_dump_extents(dir: &Path) -> std::io::Result<Vec<DumpExtent>> {
    let mut out = Vec::new();
    for ent in std::fs::read_dir(dir)? {
        let path = ent?.path();
        if path.extension().and_then(|e| e.to_str()) != Some("txt") {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        // The header + a few instruction lines live in the first few KB; a
        // dump can be large, so do not read the whole corpus.
        let Ok(text) = read_prefix(&path, 4096) else {
            continue;
        };
        if let Some(d) = parse_dump_header(&text, stem) {
            out.push(d);
        }
    }
    Ok(out)
}

pub(super) const REG_NAMES: [&str; 32] = [
    "zero", "at", "v0", "v1", "a0", "a1", "a2", "a3", "t0", "t1", "t2", "t3", "t4", "t5", "t6",
    "t7", "s0", "s1", "s2", "s3", "s4", "s5", "s6", "s7", "t8", "t9", "k0", "k1", "gp", "sp", "s8",
    "ra",
];

pub(super) fn reg(name: &str) -> Option<u32> {
    REG_NAMES.iter().position(|&r| r == name).map(|i| i as u32)
}

pub(super) fn imm(tok: &str) -> Option<i64> {
    let (neg, t) = match tok.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, tok),
    };
    let v = match t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
        Some(h) => i64::from_str_radix(h, 16).ok()?,
        None => t.parse::<i64>().ok()?,
    };
    Some(if neg { -v } else { v })
}

/// Re-encode a printed MIPS instruction to its 32-bit word.
///
/// Only the handful of forms a function's first instructions actually take are
/// covered - that is enough to decide, from the image's own bytes, whether a
/// dump's extent belongs to this image or to a VA-aliased sibling. `None`
/// means "this module cannot encode that text", which the caller treats as
/// *unverifiable*, never as a mismatch.
pub fn encode_insn(text: &str) -> Option<u32> {
    let text = text.split(';').next()?.trim();
    if text == "nop" {
        return Some(0);
    }
    if text == "jr ra" {
        return Some(0x03E0_0008);
    }
    let (op, args) = text.split_once(' ')?;
    let a: Vec<&str> = args.split(',').map(|s| s.trim()).collect();
    match op {
        // `addiu rt,rs,imm`
        "addiu" if a.len() == 3 => {
            let (rt, rs) = (reg(a[0])?, reg(a[1])?);
            let i = imm(a[2])? as i16 as u32 & 0xFFFF;
            Some(0x2400_0000 | (rs << 21) | (rt << 16) | i)
        }
        // `li rt,imm` = `addiu rt,zero,imm`
        "li" if a.len() == 2 => {
            let rt = reg(a[0])?;
            let i = imm(a[1])? as i16 as u32 & 0xFFFF;
            Some(0x2400_0000 | (rt << 16) | i)
        }
        // `lui rt,imm`
        "lui" if a.len() == 2 => {
            let rt = reg(a[0])?;
            let i = imm(a[1])? as u32 & 0xFFFF;
            Some(0x3C00_0000 | (rt << 16) | i)
        }
        // `move rd,rs` = `addu rd,rs,zero`
        "move" if a.len() == 2 => {
            let (rd, rs) = (reg(a[0])?, reg(a[1])?);
            Some((rs << 21) | (rd << 11) | 0x21)
        }
        // `j`/`jal target`
        "j" | "jal" if a.len() == 1 => {
            let t = imm(a[0])? as u32;
            let base = if op == "j" { 0x0800_0000 } else { 0x0C00_0000 };
            Some(base | ((t >> 2) & 0x03FF_FFFF))
        }
        // `sw/lw/... rt,off(rs)`
        "sw" | "lw" | "sb" | "lb" | "sh" | "lh" | "lbu" | "lhu" if a.len() == 2 => {
            let rt = reg(a[0])?;
            let (off, rest) = a[1].split_once('(')?;
            let rs = reg(rest.trim_end_matches(')'))?;
            let i = imm(off)? as i16 as u32 & 0xFFFF;
            let primary: u32 = match op {
                "lb" => 0x20,
                "lh" => 0x21,
                "lw" => 0x23,
                "lbu" => 0x24,
                "lhu" => 0x25,
                "sb" => 0x28,
                "sh" => 0x29,
                _ => 0x2B,
            };
            Some((primary << 26) | (rs << 21) | (rt << 16) | i)
        }
        _ => None,
    }
}

/// Verdict of comparing a dump's printed instructions to an image's bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Attribution {
    /// At least one printed instruction re-encodes to a **non-zero** word that
    /// the image carries there.
    Confirmed,
    /// A printed instruction re-encodes to a different word: the extent's
    /// bytes are not this image's.
    Refuted,
    /// No printed instruction could be re-encoded, so the bytes say nothing.
    Unverifiable,
}

/// Compare a dump's head instructions against `image` loaded at `base_va`.
///
/// A match on the word `0x00000000` is **not** evidence. `nop` is in the
/// encodable grammar and encodes to zero, so a dump whose head is `nop` agrees
/// with any zero fill in any image at any base - and the corpus contains such
/// dumps, taken over a sibling image's own zero region. One of them
/// (`FUN_801d84b4`, 4646 printed `nop`s) confirmed a 20060-byte extent inside
/// entry `0970`'s 131172-byte zero hole and carried most of that entry's
/// reported code share. A zero match therefore counts for nothing: the verdict
/// needs one printed instruction that re-encodes to a non-zero word the image
/// really carries.
///
/// A mismatch is still a refutation whatever the word, because a *difference*
/// is informative where an agreement with fill is not.
pub fn attribute(dump: &DumpExtent, image: &[u8], base_va: u32) -> Attribution {
    let Some(off) = dump.entry_va.checked_sub(base_va).map(|v| v as usize) else {
        return Attribution::Unverifiable;
    };
    let mut seen_nonzero = false;
    for (i, text) in dump.head_insns.iter().enumerate() {
        let Some(want) = encode_insn(text) else {
            continue;
        };
        let at = off + i * 4;
        let Some(w) = image.get(at..at + 4) else {
            return Attribution::Unverifiable;
        };
        if u32::from_le_bytes(w.try_into().unwrap()) != want {
            return Attribution::Refuted;
        }
        seen_nonzero |= want != 0;
    }
    if seen_nonzero {
        Attribution::Confirmed
    } else {
        Attribution::Unverifiable
    }
}
