//! Byte accounting inside one PROT entry: which bytes a parser claims, and
//! what shape the rest has.
//!
//! [`crate::categorize`] answers *what format an entry is*. That is format
//! **recognition**, and `scripts/ci/disc-coverage.py` says so in its own
//! output: knowing entry `0867` is a 15.9 MB monster archive says nothing
//! about which bytes inside it are understood. This module answers the
//! sibling question - **accounting** - by running every parser this workspace
//! already has that applies to the entry's class, collecting the byte ranges
//! those parsers actually consume, and reporting the complement.
//!
//! ## Method
//!
//! 1. Classify the buffer with [`crate::categorize::classify`] (plus two
//!    index-keyed overrides, below) to pick a [`Walker`].
//! 2. Run the walker. It emits [`Claim`]s - half-open `[start, end)` byte
//!    ranges, each tagged with an [owner](OWNERS) naming *what kind of thing*
//!    consumes those bytes and a free-form `detail` naming the instance.
//! 3. Merge the claims (they may overlap and arrive out of order) and take the
//!    complement against the buffer length. Each maximal uncovered run becomes
//!    one [`Residue`], classified by [`ResidueShape`].
//! 4. Where a claim covers a *compressed* span, the walker also decodes it and
//!    accounts the decoded payload in a nested pass ([`Nested`]). The two
//!    figures are reported separately: an LZS stream is 100 % accounted on the
//!    outer pass by construction, and the interesting number is what the
//!    decoded side accounts to.
//!
//! ## What the number does and does not mean
//!
//! `accounted_pct` is *the share of the entry's bytes some parser in this
//! workspace consumes*, not the share whose meaning is understood. A fixed
//! four-region file like [`crate::field_map`] accounts to 100 % the moment its
//! region constants are written down, and three of those four regions have
//! per-field semantics that are only partly pinned. Read the figure as an
//! upper bound on understanding and a lower bound on structure, and read the
//! residue list as the worklist - a large high-entropy or plausible-MIPS run
//! that no parser claims is a format nobody has walked.
//!
//! Claims are also tiered. A claim whose owner is [`OWNER_SCAN`] came from a
//! magic sweep over the residue ([`AccountOptions::rescan`]) rather than from
//! a structural walk, so it is evidence that a sub-asset is *there*, not that
//! the container's own layout was followed to it. [`Account::structural`]
//! excludes those; [`Account::accounted`] includes them.
//!
//! ## Overlay code entries
//!
//! For an entry that is a runtime overlay image, the "parser" is the Ghidra
//! dump corpus: a dumped function's `(entry, size)` header states an extent,
//! and that extent maps to file offsets through the overlay's load base
//! (`crates/asset/data/static-overlays.toml`). Slot-A overlays all share base
//! `0x801CE818`, so a printed VA alone cannot say which image a dump belongs
//! to (`docs/tooling/call-target-integrity.md`). This module resolves that
//! from the bytes: it re-encodes the dump's first printed instructions and
//! compares them word-for-word against the image at the mapped offset. An
//! extent whose instructions match is credited; one that mismatches belongs to
//! an aliased sibling and is dropped; one whose instruction text this module
//! cannot encode stays [`ambiguous`](Account::ambiguous_dumps) and is credited
//! only when the dump's filename label names this entry.
//!
//! No Ghidra invocation is involved - the header parse mirrors
//! `scripts/ghidra-analysis/dump_header.py` and is re-implemented here so the
//! instrument runs from a checkout plus a dump directory.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::categorize::{Class, classify};
use crate::{AssetType, parse_streaming};

// ---------------------------------------------------------------------------
// Owner vocabulary
// ---------------------------------------------------------------------------

/// A container's own fixed head words (magic, counts, totals).
pub const OWNER_HEADER: &str = "header";
/// An offset / descriptor / index table the container's reader walks.
pub const OWNER_TOC: &str = "toc";
/// A compressed span. The decoded side is accounted in a [`Nested`] pass.
pub const OWNER_LZS: &str = "lzs";
/// A PSX TIM (header + CLUT block + pixel block).
pub const OWNER_TIM: &str = "tim";
/// A Legaia TMD mesh.
pub const OWNER_TMD: &str = "tmd";
/// A Sony VAB instrument bank (header, program/tone tables, VAG bodies).
pub const OWNER_VAB: &str = "vab";
/// A PsyQ SEQ sequence (header + event stream to the end-of-track meta).
pub const OWNER_SEQ: &str = "seq";
/// An ANM animation record.
pub const OWNER_ANM: &str = "anm";
/// A fixed-stride data record (stat record, spell entry, trigger row).
pub const OWNER_RECORD: &str = "record";
/// A dense per-tile grid (collision, object index, floor height).
pub const OWNER_GRID: &str = "grid";
/// A bytecode / script body (field-VM, move-VM, event prescript).
pub const OWNER_SCRIPT: &str = "script";
/// MIPS instructions inside a dumped function's extent.
pub const OWNER_CODE: &str = "code";
/// A raw texture page (indices, no TIM header).
pub const OWNER_TEXTURE: &str = "texture";
/// A palette / CLUT region.
pub const OWNER_CLUT: &str = "clut";
/// A NUL-terminated string a parser resolves a pointer to.
pub const OWNER_STRING: &str = "string";
/// Bytes a container's own size math covers but that carry no content
/// (declared slack inside a fixed-stride slot).
pub const OWNER_PAD: &str = "pad";
/// Another image's bytes, at the same file offset: the run from where this
/// overlay stops being its own content. See [`crate::inherited_tail`].
pub const OWNER_INHERITED_TAIL: &str = "inherited_tail";
/// Found by a magic sweep over the residue, not by a structural walk.
pub const OWNER_SCAN: &str = "scan";

/// The owner vocabulary, `(owner, meaning)`. `docs/tooling/byte-accounting.md`
/// documents the same list; keep the two in step.
pub const OWNERS: &[(&str, &str)] = &[
    (OWNER_HEADER, "container's own fixed head words"),
    (OWNER_TOC, "offset / descriptor / index table"),
    (OWNER_LZS, "compressed span (decoded side accounted nested)"),
    (OWNER_TIM, "PSX TIM"),
    (OWNER_TMD, "Legaia TMD mesh"),
    (OWNER_VAB, "Sony VAB instrument bank"),
    (OWNER_SEQ, "PsyQ SEQ sequence"),
    (OWNER_ANM, "ANM animation record"),
    (OWNER_RECORD, "fixed-stride data record"),
    (OWNER_GRID, "dense per-tile grid"),
    (OWNER_SCRIPT, "bytecode / script body"),
    (OWNER_CODE, "MIPS instructions in a dumped function extent"),
    (OWNER_TEXTURE, "raw texture page"),
    (OWNER_CLUT, "palette / CLUT region"),
    (OWNER_STRING, "NUL-terminated string reached by a pointer"),
    (OWNER_PAD, "declared slack inside a fixed-stride slot"),
    (
        OWNER_INHERITED_TAIL,
        "another image's bytes at the same file offset (mastering-buffer residue)",
    ),
    (
        OWNER_SCAN,
        "magic sweep over the residue, not a structural walk",
    ),
];

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// One byte range a parser consumes.
#[derive(Debug, Clone, Serialize)]
pub struct Claim {
    pub start: usize,
    pub end: usize,
    pub owner: &'static str,
    pub detail: String,
}

impl Claim {
    pub fn new(start: usize, end: usize, owner: &'static str, detail: impl Into<String>) -> Self {
        Self {
            start,
            end,
            owner,
            detail: detail.into(),
        }
    }
    pub fn len(&self) -> usize {
        self.end.saturating_sub(self.start)
    }
    pub fn is_empty(&self) -> bool {
        self.end <= self.start
    }
}

/// Shape of a run of bytes no parser claims.
///
/// The order the tests run in is the order of the variants below, and it is
/// load-bearing: a pointer table decodes as plausible MIPS (`0x801Cxxxx` has
/// primary opcode `0x20`), so [`PointerDense`](ResidueShape::PointerDense) is
/// tested before [`PlausibleMips`](ResidueShape::PlausibleMips), exactly as
/// `scripts/ci/disc-coverage.py` does for code gaps.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ResidueShape {
    /// Every byte `0x00`: sector / record padding.
    ZeroPad,
    /// Shorter than [`ALIGNMENT_MAX`] and not all zero: inter-record alignment.
    Alignment,
    /// The run is one short pattern (period 1, 2, 4, 8 or 16) repeated:
    /// dev fill, not content.
    RepeatedFill,
    /// At least [`ASCII_MIN`] of the bytes are printable ASCII or NUL: a
    /// string pool.
    AsciiText,
    /// At least [`PTR_MIN`] of the words land in the PSX RAM window: a pointer
    /// or jump table.
    PointerDense,
    /// Nearly every halfword is below `0x8000` and the run carries a wide,
    /// high-entropy spread of them: PSX 15-bit colour data with the STP bit
    /// clear - a CLUT block, a 16bpp page, or raw VRAM. Tested before
    /// [`PlausibleMips`](ResidueShape::PlausibleMips) because a BGR555
    /// halfword pair decodes to a word in the low opcode range and therefore
    /// passes an opcode-plausibility test with a perfect score.
    Bgr555,
    /// The words carry plausible MIPS primary opcodes, spread across at least
    /// [`CODE_MIN_DISTINCT_OPS`] of them, with almost no pointers and no
    /// SPECIAL-opcode landslide: un-dumped code.
    PlausibleMips,
    /// Shannon entropy below [`LOW_ENTROPY_MAX`] bits/byte: tabular data,
    /// sparse vectors, geometry.
    LowEntropy,
    /// Shannon entropy at or above [`HIGH_ENTROPY_MIN`] bits/byte: already
    /// compressed, or sample data.
    HighEntropy,
    /// None of the above.
    Mixed,
}

impl ResidueShape {
    pub fn name(&self) -> &'static str {
        match self {
            ResidueShape::ZeroPad => "zero_pad",
            ResidueShape::Alignment => "alignment",
            ResidueShape::RepeatedFill => "repeated_fill",
            ResidueShape::AsciiText => "ascii_text",
            ResidueShape::PointerDense => "pointer_dense",
            ResidueShape::Bgr555 => "bgr555",
            ResidueShape::PlausibleMips => "plausible_mips",
            ResidueShape::LowEntropy => "low_entropy",
            ResidueShape::HighEntropy => "high_entropy",
            ResidueShape::Mixed => "mixed",
        }
    }
}

/// Runs shorter than this that are not all zero are alignment, not a finding.
pub const ALIGNMENT_MAX: usize = 16;
/// Printable-ASCII share at which a run reads as a string pool.
pub const ASCII_MIN: f32 = 0.80;
/// Share of words in `0x80000000..0x80200000` at which a run reads as a
/// pointer table.
pub const PTR_MIN: f32 = 0.20;
/// Share of words with a plausible MIPS primary opcode at which a run reads as
/// code (mirrors `disc-coverage.py`).
pub const CODE_PLAUSIBLE_MIN: f32 = 0.90;
/// Pointer share above which a run is a table even if it decodes as code.
pub const CODE_PTR_MAX: f32 = 0.03;
/// Share of halfwords that must be below `0x8000` to read as 15-bit colour.
pub const BGR555_MIN: f32 = 0.995;
/// Distinct halfword values a run must carry to read as 15-bit colour rather
/// than as a small-value table.
pub const BGR555_MIN_DISTINCT: usize = 256;
/// Entropy floor for the same test.
pub const BGR555_MIN_ENTROPY: f32 = 4.0;
/// Distinct plausible primary opcodes a run must carry to read as code.
pub const CODE_MIN_DISTINCT_OPS: u32 = 4;
/// Share of words with primary opcode `0` (SPECIAL) above which a run is a
/// small-value table rather than code.
pub const CODE_SPECIAL_MAX: f32 = 0.60;
/// Entropy below this is tabular / sparse.
pub const LOW_ENTROPY_MAX: f32 = 4.0;
/// Entropy at or above this is compressed-looking.
pub const HIGH_ENTROPY_MIN: f32 = 7.2;

/// MIPS primary opcodes a PSX build actually emits. Same set as
/// `scripts/ci/disc-coverage.py`'s `PLAUSIBLE_OPS`.
const PLAUSIBLE_OPS: [bool; 64] = {
    let mut t = [false; 64];
    let ops = [
        0x00usize, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A, 0x0B, 0x0C, 0x0D,
        0x0E, 0x0F, 0x10, 0x11, 0x12, 0x20, 0x21, 0x22, 0x23, 0x24, 0x25, 0x26, 0x28, 0x29, 0x2A,
        0x2B, 0x2E, 0x32, 0x3A,
    ];
    let mut i = 0;
    while i < ops.len() {
        t[ops[i]] = true;
        i += 1;
    }
    t
};

/// One maximal run of bytes no claim covers.
#[derive(Debug, Clone, Serialize)]
pub struct Residue {
    pub start: usize,
    pub end: usize,
    pub len: usize,
    pub shape: ResidueShape,
    pub entropy_bits: f32,
    pub zero_fraction: f32,
    /// First 16 bytes, hex - enough to recognise a magic without dumping data.
    pub head: String,
}

/// Which walker produced an [`Account`]'s claims.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Walker {
    SceneAssetTable,
    DescriptorBundle,
    PochiFiller,
    RingsideStill,
    Stream,
    MonsterArchive,
    MonsterBlock,
    SummonReadef,
    BattleDataPack,
    MeArchive,
    BseBank,
    InitPak,
    FieldMap,
    SceneV12,
    SceneEventScripts,
    EffectBundle,
    EfectPack,
    TimPack,
    Pack,
    ClipBank,
    OffsetPack,
    Mes,
    CardFontPack,
    Tim,
    Tmd,
    Vab,
    VabMultiBank,
    Seq,
    Anm,
    Man,
    OverlayCode,
    SlotBModule,
    Generic,
}

impl Walker {
    pub fn name(&self) -> &'static str {
        match self {
            Walker::SceneAssetTable => "scene_asset_table",
            Walker::DescriptorBundle => "descriptor_bundle",
            Walker::PochiFiller => "pochi_filler",
            Walker::RingsideStill => "ringside_still",
            Walker::Stream => "stream",
            Walker::MonsterArchive => "monster_archive",
            Walker::MonsterBlock => "monster_block",
            Walker::SummonReadef => "summon_readef",
            Walker::BattleDataPack => "battle_data_pack",
            Walker::MeArchive => "me_archive",
            Walker::BseBank => "bse_bank",
            Walker::InitPak => "init_pak",
            Walker::FieldMap => "field_map",
            Walker::SceneV12 => "scene_v12_table",
            Walker::SceneEventScripts => "scene_event_scripts",
            Walker::EffectBundle => "effect_bundle",
            Walker::EfectPack => "efect_pack",
            Walker::TimPack => "tim_pack",
            Walker::Pack => "pack",
            Walker::ClipBank => "clip_bank",
            Walker::OffsetPack => "offset_pack",
            Walker::Mes => "mes",
            Walker::CardFontPack => "card_font_pack",
            Walker::Tim => "tim",
            Walker::Tmd => "tmd",
            Walker::Vab => "vab",
            Walker::VabMultiBank => "vab_multi_bank",
            Walker::Seq => "seq",
            Walker::Anm => "anm",
            Walker::Man => "man",
            Walker::OverlayCode => "overlay_code",
            Walker::SlotBModule => "slot_b_module",
            Walker::Generic => "generic",
        }
    }
}

/// A decoded payload accounted in its own right.
#[derive(Debug, Clone, Serialize)]
pub struct Nested {
    /// Where the payload came from, e.g. `lzs@0x140000 monster id 11`.
    pub origin: String,
    pub account: Account,
}

/// Per-owner byte + claim totals. `bytes` is merged **within** the owner, so
/// the column never exceeds the buffer even where claims overlap - but the
/// columns can sum past the total, because two owners may cover the same byte
/// (a slot's declared `pad` footprint contains its own `lzs` stream).
#[derive(Debug, Clone, Serialize)]
pub struct OwnerTotal {
    pub owner: String,
    pub claims: usize,
    pub bytes: usize,
}

/// Per-shape residue totals.
#[derive(Debug, Clone, Serialize)]
pub struct ShapeTotal {
    pub shape: String,
    pub runs: usize,
    pub bytes: usize,
}

/// The accounting of one buffer.
#[derive(Debug, Clone, Serialize)]
pub struct Account {
    pub label: String,
    pub size: usize,
    /// [`crate::categorize`] class name, or `nested` for a decoded payload.
    pub class: String,
    pub walker: Walker,
    /// Merged length of every claim, including [`OWNER_SCAN`].
    pub accounted: usize,
    /// Merged length of every claim except [`OWNER_SCAN`].
    pub structural: usize,
    pub accounted_pct: f64,
    pub structural_pct: f64,
    pub residue_bytes: usize,
    pub by_owner: Vec<OwnerTotal>,
    pub by_shape: Vec<ShapeTotal>,
    /// Residue runs at least [`AccountOptions::min_residue`] long, largest
    /// first. Short runs are counted in [`Account::by_shape`] but not listed.
    pub residue: Vec<Residue>,
    pub residue_runs: usize,
    /// Dump extents inside this image's VA span whose bytes neither confirmed
    /// nor refuted the attribution ([`Walker::OverlayCode`] only).
    pub ambiguous_dumps: usize,
    /// Dump extents the bytes placed in an aliased sibling image.
    pub refuted_dumps: usize,
    pub notes: Vec<String>,
    pub nested: Vec<Nested>,
    /// Claims, largest first. Only populated when
    /// [`AccountOptions::keep_claims`] is set - a 15 MB archive produces tens
    /// of thousands.
    pub claims: Vec<Claim>,
}

/// Knobs for [`account`].
#[derive(Debug, Clone)]
pub struct AccountOptions {
    pub label: String,
    /// PROT extraction index, when known. Selects the index-keyed overrides:
    /// the monster archive (which classifies as a generic blob), the card
    /// font pack (which classifies as a truncated stream), and the
    /// overlay-code walker.
    pub prot_index: Option<u32>,
    /// Directory of Ghidra dumps (`ghidra/scripts/funcs`). Required for
    /// [`Walker::OverlayCode`].
    pub funcs_dir: Option<PathBuf>,
    /// Directory of extracted PROT entries (`extracted/PROT`). An overlay
    /// image's **inherited tail** is a comparison against its siblings, so the
    /// cut is only available when the sibling entries can be read; without this
    /// the walker says so in a note rather than counting another module's code
    /// as this one's residue silently. See [`crate::inherited_tail`].
    pub prot_dir: Option<PathBuf>,
    /// Nesting depth budget. `0` accounts the outer buffer only.
    pub depth: u8,
    /// Sweep the residue for TIM / TMD / VAB / SEQ magics and claim the hits
    /// as [`OWNER_SCAN`].
    pub rescan: bool,
    /// Shortest residue run to list individually.
    pub min_residue: usize,
    /// Keep the per-claim list in the [`Account`].
    pub keep_claims: bool,
    /// Cap on nested accounts kept per buffer (they dominate JSON size).
    pub max_nested: usize,
}

impl Default for AccountOptions {
    fn default() -> Self {
        Self {
            label: String::new(),
            prot_index: None,
            funcs_dir: None,
            prot_dir: None,
            depth: 1,
            rescan: true,
            min_residue: 64,
            keep_claims: false,
            max_nested: 8,
        }
    }
}

// ---------------------------------------------------------------------------
// Range algebra
// ---------------------------------------------------------------------------

/// Merge overlapping / touching claims into a sorted, disjoint cover, clamped
/// to `size`. Empty and out-of-range claims are dropped.
pub fn merge_ranges(claims: &[Claim], size: usize) -> Vec<(usize, usize)> {
    let mut v: Vec<(usize, usize)> = claims
        .iter()
        .filter_map(|c| {
            let a = c.start.min(size);
            let b = c.end.min(size);
            (b > a).then_some((a, b))
        })
        .collect();
    v.sort_unstable();
    let mut out: Vec<(usize, usize)> = Vec::with_capacity(v.len());
    for (a, b) in v {
        match out.last_mut() {
            Some(last) if a <= last.1 => last.1 = last.1.max(b),
            _ => out.push((a, b)),
        }
    }
    out
}

/// Complement of a merged cover over `[0, size)`.
pub fn complement(merged: &[(usize, usize)], size: usize) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut cursor = 0usize;
    for &(a, b) in merged {
        if a > cursor {
            out.push((cursor, a));
        }
        cursor = cursor.max(b);
    }
    if cursor < size {
        out.push((cursor, size));
    }
    out
}

// ---------------------------------------------------------------------------
// Residue classification
// ---------------------------------------------------------------------------

/// Shannon entropy of `buf` in bits/byte.
pub fn entropy_bits(buf: &[u8]) -> f32 {
    if buf.is_empty() {
        return 0.0;
    }
    let mut hist = [0u32; 256];
    for &b in buf {
        hist[b as usize] += 1;
    }
    let n = buf.len() as f32;
    -hist
        .iter()
        .filter(|&&c| c > 0)
        .map(|&c| {
            let p = c as f32 / n;
            p * p.log2()
        })
        .sum::<f32>()
}

fn repeats_short_pattern(buf: &[u8]) -> bool {
    for period in [1usize, 2, 4, 8, 16] {
        if buf.len() < period * 2 || !buf.len().is_multiple_of(period) {
            continue;
        }
        if buf.chunks_exact(period).all(|c| c == &buf[..period]) {
            return true;
        }
    }
    false
}

