//! Static overlay-extraction pipeline.
//!
//! Most of Legaia's gameplay logic lives in RAM **overlays** paged into the
//! `0x801C0000+` overlay window per game mode (title / field / battle / menu /
//! world-map / cutscene / minigames). The historical way to reverse them is to
//! capture an emulator save state and import the live RAM image into Ghidra at
//! its runtime base (see [`docs/tooling/overlay-capture.md`]). That works, but
//! it throws away **overlay identity**: many overlays link to the same VA range
//! (e.g. `0x801DD864` is a battle-action function in one overlay and a
//! muscle-dome function in another), which is why the repo disambiguates with
//! the `overlay_<label>_<addr>` naming and behavioural fingerprints.
//!
//! PSX overlays are normally **clean copies** of a fixed-VA-linked blob: the
//! loader DMAs the bytes into the overlay window, runs `FlushCache`, and jumps
//! in - no per-load relocation. This game's overlay code ships as
//! MIPS-code entries inside `PROT.DAT` (the [`crate::mips_overlay`] /
//! [`crate::overlay_ptr_table`] detectors already flag the small ones; the big
//! scene overlays are raw too, just data-section-first). So each overlay can be
//! extracted **statically** from `PROT.DAT` and disassembled at its load base,
//! with identity attached from the first byte: an overlay becomes "PROT entry N
//! at base X", not a guessed label.
//!
//! What this buys (and its limit):
//! - It solves the VA-aliasing identity problem **structurally** - the source
//!   PROT entry is the identity.
//! - Overlay disassembly becomes reproducible from the user's disc with no
//!   curated save state, including overlays nobody ever captured.
//! - It does **not** unblock runtime-value captures (`gp[0x754]==3`,
//!   watchpoint results, `ctx[+0x274]` bytes) - those still need live probes.
//!   This is a workflow + coverage + identity win; the dynamic captures remain
//!   authoritative for runtime values.
//!
//! ### Clean-copy proof
//!
//! A clean copy is verified two ways:
//! - **Static reproducibility:** the as-loaded bytes extracted from the disc
//!   hash to the committed [`OverlayRecord::fingerprint_sha256`] (no Sony bytes
//!   committed - just the hash).
//! - **Runtime byte-match (disc + save-state gated):** the on-disc as-loaded
//!   bytes are byte-identical to the resident RAM image over the whole
//!   `.text`+`.rodata` region; only the trailing `.bss` / runtime-state region
//!   diverges (the runtime zeroes / writes it after the copy). The verified
//!   prefix length is [`OverlayRecord::clean_copy_bytes`]. For PROT 0898
//!   (battle) the prefix is `0x28800` of `0x29800` bytes - 100% of code/rodata.
//!
//! ### Base recovery
//!
//! The load base is recovered **statically** from the overlay's own internal
//! `jal` targets via a voting scheme ([`recover_base`]): for the true base `B`
//! every internal call target `T` maps to file offset `T - B`, which begins a
//! function prologue (`addiu sp, sp, -X`). Tallying `B = T - prologue_offset`
//! over every `(jal_target, prologue_offset)` pair, the true base wins by a
//! landslide. The runtime byte-match cross-checks the recovered base against the
//! RAM-observed one.
//!
//! See [`docs/tooling/static-overlay-pipeline.md`] for the end-to-end workflow.

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::sync::OnceLock;

/// The runtime overlay window (`0x801C0000`–`0x80200000`, 256 KiB). Every
/// overlay loads at a base inside this range.
pub const OVERLAY_WINDOW_LO: u32 = 0x801C_0000;
pub const OVERLAY_WINDOW_HI: u32 = 0x8020_0000;

/// On-disc storage form of an overlay's `PROT.DAT` entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OverlayForm {
    /// Stored uncompressed - the entry bytes are the as-loaded bytes verbatim.
    #[default]
    Raw,
    /// Stored LZS-compressed - decompress to get the as-loaded bytes. Requires
    /// [`OverlayRecord::decompressed_size`] (LZS carries no length prefix).
    Lzs,
}

