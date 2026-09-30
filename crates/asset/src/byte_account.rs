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

mod code_scan;
mod dump_corpus;
mod pinned;
mod residue;
mod walk_battle;
mod walk_code;
mod walk_core;
mod walk_formats;

pub use code_scan::*;
pub use dump_corpus::*;
pub use pinned::*;
pub use residue::*;
use walk_battle::*;
pub use walk_code::*;
use walk_core::*;
pub use walk_formats::*;

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
/// A scalar variable in an overlay's data segment, at an address the image's
/// own code loads or stores directly, sized by that access's width.
pub const OWNER_GLOBAL: &str = "global";

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
    (
        OWNER_GLOBAL,
        "scalar variable the image's code loads / stores directly",
    ),
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

mod arrays;
pub use arrays::{BumpArray, IndexedArray, indexed_arrays, pointer_bump_arrays};

#[cfg(test)]
mod tests;