/// Classify one run of unclaimed bytes. See [`ResidueShape`] for the order.
pub fn classify_residue(buf: &[u8]) -> ResidueShape {
    if buf.is_empty() || buf.iter().all(|&b| b == 0) {
        return ResidueShape::ZeroPad;
    }
    if buf.len() < ALIGNMENT_MAX {
        return ResidueShape::Alignment;
    }
    if repeats_short_pattern(buf) {
        return ResidueShape::RepeatedFill;
    }
    let n = buf.len() as f32;
    let printable = buf
        .iter()
        .filter(|&&b| b == 0 || b == b'\n' || b == b'\r' || b == b'\t' || (0x20..0x7F).contains(&b))
        .count() as f32
        / n;
    if printable >= ASCII_MIN {
        return ResidueShape::AsciiText;
    }
    let words = buf.len() / 4;
    if words >= 4 {
        let mut ptrs = 0usize;
        let mut plausible = 0usize;
        let mut special = 0usize;
        let mut seen_ops = 0u64;
        for i in 0..words {
            let w = u32::from_le_bytes(buf[i * 4..i * 4 + 4].try_into().unwrap());
            if (0x8000_0000..0x8020_0000).contains(&w) {
                ptrs += 1;
            }
            let op = (w >> 26) as usize;
            if PLAUSIBLE_OPS[op] {
                plausible += 1;
                seen_ops |= 1u64 << op;
            }
            if op == 0 {
                special += 1;
            }
        }
        let wf = words as f32;
        if ptrs as f32 / wf >= PTR_MIN {
            return ResidueShape::PointerDense;
        }
        if is_bgr555(buf) {
            return ResidueShape::Bgr555;
        }
        // The opcode-plausibility test alone is not enough, and the failure is
        // one-sided: a table of small little-endian values has primary opcode
        // `0` (SPECIAL) in every word, so *any* sparse 16-bit table reads as
        // code. Real code mixes primaries - loads, stores, immediates,
        // branches - so require both a spread and a bound on the SPECIAL share.
        if plausible as f32 / wf >= CODE_PLAUSIBLE_MIN
            && (ptrs as f32 / wf) < CODE_PTR_MAX
            && seen_ops.count_ones() >= CODE_MIN_DISTINCT_OPS
            && (special as f32 / wf) < CODE_SPECIAL_MAX
        {
            return ResidueShape::PlausibleMips;
        }
    }
    let e = entropy_bits(buf);
    if e < LOW_ENTROPY_MAX {
        ResidueShape::LowEntropy
    } else if e >= HIGH_ENTROPY_MIN {
        ResidueShape::HighEntropy
    } else {
        ResidueShape::Mixed
    }
}

/// PSX 15-bit colour data: the STP bit is clear in nearly every halfword, and
/// the run carries a wide, high-entropy spread of values.
///
/// The width test alone would also match a sparse small-value table, so the
/// distinct-value floor and the entropy floor are both load-bearing. Real MIPS
/// never passes it - every `lw` / `sw` / `lui`-pair word puts a halfword at or
/// above `0x8000`.
fn is_bgr555(buf: &[u8]) -> bool {
    let n = buf.len() / 2;
    if n < 64 {
        return false;
    }
    let mut low = 0usize;
    let mut seen = std::collections::HashSet::new();
    for i in 0..n {
        let h = u16::from_le_bytes([buf[i * 2], buf[i * 2 + 1]]);
        if h < 0x8000 {
            low += 1;
        }
        if seen.len() < BGR555_MIN_DISTINCT {
            seen.insert(h);
        }
    }
    low as f32 / n as f32 >= BGR555_MIN
        && seen.len() >= BGR555_MIN_DISTINCT
        && entropy_bits(buf) >= BGR555_MIN_ENTROPY
}

fn hex_head(buf: &[u8], n: usize) -> String {
    buf.iter().take(n).map(|b| format!("{b:02x}")).collect()
}

fn zero_fraction(buf: &[u8]) -> f32 {
    if buf.is_empty() {
        return 0.0;
    }
    buf.iter().filter(|&&b| b == 0).count() as f32 / buf.len() as f32
}

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
const NOT_A_DUMP: [&str; 5] = [
    "citation pointer",
    "cite of",
    "NOFUNC",
    "DATA REGION",
    "DATA WINDOW",
];

fn hex8(tok: &str) -> Option<u32> {
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

fn read_prefix(path: &Path, n: usize) -> std::io::Result<String> {
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

const REG_NAMES: [&str; 32] = [
    "zero", "at", "v0", "v1", "a0", "a1", "a2", "a3", "t0", "t1", "t2", "t3", "t4", "t5", "t6",
    "t7", "s0", "s1", "s2", "s3", "s4", "s5", "s6", "s7", "t8", "t9", "k0", "k1", "gp", "sp", "s8",
    "ra",
];

fn reg(name: &str) -> Option<u32> {
    REG_NAMES.iter().position(|&r| r == name).map(|i| i as u32)
}

fn imm(tok: &str) -> Option<i64> {
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

// ---------------------------------------------------------------------------
// An image's own uninitialised data region
// ---------------------------------------------------------------------------

/// Shortest all-zero run considered as an image's uninitialised data region.
pub const BSS_RUN_MIN: usize = 256;

/// Sites that must form one single address inside a zero run for it to count
/// as addressed, when no second distinct address does.
pub const BSS_MIN_SITES: usize = 4;

/// How many instructions after a `lui` its half may be completed in.
///
/// A MIPS address materialises as `lui rt, hi` plus a second instruction that
/// uses `rt` as its base, and the assembler is free to put anything in
/// between - including the `jal` whose delay slot carries the pair's low half,
/// which is where the STR overlay hands the VLC unpacker its destination
/// (`0x801CF214` / `0x801CF218`). The window is walked forward and abandoned
/// the moment something redefines `rt`, so a stale high half can never be
/// paired with an unrelated low one; a backward-only scan from the second
/// instruction misses the delay-slot form entirely.
const LUI_PAIR_WINDOW: usize = 16;

/// The register a MIPS word writes, or `None` for the forms that write none.
fn defines(w: u32) -> Option<u32> {
    let op = w >> 26;
    let rt = (w >> 16) & 0x1F;
    match op {
        // SPECIAL: `rd`, except the two jump-register forms.
        0x00 => match w & 0x3F {
            0x08 => None,     // jr
            0x09 => Some(31), // jalr (retail always links to ra)
            _ => Some((w >> 11) & 0x1F),
        },
        0x01 | 0x04..=0x07 => None, // branches
        0x02 => None,               // j
        0x03 => Some(31),           // jal
        0x08..=0x0F => Some(rt),    // immediate ALU + lui
        0x20..=0x25 => Some(rt),    // loads
        0x28..=0x2B => None,        // stores
        0x10 | 0x12 => match (w >> 21) & 0x1F {
            0x00 | 0x02 => Some(rt), // mfc0 / mfc2
            _ => None,
        },
        _ => Some(rt),
    }
}

/// Every `(site_va, target_va)` the image's own code forms with a `lui` pair.
///
/// This is the structural half of the uninitialised-data claim below: a zero
/// run is only that image's own declared buffer if the image's own code
/// computes an address inside it. Shape cannot say so - zero fill looks the
/// same whoever wrote it - which is why the test is a pointer-forming
/// instruction and not a byte statistic.
pub fn formed_addresses(image: &[u8], base_va: u32) -> Vec<(u32, u32)> {
    let mut out = Vec::new();
    let word = |off: usize| -> Option<u32> {
        image
            .get(off..off + 4)
            .map(|w| u32::from_le_bytes(w.try_into().unwrap()))
    };
    let mut off = 0usize;
    while off + 4 <= image.len() {
        let Some(w) = word(off) else { break };
        if w >> 26 != 0x0F {
            off += 4;
            continue;
        }
        let rt = (w >> 16) & 0x1F;
        let hi = (w & 0xFFFF) << 16;
        for k in 1..=LUI_PAIR_WINDOW {
            let at = off + 4 * k;
            let Some(v) = word(at) else { break };
            let op = v >> 26;
            let rs = (v >> 21) & 0x1F;
            let low = v & 0xFFFF;
            if rs == rt {
                let target = match op {
                    // ori: the low half is zero-extended.
                    0x0D => Some(hi | low),
                    // addiu and every load/store form: sign-extended.
                    0x09 | 0x20..=0x25 | 0x28..=0x2B => Some(hi.wrapping_add(low as i16 as u32)),
                    _ => None,
                };
                if let Some(t) = target {
                    out.push((base_va + at as u32, t));
                }
            }
            if defines(v) == Some(rt) {
                break;
            }
        }
        off += 4;
    }
    out
}

/// Maximal all-zero runs of at least `min` bytes, as `(start, end)`.
pub fn zero_runs(buf: &[u8], min: usize) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < buf.len() {
        if buf[i] != 0 {
            i += 1;
            continue;
        }
        let start = i;
        while i < buf.len() && buf[i] == 0 {
            i += 1;
        }
        if i - start >= min {
            out.push((start, i));
        }
    }
    out
}

/// Claim the zero runs an overlay image's own code addresses.
///
/// An overlay is streamed by a **fixed-length** transfer: `FUN_8003EBE4` asks
/// `FUN_8003E8A8` for the entry's sector count - `toc[i+3] - toc[i+2]`, the
/// gap to the next entry - and hands it straight to `FUN_8003E800`. So the
/// whole extent reaches RAM whatever is in it, and a linked image's
/// uninitialised data region travels with its code as zero fill. Those bytes
/// are not a format nobody has walked; they are the buffers the image's own
/// code writes at runtime, and the disc's largest single unclaimed run (PROT
/// `0970`, 131172 bytes) is one.
///
/// Two rules keep this from being a way to buy percentage points, and both are
/// asserted against the raw file rather than against the parser:
///
/// * the claim is exactly one maximal **all-zero** run - it can never grow
///   into live content, and a run interrupted by a single non-zero byte is two
///   runs;
/// * the image's own code must address the run, and once is not enough. A
///   single `lui` pair landing somewhere in a multi-kilobyte window is a
///   coincidence an image with thousands of pairs will produce; two distinct
///   addresses, or one formed at [`BSS_MIN_SITES`] separate sites, is a
///   structure. A zero region below that bar stays residue, which is what keeps
///   a donor's zero tail - and a zero hole inside a sparse data segment - out of
///   the figure. Entry `0970`'s post-blob slack and its 256-byte data-segment
///   hole are refused for having no site at all; the menu overlay's largest
///   data-segment hole is refused on the bar.
///
/// The claim's `detail` reports both counts, so the reader can weigh a run
/// addressed twice against one addressed hundreds of times.
fn claim_uninitialised_data(buf: &[u8], sink: &mut Sink, base_va: u32) {
    let formed = formed_addresses(buf, base_va);
    let mut claimed = 0usize;
    let mut runs = 0usize;
    for (start, end) in zero_runs(buf, BSS_RUN_MIN) {
        let lo = base_va.wrapping_add(start as u32);
        let hi = base_va.wrapping_add(end as u32);
        let sites = formed.iter().filter(|(_, t)| *t >= lo && *t < hi).count();
        let distinct = formed
            .iter()
            .filter(|(_, t)| *t >= lo && *t < hi)
            .map(|(_, t)| *t)
            .collect::<std::collections::BTreeSet<_>>()
            .len();
        if distinct < 2 && sites < BSS_MIN_SITES {
            continue;
        }
        sink.claim(
            start,
            end,
            OWNER_PAD,
            format!(
                "uninitialised data region {lo:#010x}..{hi:#010x}, \
                 {distinct} address(es) formed inside it at {sites} site(s)"
            ),
        );
        claimed += end - start;
        runs += 1;
    }
    if runs > 0 {
        sink.note(format!(
            "{runs} uninitialised data region(s), {claimed} bytes: \
             zero fill the loader transfers because the read length is the \
             entry's own sector extent, addressed by this image's own code"
        ));
    }
}

// ---------------------------------------------------------------------------
// Walkers
// ---------------------------------------------------------------------------

/// Accumulator every walker writes into.
struct Sink {
    claims: Vec<Claim>,
    nested: Vec<Nested>,
    notes: Vec<String>,
    ambiguous_dumps: usize,
    refuted_dumps: usize,
}

impl Sink {
    fn new() -> Self {
        Self {
            claims: Vec::new(),
            nested: Vec::new(),
            notes: Vec::new(),
            ambiguous_dumps: 0,
            refuted_dumps: 0,
        }
    }
    fn claim(&mut self, start: usize, end: usize, owner: &'static str, detail: impl Into<String>) {
        if end > start {
            self.claims.push(Claim::new(start, end, owner, detail));
        }
    }
    fn note(&mut self, s: impl Into<String>) {
        self.notes.push(s.into());
    }
    fn nest(
        &mut self,
        opts: &AccountOptions,
        depth: u8,
        origin: impl Into<String>,
        bytes: &[u8],
        walker: Walker,
    ) {
        if depth == 0 || bytes.is_empty() {
            return;
        }
        let origin = origin.into();
        let acc = run(
            bytes,
            origin.clone(),
            "nested".into(),
            walker,
            opts,
            depth - 1,
        );
        if self.nested.len() < opts.max_nested {
            self.nested.push(Nested {
                origin,
                account: acc,
            });
        }
    }
}

/// LZS-decode at `off`, claiming the compressed span. Returns the payload.
fn take_lzs(
    sink: &mut Sink,
    buf: &[u8],
    off: usize,
    dec_size: usize,
    detail: &str,
) -> Option<Vec<u8>> {
    let src = buf.get(off..)?;
    match legaia_lzs::decompress_tracked(src, dec_size) {
        Ok((out, consumed)) => {
            sink.claim(off, off + consumed, OWNER_LZS, detail.to_string());
            Some(out)
        }
        Err(_) => None,
    }
}

/// Pick the walker for a bundle section from its type byte **and** its bytes.
///
/// The type byte says what kind of asset the section holds, not whether it
/// holds one or a [pack](crate::pack) of them, and the retail bundles use both:
/// a kingdom bundle's `TIM_LIST` section is a pack of atlases, a town bundle's
/// `TMD` section is one mesh. A pack read as a single asset leaves its offset
/// table and its inter-member slack in the residue, so the pack test runs
/// first for the two types that carry one.
fn walker_for_section(type_byte: u8, payload: &[u8]) -> Walker {
    if matches!(
        AssetType::from_byte(type_byte),
        AssetType::Tim | AssetType::TimList | AssetType::Tmd | AssetType::Tmd2
    ) && walker_for_payload(payload) == Walker::Pack
    {
        return Walker::Pack;
    }
    walker_for_type(type_byte)
}

/// Pick the walker for a decoded payload from its asset type byte.
fn walker_for_type(type_byte: u8) -> Walker {
    match AssetType::from_byte(type_byte) {
        AssetType::Tim | AssetType::TimList => Walker::Tim,
        AssetType::Tmd | AssetType::Tmd2 => Walker::Tmd,
        AssetType::Man => Walker::Man,
        // Type `0x05` is labelled MOVE by the dispatcher table but carries an
        // ANM clip bank, not a Tactical-Arts move table
        // (`docs/formats/world-map-overlay.md`, `legaia_asset::player_anm`).
        AssetType::Move | AssetType::Move2 => Walker::ClipBank,
        AssetType::Anm => Walker::Anm,
        AssetType::Mes => Walker::Mes,
        // VDF sections are the same `[u32 count][u32 byte_offset[count]]`
        // container as the clip bank, without the `0x080C` record header.
        AssetType::Vdf => Walker::OffsetPack,
        _ => Walker::Generic,
    }
}

// --- scene bundles ---------------------------------------------------------

fn walk_scene_asset_table(buf: &[u8], sink: &mut Sink, opts: &AccountOptions, depth: u8) {
    let Some(r) = crate::scene_asset_table::resolve(buf) else {
        sink.note("scene_asset_table::resolve returned None");
        return;
    };
    let base = r.table_base;
    if base > 0 {
        // The prescript that precedes the sector-aligned table.
        if let Some(ranges) = crate::scene_scripted_asset_table::record_ranges(buf) {
            sink.claim(0, 4, OWNER_HEADER, "prescript count + first offset");
            for (i, (a, b)) in ranges.iter().enumerate() {
                sink.claim(*a, *b, OWNER_SCRIPT, format!("prescript record {i}"));
            }
        }
    }
    sink.claim(base, base + 8, OWNER_HEADER, "count + meta1");
    let count = r.table.count;
    sink.claim(
        base + 8,
        base + 8 + count * 8,
        OWNER_TOC,
        format!("{count} descriptors"),
    );
    for (i, d) in r.table.used().iter().enumerate() {
        let start = base + d.data_offset as usize;
        let detail = format!(
            "slot {i} type {:#04x} ({}) decoded {} B",
            d.type_byte,
            AssetType::from_byte(d.type_byte).name(),
            d.size
        );
        if let Some(out) = take_lzs(sink, buf, start, d.size as usize, &detail) {
            sink.nest(
                opts,
                depth,
                format!("slot {i} {}", AssetType::from_byte(d.type_byte).name()),
                &out,
                walker_for_section(d.type_byte, &out),
            );
        } else {
            sink.note(format!("{detail}: LZS decode failed"));
        }
    }
}

/// The same bundle, walked the way `FUN_80020224` walks it.
///
/// [`walk_scene_asset_table`] goes through the *detector*, whose count
/// allow-list is `4..=7` plus a MAN requirement below 6 - a classifier
/// heuristic, not a runtime rule. Retail reads the count word and loops
/// (`lw s3,0x0(s4)` / `blez s3`), so the count-1 and count-3 bundles this disc
/// also ships walk identically at runtime and had no walker here at all. Their
/// class is [`Class::LzsContainer`], whose own descriptor count is *fitted*
/// from a fixed list `{1,2,3,4,8,16}` that cannot even express the two count-5
/// entries - so the class figure was never the header's own count.
///
/// The claims are the same three kinds the scene-bundle walker makes, and the
/// payload extents are **measured** rather than inferred: a descriptor states
/// only the decompressed size, so the compressed span's end comes from what
/// `legaia_lzs::decompress_tracked` consumed.
fn walk_descriptor_bundle(buf: &[u8], sink: &mut Sink, opts: &AccountOptions, depth: u8) {
    let Some(descriptors) = crate::scene_asset_table::descriptor_bundle_walk(buf) else {
        walk_lzs_container_orphan(buf, sink, opts, depth);
        return;
    };
    let count = descriptors.len();
    sink.claim(0, 8, OWNER_HEADER, "count + decompressed-size total");
    sink.claim(8, 8 + count * 8, OWNER_TOC, format!("{count} descriptors"));
    for (i, d) in descriptors.iter().enumerate() {
        let start = d.data_offset as usize;
        let ty = AssetType::from_byte(d.type_byte);
        let detail = format!(
            "slot {i} type {:#04x} ({}) decoded {} B",
            d.type_byte,
            ty.name(),
            d.size
        );
        let Some(out) = take_lzs(sink, buf, start, d.size as usize, &detail) else {
            sink.note(format!("{detail}: LZS decode failed"));
            continue;
        };
        // A `FLAG` slot is one the dispatcher answers with `type << 8` without
        // reading a byte (`docs/formats/asset-type.md`), and on this disc every
        // one of them decodes to the pochi fill file - the authoring tool wrote
        // its filler into the reserved descriptor. Account it as the filler it
        // is rather than sending 1927 bytes of ASCII to the generic walker.
        let walker = if crate::categorize::is_pochi_filler(&out) {
            Walker::PochiFiller
        } else {
            walker_for_section(d.type_byte, &out)
        };
        sink.nest(opts, depth, format!("slot {i} {}", ty.name()), &out, walker);
    }
}

/// Does this buffer open with an [offset pack](walk_offset_pack)?
///
/// The discriminating word is the **first offset**, not the count: a table of
/// `count` byte offsets puts member 0 immediately after itself, at
/// `4 + 4 * count`. A word-offset [`crate::pack`] would put it four times
/// further on, and an arbitrary pair of small integers almost never lands on
/// the identity exactly. That equality is the whole test, and it is why the
/// predicate can be used as a fallback without guessing.
fn has_offset_pack_anchor(buf: &[u8]) -> bool {
    let Some(count) = legaia_bytes::u32_le(buf, 0).map(|c| c as usize) else {
        return false;
    };
    if count == 0 || 4 + count * 4 > buf.len() {
        return false;
    }
    legaia_bytes::u32_le(buf, 4).is_some_and(|off| off as usize == 4 + count * 4)
}

/// Three `lzs_container` entries are not descriptor bundles at all, because
/// that class never reads the header's count word - it *fits* a descriptor
/// count out of a fixed list, so any buffer whose first words happen to pass
/// the per-descriptor checks joins the class. This is where they land.
///
/// Two of the three are offset packs, one bare and one behind a DATA_FIELD
/// chunk header, and both are recovered from the anchor rather than from the
/// class ([`has_offset_pack_anchor`]). The third is a code image with a
/// leading string pool, which has no structural walker here and stays residue.
fn walk_lzs_container_orphan(buf: &[u8], sink: &mut Sink, opts: &AccountOptions, depth: u8) {
    if has_offset_pack_anchor(buf) {
        sink.note("not a descriptor bundle - a bare offset pack");
        let members = walk_offset_pack(buf, sink, OWNER_RECORD, "member");
        nest_pack_members(buf, sink, opts, depth, &members);
        return;
    }
    // `[u32 (type << 24) | payload_len]` then the pack, the same wrapper
    // `prot::timpack` reads past for a `TIM_LIST` chunk
    // (`docs/formats/tim-pack.md`). The header's own length word has to agree
    // with the entry for the offset to mean anything.
    let header = legaia_bytes::u32_le(buf, 0).unwrap_or(0);
    let payload_len = (header & 0x00FF_FFFF) as usize;
    if payload_len >= 8
        && 4 + payload_len <= buf.len()
        && buf.len() >= 4
        && has_offset_pack_anchor(&buf[4..])
    {
        let ty = (header >> 24) as u8;
        sink.claim(
            0,
            4,
            OWNER_HEADER,
            format!(
                "chunk header type {:#04x} ({}), payload {payload_len} B",
                ty,
                AssetType::from_byte(ty).name()
            ),
        );
        let mut inner = Sink::new();
        let members = walk_offset_pack(&buf[4..], &mut inner, OWNER_RECORD, "member");
        for c in inner.claims {
            sink.claim(c.start + 4, c.end + 4, c.owner, c.detail);
        }
        for n in inner.notes {
            sink.note(n);
        }
        let shifted: Vec<_> = members.iter().map(|r| r.start + 4..r.end + 4).collect();
        nest_pack_members(buf, sink, opts, depth, &shifted);
        return;
    }
    sink.note("not a descriptor bundle and not an offset pack");
}

/// Account each member of a pack in its own right, picking the walker from the
/// member's own bytes.
fn nest_pack_members(
    buf: &[u8],
    sink: &mut Sink,
    opts: &AccountOptions,
    depth: u8,
    members: &[std::ops::Range<usize>],
) {
    for (i, r) in members.iter().enumerate() {
        let Some(bytes) = buf.get(r.clone()) else {
            continue;
        };
        let walker = walker_for_payload(bytes);
        if walker != Walker::Generic {
            sink.nest(opts, depth, format!("member {i}"), bytes, walker);
        }
    }
}

/// A pochi filler slot: the fill file, then the mastering buffer's leftovers.
///
/// Both claims are `pad` - a filler slot carries no content by construction -
/// but they are separate claims because they are two different things, and the
/// residue classifier would otherwise rank 266 sectors of dev fill as work: the
/// fill is text-shaped, and `repeated_fill` only tests periods 1/2/4/8/16 while
/// the pochi line is 52 bytes long.
///
/// See [`docs/formats/pochi.md`](../../../docs/formats/pochi.md) for what pins
/// the tail: it is byte-identical to some other entry's bytes at the same file
/// offset, in all 266 slots.
fn walk_pochi_filler(buf: &[u8], sink: &mut Sink) {
    let fill_end = crate::categorize::POCHI_FILL_LEN.min(buf.len());
    sink.claim(
        0,
        fill_end,
        OWNER_PAD,
        "pochi fill file, through the EOF byte",
    );
    if buf.len() > fill_end {
        sink.claim(
            fill_end,
            buf.len(),
            OWNER_PAD,
            "sector tail - the mastering buffer's prior contents, not fill",
        );
    }
}

/// One of the two headerless 16bpp stills, claimed as the four bands the
/// consumer uploads ([`crate::ringside_still`]).
///
/// Claiming four bands rather than one buffer is the point: the band size is
/// what the seek stride and the `LoadImage` rectangle independently agree on,
/// so a still that is the wrong length leaves the shortfall in the residue
/// instead of being absorbed by a whole-file claim.
fn walk_ringside_still(buf: &[u8], sink: &mut Sink) {
    use crate::ringside_still as still;
    if !still::has_still_shape(buf) {
        sink.note(format!(
            "not {} bytes - the four {}-byte band uploads do not tile this buffer",
            still::ENTRY_BYTES,
            still::BAND_BYTES
        ));
        return;
    }
    for i in 0..still::BAND_COUNT {
        let span = still::band_span(i).expect("band index inside BAND_COUNT");
        let (x, y, w, h) = still::band_rect(i).expect("band index inside BAND_COUNT");
        sink.claim(
            span.start,
            span.end,
            OWNER_TEXTURE,
            format!("band {i} -> LoadImage rect ({x},{y}) {w}x{h}"),
        );
    }
}

// --- DATA_FIELD / streaming variants --------------------------------------

fn walk_stream(buf: &[u8], sink: &mut Sink, opts: &AccountOptions, depth: u8) {
    // `[u32 size][bare TMD][chunks]` variant first - its leading chunk has no
    // typed header the generic walker would recognise.
    if let Some(s) = crate::scene_tmd_stream::detect(buf) {
        sink.claim(0, 4, OWNER_HEADER, "chunk0 header (bare TMD size)");
        sink.claim(
            4,
            4 + s.tmd_size,
            OWNER_TMD,
            format!("leading TMD, {} objects", s.tmd_nobj),
        );
        for (i, c) in s.tail_chunks.iter().enumerate() {
            let end = c.offset + 4 + ((c.size as usize) & !3);
            sink.claim(c.offset, c.offset + 4, OWNER_HEADER, format!("chunk {i}"));
            sink.claim(
                c.offset + 4,
                end,
                payload_owner(c.asset_type),
                format!("chunk {i} {}", c.asset_type.name()),
            );
            if let Some(p) = buf.get(c.offset + 4..end) {
                sink.nest(
                    opts,
                    depth,
                    format!("chunk {i} {}", c.asset_type.name()),
                    p,
                    walker_for_payload(p),
                );
            }
        }
        if s.tail_terminated {
            sink.claim(s.tail_end - 4, s.tail_end, OWNER_HEADER, "terminator");
            claim_last_sector_slack(buf, sink, s.tail_end, "slack past the terminator");
        }
        return;
    }
    let Ok(rep) = parse_streaming(buf, 8192) else {
        sink.note("parse_streaming failed");
        return;
    };
    if rep.chunks.is_empty() {
        sink.note("no streaming chunks parsed");
        return;
    }
    for (i, c) in rep.chunks.iter().enumerate() {
        let start = c.header_offset;
        let end = start + 4 + ((c.size as usize) & !3);
        sink.claim(start, start + 4, OWNER_HEADER, format!("chunk {i} header"));
        let t = AssetType::from_byte(c.type_byte);
        let owner = buf
            .get(start + 4..end.min(buf.len()))
            .map_or_else(|| payload_owner(t), |p| payload_owner_of(t, p));
        sink.claim(
            start + 4,
            end,
            owner,
            format!("chunk {i} {} ({} B)", c.type_name, c.size),
        );
        if let Some(p) = buf.get(start + 4..end.min(buf.len())) {
            sink.nest(
                opts,
                depth,
                format!("chunk {i} {}", c.type_name),
                p,
                walker_for_payload(p),
            );
        }
    }
    if rep.terminated {
        sink.claim(
            rep.bytes_consumed - 4,
            rep.bytes_consumed,
            OWNER_HEADER,
            "terminator",
        );
        claim_last_sector_slack(buf, sink, rep.bytes_consumed, "slack past the terminator");
    } else {
        sink.note(format!(
            "stream unterminated after {} chunks ({} B consumed)",
            rep.chunks.len(),
            rep.bytes_consumed
        ));
    }
}

fn payload_owner(t: AssetType) -> &'static str {
    match t {
        AssetType::Tim | AssetType::TimList => OWNER_TIM,
        AssetType::Tmd | AssetType::Tmd2 => OWNER_TMD,
        AssetType::Anm => OWNER_ANM,
        AssetType::Man => OWNER_SCRIPT,
        _ => OWNER_RECORD,
    }
}

/// The owner for a chunk payload, preferring the payload's **own** magic over
/// the chunk header's type byte.
///
/// The two disagree on this disc: the standalone BGM streams carry their SEQ
/// behind a type-`0x02` header, which [`payload_owner`] would read as a TMD
/// and label `tmd`. The type byte selects the runtime's handler; the magic
/// says what the bytes are, and an owner names what the bytes are.
fn payload_owner_of(t: AssetType, payload: &[u8]) -> &'static str {
    match legaia_bytes::u32_le(payload, 0) {
        Some(0x0000_0010) => OWNER_TIM,
        Some(0x8000_0002) => OWNER_TMD,
        Some(0x5641_4270) => OWNER_VAB,
        _ if payload.starts_with(b"pQES") => OWNER_SEQ,
        _ => payload_owner(t),
    }
}