/// How strongly the clean-copy / base claim is backed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Eligibility {
    /// Byte-matched against a resident RAM image from a save state: the base is
    /// RAM-confirmed and the `.text`+`.rodata` prefix is byte-identical.
    Verified,
    /// Extracted + MIPS-confirmed + base statically recovered, but no save
    /// state captures this overlay resident, so the clean copy is asserted from
    /// the disc bytes alone (no RAM cross-check).
    Static,
    /// Not a clean copy - runtime-relocated or runtime-constructed. Keep on the
    /// dynamic capture path; do not trust a static disassembly.
    Ineligible,
}

/// How the committed `base_va` was determined - gates the reproducibility
/// check (jal-recovery is asserted only for `Jal`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BaseSource {
    /// Recovered from the overlay's own internal `jal` call graph
    /// ([`recover_base`]). The reproducibility test asserts the recovery
    /// reproduces this base.
    #[default]
    Jal,
    /// Pinned by byte-matching a resident RAM image (a function anchor or a
    /// clean prefix). Used where the overlay's internal call graph is too sparse
    /// to triangulate.
    Capture,
    /// Cross-referenced from another pinned RE result in-tree (e.g. the summon
    /// cluster's link base in `summon_overlay::SUMMON_OVERLAY_LINK_BASE`). Used
    /// for timeshared-buffer overlays that have no clean whole-overlay RAM match
    /// and no internal call graph to recover from.
    CrossRef,
}

/// One overlay's identity + load metadata. The committed map is a list of these.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OverlayRecord {
    /// `PROT.DAT` entry index this overlay is extracted from.
    pub prot_index: u32,
    /// Short identity handle (matches the `overlay_<label>_<addr>` dump naming).
    pub label: String,
    /// Load base VA inside the overlay window - the first as-loaded byte maps
    /// here.
    pub base_va: u32,
    /// On-disc storage form.
    #[serde(default)]
    pub form: OverlayForm,
    /// Decompressed size in bytes - required (and only used) when `form = lzs`.
    #[serde(default)]
    pub decompressed_size: Option<u32>,
    /// Length of the byte-verified `.text`+`.rodata` prefix (the clean-copy
    /// region). `None` when not RAM-cross-checked.
    #[serde(default)]
    pub clean_copy_bytes: Option<u32>,
    /// Backing strength of the clean-copy / base claim.
    pub eligibility: Eligibility,
    /// How `base_va` was determined (gates the jal-recovery reproducibility
    /// assertion). Defaults to [`BaseSource::Jal`].
    #[serde(default)]
    pub base_source: BaseSource,
    /// Optional known function VA that must land on a function prologue at
    /// `base_va` (file offset `anchor_va - base_va`). A capture-free,
    /// disc-reproducible cross-check of the load base - decisive for rows whose
    /// `base_source` is not `jal` (the jal-recovery assertion is skipped for
    /// those, so the anchor keeps the base claim non-vacuous). See
    /// [`anchor_lands_on_prologue`].
    #[serde(default)]
    pub anchor_va: Option<u32>,
    /// sha256 (hex) of the as-loaded bytes. Disc-derived hash - committable, no
    /// Sony bytes. Re-extraction must reproduce this exactly.
    #[serde(default)]
    pub fingerprint_sha256: Option<String>,
    /// Free-text identity notes (which subsystems / entry points live here).
    #[serde(default)]
    pub notes: String,
}

impl OverlayRecord {
    /// Stable program name a Ghidra import uses: `overlay_<label>`.
    pub fn program_name(&self) -> String {
        format!("overlay_{}", self.label)
    }

    /// Filename for the extracted as-loaded blob (gitignored - it's Sony code).
    pub fn bin_filename(&self) -> String {
        format!("overlay_{}_{:04}.bin", self.label, self.prot_index)
    }
}

/// The parsed static-overlay map.
#[derive(Debug, Clone, Deserialize)]
pub struct OverlayMap {
    #[serde(default)]
    pub overlays: Vec<OverlayRecord>,
}