/// Pick a walker for a payload from its own magic.
fn walker_for_payload(buf: &[u8]) -> Walker {
    match legaia_bytes::u32_le(buf, 0) {
        Some(0x0000_0010) => Walker::Tim,
        Some(0x8000_0002) => Walker::Tmd,
        Some(0x5641_4270) => Walker::Vab,
        _ if buf.starts_with(b"pQES") => Walker::Seq,
        // A `TIM_LIST` / `TMD` chunk's payload is often a pack rather than a
        // single asset, and a pack's head word is a count with no magic - so
        // without this the payload fell to `Generic` and its members were
        // found only by the magic sweep, i.e. `accounted` near 100 % with
        // `structural` at 0. The pack anchor is checked before claiming it.
        _ if crate::pack::parse_pack(buf)
            .is_ok_and(|e| e.first().is_some_and(|f| f.byte_offset == 4 + 4 * e.len())) =>
        {
            Walker::Pack
        }
        _ => Walker::Generic,
    }
}

// --- fixed-stride streaming slots -----------------------------------------

/// Claim the trailing fill of one fixed-stride streaming slot as [`OWNER_PAD`].
///
/// Three archives on this disc are a flat array of fixed-size slots that the
/// runtime transfers **whole**, content length or not:
///
/// - the monster archive (`0867`), `0x14000` per slot: the battle loader
///   `FUN_800542C8` seeks `(id-1) * 0x14000` bytes (`sll v0,v1,0x2; addu
///   v0,v0,v1; sll v0,v0,0xe` at `0x80054524`) and reads `0x28` sectors
///   (`li a1,0x28` at `0x80054608`, `jal 0x8003E800`), then hands the LZS
///   decoder `slot + 4` - so the decoder stops at its own terminator and the
///   rest of the transferred window is never interpreted.
/// - `summon.dat` / `readef.DAT` (`0893` / `0894`), `0x10800` per slot: the
///   streaming SM `FUN_801F17F8` seeks `slot * 33 * 0x800` (`sll a1,v0,0x5;
///   addu a1,a1,v0; sll a1,a1,0xb` at `0x801F1948`) and reads `0x10800` bytes
///   (`lui a2,0x1; ori a2,a2,0x800` at `0x801F1958`/`0x801F1970` ->
///   `FUN_800559EC`, which divides by `0x800` for the sector count).
///
/// Every one of the three file extents is an exact multiple of its stride, so
/// the slot boundary is a declared bound rather than an inferred one, and the
/// bytes between a slot's content and that bound are the [`OWNER_PAD`]
/// definition verbatim - the same reading the multi-bank VAB's sector slack
/// gets.
///
/// The claim starts where the fill starts, not where the walker stopped: only
/// the slot's maximal all-zero **suffix** is claimed. That is the guard rail.
/// Claiming the whole gap unconditionally would absorb a walker that stopped
/// early inside real content, and the instrument would gain percentage points
/// by redefining itself instead of by reading the disc.
fn claim_slot_fill(buf: &[u8], sink: &mut Sink, start: usize, end: usize, detail: String) {
    let Some(slot) = buf.get(start..end.min(buf.len())) else {
        return;
    };
    let mut fill = slot.len();
    while fill > 0 && slot[fill - 1] == 0 {
        fill -= 1;
    }
    // An entirely-zero slot is not a tail; leave it visible as residue.
    if fill == 0 || fill == slot.len() {
        return;
    }
    sink.claim(start + fill, start + slot.len(), OWNER_PAD, detail);
}

/// Shortest internal all-zero run that gets its own residue entry.
const FILL_SPLIT_BYTES: usize = 2048;

/// Cut a residue run wherever a sector or more of fill sits inside it.
///
/// A residue run's boundaries are drawn by the *claims* around it, so a region
/// that is one kilobyte of content followed by a hundred kilobytes of fill
/// arrives as a single run - and the shape vocabulary then has to name the
/// whole thing with one word. It picks `ascii_text`, because that test counts
/// NUL as printable and one non-zero byte disqualifies `zero_pad`, so the fill
/// lands in `work_bytes` as if it were an unwalked string pool. Entry `0970`'s
/// 131172-byte hole did exactly that.
///
/// Splitting is not reclassifying: each piece still gets whatever shape its own
/// bytes earn, and the total residue is unchanged. It only stops one run from
/// being two findings glued together. The bound is a sector, so inter-record
/// zeros stay attached to the run they belong to.
fn split_off_fill(buf: &[u8], gaps: Vec<(usize, usize)>) -> Vec<(usize, usize)> {
    let mut out = Vec::with_capacity(gaps.len());
    for (a, b) in gaps {
        let Some(s) = buf.get(a..b) else {
            out.push((a, b));
            continue;
        };
        let mut cut = a;
        let mut i = 0usize;
        while i < s.len() {
            if s[i] != 0 {
                i += 1;
                continue;
            }
            let mut j = i;
            while j < s.len() && s[j] == 0 {
                j += 1;
            }
            if j - i >= FILL_SPLIT_BYTES {
                if a + i > cut {
                    out.push((cut, a + i));
                }
                out.push((a + i, a + j));
                cut = a + j;
            }
            i = j;
        }
        if b > cut {
            out.push((cut, b));
        }
    }
    out
}

/// Claim what is left of a PROT entry's **last sector** past a declared end.
///
/// A PROT entry's extent is sector-granular ([`prot.md`](https://andrewaltimit.github.io/legend-of-legaia-re/formats/prot.html):
/// `toc[p+3] - toc[p+2]`) while the container inside it declares its own end -
/// a stream terminator, a length word. What lies between the two is the
/// builder's sector buffer, and nothing addresses it: the reader stops at the
/// declared end and the next entry starts at the next sector.
///
/// The bound is deliberately one sector. A remainder of a sector or more is a
/// second region, not slack, and stays residue so it keeps ranking as work.
fn claim_last_sector_slack(buf: &[u8], sink: &mut Sink, end: usize, detail: &'static str) {
    const SECTOR: usize = 2048;
    if end < buf.len() && buf.len() - end < SECTOR {
        sink.claim(end, buf.len(), OWNER_PAD, detail);
    }
}

// --- monster archive (PROT 0867) ------------------------------------------

fn walk_monster_archive(buf: &[u8], sink: &mut Sink, opts: &AccountOptions, depth: u8) {
    use crate::monster_archive::SLOT_STRIDE;
    let slots = buf.len() / SLOT_STRIDE;
    let mut populated = 0usize;
    let mut raw_tims = 0usize;
    let mut fill_slots = 0usize;
    for id in 1..=slots {
        let slot = (id - 1) * SLOT_STRIDE;
        // The loader transfers the whole 0x14000-byte slot whatever the block
        // costs; everything past the LZS stream's own terminator is declared
        // slack. See `claim_slot_fill`.
        let before = sink.claims.len();
        claim_slot_fill(
            buf,
            sink,
            slot,
            slot + SLOT_STRIDE,
            format!("slot {id} fill past the block"),
        );
        fill_slots += usize::from(sink.claims.len() > before);
        let Some(dec_size) = legaia_bytes::u32_le(buf, slot).map(|v| v as usize) else {
            continue;
        };
        // The archive's trailing slots do not hold a `[u32 dec_size][LZS]`
        // monster block at all: their first word is the PSX TIM magic and the
        // slot head is a raw TIM, zero-padded to the stride. Claiming the head
        // word as a `dec_size` there would be reading the magic as a length.
        if dec_size == 0x10 {
            if let Some(hit) = buf
                .get(slot..slot + SLOT_STRIDE)
                .map(crate::tim_scan::scan_buffer)
                .and_then(|hits| hits.into_iter().find(|h| h.offset == 0))
            {
                sink.claim(
                    slot,
                    slot + hit.byte_len,
                    OWNER_TIM,
                    format!(
                        "slot {id} raw TIM {}x{} {}bpp",
                        hit.width, hit.height, hit.bpp
                    ),
                );
                raw_tims += 1;
            }
            continue;
        }
        if !(0x4C..=SLOT_STRIDE * 8).contains(&dec_size) {
            continue;
        }
        sink.claim(slot, slot + 4, OWNER_HEADER, format!("slot {id} dec_size"));
        let detail = format!("monster id {id} block ({dec_size} B)");
        let Some(block) = take_lzs(sink, buf, slot + 4, dec_size, &detail) else {
            sink.note(format!("monster id {id}: LZS decode failed"));
            continue;
        };
        populated += 1;
        sink.nest(
            opts,
            depth,
            format!("lzs@{:#x} monster id {id}", slot + 4),
            &block,
            Walker::MonsterBlock,
        );
    }
    sink.note(format!(
        "{populated} of {slots} {SLOT_STRIDE:#x}-byte slots carry a decodable block; \
         {raw_tims} carry a raw TIM at the slot head instead; \
         {fill_slots} end in fill the loader transfers and never reads"
    ));
}

/// Offset of the per-action animation stream inside a spell/action entry
/// (`docs/formats/monster-animation.md`).
const MONSTER_ANIM_STREAM_OFFSET: usize = 0x8C;
/// Bytes per part record in the packed stream (six 12-bit fields).
const MONSTER_ANIM_PART_STRIDE: usize = 9;

fn walk_monster_block(block: &[u8], sink: &mut Sink) {
    sink.claim(0, 0x4C, OWNER_RECORD, "stat record head");
    if let Some(name_off) = legaia_bytes::u32_le(block, 0).map(|v| v as usize)
        && let Some(rest) = block.get(name_off..)
    {
        let n = rest.iter().take_while(|&&b| b != 0).count();
        if n > 0 && n < 64 {
            sink.claim(name_off, name_off + n + 1, OWNER_STRING, "monster name");
        }
    }
    // TMD + texture pool: record `+0x04` / `+0x08`.
    let tmd_off = legaia_bytes::u32_le(block, 0x04).unwrap_or(0) as usize;
    if legaia_bytes::u32_le(block, tmd_off) == Some(0x8000_0002) {
        let len = crate::tmd_scan::scan_buffer(block)
            .into_iter()
            .find(|h| h.offset == tmd_off)
            .map(|h| h.byte_len);
        match len {
            Some(n) => sink.claim(tmd_off, tmd_off + n, OWNER_TMD, "monster mesh"),
            None => sink.note("TMD magic at +0x04 but tmd_scan did not size it"),
        }
    }
    let pool = legaia_bytes::u32_le(block, 0x08).unwrap_or(0) as usize;
    if pool > 0 && pool < block.len() {
        // `MonsterMesh::texture` reads `pool = &block[pool_off..]` whole: the
        // CLUT region then a 4bpp page filling the rest of the block.
        let clut_end = (pool + crate::monster_archive::CLUT_REGION_BYTES).min(block.len());
        sink.claim(pool, clut_end, OWNER_CLUT, "15 x 16-colour palettes");
        sink.claim(clut_end, block.len(), OWNER_TEXTURE, "4bpp page, 256 rows");
    }
    // Spell / action entries: `+0x4A` count, `+0x4C` offsets.
    let count = block.get(0x4A).copied().unwrap_or(0) as usize;
    if count == 0 || count > 64 {
        return;
    }
    sink.claim(
        0x4C,
        0x4C + count * 4,
        OWNER_TOC,
        format!("{count} spell offsets"),
    );
    let eff_base = (count + 0x13) * 4;
    let mut eff_max = 0usize;
    for i in 0..count {
        let Some(off) = legaia_bytes::u32_le(block, 0x4C + i * 4).map(|v| v as usize) else {
            continue;
        };
        if off == 0 || off >= block.len() {
            continue;
        }
        let head_end = (off + MONSTER_ANIM_STREAM_OFFSET).min(block.len());
        sink.claim(off, head_end, OWNER_RECORD, format!("action entry {i}"));
        for f in [0x04usize, 0x08] {
            let idx = legaia_bytes::u32_le(block, off + f).unwrap_or(0) as usize;
            eff_max = eff_max.max(idx);
        }
        // `[u8 part_count][u8 frame_count][frames * parts * 9]`
        let (parts, frames) = (
            block.get(head_end).copied().unwrap_or(0) as usize,
            block.get(head_end + 1).copied().unwrap_or(0) as usize,
        );
        if parts > 0 && frames > 0 {
            let end = head_end + 2 + frames * parts * MONSTER_ANIM_PART_STRIDE;
            if end <= block.len() {
                sink.claim(
                    head_end,
                    end,
                    OWNER_ANM,
                    format!("action {i} keyframes ({parts}p x {frames}f)"),
                );
            }
        }
    }
    if eff_max > 0 {
        sink.claim(
            eff_base,
            eff_base + eff_max * 4,
            OWNER_TOC,
            format!("effect-offset table ({eff_max} words)"),
        );
    }
}

// --- summon.dat / readef.DAT (PROT 0893 / 0894) ---------------------------

fn walk_summon_readef(buf: &[u8], sink: &mut Sink, opts: &AccountOptions, depth: u8) {
    use crate::summon_readef::{SLOT_BYTES, SlotKind};
    let Ok(f) = crate::summon_readef::parse(buf) else {
        sink.note("summon_readef::parse failed");
        return;
    };
    let (mut tex, mut actor, mut me, mut raw) = (0, 0, 0, 0);
    let mut fill_slots = 0usize;
    for s in &f.slots {
        let base = s.index * SLOT_BYTES;
        // Both files stream in whole `0x10800` slots whatever the slot holds;
        // the tail fill is declared slack. See `claim_slot_fill`.
        let before = sink.claims.len();
        claim_slot_fill(
            buf,
            sink,
            base,
            base + SLOT_BYTES,
            format!("slot {} fill past the content", s.index),
        );
        fill_slots += usize::from(sink.claims.len() > before);
        match &s.kind {
            SlotKind::Texture(t) => {
                tex += 1;
                sink.claim(base, base + 4, OWNER_HEADER, "texture-slot mode");
                let clut_end = base + 4 + t.clut_bytes();
                sink.claim(
                    base + 4,
                    clut_end,
                    OWNER_CLUT,
                    format!("{} CLUT row(s)", t.clut_rows),
                );
                let tex_start = base + t.texture_offset;
                sink.claim(
                    tex_start,
                    tex_start + t.texture_bytes(),
                    OWNER_TEXTURE,
                    format!("{}-halfword page", t.texture_width_halfwords),
                );
            }
            SlotKind::ActorRecord(a) => {
                actor += 1;
                sink.claim(base, base + 0x4C, OWNER_RECORD, "actor record head");
                sink.claim(
                    base + 0x4C,
                    base + 0x4C + a.part_offsets.len() * 4,
                    OWNER_TOC,
                    format!("{} part offsets", a.part_count),
                );
                if let Some(n) = a.name.as_ref().map(|n| n.len()) {
                    sink.claim(
                        base + a.name_offset,
                        base + a.name_offset + n + 1,
                        OWNER_STRING,
                        "attack name",
                    );
                }
                // Each part offset names a sub-mesh; they tile the region
                // between the part table and the texture pool.
                let mut parts: Vec<usize> = a.part_offsets.iter().map(|&o| o as usize).collect();
                parts.sort_unstable();
                parts.dedup();
                for (n, off) in parts.iter().enumerate() {
                    let end = parts
                        .get(n + 1)
                        .copied()
                        .unwrap_or(a.texture_pool_offset)
                        .max(*off);
                    sink.claim(base + off, base + end, OWNER_TMD, format!("part {n}"));
                }
                let tmd_off = base + a.tmd_offset;
                if let Some(slot) = buf.get(base..base + SLOT_BYTES) {
                    if let Some(h) = crate::tmd_scan::scan_buffer(slot)
                        .into_iter()
                        .find(|h| h.offset == a.tmd_offset)
                    {
                        sink.claim(tmd_off, tmd_off + h.byte_len, OWNER_TMD, "summon mesh");
                    }
                    sink.claim(
                        base + a.texture_pool_offset,
                        base + SLOT_BYTES,
                        OWNER_TEXTURE,
                        "texture pool",
                    );
                }
            }
            SlotKind::MeArchive { count, compressed } => {
                me += 1;
                if let Some(slot) = buf.get(base..base + SLOT_BYTES) {
                    walk_me_archive_at(slot, base, sink);
                }
                if s.index < 4 {
                    sink.note(format!(
                        "slot {}: ME archive, {count} entries ({compressed} compressed)",
                        s.index
                    ));
                }
                if let Some(slot) = buf.get(base..base + SLOT_BYTES) {
                    sink.nest(
                        opts,
                        depth,
                        format!("slot {} ME archive", s.index),
                        slot,
                        Walker::MeArchive,
                    );
                }
            }
            SlotKind::Payload => {
                raw += 1;
                // The documented raw slot: `[0x1E0 CLUT][0x8000 4bpp page]
                // [part pool to the slot end]` - the big-summon group's third
                // member (`summon_readef::RAW_SLOT_*`). Filler slots are
                // skipped so the constants are not applied to empty space.
                let Some(slot) = buf.get(base..base + SLOT_BYTES) else {
                    continue;
                };
                let nonzero = slot.iter().filter(|&&b| b != 0).count();
                if nonzero * 4 < SLOT_BYTES {
                    continue;
                }
                use crate::summon_readef::{
                    RAW_SLOT_CLUT_BYTES, RAW_SLOT_PAGE_BYTES, RAW_SLOT_PART_POOL_BYTES,
                    RAW_SLOT_PART_POOL_OFFSET,
                };
                sink.claim(
                    base,
                    base + RAW_SLOT_CLUT_BYTES,
                    OWNER_CLUT,
                    "raw slot CLUT block",
                );
                sink.claim(
                    base + RAW_SLOT_CLUT_BYTES,
                    base + RAW_SLOT_CLUT_BYTES + RAW_SLOT_PAGE_BYTES,
                    OWNER_TEXTURE,
                    "raw slot 4bpp page",
                );
                sink.claim(
                    base + RAW_SLOT_PART_POOL_OFFSET,
                    base + RAW_SLOT_PART_POOL_OFFSET + RAW_SLOT_PART_POOL_BYTES,
                    OWNER_RECORD,
                    "raw slot part pool",
                );
            }
        }
    }
    sink.note(format!(
        "{} slots: {tex} texture, {actor} actor record, {me} ME archive, {raw} unclassified; \
         {fill_slots} end in fill the stream SM transfers and never reads",
        f.slots.len()
    ));
}

fn walk_me_archive_at(slot: &[u8], base: usize, sink: &mut Sink) {
    let Ok(me) = crate::me_archive::parse(slot) else {
        return;
    };
    let n = me.len();
    sink.claim(
        base,
        base + 4 + n * 8,
        OWNER_TOC,
        format!("ME toc, {n} entries"),
    );
    for i in 0..n {
        if let Some(body) = me.raw_body(i) {
            // `raw_body` borrows the slot, so the offset is recoverable by
            // pointer arithmetic against the slot's own start.
            let off = body.as_ptr() as usize - slot.as_ptr() as usize;
            sink.claim(
                base + off,
                base + off + body.len(),
                OWNER_ANM,
                format!("ME entry {i}"),
            );
        }
    }
}

// --- player battle files ---------------------------------------------------

fn walk_battle_data_pack(buf: &[u8], sink: &mut Sink, opts: &AccountOptions, depth: u8) {
    let Some(pack) = crate::battle_data_pack::detect(buf) else {
        sink.note("battle_data_pack::detect returned None");
        return;
    };
    sink.claim(0, 0x10, OWNER_HEADER, "desc_off + CLUT offsets + budget");
    // record[0] is an LZS stream between the header and the table.
    if let Some(dec) = legaia_bytes::u32_le(buf, 12).map(|v| v as usize)
        && take_lzs(sink, buf, 0x10, dec, "record[0] (art records)").is_none()
    {
        sink.note("record[0] LZS decode failed");
    }
    let n = pack.records.len();
    let table_end = pack.table_offset + (n + 1) * 12;
    sink.claim(
        pack.table_offset,
        table_end,
        OWNER_TOC,
        format!("{n} [id, offset, size] entries + terminator"),
    );
    // Between the descriptor table's terminator and the compressed-data
    // section the file carries zero fill, and both ends of it are declared:
    // the table ends where its own terminator does, and the data section
    // begins where the pack's `data_base` plus the first descriptor's offset
    // says. Nothing reads between them - the loader seeks each slot by
    // descriptor - so it is slack the container states, not an unwalked
    // region. Claimed only when every byte of it is zero, which is the guard
    // that stops a short walk from buying the gap.
    let data_start = pack
        .records
        .iter()
        .map(|r| pack.data_base + r.data_offset as usize)
        .min()
        .unwrap_or(table_end);
    if data_start > table_end
        && buf
            .get(table_end..data_start)
            .is_some_and(|g| g.iter().all(|&b| b == 0))
    {
        sink.claim(
            table_end,
            data_start,
            OWNER_PAD,
            "descriptor-table to data-section slack",
        );
    }
    for (i, r) in pack.records.iter().enumerate() {
        let off = pack.data_base + r.data_offset as usize;
        sink.claim(off, off + 4, OWNER_HEADER, format!("slot {i} dec_size"));
        let Some(dec) = legaia_bytes::u32_le(buf, off).map(|v| v as usize) else {
            continue;
        };
        let detail = format!("equip slot {i} (id {:#x})", r.id);
        if let Some(out) = take_lzs(sink, buf, off + 4, dec, &detail) {
            sink.nest(
                opts,
                depth,
                format!("slot {i} id {:#x}", r.id),
                &out,
                Walker::Generic,
            );
        }
        // The slot's declared footprint is sector-aligned slack the container
        // itself accounts for.
        sink.claim(
            off,
            off + r.size as usize,
            OWNER_PAD,
            format!("slot {i} declared footprint"),
        );
    }
}

// --- small fixed formats ---------------------------------------------------

fn walk_bse_bank(buf: &[u8], sink: &mut Sink) {
    let Some(b) = crate::bse_bank::detect(buf) else {
        sink.note("bse_bank::detect returned None");
        return;
    };
    sink.claim(0, b.body_offset, OWNER_HEADER, "tag + body offset");
    sink.claim(
        b.body_offset,
        b.body_offset + b.records * crate::bse_bank::RECORD_BYTES,
        OWNER_RECORD,
        format!("{} 8-byte records", b.records),
    );
}

fn walk_init_pak(buf: &[u8], sink: &mut Sink, opts: &AccountOptions, depth: u8) {
    match crate::init_pak::parse(buf) {
        Ok(p) => {
            for (i, l) in p.logos.iter().enumerate() {
                sink.claim(
                    l.file_offset,
                    l.file_offset + l.byte_len,
                    OWNER_TIM,
                    format!("publisher logo {i}"),
                );
            }
        }
        Err(e) => sink.note(format!("init_pak::parse: {e}")),
    }
    // `init.pak` is BOTH: a boot overlay with a static-overlay row and a
    // five-TIM logo pack. The dump corpus is the parser for the code half, so
    // run it here rather than letting the overlay-row override in
    // `pick_walker` replace this walker - that override used to drop every
    // logo claim whenever `--funcs` was given, which is exactly when the
    // sweep runs, so the pack's own bytes read as unwalked format.
    if opts.funcs_dir.is_some() {
        walk_overlay_code(buf, sink, opts);
    } else {
        sink.note("non-TIM region is the boot overlay's code; pass --funcs to credit it");
    }
    let _ = depth;
}

fn walk_field_map(buf: &[u8], sink: &mut Sink) {
    use crate::field_map as fm;
    if fm::detect(buf).is_none() {
        sink.note("field_map::detect returned None");
        return;
    }
    sink.claim(
        fm::OBJECT_RECORDS_OFFSET,
        fm::OBJECT_RECORDS_OFFSET + fm::OBJECT_RECORDS_BYTES,
        OWNER_RECORD,
        format!("{} object descriptors", fm::OBJECT_RECORD_COUNT),
    );
    sink.claim(
        fm::COLLISION_GRID_OFFSET,
        fm::COLLISION_GRID_OFFSET + fm::COLLISION_GRID_BYTES,
        OWNER_GRID,
        "collision + floor grid",
    );
    sink.claim(
        fm::OBJECT_GRID_OFFSET,
        fm::OBJECT_GRID_OFFSET + fm::OBJECT_GRID_BYTES,
        OWNER_GRID,
        "per-tile object index",
    );
    sink.claim(
        fm::TRIGGER_BLOCK_OFFSET,
        fm::TRIGGER_BLOCK_OFFSET + fm::TRIGGER_BLOCK_BYTES,
        OWNER_RECORD,
        "trigger block",
    );
    sink.note(
        "a fixed-layout region file accounts to 100% by construction - the figure \
         says the regions are written down, not that their fields are pinned",
    );
}

fn walk_scene_v12(buf: &[u8], sink: &mut Sink) {
    let Some(t) = crate::scene_v12_table::detect(buf) else {
        sink.note("scene_v12_table::detect returned None");
        return;
    };
    sink.claim(
        0,
        crate::scene_v12_table::RECORDS_OFFSET,
        OWNER_HEADER,
        "8-word v12 header",
    );
    let n = t.records.len();
    sink.claim(
        crate::scene_v12_table::RECORDS_OFFSET,
        crate::scene_v12_table::RECORDS_OFFSET + n * 8,
        OWNER_RECORD,
        format!("{n} inline trigger records"),
    );
}

fn walk_scene_event_scripts(buf: &[u8], sink: &mut Sink) {
    let Some(ranges) = crate::scene_event_scripts::record_ranges(buf) else {
        sink.note("scene_event_scripts::record_ranges returned None");
        return;
    };
    let n = ranges.len();
    sink.claim(0, 2 + n * 2, OWNER_TOC, format!("{n} record offsets"));
    for (i, (a, b)) in ranges.iter().enumerate() {
        sink.claim(*a, *b, OWNER_SCRIPT, format!("stager record {i}"));
    }
}

/// The runtime `efect.dat` 2-pack (PROT `0873`).
///
/// Not the magic-prefixed [effect bundle](crate::effect_bundle) - a headerless
/// file whose first two words are its two packs' offsets, with the sprite
/// atlas inline between the header and pack 0
/// ([`crate::efect_pack`]). The class had a walker slot and no walker behind
/// it, so the whole 8 KB read as unwalked format.
///
/// Both packs address their members by **absolute file offset**, so a member
/// runs to the next offset in its own table and the last to its pack's extent -
/// pack 0's being pack 1's start, not the file's end.
fn walk_efect_dat(buf: &[u8], sink: &mut Sink) {
    let Some(p) = crate::efect_pack::detect(buf) else {
        sink.note("efect_pack::detect returned None");
        return;
    };
    sink.claim(0, 8, OWNER_HEADER, "pack0 + pack1 offsets");
    if p.atlas_entries > 0 {
        sink.claim(
            8,
            p.pack0_offset,
            OWNER_RECORD,
            format!("{} sprite-atlas entries", p.atlas_entries),
        );
    }
    for (pack, at, count, limit, owner, what) in [
        (
            0usize,
            p.pack0_offset,
            p.pack0_count,
            p.pack1_offset,
            OWNER_ANM,
            "frame batch",
        ),
        (
            1,
            p.pack1_offset,
            p.pack1_count,
            buf.len(),
            OWNER_SCRIPT,
            "spawn script",
        ),
    ] {
        sink.claim(
            at,
            at + 4 + 4 * count,
            OWNER_TOC,
            format!("pack {pack}: {count} absolute offsets"),
        );
        for i in 0..count {
            let Some(start) = legaia_bytes::u32_le(buf, at + 4 + 4 * i).map(|v| v as usize) else {
                break;
            };
            let end = legaia_bytes::u32_le(buf, at + 8 + 4 * i)
                .map(|v| v as usize)
                .filter(|_| i + 1 < count)
                .unwrap_or(limit)
                .min(buf.len());
            if end > start {
                sink.claim(start, end, owner, format!("pack {pack} {what} {i}"));
            }
        }
    }
}

fn walk_effect_bundle(buf: &[u8], sink: &mut Sink) {
    let Some(e) = crate::effect_bundle::detect(buf) else {
        sink.note("effect_bundle::detect returned None");
        return;
    };
    sink.claim(
        e.magic_offset,
        e.table_offset,
        OWNER_HEADER,
        "magic + header",
    );
    sink.claim(
        e.table_offset,
        e.table_offset + crate::effect_bundle::TABLE_SIZE,
        OWNER_TOC,
        format!("{}-slot schema", crate::effect_bundle::RECORD_COUNT),
    );
    for s in &e.slots {
        let start = e.magic_offset + s.offset as usize;
        if let Some(size) = s.size {
            sink.claim(start, start + size as usize, OWNER_RECORD, "effect slot");
        }
    }
}

/// `prot::timpack`: `[2 header bytes][u32 tim_num][i32 word_offsets]`, each
/// member starting at `word_index * 4 + 4` and running to the next member.
/// Re-derived from `docs/formats/tim-pack.md` - `legaia_prot::timpack` exposes
/// the member *bytes* but not the offsets they came from.
fn walk_tim_pack(buf: &[u8], sink: &mut Sink) {
    if !legaia_prot::timpack::is_tim_pack(buf) {
        sink.note("timpack::is_tim_pack rejected the buffer");
        return;
    }
    let n = i32::from_le_bytes(buf[4..8].try_into().unwrap()) as usize;
    sink.claim(0, 8 + n * 4, OWNER_TOC, format!("{n} word offsets"));
    let mut offsets: Vec<usize> = (0..n)
        .filter_map(|x| {
            let e = i32::from_le_bytes(buf[8 + 4 * x..12 + 4 * x].try_into().unwrap());
            let off = (e as i64) * 4 + 4;
            (off >= 0 && off as usize <= buf.len()).then_some(off as usize)
        })
        .collect();
    offsets.sort_unstable();
    offsets.dedup();
    offsets.push(buf.len());
    for (i, w) in offsets.windows(2).enumerate() {
        let owner = if buf.get(w[0]) == Some(&0x10) {
            OWNER_TIM
        } else {
            OWNER_RECORD
        };
        sink.claim(w[0], w[1], owner, format!("member {i}"));
    }
}

/// `asset::pack` whose members are whole TIMs, claimed at their own extent
/// rather than out to the next member.
///
/// [`walk_pack`] ends the last member at the buffer end, which would swallow
/// any tail the pack does not reference. Entry 0892 has 948 such bytes past
/// its second TIM, and they are the interesting part of the accounting.
fn walk_card_font_pack(buf: &[u8], sink: &mut Sink) {
    let Ok(entries) = crate::pack::parse_pack(buf) else {
        sink.note("pack::parse_pack failed");
        return;
    };
    let n = entries.len();
    sink.claim(0, 4 + n * 4, OWNER_TOC, format!("{n} word offsets"));
    for e in &entries {
        let member = &buf[e.byte_offset..e.byte_offset + e.size];
        let mut claimed = false;
        for h in crate::tim_scan::scan_buffer(member) {
            if h.offset != 0 {
                continue;
            }
            sink.claim(
                e.byte_offset,
                e.byte_offset + h.byte_len,
                OWNER_TIM,
                format!("member {} - {}x{} {}bpp", e.index, h.width, h.height, h.bpp),
            );
            claimed = true;
            break;
        }
        if !claimed {
            sink.claim(
                e.byte_offset,
                e.byte_offset + e.size,
                OWNER_RECORD,
                format!("member {}", e.index),
            );
        }
    }
}

fn walk_pack(buf: &[u8], sink: &mut Sink) {
    let Ok(entries) = crate::pack::parse_pack(buf) else {
        sink.note("pack::parse_pack failed");
        return;
    };
    let n = entries.len();
    sink.claim(0, 4 + n * 4, OWNER_TOC, format!("{n} word offsets"));
    for e in &entries {
        sink.claim(
            e.byte_offset,
            e.byte_offset + e.size,
            member_owner(buf, e.byte_offset),
            format!("member {}", e.index),
        );
    }
}

/// Owner for a pack member, from its own leading magic.
fn member_owner(buf: &[u8], start: usize) -> &'static str {
    match legaia_bytes::u32_le(buf, start) {
        Some(0x0000_0010) => OWNER_TIM,
        Some(0x8000_0002) => OWNER_TMD,
        _ => OWNER_RECORD,
    }
}

/// `[u32 count][u32 byte_offset[count]][members]` with **absolute** byte
/// offsets - the container the bundle's VDF (type `0x07`) and clip-bank
/// (type `0x05`) sections share, and the fallback when a clip bank's records
/// do not carry the ANM header. Distinct from [`crate::pack`], whose offsets
/// are word indices; the anchor `offsets[0] == 4 + 4*count` tells the two
/// apart because a word-offset table would put member 0 four times further on.
///
/// Returns the member ranges it claimed, so a caller can walk inside them.
fn walk_offset_pack(
    buf: &[u8],
    sink: &mut Sink,
    owner: &'static str,
    what: &str,
) -> Vec<std::ops::Range<usize>> {
    let Some(count) = legaia_bytes::u32_le(buf, 0) else {
        sink.note("buffer too small for an offset-pack header");
        return Vec::new();
    };
    let count = count as usize;
    let table_end = 4 + count * 4;
    if count == 0 {
        // Several bundles ship an empty section - the count word and nothing
        // else. That is the whole container, not a parse failure.
        sink.claim(
            0,
            4.min(buf.len()),
            OWNER_HEADER,
            "empty container (count 0)",
        );
        return Vec::new();
    }
    if table_end > buf.len() {
        sink.note(format!("implausible offset-pack count {count}"));
        return Vec::new();
    }
    let mut offsets = Vec::with_capacity(count);
    for i in 0..count {
        let Some(off) = legaia_bytes::u32_le(buf, 4 + i * 4) else {
            sink.note("offset table truncated");
            return Vec::new();
        };
        let off = off as usize;
        if off < table_end || off > buf.len() || offsets.last().is_some_and(|&p| off < p) {
            sink.note(format!("offset[{i}] = 0x{off:X} is not a member start"));
            return Vec::new();
        }
        offsets.push(off);
    }
    sink.claim(0, table_end, OWNER_TOC, format!("{count} byte offsets"));
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        let end = offsets.get(i + 1).copied().unwrap_or(buf.len());
        sink.claim(offsets[i], end, owner, format!("{what} {i}"));
        out.push(offsets[i]..end);
    }
    out
}