const MAP_TOML: &str = include_str!("../data/static-overlays.toml");

/// The committed static-overlay map, parsed once. Panics at first use if the
/// embedded TOML is malformed - that is a build-time authoring error, caught by
/// [`tests::embedded_map_parses`].
pub fn overlay_map() -> &'static OverlayMap {
    static MAP: OnceLock<OverlayMap> = OnceLock::new();
    MAP.get_or_init(|| {
        toml::from_str(MAP_TOML).expect("crates/asset/data/static-overlays.toml is malformed")
    })
}

impl OverlayMap {
    /// Parse a map from TOML text (for tooling that loads an external map).
    pub fn from_toml(text: &str) -> Result<Self> {
        toml::from_str(text).context("parsing static-overlay map TOML")
    }

    /// Look up by PROT index.
    pub fn by_prot_index(&self, idx: u32) -> Option<&OverlayRecord> {
        self.overlays.iter().find(|o| o.prot_index == idx)
    }

    /// Look up by label.
    pub fn by_label(&self, label: &str) -> Option<&OverlayRecord> {
        self.overlays.iter().find(|o| o.label == label)
    }
}

/// Turn a `PROT.DAT` entry's raw bytes into its **as-loaded** form (the bytes
/// the loader DMAs into the overlay window). For `Raw` this is the entry bytes
/// unchanged; for `Lzs` it decompresses to `decompressed_size`.
pub fn as_loaded(entry_bytes: &[u8], rec: &OverlayRecord) -> Result<Vec<u8>> {
    match rec.form {
        OverlayForm::Raw => Ok(entry_bytes.to_vec()),
        OverlayForm::Lzs => {
            let size = rec.decompressed_size.context(
                "overlay form = lzs requires decompressed_size (LZS carries no length prefix)",
            )? as usize;
            legaia_lzs::decompress(entry_bytes, size)
                .with_context(|| format!("LZS-decompressing overlay {}", rec.label))
        }
    }
}