/// A bundle's type-`0x05` section: the ANM **clip bank**, not a Tactical-Arts
/// move table. Each clip is an 8-byte [`crate::player_anm`] header followed by
/// `bone_count * frame_count` 8-byte per-(bone, frame) transforms and an
/// 8-byte record-boundary trailer, so a clip's claimed extent is
/// `8 + bones*frames*8 + 8` and any shortfall against the offset table shows
/// up as residue rather than being absorbed.
fn walk_clip_bank(buf: &[u8], sink: &mut Sink) {
    let Ok(bank) = crate::player_anm::parse(buf) else {
        // Not the `0x080C` record family - still an offset pack, and its
        // members are still real extents.
        walk_offset_pack(buf, sink, OWNER_ANM, "clip");
        return;
    };
    let n = bank.record_offsets.len();
    sink.claim(0, 4 + n * 4, OWNER_TOC, format!("{n} clip offsets"));
    for i in 0..n {
        let start = bank.record_offsets[i] as usize;
        let end = start + bank.record_sizes[i] as usize;
        let rec = match bank.record(i) {
            Ok(r) => r,
            Err(e) => {
                sink.note(format!("clip {i}: {e}"));
                continue;
            }
        };
        let body = crate::player_anm::RECORD_HEADER_SIZE
            + rec.bone_count as usize
                * rec.frame_count as usize
                * crate::player_anm::BONE_FRAME_BYTES;
        sink.claim(
            start,
            start + crate::player_anm::RECORD_HEADER_SIZE,
            OWNER_HEADER,
            format!("clip {i} header"),
        );
        let body_end = (start + body).min(end);
        sink.claim(
            start + crate::player_anm::RECORD_HEADER_SIZE,
            body_end,
            OWNER_ANM,
            format!(
                "clip {i}: {} bones x {} frames",
                rec.bone_count, rec.frame_count
            ),
        );
        if body_end < end {
            sink.claim(body_end, end, OWNER_PAD, format!("clip {i} trailer"));
        }
    }
}

/// A bundle's type-`0x04` section - a MES dialog container
/// (`docs/formats/mes.md`, crate `legaia-mes`).
///
/// The `Compact` form leads with the `0x00000404` magic and a fixed header
/// region; the `Records` form is variable-stride records delimited by the
/// `0x44 0x78` marker. Several retail bundles carry a 40-byte *empty* compact
/// MES - the magic and nothing else - which is the shape the header-region
/// bound below exists for.
fn walk_mes(buf: &[u8], sink: &mut Sink) {
    match legaia_mes::detect_format(buf) {
        Some(legaia_mes::Format::Compact) => {
            let head = legaia_mes::compact::OFFSET_TABLE_END.min(buf.len());
            sink.claim(0, 4.min(buf.len()), OWNER_HEADER, "compact magic");
            sink.claim(4.min(buf.len()), head, OWNER_TOC, "compact header region");
            if head < buf.len() {
                sink.claim(head, buf.len(), OWNER_SCRIPT, "dialog bytecode");
            }
        }
        Some(legaia_mes::Format::Records) => {
            let Ok(blob) = legaia_mes::parse(buf) else {
                sink.note("mes::parse failed on a records blob");
                return;
            };
            let marks: Vec<usize> = blob
                .records
                .unwrap_or_default()
                .iter()
                .map(|r| r.offset)
                .collect();
            if let Some(&first) = marks.first() {
                sink.claim(0, first, OWNER_HEADER, "pre-record head");
            }
            for (i, &m) in marks.iter().enumerate() {
                let end = marks.get(i + 1).copied().unwrap_or(buf.len());
                sink.claim(m, end, OWNER_RECORD, format!("record {i}"));
            }
        }
        None => sink.note("mes::detect_format matched neither layout"),
    }
}

// --- leaf sub-assets -------------------------------------------------------

fn walk_tim(buf: &[u8], sink: &mut Sink) {
    for h in crate::tim_scan::scan_buffer(buf) {
        sink.claim(
            h.offset,
            h.offset + h.byte_len,
            OWNER_TIM,
            format!("{}x{} {}bpp", h.width, h.height, h.bpp),
        );
    }
}

fn walk_tmd(buf: &[u8], sink: &mut Sink) {
    for h in crate::tmd_scan::scan_buffer(buf) {
        sink.claim(
            h.offset,
            h.offset + h.byte_len,
            OWNER_TMD,
            format!("{} objects, {} verts", h.n_obj, h.total_verts),
        );
    }
}

fn walk_vab(buf: &[u8], sink: &mut Sink) {
    match legaia_vab::parse(buf, 0) {
        Ok(r) => {
            let body = legaia_vab::VAB_HEADER_SIZE
                + legaia_vab::PROGRAMS_TABLE_SIZE
                + r.programs.len() * legaia_vab::TONES_PER_PROGRAM * legaia_vab::TONE_SIZE
                + legaia_vab::VAG_TABLE_ENTRIES * 2;
            sink.claim(0, body.min(buf.len()), OWNER_TOC, "VAB header + tables");
            for s in &r.vag_samples {
                sink.claim(
                    s.byte_offset,
                    s.byte_offset + s.size,
                    OWNER_VAB,
                    format!("VAG {}", s.index),
                );
            }
            sink.note(format!("VAB declares fsize {} B", r.header.fsize));
        }
        Err(e) => sink.note(format!("vab::parse: {e}")),
    }
}

/// Walk a Legaia SEQ from `off` and claim `[off, end_of_track)`.
///
/// Re-derived from `docs/formats/seq.md` rather than taken from `legaia-seq`,
/// which is a dev-dependency of this crate. Two Legaia-specific divergences
/// from PsyQ matter: the version field is a `u32` BE (header is 15 bytes, not
/// 13), and a meta event carries **no** MIDI variable-length `length` byte -
/// `FF 51` is followed by exactly 3 tempo bytes and `FF 2F` ends the track.
pub fn seq_extent(buf: &[u8], off: usize) -> Option<usize> {
    const HEADER: usize = 0x0F;
    if buf.get(off..off + 4)? != b"pQES" {
        return None;
    }
    let mut p = off + HEADER;
    let mut running: u8 = 0;
    loop {
        // Delta time (MIDI VLQ).
        let mut guard = 0;
        loop {
            let b = *buf.get(p)?;
            p += 1;
            guard += 1;
            if b & 0x80 == 0 || guard > 4 {
                break;
            }
        }
        let b = *buf.get(p)?;
        let status = if b & 0x80 != 0 {
            p += 1;
            running = b;
            b
        } else {
            running
        };
        match status {
            0xFF => {
                let kind = *buf.get(p)?;
                p += 1;
                match kind {
                    0x2F => return Some(p - off),
                    0x51 => p += 3,
                    0x58 => p += 4,
                    _ => return None,
                }
            }
            0x80..=0xBF | 0xE0..=0xEF => p += 2,
            0xC0..=0xDF => p += 1,
            _ => return None,
        }
        if p > buf.len() {
            return None;
        }
    }
}

/// Walk the multi-bank VAB archive (`monster.snd`, extraction 891).
///
/// Every claim comes out of a length the container states: the bank count and
/// the `count + 1` start sectors `FUN_8003E104` indexes, then each bank's two
/// DATA_FIELD chunk headers. Nothing here rests on a magic sweep - the `pBAV`
/// magic is only a gate on the class, never a claim boundary.
/// See [`crate::vab_multi_bank`].
fn walk_vab_multi_bank(buf: &[u8], sink: &mut Sink) {
    use crate::vab_multi_bank::{self, SECTOR};
    let Some(r) = vab_multi_bank::detect(buf) else {
        sink.note("vab_multi_bank::detect declined");
        return;
    };
    sink.claim(0, 8, OWNER_HEADER, "reserved word + bank count");
    sink.claim(
        8,
        r.table_end().min(buf.len()),
        OWNER_TOC,
        format!(
            "bank start-sector table, {} words (one per bank plus the end sentinel)",
            r.count + 1
        ),
    );
    if r.banks.len() != r.count {
        sink.note(format!(
            "bank walk resolved {} of the {} banks the head declares",
            r.banks.len(),
            r.count
        ));
    }
    // The archive's first sector holds the index table; the reader stages
    // 0x400 bytes of it, so the rest of that sector is slack by construction.
    if r.banks.first().is_some_and(|b| b.offset() >= SECTOR) {
        sink.claim(r.table_end(), SECTOR, OWNER_PAD, "index-sector slack");
    }
    let mut vag_total = 0usize;
    for b in &r.banks {
        let i = b.index;
        sink.claim(
            b.offset(),
            b.offset() + 4,
            OWNER_HEADER,
            format!("bank {i} chunk 0 header"),
        );
        sink.claim(
            b.vab_offset(),
            b.body_chunk_offset(),
            OWNER_TOC,
            format!(
                "bank {i} VAB header + {} program slot(s), tone rows, VAG size table",
                b.programs
            ),
        );
        sink.claim(
            b.body_chunk_offset(),
            b.body_offset(),
            OWNER_HEADER,
            format!("bank {i} chunk 1 header"),
        );
        sink.claim(
            b.body_offset(),
            b.content_end(),
            OWNER_VAB,
            format!("bank {i} VAG bodies, {} samples", b.vags),
        );
        vag_total += b.vags as usize;
        // The stream terminator, then the sector slack the index table's next
        // entry declares. Not zero fill: the builder left its sector buffer's
        // previous contents behind it, which nothing reads - both chunk
        // lengths and `fsize` end before it.
        let end = b.offset() + b.span();
        if legaia_bytes::u32_le(buf, b.content_end()) == Some(0) {
            sink.claim(
                b.content_end(),
                b.content_end() + 4,
                OWNER_HEADER,
                format!("bank {i} stream terminator"),
            );
            sink.claim(
                b.content_end() + 4,
                end.min(buf.len()),
                OWNER_PAD,
                format!("bank {i} sector slack past the declared stream"),
            );
        } else {
            sink.claim(
                b.content_end(),
                end.min(buf.len()),
                OWNER_PAD,
                format!("bank {i} sector slack past the declared stream"),
            );
        }
    }
    sink.note(format!("{} banks, {vag_total} VAG bodies", r.banks.len()));
}

fn walk_seq(buf: &[u8], sink: &mut Sink) {
    match seq_extent(buf, 0) {
        Some(n) => {
            sink.claim(0, 0x0F, OWNER_HEADER, "pQES header");
            sink.claim(0x0F, n, OWNER_SEQ, "event stream to end-of-track");
        }
        None => sink.note("SEQ event walk did not reach an end-of-track meta"),
    }
}

fn walk_anm(buf: &[u8], sink: &mut Sink) {
    let payload = legaia_anm::peel_preamble(buf).unwrap_or(buf);
    let skew = buf.len() - payload.len();
    match legaia_anm::parse(payload) {
        Ok(pack) => {
            sink.claim(
                skew,
                skew + 4 + pack.records.len() * 4,
                OWNER_TOC,
                format!("{} record offsets", pack.records.len()),
            );
            for (i, r) in pack.records.iter().enumerate() {
                sink.claim(
                    skew + r.offset,
                    skew + r.offset + r.size,
                    OWNER_ANM,
                    format!("record {i}"),
                );
            }
        }
        Err(e) => {
            // The kingdom bundles' type-`0x06` sections are the same
            // `[u32 count][u32 byte_offset[count]]` container without the
            // `0x080C` record header `legaia_anm::parse` gates on, so the
            // member extents are still readable.
            sink.note(format!("anm::parse: {e}"));
            walk_offset_pack(buf, sink, OWNER_ANM, "record");
        }
    }
}

/// A decompressed MAN: `[0x2B header][u24 record offsets][record bodies]
/// [6 chained tail sections]`.
///
/// The record bodies are the bulk, and `man_section` exposes them only as the
/// partition offset tables - so the per-record extents are re-derived here from
/// those tables, the way `docs/formats/man-relocation.md` describes the layout:
/// an offset is relative to the data region, records tile it in table order,
/// and the region ends where section 0 starts (`data_region + u24_at_28`).
fn walk_man(buf: &[u8], sink: &mut Sink) {
    use crate::man_section::RECORDS_BEGIN_OFFSET;
    match crate::man_section::parse(buf) {
        Ok(m) => {
            sink.claim(0, RECORDS_BEGIN_OFFSET, OWNER_HEADER, "MAN header");
            sink.claim(
                RECORDS_BEGIN_OFFSET,
                m.data_region_offset,
                OWNER_TOC,
                "u24 record-offset partitions",
            );
            let base = m.data_region_offset;
            let region_end = base.saturating_add(m.header.u24_at_28 as usize);
            let mut offs: Vec<usize> = m
                .partitions
                .iter()
                .flatten()
                .map(|&o| o as usize)
                .filter(|&o| base + o < region_end)
                .collect();
            offs.sort_unstable();
            offs.dedup();
            for (i, o) in offs.iter().enumerate() {
                let end = offs.get(i + 1).map_or(region_end, |n| base + n);
                sink.claim(base + o, end, OWNER_SCRIPT, format!("record {i}"));
            }
            for (i, s) in m.sections.iter().enumerate() {
                sink.claim(
                    s.offset,
                    s.offset + 3 + s.length as usize,
                    OWNER_SCRIPT,
                    format!("tail section {i}"),
                );
            }
        }
        Err(e) => sink.note(format!("man_section::parse: {e:?}")),
    }
}

// --- overlay code ----------------------------------------------------------

fn walk_overlay_code(buf: &[u8], sink: &mut Sink, opts: &AccountOptions) {
    let Some(idx) = opts.prot_index else {
        sink.note("overlay walker needs --prot-index");
        return;
    };
    let Some(rec) = crate::static_overlay::overlay_map()
        .overlays
        .iter()
        .find(|r| r.prot_index == idx)
    else {
        sink.note(format!("no static-overlays.toml row for PROT {idx}"));
        return;
    };
    let Some(dir) = opts.funcs_dir.as_ref() else {
        sink.note("overlay walker needs --funcs <ghidra/scripts/funcs>");
        return;
    };
    let dumps = match read_dump_extents(dir) {
        Ok(d) => d,
        Err(e) => {
            sink.note(format!("reading {}: {e}", dir.display()));
            return;
        }
    };
    let base = rec.base_va;
    let hi = base as u64 + buf.len() as u64;
    let label_ok = |l: &Option<String>| match l {
        Some(l) => {
            l == &rec.label || l.starts_with(&format!("{idx:04}")) || l.starts_with(&rec.label)
        }
        None => false,
    };
    let (mut confirmed, mut refuted, mut ambiguous, mut credited_by_label) = (0, 0, 0, 0);
    let mut fill_extents = 0usize;
    let mut uncorroborated_labels = 0usize;
    let mut by_label: Vec<(usize, usize, u32)> = Vec::new();
    for d in &dumps {
        if (d.entry_va as u64) < base as u64 || (d.entry_va as u64) >= hi {
            continue;
        }
        let start = (d.entry_va - base) as usize;
        let end = (start + d.bytes as usize).min(buf.len());
        // A dump over an image's zero region is in the corpus (one is 4646
        // printed `nop`s) and its extent is fill, not code, in whatever image
        // it is checked against - including the one its filename names. Never
        // credit an all-zero extent as `code`: the shape classifier will call
        // the run `zero_pad`, which is what it is.
        if buf
            .get(start..end)
            .is_some_and(|w| w.iter().all(|&b| b == 0))
        {
            fill_extents += 1;
            continue;
        }
        match attribute(d, buf, base) {
            Attribution::Confirmed => {
                confirmed += 1;
                sink.claim(start, end, OWNER_CODE, format!("FUN_{:08x}", d.entry_va));
            }
            Attribution::Refuted => refuted += 1,
            Attribution::Unverifiable => {
                if label_ok(&d.label) {
                    by_label.push((start, end, d.entry_va));
                } else {
                    ambiguous += 1;
                }
            }
        }
    }
    // A label-credited extent is the one claim here that rests on a filename,
    // and a filename says where a dump was TAKEN, not which image the bytes
    // are. In an image where some other extent re-encodes to this image's own
    // words the label is corroborated by those; in an image where NOTHING
    // confirms, it is the whole of the evidence - and that is exactly the case
    // where the dump program's base was wrong, so the extents land at arbitrary
    // offsets in a file that never held them. Credit the label only alongside a
    // byte confirmation.
    if confirmed > 0 {
        for (start, end, entry_va) in by_label {
            credited_by_label += 1;
            sink.claim(
                start,
                end,
                OWNER_CODE,
                format!("FUN_{entry_va:08x} (by label)"),
            );
        }
    } else {
        uncorroborated_labels = by_label.len();
    }
    sink.ambiguous_dumps = ambiguous + uncorroborated_labels;
    sink.refuted_dumps = refuted;
    sink.note(format!(
        "base {:#010x} ({}); {confirmed} extents confirmed by bytes, \
         {credited_by_label} credited by filename label, {ambiguous} unverifiable, \
         {refuted} refuted (aliased sibling), {fill_extents} land on fill",
        base, rec.label
    ));
    if uncorroborated_labels > 0 {
        sink.note(format!(
            "{uncorroborated_labels} label-matching extent(s) left uncredited: \
             no dump in this image confirms by bytes, so a filename is the whole \
             of their evidence"
        ));
    }
    claim_uninitialised_data(buf, sink, base);
    claim_pinned_overlay_assets(buf, sink, idx);
    claim_formed_strings(buf, sink, base);
}

/// Shortest share of printable ASCII (`0x20..=0x7E`) a NUL-terminated run
/// must reach to be read as a string - three quarters, so the dialog escape
/// bytes some labels carry do not disqualify them.
const FORMED_STRING_PRINTABLE_NUM: usize = 3;
const FORMED_STRING_PRINTABLE_DEN: usize = 4;

/// End (one past the NUL) of the C string at `off`, or `None` when the bytes
/// there are not one.
fn cstring_end(buf: &[u8], off: usize) -> Option<usize> {
    let tail = buf.get(off..)?;
    let len = tail.iter().position(|&b| b == 0)?;
    if len == 0 {
        return None;
    }
    let printable = tail[..len]
        .iter()
        .filter(|&&b| (0x20..0x7F).contains(&b))
        .count();
    (printable * FORMED_STRING_PRINTABLE_DEN >= len * FORMED_STRING_PRINTABLE_NUM)
        .then_some(off + len + 1)
}

/// Strings - and tables of pointers to strings - whose address the image's own
/// code forms with a `lui` pair.
///
/// An overlay's rodata string pool has no header and no count; what bounds a
/// string is its own NUL, and what makes a byte run a *string of this image*
/// rather than text-shaped data is that this image's code computes its address
/// ([`formed_addresses`], the same pointer-forming test the uninitialised-data
/// claim rests on). One level of indirection is followed: where the formed
/// address holds a run of two or more in-image words that each point at a
/// string (or at an empty / one-byte one), the words are a pointer table and
/// are claimed with the strings they name. A target already inside a claim (code, a pinned table, the inherited
/// tail) is left to that claim.
fn claim_formed_strings(buf: &[u8], sink: &mut Sink, base: u32) {
    let in_claim =
        |sink: &Sink, off: usize| sink.claims.iter().any(|c| c.start <= off && off < c.end);
    let to_off = |va: u32| -> Option<usize> {
        let o = va.checked_sub(base)? as usize;
        (o < buf.len()).then_some(o)
    };
    // A pair issued from the inherited tail is the donor's code forming the
    // donor's addresses; it names nothing of this image.
    let tail: Vec<(usize, usize)> = sink
        .claims
        .iter()
        .filter(|c| c.owner == OWNER_INHERITED_TAIL)
        .map(|c| (c.start, c.end))
        .collect();
    let mut targets: Vec<u32> = formed_addresses(buf, base)
        .into_iter()
        .filter(|&(site, _)| {
            let s = site.wrapping_sub(base) as usize;
            !tail.iter().any(|&(a, b)| a <= s && s < b)
        })
        .map(|(_, t)| t)
        .collect();
    targets.sort_unstable();
    targets.dedup();
    let (mut strings, mut tables) = (0usize, 0usize);
    for t in targets {
        let Some(off) = to_off(t) else { continue };
        if in_claim(sink, off) {
            continue;
        }
        if let Some(end) = cstring_end(buf, off) {
            sink.claim(
                off,
                end,
                OWNER_STRING,
                format!("string, address formed by this image ({t:#010x})"),
            );
            strings += 1;
            continue;
        }
        if off % 4 != 0 {
            continue;
        }
        let mut k = off;
        let mut named: Vec<(usize, usize)> = Vec::new();
        while let Some(w) = legaia_bytes::u32_le(buf, k) {
            let Some(s) = to_off(w) else { break };
            // Inside a table an entry may be empty or one glyph byte (the
            // options screen's button-glyph choice): the neighbours already
            // say what the table is, so the printable test is not asked of a
            // string too short to carry it.
            let short = buf
                .get(s..s + 2)
                .and_then(|b| b.iter().position(|&x| x == 0));
            let Some(e) = cstring_end(buf, s).or(short.map(|n| s + n + 1)) else {
                break;
            };
            named.push((s, e));
            k += 4;
        }
        if named.len() >= 2 {
            sink.claim(
                off,
                k,
                OWNER_TOC,
                format!(
                    "string pointer table, {} words (formed at {t:#010x})",
                    named.len()
                ),
            );
            for (s, e) in named {
                if !in_claim(sink, s) {
                    sink.claim(s, e, OWNER_STRING, "string named by a formed pointer table");
                }
            }
            tables += 1;
        }
    }
    if strings + tables > 0 {
        sink.note(format!(
            "{strings} string(s) and {tables} string-pointer table(s) at addresses this image's own code forms"
        ));
    }
}

/// Link base of the image being accounted, from its `static-overlays.toml` row.
/// Falls back to the slot-B base, which is the only base a slot-B walk is ever
/// selected for.
fn base_for(opts: &AccountOptions) -> u32 {
    opts.prot_index
        .and_then(|i| crate::static_overlay::overlay_map().by_prot_index(i))
        .map(|r| r.base_va)
        .unwrap_or(crate::slot_b_module::SLOT_B_LINK_BASE)
}

/// File offset at which this image stops being its own content, when it is a
/// mapped overlay and the sibling entries can be read. See
/// [`crate::inherited_tail`].
fn inherited_tail_start(buf: &[u8], opts: &AccountOptions) -> Option<usize> {
    let idx = opts.prot_index?;
    let dir = opts.prot_dir.as_ref()?;
    let t = crate::inherited_tail::tails_cached(dir)
        .get(&idx)
        .cloned()?;
    (t.image_bytes == buf.len() && t.start < buf.len()).then_some(t.start)
}

/// Claim the run at which this overlay image stops being its own content.
///
/// The packer wrote every overlay into a buffer it did not clear, so a module
/// shorter than the buffer flushes its own bytes and then the previous, longer
/// module's residue - inside the entry, at the file offsets that module
/// occupies. Those bytes are that module's, so no parser of THIS entry can ever
/// consume them: counting them as residue puts work on the worklist that no
/// work can close, and gives the run the shape of un-dumped code.
/// `scripts/ci/disc-coverage.py` has cut tails out of its denominator since the
/// rule was found; this is the byte account's side of the same cut, and
/// [`crate::inherited_tail`] is the shared measurement.
///
/// The claim is made before any walker runs, so a walker that reaches into the
/// tail (a slot-B record chain walking on into the donor's residue) loses no
/// claim of its own - claims merge - while the residue classifier no longer
/// sees the run.
fn claim_inherited_tail(buf: &[u8], sink: &mut Sink, opts: &AccountOptions) {
    let Some(idx) = opts.prot_index else {
        return;
    };
    if crate::static_overlay::overlay_map()
        .by_prot_index(idx)
        .is_none()
    {
        return;
    }
    let Some(dir) = opts.prot_dir.as_ref() else {
        sink.note(
            "inherited-tail cut unavailable: an overlay's tail is a comparison \
             against its sibling entries, and no --prot-dir was given",
        );
        return;
    };
    let tails = crate::inherited_tail::tails_cached(dir);
    let Some(t) = tails.get(&idx) else {
        // No mapped sibling reproduces the tail - but the donor need not be an
        // overlay at all. The packer's one buffer predicts the slack from
        // whichever entry held those offsets last; PROT 0898's last sector
        // above the slot-B base and PROT 0895's are PROT 0894's bytes.
        claim_buffer_suffix(buf, sink, dir, idx);
        return;
    };
    // A nested pass (a decoded LZS payload) carries the outer entry's index but
    // not its bytes, and a file offset measured on the image means nothing in
    // it.
    if t.image_bytes != buf.len() || t.start >= buf.len() {
        return;
    }
    sink.claim(
        t.start,
        buf.len(),
        OWNER_INHERITED_TAIL,
        format!(
            "PROT {:04} ({})'s bytes at the same file offset",
            t.donor_prot_index, t.donor_label
        ),
    );
}

/// Claim the last-sector slack above a parser's measured content end when the
/// packer's buffer reproduces it byte for byte.
///
/// The mapped overlays get the same cut from [`claim_inherited_tail`], where
/// the own-content end is itself a measurement the cut feeds back into. Every
/// other entry has a parser whose claims already *are* the content end - a
/// scene bundle's last descriptor stops where `legaia_lzs::decompress_tracked`
/// stopped consuming - so the slack above the highest claim is tested whole
/// against [`crate::inherited_tail::buffer_run`]: the nearest earlier entry
/// reaching each offset must hold the same byte there. All or nothing; a run
/// with one byte the prediction does not reproduce stays residue, and an
/// all-zero run stays the `zero_pad` it already is.
///
/// This is the whole of the `scene_asset_table` class's residue: every bundle
/// on the disc ends its last LZS stream inside its last sector, and the bytes
/// above are an earlier entry's, at the same file offsets.
fn claim_buffer_slack(buf: &[u8], sink: &mut Sink, opts: &AccountOptions) {
    let (Some(idx), Some(dir)) = (opts.prot_index, opts.prot_dir.as_ref()) else {
        return;
    };
    if crate::static_overlay::overlay_map()
        .by_prot_index(idx)
        .is_some()
    {
        return;
    }
    let Some(end) = sink.claims.iter().map(|c| c.end).max() else {
        return;
    };
    if end >= buf.len() || buf[end..].iter().all(|&b| b == 0) {
        return;
    }
    let Some(pieces) = crate::inherited_tail::buffer_run(dir, idx, buf, end) else {
        return;
    };
    for p in pieces {
        let detail = match p.donor {
            Some(d) => format!("PROT {d:04}'s bytes at the same file offset (packer buffer)"),
            None => "zero - no earlier entry reached this offset (packer buffer)".to_string(),
        };
        sink.claim(p.start, p.end, OWNER_INHERITED_TAIL, detail);
    }
}

/// A mapped overlay's last-sector slack when its donor is not a mapped
/// overlay: the suffix [`crate::inherited_tail::buffer_suffix_start`] finds,
/// word-aligned up (the image's own content is word-granular, so a byte or
/// three of coincidental agreement below the true end is not a tail).
fn claim_buffer_suffix(buf: &[u8], sink: &mut Sink, dir: &std::path::Path, idx: u32) {
    let Some(start) = crate::inherited_tail::buffer_suffix_start(dir, idx, buf) else {
        return;
    };
    let start = (start + 3) & !3;
    let Some(pieces) = crate::inherited_tail::buffer_run(dir, idx, buf, start) else {
        return;
    };
    for p in pieces {
        let detail = match p.donor {
            Some(d) => format!("PROT {d:04}'s bytes at the same file offset (packer buffer)"),
            None => "zero - no earlier entry reached this offset (packer buffer)".to_string(),
        };
        sink.claim(p.start, p.end, OWNER_INHERITED_TAIL, detail);
    }
}

/// Sub-assets an overlay image carries at an offset this workspace has pinned.
///
/// The dump corpus is the parser for a code image's code and says nothing about
/// its data segment, so an asset sitting in that segment falls to the magic
/// sweep - which finds it, tags the claim `scan`, and thereby reports "found by
/// guessing" for something a module here already reads at a named constant.
/// That gap is a binding, not a format: the offsets below are the constants,
/// and the extents come from the TIM headers rather than from this table.
fn claim_pinned_overlay_assets(buf: &[u8], sink: &mut Sink, prot_index: u32) {
    const MENU_OVERLAY: u32 = 899;
    if prot_index == MENU_OVERLAY {
        // The option-node list's extent is its own zero-word terminator, so
        // it is measured here rather than carried as a fixed row.
        if let Some(len) = crate::menu_windows::options_node_list_len(buf) {
            let off = crate::menu_windows::OPTIONS_NODE_LIST_OFFSET;
            sink.claim(
                off,
                off + len,
                OWNER_RECORD,
                "options row-descriptor list (menu_windows)",
            );
        }
        for (off, what) in [
            (
                crate::title_pak::OVERLAY_SAVE_MENU_TIM_OFFSET,
                "save-menu UI atlas",
            ),
            (crate::save_icon::PROT_ENTRY_OFFSET, "save-slot icon sheet"),
        ] {
            match crate::tim_scan::parse_at(buf, off) {
                Some(h) => sink.claim(
                    off,
                    (off + h.byte_len).min(buf.len()),
                    OWNER_TIM,
                    format!("{what}, {}x{} {}bpp", h.width, h.height, h.bpp),
                ),
                None => sink.note(format!("no TIM at the pinned {what} offset {off:#x}")),
            }
        }
    }
    for (off, len, owner, what) in pinned_overlay_tables(prot_index) {
        let end = off + len;
        if end > buf.len() {
            sink.note(format!(
                "pinned {what} at {off:#x} + {len} runs past this entry"
            ));
            continue;
        }
        sink.claim(off, end, owner, what);
    }
    if prot_index == 898 {
        claim_effect_proto_records(buf, sink);
        claim_battle_overlay_strings(buf, sink);
        claim_battle_jump_tables(buf, sink);
    }
    if prot_index == STR_OVERLAY_PROT_INDEX {
        claim_str_overlay_tables(buf, sink);
    }
    if prot_index == crate::other3_roster::OVERLAY_PROT_INDEX {
        claim_other3_roster(buf, sink);
    }
}

/// The `OTHER3` dev module's 81-record selection roster (PROT `0974`).
///
/// Three quarters of that entry is one fixed-stride table of NUL-padded
/// labels, and a shape test can only call the whole thing `ascii_text` - the
/// stride is in the drawing loop's index arithmetic, not in the bytes
/// ([`crate::other3_roster`]). Claiming each record at the stride covers its
/// padding too, because the stride is what the loop advances by.
fn claim_other3_roster(buf: &[u8], sink: &mut Sink) {
    use crate::other3_roster as roster;
    if roster::records(buf).is_none() {
        sink.note("no OTHER3 roster at the pinned offset in PROT 0974");
        return;
    }
    for i in 0..roster::RECORD_COUNT {
        let Some((off, len)) = roster::record_extent(i) else {
            break;
        };
        sink.claim(
            off,
            (off + len).min(buf.len()),
            OWNER_STRING,
            format!("OTHER3 roster label {i} (other3_roster)"),
        );
    }
    sink.note(format!(
        "{} roster labels on a {:#x} stride at {:#010x} (other3_roster)",
        roster::RECORD_COUNT,
        roster::RECORD_STRIDE,
        roster::ROSTER_VA
    ));
}

/// PROT index of the STR/MDEC cutscene overlay.
const STR_OVERLAY_PROT_INDEX: u32 = 970;

/// The STR/MDEC overlay's two data-segment structures: the per-`fmv_id`
/// dispatch table with the movie paths it points at, and the compressed blob
/// the VLC lookup table is unpacked from.
///
/// Both are consumed by modules in this workspace at pinned constants
/// ([`crate::fmv_dispatch`], `legaia_mdec::strv2_table`) and neither was
/// claimed: the dispatch table and its path strings read as `plausible_mips` /
/// `ascii_text` residue, and the blob - the second-largest overlay residue run
/// on the disc - read as `mixed`, which is what a compressed stream looks like
/// to a shape test.
///
/// The blob's extent is **measured**, not assumed: `unpack_lz_tracked` walks
/// the control bytes to the `0xFF 0xFF` terminator and reports what it
/// consumed, the same way [`take_lzs`] measures an LZS span. What is left of
/// the entry's last sector past that terminator is the builder's sector
/// buffer, and takes the same one-sector-wide slack rule the streaming classes
/// take.
fn claim_str_overlay_tables(buf: &[u8], sink: &mut Sink) {
    use crate::fmv_dispatch as fmv;
    use legaia_mdec::strv2_table as vlc;

    let base = fmv::STR_OVERLAY_BASE_VA;
    let table_off = (fmv::FMV_TABLE_VA - base) as usize;
    let table_len = fmv::FMV_SLOT_COUNT * fmv::SLOT_STRIDE;
    match fmv::FmvTable::from_str_overlay(buf) {
        Some(_) => {
            sink.claim(
                table_off,
                (table_off + table_len).min(buf.len()),
                OWNER_TOC,
                format!(
                    "FMV dispatch table, {} x {} bytes (fmv_dispatch)",
                    fmv::FMV_SLOT_COUNT,
                    fmv::SLOT_STRIDE
                ),
            );
            // Each slot's `+0x00` is a pointer to its ISO9660 movie path; the
            // strings sit in the image's own head pool, so the extents come
            // from the pointers plus a NUL scan rather than from a table.
            for i in 0..fmv::FMV_SLOT_COUNT {
                let at = table_off + i * fmv::SLOT_STRIDE;
                let Some(w) = buf.get(at..at + 4) else { break };
                let ptr = u32::from_le_bytes(w.try_into().unwrap());
                let Some(off) = ptr.checked_sub(base).map(|o| o as usize) else {
                    continue;
                };
                let Some(tail) = buf.get(off..) else { continue };
                let Some(len) = tail.iter().position(|&b| b == 0) else {
                    continue;
                };
                sink.claim(
                    off,
                    off + len + 1,
                    OWNER_STRING,
                    format!("movie path for fmv_id {i} (fmv_dispatch)"),
                );
            }
        }
        None => sink.note("no FMV dispatch table at the pinned offset in PROT 0970"),
    }

    // The two MDEC command packets the table upload sends, each checked by
    // its own header word before it is claimed.
    for (va, header, what) in [
        (
            fmv::MDEC_QUANT_PACKET_VA,
            fmv::MDEC_QUANT_PACKET_HEADER,
            "MDEC quant-table packet: header + luma + chroma matrices (fmv_dispatch)",
        ),
        (
            fmv::MDEC_IDCT_PACKET_VA,
            fmv::MDEC_IDCT_PACKET_HEADER,
            "MDEC IDCT-table packet: header + 64-halfword matrix (fmv_dispatch)",
        ),
    ] {
        let off = (va - base) as usize;
        if legaia_bytes::u32_le(buf, off) == Some(header)
            && off + fmv::MDEC_PACKET_BYTES <= buf.len()
        {
            sink.claim(off, off + fmv::MDEC_PACKET_BYTES, OWNER_RECORD, what);
        } else {
            sink.note(format!(
                "no MDEC packet header {header:#010x} at {va:#010x}"
            ));
        }
    }

    let src = (vlc::STRV2_PACKED_VA - base) as usize;
    match buf.get(src..).map(vlc::unpack_lz_tracked) {
        Some(Ok((table, consumed))) => {
            let end = (src + consumed).min(buf.len());
            sink.claim(
                src,
                end,
                OWNER_LZS,
                format!(
                    "STRv2 VLC table source, mode-switched LZ77 -> {} bytes at {:#010x} \
                     (legaia_mdec::strv2_table, FUN_801f1a00)",
                    table.len(),
                    vlc::STRV2_TABLE_VA
                ),
            );
            // Past the terminator, inside the entry's last sector: the
            // builder's buffer, the same slack the streaming classes declare.
            claim_last_sector_slack(buf, sink, end, "slack past the VLC blob terminator");
        }
        _ => sink.note("the VLC blob at the pinned offset does not terminate"),
    }
}

/// The battle overlay's head: twenty-two `switch` jump tables and the C
/// strings in front of them, each bound to the instruction pair that forms its
/// address ([`crate::battle_jump_tables`]).
///
/// A table's extent is its consumer's `sltiu` bound times four - read off the
/// dispatch, not scanned out of the bytes - so the claims are structural. They
/// are made only when [`crate::battle_jump_tables::check`] re-derives every row
/// from this image's own instructions; an image that disagrees gets a note and
/// no claim.
fn claim_battle_jump_tables(buf: &[u8], sink: &mut Sink) {
    use crate::battle_jump_tables as bjt;
    let errs = bjt::check(buf);
    if !errs.is_empty() {
        sink.note(format!(
            "battle jump tables not claimed: {} row(s) disagree with this image ({})",
            errs.len(),
            errs[0]
        ));
        return;
    }
    for t in &bjt::JUMP_TABLES {
        sink.claim(
            t.offset(),
            t.offset() + t.byte_len(),
            OWNER_TOC,
            format!(
                "jump table, {} arms on {} (jr {:#010x})",
                t.arms, t.index, t.jr
            ),
        );
    }
    for s in &bjt::HEAD_STRINGS {
        let off = s.offset();
        if let Some(len) = buf.get(off..).and_then(|t| t.iter().position(|&b| b == 0)) {
            sink.claim(
                off,
                off + len + 1,
                OWNER_STRING,
                format!("head string, address formed at {:#010x}", s.site),
            );
        }
    }
}

/// The battle overlay's NUL-terminated UI strings, whose extents no fixed
/// `count * stride` row can express.
///
/// Every address here is a `pub const` (or a pointer read from one), so this is
/// the same kind of binding as [`pinned_overlay_tables`] - what differs is only
/// that a C string's length is in its own bytes rather than in a table, so the
/// extent is NUL-scanned from the pinned start instead of computed. Without
/// that these read as `ascii_text` residue while the parser that consumes them
/// names the exact byte they start at.
fn claim_battle_overlay_strings(buf: &[u8], sink: &mut Sink) {
    use crate::{battle_ui_strings as bui, muscle_dome as dome};
    let base = bui::OVERLAY_BASE_VA;
    let claim_cstr = |off: usize, what: String, sink: &mut Sink| {
        let Some(tail) = buf.get(off..) else { return };
        // A string with no terminator in the image is not a string - claim
        // nothing rather than run to the end of the entry.
        let Some(len) = tail.iter().position(|&b| b == 0) else {
            sink.note(format!("{what}: no NUL terminator at {off:#x}"));
            return;
        };
        sink.claim(off, off + len + 1, OWNER_STRING, what);
    };
    for (va, label) in bui::OVERLAY_LABELS {
        let Some(off) = va.checked_sub(base).map(|o| o as usize) else {
            continue;
        };
        claim_cstr(
            off,
            format!("battle UI label {label:?} (battle_ui_strings)"),
            sink,
        );
    }
    for (i, off) in dome::victory_message_offsets(buf).into_iter().enumerate() {
        claim_cstr(
            off,
            format!("muscle-dome victory message {i} (muscle_dome)"),
            sink,
        );
    }
}

/// The battle overlay's **effect-prototype record pool** - the bytes the
/// `0x801F6324` pointer table points INTO, as distinct from the table itself.
///
/// The table has a pinned constant and a row in [`pinned_overlay_tables`], so
/// the 61 pointers were credited while the 54 unique records they name were
/// not: the pool read as one unbroken `low_entropy` residue run, which is how a
/// fully decoded structure looks when only its index is claimed.
/// [`crate::move_power::parse_effect_proto_records`] already decodes it to
/// `[i16 model_sel][u16 reserved][move-VM bytecode]` part records
/// ([`move-power.md`](../../../docs/formats/move-power.md)); this walks the same
/// offsets and claims each record's extent.
///
/// A record ends where the next one begins - the pool is packed, with the last
/// record bounded by the table itself rather than by the end of the entry, which
/// is the one place `parse_records_at`'s generic bound is too generous for a
/// byte claim.
fn claim_effect_proto_records(buf: &[u8], sink: &mut Sink) {
    use crate::move_power as mp;
    let Some(aux) = mp::EffectAuxTables::parse(buf) else {
        sink.note("no effect-prototype table: PROT 0898 structural guard failed");
        return;
    };
    let table = mp::EFFECT_PROTO_TABLE_FILE_OFFSET;
    let mut offs: Vec<usize> = (0..mp::EFFECT_AUX_TABLE_LEN as u8)
        .filter_map(|i| aux.proto_record_offset(i))
        .filter(|&f| f + 4 <= table)
        .collect();
    offs.sort_unstable();
    offs.dedup();
    for (i, &f) in offs.iter().enumerate() {
        let end = offs.get(i + 1).copied().unwrap_or(table);
        let model_sel = i16::from_le_bytes([buf[f], buf[f + 1]]);
        sink.claim(
            f,
            end,
            OWNER_RECORD,
            format!("move-FX part record, model_sel {model_sel} (move_power)"),
        );
    }
    sink.note(format!(
        "{} unique effect-prototype record(s) behind the {}-entry \
         0x801F6324 table (move_power::parse_effect_proto_records)",
        offs.len(),
        mp::EFFECT_AUX_TABLE_LEN
    ));
}