/// sha256 (lowercase hex) of a byte slice - the as-loaded fingerprint.
pub fn fingerprint(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    let digest = h.finalize();
    let mut s = String::with_capacity(64);
    for b in digest {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

/// First printable-ASCII run of at least `min_len` bytes within the first
/// `window` bytes of an overlay. The data-section-first overlays lead with
/// developer debug strings that name the subsystem (`town01`, `Display Off`,
/// `\DATA\MOV.STR;1`, `Hell's Music`, …), so this is the cheapest identity
/// tell for an entry the base recovery alone can't name. Returns the run
/// trimmed of trailing whitespace, or `None` when the head is binary.
pub fn head_string(bytes: &[u8], window: usize, min_len: usize) -> Option<String> {
    let end = window.min(bytes.len());
    let mut best: Option<String> = None;
    let mut run = String::new();
    let flush = |run: &mut String, best: &mut Option<String>| {
        if run.len() >= min_len {
            let trimmed = run.trim();
            if !trimmed.is_empty() && best.is_none() {
                *best = Some(trimmed.to_string());
            }
        }
        run.clear();
    };
    for &b in &bytes[..end] {
        // Printable ASCII incl. space + tab + backslash; the dev strings use
        // backslashes for DOS paths and `%` for printf specifiers.
        if (0x20..=0x7e).contains(&b) || b == b'\t' {
            run.push(b as char);
        } else {
            flush(&mut run, &mut best);
            if best.is_some() {
                break;
            }
        }
    }
    flush(&mut run, &mut best);
    best
}

/// Locate a function-head instruction signature in a blob. Returns the file
/// offset of the first occurrence. Pair with a known anchor VA to infer the
/// load base (`base = anchor_va - offset`) - the byte-search that pinned the
/// menu overlay (PROT 0899) by `FUN_801CF650`'s signature, generalised. The
/// signature is the literal little-endian machine code of the first few
/// instructions; instructions that reference fixed SCUS globals (`lui`/`lw`
/// against `0x8008xxxx`) encode identically regardless of the overlay's load
/// base, so the search is base-independent.
pub fn find_signature(blob: &[u8], signature: &[u8]) -> Option<usize> {
    if signature.is_empty() || signature.len() > blob.len() {
        return None;
    }
    blob.windows(signature.len()).position(|w| w == signature)
}

/// How many of an overlay's internal absolute pointers resolve inside the
/// file when it is loaded at `base_va`. The slot-B overlays (summon stagers,
/// effect data) have sparse internal `jal` call graphs - too sparse to
/// recover a base from - but they are dense with absolute self-pointers built
/// as `lui rX, hi ; addiu rX, rX, lo` (the standard MIPS 32-bit-immediate
/// idiom). If the committed base is right, a high fraction of those pointers
/// land inside `[base_va, base_va + len)`. This is the base cross-check for
/// `cross_ref` slot-B rows, analogous to [`anchor_lands_on_prologue`] for
/// slot-A. Returns `(resolved_in_file, total_pointers_found)`.
pub fn pointer_resolution(as_loaded: &[u8], base_va: u32) -> (u32, u32) {
    let len = as_loaded.len() as u32;
    let hi_lo = base_va & 0xFFFF_0000;
    let hi_hi = hi_lo.wrapping_add(0x0001_0000);
    // The overlay window's two high halves the pointers can carry (e.g. 0x801F
    // and 0x8020 for the summon link base 0x801F69D8).
    let hi0 = hi_lo >> 16;
    let hi1 = hi_hi >> 16;
    let mut total = 0u32;
    let mut resolved = 0u32;
    let words = as_loaded.len() / 4;
    for i in 0..words {
        let w1 = legaia_bytes::u32_le(as_loaded, i * 4).unwrap();
        if w1 >> 26 != 0x0F {
            continue; // not lui
        }
        let rt = (w1 >> 16) & 0x1F;
        let hi = w1 & 0xFFFF;
        if hi != hi0 && hi != hi1 {
            continue;
        }
        // Look for the paired `addiu rt, rt, lo` within a short window.
        for j in (i + 1)..(i + 7).min(words) {
            let w2 = legaia_bytes::u32_le(as_loaded, j * 4).unwrap();
            if w2 >> 26 == 0x09 && (w2 >> 21) & 0x1F == rt {
                let lo = (w2 & 0xFFFF) as i16 as i32;
                let addr = ((hi << 16) as i32).wrapping_add(lo) as u32;
                total += 1;
                if (base_va..base_va.wrapping_add(len)).contains(&addr) {
                    resolved += 1;
                }
                break;
            }
        }
    }
    (resolved, total)
}

/// Result of a static base recovery.
#[derive(Debug, Clone, Copy)]
pub struct BaseRecovery {
    /// Recovered load base VA (first as-loaded byte maps here).
    pub base_va: u32,
    /// How many internal `jal` targets corroborate this base (= how many call
    /// targets land on a function prologue at file offset `target - base`).
    pub votes: u32,
    /// Total **distinct** internal `jal` targets considered.
    pub jal_targets: u32,
    /// Total `addiu sp, sp, -X` prologues found in the blob.
    pub prologues: u32,
}

const MIPS_JAL_OP: u32 = 0x03;
const ADDIU_SP_NEG: u32 = 0x27BD_FF00;
const ADDIU_SP_NEG_MASK: u32 = 0xFFFF_FF00;

/// Is this word a function-prologue `addiu sp, sp, -X` with a plausible stack
/// adjust (8..=128 bytes)? Mirrors [`crate::mips_overlay`]'s first check.
#[inline]
pub fn is_prologue(word: u32) -> bool {
    word & ADDIU_SP_NEG_MASK == ADDIU_SP_NEG && (0x80..=0xF8).contains(&(word & 0xFF))
}

/// Does a known function VA land on a function prologue when the as-loaded
/// blob is placed at `base_va`? The file offset is `anchor_va - base_va`; the
/// word there must be an `addiu sp, sp, -X` prologue. This is the capture-free
/// base cross-check for rows whose base did not come from jal-recovery (a
/// documented function VA is a pinned in-tree RE result; if the committed base
/// is right, that function's first instruction is its prologue).
pub fn anchor_lands_on_prologue(as_loaded: &[u8], anchor_va: u32, base_va: u32) -> bool {
    let off = match anchor_va.checked_sub(base_va) {
        Some(o) => o as usize,
        None => return false,
    };
    match legaia_bytes::u32_le(as_loaded, off) {
        Some(word) => is_prologue(word),
        None => false,
    }
}

/// Recover an overlay's load base **statically** from its own internal call
/// graph. See the module docs for the voting scheme. Returns `None` when the
/// blob carries too little internal call structure to triangulate (fewer than
/// `min_votes` corroborating targets).
pub fn recover_base(code: &[u8], min_votes: u32) -> Option<BaseRecovery> {
    let words = code.len() / 4;
    if words < 16 {
        return None;
    }

    // Prologue offsets (4-aligned file offsets that begin a function), as a set.
    let mut prologue_offsets: Vec<u32> = Vec::new();
    for w in 0..words {
        let word = legaia_bytes::u32_le(code, w * 4).unwrap();
        if is_prologue(word) {
            prologue_offsets.push((w * 4) as u32);
        }
    }
    if prologue_offsets.is_empty() {
        return None;
    }
    let prologue_set: std::collections::HashSet<u32> = prologue_offsets.iter().copied().collect();

    // Internal jal targets (within the overlay window). Dedup to DISTINCT
    // targets: many call sites to one function must not nominate a phantom base
    // (offset by that function's own prologue offset) more than the true base.
    let mut distinct_targets: Vec<u32> = Vec::new();
    {
        let mut seen = std::collections::HashSet::new();
        for w in 0..words {
            let word = legaia_bytes::u32_le(code, w * 4).unwrap();
            if word >> 26 == MIPS_JAL_OP {
                // jal target = (PC & 0xF000_0000) | (imm26 << 2); overlay PC top
                // nibble is 0x8, so the target high nibble is forced to 0x8.
                let target = 0x8000_0000 | ((word & 0x03FF_FFFF) << 2);
                if (OVERLAY_WINDOW_LO..OVERLAY_WINDOW_HI).contains(&target) && seen.insert(target) {
                    distinct_targets.push(target);
                }
            }
        }
    }
    if distinct_targets.is_empty() {
        return None;
    }

    // Vote: each (distinct target, prologue_offset) pair nominates
    // base = target - off. The true base collects one vote per distinct
    // internal function called (each function's call target minus its own
    // prologue offset lands on the same base); phantom bases only align for a
    // coincidental subset.
    let mut votes: std::collections::HashMap<u32, u32> = std::collections::HashMap::new();
    for &t in &distinct_targets {
        for &off in &prologue_offsets {
            let base = t.wrapping_sub(off);
            if (OVERLAY_WINDOW_LO..OVERLAY_WINDOW_HI).contains(&base) {
                *votes.entry(base).or_insert(0) += 1;
            }
        }
    }

    // The winning base is the one most distinct targets agree on. `corroborating`
    // counts the distinct call targets that land on a prologue at `target - base`.
    let (&best_base, _) = votes.iter().max_by_key(|&(_, &c)| c)?;
    let corroborating = distinct_targets
        .iter()
        .filter(|&&t| {
            let off = t.wrapping_sub(best_base);
            prologue_set.contains(&off)
        })
        .count() as u32;

    if corroborating < min_votes {
        return None;
    }

    Some(BaseRecovery {
        base_va: best_base,
        votes: corroborating,
        jal_targets: distinct_targets.len() as u32,
        prologues: prologue_offsets.len() as u32,
    })
}

/// Generate a Ghidra Jython import-and-name script for one overlay. The script
/// imports the gitignored as-loaded `.bin` at the overlay's recovered base with
/// the program named `overlay_<label>`, so the disassembly carries identity from
/// the first byte. ASCII-only + the `# @runtime` / `# @category` headers Ghidra
/// requires (Jython 2.7 chokes on non-ASCII source).
pub fn ghidra_import_jython(rec: &OverlayRecord) -> String {
    let prog = rec.program_name();
    let bin = rec.bin_filename();
    let mut s = String::new();
    s.push_str("# @runtime Jython\n");
    s.push_str("# @category Legaia\n");
    s.push_str("#\n");
    s.push_str(&format!(
        "# Static overlay import: PROT entry {} -> base 0x{:08X} ({}).\n",
        rec.prot_index, rec.base_va, prog
    ));
    s.push_str("# Auto-generated by `asset overlay ghidra`. Imports the as-loaded\n");
    s.push_str("# overlay blob at its fixed VA so functions land at their real\n");
    s.push_str("# addresses, identity attached from PROT entry, not a guessed label.\n");
    s.push_str("#\n");
    s.push_str("# Run headless, e.g.:\n");
    s.push_str(&format!(
        "#   analyzeHeadless /projects legaia -import /data/{} \\\n",
        bin
    ));
    s.push_str(&format!(
        "#     -loader BinaryLoader -loader-baseAddr 0x{:08X} \\\n",
        rec.base_va
    ));
    s.push_str("#     -processor MIPS:LE:32:default -overwrite\n");
    s.push_str("#\n");
    s.push_str("# This script (post-import) just renames the program for identity.\n");
    s.push_str("import os\n\n");
    s.push_str("prog = getCurrentProgram()\n");
    s.push_str(&format!("prog.setName(\"{}\")\n", prog));
    s.push_str(&format!(
        "print(\"[overlay] {} <- PROT {} @ base 0x{:08X}\")\n",
        prog, rec.prot_index, rec.base_va
    ));
    s
}

/// Generate a shell driver that imports every eligible overlay into the Ghidra
/// compose service at its recovered base, naming each program `overlay_<label>`.
/// Mirrors the manual flow in `docs/tooling/overlay-capture.md`, but sourced
/// from the disc instead of a save state.
pub fn ghidra_import_driver(map: &OverlayMap) -> String {
    let mut s = String::new();
    s.push_str("#!/usr/bin/env bash\n");
    s.push_str("# Auto-generated by `asset overlay ghidra`. Imports each\n");
    s.push_str("# statically-extracted overlay into the Ghidra compose service\n");
    s.push_str("# at its recovered base, program named overlay_<label>.\n");
    s.push_str("#\n");
    s.push_str("# Usage: copy the extracted overlay .bin files into ghidra:/data\n");
    s.push_str("#   then run this script from the repo root.\n");
    s.push_str("set -euo pipefail\n");
    s.push_str("GHIDRA=(docker compose exec -T ghidra /ghidra/support/analyzeHeadless)\n\n");
    for rec in &map.overlays {
        if rec.eligibility == Eligibility::Ineligible {
            continue;
        }
        let bin = rec.bin_filename();
        s.push_str(&format!(
            "# PROT {} -> {} @ base 0x{:08X}\n",
            rec.prot_index,
            rec.program_name(),
            rec.base_va
        ));
        s.push_str(&format!(
            "\"${{GHIDRA[@]}}\" /projects legaia -import \"/data/{}\" \\\n",
            bin
        ));
        s.push_str(&format!(
            "  -loader BinaryLoader -loader-baseAddr 0x{:08X} \\\n",
            rec.base_va
        ));
        s.push_str("  -processor MIPS:LE:32:default -overwrite\n\n");
    }
    s
}

/// Bail unless a re-extracted blob hashes to the committed fingerprint. Used by
/// the disc-gated reproducibility check and the `verify` CLI.
pub fn verify_fingerprint(rec: &OverlayRecord, as_loaded_bytes: &[u8]) -> Result<()> {
    let want = match &rec.fingerprint_sha256 {
        Some(f) => f,
        None => return Ok(()), // nothing committed to verify against
    };
    let got = fingerprint(as_loaded_bytes);
    if &got != want {
        bail!(
            "overlay {} (PROT {}) fingerprint mismatch: committed {} != re-extracted {}",
            rec.label,
            rec.prot_index,
            want,
            got
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_map_parses() {
        let m = overlay_map();
        assert!(
            !m.overlays.is_empty(),
            "embedded static-overlay map should not be empty"
        );
        // Every base is inside the overlay window; fingerprints are 64-hex.
        for o in &m.overlays {
            assert!(
                (OVERLAY_WINDOW_LO..OVERLAY_WINDOW_HI).contains(&o.base_va),
                "{} base 0x{:08x} outside overlay window",
                o.label,
                o.base_va
            );
            if let Some(fp) = &o.fingerprint_sha256 {
                assert_eq!(fp.len(), 64, "{} fingerprint not 64 hex chars", o.label);
                assert!(fp.bytes().all(|b| b.is_ascii_hexdigit()));
            }
            if o.form == OverlayForm::Lzs {
                assert!(
                    o.decompressed_size.is_some(),
                    "{} form=lzs needs decompressed_size",
                    o.label
                );
            }
        }
    }

    #[test]
    fn labels_and_indices_unique() {
        let m = overlay_map();
        let mut idx: Vec<u32> = m.overlays.iter().map(|o| o.prot_index).collect();
        let mut lbl: Vec<&str> = m.overlays.iter().map(|o| o.label.as_str()).collect();
        idx.sort_unstable();
        let before = idx.len();
        idx.dedup();
        assert_eq!(before, idx.len(), "duplicate prot_index in map");
        lbl.sort_unstable();
        let before = lbl.len();
        lbl.dedup();
        assert_eq!(before, lbl.len(), "duplicate label in map");
    }

    #[test]
    fn fingerprint_is_stable_sha256() {
        // sha256("") known vector.
        assert_eq!(
            fingerprint(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            fingerprint(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn as_loaded_raw_is_identity() {
        let rec = OverlayRecord {
            prot_index: 1,
            label: "t".into(),
            base_va: 0x801C_0000,
            form: OverlayForm::Raw,
            decompressed_size: None,
            clean_copy_bytes: None,
            eligibility: Eligibility::Static,
            base_source: BaseSource::Jal,
            anchor_va: None,
            fingerprint_sha256: None,
            notes: String::new(),
        };
        let bytes = vec![1u8, 2, 3, 4];
        assert_eq!(as_loaded(&bytes, &rec).unwrap(), bytes);
    }

    /// Synthesise a tiny overlay: a data run, then several functions that call
    /// each other via `jal`. `recover_base` must find the base purely from those
    /// internal calls. Multiple DISTINCT call targets are what break the
    /// base-vs-(base +/- prologue_offset) tie a single-target blob would have.
    #[test]
    fn recover_base_from_synthetic_call_graph() {
        let base: u32 = 0x801C_E818;
        // Functions at these file offsets (each begins with a prologue).
        let fn_offs = [0x10u32, 0x40, 0x60, 0x90, 0xB0];
        let mut code = vec![0u8; 0x100];
        let put = |code: &mut [u8], off: usize, word: u32| {
            code[off..off + 4].copy_from_slice(&word.to_le_bytes());
        };
        let jal_to = |va: u32| (MIPS_JAL_OP << 26) | ((va & 0x0FFF_FFFF) >> 2);
        // addiu sp, sp, -0x18 = 0x27BDFFE8 prologue at each function start.
        for &f in &fn_offs {
            put(&mut code, f as usize, 0x27BD_FFE8);
        }
        // Each function calls the next (distinct targets) -> the base is the
        // unique value where every target minus its prologue offset agrees.
        for i in 0..fn_offs.len() - 1 {
            let caller = fn_offs[i] as usize + 8;
            let callee_va = base + fn_offs[i + 1];
            put(&mut code, caller, jal_to(callee_va));
        }
        let rec = recover_base(&code, 3).expect("should recover base");
        assert_eq!(rec.base_va, base, "recovered base mismatch");
        assert!(rec.votes >= 3, "votes = {}", rec.votes);
    }

    #[test]
    fn head_string_finds_leading_run() {
        // Binary prologue, then a dev string, then more binary.
        let mut blob = vec![0u8, 1, 2, 3];
        blob.extend_from_slice(b"Display Off");
        blob.extend_from_slice(&[0, 0, 0xff]);
        assert_eq!(head_string(&blob, 0x400, 5).as_deref(), Some("Display Off"));
        // A run shorter than min_len is ignored.
        assert_eq!(head_string(b"\x00ab\x00", 0x400, 5), None);
        // Pure binary head -> None.
        assert_eq!(head_string(&[0u8, 1, 2, 0xff, 0x80], 0x400, 4), None);
    }

    #[test]
    fn pointer_resolution_counts_in_file_self_pointers() {
        let base = 0x801F_69D8u32;
        // Build `lui v0, 0x801f ; addiu v0, v0, 0x6a00` -> 0x801F6A00 (in file)
        // and `lui v0, 0x8020 ; addiu v0, v0, -0x7000` -> 0x8019_9000 (out).
        let lui = |rt: u32, hi: u32| (0x0Fu32 << 26) | (rt << 16) | hi;
        let addiu = |rt: u32, lo: u16| (0x09u32 << 26) | (rt << 21) | (rt << 16) | lo as u32;
        let mut code: Vec<u8> = Vec::new();
        let mut push = |w: u32| code.extend_from_slice(&w.to_le_bytes());
        push(lui(2, 0x801F));
        push(addiu(2, 0x6A00)); // -> 0x801F6A00, inside a 0x8000-byte file
        push(lui(2, 0x8020));
        push(addiu(2, 0x9000u16)); // 0x8020_0000 + (i16)0x9000(-0x7000) = 0x801F9000, in file
        code.resize(0x8000, 0);
        let (resolved, total) = pointer_resolution(&code, base);
        assert_eq!(total, 2);
        assert_eq!(resolved, 2);
        // Wrong base -> nothing resolves.
        let (r2, t2) = pointer_resolution(&code, 0x801C_E818);
        assert_eq!(t2, 0, "no 0x801c/0x801d pointers in this blob");
        assert_eq!(r2, 0);
    }

    #[test]
    fn anchor_prologue_check() {
        // Blob where file offset 0x10 holds `addiu sp, sp, -0x18` (a prologue).
        let mut blob = vec![0u8; 0x20];
        blob[0x10..0x14].copy_from_slice(&0x27BD_FFE8u32.to_le_bytes());
        let base = 0x801C_E818;
        assert!(anchor_lands_on_prologue(&blob, base + 0x10, base));
        // Offset 0 is not a prologue.
        assert!(!anchor_lands_on_prologue(&blob, base, base));
        // Anchor below base -> false (no underflow panic).
        assert!(!anchor_lands_on_prologue(&blob, base - 4, base));
    }

    #[test]
    fn find_signature_locates_and_misses() {
        let blob = [0u8, 1, 2, 0xde, 0xad, 0xbe, 0xef, 9];
        assert_eq!(find_signature(&blob, &[0xde, 0xad, 0xbe, 0xef]), Some(3));
        assert_eq!(find_signature(&blob, &[0x12, 0x34]), None);
        assert_eq!(find_signature(&blob, &[]), None);
    }

    #[test]
    fn ghidra_jython_is_ascii_and_has_headers() {
        let rec = OverlayRecord {
            prot_index: 898,
            label: "battle_action".into(),
            base_va: 0x801C_E818,
            form: OverlayForm::Raw,
            decompressed_size: None,
            clean_copy_bytes: Some(0x28800),
            eligibility: Eligibility::Verified,
            base_source: BaseSource::Jal,
            anchor_va: None,
            fingerprint_sha256: None,
            notes: String::new(),
        };
        let script = ghidra_import_jython(&rec);
        assert!(script.is_ascii(), "Jython source must be ASCII-only");
        assert!(script.contains("# @runtime Jython"));
        assert!(script.contains("# @category Legaia"));
        assert!(script.contains("overlay_battle_action"));
        assert!(script.contains("0x801CE818"));
    }
}