/// Data-segment tables an overlay image carries at an offset a parser in this
/// workspace already reads, with the extent that parser's own `count * stride`.
///
/// Every row is a **binding**, not a discovery: each offset and each length is
/// a `pub const` of the module named in the detail, so nothing here is a new
/// claim about the disc and nothing here can be tuned to buy percentage points
/// - widening a row means widening the parser that reads it. The rows are
///   asserted against the parsers' constants by the disc-gated tests, so a
///   parser that re-pins a table moves this table with it or the test fails.
///
/// What this closes is the gap the sweep kept reporting as unwalked format: an
/// overlay's code is credited from the dump corpus and its data segment from
/// nothing, so a table with a named constant and a decoded record layout ranked
/// beside a format nobody had opened.
pub fn pinned_overlay_tables(prot_index: u32) -> Vec<(usize, usize, &'static str, &'static str)> {
    use crate::{
        baka_opponents as baka, battle_attack_camera_table as atkcam, battle_camera_table as camh,
        battle_ui_strings as bui, dance_art, dance_cast, dance_chart, element_affinity as elem,
        menu_windows as menu, minigame_art as art, minigame_slot_scene as slot, move_power as mp,
        muscle_dome as dome, seru_side_effect as seru, slot_payout as payout,
    };
    const SLOT_A: u32 = 0x801C_E818;
    let at = |va: u32| (va - SLOT_A) as usize;
    match prot_index {
        898 => vec![
            (
                at(dome::DECK_TABLE_VA),
                dome::HAND_SLOTS,
                OWNER_RECORD,
                "muscle-dome deck move-index table (muscle_dome)",
            ),
            (
                at(dome::HAND_SPRITE_TABLE_VA),
                dome::HAND_SLOTS,
                OWNER_RECORD,
                "muscle-dome hand sprite-id table (muscle_dome)",
            ),
            (
                at(bui::RASERU_LABEL_TABLE_VA),
                (bui::RASERU_LABEL_MAX as usize + 1) * bui::RASERU_LABEL_STRIDE as usize,
                OWNER_STRING,
                "Ra-Seru magic-command labels (battle_ui_strings)",
            ),
            (
                camh::CAMERA_HEIGHT_FILE_OFFSET,
                camh::CAMERA_HEIGHT_LEN * 2,
                OWNER_RECORD,
                "battle camera-height table (battle_camera_table)",
            ),
            (
                at(dome::SUBDRAW_PTR_TABLE_VA),
                dome::SUBDRAW_PTR_TABLE_LEN * 4,
                OWNER_TOC,
                "muscle-dome sub-draw record pointers (muscle_dome)",
            ),
            (
                at(dome::VICTORY_MSG_TABLE_VA),
                dome::VICTORY_MSG_TABLE_LEN * 4,
                OWNER_TOC,
                "muscle-dome victory-message pointers (muscle_dome)",
            ),
            (
                atkcam::ATTACK_CAMERA_FILE_OFFSET,
                atkcam::ATTACK_CAMERA_LEN,
                OWNER_RECORD,
                "per-art attack-camera tracks (battle_attack_camera_table)",
            ),
            (
                mp::MOVE_ID_INDEX_MAP_FILE_OFFSET,
                mp::MOVE_ID_INDEX_MAP_LEN,
                OWNER_RECORD,
                "move-id to record-index map (move_power)",
            ),
            (
                mp::MOVE_POWER_TABLE_FILE_OFFSET,
                mp::MOVE_POWER_TABLE_LEN * mp::MOVE_POWER_RECORD_STRIDE,
                OWNER_RECORD,
                "move power + behaviour table (move_power)",
            ),
            (
                mp::IMPACT_EFFECT_TABLE_FILE_OFFSET,
                mp::IMPACT_EFFECT_TABLE_LEN * 4,
                OWNER_RECORD,
                "impact-effect config table (move_power)",
            ),
            (
                elem::AFFINITY_MATRIX_FILE_OFFSET,
                elem::ELEMENT_COUNT * elem::ELEMENT_COUNT,
                OWNER_RECORD,
                "element-affinity matrix (element_affinity)",
            ),
            (
                elem::SUMMON_POWER_PCT_FILE_OFFSET,
                elem::SUMMON_POWER_PCT_ROWS * elem::ELEMENT_COUNT,
                OWNER_RECORD,
                "summon power-percent table (element_affinity)",
            ),
            (
                elem::CHARACTER_ELEMENTS_FILE_OFFSET,
                elem::CHARACTER_ELEMENTS_LEN,
                OWNER_RECORD,
                "per-character element table (element_affinity)",
            ),
            (
                mp::EFFECT_PROTO_TABLE_FILE_OFFSET,
                mp::EFFECT_AUX_TABLE_LEN * 4,
                OWNER_TOC,
                "move effect-prototype pointers (move_power)",
            ),
            (
                mp::EFFECT_CLUT_TABLE_FILE_OFFSET,
                mp::EFFECT_AUX_TABLE_LEN,
                OWNER_RECORD,
                "move effect CLUT ids (move_power)",
            ),
            (
                mp::CUE_GROUP_TABLE_FILE_OFFSET,
                mp::CUE_GROUP_TABLE_LEN * mp::CUE_GROUP_STRIDE,
                OWNER_RECORD,
                "move cue-group table (move_power)",
            ),
            (
                seru::SIDE_EFFECT_TABLE_FILE_OFFSET,
                seru::SIDE_EFFECT_ELEMENTS
                    * seru::SIDE_EFFECT_BANDS
                    * seru::SIDE_EFFECT_RECORD_STRIDE,
                OWNER_RECORD,
                "Seru-magic side-effect table (seru_side_effect)",
            ),
        ],
        899 => vec![
            (
                menu::EQUIP_BROWSE_MAP_OFFSET,
                menu::EQUIP_BROWSE_MAP_LEN,
                OWNER_RECORD,
                "equip browse-row to equip-byte map (menu_windows)",
            ),
            (
                menu::EQUIP_BROWSE_MAP_OFFSET + menu::EQUIP_BROWSE_MAP_LEN,
                1,
                OWNER_PAD,
                "pad byte between the browse map and the equip mask",
            ),
            (
                menu::CHARACTER_EQUIP_MASK_OFFSET,
                menu::CHARACTER_EQUIP_MASK_LEN,
                OWNER_RECORD,
                "per-character equip mask (menu_windows)",
            ),
            (
                menu::SLOT_PICTOGRAM_OFFSET,
                menu::SLOT_PICTOGRAM_LEN * 2,
                OWNER_RECORD,
                "equip slot pictogram ids (menu_windows)",
            ),
            (
                menu::MENU_WINDOW_TABLE_OFFSET,
                menu::MENU_WINDOW_COUNT * menu::MENU_WINDOW_RECORD_STRIDE,
                OWNER_RECORD,
                "pause-menu window descriptor table (menu_windows)",
            ),
            (
                menu::OPTIONS_LAYOUT_OFFSET,
                menu::OPTIONS_LAYOUT_ROWS * menu::OPTIONS_LAYOUT_STRIDE,
                OWNER_RECORD,
                "options display-layout table (menu_windows)",
            ),
            (
                menu::PRIZE_TABLE_OFFSET,
                menu::PRIZE_TABLE_BLOCKS * menu::PRIZE_BLOCK_BYTES,
                OWNER_RECORD,
                "casino prize table (menu_windows)",
            ),
        ],
        975 => vec![
            (
                slot::MESSAGE_TABLE_OFFSET,
                slot::MESSAGE_COUNT * slot::MESSAGE_STRIDE,
                OWNER_RECORD,
                "slot-machine message table (minigame_slot_scene)",
            ),
            (
                payout::SLOT_PAYOUT_FILE_OFFSET,
                payout::SLOT_SYMBOL_COUNT,
                OWNER_RECORD,
                "per-symbol payout ladder (slot_payout)",
            ),
            (
                slot::PAYLINE_TABLE_OFFSET,
                slot::PAYLINE_COUNT * 16,
                OWNER_RECORD,
                "payline geometry table (minigame_slot_scene)",
            ),
            (
                slot::MARQUEE_TABLE_OFFSET,
                slot::MARQUEE_COUNT * 16,
                OWNER_RECORD,
                "marquee cell table (minigame_slot_scene)",
            ),
            (
                slot::MEDALLION_TABLE_OFFSET,
                slot::LAMP_COUNT * slot::LAMP_RECORD_STRIDE,
                OWNER_RECORD,
                "payline medallion positions (minigame_slot_scene)",
            ),
            (
                slot::LAMP_TABLE_OFFSET,
                slot::LAMP_COUNT * slot::LAMP_RECORD_STRIDE,
                OWNER_RECORD,
                "payline lamp positions (minigame_slot_scene)",
            ),
            (
                art::SLOT_HUD_TABLE_OFFSET,
                art::SLOT_HUD_RECORDS * art::SLOT_HUD_STRIDE,
                OWNER_RECORD,
                "slot-machine HUD sprite records (minigame_art)",
            ),
        ],
        976 => vec![
            (
                baka::HUD_WIDGET_TABLE_FILE_OFFSET,
                baka::HUD_WIDGET_COUNT * baka::HUD_WIDGET_STRIDE,
                OWNER_RECORD,
                "Baka Fighter HUD widget table (baka_opponents)",
            ),
            (
                baka::ACTOR_PROTOTYPE_TABLE_FILE_OFFSET,
                baka::ACTOR_PROTOTYPE_COUNT * baka::ACTOR_PROTOTYPE_STRIDE,
                OWNER_RECORD,
                "Baka Fighter actor prototypes (baka_opponents)",
            ),
            (
                baka::OPPONENT_TABLE_FILE_OFFSET,
                baka::OPPONENT_COUNT * baka::OPPONENT_RECORD_STRIDE,
                OWNER_RECORD,
                "Baka Fighter opponent roster (baka_opponents)",
            ),
            (
                (baka::ACTION_PTR_TABLE_VA - SLOT_A) as usize,
                baka::OPPONENT_COUNT * 4,
                OWNER_TOC,
                "Baka Fighter per-fighter action-table pointers (baka_opponents)",
            ),
            (
                baka::BLIT_RECT_TABLE_FILE_OFFSET,
                baka::BLIT_RECT_COUNT * baka::BLIT_RECT_STRIDE,
                OWNER_RECORD,
                "Baka Fighter blit source rects (baka_opponents)",
            ),
        ],
        980 => vec![
            (
                (dance_art::WIDGET_TABLE_VA - SLOT_A) as usize,
                dance_art::WIDGET_COUNT * dance_art::WIDGET_STRIDE,
                OWNER_RECORD,
                "dance widget table (dance_art)",
            ),
            (
                (dance_cast::KIND_TABLE_VA - SLOT_A) as usize,
                dance_cast::KIND_COUNT * dance_cast::KIND_STRIDE,
                OWNER_RECORD,
                "dance cast kind table (dance_cast)",
            ),
            (
                dance_chart::DANCE_CHART_FILE_OFFSET,
                dance_chart::DANCE_CHART_ROWS * dance_chart::BEATS_PER_ROW,
                OWNER_RECORD,
                "dance step chart (dance_chart)",
            ),
        ],
        _ => Vec::new(),
    }
}

/// A slot-B module image: the dump corpus for its code, plus the image's own
/// structural regions - the head jump table and the spawn-record band.
///
/// The record claims need no dump directory: both ends of every one of them are
/// addresses the module's own code computes and hands to `FUN_80021B04` /
/// `FUN_80050ED4`. See [`crate::slot_b_module`] and
/// [`docs/formats/slot-b-module-layout.md`](https://andrewaltimit.github.io/legend-of-legaia-re/formats/slot-b-module-layout.html).
fn walk_slot_b_module(buf: &[u8], sink: &mut Sink, opts: &AccountOptions) {
    // The record walk is cut at the inherited tail for the same reason the
    // band-level measurement is: a spawn call site up there belongs to the
    // donor whose bytes those are, and so does the record pointer it forms.
    let layout =
        crate::slot_b_module::parse_with_tail(buf, base_for(opts), inherited_tail_start(buf, opts));
    if let Some(h) = layout.head_table.clone() {
        sink.claim(
            h.start,
            h.end,
            OWNER_TOC,
            format!("head jump table, {} arms", (h.end - h.start) / 4),
        );
    }
    for (i, r) in layout.records.iter().enumerate() {
        sink.claim(
            r.start,
            r.end,
            OWNER_RECORD,
            format!("spawn record {i} (model_sel {})", r.model_sel),
        );
    }
    // Records above the highest consumer-credited one. Their starts come from
    // the move-VM program walk rather than from a pointer, so the reason line
    // says which evidence the claim rests on.
    for (i, r) in layout.chained_records.iter().enumerate() {
        sink.claim(
            r.start,
            r.end,
            OWNER_RECORD,
            format!(
                "spawn record {} (model_sel {}, chained from the record below by \
                 its program's terminator)",
                layout.records.len() + i,
                r.model_sel
            ),
        );
    }
    sink.note(format!(
        "slot-B module: {} framed functions, code ends at {:#x}; {} spawn sites, \
         {} records claimed ({} bytes) + {} chained ({} bytes){}",
        layout.functions.len(),
        layout.code_end(),
        layout.spawn_sites,
        layout.records.len(),
        layout.record_bytes(),
        layout.chained_records.len(),
        layout
            .chained_records
            .iter()
            .map(crate::slot_b_module::RecordSpan::len)
            .sum::<usize>(),
        match layout.unbounded_record {
            Some(o) => format!(
                "; the highest record at {o:#x} has no boundary above it and \
                 its program does not terminate, so it stays residue"
            ),
            None => String::new(),
        }
    ));
    // The dump corpus is the parser for the code half, and it is optional here
    // - `--funcs` absent still gives the structural regions.
    if opts.funcs_dir.is_some() {
        walk_overlay_code(buf, sink, opts);
    }
}

// --- fallback --------------------------------------------------------------

fn walk_generic(buf: &[u8], sink: &mut Sink) {
    // Nothing structural. The magic sweep in `finish` is the only claim
    // source, and it is tagged `scan` so it never inflates `structural`.
    sink.note(format!(
        "no structural walker for this class; entropy {:.2} bits/byte",
        entropy_bits(buf)
    ));
}

// ---------------------------------------------------------------------------
// Driver
// ---------------------------------------------------------------------------

/// PROT extraction index of the monster archive. It classifies as a generic
/// `overlay_data_blob` (its head is a `dec_size` word, not a magic), so the
/// walker is selected by index rather than by class.
pub const MONSTER_ARCHIVE_PROT_INDEX: u32 = 867;

/// PROT extraction index of the memory-card screen's kanji-font pack
/// (`card_data`). It classifies as `data_field_truncated` because its
/// [`crate::pack`] header words decode as three tiny streaming chunks, so the
/// walker is selected by index rather than by class - see
/// [`docs/formats/data-field.md`](https://andrewaltimit.github.io/legend-of-legaia-re/formats/data-field.html).
pub const CARD_FONT_PROT_INDEX: u32 = 892;

/// Choose the walker for a buffer.
///
/// Class first, then the index-keyed overrides: the monster archive (no
/// detector fires on it), the card font pack (the wrong detector fires on it),
/// and any entry with a `static-overlays.toml` row plus a dump directory (a
/// code image, whose "parser" is the dump corpus).
pub fn pick_walker(buf: &[u8], class: Class, opts: &AccountOptions) -> Walker {
    if opts.prot_index == Some(MONSTER_ARCHIVE_PROT_INDEX)
        && buf.len() >= crate::monster_archive::SLOT_STRIDE
    {
        return Walker::MonsterArchive;
    }
    if opts.prot_index == Some(CARD_FONT_PROT_INDEX) {
        return Walker::CardFontPack;
    }
    // The two stills carry no magic - only a length and an index. Selecting
    // them on the index is not a shortcut: nothing in the bytes distinguishes
    // a still from any other 16bpp region, and the rectangle that gives them
    // their shape lives in the consumer's code.
    if opts
        .prot_index
        .is_some_and(crate::ringside_still::is_ringside_still)
    {
        return Walker::RingsideStill;
    }
    // Every mapped image at the slot-B link base, not just the 0903..=0966 cast
    // band: the walk's three regions are recovered by resolving words against
    // that base, so the base is what makes the walk apply (see
    // `slot_b_module::is_slot_b_image`). Its structural regions come out of the
    // image, so the walker runs with or without a dump directory (it delegates
    // to the code walker when one is given).
    if opts
        .prot_index
        .is_some_and(crate::slot_b_module::is_slot_b_image)
    {
        return Walker::SlotBModule;
    }
    // An entry can be a runtime overlay AND a container. Where the class
    // walker has structural claims of its own, it owns the entry and
    // delegates to the code walker itself (`walk_init_pak`, `walk_slot_b_module`).
    let composes_code_walk = class == Class::InitPak;
    if let (Some(idx), Some(_), false) =
        (opts.prot_index, opts.funcs_dir.as_ref(), composes_code_walk)
    {
        let is_overlay = crate::static_overlay::overlay_map()
            .overlays
            .iter()
            .any(|r| r.prot_index == idx);
        if is_overlay {
            return Walker::OverlayCode;
        }
    }
    match class {
        Class::SceneAssetTable | Class::SceneScriptedAssetTable => Walker::SceneAssetTable,
        Class::LzsContainer => Walker::DescriptorBundle,
        Class::PochiFiller => Walker::PochiFiller,
        Class::DataFieldStreaming
        | Class::DataFieldTruncated
        | Class::SceneVabStream
        | Class::SceneTmdStream
        | Class::TmdSizePrefix => Walker::Stream,
        Class::SummonReadef => Walker::SummonReadef,
        Class::BattleDataPack => Walker::BattleDataPack,
        Class::BseBank => Walker::BseBank,
        Class::InitPak => Walker::InitPak,
        Class::FieldMap => Walker::FieldMap,
        Class::SceneV12Table => Walker::SceneV12,
        Class::SceneEventScripts => Walker::SceneEventScripts,
        Class::EffectBundle => Walker::EffectBundle,
        Class::EfectPack => Walker::EfectPack,
        Class::TimPack => Walker::TimPack,
        Class::Pack => Walker::Pack,
        Class::TimPassthrough => Walker::Tim,
        Class::VabMultiBank => Walker::VabMultiBank,
        Class::SeqContainer => Walker::Seq,
        Class::AnmContainer => Walker::Anm,
        Class::MipsOverlay | Class::OverlayPtrTable => Walker::OverlayCode,
        // Last resort before the magic sweep: a buffer whose own bytes walk to
        // a DATA_FIELD terminator IS a chunk stream, whatever class fired on
        // it. One retail entry (`1062`, a standalone BGM SEQ behind a single
        // `(type << 24) | len` header) reaches this arm; the rest of the
        // `Generic` population is all-zero filler and the un-based `0896`,
        // neither of which walks.
        _ if walks_as_chunk_stream(buf) => Walker::Stream,
        _ => Walker::Generic,
    }
}

/// Does this buffer walk to a DATA_FIELD stream terminator?
///
/// The test is the walk itself, not a magic: every chunk header's payload must
/// fit inside the buffer, the walk must reach a zero-size header, and it must
/// have consumed at least one chunk (an all-zero buffer terminates on its first
/// word without consuming anything). Sector padding past the terminator is
/// allowed - that is what the rest of the last sector always is.
fn walks_as_chunk_stream(buf: &[u8]) -> bool {
    let Ok(rep) = parse_streaming(buf, 64) else {
        return false;
    };
    rep.terminated && !rep.chunks.is_empty() && rep.bytes_consumed <= buf.len()
}

fn dispatch(buf: &[u8], walker: Walker, sink: &mut Sink, opts: &AccountOptions, depth: u8) {
    claim_inherited_tail(buf, sink, opts);
    match walker {
        Walker::SceneAssetTable => walk_scene_asset_table(buf, sink, opts, depth),
        Walker::DescriptorBundle => walk_descriptor_bundle(buf, sink, opts, depth),
        Walker::PochiFiller => walk_pochi_filler(buf, sink),
        Walker::RingsideStill => walk_ringside_still(buf, sink),
        Walker::Stream => walk_stream(buf, sink, opts, depth),
        Walker::MonsterArchive => walk_monster_archive(buf, sink, opts, depth),
        Walker::MonsterBlock => walk_monster_block(buf, sink),
        Walker::SummonReadef => walk_summon_readef(buf, sink, opts, depth),
        Walker::BattleDataPack => walk_battle_data_pack(buf, sink, opts, depth),
        Walker::MeArchive => walk_me_archive_at(buf, 0, sink),
        Walker::BseBank => walk_bse_bank(buf, sink),
        Walker::InitPak => walk_init_pak(buf, sink, opts, depth),
        Walker::FieldMap => walk_field_map(buf, sink),
        Walker::SceneV12 => walk_scene_v12(buf, sink),
        Walker::SceneEventScripts => walk_scene_event_scripts(buf, sink),
        Walker::EffectBundle => walk_effect_bundle(buf, sink),
        Walker::EfectPack => walk_efect_dat(buf, sink),
        Walker::Pack => walk_pack(buf, sink),
        Walker::CardFontPack => walk_card_font_pack(buf, sink),
        Walker::TimPack => walk_tim_pack(buf, sink),
        Walker::ClipBank => walk_clip_bank(buf, sink),
        Walker::OffsetPack => {
            walk_offset_pack(buf, sink, OWNER_RECORD, "member");
        }
        Walker::Mes => walk_mes(buf, sink),
        Walker::Tim => walk_tim(buf, sink),
        Walker::Tmd => walk_tmd(buf, sink),
        Walker::Vab => walk_vab(buf, sink),
        Walker::VabMultiBank => walk_vab_multi_bank(buf, sink),
        Walker::Seq => walk_seq(buf, sink),
        Walker::Anm => walk_anm(buf, sink),
        Walker::Man => walk_man(buf, sink),
        Walker::OverlayCode => walk_overlay_code(buf, sink, opts),
        Walker::SlotBModule => walk_slot_b_module(buf, sink, opts),
        Walker::Generic => walk_generic(buf, sink),
    }
}

/// Longest residue run the magic sweep will look inside. A sweep over a
/// multi-megabyte run of zeros finds nothing and costs the whole run.
const RESCAN_MIN: usize = 64;

fn rescan(buf: &[u8], residue: &[(usize, usize)], sink: &mut Sink) {
    for &(a, b) in residue {
        if b - a < RESCAN_MIN {
            continue;
        }
        let run = &buf[a..b];
        if run.iter().all(|&x| x == 0) {
            continue;
        }
        for h in crate::tim_scan::scan_buffer(run) {
            sink.claim(
                a + h.offset,
                a + h.offset + h.byte_len,
                OWNER_SCAN,
                format!("TIM {}x{} {}bpp", h.width, h.height, h.bpp),
            );
        }
        for h in crate::tmd_scan::scan_buffer(run) {
            sink.claim(
                a + h.offset,
                a + h.offset + h.byte_len,
                OWNER_SCAN,
                format!("TMD, {} objects", h.n_obj),
            );
        }
        for i in 0..run.len().saturating_sub(4) {
            if &run[i..i + 4] == b"pQES" {
                if let Some(n) = seq_extent(run, i) {
                    sink.claim(a + i, a + i + n, OWNER_SCAN, "SEQ");
                }
            } else if legaia_bytes::u32_le(run, i) == Some(legaia_vab::VAB_MAGIC)
                && let Ok(h) = legaia_vab::parse_header(run, i)
            {
                sink.claim(a + i, a + i + h.fsize as usize, OWNER_SCAN, "VAB");
            }
        }
    }
}

fn run(
    buf: &[u8],
    label: String,
    class: String,
    walker: Walker,
    opts: &AccountOptions,
    depth: u8,
) -> Account {
    let mut sink = Sink::new();
    dispatch(buf, walker, &mut sink, opts, depth);
    if depth == opts.depth {
        claim_buffer_slack(buf, &mut sink, opts);
    }

    let size = buf.len();
    let mut merged = merge_ranges(&sink.claims, size);
    if opts.rescan {
        let gaps = complement(&merged, size);
        rescan(buf, &gaps, &mut sink);
        merged = merge_ranges(&sink.claims, size);
    }
    let accounted: usize = merged.iter().map(|(a, b)| b - a).sum();
    let structural_claims: Vec<Claim> = sink
        .claims
        .iter()
        .filter(|c| c.owner != OWNER_SCAN)
        .cloned()
        .collect();
    let structural: usize = merge_ranges(&structural_claims, size)
        .iter()
        .map(|(a, b)| b - a)
        .sum();

    // Per-owner bytes are MERGED within the owner, not a sum of claim lengths.
    // Claims overlap - several dumps can state the same extent, and a slot's
    // declared footprint covers its own compressed stream - so a raw sum runs
    // past the buffer and reads as a coverage figure it is not.
    let mut owner_claims: BTreeMap<&'static str, Vec<Claim>> = BTreeMap::new();
    for c in &sink.claims {
        owner_claims.entry(c.owner).or_default().push(c.clone());
    }
    let mut by_owner: Vec<OwnerTotal> = owner_claims
        .into_iter()
        .map(|(owner, cs)| OwnerTotal {
            owner: owner.to_string(),
            claims: cs.len(),
            bytes: merge_ranges(&cs, size).iter().map(|(a, b)| b - a).sum(),
        })
        .collect();
    by_owner.sort_by_key(|o| std::cmp::Reverse(o.bytes));

    let gaps = split_off_fill(buf, complement(&merged, size));
    let mut runs: Vec<Residue> = gaps
        .iter()
        .map(|&(a, b)| {
            let s = &buf[a..b];
            Residue {
                start: a,
                end: b,
                len: b - a,
                shape: classify_residue(s),
                entropy_bits: entropy_bits(s),
                zero_fraction: zero_fraction(s),
                head: hex_head(s, 16),
            }
        })
        .collect();
    let mut by_shape: BTreeMap<&'static str, (usize, usize)> = BTreeMap::new();
    for r in &runs {
        let e = by_shape.entry(r.shape.name()).or_insert((0, 0));
        e.0 += 1;
        e.1 += r.len;
    }
    let mut by_shape: Vec<ShapeTotal> = by_shape
        .into_iter()
        .map(|(shape, (n, bytes))| ShapeTotal {
            shape: shape.to_string(),
            runs: n,
            bytes,
        })
        .collect();
    by_shape.sort_by_key(|s| std::cmp::Reverse(s.bytes));

    let residue_runs = runs.len();
    let residue_bytes: usize = runs.iter().map(|r| r.len).sum();
    runs.retain(|r| r.len >= opts.min_residue);
    runs.sort_by_key(|r| std::cmp::Reverse(r.len));
    runs.truncate(64);

    let mut claims = if opts.keep_claims {
        sink.claims.clone()
    } else {
        Vec::new()
    };
    claims.sort_by_key(|c| std::cmp::Reverse(c.len()));
    claims.truncate(256);

    let pct = |n: usize| {
        if size == 0 {
            0.0
        } else {
            100.0 * n as f64 / size as f64
        }
    };

    Account {
        label,
        size,
        class,
        walker,
        accounted,
        structural,
        accounted_pct: pct(accounted),
        structural_pct: pct(structural),
        residue_bytes,
        by_owner,
        by_shape,
        residue: runs,
        residue_runs,
        ambiguous_dumps: sink.ambiguous_dumps,
        refuted_dumps: sink.refuted_dumps,
        notes: sink.notes,
        nested: sink.nested,
        claims,
    }
}

/// Account one buffer's bytes.
pub fn account(buf: &[u8], opts: &AccountOptions) -> Account {
    let report = classify(buf);
    let walker = pick_walker(buf, report.class, opts);
    let label = if opts.label.is_empty() {
        "<buffer>".to_string()
    } else {
        opts.label.clone()
    };
    run(
        buf,
        label,
        report.class.name().to_string(),
        walker,
        opts,
        opts.depth,
    )
}

/// PROT extraction index from an extracted filename (`0867_battle_data.BIN`).
pub fn prot_index_from_name(name: &str) -> Option<u32> {
    let head: String = name.chars().take_while(|c| c.is_ascii_digit()).collect();
    (head.len() == 4).then(|| head.parse().ok()).flatten()
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

fn commas(n: usize) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// Human-readable report, one entry per call.
pub fn render_text(acc: &Account, indent: usize) -> String {
    let pad = " ".repeat(indent);
    let mut s = String::new();
    s.push_str(&format!(
        "{pad}{}  {} bytes  class={} walker={}\n",
        acc.label,
        commas(acc.size),
        acc.class,
        acc.walker.name()
    ));
    s.push_str(&format!(
        "{pad}  accounted {} ({:.1}%)   structural {} ({:.1}%)   residue {} in {} runs\n",
        commas(acc.accounted),
        acc.accounted_pct,
        commas(acc.structural),
        acc.structural_pct,
        commas(acc.residue_bytes),
        acc.residue_runs
    ));
    if !acc.by_owner.is_empty() {
        s.push_str(&format!("{pad}  by owner:\n"));
        for o in acc.by_owner.iter().take(10) {
            s.push_str(&format!(
                "{pad}    {:<10} {:>14}  {} claims\n",
                o.owner,
                commas(o.bytes),
                commas(o.claims)
            ));
        }
    }
    if !acc.by_shape.is_empty() {
        s.push_str(&format!("{pad}  residue by shape:\n"));
        for r in &acc.by_shape {
            s.push_str(&format!(
                "{pad}    {:<15} {:>14}  {} runs\n",
                r.shape,
                commas(r.bytes),
                commas(r.runs)
            ));
        }
    }
    if !acc.residue.is_empty() {
        s.push_str(&format!("{pad}  largest residue runs:\n"));
        for r in acc.residue.iter().take(12) {
            s.push_str(&format!(
                "{pad}    {:#010x} +{:#x} ({:>12})  {:<14} H={:.2}  {}\n",
                r.start,
                r.len,
                commas(r.len),
                r.shape.name(),
                r.entropy_bits,
                r.head
            ));
        }
    }
    if acc.ambiguous_dumps > 0 || acc.refuted_dumps > 0 {
        s.push_str(&format!(
            "{pad}  dumps: {} unverifiable, {} refuted by bytes\n",
            acc.ambiguous_dumps, acc.refuted_dumps
        ));
    }
    for n in &acc.notes {
        s.push_str(&format!("{pad}  note: {n}\n"));
    }
    for n in &acc.nested {
        s.push_str(&format!("{pad}  nested: {}\n", n.origin));
        s.push_str(&render_text(&n.account, indent + 4));
    }
    s
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn c(a: usize, b: usize) -> Claim {
        Claim::new(a, b, OWNER_RECORD, "t")
    }

    #[test]
    fn merge_orders_overlaps_and_clamps() {
        let claims = vec![c(10, 20), c(0, 5), c(15, 30), c(40, 44), c(90, 200)];
        assert_eq!(
            merge_ranges(&claims, 100),
            vec![(0, 5), (10, 30), (40, 44), (90, 100)]
        );
    }

    #[test]
    fn merge_drops_empty_and_out_of_range() {
        let claims = vec![c(5, 5), c(200, 300), c(7, 9)];
        assert_eq!(merge_ranges(&claims, 100), vec![(7, 9)]);
    }

    #[test]
    fn merge_joins_touching_ranges() {
        // Adjacent claims are one covered run, not two.
        assert_eq!(merge_ranges(&[c(0, 8), c(8, 16)], 16), vec![(0, 16)]);
    }

    #[test]
    fn complement_is_the_uncovered_runs() {
        let merged = vec![(0, 5), (10, 30), (90, 100)];
        assert_eq!(complement(&merged, 100), vec![(5, 10), (30, 90)]);
        assert_eq!(complement(&[], 8), vec![(0, 8)]);
        assert_eq!(complement(&[(0, 8)], 8), Vec::<(usize, usize)>::new());
    }

    #[test]
    fn complement_covers_the_tail() {
        assert_eq!(complement(&[(0, 4)], 16), vec![(4, 16)]);
    }

    #[test]
    fn residue_zero_pad() {
        assert_eq!(classify_residue(&[0u8; 4096]), ResidueShape::ZeroPad);
    }

    #[test]
    fn residue_alignment_is_short_and_nonzero() {
        assert_eq!(classify_residue(&[0, 0, 1]), ResidueShape::Alignment);
        // ...but a short all-zero run is padding, not alignment.
        assert_eq!(classify_residue(&[0, 0, 0]), ResidueShape::ZeroPad);
    }

    #[test]
    fn residue_repeated_fill_catches_multi_byte_periods() {
        let run: Vec<u8> = b"bX".iter().cycle().take(1024).copied().collect();
        assert_eq!(classify_residue(&run), ResidueShape::RepeatedFill);
        assert_eq!(classify_residue(&[0xAAu8; 512]), ResidueShape::RepeatedFill);
    }

    #[test]
    fn residue_ascii_text() {
        let run: Vec<u8> = b"Display Off\0Gradual\0Immediate\0Turns Left:\0"
            .iter()
            .cycle()
            .take(600)
            .copied()
            .collect();
        // A repeated string pool would read as RepeatedFill; break the period.
        let mut run = run;
        run.push(b'z');
        assert_eq!(classify_residue(&run), ResidueShape::AsciiText);
    }

    #[test]
    fn residue_pointer_dense_beats_plausible_mips() {
        // `0x801C0000` words have primary opcode 0x20 (a plausible `lb`), so a
        // pointer table would read as code if the order were reversed.
        let mut run = Vec::new();
        for i in 0..64u32 {
            run.extend_from_slice(&(0x801C_0000 + i * 4).to_le_bytes());
        }
        assert_eq!(classify_residue(&run), ResidueShape::PointerDense);
    }

    #[test]
    fn residue_plausible_mips() {
        // A run of real prologue / store instructions.
        let words = [
            0x27BD_FFE8u32,
            0xAFBF_0014,
            0xAFB0_0010,
            0x0C00_1234,
            0x8FBF_0014,
            0x8FB0_0010,
            0x27BD_0018,
            0x03E0_0008,
        ];
        let mut run = Vec::new();
        for _ in 0..8 {
            for w in words {
                run.extend_from_slice(&w.to_le_bytes());
            }
        }
        assert_eq!(classify_residue(&run), ResidueShape::PlausibleMips);
    }

    #[test]
    fn residue_bgr555_beats_plausible_mips() {
        // A 15-bit colour page: every halfword under 0x8000, wide spread. A
        // BGR555 pair decodes to a word in the low opcode range, so without
        // this test the run would read as un-dumped code.
        let mut run = Vec::new();
        let mut x = 12345u32;
        for _ in 0..4096 {
            x = x.wrapping_mul(1664525).wrapping_add(1013904223);
            run.extend_from_slice(&(((x >> 9) as u16) & 0x7FFF).to_le_bytes());
        }
        assert_eq!(classify_residue(&run), ResidueShape::Bgr555);

        // Real MIPS is not stolen by it: every load/store word puts a halfword
        // at or above 0x8000.
        let words = [
            0x27BD_FFE8u32,
            0xAFBF_0014,
            0xAFB0_0010,
            0x0C00_1234,
            0x8FBF_0014,
            0x8FB0_0010,
            0x27BD_0018,
            0x03E0_0008,
        ];
        let mut code = Vec::new();
        for _ in 0..8 {
            for w in words {
                code.extend_from_slice(&w.to_le_bytes());
            }
        }
        assert_eq!(classify_residue(&code), ResidueShape::PlausibleMips);
    }

    #[test]
    fn residue_bgr555_does_not_steal_a_small_value_table() {
        // Narrow spread: a sparse table, not a colour page.
        let mut low = Vec::new();
        for i in 0..2048u16 {
            low.extend_from_slice(&(i % 7).to_le_bytes());
        }
        assert_eq!(classify_residue(&low), ResidueShape::LowEntropy);
    }

    #[test]
    fn residue_low_and_high_entropy() {
        // Sparse 16-bit vectors: few distinct byte values.
        let mut low = Vec::new();
        for i in 0..512u16 {
            low.extend_from_slice(&(i % 7).to_le_bytes());
        }
        assert_eq!(classify_residue(&low), ResidueShape::LowEntropy);

        // A deterministic full-spectrum permutation stands in for compressed
        // bytes: every value appears equally often, so H = 8.0.
        let mut hi = Vec::new();
        let mut x = 1u32;
        for _ in 0..64 {
            for b in 0..=255u8 {
                x = x.wrapping_mul(1664525).wrapping_add(1013904223);
                hi.push(b ^ (x >> 24) as u8);
            }
        }
        assert_eq!(classify_residue(&hi), ResidueShape::HighEntropy);
    }

    #[test]
    fn entropy_bounds() {
        assert!(entropy_bits(&[0u8; 256]) < 0.001);
        let all: Vec<u8> = (0..=255u8).collect();
        assert!((entropy_bits(&all) - 8.0).abs() < 0.001);
    }

    #[test]
    fn dump_header_accepts_every_spelling() {
        let a = "== tmd_render 8002735c (entry=8002735c) ==\nsize=2320 bytes, 580 instructions\n\n--- DISASSEMBLY ---\n8002735c  addiu sp,sp,-0x158\n80027360  lui v0,0x8008\n";
        let d = parse_dump_header(a, "8002735c").expect("bare VA");
        assert_eq!(d.entry_va, 0x8002_735C);
        assert_eq!(d.bytes, 2320);
        assert_eq!(d.label, None);
        assert_eq!(d.head_insns[0], "addiu sp,sp,-0x158");

        let b =
            "== FUN_801cf650 0x801CF650 (entry=0x801cf650, label=equip) [menu] ==\nsize=64 bytes\n";
        let d = parse_dump_header(b, "overlay_menu_801cf650").expect("0x spelling");
        assert_eq!(d.entry_va, 0x801C_F650);
        assert_eq!(d.bytes, 64);
        assert_eq!(d.label.as_deref(), Some("menu"));

        let c = "-- FUN_80012345 (entry 80012345) --\nmin=80012345 max=80012351\n";
        let d = parse_dump_header(c, "80012345").expect("min/max spelling");
        assert_eq!(d.bytes, 0x10);
    }

    #[test]
    fn dump_header_rejects_recorded_answers() {
        let t = "== citation pointer 801cf650 -> FUN_801cf600 ==\nsize=4 bytes\n";
        assert!(parse_dump_header(t, "overlay_menu_801cf650").is_none());
    }

    #[test]
    fn insn_encoder_round_trips_the_common_first_instructions() {
        assert_eq!(encode_insn("nop"), Some(0));
        assert_eq!(encode_insn("addiu sp,sp,-0x18"), Some(0x27BD_FFE8));
        assert_eq!(encode_insn("sw ra,0x14(sp)"), Some(0xAFBF_0014));
        assert_eq!(encode_insn("lui v0,0x8008"), Some(0x3C02_8008));
        assert_eq!(encode_insn("li v0,0x1"), Some(0x2402_0001));
        assert_eq!(encode_insn("jal 0x80012340"), Some(0x0C00_48D0));
        assert_eq!(encode_insn("jr ra"), Some(0x03E0_0008));
        // Unknown mnemonics are unverifiable, never a mismatch.
        assert_eq!(encode_insn("mtc2 v0,$12"), None);
    }

    #[test]
    fn attribution_uses_the_bytes_not_the_address() {
        let dump = DumpExtent {
            entry_va: 0x801C_E818,
            bytes: 16,
            label: Some("menu".into()),
            head_insns: vec!["addiu sp,sp,-0x18".into(), "sw ra,0x14(sp)".into()],
        };
        let mut img = Vec::new();
        img.extend_from_slice(&0x27BD_FFE8u32.to_le_bytes());
        img.extend_from_slice(&0xAFBF_0014u32.to_le_bytes());
        assert_eq!(attribute(&dump, &img, 0x801C_E818), Attribution::Confirmed);

        // An aliased sibling at the same VA holds different bytes.
        let mut other = Vec::new();
        other.extend_from_slice(&0x3C02_8008u32.to_le_bytes());
        other.extend_from_slice(&0xAFBF_0014u32.to_le_bytes());
        assert_eq!(attribute(&dump, &other, 0x801C_E818), Attribution::Refuted);

        // Nothing encodable: the bytes say nothing either way.
        let quiet = DumpExtent {
            head_insns: vec!["mtc2 v0,$12".into()],
            ..dump.clone()
        };
        assert_eq!(
            attribute(&quiet, &img, 0x801C_E818),
            Attribution::Unverifiable
        );

        // A `nop`-headed dump agrees with zero fill in every image at every
        // base, so agreement carries no information. The corpus has such
        // dumps, taken over a sibling image's own zero region, and one of them
        // used to confirm a 20060-byte extent inside a 131172-byte hole.
        let nops = DumpExtent {
            head_insns: vec!["nop".into(), "nop".into(), "nop".into()],
            ..dump.clone()
        };
        assert_eq!(
            attribute(&nops, &[0u8; 0x40], 0x801C_E818),
            Attribution::Unverifiable,
            "matching only zero words is not a confirmation"
        );
        // A real instruction beside the zeros still decides it.
        let mixed = DumpExtent {
            head_insns: vec!["nop".into(), "addiu sp,sp,-0x18".into()],
            ..dump.clone()
        };
        let mut with_code = vec![0u8; 4];
        with_code.extend_from_slice(&0x27BD_FFE8u32.to_le_bytes());
        assert_eq!(
            attribute(&mixed, &with_code, 0x801C_E818),
            Attribution::Confirmed
        );
        // And a mismatch is still a refutation, zero word or not.
        assert_eq!(
            attribute(&mixed, &[0u8; 0x40], 0x801C_E818),
            Attribution::Refuted
        );
    }

    #[test]
    fn scan_claims_do_not_inflate_structural() {
        // A buffer with one TIM inside a run no structural walker claims.
        let mut buf = vec![0u8; 0x400];
        // A minimal 16bpp TIM: magic, flags=2 (no CLUT), then a 4x4 image block.
        buf[0x100..0x104].copy_from_slice(&0x0000_0010u32.to_le_bytes());
        buf[0x104..0x108].copy_from_slice(&0x0000_0002u32.to_le_bytes());
        let img_len = 12 + 4 * 4 * 2;
        buf[0x108..0x10C].copy_from_slice(&(img_len as u32).to_le_bytes());
        buf[0x10C..0x110].copy_from_slice(&0u32.to_le_bytes()); // fb x/y
        buf[0x110..0x112].copy_from_slice(&4u16.to_le_bytes()); // w halfwords
        buf[0x112..0x114].copy_from_slice(&4u16.to_le_bytes()); // h
        for (i, b) in buf.iter_mut().skip(0x114).take(32).enumerate() {
            *b = (i as u8) | 0x40;
        }
        let opts = AccountOptions {
            label: "synthetic".into(),
            rescan: true,
            ..Default::default()
        };
        let acc = account(&buf, &opts);
        assert_eq!(acc.structural, 0, "no structural walker fired");
        if acc.accounted > 0 {
            assert!(
                acc.by_owner.iter().any(|o| o.owner == OWNER_SCAN),
                "any claim here must be tagged as a scan"
            );
        }
    }

    #[test]
    fn prot_index_parses_only_a_four_digit_prefix() {
        assert_eq!(prot_index_from_name("0867_battle_data.BIN"), Some(867));
        assert_eq!(prot_index_from_name("1221_other5.BIN"), Some(1221));
        assert_eq!(prot_index_from_name("battle_data.BIN"), None);
        assert_eq!(prot_index_from_name("867_x.BIN"), None);
    }

    #[test]
    fn seq_extent_walks_to_end_of_track() {
        // Legaia meta encoding: no MIDI length byte after `FF 51` / `FF 2F`.
        let mut s = Vec::new();
        s.extend_from_slice(b"pQES");
        s.extend_from_slice(&[0, 0, 0, 1]); // version u32 BE
        s.extend_from_slice(&480u16.to_be_bytes()); // ppqn
        s.extend_from_slice(&[0x07, 0xA1, 0x20]); // tempo (3 bytes)
        s.extend_from_slice(&[4, 2]); // time signature
        assert_eq!(s.len(), 0x0F);
        s.extend_from_slice(&[0x00, 0x90, 0x3C, 0x40]); // delta, note on
        s.extend_from_slice(&[0x60, 0x3C, 0x00]); // delta, running status
        s.extend_from_slice(&[0x00, 0xFF, 0x2F]); // delta, end of track
        let n = seq_extent(&s, 0).expect("walks to end-of-track");
        assert_eq!(n, s.len());
        // A truncated stream never claims bytes it did not reach.
        assert_eq!(seq_extent(&s[..s.len() - 1], 0), None);
    }

    #[test]
    fn account_of_all_zeros_claims_nothing_and_names_the_shape() {
        let buf = vec![0u8; 0x2000];
        let acc = account(&buf, &AccountOptions::default());
        assert_eq!(acc.accounted, 0);
        assert_eq!(acc.residue_bytes, buf.len());
        assert_eq!(acc.by_shape[0].shape, "zero_pad");
    }
}
